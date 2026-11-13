use crate::i18n::{gettext, gettext_f, gettext_noop};

use std::rc::Rc;

use adw::prelude::*;
use gtk::{Align, Orientation, pango};

use crate::providers::{
    DEFAULT_OLLAMA_BASE_URL, MANAGED_OLLAMA_DEFAULT_PORT, NewProvider, Provider, ProviderKind,
    managed_ollama_port_from_base_url, managed_ollama_port_is_available, policy::RemotePermissions,
};

use super::{
    Backend, WindowUi, active_provider, apply_active_provider, clear_active_provider,
    managed_install, provider_change_is_blocked, refresh_models, show_chat, show_error, widgets,
};

pub(super) struct Switcher {
    pub(super) button: gtk::MenuButton,
    pub(super) refresh_button: gtk::Button,
    title: gtk::Label,
    status: gtk::Label,
    icon: gtk::Image,
    popover: gtk::Popover,
    list: gtk::ListBox,
    placeholder: gtk::Label,
    add_button: gtk::Button,
}

impl Switcher {
    pub(super) fn new() -> Self {
        let title = gtk::Label::builder()
            .label(gettext("Select Provider"))
            .xalign(0.0)
            .ellipsize(pango::EllipsizeMode::End)
            .single_line_mode(true)
            .hexpand(true)
            .css_classes(["moose-provider-title"])
            .build();
        let status = gtk::Label::builder()
            .label(gettext("Not Set"))
            .xalign(0.0)
            .ellipsize(pango::EllipsizeMode::End)
            .single_line_mode(true)
            .hexpand(true)
            .css_classes(["caption", "dim-label"])
            .build();
        let labels = gtk::Box::builder()
            .orientation(Orientation::Vertical)
            .spacing(2)
            .hexpand(true)
            .valign(Align::Center)
            .build();
        labels.append(&title);
        labels.append(&status);
        let icon = gtk::Image::from_icon_name("network-server-symbolic");
        icon.add_css_class("dim-label");
        let button_content = gtk::Box::new(Orientation::Horizontal, 8);
        button_content.append(&icon);
        button_content.append(&labels);

        let button = gtk::MenuButton::builder()
            .child(&button_content)
            .direction(gtk::ArrowType::Up)
            .always_show_arrow(true)
            .can_shrink(true)
            .hexpand(true)
            .tooltip_text(gettext("Switch Provider"))
            .css_classes(["flat"])
            .build();
        button.update_property(&[gtk::accessible::Property::Label(&gettext(
            "Switch Provider",
        ))]);

        let heading = gtk::Label::builder()
            .label(gettext("Providers"))
            .xalign(0.0)
            .hexpand(true)
            .css_classes(["heading"])
            .build();
        let refresh_button =
            widgets::icon_button("view-refresh-symbolic", &gettext("Refresh Models"));
        let header = gtk::Box::builder()
            .orientation(Orientation::Horizontal)
            .spacing(12)
            .margin_start(6)
            .build();
        header.append(&heading);
        header.append(&refresh_button);

        let placeholder = gtk::Label::builder()
            .label(gettext("No providers added"))
            .wrap(true)
            .margin_top(24)
            .margin_bottom(24)
            .margin_start(12)
            .margin_end(12)
            .css_classes(["dim-label"])
            .build();
        let list = gtk::ListBox::builder()
            .selection_mode(gtk::SelectionMode::None)
            .activate_on_single_click(true)
            .css_classes(["boxed-list"])
            .build();
        list.set_placeholder(Some(&placeholder));
        let scrolled = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vscrollbar_policy(gtk::PolicyType::Automatic)
            .max_content_height(360)
            .propagate_natural_height(true)
            .child(&list)
            .build();

        let add_button = gtk::Button::builder()
            .child(
                &adw::ButtonContent::builder()
                    .icon_name("list-add-symbolic")
                    .label(gettext("Add Provider…"))
                    .halign(Align::Start)
                    .build(),
            )
            .css_classes(["flat"])
            .build();
        let content = gtk::Box::builder()
            .orientation(Orientation::Vertical)
            .spacing(12)
            .width_request(360)
            .margin_top(6)
            .margin_bottom(6)
            .margin_start(6)
            .margin_end(6)
            .build();
        content.append(&header);
        content.append(&scrolled);
        content.append(&gtk::Separator::new(Orientation::Horizontal));
        content.append(&add_button);

        let popover = gtk::Popover::builder()
            .halign(Align::Start)
            .child(&content)
            .build();
        popover.set_offset(6, 0);
        button.set_popover(Some(&popover));

        Self {
            button,
            refresh_button,
            title,
            status,
            icon,
            popover,
            list,
            placeholder,
            add_button,
        }
    }

