use serde::{Deserialize, Serialize};

use crate::{
    core::{new_id, utc_now},
    error::{MooseError, Result},
};

pub const GENERAL_PROFILE_ID: &str = "builtin-general";
pub const MAX_PROFILE_NAME_LENGTH: usize = 48;
pub const MAX_PROFILE_DESCRIPTION_LENGTH: usize = 160;
pub const MAX_SYSTEM_PROMPT_LENGTH: usize = 12_000;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChatProfile {
    pub id: String,
    pub name: String,
    pub description: String,
    pub temperature: f64,
    pub system_prompt: String,
    pub is_builtin: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct NewChatProfile {
    pub name: String,
    pub description: String,
    pub temperature: f64,
    pub system_prompt: String,
}

impl NewChatProfile {
    pub fn into_profile(self) -> Result<ChatProfile> {
        let timestamp = utc_now();
        Ok(ChatProfile {
            id: new_id(),
            name: validate_profile_name(&self.name)?,
            description: validate_profile_description(&self.description)?,
            temperature: validate_temperature(self.temperature)?,
            system_prompt: validate_system_prompt(&self.system_prompt)?,
            is_builtin: false,
            created_at: timestamp.clone(),
            updated_at: timestamp,
        })
    }
}

pub fn validate_profile_name(value: &str) -> Result<String> {
    validate_text(value, MAX_PROFILE_NAME_LENGTH, false).ok_or(MooseError::InvalidProfileName)
}

pub fn validate_profile_description(value: &str) -> Result<String> {
    validate_text(value, MAX_PROFILE_DESCRIPTION_LENGTH, false)
        .ok_or(MooseError::InvalidProfileDescription)
}

pub fn validate_system_prompt(value: &str) -> Result<String> {
    let trimmed = value.trim();
    if trimmed.len() > MAX_SYSTEM_PROMPT_LENGTH
        || trimmed
            .chars()
            .any(|character| character.is_control() && !matches!(character, '\n' | '\r' | '\t'))
    {
        return Err(MooseError::InvalidSystemPrompt);
    }
    Ok(trimmed.to_string())
}

fn validate_temperature(value: f64) -> Result<f64> {
    if !value.is_finite() || !(0.0..=2.0).contains(&value) {
        return Err(MooseError::InvalidGenerationSettings);
    }
    Ok(value)
}

fn validate_text(value: &str, max_length: usize, allow_empty: bool) -> Option<String> {
    let trimmed = value.trim();
    if (!allow_empty && trimmed.is_empty())
        || trimmed.len() > max_length
        || trimmed.chars().any(char::is_control)
    {
        return None;
    }
    Some(trimmed.to_string())
}
