use super::*;
use crate::providers::{
    NewProvider,
    credentials::{self, ApiKey},
};

fn password_entry() -> gtk::PasswordEntry {
    gtk::PasswordEntry::builder()
        .show_peek_icon(false)
        .hexpand(true)
        .build()
}

pub(super) fn show(ui: &Rc<WindowUi>, backend: &Rc<Backend>) {
    if provider_change_is_blocked(ui, backend) {
        return;
    }
    let dialog = adw::Dialog::builder()
        .title("Connect Cloud Provider")
        .content_width(560)
        .content_height(580)
        .build();
    let header_bar = adw::HeaderBar::builder()
        .show_start_title_buttons(false)
        .show_end_title_buttons(false)
        .build();
    let description = gtk::Label::builder()
        .label("Connecting enables remote connections. Moose asks before sharing chats or files with this provider.")
        .wrap(true)
        .wrap_mode(gtk::pango::WrapMode::WordChar)
        .width_chars(1)
        .xalign(0.0)
        .build();
    description.add_css_class("dim-label");
    let group = adw::PreferencesGroup::builder().title("Provider").build();
    let labels = ProviderKind::CLOUD.map(ProviderKind::label);
    let kinds = adw::ComboRow::builder()
        .title("Provider")
        .model(&gtk::StringList::new(&labels))
        .build();
    let name = adw::EntryRow::builder()
        .title("Name")
        .text(labels[0])
        .build();
    let endpoint = adw::ActionRow::builder()
        .title("Server")
        .subtitle(ProviderKind::CLOUD[0].base_url())
        .subtitle_lines(2)
        .build();
    group.add(&kinds);
    group.add(&name);
    group.add(&endpoint);
    let key = password_entry();
    key.set_placeholder_text(Some("Enter your API key"));
    key.set_activates_default(true);
    key.set_height_request(44);
    let key_group = adw::PreferencesGroup::builder()
        .title("API Key")
        .description("Saved securely in your desktop keyring and excluded from chats and exports.")
        .build();
    key_group.add(&key);
    let hint = gtk::Label::builder()
        .label(provider_privacy_hint(ProviderKind::CLOUD[0]))
        .wrap(true)
        .wrap_mode(gtk::pango::WrapMode::WordChar)
        .width_chars(1)
        .xalign(0.0)
        .build();
    hint.add_css_class("dim-label");
    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(18)
        .build();
    content.add_css_class("moose-instance-content");
    content.append(&description);
    content.append(&group);
    content.append(&key_group);
    content.append(&hint);
    let scrolled = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .child(&content)
        .build();
    let cancel_button = gtk::Button::with_label("Cancel");
    let connect_button = gtk::Button::with_label("Save Key and Connect");
    connect_button.add_css_class("suggested-action");
    connect_button.set_sensitive(false);
    let actions = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(10)
        .homogeneous(true)
        .build();
    actions.add_css_class("moose-instance-actions");
    actions.append(&cancel_button);
    actions.append(&connect_button);
    let toolbar_view = adw::ToolbarView::builder()
        .top_bar_style(adw::ToolbarStyle::Flat)
        .bottom_bar_style(adw::ToolbarStyle::Flat)
        .content(&scrolled)
        .build();
    toolbar_view.add_top_bar(&header_bar);
    toolbar_view.add_bottom_bar(&actions);
    dialog.set_child(Some(&toolbar_view));
    dialog.set_default_widget(Some(&connect_button));
    dialog.set_focus(Some(&kinds));
    let target_dialog = dialog.clone();
    cancel_button.connect_clicked(move |_| {
        target_dialog.close();
    });
    let target_key = key.clone();
    dialog.connect_closed(move |_| {
        target_key.set_text("");
    });
    let weak = connect_button.downgrade();
    key.connect_changed(move |entry| {
        if let Some(button) = weak.upgrade() {
            button.set_sensitive(!entry.text().is_empty());
        }
    });
    let target_name = name.clone();
    let target_key = key.clone();
    kinds.connect_selected_notify(move |row| {
        let kind = ProviderKind::CLOUD[row.selected() as usize];
        target_key.set_text("");
        target_name.set_text(kind.label());
        endpoint.set_subtitle(kind.base_url());
        hint.set_label(provider_privacy_hint(kind));
    });
    let target_ui = Rc::clone(ui);
    let target_backend = Rc::clone(backend);
    let target_dialog = dialog.clone();
    connect_button.connect_clicked(move |_| {
        let value = key.text().to_string();
        key.set_text("");
        let kind = ProviderKind::CLOUD[kinds.selected() as usize];
        let result = ApiKey::new(value).and_then(|key| {
            NewProvider {
                kind,
                name: name.text().to_string(),
                base_url: kind.base_url().into(),
                is_managed: false,
                is_default: true,
            }.into_provider().map(|provider| (provider, key))
        });
        let (provider, key) = match result {
            Ok(value) => value,
            Err(error) => {
                show_error(&target_ui.window, "Provider could not be connected", &error);
                return;
            }
        };
        if provider_change_is_blocked(&target_ui, &target_backend) {
            return;
        }
        target_dialog.close();
        target_backend.credential_operation.set(true);
        target_ui.toast_overlay.add_toast(adw::Toast::new(
            "Saving key securely… Unlock your desktop keyring if requested."
        ));
        let stored_provider = provider.clone();
        let handle = target_backend.runtime.spawn(async move {
            credentials::save(&stored_provider, &key).await
        });
        let ui = Rc::clone(&target_ui);
        let backend = Rc::clone(&target_backend);
        gtk::glib::spawn_future_local(async move {
            let result = handle.await;
            backend.credential_operation.set(false);
            match result {
                Ok(Ok(())) => match backend.repository.insert_remote(provider.clone()) {
                    Ok(provider) => {
                        backend.network_policy.set_local_only(false);
                        apply_active_provider(&ui, &backend, provider);
                        show_chat(&ui);
                        ui.toast_overlay.add_toast(adw::Toast::new("Provider connected · Key saved securely"));
                    }
                    Err(error) => {
                        let cleanup = backend.runtime.spawn(async move { credentials::delete(&provider).await });
                        let cleaned = matches!(cleanup.await, Ok(Ok(())));
                        show_error(&ui.window, "Provider could not be saved", &error);
                        if !cleaned {
                            ui.toast_overlay.add_toast(adw::Toast::new("An unused Moose key remains in the desktop keyring. Remove it using your keyring manager."));
                        }
                    }
                },
                Ok(Err(error)) => show_error(&ui.window, "API key could not be saved securely", &error),
                Err(_) => ui.toast_overlay.add_toast(adw::Toast::new("Credential operation stopped. Try again.")),
            }
        });
    });
    dialog.present(Some(&ui.window));
}

