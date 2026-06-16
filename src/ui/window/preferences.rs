use std::{collections::HashMap, rc::Rc};

use adw::prelude::*;
use gtk::{Align, Orientation};

use crate::providers::{
    MANAGED_OLLAMA_DEFAULT_PORT, ProviderUpdate, managed_ollama_base_url,
    managed_ollama_port_from_base_url, validate_managed_ollama_port,
};

use super::{
    Backend, WindowUi, active_provider, apply_active_provider, clear_active_provider,
    provider_change_is_blocked, refresh_models, show_error, update_provider_summary, widgets,
    reset_shortcut_values, save_shortcut_values, set_shortcut_capture_active, shortcut_values,
    shortcuts,
};

pub(super) fn dialog(
    parent: &adw::ApplicationWindow,
    ui: &Rc<WindowUi>,
    backend: &Rc<Backend>,
) -> adw::PreferencesDialog {
    let dialog = adw::PreferencesDialog::builder()
        .title("Preferences")
        .search_enabled(false)
        .build();

    let provider = active_provider(backend);
    let provider_name = provider
        .as_ref()
        .map(|provider| provider.name.as_str())
        .unwrap_or("");
    let provider_url = provider
        .as_ref()
        .map(|provider| provider.base_url.as_str())
        .unwrap_or("");
    let managed_port = provider
        .as_ref()
        .filter(|provider| provider.is_managed)
        .and_then(|provider| managed_ollama_port_from_base_url(&provider.base_url).ok())
        .unwrap_or(MANAGED_OLLAMA_DEFAULT_PORT);
    let provider_is_managed = provider
        .as_ref()
        .is_some_and(|provider| provider.is_managed);
    let provider_page = adw::PreferencesPage::builder()
        .title("Provider")
        .icon_name("network-server-symbolic")
        .build();
    let provider_group = adw::PreferencesGroup::builder()
        .title("Ollama Provider")
        .build();
    let name_row = adw::EntryRow::builder()
        .title("Name")
        .text(provider_name)
        .build();
    let url_row = adw::EntryRow::builder()
        .title("Base URL")
        .text(provider_url)
        .build();
    if provider_is_managed {
        url_row.set_editable(false);
    }
    let port_row = provider_is_managed.then(|| {
        adw::EntryRow::builder()
            .title("Managed Port")
            .text(managed_port.to_string())
            .build()
    });
    let action_box = gtk::Box::builder()
        .orientation(Orientation::Horizontal)
        .spacing(6)
        .halign(Align::End)
        .margin_top(6)
        .margin_bottom(6)
        .build();
    let save_button = widgets::icon_button("document-save-symbolic", "Save Provider");
    let add_button = widgets::icon_button("list-add-symbolic", "Add Provider");
    let delete_button = widgets::icon_button("user-trash-symbolic", "Delete Provider");

    save_button.add_css_class("suggested-action");
    delete_button.add_css_class("destructive-action");
    save_button.set_sensitive(provider.is_some());
    delete_button.set_sensitive(provider.is_some());
    action_box.append(&add_button);
    action_box.append(&delete_button);
    action_box.append(&save_button);
    if provider_is_managed {
        provider_group.add(
            &adw::ActionRow::builder()
                .title("Managed by Moose")
                .subtitle("Local sandbox service")
                .build(),
        );
    }
    provider_group.add(&name_row);
    if let Some(port_row) = port_row.as_ref() {
        provider_group.add(port_row);
    }
    provider_group.add(&url_row);
    provider_group.add(&action_box);
    provider_page.add(&provider_group);

    let privacy_page = adw::PreferencesPage::builder()
        .title("Privacy")
        .icon_name("changes-prevent-symbolic")
        .build();
    let privacy_group = adw::PreferencesGroup::builder().title("Local Data").build();
    privacy_group.add(
        &adw::ActionRow::builder()
            .title("Telemetry")
            .subtitle("Disabled")
            .build(),
    );
    privacy_group.add(
        &adw::ActionRow::builder()
            .title("Conversation Storage")
            .subtitle("Local application data")
            .build(),
    );
    privacy_page.add(&privacy_group);

    let shortcuts_page = adw::PreferencesPage::builder()
        .title("Shortcuts")
        .icon_name("preferences-desktop-keyboard-symbolic")
        .build();
    let shortcuts_group = adw::PreferencesGroup::builder()
        .title("Keyboard Shortcuts")
        .description("Select a command, then press the shortcut you want to use.")
        .build();
    let current_shortcuts = shortcut_values(backend);
    let mut shortcut_rows = Vec::new();
    for definition in shortcuts::DEFINITIONS {
        let value = current_shortcuts
            .get(definition.id)
            .map(String::as_str)
            .unwrap_or(definition.default);
        let row = adw::ActionRow::builder()
            .title(definition.title)
            .subtitle(definition.description)
            .subtitle_lines(2)
            .build();
        let button = shortcut_button(value);
        let target_dialog = dialog.clone();
        let target_ui = Rc::clone(ui);
        let target_backend = Rc::clone(backend);
        let id = definition.id.to_string();
        let title = definition.title.to_string();
        let target_button = button.clone();
        button.connect_clicked(move |_| {
            show_shortcut_capture_dialog(
                &target_dialog,
                &target_ui,
                &target_backend,
                &id,
                &title,
                &target_button,
            );
        });
        row.add_suffix(&button);
        row.set_activatable_widget(Some(&button));
        shortcuts_group.add(&row);
        shortcut_rows.push((definition.id.to_string(), button));
    }
    let shortcut_rows = Rc::new(shortcut_rows);
    let shortcut_actions = gtk::Box::builder()
        .orientation(Orientation::Horizontal)
        .spacing(6)
        .halign(Align::End)
        .margin_top(6)
        .margin_bottom(6)
        .build();
    let reset_shortcuts_button = gtk::Button::with_label("Reset Shortcuts");
    shortcut_actions.append(&reset_shortcuts_button);
    shortcuts_group.add(&shortcut_actions);
    shortcuts_page.add(&shortcuts_group);

    dialog.add(&provider_page);
    dialog.add(&shortcuts_page);
    dialog.add(&privacy_page);

    let target_dialog = dialog.clone();
    let target_ui = Rc::clone(ui);
    let target_backend = Rc::clone(backend);
    let target_rows = Rc::clone(&shortcut_rows);
    reset_shortcuts_button.connect_clicked(move |_| {
        match reset_shortcut_values(&target_ui, &target_backend) {
            Ok(values) => apply_shortcut_row_values(target_rows.as_ref().as_slice(), &values),
            Err(message) => target_dialog.add_toast(adw::Toast::new(&message)),
        }
    });

    if let Some(port_row) = port_row.as_ref() {
        let target_url_row = url_row.clone();
        port_row.connect_text_notify(move |row| {
            if let Ok(port) = managed_port_from_row(row)
                && let Ok(base_url) = managed_ollama_base_url(port)
            {
                target_url_row.set_text(&base_url);
            }
        });
    }

    let target_ui = Rc::clone(ui);
    let target_backend = Rc::clone(backend);
    let target_parent = parent.clone();
    let target_name_row = name_row.clone();
    let target_url_row = url_row.clone();
    let target_port_row = port_row.clone();
    save_button.connect_clicked(move |_| {
        if provider_change_is_blocked(&target_ui, &target_backend) {
            return;
        }

        let Some(current) = active_provider(&target_backend) else {
            target_ui
                .toast_overlay
                .add_toast(adw::Toast::new("Create or connect an instance first"));
            return;
        };
        let base_url = if current.is_managed {
            let Some(port_row) = target_port_row.as_ref() else {
                target_ui
                    .toast_overlay
                    .add_toast(adw::Toast::new("Managed port is unavailable"));
                return;
            };
            match managed_port_from_row(port_row).and_then(managed_ollama_base_url) {
                Ok(base_url) => base_url,
                Err(error) => {
                    show_error(&target_parent, "Managed port is invalid", &error);
                    return;
                }
            }
        } else {
            target_url_row.text().to_string()
        };
        match target_backend.repository.update(ProviderUpdate {
            id: current.id,
            name: target_name_row.text().to_string(),
            base_url,
            is_default: true,
        }) {
            Ok(provider) => {
                *target_backend.provider.borrow_mut() = Some(provider.clone());
                target_url_row.set_text(&provider.base_url);
                if let Some(port_row) = target_port_row.as_ref()
                    && let Ok(port) = managed_ollama_port_from_base_url(&provider.base_url)
                {
                    port_row.set_text(&port.to_string());
                }
                update_provider_summary(&target_ui, &provider);
                refresh_models(&target_ui, &target_backend);
                target_ui
                    .toast_overlay
                    .add_toast(adw::Toast::new("Provider saved"));
            }
            Err(error) => show_error(&target_parent, "Provider could not be saved", &error),
        }
    });

    let target_ui = Rc::clone(ui);
    let target_backend = Rc::clone(backend);
    add_button.connect_clicked(move |_| {
        if provider_change_is_blocked(&target_ui, &target_backend) {
            return;
        }

        super::show_connect_external_dialog(&target_ui, &target_backend);
    });

    let target_ui = Rc::clone(ui);
    let target_backend = Rc::clone(backend);
    let target_parent = parent.clone();
    let target_name_row = name_row;
    let target_url_row = url_row;
    delete_button.connect_clicked(move |_| {
        if provider_change_is_blocked(&target_ui, &target_backend) {
            return;
        }

        let Some(current) = active_provider(&target_backend) else {
            target_ui
                .toast_overlay
                .add_toast(adw::Toast::new("Create or connect an instance first"));
            return;
        };
        match target_backend
            .repository
            .delete(&current.id)
            .and_then(|_| target_backend.repository.ensure_default_provider())
        {
            Ok(Some(provider)) => {
                target_name_row.set_text(&provider.name);
                target_url_row.set_text(&provider.base_url);
                target_url_row.set_editable(!provider.is_managed);
                apply_active_provider(&target_ui, &target_backend, provider);
                target_ui
                    .toast_overlay
                    .add_toast(adw::Toast::new("Provider removed"));
            }
            Ok(None) => {
                target_name_row.set_text("");
                target_url_row.set_text("");
                target_url_row.set_editable(true);
                clear_active_provider(&target_ui, &target_backend);
                target_ui
                    .toast_overlay
                    .add_toast(adw::Toast::new("Provider removed"));
            }
            Err(error) => show_error(&target_parent, "Provider could not be removed", &error),
        }
    });

    dialog
}

