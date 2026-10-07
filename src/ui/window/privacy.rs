use super::*;
use crate::providers::policy::RemotePermissions;

pub(super) fn message_permission_row() -> adw::ComboRow {
    adw::ComboRow::builder()
        .title("Message Sharing")
        .subtitle("Prompts, instructions, and history. Files require separate permission.")
        .subtitle_lines(2)
        .model(&gtk::StringList::new(&[
            "Ask in Each Chat",
            "Allow in All Chats",
        ]))
        .selected(0)
        .build()
}

pub(super) fn settings_group(ui: &Rc<WindowUi>, backend: &Rc<Backend>) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::builder()
        .title("Connections")
        .build();
    let row = adw::SwitchRow::builder()
        .title("Local Only")
        .subtitle(
            "Only use Ollama managed by Moose. Cloud and external Ollama connections are blocked.",
        )
        .active(backend.network_policy.local_only())
        .build();
    let ui = Rc::clone(ui);
    let backend = Rc::clone(backend);
    row.connect_active_notify(move |row| {
        let enabled = row.is_active();
        if enabled == backend.network_policy.local_only() {
            return;
        }
        if provider_change_is_blocked(&ui, &backend) {
            row.set_active(backend.network_policy.local_only());
            return;
        }
        match backend.repository.set_local_only(enabled) {
            Ok(()) => {
                backend.network_policy.set_local_only(enabled);
                ui.attachments.capabilities.borrow_mut().clear();
                ui.thinking_state.borrow_mut().cache.clear();
                refresh_models(&ui, &backend);
            }
            Err(error) => {
                row.set_active(backend.network_policy.local_only());
                show_error(&ui.window, "Privacy setting could not be saved", &error);
            }
        }
    });
    group.add(&row);
    group
}

fn permission_summary(provider: &Provider, permissions: RemotePermissions) -> String {
    format!(
        "Messages: {}\nFiles: {}\n{}",
        if permissions.messages {
            "allowed in all chats"
        } else {
            "ask in each chat"
        },
        if permissions.files {
            "allowed in all chats"
        } else {
            "ask in each chat"
        },
        provider.base_url,
    )
}

pub(super) fn sharing_group(
    ui: &Rc<WindowUi>,
    backend: &Rc<Backend>,
) -> Result<adw::PreferencesGroup> {
    let group = adw::PreferencesGroup::builder()
        .title("Sharing Permissions")
        .description("Remembered permissions apply only to the selected provider and server. Reset clears them and permissions in existing chats. Local Only blocks remote connections regardless of these permissions.")
        .build();
    let providers = backend.repository.list()?;
    let mut has_remote = false;
    for provider in providers.into_iter().filter(Provider::is_remote) {
        has_remote = true;
        let permissions = backend.repository.remote_permissions(&provider)?;
        let row = adw::ActionRow::builder()
            .title(&provider.name)
            .subtitle(permission_summary(&provider, permissions))
            .subtitle_lines(3)
            .use_markup(false)
            .build();
        let button = gtk::Button::with_label("Reset");
        button.set_valign(gtk::Align::Center);
        button.set_tooltip_text(Some("Reset Permissions in All Chats"));
        row.add_suffix(&button);
        group.add(&row);
        let weak_row = row.downgrade();
        let ui = Rc::clone(ui);
        let backend = Rc::clone(backend);
        button.connect_clicked(move |_| {
            if provider_change_is_blocked(&ui, &backend) {
                return;
            }
            match backend.repository.reset_remote_permissions(&provider.id) {
                Ok(()) => {
                    if let Some(row) = weak_row.upgrade() {
                        row.set_subtitle(&permission_summary(
                            &provider,
                            RemotePermissions::default(),
                        ));
                    }
                    ui.toast_overlay.add_toast(adw::Toast::new(
                        "Sharing permissions reset for this provider",
                    ));
                }
                Err(error) => show_error(&ui.window, "Permissions could not be reset", &error),
            }
        });
    }
    if !has_remote {
        group.add(
            &adw::ActionRow::builder()
                .title("No Remote Providers")
                .subtitle("Managed Ollama runs on this device and needs no sharing permission.")
                .build(),
        );
    }
    Ok(group)
}

