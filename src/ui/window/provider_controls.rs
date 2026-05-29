use std::rc::Rc;

use adw::prelude::*;
use gtk::{Align, Orientation, pango};

use crate::providers::{
    DEFAULT_OLLAMA_BASE_URL, MANAGED_OLLAMA_DEFAULT_PORT, NewProvider, Provider, ProviderKind,
    managed_ollama_port_from_base_url, managed_ollama_port_is_available,
};

use super::{
    Backend, WindowUi, active_provider, apply_active_provider, clear_active_provider,
    managed_install, provider_change_is_blocked, show_chat, show_error, widgets,
};

pub(super) fn show_switcher(anchor: &gtk::Button, ui: &Rc<WindowUi>, backend: &Rc<Backend>) {
    let providers = match backend.repository.list() {
        Ok(providers) => providers,
        Err(error) => {
            ui.toast_overlay.add_toast(adw::Toast::new(&format!(
                "Providers could not be loaded: {error}"
            )));
            return;
        }
    };

    let popover = gtk::Popover::builder()
        .autohide(true)
        .has_arrow(false)
        .build();
    popover.add_css_class("moose-provider-popover");
    popover.set_parent(anchor);

    let content = gtk::Box::builder()
        .orientation(Orientation::Vertical)
        .spacing(0)
        .build();
    content.add_css_class("moose-provider-popover-content");
    content.set_size_request(300, -1);

    let list = gtk::ListBox::new();
    list.set_selection_mode(gtk::SelectionMode::None);
    list.set_activate_on_single_click(true);
    list.add_css_class("moose-provider-list");
    list.add_css_class("moose-provider-switch-list");

    let active_provider_id = active_provider(backend).map(|provider| provider.id);
    let mut provider_ids = Vec::new();
    for provider in providers {
        let row = provider_switch_row(
            &provider,
            active_provider_id
                .as_deref()
                .is_some_and(|id| id == provider.id.as_str()),
        );
        provider_ids.push(provider.id.clone());

        let delete_button =
            widgets::icon_button("user-trash-symbolic", &format!("Delete {}", provider.name));
        delete_button.add_css_class("destructive-action");
        delete_button.add_css_class("moose-provider-delete-button");
        let provider_id = provider.id.clone();
        let target_ui = Rc::clone(ui);
        let target_backend = Rc::clone(backend);
        let target_popover = popover.clone();
        delete_button.connect_clicked(move |_| {
            target_popover.popdown();
            confirm_provider_delete(&target_ui, &target_backend, &provider_id);
        });
        if let Some(content) = row
            .child()
            .and_then(|child| child.downcast::<gtk::Box>().ok())
        {
            content.append(&delete_button);
        }
        list.append(&row);
    }

    let add_row = provider_add_row();
    list.append(&add_row);

    let provider_ids = Rc::new(provider_ids);
    let target_ui = Rc::clone(ui);
    let target_backend = Rc::clone(backend);
    let target_popover = popover.clone();
    list.connect_row_activated(move |_, row| {
        let Ok(index) = usize::try_from(row.index()) else {
            return;
        };

        target_popover.popdown();
        if let Some(provider_id) = provider_ids.get(index) {
            activate_provider(&target_ui, &target_backend, provider_id);
        } else if index == provider_ids.len() {
            show_add_provider_dialog(&target_ui, &target_backend);
        }
    });

    content.append(&list);
    popover.set_child(Some(&content));
    popover.popup();
}

