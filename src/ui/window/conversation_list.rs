use std::rc::Rc;

use adw::prelude::*;
use gtk::{Align, Orientation, pango};

use crate::{
    conversations::{ConversationSummary, ConversationTitleUpdate},
    error::Result,
};

use super::{
    Backend, WindowUi, clear_messages, load_conversation, restore_selected_provider_model,
    update_profile_indicator,
};

pub(super) fn refresh(ui: &Rc<WindowUi>, backend: &Rc<Backend>) {
    let query = ui.conversation_search_entry.text().to_string();
    let include_archived = ui.archived_conversations_button.is_active();
    match backend
        .conversation_repository
        .search_summaries(&query, include_archived, 60)
    {
        Ok(summaries) => set(ui, backend, summaries),
        Err(error) => ui.toast_overlay.add_toast(adw::Toast::new(&format!(
            "Conversations could not be loaded: {error}"
        ))),
    }
}

pub(super) fn select(ui: &WindowUi, conversation_id: &str) {
    let Some(index) = ui
        .conversation_ids
        .borrow()
        .iter()
        .position(|id| id.as_deref() == Some(conversation_id))
    else {
        ui.conversation_list.unselect_all();
        return;
    };

    let Ok(index) = i32::try_from(index) else {
        ui.conversation_list.unselect_all();
        return;
    };

    if let Some(row) = ui.conversation_list.row_at_index(index) {
        ui.conversation_list.select_row(Some(&row));
    } else {
        ui.conversation_list.unselect_all();
    }
}

pub(super) fn load_selected(ui: &WindowUi, backend: &Backend, row: &gtk::ListBoxRow) -> Result<()> {
    let Ok(index) = usize::try_from(row.index()) else {
        return Ok(());
    };
    let Some(Some(conversation_id)) = ui.conversation_ids.borrow().get(index).cloned() else {
        return Ok(());
    };
    load_conversation(ui, backend, &conversation_id)
}

fn set(ui: &Rc<WindowUi>, backend: &Rc<Backend>, summaries: Vec<ConversationSummary>) {
    *ui.restoring_conversation_selection.borrow_mut() = true;

    while let Some(child) = ui.conversation_list.first_child() {
        ui.conversation_list.remove(&child);
    }

    if summaries.is_empty() {
        let row = gtk::ListBoxRow::new();
        let text = if ui.conversation_search_entry.text().is_empty() {
            "No conversations yet"
        } else {
            "No matching conversations"
        };
        row.set_child(Some(&conversation_row_content(text, false, false)));
        row.set_height_request(45);
        row.set_selectable(false);
        row.set_sensitive(false);
        row.add_css_class("moose-conversation-row");
        ui.conversation_list.append(&row);
        *ui.conversation_ids.borrow_mut() = vec![None];
        ui.conversation_list.unselect_all();
        *ui.restoring_conversation_selection.borrow_mut() = false;
        return;
    }

    let mut ids = Vec::new();
    let mut pinned_heading_added = false;
    let mut recent_heading_added = false;
    let mut archived_heading_added = false;

    for summary in summaries {
        if summary.conversation.archived_at.is_some() {
            if !archived_heading_added {
                append_heading(ui, &mut ids, "Archived");
                archived_heading_added = true;
            }
        } else if summary.conversation.pinned_at.is_some() {
            if !pinned_heading_added {
                append_heading(ui, &mut ids, "Pinned");
                pinned_heading_added = true;
            }
        } else if !recent_heading_added {
            append_heading(ui, &mut ids, "Recent");
            recent_heading_added = true;
        }

        ids.push(Some(summary.conversation.id.clone()));
        ui.conversation_list.append(&row(&summary, ui, backend));
    }
    *ui.conversation_ids.borrow_mut() = ids;

    if let Some(conversation_id) = backend.active_conversation_id.borrow().as_deref() {
        select(ui, conversation_id);
    } else {
        ui.conversation_list.unselect_all();
    }

    *ui.restoring_conversation_selection.borrow_mut() = false;
}

