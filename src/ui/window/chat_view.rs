use std::{cell::Cell, time::Duration};

use adw::prelude::*;
use gtk::{Align, Orientation, glib::DateTime};

use crate::{
    APPLICATION_ID,
    conversations::{Message, MessageRole, MessageStatus},
};

use super::{
    markdown_live::LiveMarkdown,
    markdown_view,
    widgets::{composer_button, icon_button, string_list_factory},
};

pub(super) struct Chat {
    pub(super) root: gtk::Box,
    pub(super) messages: gtk::Box,
    pub(super) messages_scrolled: gtk::ScrolledWindow,
    pub(super) status_page: adw::StatusPage,
    pub(super) welcome: adw::Clamp,
    pub(super) message_stack: gtk::Stack,
    pub(super) entry: gtk::TextView,
    pub(super) model_picker: gtk::DropDown,
    pub(super) profile_label: gtk::Label,
    pub(super) chat_settings_button: gtk::Button,
    pub(super) send_button: gtk::Button,
    pub(super) stop_button: gtk::Button,
    pub(super) draft_label: gtk::Label,
    pub(super) thinking_picker: gtk::DropDown,
    pub(super) attachments: super::attachments::Controls,
}

pub(super) struct StreamingMessage {
    content: LiveMarkdown,
    indicator: gtk::Box,
    spinner: gtk::Spinner,
    status: gtk::Label,
    reasoning: gtk::Expander,
    reasoning_text: gtk::TextBuffer,
    content_length: Cell<usize>,
    reasoning_length: Cell<usize>,
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
        .spacing(12)
        .hexpand(true)
        .build();
    messages.add_css_class("moose-chat-column");

    let scrolled = gtk::ScrolledWindow::builder()
        .hexpand(true)
        .vexpand(true)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vscrollbar_policy(gtk::PolicyType::External)
        .overlay_scrolling(true)
        .child(&messages)
        .build();

    let message_stack = gtk::Stack::builder().hexpand(true).vexpand(true).build();
    message_stack.add_named(&empty_clamp, Some("empty"));
    message_stack.add_named(&scrolled, Some("messages"));
    message_stack.set_visible_child_name("empty");

    let composer = gtk::Box::builder()
        .orientation(Orientation::Vertical)
        .spacing(0)
        .hexpand(true)
        .build();
    composer.add_css_class("moose-composer");

    let composer_area = gtk::Box::builder()
        .orientation(Orientation::Vertical)
        .spacing(4)
        .hexpand(true)
        .margin_top(8)
        .margin_bottom(12)
        .build();

    let entry_buffer = gtk::TextBuffer::new(None);
    let entry = gtk::TextView::builder()
        .buffer(&entry_buffer)
        .accepts_tab(false)
        .bottom_margin(10)
        .hexpand(true)
        .left_margin(18)
        .right_margin(18)
        .top_margin(16)
        .wrap_mode(gtk::WrapMode::WordChar)
        .build();
    entry.add_css_class("flat");
    entry.add_css_class("moose-composer-entry");

    let entry_scroll = gtk::ScrolledWindow::builder()
        .child(&entry)
        .hexpand(true)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .max_content_height(180)
        .min_content_height(56)
        .propagate_natural_height(true)
        .vscrollbar_policy(gtk::PolicyType::Automatic)
        .build();
    entry_scroll.add_css_class("moose-composer-input");

    let entry_placeholder = gtk::Label::builder()
        .can_target(false)
        .halign(Align::Start)
        .label("Ask anything…")
        .margin_start(18)
        .margin_top(16)
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

