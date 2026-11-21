use crate::i18n::gettext;

use crate::attachments::{
    ExtractedPage, ImportedAsset, MAX_FILE_BYTES, decode_text, error,
    ocr::{self, Job, PdfMode},
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
    if bytes.starts_with(b"%PDF-") {
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
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n")
        || bytes.starts_with(&[0xff, 0xd8, 0xff])
        || (bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP"))
    {
        return image(name, bytes, cache, job, false);
    }
    if matches!(extension.as_str(), "png" | "jpg" | "jpeg" | "webp") {
        return Err(error(gettext(
            "This file is not a supported PNG, JPEG or WebP image",
        )));
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

pub(super) fn image(
    name: String,
    bytes: Vec<u8>,
    cache: &Path,
    job: &Job,
    recognize: bool,
) -> Result<ImportedAsset> {
    job.check()?;
    let loader = gdk_pixbuf::PixbufLoader::new();
    let oversized = std::rc::Rc::new(std::cell::Cell::new(false));
    let target = oversized.clone();
    let limit = if recognize { 4096.0 } else { 2048.0 };
    loader.connect_size_prepared(move |loader, width, height| {
        if width <= 0 || height <= 0 || i64::from(width) * i64::from(height) > 40_000_000 {
            target.set(true);
            loader.set_size(1, 1);
        } else {
            let ratio = (limit / f64::from(width.max(height))).min(1.0);
            loader.set_size(
                (f64::from(width) * ratio).round().max(1.0) as i32,
                (f64::from(height) * ratio).round().max(1.0) as i32,
            );
        }
    });
    loader.write(&bytes).map_err(|_| {
        error(gettext(
            "This image could not be decoded. Use PNG, JPEG or WebP.",
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
    let pages = if recognize {
        let source = pixbuf
            .save_to_bufferv("png", &[])
            .map_err(|_| error(gettext("This image could not be prepared")))?;
        vec![ocr::image(&source, cache, job)?]
    } else {
        Vec::new()
    };
    let ratio = (2048.0 / f64::from(pixbuf.width().max(pixbuf.height()))).min(1.0);
    let pixbuf = if ratio < 1.0 {
        pixbuf
            .scale_simple(
                (f64::from(pixbuf.width()) * ratio).round().max(1.0) as i32,
                (f64::from(pixbuf.height()) * ratio).round().max(1.0) as i32,
                gdk_pixbuf::InterpType::Bilinear,
            )
            .ok_or_else(|| error(gettext("This image could not be prepared")))?
    } else {
        pixbuf
    };
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
        pages,
    })
}