pub(super) fn conversation_group(
    ui: &Rc<WindowUi>,
    backend: &Rc<Backend>,
) -> adw::PreferencesGroup {
    let group=adw::PreferencesGroup::builder().title("Remote Context")
        .description("Remote requests can include recent messages, instructions, and approved attachments. Document search stays on this device.").build();
    let Some(provider) = active_provider(backend) else {
        return group;
    };
    let button = gtk::Button::with_label("Reset");
    button.set_valign(gtk::Align::Center);
    button.set_tooltip_text(Some("Reset Permissions in All Chats"));
    button.add_css_class("moose-settings-secondary-action");
    let row = adw::ActionRow::builder()
        .title("Provider Sharing Permissions")
        .subtitle(format!(
            "Ask again in all chats with {}. Also resets file sharing.",
            provider.name
        ))
        .subtitle_lines(2)
        .use_markup(false)
        .build();
    row.add_suffix(&button);
    group.add(&row);
    let ui = Rc::clone(ui);
    let backend = Rc::clone(backend);
    button.connect_clicked(move |_| {
        if provider_change_is_blocked(&ui, &backend) {
            return;
        }
        match backend.repository.reset_remote_permissions(&provider.id) {
            Ok(()) => ui.toast_overlay.add_toast(adw::Toast::new(
                "Sharing permissions reset for this provider",
            )),
            Err(error) => show_error(&ui.window, "Permissions could not be reset", &error),
        }
    });
    group
}

