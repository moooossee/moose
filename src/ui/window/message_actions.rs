use std::{rc::Rc, time::Duration};

use adw::prelude::*;
use gtk::{Align, Orientation, gio, glib};

use crate::{
    conversations::{Message, MessageRole, MessageStatus},
    error::{MooseError, Result},
};

pub(super) use crate::chat::ChatSubmission as Submission;

use super::{
    Backend, WindowUi, chat_view, conversation_list, load_conversation, submit_message, widgets,
};

pub(super) fn bind(ui: &Rc<WindowUi>, backend: &Rc<Backend>) {
    for name in ["edit", "regenerate", "version", "fork"] {
        let action = gio::SimpleAction::new(name, Some(glib::VariantTy::STRING));
        let weak_ui = Rc::downgrade(ui);
        let weak_backend = Rc::downgrade(backend);
        action.connect_activate(move |_, parameter| {
            let (Some(ui), Some(backend), Some(id)) = (
                weak_ui.upgrade(),
                weak_backend.upgrade(),
                parameter.and_then(|value| value.str()),
            ) else {
                return;
            };
            if backend.active_generation.borrow().is_some() {
                return;
            }
            let result = match name {
                "edit" => edit_inline(&ui, &backend, id),
                "regenerate" => {
                    submit_message(&ui, &backend, Submission::Regenerate(id.to_string()));
                    Ok(())
                }
                "version" => select_version(&ui, &backend, id),
                _ => fork(&ui, &backend, id),
            };
            if let Err(error) = result {
                ui.toast_overlay.add_toast(adw::Toast::new(&format!(
                    "Message action could not be completed: {error}"
                )));
            }
        });
        ui.message_action_group.add_action(&action);
    }
    ui.window
        .insert_action_group("message", Some(&ui.message_action_group));
}

pub(super) fn set_enabled(ui: &WindowUi, enabled: bool) {
    for name in ["edit", "regenerate", "version", "fork"] {
        if let Some(action) = ui
            .message_action_group
            .lookup_action(name)
            .and_then(|action| action.downcast::<gio::SimpleAction>().ok())
        {
            action.set_enabled(enabled);
        }
    }
}

pub(super) fn render(ui: &WindowUi, backend: &Backend, skip: Option<&str>) -> Result<()> {
    let Some(id) = backend.active_conversation_id.borrow().clone() else {
        return Ok(());
    };
    let messages = backend.conversation_repository.list_messages(&id)?;
    let groups = backend.conversation_repository.message_versions(&id)?;
    let versions = groups
        .iter()
        .flat_map(|ids| ids.iter().map(move |id| (id.as_str(), ids)))
        .collect::<std::collections::HashMap<_, _>>();
    if gtk::prelude::GtkWindowExt::focus(&ui.window)
        .is_none_or(|focus| focus.is_ancestor(&ui.messages))
    {
        ui.entry.grab_focus();
    }
    let mut expanded = std::collections::HashSet::new();
    let mut row = ui.messages.first_child();
    while let Some(message_row) = row {
        let mut child = message_row.first_child();
        while let Some(widget) = child {
            if let Ok(expander) = widget.clone().downcast::<gtk::Expander>() {
                if expander.is_expanded() {
                    expanded.insert((message_row.widget_name().to_string(), expander.label()));
                }
            }
            child = widget.next_sibling();
        }
        row = message_row.next_sibling();
    }
    ui.streaming_message.borrow_mut().take();
    while let Some(child) = ui.messages.first_child() {
        ui.messages.remove(&child);
    }
    for message in messages
        .iter()
        .filter(|message| Some(message.id.as_str()) != skip)
    {
        if backend.active_generation.borrow().is_some()
            && backend.active_assistant_message_id.borrow().as_deref() == Some(message.id.as_str())
        {
            super::generation::append_live(ui, backend, message);
            continue;
        }
        let row = chat_view::append_stored_message(&ui.messages, message);
        if let Some(details) = backend
            .conversation_repository
            .message_details(&message.id)?
        {
            chat_view::append_details(&row, &details);
        }
        super::attachments::append_stored(ui, backend, &row, &message.id)?;
        append_toolbar(&row, message, versions.get(message.id.as_str()).copied());
        row.set_widget_name(&message.id);
        let mut child = row.first_child();
        while let Some(widget) = child {
            if let Ok(expander) = widget.clone().downcast::<gtk::Expander>() {
                expander.set_expanded(expanded.contains(&(message.id.clone(), expander.label())));
            }
            child = widget.next_sibling();
        }
    }
    Ok(())
}

