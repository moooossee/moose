use super::*;
use crate::{
    providers::credentials::{self, ApiKey},
    tools::web::{SearchProvider, WebConfig},
};

pub(super) fn authorize(ui: &Rc<WindowUi>, backend: &Rc<Backend>, pending: PendingChat) -> bool {
    let Some(config) = pending.web.as_ref() else {
        return start_authorized_chat(ui, backend, pending);
    };
    match backend
        .conversation_repository
        .web_permission(&pending.conversation_id, config)
    {
        Ok(true) => return start_authorized_chat(ui, backend, pending),
        Ok(false) => {}
        Err(error) => {
            show_error(&ui.window, "Web permission could not be loaded", &error);
            return false;
        }
    }
    let remember = remember_permission_row(false);
    let permissions = adw::PreferencesGroup::new();
    permissions.add(&remember);
    let dialog = adw::AlertDialog::builder()
        .heading("Allow Web Access?")
        .body(format!("{}\n\nThe model can send search queries and URLs based on this conversation, including attached content. Search providers receive those queries; visited websites receive page requests. Results are shared with this chat’s model provider.\n\nPermission is saved for this chat. You can remember it for all chats using this search provider below. Manage permissions in Preferences → Web.", config.description()))
        .extra_child(&permissions)
        .close_response("cancel")
        .default_response("cancel")
        .build();
    dialog.add_response("cancel", "Cancel");
    dialog.add_response("allow", "Allow Web");
    dialog.set_response_appearance("allow", adw::ResponseAppearance::Suggested);
    let pending = RefCell::new(Some(pending));
    let parent = ui.window.clone();
    let ui = Rc::clone(ui);
    let backend = Rc::clone(backend);
    dialog.connect_response(None, move |_, response| {
        let Some(pending) = pending.borrow_mut().take() else {
            return;
        };
        if response != "allow" {
            return;
        }
        let Some(config) = &pending.web else {
            return;
        };
        if backend.network_policy.local_only()
            || !backend
                .repository
                .web_config()
                .is_ok_and(|current| current == *config)
            || backend.active_conversation_id.borrow().as_deref() != Some(&pending.conversation_id)
            || provider_change_is_blocked(&ui, &backend)
        {
            ui.toast_overlay.add_toast(adw::Toast::new(
                "Settings changed. Review Web settings and send your message again.",
            ));
            return;
        }
        if let Err(error) = backend.conversation_repository.allow_web(
            &pending.conversation_id,
            config,
            remember.is_active(),
        ) {
            show_error(&ui.window, "Web permission could not be saved", &error);
            return;
        }
        start_authorized_chat(&ui, &backend, pending);
    });
    dialog.present(Some(&parent));
    false
}

pub(super) fn show_settings(
    parent: &impl IsA<gtk::Widget>,
    ui: &Rc<WindowUi>,
    backend: &Rc<Backend>,
) -> adw::PreferencesDialog {
    let dialog = adw::PreferencesDialog::builder()
        .title("Web Settings")
        .search_enabled(false)
        .build();
    dialog.add(&preferences_page(&dialog, ui, backend));
    dialog.present(Some(parent));
    dialog
}

