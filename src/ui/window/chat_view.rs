use adw::prelude::*;
use gtk::{Align, Orientation};

use crate::{
    APPLICATION_ID,
    conversations::{Message, MessageRole, MessageStatus},
};

use super::widgets::composer_button;

pub(super) struct Chat {
    pub(super) root: gtk::Box,
    pub(super) messages: gtk::Box,
    pub(super) status_page: adw::StatusPage,
    pub(super) message_stack: gtk::Stack,
    pub(super) entry: gtk::Entry,
    pub(super) model_picker: gtk::DropDown,
    pub(super) send_button: gtk::Button,
    pub(super) stop_button: gtk::Button,
}

pub(super) fn build() -> Chat {
    let root = gtk::Box::builder()
        .orientation(Orientation::Vertical)
        .spacing(0)
        .build();

    let status_page = adw::StatusPage::builder()
        .icon_name(APPLICATION_ID)
        .title("No Conversation Selected")
        .description("Choose a model and start a conversation.")
        .hexpand(true)
        .vexpand(true)
        .build();

    let empty_clamp = adw::Clamp::builder()
        .maximum_size(860)
        .tightening_threshold(560)
        .hexpand(true)
        .vexpand(true)
        .child(&status_page)
        .build();

    let messages = gtk::Box::builder()
        .orientation(Orientation::Vertical)
        .spacing(8)
        .hexpand(true)
        .build();
    messages.add_css_class("moose-chat-column");

    let messages_clamp = adw::Clamp::builder()
        .maximum_size(980)
        .tightening_threshold(560)
        .hexpand(true)
        .vexpand(true)
        .child(&messages)
        .build();

    let scrolled = gtk::ScrolledWindow::builder()
        .hexpand(true)
        .vexpand(true)
        .child(&messages_clamp)
        .build();

    let message_stack = gtk::Stack::builder().hexpand(true).vexpand(true).build();
    message_stack.add_named(&empty_clamp, Some("empty"));
    message_stack.add_named(&scrolled, Some("messages"));
    message_stack.set_visible_child_name("empty");

    let composer = gtk::Box::builder()
        .orientation(Orientation::Vertical)
        .spacing(6)
        .hexpand(true)
        .build();
    composer.add_css_class("moose-composer");

    let input_row = gtk::Box::builder()
        .orientation(Orientation::Horizontal)
        .spacing(6)
        .hexpand(true)
        .build();

    let entry = gtk::Entry::builder()
        .placeholder_text("Message")
        .hexpand(true)
        .build();
    entry.add_css_class("flat");
    entry.add_css_class("moose-composer-entry");

    let stop_button = composer_button("media-playback-stop-symbolic", "Cancel Generation");
    let send_button = composer_button("mail-send-symbolic", "Send Message");

    send_button.add_css_class("suggested-action");
    stop_button.add_css_class("destructive-action");
    send_button.set_sensitive(false);
    stop_button.set_sensitive(false);
    input_row.append(&entry);
    input_row.append(&stop_button);
    input_row.append(&send_button);

    let model_picker = gtk::DropDown::from_strings(&["No model selected"]);
    model_picker.set_tooltip_text(Some("Active Model"));
    model_picker.set_sensitive(false);
    model_picker.set_halign(Align::Start);
    model_picker.set_size_request(210, -1);
    model_picker.add_css_class("flat");
    model_picker.add_css_class("moose-model-picker");

    let model_row = gtk::Box::builder()
        .orientation(Orientation::Horizontal)
        .spacing(6)
        .hexpand(true)
        .build();
    model_row.add_css_class("moose-model-row");
    model_row.append(&model_picker);

    composer.append(&input_row);
    composer.append(&model_row);

    let composer_clamp = adw::Clamp::builder()
        .maximum_size(980)
        .tightening_threshold(560)
        .margin_top(10)
        .margin_bottom(16)
        .margin_start(12)
        .margin_end(12)
        .hexpand(true)
        .child(&composer)
        .build();

    root.append(&message_stack);
    root.append(&composer_clamp);

    Chat {
        root,
        messages,
        status_page,
        message_stack,
        entry,
        model_picker,
        send_button,
        stop_button,
    }
}

pub(super) fn append_stored_message(messages: &gtk::Box, message: &Message) {
    let content = stored_message_content(message);
    append_message(messages, message_role_label(&message.role), &content);
}

pub(super) fn append_message(messages: &gtk::Box, role: &str, content: &str) -> gtk::Label {
    let is_user = role == "You";
    let text_alignment = if is_user { 1.0 } else { 0.0 };
    let justification = if is_user {
        gtk::Justification::Right
    } else {
        gtk::Justification::Left
    };
    let row = gtk::Box::builder()
        .orientation(Orientation::Vertical)
        .spacing(6)
        .halign(Align::Fill)
        .hexpand(true)
        .build();
    let role_label = gtk::Label::builder()
        .label(role)
        .halign(Align::Fill)
        .xalign(text_alignment)
        .justify(justification)
        .build();
    let content_label = gtk::Label::builder()
        .label(content)
        .halign(Align::Fill)
        .hexpand(true)
        .xalign(text_alignment)
        .justify(justification)
        .wrap(true)
        .wrap_mode(gtk::pango::WrapMode::Char)
        .natural_wrap_mode(gtk::NaturalWrapMode::None)
        .width_chars(120)
        .selectable(true)
        .build();

    row.add_css_class("moose-message");
    if is_user {
        role_label.add_css_class("moose-message-user");
    }
    role_label.add_css_class("caption-heading");
    role_label.add_css_class("dim-label");
    content_label.add_css_class("body");
    content_label.add_css_class("moose-message-content");
    row.append(&role_label);
    row.append(&content_label);
    messages.append(&row);
    content_label
}

pub(super) fn set_empty_state(status_page: &adw::StatusPage, title: &str, description: &str) {
    status_page.set_title(title);
    status_page.set_description(Some(description));
}

fn message_role_label(role: &MessageRole) -> &'static str {
    match role {
        MessageRole::System => "System",
        MessageRole::User => "You",
        MessageRole::Assistant => "Assistant",
        MessageRole::Tool => "Tool",
    }
}

fn stored_message_content(message: &Message) -> String {
    match message.status {
        MessageStatus::Streaming if message.content.trim().is_empty() => {
            "Generating response...".to_string()
        }
        MessageStatus::Cancelled => message_content_with_state(message, "Generation cancelled"),
        MessageStatus::Failed => message_content_with_state(message, "Generation failed"),
        _ => message.content.clone(),
    }
}

fn message_content_with_state(message: &Message, state: &str) -> String {
    if message.content.trim().is_empty() {
        state.to_string()
    } else {
        format!("{}\n\n{state}", message.content)
    }
}
