use std::{cell::RefCell, rc::Rc};

use adw::prelude::*;
use gtk::{Align, Orientation, gdk, gio, pango};

use crate::{
    conversations::{ConversationFolder, ConversationFolderFilter, validate_folder_name},
    error::{MooseError, Result},
    i18n::{gettext, gettext_f},
};

use super::{Backend, WindowUi, conversation_list};

mod dialogs;

use dialogs::{confirm_delete, edit_name};

const SETTINGS_SELECTED_FOLDER: &str = "selected-chat-folder";

pub(super) struct Controls {
    pub(super) root: gtk::Box,
    pub(super) navigation: gtk::Box,
    chat_heading: gtk::Label,
    back_button: gtk::Button,
    list: gtk::ListBox,
    general_create_button: gtk::Button,
    selection: RefCell<ConversationFolderFilter>,
    snapshot: RefCell<Option<Vec<ConversationFolder>>>,
}

impl Controls {
    pub(super) fn filter(&self) -> ConversationFolderFilter {
        self.selection.borrow().clone()
    }

    fn set_filter(&self, backend: &Backend, filter: ConversationFolderFilter) {
        if self.filter() == filter {
            return;
        }
        let value = match &filter {
            ConversationFolderFilter::General => "",
            ConversationFolderFilter::Folder(id) => id,
        };
        if let Some(settings) = folder_settings(backend)
            && let Err(error) = settings.set_string(SETTINGS_SELECTED_FOLDER, value)
        {
            crate::logging::record("WARN", "folders.selection", &error.to_string());
        }
        *self.selection.borrow_mut() = filter;
    }

    pub(super) fn selected_folder_id(&self) -> Option<String> {
        match self.filter() {
            ConversationFolderFilter::Folder(id) => Some(id),
            _ => None,
        }
    }
}

fn new_folder_button() -> gtk::Button {
    let button = super::widgets::icon_button("folder-new-symbolic", &gettext("New Folder"));
    button.add_css_class("moose-sidebar-button");
    button.set_action_name(Some("win.new-folder"));
    button.update_property(&[gtk::accessible::Property::Label(&gettext("New Folder"))]);
    button
}

pub(super) fn build() -> Controls {
    let root = gtk::Box::new(Orientation::Vertical, 0);
    root.set_visible(false);
    let heading = gtk::Box::builder()
        .orientation(Orientation::Horizontal)
        .spacing(6)
        .build();
    heading.add_css_class("moose-folder-heading");
    let label = gtk::Label::builder()
        .label(gettext("Folders"))
        .hexpand(true)
        .xalign(0.0)
        .build();
    label.add_css_class("caption-heading");
    label.add_css_class("dim-label");
    let create_button = new_folder_button();
    heading.append(&label);
    heading.append(&create_button);
    root.append(&heading);

    let list = gtk::ListBox::new();
    list.add_css_class("navigation-sidebar");
    list.add_css_class("moose-folder-list");
    list.set_selection_mode(gtk::SelectionMode::Single);
    list.update_property(&[gtk::accessible::Property::Label(&gettext("Chat Folders"))]);
    root.append(&list);

    let navigation = gtk::Box::builder()
        .orientation(Orientation::Horizontal)
        .spacing(6)
        .build();
    navigation.add_css_class("moose-chat-navigation");
    let back_button =
        super::widgets::icon_button("go-previous-symbolic", &gettext("Back to Chats"));
    back_button.add_css_class("moose-sidebar-button");
    back_button.set_visible(false);
    back_button.update_property(&[gtk::accessible::Property::Label(&gettext("Back to Chats"))]);
    let chat_heading = gtk::Label::builder()
        .label(gettext("Chats"))
        .halign(Align::Start)
        .hexpand(true)
        .xalign(0.0)
        .ellipsize(pango::EllipsizeMode::End)
        .build();
    chat_heading.add_css_class("heading");
    let general_create_button = new_folder_button();
    navigation.append(&back_button);
    navigation.append(&chat_heading);
    navigation.append(&general_create_button);

    Controls {
        root,
        navigation,
        chat_heading,
        back_button,
        list,
        general_create_button,
        selection: RefCell::new(ConversationFolderFilter::General),
        snapshot: RefCell::new(None),
    }
}

