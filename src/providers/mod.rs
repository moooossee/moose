use std::{fmt, str::FromStr};

use serde::{Deserialize, Serialize};

use crate::{
    core::{new_id, utc_now},
    error::{MooseError, Result},
};
pub mod client;
pub mod credentials;
mod custom;
pub mod policy;
mod stream;
mod wire;

pub use custom::{ApiProtocol, CustomAuthentication, CustomModel, CustomProviderConfig};

pub use crate::ollama::service::{
    MANAGED_OLLAMA_BASE_URL, MANAGED_OLLAMA_BIND_ADDRESS, MANAGED_OLLAMA_DEFAULT_PORT,
    MANAGED_OLLAMA_HOST, MANAGED_OLLAMA_MIN_PORT, MANAGED_OLLAMA_RESERVED_PORT,
    managed_ollama_base_url, managed_ollama_host, managed_ollama_port_is_available,
    validate_managed_ollama_port,
};

pub const DEFAULT_OLLAMA_BASE_URL: &str = "http://127.0.0.1:11434/api";
pub const NVIDIA_BASE_URL: &str = "https://integrate.api.nvidia.com/v1";
pub const CLOUDFLARE_API_HOST: &str = "api.cloudflare.com";
const CLOUDFLARE_ACCOUNT_ID_LEN: usize = 32;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum ProviderKind {
    Ollama,
    OllamaCloud,
    OpenAi,
    Anthropic,
    Groq,
    Gemini,
    Mistral,
    DeepSeek,
    Nvidia,
    Cloudflare,
    Custom,
}

impl ProviderKind {
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Ollama => "ollama",
            Self::OllamaCloud => "ollama-cloud",
            Self::OpenAi => "openai",
            Self::Anthropic => "anthropic",
            Self::Groq => "groq",
            Self::Gemini => "gemini",
            Self::Mistral => "mistral",
            Self::DeepSeek => "deepseek",
            Self::Nvidia => "nvidia",
            Self::Cloudflare => "cloudflare",
            Self::Custom => "custom",
        }
    }

    fn parse_kind(value: &str) -> Result<Self> {
        match value {
            "ollama" => Ok(Self::Ollama),
            "ollama-cloud" => Ok(Self::OllamaCloud),
            "openai" => Ok(Self::OpenAi),
            "anthropic" => Ok(Self::Anthropic),
            "groq" => Ok(Self::Groq),
            "gemini" => Ok(Self::Gemini),
            "mistral" => Ok(Self::Mistral),
            "deepseek" => Ok(Self::DeepSeek),
            "nvidia" => Ok(Self::Nvidia),
            "cloudflare" => Ok(Self::Cloudflare),
            "custom" => Ok(Self::Custom),
            _ => Err(MooseError::InvalidOllamaResponse(format!(
                "unknown provider kind {value}"
            ))),
        }
    }

    pub const CLOUD: [Self; 10] = [
        Self::OllamaCloud,
        Self::Groq,
        Self::OpenAi,
        Self::Anthropic,
        Self::Gemini,
        Self::Mistral,
        Self::DeepSeek,
        Self::Nvidia,
        Self::Cloudflare,
        Self::Custom,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Ollama => "Ollama",
            Self::OllamaCloud => "Ollama Cloud",
            Self::OpenAi => "OpenAI",
            Self::Anthropic => "Claude (Anthropic)",
            Self::Groq => "Groq",
            Self::Gemini => "Google Gemini",
            Self::Mistral => "Mistral AI",
            Self::DeepSeek => "DeepSeek",
            Self::Nvidia => "NVIDIA NIM",
            Self::Cloudflare => "Cloudflare Workers AI",
            Self::Custom => "Custom Provider",
        }
    }

    /// Fixed base URL for providers that have one. Cloudflare is account
    /// specific, so it has no default: build it with `cloudflare_base_url`.
    pub const fn default_base_url(self) -> Option<&'static str> {
        match self {
            Self::Ollama => Some(DEFAULT_OLLAMA_BASE_URL),
            Self::OllamaCloud => Some("https://ollama.com/api"),
            Self::OpenAi => Some("https://api.openai.com/v1"),
            Self::Anthropic => Some("https://api.anthropic.com/v1"),
            Self::Groq => Some("https://api.groq.com/openai/v1"),
            Self::Gemini => Some("https://generativelanguage.googleapis.com/v1beta"),
            Self::Mistral => Some("https://api.mistral.ai/v1"),
            Self::DeepSeek => Some("https://api.deepseek.com"),
            Self::Nvidia => Some(NVIDIA_BASE_URL),
            Self::Cloudflare | Self::Custom => None,
        }
    }

    pub const fn requires_key(self) -> bool {
        !matches!(self, Self::Ollama)
    }

    pub fn validate_url(self, value: &str) -> Result<String> {
        let url = validate_base_url(value)?;
        if self == Self::Custom {
            return Ok(reqwest::Url::parse(&url)?
                .to_string()
                .trim_end_matches('/')
                .into());
        }
        if self == Self::Cloudflare {
            return validate_cloudflare_url(&url);
        }
        if self.requires_key() && Some(url.as_str()) != self.default_base_url() {
            return Err(MooseError::InvalidProviderUrl);
        }
        Ok(url)
    }

    pub fn validate_model_id(self, value: &str) -> Result<String> {
        let model = validate_model_id(value)?;
        match self {
            Self::Ollama | Self::OllamaCloud => validate_model_name(&model),
            Self::Gemini => {
                let name = model.strip_prefix("models/").unwrap_or(&model);
                if name.is_empty()
                    || !name
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || b"-_.".contains(&byte))
                {
                    return Err(MooseError::InvalidModelName);
                }
                Ok(model)
            }
            _ => Ok(model),
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
    pub custom_config: Option<CustomProviderConfig>,
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
        if self.is_managed && self.kind != ProviderKind::Ollama {
            return Err(MooseError::InvalidProviderUrl);
        }
        let base_url = if self.is_managed {
            validate_managed_provider_base_url(&self.base_url)?
        } else {
            self.kind.validate_url(&self.base_url)?
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
            custom_config: (self.kind == ProviderKind::Custom).then(CustomProviderConfig::default),
        })
    }
}

