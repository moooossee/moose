use crate::i18n::{gettext, gettext_noop};

use adw::prelude::*;
use gtk::gio;
use std::rc::Rc;

use super::{Backend, WindowUi, widgets::icon_button};
use crate::{APPLICATION_ID, APPLICATION_NAME};

pub(super) struct Controls {
    pub(super) sidebar: adw::HeaderBar,
    pub(super) content: adw::HeaderBar,
    pub(super) title: adw::WindowTitle,
    pub(super) models_action: gio::SimpleAction,
    sidebar_toggle: gtk::ToggleButton,
    sidebar_close: gtk::Button,
    sidebar_menu: gtk::MenuButton,
    new_chat: gtk::Button,
    back: gtk::Button,
    menu: gtk::MenuButton,
}

pub(super) fn build(library_settings: &gtk::MenuButton) -> Controls {
    let menu_model = gio::Menu::new();
    let conversation_menu = gio::Menu::new();
    conversation_menu.append(Some(&gettext("_Search Chats")), Some("win.search-chats"));
    conversation_menu.append(Some(&gettext("New _Folder…")), Some("win.new-folder"));
    menu_model.append_section(None, &conversation_menu);

    let navigation_menu = gio::Menu::new();
    navigation_menu.append(Some(&gettext("_Files and Library")), Some("win.show-files"));
    let models_item = gio::MenuItem::new(Some(&gettext("_Models")), Some("win.show-models"));
    models_item.set_attribute_value("hidden-when", Some(&"action-disabled".to_variant()));
    navigation_menu.append_item(&models_item);
    menu_model.append_section(None, &navigation_menu);

    let app_menu = gio::Menu::new();
    app_menu.append(Some(&gettext("_Preferences")), Some("win.preferences"));
    app_menu.append(
        Some(&gettext("_Keyboard Shortcuts")),
        Some("win.keyboard-shortcuts"),
    );
    app_menu.append(Some(&gettext("_About Moose")), Some("win.about"));
    menu_model.append_section(None, &app_menu);

    let sidebar = adw::HeaderBar::new();
    sidebar.set_title_widget(Some(&adw::WindowTitle::new(APPLICATION_NAME, "")));
    let sidebar_close = header_button("sidebar-show-symbolic", &gettext("Hide Sidebar"));
    sidebar_close.set_action_name(Some("win.toggle-sidebar"));
    sidebar_close.set_visible(false);
    sidebar.pack_start(&sidebar_close);
    let sidebar_new_chat = new_chat_button();
    sidebar.pack_start(&sidebar_new_chat);
    let sidebar_menu = menu_button(&menu_model);
    sidebar.pack_end(&sidebar_menu);

    let content = adw::HeaderBar::new();
    let title = adw::WindowTitle::new(&gettext("New Conversation"), "");
    content.set_title_widget(Some(&title));
    let sidebar_toggle = gtk::ToggleButton::builder()
        .icon_name("sidebar-show-symbolic")
        .tooltip_text(gettext("Hide Sidebar"))
        .active(true)
        .build();
    sidebar_toggle.add_css_class("flat");
    content.pack_start(&sidebar_toggle);
    let new_chat = new_chat_button();
    new_chat.set_visible(false);
    content.pack_start(&new_chat);
    let back = header_button("go-previous-symbolic", &gettext("Back to Chat"));
    back.set_action_name(Some("win.show-chat"));
    back.set_visible(false);
    content.pack_start(&back);
    let menu = menu_button(&menu_model);
    menu.set_visible(false);
    content.pack_end(&menu);
    content.pack_end(library_settings);
    library_settings.set_visible(false);

    let models_action = gio::SimpleAction::new("show-models", None);
    models_action.set_enabled(false);

    Controls {
        sidebar,
        content,
        title,
        models_action,
        sidebar_toggle,
        sidebar_close,
        sidebar_menu,
        new_chat,
        back,
        menu,
    }
}

pub(super) fn bind_layout(ui: &Rc<WindowUi>) {
    ui.split_view
        .bind_property("show-sidebar", &ui.header.sidebar_toggle, "active")
        .bidirectional()
        .sync_create()
        .build();
    if let Some(sidebar) = ui.split_view.sidebar() {
        ui.header
            .sidebar_toggle
            .update_relation(&[gtk::accessible::Relation::Controls(&[sidebar.upcast_ref()])]);
    }

    let weak_ui = Rc::downgrade(ui);
    ui.split_view.connect_show_sidebar_notify(move |_| {
        if let Some(ui) = weak_ui.upgrade() {
            sync_layout(&ui);
        }
    });
    let weak_ui = Rc::downgrade(ui);
    ui.split_view.connect_collapsed_notify(move |_| {
        if let Some(ui) = weak_ui.upgrade() {
            sync_layout(&ui);
        }
    });
    let weak_ui = Rc::downgrade(ui);
    ui.content_stack
        .connect_visible_child_name_notify(move |_| {
            if let Some(ui) = weak_ui.upgrade() {
                sync_layout(&ui);
            }
        });

    let breakpoint = adw::Breakpoint::new(adw::BreakpointCondition::new_length(
        adw::BreakpointConditionLengthType::MaxWidth,
        960.0,
        adw::LengthUnit::Sp,
    ));
    breakpoint.add_setter(&ui.split_view, "collapsed", Some(&true.to_value()));
    ui.window.add_breakpoint(breakpoint);
    sync_layout(ui);
}

