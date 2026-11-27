use crate::{attachments::error, error::Result, i18n::gettext};
use futures_util::StreamExt;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions, TryLockError},
    future::Future,
    io::Read,
    os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Duration,
};
use tokio::io::AsyncWriteExt;

#[derive(Deserialize)]
pub struct Catalog {
    pub revision: String,
    best_revision: String,
    pub models: Vec<Language>,
}

#[derive(Deserialize)]
pub struct Language {
    pub code: String,
    pub name: String,
    #[serde(flatten)]
    fast: Model,
    best: Option<Model>,
    #[serde(skip)]
    verified: Mutex<Option<([u64; 7], bool)>>,
}

#[derive(Deserialize)]
struct Model {
    size: u64,
    sha256: String,
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
        self.installed_size(directory).is_some()
    }

    pub fn installed_size(&self, directory: &Path) -> Option<u64> {
        let metadata = fs::symlink_metadata(self.path(directory)).ok()?;
        let size = metadata.len();
        (metadata.is_file() && (size == self.fast.size || size == self.download_size()))
            .then_some(size)
    }

    pub fn download_size(&self) -> u64 {
        self.best.as_ref().unwrap_or(&self.fast).size
    }

    pub fn selectable(&self) -> bool {
        self.code != "osd"
    }

    pub(super) fn verify(&self, directory: &Path) -> Result<bool> {
        self.verify_model(directory, false)
    }

    fn verify_model(&self, directory: &Path, preferred_only: bool) -> Result<bool> {
        let mut file = match OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(self.path(directory))
        {
            Ok(file) => file,
            Err(failure) if failure.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(failure) if failure.raw_os_error() == Some(libc::ELOOP) => return Ok(false),
            Err(failure) => return Err(failure.into()),
        };
        let metadata = file.metadata()?;
        let size = metadata.len();
        if !metadata.is_file()
            || (preferred_only && size != self.download_size())
            || (size != self.fast.size && size != self.download_size())
        {
            return Ok(false);
        }
        let key = model_key(&metadata);
        let mut cached = self
            .verified
            .lock()
            .unwrap_or_else(|failure| failure.into_inner());
        if cached
            .is_some_and(|(version, preferred)| version == key && (!preferred_only || preferred))
        {
            return Ok(true);
        }
        let mut hasher = Sha256::new();
        let mut bytes = [0; 64 * 1024];
        let mut input = (&mut file).take(size + 1);
        loop {
            let length = input.read(&mut bytes)?;
            if length == 0 {
                break;
            }
            hasher.update(&bytes[..length]);
        }
        let digest = digest_hex(hasher);
        let matches = |model: &Model| size == model.size && digest == model.sha256;
        let preferred = matches(self.best.as_ref().unwrap_or(&self.fast));
        let valid = model_key(&file.metadata()?) == key
            && (preferred || (!preferred_only && matches(&self.fast)));
        if valid {
            *cached = Some((key, preferred));
        }
        Ok(valid)
    }
}

fn digest_hex(digest: Sha256) -> String {
    digest
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn model_key(metadata: &fs::Metadata) -> [u64; 7] {
    [
        metadata.dev(),
        metadata.ino(),
        metadata.len(),
        metadata.mtime() as u64,
        metadata.mtime_nsec() as u64,
        metadata.ctime() as u64,
        metadata.ctime_nsec() as u64,
    ]
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
    let (repository, revision, model) = match &language.best {
        Some(best) => ("tessdata_best", &catalog().best_revision, best),
        None => ("tessdata_fast", &catalog().revision, &language.fast),
    };
    let directory_to_verify = directory.to_path_buf();
    if download
        .wait(tokio::task::spawn_blocking(move || {
            language.verify_model(&directory_to_verify, true)
        }))
        .await?
        .map_err(|_| error(gettext("The OCR language could not be verified")))??
    {
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
        "https://raw.githubusercontent.com/tesseract-ocr/{repository}/{revision}/{code}.traineddata"
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
        .is_some_and(|size| size != model.size)
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
        if bytes > model.size {
            return Err(error(gettext(
                "The OCR language download exceeds its expected size",
            )));
        }
        download.wait(file.write_all(&chunk)).await??;
        digest.update(&chunk);
        download.0.bytes.store(bytes, Ordering::Relaxed);
    }
    let digest = digest_hex(digest);
    if bytes != model.size || digest != model.sha256 {
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
