use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
};

use crate::platform::AppPaths;

static LOGGER: OnceLock<Mutex<LogWriter>> = OnceLock::new();
const MAX_LOG_BYTES: u64 = 1024 * 1024;
const MAX_RECENT_BYTES: u64 = 128 * 1024;

struct LogWriter {
    path: PathBuf,
    file: Option<File>,
    size: u64,
    limit: u64,
}

impl LogWriter {
    fn new(path: PathBuf, limit: u64) -> std::io::Result<Self> {
        let file = open_log(&path)?;
        let size = file.metadata()?.len();
        Ok(Self {
            path,
            file: Some(file),
            size,
            limit,
        })
    }

    fn write(&mut self, line: &str) -> std::io::Result<()> {
        if self.size.saturating_add(line.len() as u64) > self.limit {
            self.file.take();
            let previous = self.path.with_extension("log.1");
            match fs::rename(&previous, self.path.with_extension("log.2")) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
            fs::rename(&self.path, previous)?;
            self.file = Some(open_log(&self.path)?);
            self.size = 0;
        }
        if self.file.is_none() {
            self.file = Some(open_log(&self.path)?);
            self.size = self.file.as_ref().unwrap().metadata()?.len();
        }
        let file = self.file.as_mut().unwrap();
        file.write_all(line.as_bytes())?;
        file.flush()?;
        self.size += line.len() as u64;
        Ok(())
    }
}

fn open_log(path: &Path) -> std::io::Result<File> {
    let mut options = OpenOptions::new();
    options.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    options.open(path)
}

pub fn init(paths: &AppPaths) {
    let result = fs::create_dir_all(paths.cache_dir())
        .and_then(|()| LogWriter::new(paths.app_log_path(), MAX_LOG_BYTES));
    match result {
        Ok(writer) => {
            let _ = LOGGER.set(Mutex::new(writer));
        }
        Err(error) => eprintln!("Could not initialize Moose logging: {error}"),
    }
    record(
        "INFO",
        "application.start",
        concat!("Moose ", env!("CARGO_PKG_VERSION")),
    );
}

pub fn record(level: &str, event: &str, detail: &str) {
    let detail = redact(detail).chars().take(32 * 1024).collect::<String>();
    let line = serde_json::json!({
        "time": crate::core::utc_now(),
        "level": level,
        "event": event,
        "detail": detail,
    })
    .to_string()
        + "\n";
    eprint!("{line}");
    if let Some(logger) = LOGGER.get()
        && let Ok(mut writer) = logger.lock()
        && let Err(error) = writer.write(&line)
    {
        eprintln!("Could not write Moose log: {error}");
    }
}

pub fn recent(path: &Path) -> std::io::Result<String> {
    let mut file = File::open(path)?;
    let size = file.metadata()?.len();
    let offset = size.saturating_sub(MAX_RECENT_BYTES);
    file.seek(SeekFrom::Start(offset))?;
    let mut bytes = Vec::new();
    file.take(MAX_RECENT_BYTES).read_to_end(&mut bytes)?;
    let text = String::from_utf8_lossy(&bytes);
    let text = if offset > 0 {
        text.split_once('\n').map(|(_, rest)| rest).unwrap_or("")
    } else {
        &text
    };
    Ok(redact(text))
}

pub fn redact(input: &str) -> String {
    input
        .lines()
        .map(|line| {
            let lowercase = line.to_ascii_lowercase();
            if [
                "authorization",
                "bearer ",
                "api_key",
                "api-key",
                "access_token",
                "password",
                "sk-",
            ]
            .iter()
            .any(|key| lowercase.contains(key))
            {
                "[Sensitive log line omitted]".to_string()
            } else {
                redact_urls(line)
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn redact_urls(mut remaining: &str) -> String {
    let mut output = String::new();
    while let Some(start) = remaining
        .find("http://")
        .into_iter()
        .chain(remaining.find("https://"))
        .min()
    {
        output.push_str(&remaining[..start]);
        remaining = &remaining[start..];
        let end = remaining
            .find(|character: char| {
                character.is_whitespace() || matches!(character, '"' | '\'' | '<' | '>')
            })
            .unwrap_or(remaining.len());
        match url::Url::parse(&remaining[..end]) {
            Ok(mut url) => {
                let _ = url.set_username("");
                let _ = url.set_password(None);
                url.set_query(None);
                url.set_fragment(None);
                output.push_str(url.as_str());
            }
            Err(_) => output.push_str("[Invalid URL omitted]"),
        }
        remaining = &remaining[end..];
    }
    output.push_str(remaining);
    output
}
