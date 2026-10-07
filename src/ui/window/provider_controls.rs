use std::rc::Rc;

use adw::prelude::*;
use gtk::{Align, Orientation, pango};

use crate::providers::{
    DEFAULT_OLLAMA_BASE_URL, MANAGED_OLLAMA_DEFAULT_PORT, NewProvider, Provider, ProviderKind,
    managed_ollama_port_from_base_url, managed_ollama_port_is_available, policy::RemotePermissions,
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
        .label(format!(
            "{} · {}",
            provider.kind.label(),
            provider.location_label()
        ))
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
    show_provider_setup(ui, backend, false);
}

pub(super) fn show_connect_external_dialog(ui: &Rc<WindowUi>, backend: &Rc<Backend>) {
    show_provider_setup(ui, backend, true);
}

fn show_provider_setup(ui: &Rc<WindowUi>, backend: &Rc<Backend>, external_only: bool) {
    if provider_change_is_blocked(ui, backend) {
        return;
    }

    let dialog = adw::Dialog::builder()
        .title(if external_only {
            "Connect Provider"
        } else {
            "Add Provider"
        })
        .follows_content_size(true)
        .build();
    let header_bar = adw::HeaderBar::builder()
        .show_start_title_buttons(false)
        .show_end_title_buttons(false)
        .build();

    let type_toggle = adw::ToggleGroup::builder().homogeneous(true).build();
    type_toggle.add(
        adw::Toggle::builder()
            .name("managed")
            .label("Set Up for Me")
            .build(),
    );
    type_toggle.add(
        adw::Toggle::builder()
            .name("external")
            .label("Connect Existing")
            .build(),
    );
    type_toggle.add(
        adw::Toggle::builder()
            .name("cloud")
            .label("Cloud Provider")
            .build(),
    );
    type_toggle.set_active(u32::from(external_only));

    let description = gtk::Label::builder()
        .label(if external_only {
            "Connect to an existing Ollama server. This enables external connections; its execution location cannot be verified by Moose."
        } else {
            "Moose installs and runs Ollama on this device."
        })
        .wrap(true)
        .wrap_mode(pango::WrapMode::WordChar)
        .width_chars(1)
        .max_width_chars(48)
        .xalign(0.0)
        .build();
    description.add_css_class("dim-label");

    let name_row = adw::EntryRow::builder()
        .title("Instance Name")
        .text(next_provider_name(backend))
        .build();
    let url_row = adw::EntryRow::builder()
        .title("Ollama URL")
        .text(DEFAULT_OLLAMA_BASE_URL)
        .build();
    url_row.set_input_purpose(gtk::InputPurpose::Url);
    let connection_group = adw::PreferencesGroup::new();
    connection_group.add(&name_row);
    connection_group.add(&url_row);
    connection_group.set_visible(external_only);
    let sharing_group = adw::PreferencesGroup::builder()
        .title("Sharing Permissions")
        .description("Choose whether this server can receive messages in all chats. Files require separate permission. Shared content is processed under the server's own data policy.")
        .visible(external_only)
        .build();
    let message_permission = super::privacy::message_permission_row();
    sharing_group.add(&message_permission);

    let content = gtk::Box::builder()
        .orientation(Orientation::Vertical)
        .spacing(14)
        .build();
    content.add_css_class("moose-instance-content");
    content.append(&type_toggle);
    content.append(&description);
    content.append(&connection_group);
    content.append(&sharing_group);

    let scrolled = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .propagate_natural_height(true)
        .max_content_height(420)
        .child(&content)
        .build();

    let cancel_button = gtk::Button::with_label("Cancel");
    let primary_button =
        gtk::Button::with_label(if external_only { "Connect" } else { "Continue" });
    primary_button.add_css_class("suggested-action");
    let actions = gtk::Box::builder()
        .orientation(Orientation::Horizontal)
        .spacing(10)
        .homogeneous(true)
        .build();
    actions.add_css_class("moose-instance-actions");
    actions.append(&cancel_button);
    actions.append(&primary_button);

    let toolbar_view = adw::ToolbarView::builder()
        .width_request(440)
        .top_bar_style(adw::ToolbarStyle::Flat)
        .bottom_bar_style(adw::ToolbarStyle::Flat)
        .content(&scrolled)
        .build();
    toolbar_view.add_top_bar(&header_bar);
    toolbar_view.add_bottom_bar(&actions);
    dialog.set_child(Some(&toolbar_view));
    dialog.set_default_widget(Some(&primary_button));
    if external_only {
        dialog.set_focus(Some(&url_row));
    } else {
        dialog.set_focus(Some(&type_toggle));
    }

    let target_description = description.clone();
    let target_group = connection_group.clone();
    let target_sharing_group = sharing_group.clone();
    let target_button = primary_button.clone();
    let target_scroll = scrolled.downgrade();
    type_toggle.connect_active_notify(move |toggle| {
        let external = toggle.active() == 1;
        let cloud = toggle.active() == 2;
        target_description.set_label(if cloud {
            "Connect Ollama Cloud, Groq, OpenAI, Claude or Gemini using your own API key."
        } else if external {
            "Connect to an existing Ollama server. This enables external connections; its execution location cannot be verified by Moose."
        } else {
            "Moose installs and runs Ollama on this device."
        });
        target_group.set_visible(external);
        target_sharing_group.set_visible(external);
        target_button.set_label(if external { "Connect" } else { "Continue" });
        if let Some(scrolled) = target_scroll.upgrade() {
            let adjustment = scrolled.vadjustment();
            adjustment.set_value(adjustment.lower());
        }
    });

    let target_url_row = url_row.clone();
    name_row.connect_entry_activated(move |_| {
        target_url_row.grab_focus();
    });
    let target_message_permission = message_permission.clone();
    url_row.connect_text_notify(move |_| {
        target_message_permission.set_selected(0);
    });
    let target_button = primary_button.clone();
    url_row.connect_entry_activated(move |_| {
        target_button.emit_clicked();
    });

    let target_dialog = dialog.clone();
    cancel_button.connect_clicked(move |_| {
        target_dialog.close();
    });

    let target_ui = Rc::clone(ui);
    let target_backend = Rc::clone(backend);
    let target_dialog = dialog.clone();
    primary_button.connect_clicked(move |_| {
        if provider_change_is_blocked(&target_ui, &target_backend) {
            return;
        }
        if type_toggle.active() == 0 {
            target_dialog.close();
            managed_install::show_dialog(&target_ui, &target_backend);
        } else if type_toggle.active() == 2 {
            target_dialog.close();
            super::cloud_setup::show(&target_ui, &target_backend);
        } else if add_provider_from_sidebar(
            &target_ui,
            &target_backend,
            name_row.text().to_string(),
            url_row.text().to_string(),
            RemotePermissions {
                messages: message_permission.selected() == 1,
                files: false,
            },
        ) {
            target_dialog.close();
        }
    });

    dialog.present(Some(&ui.window));
}

pub(super) fn confirm_provider_delete(ui: &Rc<WindowUi>, backend: &Rc<Backend>, provider_id: &str) {
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
    permissions: RemotePermissions,
) -> bool {
    if provider_change_is_blocked(ui, backend) {
        return false;
    }

    match (NewProvider {
        kind: ProviderKind::Ollama,
        name,
        base_url,
        is_managed: false,
        is_default: true,
    })
    .into_provider()
    .and_then(|provider| backend.repository.insert_remote(provider, permissions))
    {
        Ok(provider) => {
            backend.network_policy.set_local_only(false);
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

    if let Ok(Some(provider)) = backend.repository.get(provider_id)
        && provider.kind.requires_key()
    {
        super::cloud_setup::delete_provider(ui, backend, provider);
        return;
    }
    delete_provider_record(ui, backend, provider_id);
}

pub(super) fn delete_provider_record(ui: &Rc<WindowUi>, backend: &Rc<Backend>, provider_id: &str) {
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