pub(super) fn authorize(ui: &Rc<WindowUi>, backend: &Rc<Backend>, pending: PendingChat) -> bool {
    if !pending.provider.is_remote() {
        return start_prepared_chat(ui, backend, pending);
    }
    let images: usize = pending
        .request
        .messages
        .iter()
        .map(|m| m.images.len())
        .sum();
    let includes_files = images > 0 || !pending.sources.is_empty();
    let permissions = match backend
        .conversation_repository
        .remote_permissions(&pending.conversation_id, &pending.provider)
    {
        Ok(value) => value,
        Err(error) => {
            show_error(&ui.window, "Sharing permission could not be loaded", &error);
            return false;
        }
    };
    if permissions.messages && (!includes_files || permissions.files) {
        return start_prepared_chat(ui, backend, pending);
    }
    let remembered = match backend.repository.remote_permissions(&pending.provider) {
        Ok(value) => value,
        Err(error) => {
            show_error(&ui.window, "Sharing permission could not be loaded", &error);
            return false;
        }
    };

    let description = format!(
        "Send this conversation's context to {} at {}?\n\nThis request includes {} messages (including instructions and history), {} images and {} document excerpts. The provider processes this content under its own data policy.\n\nPermission is saved for this chat. Choose the options below to allow sharing in all chats with this provider and server. Messages and files have separate permissions. You can reset them in Preferences → Privacy or Chat Settings.",
        pending.provider.name,
        pending.provider.base_url,
        pending.request.messages.len(),
        images,
        pending.sources.len()
    );
    let dialog = adw::Dialog::builder()
        .title(if permissions.messages && includes_files {
            "Share Files with Provider?"
        } else if includes_files {
            "Share Context and Files?"
        } else {
            "Share Context with Provider?"
        })
        .content_width(560)
        .content_height(580)
        .build();
    let header_bar = adw::HeaderBar::builder()
        .show_start_title_buttons(false)
        .show_end_title_buttons(false)
        .build();
    let description = gtk::Label::builder()
        .label(description)
        .wrap(true)
        .wrap_mode(gtk::pango::WrapMode::WordChar)
        .width_chars(1)
        .xalign(0.0)
        .build();
    let preview = gtk::TextView::builder()
        .editable(false)
        .cursor_visible(false)
        .wrap_mode(gtk::WrapMode::WordChar)
        .left_margin(8)
        .right_margin(8)
        .top_margin(8)
        .bottom_margin(8)
        .build();
    let text = pending
        .request
        .messages
        .iter()
        .map(|m| {
            format!(
                "{:?}:\n{}\n{}",
                m.role,
                m.content,
                if m.images.is_empty() {
                    String::new()
                } else {
                    format!("[{} attached images]", m.images.len())
                }
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    preview.buffer().set_text(&text);
    let scroll = gtk::ScrolledWindow::builder()
        .child(&preview)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .min_content_height(100)
        .max_content_height(260)
        .propagate_natural_height(true)
        .build();
    let expander = gtk::Expander::builder()
        .label("Review text being sent")
        .child(&scroll)
        .build();
    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(18)
        .build();
    content.add_css_class("moose-instance-content");
    content.append(&description);
    let remember_group = adw::PreferencesGroup::builder()
        .title("Remember Permission")
        .build();
    let remember_messages = adw::SwitchRow::builder()
        .title("Messages in All Chats")
        .subtitle(
            "Allow prompts, instructions, and history with this provider without asking again.",
        )
        .subtitle_lines(2)
        .visible(!remembered.messages)
        .build();
    let remember_files = adw::SwitchRow::builder()
        .title("Files in All Chats")
        .subtitle(
            "Allow attached images and document excerpts with this provider without asking again.",
        )
        .subtitle_lines(2)
        .visible(includes_files && !remembered.files)
        .build();
    remember_group.add(&remember_messages);
    remember_group.add(&remember_files);
    content.append(&remember_group);
    content.append(&expander);
    let scrolled = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .child(&content)
        .build();
    let cancel_button = gtk::Button::with_label("Cancel");
    let allow_button = gtk::Button::with_label("Allow");
    cancel_button.set_hexpand(true);
    allow_button.set_hexpand(true);
    allow_button.add_css_class("suggested-action");
    let actions = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(10)
        .build();
    actions.add_css_class("moose-instance-actions");
    actions.append(&cancel_button);
    actions.append(&allow_button);
    let toolbar_view = adw::ToolbarView::builder()
        .top_bar_style(adw::ToolbarStyle::Flat)
        .bottom_bar_style(adw::ToolbarStyle::Flat)
        .content(&scrolled)
        .build();
    toolbar_view.add_top_bar(&header_bar);
    toolbar_view.add_bottom_bar(&actions);
    dialog.set_child(Some(&toolbar_view));
    dialog.set_default_widget(Some(&cancel_button));
    dialog.set_focus(Some(&cancel_button));
    let target_dialog = dialog.clone();
    cancel_button.connect_clicked(move |_| {
        target_dialog.close();
    });
    let parent = ui.window.clone();
    let pending = RefCell::new(Some(pending));
    let ui = Rc::clone(ui);
    let backend = Rc::clone(backend);
    let target_dialog = dialog.clone();
    allow_button.connect_clicked(move |_| {
        let Some(pending) = pending.borrow_mut().take() else {
            return;
        };
        target_dialog.close();
        if let Err(error) = backend.network_policy.check(&pending.provider) {
            show_error(&ui.window, "Connection blocked", &error);
            return;
        }
        if active_provider(&backend).is_none_or(|provider| {
            provider.id != pending.provider.id
                || provider.kind != pending.provider.kind
                || provider.base_url != pending.provider.base_url
        }) || backend.active_conversation_id.borrow().as_deref()
            != Some(pending.conversation_id.as_str())
        {
            ui.toast_overlay.add_toast(adw::Toast::new(
                "The active chat or provider changed. Send the message again to review it.",
            ));
            return;
        }
        if let Err(error) = backend.conversation_repository.grant_remote_permissions(
            &pending.conversation_id,
            &pending.provider,
            RemotePermissions {
                messages: true,
                files: includes_files,
            },
            RemotePermissions {
                messages: !remembered.messages && remember_messages.is_active(),
                files: includes_files && !remembered.files && remember_files.is_active(),
            },
        ) {
            show_error(&ui.window, "Sharing permission could not be saved", &error);
            return;
        }
        start_prepared_chat(&ui, &backend, pending);
    });
    dialog.present(Some(&parent));
    false
}
