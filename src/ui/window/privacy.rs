use super::*;
use crate::providers::policy::RemotePermissions;

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

pub(super) fn conversation_group(
    ui: &Rc<WindowUi>,
    backend: &Rc<Backend>,
    conversation: &str,
) -> adw::PreferencesGroup {
    let group=adw::PreferencesGroup::builder().title("Remote Context")
        .description("Remote requests can include recent messages, instructions, and approved attachments. Document search stays on this device.").build();
    let button = gtk::Button::with_label("Reset");
    button.set_valign(gtk::Align::Center);
    button.set_tooltip_text(Some("Reset Sharing Permission"));
    button.add_css_class("moose-settings-secondary-action");
    let row = adw::ActionRow::builder()
        .title("Sharing Permission")
        .subtitle("Ask before sharing this chat again")
        .subtitle_lines(2)
        .build();
    row.add_suffix(&button);
    group.add(&row);
    let conversation = conversation.to_string();
    let ui = Rc::clone(ui);
    let backend = Rc::clone(backend);
    button.connect_clicked(move |_| {
        if provider_change_is_blocked(&ui, &backend) {
            return;
        }
        if let Some(provider) = active_provider(&backend) {
            match backend.conversation_repository.set_remote_permissions(
                &conversation,
                &provider,
                RemotePermissions::default(),
            ) {
                Ok(()) => ui
                    .toast_overlay
                    .add_toast(adw::Toast::new("Sharing permission reset")),
                Err(error) => show_error(&ui.window, "Permission could not be reset", &error),
            }
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

    let description = format!(
        "Send this conversation's context to {} at {}?\n\nThis request includes {} messages (including instructions and history), {} images and {} document excerpts. The provider processes this content under its own data policy.\n\nAllowing this chat also permits future requests with the same types of content. You can reset sharing permission in Chat Settings.",
        pending.provider.name,
        pending.provider.base_url,
        pending.request.messages.len(),
        images,
        pending.sources.len()
    );
    let dialog = adw::Dialog::builder()
        .title(if includes_files {
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
    content.append(&expander);
    let scrolled = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .child(&content)
        .build();
    let cancel_button = gtk::Button::with_label("Cancel");
    let allow_button = gtk::Button::with_label(if includes_files {
        "Allow Context and Files"
    } else {
        "Allow This Chat"
    });
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
        if let Err(error) = backend.conversation_repository.set_remote_permissions(
            &pending.conversation_id,
            &pending.provider,
            RemotePermissions {
                messages: true,
                files: includes_files || permissions.files,
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
