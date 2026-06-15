use serde::{Deserialize, Serialize};

use crate::conversations::{Message, MessageRole, MessageStatus};
use crate::providers::validate_model_name;

use crate::error::Result;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChatRole {
    System,
    User,
    Assistant,
    Tool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: ChatRole,
    pub content: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    pub stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub options: Option<ChatOptions>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct ChatOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_k: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seed: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub num_ctx: Option<i64>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ChatStreamEvent {
    Token(String),
    Done,
}

impl ChatMessage {
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: ChatRole::System,
            content: content.into(),
        }
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: ChatRole::User,
            content: content.into(),
        }
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: ChatRole::Assistant,
            content: content.into(),
        }
    }
}

impl ChatRequest {
    pub fn streaming(model: impl AsRef<str>, messages: Vec<ChatMessage>) -> Result<Self> {
        Ok(Self {
            model: validate_model_name(model.as_ref())?,
            messages,
            stream: true,
            format: None,
            options: None,
        })
    }

    pub fn streaming_with_temperature(
        model: impl AsRef<str>,
        messages: Vec<ChatMessage>,
        temperature: f64,
    ) -> Result<Self> {
        Self::streaming_with_options(
            model,
            messages,
            ChatOptions {
                temperature: Some(temperature),
                top_p: None,
                top_k: None,
                seed: None,
                num_ctx: None,
            },
        )
    }

    pub fn streaming_with_options(
        model: impl AsRef<str>,
        messages: Vec<ChatMessage>,
        options: ChatOptions,
    ) -> Result<Self> {
        let mut request = Self::streaming(model, messages)?;
        if !options.is_empty() {
            request.options = Some(options);
        }
        Ok(request)
    }
}

impl ChatOptions {
    pub const fn is_empty(&self) -> bool {
        self.temperature.is_none()
            && self.top_p.is_none()
            && self.top_k.is_none()
            && self.seed.is_none()
            && self.num_ctx.is_none()
    }
}

pub fn build_conversation_context(
    history: &[Message],
    current_prompt: &str,
    context_limit: usize,
    system_prompt: Option<&str>,
) -> Vec<ChatMessage> {
    let context_limit = context_limit.max(1);
    let historical_limit = context_limit.saturating_sub(1);
    let mut historical_messages = history
        .iter()
        .filter_map(chat_message_from_stored_message)
        .collect::<Vec<_>>();

    if historical_messages.len() > historical_limit {
        historical_messages =
            historical_messages.split_off(historical_messages.len() - historical_limit);
    }

    let mut messages = Vec::new();
    if let Some(system_prompt) = system_prompt
        .map(str::trim)
        .filter(|system_prompt| !system_prompt.is_empty())
    {
        messages.push(ChatMessage::system(system_prompt));
    }
    messages.extend(historical_messages);
    messages.push(ChatMessage::user(current_prompt));
    messages
}

fn chat_message_from_stored_message(message: &Message) -> Option<ChatMessage> {
    if message.content.trim().is_empty() || matches!(message.status, MessageStatus::Streaming) {
        return None;
    }

    match message.role {
        MessageRole::User => Some(ChatMessage::user(message.content.clone())),
        MessageRole::Assistant => Some(ChatMessage::assistant(message.content.clone())),
        MessageRole::System | MessageRole::Tool => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{ChatMessage, ChatRequest};

    #[test]
    fn chat_request_validates_model_name() {
        assert!(
            ChatRequest::streaming("llama3.2:latest", vec![ChatMessage::user("Hello")]).is_ok()
        );
        assert!(ChatRequest::streaming("llama 3", vec![ChatMessage::user("Hello")]).is_err());
    }
}
