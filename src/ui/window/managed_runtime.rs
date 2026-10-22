use std::{
    cell::RefCell,
    rc::Rc,
    sync::{Arc, mpsc},
    time::Duration,
};

use adw::prelude::*;

use crate::ollama::{
    manager::{ManagedOllamaInstallProgress, ManagedOllamaManager, ManagedOllamaManifest},
    releases::{self, CachedRelease},
    service::ManagedOllamaConfig,
    update::{self, InstallCancellation},
};

use super::{
    Backend, WindowUi, active_provider, diagnostics, managed_install, model_actions,
    provider_change_is_blocked, provider_controls, refresh_models, show_error, widgets,
};

#[derive(Clone)]
struct Controls {
    dialog: adw::Dialog,
    summary: gtk::Label,
    installed: gtk::Label,
    available: gtk::Label,
    actions: adw::PreferencesGroup,
    primary: gtk::Box,
    activity: gtk::Box,
    progress: gtk::ProgressBar,
    status: gtk::Label,
    last_checked: gtk::Label,
    check: gtk::Button,
    check_label: gtk::Label,
    check_spinner: gtk::Spinner,
    update: gtk::Button,
    repair: adw::ActionRow,
    configure: adw::ActionRow,
    cancel: gtk::Button,
    candidate: Rc<RefCell<ManagedOllamaManifest>>,
}

enum Event {
    Progress(ManagedOllamaInstallProgress),
    Finished(std::result::Result<(), String>),
}

pub(super) fn notify_update_available(ui: &Rc<WindowUi>, backend: &Rc<Backend>) {
    let Ok(manager) = ManagedOllamaManager::new(backend.paths.clone()) else {
        return;
    };
    if !manager.is_installed() {
        return;
    }
    let Ok(Some(installed)) = manager.installed_version() else {
        return;
    };
    if !releases::is_newer(&manager.manifest().version, &installed) {
        return;
    }
    let toast = adw::Toast::builder()
        .title("An Ollama update is available")
        .button_label("Review")
        .timeout(10)
        .build();
    let ui = Rc::clone(ui);
    let backend = Rc::clone(backend);
    let target_ui = Rc::clone(&ui);
    toast.connect_button_clicked(move |_| show_dialog(&target_ui, &backend, None));
    ui.toast_overlay.add_toast(toast);
}

