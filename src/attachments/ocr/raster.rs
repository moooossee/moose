use std::{
    fs::File,
    io::{self, BufRead, BufReader, BufWriter, Read, Seek, SeekFrom, Write},
    path::Path,
};

pub(super) struct Raster {
    file: BufReader<File>,
    width: usize,
    height: usize,
    start: u64,
}

impl Raster {
    pub(super) fn open(path: &Path) -> io::Result<Option<Self>> {
        let mut file = BufReader::new(File::open(path)?);
        let mut magic = [0; 3];
        file.read_exact(&mut magic)?;
        if &magic != b"P6\n" {
            return Ok(None);
        }
        let mut size = String::new();
        (&mut file).take(32).read_line(&mut size)?;
        let mut size = size.split_whitespace().map(str::parse::<usize>);
        let (Some(Ok(width)), Some(Ok(height))) = (size.next(), size.next()) else {
            return Ok(None);
        };
        let max = super::MAX_RASTER_EDGE as usize + 1;
        let mut depth = [0; 4];
        file.read_exact(&mut depth)?;
        if &depth != b"255\n" || !(1..=max).contains(&width) || !(1..=max).contains(&height) {
            return Ok(None);
        }
        let start = file.stream_position()?;
        Ok(Some(Self {
            file,
            width,
            height,
            start,
        }))
    }

    pub(super) fn dimensions(&self) -> [u32; 2] {
        [self.width as u32, self.height as u32]
    }

    pub(super) fn read_gray(&mut self) -> io::Result<Vec<u8>> {
        self.file.seek(SeekFrom::Start(self.start))?;
        let mut row = vec![0; self.width * 3];
        let mut pixels = Vec::with_capacity(self.width * self.height);
        for _ in 0..self.height {
            self.file.read_exact(&mut row)?;
            pixels.extend(row.as_chunks::<3>().0.iter().copied().map(grayscale));
        }
        Ok(pixels)
    }

    pub(super) fn has_contrast(&mut self) -> io::Result<bool> {
        let mut row = vec![0; self.width * 3];
        let (mut low, mut high) = (u8::MAX, u8::MIN);
        for y in (0..self.height).step_by((self.height / 64).max(1)) {
            self.file
                .seek(SeekFrom::Start(self.start + (y * row.len()) as u64))?;
            self.file.read_exact(&mut row)?;
            for &pixel in row.as_chunks::<3>().0 {
                let gray = grayscale(pixel);
                low = low.min(gray);
                high = high.max(gray);
            }
            if high - low >= 30 {
                return Ok(true);
            }
        }
        Ok(false)
    }

    pub(super) fn write_region(
        &mut self,
        destination: &Path,
        [x, y, width, height]: [u32; 4],
        compact: bool,
    ) -> io::Result<()> {
        let (x, y, width, height) = (x as usize, y as usize, width as usize, height as usize);
        if width == 0 || height == 0 || x + width > self.width || y + height > self.height {
            return Err(io::Error::from(io::ErrorKind::InvalidInput));
        }
        let stride = width * 3;
        let mut pixels = vec![0; stride * height];
        for (index, row) in pixels.chunks_exact_mut(stride).enumerate() {
            let offset = ((index + y) * self.width + x) * 3;
            self.file
                .seek(SeekFrom::Start(self.start + offset as u64))?;
            self.file.read_exact(row)?;
        }
        let out_height = if compact {
            (height * 3).div_ceil(4)
        } else {
            height
        };
        let mut output = BufWriter::new(File::create(destination)?);
        write!(output, "P6\n{width} {out_height}\n255\n")?;
        if out_height == height {
            output.write_all(&pixels)?;
        } else {
            let mut row = vec![0; stride];
            for index in 0..out_height {
                let top = index * height;
                let bottom = top + height;
                let first = top / out_height;
                let last = bottom.div_ceil(out_height);
                for (column, pixel) in row.iter_mut().enumerate() {
                    let value = (first..last)
                        .map(|source| {
                            let overlap = bottom.min((source + 1) * out_height)
                                - top.max(source * out_height);
                            usize::from(pixels[source * stride + column]) * overlap
                        })
                        .sum::<usize>();
                    *pixel = ((value + height / 2) / height) as u8;
                }
                output.write_all(&row)?;
            }
        }
        output.flush()
    }

