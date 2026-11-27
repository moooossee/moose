use crate::i18n::gettext;

use crate::error::{MooseError, Result};
use serde::{Deserialize, Serialize};

pub mod ocr;

pub const MAX_FILE_BYTES: usize = 25 * 1024 * 1024;
pub const MAX_TEXT_BYTES: usize = 4 * 1024 * 1024;
pub const MAX_ATTACHMENTS: usize = 8;
pub const MAX_IMAGE_BYTES: usize = 20 * 1024 * 1024;
pub(crate) const CHUNK_OVERLAP: usize = 180;

#[derive(Clone, Debug, Serialize)]
pub struct Asset {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub mime_type: String,
    pub byte_size: i64,
    pub page_count: i64,
    pub in_library: bool,
    pub has_text: bool,
    pub text_only: bool,
}

impl Asset {
    pub fn is_paged_document(&self) -> bool {
        self.kind == "pdf" || self.mime_type == "image/tiff"
    }

    pub fn supports_ocr(&self) -> bool {
        self.kind == "image" || self.is_paged_document()
    }

    pub fn requires_vision(&self) -> bool {
        self.kind == "image" && !self.text_only
    }
}

pub enum FileEntry {
    Uploaded(Asset),
    Generated {
        document: crate::documents::CreatedDocument,
        conversation_title: String,
    },
}

pub struct ImportedAsset {
    pub name: String,
    pub kind: String,
    pub mime_type: String,
    pub payload: Vec<u8>,
    pub original: Option<Vec<u8>>,
    pub pages: Vec<ExtractedPage>,
}

#[derive(Clone, Debug, Default)]
pub struct ExtractedPage {
    pub text: String,
    pub method: String,
    pub language: String,
    pub confidence: Option<f64>,
    pub error: String,
    pub review: NumericReview,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct NumericReview {
    pub dimensions: [u32; 2],
    pub fields: Vec<NumericReading>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NumericReading {
    pub text: String,
    pub score: u8,
    pub bounds: [u32; 4],
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alternative: Option<String>,
}

impl From<String> for ExtractedPage {
    fn from(text: String) -> Self {
        Self {
            text,
            method: "native".into(),
            ..Self::default()
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct Source {
    pub asset_id: String,
    pub name: String,
    pub page: i64,
    pub content: String,
}

pub fn error(message: impl Into<String>) -> MooseError {
    MooseError::Attachment(message.into())
}

pub fn chunks(text: &str) -> Vec<String> {
    let mut result = Vec::new();
    let mut start = 0;
    while start < text.len() {
        let mut end = text.len();
        let mut separator = None;
        for (index, (byte, ch)) in text[start..].char_indices().take(1601).enumerate() {
            if index == 1600 {
                end = start + separator.unwrap_or(byte);
                break;
            }
            if index >= 1000 && matches!(ch, '\n' | ' ') {
                separator = Some(byte);
            }
        }
        result.push(text[start..end].to_owned());
        if end == text.len() {
            break;
        }
        start += text[start..end]
            .char_indices()
            .rev()
            .nth(CHUNK_OVERLAP - 1)
            .map_or(end - start, |(byte, _)| byte);
    }
    result
}

pub fn search_expression(query: &str) -> String {
    query
        .split(|ch: char| !ch.is_alphanumeric())
        .filter(|word| word.chars().count() >= 2)
        .filter(|word| {
            !matches!(
                word.to_lowercase().as_str(),
                "the"
                    | "and"
                    | "this"
                    | "that"
                    | "with"
                    | "what"
                    | "please"
                    | "about"
                    | "from"
                    | "para"
                    | "como"
                    | "cómo"
                    | "que"
                    | "qué"
                    | "los"
                    | "las"
                    | "del"
                    | "una"
                    | "con"
            )
        })
        .take(24)
        .map(|word| format!("\"{word}\"*"))
        .collect::<Vec<_>>()
        .join(" OR ")
}

pub fn decode_text(bytes: &[u8]) -> Result<String> {
    let text = if bytes.starts_with(&[0xff, 0xfe]) || bytes.starts_with(&[0xfe, 0xff]) {
        let (pairs, remainder) = bytes[2..].as_chunks::<2>();
        if !remainder.is_empty() {
            return Err(error(gettext("This UTF-16 file is incomplete")));
        }
        let little = bytes[0] == 0xff;
        let words = pairs
            .iter()
            .map(|pair| {
                if little {
                    u16::from_le_bytes(*pair)
                } else {
                    u16::from_be_bytes(*pair)
                }
            })
            .collect::<Vec<_>>();
        String::from_utf16(&words)
            .map_err(|_| error(gettext("This document contains invalid UTF-16 text")))?
    } else {
        std::str::from_utf8(bytes)
            .map_err(|_| error(gettext("Use a UTF-8 or UTF-16 text document")))?
            .trim_start_matches('\u{feff}')
            .to_string()
    };
    if text.len() > MAX_TEXT_BYTES {
        return Err(error(gettext(
            "Documents can contain up to 4 MB of extracted text",
        )));
    }
    if text
        .chars()
        .any(|ch| ch == '\0' || (ch.is_control() && !matches!(ch, '\n' | '\r' | '\t' | '\u{c}')))
    {
        return Err(error(gettext("This file is not a supported text document")));
    }
    if text.trim().is_empty() {
        return Err(error(gettext("This document is empty")));
    }
    Ok(text.replace("\r\n", "\n"))
}
