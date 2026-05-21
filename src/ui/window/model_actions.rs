use std::{
    rc::Rc,
    sync::{Arc, mpsc},
    time::Duration,
};

use adw::prelude::*;

use crate::{
    APPLICATION_ID,
    models::{DownloadJob, DownloadJobStatus, NewDownloadJob},
    ollama::OllamaPullProgress,
    providers::{Provider, validate_model_name},
};

use super::{
    ActiveModelPull, Backend, WindowUi, model_manager, prepared_ollama_client, refresh_models,
    require_active_provider, show_model_manager, widgets,
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

pub(super) fn show_pull_dialog(
    parent: &adw::ApplicationWindow,
    ui: &Rc<WindowUi>,
    backend: &Rc<Backend>,
) {
    if backend.active_generation.borrow().is_some() {
        ui.toast_overlay
            .add_toast(adw::Toast::new("Finish the active generation first"));
        return;
    }

    if backend.active_model_pull.borrow().is_some() {
        ui.toast_overlay
            .add_toast(adw::Toast::new("A model download is already running"));
        return;
    }

    let model_row = adw::EntryRow::builder().title("Model Name").build();
    let group = adw::PreferencesGroup::new();
    group.add(&model_row);

    let dialog = adw::AlertDialog::builder()
        .heading("Pull Model")
        .body("Enter an Ollama model name, such as llama3.2:latest.")
        .extra_child(&group)
        .close_response("cancel")
        .default_response("pull")
        .build();
    dialog.add_response("cancel", "Cancel");
    dialog.add_response("pull", "Download");
    dialog.set_response_appearance("pull", adw::ResponseAppearance::Suggested);

    let target_parent = parent.clone();
    let target_ui = Rc::clone(ui);
    let target_backend = Rc::clone(backend);
    dialog.connect_response(Some("pull"), move |_, _| {
        let model_text = model_row.text().to_string();
        let model = match validate_model_name(&model_text) {
            Ok(model) => model,
            Err(_) => {
                target_ui
                    .toast_overlay
                    .add_toast(adw::Toast::new("Model name is invalid"));
                return;
            }
        };

        request_model_pull(&target_parent, &target_ui, &target_backend, model);
    });

    dialog.present(Some(parent));
}

pub(super) fn request_model_pull(
    parent: &adw::ApplicationWindow,
    ui: &Rc<WindowUi>,
    backend: &Rc<Backend>,
    model: String,
) {
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
    dialog.connect_response(Some("download"), move |_, _| {
        start_model_pull(&target_ui, &target_backend, model.clone());
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
            list.append(&download_job_row(job));
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

fn download_job_row(job: &DownloadJob) -> adw::ActionRow {
    let row = adw::ActionRow::builder()
        .title(&job.model_name)
        .subtitle(&download_job_subtitle(job))
        .subtitle_lines(3)
        .build();
    row.add_css_class("moose-model-row-item");
    row.set_tooltip_text(Some(&job.model_name));
    row.add_suffix(&download_job_status_label(&job.status));
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

fn start_model_pull(ui: &Rc<WindowUi>, backend: &Rc<Backend>, model: String) {
    if backend.active_generation.borrow().is_some() {
        ui.toast_overlay
            .add_toast(adw::Toast::new("Finish the active generation first"));
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
    ui.refresh_button.set_sensitive(false);
    model_manager::set_pull_started(&ui.model_manager, &model);

    let Some(provider) = require_active_provider(ui, backend) else {
        ui.refresh_button.set_sensitive(true);
        model_manager::set_pull_finished(
            &ui.model_manager,
            "Download Failed",
            "Create or connect an Ollama instance first.",
            0.0,
        );
        return;
    };
    let provider_id = provider.id.clone();
    let job = match backend.download_job_repository.create(NewDownloadJob {
        provider_id,
        model_name: model.clone(),
    }) {
        Ok(job) => job,
        Err(error) => {
            ui.refresh_button.set_sensitive(true);
            model_manager::set_pull_finished(
                &ui.model_manager,
                "Download Failed",
                &format!("Download job could not be saved: {error}"),
                0.0,
            );
            model_manager::clear_download_job(&ui.model_manager);
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
    let handle = backend.runtime.spawn(async move {
        let client = match prepared_ollama_client(paths, managed_ollama, provider).await {
            Ok(client) => client,
            Err(error) => {
                let _ = sender.send(ModelPullUiEvent::Failed(error.to_string()));
                return;
            }
        };
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
                let _ = sender.send(ModelPullUiEvent::Failed(error.to_string()));
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
                    model_manager::set_pull_finished(
                        &target_ui.model_manager,
                        "Download Complete",
                        &format!("{model} is ready to use."),
                        1.0,
                    );
                    model_manager::clear_download_job(&target_ui.model_manager);
                    target_ui
                        .toast_overlay
                        .add_toast(adw::Toast::new("Model download complete"));
                    refresh_models(&target_ui, &target_backend);
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
                    model_manager::set_pull_finished(
                        &target_ui.model_manager,
                        "Download Failed",
                        &error,
                        0.0,
                    );
                    model_manager::clear_download_job(&target_ui.model_manager);
                    target_ui
                        .toast_overlay
                        .add_toast(adw::Toast::new(&format!("Model download failed: {error}")));
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
                        model_manager::set_pull_finished(
                            &target_ui.model_manager,
                            "Download Stopped",
                            "The model download stopped before completion.",
                            0.0,
                        );
                        model_manager::clear_download_job(&target_ui.model_manager);
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
    let handle = backend.runtime.spawn(async move {
        let client = match prepared_ollama_client(paths, managed_ollama, provider).await {
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
                model_manager::clear_download_job(&target_ui.model_manager);
                target_ui
                    .toast_overlay
                    .add_toast(adw::Toast::new(&format!("Model deletion failed: {error}")));
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
