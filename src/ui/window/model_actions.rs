use std::{
    rc::Rc,
    sync::{Arc, mpsc},
    time::Duration,
};

use adw::prelude::*;

use crate::{
    APPLICATION_ID,
    models::{
        DownloadJob, DownloadJobStatus, NewDownloadJob, hugging_face_repository, model_names_match,
    },
    ollama::OllamaPullProgress,
    providers::{Provider, ProviderKind, validate_model_name},
};

use super::{
    ActiveModelPull, Backend, WindowUi, model_download, model_manager, prepared_provider_client,
    refresh_models, require_active_provider, show_model_manager, widgets,
};

enum ModelPullUiEvent {
    Progress(OllamaPullProgress),
    Done,
    Failed(String),
}

enum ModelDeleteUiEvent {
    Done,
    Failed(String),
}

pub(super) fn request_model_pull(
    parent: &adw::ApplicationWindow,
    ui: &Rc<WindowUi>,
    backend: &Rc<Backend>,
    model: String,
) {
    if !can_manage_models(ui, backend) {
        return;
    }
    let model = match validate_model_name(&model) {
        Ok(model) => model,
        Err(_) => {
            ui.toast_overlay
                .add_toast(adw::Toast::new("Model name is invalid"));
            return;
        }
    };

    let Some(provider) = require_active_provider(ui, backend) else {
        return;
    };

    if provider_is_local(&provider) {
        start_model_pull(ui, backend, model);
    } else {
        confirm_remote_model_pull(parent, ui, backend, model);
    }
}

fn confirm_remote_model_pull(
    parent: &adw::ApplicationWindow,
    ui: &Rc<WindowUi>,
    backend: &Rc<Backend>,
    model: String,
) {
    let Some(provider) = require_active_provider(ui, backend) else {
        return;
    };

    let dialog = adw::AlertDialog::builder()
        .heading("Download on Remote Provider?")
        .body(format!(
            "The model will be downloaded by {} at {}.",
            provider.name, provider.base_url
        ))
        .close_response("cancel")
        .default_response("download")
        .build();
    dialog.add_response("cancel", "Cancel");
    dialog.add_response("download", "Download");
    dialog.set_response_appearance("download", adw::ResponseAppearance::Suggested);

    let target_ui = Rc::clone(ui);
    let target_backend = Rc::clone(backend);
    let provider_id = provider.id.clone();
    dialog.connect_response(Some("download"), move |_, _| {
        if active_provider_matches(&target_backend, &provider_id) {
            start_model_pull(&target_ui, &target_backend, model.clone());
        } else {
            target_ui.toast_overlay.add_toast(adw::Toast::new(
                "The connection changed. Try downloading the model again.",
            ));
        }
    });

    dialog.present(Some(parent));
}

pub(super) fn show_download_jobs_dialog(
    parent: &adw::ApplicationWindow,
    ui: &Rc<WindowUi>,
    backend: &Rc<Backend>,
) {
    let Some(provider) = require_active_provider(ui, backend) else {
        return;
    };

    let jobs = match backend
        .download_job_repository
        .list_recent_for_provider(&provider.id, 50)
    {
        Ok(jobs) => jobs,
        Err(error) => {
            ui.toast_overlay.add_toast(adw::Toast::new(&format!(
                "Download jobs could not be loaded: {error}"
            )));
            return;
        }
    };

    let dialog = adw::Dialog::builder()
        .title("Download Jobs")
        .content_width(680)
        .content_height(460)
        .build();

    let header_bar = adw::HeaderBar::builder()
        .show_start_title_buttons(false)
        .show_end_title_buttons(false)
        .build();
    let title = adw::WindowTitle::new("Download Jobs", &provider.name);
    header_bar.set_title_widget(Some(&title));

    if !jobs.is_empty() {
        let clear_button = widgets::icon_button("user-trash-symbolic", "Clear Download History");
        clear_button.add_css_class("destructive-action");
        clear_button.set_sensitive(backend.active_model_pull.borrow().is_none());
        let target_parent = parent.clone();
        let target_ui = Rc::clone(ui);
        let target_backend = Rc::clone(backend);
        let target_dialog = dialog.clone();
        let provider_id = provider.id.clone();
        clear_button.connect_clicked(move |_| {
            confirm_download_history_clear(
                &target_parent,
                &target_ui,
                &target_backend,
                &target_dialog,
                &provider_id,
            );
        });
        header_bar.pack_start(&clear_button);
    }

    let close_button = widgets::icon_button("window-close-symbolic", "Close");
    let target_dialog = dialog.clone();
    close_button.connect_clicked(move |_| {
        target_dialog.close();
    });
    header_bar.pack_end(&close_button);

    let content = if jobs.is_empty() {
        let status_page = adw::StatusPage::builder()
            .icon_name(APPLICATION_ID)
            .title("No Download Jobs")
            .description("Model downloads will appear here.")
            .hexpand(true)
            .vexpand(true)
            .build();
        status_page.upcast::<gtk::Widget>()
    } else {
        let list = gtk::ListBox::new();
        list.set_selection_mode(gtk::SelectionMode::None);
        list.add_css_class("moose-model-list");
        for job in &jobs {
            list.append(&download_job_row(job, ui, backend, &provider));
        }

        gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vscrollbar_policy(gtk::PolicyType::Automatic)
            .min_content_height(320)
            .child(&list)
            .build()
            .upcast::<gtk::Widget>()
    };

    let toolbar_view = adw::ToolbarView::builder()
        .top_bar_style(adw::ToolbarStyle::Flat)
        .content(&content)
        .build();
    toolbar_view.add_top_bar(&header_bar);

    dialog.set_child(Some(&toolbar_view));
    dialog.present(Some(parent));
}