    pub(super) fn set_status(&self, status: &str) {
        self.status.set_label(status);
        self.button
            .update_property(&[gtk::accessible::Property::Description(status)]);
    }

    pub(super) fn set_provider(&self, provider: Option<&Provider>) {
        if let Some(provider) = provider {
            self.title.set_label(&provider.name);
            self.icon.set_icon_name(Some(provider_icon_name(provider)));
            self.button.set_tooltip_text(Some(&gettext_f(
                "Switch Provider\n{arg1}\n{arg2}\n{arg3}",
                &[
                    ("arg1", &(provider.name).to_string()),
                    ("arg2", &gettext(provider.location_label())),
                    ("arg3", &(provider.base_url).to_string()),
                ],
            )));
            self.button
                .update_property(&[gtk::accessible::Property::Label(&gettext_f(
                    "Switch Provider: {arg1}",
                    &[("arg1", &(provider.name).to_string())],
                ))]);
        } else {
            self.title.set_label(&gettext("Select Provider"));
            self.icon.set_icon_name(Some("network-server-symbolic"));
            self.button
                .set_tooltip_text(Some(&gettext("Add or Select a Provider")));
            self.button
                .update_property(&[gtk::accessible::Property::Label(&gettext(
                    "Select Provider",
                ))]);
        }
    }
}

pub(super) fn bind_switcher(ui: &Rc<WindowUi>, backend: &Rc<Backend>) {
    let target_ui = Rc::downgrade(ui);
    let target_backend = Rc::clone(backend);
    ui.provider_switcher.popover.connect_show(move |_| {
        if let Some(ui) = target_ui.upgrade() {
            populate_switcher(&ui, &target_backend);
        }
    });

    let target_ui = Rc::downgrade(ui);
    let target_backend = Rc::clone(backend);
    ui.provider_switcher.add_button.connect_clicked(move |_| {
        if let Some(ui) = target_ui.upgrade() {
            ui.provider_switcher.button.popdown();
            show_add_provider_dialog(&ui, &target_backend);
        }
    });

    let target_ui = Rc::downgrade(ui);
    let target_backend = Rc::clone(backend);
    ui.provider_switcher
        .refresh_button
        .connect_clicked(move |_| {
            if let Some(ui) = target_ui.upgrade() {
                ui.provider_switcher.button.popdown();
                refresh_models(&ui, &target_backend);
            }
        });
}

