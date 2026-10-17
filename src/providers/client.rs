use super::{
    CustomAuthentication, Provider, ProviderKind, credentials, is_loopback_url,
    policy::NetworkPolicy, stream::Decoder, validate_model_name, wire,
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

pub struct ModelCatalog {
    pub models: Vec<OllamaModel>,
    pub verified: bool,
    pub warning: Option<String>,
}

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
        provider.validate_configuration()?;
        let transport = if provider.kind == ProviderKind::Ollama {
            Transport::Ollama(OllamaClient::new(&provider.base_url)?)
        } else {
            let key = if provider.requires_key() {
                Some(credentials::load(&provider).await?)
            } else {
                None
            };
            policy.check(&provider)?;
            Transport::Cloud(http_client(&provider, key.as_ref())?)
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
        Ok(self.model_catalog().await?.models)
    }

    pub async fn model_catalog(&self) -> Result<ModelCatalog> {
        self.check()?;
        let mut manual = self
            .provider
            .custom_config
            .as_ref()
            .map_or_else(Vec::new, |config| {
                config
                    .models
                    .iter()
                    .map(|model| cloud_model(&model.id, &self.provider.name))
                    .collect()
            });
        if self
            .provider
            .custom_config
            .as_ref()
            .is_some_and(|config| !config.discover_models)
        {
            return Ok(ModelCatalog {
                models: manual,
                verified: false,
                warning: None,
            });
        }
        match self.discover_models().await {
            Ok(mut models) => {
                models.append(&mut manual);
                models.sort_by(|a, b| a.name.cmp(&b.name));
                models.dedup_by(|a, b| a.name == b.name);
                Ok(ModelCatalog {
                    models,
                    verified: true,
                    warning: None,
                })
            }
            Err(error) => {
                self.check()?;
                if manual.is_empty() {
                    return Err(error);
                }
                Ok(ModelCatalog {
                    models: manual,
                    verified: false,
                    warning: Some(format!(
                        "Model discovery failed: {error}. Using your saved model IDs."
                    )),
                })
            }
        }
    }

    async fn discover_models(&self) -> Result<Vec<OllamaModel>> {
        self.check()?;
        let Transport::Cloud(client) = &self.transport else {
            if let Transport::Ollama(client) = &self.transport {
                return client.list_models().await;
            }
            unreachable!();
        };
        let kind = self.provider.api_kind();
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
            models.extend(parse_models(
                kind,
                &data,
                self.provider.kind == ProviderKind::Custom,
            )?);
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
        let kind = self.provider.api_kind();
        if matches!(
            self.provider.kind,
            ProviderKind::Mistral | ProviderKind::DeepSeek | ProviderKind::Custom
        ) && request
            .messages
            .iter()
            .any(|message| !message.images.is_empty())
            && !self.supports_vision(&request.model).await?
        {
            return Err(MooseError::ProviderRequest(
                "This model does not support images. Select a model with image support.".into(),
            ));
        }
        let url = match kind {
            ProviderKind::OllamaCloud => self.endpoint("chat")?,
            ProviderKind::OpenAi => self.endpoint("responses")?,
            ProviderKind::Anthropic => self.endpoint("messages")?,
            ProviderKind::Groq
            | ProviderKind::Mistral
            | ProviderKind::DeepSeek
            | ProviderKind::Custom => self.endpoint("chat/completions")?,
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

    async fn mistral_model(&self, model: &str) -> Result<Value> {
        self.check()?;
        let model = validate_model_name(model)?;
        let Transport::Cloud(client) = &self.transport else {
            unreachable!()
        };
        let mut url = self.endpoint("models")?;
        url.path_segments_mut()
            .map_err(|_| MooseError::InvalidProviderUrl)?
            .push(&model);
        let response = client
            .get(url)
            .timeout(Duration::from_secs(30))
            .send()
            .await
            .map_err(network_error)?;
        let data = response_json(response).await?;
        if !data["capabilities"].is_object() {
            return Err(MooseError::ProviderResponse(
                "The provider returned invalid model capabilities.".into(),
            ));
        }
        Ok(data)
    }

    pub async fn supports_vision(&self, model: &str) -> Result<bool> {
        self.check()?;
        if let Transport::Ollama(client) = &self.transport {
            return client.supports_vision(model).await;
        }
        if let Some(config) = &self.provider.custom_config {
            if let Some(model) = config.models.iter().find(|entry| entry.id == model) {
                return Ok(model.vision);
            }
            if !config.discover_models {
                return Ok(false);
            }
            let Transport::Cloud(client) = &self.transport else {
                unreachable!()
            };
            let response = client
                .get(self.endpoint("models")?)
                .timeout(Duration::from_secs(30))
                .send()
                .await
                .map_err(network_error)?;
            let data = response_json(response).await?;
            return Ok(custom_model_vision(&data, model));
        }
        if self.provider.kind == ProviderKind::OllamaCloud {
            let data = self.ollama_capabilities(model).await?;
            return Ok(data["capabilities"]
                .as_array()
                .is_some_and(|a| a.iter().any(|c| c == "vision")));
        }
        if self.provider.kind == ProviderKind::Mistral {
            let data = self.mistral_model(model).await?;
            return Ok(data["capabilities"]["vision"].as_bool() == Some(true));
        }
        Ok(wire::supports_images(self.provider.kind, model))
    }

    pub async fn thinking_values(&self, model: &str) -> Result<Vec<ThinkingValue>> {
        self.check()?;
        if self.provider.kind == ProviderKind::Custom {
            return Ok(Vec::new());
        }
        if let Transport::Ollama(client) = &self.transport {
            return client.thinking_values(model).await;
        }
        if self.provider.kind == ProviderKind::OllamaCloud {
            let data = self.ollama_capabilities(model).await?;
            return Ok(
                serde_json::from_value(data["thinking"]["values"].clone()).unwrap_or_default()
            );
        }
        if self.provider.kind == ProviderKind::Mistral {
            let data = self.mistral_model(model).await?;
            for name in [data["id"].as_str(), data["root"].as_str(), Some(model)]
                .into_iter()
                .flatten()
            {
                let values = wire::thinking_values(self.provider.kind, name);
                if !values.is_empty() {
                    return Ok(values);
                }
            }
            return Ok(Vec::new());
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

fn http_client(provider: &Provider, key: Option<&credentials::ApiKey>) -> Result<Client> {
    let mut headers = HeaderMap::new();
    if provider.api_kind() == ProviderKind::Anthropic {
        headers.insert(
            HeaderName::from_static("anthropic-version"),
            reqwest::header::HeaderValue::from_static("2023-06-01"),
        );
    }
    if let Some(key) = key {
        let authentication = provider
            .custom_config
            .as_ref()
            .map(|config| config.authentication.clone())
            .unwrap_or_else(|| match provider.kind {
                ProviderKind::Anthropic => CustomAuthentication::Header("x-api-key".into()),
                ProviderKind::Gemini => CustomAuthentication::Header("x-goog-api-key".into()),
                _ => CustomAuthentication::Bearer,
            });
        match authentication {
            CustomAuthentication::Bearer => {
                headers.insert(AUTHORIZATION, key.header(true)?);
            }
            CustomAuthentication::Header(name) => {
                let name = HeaderName::from_bytes(name.as_bytes()).map_err(|_| {
                    MooseError::ProviderRequest("Invalid API key header name.".into())
                })?;
                headers.insert(name, key.header(false)?);
            }
            CustomAuthentication::None => {}
        }
    } else if provider.requires_key() {
        return Err(MooseError::MissingApiKey);
    }
    let loopback = is_loopback_url(&reqwest::Url::parse(&provider.base_url)?);
    let mut builder = Client::builder()
        .default_headers(headers)
        .redirect(reqwest::redirect::Policy::none())
        .referer(false)
        .connect_timeout(Duration::from_secs(30))
        .read_timeout(STREAM_TIMEOUT)
        .https_only(!loopback);
    if loopback {
        builder = builder.no_proxy();
    }
    builder.build().map_err(network_error)
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

fn parse_models(kind: ProviderKind, data: &Value, custom: bool) -> Result<Vec<OllamaModel>> {
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
        if !custom && kind == ProviderKind::OpenAi && !openai_responses_model(name) {
            continue;
        }
        if kind == ProviderKind::Mistral
            && (item["capabilities"]["completion_chat"].as_bool() != Some(true)
                || item["archived"].as_bool() == Some(true))
        {
            continue;
        }
        if !custom
            && kind != ProviderKind::Mistral
            && [
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
        models.push(cloud_model(name, kind.label()));
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

fn cloud_model(name: &str, family: &str) -> OllamaModel {
    OllamaModel {
        name: name.into(),
        digest: None,
        size_bytes: None,
        family: Some(family.into()),
        families: Vec::new(),
        parameter_size: None,
        quantization_level: None,
        modified_at: None,
        supports_chat: true,
    }
}

fn custom_model_vision(data: &Value, model: &str) -> bool {
    data["data"]
        .as_array()
        .or_else(|| data["models"].as_array())
        .and_then(|models| {
            models.iter().find(|entry| {
                entry["id"]
                    .as_str()
                    .or_else(|| entry["name"].as_str())
                    .is_some_and(|id| {
                        id.strip_prefix("models/").unwrap_or(id)
                            == model.strip_prefix("models/").unwrap_or(model)
                    })
            })
        })
        .is_some_and(|entry| {
            entry["capabilities"]["vision"].as_bool() == Some(true)
                || entry["architecture"]["input_modalities"]
                    .as_array()
                    .or_else(|| entry["input_modalities"].as_array())
                    .is_some_and(|modalities| modalities.iter().any(|modality| modality == "image"))
        })
}
