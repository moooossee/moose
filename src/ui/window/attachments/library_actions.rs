use super::*;

#[derive(Clone, Copy)]
enum Action {
    Preview,
    Chats,
    Folder,
    Attach,
    Remove,
}

pub(super) fn menu(ui: &Rc<WindowUi>, backend: &Rc<Backend>, asset: &Asset) -> gtk::MenuButton {
    let button = gtk::MenuButton::builder()
        .icon_name("view-more-symbolic")
        .tooltip_text(format!("Actions for {}", asset.name))
        .valign(gtk::Align::Center)
        .build();
    button.add_css_class("flat");
    button.add_css_class("moose-file-actions");
    let content = gtk::Box::new(gtk::Orientation::Vertical, 2);
    content.add_css_class("moose-attachment-menu");
    let popover = gtk::Popover::builder().child(&content).build();
    button.set_popover(Some(&popover));
    let source = backend
        .conversation_repository
        .asset_source_uri(&asset.id)
        .ok()
        .flatten();
    for (action, icon, label) in [
        (Action::Preview, "document-open-symbolic", "Preview"),
        (Action::Chats, "go-jump-symbolic", "Open Chat…"),
        (Action::Folder, "folder-open-symbolic", "Show in Folder"),
        (
            Action::Attach,
            "mail-attachment-symbolic",
            "Attach to Current Chat",
        ),
        (Action::Remove, "user-trash-symbolic", "Remove from Library"),
    ] {
        if matches!(action, Action::Remove) && !asset.in_library {
            continue;
        }
        if matches!(action, Action::Remove) {
            content.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
        }
        let item = menu_item(icon, label);
        if matches!(action, Action::Folder) {
            item.set_sensitive(source.is_some());
            item.set_tooltip_text(Some(source.as_deref().unwrap_or(
                "Original location unavailable. Open Preview to save a copy.",
            )));
        }
        if matches!(action, Action::Attach) {
            item.set_sensitive(active_provider(backend).is_some());
        }
        let target_ui = Rc::downgrade(ui);
        let target_backend = backend.clone();
        let target_asset = asset.clone();
        let weak_popover = popover.downgrade();
        item.connect_clicked(move |_| {
            if let Some(popover) = weak_popover.upgrade() {
                popover.popdown();
            }
            let Some(ui) = target_ui.upgrade() else {
                return;
            };
            match action {
                Action::Preview => preview(
                    &ui.window,
                    &target_backend.conversation_repository,
                    &target_asset,
                    1,
                ),
                Action::Chats => show_chats(&ui, &target_backend, &target_asset),
                Action::Folder => show_folder(&ui, &target_backend, &target_asset),
                Action::Attach => attach(&ui, &target_backend, &target_asset),
                Action::Remove => remove(&ui, &target_backend, &target_asset),
            }
        });
        content.append(&item);
    }
    button
}

pub(super) fn open(ui: &Rc<WindowUi>, backend: &Rc<Backend>, asset: &Asset) {
    match backend
        .conversation_repository
        .asset_conversations(&asset.id)
    {
        Ok(chats) if chats.len() == 1 => generation::open(ui, backend, &chats[0].id),
        Ok(chats) if chats.is_empty() => {
            preview(&ui.window, &backend.conversation_repository, asset, 1)
        }
        Ok(_) => show_chats(ui, backend, asset),
        Err(error) => toast(ui, &format!("Chats could not be loaded: {error}")),
    }
}

