use super::{ExtractedPage, MAX_TEXT_BYTES, NumericReading, NumericReview, error};
use crate::{
    error::{MooseError, Result},
    i18n::{gettext, gettext_f},
};
use std::{
    fs,
    io::{BufRead, BufReader, Read, Write},
    os::fd::AsRawFd,
    os::unix::fs::{DirBuilderExt, OpenOptionsExt},
    os::unix::process::CommandExt,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        Arc, OnceLock,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

mod embedded;
pub mod languages;
mod layout;
mod raster;

const MAX_PDF_PAGES: usize = 1000;
pub(crate) const MAX_RASTER_EDGE: u32 = 4608;
const PDF_DPI: u32 = 380;
const MIN_LINE_CONFIDENCE: f64 = 75.0;
const CLEAR_CONFIDENCE: f64 = 87.0;
const MAX_TOOL_FILE_BYTES: libc::rlim_t = 80 * 1024 * 1024;
const MAX_TOOL_ADDRESS_SPACE: libc::rlim_t = 2 * 1024 * 1024 * 1024;
const NATIVE_RECOVERY_BUDGET: Duration = Duration::from_secs(90);

#[derive(Clone)]
pub struct Job(Arc<State>);

struct State {
    canceled: AtomicBool,
    current: AtomicUsize,
    total: AtomicUsize,
    language: String,
    directory: PathBuf,
    ready: OnceLock<std::result::Result<bool, String>>,
    tiff_unavailable: AtomicBool,
}

#[derive(Clone, Copy)]
pub enum PdfMode {
    Automatic,
    Recognize(Option<usize>),
    RecognizeOriented(Option<usize>, PageTransform),
}

#[derive(Clone, Copy)]
pub enum PageTransform {
    Clockwise,
    UpsideDown,
    Counterclockwise,
    MirrorHorizontal,
    MirrorVertical,
}

impl PageTransform {
    fn swaps_axes(self) -> bool {
        matches!(self, Self::Clockwise | Self::Counterclockwise)
    }

    fn restore_review(self, review: &mut NumericReview) {
        let [width, height] = review.dimensions;
        if self.swaps_axes() {
            review.dimensions.swap(0, 1);
        }
        for field in &mut review.fields {
            let [x, y, w, h] = field.bounds;
            field.bounds = match self {
                Self::Clockwise => [y, width - x - w, h, w],
                Self::UpsideDown => [width - x - w, height - y - h, w, h],
                Self::Counterclockwise => [height - y - h, x, h, w],
                Self::MirrorHorizontal => [width - x - w, y, w, h],
                Self::MirrorVertical => [x, height - y - h, w, h],
            };
        }
    }
}

#[derive(Clone, Copy)]
enum RecognitionMode {
    Standard,
    Shaded,
    Crop,
    Upright(bool),
}

impl Job {
    pub fn new(language: String, directory: PathBuf) -> Self {
        Self(Arc::new(State {
            canceled: AtomicBool::new(false),
            current: AtomicUsize::new(0),
            total: AtomicUsize::new(0),
            language,
            directory,
            ready: OnceLock::new(),
            tiff_unavailable: AtomicBool::new(false),
        }))
    }

    pub fn cancel(&self) {
        self.0.canceled.store(true, Ordering::Relaxed);
    }

    pub fn check(&self) -> Result<()> {
        if self.0.canceled.load(Ordering::Relaxed) {
            return Err(error(gettext("Import canceled")));
        }
        Ok(())
    }

    pub fn progress(&self) -> (usize, usize) {
        (
            self.0.current.load(Ordering::Relaxed),
            self.0.total.load(Ordering::Relaxed),
        )
    }

    fn page(&self, current: usize, total: usize) -> Result<()> {
        self.check()?;
        self.0.total.store(total, Ordering::Relaxed);
        self.0.current.store(current, Ordering::Relaxed);
        Ok(())
    }

    fn run(&self, command: &mut Command, limit: usize, seconds: u64) -> Result<Vec<u8>> {
        self.check()?;
        let tool = command.get_program().to_string_lossy().into_owned();
        unsafe {
            command.pre_exec(|| {
                for (resource, maximum) in [
                    (libc::RLIMIT_FSIZE, MAX_TOOL_FILE_BYTES),
                    (libc::RLIMIT_AS, MAX_TOOL_ADDRESS_SPACE),
                ] {
                    let mut limit = libc::rlimit {
                        rlim_cur: 0,
                        rlim_max: 0,
                    };
                    if libc::getrlimit(resource, &mut limit) != 0 {
                        return Err(std::io::Error::last_os_error());
                    }
                    if limit.rlim_cur > maximum {
                        limit.rlim_cur = maximum;
                        if libc::setrlimit(resource, &limit) != 0 {
                            return Err(std::io::Error::last_os_error());
                        }
                    }
                }
                Ok(())
            });
        }
        let child = command
            .env("LC_ALL", "C")
            .env("OMP_THREAD_LIMIT", "1")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .process_group(0)
            .spawn()
            .map_err(|_| error(gettext_f(
                "Text extraction requires {tool}. Reinstall the Flatpak or install this tool for a native build.",
                &[("tool", &tool)],
            )))?;
        let mut process = Process {
            child,
            finished: false,
        };
        let mut stdout = process.child.stdout.take();
        let mut stderr = process.child.stderr.take();
        let mut output = Vec::new();
        let mut details = Vec::new();
        let deadline = Instant::now() + Duration::from_secs(seconds);
        loop {
            self.check()?;
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(error(gettext(
                    "Text extraction timed out. Try fewer pages or a smaller document.",
                )));
            }
            let drained = stdout.is_none() && stderr.is_none();
            if drained && let Some(status) = process.child.try_wait()? {
                process.finished = true;
                if status.success() {
                    return Ok(output);
                }
                return Err(error(gettext_f(
                    "{tool} could not process this file: {detail}",
                    &[
                        ("tool", &tool),
                        ("detail", String::from_utf8_lossy(&details).trim()),
                    ],
                )));
            }
            let mut descriptors = [pipe_events(stdout.as_ref()), pipe_events(stderr.as_ref())];
            let timeout = if drained { 2 } else { 40 };
            let timeout = remaining.as_millis().clamp(1, timeout) as i32;
            let ready =
                unsafe { libc::poll(descriptors.as_mut_ptr(), descriptors.len() as _, timeout) };
            if ready < 0 {
                let failure = std::io::Error::last_os_error();
                if failure.kind() == std::io::ErrorKind::Interrupted {
                    continue;
                }
                return Err(failure.into());
            }
            if descriptors[0].revents != 0 && capture(&mut stdout, &mut output, limit)? {
                return Err(error(gettext("The extracted text exceeds the size limit")));
            }
            if descriptors[1].revents != 0 {
                capture(&mut stderr, &mut details, 4096)?;
            }
        }
    }

    fn tesseract(&self) -> Command {
        let mut command = Command::new("tesseract");
        command.arg("--tessdata-dir").arg(&self.0.directory);
        command
    }

    fn ready(&self) -> Result<bool> {
        self.check()?;
        self.0.ready.get_or_init(|| {
            if !valid_languages(&self.0.language) {
                return Err(gettext("Install and select an OCR language in Preferences → Text Recognition before recognizing text."));
            }
            for code in self.0.language.split('+') {
                let language = languages::find(code).filter(|language| language.selectable());
                let Some(language) = language else {
                    return Err(gettext("Some selected OCR languages are unavailable"));
                };
                if !language.verify(&self.0.directory).map_err(|failure| failure.to_string())? {
                    return Err(gettext_f(
                        "The OCR language {language} is missing or damaged. Reinstall it in Preferences → Text Recognition.",
                        &[("language", code)],
                    ));
                }
            }
            Ok(languages::find("osd").is_some_and(|language| language.verify(&self.0.directory).unwrap_or(false)))
        }).as_ref().copied().map_err(|message| error(message.clone()))
    }

    fn recognize(&self, image: &Path, dpi: u32, mode: RecognitionMode) -> Result<Recognition> {
        let osd = self.ready()? && !matches!(mode, RecognitionMode::Upright(_));
        let automatic = if osd { "1" } else { "3" };
        let sparse = if osd { "12" } else { "11" };
        let global_threshold = "thresholding_method=0";
        let local_threshold = "thresholding_method=2";
        let (primary, alternate) = if matches!(
            mode,
            RecognitionMode::Shaded | RecognitionMode::Upright(true)
        ) {
            (local_threshold, global_threshold)
        } else {
            (global_threshold, local_threshold)
        };
        let mut best: Option<Recognition> = None;
        let mut alternatives = Vec::new();
        let mut failure = None;
        let cropped = matches!(mode, RecognitionMode::Crop);
        for (attempt, (layout, threshold, seconds)) in [
            (automatic, primary, if cropped { 20 } else { 75 }),
            (automatic, alternate, 35),
            (sparse, global_threshold, 35),
            ("6", local_threshold, 25),
        ]
        .into_iter()
        .take(if cropped { 1 } else { 4 })
        .enumerate()
        {
            let reconsider_orientation = osd
                && attempt == 1
                && best.as_ref().is_none_or(|best| {
                    best.page
                        .confidence
                        .is_none_or(|score| score < MIN_LINE_CONFIDENCE)
                });
            let result = self.recognize_pass(
                image,
                dpi,
                layout,
                threshold,
                seconds,
                reconsider_orientation,
            );
            self.check()?;
            match result {
                Ok(mut candidate) => {
                    if layout == sparse {
                        candidate.score *= 0.9;
                        if best.as_ref().is_some_and(Recognition::has_operator_lines)
                            && candidate.layout.has_split_rows()
                        {
                            continue;
                        }
                    }
                    if best.as_ref().is_none_or(|best| candidate.better_than(best)) {
                        if let Some(previous) = best.replace(candidate) {
                            alternatives.push(previous.numbers);
                        }
                    } else {
                        alternatives.push(candidate.numbers);
                    }
                }
                Err(error) => {
                    failure.get_or_insert(error);
                }
            }
            if best
                .as_ref()
                .is_some_and(|best| best.clear() && (attempt != 0 || !best.has_unread_regions))
            {
                break;
            }
        }
        match best {
            Some(mut best) if failure.is_none() || !best.page.text.is_empty() => {
                best.mark_disagreements(alternatives);
                if !cropped && let Some(text) = self.separate_symbols(image, dpi, &best.page.text) {
                    best.page.text = text;
                }
                self.check()?;
                Ok(best)
            }
            _ => {
                Err(failure
                    .unwrap_or_else(|| error(gettext("Text extraction stopped unexpectedly"))))
            }
        }
    }

    fn separate_symbols(&self, image: &Path, dpi: u32, text: &str) -> Option<String> {
        if text.len() > 96
            || alphanumeric_count(text) != 0
            || text.trim().contains('\n')
            || !text
                .split_whitespace()
                .any(|word| word.chars().nth(1).is_some())
        {
            return None;
        }
        let base = image.with_extension("");
        let output = base.with_added_extension("box");
        let result = self
            .run(
                self.tesseract().arg(image).arg(base).args([
                    "-l",
                    &self.0.language,
                    "--oem",
                    "1",
                    "--psm",
                    "7",
                    "--dpi",
                    &dpi.to_string(),
                    "-c",
                    "tessedit_create_boxfile=1",
                    "-c",
                    "tessedit_create_txt=0",
                ]),
                1024,
                6,
            )
            .ok()
            .and_then(|_| {
                let mut file = fs::File::open(&output).ok()?.take(16 * 1024);
                let mut boxes = String::new();
                file.read_to_string(&mut boxes).ok()?;
                layout::space_symbols(text, &boxes)
            });
        let _ = fs::remove_file(output);
        result
    }

    fn orient(
        &self,
        original: Recognition,
        path: &Path,
        dpi: u32,
        mut prepare: impl FnMut(PageTransform, &Path) -> Result<bool>,
    ) -> Result<Recognition> {
        let threshold = if original.score < 20.0 {
            CLEAR_CONFIDENCE
        } else {
            MIN_LINE_CONFIDENCE
        };
        if original
            .page
            .confidence
            .is_some_and(|score| score >= threshold)
            || original.has_operator_lines()
            || original.page.text.len() > 240
            || symbol_count(&original.page.text) > alphanumeric_count(&original.page.text)
            || self
                .0
                .language
                .split('+')
                .any(|code| code.ends_with("_vert"))
        {
            return Ok(original);
        }
        let variant = |name| {
            path.with_file_name(format!(
                "{}-{name}.{}",
                path.file_stem().unwrap_or_default().to_string_lossy(),
                path.extension().unwrap_or_default().to_string_lossy()
            ))
        };
        let transformed = variant("orientation");
        let confirmation = variant("confirmed");
        let trial: Result<Option<Recognition>> = (|| {
            let mut readings = Vec::new();
            let mut highest = 0.0;
            for transform in [
                PageTransform::UpsideDown,
                PageTransform::Clockwise,
                PageTransform::Counterclockwise,
                PageTransform::MirrorHorizontal,
                PageTransform::MirrorVertical,
            ] {
                self.check()?;
                if !prepare(transform, &transformed)? {
                    return Ok(None);
                }
                let result =
                    self.recognize_pass(&transformed, dpi, "3", "thresholding_method=0", 6, false);
                self.check()?;
                let Ok(reading) = result else { continue };
                if reading
                    .page
                    .text
                    .split_whitespace()
                    .filter(|word| word.chars().any(char::is_alphabetic))
                    .count()
                    < 2
                    || reading
                        .page
                        .confidence
                        .is_none_or(|score| score < CLEAR_CONFIDENCE)
                {
                    continue;
                }
                if reading.score >= highest {
                    fs::rename(&transformed, &confirmation)?;
                    highest = reading.score;
                }
                readings.push((transform, reading));
            }
            let Some(index) = readings
                .iter()
                .enumerate()
                .max_by(|(_, (_, a)), (_, (_, b))| a.score.total_cmp(&b.score))
                .map(|(index, _)| index)
            else {
                return Ok(None);
            };
            let (transform, mut reading) = readings.swap_remove(index);
            let next_score = readings
                .iter()
                .filter(|(_, other)| !same_words(&reading.page.text, &other.page.text))
                .map(|(_, other)| other.score)
                .fold(0.0, f64::max);
            if reading.score < 7.0 || reading.score <= (original.score.max(next_score)) * 1.25 {
                return Ok(None);
            }
            let confirmed =
                self.recognize_pass(&confirmation, dpi, "3", "thresholding_method=2", 6, false);
            self.check()?;
            let Ok(confirmed) = confirmed else {
                return Ok(None);
            };
            if confirmed
                .page
                .confidence
                .is_none_or(|score| score < CLEAR_CONFIDENCE)
                || !same_words(&confirmed.page.text, &reading.page.text)
            {
                return Ok(None);
            }
            transform.restore_review(&mut reading.page.review);
            Ok(Some(reading))
        })();
        let _ = fs::remove_file(transformed);
        let _ = fs::remove_file(confirmation);
        self.check()?;
        Ok(trial.ok().flatten().unwrap_or(original))
    }

    fn recognize_pass(
        &self,
        image: &Path,
        dpi: u32,
        layout: &str,
        threshold: &str,
        seconds: u64,
        reconsider_orientation: bool,
    ) -> Result<Recognition> {
        let output = image.with_extension("");
        let text = output.with_added_extension("txt");
        let tsv = output.with_added_extension("tsv");
        let mut command = self.tesseract();
        command.arg(image).arg(&output).args([
            "-l",
            &self.0.language,
            "--oem",
            "1",
            "--psm",
            layout,
            "--dpi",
            &dpi.to_string(),
            "-c",
            threshold,
            "-c",
            "tessedit_create_txt=1",
            "-c",
            "tessedit_create_tsv=1",
        ]);
        if reconsider_orientation {
            command.args([
                "-c",
                "min_characters_to_try=12",
                "-c",
                "min_orientation_margin=2",
            ]);
        }
        let result = self
            .run(&mut command, 1024, seconds)
            .and_then(|_| Recognition::read(&text, &tsv, &self.0.language));
        let _ = fs::remove_file(text);
        let _ = fs::remove_file(tsv);
        result
    }

    fn read_agreed_line(
        &self,
        raster: &Path,
        dpi: u32,
        accept: impl Fn(&str) -> bool,
    ) -> Option<(String, f64)> {
        let read = |threshold| {
            let page = self
                .recognize_pass(raster, dpi, "7", threshold, 3, false)
                .ok()?
                .page;
            let confidence = page.confidence?;
            (confidence >= MIN_LINE_CONFIDENCE && accept(&page.text))
                .then_some((page.text, confidence))
        };
        let (first, first_confidence) = read("thresholding_method=0")?;
        let (second, second_confidence) = read("thresholding_method=2")?;
        (first == second && first_confidence.max(second_confidence) >= CLEAR_CONFIDENCE)
            .then_some((first, (first_confidence + second_confidence) / 2.0))
    }
    fn read_decimal(&self, path: &Path, dpi: u32, current: &str) -> Option<(String, f64)> {
        let first = self
            .recognize_pass(path, dpi, "7", "thresholding_method=0", 3, false)
            .ok()?
            .page;
        let score = first.confidence?;
        if first.text == current
            || first.text.len() != current.len()
            || !first.text.bytes().all(|digit| digit.is_ascii_digit())
            || score < MIN_LINE_CONFIDENCE
        {
            return None;
        }
        let mut raster = raster::Raster::open(path).ok()??;
        let [width, height] = raster.dimensions();
        if width > 2048 || height > 512 {
            return None;
        }
        let shorter = path.with_extension("compact.ppm");
        let second = raster
            .write_region(&shorter, [0, 0, width, height], true)
            .ok()
            .and_then(|_| {
                self.recognize_pass(&shorter, dpi, "8", "thresholding_method=2", 3, false)
                    .ok()
            });
        let _ = fs::remove_file(shorter);
        let second = second?.page;
        let other = second.confidence?;
        (second.text == first.text && other >= CLEAR_CONFIDENCE)
            .then_some((first.text, score.max(other)))
    }

    fn read_region(
        &self,
        path: &Path,
        dpi: u32,
        kind: layout::RegionKind<'_>,
    ) -> Option<(String, f64)> {
        match kind {
            layout::RegionKind::Field => self.read_agreed_line(path, dpi, |text| {
                text.split_whitespace().count() == 1 && text.chars().any(char::is_numeric)
            }),
            layout::RegionKind::Line(current) => self.read_agreed_line(path, dpi, |text| {
                text.lines().count() == 1 && text.split_whitespace().eq(current.split_whitespace())
            }),
            layout::RegionKind::Number(text) => self.read_decimal(path, dpi, text),
        }
    }
}