pub(super) fn preferences_page(
    dialog: &adw::PreferencesDialog,
    ui: &Rc<WindowUi>,
    backend: &Rc<Backend>,
) -> adw::PreferencesPage {
    let page = adw::PreferencesPage::builder()
        .title("Web")
        .icon_name("moose-globe-symbolic")
        .build();
    let group = adw::PreferencesGroup::builder().title("Web Access")
        .description("Enable Web per conversation in Chat Settings. Links can be read without a search account. Local Only blocks all web tools.").build();
    page.add(&group);
    let (config, allowed) = match backend.repository.web_config().and_then(|config| {
        let allowed = backend.repository.web_permission(&config)?;
        Ok((config, allowed))
    }) {
        Ok(settings) => settings,
        Err(error) => {
            group.add(
                &adw::ActionRow::builder()
                    .title("Web settings could not be loaded")
                    .subtitle(error.to_string())
                    .use_markup(false)
                    .build(),
            );
            return page;
        }
    };
    let permission_destination = Rc::new(RefCell::new(config.destination()));
    let remember = remember_permission_row(allowed);
    let provider = adw::ComboRow::builder()
        .title("Search Provider")
        .model(&gtk::StringList::new(&[
            "DuckDuckGo",
            "Links Only",
            "Brave Search",
            "SearXNG",
        ]))
        .selected(match config.provider {
            SearchProvider::DuckDuckGo => 0,
            SearchProvider::LinksOnly => 1,
            SearchProvider::Brave => 2,
            SearchProvider::Searxng => 3,
        })
        .build();
    let endpoint = adw::EntryRow::builder()
        .title("SearXNG Search URL")
        .text(&config.endpoint)
        .build();
    let hint = adw::ActionRow::builder().title("SearXNG Instance")
        .subtitle("Use a public HTTPS search endpoint, such as https://search.example.com/search. The instance must allow JSON searches.")
        .subtitle_lines(3).build();
    let key_row = adw::ActionRow::builder().title("Brave Search API Key")
        .subtitle("Stored in the desktop keyring and excluded from exports. A Search API subscription may have usage charges.").subtitle_lines(2).build();
    let key_button = gtk::Button::with_label("Manage Key");
    key_button.set_valign(gtk::Align::Center);
    key_row.add_suffix(&key_button);
    key_row.set_activatable_widget(Some(&key_button));
    let signup = gtk::LinkButton::with_label(
        "https://api-dashboard.search.brave.com/",
        "Get a Brave Search Key",
    );
    signup.set_halign(gtk::Align::Start);
    let duckduckgo_hint = adw::ActionRow::builder()
        .title("DuckDuckGo")
        .subtitle(
            "Free search without an account or API key. DuckDuckGo may temporarily limit searches.",
        )
        .subtitle_lines(0)
        .build();
    let save = gtk::Button::with_label("Save Web Settings");
    save.add_css_class("suggested-action");
    save.set_halign(gtk::Align::End);
    save.set_margin_top(12);
    group.add(&provider);
    group.add(&endpoint);
    group.add(&hint);
    group.add(&key_row);
    group.add(&signup);
    group.add(&duckduckgo_hint);
    let update_visibility = {
        let endpoint = endpoint.clone();
        let hint = hint.clone();
        let key_row = key_row.clone();
        let signup = signup.clone();
        let duckduckgo_hint = duckduckgo_hint.clone();
        move |row: &adw::ComboRow| {
            endpoint.set_visible(row.selected() == 3);
            hint.set_visible(row.selected() == 3);
            key_row.set_visible(row.selected() == 2);
            signup.set_visible(row.selected() == 2);
            duckduckgo_hint.set_visible(row.selected() == 0);
        }
    };
    update_visibility(&provider);
    provider.connect_selected_notify(update_visibility);
    let update_permission = {
        let provider = provider.downgrade();
        let endpoint = endpoint.downgrade();
        let remember = remember.downgrade();
        let destination = Rc::clone(&permission_destination);
        Rc::new(move || {
            if let (Some(provider), Some(endpoint), Some(remember)) =
                (provider.upgrade(), endpoint.upgrade(), remember.upgrade())
            {
                let current = selected_config(&provider, &endpoint).destination();
                if current != *destination.borrow() {
                    remember.set_active(false);
                    *destination.borrow_mut() = current;
                }
            }
        })
    };
    let target_update = Rc::clone(&update_permission);
    provider.connect_selected_notify(move |_| target_update());
    endpoint.connect_text_notify(move |_| update_permission());
    let target_ui = Rc::clone(ui);
    let target_backend = Rc::clone(backend);
    key_button.connect_clicked(move |_| manage_key(&target_ui, &target_backend));
    let target_ui = Rc::clone(ui);
    let target_backend = Rc::clone(backend);
    let target_dialog = dialog.downgrade();
    let target_remember = remember.clone();
    save.connect_clicked(move |_| {
        if provider_change_is_blocked(&target_ui, &target_backend) {
            return;
        }
        let config = selected_config(&provider, &endpoint);
        let allow_all_chats = target_remember.is_active();
        match target_backend
            .repository
            .save_web_config(&config, allow_all_chats)
        {
            Ok(()) => {
                super::composer_commands::refresh(&target_ui, &target_backend);
                if let Some(dialog) = target_dialog.upgrade() {
                    dialog.add_toast(adw::Toast::new("Web settings saved"));
                }
            }
            Err(error) => show_error(&target_ui.window, "Web settings could not be saved", &error),
        }
    });
    let permissions = adw::PreferencesGroup::builder()
        .title("Permissions")
        .build();
    permissions.add(&remember);
    let reset_row = adw::ActionRow::builder()
        .title("Web in Saved Chats")
        .subtitle("Turn off Web and clear its permissions in every conversation")
        .subtitle_lines(2)
        .build();
    let reset = gtk::Button::with_label("Reset");
    reset.set_valign(gtk::Align::Center);
    reset_row.add_suffix(&reset);
    permissions.add(&reset_row);
    page.add(&permissions);
    let actions = adw::PreferencesGroup::new();
    actions.add(&save);
    page.add(&actions);
    let target_ui = Rc::clone(ui);
    let target_backend = Rc::clone(backend);
    let target_dialog = dialog.downgrade();
    reset.connect_clicked(move |_| {
        if provider_change_is_blocked(&target_ui, &target_backend) {
            return;
        }
        match target_backend.repository.reset_web_permissions() {
            Ok(()) => {
                remember.set_active(false);
                super::composer_commands::refresh(&target_ui, &target_backend);
                if let Some(dialog) = target_dialog.upgrade() {
                    dialog.add_toast(adw::Toast::new("Web turned off in all chats"));
                }
            }
            Err(error) => show_error(
                &target_ui.window,
                "Web permissions could not be reset",
                &error,
            ),
        }
    });
    page
}

