use adw::prelude::*;
use gtk::{Align, Orientation, PolicyType};

use crate::APPLICATION_NAME;
use crate::providers::MANAGED_OLLAMA_BASE_URL;

use super::widgets::{icon_button, status_label};

pub(super) struct Sidebar {
    pub(super) root: adw::ToolbarView,
    pub(super) new_chat_button: gtk::Button,
    pub(super) model_manager_button: gtk::Button,
    pub(super) provider_row: adw::ActionRow,
    pub(super) provider_status: gtk::Label,
    pub(super) provider_switch_button: gtk::Button,
    pub(super) refresh_button: gtk::Button,
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

    let new_chat_button = icon_button("list-add-symbolic", "New Conversation");
    let model_manager_button = icon_button("view-list-symbolic", "Models");
    new_chat_button.add_css_class("moose-sidebar-button");
    model_manager_button.add_css_class("moose-sidebar-button");

    header_bar.append(&title);
    header_bar.append(&model_manager_button);
    header_bar.append(&new_chat_button);

    let provider_group = gtk::ListBox::new();
    provider_group.add_css_class("moose-provider-list");
    provider_group.set_selection_mode(gtk::SelectionMode::None);

    let provider_status = status_label("Checking");
    provider_status.add_css_class("moose-provider-status");
    let provider_switch_button = icon_button("pan-down-symbolic", "Switch Provider");
    provider_switch_button.add_css_class("moose-provider-button");
    let refresh_button = icon_button("view-refresh-symbolic", "Refresh Models");
    refresh_button.add_css_class("moose-provider-button");
    let provider_row = adw::ActionRow::builder()
        .title("Managed Ollama")
        .title_lines(1)
        .subtitle_lines(1)
        .build();
    let provider_icon = gtk::Image::from_icon_name("computer-symbolic");
    provider_icon.add_css_class("moose-provider-icon");
    provider_row.set_tooltip_text(Some(MANAGED_OLLAMA_BASE_URL));
    provider_row.add_css_class("moose-provider-row");
    provider_row.add_prefix(&provider_icon);
    provider_row.add_suffix(&provider_status);
    provider_row.add_suffix(&provider_switch_button);
    provider_row.add_suffix(&refresh_button);
    provider_group.append(&provider_row);

    let search_entry = gtk::SearchEntry::builder()
        .placeholder_text("Search chats")
        .hexpand(true)
        .build();
    search_entry.add_css_class("moose-chat-search");

    let archived_button = gtk::ToggleButton::builder()
        .icon_name("folder-symbolic")
        .tooltip_text("Show Archived Chats")
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
        .label("Chats")
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

    let footer = gtk::Box::builder()
        .orientation(Orientation::Vertical)
        .spacing(0)
        .build();
    footer.add_css_class("moose-sidebar-footer");
    footer.append(&provider_group);

    root.add_top_bar(&header_bar);
    root.set_content(Some(&content));
    root.add_bottom_bar(&footer);

    Sidebar {
        root,
        new_chat_button,
        model_manager_button,
        provider_row,
        provider_status,
        provider_switch_button,
        refresh_button,
        search_entry,
        archived_button,
        conversation_list,
    }
}