pub(super) fn show_dialog(
    ui: &Rc<WindowUi>,
    backend: &Rc<Backend>,
    retry: Option<(String, String)>,
) {
    if super::runtime_install_is_active(ui, backend) {
        return;
    }
    let manager = match ManagedOllamaManager::new(backend.paths.clone()) {
        Ok(manager) => manager,
        Err(error) => {
            show_error(&ui.window, "Managed Ollama", &error);
            return;
        }
    };
    let included = manager.manifest().clone();
    let cached = releases::cached_release(&backend.paths);
    let candidate = cached
        .as_ref()
        .map(|cached| &cached.manifest)
        .filter(|manifest| releases::is_newer(&manifest.version, &included.version))
        .cloned()
        .unwrap_or_else(|| included.clone());
    let setup = widgets::SetupDialog::new(
        "Managed Ollama",
        "Ollama",
        "Your local AI engine, managed by Moose.",
    );
    let dialog = setup.dialog.clone();
    let content = &setup.content;
    let summary = gtk::Label::builder()
        .halign(gtk::Align::Center)
        .margin_top(4)
        .css_classes(["caption-heading"])
        .build();
    setup.hero.append(&summary);
    let engine = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(12)
        .build();
    let group = adw::PreferencesGroup::new();
    let installed_row = adw::ActionRow::builder()
        .title("Installed Version")
        .use_markup(false)
        .build();
    let installed = gtk::Label::builder()
        .selectable(true)
        .valign(gtk::Align::Center)
        .css_classes(["dim-label"])
        .build();
    installed_row.add_suffix(&installed);
    let available_row = adw::ActionRow::builder()
        .title("Available Version")
        .use_markup(false)
        .build();
    let available = gtk::Label::builder()
        .selectable(true)
        .valign(gtk::Align::Center)
        .css_classes(["dim-label"])
        .build();
    available_row.set_tooltip_text(Some(&format!(
        "Official stable release. Included with Moose: {}.",
        included.version
    )));
    available_row.add_suffix(&available);
    group.add(&installed_row);
    group.add(&available_row);
    engine.append(&group);
    let primary = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(8)
        .build();
    let update = gtk::Button::with_label("Update Ollama");
    primary.append(&widgets::primary_action(&update));
    primary.append(
        &gtk::Label::builder()
            .label("Your models, chats, and connections are kept.")
            .wrap(true)
            .justify(gtk::Justification::Center)
            .css_classes(["caption", "dim-label"])
            .build(),
    );
    engine.append(&primary);
    let updates = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(4)
        .build();
    let check_content = gtk::Box::builder()
        .spacing(8)
        .halign(gtk::Align::Center)
        .build();
    let check_spinner = gtk::Spinner::new();
    check_spinner.set_visible(false);
    let check_label = gtk::Label::new(Some("Check for Updates"));
    check_content.append(&check_spinner);
    check_content.append(&check_label);
    let check = gtk::Button::builder()
        .child(&check_content)
        .halign(gtk::Align::Center)
        .css_classes(["flat"])
        .build();
    updates.append(&check);
    let last_checked = gtk::Label::builder()
        .wrap(true)
        .justify(gtk::Justification::Center)
        .selectable(true)
        .css_classes(["caption", "dim-label"])
        .build();
    if let Some(cached) = cached.as_ref() {
        last_checked.set_label(&last_checked_text(&cached.checked_at));
    } else {
        last_checked.set_visible(false);
    }
    updates.append(&last_checked);
    engine.append(&updates);
    let activity = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(12)
        .visible(false)
        .build();
    let progress = gtk::ProgressBar::new();
    let status = gtk::Label::builder()
        .wrap(true)
        .wrap_mode(gtk::pango::WrapMode::WordChar)
        .selectable(true)
        .xalign(0.0)
        .build();
    let cancel = gtk::Button::builder()
        .label("Cancel")
        .halign(gtk::Align::Center)
        .visible(false)
        .css_classes(["flat"])
        .build();
    activity.append(&progress);
    activity.append(&status);
    activity.append(&cancel);
    engine.append(&activity);
    content.append(&engine);
    let actions = adw::PreferencesGroup::new();
    let configure = adw::ActionRow::builder()
        .title("Add Managed Connection")
        .activatable(true)
        .build();
    configure.add_prefix(&gtk::Image::from_icon_name("list-add-symbolic"));
    configure.add_suffix(&gtk::Image::from_icon_name("go-next-symbolic"));
    let repair = adw::ActionRow::builder()
        .title("Repair Installation")
        .use_markup(false)
        .activatable(true)
        .build();
    repair.add_prefix(&gtk::Image::from_icon_name("emblem-system-symbolic"));
    repair.add_suffix(&gtk::Image::from_icon_name("go-next-symbolic"));
    actions.add(&configure);
    actions.add(&repair);
    content.append(&actions);
    let controls = Controls {
        dialog: dialog.clone(),
        summary,
        installed,
        available,
        actions,
        primary,
        activity,
        progress,
        status,
        last_checked,
        check: check.clone(),
        check_label,
        check_spinner,
        update: update.clone(),
        repair: repair.clone(),
        configure: configure.clone(),
        cancel: cancel.clone(),
        candidate: Rc::new(RefCell::new(candidate)),
    };
    update_controls(&controls, backend);

    let target_controls = controls.clone();
    let target_backend = Rc::clone(backend);
    check.connect_clicked(move |_| check_updates(&target_controls, &target_backend));

    {
        let controls = controls.clone();
        let ui = Rc::clone(ui);
        let backend = Rc::clone(backend);
        let retry = retry.clone();
        update.connect_clicked(move |_| {
            start_install(&controls, &ui, &backend, false, retry.clone())
        });
    }
    {
        let controls = controls.clone();
        let ui = Rc::clone(ui);
        let backend = Rc::clone(backend);
        let retry = retry.clone();
        repair.connect_activated(move |_| {
            start_install(&controls, &ui, &backend, true, retry.clone())
        });
    }
    let target_backend = Rc::clone(backend);
    let target_status = controls.status.clone();
    cancel.connect_clicked(move |button| {
        if let Some(token) = target_backend.runtime_install.borrow().as_ref() {
            token.cancel();
            button.set_sensitive(false);
            target_status.set_label("Cancelling. The partial download will be kept for retry.");
        }
    });
    let target_ui = Rc::clone(ui);
    let target_backend = Rc::clone(backend);
    let target_dialog = dialog.clone();
    configure.connect_activated(move |_| {
        if provider_change_is_blocked(&target_ui, &target_backend) {
            return;
        }
        target_dialog.close();
        managed_install::show_configuration_dialog(&target_ui, &target_backend);
    });
    dialog.present(Some(&ui.window));
}

