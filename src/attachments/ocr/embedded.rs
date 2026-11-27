use super::{
    CLEAR_CONFIDENCE, ExtractedPage, Job, MAX_RASTER_EDGE, PageSize, Workspace, layout,
    raster::Raster,
};
use std::{
    fs::File,
    io::{BufWriter, Write},
    path::Path,
    process::Command,
};

const DPI: u32 = 216;
const SCALE: f64 = DPI as f64 / 72.0;
pub(super) const MAX_SOURCE_EDGE: u32 = 512;
const PADDING: usize = 12;

pub(super) fn recover(
    job: &Job,
    pdf: &Path,
    page_number: usize,
    page_size: PageSize,
    native: &str,
) -> Option<ExtractedPage> {
    let pdf = std::path::absolute(pdf).ok()?;
    let placement = Workspace::new(pdf.parent()?).ok()?;
    let number = page_number.to_string();
    let mapping = job
        .run(
            Command::new("pdftohtml")
                .current_dir(&placement.0)
                .args([
                    "-f",
                    &number,
                    "-l",
                    &number,
                    "-q",
                    "-xml",
                    "-stdout",
                    "-zoom",
                    "1",
                    "-noroundcoord",
                ])
                .arg(&pdf)
                .arg("image"),
            1024 * 1024,
            6,
        )
        .ok()?;
    let bounds = Placement::read(std::str::from_utf8(&mapping).ok()?, page_number, page_size)?;
    let native_map = job
        .run(
            Command::new("pdftotext")
                .args([
                    "-f",
                    &number,
                    "-l",
                    &number,
                    "-r",
                    &DPI.to_string(),
                    "-enc",
                    "UTF-8",
                    "-tsv",
                    "-cropbox",
                ])
                .arg(&pdf)
                .arg("-"),
            1024 * 1024,
            6,
        )
        .ok()?;
    let mut layout = layout::Layout::read(std::str::from_utf8(&native_map).ok()?);
    if bounds.rotation != 0 {
        if !layout.follows_rotation(bounds.rotation) {
            return None;
        }
        layout.unrotate(bounds.rotation, bounds.page);
    }
    let position = layout.isolated_line_position(native, bounds.reading_interval())?;

    job.run(
        Command::new("pdfimages")
            .args(["-f", &number, "-l", &number])
            .arg(&pdf)
            .arg(placement.0.join("pixels")),
        1024,
        6,
    )
    .ok()?;
    let source = placement.0.join("pixels-000.ppm");
    let mut source = Raster::open(&source).ok()??;
    let [width, height] = source.dimensions();
    if width.max(height) > MAX_SOURCE_EDGE || width.min(height) < 24 {
        return None;
    }
    let original = GrayImage::read(&mut source)?;
    let area = original.line_area()?;
    if !bounds.visible(job, &pdf, &number, &placement.0, &original) {
        return None;
    }

    let line = placement.0.join("line.pgm");
    original.write_line(&line, area, 2).ok()?;
    let mut first = job
        .recognize_pass(&line, 300, "7", "thresholding_method=0", 5, false)
        .ok()?;
    let second = job
        .recognize_pass(&line, 300, "7", "thresholding_method=2", 5, false)
        .ok()?;
    let text = first.page.text.trim();
    let confidence = first.page.confidence.zip(second.page.confidence);
    if !text.contains('\n')
        && super::alphanumeric_count(text) >= 3
        && super::same_words(text, &second.page.text)
        && !text
            .split(|ch: char| !ch.is_alphanumeric())
            .any(|word| word.chars().any(char::is_alphabetic) && word.chars().any(char::is_numeric))
        && first.page.review.fields.is_empty()
        && second.page.review.fields.is_empty()
        && let Some((a, b)) = confidence
        && a.min(b) >= super::MIN_LINE_CONFIDENCE
        && a.max(b) >= CLEAR_CONFIDENCE
        && !native.lines().any(|line| super::same_words(line, text))
        && original.confirm_symbols(job, &line, area, text)
    {
        let (before, after) = native.split_at(position);
        let newline = if before.is_empty() || before.ends_with('\n') {
            ""
        } else {
            "\n"
        };
        return Some(ExtractedPage {
            text: format!("{before}{newline}{text}\n{after}"),
            method: "tesseract-embedded-image".into(),
            language: job.0.language.clone(),
            confidence: Some((a + b) / 2.0),
            ..ExtractedPage::default()
        });
    }

    first.mark_disagreements(vec![second.numbers]);
    first
        .page
        .review
        .fields
        .retain(|field| field.alternative.is_some());
    first.page.review.fields.retain_mut(|field| {
        let Some(region) = bounds.review_region(field.bounds, &original, area) else {
            return false;
        };
        field.bounds = region;
        true
    });
    if first.page.review.fields.is_empty() {
        return None;
    }
    first.page.review.dimensions = bounds.page;
    Some(ExtractedPage {
        review: first.page.review,
        ..ExtractedPage::from(native.to_owned())
    })
}

