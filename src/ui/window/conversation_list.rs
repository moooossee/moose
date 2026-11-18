use crate::i18n::{gettext, gettext_f, gettext_noop};

use std::rc::Rc;

use adw::prelude::*;
use gtk::{Align, Orientation, gio, pango};

use crate::{
    conversations::{ConversationFolderFilter, ConversationSummary, ConversationTitleUpdate},
    error::Result,
};

use super::{
    Backend, WindowUi, clear_messages, load_conversation, restore_selected_provider_model,
    update_profile_indicator,
};

const ARCHIVED_ROW_CLASS: &str = "moose-archived-conversation";

pub(super) fn configure_headers(list: &gtk::ListBox) {
    list.set_header_func(|row, previous| {
        let archived = row.has_css_class(ARCHIVED_ROW_CLASS);
        let previous_archived = previous.is_some_and(|row| row.has_css_class(ARCHIVED_ROW_CLASS));
        if archived && !previous_archived {
            let heading = gtk::Label::builder()
                .label(gettext("Archived"))
                .halign(Align::Start)
                .xalign(0.0)
                .margin_start(8)
                .margin_end(8)
                .margin_top(6)
                .margin_bottom(4)
                .css_classes(["caption-heading", "dim-label"])
                .build();
            row.set_header(Some(&heading));
        } else if !archived && previous_archived {
            let separator = gtk::Separator::builder()
                .orientation(Orientation::Horizontal)
                .margin_start(8)
                .margin_end(8)
                .margin_top(8)
                .margin_bottom(8)
                .build();
            row.set_header(Some(&separator));
        } else {
            row.set_header(None::<&gtk::Widget>);
        }
    });
}

