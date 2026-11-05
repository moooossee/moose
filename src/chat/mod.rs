use serde::{Deserialize, Serialize};

use crate::conversations::{Message, MessageRole, MessageStatus};
use crate::providers::validate_model_id;

use crate::error::{MooseError, Result};
use crate::tools::{ToolCall, ToolDefinition};

pub mod context;
pub mod runner;

pub enum ChatSubmission {
    New(String),
    Edit { id: String, content: String },
    Regenerate(String),
}

impl ChatSubmission {
    pub fn prepare(&self, history: &[Message]) -> Result<(String, Vec<Message>)> {
        let (content, end) = match self {
            Self::New(content) => (content.as_str(), history.len()),
            Self::Edit { id, content } => {
                let index = history
                    .iter()
                    .position(|message| message.id == *id)
                    .ok_or(MooseError::MessageNotFound)?;
                if history[index].role != MessageRole::User {
                    return Err(MooseError::InvalidMessageRole);
                }
                (content.as_str(), index)
            }
            Self::Regenerate(id) => {
                let index = history
                    .iter()
                    .position(|message| message.id == *id)
                    .ok_or(MooseError::MessageNotFound)?;
                if history[index].role != MessageRole::Assistant
                    || !history[index].status.is_finished()
                {
                    return Err(MooseError::InvalidMessageRole);
                }
                let user_index = index.checked_sub(1).ok_or(MooseError::MessageNotFound)?;
                if history[user_index].role != MessageRole::User {
                    return Err(MooseError::InvalidMessageRole);
                }
                (history[user_index].content.as_str(), user_index)
            }
        };
        let content = crate::conversations::validate_message_content(
            content.trim(),
            &MessageStatus::Complete,
        )?;
        Ok((content, history[..end].to_vec()))
    }
}

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
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub thinking: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<ToolCall>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub images: Vec<String>,
    pub role: ChatRole,
    pub content: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ThinkingValue {
    Enabled(bool),
    Level(String),
}

impl ThinkingValue {
    pub fn label(&self) -> String {
        match self {
            Self::Enabled(true) => "On".into(),
            Self::Enabled(false) => "Off".into(),
            Self::Level(level) => {
                let mut chars = level.chars();
                chars
                    .next()
                    .map(|first| first.to_uppercase().collect::<String>() + chars.as_str())
                    .unwrap_or_default()
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ChatRequest {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<ToolDefinition>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub think: Option<ThinkingValue>,
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
    ToolCall(ToolCall),
    Thinking(String),
    Token(String),
    ResponseLimit,
    Done,
}

impl ChatMessage {
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            thinking: String::new(),
            tool_calls: Vec::new(),
            tool_name: None,
            images: Vec::new(),
            role: ChatRole::System,
            content: content.into(),
        }
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self {
            thinking: String::new(),
            tool_calls: Vec::new(),
            tool_name: None,
            images: Vec::new(),
            role: ChatRole::User,
            content: content.into(),
        }
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            thinking: String::new(),
            tool_calls: Vec::new(),
            tool_name: None,
            images: Vec::new(),
            role: ChatRole::Assistant,
            content: content.into(),
        }
    }

    pub fn tool(name: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: ChatRole::Tool,
            tool_name: Some(name.into()),
            ..Self::assistant(content)
        }
    }
}

impl ChatRequest {
    pub fn streaming(model: impl AsRef<str>, messages: Vec<ChatMessage>) -> Result<Self> {
        Ok(Self {
            tools: Vec::new(),
            think: None,
            model: validate_model_id(model.as_ref())?,
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
    let mut messages = Vec::new();
    if let Some(system_prompt) = system_prompt
        .map(str::trim)
        .filter(|system_prompt| !system_prompt.is_empty())
    {
        messages.push(ChatMessage::system(system_prompt));
    }
    for [user, assistant] in context::recent_exchanges(history, context_limit) {
        messages.push(ChatMessage::user(&user.content));
        messages.push(ChatMessage::assistant(&assistant.content));
    }
    messages.push(ChatMessage::user(current_prompt));
    messages
}

#[cfg(test)]
mod tests {
    use super::{ChatMessage, ChatRequest};

    #[test]
    fn chat_request_validates_model_id() {
        for model in ["llama3.2:latest", "llama 3", "Qwen3.8 Flash Next"] {
            let request = ChatRequest::streaming(model, vec![ChatMessage::user("Hello")]).unwrap();
            assert_eq!(request.model, model);
        }

        for model in ["", "   ", "llama\n3", "llama\t3", "llama3\0"] {
            assert!(
                ChatRequest::streaming(model, vec![ChatMessage::user("Hello")]).is_err(),
                "{model:?}"
            );
        }
    }
}