fn last_checked_text(timestamp: &str) -> String {
    if let Some((date, time)) = timestamp
        .strip_suffix('Z')
        .and_then(|time| time.split_once('T'))
        && let Some(time) = time.get(..5)
    {
        format!("Last checked: {date} at {time} UTC")
    } else {
        format!("Last checked: {timestamp}")
    }
}

fn set_summary(controls: &Controls, text: &str, style: &str) {
    controls.summary.set_label(text);
    for class in ["accent", "success", "warning", "error", "dim-label"] {
        controls.summary.remove_css_class(class);
    }
    controls.summary.add_css_class(style);
}

fn update_controls(controls: &Controls, backend: &Backend) {
    let Ok(manager) = ManagedOllamaManager::new(backend.paths.clone()) else {
        return;
    };
    let version = manager.installed_version().ok().flatten();
    let exists = manager.is_installed();
    controls.installed.set_label(if exists {
        version.as_deref().unwrap_or("Unknown version")
    } else {
        "Not installed"
    });
    let candidate = controls.candidate.borrow();
    controls.available.set_label(&candidate.version);
    controls.update.set_label(if exists {
        "Update Ollama"
    } else {
        "Install Ollama"
    });
    let update_available = !exists
        || version
            .as_deref()
            .is_some_and(|version| releases::is_newer(&candidate.version, version));
    controls.primary.set_visible(update_available);
    controls.update.set_sensitive(update_available);
    if !exists {
        set_summary(controls, "Not installed", "dim-label");
    } else if version
        .as_deref()
        .is_none_or(|version| releases::RuntimeVersion::parse(version).is_none())
    {
        set_summary(controls, "Version unavailable", "warning");
    } else if update_available {
        set_summary(controls, "Update available", "accent");
    } else {
        set_summary(controls, "Installed", "success");
    }
    let repair_version = manager
        .installed_manifest()
        .ok()
        .flatten()
        .map(|manifest| manifest.version)
        .unwrap_or_else(|| candidate.version.clone());
    controls
        .repair
        .set_subtitle(&format!("Reinstall {repair_version} and keep your data"));
    let can_repair = backend.paths.ollama_installation_dir().exists();
    controls.repair.set_visible(can_repair);
    controls.repair.set_sensitive(can_repair);
    controls.configure.set_visible(exists);
    controls.configure.set_sensitive(exists);
    controls.actions.set_visible(exists || can_repair);
    controls.check.set_sensitive(true);
    controls.check.set_visible(true);
    controls
        .last_checked
        .set_visible(!controls.last_checked.text().is_empty());
}

fn set_busy(controls: &Controls, busy: bool) {
    for button in [&controls.check, &controls.update] {
        button.set_sensitive(!busy);
    }
    controls.repair.set_sensitive(!busy);
    controls.configure.set_sensitive(!busy);
}

fn check_updates(controls: &Controls, backend: &Rc<Backend>) {
    set_busy(controls, true);
    controls.activity.set_visible(false);
    controls.check_label.set_label("Checking for Updates…");
    controls.check_spinner.set_visible(true);
    controls.check_spinner.start();
    set_summary(controls, "Checking for updates", "accent");
    let (sender, receiver) = mpsc::channel();
    let paths = backend.paths.clone();
    backend.runtime.spawn(async move {
        let result = releases::check_latest_release(&paths)
            .await
            .map_err(|error| error.to_string());
        let _ = sender.send(result);
    });
    let controls = controls.clone();
    let backend = Rc::clone(backend);
    gtk::glib::timeout_add_local(Duration::from_millis(100), move || {
        let result = match receiver.try_recv() {
            Ok(result) => result,
            Err(mpsc::TryRecvError::Empty) => return gtk::glib::ControlFlow::Continue,
            Err(mpsc::TryRecvError::Disconnected) => {
                Err("The update check stopped. Try again.".into())
            }
        };
        let failed = result.is_err();
        match result {
            Ok(CachedRelease {
                manifest,
                checked_at,
                ..
            }) => {
                if releases::is_newer(&manifest.version, &controls.candidate.borrow().version) {
                    *controls.candidate.borrow_mut() = manifest;
                }
                controls
                    .last_checked
                    .set_label(&last_checked_text(&checked_at));
            }
            Err(error) => {
                crate::logging::record("ERROR", "ollama.update_check", &error);
                controls.status.set_label(&error);
                controls.status.remove_css_class("success");
                controls.status.add_css_class("error");
                controls.progress.set_visible(false);
                controls.activity.set_visible(true);
            }
        }
        controls.check_spinner.stop();
        controls.check_spinner.set_visible(false);
        controls.check_label.set_label("Check for Updates");
        update_controls(&controls, &backend);
        if failed {
            set_summary(&controls, "Update check failed", "error");
        } else if releases::RuntimeVersion::parse(controls.installed.text().as_str()).is_some()
            && !releases::is_newer(
                &controls.candidate.borrow().version,
                controls.installed.text().as_str(),
            )
        {
            set_summary(&controls, "Up to date", "success");
        }
        gtk::glib::ControlFlow::Break
    });
}

