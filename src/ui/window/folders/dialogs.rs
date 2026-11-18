use super::*;

pub(super) fn edit_name(
    ui: &Rc<WindowUi>,
    backend: &Rc<Backend>,
    folder: Option<ConversationFolder>,
    conversation_id: Option<String>,
) {
    let title = if folder.is_some() {
        gettext("Rename Folder")
    } else {
        gettext("New Folder")
    };
    let dialog = adw::Dialog::builder()
        .title(&title)
        .content_width(400)
        .follows_content_size(true)
        .build();
    let toolbar = adw::ToolbarView::new();
    toolbar.set_width_request(400);
    toolbar.set_top_bar_style(adw::ToolbarStyle::Flat);
    let header = adw::HeaderBar::new();
    header.set_show_start_title_buttons(false);
    header.set_show_end_title_buttons(false);
    let cancel = gtk::Button::with_label(&gettext("Cancel"));
    let save = gtk::Button::with_label(&if folder.is_some() {
        gettext("Rename")
    } else {
        gettext("Create")
    });
    save.add_css_class("suggested-action");
    save.set_sensitive(false);
    toolbar.add_top_bar(&header);
    let content = gtk::Box::builder()
        .orientation(Orientation::Vertical)
        .spacing(16)
        .margin_top(8)
        .margin_bottom(24)
        .margin_start(24)
        .margin_end(24)
        .build();
    let group = adw::PreferencesGroup::new();
    let entry = adw::EntryRow::builder()
        .title(gettext("Folder Name"))
        .build();
    if let Some(folder) = &folder {
        entry.set_text(&folder.name);
    }
    group.add(&entry);
    content.append(&group);
    let error_label = gtk::Label::builder()
        .wrap(true)
        .xalign(0.0)
        .visible(false)
        .build();
    error_label.set_wrap_mode(pango::WrapMode::WordChar);
    error_label.set_width_chars(1);
    error_label.add_css_class("error");
    error_label.add_css_class("caption");
    content.append(&error_label);
    let actions = gtk::Box::builder()
        .orientation(Orientation::Horizontal)
        .spacing(12)
        .homogeneous(true)
        .margin_top(8)
        .build();
    actions.append(&cancel);
    actions.append(&save);
    content.append(&actions);
    let scrolled = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .propagate_natural_height(true)
        .child(&content)
        .build();
    toolbar.set_content(Some(&scrolled));
    dialog.set_child(Some(&toolbar));
    dialog.set_default_widget(Some(&save));
    dialog.set_focus(Some(&entry));

    let target_dialog = dialog.downgrade();
    cancel.connect_clicked(move |_| {
        if let Some(dialog) = target_dialog.upgrade() {
            dialog.close();
        }
    });
    let target_save = save.downgrade();
    let target_error = error_label.clone();
    let original_name = folder.as_ref().map(|folder| folder.name.clone());
    entry.connect_changed(move |entry| {
        let Some(target_save) = target_save.upgrade() else {
            return;
        };
        let name = entry.text();
        let validation = validate_folder_name(name.as_str());
        target_save
            .set_sensitive(validation.is_ok() && original_name.as_deref() != Some(name.trim()));
        if let Err(error) = validation
            && !name.trim().is_empty()
        {
            target_error.set_label(&error.to_string());
            target_error.set_visible(true);
            entry.add_css_class("error");
        } else {
            target_error.set_visible(false);
            entry.remove_css_class("error");
        }
    });
    let target_save = save.downgrade();
    entry.connect_entry_activated(move |_| {
        if let Some(save) = target_save.upgrade()
            && save.is_sensitive()
        {
            save.emit_clicked();
        }
    });
    let weak_ui = Rc::downgrade(ui);
    let backend = Rc::clone(backend);
    let target_dialog = dialog.downgrade();
    let target_entry = entry.downgrade();
    save.connect_clicked(move |_| {
        let (Some(ui), Some(target_entry)) = (weak_ui.upgrade(), target_entry.upgrade()) else {
            return;
        };
        let result = (|| -> Result<()> {
            if let Some(folder) = &folder {
                return backend
                    .conversation_repository
                    .rename_folder(&folder.id, target_entry.text().as_str());
            }
            let previous = conversation_id
                .as_deref()
                .map(|id| {
                    backend
                        .conversation_repository
                        .get(id)?
                        .ok_or(MooseError::ConversationNotFound)
                })
                .transpose()?;
            let folder_id = backend
                .conversation_repository
                .create_folder(target_entry.text().as_str(), conversation_id.as_deref())?;
            if let Some(conversation) = previous {
                show_move_toast(&ui, &backend, &conversation.id, conversation.folder_id);
            } else {
                ui.folders
                    .set_filter(&backend, ConversationFolderFilter::Folder(folder_id));
                ui.conversation_search_entry.set_text("");
            }
            Ok(())
        })();
        match result {
            Ok(()) => {
                if let Some(dialog) = target_dialog.upgrade() {
                    dialog.close();
                }
                conversation_list::refresh(&ui, &backend);
            }
            Err(error) => {
                error_label.set_label(&error.to_string());
                error_label.set_visible(true);
                target_entry.add_css_class("error");
                target_entry.grab_focus();
            }
        }
    });
    dialog.present(Some(&ui.window));
    entry.select_region(0, -1);
}