struct Recognition {
    page: ExtractedPage,
    score: f64,
    layout: layout::Layout,
    has_unread_regions: bool,
    numbers: Vec<NumericReading>,
}

impl Recognition {
    fn has_operator_lines(&self) -> bool {
        if self.page.text.len() > 240 {
            return false;
        }
        let lines = self
            .page
            .text
            .lines()
            .filter(|line| !line.trim().is_empty());
        lines.clone().count() <= 6
            && lines
                .filter(|line| line.contains(['=', '×', '÷', '±', '≤', '≥', '√']))
                .nth(1)
                .is_some()
    }

    fn read(text: &Path, tsv: &Path, language: &str) -> Result<Self> {
        let mut text = fs::File::open(text)?;
        if text.metadata()?.len() > (MAX_TEXT_BYTES + 2) as u64 {
            return Err(text_limit());
        }
        let mut content = String::new();
        text.read_to_string(&mut content)?;
        let content = content.trim_end_matches('\u{c}').trim_end();
        if content.len() > MAX_TEXT_BYTES {
            return Err(text_limit());
        }
        let tsv = fs::File::open(tsv)?;
        if tsv.metadata()?.len() > (MAX_TEXT_BYTES * 12) as u64 {
            return Err(text_limit());
        }
        let mut characters = 0.0;
        let mut confidence = 0.0;
        let mut score = 0.0;
        let mut page_area = 0.0;
        let mut unread_area = 0.0;
        let mut review = NumericReview::default();
        let mut numbers = Vec::new();
        let mut layout = layout::Layout::default();
        let mut row = String::new();
        let mut tsv = BufReader::new(tsv);
        while tsv.read_line(&mut row)? != 0 {
            if let Some(fields) = layout::columns(row.trim_end_matches(['\r', '\n'])) {
                if fields[0] == "4" {
                    layout.line(fields[7], fields[9]);
                }
                if fields[0] == "1" || (fields[0] == "5" && fields[11].trim().is_empty()) {
                    let area = match (fields[8].parse::<u32>(), fields[9].parse::<u32>()) {
                        (Ok(width), Ok(height)) => f64::from(width) * f64::from(height),
                        _ => 0.0,
                    };
                    if fields[0] == "1" {
                        page_area = area;
                        if let (Ok(w), Ok(h)) = (fields[8].parse(), fields[9].parse()) {
                            review.dimensions = [w, h];
                        }
                    } else {
                        unread_area += area;
                    }
                }
                if fields[0] == "5"
                    && !fields[11].trim().is_empty()
                    && let Ok(value) = fields[10].parse::<f64>()
                    && (0.0..=100.0).contains(&value)
                {
                    let weight = alphanumeric_count(fields[11]) as f64;
                    characters += weight;
                    confidence += weight * value;
                    score += weight * ((value - 35.0).max(0.0) / 65.0).powi(2);
                    layout.word(
                        fields[11],
                        value,
                        [fields[6], fields[7], fields[8], fields[9]],
                    );
                    if (value < MIN_LINE_CONFIDENCE || numbers.len() < 1024)
                        && fields[11].len() <= 128
                        && fields[11].chars().any(char::is_numeric)
                        && let [Ok(x), Ok(y), Ok(w), Ok(h)] =
                            [fields[6], fields[7], fields[8], fields[9]].map(str::parse::<u32>)
                        && w > 0
                        && h > 0
                        && x.saturating_add(w) <= review.dimensions[0]
                        && y.saturating_add(h) <= review.dimensions[1]
                    {
                        let reading = NumericReading {
                            text: fields[11].to_owned(),
                            score: value.round() as u8,
                            bounds: [x, y, w, h],
                            alternative: None,
                        };
                        if numbers.len() < 1024 && numeric_field(&reading.text) {
                            numbers.push(reading.clone());
                        }
                        if value < MIN_LINE_CONFIDENCE {
                            let position = review
                                .fields
                                .partition_point(|field| field.score <= reading.score);
                            if position < 8 {
                                review.fields.insert(position, reading);
                                review.fields.truncate(8);
                            }
                        }
                    }
                }
            }
            row.clear();
        }
        Ok(Self {
            page: ExtractedPage {
                text: if content.trim_start().is_empty() {
                    String::new()
                } else {
                    content.to_owned()
                },
                method: "tesseract-lstm-adaptive-v3".into(),
                language: language.into(),
                review,
                confidence: (characters > 0.0).then(|| confidence / characters),
                ..ExtractedPage::default()
            },
            score,
            layout,
            numbers,
            has_unread_regions: page_area > 0.0 && unread_area / page_area >= 0.12,
        })
    }