fn row(summary: &ConversationSummary, ui: &Rc<WindowUi>, backend: &Rc<Backend>) -> gtk::ListBoxRow {
    let row = gtk::ListBoxRow::new();
    row.set_child(Some(&conversation_row_content(
        &summary.conversation.title,
        summary.conversation.pinned_at.is_some(),
        summary.conversation.archived_at.is_some(),
    )));
    row.set_height_request(45);
    row.set_tooltip_text(Some(&summary.conversation.title));
    row.add_css_class("moose-conversation-row");

    let click = gtk::GestureClick::builder().button(3).build();
    let target_row = row.clone();
    let target_ui = Rc::clone(ui);
    let target_backend = Rc::clone(backend);
    let conversation_id = summary.conversation.id.clone();

    click.connect_pressed(move |_, _, x, y| {
        show_menu(
            &target_row,
            &target_ui,
            &target_backend,
            &conversation_id,
            x,
            y,
        );
    });
    row.add_controller(click);
    row
}

fn append_heading(ui: &Rc<WindowUi>, ids: &mut Vec<Option<String>>, title: &str) {
    let row = gtk::ListBoxRow::new();
    let label = gtk::Label::builder()
        .label(title)
        .halign(Align::Start)
        .xalign(0.0)
        .build();
    label.add_css_class("moose-conversation-heading");
    row.set_child(Some(&label));
    row.set_selectable(false);
    row.set_sensitive(false);
    row.add_css_class("moose-conversation-heading-row");
    ui.conversation_list.append(&row);
    ids.push(None);
}

fn conversation_row_content(title: &str, pinned: bool, archived: bool) -> gtk::Box {
    let content = gtk::Box::builder()
        .orientation(Orientation::Horizontal)
        .spacing(5)
        .valign(Align::Center)
        .build();

    let title_label = gtk::Label::builder()
        .label(title)
        .halign(Align::Start)
        .hexpand(true)
        .xalign(0.0)
        .build();
    title_label.set_wrap(true);
    title_label.set_wrap_mode(pango::WrapMode::WordChar);
    title_label.set_ellipsize(pango::EllipsizeMode::End);
    title_label.add_css_class("moose-conversation-title");
    content.append(&title_label);

    if pinned {
        let icon = gtk::Image::from_icon_name("view-pin-symbolic");
        icon.add_css_class("moose-conversation-state-icon");
        content.append(&icon);
    }

    if archived {
        let icon = gtk::Image::from_icon_name("folder-symbolic");
        icon.add_css_class("moose-conversation-state-icon");
        content.append(&icon);
    }

    content
}

fn show_menu(
    row: &gtk::ListBoxRow,
    ui: &Rc<WindowUi>,
    backend: &Rc<Backend>,
    conversation_id: &str,
    x: f64,
    y: f64,
) {
    let popover = gtk::Popover::builder()
        .autohide(true)
        .has_arrow(false)
        .build();
    let menu = gtk::Box::builder()
        .orientation(Orientation::Vertical)
        .spacing(0)
        .build();
    let conversation = backend
        .conversation_repository
        .get(conversation_id)
        .ok()
        .flatten();
    let is_pinned = conversation
        .as_ref()
        .and_then(|conversation| conversation.pinned_at.as_ref())
        .is_some();
    let is_archived = conversation
        .as_ref()
        .and_then(|conversation| conversation.archived_at.as_ref())
        .is_some();
    let rename_button = gtk::Button::with_label("Rename");
    let pin_button = gtk::Button::with_label(if is_pinned { "Unpin" } else { "Pin" });
    let archive_button = gtk::Button::with_label(if is_archived { "Unarchive" } else { "Archive" });
    let export_button = gtk::Button::with_label("Export Conversation");
    let delete_button = gtk::Button::with_label("Delete Conversation");

    rename_button.add_css_class("flat");
    pin_button.add_css_class("flat");
    archive_button.add_css_class("flat");
    export_button.add_css_class("flat");
    delete_button.add_css_class("flat");
    delete_button.add_css_class("destructive-action");
    menu.append(&rename_button);
    if !is_archived {
        menu.append(&pin_button);
    }
    menu.append(&archive_button);
    menu.append(&export_button);
    menu.append(&delete_button);
    popover.set_child(Some(&menu));
    popover.set_parent(row);
    popover.set_pointing_to(Some(&gtk::gdk::Rectangle::new(
        x.round() as i32,
        y.round() as i32,
        1,
        1,
    )));

    let target_ui = Rc::clone(ui);
    let target_backend = Rc::clone(backend);
    let target_conversation_id = conversation_id.to_string();
    let target_popover = popover.clone();
    rename_button.connect_clicked(move |_| {
        target_popover.popdown();
        rename(&target_ui, &target_backend, &target_conversation_id);
    });

    let target_ui = Rc::clone(ui);
    let target_backend = Rc::clone(backend);
    let target_conversation_id = conversation_id.to_string();
    let target_popover = popover.clone();
    pin_button.connect_clicked(move |_| {
        target_popover.popdown();
        set_pinned(
            &target_ui,
            &target_backend,
            &target_conversation_id,
            !is_pinned,
        );
    });

    let target_ui = Rc::clone(ui);
    let target_backend = Rc::clone(backend);
    let target_conversation_id = conversation_id.to_string();
    let target_popover = popover.clone();
    archive_button.connect_clicked(move |_| {
        target_popover.popdown();
        set_archived(
            &target_ui,
            &target_backend,
            &target_conversation_id,
            !is_archived,
        );
    });

    let target_ui = Rc::clone(ui);
    let target_backend = Rc::clone(backend);
    let target_conversation_id = conversation_id.to_string();
    let target_popover = popover.clone();
    export_button.connect_clicked(move |_| {
        target_popover.popdown();
        super::conversation_export::show_dialog(
            &target_ui,
            &target_backend,
            &target_conversation_id,
        );
    });

    let target_ui = Rc::clone(ui);
    let target_backend = Rc::clone(backend);
    let target_conversation_id = conversation_id.to_string();
    let target_popover = popover.clone();
    delete_button.connect_clicked(move |_| {
        target_popover.popdown();
        confirm_delete(&target_ui, &target_backend, &target_conversation_id);
    });
    popover.popup();
}

