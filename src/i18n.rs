pub(crate) const fn gettext_noop(message: &str) -> &str {
    message
}

pub(crate) fn gettext(message: impl Into<String>) -> String {
    #[cfg(feature = "i18n")]
    {
        gettextrs::gettext(message)
    }
    #[cfg(not(feature = "i18n"))]
    {
        message.into()
    }
}

pub(crate) fn document_title(title: &str) -> String {
    if title == "Untitled Document" {
        gettext("Untitled Document")
    } else {
        title.to_string()
    }
}

pub(crate) fn message_text(message: &str) -> String {
    if matches!(
        message,
        crate::chat::CONTINUE_PROMPT | crate::chat::EMPTY_RESPONSE
    ) {
        gettext(message)
    } else {
        message.to_string()
    }
}

pub(crate) fn moose_error_message(error: &crate::error::MooseError) -> String {
    use crate::error::MooseError;
    match error {
        MooseError::CredentialStoreUnavailable => gettext(
            "secure credential storage is unavailable; unlock your desktop keyring or enable the Secret portal and try again",
        ),
        MooseError::MissingApiKey => {
            gettext("no API key is saved for this provider; add one in Preferences")
        }
        MooseError::InvalidApiKey => {
            gettext("invalid API key; enter the key without spaces or line breaks")
        }
        MooseError::LocalOnly => gettext(
            "this connection is blocked by Local Only mode; only Ollama managed by Moose is allowed",
        ),
        MooseError::ProviderResponse(detail) => gettext_f(
            "invalid provider response: {detail}",
            &[("detail", &detail.to_string())],
        ),
        MooseError::ResponseLimit => {
            gettext("Response incomplete: the model reached its generation limit.")
        }
        MooseError::ProviderRequest(detail) => gettext(detail),
        MooseError::ContextTooLarge => gettext(
            "The current message, instructions, attachments or tool results leave too little room for a response. Shorten the message, use fewer attachments, or increase Context Window in Chat Settings.",
        ),
        MooseError::Attachment(detail) => gettext(detail),
        MooseError::Io(detail) => {
            gettext_f("I/O failed: {detail}", &[("detail", &detail.to_string())])
        }
        MooseError::Database(detail) => gettext_f(
            "database operation failed: {detail}",
            &[("detail", &detail.to_string())],
        ),
        MooseError::Http(detail) => gettext_f(
            "HTTP request failed: {detail}",
            &[("detail", &detail.to_string())],
        ),
        MooseError::StreamStalled { seconds } => ngettext_f(
            "generation stream stalled after {seconds} second",
            "generation stream stalled after {seconds} seconds",
            u32::try_from(*seconds).unwrap_or(u32::MAX),
            &[("seconds", &seconds.to_string())],
        ),
        MooseError::HttpStatus { status, message } => gettext_f(
            "HTTP request returned status {status}: {message}",
            &[
                ("status", &status.to_string()),
                ("message", &message.to_string()),
            ],
        ),
        MooseError::Json(detail) => gettext_f(
            "JSON parsing failed: {detail}",
            &[("detail", &detail.to_string())],
        ),
        MooseError::Url(detail) => gettext_f(
            "URL parsing failed: {detail}",
            &[("detail", &detail.to_string())],
        ),
        MooseError::Utf8(detail) => gettext_f(
            "stream text decoding failed: {detail}",
            &[("detail", &detail.to_string())],
        ),
        MooseError::MissingHomeDirectory => gettext("missing home directory"),
        MooseError::InvalidProviderUrl => gettext("invalid provider URL"),
        MooseError::InvalidProviderName => gettext("invalid provider name"),
        MooseError::InvalidModelName => gettext("invalid model name"),
        MooseError::InvalidConversationTitle => gettext("invalid conversation title"),
        MooseError::InvalidMessageContent => gettext("invalid message content"),
        MooseError::InvalidMessageRole => gettext("invalid message role"),
        MooseError::InvalidMessageStatus => gettext("invalid message status"),
        MooseError::InvalidIdentifier => gettext("invalid identifier"),
        MooseError::InvalidGenerationSettings => gettext("invalid generation settings"),
        MooseError::InvalidProfileName => gettext("invalid profile name"),
        MooseError::InvalidProfileDescription => gettext("invalid profile description"),
        MooseError::InvalidSystemPrompt => gettext("invalid system prompt"),
        MooseError::InvalidDownloadJobStatus => gettext("invalid download job status"),
        MooseError::InvalidDownloadJobProgress => gettext("invalid download job progress"),
        MooseError::ConversationNotFound => gettext("conversation was not found"),
        MooseError::MessageNotFound => gettext("message was not found"),
        MooseError::GenerationSettingsNotFound => gettext("generation settings were not found"),
        MooseError::ProfileNotFound => gettext("profile was not found"),
        MooseError::ProfileNameAlreadyExists => gettext("a profile with that name already exists"),
        MooseError::BuiltinProfileCannotBeDeleted => gettext("built-in profiles cannot be deleted"),
        MooseError::DownloadJobNotFound => gettext("download job was not found"),
        MooseError::ProviderNotConfigured => gettext("no provider is configured"),
        MooseError::InvalidOllamaResponse(detail) => gettext_f(
            "invalid Ollama response: {detail}",
            &[("detail", &detail.to_string())],
        ),
        MooseError::OllamaUpdateRequired(detail) => gettext_f(
            "Ollama must be updated to download this model: {detail}",
            &[("detail", &detail.to_string())],
        ),
        MooseError::ManagedOllamaUpdate(detail) => gettext_f(
            "managed Ollama update failed: {detail}",
            &[("detail", &detail.to_string())],
        ),
        MooseError::ManagedOllamaCancelled => gettext("managed Ollama installation was cancelled"),
        MooseError::ManagedOllamaUnsupportedArchitecture(detail) => gettext_f(
            "managed Ollama does not support architecture {detail}",
            &[("detail", &detail.to_string())],
        ),
        MooseError::ManagedOllamaManifestInvalid(detail) => gettext_f(
            "managed Ollama manifest is invalid: {detail}",
            &[("detail", &detail.to_string())],
        ),
        MooseError::ManagedOllamaDownloadFailed(detail) => gettext_f(
            "managed Ollama download failed: {detail}",
            &[("detail", &detail.to_string())],
        ),
        MooseError::ManagedOllamaChecksumMismatch { expected, actual } => gettext_f(
            "managed Ollama checksum mismatch: expected {expected}, got {actual}",
            &[
                ("expected", &expected.to_string()),
                ("actual", &actual.to_string()),
            ],
        ),
        MooseError::ManagedOllamaExtractionFailed(detail) => gettext_f(
            "managed Ollama extraction failed: {detail}",
            &[("detail", &detail.to_string())],
        ),
        MooseError::ManagedOllamaBinaryMissing(detail) => gettext_f(
            "managed Ollama binary is missing at {detail}",
            &[("detail", &detail.display().to_string())],
        ),
        MooseError::ManagedOllamaUnavailable => gettext("managed Ollama is unavailable"),
        MooseError::ManagedOllamaStartFailed(detail) => gettext_f(
            "managed Ollama failed to start: {detail}",
            &[("detail", &detail.to_string())],
        ),
        MooseError::ManagedOllamaTimedOut => gettext("managed Ollama did not become ready in time"),
        MooseError::ManagedOllamaInUse => gettext(
            "managed Ollama is already running in another Moose instance; close that instance and retry",
        ),
        MooseError::ManagedOllamaPortUnavailable(detail) => gettext_f(
            "managed Ollama port is already in use: {detail}; close any previous Moose instances or choose another managed port",
            &[("detail", &detail.to_string())],
        ),
        MooseError::ManagedOllamaInvalidPort(detail) => gettext_f(
            "managed Ollama port {detail} is invalid; use 1024-65535 except 11434",
            &[("detail", &detail.to_string())],
        ),
        MooseError::NumericConversion(detail) => gettext_f(
            "numeric conversion failed: {detail}",
            &[("detail", &detail.to_string())],
        ),
    }
}

