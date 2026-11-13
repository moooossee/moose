use crate::i18n::{gettext, gettext_noop};

use std::rc::Rc;

use adw::prelude::*;
use gtk::{Align, Orientation};

pub(super) fn build(
    entry: &gtk::TextView,
    previews: &gtk::FlowBox,
    attachment_button: &gtk::MenuButton,
) -> adw::Clamp {
    let content = gtk::Box::new(Orientation::Vertical, 16);
    content.add_css_class("moose-welcome-content");
    let suggestions = gtk::FlowBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .min_children_per_line(1)
        .max_children_per_line(3)
        .homogeneous(true)
        .column_spacing(8)
        .row_spacing(8)
        .build();
    suggestions.add_css_class("moose-welcome-suggestions");
    let mut buttons = Vec::new();
    for (title, prompt) in [
        (
            gettext_noop("Explain a topic"),
            gettext_noop("Explain [topic] in simple terms, with a practical example."),
        ),
        (
            gettext_noop("Help me write"),
            gettext_noop(
                "Help me write [what you need]. Ask me about the audience and tone first.",
            ),
        ),
        (
            gettext_noop("Work through code"),
            gettext_noop("Help me with [a coding question]. Explain the approach step by step."),
        ),
    ] {
        let button = gtk::Button::with_label(&gettext(title));
        button.add_css_class("flat");
        button.add_css_class("moose-welcome-suggestion");
        button.set_tooltip_text(Some(&gettext("Start an editable draft")));
        let weak_entry = entry.downgrade();
        let prompt = gettext(prompt);
        button.connect_clicked(move |_| {
            let Some(entry) = weak_entry.upgrade() else {
                return;
            };
            let buffer = entry.buffer();
            if buffer.char_count() > 0 {
                entry.grab_focus();
                return;
            }
            entry.grab_focus();
            buffer.set_text(&prompt);
            if let Some(position) = prompt.find('[')
                && let Some(length) = prompt[position..].find(']')
            {
                let start = prompt[..position].chars().count() as i32;
                let end = prompt[..position + length + 1].chars().count() as i32;
                buffer.select_range(&buffer.iter_at_offset(start), &buffer.iter_at_offset(end));
            }
        });
        let child = gtk::FlowBoxChild::new();
        child.set_focusable(false);
        child.set_child(Some(&button));
        suggestions.insert(&child, -1);
        buttons.push(button.downgrade());
    }
    content.append(&suggestions);
    let files = gtk::Button::with_label(&gettext("Use an Image or Document"));
    files.set_halign(Align::Center);
    files.add_css_class("flat");
    files.add_css_class("moose-welcome-files");
    files.set_tooltip_text(Some(&gettext(
        "Add files, paste an image, or open your document library",
    )));
    let weak_attachment_button = attachment_button.downgrade();
    files.connect_clicked(move |_| {
        if let Some(button) = weak_attachment_button.upgrade() {
            button.grab_focus();
            button.popup();
        }
    });
    content.append(&files);
    let hint = gtk::Label::builder()
        .label(gettext(
            "Suggestions fill your draft. You choose when to send.",
        ))
        .single_line_mode(true)
        .width_chars(1)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .build();
    hint.add_css_class("caption");
    hint.add_css_class("dim-label");
    content.append(&hint);
    let weak_hint = hint.downgrade();
    let weak_entry = entry.downgrade();
    let weak_previews = previews.downgrade();
    let update = Rc::new(move || {
        let (Some(hint), Some(entry), Some(previews)) = (
            weak_hint.upgrade(),
            weak_entry.upgrade(),
            weak_previews.upgrade(),
        ) else {
            return;
        };
        let has_draft = entry.buffer().char_count() > 0;
        for button in &buttons {
            if let Some(button) = button.upgrade() {
                button.set_sensitive(!has_draft);
            }
        }
        let text = gettext(if previews.is_visible() {
            gettext_noop("Your files are attached. Ask a question about them below.")
        } else if has_draft {
            gettext_noop("Your draft is below. Make it yours before sending.")
        } else {
            gettext_noop("Suggestions fill your draft. You choose when to send.")
        });
        hint.set_label(&text);
        hint.set_tooltip_text(Some(&text));
    });
    let on_change = update.clone();
    entry.buffer().connect_changed(move |_| on_change());
    let on_change = update.clone();
    previews.connect_visible_notify(move |_| on_change());
    update();
    adw::Clamp::builder()
        .maximum_size(520)
        .tightening_threshold(380)
        .child(&content)
        .build()
}

pub(super) fn show(status: &adw::StatusPage, content: &adw::Clamp) {
    status.add_css_class("compact");
    status.add_css_class("moose-chat-welcome");
    status.set_title(&gettext("What would you like to explore?"));
    status.set_description(Some(&gettext(
        "Ask a question, work through an idea, or bring a file.",
    )));
    status.set_child(Some(content));
}