    pub(super) fn write_transformed(
        &mut self,
        destination: &Path,
        transform: super::PageTransform,
        job: &super::Job,
    ) -> crate::error::Result<()> {
        let (width, height) = (self.width, self.height);
        use super::PageTransform::*;
        let (out_width, out_height) = if transform.swaps_axes() {
            (height, width)
        } else {
            (width, height)
        };
        let mut pixels = vec![0; width * height * 3];
        self.file.seek(SeekFrom::Start(self.start))?;
        self.file.read_exact(&mut pixels)?;
        let pixels = pixels.as_chunks::<3>().0;
        let mut destination = BufWriter::new(File::create(destination)?);
        write!(destination, "P6\n{out_width} {out_height}\n255\n")?;
        let mut row = vec![0; out_width * 3];
        for y in 0..out_height {
            job.check()?;
            let row_pixels = row.as_chunks_mut::<3>().0;
            match transform {
                Counterclockwise => {
                    let column = width - 1 - y;
                    for (out, source) in row_pixels
                        .iter_mut()
                        .zip(pixels[column..].iter().step_by(width))
                    {
                        *out = *source;
                    }
                }
                UpsideDown | MirrorHorizontal => {
                    let source_y = if matches!(transform, UpsideDown) {
                        height - 1 - y
                    } else {
                        y
                    };
                    let start = source_y * width;
                    for (out, source) in row_pixels
                        .iter_mut()
                        .zip(pixels[start..start + width].iter().rev())
                    {
                        *out = *source;
                    }
                }
                Clockwise => {
                    for (out, source) in row_pixels
                        .iter_mut()
                        .zip(pixels[y..].iter().step_by(width).rev())
                    {
                        *out = *source;
                    }
                }
                MirrorVertical => {
                    let start = (height - 1 - y) * width;
                    row_pixels.copy_from_slice(&pixels[start..start + width]);
                }
            }
            destination.write_all(&row)?;
        }
        destination.flush()?;
        Ok(())
    }

    pub(super) fn crop_margins(
        &mut self,
        destination: &Path,
    ) -> io::Result<Option<(usize, usize)>> {
        let (width, height) = (self.width, self.height);
        let mut row = vec![0; width * 3];
        let (mut left, mut top, mut right, mut bottom) = (width, height, 0, 0);
        for y in 0..height {
            self.file.read_exact(&mut row)?;
            let mut colors = row.as_chunks::<3>().0.iter();
            if let Some(start) = colors.clone().position(|color| *color != [255; 3]) {
                let end = colors
                    .rposition(|color| *color != [255; 3])
                    .unwrap_or(start);
                left = left.min(start);
                right = right.max(end + 1);
                top = top.min(y);
                bottom = y + 1;
                if (right - left) * (bottom - top) * 3 > width * height * 2 {
                    return Ok(None);
                }
            }
        }
        if bottom == 0 {
            return Ok(None);
        }
        let margin = 32;
        left = left.saturating_sub(margin);
        top = top.saturating_sub(margin);
        right = (right + margin).min(width);
        bottom = (bottom + margin).min(height);
        if (right - left) * (bottom - top) * 3 > width * height * 2 {
            return Ok(None);
        }
        self.file
            .seek(SeekFrom::Start(self.start + (top * row.len()) as u64))?;
        let mut destination = BufWriter::new(File::create(destination)?);
        write!(destination, "P6\n{} {}\n255\n", right - left, bottom - top)?;
        for _ in top..bottom {
            self.file.read_exact(&mut row)?;
            destination.write_all(&row[left * 3..right * 3])?;
        }
        destination.flush()?;
        Ok(Some((left, top)))
    }

    pub(super) fn read_fields(
        &mut self,
        missing: &[(usize, [usize; 4])],
        page: &Path,
        job: &super::Job,
        dpi: u32,
    ) -> Option<Vec<(usize, String, bool, f64)>> {
        let mut fields = Vec::new();
        for &(slot, bounds) in missing {
            job.check().ok()?;
            if let Some((before, field)) = self.isolated_field(bounds) {
                fields.push((slot, before, field));
                if fields.len() == 4 {
                    break;
                }
            }
        }
        if fields.is_empty() {
            return Some(Vec::new());
        }
        let crops = super::Workspace::new(page.parent()?).ok()?;
        let directory = &crops.0;
        let mut list = String::new();
        for (index, (_, _, field)) in fields.iter().enumerate() {
            for (rotation, clockwise) in [false, true].into_iter().enumerate() {
                let name = format!("field-{index}-{rotation}.pgm");
                field
                    .write_horizontal(&directory.join(&name), clockwise)
                    .ok()?;
                list.push_str(&name);
                list.push('\n');
            }
        }
        let manifest = directory.join("fields.list");
        std::fs::write(&manifest, list).ok()?;
        let models = std::path::absolute(&job.0.directory).ok()?;
        let read = |threshold| {
            let output = job
                .run(
                    std::process::Command::new("tesseract")
                        .current_dir(directory)
                        .arg(manifest.file_name()?)
                        .arg("stdout")
                        .arg("--tessdata-dir")
                        .arg(&models)
                        .args([
                            "-l",
                            &job.0.language,
                            "--oem",
                            "1",
                            "--psm",
                            "7",
                            "--dpi",
                            &dpi.to_string(),
                            "-c",
                            threshold,
                            "-c",
                            "tessedit_create_tsv=1",
                        ]),
                    64 * 1024,
                    8,
                )
                .ok()?;
            let mut readings = vec![Vec::new(); fields.len() * 2];
            for row in std::str::from_utf8(&output)
                .ok()?
                .lines()
                .filter_map(super::layout::columns)
            {
                if row[0] == "5" && !row[11].trim().is_empty() {
                    let index = row[1].parse::<usize>().ok()?.checked_sub(1)?;
                    let score = row[10].parse::<f64>().ok()?;
                    if !(0.0..=100.0).contains(&score) {
                        return None;
                    }
                    readings.get_mut(index)?.push((row[11].to_owned(), score));
                }
            }
            Some(readings)
        };
        let otsu = read("thresholding_method=0")?;
        let sauvola = read("thresholding_method=2")?;
        let agreed = |index: usize| {
            let ([(a, p)], [(b, q)]) = (otsu[index].as_slice(), sauvola[index].as_slice()) else {
                return None;
            };
            (a == b
                && a.chars().filter(|c| c.is_numeric()).count() >= 2
                && a.chars()
                    .all(|c| c.is_numeric() || matches!(c, '.' | ',' | '/' | '-' | '%'))
                && p.min(*q) >= super::MIN_LINE_CONFIDENCE
                && p.max(*q) >= super::CLEAR_CONFIDENCE)
                .then(|| (a.clone(), (p + q) / 2.0))
        };
        Some(
            fields
                .into_iter()
                .enumerate()
                .filter_map(|(i, (slot, before, _))| {
                    let (text, confidence) = match (agreed(i * 2), agreed(i * 2 + 1)) {
                        (Some(value), None) | (None, Some(value)) => value,
                        (Some((a, p)), Some((b, q))) if a == b => (a, p.max(q)),
                        _ => return None,
                    };
                    Some((slot, text, before, confidence))
                })
                .collect(),
        )
    }

