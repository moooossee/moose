use crate::attachments::{ImportedAsset, MAX_FILE_BYTES, MAX_TEXT_BYTES, decode_text, error};
use crate::error::Result;
use gtk::{gdk_pixbuf, prelude::*};
use std::{
    fs,
    io::Read,
    path::Path,
    process::{Command, Stdio},
    sync::mpsc,
    time::{Duration, Instant},
};

pub(super) fn parse(name: String, bytes: Vec<u8>, cache: &Path) -> Result<ImportedAsset> {
    if bytes.is_empty() {
        return Err(error("This file is empty"));
    }
    if bytes.len() > MAX_FILE_BYTES {
        return Err(error("Choose a file smaller than 25 MB"));
    }
    let extension = Path::new(&name)
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if bytes.starts_with(b"%PDF-") {
        let pages = pdf_text(&bytes, cache)?;
        return Ok(ImportedAsset {
            name,
            kind: "pdf".into(),
            mime_type: "application/pdf".into(),
            payload: bytes,
            pages,
        });
    }
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n")
        || bytes.starts_with(&[0xff, 0xd8, 0xff])
        || (bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP"))
    {
        return image(name, &bytes);
    }
    if matches!(extension.as_str(), "png" | "jpg" | "jpeg" | "webp") {
        return Err(error(
            "This file is not a supported PNG, JPEG or WebP image",
        ));
    }
    if extension == "pdf" {
        return Err(error("This file is not a valid PDF"));
    }
    let text = decode_text(&bytes)?;
    Ok(ImportedAsset {
        name,
        kind: "document".into(),
        mime_type: "text/plain".into(),
        payload: bytes,
        pages: vec![text],
    })
}

fn image(name: String, bytes: &[u8]) -> Result<ImportedAsset> {
    let loader = gdk_pixbuf::PixbufLoader::new();
    let oversized = std::rc::Rc::new(std::cell::Cell::new(false));
    let target = oversized.clone();
    loader.connect_size_prepared(move |loader, width, height| {
        if width <= 0 || height <= 0 || i64::from(width) * i64::from(height) > 40_000_000 {
            target.set(true);
            loader.set_size(1, 1);
        } else {
            let ratio = (2048.0 / f64::from(width.max(height))).min(1.0);
            loader.set_size(
                (f64::from(width) * ratio).round().max(1.0) as i32,
                (f64::from(height) * ratio).round().max(1.0) as i32,
            );
        }
    });
    loader
        .write(bytes)
        .map_err(|_| error("This image could not be decoded. Use PNG, JPEG or WebP."))?;
    loader
        .close()
        .map_err(|_| error("This image is incomplete or damaged"))?;
    if oversized.get() {
        return Err(error("Choose an image with fewer than 40 million pixels"));
    }
    let pixbuf = loader
        .pixbuf()
        .ok_or_else(|| error("This image could not be opened"))?;
    let pixbuf = pixbuf.apply_embedded_orientation().unwrap_or(pixbuf);
    let payload = pixbuf
        .save_to_bufferv("png", &[])
        .map_err(|_| error("This image could not be prepared"))?;
    if payload.len() > crate::attachments::MAX_IMAGE_BYTES {
        return Err(error("This image is too large after processing"));
    }
    Ok(ImportedAsset {
        name,
        kind: "image".into(),
        mime_type: "image/png".into(),
        payload,
        pages: Vec::new(),
    })
}

struct TemporaryPdf(std::path::PathBuf);
impl Drop for TemporaryPdf {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

fn pdf_text(bytes: &[u8], cache: &Path) -> Result<Vec<String>> {
    let directory = cache.join("document-imports");
    fs::create_dir_all(&directory)?;
    let temporary = TemporaryPdf(directory.join(format!("{}.pdf", crate::core::new_id())));
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temporary.0)?;
        file.write_all(bytes)?;
    }
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut child = Command::new("pdftotext").args(["-layout", "-enc", "UTF-8", "-f", "1", "-l", "1001"]).arg(&temporary.0).arg("-")
        .stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn()
        .map_err(|_| error("PDF support requires Poppler (pdftotext). Reinstall the Flatpak or install poppler-utils for a native build."))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| error("The PDF reader did not return any text"))?;
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let mut output = Vec::new();
        let result = stdout
            .take((MAX_TEXT_BYTES + 1) as u64)
            .read_to_end(&mut output)
            .map(|_| output);
        let _ = sender.send(result);
    });
    let output = match receiver.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
        Ok(Ok(output)) if output.len() <= MAX_TEXT_BYTES => output,
        Ok(Ok(_)) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(error(
                "This PDF contains too much text. Import a smaller document.",
            ));
        }
        _ => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(error(
                "PDF extraction could not finish. The document may be damaged or too complex.",
            ));
        }
    };
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(10)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(error(
                    "PDF extraction could not finish. Try a smaller document.",
                ));
            }
        }
    };
    if !status.success() {
        return Err(error(
            "This PDF could not be read. It may be password-protected or damaged.",
        ));
    }
    let text = String::from_utf8(output).map_err(|_| error("PDF text could not be decoded"))?;
    let mut pages = text.split('\u{c}').map(str::to_string).collect::<Vec<_>>();
    if pages.last().is_some_and(|page| page.trim().is_empty()) {
        pages.pop();
    }
    if pages.len() > 1000 {
        return Err(error(
            "PDFs can contain up to 1,000 pages. Split this document before importing.",
        ));
    }
    if pages.iter().all(|page| page.trim().is_empty()) {
        return Err(error(
            "This PDF has no selectable text. Scan-only PDFs need OCR before importing.",
        ));
    }
    Ok(pages)
}