pub(super) fn refresh(ui: &WindowUi, backend: &Backend) {
    let should_scroll = chat_view::should_stick_to_bottom(&ui.messages_scrolled);
    if let Err(error) = render(ui, backend, None) {
        ui.toast_overlay.add_toast(adw::Toast::new(&format!(
            "Conversation could not be displayed: {error}"
        )));
    }
    if should_scroll {
        chat_view::scroll_to_bottom(&ui.messages_scrolled);
    }
}

fn append_toolbar(row: &gtk::Box, message: &Message, versions: Option<&Vec<String>>) {
    let toolbar = gtk::Box::builder()
        .orientation(Orientation::Horizontal)
        .spacing(4)
        .halign(if message.role == MessageRole::User {
            Align::End
        } else {
            Align::Start
        })
        .build();
    toolbar.add_css_class("moose-message-actions");
    let copy = widgets::icon_button("edit-copy-symbolic", "Copy Message");
    copy.set_sensitive(!message.content.is_empty());
    let content = message.content.clone();
    copy.connect_clicked(move |button| {
        button.clipboard().set_text(&content);
        button.set_icon_name("object-select-symbolic");
        button.set_tooltip_text(Some("Copied"));
        let weak = button.downgrade();
        glib::timeout_add_local_once(Duration::from_millis(1600), move || {
            if let Some(button) = weak.upgrade() {
                button.set_icon_name("edit-copy-symbolic");
                button.set_tooltip_text(Some("Copy Message"));
            }
        });
    });
    toolbar.append(&copy);
    if message.role == MessageRole::User {
        toolbar.append(&action_button(
            "document-edit-symbolic",
            "Edit Message",
            "edit",
            &message.id,
        ));
    } else if message.role == MessageRole::Assistant && message.status.is_finished() {
        let retry = matches!(
            message.status,
            MessageStatus::Failed | MessageStatus::Cancelled
        );
        let button = action_button(
            "view-refresh-symbolic",
            if retry {
                "Retry Response"
            } else {
                "Regenerate Response"
            },
            "regenerate",
            &message.id,
        );
        if retry {
            button.set_child(Some(
                &adw::ButtonContent::builder()
                    .icon_name("view-refresh-symbolic")
                    .label("Retry")
                    .build(),
            ));
        }
        toolbar.append(&button);
    }
    if let Some(versions) = versions
        && let Some(index) = versions.iter().position(|id| *id == message.id)
    {
        let previous = action_button(
            "go-previous-symbolic",
            "Previous Version",
            "version",
            &versions[index.saturating_sub(1)],
        );
        if index == 0 {
            previous.set_action_name(None);
        }
        previous.set_sensitive(index > 0);
        let next = action_button(
            "go-next-symbolic",
            "Next Version",
            "version",
            &versions[(index + 1).min(versions.len() - 1)],
        );
        if index + 1 == versions.len() {
            next.set_action_name(None);
        }
        next.set_sensitive(index + 1 < versions.len());
        let label = gtk::Label::new(Some(&format!("{} / {}", index + 1, versions.len())));
        label.add_css_class("caption");
        label.set_tooltip_text(Some("Switch versions to restore their conversation"));
        toolbar.append(&previous);
        toolbar.append(&label);
        toolbar.append(&next);
    }
    let menu = gio::Menu::new();
    let item = gio::MenuItem::new(Some("Continue in New Chat"), None);
    item.set_action_and_target_value(Some("message.fork"), Some(&message.id.to_variant()));
    menu.append_item(&item);
    let more = gtk::MenuButton::builder()
        .icon_name("view-more-symbolic")
        .tooltip_text("More Message Actions")
        .menu_model(&menu)
        .build();
    more.add_css_class("flat");
    toolbar.append(&more);
    row.append(&toolbar);
}