fn provider_privacy_hint(kind: ProviderKind) -> &'static str {
    match kind {
        ProviderKind::Groq => {
            "Enable Zero Data Retention in your Groq console for stricter privacy."
        }
        ProviderKind::Gemini => {
            "Use a billing-enabled project for Gemini's paid-service data policy."
        }
        _ => "Shared content is processed under the provider's own data policy.",
    }
}

pub(super) fn edit_key(ui: &Rc<WindowUi>, backend: &Rc<Backend>, provider: Provider) {
    if provider_change_is_blocked(ui, backend) {
        return;
    }
    let key = password_entry();
    let dialog=adw::AlertDialog::builder().heading(format!("{} API Key",provider.name))
        .body("Enter a replacement key, or remove the saved key. Existing keys are never displayed. Your desktop may ask you to unlock its keyring.")
        .extra_child(&key).close_response("cancel").default_response("cancel").build();
    dialog.add_response("cancel", "Cancel");
    dialog.add_response("delete", "Remove Key");
    dialog.add_response("save", "Save Key");
    dialog.set_response_appearance("delete", adw::ResponseAppearance::Destructive);
    dialog.set_response_appearance("save", adw::ResponseAppearance::Suggested);
    dialog.set_response_enabled("save", false);
    let weak = dialog.downgrade();
    key.connect_changed(move |entry| {
        if let Some(dialog) = weak.upgrade() {
            dialog.set_response_enabled("save", !entry.text().is_empty());
        }
    });
    let target_ui = Rc::clone(ui);
    let target_backend = Rc::clone(backend);
    dialog.connect_response(None, move |_, response| {
        let value = if response == "save" {
            Some(key.text().to_string())
        } else {
            None
        };
        key.set_text("");
        if response != "save" && response != "delete" {
            return;
        }
        let api_key = match value.map(ApiKey::new).transpose() {
            Ok(key) => key,
            Err(error) => {
                show_error(&target_ui.window, "Invalid API key", &error);
                return;
            }
        };
        if provider_change_is_blocked(&target_ui, &target_backend) {
            return;
        }
        target_backend.credential_operation.set(true);
        target_backend
            .model_load_revision
            .set(target_backend.model_load_revision.get().wrapping_add(1));
        target_ui
            .toast_overlay
            .add_toast(adw::Toast::new("Updating the desktop keyring…"));
        let provider = provider.clone();
        let provider_id = provider.id.clone();
        let removed = api_key.is_none();
        let handle = target_backend.runtime.spawn(async move {
            if let Some(key) = api_key {
                credentials::save(&provider, &key).await
            } else {
                credentials::delete(&provider).await
            }
        });
        let ui = Rc::clone(&target_ui);
        let backend = Rc::clone(&target_backend);
        gtk::glib::spawn_future_local(async move {
            let result = handle.await;
            backend.credential_operation.set(false);
            match result {
                Ok(Ok(())) => {
                    ui.toast_overlay.add_toast(adw::Toast::new(if removed {
                        "API key removed"
                    } else {
                        "API key saved securely"
                    }));
                    if active_provider(&backend).is_some_and(|p| p.id == provider_id) {
                        ui.attachments.capabilities.borrow_mut().clear();
                        ui.thinking_state.borrow_mut().cache.clear();
                        if removed {
                            set_model_picker(&ui, Vec::new(), None);
                            set_installed_models(&ui, &backend, Vec::new());
                            ui.provider_status.set_text("API Key Required");
                        } else {
                            refresh_models(&ui, &backend);
                        }
                    }
                }
                Ok(Err(error)) => show_error(&ui.window, "Secure key operation failed", &error),
                Err(_) => ui
                    .toast_overlay
                    .add_toast(adw::Toast::new("Credential operation stopped. Try again.")),
            }
        });
    });
    dialog.present(Some(&ui.window));
}

pub(super) fn delete_provider(ui: &Rc<WindowUi>, backend: &Rc<Backend>, provider: Provider) {
    match backend.repository.has_conversations(&provider.id) {
        Ok(false) => {}
        Ok(true) => {
            ui.toast_overlay.add_toast(adw::Toast::new("This provider has saved chats. Remove its API key in Preferences, or delete those chats first."));
            return;
        }
        Err(error) => {
            show_error(&ui.window, "Provider could not be deleted", &error);
            return;
        }
    }
    backend.credential_operation.set(true);
    let id = provider.id.clone();
    let handle = backend
        .runtime
        .spawn(async move { credentials::delete(&provider).await });
    let ui = Rc::clone(ui);
    let backend = Rc::clone(backend);
    gtk::glib::spawn_future_local(async move {
        let result = handle.await;
        backend.credential_operation.set(false);
        match result {
            Ok(Ok(())) => provider_controls::delete_provider_record(&ui, &backend, &id),
            Ok(Err(error)) => show_error(
                &ui.window,
                "API key could not be removed; provider kept",
                &error,
            ),
            Err(_) => ui.toast_overlay.add_toast(adw::Toast::new(
                "Credential operation stopped. Provider kept.",
            )),
        }
    });
}
