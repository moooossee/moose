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
    #[error("invalid profile name")]
    InvalidProfileName,
    #[error("invalid profile description")]
    InvalidProfileDescription,
    #[error("invalid system prompt")]
    InvalidSystemPrompt,
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
    #[error("profile was not found")]
    ProfileNotFound,
    #[error("a profile with that name already exists")]
    ProfileNameAlreadyExists,
    #[error("built-in profiles cannot be deleted")]
    BuiltinProfileCannotBeDeleted,
    #[error("download job was not found")]
    DownloadJobNotFound,
    #[error("no Ollama instance is configured")]
    ProviderNotConfigured,
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
    #[error("managed Ollama is unavailable")]
    ManagedOllamaUnavailable,
    #[error("managed Ollama failed to start: {0}")]
    ManagedOllamaStartFailed(String),
    #[error("managed Ollama did not become ready in time")]
    ManagedOllamaTimedOut,
    #[error("managed Ollama port is unavailable: {0}")]
    ManagedOllamaPortUnavailable(String),
    #[error("managed Ollama port {0} is invalid; use 1024-65535 except 11434")]
    ManagedOllamaInvalidPort(u16),
    #[error("numeric conversion failed: {0}")]
    NumericConversion(#[from] TryFromIntError),
}