fn confirm_download_history_clear(
    parent: &adw::ApplicationWindow,
    ui: &Rc<WindowUi>,
    backend: &Rc<Backend>,
    dialog: &adw::Dialog,
    provider_id: &str,
) {
    if backend.active_model_pull.borrow().is_some() {
        ui.toast_overlay
            .add_toast(adw::Toast::new("Finish the active model download first"));
        return;
    }

    let alert = adw::AlertDialog::builder()
        .heading("Clear Download History?")
        .body("Remove all model download jobs for the active provider?")
        .close_response("cancel")
        .default_response("cancel")
        .build();
    alert.add_response("cancel", "Cancel");
    alert.add_response("clear", "Clear");
    alert.set_response_appearance("clear", adw::ResponseAppearance::Destructive);

    let target_ui = Rc::clone(ui);
    let target_backend = Rc::clone(backend);
    let target_dialog = dialog.clone();
    let provider_id = provider_id.to_string();
    alert.connect_response(Some("clear"), move |_, _| {
        match target_backend
            .download_job_repository
            .delete_for_provider(&provider_id)
        {
            Ok(_) => {
                target_dialog.close();
                target_ui
                    .toast_overlay
                    .add_toast(adw::Toast::new("Download history cleared"));
            }
            Err(error) => {
                target_ui.toast_overlay.add_toast(adw::Toast::new(&format!(
                    "Download history could not be cleared: {error}"
                )));
            }
        }
    });
    alert.present(Some(parent));
}

fn download_job_row(
    job: &DownloadJob,
    ui: &Rc<WindowUi>,
    backend: &Rc<Backend>,
    provider: &Provider,
) -> adw::ActionRow {
    let row = adw::ActionRow::builder()
        .title(&job.model_name)
        .subtitle(download_job_subtitle(job))
        .use_markup(false)
        .subtitle_lines(3)
        .build();
    row.add_css_class("moose-model-row-item");
    row.set_tooltip_text(Some(&job.model_name));
    row.add_suffix(&download_job_status_label(&job.status));
    if let Some(error) = job.error_message.as_ref().filter(|error| !error.is_empty()) {
        row.set_activatable(true);
        let ui = Rc::clone(ui);
        let backend = Rc::clone(backend);
        let provider = provider.clone();
        let model = job.model_name.clone();
        let error = error.clone();
        row.connect_activated(move |_| show_pull_error(&ui, &backend, &provider, &model, &error));
    }
    row
}

fn download_job_status_label(status: &DownloadJobStatus) -> gtk::Label {
    let label = gtk::Label::builder()
        .label(download_job_status_text(status))
        .halign(gtk::Align::Center)
        .valign(gtk::Align::Center)
        .build();
    label.add_css_class("moose-model-pill");
    label
}