pub(super) fn bind(ui: &Rc<WindowUi>, backend: &Rc<Backend>) {
    if let Some(settings) = folder_settings(backend) {
        let value = settings.string(SETTINGS_SELECTED_FOLDER);
        *ui.folders.selection.borrow_mut() = match value.as_str() {
            "" => ConversationFolderFilter::General,
            id => ConversationFolderFilter::Folder(id.to_string()),
        };
    }
    let weak_ui = Rc::downgrade(ui);
    let target_backend = Rc::clone(backend);
    ui.folders.back_button.connect_clicked(move |_| {
        let Some(ui) = weak_ui.upgrade() else { return };
        ui.folders
            .set_filter(&target_backend, ConversationFolderFilter::General);
        ui.conversation_search_entry.set_text("");
        conversation_list::refresh(&ui, &target_backend);
        ui.conversation_search_entry.grab_focus();
    });

    let weak_ui = Rc::downgrade(ui);
    let target_backend = Rc::clone(backend);
    ui.folders.list.connect_row_activated(move |_, row| {
        let Some(ui) = weak_ui.upgrade() else { return };
        let Some(folder_id) = usize::try_from(row.index()).ok().and_then(|index| {
            ui.folders
                .snapshot
                .borrow()
                .as_ref()?
                .get(index)
                .map(|folder| folder.id.clone())
        }) else {
            return;
        };
        ui.folders
            .set_filter(&target_backend, ConversationFolderFilter::Folder(folder_id));
        ui.conversation_search_entry.set_text("");
        conversation_list::refresh(&ui, &target_backend);
        ui.conversation_search_entry.grab_focus();
    });
}

pub(super) fn create(ui: &Rc<WindowUi>, backend: &Rc<Backend>) {
    edit_name(ui, backend, None, None);
}

pub(super) fn show_move_dialog(ui: &Rc<WindowUi>, backend: &Rc<Backend>, conversation_id: &str) {
    dialogs::show_move_dialog(ui, backend, conversation_id);
}

pub(super) fn remove_from_folder(ui: &Rc<WindowUi>, backend: &Rc<Backend>, conversation_id: &str) {
    move_conversation(ui, backend, conversation_id, None);
}

pub(super) fn reveal(ui: &Rc<WindowUi>, backend: &Rc<Backend>, conversation_id: &str) {
    match backend.conversation_repository.get(conversation_id) {
        Ok(Some(conversation)) => {
            let visible = match ui.folders.filter() {
                ConversationFolderFilter::General => conversation.folder_id.is_none(),
                ConversationFolderFilter::Folder(id) => {
                    conversation.folder_id.as_deref() == Some(id.as_str())
                }
            };
            if !visible {
                ui.folders.set_filter(
                    backend,
                    match conversation.folder_id {
                        Some(id) => ConversationFolderFilter::Folder(id),
                        None => ConversationFolderFilter::General,
                    },
                );
            }
            ui.conversation_search_entry.set_text("");
            if conversation.archived_at.is_some() {
                ui.archived_conversations_button.set_active(true);
            }
            conversation_list::refresh(ui, backend);
            conversation_list::select(ui, conversation_id);
        }
        Ok(None) => {}
        Err(error) => show_error(ui, &error),
    }
}

