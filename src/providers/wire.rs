use crate::i18n::{gettext, gettext_noop};

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
            gettext_noop("This image format is not supported. Use PNG, JPEG, WebP or GIF.").into(),
        ))
    }
}

fn role(message: &ChatMessage) -> &'static str {
    match message.role {
        ChatRole::System => "system",
        ChatRole::User => "user",
        ChatRole::Assistant => "assistant",
        ChatRole::Tool => "tool",
    }
}

pub(super) fn request(kind: ProviderKind, request: &ChatRequest) -> Result<Value> {
    let mut value = match kind {
        ProviderKind::Ollama | ProviderKind::OllamaCloud => Ok(serde_json::to_value(request)?),
        ProviderKind::OpenAi => openai(request),
        ProviderKind::Groq
        | ProviderKind::Mistral
        | ProviderKind::DeepSeek
        | ProviderKind::OpenRouter
        | ProviderKind::Nvidia
        | ProviderKind::Cloudflare
        | ProviderKind::Custom => chat_completions(kind, request),
        ProviderKind::Anthropic => anthropic(request),
        ProviderKind::Gemini => gemini(request),
    }?;
    if request.tools_disabled {
        match kind {
            ProviderKind::Ollama | ProviderKind::OllamaCloud => {
                value.as_object_mut().unwrap().remove("tools");
            }
            ProviderKind::Anthropic => value["tool_choice"] = json!({"type":"none"}),
            ProviderKind::Gemini => {
                value["toolConfig"] = json!({"functionCallingConfig":{"mode":"NONE"}});
            }
            _ => value["tool_choice"] = json!("none"),
        }
    }
    if let Some(limit) = request
        .options
        .as_ref()
        .and_then(|options| options.num_predict)
        .filter(|limit| *limit > 0)
    {
        match kind {
            ProviderKind::Ollama | ProviderKind::OllamaCloud => {}
            ProviderKind::OpenAi => value["max_output_tokens"] = json!(limit),
            ProviderKind::Gemini => value["generationConfig"]["maxOutputTokens"] = json!(limit),
            ProviderKind::Groq => value["max_completion_tokens"] = json!(limit),
            _ => value["max_tokens"] = json!(limit),
        }
    }
    Ok(value)
}

fn tool_id(message: &ChatMessage) -> Result<&str> {
    message
        .tool_call_id
        .as_deref()
        .filter(|id| !id.is_empty())
        .ok_or_else(|| {
            MooseError::ProviderRequest(gettext("A tool result is missing its call identifier."))
        })
}

fn tool_output(message: &ChatMessage) -> Value {
    match serde_json::from_str::<Value>(&message.content) {
        Ok(value) if value.is_object() => value,
        _ => json!({"result":message.content}),
    }
}

fn push_parts(messages: &mut Vec<Value>, role: &str, key: &str, parts: Vec<Value>) {
    if let Some(previous) = messages
        .last_mut()
        .filter(|message| message["role"] == role)
    {
        previous[key].as_array_mut().unwrap().extend(parts);
    } else {
        messages.push(json!({"role":role,(key):parts}));
    }
}