fn action_button(icon: &str, tooltip: &str, action: &str, id: &str) -> gtk::Button {
    let button = widgets::icon_button(icon, tooltip);
    button.set_action_target_value(Some(&id.to_variant()));
    button.set_action_name(Some(&format!("message.{action}")));
    button
}

fn current_message(ui_backend: &Backend, id: &str) -> Result<Message> {
    let conversation_id = ui_backend
        .active_conversation_id
        .borrow()
        .clone()
        .ok_or(MooseError::ConversationNotFound)?;
    ui_backend
        .conversation_repository
        .list_messages(&conversation_id)?
        .into_iter()
        .find(|message| message.id == id)
        .ok_or(MooseError::MessageNotFound)
}

fn select_version(ui: &Rc<WindowUi>, backend: &Rc<Backend>, id: &str) -> Result<()> {
    let message = backend
        .conversation_repository
        .get_message(id)?
        .ok_or(MooseError::MessageNotFound)?;
    if backend.active_conversation_id.borrow().as_deref() != Some(&message.conversation_id) {
        return Err(MooseError::MessageNotFound);
    }
    let focused_action =
        gtk::prelude::GtkWindowExt::focus(&ui.window).and_then(|focus| focus.tooltip_text());
    let position = ui.messages_scrolled.vadjustment().value();
    backend.conversation_repository.select_message_version(id)?;
    render(ui, backend, None)?;
    conversation_list::refresh(ui, backend);
    if let Some(tooltip) = focused_action {
        let mut child = ui.messages.first_child();
        while let Some(row) = child {
            if row.widget_name() == id {
                if let Some(toolbar) = row.last_child() {
                    let mut control = toolbar.first_child();
                    while let Some(button) = control {
                        if button.tooltip_text().as_deref() == Some(tooltip.as_str())
                            && button.is_sensitive()
                        {
                            button.grab_focus();
                            break;
                        }
                        control = button.next_sibling();
                    }
                }
                break;
            }
            child = row.next_sibling();
        }
    }
    let adjustment = ui.messages_scrolled.vadjustment();
    glib::idle_add_local_once(move || {
        adjustment.set_value(position.min((adjustment.upper() - adjustment.page_size()).max(0.0)))
    });
    Ok(())
}

fn fork(ui: &Rc<WindowUi>, backend: &Rc<Backend>, id: &str) -> Result<()> {
    current_message(backend, id)?;
    let conversation = backend.conversation_repository.fork_at_message(id)?;
    let draft = super::prompt_text(&ui.entry);
    load_conversation(ui, backend, &conversation.id)?;
    ui.entry.buffer().set_text(&draft);
    conversation_list::refresh(ui, backend);
    super::show_chat(ui);
    super::workspace::sync_provider(ui, backend);
    super::reasoning::refresh(ui, backend);
    ui.entry.grab_focus();
    ui.toast_overlay.add_toast(adw::Toast::new(
        "New chat created. The original conversation is unchanged.",
    ));
    Ok(())
}

