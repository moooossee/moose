use super::*;

struct Library {
    dialog: adw::Dialog,
    list: gtk::ListBox,
    search: gtk::SearchEntry,
    empty: adw::StatusPage,
    results: gtk::Label,
    conversation: Option<String>,
}

pub(super) fn show(ui: &Rc<WindowUi>, backend: &Rc<Backend>) {
    let conversation = if active_provider(backend).is_some() {
        match draft_id(ui, backend) {
            Ok(id) => Some(id),
            Err(error) => {
                toast(ui, &error.to_string());
                return;
            }
        }
    } else {
        None
    };
    let dialog = adw::Dialog::builder()
        .title("Document Library")
        .content_width(620)
        .content_height(580)
        .build();
    let header = adw::HeaderBar::new();
    let add = gtk::Button::with_label("Import…");
    add.add_css_class("suggested-action");
    header.pack_start(&add);
    let search = gtk::SearchEntry::builder()
        .placeholder_text("Search documents and their contents")
        .hexpand(true)
        .build();
    let scope = adw::SwitchRow::builder()
        .title("Search Library in This Chat")
        .subtitle("Include relevant excerpts when you send a question.")
        .active(conversation.as_deref().is_some_and(|id| {
            backend
                .conversation_repository
                .library_enabled(id)
                .unwrap_or(false)
        }))
        .build();
    scope.set_sensitive(conversation.is_some());
    if conversation.is_none() {
        scope.set_subtitle("Connect an Ollama instance to ask questions about your documents.");
    }
    let group = adw::PreferencesGroup::new();
    group.add(&scope);
    let provider_name = active_provider(backend)
        .map(|provider| provider.name)
        .unwrap_or_else(|| "the selected Ollama instance".into());
    let hint = gtk::Label::builder().label(format!("Documents are stored on this device. Selected images and relevant text are sent to {provider_name} when you send a message."))
        .wrap(true).xalign(0.0).build();
    hint.add_css_class("caption");
    hint.add_css_class("dim-label");
    let list = gtk::ListBox::new();
    list.set_selection_mode(gtk::SelectionMode::None);
    list.add_css_class("boxed-list");
    let empty = adw::StatusPage::builder().icon_name("folder-documents-symbolic").title("Your Documents, Ready to Ask")
        .description("Import PDF, text, Markdown or source code. Attach a document to a message, or enable library search for this chat.").build();
    let content = gtk::Box::new(gtk::Orientation::Vertical, 14);
    content.set_margin_start(20);
    content.set_margin_end(20);
    content.set_margin_bottom(20);
    content.append(&search);
    content.append(&group);
    content.append(&hint);
    content.append(&empty);
    content.append(&list);
    let results = gtk::Label::builder().wrap(true).xalign(0.0).build();
    results.add_css_class("caption");
    results.add_css_class("dim-label");
    content.append(&results);
    let scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .child(&content)
        .build();
    let toolbar = adw::ToolbarView::builder().content(&scroll).build();
    toolbar.add_top_bar(&header);
    dialog.set_child(Some(&toolbar));
    let page = Rc::new(Library {
        dialog: dialog.clone(),
        list,
        search,
        empty,
        results,
        conversation,
    });
    let target_ui = ui.clone();
    let target_backend = backend.clone();
    add.connect_clicked(move |_| choose_files(&target_ui, &target_backend, true));
    let target_ui = ui.clone();
    let target_backend = backend.clone();
    let id = page.conversation.clone();
    scope.connect_active_notify(move |row| {
        let Some(id) = &id else {
            return;
        };
        if let Err(error) = target_backend
            .conversation_repository
            .set_library_enabled(id, row.is_active())
        {
            toast(
                &target_ui,
                &format!("Library setting could not be saved: {error}"),
            );
        }
        update_status(&target_ui, &target_backend);
    });
    let target_page = Rc::downgrade(&page);
    let target_ui = ui.clone();
    let target_backend = backend.clone();
    page.search.connect_search_changed(move |_| {
        if let Some(page) = target_page.upgrade() {
            populate(&page, &target_ui, &target_backend);
        }
    });
    let weak_page = Rc::downgrade(&page);
    let weak_ui = Rc::downgrade(ui);
    let weak_backend = Rc::downgrade(backend);
    *ui.attachments.library_changed.borrow_mut() = Some(Box::new(move || {
        if let (Some(page), Some(ui), Some(backend)) = (
            weak_page.upgrade(),
            weak_ui.upgrade(),
            weak_backend.upgrade(),
        ) {
            populate(&page, &ui, &backend);
        }
    }));
    populate(&page, ui, backend);
    let lifetime = RefCell::new(Some(page));
    dialog.connect_closed(move |_| {
        lifetime.borrow_mut().take();
    });
    dialog.present(Some(&ui.window));
}