fn openai(request: &ChatRequest) -> Result<Value> {
    let mut input = Vec::new();
    for message in &request.messages {
        if message.role == ChatRole::Tool {
            input.push(json!({"type":"function_call_output","call_id":tool_id(message)?,"output":message.content}));
            continue;
        }
        if !message.provider_content.is_empty() {
            input.extend(message.provider_content.iter().cloned());
            continue;
        }
        let role = role(message);
        if !message.content.is_empty()
            || !message.images.is_empty()
            || message.tool_calls.is_empty()
        {
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
        for call in &message.tool_calls {
            let id = call.id.as_deref().ok_or_else(|| {
                MooseError::ProviderRequest(gettext("A tool call is missing its identifier."))
            })?;
            input.push(json!({"type":"function_call","call_id":id,"name":call.function.name,"arguments":call.function.arguments.to_string()}));
        }
    }
    let mut value = json!({"model":request.model,"input":input,"stream":true,"store":false});
    if !request.tools.is_empty() {
        value["tools"] = Value::Array(request.tools.iter().map(|tool| json!({
            "type":"function","name":tool.function.name,"description":tool.function.description,
            "parameters":tool.function.parameters,"strict":!request.model.starts_with("ft:")
        })).collect());
        value["include"] = json!(["reasoning.encrypted_content"]);
    }
    if let Some(ThinkingValue::Level(effort)) = &request.think {
        value["reasoning"] = json!({"effort":effort});
        if !request.model.starts_with("o3-mini") && !request.model.starts_with("o1") {
            value["reasoning"]["summary"] = json!("auto");
        }
    }
    Ok(value)
}

fn chat_completions(kind: ProviderKind, request: &ChatRequest) -> Result<Value> {
    let mut messages = Vec::new();
    for message in &request.messages {
        if message.role == ChatRole::Assistant && !message.provider_content.is_empty() {
            for mut entry in message.provider_content.iter().cloned() {
                if kind == ProviderKind::DeepSeek
                    && !request.tools.is_empty()
                    && entry.get("reasoning_content").is_none()
                {
                    entry["reasoning_content"] = json!(message.thinking);
                }
                messages.push(entry);
            }
            continue;
        }
        let role = role(message);
        let mut entry = if message.images.is_empty() {
            json!({"role":role,"content":message.content})
        } else {
            if kind != ProviderKind::Groq && role != "user" {
                return Err(MooseError::ProviderRequest(gettext(
                    "Images are only supported in user messages for this provider.",
                )));
            }
            let mut content = Vec::new();
            if !message.content.is_empty() {
                content.push(json!({"type":"text","text":message.content}));
            }
            for image in &message.images {
                content.push(json!({"type":"image_url","image_url":{"url":format!("data:{};base64,{image}",image_mime(image)?)}}));
            }
            json!({"role":role,"content":content})
        };
        if message.role == ChatRole::Tool {
            entry["tool_call_id"] = json!(tool_id(message)?);
        }
        if message.role == ChatRole::Assistant
            && kind == ProviderKind::DeepSeek
            && !request.tools.is_empty()
        {
            entry["reasoning_content"] = json!(message.thinking);
        }
        if !message.tool_calls.is_empty() {
            let mut calls = Vec::new();
            for call in &message.tool_calls {
                let id = call.id.as_deref().ok_or_else(|| {
                    MooseError::ProviderRequest(gettext("A tool call is missing its identifier."))
                })?;
                calls.push(json!({"id":id,"type":"function","function":{"name":call.function.name,"arguments":call.function.arguments.to_string()}}));
            }
            entry["tool_calls"] = json!(calls);
            if message.content.is_empty() {
                entry["content"] = Value::Null;
            }
            if !message.thinking.is_empty() {
                entry["reasoning_content"] = json!(message.thinking);
            }
        }
        messages.push(entry);
    }
    let mut value = json!({"model":request.model,"messages":messages,"stream":true});
    if !request.tools.is_empty() {
        value["tools"] = serde_json::to_value(&request.tools)?;
        if kind == ProviderKind::OpenRouter {
            value["provider"] = json!({"require_parameters":true});
        }
    }
    if kind == ProviderKind::Groq {
        value["store"] = json!(false);
    }
    let deepseek_thinking = kind == ProviderKind::DeepSeek
        && !matches!(&request.think, Some(ThinkingValue::Enabled(false)))
        && !matches!(&request.think, Some(ThinkingValue::Level(effort)) if effort == "none");
    if let Some(options) = &request.options {
        if let Some(t) = options.temperature.filter(|_| !deepseek_thinking) {
            value["temperature"] = json!(t);
        }
        if let Some(p) = options
            .top_p
            .filter(|_| kind != ProviderKind::DeepSeek || deepseek_thinking)
        {
            value["top_p"] = json!(p);
        }
    }
    if kind == ProviderKind::OpenRouter {
        match &request.think {
            Some(ThinkingValue::Level(effort)) => value["reasoning"] = json!({"effort":effort}),
            Some(ThinkingValue::Enabled(enabled)) => {
                value["reasoning"] = json!({"enabled":enabled})
            }
            None => {}
        }
    } else if let Some(ThinkingValue::Level(effort)) = &request.think {
        value["reasoning_effort"] = json!(effort);
    }
    if kind == ProviderKind::DeepSeek && request.think.is_some() {
        value["thinking"] = json!({"type":if deepseek_thinking {"enabled"} else {"disabled"}});
    }
    Ok(value)
}

fn anthropic(request: &ChatRequest) -> Result<Value> {
    let mut messages = Vec::new();
    let mut system = Vec::new();
    for message in &request.messages {
        if message.role == ChatRole::System {
            system.push(message.content.as_str());
            continue;
        }
        if message.role == ChatRole::Tool {
            let output = tool_output(message);
            let result = json!({"type":"tool_result","tool_use_id":tool_id(message)?,"content":message.content,"is_error":output.get("error").is_some()});
            push_parts(&mut messages, "user", "content", vec![result]);
            continue;
        }
        let mut content = message.provider_content.clone();
        if content.is_empty() {
            if !message.content.is_empty() {
                content.push(json!({"type":"text","text":message.content}));
            }
            for image in &message.images {
                content.push(json!({"type":"image","source":{"type":"base64","media_type":image_mime(image)?,"data":image}}));
            }
            for call in &message.tool_calls {
                let id = call.id.as_deref().ok_or_else(|| {
                    MooseError::ProviderRequest(gettext("A tool call is missing its identifier."))
                })?;
                content.push(json!({"type":"tool_use","id":id,"name":call.function.name,"input":call.function.arguments}));
            }
        }
        push_parts(&mut messages, role(message), "content", content);
    }
    let mut value =
        json!({"model":request.model,"messages":messages,"max_tokens":4096,"stream":true});
    if !request.tools.is_empty() {
        value["tools"] = Value::Array(request.tools.iter().map(|tool| json!({
            "name":tool.function.name,"description":tool.function.description,"input_schema":tool.function.parameters
        })).collect());
    }
    if !system.is_empty() {
        value["system"] = json!(system.join("\n\n"));
    }
    Ok(value)
}

fn gemini(request: &ChatRequest) -> Result<Value> {
    let mut contents = Vec::new();
    let mut system = Vec::new();
    for message in &request.messages {
        if message.role == ChatRole::System {
            system.push(json!({"text":message.content}));
            continue;
        }
        if message.role == ChatRole::Tool {
            let name = message.tool_name.as_deref().ok_or_else(|| {
                MooseError::ProviderRequest(gettext("A tool result is missing its function name."))
            })?;
            let mut response = json!({"name":name,"response":tool_output(message)});
            if let Some(id) = &message.tool_call_id {
                response["id"] = json!(id);
            }
            push_parts(
                &mut contents,
                "user",
                "parts",
                vec![json!({"functionResponse":response})],
            );
            continue;
        }
        let mut parts = message.provider_content.clone();
        if parts.is_empty() {
            if !message.content.is_empty() || message.tool_calls.is_empty() {
                parts.push(json!({"text":message.content}));
            }
            for image in &message.images {
                parts.push(json!({"inlineData":{"mimeType":image_mime(image)?,"data":image}}));
            }
            for call in &message.tool_calls {
                let mut function =
                    json!({"name":call.function.name,"args":call.function.arguments});
                if let Some(id) = &call.id {
                    function["id"] = json!(id);
                }
                parts.push(json!({"functionCall":function}));
            }
        }
        push_parts(
            &mut contents,
            if message.role == ChatRole::Assistant {
                "model"
            } else {
                "user"
            },
            "parts",
            parts,
        );
    }
    let mut value = json!({"contents":contents});
    if !request.tools.is_empty() {
        let declarations: Vec<_> = request.tools.iter().map(|tool| {
            json!({"name":tool.function.name,"description":tool.function.description,"parametersJsonSchema":tool.function.parameters})
        }).collect();
        value["tools"] = json!([{"functionDeclarations":declarations}]);
    }
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
        ProviderKind::DeepSeek => {
            matches!(model, "deepseek-flash" | "deepseek-v4-flash-vision-exp")
        }
        ProviderKind::Nvidia => {
            matches!(
                model,
                "meta/llama-3.2-11b-vision-instruct"
                    | "meta/llama-3.2-90b-vision-instruct"
                    | "microsoft/phi-3.5-vision-instruct"
                    | "nvidia/nemotron-nano-12b-v2-vl"
            ) || model.starts_with("qwen/qwen2.5-vl-")
                || model.starts_with("qwen/qwen3.5-")
        }
        _ => false,
    }
}