pub(super) fn refresh(ui: &Rc<WindowUi>, backend: &Rc<Backend>) -> Result<()> {
    let folders = backend
        .conversation_repository
        .list_folders(ui.archived_conversations_button.is_active())?;
    if let ConversationFolderFilter::Folder(id) = ui.folders.filter()
        && !folders.iter().any(|folder| folder.id == id)
    {
        ui.folders
            .set_filter(backend, ConversationFolderFilter::General);
    }
    if ui.folders.snapshot.borrow().as_ref() != Some(&folders) {
        while let Some(child) = ui.folders.list.first_child() {
            ui.folders.list.remove(&child);
        }
        for folder in &folders {
            let row = folder_row(ui, backend, folder);
            add_drop_target(&row, ui, backend, folder.id.clone());
            ui.folders.list.append(&row);
        }
        *ui.folders.snapshot.borrow_mut() = Some(folders.clone());
    }
    let filter = ui.folders.filter();
    let selected = match &filter {
        ConversationFolderFilter::General => None,
        ConversationFolderFilter::Folder(id) => folders.iter().position(|folder| &folder.id == id),
    };
    if let Some(row) = selected.and_then(|index| ui.folders.list.row_at_index(index as i32)) {
        ui.folders.list.select_row(Some(&row));
    } else {
        ui.folders.list.unselect_all();
    }

    let name = match filter {
        ConversationFolderFilter::General => gettext("Chats"),
        ConversationFolderFilter::Folder(id) => folders
            .iter()
            .find(|folder| folder.id == id)
            .map(|folder| folder.name.clone())
            .unwrap_or_else(|| gettext("Chats")),
    };
    let is_general = ui.folders.filter() == ConversationFolderFilter::General;
    ui.folders
        .root
        .set_visible(is_general && !folders.is_empty());
    ui.folders
        .general_create_button
        .set_visible(is_general && folders.is_empty());
    ui.folders.back_button.set_visible(!is_general);
    ui.folders.chat_heading.set_label(&name);
    ui.folders.chat_heading.set_tooltip_text(Some(&name));
    ui.conversation_search_entry
        .set_placeholder_text(Some(
            &if ui.folders.filter() == ConversationFolderFilter::General {
                gettext("Search chats")
            } else {
                gettext_f("Search in {folder}", &[("folder", &name)])
            },
        ));
    Ok(())
}

fn folder_row(
    ui: &Rc<WindowUi>,
    backend: &Rc<Backend>,
    folder: &ConversationFolder,
) -> gtk::ListBoxRow {
    let name = folder.name.as_str();
    let row = gtk::ListBoxRow::new();
    row.set_tooltip_text(Some(name));
    let content = gtk::Box::new(Orientation::Horizontal, 8);
    content.add_css_class("moose-folder-content");
    content.append(&gtk::Image::from_icon_name("folder-symbolic"));
    let label = gtk::Label::builder()
        .label(name)
        .hexpand(true)
        .xalign(0.0)
        .ellipsize(pango::EllipsizeMode::End)
        .build();
    content.append(&label);
    let count = gtk::Label::new(Some(&folder.conversation_count.to_string()));
    count.add_css_class("caption");
    count.add_css_class("dim-label");
    content.append(&count);
    row.set_child(Some(&content));
    let click = gtk::GestureClick::builder().button(3).build();
    let weak_row = row.downgrade();
    let weak_ui = Rc::downgrade(ui);
    let backend = Rc::clone(backend);
    let folder = folder.clone();
    click.connect_pressed(move |gesture, _, x, y| {
        if let (Some(row), Some(ui)) = (weak_row.upgrade(), weak_ui.upgrade()) {
            gesture.set_state(gtk::EventSequenceState::Claimed);
            show_folder_menu(&row, &ui, &backend, &folder, x, y);
        }
    });
    row.add_controller(click);
    row
}

fn show_folder_menu(
    row: &gtk::ListBoxRow,
    ui: &Rc<WindowUi>,
    backend: &Rc<Backend>,
    folder: &ConversationFolder,
    x: f64,
    y: f64,
) {
    let model = gio::Menu::new();
    model.append(Some(&gettext("Rename…")), Some("folder.rename"));
    model.append(Some(&gettext("Delete Folder…")), Some("folder.delete"));
    let popover = gtk::PopoverMenu::from_model(Some(&model));
    popover.set_has_arrow(false);
    popover.set_parent(row);
    popover.set_pointing_to(Some(&gdk::Rectangle::new(
        x.round() as i32,
        y.round() as i32,
        1,
        1,
    )));
    let actions = gio::SimpleActionGroup::new();
    for name in ["rename", "delete"] {
        let action = gio::SimpleAction::new(name, None);
        let weak_ui = Rc::downgrade(ui);
        let weak_popover = popover.downgrade();
        let backend = Rc::clone(backend);
        let folder = folder.clone();
        action.connect_activate(move |_, _| {
            if let Some(popover) = weak_popover.upgrade() {
                popover.popdown();
            }
            let Some(ui) = weak_ui.upgrade() else { return };
            if name == "rename" {
                edit_name(&ui, &backend, Some(folder.clone()), None);
            } else {
                confirm_delete(&ui, &backend, &folder);
            }
        });
        actions.add_action(&action);
    }
    popover.insert_action_group("folder", Some(&actions));
    popover.connect_closed(|popover| popover.unparent());
    popover.popup();
}