pub(crate) fn gettext_f(message: &str, args: &[(&str, &str)]) -> String {
    format_message(&gettext(message), args)
}

pub(crate) fn ngettext_f(
    singular: &str,
    plural: &str,
    count: u32,
    args: &[(&str, &str)],
) -> String {
    {
        #[cfg(feature = "i18n")]
        let message = gettextrs::ngettext(singular, plural, count);
        #[cfg(not(feature = "i18n"))]
        let message = if count == 1 { singular } else { plural }.to_string();
        format_message(&message, args)
    }
}

fn format_message(message: &str, args: &[(&str, &str)]) -> String {
    let mut result = String::with_capacity(message.len());
    let mut chars = message.chars().peekable();
    while let Some(character) = chars.next() {
        match character {
            '{' if chars.peek() == Some(&'{') => {
                chars.next();
                result.push('{');
            }
            '}' if chars.peek() == Some(&'}') => {
                chars.next();
                result.push('}');
            }
            '{' => {
                let mut key = String::new();
                let mut closed = false;
                for character in chars.by_ref() {
                    if character == '}' {
                        closed = true;
                        break;
                    }
                    key.push(character);
                }
                if closed && let Some((_, value)) = args.iter().find(|(name, _)| *name == key) {
                    result.push_str(value);
                } else {
                    result.push('{');
                    result.push_str(&key);
                    if closed {
                        result.push('}');
                    }
                }
            }
            _ => result.push(character),
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::format_message;

    #[test]
    fn formats_reordered_and_repeated_placeholders() {
        assert_eq!(
            format_message(
                "{total}: {name}, {name}",
                &[("name", "Moose"), ("total", "3")]
            ),
            "3: Moose, Moose"
        );
    }

    #[test]
    fn preserves_placeholder_text_inside_values() {
        assert_eq!(
            format_message("{name}: {total}", &[("name", "{total}"), ("total", "3")]),
            "{total}: 3"
        );
    }

    #[test]
    fn handles_literal_braces_and_incomplete_placeholders() {
        assert_eq!(
            format_message("{{value}} {missing} {open", &[]),
            "{value} {missing} {open"
        );
    }
}

#[cfg(feature = "gui")]
pub(crate) use runtime::*;

#[cfg(feature = "gui")]
mod runtime {
    use super::gettext;
    use gettextrs::{
        LocaleCategory, bind_textdomain_codeset, bindtextdomain, setlocale, textdomain,
    };

    const DOMAIN: &str = "moose";

    pub(crate) fn init() {
        unsafe { setlocale(LocaleCategory::LcAll, "") };
        let locale_dir = option_env!("MOOSE_LOCALEDIR").unwrap_or("/usr/share/locale");
        bindtextdomain(DOMAIN, locale_dir).expect("Unable to bind translation domain");
        bind_textdomain_codeset(DOMAIN, "UTF-8").expect("Unable to set translation encoding");
        textdomain(DOMAIN).expect("Unable to select translation domain");
    }

    pub(crate) fn error_message(error: &(impl std::fmt::Display + ?Sized)) -> String {
        gettext(error.to_string())
    }

    pub(crate) fn web_description(config: &crate::tools::web::WebConfig) -> String {
        config.description()
    }

    pub(crate) fn profile_name(profile: &crate::profiles::ChatProfile) -> String {
        if !profile.is_builtin {
            return profile.name.clone();
        }
        match profile.id.as_str() {
            "builtin-general" => gettext("General"),
            "builtin-code" => gettext("Code"),
            "builtin-writing" => gettext("Writing"),
            "builtin-translation" => gettext("Translation"),
            "builtin-reasoning" => gettext("Reasoning"),
            _ => profile.name.clone(),
        }
    }

    pub(crate) fn profile_description(profile: &crate::profiles::ChatProfile) -> String {
        if !profile.is_builtin {
            return profile.description.clone();
        }
        match profile.id.as_str() {
            "builtin-general" => gettext("Everyday questions, explanations and practical help"),
            "builtin-code" => gettext("Programming, debugging and technical explanations"),
            "builtin-writing" => gettext("Drafting, rewriting, ideas and tone"),
            "builtin-translation" => gettext("Translation and language adaptation"),
            "builtin-reasoning" => gettext("Complex problems, analysis and decisions"),
            _ => profile.description.clone(),
        }
    }

    pub(crate) fn profile_system_prompt(profile: &crate::profiles::ChatProfile) -> String {
        if !profile.is_builtin {
            return profile.system_prompt.clone();
        }
        match profile.id.as_str() {
            "builtin-general" => gettext(
                "Be clear, helpful and direct. Give practical answers and explain important details without unnecessary complexity.",
            ),
            "builtin-code" => gettext(
                "Respond with technical precision. Ask for missing context when it affects the solution, prioritize verifiable approaches and clearly identify assumptions.",
            ),
            "builtin-writing" => gettext(
                "Help write with clarity, purpose and a consistent style. Preserve the intended voice while improving structure and wording.",
            ),
            "builtin-translation" => gettext(
                "Translate while preserving meaning, tone, formatting and relevant cultural context. Flag genuine ambiguities instead of silently guessing.",
            ),
            "builtin-reasoning" => gettext(
                "Analyze carefully, break complex problems into useful parts and distinguish conclusions from assumptions and uncertainty.",
            ),
            _ => profile.system_prompt.clone(),
        }
    }

    pub(crate) fn profile_prompt_value(
        profile: &crate::profiles::ChatProfile,
        displayed: &str,
    ) -> String {
        if profile.is_builtin && displayed == profile_system_prompt(profile) {
            profile.system_prompt.clone()
        } else {
            displayed.to_string()
        }
    }

    pub(crate) fn conversation_title(title: &str) -> String {
        if title == crate::conversations::DEFAULT_CONVERSATION_TITLE {
            gettext("New Conversation")
        } else {
            title.to_string()
        }
    }

    pub(crate) fn thinking_label(value: &crate::chat::ThinkingValue) -> String {
        use crate::chat::ThinkingValue;
        match value {
            ThinkingValue::Enabled(true) => gettext("On"),
            ThinkingValue::Enabled(false) => gettext("Off"),
            ThinkingValue::Level(level) => match level.as_str() {
                "none" => gettext("None"),
                "minimal" => gettext("Minimal"),
                "low" => gettext("Low"),
                "medium" => gettext("Medium"),
                "high" => gettext("High"),
                "xhigh" => gettext("Extra High"),
                _ => value.label(),
            },
        }
    }
}