pub(super) fn confirm_delete(
    ui: &Rc<WindowUi>,
    backend: &Rc<Backend>,
    folder: &ConversationFolder,
) {
    let dialog = adw::AlertDialog::builder()
        .heading(gettext_f("Delete “{folder}”?", &[("folder", &folder.name)]))
        .body(gettext(
            "The chats in this folder will return to the main chat list.",
        ))
        .close_response("cancel")
        .default_response("cancel")
        .build();
    dialog.add_response("cancel", &gettext("Cancel"));
    dialog.add_response("delete", &gettext("Delete Folder"));
    dialog.set_response_appearance("delete", adw::ResponseAppearance::Destructive);
    let weak_ui = Rc::downgrade(ui);
    let backend = Rc::clone(backend);
    let folder_id = folder.id.clone();
    dialog.connect_response(Some("delete"), move |_, _| {
        let Some(ui) = weak_ui.upgrade() else { return };
        match backend.conversation_repository.delete_folder(&folder_id) {
            Ok(()) => {
                if ui.folders.selected_folder_id().as_deref() == Some(folder_id.as_str()) {
                    ui.folders
                        .set_filter(&backend, ConversationFolderFilter::General);
                }
                conversation_list::refresh(&ui, &backend);
                ui.toast_overlay.add_toast(adw::Toast::new(&gettext(
                    "Folder deleted. Chats were kept.",
                )));
            }
            Err(error) => show_error(&ui, &error),
        }
    });
    dialog.present(Some(&ui.window));
}