    fn mark_disagreements(&mut self, mut alternatives: Vec<Vec<NumericReading>>) {
        if alternatives.is_empty() || self.numbers.is_empty() {
            return;
        }
        let mut conflicts = Vec::new();
        for readings in &mut alternatives {
            readings.sort_unstable_by_key(|reading| reading.bounds[1]);
        }
        for reading in &self.numbers {
            let [_, top, _, height] = reading.bounds;
            let limit = height / 2;
            let best = alternatives
                .iter()
                .filter_map(|pass| {
                    let start = pass.partition_point(|candidate| {
                        candidate.bounds[1] < top.saturating_sub(limit)
                    });
                    let mut matches = pass[start..]
                        .iter()
                        .take_while(|candidate| candidate.bounds[1] <= top + limit)
                        .filter(|candidate| overlapping_words(reading.bounds, candidate.bounds));
                    let candidate = matches.next()?;
                    (matches.next().is_none()
                        && candidate.text != reading.text
                        && f64::from(candidate.score) >= MIN_LINE_CONFIDENCE)
                        .then_some(candidate)
                })
                .max_by_key(|candidate| candidate.score);
            if let Some(other) = best {
                self.page
                    .review
                    .fields
                    .retain(|field| field.bounds != reading.bounds);
                let mut conflict = reading.clone();
                conflict.alternative = Some(other.text.clone());
                conflicts.push(conflict);
                if conflicts.len() == 8 {
                    break;
                }
            }
        }
        if !conflicts.is_empty() {
            conflicts.append(&mut self.page.review.fields);
            conflicts.truncate(8);
            self.page.review.fields = conflicts;
        }
    }