    let stop_button = composer_button("media-playback-stop-symbolic", "Stop Response");
    let send_button = composer_button("go-up-symbolic", "Send Message (Enter)");
    send_button.add_css_class("moose-send-button");
    stop_button.add_css_class("moose-stop-button");
    send_button.set_sensitive(false);
    stop_button.set_sensitive(false);
    send_button.set_valign(Align::Center);
    stop_button.set_valign(Align::Center);
    let attachments = super::attachments::build();
    let composer_actions = gtk::Box::new(Orientation::Horizontal, 10);
    composer_actions.add_css_class("moose-composer-actions");
    let action_stack = gtk::Stack::builder()
        .transition_type(gtk::StackTransitionType::Crossfade)
        .transition_duration(120)
        .valign(Align::Center)
        .build();
    action_stack.add_named(&send_button, Some("send"));
    action_stack.add_named(&stop_button, Some("stop"));
    let hint = gtk::Label::builder()
        .label("Enter to send · Shift+Enter for a new line")
        .xalign(0.0)
        .width_chars(1)
        .hexpand(true)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .build();
    hint.add_css_class("moose-composer-hint");
    let feedback_revealer = gtk::Revealer::builder()
        .child(&hint)
        .hexpand(true)
        .valign(Align::Center)
        .transition_type(gtk::RevealerTransitionType::Crossfade)
        .transition_duration(120)
        .build();
    composer_actions.append(&attachments.button);
    composer_actions.append(&feedback_revealer);
    composer_actions.append(&action_stack);
    let focus = gtk::EventControllerFocus::new();
    composer.add_controller(focus.clone());
    let weak_composer = composer.downgrade();
    let weak_entry = entry.downgrade();
    let weak_placeholder = entry_placeholder.downgrade();
    let weak_stop = stop_button.downgrade();
    let weak_previews = attachments.previews.downgrade();
    let weak_stack = action_stack.downgrade();
    let weak_hint = hint.downgrade();
    let weak_revealer = feedback_revealer.downgrade();
    let weak_focus = focus.downgrade();
    let update_feedback = std::rc::Rc::new(move || {
        let (
            Some(composer),
            Some(entry),
            Some(placeholder),
            Some(stop),
            Some(previews),
            Some(stack),
            Some(hint),
            Some(revealer),
            Some(focus),
        ) = (
            weak_composer.upgrade(),
            weak_entry.upgrade(),
            weak_placeholder.upgrade(),
            weak_stop.upgrade(),
            weak_previews.upgrade(),
            weak_stack.upgrade(),
            weak_hint.upgrade(),
            weak_revealer.upgrade(),
            weak_focus.upgrade(),
        )
        else {
            return;
        };
        let running = stop.is_sensitive();
        let has_text = entry.buffer().char_count() > 0;
        if has_text {
            composer.add_css_class("has-text");
        } else {
            composer.remove_css_class("has-text");
        }
        placeholder.set_visible(!has_text);
        placeholder.set_label(if previews.is_visible() {
            "Ask about your files…"
        } else if running {
            "Write your next message…"
        } else {
            "Ask anything…"
        });
        if !running && stop.has_focus() {
            entry.grab_focus();
        }
        stack.set_visible_child_name(if running { "stop" } else { "send" });
        let text = if running {
            "Responding… You can keep writing"
        } else {
            "Enter to send · Shift+Enter for a new line"
        };
        hint.set_label(text);
        hint.set_tooltip_text(Some(text));
        revealer.set_reveal_child(running || focus.contains_focus());
    });
    let update = update_feedback.clone();
    entry_buffer.connect_changed(move |_| update());
    let update = update_feedback.clone();
    stop_button.connect_sensitive_notify(move |_| update());
    let update = update_feedback.clone();
    attachments
        .previews
        .connect_visible_notify(move |_| update());
    let update = update_feedback.clone();
    focus.connect_contains_focus_notify(move |_| update());
    update_feedback();

    let model_picker = gtk::DropDown::from_strings(&["No model selected"]);
    model_picker.set_tooltip_text(Some("Active Model"));
    model_picker.set_sensitive(false);
    model_picker.set_halign(Align::Start);
    model_picker.set_valign(Align::Center);
    model_picker.set_hexpand(false);
    model_picker.set_enable_search(false);
    model_picker.set_factory(Some(&string_list_factory(22, false)));
    model_picker.set_list_factory(Some(&string_list_factory(44, true)));
    model_picker.add_css_class("flat");
    model_picker.add_css_class("moose-model-picker");

    let chat_settings_button = icon_button("preferences-system-symbolic", "Chat Settings");
    chat_settings_button.add_css_class("moose-chat-settings-button");

    let profile_label = gtk::Label::new(None);
    profile_label.set_width_chars(1);
    profile_label.set_max_width_chars(14);
    profile_label.set_ellipsize(gtk::pango::EllipsizeMode::End);
    profile_label.add_css_class("moose-profile-badge");
    profile_label.set_visible(false);

