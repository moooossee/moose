use super::ProviderKind;
use crate::{
    chat::{ChatMessage, ChatRequest, ChatRole, ThinkingValue},
    error::{MooseError, Result},
};
use serde_json::{Value, json};

fn image_mime(data: &str) -> Result<&'static str> {
    if data.starts_with("iVBOR") {
        Ok("image/png")
    } else if data.starts_with("/9j/") {
        Ok("image/jpeg")
    } else if data.starts_with("UklGR") {
        Ok("image/webp")
    } else if data.starts_with("R0lGOD") {
        Ok("image/gif")
    } else {
        Err(MooseError::ProviderRequest(
            "This image format is not supported. Use PNG, JPEG, WebP or GIF.".into(),
        ))
    }
}

fn role(message: &ChatMessage) -> Result<&'static str> {
    match message.role {
        ChatRole::System => Ok("system"),
        ChatRole::User => Ok("user"),
        ChatRole::Assistant => Ok("assistant"),
        ChatRole::Tool => Err(MooseError::ProviderRequest(
            "Tool messages are not supported by this chat.".into(),
        )),
    }
}

pub(super) fn request(kind: ProviderKind, request: &ChatRequest) -> Result<Value> {
    match kind {
        ProviderKind::Ollama | ProviderKind::OllamaCloud => Ok(serde_json::to_value(request)?),
        ProviderKind::OpenAi => openai(request),
        ProviderKind::Groq => groq(request),
        ProviderKind::Anthropic => anthropic(request),
        ProviderKind::Gemini => gemini(request),
    }
}

fn openai(request: &ChatRequest) -> Result<Value> {
    let mut input = Vec::new();
    for message in &request.messages {
        let role = role(message)?;
        if message.images.is_empty() {
            input.push(json!({"role":role,"content":message.content}));
        } else {
            let mut content = vec![json!({"type":"input_text","text":message.content})];
            for image in &message.images {
                content.push(json!({"type":"input_image","image_url":format!("data:{};base64,{image}",image_mime(image)?)}));
            }
            input.push(json!({"role":role,"content":content}));
        }
    }
    let mut value = json!({"model":request.model,"input":input,"stream":true,"store":false});
    if let Some(ThinkingValue::Level(effort)) = &request.think {
        value["reasoning"] = json!({"effort":effort});
        if !request.model.starts_with("o3-mini") && !request.model.starts_with("o1") {
            value["reasoning"]["summary"] = json!("auto");
        }
    }
    Ok(value)
}

fn groq(request: &ChatRequest) -> Result<Value> {
    let mut messages = Vec::new();
    for message in &request.messages {
        let role = role(message)?;
        if message.images.is_empty() {
            messages.push(json!({"role":role,"content":message.content}));
        } else {
            let mut content = vec![json!({"type":"text","text":message.content})];
            for image in &message.images {
                content.push(json!({"type":"image_url","image_url":{"url":format!("data:{};base64,{image}",image_mime(image)?)}}));
            }
            messages.push(json!({"role":role,"content":content}));
        }
    }
    let mut value = json!({"model":request.model,"messages":messages,"stream":true,"store":false});
    if let Some(options) = &request.options {
        if let Some(t) = options.temperature {
            value["temperature"] = json!(t);
        }
        if let Some(p) = options.top_p {
            value["top_p"] = json!(p);
        }
    }
    if let Some(ThinkingValue::Level(effort)) = &request.think {
        value["reasoning_effort"] = json!(effort);
    }
    Ok(value)
}

fn anthropic(request: &ChatRequest) -> Result<Value> {
    let mut messages = Vec::new();
    let mut system = Vec::new();
    for message in &request.messages {
        let role = role(message)?;
        if role == "system" {
            system.push(message.content.as_str());
            continue;
        }
        let mut content = Vec::new();
        if !message.content.is_empty() {
            content.push(json!({"type":"text","text":message.content}));
        }
        for image in &message.images {
            content.push(json!({"type":"image","source":{"type":"base64","media_type":image_mime(image)?,"data":image}}));
        }
        messages.push(json!({"role":role,"content":content}));
    }
    let mut value =
        json!({"model":request.model,"messages":messages,"max_tokens":4096,"stream":true});
    if !system.is_empty() {
        value["system"] = json!(system.join("\n\n"));
    }
    Ok(value)
}

fn gemini(request: &ChatRequest) -> Result<Value> {
    let mut contents = Vec::new();
    let mut system = Vec::new();
    for message in &request.messages {
        let role = role(message)?;
        if role == "system" {
            system.push(json!({"text":message.content}));
            continue;
        }
        let mut parts = vec![json!({"text":message.content})];
        for image in &message.images {
            parts.push(json!({"inlineData":{"mimeType":image_mime(image)?,"data":image}}));
        }
        contents.push(json!({"role":if role=="assistant" {"model"} else {"user"},"parts":parts}));
    }
    let mut value = json!({"contents":contents});
    if !system.is_empty() {
        value["systemInstruction"] = json!({"parts":system});
    }
    if let Some(options) = &request.options {
        let mut config = serde_json::Map::new();
        if let Some(t) = options.temperature {
            config.insert("temperature".into(), json!(t));
        }
        if let Some(p) = options.top_p {
            config.insert("topP".into(), json!(p));
        }
        if !config.is_empty() {
            value["generationConfig"] = Value::Object(config);
        }
    }
    Ok(value)
}

pub(super) fn supports_images(kind: ProviderKind, model: &str) -> bool {
    match kind {
        ProviderKind::Anthropic => model.starts_with("claude-"),
        ProviderKind::Gemini => model.starts_with("gemini-"),
        ProviderKind::OpenAi => {
            ["gpt-4o", "gpt-4.1", "gpt-5", "gpt-6", "o3", "o4"]
                .iter()
                .any(|p| model.starts_with(p))
                && !model.starts_with("o3-mini")
        }
        ProviderKind::Groq => {
            model.starts_with("meta-llama/llama-4") || model.starts_with("qwen/qwen3.8")
        }
        _ => false,
    }
}

pub(super) fn thinking_values(kind: ProviderKind, model: &str) -> Vec<ThinkingValue> {
    let supported = match kind {
        ProviderKind::OpenAi => {
            ["gpt-5", "gpt-6", "o3", "o4"]
                .iter()
                .any(|p| model.starts_with(p))
                && !model.contains("chat")
        }
        ProviderKind::Groq => model.starts_with("openai/gpt-oss"),
        _ => false,
    };
    if supported {
        if kind == ProviderKind::OpenAi && model.contains("-pro") {
            return vec![ThinkingValue::Level("high".into())];
        }
        ["low", "medium", "high"]
            .map(|s| ThinkingValue::Level(s.into()))
            .to_vec()
    } else {
        Vec::new()
    }
}