    fn clear(&self) -> bool {
        self.score >= 30.0
            && self
                .page
                .confidence
                .is_some_and(|value| value >= CLEAR_CONFIDENCE)
    }

    fn better_than(&self, best: &Self) -> bool {
        !self.page.text.is_empty() && (best.page.text.is_empty() || self.score > best.score * 1.05)
    }

    fn into_page(mut self) -> ExtractedPage {
        if let Some(text) = self.layout.column_order(&self.page.text) {
            self.page.text = text;
        }
        self.page
    }

    fn merge_native(
        mut self,
        native: &layout::Layout,
        text: &str,
        options: layout::MergeOptions,
        recover: impl FnMut([u32; 4], layout::RegionKind<'_>) -> Option<(String, f64)>,
        discover: impl FnOnce(&[(usize, [usize; 4])]) -> Vec<(usize, String, bool, f64)>,
    ) -> Option<ExtractedPage> {
        let offset = options.offset;
        let merged = self
            .layout
            .merge(&self.page.text, native, text, options, recover, discover);
        let incomplete = self.layout.has_unmerged_text(
            &self.page.text,
            native,
            merged.as_ref().map_or(text, |page| page.text.as_str()),
            offset,
            merged
                .as_ref()
                .map_or(&[], |page| page.repaired_lines.as_slice()),
        );
        let notice = || gettext("Some text detected in embedded images could not be added safely");
        let Some(layout::Merged {
            text: merged_text,
            confidence,
            ..
        }) = merged
        else {
            return (options.final_pass && incomplete).then(|| ExtractedPage {
                error: notice(),
                ..ExtractedPage::from(text.to_owned())
            });
        };
        if incomplete {
            if !options.final_pass {
                return None;
            }
            self.page.error = notice();
        }
        for field in &mut self.page.review.fields {
            field.bounds[0] += offset.0 as u32;
            field.bounds[1] += offset.1 as u32;
        }
        self.page.text = merged_text;
        self.page.confidence = Some(confidence);
        self.page.method = "tesseract-lstm-mixed-v3".into();
        Some(self.page)
    }
}

fn numeric_field(text: &str) -> bool {
    text.chars().filter(|ch| ch.is_numeric()).count() >= 2
        && text.chars().all(|ch| {
            ch.is_numeric() || matches!(ch, '.' | ',' | '-' | '+' | '/' | '%' | '٫' | '٬')
        })
}

fn overlapping_words([x, y, width, height]: [u32; 4], [left, top, w, h]: [u32; 4]) -> bool {
    let intersection = u64::from(
        x.saturating_add(width)
            .min(left.saturating_add(w))
            .saturating_sub(x.max(left)),
    ) * u64::from(
        y.saturating_add(height)
            .min(top.saturating_add(h))
            .saturating_sub(y.max(top)),
    );
    let area = u64::from(width) * u64::from(height) + u64::from(w) * u64::from(h);
    intersection * 5 >= area * 2
}

pub fn valid_languages(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 4096
        && value.split('+').all(|language| {
            let language = language.strip_prefix("script/").unwrap_or(language);
            !language.is_empty()
                && language
                    .bytes()
                    .all(|ch| ch.is_ascii_alphabetic() || ch == b'_')
        })
}

fn pipe_events(pipe: Option<&impl AsRawFd>) -> libc::pollfd {
    libc::pollfd {
        fd: pipe.map_or(-1, AsRawFd::as_raw_fd),
        events: libc::POLLIN,
        revents: 0,
    }
}

fn capture(
    pipe: &mut Option<impl Read>,
    output: &mut Vec<u8>,
    limit: usize,
) -> std::io::Result<bool> {
    let Some(reader) = pipe.as_mut() else {
        return Ok(false);
    };
    let mut buffer = [0; 16 * 1024];
    let length = match reader.read(&mut buffer) {
        Ok(length) => length,
        Err(failure) if failure.kind() == std::io::ErrorKind::Interrupted => return Ok(false),
        Err(failure) => return Err(failure),
    };
    if length == 0 {
        *pipe = None;
    }
    let retained = length.min(limit.saturating_sub(output.len()));
    output.extend_from_slice(&buffer[..retained]);
    Ok(retained != length)
}

struct Process {
    child: Child,
    finished: bool,
}

impl Drop for Process {
    fn drop(&mut self) {
        if !self.finished {
            unsafe { libc::kill(-(self.child.id() as i32), libc::SIGKILL) };
            let _ = self.child.wait();
        }
    }
}

struct Workspace(PathBuf);

impl Workspace {
    fn new(cache: &Path) -> Result<Self> {
        let root = cache.join("document-imports");
        fs::create_dir_all(&root)?;
        let directory = root.join(crate::core::new_id());
        fs::DirBuilder::new().mode(0o700).create(&directory)?;
        Ok(Self(directory))
    }

