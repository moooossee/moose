use super::{
    Provider, ProviderKind, credentials, policy::NetworkPolicy, stream::Decoder,
    validate_model_name, wire,
};
use crate::{
    chat::{ChatRequest, ChatStreamEvent, ThinkingValue},
    error::{MooseError, Result},
    ollama::{OllamaClient, OllamaHealth, OllamaModel, OllamaPullProgress},
};
use futures_util::StreamExt;
use reqwest::{
    Client, Response,
    header::{AUTHORIZATION, HeaderMap, HeaderName},
};
use serde_json::{Value, json};
use std::time::Duration;

const MAX_RESPONSE_BYTES: usize = 8 * 1024 * 1024;
const STREAM_TIMEOUT: Duration = Duration::from_secs(300);

pub struct ProviderClient {
    provider: Provider,
    policy: NetworkPolicy,
    transport: Transport,
}
enum Transport {
    Ollama(OllamaClient),
    Cloud(Client),
}

impl ProviderClient {
    pub async fn new(provider: Provider, policy: NetworkPolicy) -> Result<Self> {
        policy.check(&provider)?;
        let transport = if provider.kind == ProviderKind::Ollama {
            Transport::Ollama(OllamaClient::new(&provider.base_url)?)
        } else {
            let key = credentials::load(&provider).await?;
            policy.check(&provider)?;
            Transport::Cloud(http_client(provider.kind, &key)?)
        };
        Ok(Self {
            provider,
            policy,
            transport,
        })
    }

    fn check(&self) -> Result<()> {
        self.policy.check(&self.provider)
    }

    pub async fn health(&self) -> OllamaHealth {
        if let Err(error) = self.check() {
            return OllamaHealth {
                available: false,
                version: None,
                message: error.to_string(),
            };
        }
        match &self.transport {
            Transport::Ollama(client) => client.health().await,
            Transport::Cloud(_) => OllamaHealth {
                available: true,
                version: None,
                message: self.provider.kind.label().into(),
            },
        }
    }

    pub async fn list_models(&self) -> Result<Vec<OllamaModel>> {
        self.check()?;
        let Transport::Cloud(client) = &self.transport else {
            if let Transport::Ollama(client) = &self.transport {
                return client.list_models().await;
            }
            unreachable!();
        };
        let kind = self.provider.kind;
        let path = if kind == ProviderKind::OllamaCloud {
            "tags"
        } else {
            "models"
        };
        let mut url = self.endpoint(path)?;
        if kind == ProviderKind::Anthropic {
            url.query_pairs_mut().append_pair("limit", "1000");
        }
        if kind == ProviderKind::Gemini {
            url.query_pairs_mut().append_pair("pageSize", "1000");
        }
        let mut models = Vec::new();
        for _ in 0..100 {
            self.check()?;
            let response = client
                .get(url.clone())
                .timeout(Duration::from_secs(30))
                .send()
                .await
                .map_err(network_error)?;
            let data = response_json(response).await?;
            models.extend(parse_models(kind, &data)?);
            let next =
                if kind == ProviderKind::Anthropic && data["has_more"].as_bool() == Some(true) {
                    data["last_id"].as_str().map(|s| ("after_id", s))
                } else if kind == ProviderKind::Gemini {
                    data["nextPageToken"]
                        .as_str()
                        .filter(|s| !s.is_empty())
                        .map(|s| ("pageToken", s))
                } else {
                    None
                };
            if let Some((key, token)) = next {
                url = self.endpoint(path)?;
                url.query_pairs_mut().append_pair(key, token);
            } else {
                models.sort_by(|a, b| a.name.cmp(&b.name));
                models.dedup_by(|a, b| a.name == b.name);
                return Ok(models);
            }
        }
        Err(MooseError::ProviderResponse(
            "The model list exceeded its pagination limit.".into(),
        ))
    }

