use std::{
    rc::Rc,
    sync::{Arc, mpsc},
    time::Duration,
};

use adw::prelude::*;

use crate::{
    error::Result,
    ollama::{
        OllamaClient, OllamaModel,
        manager::{ManagedOllamaInstallProgress, ManagedOllamaInstallStatus, ManagedOllamaManager},
        service::ManagedOllamaService,
    },
    platform::AppPaths,
    providers::{
        MANAGED_OLLAMA_BIND_ADDRESS, NewProvider, Provider, ProviderKind, ProviderUpdate,
        managed_ollama_base_url, validate_managed_ollama_port, validate_provider_name,
    },
};

use super::{
    Backend, MANAGED_OLLAMA_READY_TIMEOUT, ManagedOllamaHandle, WindowUi, apply_provider_state,
    conversation_list, model_actions::format_download_size, model_manager,
    provider_change_is_blocked, provider_controls, reset_active_conversation, set_models,
    show_chat, show_error, widgets,
};

#[derive(Clone)]
struct ManagedInstallControls {
    dialog: adw::Dialog,
    progress: gtk::ProgressBar,
    status: gtk::Label,
    install_button: gtk::Button,
    close_button: gtk::Button,
}

#[derive(Clone)]
struct ManagedConfigurationControls {
    dialog: adw::Dialog,
    progress: gtk::ProgressBar,
    status: gtk::Label,
    create_button: gtk::Button,
    close_button: gtk::Button,
}

enum ManagedOllamaInstallUiEvent {
    Progress(ManagedOllamaInstallProgress),
    Installed,
    Failed(String),
}

enum ManagedOllamaStartUiEvent {
    Ready(Vec<OllamaModel>),
    Failed(String),
}

pub(super) fn show_dialog(ui: &Rc<WindowUi>, backend: &Rc<Backend>) {
    if provider_change_is_blocked(ui, backend) {
        return;
    }

    let manager = match ManagedOllamaManager::new(backend.paths.clone()) {
        Ok(manager) => manager,
        Err(error) => {
            show_error(&ui.window, "Install Ollama", &error);
            return;
        }
    };

    match manager.status() {
        Ok(ManagedOllamaInstallStatus::Installed { .. }) => {
            show_configuration_dialog(ui, backend);
        }
        Ok(ManagedOllamaInstallStatus::NotInstalled) => {
            show_install_dialog(ui, backend, manager);
        }
        Err(error) => {
            show_error(&ui.window, "Install Ollama", &error);
        }
    }
}