fn start_install(
    controls: &Controls,
    ui: &Rc<WindowUi>,
    backend: &Rc<Backend>,
    repair: bool,
    retry: Option<(String, String)>,
) {
    if provider_change_is_blocked(ui, backend) {
        return;
    }
    let setup = (|| -> crate::error::Result<_> {
        let current = ManagedOllamaManager::new(backend.paths.clone())?;
        let manifest = if repair {
            current
                .installed_manifest()
                .ok()
                .flatten()
                .unwrap_or_else(|| controls.candidate.borrow().clone())
        } else {
            controls.candidate.borrow().clone()
        };
        if !repair && current.is_installed() {
            let installed = current.installed_version()?.ok_or_else(|| {
                crate::error::MooseError::ManagedOllamaUpdate(
                    "The installed version is unknown. Use Repair Installation.".into(),
                )
            })?;
            if !releases::is_newer(&manifest.version, &installed) {
                return Err(crate::error::MooseError::ManagedOllamaUpdate(
                    "The installed version is already up to date.".into(),
                ));
            }
        }
        let managed = active_provider(backend).filter(|provider| provider.is_managed);
        let port = match managed.as_ref() {
            Some(provider) => {
                crate::providers::managed_ollama_port_from_base_url(&provider.base_url)?
            }
            None => provider_controls::next_managed_port(backend),
        };
        let config = ManagedOllamaConfig::from_paths_with_port_and_gpu(
            &backend.paths,
            port,
            backend.managed_gpu.borrow().clone(),
        )?;
        let needs_configuration = !current.is_installed()
            && !backend
                .repository
                .list()?
                .iter()
                .any(|provider| provider.is_managed);
        let manager = ManagedOllamaManager::with_manifest(backend.paths.clone(), manifest)?;
        Ok((manager, config, managed.is_some(), needs_configuration))
    })();
    let (manager, config, keep_running, needs_configuration) = match setup {
        Ok(setup) => setup,
        Err(error) => {
            show_error(&ui.window, "Ollama Update", &error);
            return;
        }
    };
    let cancellation = InstallCancellation::default();
    *backend.runtime_install.borrow_mut() = Some(cancellation.clone());
    backend
        .model_load_revision
        .set(backend.model_load_revision.get().wrapping_add(1));
    set_busy(controls, true);
    set_summary(
        controls,
        if repair {
            "Repairing Ollama"
        } else if manager.is_installed() {
            "Updating Ollama"
        } else {
            "Installing Ollama"
        },
        "accent",
    );
    controls.primary.set_visible(false);
    controls.check.set_visible(false);
    controls.last_checked.set_visible(false);
    controls.activity.set_visible(true);
    controls.status.remove_css_class("error");
    controls.status.remove_css_class("success");
    controls.dialog.set_can_close(false);
    controls.cancel.set_visible(true);
    controls.cancel.set_sensitive(true);
    controls.progress.set_fraction(0.0);
    controls.progress.set_visible(true);
    controls
        .status
        .set_label("Preparing Ollama. You can cancel before installation begins.");
    let (sender, receiver) = mpsc::channel();
    let service = Arc::clone(&backend.managed_ollama);
    backend.runtime.spawn(async move {
        let progress_sender = sender.clone();
        let result = update::install(
            manager,
            service,
            update::InstallOptions {
                config,
                keep_running,
                repair,
            },
            cancellation,
            move |progress| {
                let _ = progress_sender.send(Event::Progress(progress));
            },
        )
        .await
        .map_err(|error| error.to_string());
        let _ = sender.send(Event::Finished(result));
    });
    let controls = controls.clone();
    let ui = Rc::clone(ui);
    let backend = Rc::clone(backend);
    gtk::glib::timeout_add_local(Duration::from_millis(100), move || {
        loop {
            let event = match receiver.try_recv() {
                Ok(event) => event,
                Err(mpsc::TryRecvError::Empty) => return gtk::glib::ControlFlow::Continue,
                Err(mpsc::TryRecvError::Disconnected) => Event::Finished(Err(
                    "The installation stopped unexpectedly. Reopen Moose to recover it.".into(),
                )),
            };
            match event {
                Event::Progress(progress) => {
                    if matches!(
                        progress,
                        ManagedOllamaInstallProgress::Installing
                            | ManagedOllamaInstallProgress::Starting
                            | ManagedOllamaInstallProgress::Restoring
                            | ManagedOllamaInstallProgress::Installed { .. }
                    ) {
                        controls.cancel.set_sensitive(false);
                    }
                    match &progress {
                        ManagedOllamaInstallProgress::Starting => {
                            set_summary(&controls, "Starting Ollama", "accent")
                        }
                        ManagedOllamaInstallProgress::Restoring => {
                            set_summary(&controls, "Restoring previous version", "warning")
                        }
                        _ => {}
                    }
                    managed_install::apply_install_progress(
                        &controls.progress,
                        &controls.status,
                        &progress,
                    );
                }
                Event::Finished(result) => {
                    let cancelled = backend
                        .runtime_install
                        .borrow_mut()
                        .take()
                        .is_some_and(|token| token.check().is_err());
                    controls.dialog.set_can_close(true);
                    controls.cancel.set_visible(false);
                    controls.progress.set_visible(false);
                    update_controls(&controls, &backend);
                    match result {
                        Ok(()) => {
                            controls.progress.set_fraction(1.0);
                            set_summary(&controls, "Ready to use", "success");
                            controls.status.add_css_class("success");
                            controls.status.set_label(
                                "Ollama is ready. Your models and chats have been preserved.",
                            );
                            if let Some((provider_id, model)) = retry.as_ref()
                                && active_provider(&backend).is_some_and(|provider| {
                                    provider.is_managed && provider.id == *provider_id
                                })
                            {
                                controls.dialog.close();
                                model_actions::request_model_pull(
                                    &ui.window,
                                    &ui,
                                    &backend,
                                    model.clone(),
                                );
                            } else if needs_configuration {
                                controls.dialog.close();
                                managed_install::show_configuration_dialog(&ui, &backend);
                            } else {
                                refresh_models(&ui, &backend);
                            }
                        }
                        Err(error) => {
                            crate::logging::record("ERROR", "ollama.install", &error);
                            controls.status.set_label(&error);
                            if cancelled {
                                set_summary(&controls, "Installation cancelled", "dim-label");
                            } else {
                                set_summary(&controls, "Installation failed", "error");
                                controls.status.add_css_class("error");
                            }
                            controls.progress.set_fraction(0.0);
                            refresh_models(&ui, &backend);
                        }
                    }
                    return gtk::glib::ControlFlow::Break;
                }
            }
        }
    });
}