fn download_job_subtitle(job: &DownloadJob) -> String {
    let mut lines = Vec::new();
    if let Some(progress) = download_job_progress_text(job) {
        lines.push(progress);
    }
    lines.push(format!(
        "Updated {}",
        compact_download_timestamp(&job.updated_at)
    ));
    if matches!(&job.status, DownloadJobStatus::Failed)
        && let Some(error_message) = job
            .error_message
            .as_deref()
            .filter(|message| !message.is_empty())
    {
        lines.push(error_message.to_string());
    }
    lines.join("\n")
}

fn download_job_status_text(status: &DownloadJobStatus) -> &'static str {
    match status {
        DownloadJobStatus::Queued => "Queued",
        DownloadJobStatus::Running => "Running",
        DownloadJobStatus::Complete => "Complete",
        DownloadJobStatus::Cancelled => "Cancelled",
        DownloadJobStatus::Failed => "Failed",
    }
}

fn download_job_progress_text(job: &DownloadJob) -> Option<String> {
    match (
        optional_i64_to_u64(job.completed_bytes),
        optional_i64_to_u64(job.total_bytes),
    ) {
        (Some(completed), Some(total)) if total > 0 => Some(format!(
            "{} of {}",
            format_download_size(completed),
            format_download_size(total)
        )),
        (Some(completed), _) if completed > 0 => {
            Some(format!("{} downloaded", format_download_size(completed)))
        }
        (_, Some(total)) if total > 0 => Some(format!("{} total", format_download_size(total))),
        _ => None,
    }
}

fn optional_i64_to_u64(value: Option<i64>) -> Option<u64> {
    value.and_then(|value| u64::try_from(value).ok())
}

fn compact_download_timestamp(value: &str) -> String {
    value
        .trim_end_matches('Z')
        .split_once('.')
        .map(|(value, _)| value)
        .unwrap_or(value)
        .replace('T', " ")
}

pub(super) fn format_download_size(size_bytes: u64) -> String {
    let units = ["B", "KB", "MB", "GB", "TB"];
    let mut size = size_bytes as f64;
    let mut unit_index = 0;

    while size >= 1024.0 && unit_index + 1 < units.len() {
        size /= 1024.0;
        unit_index += 1;
    }

    if unit_index == 0 {
        format!("{size_bytes} {}", units[unit_index])
    } else if size >= 10.0 {
        format!("{size:.0} {}", units[unit_index])
    } else {
        format!("{size:.1} {}", units[unit_index])
    }
}