fn populate_switcher(ui: &Rc<WindowUi>, backend: &Rc<Backend>) {
    let switcher = &ui.provider_switcher;
    switcher.list.remove_all();
    switcher
        .placeholder
        .set_label(&gettext("No providers added"));

    let providers = match backend.repository.list() {
        Ok(providers) => providers,
        Err(error) => {
            switcher
                .placeholder
                .set_label(&gettext("Providers could not be loaded"));
            ui.toast_overlay.add_toast(adw::Toast::new(&gettext_f(
                "Providers could not be loaded: {error}",
                &[("error", &(error).to_string())],
            )));
            return;
        }
    };

    let active_provider_id = active_provider(backend).map(|provider| provider.id);
    let mut group: Option<gtk::CheckButton> = None;
    for provider in providers {
        let row = provider_switch_row(&provider);
        let check = gtk::CheckButton::builder().valign(Align::Center).build();
        check.set_group(group.as_ref());
        check.set_active(active_provider_id.as_deref() == Some(provider.id.as_str()));
        check.update_property(&[gtk::accessible::Property::Label(&provider.name)]);
        if group.is_none() {
            group = Some(check.clone());
        }
        row.add_suffix(&check);
        row.set_activatable_widget(Some(&check));

        let provider_id = provider.id.clone();
        let target_ui = Rc::downgrade(ui);
        let target_backend = Rc::clone(backend);
        check.connect_toggled(move |check| {
            if check.is_active()
                && let Some(ui) = target_ui.upgrade()
            {
                ui.provider_switcher.button.popdown();
                activate_provider(&ui, &target_backend, &provider_id);
            }
        });

        let target_popover = switcher.popover.downgrade();
        check.connect_activate(move |_| {
            if let Some(popover) = target_popover.upgrade() {
                popover.popdown();
            }
        });

        let delete_button = widgets::icon_button(
            "user-trash-symbolic",
            &gettext_f("Delete {arg1}…", &[("arg1", &(provider.name).to_string())]),
        );
        delete_button.set_valign(Align::Center);
        let provider_id = provider.id.clone();
        let target_ui = Rc::downgrade(ui);
        let target_backend = Rc::clone(backend);
        delete_button.connect_clicked(move |_| {
            if let Some(ui) = target_ui.upgrade() {
                ui.provider_switcher.button.popdown();
                confirm_provider_delete(&ui, &target_backend, &provider_id);
            }
        });
        row.add_suffix(&delete_button);
        switcher.list.append(&row);
    }
}

fn provider_switch_row(provider: &Provider) -> adw::ActionRow {
    let subtitle = if provider.kind == ProviderKind::Ollama {
        gettext(provider.location_label())
    } else {
        format!(
            "{} · {}",
            provider.kind.label(),
            gettext(provider.location_label())
        )
    };
    let row = adw::ActionRow::builder()
        .title(&provider.name)
        .subtitle(subtitle)
        .title_lines(1)
        .subtitle_lines(2)
        .use_markup(false)
        .build();
    row.set_tooltip_text(Some(&format!(
        "{}\n{}\n{}",
        provider.name,
        gettext(provider.location_label()),
        provider.base_url
    )));

    let icon = gtk::Image::from_icon_name(provider_icon_name(provider));
    icon.add_css_class("dim-label");
    row.add_prefix(&icon);
    row
}

fn provider_icon_name(provider: &Provider) -> &'static str {
    if provider.is_managed {
        "computer-symbolic"
    } else {
        "network-server-symbolic"
    }
}

pub(super) fn show_add_provider_dialog(ui: &Rc<WindowUi>, backend: &Rc<Backend>) {
    show_provider_setup(ui, backend);
}