    fn write(&self, name: &str, bytes: &[u8]) -> Result<PathBuf> {
        let path = self.0.join(name);
        fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)?
            .write_all(bytes)?;
        Ok(path)
    }
}

impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn text_limit() -> MooseError {
    error(gettext(
        "Documents can contain up to 4 MB of extracted text",
    ))
}

fn same_words(a: &str, b: &str) -> bool {
    a.split_whitespace().eq(b.split_whitespace())
}

fn symbol_count(text: &str) -> usize {
    text.chars()
        .filter(|ch| !ch.is_alphanumeric() && !ch.is_whitespace() && !ch.is_control())
        .count()
}

fn alphanumeric_count(text: &str) -> usize {
    text.chars().filter(|ch| ch.is_alphanumeric()).count()
}

pub fn image(bytes: &[u8], cache: &Path, job: &Job) -> Result<ExtractedPage> {
    image_with_orientation(bytes, cache, job, false, None, |_, _| Ok(false))
}

pub(crate) fn image_with_orientation(
    bytes: &[u8],
    cache: &Path,
    job: &Job,
    shaded: bool,
    transform: Option<PageTransform>,
    mut prepare: impl FnMut(PageTransform, &Path) -> Result<bool>,
) -> Result<ExtractedPage> {
    job.page(1, 1)?;
    let workspace = Workspace::new(cache)?;
    let path = workspace.0.join("image.png");
    if let Some(transform) = transform {
        prepare(transform, &path)?;
        let mut page = job
            .recognize(&path, 300, RecognitionMode::Upright(shaded))?
            .into_page();
        transform.restore_review(&mut page.review);
        return Ok(page);
    }
    let mode = if shaded {
        RecognitionMode::Shaded
    } else {
        RecognitionMode::Standard
    };
    workspace.write("image.png", bytes)?;
    let recognized = job.recognize(&path, 300, mode)?;
    Ok(job.orient(recognized, &path, 300, prepare)?.into_page())
}

pub fn pdf(bytes: &[u8], cache: &Path, job: &Job, mode: PdfMode) -> Result<Vec<ExtractedPage>> {
    job.check()?;
    let workspace = Workspace::new(cache)?;
    let path = workspace.write("input.pdf", bytes)?;
    read_pdf(&path, job, mode)
}

pub fn tiff(bytes: &[u8], cache: &Path, job: &Job, mode: PdfMode) -> Result<Vec<ExtractedPage>> {
    job.check()?;
    let workspace = Workspace::new(cache)?;
    let path = tiff_to_pdf(bytes, &workspace, job)?;
    read_pdf(&path, job, mode)
}

fn tiff_to_pdf(bytes: &[u8], workspace: &Workspace, job: &Job) -> Result<PathBuf> {
    let input = workspace.write("input.tif", bytes)?;
    let path = workspace.0.join("scan.pdf");
    job.run(
        Command::new("tiff2pdf")
            .args(["-z", "-o"])
            .arg(&path)
            .arg(input),
        1024,
        30,
    )?;
    Ok(path)
}

#[cfg(feature = "gui")]
pub(crate) fn page_region(
    bytes: &[u8],
    tiff: bool,
    number: i64,
    edge: u32,
    [x, y, w, h]: [u32; 4],
    cache: &Path,
    job: &Job,
) -> Result<Vec<u8>> {
    if !(1..=MAX_PDF_PAGES as i64).contains(&number)
        || !(1..=MAX_RASTER_EDGE + 1).contains(&edge)
        || w == 0
        || h == 0
        || x.saturating_add(w) > edge
        || y.saturating_add(h) > edge
    {
        return Err(error(gettext("This region is unavailable")));
    }
    let workspace = Workspace::new(cache)?;
    let path = if tiff {
        tiff_to_pdf(bytes, &workspace, job)?
    } else {
        workspace.write("input.pdf", bytes)?
    };
    let ratio = (1024.0 / f64::from(w)).min(320.0 / f64::from(h)).min(1.0);
    let scale = |n| (f64::from(n) * ratio).round() as u32;
    let number = number.to_string();
    job.run(
        Command::new("pdftoppm")
            .args([
                "-f",
                &number,
                "-l",
                &number,
                "-cropbox",
                "-singlefile",
                "-png",
                "-scale-to",
                &scale(edge).max(1).to_string(),
                "-x",
                &scale(x).to_string(),
                "-y",
                &scale(y).to_string(),
                "-W",
                &scale(w).max(1).to_string(),
                "-H",
                &scale(h).max(1).to_string(),
            ])
            .arg(path),
        4 * 1024 * 1024,
        15,
    )
}

