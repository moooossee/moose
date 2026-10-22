use std::{
    fs::{self, File, OpenOptions, TryLockError},
    io::Write,
    path::Path,
};

use serde::{Deserialize, Serialize};

use crate::{
    error::{MooseError, Result},
    platform::AppPaths,
};

use super::{path_exists, remove_path_if_exists};

const JOURNAL: &str = "ollama-update.json";
const BACKUP: &str = ".ollama-installation-backup";

#[derive(Deserialize, Serialize)]
struct Journal {
    had_previous: bool,
    committed: bool,
}

pub(crate) struct InstallationTransaction {
    paths: AppPaths,
    journal: Journal,
}

impl InstallationTransaction {
    pub fn begin(paths: &AppPaths, staging: &Path) -> Result<Self> {
        if is_pending(paths) || path_exists(&paths.data_dir().join(BACKUP)) {
            return Err(update_error(
                "An earlier update needs recovery before installing.",
            ));
        }
        let transaction = Self {
            paths: paths.clone(),
            journal: Journal {
                had_previous: path_exists(&paths.ollama_installation_dir()),
                committed: false,
            },
        };
        write_journal(paths, &transaction.journal)?;
        let result = (|| -> Result<()> {
            if transaction.journal.had_previous {
                fs::rename(
                    paths.ollama_installation_dir(),
                    paths.data_dir().join(BACKUP),
                )?;
                sync_directory(paths.data_dir())?;
            }
            fs::rename(staging, paths.ollama_installation_dir())?;
            sync_directory(paths.data_dir())?;
            Ok(())
        })();
        if let Err(error) = result {
            recover(paths).map_err(|recovery| {
                update_error(format!("{error}; recovery failed: {recovery}"))
            })?;
            return Err(error);
        }
        Ok(transaction)
    }

    pub fn commit(&mut self) -> Result<()> {
        self.journal.committed = true;
        write_journal(&self.paths, &self.journal)?;
        if let Err(error) = recover(&self.paths) {
            crate::logging::record("WARN", "ollama.update_cleanup", &error.to_string());
        }
        Ok(())
    }

    pub fn rollback(mut self) -> Result<()> {
        if self.journal.committed {
            self.journal.committed = false;
            write_journal(&self.paths, &self.journal)?;
        }
        recover(&self.paths)
    }
}

pub(super) fn lock_installation(paths: &AppPaths) -> Result<File> {
    let mut options = OpenOptions::new();
    options.create(true).truncate(false).read(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    let lock = options.open(paths.data_dir().join("ollama-install.lock"))?;
    match lock.try_lock() {
        Ok(()) => Ok(lock),
        Err(TryLockError::WouldBlock) => Err(update_error(
            "Another Ollama installation or update is already in progress.",
        )),
        Err(TryLockError::Error(error)) => Err(error.into()),
    }
}

pub(crate) fn is_pending(paths: &AppPaths) -> bool {
    path_exists(&paths.data_dir().join(JOURNAL))
}

pub(super) fn finish_committed(paths: &AppPaths) -> Result<bool> {
    let journal: Journal = serde_json::from_slice(&fs::read(paths.data_dir().join(JOURNAL))?)?;
    if !journal.committed {
        return Ok(false);
    }
    recover(paths)?;
    Ok(true)
}

pub(crate) fn recover(paths: &AppPaths) -> Result<()> {
    let journal_path = paths.data_dir().join(JOURNAL);
    if !path_exists(&journal_path) {
        return Ok(());
    }
    let journal: Journal = serde_json::from_slice(&fs::read(&journal_path)?)?;
    let installation = paths.ollama_installation_dir();
    let backup = paths.data_dir().join(BACKUP);
    if journal.committed {
        if !installation.join("bin/ollama").is_file() {
            return Err(update_error(
                "The completed installation is missing. Its backup has been retained.",
            ));
        }
        remove_path_if_exists(&backup)?;
    } else if path_exists(&backup) {
        remove_path_if_exists(&installation)?;
        fs::rename(&backup, &installation)?;
        sync_directory(paths.data_dir())?;
    } else if !journal.had_previous {
        remove_path_if_exists(&installation)?;
    } else if !path_exists(&installation) {
        return Err(update_error(
            "The previous installation and its backup are missing. Recovery cannot continue.",
        ));
    }
    fs::remove_file(journal_path)?;
    sync_directory(paths.data_dir())?;
    crate::logging::record(
        "INFO",
        "ollama.update_recovery",
        if journal.committed {
            "Completed update cleanup"
        } else {
            "Restored the previous installation"
        },
    );
    Ok(())
}

fn write_journal(paths: &AppPaths, journal: &Journal) -> Result<()> {
    let temporary = paths.data_dir().join(".ollama-update.json.tmp");
    let mut options = OpenOptions::new();
    options.create(true).truncate(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    let mut file = options.open(&temporary)?;
    file.write_all(&serde_json::to_vec(journal)?)?;
    file.sync_all()?;
    fs::rename(temporary, paths.data_dir().join(JOURNAL))?;
    sync_directory(paths.data_dir())
}

fn sync_directory(directory: &Path) -> Result<()> {
    File::open(directory)?.sync_all()?;
    Ok(())
}

pub(super) fn check_disk_space(directory: &Path, needed: u64) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        let path = std::ffi::CString::new(directory.as_os_str().as_bytes())
            .map_err(|_| update_error("The installation path is invalid."))?;
        let mut info = std::mem::MaybeUninit::<libc::statvfs>::uninit();
        if unsafe { libc::statvfs(path.as_ptr(), info.as_mut_ptr()) } != 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        let info = unsafe { info.assume_init() };
        let available = u128::from(info.f_bavail) * u128::from(info.f_frsize);
        if available < u128::from(needed) + 16 * 1024 * 1024 {
            return Err(update_error(
                "There is not enough free disk space to prepare Ollama. Free some space and retry.",
            ));
        }
    }
    #[cfg(not(unix))]
    let _ = (directory, needed);
    Ok(())
}

fn update_error(message: impl Into<String>) -> MooseError {
    MooseError::ManagedOllamaUpdate(message.into())
}