fn rename(ui: &Rc<WindowUi>, backend: &Rc<Backend>, conversation_id: &str) {
    let conversation = match backend.conversation_repository.get(conversation_id) {
        Ok(Some(conversation)) => conversation,
        Ok(None) => {
            refresh(ui, backend);
            ui.toast_overlay
                .add_toast(adw::Toast::new("Conversation was not found"));
            return;
        }
        Err(error) => {
            ui.toast_overlay.add_toast(adw::Toast::new(&format!(
                "Conversation could not be loaded: {error}"
            )));
            return;
        }
    };

    let dialog = adw::Dialog::builder()
        .title("Rename Conversation")
        .content_width(420)
        .build();
    let content = gtk::Box::builder()
        .orientation(Orientation::Vertical)
        .spacing(14)
        .margin_top(18)
        .margin_bottom(18)
        .margin_start(18)
        .margin_end(18)
        .build();
    let entry = gtk::Entry::builder()
        .text(&conversation.title)
        .activates_default(true)
        .hexpand(true)
        .build();
    let cancel_button = gtk::Button::with_label("Cancel");
    let save_button = gtk::Button::with_label("Rename");
    save_button.add_css_class("suggested-action");
    let actions = gtk::Box::builder()
        .orientation(Orientation::Horizontal)
        .spacing(8)
        .halign(Align::End)
        .build();
    actions.append(&cancel_button);
    actions.append(&save_button);
    content.append(&entry);
    content.append(&actions);
    dialog.set_child(Some(&content));
    dialog.set_default_widget(Some(&save_button));

    let target_dialog = dialog.clone();
    cancel_button.connect_clicked(move |_| {
        target_dialog.close();
    });

    let target_ui = Rc::clone(ui);
    let target_backend = Rc::clone(backend);
    let target_conversation_id = conversation_id.to_string();
    let target_dialog = dialog.clone();
    save_button.connect_clicked(move |_| {
        let title = entry.text().to_string();
        match target_backend
            .conversation_repository
            .update_title(ConversationTitleUpdate {
                id: target_conversation_id.clone(),
                title,
            }) {
            Ok(_) => {
                target_dialog.close();
                refresh(&target_ui, &target_backend);
                target_ui
                    .toast_overlay
                    .add_toast(adw::Toast::new("Conversation renamed"));
            }
            Err(error) => target_ui.toast_overlay.add_toast(adw::Toast::new(&format!(
                "Conversation could not be renamed: {error}"
            ))),
        }
    });

    dialog.present(Some(&ui.window));
}