struct Placement {
    page: [u32; 2],
    rect: [f64; 4],
    rotation: i32,
}

impl Placement {
    fn read(xml: &str, number: usize, size: PageSize) -> Option<Self> {
        let mut pages = xml.lines().filter(|line| line.starts_with("<page "));
        let page = pages.next()?;
        if pages.next().is_some() || attribute(page, "number")?.parse::<usize>().ok()? != number {
            return None;
        }
        let [width, height] = if matches!(size.rotation, 90 | 270) {
            [size.height, size.width]
        } else {
            [size.width, size.height]
        };
        if (coordinate(page, "width")? - width).abs() >= 1.0
            || (coordinate(page, "height")? - height).abs() >= 1.0
            || width.max(height) * SCALE > f64::from(MAX_RASTER_EDGE)
        {
            return None;
        }
        let mut images = xml.lines().filter(|line| line.starts_with("<image "));
        let image = images.next()?;
        let x = coordinate(image, "left")?;
        let y = coordinate(image, "top")?;
        let w = coordinate(image, "width")?;
        let h = coordinate(image, "height")?;
        if images.next().is_some()
            || (w.is_sign_negative(), h.is_sign_negative())
                != match size.rotation {
                    90 => (false, true),
                    180 => (true, true),
                    270 => (true, false),
                    _ => (false, false),
                }
        {
            return None;
        }
        let (left, top) = (x.min(x + w), y.min(y + h));
        if left < 0.0
            || top < 0.0
            || left + w.abs() > width
            || top + h.abs() > height
            || w.abs().min(h.abs()) < 1.0
        {
            return None;
        }
        Some(Self {
            page: [
                (width * SCALE).ceil() as u32,
                (height * SCALE).ceil() as u32,
            ],
            rect: [left * SCALE, top * SCALE, w.abs() * SCALE, h.abs() * SCALE],
            rotation: size.rotation,
        })
    }

    fn reading_interval(&self) -> [f64; 2] {
        let [x, y, w, h] = self.rect;
        let [width, height] = self.page.map(f64::from);
        match self.rotation {
            90 => [width - x - w, width - x],
            180 => [height - y - h, height - y],
            270 => [x, x + w],
            _ => [y, y + h],
        }
    }

    fn point(&self, u: f64, v: f64) -> [f64; 2] {
        let [x, y, width, height] = self.rect;
        let (u, v) = match self.rotation {
            90 => (1.0 - v, u),
            180 => (1.0 - u, 1.0 - v),
            270 => (v, 1.0 - u),
            _ => (u, v),
        };
        [x + width * u, y + height * v]
    }

    fn visible(
        &self,
        job: &Job,
        pdf: &Path,
        number: &str,
        directory: &Path,
        original: &GrayImage,
    ) -> bool {
        let [x, y, w, h] = self.rect;
        let left = x.floor() as usize;
        let top = y.floor() as usize;
        let width = (x + w).ceil() as usize - left;
        let height = (y + h).ceil() as usize - top;
        let basename = directory.join("visible");
        if job
            .run(
                Command::new("pdftoppm")
                    .args([
                        "-f",
                        number,
                        "-l",
                        number,
                        "-cropbox",
                        "-singlefile",
                        "-r",
                        &DPI.to_string(),
                        "-x",
                        &left.to_string(),
                        "-y",
                        &top.to_string(),
                        "-W",
                        &width.to_string(),
                        "-H",
                        &height.to_string(),
                    ])
                    .arg(pdf)
                    .arg(&basename),
                1024,
                6,
            )
            .is_err()
        {
            return false;
        }
        let Ok(Some(mut raster)) = Raster::open(&basename.with_extension("ppm")) else {
            return false;
        };
        let Some(visible) = GrayImage::read(&mut raster) else {
            return false;
        };
        if (visible.width, visible.height) != (width, height) {
            return false;
        }
        let mut deviation = 0;
        for (index, &value) in original.pixels.iter().enumerate() {
            let u = (index % original.width) as f64 + 0.5;
            let v = (index / original.width) as f64 + 0.5;
            let [x, y] = self.point(u / original.width as f64, v / original.height as f64);
            let x = (x as usize).saturating_sub(left);
            let y = (y as usize).saturating_sub(top);
            if x >= width || y >= height {
                return false;
            }
            let difference = value.abs_diff(visible.pixels[y * width + x]);
            if difference > 80 {
                return false;
            }
            deviation += usize::from(difference);
        }
        deviation <= original.pixels.len() * 8
    }

