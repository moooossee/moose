use serde::{Deserialize, Serialize};

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

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    pub stream: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ChatStreamEvent {
    Token(String),
    Done,
}

impl ChatMessage {
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
        })
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