fn set_pinned(ui: &Rc<WindowUi>, backend: &Rc<Backend>, conversation_id: &str, pinned: bool) {
    match backend
        .conversation_repository
        .set_pinned(conversation_id, pinned)
    {
        Ok(_) => {
            refresh(ui, backend);
            ui.toast_overlay.add_toast(adw::Toast::new(if pinned {
                "Conversation pinned"
            } else {
                "Conversation unpinned"
            }));
        }
        Err(error) => ui.toast_overlay.add_toast(adw::Toast::new(&format!(
            "Conversation could not be updated: {error}"
        ))),
    }
}

fn set_archived(ui: &Rc<WindowUi>, backend: &Rc<Backend>, conversation_id: &str, archived: bool) {
    let result = if archived {
        backend.conversation_repository.archive(conversation_id)
    } else {
        backend.conversation_repository.unarchive(conversation_id)
    };

    match result {
        Ok(_) => {
            if archived
                && backend.active_conversation_id.borrow().as_deref() == Some(conversation_id)
            {
                backend.active_conversation_id.borrow_mut().take();
                backend.active_assistant_message_id.borrow_mut().take();
                backend.active_assistant_content.borrow_mut().clear();
                ui.conversation_list.unselect_all();
                clear_messages(ui);
                restore_selected_provider_model(ui, backend);
                update_profile_indicator(ui, backend, None).ok();
            }
            refresh(ui, backend);
            ui.toast_overlay.add_toast(adw::Toast::new(if archived {
                "Conversation archived"
            } else {
                "Conversation restored"
            }));
        }
        Err(error) => ui.toast_overlay.add_toast(adw::Toast::new(&format!(
            "Conversation could not be updated: {error}"
        ))),
    }
}

fn confirm_delete(ui: &Rc<WindowUi>, backend: &Rc<Backend>, conversation_id: &str) {
    if backend.active_generation.borrow().is_some() {
        ui.toast_overlay
            .add_toast(adw::Toast::new("Finish the active generation first"));
        return;
    }

    let title = match backend.conversation_repository.get(conversation_id) {
        Ok(Some(conversation)) => conversation.title,
        Ok(None) => {
            refresh(ui, backend);
            ui.toast_overlay
                .add_toast(adw::Toast::new("Conversation was not found"));
            return;
        }
        Err(error) => {
            ui.toast_overlay.add_toast(adw::Toast::new(&format!(
                "Conversation could not be loaded: {error}"
            )));
            return;
        }
    };

    let dialog = adw::AlertDialog::builder()
        .heading("Delete Conversation?")
        .body(format!("Delete \"{title}\" and all of its messages?"))
        .close_response("cancel")
        .default_response("cancel")
        .build();
    dialog.add_response("cancel", "Cancel");
    dialog.add_response("delete", "Delete");
    dialog.set_response_appearance("delete", adw::ResponseAppearance::Destructive);

    let target_ui = Rc::clone(ui);
    let target_backend = Rc::clone(backend);
    let target_conversation_id = conversation_id.to_string();
    dialog.connect_response(Some("delete"), move |_, _| {
        delete(&target_ui, &target_backend, &target_conversation_id);
    });
    dialog.present(Some(&ui.window));
}

fn delete(ui: &Rc<WindowUi>, backend: &Rc<Backend>, conversation_id: &str) {
    match backend.conversation_repository.delete(conversation_id) {
        Ok(()) => {
            if backend.active_conversation_id.borrow().as_deref() == Some(conversation_id) {
                backend.active_conversation_id.borrow_mut().take();
                backend.active_assistant_message_id.borrow_mut().take();
                backend.active_assistant_content.borrow_mut().clear();
                ui.conversation_list.unselect_all();
                clear_messages(ui);
                restore_selected_provider_model(ui, backend);
                update_profile_indicator(ui, backend, None).ok();
            }
            refresh(ui, backend);
            ui.toast_overlay
                .add_toast(adw::Toast::new("Conversation deleted"));
        }
        Err(error) => ui.toast_overlay.add_toast(adw::Toast::new(&format!(
            "Conversation could not be deleted: {error}"
        ))),
    }
}
