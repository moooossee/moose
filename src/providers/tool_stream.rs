use std::collections::{BTreeMap, HashSet};

use serde_json::{Value, json};

use super::{ProviderKind, stream::invalid};
use crate::{
    chat::ChatStreamEvent,
    error::Result,
    tools::{MAX_ARGUMENT_BYTES, MAX_CALLS, ToolCall},
};

#[derive(Default)]
pub(super) struct ToolStream {
    content: BTreeMap<u64, Value>,
    calls: BTreeMap<u64, Value>,
    arguments: BTreeMap<u64, String>,
    reasoning: BTreeMap<u64, Value>,
    open_blocks: HashSet<u64>,
    message: Value,
    bytes: usize,
    expected_calls: bool,
}

impl ToolStream {
    pub fn push(&mut self, kind: ProviderKind, value: &Value) -> Result<()> {
        if matches!(kind, ProviderKind::Ollama | ProviderKind::OllamaCloud) {
            return Ok(());
        }
        self.bytes = self.bytes.saturating_add(value.to_string().len());
        if self.bytes > 8 * 1024 * 1024 {
            return Err(invalid());
        }
        match kind {
            ProviderKind::OpenAi => {
                if value["type"] == "response.output_item.done" {
                    let index = value["output_index"].as_u64().ok_or_else(invalid)?;
                    if self.content.insert(index, value["item"].clone()).is_some() {
                        return Err(invalid());
                    }
                }
                if value["type"] == "response.completed"
                    && let Some(output) = value["response"]["output"].as_array()
                {
                    self.content = output
                        .iter()
                        .cloned()
                        .enumerate()
                        .map(|(index, item)| (index as u64, item))
                        .collect();
                }
            }
            ProviderKind::Anthropic => self.anthropic(value)?,
            ProviderKind::Gemini => {
                if let Some(parts) = value["candidates"][0]["content"]["parts"].as_array() {
                    for part in parts {
                        self.content.insert(self.content.len() as u64, part.clone());
                    }
                }
            }
            _ => self.chat_completions(value)?,
        }
        Ok(())
    }

    fn anthropic(&mut self, value: &Value) -> Result<()> {
        let kind = value["type"].as_str().unwrap_or_default();
        if kind == "message_delta" {
            self.expected_calls |= value["delta"]["stop_reason"] == "tool_use";
        }
        if !matches!(
            kind,
            "content_block_start" | "content_block_delta" | "content_block_stop"
        ) {
            return Ok(());
        }
        let index = value["index"].as_u64().ok_or_else(invalid)?;
        if kind == "content_block_start" {
            if !value["content_block"].is_object()
                || self
                    .content
                    .insert(index, value["content_block"].clone())
                    .is_some()
            {
                return Err(invalid());
            }
            self.open_blocks.insert(index);
            return Ok(());
        }
        if !self.open_blocks.contains(&index) {
            return Err(invalid());
        }
        let block = self.content.get_mut(&index).ok_or_else(invalid)?;
        if kind == "content_block_stop" {
            if let Some(arguments) = self.arguments.remove(&index) {
                block["input"] = serde_json::from_str(&arguments).map_err(|_| invalid())?;
            }
            self.open_blocks.remove(&index);
            return Ok(());
        }
        let delta = &value["delta"];
        match delta["type"].as_str() {
            Some("input_json_delta") => {
                if block["type"] != "tool_use" {
                    return Err(invalid());
                }
                let arguments = self.arguments.entry(index).or_default();
                arguments.push_str(delta["partial_json"].as_str().ok_or_else(invalid)?);
                if arguments.len() > MAX_ARGUMENT_BYTES {
                    return Err(invalid());
                }
            }
            Some("text_delta" | "thinking_delta" | "signature_delta") => {
                let field = match delta["type"].as_str() {
                    Some("text_delta") => "text",
                    Some("thinking_delta") => "thinking",
                    _ => "signature",
                };
                merge(&mut block[field], &delta[field], field)?;
            }
            Some("citations_delta") => {
                if block["citations"].is_null() {
                    block["citations"] = json!([]);
                }
                block["citations"]
                    .as_array_mut()
                    .ok_or_else(invalid)?
                    .push(delta["citation"].clone());
            }
            _ => {}
        }
        Ok(())
    }

    fn chat_completions(&mut self, value: &Value) -> Result<()> {
        let Some(choice) = value["choices"]
            .as_array()
            .and_then(|choices| choices.first())
        else {
            return Ok(());
        };
        self.expected_calls |= choice["finish_reason"] == "tool_calls";
        let delta = &choice["delta"];
        if self.message.is_null() {
            self.message = json!({"role":"assistant","content":null});
        }
        for field in ["content", "reasoning_content", "reasoning", "refusal"] {
            if !delta[field].is_null() {
                merge(&mut self.message[field], &delta[field], field)?;
            }
        }
        if let Some(details) = delta["reasoning_details"].as_array() {
            for (position, detail) in details.iter().enumerate() {
                let index = detail["index"]
                    .as_u64()
                    .or_else(|| {
                        detail["id"].as_str().and_then(|id| {
                            self.reasoning
                                .iter()
                                .find(|(_, previous)| previous["id"] == id)
                                .map(|(index, _)| *index)
                        })
                    })
                    .unwrap_or(position as u64);
                merge(
                    self.reasoning.entry(index).or_default(),
                    detail,
                    "reasoning_details",
                )?;
            }
        }
        if let Some(calls) = delta["tool_calls"].as_array() {
            for call in calls {
                let index = call["index"].as_u64().ok_or_else(invalid)?;
                if !self.calls.contains_key(&index) && self.calls.len() >= MAX_CALLS {
                    return Err(invalid());
                }
                let target = self
                    .calls
                    .entry(index)
                    .or_insert_with(|| json!({"type":"function","function":{"name":""}}));
                merge(target, call, "tool_calls")?;
                let arguments = &target["function"]["arguments"];
                if !arguments.is_null()
                    && match arguments {
                        Value::String(arguments) => arguments.len() > MAX_ARGUMENT_BYTES,
                        Value::Object(_) => arguments.to_string().len() > MAX_ARGUMENT_BYTES,
                        _ => true,
                    }
                {
                    return Err(invalid());
                }
            }
        }
        Ok(())
    }