pub(super) fn show_move_dialog(ui: &Rc<WindowUi>, backend: &Rc<Backend>, conversation_id: &str) {
    let result = (|| -> Result<_> {
        let conversation = backend
            .conversation_repository
            .get(conversation_id)?
            .ok_or(MooseError::ConversationNotFound)?;
        let folders = backend.conversation_repository.list_folders(true)?;
        Ok((conversation, folders))
    })();
    let (conversation, folders) = match result {
        Ok(value) => value,
        Err(error) => {
            show_error(ui, &error);
            return;
        }
    };
    let dialog = adw::Dialog::builder()
        .title(gettext("Move Chat to Folder"))
        .content_width(380)
        .content_height(420)
        .build();
    let toolbar = adw::ToolbarView::new();
    let header = adw::HeaderBar::new();
    header.set_show_start_title_buttons(false);
    header.set_show_end_title_buttons(false);
    let cancel = gtk::Button::with_label(&gettext("Cancel"));
    let move_button = gtk::Button::with_label(&gettext("Move"));
    move_button.add_css_class("suggested-action");
    move_button.set_sensitive(false);
    header.pack_start(&cancel);
    header.pack_end(&move_button);
    toolbar.add_top_bar(&header);
    let content = gtk::Box::builder()
        .orientation(Orientation::Vertical)
        .spacing(12)
        .margin_top(12)
        .margin_bottom(18)
        .margin_start(18)
        .margin_end(18)
        .build();
    let search = gtk::SearchEntry::builder()
        .placeholder_text(gettext("Search folders"))
        .build();
    content.append(&search);
    let list = gtk::ListBox::new();
    list.add_css_class("boxed-list");
    list.set_selection_mode(gtk::SelectionMode::None);
    let placeholder = gtk::Label::builder()
        .label(gettext("No folders found"))
        .wrap(true)
        .margin_top(24)
        .margin_bottom(24)
        .css_classes(["dim-label"])
        .build();
    list.set_placeholder(Some(&placeholder));
    let selection = Rc::new(RefCell::new(conversation.folder_id.clone()));
    let mut group = None;
    let mut searchable_rows = Vec::new();
    for folder in folders {
        let folder_id = folder.id;
        let name = folder.name;
        let row = adw::ActionRow::builder()
            .title(gtk::glib::markup_escape_text(&name))
            .activatable(true)
            .build();
        row.set_tooltip_text(Some(&name));
        row.set_title_lines(1);
        row.add_prefix(&gtk::Image::from_icon_name("folder-symbolic"));
        let check = gtk::CheckButton::new();
        check.set_valign(Align::Center);
        check.set_can_target(false);
        check.set_focusable(false);
        check.set_group(group.as_ref());
        if group.is_none() {
            group = Some(check.clone());
        }
        check.set_active(Some(&folder_id) == conversation.folder_id.as_ref());
        row.add_suffix(&check);
        row.set_activatable_widget(Some(&check));
        let selected = Rc::clone(&selection);
        let target_move = move_button.clone();
        let original = conversation.folder_id.clone();
        check.connect_toggled(move |check| {
            if check.is_active() {
                *selected.borrow_mut() = Some(folder_id.clone());
                target_move.set_sensitive(Some(&folder_id) != original.as_ref());
            }
        });
        list.append(&row);
        searchable_rows.push((row, name.to_lowercase()));
    }
    search.connect_search_changed(move |entry| {
        let query = entry.text().trim().to_lowercase();
        for (row, name) in &searchable_rows {
            row.set_visible(name.contains(&query));
        }
    });
    let scrolled = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .child(&list)
        .build();
    content.append(&scrolled);
    let create = gtk::Button::with_label(&gettext("New Folder…"));
    create.add_css_class("flat");
    content.append(&create);
    toolbar.set_content(Some(&content));
    dialog.set_child(Some(&toolbar));
    dialog.set_default_widget(Some(&move_button));
    dialog.set_focus(Some(&search));
    let target_dialog = dialog.downgrade();
    cancel.connect_clicked(move |_| {
        if let Some(dialog) = target_dialog.upgrade() {
            dialog.close();
        }
    });
    let weak_ui = Rc::downgrade(ui);
    let target_backend = Rc::clone(backend);
    let target_dialog = dialog.downgrade();
    let target_id = conversation.id.clone();
    create.connect_clicked(move |_| {
        if let Some(ui) = weak_ui.upgrade() {
            if let Some(dialog) = target_dialog.upgrade() {
                dialog.close();
            }
            edit_name(&ui, &target_backend, None, Some(target_id.clone()));
        }
    });
    let weak_ui = Rc::downgrade(ui);
    let backend = Rc::clone(backend);
    let target_dialog = dialog.downgrade();
    move_button.connect_clicked(move |_| {
        if let Some(ui) = weak_ui.upgrade()
            && move_conversation(
                &ui,
                &backend,
                &conversation.id,
                selection.borrow().as_deref(),
            )
        {
            if let Some(dialog) = target_dialog.upgrade() {
                dialog.close();
            }
        }
    });
    dialog.present(Some(&ui.window));
}