fn show_install_dialog(ui: &Rc<WindowUi>, backend: &Rc<Backend>, manager: ManagedOllamaManager) {
    let dialog = adw::Dialog::builder()
        .title("Install Ollama")
        .content_width(500)
        .content_height(440)
        .build();
    let header_bar = adw::HeaderBar::builder()
        .show_start_title_buttons(false)
        .show_end_title_buttons(false)
        .build();
    let title = adw::WindowTitle::new("Install Ollama", "Managed by Moose");
    header_bar.set_title_widget(Some(&title));

    let close_button = widgets::icon_button("window-close-symbolic", "Close");
    let target_dialog = dialog.clone();
    close_button.connect_clicked(move |_| {
        target_dialog.close();
    });
    header_bar.pack_end(&close_button);

    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(18)
        .hexpand(true)
        .vexpand(true)
        .build();
    content.add_css_class("moose-install-content");

    let overview_group = adw::PreferencesGroup::builder()
        .title("Managed Ollama")
        .description("Moose installs Ollama in app data, separate from any system Ollama.")
        .build();
    overview_group.add(&install_detail_row(
        "software-update-available-symbolic",
        "Official Release",
        "Downloaded from the bundled managed Ollama manifest",
    ));
    overview_group.add(&install_detail_row(
        "folder-symbolic",
        "Private App Data",
        "Models and runtime files stay inside Moose data directories",
    ));
    overview_group.add(&install_detail_row(
        "network-server-symbolic",
        "Port Setup Comes Next",
        "After installation, choose the managed host and port",
    ));

    let progress = gtk::ProgressBar::builder()
        .pulse_step(0.08)
        .show_text(false)
        .build();
    progress.add_css_class("moose-pull-progress");

    let status = gtk::Label::builder()
        .label("Ready to install Ollama")
        .wrap(true)
        .wrap_mode(gtk::pango::WrapMode::WordChar)
        .halign(gtk::Align::Start)
        .xalign(0.0)
        .build();
    status.add_css_class("dim-label");

    let progress_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(10)
        .build();
    progress_box.add_css_class("moose-install-progress");
    progress_box.append(&status);
    progress_box.append(&progress);

    let progress_group = adw::PreferencesGroup::builder().title("Progress").build();
    progress_group.add(&progress_box);

    let install_button = gtk::Button::with_label("Install Ollama");
    install_button.add_css_class("suggested-action");

    let actions = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .halign(gtk::Align::End)
        .build();
    actions.append(&install_button);

    content.append(&overview_group);
    content.append(&progress_group);
    content.append(&actions);

    let toolbar_view = adw::ToolbarView::builder()
        .top_bar_style(adw::ToolbarStyle::Flat)
        .content(&content)
        .build();
    toolbar_view.add_top_bar(&header_bar);
    dialog.set_child(Some(&toolbar_view));

    let controls = ManagedInstallControls {
        dialog: dialog.clone(),
        progress,
        status,
        install_button: install_button.clone(),
        close_button,
    };

    let target_ui = Rc::clone(ui);
    let target_backend = Rc::clone(backend);
    let target_controls = controls.clone();
    install_button.connect_clicked(move |_| {
        start_managed_ollama_install(
            manager.clone(),
            &target_controls,
            &target_ui,
            &target_backend,
        );
    });

    dialog.present(Some(&ui.window));
}

fn install_detail_row(icon_name: &str, title: &str, subtitle: &str) -> adw::ActionRow {
    let row = adw::ActionRow::builder()
        .title(title)
        .subtitle(subtitle)
        .subtitle_lines(2)
        .build();
    let icon = gtk::Image::from_icon_name(icon_name);
    icon.set_pixel_size(20);
    icon.add_css_class("dim-label");
    row.add_prefix(&icon);
    row
}

fn start_managed_ollama_install(
    manager: ManagedOllamaManager,
    controls: &ManagedInstallControls,
    ui: &Rc<WindowUi>,
    backend: &Rc<Backend>,
) {
    controls.install_button.set_sensitive(false);
    controls.install_button.set_label("Installing");
    controls.close_button.set_sensitive(false);
    controls.progress.set_fraction(0.0);
    controls.status.set_label("Downloading Ollama");

    let (sender, receiver) = mpsc::channel();
    backend.runtime.spawn(async move {
        let progress_sender = sender.clone();
        match manager
            .install(move |progress| {
                let _ = progress_sender.send(ManagedOllamaInstallUiEvent::Progress(progress));
            })
            .await
        {
            Ok(()) => {
                let _ = sender.send(ManagedOllamaInstallUiEvent::Installed);
            }
            Err(error) => {
                let _ = sender.send(ManagedOllamaInstallUiEvent::Failed(error.to_string()));
            }
        }
    });

    let target_controls = controls.clone();
    let target_ui = Rc::clone(ui);
    let target_backend = Rc::clone(backend);
    gtk::glib::timeout_add_local(Duration::from_millis(80), move || {
        loop {
            match receiver.try_recv() {
                Ok(ManagedOllamaInstallUiEvent::Progress(event)) => {
                    apply_install_progress(
                        &target_controls.progress,
                        &target_controls.status,
                        &event,
                    );
                }
                Ok(ManagedOllamaInstallUiEvent::Installed) => {
                    target_controls.progress.set_fraction(1.0);
                    target_controls.status.set_label("Ollama installed");
                    target_controls.dialog.close();
                    show_configuration_dialog(&target_ui, &target_backend);
                    return gtk::glib::ControlFlow::Break;
                }
                Ok(ManagedOllamaInstallUiEvent::Failed(error)) => {
                    target_controls.close_button.set_sensitive(true);
                    target_controls.install_button.set_label("Retry");
                    target_controls.install_button.set_sensitive(true);
                    target_controls.status.set_label(&error);
                    target_controls.progress.set_fraction(0.0);
                    return gtk::glib::ControlFlow::Break;
                }
                Err(mpsc::TryRecvError::Empty) => return gtk::glib::ControlFlow::Continue,
                Err(mpsc::TryRecvError::Disconnected) => {
                    target_controls.close_button.set_sensitive(true);
                    target_controls.install_button.set_label("Retry");
                    target_controls.install_button.set_sensitive(true);
                    target_controls
                        .status
                        .set_label("Installation stopped before completion");
                    target_controls.progress.set_fraction(0.0);
                    return gtk::glib::ControlFlow::Break;
                }
            }
        }
    });
}

