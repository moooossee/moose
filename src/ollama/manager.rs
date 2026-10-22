use std::{
    collections::{BTreeMap, HashSet},
    fs::{self, File, OpenOptions},
    io::{self, Read},
    path::{Component, Path, PathBuf},
};

#[cfg(unix)]
use std::os::unix::{fs as unix_fs, fs::PermissionsExt};

use serde::{Deserialize, Serialize};
use tar::EntryType;

use crate::{
    core::{new_id, utc_now},
    error::{MooseError, Result},
    platform::AppPaths,
};

mod download;
pub(super) mod transaction;

use super::update::InstallCancellation;

const BUNDLED_MANAGED_OLLAMA_MANIFEST: &str =
    include_str!("../../data/managed_ollama_manifest.json");
const INSTALLATION_METADATA_FILE: &str = "managed_ollama_installation.json";
const OLLAMA_BINARY_RELATIVE_PATH: &str = "bin/ollama";

#[derive(Clone)]
pub struct ManagedOllamaManager {
    paths: AppPaths,
    client: reqwest::Client,
    manifest: ManagedOllamaManifest,
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct ManagedOllamaManifest {
    pub version: String,
    pub assets: BTreeMap<String, ManagedOllamaAsset>,
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct ManagedOllamaAsset {
    pub url: String,
    pub sha256: String,
    #[serde(default)]
    pub size_bytes: Option<u64>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub enum ManagedOllamaArchitecture {
    Amd64,
    Arm64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ManagedOllamaInstallStatus {
    NotInstalled,
    Installed { version: Option<String> },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ManagedOllamaInstallProgress {
    DownloadStarted {
        total_bytes: Option<u64>,
    },
    Downloading {
        downloaded_bytes: u64,
        total_bytes: Option<u64>,
    },
    Verifying {
        downloaded_bytes: u64,
    },
    Extracting,
    Installing,
    Starting,
    Restoring,
    Installed {
        version: String,
    },
}

#[derive(Debug, Deserialize, Serialize)]
struct ManagedOllamaInstallationMetadata {
    version: String,
    architecture: String,
    source_url: String,
    sha256: String,
    installed_at: String,
}

impl ManagedOllamaManager {
    pub fn new(paths: AppPaths) -> Result<Self> {
        Self::with_manifest(paths, ManagedOllamaManifest::bundled()?)
    }

    pub fn with_manifest(paths: AppPaths, manifest: ManagedOllamaManifest) -> Result<Self> {
        manifest.validate()?;
        let client = reqwest::Client::builder()
            .user_agent(concat!("Moose/", env!("CARGO_PKG_VERSION")))
            .https_only(true)
            .connect_timeout(std::time::Duration::from_secs(20))
            .read_timeout(std::time::Duration::from_secs(60))
            .build()?;
        Ok(Self {
            paths,
            client,
            manifest,
        })
    }

    pub fn manifest(&self) -> &ManagedOllamaManifest {
        &self.manifest
    }

    pub fn status(&self) -> Result<ManagedOllamaInstallStatus> {
        if !self.is_installed() {
            return Ok(ManagedOllamaInstallStatus::NotInstalled);
        }

        Ok(ManagedOllamaInstallStatus::Installed {
            version: self.installed_version()?,
        })
    }

    pub fn is_installed(&self) -> bool {
        self.paths.ollama_binary_path().is_file()
    }

    pub fn installed_version(&self) -> Result<Option<String>> {
        let metadata_path = self.metadata_path();
        if !metadata_path.is_file() {
            return Ok(None);
        }

        let metadata = fs::read_to_string(metadata_path)?;
        let metadata: ManagedOllamaInstallationMetadata = serde_json::from_str(&metadata)?;
        Ok(Some(metadata.version))
    }

    pub fn installed_manifest(&self) -> Result<Option<ManagedOllamaManifest>> {
        if !self.metadata_path().is_file() {
            return Ok(None);
        }
        let metadata: ManagedOllamaInstallationMetadata =
            serde_json::from_slice(&fs::read(self.metadata_path())?)?;
        let manifest = ManagedOllamaManifest {
            version: metadata.version,
            assets: BTreeMap::from([(
                metadata.architecture,
                ManagedOllamaAsset {
                    url: metadata.source_url,
                    sha256: metadata.sha256,
                    size_bytes: None,
                },
            )]),
        };
        manifest.validate()?;
        Ok(Some(manifest))
    }

    pub async fn prepare<F>(
        &self,
        cancellation: &InstallCancellation,
        on_progress: &mut F,
    ) -> Result<PreparedInstallation>
    where
        F: FnMut(ManagedOllamaInstallProgress) + Send,
    {
        self.paths.create_all()?;
        let lock = transaction::lock_installation(&self.paths)?;
        if transaction::is_pending(&self.paths) && !transaction::finish_committed(&self.paths)? {
            let _process_lock = super::service::acquire_process_lock(self.paths.data_dir())?;
            transaction::recover(&self.paths)?;
        }
        for entry in fs::read_dir(self.paths.data_dir())? {
            let entry = entry?;
            let name = entry.file_name();
            if name
                .to_str()
                .and_then(|name| name.strip_prefix(".ollama-install-"))
                .is_some_and(|id| uuid::Uuid::parse_str(id).is_ok())
            {
                remove_path_if_exists(&entry.path())?;
            }
        }
        cancellation.check()?;
        let architecture = ManagedOllamaArchitecture::current()?;
        let asset = self.manifest.asset_for_architecture(architecture)?.clone();
        fs::create_dir_all(self.paths.ollama_download_cache_dir())?;
        fs::create_dir_all(self.paths.ollama_models_dir())?;
        let archive_path = self.paths.ollama_download_cache_dir().join(format!(
            "ollama-{}-{}.tar.zst.download",
            architecture.as_str(),
            asset.sha256
        ));
        on_progress(ManagedOllamaInstallProgress::DownloadStarted {
            total_bytes: asset.size_bytes,
        });
        let (size, digest) = download::download_asset(
            &self.client,
            &asset,
            &archive_path,
            cancellation,
            on_progress,
        )
        .await?;
        on_progress(ManagedOllamaInstallProgress::Verifying {
            downloaded_bytes: size,
        });
        if let Err(error) = verify_download(&asset, size, &digest) {
            let _ = remove_path_if_exists(&archive_path);
            return Err(error);
        }
        cancellation.check()?;
        on_progress(ManagedOllamaInstallProgress::Extracting);
        let prepared = PreparedInstallation {
            paths: self.paths.clone(),
            staging_dir: self
                .paths
                .data_dir()
                .join(format!(".ollama-install-{}", new_id())),
            _lock: lock,
        };
        let version = self.manifest.version.clone();
        let cancellation = cancellation.clone();
        tokio::task::spawn_blocking(move || -> Result<PreparedInstallation> {
            fs::create_dir_all(&prepared.staging_dir)?;
            extract_zstd_tar(&archive_path, &prepared.staging_dir, &cancellation)?;
            cancellation.check()?;
            ensure_executable_binary(&prepared.staging_dir.join(OLLAMA_BINARY_RELATIVE_PATH))?;
            write_installation_metadata(&prepared.staging_dir, &version, architecture, &asset)?;
            sync_installation(&prepared.staging_dir)?;
            Ok(prepared)
        })
        .await
        .map_err(|error| MooseError::ManagedOllamaUpdate(error.to_string()))?
    }

    fn metadata_path(&self) -> PathBuf {
        self.paths
            .ollama_installation_dir()
            .join(INSTALLATION_METADATA_FILE)
    }

    pub(super) fn clear_completed_download(&self) {
        let Ok(architecture) = ManagedOllamaArchitecture::current() else {
            return;
        };
        let Ok(asset) = self.manifest.asset_for_architecture(architecture) else {
            return;
        };
        let path = self.paths.ollama_download_cache_dir().join(format!(
            "ollama-{}-{}.tar.zst.download",
            architecture.as_str(),
            asset.sha256
        ));
        if let Err(error) = remove_path_if_exists(&path) {
            crate::logging::record("WARN", "ollama.download_cleanup", &error.to_string());
        }
    }
}

impl ManagedOllamaManifest {
    pub fn bundled() -> Result<Self> {
        Self::from_json(BUNDLED_MANAGED_OLLAMA_MANIFEST)
    }

    pub fn from_json(value: &str) -> Result<Self> {
        let manifest: Self = serde_json::from_str(value).map_err(|error| {
            MooseError::ManagedOllamaManifestInvalid(format!("could not parse JSON: {error}"))
        })?;
        manifest.validate()?;
        Ok(manifest)
    }

    pub fn validate(&self) -> Result<()> {
        if !self.version.starts_with('v')
            || super::releases::RuntimeVersion::parse(&self.version).is_none()
        {
            return Err(MooseError::ManagedOllamaManifestInvalid(
                "version must be a fixed release tag beginning with v".to_string(),
            ));
        }

        if self.assets.is_empty() {
            return Err(MooseError::ManagedOllamaManifestInvalid(
                "no supported packages".into(),
            ));
        }
        for (key, asset) in &self.assets {
            asset.validate(
                &self.version,
                ManagedOllamaArchitecture::from_manifest_key(key)?,
            )?;
        }

        Ok(())
    }

    pub fn asset_for_architecture(
        &self,
        architecture: ManagedOllamaArchitecture,
    ) -> Result<&ManagedOllamaAsset> {
        self.assets.get(architecture.as_str()).ok_or_else(|| {
            MooseError::ManagedOllamaUnsupportedArchitecture(architecture.to_string())
        })
    }
}

impl ManagedOllamaAsset {
    fn validate(&self, version: &str, architecture: ManagedOllamaArchitecture) -> Result<()> {
        let expected_path = format!(
            "/ollama/ollama/releases/download/{}/ollama-linux-{}.tar.zst",
            version,
            architecture.as_str()
        );
        let url = reqwest::Url::parse(&self.url).map_err(|error| {
            MooseError::ManagedOllamaManifestInvalid(format!("invalid asset URL: {error}"))
        })?;

        if url.scheme() != "https"
            || url.host_str() != Some("github.com")
            || !url.username().is_empty()
            || url.password().is_some()
            || url.port().is_some()
            || url.path() != expected_path
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err(MooseError::ManagedOllamaManifestInvalid(format!(
                "asset URL for {} must point to the official Ollama GitHub release",
                architecture.as_str()
            )));
        }

        if !is_sha256_hex(&self.sha256) {
            return Err(MooseError::ManagedOllamaManifestInvalid(format!(
                "asset checksum for {} must be a SHA-256 hex digest",
                architecture.as_str()
            )));
        }

        if matches!(self.size_bytes, Some(0)) {
            return Err(MooseError::ManagedOllamaManifestInvalid(format!(
                "asset size for {} must be greater than zero",
                architecture.as_str()
            )));
        }

        Ok(())
    }
}

impl ManagedOllamaArchitecture {
    pub fn current() -> Result<Self> {
        Self::from_rust_arch(std::env::consts::ARCH)
    }

    pub fn from_rust_arch(value: &str) -> Result<Self> {
        match value {
            "x86_64" => Ok(Self::Amd64),
            "aarch64" => Ok(Self::Arm64),
            value => Err(MooseError::ManagedOllamaUnsupportedArchitecture(
                value.to_string(),
            )),
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Amd64 => "amd64",
            Self::Arm64 => "arm64",
        }
    }

    fn from_manifest_key(value: &str) -> Result<Self> {
        match value {
            "amd64" => Ok(Self::Amd64),
            "arm64" => Ok(Self::Arm64),
            value => Err(MooseError::ManagedOllamaManifestInvalid(format!(
                "unsupported manifest architecture {value}"
            ))),
        }
    }
}

impl std::fmt::Display for ManagedOllamaArchitecture {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

fn verify_download(asset: &ManagedOllamaAsset, downloaded_bytes: u64, actual: &str) -> Result<()> {
    if let Some(expected_bytes) = asset.size_bytes
        && downloaded_bytes != expected_bytes
    {
        return Err(MooseError::ManagedOllamaDownloadFailed(format!(
            "expected {expected_bytes} bytes, downloaded {downloaded_bytes} bytes"
        )));
    }

    if !asset.sha256.eq_ignore_ascii_case(actual) {
        return Err(MooseError::ManagedOllamaChecksumMismatch {
            expected: asset.sha256.clone(),
            actual: actual.to_string(),
        });
    }

    Ok(())
}

fn extract_zstd_tar(
    archive_path: &Path,
    destination: &Path,
    cancellation: &InstallCancellation,
) -> Result<()> {
    let file = File::open(archive_path).map_err(|error| {
        extraction_failed(format!(
            "could not open archive {}: {error}",
            archive_path.display()
        ))
    })?;
    let decoder = zstd::stream::read::Decoder::new(file)
        .map_err(|error| extraction_failed(format!("could not decode zstd archive: {error}")))?;
    let mut archive = tar::Archive::new(decoder);
    extract_tar_archive(&mut archive, destination, cancellation)
}

fn extract_tar_archive<R: Read>(
    archive: &mut tar::Archive<R>,
    destination: &Path,
    cancellation: &InstallCancellation,
) -> Result<()> {
    fs::create_dir_all(destination).map_err(|error| {
        extraction_failed(format!(
            "could not create extraction directory {}: {error}",
            destination.display()
        ))
    })?;
    let destination = fs::canonicalize(destination).map_err(|error| {
        extraction_failed(format!(
            "could not resolve extraction directory {}: {error}",
            destination.display()
        ))
    })?;

    let mut extracted_paths = HashSet::new();

    for entry in archive
        .entries()
        .map_err(|error| extraction_failed(format!("could not read archive entries: {error}")))?
    {
        cancellation.check()?;
        let mut entry = entry
            .map_err(|error| extraction_failed(format!("could not read archive entry: {error}")))?;
        let entry_type = entry.header().entry_type();

        if is_metadata_entry(entry_type) {
            continue;
        }

        let raw_path = entry
            .path()
            .map_err(|error| extraction_failed(format!("invalid archive path: {error}")))?
            .into_owned();
        let relative_path = safe_relative_path(&raw_path)?;
        let output_path = destination.join(&relative_path);

        if !entry_type.is_dir() && !extracted_paths.insert(relative_path.clone()) {
            return Err(extraction_failed(format!(
                "duplicate archive entry {}",
                relative_path.display()
            )));
        }

        if entry_type.is_dir() {
            ensure_directory_without_symlinks(&output_path)?;
            set_unix_mode(&output_path, entry.header().mode().ok());
        } else if entry_type.is_file() {
            transaction::check_disk_space(&destination, entry.size())?;
            extract_file_entry(&mut entry, &output_path, cancellation)?;
        } else if entry_type.is_symlink() {
            extract_symlink_entry(&mut entry, &output_path)?;
        } else if entry_type.is_hard_link() {
            extract_hardlink_entry(&mut entry, &destination, &output_path)?;
        } else {
            return Err(extraction_failed(format!(
                "unsupported archive entry type {:?} for {}",
                entry_type,
                relative_path.display()
            )));
        }
    }

    Ok(())
}

fn extract_file_entry<R: Read>(
    entry: &mut tar::Entry<'_, R>,
    output_path: &Path,
    cancellation: &InstallCancellation,
) -> Result<()> {
    let parent = output_path
        .parent()
        .ok_or_else(|| extraction_failed("archive entry has no parent directory"))?;
    ensure_directory_without_symlinks(parent)?;
    ensure_path_is_absent(output_path)?;

    let mode = entry.header().mode().ok();
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output_path)
        .map_err(|error| {
            extraction_failed(format!(
                "could not create {}: {error}",
                output_path.display()
            ))
        })?;
    let mut buffer = [0_u8; 128 * 1024];
    loop {
        cancellation.check()?;
        let read = entry.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        std::io::Write::write_all(&mut output, &buffer[..read])?;
    }
    output.sync_all()?;
    set_unix_mode(output_path, mode);
    Ok(())
}

fn extract_symlink_entry<R: Read>(entry: &mut tar::Entry<'_, R>, output_path: &Path) -> Result<()> {
    let target = entry
        .link_name()
        .map_err(|error| extraction_failed(format!("invalid symlink target: {error}")))?
        .ok_or_else(|| extraction_failed("symlink entry is missing a target"))?
        .into_owned();
    let target = safe_relative_path(&target)?;
    let parent = output_path
        .parent()
        .ok_or_else(|| extraction_failed("symlink entry has no parent directory"))?;
    ensure_directory_without_symlinks(parent)?;
    ensure_path_is_absent(output_path)?;

    #[cfg(unix)]
    unix_fs::symlink(&target, output_path).map_err(|error| {
        extraction_failed(format!(
            "could not create symlink {} -> {}: {error}",
            output_path.display(),
            target.display()
        ))
    })?;

    #[cfg(not(unix))]
    return Err(extraction_failed("symlink extraction requires Unix"));

    #[cfg(unix)]
    Ok(())
}

fn extract_hardlink_entry<R: Read>(
    entry: &mut tar::Entry<'_, R>,
    destination: &Path,
    output_path: &Path,
) -> Result<()> {
    let target = entry
        .link_name()
        .map_err(|error| extraction_failed(format!("invalid hardlink target: {error}")))?
        .ok_or_else(|| extraction_failed("hardlink entry is missing a target"))?
        .into_owned();
    let target = safe_relative_path(&target)?;
    let source_path = destination.join(&target);
    let parent = output_path
        .parent()
        .ok_or_else(|| extraction_failed("hardlink entry has no parent directory"))?;
    ensure_directory_without_symlinks(parent)?;
    ensure_path_is_absent(output_path)?;
    ensure_regular_file_without_symlink(&source_path)?;
    fs::hard_link(&source_path, output_path).map_err(|error| {
        extraction_failed(format!(
            "could not create hardlink {} -> {}: {error}",
            output_path.display(),
            source_path.display()
        ))
    })?;
    Ok(())
}

fn safe_relative_path(path: &Path) -> Result<PathBuf> {
    let mut safe_path = PathBuf::new();

    for component in path.components() {
        match component {
            Component::Normal(value) => safe_path.push(value),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(extraction_failed(format!(
                    "archive path {} is not a safe relative path",
                    path.display()
                )));
            }
        }
    }

    if safe_path.as_os_str().is_empty() {
        return Err(extraction_failed("archive path is empty"));
    }

    Ok(safe_path)
}

fn ensure_directory_without_symlinks(path: &Path) -> Result<()> {
    let mut current = PathBuf::new();

    for component in path.components() {
        current.push(component.as_os_str());
        match fs::symlink_metadata(&current) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() || !metadata.is_dir() {
                    return Err(extraction_failed(format!(
                        "{} is not a safe directory",
                        current.display()
                    )));
                }
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                fs::create_dir(&current).map_err(|error| {
                    extraction_failed(format!(
                        "could not create directory {}: {error}",
                        current.display()
                    ))
                })?;
            }
            Err(error) => {
                return Err(extraction_failed(format!(
                    "could not inspect directory {}: {error}",
                    current.display()
                )));
            }
        }
    }

    Ok(())
}

fn ensure_path_is_absent(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(_) => Err(extraction_failed(format!(
            "{} already exists during extraction",
            path.display()
        ))),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(extraction_failed(format!(
            "could not inspect {}: {error}",
            path.display()
        ))),
    }
}

