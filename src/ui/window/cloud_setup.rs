use super::*;
use crate::providers::{
    NewProvider, cloudflare_base_url,
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
    let setup = widgets::SetupDialog::new(
        if editing {
            "Configure Custom Provider"
        } else {
            "Connect Cloud Provider"
        },
        initial_kind.label(),
        if editing {
            "Manage your custom AI connection."
        } else {
            "Connect using your own API key."
        },
    );
    let dialog = setup.dialog.clone();
    let description = gtk::Label::builder()
        .label("Changing the server, API format or authentication resets sharing permissions and requires a new API key. Leave the key blank to keep it when these settings stay the same.")
        .wrap(true)
        .wrap_mode(gtk::pango::WrapMode::WordChar)
        .width_chars(1)
        .xalign(0.0)
        .visible(editing)
        .css_classes(["caption", "dim-label"])
        .build();
    let group = adw::PreferencesGroup::builder().title("Connection").build();
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
        .use_markup(false)
        .subtitle(initial_kind.default_base_url().unwrap_or_default())
        .subtitle_lines(2)
        .visible(initial_kind != ProviderKind::Custom)
        .build();
    let account_id = adw::EntryRow::builder()
        .title("Cloudflare Account ID")
        .visible(initial_kind == ProviderKind::Cloudflare)
        .build();
    let account_hint = gtk::Label::builder()
        .label("Find your account ID in the Cloudflare dashboard. Enter an API token with Workers AI permission below.")
        .wrap(true)
        .width_chars(1)
        .xalign(0.0)
        .visible(initial_kind == ProviderKind::Cloudflare)
        .css_classes(["caption", "dim-label"])
        .build();
    let custom_url = adw::EntryRow::builder()
        .title("Base URL")
        .input_purpose(gtk::InputPurpose::Url)
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
        .css_classes(["caption", "dim-label"])
        .build();
    group.add(&kinds);
    group.add(&name);
    group.add(&account_id);
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
        .description("Connecting enables remote connections. Shared messages and files are processed under the provider's own data policy.")
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
    let target_key = key.downgrade();
    let target_endpoint = endpoint.downgrade();
    let target_message_permission = message_permission.clone();
    account_id.connect_text_notify(move |row| {
        if let Some(key) = target_key.upgrade() {
            key.set_text("");
        }
        target_message_permission.set_selected(0);
        if let Some(endpoint) = target_endpoint.upgrade() {
            endpoint.set_subtitle(
                &cloudflare_base_url(&row.text())
                    .unwrap_or_else(|_| "Enter a valid account ID".into()),
            );
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
        .css_classes(["caption", "dim-label"])
        .build();
    let connection = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(8)
        .build();
    connection.append(&group);
    connection.append(&account_hint);
    connection.append(&url_hint);
    let content = &setup.content;
    content.append(&description);
    content.append(&connection);
    content.append(&custom_fields.connection_group);
    content.append(&key_group);
    content.append(&custom_fields.models_group);
    content.append(&sharing_group);
    content.append(&hint);
    let cancel_button = gtk::Button::with_label("Cancel");
    let connect_button = gtk::Button::with_label(if editing {
        "Save Changes"
    } else {
        "Save and Connect"
    });
    setup.set_actions(&connect_button, &cancel_button);
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
    let target_account_id = account_id.clone();
    let target_heading = setup.heading.downgrade();
    let target_description = setup.description.downgrade();
    kinds.connect_selected_notify(move |row| {
        let kind = ProviderKind::CLOUD[row.selected() as usize];
        let custom = kind == ProviderKind::Custom;
        if let Some(heading) = target_heading.upgrade() {
            heading.set_label(kind.label());
        }
        if let Some(description) = target_description.upgrade() {
            description.set_label(if custom {
                "Connect to a service using your own connection settings."
            } else {
                "Connect using your own API key."
            });
        }
        target_key.set_text("");
        target_message_permission.set_selected(0);
        target_name.set_text(kind.label());
        let cloudflare = kind == ProviderKind::Cloudflare;
        if cloudflare {
            endpoint.set_subtitle(
                &cloudflare_base_url(&target_account_id.text())
                    .unwrap_or_else(|_| "Enter your Cloudflare account ID".into()),
            );
        } else {
            endpoint.set_subtitle(kind.default_base_url().unwrap_or_default());
        }
        endpoint.set_visible(!custom);
        target_account_id.set_visible(cloudflare);
        account_hint.set_visible(cloudflare);
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
                    base_url: match kind {
                        ProviderKind::Custom => custom_url.text().to_string(),
                        ProviderKind::Cloudflare => cloudflare_base_url(&account_id.text())
                            .map_err(|_| MooseError::ProviderRequest(
                                "Enter a valid Cloudflare account ID (32 hexadecimal characters).".into(),
                            ))?,
                        _ => kind.default_base_url()
                            .ok_or(MooseError::InvalidProviderUrl)?
                            .into(),
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