fn provider_switch_row(provider: &Provider, is_active: bool) -> gtk::ListBoxRow {
    let row = gtk::ListBoxRow::new();
    row.add_css_class("moose-provider-switch-row");
    row.set_tooltip_text(Some(&provider.base_url));

    let content = gtk::Box::builder()
        .orientation(Orientation::Horizontal)
        .spacing(8)
        .margin_top(7)
        .margin_bottom(7)
        .margin_start(9)
        .margin_end(6)
        .build();

    let labels = gtk::Box::builder()
        .orientation(Orientation::Vertical)
        .spacing(1)
        .hexpand(true)
        .valign(Align::Center)
        .build();

    let title = gtk::Label::builder()
        .label(&provider.name)
        .halign(Align::Start)
        .hexpand(true)
        .xalign(0.0)
        .build();
    title.set_ellipsize(pango::EllipsizeMode::End);
    title.add_css_class("moose-provider-switch-title");
    labels.append(&title);

    let subtitle = gtk::Label::builder()
        .label(&provider.base_url)
        .halign(Align::Start)
        .hexpand(true)
        .xalign(0.0)
        .build();
    subtitle.set_ellipsize(pango::EllipsizeMode::End);
    subtitle.add_css_class("dim-label");
    subtitle.add_css_class("moose-provider-switch-subtitle");
    labels.append(&subtitle);

    content.append(&labels);

    if is_active {
        let active_icon = gtk::Image::from_icon_name("object-select-symbolic");
        active_icon.set_tooltip_text(Some("Active Provider"));
        active_icon.add_css_class("moose-provider-active-icon");
        content.append(&active_icon);
    }

    row.set_child(Some(&content));
    row
}

fn provider_add_row() -> gtk::ListBoxRow {
    let row = gtk::ListBoxRow::new();
    row.add_css_class("moose-provider-add-row");
    row.set_tooltip_text(Some("Add Provider"));

    let content = gtk::Box::builder()
        .orientation(Orientation::Horizontal)
        .spacing(8)
        .margin_top(8)
        .margin_bottom(8)
        .margin_start(9)
        .margin_end(9)
        .valign(Align::Center)
        .build();

    let icon = gtk::Image::from_icon_name("list-add-symbolic");
    icon.add_css_class("moose-provider-add-icon");

    let label = gtk::Label::builder()
        .label("Add Provider")
        .halign(Align::Start)
        .hexpand(true)
        .xalign(0.0)
        .build();
    label.add_css_class("moose-provider-add-label");

    content.append(&icon);
    content.append(&label);
    row.set_child(Some(&content));
    row
}

fn show_add_provider_dialog(ui: &Rc<WindowUi>, backend: &Rc<Backend>) {
    if provider_change_is_blocked(ui, backend) {
        return;
    }

    let type_toggle = adw::ToggleGroup::builder()
        .active(0)
        .homogeneous(true)
        .build();
    type_toggle.add(
        adw::Toggle::builder()
            .name("managed")
            .label("Managed")
            .build(),
    );
    type_toggle.add(
        adw::Toggle::builder()
            .name("external")
            .label("External")
            .build(),
    );

    let connection_row = adw::ExpanderRow::builder()
        .title("Connection Type")
        .subtitle("Managed by Moose")
        .expanded(true)
        .build();
    connection_row.add_suffix(&type_toggle);

    let managed_row = adw::ActionRow::builder()
        .title("Managed by Moose")
        .subtitle("Moose installs Ollama if needed, then lets you choose the managed port.")
        .build();

    let external_name_row = adw::EntryRow::builder()
        .title("Name")
        .text(&next_provider_name(backend))
        .build();
    let external_url_row = adw::EntryRow::builder()
        .title("Base URL")
        .text(DEFAULT_OLLAMA_BASE_URL)
        .build();

    external_name_row.set_visible(false);
    external_url_row.set_visible(false);
    connection_row.add_row(&managed_row);
    connection_row.add_row(&external_name_row);
    connection_row.add_row(&external_url_row);

    let connection_group = adw::PreferencesGroup::builder()
        .description(
            "Choose whether Moose should manage Ollama or connect to an existing endpoint.",
        )
        .build();
    connection_group.add(&connection_row);

    let primary_button = gtk::Button::with_label("Continue");
    primary_button.add_css_class("suggested-action");
    let actions = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .halign(gtk::Align::End)
        .build();
    actions.append(&primary_button);

    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(14)
        .build();
    content.set_size_request(420, -1);
    content.append(&connection_group);
    content.append(&actions);

    let dialog = adw::AlertDialog::builder()
        .heading("Add Ollama Instance")
        .extra_child(&content)
        .close_response("cancel")
        .default_response("cancel")
        .build();
    dialog.add_response("cancel", "Cancel");

    let target_connection_row = connection_row.clone();
    let target_managed_row = managed_row.clone();
    let target_name_row = external_name_row.clone();
    let target_url_row = external_url_row.clone();
    let target_button = primary_button.clone();
    type_toggle.connect_active_notify(move |toggle| {
        if toggle.active() == 0 {
            target_connection_row.set_subtitle("Managed by Moose");
            target_managed_row.set_visible(true);
            target_name_row.set_visible(false);
            target_url_row.set_visible(false);
            target_button.set_label("Continue");
        } else {
            target_connection_row.set_subtitle("External Ollama");
            target_managed_row.set_visible(false);
            target_name_row.set_visible(true);
            target_url_row.set_visible(true);
            target_button.set_label("Connect URL");
        }
    });

    let target_ui = Rc::clone(ui);
    let target_backend = Rc::clone(backend);
    let target_dialog = dialog.clone();
    primary_button.connect_clicked(move |_| {
        if type_toggle.active() == 0 {
            target_dialog.close();
            managed_install::show_dialog(&target_ui, &target_backend);
        } else if add_provider_from_sidebar(
            &target_ui,
            &target_backend,
            external_name_row.text().to_string(),
            external_url_row.text().to_string(),
        ) {
            target_dialog.close();
        }
    });

    dialog.present(Some(&ui.window));
}