fn show_provider_setup(ui: &Rc<WindowUi>, backend: &Rc<Backend>) {
    if provider_change_is_blocked(ui, backend) {
        return;
    }

    let dialog = adw::Dialog::builder()
        .title(gettext("Add Provider"))
        .content_width(500)
        .follows_content_size(true)
        .build();
    let content = gtk::Box::builder()
        .orientation(Orientation::Vertical)
        .spacing(16)
        .margin_top(8)
        .margin_bottom(20)
        .margin_start(20)
        .margin_end(20)
        .build();

    let type_toggle = adw::ToggleGroup::builder().homogeneous(true).build();
    for (name, label) in [
        ("managed", gettext_noop("Set Up for Me")),
        ("external", gettext_noop("Connect Existing")),
        ("cloud", gettext_noop("Cloud Provider")),
    ] {
        type_toggle.add(
            adw::Toggle::builder()
                .name(name)
                .label(gettext(label))
                .build(),
        );
    }
    type_toggle.set_active(0);
    content.append(&type_toggle);

    let description = gtk::Label::builder()
        .label(gettext("Moose installs and runs Ollama on this device."))
        .wrap(true)
        .wrap_mode(pango::WrapMode::WordChar)
        .width_chars(1)
        .justify(gtk::Justification::Center)
        .css_classes(["dim-label"])
        .build();
    content.append(&description);

    let cancel = gtk::Button::builder()
        .label(gettext("Cancel"))
        .css_classes(["flat"])
        .build();
    let continue_button = gtk::Button::builder()
        .label(gettext("Continue"))
        .css_classes(["suggested-action", "pill"])
        .build();
    let actions = gtk::Box::builder()
        .orientation(Orientation::Horizontal)
        .spacing(8)
        .halign(Align::Center)
        .build();
    actions.append(&cancel);
    actions.append(&continue_button);
    content.append(&actions);
    let toolbar = adw::ToolbarView::builder().content(&content).build();
    toolbar.add_top_bar(&adw::HeaderBar::new());
    dialog.set_child(Some(&toolbar));
    dialog.set_default_widget(Some(&continue_button));
    dialog.set_focus(Some(&type_toggle));

    let target_description = description.downgrade();
    type_toggle.connect_active_notify(move |toggle| {
        if let Some(target) = target_description.upgrade() {
            target.set_label(&gettext(match toggle.active() {
                1 => gettext_noop("Use an Ollama server you already run."),
                2 => gettext_noop("Connect to an AI service using your own API key."),
                _ => gettext_noop("Moose installs and runs Ollama on this device."),
            }));
        }
    });

    let target_dialog = dialog.downgrade();
    cancel.connect_clicked(move |_| {
        if let Some(dialog) = target_dialog.upgrade() {
            dialog.close();
        }
    });

    let target_ui = Rc::clone(ui);
    let target_backend = Rc::clone(backend);
    let target_dialog = dialog.downgrade();
    continue_button.connect_clicked(move |_| {
        let Some(dialog) = target_dialog.upgrade() else {
            return;
        };
        if provider_change_is_blocked(&target_ui, &target_backend) {
            return;
        }
        match type_toggle.active() {
            0 => {
                dialog.close();
                managed_install::show_dialog(&target_ui, &target_backend);
            }
            1 => {
                dialog.close();
                show_connect_external_dialog(&target_ui, &target_backend);
            }
            2 => {
                dialog.close();
                super::cloud_setup::show(&target_ui, &target_backend);
            }
            _ => {}
        }
    });

    dialog.present(Some(&ui.window));
}