fn apply_install_progress(
    progress_bar: &gtk::ProgressBar,
    status: &gtk::Label,
    progress: &ManagedOllamaInstallProgress,
) {
    match progress {
        ManagedOllamaInstallProgress::DownloadStarted { total_bytes } => {
            progress_bar.set_fraction(0.0);
            status.set_label(&download_status_text(0, *total_bytes));
        }
        ManagedOllamaInstallProgress::Downloading {
            downloaded_bytes,
            total_bytes,
        } => {
            if let Some(total_bytes) = total_bytes.as_ref().copied().filter(|total| *total > 0) {
                progress_bar
                    .set_fraction((*downloaded_bytes as f64 / total_bytes as f64).clamp(0.0, 0.7));
            } else {
                progress_bar.pulse();
            }
            status.set_label(&download_status_text(*downloaded_bytes, *total_bytes));
        }
        ManagedOllamaInstallProgress::Verifying { downloaded_bytes } => {
            progress_bar.set_fraction(0.75);
            status.set_label(&format!(
                "Verifying {}",
                format_download_size(*downloaded_bytes)
            ));
        }
        ManagedOllamaInstallProgress::Extracting => {
            progress_bar.set_fraction(0.86);
            status.set_label("Extracting Ollama");
        }
        ManagedOllamaInstallProgress::Installing => {
            progress_bar.set_fraction(0.94);
            status.set_label("Installing Ollama");
        }
        ManagedOllamaInstallProgress::Installed { version } => {
            progress_bar.set_fraction(1.0);
            status.set_label(&format!("Ollama {version} installed"));
        }
    }
}

fn download_status_text(downloaded_bytes: u64, total_bytes: Option<u64>) -> String {
    match total_bytes {
        Some(total_bytes) if total_bytes > 0 => format!(
            "Downloading {} of {}",
            format_download_size(downloaded_bytes),
            format_download_size(total_bytes)
        ),
        _ => format!("Downloading {}", format_download_size(downloaded_bytes)),
    }
}