    fn isolated_field(&mut self, [left, top, right, bottom]: [usize; 4]) -> Option<(bool, Field)> {
        let h = bottom.checked_sub(top)?;
        let y = top.saturating_sub(h / 2);
        let end = bottom.saturating_add(h / 2).min(self.height);
        let height = end.checked_sub(y)?;
        if height == 0 || height > 256 {
            return None;
        }
        let mut band = vec![0; self.width * height * 3];
        self.file
            .seek(SeekFrom::Start(self.start + (y * self.width * 3) as u64))
            .ok()?;
        self.file.read_exact(&mut band).ok()?;
        let mut field = None;
        for (before, begin, end) in [
            (true, 0, left.saturating_sub(h / 2).min(self.width)),
            (
                false,
                right.saturating_add(h / 2).min(self.width),
                self.width,
            ),
        ] {
            let (mut x0, mut y0, mut x1, mut y1) = (end, height, begin, 0);
            for (row_index, row) in band.chunks_exact(self.width * 3).enumerate() {
                let mut pixels = row[begin * 3..end * 3].as_chunks::<3>().0.iter();
                if let Some(first) = pixels.clone().position(|p| *p != [255; 3]) {
                    let last = pixels.rposition(|p| *p != [255; 3])?;
                    x0 = x0.min(begin + first);
                    x1 = x1.max(begin + last + 1);
                    y0 = y0.min(row_index);
                    y1 = row_index + 1;
                }
            }
            if y1 == 0
                || y0 == 0
                || y1 >= height
                || x0 == begin
                || x1 == end
                || x1 - x0 < 3
                || y1 - y0 < 3
                || ((y1 - y0) > (x1 - x0) && (y1 - y0) < 2 * (x1 - x0))
            {
                continue;
            }
            if field.is_some() {
                return None;
            }
            let mut ink = Vec::with_capacity((x1 - x0) * (y1 - y0));
            for row in band.chunks_exact(self.width * 3).take(y1).skip(y0) {
                ink.extend(row[x0 * 3..x1 * 3].as_chunks::<3>().0.iter().map(|p| {
                    ((77 * u32::from(p[0]) + 150 * u32::from(p[1]) + 29 * u32::from(p[2])) >> 8)
                        as u8
                }));
            }
            field = Some((
                before,
                Field {
                    width: x1 - x0,
                    height: y1 - y0,
                    ink,
                },
            ));
        }
        field
    }
}

fn grayscale([r, g, b]: [u8; 3]) -> u8 {
    ((77 * u32::from(r) + 150 * u32::from(g) + 29 * u32::from(b)) >> 8) as u8
}

struct Field {
    width: usize,
    height: usize,
    ink: Vec<u8>,
}

impl Field {
    fn write_horizontal(&self, path: &Path, clockwise: bool) -> io::Result<()> {
        let vertical = self.height > self.width;
        let (w, h) = if vertical {
            (self.height, self.width)
        } else {
            (self.width, self.height)
        };
        let width = w + 24;
        let height = h + 24;
        let mut pixels = vec![255; width * height];
        for y in 0..h {
            for x in 0..w {
                let (column, row) = match (vertical, clockwise) {
                    (true, true) => (y, self.height - 1 - x),
                    (true, false) => (self.width - 1 - y, x),
                    (false, false) => (x, y),
                    (false, true) => (self.width - 1 - x, self.height - 1 - y),
                };
                pixels[(y + 12) * width + x + 12] = self.ink[row * self.width + column];
            }
        }
        let mut out = BufWriter::new(File::create(path)?);
        write!(out, "P5\n{width} {height}\n255\n")?;
        out.write_all(&pixels)?;
        out.flush()
    }
}
