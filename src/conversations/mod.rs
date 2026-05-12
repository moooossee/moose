use std::{fmt, str::FromStr};

use serde::{Deserialize, Serialize};

use crate::{
    core::{new_id, utc_now},
    error::{MooseError, Result},
    providers::validate_model_name,
};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Conversation {
    pub id: String,
    pub provider_id: String,
    pub model_id: Option<String>,
    pub title: String,
    pub created_at: String,
    pub updated_at: String,
    pub archived_at: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NewConversation {
    pub provider_id: String,
    pub model_id: Option<String>,
    pub title: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConversationTitleUpdate {
    pub id: String,
    pub title: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MessageRole {
    System,
    User,
    Assistant,
    Tool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MessageStatus {
    Streaming,
    Complete,
    Cancelled,
    Failed,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Message {
    pub id: String,
    pub conversation_id: String,
    pub role: MessageRole,
    pub content: String,
    pub status: MessageStatus,
    pub token_count: Option<i64>,
    pub created_at: String,
    pub completed_at: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NewMessage {
    pub conversation_id: String,
    pub role: MessageRole,
    pub content: String,
    pub status: MessageStatus,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MessageUpdate {
    pub id: String,
    pub content: String,
    pub status: MessageStatus,
    pub token_count: Option<i64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GenerationSettings {
    pub id: String,
    pub conversation_id: Option<String>,
    pub model: Option<String>,
    pub temperature: Option<f64>,
    pub top_p: Option<f64>,
    pub top_k: Option<i64>,
    pub seed: Option<i64>,
    pub num_ctx: Option<i64>,
    pub system_prompt: Option<String>,
    pub created_at: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct NewGenerationSettings {
    pub conversation_id: Option<String>,
    pub model: Option<String>,
    pub temperature: Option<f64>,
    pub top_p: Option<f64>,
    pub top_k: Option<i64>,
    pub seed: Option<i64>,
    pub num_ctx: Option<i64>,
    pub system_prompt: Option<String>,
}

impl NewConversation {
    pub fn into_conversation(self) -> Result<Conversation> {
        let timestamp = utc_now();
        Ok(Conversation {
            id: new_id(),
            provider_id: validate_required_id(&self.provider_id)?,
            model_id: self
                .model_id
                .map(|id| validate_required_id(&id))
                .transpose()?,
            title: validate_conversation_title(&self.title)?,
            created_at: timestamp.clone(),
            updated_at: timestamp,
            archived_at: None,
        })
    }
}

impl MessageRole {
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::System => "system",
            Self::User => "user",
            Self::Assistant => "assistant",
            Self::Tool => "tool",
        }
    }
}

impl fmt::Display for MessageRole {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for MessageRole {
    type Err = MooseError;

    fn from_str(value: &str) -> std::result::Result<Self, Self::Err> {
        match value {
            "system" => Ok(Self::System),
            "user" => Ok(Self::User),
            "assistant" => Ok(Self::Assistant),
            "tool" => Ok(Self::Tool),
            _ => Err(MooseError::InvalidMessageRole),
        }
    }
}

impl MessageStatus {
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Streaming => "streaming",
            Self::Complete => "complete",
            Self::Cancelled => "cancelled",
            Self::Failed => "failed",
        }
    }

    pub const fn is_finished(&self) -> bool {
        matches!(self, Self::Complete | Self::Cancelled | Self::Failed)
    }
}

impl fmt::Display for MessageStatus {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for MessageStatus {
    type Err = MooseError;

    fn from_str(value: &str) -> std::result::Result<Self, Self::Err> {
        match value {
            "streaming" => Ok(Self::Streaming),
            "complete" => Ok(Self::Complete),
            "cancelled" => Ok(Self::Cancelled),
            "failed" => Ok(Self::Failed),
            _ => Err(MooseError::InvalidMessageStatus),
        }
    }
}

impl NewMessage {
    pub fn user(conversation_id: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            conversation_id: conversation_id.into(),
            role: MessageRole::User,
            content: content.into(),
            status: MessageStatus::Complete,
        }
    }

    pub fn assistant_streaming(conversation_id: impl Into<String>) -> Self {
        Self {
            conversation_id: conversation_id.into(),
            role: MessageRole::Assistant,
            content: String::new(),
            status: MessageStatus::Streaming,
        }
    }

    pub fn into_message(self) -> Result<Message> {
        let timestamp = utc_now();
        let completed_at = self.status.is_finished().then(|| timestamp.clone());

        Ok(Message {
            id: new_id(),
            conversation_id: validate_required_id(&self.conversation_id)?,
            role: self.role,
            content: validate_message_content(&self.content, &self.status)?,
            status: self.status,
            token_count: None,
            created_at: timestamp,
            completed_at,
        })
    }
}

impl MessageUpdate {
    pub fn completed(id: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            content: content.into(),
            status: MessageStatus::Complete,
            token_count: None,
        }
    }

    pub fn cancelled(id: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            content: content.into(),
            status: MessageStatus::Cancelled,
            token_count: None,
        }
    }

    pub fn failed(id: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            content: content.into(),
            status: MessageStatus::Failed,
            token_count: None,
        }
    }
}

impl NewGenerationSettings {
    pub fn into_generation_settings(self) -> Result<GenerationSettings> {
        let model = self
            .model
            .map(|model| validate_model_name(&model))
            .transpose()?;

        Ok(GenerationSettings {
            id: new_id(),
            conversation_id: self
                .conversation_id
                .map(|id| validate_required_id(&id))
                .transpose()?,
            model,
            temperature: validate_probability_like(self.temperature)?,
            top_p: validate_probability_like(self.top_p)?,
            top_k: validate_non_negative(self.top_k)?,
            seed: self.seed,
            num_ctx: validate_non_negative(self.num_ctx)?,
            system_prompt: self.system_prompt,
            created_at: utc_now(),
        })
    }
}

pub fn validate_conversation_title(value: &str) -> Result<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed.len() > 160 || trimmed.chars().any(invalid_text_character) {
        return Err(MooseError::InvalidConversationTitle);
    }
    Ok(trimmed.to_string())
}

pub fn validate_message_content(value: &str, status: &MessageStatus) -> Result<String> {
    if value.chars().any(invalid_text_character) {
        return Err(MooseError::InvalidMessageContent);
    }

    if value.trim().is_empty() && !matches!(status, MessageStatus::Streaming) {
        return Err(MooseError::InvalidMessageContent);
    }

    Ok(value.to_string())
}

fn validate_required_id(value: &str) -> Result<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed.chars().any(char::is_control) {
        return Err(MooseError::InvalidIdentifier);
    }
    Ok(trimmed.to_string())
}

fn validate_probability_like(value: Option<f64>) -> Result<Option<f64>> {
    if value.is_some_and(|value| !value.is_finite() || value < 0.0) {
        return Err(MooseError::InvalidGenerationSettings);
    }
    Ok(value)
}

fn validate_non_negative(value: Option<i64>) -> Result<Option<i64>> {
    if value.is_some_and(|value| value < 0) {
        return Err(MooseError::InvalidGenerationSettings);
    }
    Ok(value)
}

fn invalid_text_character(character: char) -> bool {
    character.is_control() && !matches!(character, '\n' | '\r' | '\t')
}

#[cfg(test)]
mod tests {
    use super::{
        MessageRole, MessageStatus, NewConversation, NewGenerationSettings, NewMessage,
        validate_conversation_title,
    };

    #[test]
    fn conversation_creation_trims_title() {
        let conversation = NewConversation {
            provider_id: "provider-id".to_string(),
            model_id: Some("model-id".to_string()),
            title: "  First prompt  ".to_string(),
        }
        .into_conversation()
        .unwrap();

        assert_eq!(conversation.title, "First prompt");
        assert_eq!(conversation.provider_id, "provider-id");
        assert_eq!(conversation.model_id.as_deref(), Some("model-id"));
        assert!(conversation.archived_at.is_none());
    }

    #[test]
    fn conversation_title_rejects_empty_values() {
        assert!(validate_conversation_title("   ").is_err());
    }

    #[test]
    fn message_role_round_trips_storage_value() {
        assert_eq!(
            "assistant".parse::<MessageRole>().unwrap(),
            MessageRole::Assistant
        );
        assert_eq!(MessageRole::Tool.as_str(), "tool");
        assert!("unknown".parse::<MessageRole>().is_err());
    }

    #[test]
    fn message_status_round_trips_storage_value() {
        assert_eq!(
            "cancelled".parse::<MessageStatus>().unwrap(),
            MessageStatus::Cancelled
        );
        assert!(MessageStatus::Complete.is_finished());
        assert!("unknown".parse::<MessageStatus>().is_err());
    }

    #[test]
    fn user_message_is_complete() {
        let message = NewMessage::user("conversation-id", "Hello")
            .into_message()
            .unwrap();

        assert_eq!(message.role, MessageRole::User);
        assert_eq!(message.status, MessageStatus::Complete);
        assert!(message.completed_at.is_some());
    }

    #[test]
    fn assistant_streaming_message_can_start_empty() {
        let message = NewMessage::assistant_streaming("conversation-id")
            .into_message()
            .unwrap();

        assert_eq!(message.role, MessageRole::Assistant);
        assert_eq!(message.status, MessageStatus::Streaming);
        assert!(message.content.is_empty());
        assert!(message.completed_at.is_none());
    }

    #[test]
    fn generation_settings_validate_model_name() {
        let settings = NewGenerationSettings {
            conversation_id: Some("conversation-id".to_string()),
            model: Some("llama3.2:latest".to_string()),
            temperature: Some(0.7),
            top_p: Some(0.9),
            top_k: Some(40),
            seed: Some(42),
            num_ctx: Some(4096),
            system_prompt: None,
        }
        .into_generation_settings()
        .unwrap();

        assert_eq!(settings.model.as_deref(), Some("llama3.2:latest"));
    }
}
