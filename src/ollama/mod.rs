use std::time::Duration;

use futures_util::StreamExt;
use serde::Deserialize;

use crate::{
    chat::{ChatRequest, ChatStreamEvent},
    error::{MooseError, Result},
    providers::validate_base_url,
};

#[derive(Clone)]
pub struct OllamaClient {
    client: reqwest::Client,
    base_url: reqwest::Url,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OllamaHealth {
    pub available: bool,
    pub version: Option<String>,
    pub message: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OllamaModel {
    pub name: String,
    pub digest: Option<String>,
    pub size_bytes: Option<u64>,
    pub family: Option<String>,
    pub parameter_size: Option<String>,
    pub quantization_level: Option<String>,
    pub modified_at: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OllamaVersion {
    pub version: String,
}

#[derive(Debug, Deserialize)]
struct VersionResponse {
    version: String,
}

#[derive(Debug, Deserialize)]
struct TagsResponse {
    models: Vec<TagModel>,
}

#[derive(Debug, Deserialize)]
struct TagModel {
    name: String,
    modified_at: Option<String>,
    size: Option<u64>,
    digest: Option<String>,
    details: Option<TagModelDetails>,
}

#[derive(Debug, Deserialize)]
struct TagModelDetails {
    family: Option<String>,
    parameter_size: Option<String>,
    quantization_level: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ChatLine {
    message: Option<ChatLineMessage>,
    done: bool,
}

#[derive(Debug, Deserialize)]
struct ChatLineMessage {
    content: String,
}

impl OllamaClient {
    pub fn new(base_url: &str) -> Result<Self> {
        Self::with_timeout(base_url, Duration::from_secs(30))
    }

    pub fn with_timeout(base_url: &str, timeout: Duration) -> Result<Self> {
        let normalized_url = validate_base_url(base_url)?;
        let client = reqwest::Client::builder().timeout(timeout).build()?;
        let base_url = reqwest::Url::parse(&normalized_url)?;
        Ok(Self { client, base_url })
    }

    pub fn base_url(&self) -> &reqwest::Url {
        &self.base_url
    }

    pub async fn health(&self) -> OllamaHealth {
        match self.version().await {
            Ok(version) => OllamaHealth {
                available: true,
                version: Some(version.version.clone()),
                message: format!("Ollama {}", version.version),
            },
            Err(error) => OllamaHealth {
                available: false,
                version: None,
                message: error.to_string(),
            },
        }
    }

    pub async fn version(&self) -> Result<OllamaVersion> {
        let text = self.get_text("version").await?;
        parse_version_response(&text)
    }

    pub async fn list_models(&self) -> Result<Vec<OllamaModel>> {
        let text = self.get_text("tags").await?;
        parse_models_response(&text)
    }

    pub async fn stream_chat<F>(&self, request: ChatRequest, mut on_event: F) -> Result<()>
    where
        F: FnMut(ChatStreamEvent) + Send,
    {
        let response = self
            .client
            .post(self.endpoint("chat")?)
            .json(&request)
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(MooseError::HttpStatus(response.status()));
        }

        let mut stream = response.bytes_stream();
        let mut buffer = Vec::new();

        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            buffer.extend_from_slice(&chunk);

            while let Some(index) = buffer.iter().position(|byte| *byte == b'\n') {
                let line = buffer.drain(..=index).collect::<Vec<_>>();
                let line = std::str::from_utf8(&line[..line.len().saturating_sub(1)])?.trim();
                if let Some(event) = parse_chat_stream_line(line)? {
                    let done = matches!(event, ChatStreamEvent::Done);
                    on_event(event);
                    if done {
                        return Ok(());
                    }
                }
            }
        }

        let line = std::str::from_utf8(&buffer)?.trim();
        if !line.is_empty()
            && let Some(event) = parse_chat_stream_line(line)?
        {
            on_event(event);
        }

        Ok(())
    }

    async fn get_text(&self, endpoint: &str) -> Result<String> {
        let response = self.client.get(self.endpoint(endpoint)?).send().await?;
        if !response.status().is_success() {
            return Err(MooseError::HttpStatus(response.status()));
        }
        response.text().await.map_err(Into::into)
    }

    fn endpoint(&self, endpoint: &str) -> Result<reqwest::Url> {
        let mut url = self.base_url.clone();
        let path = format!(
            "{}/{}",
            url.path().trim_end_matches('/'),
            endpoint.trim_start_matches('/')
        );
        url.set_path(&path);
        Ok(url)
    }
}

pub fn parse_version_response(input: &str) -> Result<OllamaVersion> {
    let response: VersionResponse = serde_json::from_str(input)?;
    Ok(OllamaVersion {
        version: response.version,
    })
}

pub fn parse_models_response(input: &str) -> Result<Vec<OllamaModel>> {
    let response: TagsResponse = serde_json::from_str(input)?;
    Ok(response
        .models
        .into_iter()
        .map(|model| {
            let details = model.details;
            OllamaModel {
                name: model.name,
                digest: model.digest,
                size_bytes: model.size,
                family: details.as_ref().and_then(|details| details.family.clone()),
                parameter_size: details
                    .as_ref()
                    .and_then(|details| details.parameter_size.clone()),
                quantization_level: details.and_then(|details| details.quantization_level),
                modified_at: model.modified_at,
            }
        })
        .collect())
}

pub fn parse_chat_stream_line(line: &str) -> Result<Option<ChatStreamEvent>> {
    if line.is_empty() {
        return Ok(None);
    }

    let response: ChatLine = serde_json::from_str(line)?;

    if response.done {
        return Ok(Some(ChatStreamEvent::Done));
    }

    Ok(response
        .message
        .map(|message| ChatStreamEvent::Token(message.content)))
}

#[cfg(test)]
mod tests {
    use super::{
        OllamaClient, parse_chat_stream_line, parse_models_response, parse_version_response,
    };
    use crate::chat::ChatStreamEvent;

    #[test]
    fn ollama_client_rejects_invalid_urls() {
        assert!(OllamaClient::new("file:///tmp/ollama").is_err());
    }

    #[test]
    fn parse_version_reads_version() {
        let version = parse_version_response(r#"{"version":"0.5.1"}"#).unwrap();

        assert_eq!(version.version, "0.5.1");
    }

    #[test]
    fn parse_models_reads_tags_response() {
        let models = parse_models_response(
            r#"{
                "models": [
                    {
                        "name": "llama3.2:latest",
                        "modified_at": "2026-05-11T12:00:00Z",
                        "size": 2019393189,
                        "digest": "sha256:abc",
                        "details": {
                            "family": "llama",
                            "parameter_size": "3.2B",
                            "quantization_level": "Q4_K_M"
                        }
                    }
                ]
            }"#,
        )
        .unwrap();

        assert_eq!(models.len(), 1);
        assert_eq!(models[0].name, "llama3.2:latest");
        assert_eq!(models[0].size_bytes, Some(2019393189));
        assert_eq!(models[0].family.as_deref(), Some("llama"));
    }

    #[test]
    fn parse_models_rejects_invalid_json() {
        assert!(parse_models_response("{").is_err());
    }

    #[test]
    fn parse_chat_stream_reads_tokens_and_done() {
        let token = parse_chat_stream_line(
            r#"{"message":{"role":"assistant","content":"Hello"},"done":false}"#,
        )
        .unwrap();
        let done = parse_chat_stream_line(r#"{"done":true}"#).unwrap();

        assert_eq!(token, Some(ChatStreamEvent::Token("Hello".to_string())));
        assert_eq!(done, Some(ChatStreamEvent::Done));
    }
}