fn remember_permission_row(active: bool) -> adw::SwitchRow {
    adw::SwitchRow::builder()
        .title("Allow Web in All Chats")
        .subtitle("Skip permission prompts with the selected search provider. Web must still be enabled in each chat.")
        .subtitle_lines(0)
        .active(active)
        .build()
}

fn selected_config(provider: &adw::ComboRow, endpoint: &adw::EntryRow) -> WebConfig {
    WebConfig {
        provider: match provider.selected() {
            1 => SearchProvider::LinksOnly,
            2 => SearchProvider::Brave,
            3 => SearchProvider::Searxng,
            _ => SearchProvider::DuckDuckGo,
        },
        endpoint: endpoint.text().trim().to_string(),
    }
}

fn manage_key(ui: &Rc<WindowUi>, backend: &Rc<Backend>) {
    if provider_change_is_blocked(ui, backend) {
        return;
    }
    let key = gtk::PasswordEntry::builder()
        .show_peek_icon(false)
        .hexpand(true)
        .build();
    let dialog = adw::AlertDialog::builder().heading("Brave Search API Key")
        .body("Add, replace or remove the saved key. Existing keys are never displayed. Your desktop may ask you to unlock its keyring.")
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
        let value = (response == "save").then(|| key.text().to_string());
        key.set_text("");
        if !matches!(response, "save" | "delete")
            || provider_change_is_blocked(&target_ui, &target_backend)
        {
            return;
        }
        let key = match value.map(ApiKey::new).transpose() {
            Ok(key) => key,
            Err(error) => {
                show_error(&target_ui.window, "Invalid API key", &error);
                return;
            }
        };
        target_backend.credential_operation.set(true);
        target_ui
            .toast_overlay
            .add_toast(adw::Toast::new("Updating the desktop keyring…"));
        let removed = key.is_none();
        let handle = target_backend.runtime.spawn(async move {
            if let Some(key) = key {
                credentials::save_web_key(&key).await
            } else {
                credentials::delete_web_key().await
            }
        });
        let ui = Rc::clone(&target_ui);
        let backend = Rc::clone(&target_backend);
        gtk::glib::spawn_future_local(async move {
            let result = handle.await;
            backend.credential_operation.set(false);
            match result {
                Ok(Ok(())) => ui.toast_overlay.add_toast(adw::Toast::new(if removed {
                    "Brave Search key removed"
                } else {
                    "Brave Search key saved securely"
                })),
                Ok(Err(error)) => show_error(&ui.window, "Secure key operation failed", &error),
                Err(_) => ui
                    .toast_overlay
                    .add_toast(adw::Toast::new("Credential operation stopped. Try again.")),
            }
        });
    });
    dialog.present(Some(&ui.window));
}
