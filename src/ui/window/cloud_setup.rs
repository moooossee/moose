use super::*;
use crate::providers::{
    NewProvider,
    credentials::{self, ApiKey},
    policy::{RemotePermissions, destination},
    validate_provider_name,
};

use super::custom_provider::CustomProviderFields;

fn password_entry() -> gtk::PasswordEntry {
    gtk::PasswordEntry::builder()
        .show_peek_icon(false)
        .hexpand(true)
        .build()
}

pub(super) fn show(ui: &Rc<WindowUi>, backend: &Rc<Backend>) {
    show_connection(ui, backend, None);
}

pub(super) fn edit_custom(ui: &Rc<WindowUi>, backend: &Rc<Backend>, provider: Provider) {
    if provider.kind == ProviderKind::Custom {
        show_connection(ui, backend, Some(provider));
    }
}

fn show_connection(ui: &Rc<WindowUi>, backend: &Rc<Backend>, previous: Option<Provider>) {
    if provider_change_is_blocked(ui, backend) {
        return;
    }
    let editing = previous.is_some();
    let initial_kind = previous
        .as_ref()
        .map_or(ProviderKind::CLOUD[0], |provider| provider.kind);
    let dialog = adw::Dialog::builder()
        .title(if editing {
            "Configure Custom Provider"
        } else {
            "Connect Cloud Provider"
        })
        .content_width(580)
        .content_height(680)
        .build();
    let header_bar = adw::HeaderBar::builder()
        .show_start_title_buttons(false)
        .show_end_title_buttons(false)
        .build();
    let description = gtk::Label::builder()
        .label(if editing {
            "Changing the server, API format or authentication resets sharing permissions and requires a new API key. Leave the key blank to keep it when these settings stay the same."
        } else {
            "Choose a provider or add a custom connection. Connecting enables remote connections. Shared messages and files are processed under the provider's own data policy."
        })
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
        .selected(
            ProviderKind::CLOUD
                .iter()
                .position(|kind| *kind == initial_kind)
                .unwrap_or(0) as u32,
        )
        .visible(!editing)
        .build();
    let name = adw::EntryRow::builder()
        .title("Name")
        .text(
            previous
                .as_ref()
                .map_or(initial_kind.label(), |provider| provider.name.as_str()),
        )
        .build();
    let endpoint = adw::ActionRow::builder()
        .title("Server")
        .subtitle(initial_kind.default_base_url().unwrap_or_default())
        .subtitle_lines(2)
        .visible(initial_kind != ProviderKind::Custom)
        .build();
    let custom_url = adw::EntryRow::builder()
        .title("Base URL")
        .text(
            previous
                .as_ref()
                .map_or("", |provider| provider.base_url.as_str()),
        )
        .visible(initial_kind == ProviderKind::Custom)
        .build();
    let url_hint = gtk::Label::builder()
        .label("Enter the API base URL, including its version, such as https://example.com/v1. Do not append /chat/completions or another endpoint.")
        .wrap(true)
        .width_chars(1)
        .xalign(0.0)
        .visible(initial_kind == ProviderKind::Custom)
        .build();
    url_hint.add_css_class("dim-label");
    group.add(&kinds);
    group.add(&name);
    group.add(&endpoint);
    group.add(&custom_url);
    let custom_fields = CustomProviderFields::new(
        &previous
            .as_ref()
            .and_then(|provider| provider.custom_config.clone())
            .unwrap_or_default(),
    );
    custom_fields.set_visible(initial_kind == ProviderKind::Custom);
    let sharing_group = adw::PreferencesGroup::builder()
        .title("Sharing Permissions")
        .visible(!editing)
        .build();
    let message_permission = super::privacy::message_permission_row();
    sharing_group.add(&message_permission);
    let key = password_entry();
    key.set_placeholder_text(Some(if editing {
        "Leave blank to keep the saved key"
    } else {
        "Enter your API key"
    }));
    key.set_activates_default(true);
    key.set_height_request(44);
    custom_fields.clear_key_on_destination_change(&key);
    let target_key = key.downgrade();
    custom_url.connect_text_notify(move |_| {
        if let Some(key) = target_key.upgrade() {
            key.set_text("");
        }
    });
    let key_group = adw::PreferencesGroup::builder()
        .title("API Key")
        .description("Saved securely in your desktop keyring and excluded from chats and exports.")
        .visible(
            initial_kind != ProviderKind::Custom || custom_fields.authentication.selected() != 2,
        )
        .build();
    key_group.add(&key);
    let hint = gtk::Label::builder()
        .label(provider_privacy_hint(initial_kind))
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
    content.append(&url_hint);
    content.append(&custom_fields.connection_group);
    content.append(&key_group);
    content.append(&custom_fields.models_group);
    content.append(&sharing_group);
    content.append(&hint);
    let scrolled = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .child(&content)
        .build();
    let cancel_button = gtk::Button::with_label("Cancel");
    let connect_button = gtk::Button::with_label(if editing {
        "Save Changes"
    } else {
        "Save and Connect"
    });
    connect_button.add_css_class("suggested-action");
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
    if editing {
        dialog.set_focus(Some(&name));
    } else {
        dialog.set_focus(Some(&kinds));
    }
    let target_dialog = dialog.downgrade();
    cancel_button.connect_clicked(move |_| {
        if let Some(dialog) = target_dialog.upgrade() {
            dialog.close();
        }
    });
    let target_key = key.clone();
    dialog.connect_closed(move |_| {
        target_key.set_text("");
    });
    let target_group = key_group.downgrade();
    let target_key = key.clone();
    let target_kinds = kinds.downgrade();
    custom_fields
        .authentication
        .connect_selected_notify(move |row| {
            if let (Some(group), Some(kinds)) = (target_group.upgrade(), target_kinds.upgrade()) {
                let custom = ProviderKind::CLOUD[kinds.selected() as usize] == ProviderKind::Custom;
                group.set_visible(!custom || row.selected() != 2);
                target_key.set_text("");
            }
        });
    let target_name = name.clone();
    let target_key = key.clone();
    let target_message_permission = message_permission.clone();
    let target_custom_fields = custom_fields.clone();
    let target_custom_url = custom_url.clone();
    kinds.connect_selected_notify(move |row| {
        let kind = ProviderKind::CLOUD[row.selected() as usize];
        let custom = kind == ProviderKind::Custom;
        target_key.set_text("");
        target_message_permission.set_selected(0);
        target_name.set_text(kind.label());
        endpoint.set_subtitle(kind.default_base_url().unwrap_or_default());
        endpoint.set_visible(!custom);
        target_custom_url.set_visible(custom);
        url_hint.set_visible(custom);
        target_custom_fields.set_visible(custom);
        key_group.set_visible(!custom || target_custom_fields.authentication.selected() != 2);
        hint.set_label(provider_privacy_hint(kind));
    });
    let target_ui = Rc::clone(ui);
    let target_backend = Rc::clone(backend);
    let target_dialog = dialog.downgrade();
    connect_button.connect_clicked(move |_| {
        let Some(dialog) = target_dialog.upgrade() else {
            return;
        };
        if provider_change_is_blocked(&target_ui, &target_backend) {
            return;
        }
        let kind = ProviderKind::CLOUD[kinds.selected() as usize];
        let result = (|| -> Result<(Provider, Option<ApiKey>)> {
            if let Some(previous) = &previous
                && target_backend.repository.get(&previous.id)?.as_ref() != Some(previous)
            {
                return Err(MooseError::ProviderRequest(
                    "The provider changed. Reopen its settings and try again.".into(),
                ));
            }
            let mut provider = if let Some(previous) = &previous {
                let mut provider = previous.clone();
                provider.name = validate_provider_name(&name.text())?;
                provider.base_url = kind.validate_url(&custom_url.text())?;
                provider
            } else {
                NewProvider {
                    kind,
                    name: name.text().to_string(),
                    base_url: if kind == ProviderKind::Custom {
                        custom_url.text().to_string()
                    } else {
                        kind.default_base_url()
                            .ok_or(MooseError::InvalidProviderUrl)?
                            .into()
                    },
                    is_managed: false,
                    is_default: true,
                }
                .into_provider()?
            };
            if kind == ProviderKind::Custom {
                provider.custom_config = Some(custom_fields.config()?);
            }
            provider.validate_configuration()?;
            let keeping_key = previous
                .as_ref()
                .is_some_and(|previous| destination(previous) == destination(&provider));
            let api_key = if provider.requires_key() && !key.text().is_empty() {
                Some(ApiKey::new(key.text().to_string())?)
            } else if provider.requires_key() && !keeping_key {
                return Err(MooseError::ProviderRequest(
                    "Enter an API key for this server and authentication method.".into(),
                ));
            } else {
                None
            };
            Ok((provider, api_key))
        })();
        let (provider, api_key) = match result {
            Ok(value) => value,
            Err(error) => {
                show_error(&dialog, "Provider could not be saved", &error);
                return;
            }
        };
        if let Err(error) = workspace::save(&target_ui, &target_backend) {
            show_error(&dialog, "Draft could not be saved", &error);
            return;
        }
        key.set_text("");
        let permissions = RemotePermissions {
            messages: message_permission.selected() == 1,
            files: false,
        };
        save_connection(
            &target_ui,
            &target_backend,
            &dialog,
            provider,
            previous.clone(),
            api_key,
            permissions,
        );
    });
    dialog.present(Some(&ui.window));
}

