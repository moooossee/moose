use std::{io, num::TryFromIntError, path::PathBuf, str::Utf8Error};

use thiserror::Error;

pub type Result<T> = std::result::Result<T, MooseError>;

#[derive(Debug, Error)]
pub enum MooseError {
    #[error("{}", crate::i18n::moose_error_message(self))]
    CredentialStoreUnavailable,
    #[error("{}", crate::i18n::moose_error_message(self))]
    MissingApiKey,
    #[error("{}", crate::i18n::moose_error_message(self))]
    InvalidApiKey,
    #[error("{}", crate::i18n::moose_error_message(self))]
    LocalOnly,
    #[error("{}", crate::i18n::moose_error_message(self))]
    ProviderResponse(String),
    #[error("{}", crate::i18n::moose_error_message(self))]
    ResponseLimit,
    #[error("{}", crate::i18n::moose_error_message(self))]
    ProviderRequest(String),
    #[error("{}", crate::i18n::moose_error_message(self))]
    ContextTooLarge,
    #[error("{}", crate::i18n::moose_error_message(self))]
    Attachment(String),
    #[error("{}", crate::i18n::moose_error_message(self))]
    Io(#[from] io::Error),
    #[error("{}", crate::i18n::moose_error_message(self))]
    Database(#[from] rusqlite::Error),
    #[error("{}", crate::i18n::moose_error_message(self))]
    Http(#[from] reqwest::Error),
    #[error("{}", crate::i18n::moose_error_message(self))]
    StreamStalled { seconds: u64 },
    #[error("{}", crate::i18n::moose_error_message(self))]
    HttpStatus {
        status: reqwest::StatusCode,
        message: String,
    },
    #[error("{}", crate::i18n::moose_error_message(self))]
    Json(#[from] serde_json::Error),
    #[error("{}", crate::i18n::moose_error_message(self))]
    Url(#[from] url::ParseError),
    #[error("{}", crate::i18n::moose_error_message(self))]
    Utf8(#[from] Utf8Error),
    #[error("{}", crate::i18n::moose_error_message(self))]
    MissingHomeDirectory,
    #[error("{}", crate::i18n::moose_error_message(self))]
    InvalidProviderUrl,
    #[error("{}", crate::i18n::moose_error_message(self))]
    InvalidProviderName,
    #[error("{}", crate::i18n::moose_error_message(self))]
    InvalidModelName,
    #[error("{}", crate::i18n::moose_error_message(self))]
    InvalidConversationTitle,
    #[error("{}", crate::i18n::moose_error_message(self))]
    InvalidMessageContent,
    #[error("{}", crate::i18n::moose_error_message(self))]
    InvalidMessageRole,
    #[error("{}", crate::i18n::moose_error_message(self))]
    InvalidMessageStatus,
    #[error("{}", crate::i18n::moose_error_message(self))]
    InvalidIdentifier,
    #[error("{}", crate::i18n::moose_error_message(self))]
    InvalidGenerationSettings,
    #[error("{}", crate::i18n::moose_error_message(self))]
    InvalidProfileName,
    #[error("{}", crate::i18n::moose_error_message(self))]
    InvalidProfileDescription,
    #[error("{}", crate::i18n::moose_error_message(self))]
    InvalidSystemPrompt,
    #[error("{}", crate::i18n::moose_error_message(self))]
    InvalidDownloadJobStatus,
    #[error("{}", crate::i18n::moose_error_message(self))]
    InvalidDownloadJobProgress,
    #[error("{}", crate::i18n::moose_error_message(self))]
    ConversationNotFound,
    #[error("{}", crate::i18n::moose_error_message(self))]
    MessageNotFound,
    #[error("{}", crate::i18n::moose_error_message(self))]
    GenerationSettingsNotFound,
    #[error("{}", crate::i18n::moose_error_message(self))]
    ProfileNotFound,
    #[error("{}", crate::i18n::moose_error_message(self))]
    ProfileNameAlreadyExists,
    #[error("{}", crate::i18n::moose_error_message(self))]
    BuiltinProfileCannotBeDeleted,
    #[error("{}", crate::i18n::moose_error_message(self))]
    DownloadJobNotFound,
    #[error("{}", crate::i18n::moose_error_message(self))]
    ProviderNotConfigured,
    #[error("{}", crate::i18n::moose_error_message(self))]
    InvalidOllamaResponse(String),
    #[error("{}", crate::i18n::moose_error_message(self))]
    OllamaUpdateRequired(String),
    #[error("{}", crate::i18n::moose_error_message(self))]
    ManagedOllamaUpdate(String),
    #[error("{}", crate::i18n::moose_error_message(self))]
    ManagedOllamaCancelled,
    #[error("{}", crate::i18n::moose_error_message(self))]
    ManagedOllamaUnsupportedArchitecture(String),
    #[error("{}", crate::i18n::moose_error_message(self))]
    ManagedOllamaManifestInvalid(String),
    #[error("{}", crate::i18n::moose_error_message(self))]
    ManagedOllamaDownloadFailed(String),
    #[error("{}", crate::i18n::moose_error_message(self))]
    ManagedOllamaChecksumMismatch { expected: String, actual: String },
    #[error("{}", crate::i18n::moose_error_message(self))]
    ManagedOllamaExtractionFailed(String),
    #[error("{}", crate::i18n::moose_error_message(self))]
    ManagedOllamaBinaryMissing(PathBuf),
    #[error("{}", crate::i18n::moose_error_message(self))]
    ManagedOllamaUnavailable,
    #[error("{}", crate::i18n::moose_error_message(self))]
    ManagedOllamaStartFailed(String),
    #[error("{}", crate::i18n::moose_error_message(self))]
    ManagedOllamaTimedOut,
    #[error("{}", crate::i18n::moose_error_message(self))]
    ManagedOllamaInUse,
    #[error("{}", crate::i18n::moose_error_message(self))]
    ManagedOllamaPortUnavailable(String),
    #[error("{}", crate::i18n::moose_error_message(self))]
    ManagedOllamaInvalidPort(u16),
    #[error("{}", crate::i18n::moose_error_message(self))]
    NumericConversion(#[from] TryFromIntError),
}
