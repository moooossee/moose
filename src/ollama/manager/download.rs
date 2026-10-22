use std::{
    fs::OpenOptions,
    path::Path,
    time::{Duration, Instant},
};

use futures_util::StreamExt;
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::error::{MooseError, Result};

use super::{
    ManagedOllamaAsset, ManagedOllamaInstallProgress, hex_digest, transaction::check_disk_space,
};
use crate::ollama::update::InstallCancellation;

pub(super) async fn download_asset<F>(
    client: &reqwest::Client,
    asset: &ManagedOllamaAsset,
    path: &Path,
    cancellation: &InstallCancellation,
    on_progress: &mut F,
) -> Result<(u64, String)>
where
    F: FnMut(ManagedOllamaInstallProgress) + Send,
{
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    let mut file = tokio::fs::File::from_std(options.open(path)?);
    let mut hasher = Sha256::new();
    let mut offset = 0_u64;
    let mut buffer = [0_u8; 128 * 1024];
    loop {
        cancellation.check()?;
        let count = file.read(&mut buffer).await?;
        if count == 0 {
            break;
        }
        offset += count as u64;
        hasher.update(&buffer[..count]);
    }
    let digest = hex_digest(hasher.clone().finalize().as_slice());
    if asset.sha256.eq_ignore_ascii_case(&digest)
        && asset.size_bytes.is_none_or(|size| size == offset)
    {
        return Ok((offset, digest));
    }
    if asset.size_bytes.is_some_and(|size| offset >= size) {
        file.set_len(0).await?;
        offset = 0;
        hasher = Sha256::new();
    }
    if let Some(total) = asset.size_bytes {
        check_disk_space(
            path.parent().unwrap_or(Path::new(".")),
            total.saturating_sub(offset),
        )?;
    }
    let mut request = client
        .get(&asset.url)
        .header(reqwest::header::ACCEPT_ENCODING, "identity");
    if offset > 0 {
        request = request.header(reqwest::header::RANGE, format!("bytes={offset}-"));
    }
    let mut response = tokio::select! {
        _ = cancellation.cancelled() => return Err(MooseError::ManagedOllamaCancelled),
        response = request.send() => response?,
    };
    if response.status() == reqwest::StatusCode::RANGE_NOT_SATISFIABLE && offset > 0 {
        response = tokio::select! {
            _ = cancellation.cancelled() => return Err(MooseError::ManagedOllamaCancelled),
            response = client.get(&asset.url).header(reqwest::header::ACCEPT_ENCODING, "identity").send() => response?,
        };
        offset = 0;
        hasher = Sha256::new();
    }
    let total = if response.status() == reqwest::StatusCode::PARTIAL_CONTENT {
        let range = response
            .headers()
            .get(reqwest::header::CONTENT_RANGE)
            .and_then(|value| value.to_str().ok())
            .ok_or_else(|| download_error("The download has no valid resume position."))?;
        let (start, end, total) = parse_range(range)
            .ok_or_else(|| download_error("The download has an invalid byte range."))?;
        if start != offset
            || asset.size_bytes.is_some_and(|size| size != total)
            || response
                .content_length()
                .is_some_and(|length| length != end - start + 1)
        {
            return Err(download_error(
                "The server returned a different package while resuming. Retry the update.",
            ));
        }
        Some(total)
    } else if response.status() == reqwest::StatusCode::OK {
        offset = 0;
        hasher = Sha256::new();
        if let (Some(expected), Some(actual)) = (asset.size_bytes, response.content_length())
            && expected != actual
        {
            return Err(download_error(
                "The package size does not match the release metadata.",
            ));
        }
        response.content_length().or(asset.size_bytes)
    } else {
        return Err(download_error(format!(
            "HTTP {} while downloading Ollama",
            response.status()
        )));
    };
    if let Some(total) = total {
        check_disk_space(
            path.parent().unwrap_or(Path::new(".")),
            total.saturating_sub(offset),
        )?;
    }
    if offset == 0 {
        file.set_len(0).await?;
    }
    use tokio::io::AsyncSeekExt;
    file.seek(std::io::SeekFrom::Start(offset)).await?;
    let mut stream = response.bytes_stream();
    let mut last_progress = Instant::now() - Duration::from_secs(1);
    let result = async {
        loop {
            let chunk = tokio::select! {
                _ = cancellation.cancelled() => return Err(MooseError::ManagedOllamaCancelled),
                chunk = stream.next() => chunk,
            };
            let Some(chunk) = chunk else {
                break;
            };
            let chunk = chunk?;
            if total.is_some_and(|total| offset.saturating_add(chunk.len() as u64) > total) {
                return Err(download_error("The package is larger than expected."));
            }
            file.write_all(&chunk).await?;
            hasher.update(&chunk);
            offset += chunk.len() as u64;
            if last_progress.elapsed() >= Duration::from_millis(100) {
                on_progress(ManagedOllamaInstallProgress::Downloading {
                    downloaded_bytes: offset,
                    total_bytes: total,
                });
                last_progress = Instant::now();
            }
        }
        Ok::<(), MooseError>(())
    }
    .await;
    file.flush().await?;
    file.sync_all().await?;
    result?;
    if total.is_some_and(|total| total != offset) {
        return Err(download_error(
            "The package download is incomplete. Retry to resume it.",
        ));
    }
    Ok((offset, hex_digest(hasher.finalize().as_slice())))
}

fn parse_range(value: &str) -> Option<(u64, u64, u64)> {
    let (range, total) = value.strip_prefix("bytes ")?.split_once('/')?;
    let (start, end) = range.split_once('-')?;
    let (start, end, total) = (start.parse().ok()?, end.parse().ok()?, total.parse().ok()?);
    (start <= end && end < total).then_some((start, end, total))
}

fn download_error(message: impl Into<String>) -> MooseError {
    MooseError::ManagedOllamaDownloadFailed(message.into())
}
