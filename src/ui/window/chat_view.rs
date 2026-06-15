use std::{cell::Cell, rc::Rc, time::Duration};

use adw::prelude::*;
use gtk::{Align, Orientation, glib::DateTime};

use crate::{
    APPLICATION_ID,
    conversations::{Message, MessageRole, MessageStatus},
};

use super::{
    markdown_live::LiveMarkdown,
    markdown_view,
    widgets::{composer_button, icon_button},
};

pub(super) struct Chat {
    pub(super) root: gtk::Box,
    pub(super) messages: gtk::Box,
    pub(super) messages_scrolled: gtk::ScrolledWindow,
    pub(super) status_page: adw::StatusPage,
    pub(super) message_stack: gtk::Stack,
    pub(super) entry: gtk::TextView,
    pub(super) model_picker: gtk::DropDown,
    pub(super) profile_label: gtk::Label,
    pub(super) chat_settings_button: gtk::Button,
    pub(super) send_button: gtk::Button,
    pub(super) stop_button: gtk::Button,
}

pub(super) struct StreamingMessage {
    content: LiveMarkdown,
    thinking_indicator: gtk::Box,
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

    let composer_area = gtk::Box::builder()
        .orientation(Orientation::Vertical)
        .spacing(6)
        .hexpand(true)
        .build();

    let input_row = gtk::Box::builder()
        .orientation(Orientation::Horizontal)
        .spacing(6)
        .hexpand(true)
        .build();

    let entry_buffer = gtk::TextBuffer::new(None);
    let entry = gtk::TextView::builder()
        .buffer(&entry_buffer)
        .accepts_tab(false)
        .bottom_margin(10)
        .hexpand(true)
        .left_margin(12)
        .right_margin(12)
        .top_margin(10)
        .wrap_mode(gtk::WrapMode::WordChar)
        .build();
    entry.add_css_class("flat");
    entry.add_css_class("moose-composer-entry");

    let entry_scroll = gtk::ScrolledWindow::builder()
        .child(&entry)
        .hexpand(true)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .max_content_height(156)
        .min_content_height(44)
        .propagate_natural_height(true)
        .vscrollbar_policy(gtk::PolicyType::Automatic)
        .build();
    entry_scroll.add_css_class("moose-composer-input");

    let entry_placeholder = gtk::Label::builder()
        .can_target(false)
        .halign(Align::Start)
        .label("Message")
        .margin_start(14)
        .margin_top(11)
        .valign(Align::Start)
        .build();
    entry_placeholder.add_css_class("dim-label");
    entry_placeholder.add_css_class("moose-composer-placeholder");

    let entry_overlay = gtk::Overlay::builder()
        .child(&entry_scroll)
        .hexpand(true)
        .build();
    entry_overlay.add_overlay(&entry_placeholder);
    entry_overlay.set_measure_overlay(&entry_placeholder, false);

    entry_buffer.connect_changed(move |buffer| {
        let (start, end) = buffer.bounds();
        entry_placeholder.set_visible(buffer.text(&start, &end, true).is_empty());
    });

    let stop_button = composer_button("media-playback-stop-symbolic", "Cancel Generation");
    let send_button = composer_button("mail-send-symbolic", "Send Message");

    send_button.add_css_class("suggested-action");
    stop_button.add_css_class("destructive-action");
    send_button.set_sensitive(false);
    stop_button.set_sensitive(false);
    send_button.set_valign(Align::End);
    stop_button.set_valign(Align::End);
    input_row.append(&entry_overlay);
    input_row.append(&stop_button);
    input_row.append(&send_button);

    let model_picker = gtk::DropDown::from_strings(&["No model selected"]);
    model_picker.set_tooltip_text(Some("Active Model"));
    model_picker.set_sensitive(false);
    model_picker.set_halign(Align::Start);
    model_picker.set_size_request(260, -1);
    model_picker.add_css_class("flat");
    model_picker.add_css_class("moose-model-picker");

    let chat_settings_button = icon_button("preferences-system-symbolic", "Chat Settings");
    chat_settings_button.add_css_class("moose-chat-settings-button");

    let profile_label = gtk::Label::new(None);
    profile_label.add_css_class("moose-profile-badge");
    profile_label.set_visible(false);