pub(super) fn start_model_pull(ui: &Rc<WindowUi>, backend: &Rc<Backend>, model: String) {
    if !can_manage_models(ui, backend) {
        return;
    }
    if backend.active_model_pull.borrow().is_some() {
        ui.toast_overlay
            .add_toast(adw::Toast::new("A model download is already running"));
        return;
    }

    if backend.active_model_delete.borrow().is_some() {
        ui.toast_overlay
            .add_toast(adw::Toast::new("Finish the active model deletion first"));
        return;
    }

    show_model_manager(ui);
    super::generation::sync_controls(ui, backend);
    ui.refresh_button.set_sensitive(false);
    model_manager::set_pull_started(&ui.model_manager, &model);

    let Some(provider) = require_active_provider(ui, backend) else {
        ui.refresh_button.set_sensitive(true);
        model_manager::finish_pull(&ui.model_manager);
        return;
    };
    let pull_provider = provider.clone();
    let provider_id = provider.id.clone();
    let job = match backend.download_job_repository.create(NewDownloadJob {
        provider_id,
        model_name: model.clone(),
    }) {
        Ok(job) => job,
        Err(error) => {
            ui.refresh_button.set_sensitive(true);
            model_manager::finish_pull(&ui.model_manager);
            ui.toast_overlay.add_toast(adw::Toast::new(&format!(
                "Download job could not be saved: {error}"
            )));
            return;
        }
    };

    let (sender, receiver) = mpsc::channel();
    let target_model = model.clone();
    let pull_id = job.id.clone();
    let paths = backend.paths.clone();
    let managed_ollama = Arc::clone(&backend.managed_ollama);
    let managed_gpu = backend.managed_gpu.borrow().clone();
    let policy = backend.network_policy.clone();
    let handle = backend.runtime.spawn(async move {
        let client =
            match prepared_provider_client(paths, managed_ollama, managed_gpu, provider, policy)
                .await
            {
                Ok(client) => client,
                Err(error) => {
                    let _ = sender.send(ModelPullUiEvent::Failed(error.to_string()));
                    return;
                }
            };
        let health = client.health().await;
        crate::logging::record(
            "INFO",
            "model.download",
            &format!("Model: {target_model}; server: {}", health.message),
        );
        let progress_sender = sender.clone();
        let result = client
            .pull_model(&target_model, |progress| {
                let _ = progress_sender.send(ModelPullUiEvent::Progress(progress));
            })
            .await;

        match result {
            Ok(()) => {
                let _ = sender.send(ModelPullUiEvent::Done);
            }
            Err(error) => {
                let details = format!(
                    "Model: {target_model}\nServer: {}\n\n{error}",
                    health.message
                );
                let _ = sender.send(ModelPullUiEvent::Failed(details));
            }
        }
    });
    *backend.active_model_pull.borrow_mut() = Some(ActiveModelPull {
        id: pull_id.clone(),
        handle,
    });

    let target_ui = Rc::clone(ui);
    let target_backend = Rc::clone(backend);
    let mut tracking_failed = false;
    gtk::glib::timeout_add_local(Duration::from_millis(80), move || {
        if target_backend
            .active_model_pull
            .borrow()
            .as_ref()
            .is_none_or(|active| active.id != pull_id)
        {
            return gtk::glib::ControlFlow::Break;
        }
        loop {
            match receiver.try_recv() {
                Ok(ModelPullUiEvent::Progress(progress)) => {
                    if !tracking_failed
                        && let Err(error) = target_backend.download_job_repository.update_progress(
                            &pull_id,
                            progress.total_bytes,
                            progress.completed_bytes,
                        )
                    {
                        tracking_failed = true;
                        target_ui.toast_overlay.add_toast(adw::Toast::new(&format!(
                            "Download progress could not be saved: {error}"
                        )));
                    }
                    model_manager::set_pull_progress(&target_ui.model_manager, &model, &progress);
                }
                Ok(ModelPullUiEvent::Done) => {
                    if !target_backend.finish_model_pull(&pull_id) {
                        return gtk::glib::ControlFlow::Break;
                    }
                    if let Err(error) = target_backend.download_job_repository.complete(&pull_id) {
                        target_ui.toast_overlay.add_toast(adw::Toast::new(&format!(
                            "Download job could not be saved: {error}"
                        )));
                    }
                    target_ui.refresh_button.set_sensitive(true);
                    model_manager::finish_pull(&target_ui.model_manager);
                    let toast = adw::Toast::builder()
                        .title("Model download complete")
                        .timeout(10)
                        .build();
                    target_ui.toast_overlay.add_toast(toast.clone());
                    let weak_ui = Rc::downgrade(&target_ui);
                    let weak_backend = Rc::downgrade(&target_backend);
                    let provider_id = pull_provider.id.clone();
                    let downloaded_model = model.clone();
                    super::refresh_models_with_callback(
                        &target_ui,
                        &target_backend,
                        Some(Box::new(move || {
                            if let (Some(ui), Some(backend)) =
                                (weak_ui.upgrade(), weak_backend.upgrade())
                            {
                                add_download_chat_action(
                                    &ui,
                                    &backend,
                                    &provider_id,
                                    &downloaded_model,
                                    &toast,
                                );
                            }
                        })),
                    );
                    return gtk::glib::ControlFlow::Break;
                }
                Ok(ModelPullUiEvent::Failed(error)) => {
                    if !target_backend.finish_model_pull(&pull_id) {
                        return gtk::glib::ControlFlow::Break;
                    }
                    if let Err(save_error) = target_backend
                        .download_job_repository
                        .fail(&pull_id, &error)
                    {
                        target_ui.toast_overlay.add_toast(adw::Toast::new(&format!(
                            "Download job could not be saved: {save_error}"
                        )));
                    }
                    target_ui.refresh_button.set_sensitive(true);
                    model_manager::finish_pull(&target_ui.model_manager);
                    crate::logging::record(
                        "ERROR",
                        "model.download",
                        &format!(
                            "Provider: {} at {}\n{error}",
                            pull_provider.name, pull_provider.base_url
                        ),
                    );
                    let toast = adw::Toast::builder()
                        .title("Model download failed")
                        .button_label("Details")
                        .timeout(10)
                        .build();
                    let detail_ui = Rc::clone(&target_ui);
                    let detail_backend = Rc::clone(&target_backend);
                    let detail_provider = pull_provider.clone();
                    let detail_model = model.clone();
                    let detail_error = error.clone();
                    toast.connect_button_clicked(move |_| {
                        show_pull_error(
                            &detail_ui,
                            &detail_backend,
                            &detail_provider,
                            &detail_model,
                            &detail_error,
                        )
                    });
                    target_ui.toast_overlay.add_toast(toast);
                    if crate::ollama::requires_ollama_update(&error) {
                        show_pull_error(
                            &target_ui,
                            &target_backend,
                            &pull_provider,
                            &model,
                            &error,
                        );
                    }
                    return gtk::glib::ControlFlow::Break;
                }
                Err(mpsc::TryRecvError::Empty) => return gtk::glib::ControlFlow::Continue,
                Err(mpsc::TryRecvError::Disconnected) => {
                    if target_backend.finish_model_pull(&pull_id) {
                        if let Err(error) = target_backend
                            .download_job_repository
                            .fail(&pull_id, "Download stopped before completion.")
                        {
                            target_ui.toast_overlay.add_toast(adw::Toast::new(&format!(
                                "Download job could not be saved: {error}"
                            )));
                        }
                        target_ui.refresh_button.set_sensitive(true);
                        model_manager::finish_pull(&target_ui.model_manager);
                        target_ui
                            .toast_overlay
                            .add_toast(adw::Toast::new("Model download stopped before completion"));
                    }
                    return gtk::glib::ControlFlow::Break;
                }
            }
        }
    });
}

