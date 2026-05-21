use std::{
    ffi::OsString,
    fs::{self, OpenOptions},
    io,
    net::TcpListener,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

#[cfg(unix)]
use std::os::unix::process::CommandExt;

use tokio::time::sleep;

use crate::{
    error::{MooseError, Result},
    ollama::OllamaClient,
    platform::AppPaths,
};

pub const MANAGED_OLLAMA_BIND_ADDRESS: &str = "127.0.0.1";
pub const MANAGED_OLLAMA_DEFAULT_PORT: u16 = 11435;
pub const MANAGED_OLLAMA_RESERVED_PORT: u16 = 11434;
pub const MANAGED_OLLAMA_MIN_PORT: u16 = 1024;
pub const MANAGED_OLLAMA_HOST: &str = "127.0.0.1:11435";
pub const MANAGED_OLLAMA_BASE_URL: &str = "http://127.0.0.1:11435/api";

const READY_INITIAL_BACKOFF: Duration = Duration::from_millis(100);
const READY_MAX_BACKOFF: Duration = Duration::from_secs(1);
const READY_REQUEST_TIMEOUT: Duration = Duration::from_millis(500);
const EXISTING_LISTENER_READY_TIMEOUT: Duration = Duration::from_secs(2);
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(2);
const SHUTDOWN_POLL_INTERVAL: Duration = Duration::from_millis(50);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ManagedOllamaConfig {
    pub binary_path: PathBuf,
    pub host: String,
    pub base_url: String,
    pub models_dir: PathBuf,
    pub home_dir: PathBuf,
    pub cache_dir: PathBuf,
    pub config_dir: PathBuf,
    pub log_path: PathBuf,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ManagedOllamaServiceState {
    Stopped,
    Starting,
    Running,
    Failed(String),
}

pub struct ManagedOllamaService {
    config: ManagedOllamaConfig,
    child: Option<Child>,
    state: ManagedOllamaServiceState,
}

impl ManagedOllamaConfig {
    pub fn from_paths(paths: &AppPaths) -> Self {
        Self::from_paths_for_port(paths, MANAGED_OLLAMA_DEFAULT_PORT)
    }

    pub fn from_paths_with_port(paths: &AppPaths, port: u16) -> Result<Self> {
        Ok(Self::from_paths_for_port(
            paths,
            validate_managed_ollama_port(port)?,
        ))
    }

    fn from_paths_for_port(paths: &AppPaths, port: u16) -> Self {
        Self {
            binary_path: paths.ollama_binary_path(),
            host: managed_ollama_host_for_port(port),
            base_url: managed_ollama_base_url_for_port(port),
            models_dir: paths.ollama_models_dir(),
            home_dir: paths.data_dir().to_path_buf(),
            cache_dir: paths.cache_dir().to_path_buf(),
            config_dir: paths.config_dir().to_path_buf(),
            log_path: paths.ollama_log_path(),
        }
    }

    fn environment(&self) -> [(&'static str, OsString); 7] {
        [
            ("HOME", self.home_dir.as_os_str().to_os_string()),
            ("XDG_DATA_HOME", self.home_dir.as_os_str().to_os_string()),
            ("XDG_CACHE_HOME", self.cache_dir.as_os_str().to_os_string()),
            (
                "XDG_CONFIG_HOME",
                self.config_dir.as_os_str().to_os_string(),
            ),
            ("OLLAMA_HOST", OsString::from(&self.host)),
            ("OLLAMA_MODELS", self.models_dir.as_os_str().to_os_string()),
            (
                "OLLAMA_ORIGINS",
                OsString::from(managed_ollama_origin(&self.host)),
            ),
        ]
    }
}

impl ManagedOllamaService {
    pub fn new(paths: &AppPaths) -> Self {
        Self::with_config(ManagedOllamaConfig::from_paths(paths))
    }

    pub fn new_with_port(paths: &AppPaths, port: u16) -> Result<Self> {
        Ok(Self::with_config(
            ManagedOllamaConfig::from_paths_with_port(paths, port)?,
        ))
    }

    fn with_config(config: ManagedOllamaConfig) -> Self {
        Self {
            config,
            child: None,
            state: ManagedOllamaServiceState::Stopped,
        }
    }

    pub fn config(&self) -> &ManagedOllamaConfig {
        &self.config
    }

    pub fn state(&self) -> &ManagedOllamaServiceState {
        &self.state
    }

    pub fn ensure_started(&mut self) -> Result<()> {
        if self.child_is_running()? {
            return Ok(());
        }

        if !self.config.binary_path.is_file() {
            let error = MooseError::ManagedOllamaBinaryMissing(self.config.binary_path.clone());
            self.fail_from_error(&error);
            return Err(error);
        }

        self.prepare_runtime_paths()?;
        ensure_port_available(&self.config.host).inspect_err(|error| {
            self.fail_from_error(error);
        })?;

        let log = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.config.log_path)?;
        let stdout = log.try_clone()?;
        let mut command = Command::new(&self.config.binary_path);
        command
            .arg("serve")
            .envs(self.config.environment())
            .stdin(Stdio::null())
            .stdout(Stdio::from(stdout))
            .stderr(Stdio::from(log));

        #[cfg(unix)]
        command.process_group(0);

        match command.spawn() {
            Ok(child) => {
                self.child = Some(child);
                self.state = ManagedOllamaServiceState::Starting;
                Ok(())
            }
            Err(error) => {
                let error = MooseError::ManagedOllamaStartFailed(error.to_string());
                self.fail_from_error(&error);
                Err(error)
            }
        }
    }

    pub async fn wait_until_ready(&mut self, timeout: Duration) -> Result<()> {
        let client = OllamaClient::with_timeout(&self.config.base_url, READY_REQUEST_TIMEOUT)?;
        let deadline = Instant::now() + timeout;
        let mut backoff = READY_INITIAL_BACKOFF;

        loop {
            if self.child_exited()? {
                let error = MooseError::ManagedOllamaStartFailed(startup_exit_message(
                    &self.config.log_path,
                ));
                self.fail_from_error(&error);
                return Err(error);
            }

            if client.version().await.is_ok() {
                self.state = ManagedOllamaServiceState::Running;
                return Ok(());
            }

            let now = Instant::now();
            if now >= deadline {
                let error = MooseError::ManagedOllamaTimedOut;
                self.fail_from_error(&error);
                return Err(error);
            }

            sleep(backoff.min(deadline.saturating_duration_since(now))).await;
            backoff = (backoff * 2).min(READY_MAX_BACKOFF);
        }
    }

    pub async fn ensure_ready(&mut self, timeout: Duration) -> Result<()> {
        match self.ensure_started() {
            Ok(()) => self.wait_until_ready(timeout).await,
            Err(MooseError::ManagedOllamaPortUnavailable(host)) => {
                if self.existing_listener_is_usable().await {
                    self.state = ManagedOllamaServiceState::Running;
                    return Ok(());
                }

                let error = MooseError::ManagedOllamaPortUnavailable(host);
                self.fail_from_error(&error);
                Err(error)
            }
            Err(error) => Err(error),
        }
    }

    async fn existing_listener_is_usable(&self) -> bool {
        let Ok(client) =
            OllamaClient::with_timeout(&self.config.base_url, EXISTING_LISTENER_READY_TIMEOUT)
        else {
            return false;
        };

        client.version().await.is_ok() && client.list_models().await.is_ok()
    }

    pub fn shutdown(&mut self) {
        let Some(mut child) = self.child.take() else {
            self.state = ManagedOllamaServiceState::Stopped;
            return;
        };

        terminate_child(&mut child);
        let deadline = Instant::now() + SHUTDOWN_TIMEOUT;
        while Instant::now() < deadline {
            match child.try_wait() {
                Ok(Some(_)) => {
                    self.state = ManagedOllamaServiceState::Stopped;
                    return;
                }
                Ok(None) => thread::sleep(SHUTDOWN_POLL_INTERVAL),
                Err(_) => break,
            }
        }

        let _ = child.kill();
        let _ = child.wait();
        self.state = ManagedOllamaServiceState::Stopped;
    }

    fn prepare_runtime_paths(&self) -> Result<()> {
        fs::create_dir_all(&self.config.home_dir)?;
        fs::create_dir_all(&self.config.cache_dir)?;
        fs::create_dir_all(&self.config.config_dir)?;
        fs::create_dir_all(&self.config.models_dir)?;
        if let Some(parent) = self.config.log_path.parent() {
            fs::create_dir_all(parent)?;
        }
        Ok(())
    }

    fn child_is_running(&mut self) -> Result<bool> {
        let Some(child) = self.child.as_mut() else {
            return Ok(false);
        };

        match child.try_wait()? {
            Some(status) => {
                self.child = None;
                self.state =
                    ManagedOllamaServiceState::Failed(format!("process exited with {status}"));
                Ok(false)
            }
            None => Ok(true),
        }
    }

    fn child_exited(&mut self) -> Result<bool> {
        let Some(child) = self.child.as_mut() else {
            return Ok(false);
        };

        match child.try_wait()? {
            Some(status) => {
                self.child = None;
                self.state =
                    ManagedOllamaServiceState::Failed(format!("process exited with {status}"));
                Ok(true)
            }
            None => Ok(false),
        }
    }

    fn fail_from_error(&mut self, error: &MooseError) {
        self.state = ManagedOllamaServiceState::Failed(error.to_string());
    }
}

impl Drop for ManagedOllamaService {
    fn drop(&mut self) {
        self.shutdown();
    }
}

pub fn validate_managed_ollama_port(port: u16) -> Result<u16> {
    if port < MANAGED_OLLAMA_MIN_PORT || port == MANAGED_OLLAMA_RESERVED_PORT {
        return Err(MooseError::ManagedOllamaInvalidPort(port));
    }

    Ok(port)
}

pub fn managed_ollama_host(port: u16) -> Result<String> {
    Ok(managed_ollama_host_for_port(validate_managed_ollama_port(
        port,
    )?))
}

pub fn managed_ollama_base_url(port: u16) -> Result<String> {
    Ok(managed_ollama_base_url_for_port(
        validate_managed_ollama_port(port)?,
    ))
}

pub fn managed_ollama_port_is_available(port: u16) -> Result<bool> {
    match ensure_port_available(&managed_ollama_host(port)?) {
        Ok(()) => Ok(true),
        Err(MooseError::ManagedOllamaPortUnavailable(_)) => Ok(false),
        Err(error) => Err(error),
    }
}

fn managed_ollama_host_for_port(port: u16) -> String {
    format!("{MANAGED_OLLAMA_BIND_ADDRESS}:{port}")
}

fn managed_ollama_base_url_for_port(port: u16) -> String {
    format!("http://{}/api", managed_ollama_host_for_port(port))
}

fn managed_ollama_origin(host: &str) -> String {
    format!("http://{host}")
}

fn ensure_port_available(host: &str) -> Result<()> {
    match TcpListener::bind(host) {
        Ok(listener) => {
            drop(listener);
            Ok(())
        }
        Err(error) if error.kind() == io::ErrorKind::AddrInUse => {
            Err(MooseError::ManagedOllamaPortUnavailable(host.to_string()))
        }
        Err(error) => Err(MooseError::ManagedOllamaStartFailed(format!(
            "could not check {host}: {error}"
        ))),
    }
}

fn startup_exit_message(log_path: &Path) -> String {
    let mut message = "process exited before Ollama became ready".to_string();
    if let Some(log_excerpt) = recent_log_excerpt(log_path, 8) {
        message.push_str(": ");
        message.push_str(&log_excerpt);
    }
    message
}

fn recent_log_excerpt(path: &Path, line_count: usize) -> Option<String> {
    let content = fs::read_to_string(path).ok()?;
    let lines = content
        .lines()
        .rev()
        .filter_map(|line| {
            let line = line.trim();
            (!line.is_empty()).then_some(line)
        })
        .take(line_count)
        .collect::<Vec<_>>();

    if lines.is_empty() {
        return None;
    }

    Some(lines.into_iter().rev().collect::<Vec<_>>().join(" | "))
}

#[cfg(unix)]
fn terminate_child(child: &mut Child) {
    let pid = child.id();
    if pid <= i32::MAX as u32 {
        unsafe {
            libc::kill(-(pid as i32), libc::SIGTERM);
        }
    }
}

#[cfg(not(unix))]
fn terminate_child(child: &mut Child) {
    let _ = child.kill();
}

#[cfg(test)]
mod tests {
    use super::{ManagedOllamaConfig, managed_ollama_origin};
    use crate::platform::AppPaths;
    use std::{ffi::OsString, path::PathBuf};

    #[test]
    fn managed_ollama_origin_includes_http_scheme() {
        assert_eq!(
            managed_ollama_origin("127.0.0.1:11435"),
            "http://127.0.0.1:11435"
        );
    }

    #[test]
    fn managed_ollama_environment_uses_valid_origin() {
        let paths = AppPaths::from_base_dirs(
            PathBuf::from("/tmp/moose-data"),
            PathBuf::from("/tmp/moose-cache"),
            PathBuf::from("/tmp/moose-config"),
        );
        let config = ManagedOllamaConfig::from_paths(&paths);
        let environment = config.environment();
        let origins = environment
            .iter()
            .find_map(|(key, value)| (*key == "OLLAMA_ORIGINS").then_some(value));

        assert_eq!(origins, Some(&OsString::from("http://127.0.0.1:11435")));
    }

    #[test]
    fn managed_ollama_environment_overrides_xdg_paths() {
        let paths = AppPaths::from_base_dirs(
            PathBuf::from("/tmp/moose-data"),
            PathBuf::from("/tmp/moose-cache"),
            PathBuf::from("/tmp/moose-config"),
        );
        let config = ManagedOllamaConfig::from_paths(&paths);
        let environment = config.environment();

        assert_eq!(
            environment_value(&environment, "XDG_DATA_HOME"),
            Some(&OsString::from("/tmp/moose-data"))
        );
        assert_eq!(
            environment_value(&environment, "XDG_CACHE_HOME"),
            Some(&OsString::from("/tmp/moose-cache"))
        );
        assert_eq!(
            environment_value(&environment, "XDG_CONFIG_HOME"),
            Some(&OsString::from("/tmp/moose-config"))
        );
    }

    fn environment_value<'a>(
        environment: &'a [(&'static str, OsString)],
        key: &str,
    ) -> Option<&'a OsString> {
        environment
            .iter()
            .find_map(|(name, value)| (*name == key).then_some(value))
    }
}