    let model_icon = gtk::Image::from_icon_name("computer-symbolic");
    model_icon.add_css_class("dim-label");
    model_icon.add_css_class("moose-model-icon");

    let model_row = gtk::Box::builder()
        .orientation(Orientation::Horizontal)
        .spacing(6)
        .halign(Align::Start)
        .build();
    model_row.add_css_class("moose-model-row");
    model_row.append(&model_icon);
    model_row.append(&model_picker);
    model_row.append(&profile_label);
    model_row.append(&chat_settings_button);

    composer.append(&input_row);
    composer_area.append(&composer);
    composer_area.append(&model_row);

    let composer_clamp = adw::Clamp::builder()
        .maximum_size(980)
        .tightening_threshold(560)
        .margin_top(10)
        .margin_bottom(16)
        .margin_start(12)
        .margin_end(12)
        .hexpand(true)
        .child(&composer_area)
        .build();

    root.append(&message_stack);
    root.append(&composer_clamp);

    Chat {
        root,
        messages,
        messages_scrolled: scrolled,
        status_page,
        message_stack,
        entry,
        model_picker,
        profile_label,
        chat_settings_button,
        send_button,
        stop_button,
    }
}

pub(super) fn append_stored_message(messages: &gtk::Box, message: &Message) {
    let content = stored_message_content(message);
    append_message(
        messages,
        message_role_label(&message.role),
        &content,
        Some(&message.created_at),
    );
}

pub(super) fn append_message(
    messages: &gtk::Box,
    role: &str,
    content: &str,
    created_at: Option<&str>,
) {
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
        .label(message_header_label(role, created_at))
        .halign(Align::Fill)
        .xalign(text_alignment)
        .justify(justification)
        .build();
    let content_view = markdown_view::render(content);
    content_view.set_halign(if is_user { Align::Start } else { Align::Fill });
    content_view.set_hexpand(!is_user);

    row.add_css_class("moose-message");
    if is_user {
        row.add_css_class("moose-message-outgoing");
        role_label.add_css_class("moose-message-user");
    }
    role_label.add_css_class("caption-heading");
    role_label.add_css_class("dim-label");
    if is_user {
        role_label.set_xalign(1.0);
        role_label.set_justify(gtk::Justification::Right);
        markdown_view::constrain_labels(&content_view, 72);

        let bubble = gtk::Box::builder()
            .orientation(Orientation::Vertical)
            .spacing(5)
            .halign(Align::End)
            .build();
        bubble.add_css_class("moose-message-user-bubble");
        role_label.add_css_class("moose-message-user-meta");
        content_view.add_css_class("moose-message-user-content");
        bubble.append(&role_label);
        bubble.append(&content_view);
        row.append(&bubble);
    } else {
        row.append(&role_label);
        row.append(&content_view);
    }
    messages.append(&row);
}

pub(super) fn append_streaming_message(
    messages: &gtk::Box,
    model: &str,
    created_at: Option<&str>,
) -> StreamingMessage {
    let row = gtk::Box::builder()
        .orientation(Orientation::Vertical)
        .spacing(6)
        .halign(Align::Fill)
        .hexpand(true)
        .build();
    let role_label = gtk::Label::builder()
        .label(message_header_label(model, created_at))
        .halign(Align::Fill)
        .xalign(0.0)
        .justify(gtk::Justification::Left)
        .build();
    let thinking_indicator = gtk::Box::builder()
        .orientation(Orientation::Horizontal)
        .spacing(8)
        .halign(Align::Start)
        .valign(Align::Center)
        .build();
    let spinner = gtk::Spinner::new();
    spinner.set_size_request(16, 16);
    spinner.start();
    spinner.add_css_class("moose-thinking-spinner");

    let thinking_label = gtk::Label::builder()
        .label(thinking_status(model))
        .halign(Align::Start)
        .hexpand(true)
        .max_width_chars(80)
        .xalign(0.0)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .build();
    thinking_label.add_css_class("moose-thinking-label");
    start_thinking_text_cycle(&thinking_label, model);

    let content = LiveMarkdown::new();
    content.widget().set_visible(false);

    row.add_css_class("moose-message");
    role_label.add_css_class("caption-heading");
    role_label.add_css_class("dim-label");
    thinking_indicator.add_css_class("moose-thinking");
    thinking_indicator.append(&spinner);
    thinking_indicator.append(&thinking_label);
    row.append(&role_label);
    row.append(&thinking_indicator);
    row.append(content.widget());
    messages.append(&row);

    StreamingMessage {
        content,
        thinking_indicator,
    }
}

