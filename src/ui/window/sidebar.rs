use adw::prelude::*;
use gtk::{Align, Orientation, PolicyType};

use crate::APPLICATION_NAME;
use crate::providers::DEFAULT_OLLAMA_BASE_URL;

use super::widgets::{icon_button, section_label, status_label};

pub(super) struct Sidebar {
    pub(super) root: gtk::Box,
    pub(super) new_chat_button: gtk::Button,
    pub(super) search_button: gtk::Button,
    pub(super) model_manager_button: gtk::Button,
    pub(super) provider_row: adw::ActionRow,
    pub(super) provider_status: gtk::Label,
    pub(super) refresh_button: gtk::Button,
    pub(super) conversation_list: gtk::ListBox,
}

pub(super) fn build() -> Sidebar {
    let root = gtk::Box::builder()
        .orientation(Orientation::Vertical)
        .spacing(0)
        .vexpand(true)
        .build();
    root.add_css_class("moose-sidebar");

    let top_bar = gtk::Box::builder()
        .orientation(Orientation::Horizontal)
        .spacing(4)
        .valign(Align::Start)
        .build();
    top_bar.add_css_class("moose-sidebar-topbar");

    let title = gtk::Label::builder()
        .label(APPLICATION_NAME)
        .halign(Align::Start)
        .valign(Align::Center)
        .hexpand(true)
        .xalign(0.0)
        .build();
    title.add_css_class("moose-sidebar-title");

    let new_chat_button = icon_button("list-add-symbolic", "New Conversation");
    let search_button = icon_button("system-search-symbolic", "Search Conversations");
    let model_manager_button = icon_button("view-list-symbolic", "Models");

    new_chat_button.add_css_class("moose-sidebar-button");
    search_button.add_css_class("moose-sidebar-button");
    model_manager_button.add_css_class("moose-sidebar-button");

    top_bar.append(&title);
    top_bar.append(&model_manager_button);
    top_bar.append(&new_chat_button);
    top_bar.append(&search_button);

    let provider_group = gtk::ListBox::new();
    provider_group.add_css_class("moose-provider-list");
    provider_group.set_selection_mode(gtk::SelectionMode::None);

    let provider_status = status_label("Checking");
    provider_status.add_css_class("moose-provider-status");
    let refresh_button = icon_button("view-refresh-symbolic", "Refresh Models");
    refresh_button.add_css_class("moose-provider-button");
    let provider_row = adw::ActionRow::builder().title("Local Ollama").build();
    provider_row.set_tooltip_text(Some(DEFAULT_OLLAMA_BASE_URL));
    provider_row.add_css_class("moose-provider-row");
    provider_row.add_suffix(&provider_status);
    provider_row.add_suffix(&refresh_button);
    provider_group.append(&provider_row);

    let conversation_list = gtk::ListBox::new();
    conversation_list.add_css_class("moose-conversation-list");
    conversation_list.set_selection_mode(gtk::SelectionMode::Single);

    let conversation_label = section_label("Chats");
    conversation_label.add_css_class("moose-sidebar-section");

    let scrolled = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(PolicyType::Never)
        .vscrollbar_policy(PolicyType::Automatic)
        .hexpand(true)
        .vexpand(true)
        .child(&conversation_list)
        .build();
    scrolled.add_css_class("moose-sidebar-scroll");

    let footer = gtk::Box::builder()
        .orientation(Orientation::Vertical)
        .spacing(0)
        .build();
    footer.add_css_class("moose-sidebar-footer");
    footer.append(&provider_group);

    root.append(&top_bar);
    root.append(&conversation_label);
    root.append(&scrolled);
    root.append(&footer);

    Sidebar {
        root,
        new_chat_button,
        search_button,
        model_manager_button,
        provider_row,
        provider_status,
        refresh_button,
        conversation_list,
    }
}
