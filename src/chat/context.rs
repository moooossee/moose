use super::{ChatMessage, ChatRequest, ChatRole};
use crate::{
    conversations::{Message, MessageRole, MessageStatus},
    error::{MooseError, Result},
};

const MESSAGE_OVERHEAD: usize = 16;
const REQUEST_OVERHEAD: usize = 64;
const ESTIMATED_IMAGE_TOKENS: usize = 1024;
const MAX_CONTINUATION_TOKENS: usize = 8192;

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
        let response_reserve = response_reserve(context_window);
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
        let Some(current) = current_turn(messages) else {
            self.remaining(messages, extra_tokens)?;
            return Ok(());
        };
        let mut remaining = loop {
            let occupied = messages
                .iter()
                .enumerate()
                .filter(|(index, message)| *index >= current || message.role == ChatRole::System)
                .fold(extra_tokens, |total, (_, message)| {
                    total.saturating_add(estimate_message_tokens(message))
                });
            if let Some(remaining) = self.input_tokens.checked_sub(occupied) {
                break remaining;
            }
            if !trim_web_result(&mut messages[current..], occupied - self.input_tokens)? {
                return Err(context_too_large());
            }
        };
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

pub fn response_reserve(context_window: usize) -> usize {
    (context_window / 4).clamp(1, 2048)
}

fn current_turn(messages: &[ChatMessage]) -> Option<usize> {
    messages
        .iter()
        .rposition(|message| {
            message.role == ChatRole::User && message.content != super::CONTINUE_PROMPT
        })
        .or_else(|| {
            messages
                .iter()
                .rposition(|message| message.role == ChatRole::User)
        })
}

pub fn shrink_request(request: &mut ChatRequest, excess: usize) -> Result<bool> {
    let Some(current) = current_turn(&request.messages) else {
        return Ok(false);
    };
    if let Some(start) = request.messages[..current]
        .iter()
        .position(|message| message.role == ChatRole::User)
    {
        let end = request.messages[start + 1..=current]
            .iter()
            .position(|message| message.role == ChatRole::User)
            .map(|offset| start + 1 + offset)
            .unwrap_or(current);
        let mut index = 0;
        request.messages.retain(|message| {
            let keep = index < start || index >= end || message.role == ChatRole::System;
            index += 1;
            keep
        });
        return Ok(true);
    }
    if trim_web_result(&mut request.messages[current..], excess)? {
        return Ok(true);
    }
    if request.messages.last().is_some_and(|message| {
        message.role == ChatRole::User && message.content == super::CONTINUE_PROMPT
    }) && let Some(assistant) = request.messages[current + 1..]
        .iter_mut()
        .find(|message| message.role == ChatRole::Assistant)
    {
        let tokens = estimate_text_tokens(&assistant.content);
        let available = tokens.saturating_sub(excess.max(tokens / 4).max(128));
        if available >= 256 {
            assistant.content = continuation_excerpt(&assistant.content, available)?;
            return Ok(true);
        }
    }
    Ok(false)
}

pub fn continuation_messages(
    history: &[Message],
    system_prompt: &str,
    request: &ChatRequest,
    overhead: usize,
    message_limit: usize,
) -> Result<Vec<ChatMessage>> {
    let start = history
        .iter()
        .rposition(|message| {
            message.role == MessageRole::User && message.content != super::CONTINUE_PROMPT
        })
        .ok_or(MooseError::MessageNotFound)?;
    let mut messages = Vec::new();
    if !system_prompt.trim().is_empty() {
        messages.push(ChatMessage::system(system_prompt.trim()));
    }
    messages.push(ChatMessage::user(&history[start].content));
    let written = history[start + 1..]
        .iter()
        .filter(|message| message.role == MessageRole::Assistant)
        .map(|message| message.content.as_str())
        .collect::<Vec<_>>()
        .join("\n\n");
    let assistant = messages.len();
    messages.push(ChatMessage::assistant(""));
    messages.push(ChatMessage::user(super::CONTINUE_PROMPT));
    let window = request.options.as_ref().and_then(|options| options.num_ctx);
    let available = ContextBudget::new(window)
        .remaining(&messages, overhead)?
        .saturating_sub(32)
        .min(window.map_or(MAX_CONTINUATION_TOKENS, |_| usize::MAX));
    messages[assistant].content = continuation_excerpt(&written, available)?;
    let previous = recent_exchanges(&history[..start], message_limit)
        .into_iter()
        .flat_map(|[user, assistant]| {
            [
                ChatMessage::user(&user.content),
                ChatMessage::assistant(&assistant.content),
            ]
        });
    let insert_at = usize::from(!system_prompt.trim().is_empty());
    messages.splice(insert_at..insert_at, previous);
    ContextBudget::new(window).fit(&mut messages, overhead)?;
    Ok(messages)
}

