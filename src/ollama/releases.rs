use std::{collections::BTreeMap, fs, io::Read, time::Duration};

use futures_util::StreamExt;
use serde::{Deserialize, Serialize};

use crate::{
    error::{MooseError, Result},
    platform::AppPaths,
};

use super::manager::{ManagedOllamaArchitecture, ManagedOllamaAsset, ManagedOllamaManifest};

const LATEST_RELEASE_URL: &str = "https://api.github.com/repos/ollama/ollama/releases/latest";
const MAX_RELEASE_BYTES: usize = 2 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct RuntimeVersion([u64; 3]);

impl RuntimeVersion {
    pub fn parse(value: &str) -> Option<Self> {
        let value = value.trim().strip_prefix('v').unwrap_or(value.trim());
        let parts = value.split('.').collect::<Vec<_>>();
        if parts.len() != 3 {
            return None;
        }
        let mut numbers = [0; 3];
        for (index, part) in parts.into_iter().enumerate() {
            if part.is_empty()
                || !part.bytes().all(|byte| byte.is_ascii_digit())
                || (part.len() > 1 && part.starts_with('0'))
            {
                return None;
            }
            numbers[index] = part.parse().ok()?;
        }
        Some(Self(numbers))
    }
}

pub fn is_newer(candidate: &str, installed: &str) -> bool {
    match (
        RuntimeVersion::parse(candidate),
        RuntimeVersion::parse(installed),
    ) {
        (Some(candidate), Some(installed)) => candidate > installed,
        _ => false,
    }
}

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    draft: bool,
    prerelease: bool,
    assets: Vec<ReleaseAsset>,
}

#[derive(Deserialize)]
struct ReleaseAsset {
    name: String,
    browser_download_url: String,
    size: u64,
    digest: Option<String>,
}

#[derive(Clone, Deserialize, Serialize)]
pub struct CachedRelease {
    pub manifest: ManagedOllamaManifest,
    pub checked_at: String,
    etag: Option<String>,
}

pub fn cached_release(paths: &AppPaths) -> Option<CachedRelease> {
    let file = fs::File::open(paths.cache_dir().join("ollama-release.json")).ok()?;
    let mut bytes = Vec::new();
    file.take(MAX_RELEASE_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() > MAX_RELEASE_BYTES {
        return None;
    }
    let cached: CachedRelease = serde_json::from_slice(&bytes).ok()?;
    cached.manifest.validate().ok()?;
    Some(cached)
}

pub async fn check_latest_release(paths: &AppPaths) -> Result<CachedRelease> {
    let client = reqwest::Client::builder()
        .user_agent(concat!("Moose/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(30))
        .redirect(reqwest::redirect::Policy::none())
        .build()?;
    let cached = cached_release(paths);
    let mut request = client
        .get(LATEST_RELEASE_URL)
        .header(reqwest::header::ACCEPT, "application/vnd.github+json");
    if let Some(etag) = cached.as_ref().and_then(|cached| cached.etag.as_ref()) {
        request = request.header(reqwest::header::IF_NONE_MATCH, etag);
    }
    let response = request.send().await?;
    let checked_at = crate::core::utc_now();
    let release = if response.status() == reqwest::StatusCode::NOT_MODIFIED {
        let mut cached = cached
            .ok_or_else(|| update_error("The update cache is unavailable. Try checking again."))?;
        cached.checked_at = checked_at;
        cached
    } else {
        if matches!(response.status().as_u16(), 403 | 429) {
            return Err(update_error(
                "GitHub temporarily limited update checks. Try again later. Your installed Ollama is still available.",
            ));
        }
        if !response.status().is_success() {
            return Err(update_error(format!(
                "Could not check for updates: HTTP {}",
                response.status()
            )));
        }
        let etag = response
            .headers()
            .get(reqwest::header::ETAG)
            .and_then(|value| value.to_str().ok())
            .map(str::to_string);
        let mut bytes = Vec::new();
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            if bytes.len().saturating_add(chunk.len()) > MAX_RELEASE_BYTES {
                return Err(update_error("The release response is too large."));
            }
            bytes.extend_from_slice(&chunk);
        }
        CachedRelease {
            manifest: parse_release(&bytes)?,
            checked_at,
            etag,
        }
    };
    paths.create_all()?;
    let cache_path = paths.cache_dir().join("ollama-release.json");
    let temp_path = paths
        .cache_dir()
        .join(format!(".ollama-release-{}", crate::core::new_id()));
    if let Err(error) = (|| -> Result<()> {
        fs::write(&temp_path, serde_json::to_vec(&release)?)?;
        fs::rename(&temp_path, &cache_path)?;
        Ok(())
    })() {
        let _ = fs::remove_file(temp_path);
        crate::logging::record("WARN", "ollama.update_cache", &error.to_string());
    }
    Ok(release)
}

fn parse_release(bytes: &[u8]) -> Result<ManagedOllamaManifest> {
    let release: Release = serde_json::from_slice(bytes)?;
    if release.draft
        || release.prerelease
        || RuntimeVersion::parse(&release.tag_name).is_none()
        || !release.tag_name.starts_with('v')
    {
        return Err(update_error(
            "The release is not a supported stable Ollama version.",
        ));
    }
    let mut assets = BTreeMap::new();
    for architecture in [
        ManagedOllamaArchitecture::Amd64,
        ManagedOllamaArchitecture::Arm64,
    ] {
        let name = format!("ollama-linux-{}.tar.zst", architecture.as_str());
        let matching = release
            .assets
            .iter()
            .filter(|asset| asset.name == name)
            .collect::<Vec<_>>();
        if matching.len() > 1 {
            return Err(update_error("The release contains duplicate packages."));
        }
        if let Some(asset) = matching.first() {
            let sha256 = asset
                .digest
                .as_deref()
                .and_then(|digest| digest.strip_prefix("sha256:"))
                .ok_or_else(|| {
                    update_error(format!(
                        "The {name} package has no verifiable SHA-256 checksum."
                    ))
                })?;
            assets.insert(
                architecture.as_str().to_string(),
                ManagedOllamaAsset {
                    url: asset.browser_download_url.clone(),
                    sha256: sha256.to_ascii_lowercase(),
                    size_bytes: Some(asset.size),
                },
            );
        }
    }
    let manifest = ManagedOllamaManifest {
        version: release.tag_name,
        assets,
    };
    manifest.validate()?;
    manifest.asset_for_architecture(ManagedOllamaArchitecture::current()?)?;
    Ok(manifest)
}

fn update_error(message: impl Into<String>) -> MooseError {
    MooseError::ManagedOllamaUpdate(message.into())
}
