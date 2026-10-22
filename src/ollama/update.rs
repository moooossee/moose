use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use crate::error::{MooseError, Result};

use super::{
    OllamaClient,
    manager::{ManagedOllamaInstallProgress, ManagedOllamaManager},
    releases::RuntimeVersion,
    service::{ManagedOllamaConfig, ManagedOllamaService, ManagedOllamaServiceState},
};

#[derive(Clone, Default)]
pub struct InstallCancellation(Arc<CancellationState>);

#[derive(Default)]
struct CancellationState {
    cancelled: AtomicBool,
    notify: tokio::sync::Notify,
}

impl InstallCancellation {
    pub fn cancel(&self) {
        self.0.cancelled.store(true, Ordering::Release);
        self.0.notify.notify_waiters();
    }

    pub fn check(&self) -> Result<()> {
        if self.0.cancelled.load(Ordering::Acquire) {
            Err(MooseError::ManagedOllamaCancelled)
        } else {
            Ok(())
        }
    }

    pub async fn cancelled(&self) {
        let notification = self.0.notify.notified();
        tokio::pin!(notification);
        notification.as_mut().enable();
        if self.check().is_err() {
            return;
        }
        notification.await;
    }
}

pub struct InstallOptions {
    pub config: ManagedOllamaConfig,
    pub keep_running: bool,
    pub repair: bool,
}

pub async fn install<F>(
    manager: ManagedOllamaManager,
    service: Arc<tokio::sync::Mutex<ManagedOllamaService>>,
    options: InstallOptions,
    cancellation: InstallCancellation,
    mut on_progress: F,
) -> Result<()>
where
    F: FnMut(ManagedOllamaInstallProgress) + Send,
{
    let version = manager.manifest().version.clone();
    crate::logging::record("INFO", "ollama.install", &format!("Preparing {version}"));
    let prepared = manager.prepare(&cancellation, &mut on_progress).await?;
    cancellation.check()?;
    let mut service = tokio::select! {
        _ = cancellation.cancelled() => return Err(MooseError::ManagedOllamaCancelled),
        service = service.lock() => service,
    };
    cancellation.check()?;
    if manager.is_installed() {
        let recorded = manager.installed_version().ok().flatten();
        let running = if matches!(service.state(), ManagedOllamaServiceState::Running) {
            OllamaClient::with_timeout(&service.config().base_url, Duration::from_secs(3))?
                .version()
                .await
                .ok()
                .map(|version| version.version)
        } else {
            None
        };
        let installed = running
            .as_deref()
            .or(recorded.as_deref())
            .unwrap_or("unknown");
        validate_target_version(&version, installed, options.repair)?;
    }
    let previous_config = service.config().clone();
    let was_running = matches!(
        service.state(),
        ManagedOllamaServiceState::Running | ManagedOllamaServiceState::Starting
    );
    cancellation.check()?;
    on_progress(ManagedOllamaInstallProgress::Installing);
    let lock = service.stop_for_update()?;
    service.reconfigure(options.config)?;
    let mut transaction = match prepared.activate() {
        Ok(transaction) => transaction,
        Err(error) => {
            service.recover_installation().map_err(|recovery| {
                MooseError::ManagedOllamaUpdate(format!("{error}; recovery failed: {recovery}"))
            })?;
            service.reconfigure(previous_config)?;
            if was_running {
                service.start_with_process_lock(lock)?;
            }
            return Err(error);
        }
    };
    on_progress(ManagedOllamaInstallProgress::Starting);
    let validation = async {
        service.start_with_process_lock(lock.try_clone()?)?;
        service.wait_until_ready(Duration::from_secs(30)).await?;
        let client =
            OllamaClient::with_timeout(&service.config().base_url, Duration::from_secs(5))?;
        let running = client.version().await?;
        verify_running_version(&version, &running.version)?;
        client.list_models().await?;
        Ok::<(), MooseError>(())
    }
    .await
    .and_then(|()| transaction.commit());
    if let Err(error) = validation {
        on_progress(ManagedOllamaInstallProgress::Restoring);
        service.shutdown();
        transaction.rollback().map_err(|recovery| {
            MooseError::ManagedOllamaUpdate(format!(
                "{error}; recovery failed: {recovery}. Restart Moose to retry recovery."
            ))
        })?;
        service.reconfigure(previous_config)?;
        if was_running {
            if let Err(restart) = service.start_with_process_lock(lock) {
                return Err(MooseError::ManagedOllamaUpdate(format!(
                    "{error}. The previous installation was restored but could not restart: {restart}"
                )));
            }
            if let Err(restart) = service.wait_until_ready(Duration::from_secs(30)).await {
                service.shutdown();
                return Err(MooseError::ManagedOllamaUpdate(format!(
                    "{error}. The previous installation was restored but is unavailable: {restart}"
                )));
            }
        }
        crate::logging::record("ERROR", "ollama.update_rollback", &error.to_string());
        return Err(MooseError::ManagedOllamaUpdate(format!(
            "{error}. The installation was rolled back; your models and chats were preserved."
        )));
    }
    manager.clear_completed_download();
    if !options.keep_running {
        service.shutdown();
        service.reconfigure(previous_config)?;
    }
    crate::logging::record(
        "INFO",
        "ollama.install",
        &format!("Verified Ollama {version}"),
    );
    on_progress(ManagedOllamaInstallProgress::Installed { version });
    Ok(())
}

fn validate_target_version(target: &str, installed: &str, repair: bool) -> Result<()> {
    match (
        RuntimeVersion::parse(target),
        RuntimeVersion::parse(installed),
    ) {
        (Some(target), Some(installed))
            if target > installed || (repair && target == installed) =>
        {
            Ok(())
        }
        (_, None) if repair => Ok(()),
        _ => Err(MooseError::ManagedOllamaUpdate(format!(
            "Ollama {installed} is already installed. Installing {target} would not upgrade it. Use Repair Installation to reinstall the same version."
        ))),
    }
}

fn verify_running_version(expected: &str, actual: &str) -> Result<()> {
    match (
        RuntimeVersion::parse(expected),
        RuntimeVersion::parse(actual),
    ) {
        (Some(expected), Some(actual)) if expected == actual => Ok(()),
        _ => Err(MooseError::ManagedOllamaUpdate(format!(
            "Expected Ollama {expected}, but the server reported {actual}."
        ))),
    }
}