fn show_configuration_dialog(ui: &Rc<WindowUi>, backend: &Rc<Backend>) {
    if provider_change_is_blocked(ui, backend) {
        return;
    }

    let dialog = adw::Dialog::builder()
        .title("Configure Managed Instance")
        .content_width(460)
        .content_height(430)
        .build();
    let header_bar = adw::HeaderBar::builder()
        .show_start_title_buttons(false)
        .show_end_title_buttons(false)
        .build();
    let title = adw::WindowTitle::new("Configure Managed Instance", "Managed by Moose");
    header_bar.set_title_widget(Some(&title));

    let close_button = widgets::icon_button("window-close-symbolic", "Close");
    let target_dialog = dialog.clone();
    close_button.connect_clicked(move |_| {
        target_dialog.close();
    });
    header_bar.pack_end(&close_button);

    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(14)
        .hexpand(true)
        .vexpand(true)
        .build();
    content.add_css_class("moose-install-content");

    let body = gtk::Label::builder()
        .label("Choose the local settings Moose should use for this managed Ollama instance.")
        .wrap(true)
        .wrap_mode(gtk::pango::WrapMode::WordChar)
        .halign(gtk::Align::Start)
        .xalign(0.0)
        .build();
    body.add_css_class("dim-label");

    let name_row = adw::EntryRow::builder()
        .title("Name")
        .text(&provider_controls::next_managed_provider_name(backend))
        .build();
    let host_row = adw::EntryRow::builder()
        .title("Host")
        .text(MANAGED_OLLAMA_BIND_ADDRESS)
        .editable(false)
        .build();
    let port_row = adw::EntryRow::builder()
        .title("Managed Port")
        .text(&provider_controls::next_managed_port(backend).to_string())
        .build();
    let settings_group = adw::PreferencesGroup::builder()
        .title("Instance Settings")
        .description("Port 11435 is recommended. Port 11434 is reserved for external Ollama.")
        .build();
    settings_group.add(&name_row);
    settings_group.add(&host_row);
    settings_group.add(&port_row);

    let progress = gtk::ProgressBar::builder()
        .pulse_step(0.08)
        .show_text(false)
        .build();
    progress.add_css_class("moose-pull-progress");

    let status = gtk::Label::builder()
        .label("Ready to create managed Ollama")
        .wrap(true)
        .wrap_mode(gtk::pango::WrapMode::WordChar)
        .halign(gtk::Align::Start)
        .xalign(0.0)
        .build();
    status.add_css_class("dim-label");

    let create_button = gtk::Button::with_label("Create Managed Instance");
    create_button.add_css_class("suggested-action");
    let actions = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .halign(gtk::Align::End)
        .build();
    actions.append(&create_button);

    content.append(&body);
    content.append(&settings_group);
    content.append(&progress);
    content.append(&status);
    content.append(&actions);

    let toolbar_view = adw::ToolbarView::builder()
        .top_bar_style(adw::ToolbarStyle::Flat)
        .content(&content)
        .build();
    toolbar_view.add_top_bar(&header_bar);
    dialog.set_child(Some(&toolbar_view));

    let controls = ManagedConfigurationControls {
        dialog: dialog.clone(),
        progress,
        status,
        create_button: create_button.clone(),
        close_button,
    };

    let target_ui = Rc::clone(ui);
    let target_backend = Rc::clone(backend);
    let target_controls = controls.clone();
    create_button.connect_clicked(move |_| {
        if provider_change_is_blocked(&target_ui, &target_backend) {
            return;
        }

        let name = match managed_provider_name_from_entry(&name_row) {
            Ok(name) => name,
            Err(message) => {
                target_controls.status.set_label(&message);
                return;
            }
        };
        let port = match managed_port_from_entry(&port_row) {
            Ok(port) => port,
            Err(message) => {
                target_controls.status.set_label(&message);
                return;
            }
        };

        start_managed_ollama_after_configuration(
            name,
            port,
            &target_controls,
            &target_ui,
            &target_backend,
        );
    });

    dialog.present(Some(&ui.window));
}

fn managed_provider_name_from_entry(row: &adw::EntryRow) -> std::result::Result<String, String> {
    validate_provider_name(&row.text()).map_err(|_| "Enter a provider name".to_string())
}

fn managed_port_from_entry(row: &adw::EntryRow) -> std::result::Result<u16, String> {
    let port = row
        .text()
        .trim()
        .parse::<u16>()
        .map_err(|_| "Use a port from 1024 to 65535 except 11434".to_string())?;
    validate_managed_ollama_port(port)
        .map_err(|_| "Use a port from 1024 to 65535 except 11434".to_string())
}

