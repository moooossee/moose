use crate::providers::validate_model_name;

#[derive(Clone, Debug)]
pub struct ModelDownload {
    pub name: String,
    pub hugging_face_repository: Option<String>,
}

impl ModelDownload {
    pub fn parse(value: &str) -> Result<Self, &'static str> {
        let value = value.trim();
        if value.is_empty() {
            return Err("Enter a model name or link.");
        }

        let mut words = value.split_whitespace();
        let first = words.next().ok_or("Enter a model name or link.")?;
        let value = if first == "ollama" {
            if !matches!(words.next(), Some("run" | "pull")) {
                return Err("Paste an ollama run or ollama pull command.");
            }
            words.next().ok_or("The command needs a model name.")?
        } else {
            first
        };
        if words.next().is_some() {
            return Err("Enter one model, without extra command arguments.");
        }
        let value = value
            .strip_prefix('"')
            .and_then(|value| value.strip_suffix('"'))
            .or_else(|| {
                value
                    .strip_prefix('\'')
                    .and_then(|value| value.strip_suffix('\''))
            })
            .unwrap_or(value);

        let name = if value.contains("://") {
            reference_from_url(value)?
        } else if let Some((host, _)) = value.split_once('/') {
            if host.eq_ignore_ascii_case("hf.co")
                || host.eq_ignore_ascii_case("huggingface.co")
                || host.eq_ignore_ascii_case("ollama.com")
            {
                reference_from_url(&format!("https://{value}"))?
            } else {
                value.to_string()
            }
        } else {
            if ["hf.co", "huggingface.co", "ollama.com"]
                .iter()
                .any(|host| value.eq_ignore_ascii_case(host))
            {
                return Err("Paste the link to a specific model repository.");
            }
            value.to_string()
        };

        if name.len() > 256 {
            return Err("The model reference is too long.");
        }
        if name.split('/').any(|part| matches!(part, "" | "." | "..")) {
            return Err("Enter a complete model name or repository link.");
        }
        if name.starts_with('-') || name.starts_with(':') || name.ends_with(':') {
            return Err("Enter a model name, with an optional tag after :.");
        }
        let name = validate_model_name(&name)
            .map_err(|_| "Check the model name or paste a repository link.")?;
        let hugging_face_repository = hugging_face_repository(&name).map(str::to_string);
        Ok(Self {
            name,
            hugging_face_repository,
        })
    }

    pub fn with_variant(mut self, value: &str) -> Result<Self, &'static str> {
        let value = value.trim();
        if self.hugging_face_repository.is_none() || value.is_empty() {
            return Ok(self);
        }
        if !valid_component(value) {
            return Err("Enter a quantization such as Q4_K_M, or a GGUF filename.");
        }
        let base = self
            .name
            .split_once(':')
            .map_or(self.name.as_str(), |(base, _)| base);
        self.name = format!("{base}:{value}");
        if self.name.len() > 256 {
            return Err("The model reference is too long.");
        }
        Ok(self)
    }

    pub fn variant(&self) -> Option<&str> {
        self.hugging_face_repository
            .as_ref()
            .and_then(|_| self.name.split_once(':').map(|(_, tag)| tag))
    }
}

pub fn hugging_face_repository(name: &str) -> Option<&str> {
    let (host, path) = name.split_once('/')?;
    if !host.eq_ignore_ascii_case("hf.co") && !host.eq_ignore_ascii_case("huggingface.co") {
        return None;
    }
    let repository = path
        .split_once(':')
        .map_or(path, |(repository, _)| repository);
    let (owner, name) = repository.split_once('/')?;
    (valid_component(owner) && valid_component(name)).then_some(repository)
}

pub fn model_names_match(left: &str, right: &str) -> bool {
    let normalize = |value: &str| {
        let value = value
            .rsplit_once(':')
            .filter(|(_, tag)| tag.eq_ignore_ascii_case("latest"))
            .map_or(value, |(name, _)| name);
        if hugging_face_repository(value).is_some() {
            format!(
                "hf.co/{}",
                value.split_once('/').map_or(value, |(_, path)| path)
            )
        } else {
            value.to_string()
        }
    };
    normalize(left).eq_ignore_ascii_case(&normalize(right))
}

fn reference_from_url(value: &str) -> Result<String, &'static str> {
    let url = url::Url::parse(value).map_err(|_| "Check the model link.")?;
    if !matches!(url.scheme(), "https" | "http")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
    {
        return Err("Use an Ollama or Hugging Face repository link.");
    }
    match url.host_str() {
        Some("hf.co" | "huggingface.co") => hugging_face_reference(url.path()),
        Some("ollama.com") => ollama_reference(url.path()),
        _ => Err("Use an Ollama or Hugging Face repository link."),
    }
}

fn hugging_face_reference(path: &str) -> Result<String, &'static str> {
    let path = path.trim_matches('/');
    let (repository, variant) = path
        .split_once(':')
        .map_or((path, None), |(repository, variant)| {
            (repository, Some(variant))
        });
    let (owner, name) = repository
        .split_once('/')
        .ok_or("Include both the Hugging Face owner and repository name.")?;
    if name.contains('/') {
        return Err("Paste the repository link, without a branch or file path.");
    }
    if !valid_component(owner) || !valid_component(name) {
        return Err("Check the Hugging Face owner and repository name.");
    }
    if variant.is_some_and(|variant| !valid_component(variant)) {
        return Err("Enter a quantization such as Q4_K_M, or a GGUF filename after :.");
    }
    Ok(format!("hf.co/{path}"))
}

fn ollama_reference(path: &str) -> Result<String, &'static str> {
    let path = path.trim_matches('/');
    let path = path.strip_prefix("library/").unwrap_or(path);
    if path.is_empty() {
        return Err("Paste the link to a specific Ollama model.");
    }
    Ok(path.to_string())
}

fn valid_component(value: &str) -> bool {
    !value.is_empty()
        && !matches!(value, "." | "..")
        && value.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-' | '+' | '~')
        })
}