fn save_connection(
    ui: &Rc<WindowUi>,
    backend: &Rc<Backend>,
    dialog: &adw::Dialog,
    provider: Provider,
    previous: Option<Provider>,
    key: Option<ApiKey>,
    permissions: RemotePermissions,
) {
    let changed_destination = previous
        .as_ref()
        .is_none_or(|previous| destination(previous) != destination(&provider));
    let key_written = key.is_some();
    if key_written {
        ui.toast_overlay.add_toast(adw::Toast::new(
            "Saving key securely… Unlock your desktop keyring if requested.",
        ));
    }
    attachments::invalidate_capabilities(ui);
    reasoning::reset(ui);
    dialog.set_can_close(false);
    dialog.set_sensitive(false);
    backend.credential_operation.set(true);
    backend
        .model_load_revision
        .set(backend.model_load_revision.get().wrapping_add(1));
    let stored_provider = provider.clone();
    let replacing_key = key_written && previous.is_some() && !changed_destination;
    let handle = backend.runtime.spawn(async move {
        let old_key = if replacing_key {
            match credentials::load(&stored_provider).await {
                Ok(key) => Some(key),
                Err(MooseError::MissingApiKey) => None,
                Err(error) => return Err(error),
            }
        } else {
            None
        };
        if let Some(key) = key {
            credentials::save(&stored_provider, &key).await?;
        }
        Ok(old_key)
    });
    let ui = Rc::clone(ui);
    let backend = Rc::clone(backend);
    let dialog = dialog.clone();
    gtk::glib::spawn_future_local(async move {
        let old_key = match handle.await {
            Ok(Ok(key)) => key,
            result => {
                backend.credential_operation.set(false);
                dialog.set_sensitive(true);
                dialog.set_can_close(true);
                match result {
                    Ok(Err(error)) => {
                        show_error(&dialog, "API key could not be saved securely", &error)
                    }
                    Err(_) => ui
                        .toast_overlay
                        .add_toast(adw::Toast::new("Credential operation stopped. Try again.")),
                    Ok(Ok(_)) => unreachable!(),
                }
                refresh_models(&ui, &backend);
                return;
            }
        };
        let result = if let Some(previous) = &previous {
            backend.repository.update_custom(previous, provider.clone())
        } else {
            backend
                .repository
                .insert_remote(provider.clone(), permissions)
        };
        match result {
            Ok(provider) => {
                if changed_destination
                    && let Some(previous) =
                        previous.as_ref().filter(|previous| previous.requires_key())
                {
                    let previous = previous.clone();
                    let cleanup = backend
                        .runtime
                        .spawn(async move { credentials::delete(&previous).await });
                    if !matches!(cleanup.await, Ok(Ok(()))) {
                        ui.toast_overlay.add_toast(adw::Toast::new("Provider saved. Remove the unused key for the previous connection using your keyring manager."));
                    }
                }
                backend.credential_operation.set(false);
                dialog.set_can_close(true);
                dialog.close();
                if previous.is_none() {
                    backend.network_policy.set_local_only(false);
                    apply_active_provider(&ui, &backend, provider);
                    show_chat(&ui);
                } else if active_provider(&backend).is_some_and(|active| active.id == provider.id) {
                    *backend.provider.borrow_mut() = Some(provider.clone());
                    apply_provider_state(&ui, &Some(provider));
                    refresh_models(&ui, &backend);
                } else {
                    refresh_models(&ui, &backend);
                }
                ui.toast_overlay
                    .add_toast(adw::Toast::new(if previous.is_some() {
                        "Provider settings saved"
                    } else {
                        "Provider added"
                    }));
            }
            Err(error) => {
                if key_written {
                    let cleanup = backend.runtime.spawn(async move {
                        if let Some(key) = old_key {
                            credentials::save(&provider, &key).await
                        } else {
                            credentials::delete(&provider).await
                        }
                    });
                    if !matches!(cleanup.await, Ok(Ok(()))) {
                        ui.toast_overlay.add_toast(adw::Toast::new("The keyring could not be restored. Review this provider's API key in Preferences."));
                    }
                }
                backend.credential_operation.set(false);
                dialog.set_sensitive(true);
                dialog.set_can_close(true);
                show_error(&dialog, "Provider could not be saved", &error);
                refresh_models(&ui, &backend);
            }
        }
    });
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
        match target_backend.repository.get(&provider.id) {
            Ok(Some(current))
                if destination(&current) == destination(&provider) && current.requires_key() => {}
            Ok(_) => {
                show_error(
                    &target_ui.window,
                    "Provider settings changed",
                    &MooseError::ProviderRequest(
                        "Reopen the provider's key settings and try again.".into(),
                    ),
                );
                return;
            }
            Err(error) => {
                show_error(&target_ui.window, "Provider could not be loaded", &error);
                return;
            }
        }
        attachments::invalidate_capabilities(&target_ui);
        reasoning::reset(&target_ui);
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
                        attachments::invalidate_capabilities(&ui);
                        reasoning::reset(&ui);
                        if removed {
                            set_model_picker(&ui, Vec::new(), None);
                            set_installed_models(&ui, &backend, Vec::new());
                            ui.provider_status.set_text("API Key Required");
                        } else {
                            refresh_models(&ui, &backend);
                        }
                    } else {
                        refresh_models(&ui, &backend);
                    }
                }
                Ok(Err(error)) => {
                    show_error(&ui.window, "Secure key operation failed", &error);
                    refresh_models(&ui, &backend);
                }
                Err(_) => {
                    ui.toast_overlay
                        .add_toast(adw::Toast::new("Credential operation stopped. Try again."));
                    refresh_models(&ui, &backend);
                }
            }
        });
    });
    dialog.present(Some(&ui.window));
}

pub(super) fn delete_provider(ui: &Rc<WindowUi>, backend: &Rc<Backend>, provider: Provider) {
    match backend.repository.has_conversations(&provider.id) {
        Ok(false) => {}
        Ok(true) => {
            ui.toast_overlay.add_toast(adw::Toast::new(if provider.requires_key() {
                "This provider has saved chats. Remove its API key in Preferences, or delete those chats first."
            } else {
                "This provider has saved chats. Delete those chats before removing the provider."
            }));
            return;
        }
        Err(error) => {
            show_error(&ui.window, "Provider could not be deleted", &error);
            return;
        }
    }
    if !provider.requires_key() {
        provider_controls::delete_provider_record(ui, backend, &provider.id);
        return;
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
