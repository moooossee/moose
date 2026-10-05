use super::*;

#[path = "library_actions.rs"]
mod actions;

pub(in crate::ui::window) struct Library {
    pub(in crate::ui::window) root: gtk::Box,
    grid: gtk::FlowBox,
    scrolled: gtk::ScrolledWindow,
    search: gtk::SearchEntry,
    filters: Vec<gtk::ToggleButton>,
    empty: adw::StatusPage,
    results: gtk::Label,
    more: gtk::Button,
    add: gtk::Button,
    cancel: gtk::Button,
    back: gtk::Button,
    options: gtk::MenuButton,
    scope: adw::SwitchRow,
    syncing: Cell<bool>,
    limit: Cell<usize>,
}

pub(in crate::ui::window) fn build() -> Library {
    let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
    root.add_css_class("moose-files-page");
    let content = gtk::Box::new(gtk::Orientation::Vertical, 20);
    content.add_css_class("moose-files-content");
    let heading = gtk::Box::new(gtk::Orientation::Horizontal, 16);
    let back = widgets::icon_button("go-previous-symbolic", "Back to Chat");
    back.add_css_class("flat");
    back.add_css_class("moose-files-back");
    back.set_valign(gtk::Align::Center);
    let back_content = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    back_content.append(&gtk::Image::from_icon_name("go-previous-symbolic"));
    back_content.append(&gtk::Label::new(Some("Chat")));
    back.set_child(Some(&back_content));
    let titles = gtk::Box::new(gtk::Orientation::Vertical, 4);
    titles.set_hexpand(true);
    let title = gtk::Label::builder().label("Files").xalign(0.0).build();
    title.add_css_class("moose-files-title");
    let subtitle = gtk::Label::builder()
        .label("Everything you bring to a conversation.")
        .xalign(0.0)
        .wrap(true)
        .build();
    subtitle.add_css_class("dim-label");
    subtitle.add_css_class("moose-files-subtitle");
    titles.append(&title);
    titles.append(&subtitle);
    let add = gtk::Button::with_label("Import…");
    add.set_tooltip_text(Some("Add files to your library"));
    add.set_valign(gtk::Align::Center);
    add.add_css_class("moose-files-import");
    let cancel = widgets::icon_button("process-stop-symbolic", "Cancel Import");
    cancel.add_css_class("flat");
    cancel.set_valign(gtk::Align::Center);
    cancel.set_visible(false);
    let scope = adw::SwitchRow::builder()
        .title("Search Library in This Chat")
        .subtitle("Include relevant text when you send a message.")
        .build();
    let group = adw::PreferencesGroup::new();
    group.add(&scope);
    let options_content = gtk::Box::new(gtk::Orientation::Vertical, 0);
    options_content.set_margin_top(8);
    options_content.set_margin_bottom(8);
    options_content.set_margin_start(8);
    options_content.set_margin_end(8);
    options_content.append(&group);
    let popover = gtk::Popover::builder().child(&options_content).build();
    let options = gtk::MenuButton::builder()
        .icon_name("emblem-system-symbolic")
        .tooltip_text("Chat Library Settings")
        .valign(gtk::Align::Center)
        .popover(&popover)
        .build();
    options.add_css_class("flat");
    let navigation = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    spacer.set_hexpand(true);
    navigation.append(&back);
    navigation.append(&spacer);
    navigation.append(&options);
    content.append(&navigation);
    heading.append(&titles);
    heading.append(&cancel);
    heading.append(&add);
    content.append(&heading);

    let search = gtk::SearchEntry::builder()
        .placeholder_text("Search files…")
        .hexpand(true)
        .build();
    search.set_tooltip_text(Some("Search by file name or contents"));
    search.add_css_class("moose-files-search");
    content.append(&search);
    let filter_row = gtk::Box::new(gtk::Orientation::Horizontal, 4);
    filter_row.add_css_class("moose-files-filters");
    let mut filters = Vec::<gtk::ToggleButton>::new();
    for name in ["All", "Library", "Documents", "Images"] {
        let filter = gtk::ToggleButton::with_label(name);
        filter.add_css_class("flat");
        filter.add_css_class("moose-files-filter");
        if let Some(first) = filters.first() {
            filter.set_group(Some(first));
        }
        filter_row.append(&filter);
        filters.push(filter);
    }
    filters[0].set_active(true);

    let grid = gtk::FlowBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .homogeneous(true)
        .min_children_per_line(1)
        .max_children_per_line(3)
        .column_spacing(12)
        .row_spacing(12)
        .valign(gtk::Align::Start)
        .hexpand(true)
        .build();
    grid.add_css_class("moose-files-grid");
    let empty = adw::StatusPage::builder()
        .icon_name("folder-documents-symbolic")
        .title("A Place for Your Files")
        .description("Drop files here or choose Import. Chat attachments appear here too.")
        .build();
    empty.add_css_class("moose-files-empty");
    empty.add_css_class("compact");
    let results = gtk::Label::builder()
        .xalign(1.0)
        .hexpand(true)
        .width_chars(1)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .build();
    results.add_css_class("caption");
    results.add_css_class("dim-label");
    filter_row.append(&results);
    content.append(&filter_row);
    let more = gtk::Button::with_label("Show More");
    more.set_halign(gtk::Align::Center);
    more.add_css_class("flat");
    more.add_css_class("moose-files-more");
    let header_clamp = adw::Clamp::builder()
        .maximum_size(860)
        .tightening_threshold(650)
        .child(&content)
        .build();
    root.append(&header_clamp);
    let body = gtk::Box::new(gtk::Orientation::Vertical, 16);
    body.add_css_class("moose-files-body");
    body.append(&empty);
    body.append(&grid);
    body.append(&more);
    let clamp = adw::Clamp::builder()
        .maximum_size(860)
        .tightening_threshold(650)
        .child(&body)
        .build();
    let scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .child(&clamp)
        .build();
    root.append(&scroll);
    Library {
        root,
        grid,
        scrolled: scroll,
        search,
        filters,
        empty,
        results,
        more,
        add,
        cancel,
        back,
        options,
        scope,
        syncing: Cell::new(false),
        limit: Cell::new(100),
    }
}

