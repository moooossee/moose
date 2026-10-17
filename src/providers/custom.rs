use std::collections::HashSet;

use reqwest::header::HeaderName;
use serde::{Deserialize, Serialize};

use crate::error::{MooseError, Result};

use super::validate_model_name;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ApiProtocol {
    #[default]
    ChatCompletions,
    Responses,
    Anthropic,
    Gemini,
}

impl ApiProtocol {
    pub const ALL: [Self; 4] = [
        Self::ChatCompletions,
        Self::Responses,
        Self::Anthropic,
        Self::Gemini,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ChatCompletions => "chat-completions",
            Self::Responses => "responses",
            Self::Anthropic => "anthropic",
            Self::Gemini => "gemini",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::ChatCompletions => "OpenAI Chat Completions",
            Self::Responses => "OpenAI Responses",
            Self::Anthropic => "Anthropic Messages",
            Self::Gemini => "Google Gemini",
        }
    }

    pub fn authentication(self) -> CustomAuthentication {
        match self {
            Self::ChatCompletions | Self::Responses => CustomAuthentication::Bearer,
            Self::Anthropic => CustomAuthentication::Header("x-api-key".into()),
            Self::Gemini => CustomAuthentication::Header("x-goog-api-key".into()),
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CustomAuthentication {
    #[default]
    Bearer,
    Header(String),
    None,
}

impl CustomAuthentication {
    pub fn destination_key(&self) -> String {
        match self {
            Self::Bearer => "bearer".into(),
            Self::Header(name) => format!("header:{name}"),
            Self::None => "none".into(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CustomModel {
    pub id: String,
    #[serde(default)]
    pub vision: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CustomProviderConfig {
    pub protocol: ApiProtocol,
    pub authentication: CustomAuthentication,
    pub discover_models: bool,
    pub models: Vec<CustomModel>,
}

impl Default for CustomProviderConfig {
    fn default() -> Self {
        Self {
            protocol: ApiProtocol::default(),
            authentication: CustomAuthentication::default(),
            discover_models: true,
            models: Vec::new(),
        }
    }
}

impl CustomProviderConfig {
    pub fn validate(&self) -> Result<()> {
        if let CustomAuthentication::Header(name) = &self.authentication {
            let header = HeaderName::from_bytes(name.as_bytes()).map_err(|_| {
                MooseError::ProviderRequest("Enter a valid API key header name.".into())
            })?;
            if matches!(
                header.as_str(),
                "host"
                    | "content-length"
                    | "content-type"
                    | "transfer-encoding"
                    | "connection"
                    | "accept"
                    | "anthropic-version"
            ) {
                return Err(MooseError::ProviderRequest(
                    "This header cannot be used for an API key.".into(),
                ));
            }
        }
        if self.models.len() > 1000 {
            return Err(MooseError::ProviderRequest(
                "A provider can have up to 1,000 manual models.".into(),
            ));
        }
        let mut ids = HashSet::new();
        for model in &self.models {
            if model.id.is_empty() {
                return Err(MooseError::ProviderRequest(
                    "Enter a model ID for each manual model, or remove empty rows.".into(),
                ));
            }
            if validate_model_name(&model.id)? != model.id {
                return Err(MooseError::InvalidModelName);
            }
            if !ids.insert(&model.id) {
                return Err(MooseError::ProviderRequest(
                    "Each manual model must have a unique ID.".into(),
                ));
            }
            let gemini_id = model.id.strip_prefix("models/").unwrap_or(&model.id);
            if self.protocol == ApiProtocol::Gemini
                && (gemini_id.is_empty()
                    || !gemini_id
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || b"-_.".contains(&byte)))
            {
                return Err(MooseError::ProviderRequest(
                    "Gemini model IDs can contain letters, numbers, hyphens, underscores and periods.".into(),
                ));
            }
        }
        if !self.discover_models && self.models.is_empty() {
            return Err(MooseError::ProviderRequest(
                "Add a model ID or enable automatic model discovery.".into(),
            ));
        }
        Ok(())
    }
}