pub(super) fn preferences_page(
    dialog: &adw::PreferencesDialog,
    ui: &Rc<WindowUi>,
    backend: &Rc<Backend>,
) -> adw::PreferencesPage {
    let page = adw::PreferencesPage::builder()
        .title("Ollama & Diagnostics")
        .icon_name("network-server-symbolic")
        .build();
    let group = adw::PreferencesGroup::builder()
        .title("Managed Ollama")
        .build();
    let row = adw::ActionRow::builder()
        .title("Manage Ollama")
        .subtitle("Install, update, or repair the local engine")
        .activatable(true)
        .build();
    row.add_suffix(&gtk::Image::from_icon_name("go-next-symbolic"));
    let target_dialog = dialog.clone();
    let target_ui = Rc::clone(ui);
    let target_backend = Rc::clone(backend);
    row.connect_activated(move |_| {
        target_dialog.close();
        show_dialog(&target_ui, &target_backend, None);
    });
    group.add(&row);
    page.add(&group);
    let logs = adw::PreferencesGroup::builder()
        .title("Diagnostics")
        .description("View and copy recent log entries when reporting a problem.")
        .build();
    for (title, ollama) in [("Moose Log", false), ("Ollama Log", true)] {
        let row = adw::ActionRow::builder().title(title).build();
        let button = widgets::icon_button("document-open-symbolic", "View Log");
        let ui = Rc::clone(ui);
        let backend = Rc::clone(backend);
        let dialog = dialog.clone();
        button.connect_clicked(move |_| {
            dialog.close();
            diagnostics::show_logs(&ui, &backend, ollama);
        });
        row.add_suffix(&button);
        row.set_activatable_widget(Some(&button));
        logs.add(&row);
    }
    page.add(&logs);
    page
}