fn read_pdf(path: &Path, job: &Job, mode: PdfMode) -> Result<Vec<ExtractedPage>> {
    let info = job.run(
        Command::new("pdfinfo")
            .args(["-f", "1", "-l", &MAX_PDF_PAGES.to_string(), "-box"])
            .arg(path),
        512 * 1024,
        30,
    )?;
    let info = String::from_utf8_lossy(&info);
    let count = info
        .lines()
        .find_map(|line| line.strip_prefix("Pages:")?.trim().parse::<usize>().ok())
        .filter(|count| (1..=MAX_PDF_PAGES).contains(count))
        .ok_or_else(|| error(gettext("Documents must contain between 1 and 1,000 pages")))?;
    let automatic = matches!(mode, PdfMode::Automatic);
    let transform = match mode {
        PdfMode::RecognizeOriented(_, transform) => Some(transform),
        _ => None,
    };
    let selected = match mode {
        PdfMode::Recognize(Some(page)) | PdfMode::RecognizeOriented(Some(page), _)
            if !(1..=count).contains(&page) =>
        {
            return Err(error(gettext("This page is not available in the document")));
        }
        PdfMode::Recognize(page) | PdfMode::RecognizeOriented(page, _) => page,
        PdfMode::Automatic => None,
    };
    let bounds = crop_boxes(&info, count);
    let mut pages = Vec::new();
    let native_failure = if automatic {
        match native_text(job, path, None, 30) {
            Ok(text) => {
                pages.extend(text.split('\u{c}').take(count).map(native_page));
                None
            }
            Err(failure) => Some(failure.to_string()),
        }
    } else {
        None
    };
    let mut coverage = vec![ImageCoverage::default(); count];
    if automatic
        && (native_failure.is_some() || pages.iter().any(|page| !page.text.trim().is_empty()))
    {
        match job.run(
            Command::new("pdfimages").arg("-list").arg(path),
            1024 * 1024,
            20,
        ) {
            Ok(images) => {
                estimate_image_coverage(&String::from_utf8_lossy(&images), &bounds, &mut coverage);
            }
            Err(_) => {
                job.check()?;
                for page in pages.iter_mut().filter(|page| !page.text.trim().is_empty()) {
                    page.error = gettext("Embedded images could not be inspected for text");
                }
            }
        }
    }
    pages.resize_with(count, ExtractedPage::default);
    let mut total_bytes = pages.iter().map(|page| page.text.len()).sum::<usize>();
    if total_bytes > MAX_TEXT_BYTES {
        return Err(text_limit());
    }
    let range = selected.map_or(0..count, |page| page - 1..page);
    let total = range.len();
    if let Some(failure) = native_failure {
        let mut recovery_budget = NATIVE_RECOVERY_BUDGET;
        for (step, index) in range.enumerate() {
            job.page(step + 1, total)?;
            let page = &mut pages[index];
            page.error.clone_from(&failure);
            let available = recovery_budget.as_secs();
            if available > 0 {
                let start = Instant::now();
                match native_text(job, path, Some(index + 1), available.min(15)) {
                    Ok(text) => {
                        *page = native_page(text.trim_end_matches('\u{c}'));
                        total_bytes += page.text.len();
                        if total_bytes > MAX_TEXT_BYTES {
                            return Err(text_limit());
                        }
                    }
                    Err(failure) => page.error = failure.to_string(),
                }
                recovery_budget = recovery_budget.saturating_sub(start.elapsed());
            }
            job.check()?;
            if let Some(plan) = PagePlan::new(job, automatic, page, coverage[index], transform) {
                plan.recognize(job, path, index + 1, bounds[index], automatic, page)?
                    .apply(page, &mut total_bytes)?;
            }
        }
    } else {
        let planned: Vec<_> = range
            .clone()
            .filter_map(|index| {
                PagePlan::new(job, automatic, &pages[index], coverage[index], transform)
                    .map(|plan| (index, plan))
            })
            .collect();
        for batch in planned.chunks(pdf_workers()) {
            job.page(batch[0].0 - range.start + 1, total)?;
            let recognize = |&(index, plan): &(usize, PagePlan)| {
                plan.recognize(
                    job,
                    path,
                    index + 1,
                    bounds[index],
                    automatic,
                    &pages[index],
                )
            };
            let updates = if batch.len() == 1 {
                vec![recognize(&batch[0])?]
            } else {
                std::thread::scope(|scope| {
                    let recognize = &recognize;
                    let mut updates = Vec::with_capacity(batch.len());
                    let handles: Vec<_> = batch[1..]
                        .iter()
                        .map(|task| {
                            std::thread::Builder::new().spawn_scoped(scope, move || recognize(task))
                        })
                        .collect();
                    updates.push(recognize(&batch[0])?);
                    for (task, handle) in batch[1..].iter().zip(handles) {
                        let update = match handle {
                            Ok(worker) => worker.join().map_err(|_| {
                                error(gettext("Text extraction stopped unexpectedly"))
                            })?,
                            Err(_) => recognize(task),
                        };
                        updates.push(update?);
                    }
                    Ok::<_, MooseError>(updates)
                })?
            };
            job.check()?;
            for (&(index, plan), update) in batch.iter().zip(updates) {
                let update = if batch.len() > 1 && matches!(&update, PageUpdate::Failed(_)) {
                    plan.recognize(
                        job,
                        path,
                        index + 1,
                        bounds[index],
                        automatic,
                        &pages[index],
                    )?
                } else {
                    update
                };
                update.apply(&mut pages[index], &mut total_bytes)?;
            }
        }
    }
    job.check()?;
    Ok(pages)
}

fn pdf_workers() -> usize {
    let memory = fs::read_to_string("/proc/meminfo").ok();
    let available = memory
        .as_deref()
        .and_then(|info| {
            info.lines().find_map(|line| {
                line.strip_prefix("MemAvailable:")?
                    .split_whitespace()
                    .next()?
                    .parse::<u64>()
                    .ok()
            })
        })
        .unwrap_or_default();
    let memory_slots = available.saturating_sub(512 * 1024) / (MAX_TOOL_ADDRESS_SPACE / 1024);
    std::thread::available_parallelism()
        .map_or(1, std::num::NonZeroUsize::get)
        .min(memory_slots.clamp(1, 4) as usize)
}

#[derive(Clone, Copy)]
struct PagePlan {
    native_chars: usize,
    damaged: bool,
    needs_ocr: bool,
    likely_scan: bool,
    try_embedded: bool,
    try_crop: bool,
    confirm_isolated: bool,
    transform: Option<PageTransform>,
}

impl PagePlan {
    fn new(
        job: &Job,
        automatic: bool,
        page: &ExtractedPage,
        coverage: ImageCoverage,
        transform: Option<PageTransform>,
    ) -> Option<Self> {
        let native_chars = alphanumeric_count(&page.text);
        let damaged = damaged_text(&page.text);
        let needs_ocr = damaged || page.text.trim().is_empty();
        let likely_scan = coverage.fraction > 0.0;
        if automatic && !needs_ocr && !likely_scan && (native_chars >= 20 || job.ready().is_err()) {
            return None;
        }
        Some(Self {
            native_chars,
            damaged,
            needs_ocr,
            likely_scan,
            try_embedded: coverage.images == 1 && coverage.low_resolution,
            try_crop: likely_scan && coverage.fraction <= 0.60,
            confirm_isolated: likely_scan && !coverage.has_large_image,
            transform,
        })
    }

    fn recognize(
        self,
        job: &Job,
        path: &Path,
        number: usize,
        size: Option<PageSize>,
        automatic: bool,
        page: &ExtractedPage,
    ) -> Result<PageUpdate> {
        let native = (automatic && !self.needs_ocr).then_some(page.text.as_str());
        let mut result = recognize_pdf_page(job, path, number, size, native, self);
        job.check()?;
        if self.try_embedded
            && let (Some(native), Some(size)) = (native, size)
            && let Ok(current) = &mut result
            && current
                .as_ref()
                .is_none_or(|page| same_words(&page.text, native))
            && let Some(mut recovered) = embedded::recover(job, path, number, size, native)
        {
            if let Some(previous) = current {
                recovered.error.clone_from(&previous.error);
                if same_words(&recovered.text, native) && !previous.review.fields.is_empty() {
                    recovered.review = std::mem::take(&mut previous.review);
                }
            }
            *current = Some(recovered);
        }
        job.check()?;
        Ok(match result {
            Ok(Some(recognized))
                if (page.error.is_empty() || !recognized.text.trim().is_empty())
                    && (!automatic
                        || page.text.trim().is_empty()
                        || !self.damaged
                        || (recognized.confidence.is_some_and(|value| value >= 70.0)
                            && alphanumeric_count(&recognized.text)
                                >= (self.native_chars - self.native_chars / 10).max(1))) =>
            {
                PageUpdate::Recognized(recognized)
            }
            Err(failure) if !automatic || self.needs_ocr || self.likely_scan => {
                PageUpdate::Failed(failure.to_string())
            }
            _ => PageUpdate::Unchanged,
        })
    }
}

enum PageUpdate {
    Recognized(ExtractedPage),
    Failed(String),
    Unchanged,
}

impl PageUpdate {
    fn apply(self, page: &mut ExtractedPage, total_bytes: &mut usize) -> Result<()> {
        match self {
            Self::Recognized(recognized) => {
                *total_bytes = *total_bytes - page.text.len() + recognized.text.len();
                *page = recognized;
            }
            Self::Failed(failure) => page.error = failure,
            Self::Unchanged => {}
        }
        if *total_bytes > MAX_TEXT_BYTES {
            return Err(text_limit());
        }
        Ok(())
    }
}

