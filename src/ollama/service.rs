use std::{
    ffi::OsString,
    fs::{self, File, OpenOptions, TryLockError},
    io,
    net::TcpListener,
    path::{Path, PathBuf},
    process::{Child, Command, ExitStatus, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
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
    pub gpu: ManagedOllamaGpuConfig,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ManagedOllamaGpuConfig {
    pub vulkan: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ManagedOllamaAcceleration {
    Unknown,
    Cpu,
    Vulkan,
    Rocm,
    Cuda,
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
    shutdown: ManagedOllamaShutdown,
    process_lock: Option<File>,
}

#[derive(Clone, Default)]
pub struct ManagedOllamaShutdown(Arc<AtomicBool>);

impl ManagedOllamaShutdown {
    pub fn request(&self) -> bool {
        !self.0.swap(true, Ordering::AcqRel)
    }
}

impl ManagedOllamaConfig {
    pub fn from_paths(paths: &AppPaths) -> Self {
        Self::from_paths_for_port(
            paths,
            MANAGED_OLLAMA_DEFAULT_PORT,
            ManagedOllamaGpuConfig::default(),
        )
    }

    pub fn from_paths_with_port(paths: &AppPaths, port: u16) -> Result<Self> {
        Ok(Self::from_paths_for_port(
            paths,
            validate_managed_ollama_port(port)?,
            ManagedOllamaGpuConfig::default(),
        ))
    }

    pub fn from_paths_with_gpu(paths: &AppPaths, gpu: ManagedOllamaGpuConfig) -> Self {
        Self::from_paths_for_port(paths, MANAGED_OLLAMA_DEFAULT_PORT, gpu)
    }

    pub fn from_paths_with_port_and_gpu(
        paths: &AppPaths,
        port: u16,
        gpu: ManagedOllamaGpuConfig,
    ) -> Result<Self> {
        Ok(Self::from_paths_for_port(
            paths,
            validate_managed_ollama_port(port)?,
            gpu,
        ))
    }

    fn from_paths_for_port(paths: &AppPaths, port: u16, gpu: ManagedOllamaGpuConfig) -> Self {
        Self {
            binary_path: paths.ollama_binary_path(),
            host: managed_ollama_host_for_port(port),
            base_url: managed_ollama_base_url_for_port(port),
            models_dir: paths.ollama_models_dir(),
            home_dir: paths.data_dir().to_path_buf(),
            cache_dir: paths.cache_dir().to_path_buf(),
            config_dir: paths.config_dir().to_path_buf(),
            log_path: paths.ollama_log_path(),
            gpu,
        }
    }

    fn environment(&self) -> Vec<(&'static str, OsString)> {
        let mut environment = vec![
            ("HOME", self.home_dir.as_os_str().to_os_string()),
            ("XDG_DATA_HOME", self.home_dir.as_os_str().to_os_string()),
            ("XDG_CACHE_HOME", self.cache_dir.as_os_str().to_os_string()),
            (
                "XDG_CONFIG_HOME",
                self.config_dir.as_os_str().to_os_string(),
            ),
            ("OLLAMA_HOST", OsString::from(&self.host)),
            ("OLLAMA_NO_CLOUD", OsString::from("1")),
            ("OLLAMA_MODELS", self.models_dir.as_os_str().to_os_string()),
            (
                "OLLAMA_ORIGINS",
                OsString::from(managed_ollama_origin(&self.host)),
            ),
        ];
        environment.push((
            "OLLAMA_VULKAN",
            OsString::from(if self.gpu.vulkan { "1" } else { "0" }),
        ));
        environment
    }
}

impl Default for ManagedOllamaGpuConfig {
    fn default() -> Self {
        Self { vulkan: true }
    }
}

impl ManagedOllamaAcceleration {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Unknown => "Unknown",
            Self::Cpu => "CPU",
            Self::Vulkan => "Vulkan",
            Self::Rocm => "ROCm",
            Self::Cuda => "CUDA",
        }
    }
}

impl ManagedOllamaService {
    pub fn new(paths: &AppPaths) -> Self {
        Self::with_config(ManagedOllamaConfig::from_paths(paths))
    }

    pub fn new_with_gpu(paths: &AppPaths, gpu: ManagedOllamaGpuConfig) -> Self {
        Self::with_config(ManagedOllamaConfig::from_paths_with_gpu(paths, gpu))
    }

    pub fn new_with_port(paths: &AppPaths, port: u16) -> Result<Self> {
        Ok(Self::with_config(
            ManagedOllamaConfig::from_paths_with_port(paths, port)?,
        ))
    }

    pub fn new_with_port_and_gpu(
        paths: &AppPaths,
        port: u16,
        gpu: ManagedOllamaGpuConfig,
    ) -> Result<Self> {
        Ok(Self::with_config(
            ManagedOllamaConfig::from_paths_with_port_and_gpu(paths, port, gpu)?,
        ))
    }

    fn with_config(config: ManagedOllamaConfig) -> Self {
        Self {
            config,
            child: None,
            state: ManagedOllamaServiceState::Stopped,
            shutdown: ManagedOllamaShutdown::default(),
            process_lock: None,
        }
    }

    pub fn config(&self) -> &ManagedOllamaConfig {
        &self.config
    }

    pub fn state(&self) -> &ManagedOllamaServiceState {
        &self.state
    }

    pub fn shutdown_handle(&self) -> ManagedOllamaShutdown {
        self.shutdown.clone()
    }

    pub fn reconfigure(&mut self, config: ManagedOllamaConfig) -> Result<()> {
        self.check_shutdown()?;
        if self.config != config {
            self.shutdown();
            self.config = config;
        }
        Ok(())
    }

    pub fn acceleration(&self) -> ManagedOllamaAcceleration {
        detect_managed_ollama_acceleration(&self.config.log_path)
    }

    pub fn ensure_started(&mut self) -> Result<()> {
        self.check_shutdown()?;
        if self.child_is_running()? {
            return Ok(());
        }

        self.prepare_runtime_paths()?;
        let process_lock = acquire_process_lock(&self.config.home_dir).inspect_err(|error| {
            self.fail_from_error(error);
        })?;
        self.recover_installation().inspect_err(|error| {
            self.fail_from_error(error);
        })?;
        self.start_with_process_lock(process_lock)
    }

    pub(super) fn recover_installation(&self) -> Result<()> {
        let paths = AppPaths::from_base_dirs(
            self.config.home_dir.clone(),
            self.config.cache_dir.clone(),
            self.config.config_dir.clone(),
        );
        super::manager::transaction::recover(&paths)
    }

    pub(super) fn stop_for_update(&mut self) -> Result<File> {
        self.check_shutdown()?;
        self.prepare_runtime_paths()?;
        let lock = match self.process_lock.take() {
            Some(lock) => lock,
            None => acquire_process_lock(&self.config.home_dir)?,
        };
        self.shutdown();
        Ok(lock)
    }

    pub(super) fn start_with_process_lock(&mut self, process_lock: File) -> Result<()> {
        self.check_shutdown()?;
        if !self.config.binary_path.is_file() {
            let error = MooseError::ManagedOllamaBinaryMissing(self.config.binary_path.clone());
            self.fail_from_error(&error);
            return Err(error);
        }
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
        configure_child(&mut command, &process_lock);

        match command.spawn() {
            Ok(child) => {
                self.child = Some(child);
                self.process_lock = Some(process_lock);
                self.state = ManagedOllamaServiceState::Starting;
                crate::logging::record(
                    "INFO",
                    "ollama.start",
                    &format!("Starting managed Ollama at {}", self.config.base_url),
                );
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
            self.check_shutdown()?;
            if self.child.is_none() || self.child_exited()? {
                let error = MooseError::ManagedOllamaStartFailed(startup_exit_message(
                    &self.config.log_path,
                ));
                self.fail_from_error(&error);
                return Err(error);
            }

            if client.version().await.is_ok() {
                self.check_shutdown()?;
                if self.child_exited()? {
                    continue;
                }
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
        self.ensure_started()?;
        if let Err(error) = self.wait_until_ready(timeout).await {
            self.shutdown();
            self.fail_from_error(&error);
            return Err(error);
        }
        Ok(())
    }

    pub fn shutdown(&mut self) {
        let Some(mut child) = self.child.take() else {
            self.process_lock = None;
            self.state = ManagedOllamaServiceState::Stopped;
            return;
        };

        terminate_child(&mut child);
        let deadline = Instant::now() + SHUTDOWN_TIMEOUT;
        while Instant::now() < deadline {
            match reap_exited_child(&mut child) {
                Ok(Some(_)) => {
                    self.process_lock = None;
                    self.state = ManagedOllamaServiceState::Stopped;
                    return;
                }
                Ok(None) => thread::sleep(SHUTDOWN_POLL_INTERVAL),
                Err(_) => break,
            }
        }

        force_terminate_child(&mut child);
        let _ = child.wait();
        self.process_lock = None;
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

    fn check_shutdown(&self) -> Result<()> {
        if self.shutdown.0.load(Ordering::Acquire) {
            return Err(MooseError::ManagedOllamaStartFailed(
                "Moose is shutting down".into(),
            ));
        }
        Ok(())
    }

    fn child_is_running(&mut self) -> Result<bool> {
        let Some(child) = self.child.as_mut() else {
            return Ok(false);
        };

        match reap_exited_child(child)? {
            Some(status) => {
                self.child = None;
                self.process_lock = None;
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

        match reap_exited_child(child)? {
            Some(status) => {
                self.child = None;
                self.process_lock = None;
                self.state =
                    ManagedOllamaServiceState::Failed(format!("process exited with {status}"));
                Ok(true)
            }
            None => Ok(false),
        }
    }

    fn fail_from_error(&mut self, error: &MooseError) {
        crate::logging::record("ERROR", "ollama.service", &error.to_string());
        self.state = ManagedOllamaServiceState::Failed(error.to_string());
    }
}

pub fn detect_managed_ollama_acceleration(log_path: &Path) -> ManagedOllamaAcceleration {
    let Ok(content) = fs::read_to_string(log_path) else {
        return ManagedOllamaAcceleration::Unknown;
    };

    for line in content.lines().rev().map(str::to_ascii_lowercase) {
        if line.contains("library=cuda") {
            return ManagedOllamaAcceleration::Cuda;
        }
        if line.contains("library=rocm") {
            return ManagedOllamaAcceleration::Rocm;
        }
        if line.contains("library=vulkan") {
            return ManagedOllamaAcceleration::Vulkan;
        }
        if line.contains("library=cpu") {
            return ManagedOllamaAcceleration::Cpu;
        }
    }

    ManagedOllamaAcceleration::Unknown
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

pub(super) fn acquire_process_lock(home_dir: &Path) -> Result<File> {
    let mut options = OpenOptions::new();
    options.create(true).truncate(false).read(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    let file = options.open(home_dir.join("ollama.lock"))?;
    match file.try_lock() {
        Ok(()) => Ok(file),
        Err(TryLockError::WouldBlock) => Err(MooseError::ManagedOllamaInUse),
        Err(TryLockError::Error(error)) => Err(MooseError::ManagedOllamaStartFailed(format!(
            "could not lock the managed Ollama installation: {error}"
        ))),
    }
}

#[cfg(unix)]
fn configure_child(command: &mut Command, process_lock: &File) {
    use std::os::fd::AsRawFd;

    let lock_fd = process_lock.as_raw_fd();
    #[cfg(target_os = "linux")]
    let parent_pid = unsafe { libc::getpid() };
    command.process_group(0);
    unsafe {
        command.pre_exec(move || {
            #[cfg(target_os = "linux")]
            {
                if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM) == -1 {
                    return Err(io::Error::last_os_error());
                }
                if libc::getppid() != parent_pid {
                    return Err(io::Error::from(io::ErrorKind::Interrupted));
                }
            }
            let flags = libc::fcntl(lock_fd, libc::F_GETFD);
            if flags == -1 || libc::fcntl(lock_fd, libc::F_SETFD, flags & !libc::FD_CLOEXEC) == -1 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        });
    }
}

#[cfg(unix)]
fn signal_child_group(child: &Child, signal: libc::c_int) {
    let pid = child.id();
    if pid <= i32::MAX as u32 {
        unsafe {
            libc::kill(-(pid as i32), signal);
        }
    }
}

#[cfg(unix)]
fn terminate_child(child: &mut Child) {
    signal_child_group(child, libc::SIGTERM);
}

fn force_terminate_child(child: &mut Child) {
    #[cfg(unix)]
    signal_child_group(child, libc::SIGKILL);
    let _ = child.kill();
}

#[cfg(target_os = "linux")]
fn reap_exited_child(child: &mut Child) -> io::Result<Option<ExitStatus>> {
    let mut status = unsafe { std::mem::zeroed::<libc::siginfo_t>() };
    loop {
        let result = unsafe {
            libc::waitid(
                libc::P_PID,
                child.id(),
                &mut status,
                libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
            )
        };
        if result != -1 {
            break;
        }
        let error = io::Error::last_os_error();
        if error.kind() != io::ErrorKind::Interrupted {
            return Err(error);
        }
    }
    if unsafe { status.si_pid() } == 0 {
        return Ok(None);
    }
    signal_child_group(child, libc::SIGKILL);
    child.wait().map(Some)
}

#[cfg(not(target_os = "linux"))]
fn reap_exited_child(child: &mut Child) -> io::Result<Option<ExitStatus>> {
    child.try_wait()
}

#[cfg(not(unix))]
fn terminate_child(child: &mut Child) {
    let _ = child.kill();
}

#[cfg(test)]
mod tests {
    use super::{ManagedOllamaAcceleration, ManagedOllamaConfig, managed_ollama_origin};
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
    fn managed_ollama_environment_enables_vulkan_by_default() {
        let paths = AppPaths::from_base_dirs(
            PathBuf::from("/tmp/moose-data"),
            PathBuf::from("/tmp/moose-cache"),
            PathBuf::from("/tmp/moose-config"),
        );
        let config = ManagedOllamaConfig::from_paths(&paths);
        let environment = config.environment();

        assert_eq!(
            environment_value(&environment, "OLLAMA_VULKAN"),
            Some(&OsString::from("1"))
        );
    }

    #[test]
    fn managed_ollama_acceleration_detects_latest_backend_line() {
        let path =
            std::env::temp_dir().join(format!("moose-ollama-log-{}.txt", std::process::id()));
        std::fs::write(
            &path,
            "msg=\"loaded runner\" library=cpu\nmsg=\"loaded runner\" library=Vulkan\n",
        )
        .unwrap();

        assert_eq!(
            super::detect_managed_ollama_acceleration(&path),
            ManagedOllamaAcceleration::Vulkan
        );

        let _ = std::fs::remove_file(path);
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
