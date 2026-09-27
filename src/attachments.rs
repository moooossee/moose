use crate::error::{MooseError, Result};
use serde::Serialize;

pub const MAX_FILE_BYTES: usize = 25 * 1024 * 1024;
pub const MAX_TEXT_BYTES: usize = 4 * 1024 * 1024;
pub const MAX_ATTACHMENTS: usize = 8;
pub const MAX_IMAGE_BYTES: usize = 20 * 1024 * 1024;

#[derive(Clone, Debug, Serialize)]
pub struct Asset {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub mime_type: String,
    pub byte_size: i64,
    pub page_count: i64,
    pub in_library: bool,
}

pub struct ImportedAsset {
    pub name: String,
    pub kind: String,
    pub mime_type: String,
    pub payload: Vec<u8>,
    pub pages: Vec<String>,
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
    let chars = text.chars().collect::<Vec<_>>();
    let mut result = Vec::new();
    let mut start = 0;
    while start < chars.len() {
        let limit = (start + 1600).min(chars.len());
        let end = if limit < chars.len() {
            (start + 1000..limit)
                .rev()
                .find(|&index| chars[index] == '\n' || chars[index] == ' ')
                .unwrap_or(limit)
        } else {
            limit
        };
        let chunk = chars[start..end].iter().collect::<String>();
        result.push(chunk);
        if end == chars.len() {
            break;
        }
        start = end.saturating_sub(180).max(start + 1);
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
            return Err(error("This UTF-16 file is incomplete"));
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
            .map_err(|_| error("This document contains invalid UTF-16 text"))?
    } else {
        std::str::from_utf8(bytes)
            .map_err(|_| error("Use a UTF-8 or UTF-16 text document"))?
            .trim_start_matches('\u{feff}')
            .to_string()
    };
    if text.len() > MAX_TEXT_BYTES {
        return Err(error("Documents can contain up to 4 MB of extracted text"));
    }
    if text
        .chars()
        .any(|ch| ch == '\0' || (ch.is_control() && !matches!(ch, '\n' | '\r' | '\t' | '\u{c}')))
    {
        return Err(error("This file is not a supported text document"));
    }
    if text.trim().is_empty() {
        return Err(error("This document is empty"));
    }
    Ok(text.replace("\r\n", "\n"))
}
