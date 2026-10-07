use super::ProviderKind;
use crate::{
    chat::ChatStreamEvent,
    error::{MooseError, Result},
};
use serde_json::Value;

const MAX_EVENT_BYTES: usize = 8 * 1024 * 1024;

pub(super) struct Decoder {
    kind: ProviderKind,
    pending: Vec<u8>,
    data: Vec<u8>,
}

impl Decoder {
    pub fn new(kind: ProviderKind) -> Self {
        Self {
            kind,
            pending: Vec::new(),
            data: Vec::new(),
        }
    }
    pub fn push(&mut self, chunk: &[u8]) -> Result<Vec<ChatStreamEvent>> {
        let mut events = Vec::new();
        for part in chunk.split_inclusive(|byte| *byte == b'\n') {
            if self.pending.len().saturating_add(part.len()) > MAX_EVENT_BYTES {
                return Err(invalid());
            }
            self.pending.extend_from_slice(part);
            if part.ends_with(b"\n") {
                let line = std::mem::take(&mut self.pending);
                self.line(&line[..line.len() - 1], &mut events)?;
            }
        }
        Ok(events)
    }
    pub fn finish(&mut self) -> Result<Vec<ChatStreamEvent>> {
        let mut events = Vec::new();
        if !self.pending.is_empty() {
            let line = std::mem::take(&mut self.pending);
            self.line(&line, &mut events)?;
        }
        self.flush(&mut events)?;
        Ok(events)
    }
    fn line(&mut self, line: &[u8], events: &mut Vec<ChatStreamEvent>) -> Result<()> {
        if line.len() > MAX_EVENT_BYTES {
            return Err(invalid());
        }
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        if self.kind == ProviderKind::OllamaCloud {
            if !line.is_empty() {
                events.extend(parse(self.kind, line)?);
            }
        } else if line.is_empty() {
            self.flush(events)?;
        } else if let Some(data) = line.strip_prefix(b"data:") {
            let data = data.strip_prefix(b" ").unwrap_or(data);
            if self.data.len().saturating_add(data.len()).saturating_add(1) > MAX_EVENT_BYTES {
                return Err(invalid());
            }
            if !self.data.is_empty() {
                self.data.push(b'\n');
            }
            self.data.extend_from_slice(data);
        }
        Ok(())
    }
    fn flush(&mut self, events: &mut Vec<ChatStreamEvent>) -> Result<()> {
        if !self.data.is_empty() {
            events.extend(parse(self.kind, &std::mem::take(&mut self.data))?);
        }
        Ok(())
    }
}

fn invalid() -> MooseError {
    MooseError::ProviderResponse("The provider returned an invalid stream.".into())
}
fn incomplete() -> MooseError {
    MooseError::ProviderResponse("The provider stopped before completing the answer. Try a shorter context or another model.".into())
}
fn blocked() -> MooseError {
    MooseError::ProviderResponse("The provider declined this request.".into())
}
fn token(events: &mut Vec<ChatStreamEvent>, value: &Value, thinking: bool) {
    if let Some(text) = value.as_str().filter(|s| !s.is_empty()) {
        events.push(if thinking {
            ChatStreamEvent::Thinking(text.into())
        } else {
            ChatStreamEvent::Token(text.into())
        });
    }
}

fn parse(kind: ProviderKind, data: &[u8]) -> Result<Vec<ChatStreamEvent>> {
    if data == b"[DONE]" && kind == ProviderKind::Groq {
        return Ok(vec![ChatStreamEvent::Done]);
    }
    let v: Value = serde_json::from_slice(data).map_err(|_| invalid())?;
    if v.get("error").is_some_and(|e| !e.is_null()) || v["type"] == "error" {
        return Err(MooseError::ProviderResponse(
            "The provider reported an error while generating the answer.".into(),
        ));
    }
    let mut out = Vec::new();
    match kind {
        ProviderKind::OllamaCloud => {
            token(&mut out, &v["message"]["thinking"], true);
            token(&mut out, &v["message"]["content"], false);
            if v["done"].as_bool() == Some(true) {
                if v["done_reason"] == "length" {
                    return Err(incomplete());
                }
                out.push(ChatStreamEvent::Done);
            }
        }
        ProviderKind::OpenAi => match v["type"].as_str().unwrap_or_default() {
            "response.output_text.delta" | "response.refusal.delta" => {
                token(&mut out, &v["delta"], false)
            }
            "response.reasoning_summary_text.delta" => token(&mut out, &v["delta"], true),
            "response.completed" => out.push(ChatStreamEvent::Done),
            "response.failed" | "response.incomplete" | "response.cancelled" => {
                return Err(incomplete());
            }
            _ => {}
        },
        ProviderKind::Groq => {
            if let Some(choice) = v["choices"].as_array().and_then(|c| c.first()) {
                token(&mut out, &choice["delta"]["content"], false);
                token(&mut out, &choice["delta"]["reasoning"], true);
                match choice["finish_reason"].as_str() {
                    Some("stop") => out.push(ChatStreamEvent::Done),
                    Some("content_filter") => return Err(blocked()),
                    Some(_) => return Err(incomplete()),
                    None => {}
                }
            }
        }
        ProviderKind::Anthropic => match v["type"].as_str().unwrap_or_default() {
            "content_block_start" => {
                token(&mut out, &v["content_block"]["text"], false);
                token(&mut out, &v["content_block"]["thinking"], true);
            }
            "content_block_delta" => {
                token(&mut out, &v["delta"]["text"], false);
                token(&mut out, &v["delta"]["thinking"], true);
            }
            "message_delta" => {
                if let Some("max_tokens" | "tool_use" | "pause_turn") =
                    v["delta"]["stop_reason"].as_str()
                {
                    return Err(incomplete());
                }
            }
            "message_stop" => out.push(ChatStreamEvent::Done),
            _ => {}
        },
        ProviderKind::Gemini => {
            if v["promptFeedback"].get("blockReason").is_some() {
                return Err(blocked());
            }
            if let Some(candidate) = v["candidates"].as_array().and_then(|c| c.first()) {
                if let Some(parts) = candidate["content"]["parts"].as_array() {
                    for part in parts {
                        token(
                            &mut out,
                            &part["text"],
                            part["thought"].as_bool().unwrap_or(false),
                        );
                    }
                }
                match candidate["finishReason"].as_str() {
                    Some("STOP") => out.push(ChatStreamEvent::Done),
                    Some("MAX_TOKENS") => return Err(incomplete()),
                    Some("FINISH_REASON_UNSPECIFIED") | None => {}
                    Some(_) => return Err(blocked()),
                }
            }
        }
        ProviderKind::Ollama => return Err(invalid()),
    }
    Ok(out)
}