fn populate(page: &Rc<Library>, ui: &Rc<WindowUi>, backend: &Rc<Backend>) {
    let assets = match backend
        .conversation_repository
        .library_assets(page.search.text().as_str())
    {
        Ok(assets) => assets,
        Err(error) => {
            toast(ui, &error.to_string());
            return;
        }
    };
    page.results.set_label(if assets.len() == 500 {
        "Showing the first 500 matches. Narrow your search to find more documents."
    } else {
        ""
    });
    page.results.set_visible(assets.len() == 500);
    page.search.grab_focus();
    while let Some(row) = page.list.first_child() {
        page.list.remove(&row);
    }
    page.empty.set_visible(assets.is_empty());
    page.list.set_visible(!assets.is_empty());
    page.empty.set_title(if page.search.text().is_empty() {
        "Your Documents, Ready to Ask"
    } else {
        "No Matching Documents"
    });
    let selected = page
        .conversation
        .as_deref()
        .and_then(|id| backend.conversation_repository.draft_assets(id).ok())
        .unwrap_or_default();
    for asset in assets {
        let row = adw::ActionRow::builder()
            .title(&asset.name)
            .subtitle(format!(
                "{} · {} · {}",
                asset.kind.to_uppercase(),
                super::super::model_actions::format_download_size(asset.byte_size as u64),
                if asset.page_count == 1 {
                    "1 page".into()
                } else {
                    format!("{} pages", asset.page_count)
                }
            ))
            .activatable(true)
            .build();
        row.set_title_lines(1);
        row.set_subtitle_lines(1);
        row.set_use_markup(false);
        row.add_prefix(&gtk::Image::from_icon_name("text-x-generic-symbolic"));
        let attach = widgets::icon_button("list-add-symbolic", "Attach to This Chat");
        attach.set_valign(gtk::Align::Center);
        attach.add_css_class("flat");
        attach.set_sensitive(page.conversation.is_some());
        if selected.iter().any(|item| item.id == asset.id) {
            attach.set_icon_name("object-select-symbolic");
            attach.set_sensitive(false);
            attach.set_tooltip_text(Some("Attached to this draft"));
        }
        let remove = widgets::icon_button("user-trash-symbolic", "Remove from Library");
        remove.set_valign(gtk::Align::Center);
        remove.add_css_class("flat");
        row.add_suffix(&attach);
        row.add_suffix(&remove);
        let parent = ui.window.clone();
        let repository = backend.conversation_repository.clone();
        let target_asset = asset.clone();
        row.connect_activated(move |_| preview(&parent, &repository, &target_asset, 1));
        let target_ui = ui.clone();
        let target_backend = backend.clone();
        let id = asset.id.clone();
        let conversation = page.conversation.clone();
        attach.connect_clicked(move |button| {
            let Some(conversation) = &conversation else {
                return;
            };
            match target_backend
                .conversation_repository
                .attach_to_draft(conversation, &id)
            {
                Ok(()) => {
                    button.set_icon_name("object-select-symbolic");
                    button.set_sensitive(false);
                    refresh(&target_ui, &target_backend);
                    conversation_list::refresh(&target_ui, &target_backend);
                }
                Err(error) => toast(&target_ui, &error.to_string()),
            }
        });
        let target_page = Rc::downgrade(page);
        let target_ui = ui.clone();
        let target_backend = backend.clone();
        remove.connect_clicked(move |_| {
            let Some(target_page) = target_page.upgrade() else { return; };
            let confirm = adw::AlertDialog::builder().heading("Remove Document?")
                .body(format!("Remove “{}” from the library? Copies attached to existing chats are kept. The original file is unchanged.", asset.name))
                .close_response("cancel").default_response("cancel").build();
            confirm.add_response("cancel", "Cancel");
            confirm.add_response("remove", "Remove");
            confirm.set_response_appearance("remove", adw::ResponseAppearance::Destructive);
            let page = target_page.clone();
            let ui = target_ui.clone();
            let backend = target_backend.clone();
            let id = asset.id.clone();
            confirm.connect_response(Some("remove"), move |_, _| {
                match backend.conversation_repository.remove_from_library(&id) {
                    Ok(()) => populate(&page, &ui, &backend),
                    Err(error) => toast(&ui, &error.to_string()),
                }
            });
            confirm.present(Some(&target_page.dialog));
        });
        page.list.append(&row);
    }
}