pub(in crate::ui::window) fn bind(ui: &Rc<WindowUi>, backend: &Rc<Backend>) {
    let target_ui = ui.clone();
    let target_backend = backend.clone();
    ui.files_button
        .connect_clicked(move |_| show(&target_ui, &target_backend));
    let target_ui = ui.clone();
    let target_backend = backend.clone();
    ui.files.back.connect_clicked(move |_| {
        show_chat(&target_ui);
        let id = target_backend.active_conversation_id.borrow().clone();
        if let Some(id) = id {
            conversation_list::select(&target_ui, &id);
        }
        target_ui.entry.grab_focus();
    });
    let target_ui = ui.clone();
    let target_backend = backend.clone();
    ui.files
        .add
        .connect_clicked(move |_| choose_files(&target_ui, &target_backend, true));
    let target_ui = ui.clone();
    ui.files.cancel.connect_clicked(move |button| {
        target_ui
            .attachments
            .import_epoch
            .set(target_ui.attachments.import_epoch.get() + 1);
        button.set_sensitive(false);
    });
    let target_ui = ui.clone();
    let target_backend = backend.clone();
    ui.files.search.connect_search_changed(move |_| {
        target_ui.files.limit.set(100);
        target_ui.files.scrolled.vadjustment().set_value(0.0);
        refresh_if_visible(&target_ui, &target_backend);
    });
    for filter in &ui.files.filters {
        let target_ui = ui.clone();
        let target_backend = backend.clone();
        filter.connect_toggled(move |button| {
            if button.is_active() {
                target_ui.files.limit.set(100);
                target_ui.files.scrolled.vadjustment().set_value(0.0);
                refresh_if_visible(&target_ui, &target_backend);
            }
        });
    }
    let target_ui = ui.clone();
    let target_backend = backend.clone();
    ui.files.more.connect_clicked(move |_| {
        target_ui.files.limit.set(target_ui.files.limit.get() + 100);
        populate(&target_ui, &target_backend);
    });
    let target_ui = ui.clone();
    let target_backend = backend.clone();
    ui.files.scope.connect_active_notify(move |row| {
        if target_ui.files.syncing.get() {
            return;
        }
        let result = draft_id(&target_ui, &target_backend).and_then(|id| {
            target_backend
                .conversation_repository
                .set_library_enabled(&id, row.is_active())
        });
        if let Err(error) = result {
            toast(
                &target_ui,
                &format!("Library setting could not be saved: {error}"),
            );
            sync_scope(&target_ui, &target_backend);
        }
        update_status(&target_ui, &target_backend);
        conversation_list::refresh(&target_ui, &target_backend);
    });
    let drop = gtk::DropTarget::new(gdk::FileList::static_type(), gdk::DragAction::COPY);
    let target_ui = ui.clone();
    let target_backend = backend.clone();
    drop.connect_drop(move |_, value, _, _| {
        let Ok(files) = value.get::<gdk::FileList>() else {
            return false;
        };
        import_files(&target_ui, &target_backend, files.files(), true);
        true
    });
    ui.files.root.add_controller(drop);
}

pub(in crate::ui::window) fn show(ui: &Rc<WindowUi>, backend: &Rc<Backend>) {
    if let Err(error) = workspace::save(ui, backend) {
        toast(ui, &format!("Draft could not be saved: {error}"));
        return;
    }
    ui.root_stack.set_visible_child_name("app");
    ui.conversation_list.unselect_all();
    ui.model_manager_button
        .remove_css_class("moose-sidebar-button-active");
    ui.files_button.add_css_class("moose-sidebar-button-active");
    ui.content_stack.set_visible_child_name("files");
    sync_scope(ui, backend);
    sync_import(ui);
    populate(ui, backend);
    ui.files.search.grab_focus();
}

