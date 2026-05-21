use std::{fmt, str::FromStr};

use serde::{Deserialize, Serialize};

use crate::{
    core::{new_id, utc_now},
    error::{MooseError, Result},
};

pub use crate::ollama::service::{
    MANAGED_OLLAMA_BASE_URL, MANAGED_OLLAMA_BIND_ADDRESS, MANAGED_OLLAMA_DEFAULT_PORT,
    MANAGED_OLLAMA_HOST, MANAGED_OLLAMA_MIN_PORT, MANAGED_OLLAMA_RESERVED_PORT,
    managed_ollama_base_url, managed_ollama_host, validate_managed_ollama_port,
};

pub const DEFAULT_OLLAMA_BASE_URL: &str = "http://127.0.0.1:11434/api";

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum ProviderKind {
    Ollama,
}

impl ProviderKind {
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Ollama => "ollama",
        }
    }

    fn parse_kind(value: &str) -> Result<Self> {
        match value {
            "ollama" => Ok(Self::Ollama),
            _ => Err(MooseError::InvalidOllamaResponse(format!(
                "unknown provider kind {value}"
            ))),
        }
    }
}

impl fmt::Display for ProviderKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for ProviderKind {
    type Err = MooseError;

    fn from_str(value: &str) -> std::result::Result<Self, Self::Err> {
        Self::parse_kind(value)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Provider {
    pub id: String,
    pub kind: ProviderKind,
    pub name: String,
    pub base_url: String,
    pub is_managed: bool,
    pub is_default: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NewProvider {
    pub kind: ProviderKind,
    pub name: String,
    pub base_url: String,
    pub is_managed: bool,
    pub is_default: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProviderUpdate {
    pub id: String,
    pub name: String,
    pub base_url: String,
    pub is_default: bool,
}

impl NewProvider {
    pub fn managed_ollama(is_default: bool) -> Self {
        Self {
            kind: ProviderKind::Ollama,
            name: "Managed Ollama".to_string(),
            base_url: MANAGED_OLLAMA_BASE_URL.to_string(),
            is_managed: true,
            is_default,
        }
    }

    pub fn managed_ollama_with_port(port: u16, is_default: bool) -> Result<Self> {
        Ok(Self {
            kind: ProviderKind::Ollama,
            name: "Managed Ollama".to_string(),
            base_url: managed_ollama_base_url(port)?,
            is_managed: true,
            is_default,
        })
    }

    pub fn local_ollama(is_default: bool) -> Self {
        Self {
            kind: ProviderKind::Ollama,
            name: "Local Ollama".to_string(),
            base_url: DEFAULT_OLLAMA_BASE_URL.to_string(),
            is_managed: false,
            is_default,
        }
    }

    pub fn into_provider(self) -> Result<Provider> {
        let timestamp = utc_now();
        let base_url = if self.is_managed {
            validate_managed_provider_base_url(&self.base_url)?
        } else {
            validate_base_url(&self.base_url)?
        };

        Ok(Provider {
            id: new_id(),
            kind: self.kind,
            name: validate_provider_name(&self.name)?,
            base_url,
            is_managed: self.is_managed,
            is_default: self.is_default,
            created_at: timestamp.clone(),
            updated_at: timestamp,
        })
    }
}

pub fn validate_provider_name(value: &str) -> Result<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed.chars().any(char::is_control) {
        return Err(MooseError::InvalidProviderName);
    }
    Ok(trimmed.to_string())
}

pub fn validate_base_url(value: &str) -> Result<String> {
    let trimmed = value.trim().trim_end_matches('/');
    let url = reqwest::Url::parse(trimmed)?;
    let has_supported_scheme = matches!(url.scheme(), "http" | "https");
    let has_credentials = !url.username().is_empty() || url.password().is_some();
    if !has_supported_scheme
        || url.host().is_none()
        || has_credentials
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(MooseError::InvalidProviderUrl);
    }
    Ok(trimmed.to_string())
}

pub fn managed_ollama_port_from_base_url(value: &str) -> Result<u16> {
    let normalized = validate_base_url(value)?;
    let url = reqwest::Url::parse(&normalized)?;
    let port = url.port().ok_or(MooseError::InvalidProviderUrl)?;

    if url.scheme() != "http"
        || url.host_str() != Some(MANAGED_OLLAMA_BIND_ADDRESS)
        || url.path() != "/api"
    {
        return Err(MooseError::InvalidProviderUrl);
    }

    validate_managed_ollama_port(port)
}

pub fn validate_managed_provider_base_url(value: &str) -> Result<String> {
    managed_ollama_port_from_base_url(value).and_then(managed_ollama_base_url)
}

pub fn validate_model_name(value: &str) -> Result<String> {
    let trimmed = value.trim();
    let valid = !trimmed.is_empty()
        && trimmed.len() <= 256
        && trimmed.chars().all(|character| {
            character.is_ascii_alphanumeric()
                || matches!(character, '.' | ':' | '_' | '-' | '/' | '@')
        });
    if !valid {
        return Err(MooseError::InvalidModelName);
    }
    Ok(trimmed.to_string())
}

#[cfg(test)]
mod tests {
    use super::{validate_base_url, validate_model_name};

    #[test]
    fn provider_url_validation_accepts_local_ollama() {
        let url = validate_base_url("http://127.0.0.1:11434/api/").unwrap();

        assert_eq!(url, "http://127.0.0.1:11434/api");
    }

    #[test]
    fn provider_url_validation_rejects_credentials() {
        assert!(validate_base_url("http://token@127.0.0.1:11434/api").is_err());
    }

    #[test]
    fn model_name_validation_accepts_ollama_names() {
        assert!(validate_model_name("hf.co/library/model-q4:latest").is_ok());
    }

    #[test]
    fn model_name_validation_rejects_whitespace() {
        assert!(validate_model_name("llama 3").is_err());
    }
}
