use adw::prelude::*;
use gtk::Align;

pub(super) fn icon_button(icon_name: &str, tooltip: &str) -> gtk::Button {
    let button = gtk::Button::from_icon_name(icon_name);
    button.add_css_class("flat");
    button.set_tooltip_text(Some(tooltip));
    button
}

pub(super) fn composer_button(icon_name: &str, tooltip: &str) -> gtk::Button {
    let button = gtk::Button::from_icon_name(icon_name);
    button.add_css_class("circular");
    button.add_css_class("moose-composer-button");
    button.set_tooltip_text(Some(tooltip));
    button
}

pub(super) fn section_label(text: &str) -> gtk::Label {
    let label = gtk::Label::builder()
        .label(text)
        .halign(Align::Start)
        .xalign(0.0)
        .build();
    label.add_css_class("heading");
    label
}

pub(super) fn status_label(text: &str) -> gtk::Label {
    let label = gtk::Label::builder()
        .label(text)
        .halign(Align::Center)
        .valign(Align::Center)
        .build();
    label.add_css_class("dim-label");
    label
}
