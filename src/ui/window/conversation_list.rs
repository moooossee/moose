use std::rc::Rc;

use adw::prelude::*;
use gtk::Orientation;

use crate::{conversations::ConversationSummary, error::Result};

use super::{Backend, WindowUi, clear_messages, load_conversation};

pub(super) fn refresh(ui: &Rc<WindowUi>, backend: &Rc<Backend>) {
    match backend.conversation_repository.list_recent_summaries(30) {
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
        .position(|id| id == conversation_id)
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
    let Some(conversation_id) = ui.conversation_ids.borrow().get(index).cloned() else {
        return Ok(());
    };
    load_conversation(ui, backend, &conversation_id)
}

fn set(ui: &Rc<WindowUi>, backend: &Rc<Backend>, summaries: Vec<ConversationSummary>) {
    *ui.restoring_conversation_selection.borrow_mut() = true;

    while let Some(child) = ui.conversation_list.first_child() {
        ui.conversation_list.remove(&child);
    }

    *ui.conversation_ids.borrow_mut() = summaries
        .iter()
        .map(|summary| summary.conversation.id.clone())
        .collect();

    if summaries.is_empty() {
        let row = adw::ActionRow::builder()
            .title("No conversations yet")
            .subtitle("Start a conversation")
            .sensitive(false)
            .build();
        row.add_css_class("moose-conversation-row");
        ui.conversation_list.append(&row);
        ui.conversation_list.unselect_all();
        *ui.restoring_conversation_selection.borrow_mut() = false;
        return;
    }

    for summary in summaries {
        ui.conversation_list.append(&row(&summary, ui, backend));
    }

    if let Some(conversation_id) = backend.active_conversation_id.borrow().as_deref() {
        select(ui, conversation_id);
    } else {
        ui.conversation_list.unselect_all();
    }

    *ui.restoring_conversation_selection.borrow_mut() = false;
}

fn row(summary: &ConversationSummary, ui: &Rc<WindowUi>, backend: &Rc<Backend>) -> adw::ActionRow {
    let row = adw::ActionRow::builder()
        .title(&summary.conversation.title)
        .build();
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

fn show_menu(
    row: &adw::ActionRow,
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
    let delete_button = gtk::Button::with_label("Delete Conversation");

    delete_button.add_css_class("flat");
    delete_button.add_css_class("destructive-action");
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
    delete_button.connect_clicked(move |_| {
        target_popover.popdown();
        confirm_delete(&target_ui, &target_backend, &target_conversation_id);
    });
    popover.popup();
}

fn confirm_delete(ui: &Rc<WindowUi>, backend: &Rc<Backend>, conversation_id: &str) {
    if backend.active_generation.borrow().is_some() {
        ui.toast_overlay
            .add_toast(adw::Toast::new("Finish the active generation first"));
        return;
    }

    let title = match backend.conversation_repository.get(&conversation_id) {
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
        .body(&format!("Delete \"{title}\" and all of its messages?"))
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
