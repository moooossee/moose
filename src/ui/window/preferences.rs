use std::rc::Rc;

use adw::prelude::*;
use gtk::{Align, Orientation};

use crate::providers::{
    MANAGED_OLLAMA_DEFAULT_PORT, ProviderUpdate, managed_ollama_base_url,
    managed_ollama_port_from_base_url, validate_managed_ollama_port,
};

use super::{
    Backend, WindowUi, active_provider, apply_active_provider, clear_active_provider,
    provider_change_is_blocked, refresh_models, show_error, update_provider_summary, widgets,
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
            .text(&managed_port.to_string())
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

    dialog.add(&provider_page);
    dialog.add(&privacy_page);

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