pub(super) fn show_connect_external_dialog(ui: &Rc<WindowUi>, backend: &Rc<Backend>) {
    if provider_change_is_blocked(ui, backend) {
        return;
    }

    let setup = widgets::SetupDialog::new(
        &gettext("Connect Existing"),
        &gettext("Connect Existing"),
        &gettext("Use an Ollama server you already run."),
    );
    let dialog = setup.dialog.clone();

    let name_row = adw::EntryRow::builder()
        .title(gettext("Instance Name"))
        .text(next_provider_name(backend))
        .build();
    let url_row = adw::EntryRow::builder()
        .title(gettext("Ollama URL"))
        .text(DEFAULT_OLLAMA_BASE_URL)
        .input_purpose(gtk::InputPurpose::Url)
        .build();
    let connection_group = adw::PreferencesGroup::builder()
        .title(gettext("Connection"))
        .build();
    connection_group.add(&name_row);
    connection_group.add(&url_row);

    let sharing_group = adw::PreferencesGroup::builder()
        .title(gettext("Sharing Permissions"))
        .description(gettext("Choose whether this server can receive messages in all chats. Files require separate permission. Shared content is processed under the server's own data policy."))
        .build();
    let message_permission = super::privacy::message_permission_row();
    sharing_group.add(&message_permission);
    let description = gtk::Label::builder()
        .label(gettext(
            "Connecting enables external connections. Moose cannot verify where this server runs.",
        ))
        .wrap(true)
        .wrap_mode(pango::WrapMode::WordChar)
        .width_chars(1)
        .xalign(0.0)
        .css_classes(["caption", "dim-label"])
        .build();
    setup.content.append(&connection_group);
    setup.content.append(&sharing_group);
    setup.content.append(&description);

    let cancel = gtk::Button::with_label(&gettext("Cancel"));
    let connect = gtk::Button::with_label(&gettext("Connect"));
    setup.set_actions(&connect, &cancel);
    dialog.set_default_widget(Some(&connect));
    dialog.set_focus(Some(&url_row));

    let target_url_row = url_row.clone();
    name_row.connect_entry_activated(move |_| {
        target_url_row.grab_focus();
    });
    let target_message_permission = message_permission.clone();
    url_row.connect_text_notify(move |_| {
        target_message_permission.set_selected(0);
    });
    let target_button = connect.clone();
    url_row.connect_entry_activated(move |_| {
        target_button.emit_clicked();
    });

    let target_dialog = dialog.downgrade();
    cancel.connect_clicked(move |_| {
        if let Some(dialog) = target_dialog.upgrade() {
            dialog.close();
        }
    });

    let target_ui = Rc::clone(ui);
    let target_backend = Rc::clone(backend);
    let target_dialog = dialog.downgrade();
    connect.connect_clicked(move |_| {
        let Some(dialog) = target_dialog.upgrade() else {
            return;
        };
        if provider_change_is_blocked(&target_ui, &target_backend) {
            return;
        }
        if add_provider_from_sidebar(
            &target_ui,
            &target_backend,
            name_row.text().to_string(),
            url_row.text().to_string(),
            RemotePermissions {
                messages: message_permission.selected() == 1,
                files: false,
            },
        ) {
            dialog.close();
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
                .add_toast(adw::Toast::new(&gettext("Provider was not found")));
            return;
        }
        Err(error) => {
            ui.toast_overlay.add_toast(adw::Toast::new(&gettext_f(
                "Provider could not be loaded: {error}",
                &[("error", &(error).to_string())],
            )));
            return;
        }
    };

    let provider_id = provider.id.clone();
    let dialog = adw::AlertDialog::builder()
        .heading(gettext("Delete Provider?"))
        .body(gettext_f(
            "Delete \"{arg1}\" at {arg2}?",
            &[
                ("arg1", (provider.name.as_str())),
                ("arg2", (provider.base_url.as_str())),
            ],
        ))
        .close_response("cancel")
        .default_response("cancel")
        .build();
    dialog.add_response("cancel", &gettext("Cancel"));
    dialog.add_response("delete", &gettext("Delete"));
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
        .map(|providers| {
            gettext_f(
                "Ollama Provider {arg1}",
                &[("arg1", &(providers.len() + 1).to_string())],
            )
        })
        .unwrap_or_else(|_| gettext("Ollama Provider"))
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
        gettext("Managed Ollama")
    } else {
        gettext_f(
            "Managed Ollama {arg1}",
            &[("arg1", &(managed_count + 1).to_string())],
        )
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
                .add_toast(adw::Toast::new(&gettext("Provider added")));
            true
        }
        Err(error) => {
            show_error(&ui.window, &gettext("Provider could not be added"), &error);
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
                        show_error(
                            &ui.window,
                            &gettext("Provider could not be selected"),
                            &error,
                        );
                        return;
                    }
                }
            }
            ui.toast_overlay
                .add_toast(adw::Toast::new(&gettext("Provider deleted")));
        }
        Err(error) => show_error(
            &ui.window,
            &gettext("Provider could not be deleted"),
            &error,
        ),
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
                .add_toast(adw::Toast::new(&gettext("Provider was not found")));
            return;
        }
        Err(error) => {
            ui.toast_overlay.add_toast(adw::Toast::new(&gettext_f(
                "Provider could not be loaded: {error}",
                &[("error", &(error).to_string())],
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
                .add_toast(adw::Toast::new(&gettext("Provider switched")));
        }
        Ok(None) => {
            apply_active_provider(ui, backend, provider);
            ui.toast_overlay
                .add_toast(adw::Toast::new(&gettext("Provider switched")));
        }
        Err(error) => show_error(
            &ui.window,
            &gettext("Provider could not be switched"),
            &error,
        ),
    }
}