pub(super) fn bind_actions(ui: &Rc<WindowUi>, backend: &Rc<Backend>) {
    for name in [
        "new-conversation",
        "new-folder",
        "search-chats",
        "show-chat",
        "show-files",
        "show-models",
        "toggle-sidebar",
        "preferences",
        "keyboard-shortcuts",
        "about",
    ] {
        let action = if name == "show-models" {
            ui.header.models_action.clone()
        } else {
            gio::SimpleAction::new(name, None)
        };
        let weak_ui = Rc::downgrade(ui);
        let backend = Rc::clone(backend);
        action.connect_activate(move |_, _| {
            let Some(ui) = weak_ui.upgrade() else {
                return;
            };
            match name {
                "new-folder" => super::folders::create(&ui, &backend),
                "show-chat" => {
                    super::show_chat(&ui);
                    let id = backend.active_conversation_id.borrow().clone();
                    if let Some(id) = id {
                        super::folders::reveal(&ui, &backend, &id);
                    }
                    ui.entry.grab_focus();
                }
                "show-files" => super::attachments::library::show(&ui, &backend),
                "keyboard-shortcuts" => {
                    let dialog = super::preferences::dialog(&ui.window, &ui, &backend);
                    dialog.set_visible_page_name("shortcuts");
                    dialog.present(Some(&ui.window));
                }
                "about" => {
                    let dialog = adw::AboutDialog::builder()
                        .application_name(APPLICATION_NAME)
                        .application_icon(APPLICATION_ID)
                        .version(env!("CARGO_PKG_VERSION"))
                        .website(env!("CARGO_PKG_HOMEPAGE"))
                        .issue_url(format!("{}/issues", env!("CARGO_PKG_REPOSITORY")))
                        .license_type(gtk::License::Gpl30)
                        .build();
                    dialog.add_link(&gettext("Source Code"), env!("CARGO_PKG_REPOSITORY"));
                    dialog.present(Some(&ui.window));
                }
                _ => {
                    super::run_shortcut_action(name, &ui, &backend);
                }
            }
        });
        ui.window.add_action(&action);
    }
}

fn sync_layout(ui: &WindowUi) {
    let sidebar_visible = ui.split_view.shows_sidebar();
    let view = ui.content_stack.visible_child_name();
    let chat_visible = view.as_deref() == Some("chat");
    ui.header
        .new_chat
        .set_visible(!sidebar_visible && chat_visible);
    ui.header.menu.set_visible(!sidebar_visible);
    ui.header.menu.set_primary(!sidebar_visible);
    ui.header.sidebar_menu.set_primary(sidebar_visible);
    ui.header.back.set_visible(!chat_visible);
    ui.header
        .sidebar_close
        .set_visible(ui.split_view.is_collapsed());
    ui.files
        .options
        .set_visible(view.as_deref() == Some("files"));
    let label = gettext(if sidebar_visible {
        gettext_noop("Hide Sidebar")
    } else {
        gettext_noop("Show Sidebar")
    });
    ui.header.sidebar_toggle.set_tooltip_text(Some(&label));
    ui.header
        .sidebar_toggle
        .update_property(&[gtk::accessible::Property::Label(&label)]);
    ui.header
        .sidebar_toggle
        .update_state(&[gtk::accessible::State::Expanded(Some(sidebar_visible))]);
}

fn header_button(icon: &str, label: &str) -> gtk::Button {
    let button = icon_button(icon, label);
    button.update_property(&[gtk::accessible::Property::Label(label)]);
    button
}

fn new_chat_button() -> gtk::Button {
    let button = header_button("list-add-symbolic", &gettext("New Conversation"));
    button.set_action_name(Some("win.new-conversation"));
    button
}

fn menu_button(model: &gio::Menu) -> gtk::MenuButton {
    let button = gtk::MenuButton::builder()
        .icon_name("open-menu-symbolic")
        .tooltip_text(gettext("Main Menu"))
        .menu_model(model)
        .build();
    button.update_property(&[gtk::accessible::Property::Label(&gettext("Main Menu"))]);
    button
}