impl Provider {
    pub fn requires_key(&self) -> bool {
        self.custom_config.as_ref().map_or_else(
            || self.kind.requires_key(),
            |config| config.authentication != CustomAuthentication::None,
        )
    }

    pub fn api_kind(&self) -> ProviderKind {
        self.custom_config
            .as_ref()
            .map_or(self.kind, |config| match config.protocol {
                ApiProtocol::ChatCompletions => ProviderKind::Custom,
                ApiProtocol::Responses => ProviderKind::OpenAi,
                ApiProtocol::Anthropic => ProviderKind::Anthropic,
                ApiProtocol::Gemini => ProviderKind::Gemini,
            })
    }

    pub fn validate_configuration(&self) -> Result<()> {
        if self.is_managed && self.kind != ProviderKind::Ollama {
            return Err(MooseError::InvalidProviderUrl);
        }
        match (self.kind, &self.custom_config) {
            (ProviderKind::Custom, Some(config)) => config.validate(),
            (ProviderKind::Custom, None) => Err(MooseError::ProviderRequest(
                "Custom provider settings are missing.".into(),
            )),
            (_, Some(_)) => Err(MooseError::ProviderRequest(
                "Custom settings require a custom provider.".into(),
            )),
            (_, None) => Ok(()),
        }
    }

    pub fn is_remote(&self) -> bool {
        self.kind.requires_key() || !self.is_managed
    }

    pub fn location_label(&self) -> &'static str {
        if self.is_managed {
            "Local · Managed by Moose"
        } else if self.kind.requires_key() {
            "Cloud"
        } else {
            "External Ollama · Execution location unverified"
        }
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
    if url.scheme() == "http" && !is_loopback_url(&url) {
        return Err(MooseError::ProviderRequest(
            "Remote providers require HTTPS. HTTP is only allowed on this device.".into(),
        ));
    }
    Ok(trimmed.to_string())
}

/// Builds the Workers AI base URL from a Cloudflare account ID
/// (32 hex characters, shown in the Cloudflare dashboard).
pub fn cloudflare_base_url(account_id: &str) -> Result<String> {
    let account_id = account_id.trim();
    if account_id.len() != CLOUDFLARE_ACCOUNT_ID_LEN
        || !account_id.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(MooseError::InvalidProviderUrl);
    }
    Ok(format!(
        "https://{CLOUDFLARE_API_HOST}/client/v4/accounts/{}/ai/v1",
        account_id.to_ascii_lowercase()
    ))
}