    pub async fn stream_chat<F>(&self, request: ChatRequest, mut on_event: F) -> Result<()>
    where
        F: FnMut(ChatStreamEvent) + Send,
    {
        self.check()?;
        validate_model_name(&request.model)?;
        if let Transport::Ollama(client) = &self.transport {
            return client.stream_chat(request, on_event).await;
        }
        let Transport::Cloud(client) = &self.transport else {
            unreachable!()
        };
        let kind = self.provider.kind;
        let url = match kind {
            ProviderKind::OllamaCloud => self.endpoint("chat")?,
            ProviderKind::OpenAi => self.endpoint("responses")?,
            ProviderKind::Anthropic => self.endpoint("messages")?,
            ProviderKind::Groq => self.endpoint("chat/completions")?,
            ProviderKind::Gemini => {
                let model = request
                    .model
                    .strip_prefix("models/")
                    .unwrap_or(&request.model);
                if !model
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
                {
                    return Err(MooseError::InvalidModelName);
                }
                let mut url = self.endpoint(&format!("models/{model}:streamGenerateContent"))?;
                url.query_pairs_mut().append_pair("alt", "sse");
                url
            }
            ProviderKind::Ollama => unreachable!(),
        };
        let body = wire::request(kind, &request)?;
        self.check()?;
        let response = client
            .post(url)
            .json(&body)
            .send()
            .await
            .map_err(network_error)?;
        check_status(&response)?;
        let mut stream = response.bytes_stream();
        let mut decoder = Decoder::new(kind);
        let mut received_text = false;
        loop {
            self.check()?;
            let chunk = tokio::time::timeout(STREAM_TIMEOUT, stream.next())
                .await
                .map_err(|_| MooseError::StreamStalled { seconds: 300 })?;
            self.check()?;
            let (events, eof) = match chunk {
                Some(chunk) => (decoder.push(&chunk.map_err(network_error)?)?, false),
                None => (decoder.finish()?, true),
            };
            for event in events {
                if matches!(event, ChatStreamEvent::Token(_)) {
                    received_text = true;
                }
                if matches!(event, ChatStreamEvent::Done) {
                    if !received_text {
                        return Err(MooseError::ProviderResponse(
                            "The model returned no text. Select a model that supports chat.".into(),
                        ));
                    }
                    on_event(event);
                    return Ok(());
                }
                on_event(event);
            }
            if eof {
                return Err(MooseError::ProviderResponse(
                    "The connection ended before the answer was complete.".into(),
                ));
            }
        }
    }

    async fn ollama_capabilities(&self, model: &str) -> Result<Value> {
        self.check()?;
        let Transport::Cloud(client) = &self.transport else {
            unreachable!()
        };
        let response = client
            .post(self.endpoint("show")?)
            .timeout(Duration::from_secs(30))
            .json(&json!({"model":validate_model_name(model)?}))
            .send()
            .await
            .map_err(network_error)?;
        response_json(response).await
    }

    pub async fn supports_vision(&self, model: &str) -> Result<bool> {
        self.check()?;
        if let Transport::Ollama(client) = &self.transport {
            return client.supports_vision(model).await;
        }
        if self.provider.kind == ProviderKind::OllamaCloud {
            let data = self.ollama_capabilities(model).await?;
            return Ok(data["capabilities"]
                .as_array()
                .is_some_and(|a| a.iter().any(|c| c == "vision")));
        }
        Ok(wire::supports_images(self.provider.kind, model))
    }

    pub async fn thinking_values(&self, model: &str) -> Result<Vec<ThinkingValue>> {
        self.check()?;
        if let Transport::Ollama(client) = &self.transport {
            return client.thinking_values(model).await;
        }
        if self.provider.kind == ProviderKind::OllamaCloud {
            let data = self.ollama_capabilities(model).await?;
            return Ok(
                serde_json::from_value(data["thinking"]["values"].clone()).unwrap_or_default()
            );
        }
        Ok(wire::thinking_values(self.provider.kind, model))
    }

    pub async fn pull_model<F>(&self, model: &str, on_progress: F) -> Result<()>
    where
        F: FnMut(OllamaPullProgress) + Send,
    {
        self.check()?;
        if let Transport::Ollama(client) = &self.transport {
            return client.pull_model(model, on_progress).await;
        }
        Err(MooseError::ProviderRequest(
            "Cloud models do not need to be downloaded.".into(),
        ))
    }
    pub async fn delete_model(&self, model: &str) -> Result<()> {
        self.check()?;
        if let Transport::Ollama(client) = &self.transport {
            return client.delete_model(model).await;
        }
        Err(MooseError::ProviderRequest(
            "Cloud models cannot be deleted from Moose.".into(),
        ))
    }
    fn endpoint(&self, path: &str) -> Result<reqwest::Url> {
        self.provider.kind.validate_url(&self.provider.base_url)?;
        Ok(reqwest::Url::parse(&format!(
            "{}/{path}",
            self.provider.base_url
        ))?)
    }
}