fn managed_port_from_row(row: &adw::EntryRow) -> crate::error::Result<u16> {
    let port = row
        .text()
        .trim()
        .parse::<u16>()
        .map_err(|_| crate::error::MooseError::ManagedOllamaInvalidPort(0))?;
    validate_managed_ollama_port(port)
}

fn shortcut_button(value: &str) -> gtk::Button {
    let button = gtk::Button::with_label(&shortcut_button_label(value));
    button.add_css_class("flat");
    button
}

fn shortcut_button_label(value: &str) -> String {
    if value.trim().is_empty() {
        "Disabled".to_string()
    } else {
        value.to_string()
    }
}

fn show_shortcut_capture_dialog(
    parent: &adw::PreferencesDialog,
    ui: &Rc<WindowUi>,
    backend: &Rc<Backend>,
    id: &str,
    title: &str,
    button: &gtk::Button,
) {
    set_shortcut_capture_active(backend, true);
    let dialog = adw::Dialog::builder()
        .title(format!("Set {title} Shortcut"))
        .content_width(420)
        .build();
    let content = gtk::Box::builder()
        .orientation(Orientation::Vertical)
        .spacing(16)
        .margin_top(22)
        .margin_bottom(22)
        .margin_start(22)
        .margin_end(22)
        .build();
    let label = gtk::Label::builder()
        .label(format!("Press the new shortcut for {title}."))
        .halign(Align::Start)
        .xalign(0.0)
        .wrap(true)
        .build();
    let hint = gtk::Label::builder()
        .label("Use Ctrl, Alt, Shift or Super with printable keys. Esc can be captured here.")
        .halign(Align::Start)
        .xalign(0.0)
        .wrap(true)
        .build();
    hint.add_css_class("dim-label");
    let actions = gtk::Box::builder()
        .orientation(Orientation::Horizontal)
        .spacing(8)
        .halign(Align::End)
        .build();
    let clear_button = gtk::Button::with_label("Clear");
    let cancel_button = gtk::Button::with_label("Cancel");
    actions.append(&clear_button);
    actions.append(&cancel_button);
    content.append(&label);
    content.append(&hint);
    content.append(&actions);
    dialog.set_child(Some(&content));

    let target_dialog = dialog.clone();
    let target_backend = Rc::clone(backend);
    cancel_button.connect_clicked(move |_| {
        set_shortcut_capture_active(&target_backend, false);
        target_dialog.close();
    });

    let target_parent = parent.clone();
    let target_ui = Rc::clone(ui);
    let target_backend = Rc::clone(backend);
    let target_id = id.to_string();
    let target_button = button.clone();
    let target_dialog = dialog.clone();
    clear_button.connect_clicked(move |_| {
        if set_shortcut_value(
            &target_parent,
            &target_ui,
            &target_backend,
            &target_id,
            "",
            &target_button,
        ) {
            set_shortcut_capture_active(&target_backend, false);
            target_dialog.close();
        }
    });

    let target_parent = parent.clone();
    let target_ui = Rc::clone(ui);
    let target_backend = Rc::clone(backend);
    let target_id = id.to_string();
    let target_button = button.clone();
    let target_dialog = dialog.clone();
    let key_controller = gtk::EventControllerKey::new();
    key_controller.set_propagation_phase(gtk::PropagationPhase::Capture);
    key_controller.connect_key_pressed(move |_, key, _, state| {
        let Some(chord) = shortcuts::event_chord(key, state) else {
            target_parent.add_toast(adw::Toast::new("This key cannot be used as a shortcut"));
            return gtk::glib::Propagation::Stop;
        };
        let value = chord.label();
        if set_shortcut_value(
            &target_parent,
            &target_ui,
            &target_backend,
            &target_id,
            &value,
            &target_button,
        ) {
            set_shortcut_capture_active(&target_backend, false);
            target_dialog.close();
        }
        gtk::glib::Propagation::Stop
    });
    dialog.add_controller(key_controller);

    dialog.present(Some(parent));
}

fn set_shortcut_value(
    parent: &adw::PreferencesDialog,
    ui: &WindowUi,
    backend: &Backend,
    id: &str,
    value: &str,
    button: &gtk::Button,
) -> bool {
    let mut values = shortcut_values(backend);
    values.insert(id.to_string(), value.to_string());
    match save_shortcut_values(ui, backend, values) {
        Ok(()) => {
            button.set_label(&shortcut_button_label(value));
            true
        }
        Err(message) => {
            parent.add_toast(adw::Toast::new(&message));
            false
        }
    }
}

fn apply_shortcut_row_values(rows: &[(String, gtk::Button)], values: &HashMap<String, String>) {
    for (id, button) in rows {
        if let Some(value) = values.get(id) {
            button.set_label(&shortcut_button_label(value));
        }
    }
}
