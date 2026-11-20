use crate::{attachments::error, error::Result, i18n::gettext};
use futures_util::StreamExt;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions, TryLockError},
    future::Future,
    os::unix::fs::{DirBuilderExt, OpenOptionsExt},
    path::{Path, PathBuf},
    sync::{
        Arc, OnceLock,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Duration,
};
use tokio::io::AsyncWriteExt;

#[derive(Deserialize)]
pub struct Catalog {
    pub revision: String,
    pub models: Vec<Language>,
}

#[derive(Deserialize)]
pub struct Language {
    pub code: String,
    pub name: String,
    pub size: u64,
    pub sha256: String,
}

pub fn catalog() -> &'static Catalog {
    static CATALOG: OnceLock<Catalog> = OnceLock::new();
    CATALOG.get_or_init(|| {
        serde_json::from_str(include_str!("../../../data/ocr-languages.json"))
            .expect("Invalid OCR language catalog")
    })
}

pub fn find(code: &str) -> Option<&'static Language> {
    catalog()
        .models
        .iter()
        .find(|language| language.code == code)
}

impl Language {
    pub fn path(&self, directory: &Path) -> PathBuf {
        directory.join(format!("{}.traineddata", self.code))
    }

    pub fn installed(&self, directory: &Path) -> bool {
        fs::symlink_metadata(self.path(directory))
            .is_ok_and(|metadata| metadata.is_file() && metadata.len() == self.size)
    }

    pub fn selectable(&self) -> bool {
        self.code != "osd"
    }
}

pub fn selection(value: &str, directory: &Path) -> String {
    let mut selected = Vec::new();
    for code in value.split('+') {
        if !selected.contains(&code)
            && find(code)
                .is_some_and(|language| language.selectable() && language.installed(directory))
        {
            selected.push(code);
        }
    }
    selected.join("+")
}

#[derive(Clone, Default)]
pub struct Download(Arc<DownloadState>);

#[derive(Default)]
struct DownloadState {
    canceled: AtomicBool,
    bytes: AtomicU64,
}

impl Download {
    pub fn cancel(&self) {
        self.0.canceled.store(true, Ordering::Relaxed);
    }

    pub fn bytes(&self) -> u64 {
        self.0.bytes.load(Ordering::Relaxed)
    }

    fn check(&self) -> Result<()> {
        if self.0.canceled.load(Ordering::Relaxed) {
            return Err(error(gettext("Download canceled")));
        }
        Ok(())
    }

    async fn wait<F: Future>(&self, future: F) -> Result<F::Output> {
        tokio::pin!(future);
        loop {
            self.check()?;
            tokio::select! {
                result = &mut future => return Ok(result),
                _ = tokio::time::sleep(Duration::from_millis(100)) => {}
            }
        }
    }
}

struct PartialFile(PathBuf);

impl Drop for PartialFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

fn lock(directory: &Path, language: &Language) -> Result<File> {
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(directory)?;
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(directory.join(format!(".{}.lock", language.code.replace('/', "-"))))?;
    match file.try_lock() {
        Ok(()) => Ok(file),
        Err(TryLockError::WouldBlock) => Err(error(gettext(
            "Another operation is already changing this OCR language",
        ))),
        Err(TryLockError::Error(failure)) => Err(failure.into()),
    }
}

pub async fn install(code: &str, directory: &Path, download: &Download) -> Result<()> {
    let language = find(code).ok_or_else(|| error(gettext("This OCR language is unavailable")))?;
    let _lock = lock(directory, language)?;
    download.check()?;
    if language.installed(directory) {
        return Ok(());
    }
    let destination = language.path(directory);
    let parent = destination
        .parent()
        .ok_or_else(|| error(gettext("The OCR language directory is unavailable")))?;
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(parent)?;
    let temporary = PartialFile(parent.join(format!(".{}.part", crate::core::new_id())));
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(&temporary.0)?;
    let mut file = tokio::fs::File::from_std(file);
    let client = reqwest::Client::builder()
        .https_only(true)
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(15))
        .read_timeout(Duration::from_secs(30))
        .timeout(Duration::from_secs(900))
        .user_agent(concat!("Moose/", env!("CARGO_PKG_VERSION")))
        .build()?;
    let url = format!(
        "https://raw.githubusercontent.com/tesseract-ocr/tessdata_fast/{}/{}.traineddata",
        catalog().revision,
        language.code
    );
    let response = download
        .wait(
            client
                .get(url)
                .header(reqwest::header::ACCEPT_ENCODING, "identity")
                .send(),
        )
        .await??
        .error_for_status()?;
    if response
        .content_length()
        .is_some_and(|size| size != language.size)
    {
        return Err(error(gettext(
            "The OCR language download has an unexpected size",
        )));
    }
    let mut stream = response.bytes_stream();
    let mut digest = Sha256::new();
    let mut bytes = 0_u64;
    while let Some(chunk) = download.wait(stream.next()).await? {
        let chunk = chunk?;
        bytes += chunk.len() as u64;
        if bytes > language.size {
            return Err(error(gettext(
                "The OCR language download exceeds its expected size",
            )));
        }
        download.wait(file.write_all(&chunk)).await??;
        digest.update(&chunk);
        download.0.bytes.store(bytes, Ordering::Relaxed);
    }
    let digest = digest
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    if bytes != language.size || digest != language.sha256 {
        return Err(error(gettext(
            "The OCR language download is incomplete or failed integrity verification. Try again.",
        )));
    }
    download.wait(file.flush()).await??;
    download.wait(file.sync_all()).await??;
    drop(file);
    download.check()?;
    fs::rename(&temporary.0, &destination)?;
    File::open(parent)?.sync_all()?;
    Ok(())
}

pub fn remove(code: &str, directory: &Path) -> Result<()> {
    let language = find(code).ok_or_else(|| error(gettext("This OCR language is unavailable")))?;
    let _lock = lock(directory, language)?;
    match fs::remove_file(language.path(directory)) {
        Ok(()) => Ok(()),
        Err(failure) if failure.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(failure) => Err(failure.into()),
    }
}