fn continuation_excerpt(written: &str, available: usize) -> Result<String> {
    if estimate_text_tokens(written) <= available {
        return Ok(written.to_string());
    }
    let marker = "\n\n[Earlier passages omitted. The ending follows.]\n\n";
    let available = available
        .checked_sub(estimate_text_tokens(marker))
        .filter(|tokens| *tokens >= 128)
        .ok_or_else(context_too_large)?;
    let mut beginning = written.to_string();
    trim_text(&mut beginning, available / 4);
    let tail_budget = available.saturating_sub(estimate_text_tokens(&beginning));
    let boundaries = written
        .char_indices()
        .map(|(index, _)| index)
        .chain(std::iter::once(written.len()))
        .collect::<Vec<_>>();
    let start =
        boundaries.partition_point(|&start| estimate_text_tokens(&written[start..]) > tail_budget);
    Ok(format!(
        "{beginning}{marker}{}",
        &written[boundaries[start]..]
    ))
}

fn trim_web_result(messages: &mut [ChatMessage], excess: usize) -> Result<bool> {
    let mut largest = None;
    let mut largest_tokens = 0;
    for (index, message) in messages.iter().enumerate() {
        if message.role != ChatRole::Tool
            || !matches!(
                message.tool_name.as_deref(),
                Some("web_search" | "read_url")
            )
        {
            continue;
        }
        let Ok(result) = serde_json::from_str::<serde_json::Value>(&message.content) else {
            continue;
        };
        if let Some(sources) = result["sources"].as_array() {
            for (source, value) in sources.iter().enumerate() {
                let tokens = value["content"].as_str().map_or(0, estimate_text_tokens);
                if tokens > largest_tokens {
                    largest_tokens = tokens;
                    largest = Some((index, source, result.clone()));
                }
            }
        }
    }
    let Some((index, source, mut result)) = largest else {
        return Ok(false);
    };
    let mut content = result["sources"][source]["content"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    trim_text(
        &mut content,
        largest_tokens.saturating_sub(excess.saturating_add(32)),
    );
    result["sources"][source]["content"] = content.into();
    result["sources"][source]["truncated"] = true.into();
    messages[index].content = serde_json::to_string(&result)?;
    Ok(true)
}

pub fn trim_text(content: &mut String, tokens: usize) {
    if estimate_text_tokens(content) <= tokens {
        return;
    }
    let available = tokens.saturating_sub(estimate_text_tokens("…"));
    let boundaries = content
        .char_indices()
        .map(|(index, _)| index)
        .chain(std::iter::once(content.len()))
        .collect::<Vec<_>>();
    let end = boundaries.partition_point(|&end| estimate_text_tokens(&content[..end]) <= available);
    let mut boundary = boundaries[end.saturating_sub(1)];
    if let Some(word_end) = content[..boundary].rfind(char::is_whitespace)
        && word_end >= boundary / 2
    {
        boundary = word_end;
    }
    content.truncate(boundary);
    if tokens >= estimate_text_tokens("…") {
        content.push('…');
    }
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

pub fn context_too_large() -> MooseError {
    MooseError::ContextTooLarge
}
