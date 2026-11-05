use super::{ChatMessage, ChatRequest, ChatRole};
use crate::{
    conversations::{Message, MessageRole, MessageStatus},
    error::{MooseError, Result},
};

const MESSAGE_OVERHEAD: usize = 16;
const REQUEST_OVERHEAD: usize = 64;
const ESTIMATED_IMAGE_TOKENS: usize = 1024;
const MAX_RESPONSE_RESERVE: usize = 8192;

pub struct ContextBudget {
    input_tokens: usize,
}

impl ContextBudget {
    pub fn new(context_window: Option<i64>) -> Self {
        let Some(context_window) = context_window else {
            return Self {
                input_tokens: usize::MAX,
            };
        };
        let context_window = usize::try_from(context_window.max(1)).unwrap_or(usize::MAX);
        let response_reserve = (context_window / 2).min(MAX_RESPONSE_RESERVE);
        let safety_margin = (context_window / 20).max(128);
        Self {
            input_tokens: context_window
                .saturating_sub(response_reserve)
                .saturating_sub(safety_margin)
                .saturating_sub(REQUEST_OVERHEAD),
        }
    }

    pub fn remaining(&self, messages: &[ChatMessage], extra_tokens: usize) -> Result<usize> {
        let occupied = messages.iter().fold(extra_tokens, |total, message| {
            total.saturating_add(estimate_message_tokens(message))
        });
        self.input_tokens
            .checked_sub(occupied)
            .ok_or_else(context_too_large)
    }

    pub fn fit(&self, messages: &mut Vec<ChatMessage>, extra_tokens: usize) -> Result<()> {
        let Some(current) = messages
            .iter()
            .rposition(|message| message.role == ChatRole::User)
        else {
            self.remaining(messages, extra_tokens)?;
            return Ok(());
        };
        let occupied = messages
            .iter()
            .enumerate()
            .filter(|(index, message)| *index >= current || message.role == ChatRole::System)
            .fold(extra_tokens, |total, (_, message)| {
                total.saturating_add(estimate_message_tokens(message))
            });
        let mut remaining = self
            .input_tokens
            .checked_sub(occupied)
            .ok_or_else(context_too_large)?;
        let mut keep_from = current;
        for start in (0..current)
            .rev()
            .filter(|&index| messages[index].role == ChatRole::User)
        {
            let tokens = messages[start..keep_from]
                .iter()
                .filter(|message| message.role != ChatRole::System)
                .fold(0usize, |total, message| {
                    total.saturating_add(estimate_message_tokens(message))
                });
            let Some(left) = remaining.checked_sub(tokens) else {
                break;
            };
            remaining = left;
            keep_from = start;
        }
        let mut index = 0;
        messages.retain(|message| {
            let keep = index >= keep_from || message.role == ChatRole::System;
            index += 1;
            keep
        });
        Ok(())
    }
}

pub fn fit_request(request: &mut ChatRequest) -> Result<()> {
    let Some(context_window) = request.options.as_ref().and_then(|options| options.num_ctx) else {
        return Ok(());
    };
    let extra_tokens = request_overhead(request)?;
    ContextBudget::new(Some(context_window)).fit(&mut request.messages, extra_tokens)
}

pub fn request_overhead(request: &ChatRequest) -> Result<usize> {
    let tools = if request.tools.is_empty() {
        0
    } else {
        estimate_text_tokens(&serde_json::to_string(&request.tools)?)
    };
    let format = request.format.as_deref().map_or(0, estimate_text_tokens);
    Ok(tools.saturating_add(format))
}

pub fn recent_exchanges(history: &[Message], message_limit: usize) -> Vec<[&Message; 2]> {
    let mut exchanges = history
        .windows(2)
        .rev()
        .filter(|pair| {
            pair[0].role == MessageRole::User
                && pair[1].role == MessageRole::Assistant
                && pair.iter().all(|message| {
                    message.status != MessageStatus::Streaming && !message.content.trim().is_empty()
                })
        })
        .take(message_limit.saturating_sub(1) / 2)
        .map(|pair| [&pair[0], &pair[1]])
        .collect::<Vec<_>>();
    exchanges.reverse();
    exchanges
}

pub fn estimate_message_tokens(message: &ChatMessage) -> usize {
    let mut tokens = MESSAGE_OVERHEAD
        .saturating_add(estimate_text_tokens(&message.content))
        .saturating_add(estimate_text_tokens(&message.thinking))
        .saturating_add(message.images.len().saturating_mul(ESTIMATED_IMAGE_TOKENS));
    if let Some(name) = &message.tool_name {
        tokens = tokens.saturating_add(estimate_text_tokens(name));
    }
    for call in &message.tool_calls {
        tokens = tokens
            .saturating_add(MESSAGE_OVERHEAD)
            .saturating_add(estimate_text_tokens(&call.function.name))
            .saturating_add(estimate_text_tokens(&call.function.arguments.to_string()));
    }
    tokens
}

pub fn estimate_text_tokens(text: &str) -> usize {
    let mut tokens = 0usize;
    let mut letters = 0usize;
    let mut whitespace = 0usize;
    for byte in text.bytes() {
        if byte.is_ascii_alphabetic() {
            letters += 1;
        } else {
            tokens += letters.div_ceil(3);
            letters = 0;
            if byte.is_ascii_whitespace() {
                whitespace += 1;
            } else {
                tokens += 1;
            }
        }
    }
    tokens + letters.div_ceil(3) + whitespace.div_ceil(4)
}

fn context_too_large() -> MooseError {
    MooseError::ProviderRequest(
        "The current message, instructions, attachments or tool results leave too little room for a response. Shorten the message, use fewer attachments, or increase Context Window in Chat Settings."
            .into(),
    )
}