pub(super) fn show_connect_external_dialog(ui: &Rc<WindowUi>, backend: &Rc<Backend>) {
    let name_row = adw::EntryRow::builder()
        .title("Name")
        .text(&next_provider_name(backend))
        .build();
    let url_row = adw::EntryRow::builder()
        .title("Base URL")
        .text(DEFAULT_OLLAMA_BASE_URL)
        .build();
    let group = adw::PreferencesGroup::new();
    group.add(&name_row);
    group.add(&url_row);

    let dialog = adw::AlertDialog::builder()
        .heading("Connect External Instance")
        .body("Enter the URL for an Ollama instance.")
        .extra_child(&group)
        .close_response("cancel")
        .default_response("connect")
        .build();
    dialog.add_response("cancel", "Cancel");
    dialog.add_response("connect", "Connect");
    dialog.set_response_appearance("connect", adw::ResponseAppearance::Suggested);

    let target_ui = Rc::clone(ui);
    let target_backend = Rc::clone(backend);
    dialog.connect_response(Some("connect"), move |_, _| {
        let _ = add_provider_from_sidebar(
            &target_ui,
            &target_backend,
            name_row.text().to_string(),
            url_row.text().to_string(),
        );
    });

    dialog.present(Some(&ui.window));
}

fn confirm_provider_delete(ui: &Rc<WindowUi>, backend: &Rc<Backend>, provider_id: &str) {
    if provider_change_is_blocked(ui, backend) {
        return;
    }

    let provider = match backend.repository.get(provider_id) {
        Ok(Some(provider)) => provider,
        Ok(None) => {
            ui.toast_overlay
                .add_toast(adw::Toast::new("Provider was not found"));
            return;
        }
        Err(error) => {
            ui.toast_overlay.add_toast(adw::Toast::new(&format!(
                "Provider could not be loaded: {error}"
            )));
            return;
        }
    };

    let provider_id = provider.id.clone();
    let dialog = adw::AlertDialog::builder()
        .heading("Delete Provider?")
        .body(format!(
            "Delete \"{}\" at {}?",
            provider.name.as_str(),
            provider.base_url.as_str()
        ))
        .close_response("cancel")
        .default_response("cancel")
        .build();
    dialog.add_response("cancel", "Cancel");
    dialog.add_response("delete", "Delete");
    dialog.set_response_appearance("delete", adw::ResponseAppearance::Destructive);

    let target_ui = Rc::clone(ui);
    let target_backend = Rc::clone(backend);
    dialog.connect_response(Some("delete"), move |_, _| {
        delete_provider_from_sidebar(&target_ui, &target_backend, &provider_id);
    });
    dialog.present(Some(&ui.window));
}

fn next_provider_name(backend: &Backend) -> String {
    backend
        .repository
        .list()
        .map(|providers| format!("Ollama Provider {}", providers.len() + 1))
        .unwrap_or_else(|_| "Ollama Provider".to_string())
}