fn ensure_regular_file_without_symlink(path: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        extraction_failed(format!(
            "could not inspect hardlink target {}: {error}",
            path.display()
        ))
    })?;

    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(extraction_failed(format!(
            "hardlink target {} is not a regular file",
            path.display()
        )));
    }

    Ok(())
}

fn is_metadata_entry(entry_type: EntryType) -> bool {
    entry_type.is_pax_global_extensions()
        || entry_type.is_pax_local_extensions()
        || entry_type.is_gnu_longname()
        || entry_type.is_gnu_longlink()
}

fn write_installation_metadata(
    staging_dir: &Path,
    version: &str,
    architecture: ManagedOllamaArchitecture,
    asset: &ManagedOllamaAsset,
) -> Result<()> {
    let metadata = ManagedOllamaInstallationMetadata {
        version: version.to_string(),
        architecture: architecture.as_str().to_string(),
        source_url: asset.url.clone(),
        sha256: asset.sha256.clone(),
        installed_at: utc_now(),
    };
    let metadata = serde_json::to_string_pretty(&metadata)?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(staging_dir.join(INSTALLATION_METADATA_FILE))?;
    std::io::Write::write_all(&mut file, metadata.as_bytes())?;
    file.sync_all()?;
    Ok(())
}

fn sync_installation(directory: &Path) -> Result<()> {
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            sync_installation(&entry.path())?;
        }
    }
    File::open(directory)?.sync_all()?;
    Ok(())
}