    let thinking_picker = gtk::DropDown::from_strings(&["Reasoning: Auto"]);
    thinking_picker.add_css_class("flat");
    thinking_picker.add_css_class("moose-reasoning-picker");
    thinking_picker.set_valign(Align::Center);
    thinking_picker.set_tooltip_text(Some("Use the model’s default reasoning behavior"));
    thinking_picker.set_sensitive(false);
    thinking_picker.set_factory(Some(&string_list_factory(18, false)));
    thinking_picker.set_list_factory(Some(&string_list_factory(24, true)));

    let model_row = gtk::Box::builder()
        .orientation(Orientation::Horizontal)
        .spacing(4)
        .halign(Align::Start)
        .build();
    model_row.add_css_class("moose-model-row");
    model_row.append(&model_picker);
    model_row.append(&thinking_picker);
    model_row.append(&attachments.capability);
    model_row.append(&profile_label);
    model_row.append(&chat_settings_button);

    let attachment_scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vscrollbar_policy(gtk::PolicyType::Automatic)
        .propagate_natural_height(true)
        .max_content_height(168)
        .child(&attachments.previews)
        .build();
    let attachment_section = gtk::Box::new(Orientation::Vertical, 8);
    attachment_section.add_css_class("moose-attachment-section");
    attachment_section.append(&attachments.summary);
    attachment_section.append(&attachment_scroll);
    attachments
        .previews
        .bind_property("visible", &attachment_section, "visible")
        .sync_create()
        .build();
    composer.append(&attachment_section);
    composer.append(&entry_overlay);
    composer.append(&composer_actions);
    let attachment_status = gtk::Box::new(Orientation::Horizontal, 8);
    attachments.status.set_hexpand(true);
    attachment_status.append(&attachments.status);
    attachment_status.append(&attachments.cancel);
    attachment_status.add_css_class("moose-attachment-status");
    attachments
        .status
        .bind_property("visible", &attachment_status, "visible")
        .sync_create()
        .build();
    composer_area.append(&composer);
    composer_area.append(&attachment_status);
    let draft_label = gtk::Label::builder()
        .xalign(1.0)
        .hexpand(true)
        .valign(Align::Center)
        .width_chars(14)
        .max_width_chars(14)
        .single_line_mode(true)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .build();
    draft_label.add_css_class("caption");
    draft_label.add_css_class("dim-label");
    let composer_footer = gtk::Box::new(Orientation::Horizontal, 8);
    composer_footer.add_css_class("moose-composer-footer");
    model_row.set_valign(Align::Center);
    composer_footer.append(&model_row);
    composer_footer.append(&draft_label);
    composer_area.append(&composer_footer);

    let column = gtk::Box::builder()
        .orientation(Orientation::Vertical)
        .hexpand(true)
        .vexpand(true)
        .build();
    column.append(&message_stack);
    column.append(&composer_area);

    let column_clamp = adw::Clamp::builder()
        .maximum_size(1080)
        .tightening_threshold(800)
        .margin_start(16)
        .margin_end(16)
        .hexpand(true)
        .vexpand(true)
        .child(&column)
        .build();

    root.append(&column_clamp);
    let welcome = super::chat_welcome::build(&entry, &attachments.previews, &attachments.button);
    super::chat_welcome::show(&status_page, &welcome);

    Chat {
        root,
        messages,
        messages_scrolled: scrolled,
        status_page,
        welcome,
        message_stack,
        entry,
        model_picker,
        profile_label,
        chat_settings_button,
        send_button,
        stop_button,
        draft_label,
        thinking_picker,
        attachments,
    }
}

pub(super) fn append_stored_message(messages: &gtk::Box, message: &Message) -> gtk::Box {
    let content = stored_message_content(message);
    append_message(
        messages,
        message_role_label(&message.role),
        &content,
        Some(&message.created_at),
    )
}