pub(super) fn next_managed_provider_name(backend: &Backend) -> String {
    let managed_count = backend
        .repository
        .list()
        .map(|providers| {
            providers
                .into_iter()
                .filter(|provider| provider.is_managed)
                .count()
        })
        .unwrap_or_default();

    if managed_count == 0 {
        "Managed Ollama".to_string()
    } else {
        format!("Managed Ollama {}", managed_count + 1)
    }
}

pub(super) fn next_managed_port(backend: &Backend) -> u16 {
    let used_ports = backend
        .repository
        .list()
        .map(|providers| {
            providers
                .into_iter()
                .filter(|provider| provider.is_managed)
                .filter_map(|provider| managed_ollama_port_from_base_url(&provider.base_url).ok())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let mut port = MANAGED_OLLAMA_DEFAULT_PORT;
    while (used_ports.contains(&port) || !managed_port_is_available(port)) && port < u16::MAX {
        port += 1;
    }
    port
}

fn managed_port_is_available(port: u16) -> bool {
    managed_ollama_port_is_available(port).unwrap_or(false)
}

fn add_provider_from_sidebar(
    ui: &Rc<WindowUi>,
    backend: &Rc<Backend>,
    name: String,
    base_url: String,
) -> bool {
    if provider_change_is_blocked(ui, backend) {
        return false;
    }

    match backend.repository.create(NewProvider {
        kind: ProviderKind::Ollama,
        name,
        base_url,
        is_managed: false,
        is_default: true,
    }) {
        Ok(provider) => {
            apply_active_provider(ui, backend, provider);
            show_chat(ui);
            ui.toast_overlay
                .add_toast(adw::Toast::new("Provider added"));
            true
        }
        Err(error) => {
            show_error(&ui.window, "Provider could not be added", &error);
            false
        }
    }
}

fn delete_provider_from_sidebar(ui: &Rc<WindowUi>, backend: &Rc<Backend>, provider_id: &str) {
    if provider_change_is_blocked(ui, backend) {
        return;
    }

    let was_active = active_provider(backend)
        .as_ref()
        .is_some_and(|provider| provider.id.as_str() == provider_id);
    match backend.repository.delete(provider_id) {
        Ok(()) => {
            if was_active {
                match backend.repository.ensure_default_provider() {
                    Ok(Some(provider)) => apply_active_provider(ui, backend, provider),
                    Ok(None) => clear_active_provider(ui, backend),
                    Err(error) => {
                        show_error(&ui.window, "Provider could not be selected", &error);
                        return;
                    }
                }
            }
            ui.toast_overlay
                .add_toast(adw::Toast::new("Provider deleted"));
        }
        Err(error) => show_error(&ui.window, "Provider could not be deleted", &error),
    }
}

fn activate_provider(ui: &Rc<WindowUi>, backend: &Rc<Backend>, provider_id: &str) {
    if active_provider(backend)
        .as_ref()
        .is_some_and(|provider| provider.id.as_str() == provider_id)
    {
        return;
    }

    if provider_change_is_blocked(ui, backend) {
        return;
    }

    let provider = match backend.repository.get(provider_id) {
        Ok(Some(provider)) => provider,
        Ok(None) => {
            ui.toast_overlay
                .add_toast(adw::Toast::new("Provider was not found"));
            return;
        }
        Err(error) => {
            ui.toast_overlay.add_toast(adw::Toast::new(&format!(
                "Provider could not be loaded: {error}"
            )));
            return;
        }
    };

    match backend
        .repository
        .set_default(provider_id)
        .and_then(|_| backend.repository.get(provider_id))
    {
        Ok(Some(provider)) => {
            apply_active_provider(ui, backend, provider);
            ui.toast_overlay
                .add_toast(adw::Toast::new("Provider switched"));
        }
        Ok(None) => {
            apply_active_provider(ui, backend, provider);
            ui.toast_overlay
                .add_toast(adw::Toast::new("Provider switched"));
        }
        Err(error) => show_error(&ui.window, "Provider could not be switched", &error),
    }
}