pub(super) fn refresh(ui: &Rc<WindowUi>, backend: &Rc<Backend>) {
    super::update_window_title(ui, backend);
    if let Err(error) = super::folders::refresh(ui, backend) {
        ui.toast_overlay.add_toast(adw::Toast::new(&gettext_f(
            "Folders could not be loaded: {error}",
            &[("error", &error.to_string())],
        )));
        return;
    }
    let query = ui.conversation_search_entry.text().to_string();
    let include_archived = ui.archived_conversations_button.is_active();
    match backend.conversation_repository.search_summaries_in_folder(
        &query,
        include_archived,
        &ui.folders.filter(),
        60,
    ) {
        Ok(summaries) => set(ui, backend, summaries),
        Err(error) => ui.toast_overlay.add_toast(adw::Toast::new(&gettext_f(
            "Conversations could not be loaded: {error}",
            &[("error", &(error).to_string())],
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

    while let Some(row) = ui.conversation_list.row_at_index(0) {
        ui.conversation_list.remove(&row);
    }

    if summaries.is_empty() {
        let row = gtk::ListBoxRow::new();
        let text = gettext(if !ui.conversation_search_entry.text().trim().is_empty() {
            gettext_noop("No matching conversations")
        } else if ui.folders.filter() != ConversationFolderFilter::General {
            gettext_noop("No chats in this folder")
        } else {
            gettext_noop("No chats here yet")
        });
        row.set_child(Some(&conversation_row_content(&text, false, false, false)));
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

    let mut ids = Vec::with_capacity(summaries.len());
    for summary in summaries {
        ids.push(Some(summary.conversation.id.clone()));
        ui.conversation_list.append(&row(&summary, ui, backend));
    }
    *ui.conversation_ids.borrow_mut() = ids;

    if ui.content_stack.visible_child_name().as_deref() != Some("chat") {
        ui.conversation_list.unselect_all();
    } else if let Some(conversation_id) = backend.active_conversation_id.borrow().as_deref() {
        select(ui, conversation_id);
    } else {
        ui.conversation_list.unselect_all();
    }

    *ui.restoring_conversation_selection.borrow_mut() = false;
}

fn row(summary: &ConversationSummary, ui: &Rc<WindowUi>, backend: &Rc<Backend>) -> gtk::ListBoxRow {
    let row = gtk::ListBoxRow::new();
    let is_generating = backend.active_generation.borrow().is_some()
        && backend.generation_context.borrow().conversation_id == summary.conversation.id;
    let content = conversation_row_content(
        &crate::i18n::conversation_title(&summary.conversation.title),
        summary.conversation.pinned_at.is_some(),
        summary.conversation.archived_at.is_some(),
        is_generating,
    );
    row.set_child(Some(&content));
    let draft = backend
        .conversation_repository
        .draft(&summary.conversation.id)
        .unwrap_or_default();
    set_draft_badge(&row, &draft);
    row.set_height_request(45);
    row.set_tooltip_text(Some(&crate::i18n::conversation_title(
        &summary.conversation.title,
    )));
    row.add_css_class("moose-conversation-row");
    if summary.conversation.archived_at.is_some() {
        row.add_css_class(ARCHIVED_ROW_CLASS);
    }
    super::folders::add_drag_source(&row, &summary.conversation.id);

    let click = gtk::GestureClick::builder().button(3).build();
    let weak_row = row.downgrade();
    let weak_ui = Rc::downgrade(ui);
    let target_backend = Rc::clone(backend);
    let conversation_id = summary.conversation.id.clone();
    click.connect_pressed(move |gesture, _, x, y| {
        if let (Some(row), Some(ui)) = (weak_row.upgrade(), weak_ui.upgrade()) {
            gesture.set_state(gtk::EventSequenceState::Claimed);
            show_menu(&row, &ui, &target_backend, &conversation_id, x, y);
        }
    });
    row.add_controller(click);

    row
}

pub(super) fn update_draft(ui: &WindowUi, conversation_id: &str, content: &str) {
    let index = ui
        .conversation_ids
        .borrow()
        .iter()
        .position(|id| id.as_deref() == Some(conversation_id));
    if let Some(row) = index.and_then(|index| ui.conversation_list.row_at_index(index as i32)) {
        set_draft_badge(&row, content);
    }
}

fn set_draft_badge(row: &gtk::ListBoxRow, draft: &str) {
    let Some(content) = row
        .child()
        .and_then(|child| child.downcast::<gtk::Box>().ok())
    else {
        return;
    };
    let mut child = content.first_child();
    while let Some(widget) = child {
        child = widget.next_sibling();
        if widget.widget_name() == "draft-badge" {
            content.remove(&widget);
        }
    }
    if !draft.is_empty() {
        let label = gtk::Label::new(Some(&gettext("Draft")));
        label.set_widget_name("draft-badge");
        label.add_css_class("caption");
        label.add_css_class("dim-label");
        label.set_tooltip_text(Some(&draft.chars().take(240).collect::<String>()));
        content.insert_child_after(&label, content.first_child().as_ref());
    }
}

fn conversation_row_content(
    title: &str,
    pinned: bool,
    archived: bool,
    generating: bool,
) -> gtk::Box {
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
        let icon = gtk::Image::from_icon_name("moose-archive-symbolic");
        icon.add_css_class("moose-conversation-state-icon");
        content.append(&icon);
    }

    if generating {
        let spinner = gtk::Spinner::new();
        spinner.set_size_request(14, 14);
        spinner.set_tooltip_text(Some(&gettext("Generating Response")));
        spinner.add_css_class("moose-conversation-spinner");
        spinner.start();
        content.append(&spinner);
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
    let conversation = match backend.conversation_repository.get(conversation_id) {
        Ok(Some(conversation)) => conversation,
        Ok(None) => {
            refresh(ui, backend);
            return;
        }
        Err(error) => {
            ui.toast_overlay.add_toast(adw::Toast::new(&gettext_f(
                "Conversation could not be loaded: {error}",
                &[("error", &error.to_string())],
            )));
            return;
        }
    };
    let is_pinned = conversation.pinned_at.is_some();
    let is_archived = conversation.archived_at.is_some();
    let model = gio::Menu::new();
    let organize = gio::Menu::new();
    organize.append(Some(&gettext("Rename…")), Some("chat.rename"));
    organize.append(Some(&gettext("Move to Folder…")), Some("chat.move"));
    if conversation.folder_id.is_some() {
        organize.append(
            Some(&gettext("Remove from Folder")),
            Some("chat.remove-folder"),
        );
    }
    if !is_archived {
        organize.append(
            Some(&gettext(if is_pinned {
                gettext_noop("Unpin")
            } else {
                gettext_noop("Pin")
            })),
            Some("chat.pin"),
        );
    }
    organize.append(
        Some(&gettext(if is_archived {
            gettext_noop("Unarchive")
        } else {
            gettext_noop("Archive")
        })),
        Some("chat.archive"),
    );
    model.append_section(None, &organize);
    let export = gio::Menu::new();
    export.append(Some(&gettext("Export Conversation…")), Some("chat.export"));
    model.append_section(None, &export);
    let delete = gio::Menu::new();
    delete.append(Some(&gettext("Delete Conversation…")), Some("chat.delete"));
    model.append_section(None, &delete);
    let popover = gtk::PopoverMenu::from_model(Some(&model));
    popover.set_has_arrow(false);
    popover.set_parent(row);
    popover.set_pointing_to(Some(&gtk::gdk::Rectangle::new(
        x.round() as i32,
        y.round() as i32,
        1,
        1,
    )));
    let actions = gio::SimpleActionGroup::new();
    for name in [
        "rename",
        "move",
        "remove-folder",
        "pin",
        "archive",
        "export",
        "delete",
    ] {
        let action = gio::SimpleAction::new(name, None);
        let weak_ui = Rc::downgrade(ui);
        let weak_popover = popover.downgrade();
        let backend = Rc::clone(backend);
        let conversation_id = conversation_id.to_string();
        action.connect_activate(move |_, _| {
            if let Some(popover) = weak_popover.upgrade() {
                popover.popdown();
            }
            let Some(ui) = weak_ui.upgrade() else { return };
            match name {
                "rename" => rename(&ui, &backend, &conversation_id),
                "move" => super::folders::show_move_dialog(&ui, &backend, &conversation_id),
                "remove-folder" => {
                    super::folders::remove_from_folder(&ui, &backend, &conversation_id)
                }
                "pin" => set_pinned(&ui, &backend, &conversation_id, !is_pinned),
                "archive" => set_archived(&ui, &backend, &conversation_id, !is_archived),
                "export" => {
                    super::conversation_export::show_dialog(&ui, &backend, &conversation_id)
                }
                "delete" => confirm_delete(&ui, &backend, &conversation_id),
                _ => {}
            }
        });
        actions.add_action(&action);
    }
    popover.insert_action_group("chat", Some(&actions));
    popover.connect_closed(|popover| popover.unparent());
    popover.popup();
}

fn rename(ui: &Rc<WindowUi>, backend: &Rc<Backend>, conversation_id: &str) {
    let conversation = match backend.conversation_repository.get(conversation_id) {
        Ok(Some(conversation)) => conversation,
        Ok(None) => {
            refresh(ui, backend);
            ui.toast_overlay
                .add_toast(adw::Toast::new(&gettext("Conversation was not found")));
            return;
        }
        Err(error) => {
            ui.toast_overlay.add_toast(adw::Toast::new(&gettext_f(
                "Conversation could not be loaded: {error}",
                &[("error", &(error).to_string())],
            )));
            return;
        }
    };

    let dialog = adw::Dialog::builder()
        .title(gettext("Rename Conversation"))
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
        .text(crate::i18n::conversation_title(&conversation.title))
        .activates_default(true)
        .hexpand(true)
        .build();
    let cancel_button = gtk::Button::with_label(&gettext("Cancel"));
    let save_button = gtk::Button::with_label(&gettext("Rename"));
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
                    .add_toast(adw::Toast::new(&gettext("Conversation renamed")));
            }
            Err(error) => target_ui
                .toast_overlay
                .add_toast(adw::Toast::new(&gettext_f(
                    "Conversation could not be renamed: {error}",
                    &[("error", &(error).to_string())],
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
            ui.toast_overlay
                .add_toast(adw::Toast::new(&gettext(if pinned {
                    gettext_noop("Conversation pinned")
                } else {
                    gettext_noop("Conversation unpinned")
                })));
        }
        Err(error) => ui.toast_overlay.add_toast(adw::Toast::new(&gettext_f(
            "Conversation could not be updated: {error}",
            &[("error", &(error).to_string())],
        ))),
    }
}

fn set_archived(ui: &Rc<WindowUi>, backend: &Rc<Backend>, conversation_id: &str, archived: bool) {
    if backend.active_generation.borrow().is_some()
        && backend.generation_context.borrow().conversation_id == conversation_id
    {
        ui.toast_overlay.add_toast(adw::Toast::new(&gettext(
            "Stop this response before archiving the chat",
        )));
        return;
    }
    if let Err(error) = super::workspace::save(ui, backend) {
        ui.toast_overlay.add_toast(adw::Toast::new(&gettext_f(
            "Draft could not be saved: {error}",
            &[("error", &(error).to_string())],
        )));
        return;
    }
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
                ui.conversation_list.unselect_all();
                clear_messages(ui);
                ui.draft_label.set_label("");
                super::generation::sync_controls(ui, backend);
                super::reasoning::refresh(ui, backend);
                restore_selected_provider_model(ui, backend);
                update_profile_indicator(ui, backend, None).ok();
            }
            refresh(ui, backend);
            ui.toast_overlay
                .add_toast(adw::Toast::new(&gettext(if archived {
                    gettext_noop("Conversation archived")
                } else {
                    gettext_noop("Conversation restored")
                })));
        }
        Err(error) => ui.toast_overlay.add_toast(adw::Toast::new(&gettext_f(
            "Conversation could not be updated: {error}",
            &[("error", &(error).to_string())],
        ))),
    }
}

fn confirm_delete(ui: &Rc<WindowUi>, backend: &Rc<Backend>, conversation_id: &str) {
    if backend.active_generation.borrow().is_some()
        && backend.generation_context.borrow().conversation_id == conversation_id
    {
        ui.toast_overlay.add_toast(adw::Toast::new(&gettext(
            "Finish the active generation first",
        )));
        return;
    }

    let title = match backend.conversation_repository.get(conversation_id) {
        Ok(Some(conversation)) => crate::i18n::conversation_title(&conversation.title),
        Ok(None) => {
            refresh(ui, backend);
            ui.toast_overlay
                .add_toast(adw::Toast::new(&gettext("Conversation was not found")));
            return;
        }
        Err(error) => {
            ui.toast_overlay.add_toast(adw::Toast::new(&gettext_f(
                "Conversation could not be loaded: {error}",
                &[("error", &(error).to_string())],
            )));
            return;
        }
    };

    let dialog = adw::AlertDialog::builder()
        .heading(gettext("Delete Conversation?"))
        .body(gettext_f(
            "Delete \"{title}\" and all of its messages?",
            &[("title", &(title).to_string())],
        ))
        .close_response("cancel")
        .default_response("cancel")
        .build();
    dialog.add_response("cancel", &gettext("Cancel"));
    dialog.add_response("delete", &gettext("Delete"));
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
    if backend.active_generation.borrow().is_some()
        && backend.generation_context.borrow().conversation_id == conversation_id
    {
        ui.toast_overlay.add_toast(adw::Toast::new(&gettext(
            "Stop this response before deleting the chat",
        )));
        return;
    }
    if backend.active_conversation_id.borrow().as_deref() == Some(conversation_id)
        && let Some(source) = ui.draft_source.borrow_mut().take()
    {
        source.remove();
    }
    match backend.conversation_repository.delete(conversation_id) {
        Ok(()) => {
            if backend.active_conversation_id.borrow().as_deref() == Some(conversation_id) {
                backend.active_conversation_id.borrow_mut().take();
                ui.conversation_list.unselect_all();
                clear_messages(ui);
                ui.draft_label.set_label("");
                super::generation::sync_controls(ui, backend);
                super::reasoning::refresh(ui, backend);
                restore_selected_provider_model(ui, backend);
                update_profile_indicator(ui, backend, None).ok();
            }
            refresh(ui, backend);
            ui.toast_overlay
                .add_toast(adw::Toast::new(&gettext("Conversation deleted")));
        }
        Err(error) => ui.toast_overlay.add_toast(adw::Toast::new(&gettext_f(
            "Conversation could not be deleted: {error}",
            &[("error", &(error).to_string())],
        ))),
    }
}