pub(super) fn confirm_model_delete(
    parent: &adw::ApplicationWindow,
    ui: &Rc<WindowUi>,
    backend: &Rc<Backend>,
    model: String,
) {
    if !can_manage_models(ui, backend) {
        return;
    }
    let model = match validate_model_name(&model) {
        Ok(model) => model,
        Err(_) => {
            ui.toast_overlay
                .add_toast(adw::Toast::new("Model name is invalid"));
            return;
        }
    };

    if backend.active_generation.borrow().is_some() {
        ui.toast_overlay
            .add_toast(adw::Toast::new("Finish the active generation first"));
        return;
    }

    if backend.active_model_pull.borrow().is_some() {
        ui.toast_overlay
            .add_toast(adw::Toast::new("Finish the active model download first"));
        return;
    }

    if backend.active_model_delete.borrow().is_some() {
        ui.toast_overlay
            .add_toast(adw::Toast::new("A model deletion is already running"));
        return;
    }

    let Some(provider) = require_active_provider(ui, backend) else {
        return;
    };

    let body = if provider_is_local(&provider) {
        format!("Delete \"{model}\" from Ollama? You can download it again later.")
    } else {
        format!(
            "Delete \"{model}\" from {} at {}? This changes the remote provider.",
            provider.name, provider.base_url
        )
    };

    let dialog = adw::AlertDialog::builder()
        .heading("Delete Model?")
        .body(body)
        .close_response("cancel")
        .default_response("cancel")
        .build();
    dialog.add_response("cancel", "Cancel");
    dialog.add_response("delete", "Delete");
    dialog.set_response_appearance("delete", adw::ResponseAppearance::Destructive);

    let target_ui = Rc::clone(ui);
    let target_backend = Rc::clone(backend);
    dialog.connect_response(Some("delete"), move |_, _| {
        start_model_delete(&target_ui, &target_backend, model.clone());
    });

    dialog.present(Some(parent));
}

