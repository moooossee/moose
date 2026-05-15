use std::time::Duration;

use futures_util::StreamExt;
use serde::{Deserialize, Serialize};

use crate::{
    chat::{ChatRequest, ChatStreamEvent},
    error::{MooseError, Result},
    providers::{validate_base_url, validate_model_name},
};

#[derive(Clone)]
pub struct OllamaClient {
    client: reqwest::Client,
    base_url: reqwest::Url,
    request_timeout: Duration,
    stream_timeout: Duration,
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
    pub families: Vec<String>,
    pub parameter_size: Option<String>,
    pub quantization_level: Option<String>,
    pub modified_at: Option<String>,
    pub supports_chat: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OllamaVersion {
    pub version: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OllamaPullProgress {
    pub status: String,
    pub digest: Option<String>,
    pub total_bytes: Option<u64>,
    pub completed_bytes: Option<u64>,
    pub done: bool,
}

#[derive(Debug, Serialize)]
struct PullRequest {
    model: String,
    stream: bool,
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
    families: Option<Vec<String>>,
    parameter_size: Option<String>,
    quantization_level: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ErrorResponse {
    error: String,
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

#[derive(Debug, Deserialize)]
struct PullLine {
    status: String,
    digest: Option<String>,
    total: Option<u64>,
    completed: Option<u64>,
}

impl OllamaClient {
    pub fn new(base_url: &str) -> Result<Self> {
        Self::with_timeout(base_url, Duration::from_secs(30))
    }

    pub fn with_timeout(base_url: &str, timeout: Duration) -> Result<Self> {
        let normalized_url = validate_base_url(base_url)?;
        let stream_timeout = Duration::from_secs(300);
        let client = reqwest::Client::builder()
            .connect_timeout(timeout)
            .read_timeout(stream_timeout)
            .build()?;
        let base_url = reqwest::Url::parse(&normalized_url)?;
        Ok(Self {
            client,
            base_url,
            request_timeout: timeout,
            stream_timeout,
        })
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

    pub async fn pull_model<F>(&self, model: &str, mut on_progress: F) -> Result<()>
    where
        F: FnMut(OllamaPullProgress) + Send,
    {
        let request = PullRequest {
            model: validate_model_name(model)?,
            stream: true,
        };
        let response = self
            .client
            .post(self.endpoint("pull")?)
            .json(&request)
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(response_error(response).await);
        }

        self.stream_lines(response, |line| {
            if let Some(progress) = parse_pull_stream_line(line)? {
                let done = progress.done;
                on_progress(progress);
                if done {
                    return Ok(true);
                }
            }
            Ok(false)
        })
        .await
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
            return Err(response_error(response).await);
        }

        self.stream_lines(response, |line| {
            if let Some(event) = parse_chat_stream_line(line)? {
                let done = matches!(event, ChatStreamEvent::Done);
                on_event(event);
                if done {
                    return Ok(true);
                }
            }
            Ok(false)
        })
        .await
    }

    async fn get_text(&self, endpoint: &str) -> Result<String> {
        let response = self
            .client
            .get(self.endpoint(endpoint)?)
            .timeout(self.request_timeout)
            .send()
            .await?;
        if !response.status().is_success() {
            return Err(response_error(response).await);
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

    async fn stream_lines<F>(&self, response: reqwest::Response, mut on_line: F) -> Result<()>
    where
        F: FnMut(&str) -> Result<bool>,
    {
        let mut stream = response.bytes_stream();
        let mut buffer = Vec::new();

        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|error| {
                if error.is_timeout() {
                    MooseError::StreamStalled {
                        seconds: self.stream_timeout.as_secs(),
                    }
                } else {
                    MooseError::Http(error)
                }
            })?;
            buffer.extend_from_slice(&chunk);

            while let Some(index) = buffer.iter().position(|byte| *byte == b'\n') {
                let line = buffer.drain(..=index).collect::<Vec<_>>();
                let line = std::str::from_utf8(&line[..line.len().saturating_sub(1)])?.trim();
                if on_line(line)? {
                    return Ok(());
                }
            }
        }

        let line = std::str::from_utf8(&buffer)?.trim();
        if !line.is_empty() {
            on_line(line)?;
        }

        Ok(())
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
            let family = details.as_ref().and_then(|details| details.family.clone());
            let families = details
                .as_ref()
                .and_then(|details| details.families.clone())
                .unwrap_or_else(|| family.iter().cloned().collect());
            let supports_chat = supports_chat(&family, &families);
            OllamaModel {
                name: model.name,
                digest: model.digest,
                size_bytes: model.size,
                family,
                families,
                parameter_size: details
                    .as_ref()
                    .and_then(|details| details.parameter_size.clone()),
                quantization_level: details.and_then(|details| details.quantization_level),
                modified_at: model.modified_at,
                supports_chat,
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

pub fn parse_pull_stream_line(line: &str) -> Result<Option<OllamaPullProgress>> {
    if line.is_empty() {
        return Ok(None);
    }

    let response: PullLine = serde_json::from_str(line)?;
    let status = response.status.trim().to_string();
    if status.is_empty() {
        return Err(MooseError::InvalidOllamaResponse(
            "missing pull status".to_string(),
        ));
    }

    Ok(Some(OllamaPullProgress {
        done: status == "success",
        status,
        digest: response.digest,
        total_bytes: response.total,
        completed_bytes: response.completed,
    }))
}

async fn response_error(response: reqwest::Response) -> MooseError {
    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    let message = parse_error_body(&body).unwrap_or_else(|| status.to_string());
    MooseError::HttpStatus { status, message }
}

fn parse_error_body(body: &str) -> Option<String> {
    serde_json::from_str::<ErrorResponse>(body)
        .ok()
        .map(|response| response.error)
        .or_else(|| {
            let trimmed = body.trim();
            (!trimmed.is_empty()).then(|| trimmed.to_string())
        })
}

fn supports_chat(family: &Option<String>, families: &[String]) -> bool {
    let has_unsupported_family = family
        .iter()
        .chain(families.iter())
        .map(|family| family.as_str())
        .any(|family| matches!(family, "bert" | "clip"));
    !has_unsupported_family
}

#[cfg(test)]
mod tests {
    use super::{
        OllamaClient, parse_chat_stream_line, parse_error_body, parse_models_response,
        parse_pull_stream_line, parse_version_response,
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
                            "families": ["llama"],
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
        assert!(models[0].supports_chat);
    }

    #[test]
    fn parse_models_marks_embedding_models_as_not_chat_capable() {
        let models = parse_models_response(
            r#"{
                "models": [
                    {
                        "name": "all-minilm:latest",
                        "size": 45960996,
                        "details": {
                            "family": "bert",
                            "families": ["bert"],
                            "parameter_size": "23M",
                            "quantization_level": "F16"
                        }
                    }
                ]
            }"#,
        )
        .unwrap();

        assert!(!models[0].supports_chat);
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

    #[test]
    fn parse_error_body_reads_ollama_error() {
        let message = parse_error_body(r#"{"error":"model does not support chat"}"#).unwrap();

        assert_eq!(message, "model does not support chat");
    }

    #[test]
    fn parse_pull_stream_reads_progress() {
        let progress = parse_pull_stream_line(
            r#"{"status":"downloading","digest":"sha256:abc","total":200,"completed":50}"#,
        )
        .unwrap()
        .unwrap();

        assert_eq!(progress.status, "downloading");
        assert_eq!(progress.digest.as_deref(), Some("sha256:abc"));
        assert_eq!(progress.total_bytes, Some(200));
        assert_eq!(progress.completed_bytes, Some(50));
        assert!(!progress.done);
    }

    #[test]
    fn parse_pull_stream_marks_success_done() {
        let progress = parse_pull_stream_line(r#"{"status":"success"}"#)
            .unwrap()
            .unwrap();

        assert_eq!(progress.status, "success");
        assert!(progress.done);
    }

    #[test]
    fn parse_pull_stream_rejects_missing_status() {
        assert!(parse_pull_stream_line(r#"{"completed":50}"#).is_err());
    }
}