fn ensure_executable_binary(binary_path: &Path) -> Result<()> {
    if !binary_path.is_file() {
        return Err(MooseError::ManagedOllamaBinaryMissing(
            binary_path.to_path_buf(),
        ));
    }

    #[cfg(unix)]
    {
        let metadata = fs::metadata(binary_path)?;
        let mut permissions = metadata.permissions();
        permissions.set_mode(permissions.mode() | 0o111);
        fs::set_permissions(binary_path, permissions)?;
    }

    Ok(())
}
fn remove_path_if_exists(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            if metadata.is_dir() && !metadata.file_type().is_symlink() {
                fs::remove_dir_all(path)?;
            } else {
                fs::remove_file(path)?;
            }
            Ok(())
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

fn path_exists(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok()
}

fn set_unix_mode(path: &Path, mode: Option<u32>) {
    #[cfg(unix)]
    if let Some(mode) = mode {
        let _ = fs::set_permissions(path, fs::Permissions::from_mode(mode & 0o777));
    }

    #[cfg(not(unix))]
    let _ = (path, mode);
}

fn is_sha256_hex(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn hex_digest(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn extraction_failed(message: impl Into<String>) -> MooseError {
    MooseError::ManagedOllamaExtractionFailed(message.into())
}

pub struct PreparedInstallation {
    paths: AppPaths,
    staging_dir: PathBuf,
    _lock: File,
}

impl PreparedInstallation {
    pub(super) fn activate(&self) -> Result<transaction::InstallationTransaction> {
        transaction::InstallationTransaction::begin(&self.paths, &self.staging_dir)
    }
}

impl Drop for PreparedInstallation {
    fn drop(&mut self) {
        if let Err(error) = remove_path_if_exists(&self.staging_dir) {
            crate::logging::record("WARN", "ollama.staging_cleanup", &error.to_string());
        }
    }
}