    fn review_region(
        &self,
        [x, y, w, h]: [u32; 4],
        image: &GrayImage,
        [left, top, width, height]: [usize; 4],
    ) -> Option<[u32; 4]> {
        let x = x.checked_sub(PADDING as u32)?;
        let y = y.checked_sub(PADDING as u32)?;
        if x.checked_add(w)? > (width * 2) as u32 || y.checked_add(h)? > (height * 2) as u32 {
            return None;
        }
        let point = |x, y| {
            self.point(
                (left as f64 + f64::from(x) / 2.0) / image.width as f64,
                (top as f64 + f64::from(y) / 2.0) / image.height as f64,
            )
        };
        let a = point(x, y);
        let b = point(x + w, y + h);
        let left = a[0].min(b[0]).floor() as u32;
        let top = a[1].min(b[1]).floor() as u32;
        Some([
            left,
            top,
            a[0].max(b[0]).ceil() as u32 - left,
            a[1].max(b[1]).ceil() as u32 - top,
        ])
    }
}

fn attribute<'a>(element: &'a str, name: &str) -> Option<&'a str> {
    element
        .split_once(&format!(" {name}=\""))?
        .1
        .split_once('"')
        .map(|(value, _)| value)
}

fn coordinate(element: &str, name: &str) -> Option<f64> {
    let value = attribute(element, name)?.parse::<f64>().ok()?;
    value.is_finite().then_some(value)
}

struct GrayImage {
    width: usize,
    height: usize,
    pixels: Vec<u8>,
}

impl GrayImage {
    fn read(raster: &mut Raster) -> Option<Self> {
        let [width, height] = raster.dimensions().map(|size| size as usize);
        Some(Self {
            width,
            height,
            pixels: raster.read_gray().ok()?,
        })
    }

    fn line_area(&self) -> Option<[usize; 4]> {
        let (mut left, mut top, mut right, mut bottom) = (self.width, self.height, 0, 0);
        for (index, &pixel) in self.pixels.iter().enumerate() {
            if pixel == 255 {
                continue;
            }
            let (x, y) = (index % self.width, index / self.width);
            left = left.min(x);
            top = top.min(y);
            right = right.max(x + 1);
            bottom = bottom.max(y + 1);
        }
        let (width, height) = (right.checked_sub(left)?, bottom.checked_sub(top)?);
        (width >= height * 2 && height >= 5 && !self.has_separate_rows(top, bottom))
            .then_some([left, top, width, height])
    }

    fn write_line(
        &self,
        path: &Path,
        [left, top, width, height]: [usize; 4],
        scale: usize,
    ) -> std::io::Result<()> {
        let mut output = BufWriter::new(File::create(path)?);
        let out_width = width * scale + PADDING * 2;
        let out_height = height * scale + PADDING * 2;
        write!(output, "P5\n{out_width} {out_height}\n255\n")?;
        let mut pixels = vec![255; out_width * out_height];
        let at = |x, y| u32::from(self.pixels[(top + y) * self.width + left + x]);
        let weight = (scale * 2) as u32;
        let divisor = weight * weight;
        for y in 0..height * scale {
            let (y0, y1, b) = sample_axis(y, height, scale);
            for x in 0..width * scale {
                let (x0, x1, a) = sample_axis(x, width, scale);
                let upper = at(x0, y0) * (weight - a) + at(x1, y0) * a;
                let lower = at(x0, y1) * (weight - a) + at(x1, y1) * a;
                pixels[(y + PADDING) * out_width + x + PADDING] =
                    ((upper * (weight - b) + lower * b + divisor / 2) / divisor) as u8;
            }
        }
        output.write_all(&pixels)?;
        output.flush()
    }

    fn confirm_symbols(&self, job: &Job, path: &Path, area: [usize; 4], text: &str) -> bool {
        let needs_confirmation = text
            .contains(['+', '−', '±', '=', '×', '÷', '≤', '≥', '√', '≠', '%', '‰'])
            || text.split_whitespace().any(|word| {
                !word.chars().any(char::is_alphanumeric)
                    || word
                        .strip_prefix('-')
                        .is_some_and(|end| end.starts_with(char::is_numeric))
            });
        !needs_confirmation
            || self.write_line(path, area, 3).is_ok()
                && job
                    .recognize_pass(path, 300, "7", "thresholding_method=0", 5, false)
                    .is_ok_and(|confirmed| {
                        super::same_words(text, &confirmed.page.text)
                            && confirmed.page.review.fields.is_empty()
                            && confirmed
                                .page
                                .confidence
                                .is_some_and(|score| score >= super::MIN_LINE_CONFIDENCE)
                    })
    }

    fn has_separate_rows(&self, top: usize, bottom: usize) -> bool {
        let mut last = top;
        for (row, pixels) in self
            .pixels
            .chunks_exact(self.width)
            .enumerate()
            .take(bottom)
            .skip(top)
        {
            if pixels.iter().any(|&pixel| pixel < 192) {
                if last >= top + 2 && bottom - row >= 3 && row - last > 3.max((bottom - top) / 8) {
                    return true;
                }
                last = row;
            }
        }
        false
    }
}

fn sample_axis(coordinate: usize, dimension: usize, scale: usize) -> (usize, usize, u32) {
    let center = (coordinate * 2 + 1).saturating_sub(scale);
    let step = scale * 2;
    let start = center / step;
    (
        start,
        (start + 1).min(dimension - 1),
        (center % step) as u32,
    )
}
