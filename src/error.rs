use std::{io, num::TryFromIntError, path::PathBuf, str::Utf8Error};

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
    #[error("generation stream stalled after {seconds} seconds")]
    StreamStalled { seconds: u64 },
    #[error("HTTP request returned status {status}: {message}")]
    HttpStatus {
        status: reqwest::StatusCode,
        message: String,
    },
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
    #[error("invalid conversation title")]
    InvalidConversationTitle,
    #[error("invalid message content")]
    InvalidMessageContent,
    #[error("invalid message role")]
    InvalidMessageRole,
    #[error("invalid message status")]
    InvalidMessageStatus,
    #[error("invalid identifier")]
    InvalidIdentifier,
    #[error("invalid generation settings")]
    InvalidGenerationSettings,
    #[error("invalid download job status")]
    InvalidDownloadJobStatus,
    #[error("invalid download job progress")]
    InvalidDownloadJobProgress,
    #[error("conversation was not found")]
    ConversationNotFound,
    #[error("message was not found")]
    MessageNotFound,
    #[error("generation settings were not found")]
    GenerationSettingsNotFound,
    #[error("download job was not found")]
    DownloadJobNotFound,
    #[error("invalid Ollama response: {0}")]
    InvalidOllamaResponse(String),
    #[error("managed Ollama does not support architecture {0}")]
    ManagedOllamaUnsupportedArchitecture(String),
    #[error("managed Ollama manifest is invalid: {0}")]
    ManagedOllamaManifestInvalid(String),
    #[error("managed Ollama download failed: {0}")]
    ManagedOllamaDownloadFailed(String),
    #[error("managed Ollama checksum mismatch: expected {expected}, got {actual}")]
    ManagedOllamaChecksumMismatch { expected: String, actual: String },
    #[error("managed Ollama extraction failed: {0}")]
    ManagedOllamaExtractionFailed(String),
    #[error("managed Ollama binary is missing at {}", .0.display())]
    ManagedOllamaBinaryMissing(PathBuf),
    #[error("numeric conversion failed: {0}")]
    NumericConversion(#[from] TryFromIntError),
}
