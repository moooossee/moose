use super::{ExtractedPage, MAX_TEXT_BYTES, error};
use crate::{
    error::Result,
    i18n::{gettext, gettext_f},
};
use std::{
    fs,
    io::{Read, Write},
    os::unix::fs::{DirBuilderExt, OpenOptionsExt},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        Arc, OnceLock,
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

pub mod languages;

#[derive(Clone)]
pub struct Job(Arc<State>);

struct State {
    canceled: AtomicBool,
    current: AtomicUsize,
    total: AtomicUsize,
    language: String,
    directory: PathBuf,
    ready: OnceLock<std::result::Result<(), String>>,
}

#[derive(Clone, Copy)]
pub enum PdfMode {
    Automatic,
    Recognize(Option<usize>),
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
        let mut process = Process(command
            .env("LC_ALL", "C")
            .env("OMP_THREAD_LIMIT", "1")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|_| error(gettext_f(
                "Text extraction requires {tool}. Reinstall the Flatpak or install this tool for a native build.",
                &[("tool", &tool)],
            )))?);
        let stdout = process
            .0
            .stdout
            .take()
            .ok_or_else(|| error(gettext("Text extraction could not start")))?;
        let stderr = process
            .0
            .stderr
            .take()
            .ok_or_else(|| error(gettext("Text extraction could not start")))?;
        let (sender, receiver) = mpsc::sync_channel(1);
        std::thread::spawn(move || {
            let mut output = Vec::new();
            let result = stdout
                .take((limit + 1) as u64)
                .read_to_end(&mut output)
                .map(|_| output);
            let _ = sender.send(result);
        });
        let (error_sender, error_receiver) = mpsc::sync_channel(1);
        std::thread::spawn(move || {
            let mut stderr = stderr;
            let mut output = Vec::new();
            let _ = stderr.by_ref().take(4096).read_to_end(&mut output);
            let _ = std::io::copy(&mut stderr, &mut std::io::sink());
            let _ = error_sender.send(output);
        });
        let deadline = Instant::now() + Duration::from_secs(seconds);
        let mut output = None;
        loop {
            self.check()?;
            if Instant::now() >= deadline {
                return Err(error(gettext(
                    "Text extraction timed out. Try fewer pages or a smaller document.",
                )));
            }
            if output.is_none() {
                match receiver.recv_timeout(Duration::from_millis(50)) {
                    Ok(result) => {
                        let bytes = result?;
                        if bytes.len() > limit {
                            return Err(error(gettext(
                                "The extracted text exceeds the size limit",
                            )));
                        }
                        output = Some(bytes);
                    }
                    Err(mpsc::RecvTimeoutError::Timeout) => continue,
                    Err(_) => return Err(error(gettext("Text extraction stopped unexpectedly"))),
                }
            }
            if let Some(status) = process.0.try_wait()? {
                if !status.success() {
                    let details = error_receiver
                        .recv_timeout(Duration::from_secs(1))
                        .unwrap_or_default();
                    return Err(error(gettext_f(
                        "{tool} could not process this file: {detail}",
                        &[
                            ("tool", &tool),
                            ("detail", String::from_utf8_lossy(&details).trim()),
                        ],
                    )));
                }
                return Ok(output.unwrap_or_default());
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    fn tesseract(&self) -> Command {
        let mut command = Command::new("tesseract");
        command.arg("--tessdata-dir").arg(&self.0.directory);
        command
    }

    fn ready(&self) -> Result<()> {
        self.check()?;
        self.0.ready.get_or_init(|| {
            if !valid_languages(&self.0.language) {
                return Err(gettext("Install and select an OCR language in Preferences → Text Recognition before recognizing text."));
            }
            if self.0.language.split('+').any(|code| !languages::find(code).is_some_and(|language| language.selectable() && language.installed(&self.0.directory))) {
                return Err(gettext("Some selected OCR languages are not installed. Open Preferences → Text Recognition to install or select languages."));
            }
            let output = self.run(self.tesseract().arg("--list-langs"), 64 * 1024, 15).map_err(|failure| failure.to_string())?;
            let available = String::from_utf8_lossy(&output);
            if self.0.language.split('+').any(|language| !available.lines().any(|line| line.trim() == language)) {
                return Err(gettext_f("OCR languages {languages} are unavailable. Install their Tesseract language data or select installed languages.", &[("languages", &self.0.language)]));
            }
            Ok(())
        }).as_ref().map(|_| ()).map_err(|message| error(message.clone()))
    }

    fn recognize(&self, image: &Path) -> Result<ExtractedPage> {
        self.ready()?;
        let mut command = self.tesseract();
        command.arg(image).args([
            "stdout",
            "-l",
            &self.0.language,
            "--oem",
            "1",
            "--psm",
            if languages::find("osd").is_some_and(|language| language.installed(&self.0.directory))
            {
                "1"
            } else {
                "3"
            },
            "--dpi",
            "300",
            "-c",
            "tessedit_create_tsv=1",
        ]);
        let output = self.run(&mut command, MAX_TEXT_BYTES * 12, 120)?;
        let tsv = String::from_utf8(output)
            .map_err(|_| error(gettext("Recognized text could not be decoded")))?;
        let mut page = ExtractedPage {
            method: "tesseract-lstm-300-v1".into(),
            language: self.0.language.clone(),
            ..ExtractedPage::default()
        };
        let mut previous = Vec::new();
        let mut confidence = 0.0;
        let mut words = 0;
        for row in tsv.lines().skip(1) {
            let fields = row.splitn(12, '\t').collect::<Vec<_>>();
            if fields.len() != 12 || fields[0] != "5" || fields[11].trim().is_empty() {
                continue;
            }
            let line = &fields[1..5];
            if !page.text.is_empty() {
                page.text.push(if previous.as_slice() == line {
                    ' '
                } else {
                    '\n'
                });
            }
            page.text.push_str(fields[11].trim());
            previous = line.to_vec();
            if let Ok(value) = fields[10].parse::<f64>()
                && value.is_finite()
                && (0.0..=100.0).contains(&value)
            {
                confidence += value;
                words += 1;
            }
            if page.text.len() > MAX_TEXT_BYTES {
                return Err(error(gettext(
                    "Documents can contain up to 4 MB of extracted text",
                )));
            }
        }
        page.confidence = (words > 0).then(|| confidence / f64::from(words));
        Ok(page)
    }
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

struct Process(Child);

impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
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

pub fn image(bytes: &[u8], cache: &Path, job: &Job) -> Result<ExtractedPage> {
    job.page(1, 1)?;
    let workspace = Workspace::new(cache)?;
    job.recognize(&workspace.write("image.png", bytes)?)
}

pub fn pdf(bytes: &[u8], cache: &Path, job: &Job, mode: PdfMode) -> Result<Vec<ExtractedPage>> {
    job.check()?;
    let workspace = Workspace::new(cache)?;
    let path = workspace.write("input.pdf", bytes)?;
    let info = job.run(Command::new("pdfinfo").arg(&path), 64 * 1024, 30)?;
    let count = String::from_utf8_lossy(&info)
        .lines()
        .find_map(|line| line.strip_prefix("Pages:")?.trim().parse::<usize>().ok())
        .filter(|count| *count > 0 && *count <= 1000)
        .ok_or_else(|| error(gettext("PDFs must contain between 1 and 1,000 pages")))?;
    if let PdfMode::Recognize(Some(page)) = mode
        && !(1..=count).contains(&page)
    {
        return Err(error(gettext("This page is not available in the PDF")));
    }
    let native = match mode {
        PdfMode::Automatic => job.run(
            Command::new("pdftotext")
                .args(["-layout", "-enc", "UTF-8"])
                .arg(&path)
                .arg("-"),
            MAX_TEXT_BYTES,
            30,
        )?,
        PdfMode::Recognize(_) => Vec::new(),
    };
    let text =
        String::from_utf8(native).map_err(|_| error(gettext("PDF text could not be decoded")))?;
    let mut pages = text
        .split('\u{c}')
        .take(count)
        .map(|text| ExtractedPage::from(text.to_string()))
        .collect::<Vec<_>>();
    pages.resize_with(count, ExtractedPage::default);
    let mut total_bytes = pages.iter().map(|page| page.text.len()).sum::<usize>();
    let rendered = workspace.0.join("page.png");
    for (index, page) in pages.iter_mut().enumerate() {
        job.page(index + 1, count)?;
        let recognize = match mode {
            PdfMode::Automatic => {
                page.text.chars().filter(|ch| ch.is_alphanumeric()).count() < 20
                    || page.text.contains('\u{fffd}')
            }
            PdfMode::Recognize(selected) => selected.is_none_or(|number| number == index + 1),
        };
        if !recognize {
            continue;
        }
        let number = (index + 1).to_string();
        let result = (|| {
            job.ready()?;
            job.run(
                Command::new("pdftoppm")
                    .args([
                        "-f",
                        &number,
                        "-l",
                        &number,
                        "-r",
                        "300",
                        "-scale-to",
                        "4096",
                        "-gray",
                        "-singlefile",
                        "-png",
                    ])
                    .arg(&path)
                    .arg(workspace.0.join("page")),
                1024,
                60,
            )?;
            job.recognize(&rendered)
        })();
        let _ = fs::remove_file(&rendered);
        job.check()?;
        match result {
            Ok(recognized) => {
                if !recognized.text.trim().is_empty() || page.text.trim().is_empty() {
                    total_bytes = total_bytes - page.text.len() + recognized.text.len();
                    if total_bytes > MAX_TEXT_BYTES {
                        return Err(error(gettext(
                            "Documents can contain up to 4 MB of extracted text",
                        )));
                    }
                    *page = recognized;
                }
            }
            Err(failure) => page.error = failure.to_string(),
        }
    }
    Ok(pages)
}