fn start_managed_ollama_after_configuration(
    name: String,
    port: u16,
    controls: &ManagedConfigurationControls,
    ui: &Rc<WindowUi>,
    backend: &Rc<Backend>,
) {
    controls.progress.set_fraction(0.0);
    controls.status.set_label("Starting Ollama");
    controls.create_button.set_sensitive(false);
    controls.create_button.set_label("Starting");
    controls.close_button.set_sensitive(false);

    let (sender, receiver) = mpsc::channel();
    let paths = backend.paths.clone();
    let managed_ollama = Arc::clone(&backend.managed_ollama);
    backend.runtime.spawn(async move {
        let event = match start_managed_ollama_and_load_models(paths, managed_ollama, port).await {
            Ok(models) => ManagedOllamaStartUiEvent::Ready(models),
            Err(error) => ManagedOllamaStartUiEvent::Failed(error.to_string()),
        };
        let _ = sender.send(event);
    });

    let target_controls = controls.clone();
    let target_ui = Rc::clone(ui);
    let target_backend = Rc::clone(backend);
    gtk::glib::timeout_add_local(Duration::from_millis(80), move || {
        match receiver.try_recv() {
            Ok(ManagedOllamaStartUiEvent::Ready(models)) => {
                match create_managed_provider(&target_backend, name.clone(), port) {
                    Ok(provider) => {
                        apply_active_provider_with_models(
                            &target_ui,
                            &target_backend,
                            provider,
                            models,
                        );
                        target_controls.progress.set_fraction(1.0);
                        target_controls.status.set_label("Managed Ollama is ready");
                        target_ui.provider_status.set_text("Ready");
                        target_ui
                            .toast_overlay
                            .add_toast(adw::Toast::new("Managed instance ready"));
                        target_controls.dialog.close();
                    }
                    Err(error) => {
                        target_controls.close_button.set_sensitive(true);
                        target_controls.create_button.set_label("Retry");
                        target_controls.create_button.set_sensitive(true);
                        target_controls.status.set_label(&error.to_string());
                    }
                }
                gtk::glib::ControlFlow::Break
            }
            Ok(ManagedOllamaStartUiEvent::Failed(error)) => {
                target_controls.close_button.set_sensitive(true);
                target_controls.create_button.set_label("Retry");
                target_controls.create_button.set_sensitive(true);
                target_controls
                    .status
                    .set_label(&format!("Ollama start failed: {error}"));
                target_controls.progress.set_fraction(0.0);
                gtk::glib::ControlFlow::Break
            }
            Err(mpsc::TryRecvError::Empty) => {
                target_controls.progress.pulse();
                gtk::glib::ControlFlow::Continue
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                target_controls.close_button.set_sensitive(true);
                target_controls.create_button.set_label("Retry");
                target_controls.create_button.set_sensitive(true);
                target_controls.status.set_label("Ollama start failed");
                target_controls.progress.set_fraction(0.0);
                gtk::glib::ControlFlow::Break
            }
        }
    });
}

async fn start_managed_ollama_and_load_models(
    paths: AppPaths,
    managed_ollama: ManagedOllamaHandle,
    port: u16,
) -> Result<Vec<OllamaModel>> {
    let base_url = {
        let expected_base_url = managed_ollama_base_url(port)?;
        let mut service = managed_ollama.lock().await;
        if service.config().base_url != expected_base_url {
            service.shutdown();
            *service = ManagedOllamaService::new_with_port(&paths, port)?;
        }
        service.ensure_ready(MANAGED_OLLAMA_READY_TIMEOUT).await?;
        service.config().base_url.clone()
    };
    OllamaClient::new(&base_url)?.list_models().await
}

fn create_managed_provider(backend: &Backend, name: String, port: u16) -> Result<Provider> {
    let base_url = managed_ollama_base_url(port)?;
    if let Some(provider) = backend
        .repository
        .list()?
        .into_iter()
        .find(|provider| provider.is_managed && provider.base_url == base_url)
    {
        backend.repository.update(ProviderUpdate {
            id: provider.id,
            name,
            base_url,
            is_default: true,
        })
    } else {
        backend.repository.create(NewProvider {
            kind: ProviderKind::Ollama,
            name,
            base_url,
            is_managed: true,
            is_default: true,
        })
    }
}

fn apply_active_provider_with_models(
    ui: &Rc<WindowUi>,
    backend: &Rc<Backend>,
    provider: Provider,
    models: Vec<OllamaModel>,
) {
    *backend.provider.borrow_mut() = Some(provider.clone());
    reset_active_conversation(ui, backend);
    apply_provider_state(ui, &Some(provider));
    model_manager::clear_download_job(&ui.model_manager);
    conversation_list::refresh(ui, backend);
    set_models(ui, backend, models);
    show_chat(ui);
}
