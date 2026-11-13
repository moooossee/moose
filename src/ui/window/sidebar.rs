use crate::i18n::gettext;

use adw::prelude::*;
use gtk::{Align, Orientation, PolicyType};

use super::{provider_controls, widgets::icon_button};
use crate::APPLICATION_NAME;

pub(super) struct Sidebar {
    pub(super) root: adw::ToolbarView,
    pub(super) new_chat_button: gtk::Button,
    pub(super) model_manager_button: gtk::Button,
    pub(super) files_button: gtk::Button,
    pub(super) provider_switcher: provider_controls::Switcher,
    pub(super) search_entry: gtk::SearchEntry,
    pub(super) archived_button: gtk::ToggleButton,
    pub(super) conversation_list: gtk::ListBox,
}

pub(super) fn build() -> Sidebar {
    let root = adw::ToolbarView::new();
    root.set_top_bar_style(adw::ToolbarStyle::Flat);
    root.set_bottom_bar_style(adw::ToolbarStyle::Flat);
    root.add_css_class("moose-sidebar");

    let header_bar = gtk::Box::builder()
        .orientation(Orientation::Horizontal)
        .spacing(6)
        .valign(Align::Center)
        .build();
    header_bar.add_css_class("moose-sidebar-header");

    let title = gtk::Label::builder()
        .label(APPLICATION_NAME)
        .halign(Align::Start)
        .valign(Align::Center)
        .hexpand(true)
        .xalign(0.0)
        .build();
    title.add_css_class("title");
    title.add_css_class("moose-sidebar-title");

    let new_chat_button = icon_button("list-add-symbolic", &gettext("New Conversation"));
    let model_manager_button = icon_button("view-list-symbolic", &gettext("Models"));
    let files_button = icon_button("folder-documents-symbolic", &gettext("Files and Library"));
    new_chat_button.add_css_class("moose-sidebar-button");
    model_manager_button.add_css_class("moose-sidebar-button");
    files_button.add_css_class("moose-sidebar-button");

    header_bar.append(&title);
    header_bar.append(&files_button);
    header_bar.append(&model_manager_button);
    header_bar.append(&new_chat_button);

    let search_entry = gtk::SearchEntry::builder()
        .placeholder_text(gettext("Search chats"))
        .hexpand(true)
        .build();
    search_entry.add_css_class("moose-chat-search");

    let archived_button = gtk::ToggleButton::builder()
        .icon_name("folder-symbolic")
        .tooltip_text(gettext("Show Archived Chats"))
        .build();
    archived_button.add_css_class("flat");
    archived_button.add_css_class("moose-sidebar-button");

    let search_box = gtk::Box::builder()
        .orientation(Orientation::Horizontal)
        .spacing(6)
        .build();
    search_box.add_css_class("moose-sidebar-search");
    search_box.append(&search_entry);
    search_box.append(&archived_button);

    let conversation_list = gtk::ListBox::new();
    conversation_list.add_css_class("navigation-sidebar");
    conversation_list.add_css_class("moose-conversation-list");
    conversation_list.set_selection_mode(gtk::SelectionMode::Single);

    let scrolled = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(PolicyType::Never)
        .vscrollbar_policy(PolicyType::Automatic)
        .hexpand(true)
        .vexpand(true)
        .child(&conversation_list)
        .build();
    scrolled.add_css_class("moose-sidebar-scroll");

    let chats_label = gtk::Label::builder()
        .label(gettext("Chats"))
        .halign(Align::Start)
        .xalign(0.0)
        .build();
    chats_label.add_css_class("moose-sidebar-section");

    let content = gtk::Box::builder()
        .orientation(Orientation::Vertical)
        .spacing(0)
        .vexpand(true)
        .build();
    content.add_css_class("moose-sidebar-content");
    content.append(&search_box);
    content.append(&chats_label);
    content.append(&scrolled);

    let provider_switcher = provider_controls::Switcher::new();
    let footer = gtk::Box::builder()
        .orientation(Orientation::Vertical)
        .spacing(2)
        .margin_top(6)
        .margin_bottom(6)
        .margin_start(6)
        .margin_end(6)
        .build();
    footer.append(&provider_switcher.button);

    root.add_top_bar(&header_bar);
    root.set_content(Some(&content));
    root.add_bottom_bar(&footer);

    Sidebar {
        root,
        new_chat_button,
        model_manager_button,
        files_button,
        provider_switcher,
        search_entry,
        archived_button,
        conversation_list,
    }
}
