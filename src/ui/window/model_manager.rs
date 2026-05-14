use adw::prelude::*;
use gtk::{Align, Orientation, PolicyType};

use crate::{APPLICATION_ID, ollama::OllamaModel};

use super::widgets::{icon_button, section_label};

#[derive(Clone)]
pub(super) struct ModelManager {
    pub(super) root: gtk::Box,
    pub(super) refresh_button: gtk::Button,
    pub(super) search_entry: gtk::SearchEntry,
    model_list: gtk::ListBox,
    status_page: adw::StatusPage,
    stack: gtk::Stack,
}

pub(super) fn build() -> ModelManager {
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

    ModelManager {
        root,
        refresh_button,
        search_entry,
        model_list,
        status_page,
        stack,
    }
}

pub(super) fn set_loading(manager: &ModelManager) {
    clear_models(manager);
    manager.refresh_button.set_sensitive(false);
    manager.search_entry.set_sensitive(false);
    manager.status_page.set_title("Loading Models");
    manager.status_page.set_description(None);
    manager.stack.set_visible_child_name("empty");
}

pub(super) fn set_unavailable(manager: &ModelManager, title: &str, description: &str) {
    clear_models(manager);
    manager.refresh_button.set_sensitive(true);
    manager.search_entry.set_sensitive(false);
    manager.status_page.set_title(title);
    manager.status_page.set_description(Some(description));
    manager.stack.set_visible_child_name("empty");
}

pub(super) fn set_models(manager: &ModelManager, models: &[OllamaModel], query: &str) {
    clear_models(manager);
    manager.refresh_button.set_sensitive(true);
    manager.search_entry.set_sensitive(!models.is_empty());

    if models.is_empty() {
        manager.status_page.set_title("No Local Models");
        manager
            .status_page
            .set_description(Some("Ollama did not report any installed models."));
        manager.stack.set_visible_child_name("empty");
        return;
    }

    let query = query.trim().to_ascii_lowercase();
    let filtered_models = models
        .iter()
        .filter(|model| model_matches_query(model, &query))
        .collect::<Vec<_>>();

    if filtered_models.is_empty() {
        manager.status_page.set_title("No Matching Models");
        manager
            .status_page
            .set_description(Some("No installed models match the current search."));
        manager.stack.set_visible_child_name("empty");
        return;
    }

    for model in filtered_models {
        manager.model_list.append(&model_row(model));
    }

    manager.stack.set_visible_child_name("models");
}

fn clear_models(manager: &ModelManager) {
    while let Some(child) = manager.model_list.first_child() {
        manager.model_list.remove(&child);
    }
}

fn model_row(model: &OllamaModel) -> adw::ActionRow {
    let row = adw::ActionRow::builder()
        .title(&model.name)
        .subtitle(&model_subtitle(model))
        .subtitle_lines(2)
        .build();
    row.add_css_class("moose-model-row-item");
    row.set_tooltip_text(Some(&model.name));

    let meta = gtk::Box::builder()
        .orientation(Orientation::Horizontal)
        .spacing(6)
        .halign(Align::End)
        .valign(Align::Center)
        .build();

    let capability = if model.supports_chat { "Chat" } else { "Other" };
    meta.append(&pill_label(capability));
    meta.append(&pill_label(&format_size(model.size_bytes)));
    row.add_suffix(&meta);
    row
}

fn pill_label(text: &str) -> gtk::Label {
    let label = gtk::Label::builder()
        .label(text)
        .halign(Align::Center)
        .valign(Align::Center)
        .build();
    label.add_css_class("moose-model-pill");
    label
}

fn model_subtitle(model: &OllamaModel) -> String {
    let mut parts = Vec::new();

    if let Some(family) = model.family.as_deref().filter(|value| !value.is_empty()) {
        parts.push(family.to_string());
    }

    if let Some(parameter_size) = model
        .parameter_size
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        parts.push(parameter_size.to_string());
    }

    if let Some(quantization_level) = model
        .quantization_level
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        parts.push(quantization_level.to_string());
    }

    if let Some(modified_at) = model
        .modified_at
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        parts.push(format!("Modified {}", compact_timestamp(modified_at)));
    }

    if parts.is_empty() {
        "Installed locally".to_string()
    } else {
        parts.join(" - ")
    }
}

fn compact_timestamp(value: &str) -> String {
    value
        .split_once('T')
        .map(|(date, _)| date)
        .unwrap_or(value)
        .to_string()
}

fn format_size(size_bytes: Option<u64>) -> String {
    let Some(size_bytes) = size_bytes else {
        return "Unknown size".to_string();
    };

    let units = ["B", "KB", "MB", "GB", "TB"];
    let mut size = size_bytes as f64;
    let mut unit_index = 0;

    while size >= 1024.0 && unit_index + 1 < units.len() {
        size /= 1024.0;
        unit_index += 1;
    }

    if unit_index == 0 {
        format!("{size_bytes} {}", units[unit_index])
    } else if size >= 10.0 {
        format!("{size:.0} {}", units[unit_index])
    } else {
        format!("{size:.1} {}", units[unit_index])
    }
}

fn model_matches_query(model: &OllamaModel, query: &str) -> bool {
    if query.is_empty() {
        return true;
    }

    contains_query(&model.name, query)
        || model
            .family
            .as_deref()
            .is_some_and(|family| contains_query(family, query))
        || model
            .families
            .iter()
            .any(|family| contains_query(family, query))
        || model
            .parameter_size
            .as_deref()
            .is_some_and(|parameter_size| contains_query(parameter_size, query))
        || model
            .quantization_level
            .as_deref()
            .is_some_and(|quantization_level| contains_query(quantization_level, query))
}

fn contains_query(value: &str, query: &str) -> bool {
    value.to_ascii_lowercase().contains(query)
}