fn http_client(kind: ProviderKind, key: &credentials::ApiKey) -> Result<Client> {
    let mut headers = HeaderMap::new();
    match kind {
        ProviderKind::Anthropic => {
            headers.insert(HeaderName::from_static("x-api-key"), key.header(false)?);
            headers.insert(
                HeaderName::from_static("anthropic-version"),
                reqwest::header::HeaderValue::from_static("2023-06-01"),
            );
        }
        ProviderKind::Gemini => {
            headers.insert(
                HeaderName::from_static("x-goog-api-key"),
                key.header(false)?,
            );
        }
        _ => {
            headers.insert(AUTHORIZATION, key.header(true)?);
        }
    }
    Client::builder()
        .default_headers(headers)
        .redirect(reqwest::redirect::Policy::none())
        .referer(false)
        .connect_timeout(Duration::from_secs(30))
        .read_timeout(STREAM_TIMEOUT)
        .https_only(true)
        .build()
        .map_err(network_error)
}

fn network_error(_: reqwest::Error) -> MooseError {
    MooseError::ProviderRequest(
        "The secure connection failed. Check your network and try again.".into(),
    )
}
fn check_status(response: &Response) -> Result<()> {
    let status = response.status();
    if status.is_success() {
        return Ok(());
    }
    let message = match status.as_u16() {
        401 | 403 => "Authentication failed. Check your API key and model access in Preferences.",
        429 => {
            "The provider's rate or spending limit was reached. Wait before retrying or check your account."
        }
        400 | 422 => {
            "The provider rejected the model or request options. Check the selected model and attachments."
        }
        404 => "This model or API endpoint is unavailable. Refresh the model list.",
        300..=399 => {
            "The provider tried to redirect the request. Redirects are blocked to protect your API key."
        }
        500..=599 => "The provider is temporarily unavailable. Try again later.",
        _ => "The provider could not process this request.",
    };
    Err(MooseError::HttpStatus {
        status,
        message: message.into(),
    })
}
async fn response_json(response: Response) -> Result<Value> {
    check_status(&response)?;
    let mut stream = response.bytes_stream();
    let mut bytes = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(network_error)?;
        if bytes.len().saturating_add(chunk.len()) > MAX_RESPONSE_BYTES {
            return Err(MooseError::ProviderResponse(
                "The provider response is too large.".into(),
            ));
        }
        bytes.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&bytes)
        .map_err(|_| MooseError::ProviderResponse("The provider returned invalid JSON.".into()))
}

fn parse_models(kind: ProviderKind, data: &Value) -> Result<Vec<OllamaModel>> {
    let list = data[if matches!(kind, ProviderKind::OllamaCloud | ProviderKind::Gemini) {
        "models"
    } else {
        "data"
    }]
    .as_array()
    .ok_or_else(|| {
        MooseError::ProviderResponse("The provider returned an invalid model list.".into())
    })?;
    let mut models = Vec::new();
    for item in list {
        if kind == ProviderKind::Gemini
            && !item["supportedGenerationMethods"]
                .as_array()
                .is_some_and(|a| a.iter().any(|m| m == "generateContent"))
        {
            continue;
        }
        let Some(name) = item[if matches!(kind, ProviderKind::OllamaCloud | ProviderKind::Gemini) {
            "name"
        } else {
            "id"
        }]
        .as_str() else {
            continue;
        };
        let name = if kind == ProviderKind::Gemini {
            name.strip_prefix("models/").unwrap_or(name)
        } else {
            name
        };
        if validate_model_name(name).is_err() {
            continue;
        }
        if kind == ProviderKind::OpenAi && !openai_responses_model(name) {
            continue;
        }
        if [
            "embedding",
            "whisper",
            "tts",
            "transcribe",
            "realtime",
            "audio",
            "image",
            "moderation",
            "safeguard",
            "search",
            "computer-use",
            "compound",
        ]
        .iter()
        .any(|part| name.contains(part))
        {
            continue;
        }
        if item["active"].as_bool() == Some(false) {
            continue;
        }
        models.push(OllamaModel {
            name: name.into(),
            digest: None,
            size_bytes: None,
            family: Some(kind.label().into()),
            families: Vec::new(),
            parameter_size: None,
            quantization_level: None,
            modified_at: None,
            supports_chat: true,
        });
    }
    Ok(models)
}

fn openai_responses_model(name: &str) -> bool {
    let name = name.strip_prefix("ft:").unwrap_or(name);
    ["gpt-4o", "gpt-4.1", "gpt-5", "gpt-6", "o1", "o3", "o4"]
        .iter()
        .any(|prefix| name.starts_with(prefix))
        && !name.contains("chat-latest")
        && !name.starts_with("o1-mini")
        && !name.starts_with("o1-preview")
}
