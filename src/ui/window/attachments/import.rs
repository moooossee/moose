use crate::i18n::gettext;

use crate::attachments::{
    ExtractedPage, ImportedAsset, MAX_FILE_BYTES, decode_text, error,
    ocr::{self, Job, PageTransform, PdfMode},
};
use crate::error::Result;
use gtk::{gdk_pixbuf, prelude::*};
use std::path::Path;

pub(super) fn parse(
    name: String,
    bytes: Vec<u8>,
    cache: &Path,
    job: &Job,
) -> Result<ImportedAsset> {
    job.check()?;
    if bytes.is_empty() {
        return Err(error(gettext("This file is empty")));
    }
    if bytes.len() > MAX_FILE_BYTES {
        return Err(error(gettext("Choose a file smaller than 25 MB")));
    }
    let extension = Path::new(&name)
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if bytes.starts_with(b"%PDF-")
        || (extension == "pdf"
            && bytes[..bytes.len().min(1024)]
                .windows(5)
                .any(|window| window == b"%PDF-"))
    {
        let pages = ocr::pdf(&bytes, cache, job, PdfMode::Automatic)?;
        return Ok(ImportedAsset {
            name,
            kind: "pdf".into(),
            mime_type: "application/pdf".into(),
            payload: bytes,
            original: None,
            pages,
        });
    }
    if matches!(
        bytes.get(..4),
        Some(b"II*\0" | b"MM\0*" | b"II+\0" | b"MM\0+")
    ) {
        let pages = ocr::tiff(&bytes, cache, job, PdfMode::Automatic)?;
        return Ok(ImportedAsset {
            name,
            kind: "document".into(),
            mime_type: "image/tiff".into(),
            payload: bytes,
            original: None,
            pages,
        });
    }
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n")
        || bytes.starts_with(&[0xff, 0xd8, 0xff])
        || is_heif(&bytes)
        || (bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP"))
    {
        return image(name, bytes, job);
    }
    if matches!(
        extension.as_str(),
        "png" | "jpg" | "jpeg" | "webp" | "heic" | "heif" | "avif" | "tif" | "tiff"
    ) {
        return Err(error(gettext("This file is not a supported image")));
    }
    if extension == "pdf" {
        return Err(error(gettext("This file is not a valid PDF")));
    }
    let text = decode_text(&bytes)?;
    Ok(ImportedAsset {
        name,
        kind: "document".into(),
        mime_type: "text/plain".into(),
        payload: bytes,
        original: None,
        pages: vec![ExtractedPage::from(text)],
    })
}

fn is_heif(bytes: &[u8]) -> bool {
    if bytes.get(4..8) != Some(b"ftyp") {
        return false;
    }
    let size = u32::from_be_bytes(bytes[..4].try_into().unwrap_or_default()) as usize;
    let Some(header) = bytes.get(..size).filter(|header| header.len() >= 16) else {
        return false;
    };
    let (brands, _) = header[8..].as_chunks::<4>();
    std::iter::once(&brands[0])
        .chain(&brands[2..])
        .any(|brand| {
            matches!(
                brand,
                b"heic" | b"heix" | b"heim" | b"heis" | b"mif1" | b"avif"
            )
        })
}

fn image_scale(width: i32, height: i32, limit: u32) -> f64 {
    let edge = f64::from(width.max(height));
    let scale = f64::from(limit) / edge;
    if limit == ocr::MAX_RASTER_EDGE && scale < 1.0 {
        let pixels = f64::from(width) * f64::from(height);
        scale
            .max((8_000_000.0 / pixels).sqrt().min(16_384.0 / edge))
            .min(1.0)
    } else {
        scale.min(1.0)
    }
}

fn decode_image(bytes: &[u8], job: &Job, limit: u32) -> Result<gdk_pixbuf::Pixbuf> {
    job.check()?;
    let loader = gdk_pixbuf::PixbufLoader::new();
    let oversized = std::rc::Rc::new(std::cell::Cell::new(false));
    let target = oversized.clone();
    loader.connect_size_prepared(move |loader, width, height| {
        if width <= 0 || height <= 0 || i64::from(width) * i64::from(height) > 40_000_000 {
            target.set(true);
            loader.set_size(1, 1);
        } else {
            let ratio = image_scale(width, height, limit);
            loader.set_size(
                (f64::from(width) * ratio).round().max(1.0) as i32,
                (f64::from(height) * ratio).round().max(1.0) as i32,
            );
        }
    });
    loader.write(bytes).map_err(|_| {
        error(gettext(
            "This image could not be decoded with the installed image codecs.",
        ))
    })?;
    loader
        .close()
        .map_err(|_| error(gettext("This image is incomplete or damaged")))?;
    if oversized.get() {
        return Err(error(gettext(
            "Choose an image with fewer than 40 million pixels",
        )));
    }
    job.check()?;
    let pixbuf = loader
        .pixbuf()
        .ok_or_else(|| error(gettext("This image could not be opened")))?;
    let pixbuf = pixbuf.apply_embedded_orientation().unwrap_or(pixbuf);
    let ratio = image_scale(pixbuf.width(), pixbuf.height(), limit);
    if ratio >= 1.0 {
        return Ok(pixbuf);
    }
    pixbuf
        .scale_simple(
            (f64::from(pixbuf.width()) * ratio).round().max(1.0) as i32,
            (f64::from(pixbuf.height()) * ratio).round().max(1.0) as i32,
            gdk_pixbuf::InterpType::Bilinear,
        )
        .ok_or_else(|| error(gettext("This image could not be prepared")))
}

pub(super) fn image(name: String, bytes: Vec<u8>, job: &Job) -> Result<ImportedAsset> {
    let pixbuf = decode_image(&bytes, job, 2048)?;
    let payload = pixbuf
        .save_to_bufferv("png", &[])
        .map_err(|_| error(gettext("This image could not be prepared")))?;
    if payload.len() > crate::attachments::MAX_IMAGE_BYTES {
        return Err(error(gettext("This image is too large after processing")));
    }
    job.check()?;
    Ok(ImportedAsset {
        name,
        kind: "image".into(),
        mime_type: "image/png".into(),
        payload,
        original: Some(bytes),
        pages: Vec::new(),
    })
}

pub(super) fn prepared_image(bytes: &[u8], job: &Job) -> Result<gdk_pixbuf::Pixbuf> {
    let pixbuf = decode_image(bytes, job, ocr::MAX_RASTER_EDGE)?;
    let scale = (1800.0 / f64::from(pixbuf.width().max(pixbuf.height()))).clamp(1.0, 2.0);
    let width = (f64::from(pixbuf.width()) * scale).round() as i32;
    let height = (f64::from(pixbuf.height()) * scale).round() as i32;
    let margin = 12;
    let background = if white_ink_on_alpha(&pixbuf) {
        0x000000ff
    } else {
        0xffffffff
    };
    let prepared = gdk_pixbuf::Pixbuf::new(
        gdk_pixbuf::Colorspace::Rgb,
        false,
        8,
        width + margin * 2,
        height + margin * 2,
    )
    .ok_or_else(|| error(gettext("This image could not be prepared")))?;
    prepared.fill(background);
    pixbuf.composite(
        &prepared,
        margin,
        margin,
        width,
        height,
        f64::from(margin),
        f64::from(margin),
        f64::from(width) / f64::from(pixbuf.width()),
        f64::from(height) / f64::from(pixbuf.height()),
        gdk_pixbuf::InterpType::Bilinear,
        255,
    );
    job.check()?;
    Ok(prepared)
}

pub(super) fn recognize_image(bytes: &[u8], cache: &Path, job: &Job) -> Result<ExtractedPage> {
    recognize_image_oriented(bytes, cache, job, None)
}

pub(super) fn recognize_image_oriented(
    bytes: &[u8],
    cache: &Path,
    job: &Job,
    transform: Option<PageTransform>,
) -> Result<ExtractedPage> {
    use gdk_pixbuf::PixbufRotation;
    let prepared = prepared_image(bytes, job)?;
    let unavailable = || error(gettext("This image could not be prepared"));
    let png = if transform.is_some() {
        Vec::new()
    } else {
        prepared
            .save_to_bufferv("png", &[("compression", "1")])
            .map_err(|_| unavailable())?
    };
    let mut contrast = None;
    ocr::image_with_orientation(
        &png,
        cache,
        job,
        shaded_background(&prepared),
        transform,
        |orientation, path| {
            if transform.is_none() && !*contrast.get_or_insert_with(|| has_contrast(&prepared)) {
                return Ok(false);
            }
            let rotated = match orientation {
                PageTransform::Clockwise => prepared.rotate_simple(PixbufRotation::Clockwise),
                PageTransform::UpsideDown => prepared.rotate_simple(PixbufRotation::Upsidedown),
                PageTransform::Counterclockwise => {
                    prepared.rotate_simple(PixbufRotation::Counterclockwise)
                }
                PageTransform::MirrorHorizontal => prepared.flip(true),
                PageTransform::MirrorVertical => prepared.flip(false),
            }
            .ok_or_else(unavailable)?;
            rotated
                .savev(path, "png", &[("compression", "1")])
                .map_err(|_| unavailable())?;
            Ok(true)
        },
    )
}

fn has_contrast(pixbuf: &gdk_pixbuf::Pixbuf) -> bool {
    let Some(sample) = pixbuf.scale_simple(64, 64, gdk_pixbuf::InterpType::Tiles) else {
        return false;
    };
    let pixels = sample.read_pixel_bytes();
    let (mut low, mut high) = (u8::MAX, u8::MIN);
    for row in pixels.chunks(sample.rowstride() as usize) {
        for [red, green, blue] in row[..64 * 3].as_chunks::<3>().0 {
            let gray = ((77 * u32::from(*red) + 150 * u32::from(*green) + 29 * u32::from(*blue))
                >> 8) as u8;
            low = low.min(gray);
            high = high.max(gray);
        }
    }
    high - low >= 30
}

fn white_ink_on_alpha(pixbuf: &gdk_pixbuf::Pixbuf) -> bool {
    if !pixbuf.has_alpha() {
        return false;
    }
    let pixels = pixbuf.read_pixel_bytes();
    let first_alpha = pixels[3];
    let mut varying_alpha = false;
    let row_bytes = pixbuf.width() as usize * 4;
    for row in pixels
        .chunks(pixbuf.rowstride() as usize)
        .take(pixbuf.height() as usize)
    {
        for rgba in row[..row_bytes].as_chunks::<4>().0 {
            if rgba[3] != 0 && rgba[..3] != [255; 3] {
                return false;
            }
            varying_alpha |= rgba[3] != first_alpha;
        }
    }
    varying_alpha
}

fn shaded_background(pixbuf: &gdk_pixbuf::Pixbuf) -> bool {
    const GRID: usize = 5;
    const TILE: usize = 7;
    let size = (GRID * TILE) as i32;
    let Some(sample) = pixbuf.scale_simple(size, size, gdk_pixbuf::InterpType::Nearest) else {
        return false;
    };
    let stride = sample.rowstride() as usize;
    let pixels = sample.read_pixel_bytes();
    let mut background = [[0; GRID]; GRID];
    for (tile_y, row) in background.iter_mut().enumerate() {
        for (tile_x, level) in row.iter_mut().enumerate() {
            let mut values = [0; TILE * TILE];
            for (index, value) in values.iter_mut().enumerate() {
                let offset =
                    (tile_y * TILE + index / TILE) * stride + (tile_x * TILE + index % TILE) * 3;
                let color = &pixels[offset..offset + 3];
                *value = ((77 * u16::from(color[0])
                    + 150 * u16::from(color[1])
                    + 29 * u16::from(color[2]))
                    >> 8) as i16;
            }
            *level = *values.select_nth_unstable(TILE * TILE * 4 / 5).1;
        }
    }
    let gradient = |lines: [[i16; GRID]; GRID]| {
        let agrees = |direction: i16| {
            lines
                .iter()
                .filter(|line| {
                    line[0].max(line[GRID - 1]) >= 185
                        && (line[GRID - 1] - line[0]) * direction >= 55
                        && line
                            .windows(2)
                            .all(|pair| (pair[1] - pair[0]) * direction >= -8)
                })
                .count()
                >= GRID - 1
        };
        agrees(1) || agrees(-1)
    };
    gradient(background)
        || gradient(std::array::from_fn(|x| {
            std::array::from_fn(|y| background[y][x])
        }))
}