pub(super) fn set_streaming_message_content(message: &StreamingMessage, content: &str) {
    message.thinking_indicator.set_visible(false);
    message.content.widget().set_visible(true);
    message.content.finish(content);
}

pub(super) fn update_streaming_message_content(message: &StreamingMessage, content: &str) {
    message.thinking_indicator.set_visible(false);
    message.content.widget().set_visible(true);
    message.content.update(content);
}

pub(super) fn scroll_to_bottom(scrolled: &gtk::ScrolledWindow) {
    let adjustment = scrolled.vadjustment();
    gtk::glib::idle_add_local_once(move || {
        set_adjustment_to_bottom(&adjustment);
        schedule_bottom_adjustment(&adjustment, 16);
    });
}

pub(super) fn should_stick_to_bottom(scrolled: &gtk::ScrolledWindow) -> bool {
    is_near_bottom(&scrolled.vadjustment())
}

fn schedule_bottom_adjustment(adjustment: &gtk::Adjustment, delay_ms: u64) {
    let adjustment = adjustment.clone();
    gtk::glib::timeout_add_local_once(Duration::from_millis(delay_ms), move || {
        set_adjustment_to_bottom(&adjustment);
    });
}

fn set_adjustment_to_bottom(adjustment: &gtk::Adjustment) {
    let value = (adjustment.upper() - adjustment.page_size()).max(adjustment.lower());
    adjustment.set_value(value);
}

fn is_near_bottom(adjustment: &gtk::Adjustment) -> bool {
    let bottom = (adjustment.upper() - adjustment.page_size()).max(adjustment.lower());
    bottom - adjustment.value() <= 240.0
}

pub(super) fn set_empty_state(status_page: &adw::StatusPage, title: &str, description: &str) {
    status_page.set_title(title);
    status_page.set_description(Some(description));
}

fn start_thinking_text_cycle(label: &gtk::Label, model: &str) {
    let label = label.clone();
    let model = model.to_string();
    let index = Rc::new(Cell::new(0usize));
    let target_index = Rc::clone(&index);

    gtk::glib::timeout_add_local(Duration::from_millis(1400), move || {
        if !label.is_visible() {
            return gtk::glib::ControlFlow::Break;
        }

        let next_index = (target_index.get() + 1) % 3;
        target_index.set(next_index);
        label.set_label(&match next_index {
            0 => thinking_status(&model),
            1 => "Reading your message...".to_string(),
            _ => "Drafting response...".to_string(),
        });
        gtk::glib::ControlFlow::Continue
    });
}

fn thinking_status(model: &str) -> String {
    format!("{model} is thinking...")
}

fn message_header_label(role: &str, created_at: Option<&str>) -> String {
    created_at
        .map(format_message_timestamp)
        .filter(|timestamp| !timestamp.is_empty())
        .map(|timestamp| format!("{role} - {timestamp}"))
        .unwrap_or_else(|| role.to_string())
}

fn format_message_timestamp(value: &str) -> String {
    DateTime::from_iso8601(value, None)
        .and_then(|timestamp| timestamp.to_local())
        .map(|timestamp| format_message_datetime(&timestamp))
        .unwrap_or_else(|_| value.to_string())
}

fn format_message_datetime(timestamp: &DateTime) -> String {
    let hour = timestamp.hour();
    let display_hour = match hour % 12 {
        0 => 12,
        value => value,
    };
    let meridiem = if hour < 12 { "AM" } else { "PM" };
    let minute = timestamp.minute();

    format!(
        "{} {}, {display_hour}:{minute:02} {meridiem}",
        month_name(timestamp.month()),
        timestamp.day_of_month()
    )
}

fn month_name(month: i32) -> &'static str {
    match month {
        1 => "January",
        2 => "February",
        3 => "March",
        4 => "April",
        5 => "May",
        6 => "June",
        7 => "July",
        8 => "August",
        9 => "September",
        10 => "October",
        11 => "November",
        12 => "December",
        _ => "",
    }
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
