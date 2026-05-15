use std::rc::Rc;

use adw::prelude::*;
use gtk::{Align, Orientation};

use crate::providers::{DEFAULT_OLLAMA_BASE_URL, NewProvider, ProviderKind, ProviderUpdate};

use super::{
    Backend, WindowUi, apply_active_provider, provider_change_is_blocked, refresh_models,
    show_error, update_provider_summary, widgets,
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

    let provider = backend.provider.borrow().clone();
    let provider_page = adw::PreferencesPage::builder()
        .title("Provider")
        .icon_name("network-server-symbolic")
        .build();
    let provider_group = adw::PreferencesGroup::builder()
        .title("Ollama Provider")
        .build();
    let name_row = adw::EntryRow::builder()
        .title("Name")
        .text(&provider.name)
        .build();
    let url_row = adw::EntryRow::builder()
        .title("Base URL")
        .text(&provider.base_url)
        .build();
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
    action_box.append(&add_button);
    action_box.append(&delete_button);
    action_box.append(&save_button);
    provider_group.add(&name_row);
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

    let target_ui = Rc::clone(ui);
    let target_backend = Rc::clone(backend);
    let target_parent = parent.clone();
    let target_name_row = name_row.clone();
    let target_url_row = url_row.clone();
    save_button.connect_clicked(move |_| {
        if provider_change_is_blocked(&target_ui, &target_backend) {
            return;
        }

        let current = target_backend.provider.borrow().clone();
        match target_backend.repository.update(ProviderUpdate {
            id: current.id,
            name: target_name_row.text().to_string(),
            base_url: target_url_row.text().to_string(),
            is_default: true,
        }) {
            Ok(provider) => {
                *target_backend.provider.borrow_mut() = provider.clone();
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
    let target_parent = parent.clone();
    let target_name_row = name_row.clone();
    let target_url_row = url_row.clone();
    add_button.connect_clicked(move |_| {
        if provider_change_is_blocked(&target_ui, &target_backend) {
            return;
        }

        let count = target_backend
            .repository
            .list()
            .map(|items| items.len() + 1);
        let name = count
            .map(|count| format!("Ollama Provider {count}"))
            .unwrap_or_else(|_| "Ollama Provider".to_string());
        match target_backend.repository.create(NewProvider {
            kind: ProviderKind::Ollama,
            name,
            base_url: DEFAULT_OLLAMA_BASE_URL.to_string(),
            is_managed: false,
            is_default: true,
        }) {
            Ok(provider) => {
                target_name_row.set_text(&provider.name);
                target_url_row.set_text(&provider.base_url);
                apply_active_provider(&target_ui, &target_backend, provider);
                target_ui
                    .toast_overlay
                    .add_toast(adw::Toast::new("Provider added"));
            }
            Err(error) => show_error(&target_parent, "Provider could not be added", &error),
        }
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

        let current = target_backend.provider.borrow().clone();
        match target_backend
            .repository
            .delete(&current.id)
            .and_then(|_| target_backend.repository.ensure_default_provider())
        {
            Ok(provider) => {
                target_name_row.set_text(&provider.name);
                target_url_row.set_text(&provider.base_url);
                apply_active_provider(&target_ui, &target_backend, provider);
                target_ui
                    .toast_overlay
                    .add_toast(adw::Toast::new("Provider removed"));
            }
            Err(error) => show_error(&target_parent, "Provider could not be removed", &error),
        }
    });

    dialog
}
