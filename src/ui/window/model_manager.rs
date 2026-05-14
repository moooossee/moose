use adw::prelude::*;
use gtk::{Align, Orientation, PolicyType};

use crate::APPLICATION_ID;

use super::widgets::{icon_button, section_label};

pub(super) fn build() -> gtk::Box {
    let root = gtk::Box::builder()
        .orientation(Orientation::Vertical)
        .spacing(0)
        .hexpand(true)
        .vexpand(true)
        .build();
    root.add_css_class("moose-model-manager");

    let content = gtk::Box::builder()
        .orientation(Orientation::Vertical)
        .spacing(16)
        .hexpand(true)
        .vexpand(true)
        .build();
    content.add_css_class("moose-model-manager-content");

    let header = gtk::Box::builder()
        .orientation(Orientation::Vertical)
        .spacing(12)
        .halign(Align::Center)
        .hexpand(true)
        .build();
    header.add_css_class("moose-model-manager-header");

    let title_row = gtk::Box::builder()
        .orientation(Orientation::Horizontal)
        .spacing(8)
        .halign(Align::Center)
        .build();

    let title = gtk::Label::builder()
        .label("Models")
        .halign(Align::Center)
        .xalign(0.0)
        .build();
    title.add_css_class("title-2");

    let actions = gtk::Box::builder()
        .orientation(Orientation::Horizontal)
        .spacing(6)
        .halign(Align::Center)
        .build();

    let pull_button = icon_button("folder-download-symbolic", "Pull Model");
    pull_button.add_css_class("suggested-action");
    pull_button.set_sensitive(false);

    let refresh_button = icon_button("view-refresh-symbolic", "Refresh Models");
    refresh_button.set_sensitive(false);

    title_row.append(&title);
    actions.append(&refresh_button);
    actions.append(&pull_button);

    let search_entry = gtk::SearchEntry::builder()
        .placeholder_text("Search Models")
        .halign(Align::Center)
        .build();
    search_entry.set_size_request(420, -1);
    search_entry.set_sensitive(false);
    search_entry.add_css_class("moose-model-search");

    header.append(&title_row);
    header.append(&actions);
    header.append(&search_entry);

    let installed_label = section_label("Installed Models");
    installed_label.add_css_class("moose-model-section");

    let model_list = gtk::ListBox::new();
    model_list.set_selection_mode(gtk::SelectionMode::None);
    model_list.add_css_class("moose-model-list");

    let status_page = adw::StatusPage::builder()
        .icon_name(APPLICATION_ID)
        .title("No Models Loaded")
        .hexpand(true)
        .vexpand(true)
        .build();

    let empty_clamp = adw::Clamp::builder()
        .maximum_size(760)
        .tightening_threshold(520)
        .valign(Align::Center)
        .hexpand(true)
        .vexpand(true)
        .child(&status_page)
        .build();

    let stack = gtk::Stack::builder().hexpand(true).vexpand(true).build();
    stack.add_named(&empty_clamp, Some("empty"));
    stack.add_named(&model_list, Some("models"));
    stack.set_visible_child_name("empty");

    let scrolled = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(PolicyType::Never)
        .vscrollbar_policy(PolicyType::Automatic)
        .hexpand(true)
        .vexpand(true)
        .child(&stack)
        .build();
    scrolled.add_css_class("moose-model-scroll");

    content.append(&header);
    content.append(&installed_label);
    content.append(&scrolled);

    let clamp = adw::Clamp::builder()
        .maximum_size(900)
        .tightening_threshold(640)
        .hexpand(true)
        .vexpand(true)
        .child(&content)
        .build();

    root.append(&clamp);
    root
}