    pub fn finish(&mut self, kind: ProviderKind) -> Result<Vec<ChatStreamEvent>> {
        if matches!(kind, ProviderKind::Ollama | ProviderKind::OllamaCloud) {
            return Ok(Vec::new());
        }
        if !self.open_blocks.is_empty() {
            return Err(invalid());
        }
        let mut calls = Vec::new();
        let content = match kind {
            ProviderKind::OpenAi | ProviderKind::Anthropic | ProviderKind::Gemini => {
                let content: Vec<_> = std::mem::take(&mut self.content).into_values().collect();
                for item in &content {
                    let (function, arguments, id) = match kind {
                        ProviderKind::OpenAi if item["type"] == "function_call" => (
                            item,
                            parse_arguments(&item["arguments"])?,
                            Some(required_id(&item["call_id"])?),
                        ),
                        ProviderKind::Anthropic if item["type"] == "tool_use" => {
                            (item, item["input"].clone(), Some(required_id(&item["id"])?))
                        }
                        ProviderKind::Gemini if item["functionCall"].is_object() => {
                            let function = &item["functionCall"];
                            let arguments =
                                function.get("args").cloned().unwrap_or_else(|| json!({}));
                            let id = function.get("id").map(required_id).transpose()?;
                            (function, arguments, id)
                        }
                        _ => continue,
                    };
                    calls.push(ToolCall::new(
                        id,
                        function["name"].as_str().ok_or_else(invalid)?.into(),
                        arguments,
                    ));
                }
                content
            }
            _ => {
                let mut raw_calls = Vec::new();
                for mut call in std::mem::take(&mut self.calls).into_values() {
                    let function = &call["function"];
                    calls.push(ToolCall::new(
                        Some(required_id(&call["id"])?),
                        function["name"].as_str().ok_or_else(invalid)?.into(),
                        parse_arguments(&function["arguments"])?,
                    ));
                    call.as_object_mut().ok_or_else(invalid)?.remove("index");
                    raw_calls.push(call);
                }
                if !self.reasoning.is_empty() {
                    self.message["reasoning_details"] = json!(
                        std::mem::take(&mut self.reasoning)
                            .into_values()
                            .collect::<Vec<_>>()
                    );
                }
                if !raw_calls.is_empty() {
                    self.message["tool_calls"] = json!(raw_calls);
                }
                vec![std::mem::take(&mut self.message)]
            }
        };
        let mut ids = HashSet::new();
        if calls.len() > MAX_CALLS
            || calls.iter().any(|call| {
                !call.is_valid() || call.id.as_ref().is_some_and(|id| !ids.insert(id.clone()))
            })
            || (self.expected_calls && calls.is_empty())
        {
            return Err(invalid());
        }
        if calls.is_empty() {
            return Ok(Vec::new());
        }
        if serde_json::to_vec(&content)?.len() > 1024 * 1024 {
            return Err(invalid());
        }
        let mut events: Vec<_> = calls.into_iter().map(ChatStreamEvent::ToolCall).collect();
        events.push(ChatStreamEvent::ProviderContent(content));
        Ok(events)
    }
}

fn required_id(value: &Value) -> Result<String> {
    value
        .as_str()
        .filter(|id| !id.is_empty())
        .map(str::to_owned)
        .ok_or_else(invalid)
}

fn parse_arguments(value: &Value) -> Result<Value> {
    if value.is_object() && value.to_string().len() <= MAX_ARGUMENT_BYTES {
        return Ok(value.clone());
    }
    let arguments = value.as_str().ok_or_else(invalid)?;
    if arguments.len() > MAX_ARGUMENT_BYTES {
        return Err(invalid());
    }
    serde_json::from_str(arguments).map_err(|_| invalid())
}

fn merge(target: &mut Value, delta: &Value, field: &str) -> Result<()> {
    if delta.is_null() {
        return Ok(());
    }
    if target.is_null() {
        *target = delta.clone();
        return Ok(());
    }
    if field == "content" && target.is_string() && delta.is_array() {
        *target = json!([{"type":"text","text":target.as_str().unwrap()}]);
    }
    match (target, delta) {
        (Value::Object(target), Value::Object(delta)) => {
            for (field, value) in delta {
                merge(
                    target.entry(field.clone()).or_insert(Value::Null),
                    value,
                    field,
                )?;
            }
        }
        (Value::Array(target), Value::Array(delta)) => target.extend(delta.iter().cloned()),
        (Value::Array(target), Value::String(delta)) if field == "content" => {
            target.push(json!({"type":"text","text":delta}));
        }
        (Value::String(target), Value::String(delta))
            if field == "name" && target.as_str() == delta.as_str() => {}
        (Value::String(target), Value::String(delta))
            if !matches!(field, "id" | "type" | "format" | "role") =>
        {
            target.push_str(delta)
        }
        (target, delta) if *target == *delta => {}
        _ => return Err(invalid()),
    }
    Ok(())
}