fn edit_inline(ui: &Rc<WindowUi>, backend: &Rc<Backend>, id: &str) -> Result<()> {
    let message = current_message(backend, id)?;
    if message.role != MessageRole::User {
        return Err(MooseError::InvalidMessageRole);
    }
    let mut child = ui.messages.first_child();
    let row = loop {
        let Some(widget) = child else {
            return Err(MooseError::MessageNotFound);
        };
        if widget.widget_name() == id {
            break widget
                .downcast::<gtk::Box>()
                .map_err(|_| MooseError::MessageNotFound)?;
        }
        child = widget.next_sibling();
    };
    let content = gtk::Box::builder()
        .orientation(Orientation::Vertical)
        .spacing(12)
        .hexpand(true)
        .build();
    content.add_css_class("moose-inline-editor");
    let title = gtk::Label::builder()
        .label("Edit message")
        .halign(Align::Start)
        .build();
    title.add_css_class("caption-heading");
    title.add_css_class("dim-label");
    let buffer = gtk::TextBuffer::new(None);
    buffer.set_text(&message.content);
    let editor = gtk::TextView::builder()
        .buffer(&buffer)
        .wrap_mode(gtk::WrapMode::WordChar)
        .accepts_tab(false)
        .left_margin(2)
        .right_margin(2)
        .top_margin(4)
        .bottom_margin(4)
        .pixels_below_lines(4)
        .hexpand(true)
        .build();
    editor.add_css_class("moose-inline-editor-text");
    let scroll = gtk::ScrolledWindow::builder()
        .child(&editor)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vscrollbar_policy(gtk::PolicyType::Automatic)
        .min_content_height(88)
        .max_content_height(300)
        .propagate_natural_height(true)
        .build();
    let footer = gtk::Box::builder()
        .orientation(Orientation::Horizontal)
        .spacing(8)
        .build();
    let hint = gtk::Label::builder()
        .label("Previous versions are kept.")
        .wrap(true)
        .xalign(0.0)
        .hexpand(true)
        .build();
    hint.add_css_class("caption");
    hint.add_css_class("dim-label");
    if !backend
        .conversation_repository
        .message_assets(id)?
        .is_empty()
    {
        hint.set_label("Attachments and previous versions are kept.");
    }
    let cancel = gtk::Button::with_label("Cancel");
    cancel.set_valign(Align::Center);
    cancel.set_tooltip_text(Some("Cancel editing (Esc)"));
    cancel.add_css_class("flat");
    let send = gtk::Button::with_label("Save & Send");
    send.set_valign(Align::Center);
    send.set_tooltip_text(Some("Save and generate a new response (Ctrl+Enter)"));
    send.add_css_class("suggested-action");
    send.set_sensitive(false);
    footer.append(&hint);
    footer.append(&cancel);
    footer.append(&send);
    content.append(&title);
    content.append(&scroll);
    content.append(&footer);
    let mut children = Vec::new();
    let mut child = row.first_child();
    while let Some(widget) = child {
        child = widget.next_sibling();
        children.push(widget);
    }
    for widget in &children {
        widget.set_visible(false);
    }
    row.append(&content);
    let weak_send = send.downgrade();
    let original = message.content.trim().to_string();
    buffer.connect_changed(move |buffer| {
        let text = buffer.text(&buffer.start_iter(), &buffer.end_iter(), true);
        if let Some(send) = weak_send.upgrade() {
            send.set_sensitive(!text.trim().is_empty() && text.trim() != original);
        }
    });
    let weak_row = row.downgrade();
    let weak_content = content.downgrade();
    cancel.connect_clicked(move |_| {
        let (Some(row), Some(content)) = (weak_row.upgrade(), weak_content.upgrade()) else {
            return;
        };
        for widget in &children {
            widget.set_visible(true);
        }
        if let Some(edit) = children
            .last()
            .and_then(|toolbar| toolbar.first_child())
            .and_then(|copy| copy.next_sibling())
        {
            edit.grab_focus();
        }
        row.remove(&content);
    });
    let weak_ui = Rc::downgrade(ui);
    let weak_backend = Rc::downgrade(backend);
    send.connect_clicked(move |_| {
        let (Some(ui), Some(backend)) = (weak_ui.upgrade(), weak_backend.upgrade()) else {
            return;
        };
        let content = buffer
            .text(&buffer.start_iter(), &buffer.end_iter(), true)
            .to_string();
        if submit_message(
            &ui,
            &backend,
            Submission::Edit {
                id: message.id.clone(),
                content,
            },
        ) {
            ui.entry.grab_focus();
        }
    });
    let controller = gtk::EventControllerKey::new();
    let weak_send = send.downgrade();
    let weak_cancel = cancel.downgrade();
    controller.connect_key_pressed(move |_, key, _, state| {
        if key == gtk::gdk::Key::Escape {
            if let Some(cancel) = weak_cancel.upgrade() {
                cancel.emit_clicked();
            }
            return glib::Propagation::Stop;
        }
        if matches!(key, gtk::gdk::Key::Return | gtk::gdk::Key::KP_Enter)
            && state.contains(gtk::gdk::ModifierType::CONTROL_MASK)
        {
            if let Some(send) = weak_send.upgrade().filter(|button| button.is_sensitive()) {
                send.emit_clicked();
            }
            return glib::Propagation::Stop;
        }
        glib::Propagation::Proceed
    });
    content.add_controller(controller);
    editor.grab_focus();
    Ok(())
}

#[cfg(test)]
#[path = "message_actions_tests.rs"]
mod tests;