fn sync_scope(ui: &WindowUi, backend: &Backend) {
    let enabled = backend
        .active_conversation_id
        .borrow()
        .as_deref()
        .and_then(|id| backend.conversation_repository.library_enabled(id).ok())
        .unwrap_or(false);
    ui.files.syncing.set(true);
    ui.files.scope.set_active(enabled);
    ui.files
        .options
        .set_sensitive(active_provider(backend).is_some());
    ui.files.syncing.set(false);
}

pub(in crate::ui::window) fn sync_import(ui: &WindowUi) {
    let busy = ui.attachments.busy.get() > 0;
    ui.files.add.set_sensitive(!busy);
    ui.files
        .add
        .set_label(if busy { "Importing…" } else { "Import…" });
    ui.files.cancel.set_visible(busy);
    ui.files.cancel.set_sensitive(busy);
}

pub(in crate::ui::window) fn refresh_if_visible(ui: &Rc<WindowUi>, backend: &Rc<Backend>) {
    if ui.content_stack.visible_child_name().as_deref() == Some("files") {
        populate(ui, backend);
    }
}

fn populate(ui: &Rc<WindowUi>, backend: &Rc<Backend>) {
    let page = &ui.files;
    let selected = page.filters.iter().position(|button| button.is_active());
    let filter = match selected.unwrap_or(0) {
        1 => "library",
        2 => "documents",
        3 => "images",
        _ => "all",
    };
    let limit = page.limit.get();
    let assets = match backend.conversation_repository.file_assets(
        page.search.text().as_str(),
        filter,
        limit + 1,
    ) {
        Ok(assets) => assets,
        Err(error) => {
            while let Some(row) = page.grid.first_child() {
                page.grid.remove(&row);
            }
            page.grid.set_visible(false);
            page.more.set_visible(false);
            page.results.set_visible(false);
            page.empty.set_title("Files Could Not Be Loaded");
            page.empty.set_description(Some("Try opening Files again."));
            page.empty.set_visible(true);
            toast(ui, &error.to_string());
            return;
        }
    };
    while let Some(row) = page.grid.first_child() {
        page.grid.remove(&row);
    }
    let filtered = !page.search.text().trim().is_empty() || filter != "all";
    page.empty.set_title(if filtered {
        "No Matching Files"
    } else {
        "A Place for Your Files"
    });
    page.empty.set_description(Some(if filtered {
        "Try another search or select All."
    } else {
        "Drop files here or choose Import. Chat attachments appear here too."
    }));
    page.empty.set_visible(assets.is_empty());
    page.grid.set_visible(!assets.is_empty());
    page.more.set_visible(assets.len() > limit);
    page.results.set_visible(!assets.is_empty());
    let count = assets.len().min(limit);
    page.results.set_label(&if assets.len() > limit {
        format!("Showing {count} files")
    } else {
        format!("{count} file{}", if count == 1 { "" } else { "s" })
    });
    for asset in assets.into_iter().take(limit) {
        let kind = match asset.kind.as_str() {
            "image" => "Image",
            "pdf" => "PDF",
            _ => "Document",
        };
        let content = gtk::Box::new(gtk::Orientation::Vertical, 12);
        let icon = gtk::Image::from_icon_name(if asset.kind == "image" {
            "image-x-generic-symbolic"
        } else {
            "text-x-generic-symbolic"
        });
        icon.set_pixel_size(28);
        icon.set_halign(gtk::Align::Start);
        icon.add_css_class("moose-file-icon");
        let name = gtk::Label::builder()
            .label(&asset.name)
            .xalign(0.0)
            .yalign(0.0)
            .wrap(true)
            .wrap_mode(gtk::pango::WrapMode::WordChar)
            .lines(2)
            .width_chars(1)
            .max_width_chars(24)
            .height_request(40)
            .ellipsize(gtk::pango::EllipsizeMode::Middle)
            .build();
        name.add_css_class("moose-file-name");
        let details = gtk::Label::builder()
            .label(format!(
                "{kind} · {}",
                super::super::model_actions::format_download_size(asset.byte_size.max(0) as u64),
            ))
            .xalign(0.0)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .build();
        details.add_css_class("moose-file-details");
        content.append(&icon);
        content.append(&name);
        content.append(&details);
        let open = gtk::Button::builder()
            .child(&content)
            .width_request(208)
            .hexpand(true)
            .vexpand(true)
            .tooltip_text(format!("{} — Open chat or preview", asset.name))
            .build();
        open.add_css_class("moose-file-card");
        let menu = actions::menu(ui, backend, &asset);
        menu.set_halign(gtk::Align::End);
        menu.set_valign(gtk::Align::Start);
        menu.set_margin_top(8);
        menu.set_margin_end(8);
        let card = gtk::Overlay::builder().child(&open).build();
        card.add_css_class("moose-file-tile");
        card.add_overlay(&menu);
        let target_ui = Rc::downgrade(ui);
        let target_backend = backend.clone();
        open.connect_clicked(move |_| {
            if let Some(ui) = target_ui.upgrade() {
                actions::open(&ui, &target_backend, &asset);
            }
        });
        let child = gtk::FlowBoxChild::new();
        child.set_focusable(false);
        child.set_child(Some(&card));
        page.grid.insert(&child, -1);
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