pub(super) fn thinking_values(kind: ProviderKind, model: &str) -> Vec<ThinkingValue> {
    if kind == ProviderKind::DeepSeek && model.starts_with("deepseek-") {
        return vec![
            ThinkingValue::Enabled(false),
            ThinkingValue::Level("low".into()),
            ThinkingValue::Level("high".into()),
            ThinkingValue::Level("max".into()),
        ];
    }
    if kind == ProviderKind::Mistral {
        let levels: &[&str] = match model {
            "mistral-small-latest"
            | "mistral-small-2603"
            | "mistral-medium-latest"
            | "mistral-medium-3-5"
            | "mistral-large-latest"
            | "mistral-large-4-0" => &["none", "high"],
            "zai-glm-5-3" => &["low", "high", "max"],
            _ => &[],
        };
        return levels
            .iter()
            .map(|level| ThinkingValue::Level((*level).into()))
            .collect();
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::ChatOptions;

    fn chat_request(model: &str) -> ChatRequest {
        ChatRequest {
            tools_disabled: false,
            tools: Vec::new(),
            think: None,
            model: model.into(),
            messages: vec![
                ChatMessage::system("Be helpful."),
                ChatMessage::user("Hello"),
            ],
            stream: true,
            format: None,
            options: Some(ChatOptions {
                temperature: Some(0.7),
                top_p: Some(0.9),
                ..Default::default()
            }),
        }
    }

    #[test]
    fn new_providers_send_chat_completion_requests() {
        for (kind, model) in [
            (ProviderKind::OpenRouter, "openrouter/free"),
            (
                ProviderKind::Nvidia,
                "nvidia/nemotron-3.5-lightning-30b-a3b",
            ),
            (
                ProviderKind::Cloudflare,
                "@cf/meta/llama-4-scout-17b-16e-instruct",
            ),
        ] {
            let body = request(kind, &chat_request(model)).unwrap();
            assert_eq!(body["model"], model);
            assert_eq!(body["stream"], true);
            assert_eq!(
                body["messages"][0],
                json!({"role":"system","content":"Be helpful."})
            );
            assert_eq!(
                body["messages"][1],
                json!({"role":"user","content":"Hello"})
            );
            assert_eq!(body["temperature"], 0.7);
            assert_eq!(body["top_p"], 0.9);
            assert!(body.get("store").is_none());
            assert!(body.get("reasoning_effort").is_none());
        }
    }

    #[test]
    fn new_providers_encode_user_images_and_reject_assistant_images() {
        for kind in [
            ProviderKind::OpenRouter,
            ProviderKind::Nvidia,
            ProviderKind::Cloudflare,
        ] {
            let mut chat = chat_request("vision-model");
            chat.messages[1].images.push("iVBORexample".into());
            let body = request(kind, &chat).unwrap();
            assert_eq!(
                body["messages"][1]["content"][1],
                json!({
                    "type":"image_url",
                    "image_url":{"url":"data:image/png;base64,iVBORexample"}
                })
            );
            chat.messages[1].role = ChatRole::Assistant;
            assert!(request(kind, &chat).is_err());
        }
    }

    #[test]
    fn tool_results_require_call_identifiers() {
        for kind in [
            ProviderKind::OpenRouter,
            ProviderKind::Nvidia,
            ProviderKind::Cloudflare,
        ] {
            let mut chat = chat_request("chat-model");
            chat.messages[1].role = ChatRole::Tool;
            assert!(request(kind, &chat).is_err());
        }
    }

    #[test]
    fn nvidia_vision_support_excludes_text_models() {
        assert!(supports_images(
            ProviderKind::Nvidia,
            "nvidia/nemotron-nano-12b-v2-vl"
        ));
        assert!(supports_images(
            ProviderKind::Nvidia,
            "qwen/qwen3.5-122b-a10b"
        ));
        assert!(!supports_images(
            ProviderKind::Nvidia,
            "meta/llama-3.1-8b-instruct"
        ));
    }
}