fn native_text(job: &Job, path: &Path, page: Option<usize>, timeout: u64) -> Result<String> {
    let mut command = Command::new("pdftotext");
    command.args(["-layout", "-cropbox", "-enc", "UTF-8"]);
    if let Some(page) = page {
        let number = page.to_string();
        command.args(["-f", &number, "-l", &number]);
    }
    let bytes = job.run(
        command.arg(path).arg("-"),
        MAX_TEXT_BYTES + MAX_PDF_PAGES,
        timeout,
    )?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

fn native_page(text: &str) -> ExtractedPage {
    ExtractedPage::from(text.replace(
        |ch: char| ch.is_control() && !matches!(ch, '\n' | '\r' | '\t'),
        "\u{fffd}",
    ))
}

#[derive(Clone, Copy)]
struct PageSize {
    width: f64,
    height: f64,
    rotation: i32,
}

fn crop_boxes(info: &str, count: usize) -> Vec<Option<PageSize>> {
    let mut bounds = vec![None; count];
    let mut rotations = vec![0; count];
    for line in info.lines() {
        let Some((page, property)) = line
            .strip_prefix("Page ")
            .and_then(|line| line.trim_start().split_once(char::is_whitespace))
        else {
            continue;
        };
        let Some(index) = page
            .parse::<usize>()
            .ok()
            .and_then(|page| page.checked_sub(1))
            .filter(|&index| index < count)
        else {
            continue;
        };
        let property = property.trim_start();
        if let Some(rotation) = property.strip_prefix("rot:")
            && let Ok(rotation @ (90 | 180 | 270)) = rotation.trim().parse::<i32>()
        {
            rotations[index] = rotation;
        }
        let Some(coordinates) = property.strip_prefix("CropBox:") else {
            continue;
        };
        let mut coordinates = coordinates.split_whitespace().map(str::parse::<f64>);
        if let (Some(Ok(left)), Some(Ok(bottom)), Some(Ok(right)), Some(Ok(top))) = (
            coordinates.next(),
            coordinates.next(),
            coordinates.next(),
            coordinates.next(),
        ) {
            let (width, height) = ((right - left).abs(), (top - bottom).abs());
            if width.is_finite() && height.is_finite() && width > 0.0 && height > 0.0 {
                bounds[index] = Some(PageSize {
                    width,
                    height,
                    rotation: rotations[index],
                });
            }
        }
    }
    bounds
}

#[derive(Clone, Copy, Default)]
struct ImageCoverage {
    fraction: f64,
    has_large_image: bool,
    images: u16,
    low_resolution: bool,
}

fn estimate_image_coverage(
    images: &str,
    bounds: &[Option<PageSize>],
    coverage: &mut [ImageCoverage],
) {
    for row in images.lines().skip(2) {
        let mut columns = row.split_whitespace();
        let Some(index) = columns
            .next()
            .and_then(|page| page.parse::<usize>().ok())
            .and_then(|page| page.checked_sub(1))
        else {
            continue;
        };
        let Some(Some(page)) = bounds.get(index) else {
            continue;
        };
        coverage[index].images = coverage[index].images.saturating_add(1);
        if columns.nth(1) != Some("image") {
            continue;
        }
        let width = columns.next().and_then(|value| value.parse::<f64>().ok());
        let height = columns.next().and_then(|value| value.parse::<f64>().ok());
        let mut tail = columns.rev();
        let y_ppi = tail.nth(2).and_then(|value| value.parse::<f64>().ok());
        let x_ppi = tail.next().and_then(|value| value.parse::<f64>().ok());
        if let (Some(width), Some(height), Some(x_ppi), Some(y_ppi)) = (width, height, x_ppi, y_ppi)
            && width.max(height) >= 64.0
            && width.min(height) >= 24.0
            && x_ppi > 0.0
            && y_ppi > 0.0
        {
            let large = width.max(height) >= 160.0;
            let low_resolution = width.max(height) <= f64::from(embedded::MAX_SOURCE_EDGE)
                && x_ppi.max(y_ppi) <= 150.0;
            let (width, height) = (72.0 * width / x_ppi, 72.0 * height / y_ppi);
            let fraction = width * height / (page.width * page.height);
            if fraction >= 0.02 && width.max(height) >= 100.0 && width.min(height) >= 24.0 {
                coverage[index].fraction += fraction;
                coverage[index].has_large_image |= large;
                coverage[index].low_resolution |= low_resolution;
            }
        }
    }
}

fn recognize_pdf_page(
    job: &Job,
    path: &Path,
    number: usize,
    size: Option<PageSize>,
    native: Option<&str>,
    plan: PagePlan,
) -> Result<Option<ExtractedPage>> {
    job.ready()?;
    let points = size.map(|page| page.width.max(page.height));
    let edge = points.map_or(4096, |points| {
        (points * f64::from(PDF_DPI) / 72.0)
            .round()
            .clamp(1200.0, f64::from(MAX_RASTER_EDGE)) as u32
    });
    let resolution = points.map(|points| 72.0 * f64::from(edge) / points);
    let dpi = resolution
        .unwrap_or(f64::from(PDF_DPI))
        .round()
        .clamp(70.0, 600.0) as u32;
    let output = path.with_file_name(format!("page-{number}"));
    let number = number.to_string();
    let mut rotation = 0;
    let reference = if let Some(native) = native {
        let resolution =
            resolution.ok_or_else(|| error(gettext("Page geometry could not be read")))?;
        let tsv = job.run(
            Command::new("pdftotext")
                .args([
                    "-f",
                    &number,
                    "-l",
                    &number,
                    "-r",
                    &resolution.to_string(),
                    "-enc",
                    "UTF-8",
                    "-tsv",
                    "-cropbox",
                ])
                .arg(path)
                .arg("-"),
            MAX_TEXT_BYTES * 4,
            15,
        )?;
        let layout = layout::Layout::read(&String::from_utf8_lossy(&tsv));
        let declared = size.map_or(0, |page| page.rotation);
        if declared != 0 && layout.follows_rotation(declared) {
            rotation = declared;
        }
        Some((native, layout))
    } else {
        None
    };
    let render = |destination: &Path, format: &[&str], seconds| -> Result<()> {
        let mut command = Command::new("pdftoppm");
        command.args([
            "-f",
            &number,
            "-l",
            &number,
            "-cropbox",
            "-scale-to",
            &edge.to_string(),
            "-singlefile",
        ]);
        job.run(
            command.args(format).arg(path).arg(destination),
            1024,
            seconds,
        )?;
        Ok(())
    };
    let full = output.with_extension("ppm");
    let upright = output.with_file_name(format!("page-{number}-oriented.ppm"));
    let mut source = None;
    let mut prepare = |transform, destination: &Path| -> Result<bool> {
        if source.is_none() {
            render(&output, &[], 30)?;
            let Some(mut raster) = raster::Raster::open(&full)? else {
                return Ok(false);
            };
            if plan.transform.is_none() && !raster.has_contrast()? {
                return Ok(false);
            }
            source = Some(raster);
        }
        let Some(source) = &mut source else {
            return Ok(false);
        };
        source.write_transformed(destination, transform, job)?;
        Ok(true)
    };
    if let Some(transform) = plan.transform {
        let result = (|| {
            if !prepare(transform, &upright)? {
                return Err(error(gettext("This page could not be rendered")));
            }
            let mut page = job
                .recognize(&upright, dpi, RecognitionMode::Upright(false))?
                .into_page();
            transform.restore_review(&mut page.review);
            Ok(Some(page))
        })();
        let _ = fs::remove_file(&full);
        let _ = fs::remove_file(&upright);
        return result;
    }
    let confirm_isolated = plan.confirm_isolated;
    let final_options = layout::MergeOptions {
        offset: (0, 0),
        final_pass: true,
        confirm_isolated,
    };
    if rotation != 0
        && let Some((native, mut native_layout)) = reference
    {
        let full = output.with_extension("ppm");
        let upright = output.with_file_name(format!("page-{number}-upright.ppm"));
        let result = (|| {
            render(&output, &[], 30)?;
            let mut source = raster::Raster::open(&full)?
                .ok_or_else(|| error(gettext("This page could not be rendered")))?;
            let dimensions = source.dimensions();
            native_layout.unrotate(rotation, dimensions);
            let transform = match rotation {
                90 => PageTransform::Counterclockwise,
                180 => PageTransform::UpsideDown,
                _ => PageTransform::Clockwise,
            };
            source.write_transformed(&upright, transform, job)?;
            let mut upright_raster = raster::Raster::open(&upright)?
                .ok_or_else(|| error(gettext("This page could not be rendered")))?;
            let [width, height] = upright_raster.dimensions();
            let patch = output.with_file_name(format!("page-{number}-upright-field.ppm"));
            let mut budget = FieldBudget::default();
            let recover = |bounds: [u32; 4], kind: layout::RegionKind<'_>| {
                let region = padded_field(bounds, [width, height])?;
                budget.take(&kind)?;
                let result = upright_raster
                    .write_region(&patch, region, false)
                    .ok()
                    .and_then(|_| job.read_region(&patch, dpi, kind));
                let _ = fs::remove_file(&patch);
                result
            };
            let mut page = job
                .recognize(&upright, dpi, RecognitionMode::Upright(false))?
                .merge_native(&native_layout, native, final_options, recover, |_| {
                    Vec::new()
                });
            if let Some(page) = &mut page {
                transform.restore_review(&mut page.review);
            }
            Ok(page)
        })();
        let _ = fs::remove_file(full);
        let _ = fs::remove_file(upright);
        return result;
    }
    let mut budget = FieldBudget::default();
    let mut recover_field = |bounds: [u32; 4], kind: layout::RegionKind<'_>| {
        let [x, y, width, height] = padded_field(bounds, [MAX_RASTER_EDGE + 1; 2])?;
        budget.take(&kind)?;
        let tile = output.with_file_name(format!("page-{number}-field"));
        let raster = tile.with_extension("ppm");
        let result = render(
            &tile,
            &[
                "-x",
                &x.to_string(),
                "-y",
                &y.to_string(),
                "-W",
                &width.to_string(),
                "-H",
                &height.to_string(),
            ],
            3,
        )
        .ok()
        .and_then(|_| job.read_region(&raster, dpi, kind));
        let _ = fs::remove_file(raster);
        result
    };
    if plan.try_crop
        && let Some((native, native_layout)) = &reference
    {
        let full = output.with_extension("ppm");
        let cropped = output.with_file_name(format!("page-{number}-crop.ppm"));
        let rendered = render(&output, &[], 15).is_ok();
        let result = (|| {
            if !rendered {
                return None;
            }
            let mut raster = raster::Raster::open(&full).ok()??;
            let offset = raster.crop_margins(&cropped).ok()??;
            let recognized = job.recognize(&cropped, dpi, RecognitionMode::Crop).ok()?;
            if !recognized.clear() || recognized.has_unread_regions {
                return None;
            }
            let mut page = recognized.merge_native(
                native_layout,
                native,
                layout::MergeOptions {
                    offset,
                    final_pass: false,
                    confirm_isolated,
                },
                &mut recover_field,
                |missing| {
                    raster
                        .read_fields(missing, &full, job, dpi)
                        .unwrap_or_default()
                },
            )?;
            page.review.dimensions = raster.dimensions();
            Some(page)
        })();
        let full_recognition = if result.is_none() && rendered {
            job.recognize(&full, dpi, RecognitionMode::Standard).ok()
        } else {
            None
        };
        let _ = fs::remove_file(&full);
        let _ = fs::remove_file(&cropped);
        job.check()?;
        if result.is_some() {
            return Ok(result);
        }
        if let Some(recognized) = full_recognition {
            return Ok(recognized.merge_native(
                native_layout,
                native,
                final_options,
                recover_field,
                |_| Vec::new(),
            ));
        }
    }

    let recognize = |extension, format: &[&str]| -> Result<Recognition> {
        let rendered = output.with_extension(extension);
        let result = render(&output, format, 60)
            .and_then(|_| job.recognize(&rendered, dpi, RecognitionMode::Standard));
        let _ = fs::remove_file(&rendered);
        result
    };
    let tiff = if job.0.tiff_unavailable.load(Ordering::Relaxed) {
        None
    } else {
        let result = recognize("tif", &["-tiff", "-tiffcompression", "packbits"]).ok();
        job.check()?;
        if result.is_none() {
            job.0.tiff_unavailable.store(true, Ordering::Relaxed);
        }
        result
    };
    let recognized = match tiff {
        Some(recognized) => recognized,
        None => recognize("png", &["-png"])?,
    };
    Ok(match reference {
        Some((native, native_layout)) => {
            recognized.merge_native(&native_layout, native, final_options, recover_field, |_| {
                Vec::new()
            })
        }
        None => {
            let result = job.orient(recognized, &full, dpi, prepare);
            let _ = fs::remove_file(&full);
            Some(result?.into_page())
        }
    })
}