fn move_conversation(
    ui: &Rc<WindowUi>,
    backend: &Rc<Backend>,
    conversation_id: &str,
    folder_id: Option<&str>,
) -> bool {
    let result = (|| -> Result<_> {
        let conversation = backend
            .conversation_repository
            .get(conversation_id)?
            .ok_or(MooseError::ConversationNotFound)?;
        if conversation.folder_id.as_deref() == folder_id {
            return Ok(None);
        }
        backend
            .conversation_repository
            .move_to_folder(conversation_id, folder_id)?;
        Ok(Some(conversation.folder_id))
    })();
    match result {
        Ok(Some(previous)) => {
            conversation_list::refresh(ui, backend);
            show_move_toast(ui, backend, conversation_id, previous);
            true
        }
        Ok(None) => true,
        Err(error) => {
            show_error(ui, &error);
            false
        }
    }
}

fn show_move_toast(
    ui: &Rc<WindowUi>,
    backend: &Rc<Backend>,
    conversation_id: &str,
    previous: Option<String>,
) {
    let toast = adw::Toast::builder()
        .title(gettext("Chat moved"))
        .button_label(gettext("Undo"))
        .build();
    let weak_ui = Rc::downgrade(ui);
    let backend = Rc::clone(backend);
    let conversation_id = conversation_id.to_string();
    toast.connect_button_clicked(move |_| {
        let Some(ui) = weak_ui.upgrade() else { return };
        match backend
            .conversation_repository
            .move_to_folder(&conversation_id, previous.as_deref())
        {
            Ok(()) => conversation_list::refresh(&ui, &backend),
            Err(error) => show_error(&ui, &error),
        }
    });
    ui.toast_overlay.add_toast(toast);
}

pub(super) fn add_drag_source(row: &gtk::ListBoxRow, conversation_id: &str) {
    let source = gtk::DragSource::builder()
        .actions(gdk::DragAction::MOVE)
        .build();
    let value = format!("moose-chat:{conversation_id}");
    source.connect_prepare(move |_, _, _| Some(gdk::ContentProvider::for_value(&value.to_value())));
    let weak_row = row.downgrade();
    source.connect_drag_begin(move |source, _| {
        if let Some(row) = weak_row.upgrade() {
            source.set_icon(Some(&gtk::WidgetPaintable::new(Some(&row))), 12, 12);
        }
    });
    row.add_controller(source);
}

fn add_drop_target(
    row: &gtk::ListBoxRow,
    ui: &Rc<WindowUi>,
    backend: &Rc<Backend>,
    folder_id: String,
) {
    let target = gtk::DropTarget::new(String::static_type(), gdk::DragAction::MOVE);
    target.set_preload(true);
    target.connect_accept(|_, drop| drop.drag().is_some());
    let weak_ui = Rc::downgrade(ui);
    let backend = Rc::clone(backend);
    target.connect_drop(move |_, value, _, _| {
        let Some(ui) = weak_ui.upgrade() else {
            return false;
        };
        let Ok(value) = value.get::<String>() else {
            return false;
        };
        let Some(id) = value.strip_prefix("moose-chat:") else {
            return false;
        };
        move_conversation(&ui, &backend, id, Some(&folder_id))
    });
    row.add_controller(target);
}

fn folder_settings(backend: &Backend) -> Option<&gio::Settings> {
    backend.settings.as_ref().filter(|settings| {
        settings
            .settings_schema()
            .is_some_and(|schema| schema.has_key(SETTINGS_SELECTED_FOLDER))
    })
}

fn show_error(ui: &WindowUi, error: &MooseError) {
    ui.toast_overlay.add_toast(adw::Toast::new(&gettext_f(
        "Folders could not be updated: {error}",
        &[("error", &error.to_string())],
    )));
}