fn start_model_delete(ui: &Rc<WindowUi>, backend: &Rc<Backend>, model: String) {
    if !can_manage_models(ui, backend) {
        return;
    }
    if backend.active_generation.borrow().is_some() {
        ui.toast_overlay
            .add_toast(adw::Toast::new("Finish the active generation first"));
        return;
    }

    if backend.active_model_pull.borrow().is_some() {
        ui.toast_overlay
            .add_toast(adw::Toast::new("Finish the active model download first"));
        return;
    }

    if backend.active_model_delete.borrow().is_some() {
        ui.toast_overlay
            .add_toast(adw::Toast::new("A model deletion is already running"));
        return;
    }

    show_model_manager(ui);
    super::generation::sync_controls(ui, backend);
    ui.refresh_button.set_sensitive(false);
    model_manager::set_delete_started(&ui.model_manager, &model);

    let (sender, receiver) = mpsc::channel();
    let target_model = model.clone();
    let Some(provider) = require_active_provider(ui, backend) else {
        ui.refresh_button.set_sensitive(true);
        model_manager::set_delete_finished(
            &ui.model_manager,
            "Delete Failed",
            "Create or connect an Ollama instance first.",
            0.0,
        );
        return;
    };
    let paths = backend.paths.clone();
    let managed_ollama = Arc::clone(&backend.managed_ollama);
    let managed_gpu = backend.managed_gpu.borrow().clone();
    let policy = backend.network_policy.clone();
    let handle = backend.runtime.spawn(async move {
        let client =
            match prepared_provider_client(paths, managed_ollama, managed_gpu, provider, policy)
                .await
            {
                Ok(client) => client,
                Err(error) => {
                    let _ = sender.send(ModelDeleteUiEvent::Failed(error.to_string()));
                    return;
                }
            };
        match client.delete_model(&target_model).await {
            Ok(()) => {
                let _ = sender.send(ModelDeleteUiEvent::Done);
            }
            Err(error) => {
                let _ = sender.send(ModelDeleteUiEvent::Failed(error.to_string()));
            }
        }
    });
    *backend.active_model_delete.borrow_mut() = Some(handle);

    let target_ui = Rc::clone(ui);
    let target_backend = Rc::clone(backend);
    gtk::glib::timeout_add_local(Duration::from_millis(80), move || {
        match receiver.try_recv() {
            Ok(ModelDeleteUiEvent::Done) => {
                target_backend.finish_model_delete();
                target_ui.refresh_button.set_sensitive(true);
                model_manager::set_delete_finished(
                    &target_ui.model_manager,
                    "Model Deleted",
                    &format!("{model} was removed from Ollama."),
                    1.0,
                );
                model_manager::clear_download_job(&target_ui.model_manager);
                target_ui
                    .toast_overlay
                    .add_toast(adw::Toast::new("Model deleted"));
                refresh_models(&target_ui, &target_backend);
                gtk::glib::ControlFlow::Break
            }
            Ok(ModelDeleteUiEvent::Failed(error)) => {
                target_backend.finish_model_delete();
                target_ui.refresh_button.set_sensitive(true);
                model_manager::set_delete_finished(
                    &target_ui.model_manager,
                    "Delete Failed",
                    &error,
                    0.0,
                );
                super::diagnostics::show_error(&target_ui.window, "Model Deletion Failed", &error);
                gtk::glib::ControlFlow::Break
            }
            Err(mpsc::TryRecvError::Empty) => gtk::glib::ControlFlow::Continue,
            Err(mpsc::TryRecvError::Disconnected) => {
                target_backend.finish_model_delete();
                target_ui.refresh_button.set_sensitive(true);
                model_manager::set_delete_finished(
                    &target_ui.model_manager,
                    "Delete Stopped",
                    "The model deletion stopped before completion.",
                    0.0,
                );
                model_manager::clear_download_job(&target_ui.model_manager);
                gtk::glib::ControlFlow::Break
            }
        }
    });
}

pub(super) fn can_manage_models(ui: &Rc<WindowUi>, backend: &Rc<Backend>) -> bool {
    if super::runtime_install_is_active(ui, backend) || backend.credential_operation.get() {
        return false;
    }
    let Some(provider) = require_active_provider(ui, backend) else {
        return false;
    };
    if provider.kind != ProviderKind::Ollama {
        ui.toast_overlay.add_toast(adw::Toast::new("Cloud models are managed by the provider. Refresh the model list to see available models."));
        return false;
    }
    if let Err(error) = backend.network_policy.check(&provider) {
        ui.toast_overlay
            .add_toast(adw::Toast::new(&error.to_string()));
        return false;
    }
    true
}