fn show_chats(ui: &Rc<WindowUi>, backend: &Rc<Backend>, asset: &Asset) {
    let chats = match backend
        .conversation_repository
        .asset_conversations(&asset.id)
    {
        Ok(chats) => chats,
        Err(error) => {
            toast(ui, &format!("Chats could not be loaded: {error}"));
            return;
        }
    };
    let dialog = adw::Dialog::builder()
        .title("File Chats")
        .content_width(480)
        .content_height(420)
        .build();
    let content = gtk::Box::new(gtk::Orientation::Vertical, 16);
    content.set_margin_top(12);
    content.set_margin_bottom(20);
    content.set_margin_start(20);
    content.set_margin_end(20);
    let name = gtk::Label::builder()
        .label(&asset.name)
        .xalign(0.0)
        .wrap(true)
        .build();
    name.add_css_class("title-3");
    content.append(&name);
    if chats.is_empty() {
        content.append(
            &adw::StatusPage::builder()
                .icon_name("mail-attachment-symbolic")
                .title("Not Used in a Chat Yet")
                .description("Attach this file to a chat to start a conversation about it.")
                .build(),
        );
    } else {
        let list = gtk::ListBox::new();
        list.set_selection_mode(gtk::SelectionMode::None);
        list.add_css_class("boxed-list");
        for chat in chats {
            let row = adw::ActionRow::builder()
                .title(&chat.title)
                .subtitle(if chat.archived_at.is_some() {
                    "Archived chat"
                } else {
                    "Open conversation"
                })
                .activatable(true)
                .build();
            row.set_use_markup(false);
            row.set_title_lines(1);
            row.add_suffix(&gtk::Image::from_icon_name("go-next-symbolic"));
            let target_ui = Rc::downgrade(ui);
            let target_backend = backend.clone();
            let weak_dialog = dialog.downgrade();
            row.connect_activated(move |_| {
                if let Some(dialog) = weak_dialog.upgrade() {
                    dialog.close();
                }
                if let Some(ui) = target_ui.upgrade() {
                    generation::open(&ui, &target_backend, &chat.id);
                }
            });
            list.append(&row);
        }
        content.append(&list);
    }
    let scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .child(&content)
        .build();
    let toolbar = adw::ToolbarView::builder().content(&scroll).build();
    toolbar.add_top_bar(&adw::HeaderBar::new());
    dialog.set_child(Some(&toolbar));
    dialog.present(Some(&ui.window));
}

fn attach(ui: &Rc<WindowUi>, backend: &Rc<Backend>, asset: &Asset) {
    let result = draft_id(ui, backend).and_then(|id| {
        backend
            .conversation_repository
            .attach_to_draft(&id, &asset.id)
    });
    match result {
        Ok(()) => {
            refresh(ui, backend);
            show_chat(ui);
            conversation_list::refresh(ui, backend);
            ui.entry.grab_focus();
            toast(ui, "File attached to the current draft");
        }
        Err(error) => toast(ui, &error.to_string()),
    }
}

fn show_folder(ui: &Rc<WindowUi>, backend: &Backend, asset: &Asset) {
    let uri = match backend.conversation_repository.asset_source_uri(&asset.id) {
        Ok(Some(uri)) => uri,
        Ok(None) => {
            toast(ui, "Original location unavailable. Open Preview to save a copy.");
            return;
        }
        Err(error) => {
            toast(ui, &error.to_string());
            return;
        }
    };
    let file = gio::File::for_uri(&uri);
    let launcher = gtk::FileLauncher::new(Some(&file));
    let target_ui = ui.clone();
    glib::MainContext::default().spawn_local(async move {
        if let Err(error) = launcher
            .open_containing_folder_future(Some(&target_ui.window))
            .await
        {
            if !error.matches(gio::IOErrorEnum::Cancelled)
                && !error.matches(gtk::DialogError::Dismissed)
            {
                toast(&target_ui, "The original folder could not be opened. The file may have moved or access may have expired. Open Preview to save a copy.");
            }
        }
    });
}

fn remove(ui: &Rc<WindowUi>, backend: &Rc<Backend>, asset: &Asset) {
    let confirm = adw::AlertDialog::builder()
        .heading("Remove from Library?")
        .body(format!("Remove “{}” from your library? Files attached to chats are kept. The original file is unchanged.", asset.name))
        .close_response("cancel")
        .default_response("cancel")
        .build();
    confirm.add_response("cancel", "Cancel");
    confirm.add_response("remove", "Remove");
    confirm.set_response_appearance("remove", adw::ResponseAppearance::Destructive);
    let target_ui = Rc::downgrade(ui);
    let target_backend = backend.clone();
    let id = asset.id.clone();
    confirm.connect_response(Some("remove"), move |_, _| {
        let Some(ui) = target_ui.upgrade() else {
            return;
        };
        match target_backend
            .conversation_repository
            .remove_from_library(&id)
        {
            Ok(()) => {
                populate(&ui, &target_backend);
                toast(&ui, "Removed from library. Chat attachments are kept.");
            }
            Err(error) => toast(&ui, &error.to_string()),
        }
    });
    confirm.present(Some(&ui.window));
}