pub(super) fn preview(
    parent: &adw::ApplicationWindow,
    repository: &ConversationRepository,
    asset: &Asset,
    initial_page: i64,
) {
    let dialog = adw::Dialog::builder()
        .title(&asset.name)
        .content_width(760)
        .content_height(560)
        .build();
    let header = adw::HeaderBar::new();
    let save = gtk::Button::with_label("Save Copy…");
    header.pack_start(&save);
    let root = gtk::Box::new(gtk::Orientation::Vertical, 8);
    if asset.kind == "image" {
        match repository.asset_bytes(&asset.id).and_then(|bytes| {
            gdk::Texture::from_bytes(&glib::Bytes::from_owned(bytes))
                .map_err(|_| error("This image could not be displayed"))
        }) {
            Ok(texture) => {
                let picture = gtk::Picture::for_paintable(&texture);
                picture.set_content_fit(gtk::ContentFit::Contain);
                picture.set_can_shrink(true);
                picture.set_vexpand(true);
                root.append(&picture);
            }
            Err(error) => root.append(&gtk::Label::new(Some(&error.to_string()))),
        }
    } else {
        let page = gtk::SpinButton::with_range(1.0, asset.page_count.max(1) as f64, 1.0);
        page.set_value(initial_page.clamp(1, asset.page_count.max(1)) as f64);
        let page_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        page_row.set_halign(gtk::Align::Center);
        page_row.append(&gtk::Label::new(Some("Page")));
        page_row.append(&page);
        page_row.append(&gtk::Label::new(Some(&format!("of {}", asset.page_count))));
        let buffer = gtk::TextBuffer::new(None);
        let text = gtk::TextView::builder()
            .buffer(&buffer)
            .editable(false)
            .cursor_visible(false)
            .wrap_mode(gtk::WrapMode::WordChar)
            .left_margin(20)
            .right_margin(20)
            .top_margin(12)
            .bottom_margin(16)
            .vexpand(true)
            .build();
        let scroll = gtk::ScrolledWindow::builder()
            .child(&text)
            .vexpand(true)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .build();
        let repository = repository.clone();
        let id = asset.id.clone();
        let load = move |number| match repository.asset_page(&id, number) {
            Ok(text) if text.trim().is_empty() => {
                buffer.set_text("No selectable text on this page.")
            }
            Ok(text) => buffer.set_text(&text),
            Err(error) => buffer.set_text(&format!("Page could not be loaded: {error}")),
        };
        load(page.value_as_int() as i64);
        page.connect_value_changed(move |page| load(page.value_as_int() as i64));
        if asset.kind == "pdf" {
            let hint = gtk::Label::new(Some(
                "Extracted text · Original layout is available in the saved PDF",
            ));
            hint.add_css_class("caption");
            hint.add_css_class("dim-label");
            root.append(&hint);
        }
        root.append(&page_row);
        root.append(&scroll);
    }
    let toolbar = adw::ToolbarView::builder().content(&root).build();
    toolbar.add_top_bar(&header);
    dialog.set_child(Some(&toolbar));
    let repository = repository.clone();
    let asset = asset.clone();
    let parent_window = parent.clone();
    save.connect_clicked(move |_| {
        let filename = if asset.kind == "image" {
            format!(
                "{}.png",
                std::path::Path::new(&asset.name)
                    .file_stem()
                    .unwrap_or_default()
                    .to_string_lossy()
            )
        } else {
            asset.name.clone()
        };
        let picker = gtk::FileDialog::builder()
            .title("Save Attachment")
            .initial_name(&filename)
            .build();
        let parent = parent_window.clone();
        let repository = repository.clone();
        let id = asset.id.clone();
        glib::MainContext::default().spawn_local(async move {
            let Ok(file) = picker.save_future(Some(&parent)).await else {
                return;
            };
            let result = match repository.asset_bytes(&id) {
                Ok(bytes) => file
                    .replace_contents_future(
                        bytes,
                        None,
                        false,
                        gio::FileCreateFlags::REPLACE_DESTINATION,
                    )
                    .await
                    .map(|_| ())
                    .map_err(|(_, error)| error.to_string()),
                Err(error) => Err(error.to_string()),
            };
            if let Err(error) = result {
                let dialog = adw::AlertDialog::builder()
                    .heading("File Could Not Be Saved")
                    .body(&error)
                    .build();
                dialog.add_response("close", "Close");
                dialog.present(Some(&parent));
            }
        });
    });
    dialog.present(Some(parent));
}