pub(super) fn append_message(
    messages: &gtk::Box,
    role: &str,
    content: &str,
    created_at: Option<&str>,
) -> gtk::Box {
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
        .height_request(24)
        .halign(Align::Fill)
        .width_chars(1)
        .ellipsize(gtk::pango::EllipsizeMode::End)
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
    role_label.add_css_class("moose-message-meta");
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
    row
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
        .height_request(24)
        .halign(Align::Fill)
        .width_chars(1)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .xalign(0.0)
        .justify(gtk::Justification::Left)
        .build();
    let thinking_indicator = gtk::Box::builder()
        .orientation(Orientation::Horizontal)
        .spacing(6)
        .halign(Align::Start)
        .valign(Align::Center)
        .build();
    let spinner = gtk::Spinner::new();
    spinner.set_size_request(12, 12);
    spinner.start();
    spinner.add_css_class("moose-thinking-spinner");

    let thinking_label = gtk::Label::builder()
        .label("Waiting for response…")
        .halign(Align::Fill)
        .width_chars(1)
        .max_width_chars(28)
        .xalign(0.0)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .build();
    thinking_label.add_css_class("moose-thinking-label");
    let (reasoning, reasoning_text) = reasoning_section();
    reasoning.set_visible(false);

    let content = LiveMarkdown::new();
    content.widget().set_visible(false);

    row.add_css_class("moose-message");
    role_label.add_css_class("caption-heading");
    role_label.add_css_class("dim-label");
    role_label.add_css_class("moose-message-meta");
    thinking_indicator.add_css_class("moose-thinking");
    thinking_indicator.append(&spinner);
    thinking_indicator.append(&thinking_label);
    let header = gtk::Box::builder()
        .orientation(Orientation::Vertical)
        .spacing(2)
        .build();
    header.append(&role_label);
    header.append(&thinking_indicator);
    row.append(&header);
    row.append(&reasoning);
    row.append(content.widget());
    messages.append(&row);

    StreamingMessage {
        content,
        indicator: thinking_indicator,
        spinner,
        status: thinking_label,
        reasoning,
        reasoning_text,
        content_length: Cell::new(0),
        reasoning_length: Cell::new(0),
    }
}

pub(super) fn update_streaming_message(
    message: &StreamingMessage,
    content: &str,
    reasoning: &str,
    elapsed: u64,
    reasoning_ms: i64,
) {
    let has_content = !content.is_empty();
    message.indicator.set_visible(!has_content);
    if has_content {
        message.spinner.stop();
    } else {
        message.spinner.start();
        let phase = if reasoning.is_empty() {
            "Waiting for response"
        } else {
            "Reasoning"
        };
        message.status.set_label(&format!("{phase} · {elapsed}s"));
    }
    message.content.widget().set_visible(has_content);
    if message.content_length.replace(content.len()) != content.len() {
        message.content.update(content);
    }
    message.reasoning.set_visible(!reasoning.is_empty());
    message
        .reasoning
        .set_label(Some(&format!("Reasoning · {}s", reasoning_ms / 1000)));
    let previous_length = message.reasoning_length.replace(reasoning.len());
    if previous_length < reasoning.len() {
        message.reasoning_text.insert(
            &mut message.reasoning_text.end_iter(),
            &reasoning[previous_length..],
        );
    } else if previous_length > reasoning.len() {
        message.reasoning_text.set_text(reasoning);
    }
}

fn reasoning_section() -> (gtk::Expander, gtk::TextBuffer) {
    let buffer = gtk::TextBuffer::new(None);
    let view = gtk::TextView::builder()
        .buffer(&buffer)
        .editable(false)
        .cursor_visible(false)
        .wrap_mode(gtk::WrapMode::WordChar)
        .left_margin(12)
        .right_margin(12)
        .top_margin(10)
        .bottom_margin(10)
        .build();
    view.add_css_class("moose-reasoning-text");
    let scroll = gtk::ScrolledWindow::builder()
        .child(&view)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .min_content_height(64)
        .max_content_height(240)
        .propagate_natural_height(true)
        .build();
    let expander = gtk::Expander::builder()
        .label("Reasoning")
        .child(&scroll)
        .expanded(false)
        .build();
    expander.add_css_class("moose-reasoning");
    (expander, buffer)
}

pub(super) fn append_details(row: &gtk::Box, details: &crate::storage::MessageDetails) {
    if !details.reasoning.is_empty() {
        let (expander, buffer) = reasoning_section();
        buffer.set_text(&details.reasoning);
        expander.set_label(Some(&format!(
            "Reasoning · {}s",
            details.reasoning_ms / 1000
        )));
        row.insert_child_after(&expander, row.first_child().as_ref());
    }
    let label = gtk::Label::builder()
        .label(format!(
            "{} · {}s",
            details.model,
            details.elapsed_ms / 1000
        ))
        .xalign(0.0)
        .build();
    label.add_css_class("caption");
    label.add_css_class("dim-label");
    row.append(&label);
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
    status_page.remove_css_class("compact");
    status_page.remove_css_class("moose-chat-welcome");
    status_page.set_title(title);
    status_page.set_description(Some(description));
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
