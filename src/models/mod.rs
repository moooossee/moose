use std::{fmt, str::FromStr};

use serde::{Deserialize, Serialize};

use crate::{
    core::{new_id, utc_now},
    error::{MooseError, Result},
    providers::validate_model_name,
};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DownloadJobStatus {
    Queued,
    Running,
    Complete,
    Cancelled,
    Failed,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DownloadJob {
    pub id: String,
    pub provider_id: String,
    pub model_name: String,
    pub status: DownloadJobStatus,
    pub total_bytes: Option<i64>,
    pub completed_bytes: Option<i64>,
    pub error_message: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NewDownloadJob {
    pub provider_id: String,
    pub model_name: String,
}

impl DownloadJobStatus {
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Complete => "complete",
            Self::Cancelled => "cancelled",
            Self::Failed => "failed",
        }
    }

    pub const fn is_active(&self) -> bool {
        matches!(self, Self::Queued | Self::Running)
    }
}

impl fmt::Display for DownloadJobStatus {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for DownloadJobStatus {
    type Err = MooseError;

    fn from_str(value: &str) -> std::result::Result<Self, Self::Err> {
        match value {
            "queued" => Ok(Self::Queued),
            "running" => Ok(Self::Running),
            "complete" => Ok(Self::Complete),
            "cancelled" => Ok(Self::Cancelled),
            "failed" => Ok(Self::Failed),
            _ => Err(MooseError::InvalidDownloadJobStatus),
        }
    }
}

impl NewDownloadJob {
    pub fn into_download_job(self) -> Result<DownloadJob> {
        let timestamp = utc_now();
        Ok(DownloadJob {
            id: new_id(),
            provider_id: validate_required_id(&self.provider_id)?,
            model_name: validate_model_name(&self.model_name)?,
            status: DownloadJobStatus::Running,
            total_bytes: None,
            completed_bytes: Some(0),
            error_message: None,
            created_at: timestamp.clone(),
            updated_at: timestamp,
        })
    }
}

pub fn validate_download_byte_count(value: Option<i64>) -> Result<Option<i64>> {
    if value.is_some_and(|value| value < 0) {
        return Err(MooseError::InvalidDownloadJobProgress);
    }
    Ok(value)
}

pub fn validate_download_error_message(value: Option<&str>) -> Result<Option<String>> {
    let Some(value) = value else {
        return Ok(None);
    };
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    if trimmed.chars().any(invalid_text_character) {
        return Err(MooseError::InvalidMessageContent);
    }
    Ok(Some(trimmed.to_string()))
}

fn validate_required_id(value: &str) -> Result<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed.chars().any(char::is_control) {
        return Err(MooseError::InvalidIdentifier);
    }
    Ok(trimmed.to_string())
}

fn invalid_text_character(character: char) -> bool {
    character.is_control() && !matches!(character, '\n' | '\r' | '\t')
}