/// Pins the host and path shape so an API token can only be sent to
/// `api.cloudflare.com`, then normalizes to `.../accounts/<id>/ai/v1`.
fn validate_cloudflare_url(value: &str) -> Result<String> {
    let url = reqwest::Url::parse(value)?;
    if url.scheme() != "https"
        || url.host_str() != Some(CLOUDFLARE_API_HOST)
        || url.port().is_some()
    {
        return Err(MooseError::InvalidProviderUrl);
    }
    let segments: Vec<&str> = url
        .path_segments()
        .map(|segments| segments.collect::<Vec<_>>())
        .unwrap_or_default();
    match segments.as_slice() {
        ["client", "v4", "accounts", account_id, "ai"]
        | ["client", "v4", "accounts", account_id, "ai", "v1"] => cloudflare_base_url(account_id),
        _ => Err(MooseError::InvalidProviderUrl),
    }
}

pub fn is_loopback_url(url: &reqwest::Url) -> bool {
    match url.host() {
        Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
        Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
        Some(url::Host::Domain(host)) => host == "localhost",
        None => false,
    }
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

pub fn validate_model_id(value: &str) -> Result<String> {
    if value.trim().is_empty() || value.len() > 256 || value.chars().any(char::is_control) {
        return Err(MooseError::InvalidModelName);
    }
    Ok(value.to_string())
}

pub fn validate_model_name(value: &str) -> Result<String> {
    let trimmed = value.trim();
    let valid = !trimmed.is_empty()
        && trimmed.len() <= 256
        && trimmed.chars().all(|character| {
            character.is_ascii_alphanumeric()
                || matches!(character, '.' | ':' | '_' | '-' | '/' | '@' | '~' | '+')
        });
    if !valid {
        return Err(MooseError::InvalidModelName);
    }
    Ok(trimmed.to_string())
}

#[cfg(test)]
mod tests {
    use super::{ProviderKind, validate_base_url, validate_model_name};

    const ACCOUNT_ID: &str = "023e105f4ecef8ad9ca31a8372d0c353";

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

    #[test]
    fn model_name_validation_accepts_cloudflare_and_nvidia_names() {
        assert!(validate_model_name("@cf/meta/llama-3.1-8b-instruct").is_ok());
        assert!(validate_model_name("nvidia/llama-3.1-nemotron-70b-instruct").is_ok());
    }

    #[test]
    fn provider_kind_round_trips_new_providers() {
        for kind in ProviderKind::CLOUD {
            assert_eq!(kind.as_str().parse::<ProviderKind>().unwrap(), kind);
        }
    }

    #[test]
    fn nvidia_only_accepts_its_fixed_url() {
        let default = ProviderKind::Nvidia.default_base_url().unwrap();
        assert_eq!(ProviderKind::Nvidia.validate_url(default).unwrap(), default);
        assert!(
            ProviderKind::Nvidia
                .validate_url("https://example.com/v1")
                .is_err()
        );
    }

    #[test]
    fn cloudflare_url_is_pinned_and_normalized() {
        let good = format!("https://api.cloudflare.com/client/v4/accounts/{ACCOUNT_ID}/ai/v1");
        let kind = ProviderKind::Cloudflare;

        assert_eq!(kind.validate_url(&good).unwrap(), good);
        assert_eq!(
            kind.validate_url(&format!(
                "https://api.cloudflare.com/client/v4/accounts/{ACCOUNT_ID}/ai/"
            ))
            .unwrap(),
            good
        );
        assert!(
            kind.validate_url(&format!(
                "https://evil.example/client/v4/accounts/{ACCOUNT_ID}/ai/v1"
            ))
            .is_err()
        );
        assert!(
            kind.validate_url("https://api.cloudflare.com/client/v4/accounts/short/ai/v1")
                .is_err()
        );
        assert!(
            kind.validate_url(&format!("{good}/chat/completions"))
                .is_err()
        );
    }

    #[test]
    fn cloudflare_base_url_requires_a_valid_account_id() {
        assert!(super::cloudflare_base_url(ACCOUNT_ID).is_ok());
        assert!(super::cloudflare_base_url("not-an-account-id").is_err());
    }
}
