use crate::i18n::gettext;

use adw::prelude::*;
use gtk::{Orientation, PolicyType};

use super::{folders, provider_controls};

pub(super) struct Sidebar {
    pub(super) root: adw::ToolbarView,
    pub(super) provider_switcher: provider_controls::Switcher,
    pub(super) search_entry: gtk::SearchEntry,
    pub(super) archived_button: gtk::ToggleButton,
    pub(super) conversation_list: gtk::ListBox,
    pub(super) folders: folders::Controls,
}

pub(super) fn build(header_bar: &adw::HeaderBar) -> Sidebar {
    let root = adw::ToolbarView::new();
    root.set_top_bar_style(adw::ToolbarStyle::Flat);
    root.set_bottom_bar_style(adw::ToolbarStyle::Flat);
    root.add_css_class("moose-sidebar");

    let search_entry = gtk::SearchEntry::builder()
        .placeholder_text(gettext("Search chats"))
        .hexpand(true)
        .build();
    search_entry.add_css_class("moose-chat-search");

    let archived_button = gtk::ToggleButton::builder()
        .icon_name("moose-archive-symbolic")
        .tooltip_text(gettext("Show Archived Chats"))
        .valign(gtk::Align::Center)
        .build();
    archived_button.add_css_class("flat");
    archived_button.add_css_class("moose-sidebar-button");
    archived_button.add_css_class("moose-archive-button");
    archived_button.update_property(&[gtk::accessible::Property::Label(&gettext(
        "Show Archived Chats",
    ))]);

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
    super::conversation_list::configure_headers(&conversation_list);

    let folders = folders::build();
    let lists = gtk::Box::new(Orientation::Vertical, 6);
    lists.append(&folders.root);
    lists.append(&folders.navigation);
    lists.append(&conversation_list);

    let scrolled = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(PolicyType::Never)
        .vscrollbar_policy(PolicyType::Automatic)
        .hexpand(true)
        .vexpand(true)
        .child(&lists)
        .build();
    scrolled.add_css_class("moose-sidebar-scroll");

    let content = gtk::Box::builder()
        .orientation(Orientation::Vertical)
        .spacing(0)
        .vexpand(true)
        .build();
    content.add_css_class("moose-sidebar-content");
    content.append(&search_box);
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

    root.add_top_bar(header_bar);
    root.set_content(Some(&content));
    root.add_bottom_bar(&footer);

    Sidebar {
        root,
        provider_switcher,
        search_entry,
        archived_button,
        conversation_list,
        folders,
    }
}
