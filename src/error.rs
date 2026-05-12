use std::{io, num::TryFromIntError, str::Utf8Error};

use thiserror::Error;

pub type Result<T> = std::result::Result<T, MooseError>;

#[derive(Debug, Error)]
pub enum MooseError {
    #[error("I/O failed: {0}")]
    Io(#[from] io::Error),
    #[error("database operation failed: {0}")]
    Database(#[from] rusqlite::Error),
    #[error("HTTP request failed: {0}")]
    Http(#[from] reqwest::Error),
    #[error("HTTP request returned status {0}")]
    HttpStatus(reqwest::StatusCode),
    #[error("JSON parsing failed: {0}")]
    Json(#[from] serde_json::Error),
    #[error("URL parsing failed: {0}")]
    Url(#[from] url::ParseError),
    #[error("stream text decoding failed: {0}")]
    Utf8(#[from] Utf8Error),
    #[error("missing home directory")]
    MissingHomeDirectory,
    #[error("invalid provider URL")]
    InvalidProviderUrl,
    #[error("invalid provider name")]
    InvalidProviderName,
    #[error("invalid model name")]
    InvalidModelName,
    #[error("invalid Ollama response: {0}")]
    InvalidOllamaResponse(String),
    #[error("numeric conversion failed: {0}")]
    NumericConversion(#[from] TryFromIntError),
}