fn add_download_chat_action(
    ui: &Rc<WindowUi>,
    backend: &Rc<Backend>,
    provider_id: &str,
    model: &str,
    toast: &adw::Toast,
) {
    let installed = ui
        .installed_models
        .borrow()
        .iter()
        .find(|installed| installed.supports_chat && model_names_match(&installed.name, model))
        .map(|installed| installed.name.clone());
    if let Some(model) = installed {
        toast.set_button_label(Some("New Chat"));
        let weak_ui = Rc::downgrade(ui);
        let weak_backend = Rc::downgrade(backend);
        let provider_id = provider_id.to_string();
        toast.connect_button_clicked(move |_| {
            let (Some(ui), Some(backend)) = (weak_ui.upgrade(), weak_backend.upgrade()) else {
                return;
            };
            if active_provider_matches(&backend, &provider_id)
                && ui.model_names.borrow().iter().any(|name| name == &model)
            {
                super::new_conversation_with_model(&ui, &backend, Some(&model));
            } else {
                ui.toast_overlay.add_toast(adw::Toast::new(
                    "Select the original connection and refresh its models to start a chat.",
                ));
            }
        });
    }
}

fn active_provider_matches(backend: &Backend, provider_id: &str) -> bool {
    super::active_provider(backend).is_some_and(|provider| provider.id == provider_id)
}

fn provider_is_local(provider: &Provider) -> bool {
    if provider.is_managed {
        return true;
    }

    reqwest::Url::parse(&provider.base_url)
        .ok()
        .and_then(|url| url.host_str().map(str::to_string))
        .map(|host| {
            let host = host.trim_matches(['[', ']']);
            matches!(host, "localhost" | "127.0.0.1" | "::1")
        })
        .unwrap_or(false)
}

fn show_pull_error(
    ui: &Rc<WindowUi>,
    backend: &Rc<Backend>,
    provider: &Provider,
    model: &str,
    error: &str,
) {
    let needs_update = crate::ollama::requires_ollama_update(error);
    let details = format!(
        "Moose {}\nModel: {model}\nProvider: {}\nAddress: {}\n\n{error}",
        env!("CARGO_PKG_VERSION"),
        provider.name,
        crate::logging::redact(&provider.base_url)
    );
    let dialog = super::diagnostics::details_dialog(
        if needs_update {
            "Ollama Update Required"
        } else {
            "Model Download Failed"
        },
        &details,
    );
    if needs_update && provider.is_managed {
        dialog
            .set_body("Update the Ollama engine managed by Moose, then retry this model download.");
        dialog.add_response("update", "Update Ollama and Retry");
        dialog.set_response_appearance("update", adw::ResponseAppearance::Suggested);
        let ui = Rc::clone(ui);
        let backend = Rc::clone(backend);
        let retry = (provider.id.clone(), model.to_string());
        dialog.connect_response(Some("update"), move |_, _| {
            super::managed_runtime::show_dialog(&ui, &backend, Some(retry.clone()));
        });
    } else if needs_update {
        dialog.set_body(
            "Update Ollama on this provider's server, restart that server, and retry the download.",
        );
    } else {
        if hugging_face_repository(model).is_some() {
            let error = error.to_ascii_lowercase();
            dialog.set_body(if error.contains("401")
                || error.contains("403")
                || error.contains("unauthorized")
                || error.contains("forbidden")
            {
                "Check repository access. Private Hugging Face models require the Ollama instance's SSH key to be added to your Hugging Face account."
            } else {
                "Check the repository and variant. The model must be a GGUF supported by your Ollama version."
            });
        } else {
            dialog.set_body("Check the model name and connection, then try again.");
        }
        dialog.add_response("edit", "Edit and Retry");
        dialog.set_response_appearance("edit", adw::ResponseAppearance::Suggested);
        let weak_ui = Rc::downgrade(ui);
        let weak_backend = Rc::downgrade(backend);
        let provider_id = provider.id.clone();
        let model = model.to_string();
        dialog.connect_response(Some("edit"), move |_, _| {
            if let (Some(ui), Some(backend)) = (weak_ui.upgrade(), weak_backend.upgrade()) {
                if active_provider_matches(&backend, &provider_id) {
                    model_download::show_dialog(&ui, &backend, &model);
                } else {
                    ui.toast_overlay.add_toast(adw::Toast::new(
                        "Select the original connection before retrying this download.",
                    ));
                }
            }
        });
    }
    dialog.present(Some(&ui.window));
}