fn padded_field(
    [left, top, width, height]: [u32; 4],
    [max_x, max_y]: [u32; 2],
) -> Option<[u32; 4]> {
    if width == 0
        || height == 0
        || left.checked_add(width)? > max_x
        || top.checked_add(height)? > max_y
    {
        return None;
    }
    let margin = height.div_ceil(4);
    let x = left.saturating_sub(margin);
    let y = top.saturating_sub(margin);
    Some([
        x,
        y,
        (width + left + margin).min(max_x) - x,
        (height + top + margin).min(max_y) - y,
    ])
}

struct FieldBudget {
    fields: u8,
    numbers: u8,
}

impl Default for FieldBudget {
    fn default() -> Self {
        Self {
            fields: 2,
            numbers: 2,
        }
    }
}

impl FieldBudget {
    fn take(&mut self, kind: &layout::RegionKind<'_>) -> Option<()> {
        let remaining = match kind {
            layout::RegionKind::Field | layout::RegionKind::Line(_) => &mut self.fields,
            layout::RegionKind::Number(_) => &mut self.numbers,
        };
        *remaining = remaining.checked_sub(1)?;
        Some(())
    }
}

fn damaged_text(text: &str) -> bool {
    text.chars().any(|ch| matches!(ch, '\u{fffd}' | '\u{e000}'..='\u{f8ff}' | '\u{f0000}'..='\u{ffffd}' | '\u{100000}'..='\u{10fffd}'))
}
