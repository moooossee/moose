use crate::i18n::{gettext, gettext_f, gettext_noop, ngettext_f};

use super::*;
use crate::attachments::{Asset, MAX_ATTACHMENTS, MAX_FILE_BYTES, error};
use gtk::{gdk, glib};

mod clipboard;
mod composer;
mod document;
mod image;
mod import;
pub(super) mod library;
mod request;
pub(super) use request::prepare;

pub(super) struct Controls {
    pub(super) button: gtk::MenuButton,
    pub(super) previews: gtk::FlowBox,
    pub(super) status: gtk::Label,
    pub(super) capability: gtk::Image,
    pub(super) cancel: gtk::Button,
    import_epoch: Cell<u64>,
    add: gtk::Button,
    paste: gtk::Button,
    library: gtk::Button,
    pub(super) busy: Cell<u32>,
    pub(super) count: Cell<usize>,
    pub(super) has_images: Cell<bool>,
    vision: RefCell<Option<bool>>,
    pub(super) capabilities: RefCell<HashMap<(String, String), bool>>,
    revision: Cell<u64>,
    capability_failed: Cell<bool>,
}

pub(super) fn build() -> Controls {
    let button = gtk::MenuButton::builder()
        .icon_name("mail-attachment-symbolic")
        .tooltip_text(gettext("Add Attachments"))
        .valign(gtk::Align::Center)
        .build();
    button.add_css_class("moose-attach-button");
    button.add_css_class("flat");
    let menu = gtk::Box::new(gtk::Orientation::Vertical, 2);
    menu.add_css_class("moose-attachment-menu");
    let add = menu_item("mail-attachment-symbolic", &gettext("Attach Files…"));
    let paste = menu_item("edit-paste-symbolic", &gettext("Paste Image"));
    let library = menu_item("folder-documents-symbolic", &gettext("Browse Files"));
    menu.append(&add);
    menu.append(&paste);
    menu.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
    menu.append(&library);
    let popover = gtk::Popover::builder()
        .child(&menu)
        .position(gtk::PositionType::Top)
        .build();
    button.set_popover(Some(&popover));
    let previews = composer::previews();
    previews.set_visible(false);
    let status = gtk::Label::builder()
        .wrap(true)
        .xalign(0.0)
        .visible(false)
        .build();
    status.add_css_class("caption");
    status.add_css_class("dim-label");
    let capability = gtk::Image::from_icon_name("image-x-generic-symbolic");
    capability.set_visible(false);
    capability.set_tooltip_text(Some(&gettext("This model supports images")));
    capability.add_css_class("dim-label");
    let cancel = gtk::Button::with_label(&gettext("Cancel Import"));
    cancel.add_css_class("flat");
    cancel.set_visible(false);
    Controls {
        cancel,
        import_epoch: Cell::new(0),
        capability,
        capability_failed: Cell::new(false),
        button,
        previews,
        status,
        add,
        paste,
        library,
        busy: Cell::new(0),
        count: Cell::new(0),
        has_images: Cell::new(false),
        vision: RefCell::new(None),
        capabilities: RefCell::new(HashMap::new()),
        revision: Cell::new(0),
    }
}

pub(super) fn bind(ui: &Rc<WindowUi>, backend: &Rc<Backend>) {
    library::bind(ui, backend);
    let target_ui = ui.clone();
    ui.attachments.cancel.connect_clicked(move |button| {
        target_ui
            .attachments
            .import_epoch
            .set(target_ui.attachments.import_epoch.get() + 1);
        button.set_sensitive(false);
    });
    let target_ui = ui.clone();
    let target_backend = backend.clone();
    ui.attachments.add.connect_clicked(move |_| {
        target_ui.attachments.button.popdown();
        choose_files(&target_ui, &target_backend, false);
    });
    let target_ui = ui.clone();
    let target_backend = backend.clone();
    ui.attachments.paste.connect_clicked(move |_| {
        target_ui.attachments.button.popdown();
        paste_image(&target_ui, &target_backend);
    });
    let target_ui = ui.clone();
    let target_backend = backend.clone();
    ui.attachments.library.connect_clicked(move |_| {
        target_ui.attachments.button.popdown();
        library::show(&target_ui, &target_backend);
    });
    let target_ui = ui.clone();
    let target_backend = backend.clone();
    ui.entry.connect_paste_clipboard(move |entry| {
        let formats = entry.clipboard().formats();
        if formats.contains_type(gdk::Texture::static_type())
            && !formats.contain_mime_type("text/plain")
        {
            entry.stop_signal_emission_by_name("paste-clipboard");
            paste_image(&target_ui, &target_backend);
        }
    });
    let drop = gtk::DropTarget::new(gdk::FileList::static_type(), gdk::DragAction::COPY);
    let target_ui = ui.clone();
    let target_backend = backend.clone();
    drop.connect_drop(move |_, value, _, _| {
        let Ok(files) = value.get::<gdk::FileList>() else {
            return false;
        };
        import_files(&target_ui, &target_backend, files.files(), false);
        true
    });
    ui.message_stack.add_controller(drop);
    let drop = gtk::DropTarget::new(gdk::FileList::static_type(), gdk::DragAction::COPY);
    let target_ui = ui.clone();
    let target_backend = backend.clone();
    drop.connect_drop(move |_, value, _, _| {
        let Ok(files) = value.get::<gdk::FileList>() else {
            return false;
        };
        import_files(&target_ui, &target_backend, files.files(), false);
        true
    });
    ui.entry.add_controller(drop);
}

fn draft_id(ui: &WindowUi, backend: &Backend) -> Result<String> {
    workspace::save(ui, backend)?;
    let active = backend.active_conversation_id.borrow().clone();
    match active {
        Some(id) => Ok(id),
        None => create_empty_conversation(backend),
    }
}

pub(super) fn choose_files(ui: &Rc<WindowUi>, backend: &Rc<Backend>, library_only: bool) {
    let filter = gtk::FileFilter::new();
    filter.set_name(Some(&gettext("Images, documents and source code")));
    for extension in [
        "txt", "md", "markdown", "pdf", "rs", "py", "js", "jsx", "ts", "tsx", "json", "yaml",
        "yml", "toml", "xml", "html", "css", "csv", "log", "c", "h", "cpp", "hpp", "java", "go",
        "sh", "rb", "sql", "swift", "kt", "ini", "conf",
    ] {
        filter.add_suffix(extension);
    }
    filter.add_mime_type("text/*");
    for mime in ["image/png", "image/jpeg", "image/webp"] {
        filter.add_mime_type(mime);
    }
    let filters = gio::ListStore::new::<gtk::FileFilter>();
    filters.append(&filter);
    let all = gtk::FileFilter::new();
    all.set_name(Some(&gettext("All files")));
    all.add_pattern("*");
    filters.append(&all);
    let dialog = gtk::FileDialog::builder()
        .title(gettext(if library_only {
            gettext_noop("Import Files")
        } else {
            gettext_noop("Attach Files")
        }))
        .accept_label(gettext(if library_only {
            gettext_noop("Import")
        } else {
            gettext_noop("Attach")
        }))
        .filters(&filters)
        .default_filter(&filter)
        .build();
    let ui = ui.clone();
    let backend = backend.clone();
    glib::MainContext::default().spawn_local(async move {
        match dialog.open_multiple_future(Some(&ui.window)).await {
            Ok(files) => {
                let files = (0..files.n_items())
                    .filter_map(|index| files.item(index).and_downcast::<gio::File>())
                    .collect();
                import_files(&ui, &backend, files, library_only);
            }
            Err(error)
                if error.matches(gio::IOErrorEnum::Cancelled)
                    || error.matches(gtk::DialogError::Dismissed) => {}
            Err(error) => toast(
                &ui,
                &gettext_f(
                    "Files could not be selected: {error}",
                    &[("error", &(error).to_string())],
                ),
            ),
        }
    });
}

async fn import_step<T>(
    ui: &WindowUi,
    epoch: u64,
    future: impl std::future::Future<Output = Result<T>>,
) -> Result<T> {
    let cancellation = async {
        for _ in 0..350 {
            glib::timeout_future(Duration::from_millis(100)).await;
            if ui.attachments.import_epoch.get() != epoch {
                return Err(error(gettext("Import canceled")));
            }
        }
        Err(error(gettext("This file took too long to read or process")))
    };
    match futures_util::future::select(Box::pin(future), Box::pin(cancellation)).await {
        futures_util::future::Either::Left((result, _))
        | futures_util::future::Either::Right((result, _)) => result,
    }
}

async fn read_file(file: &gio::File) -> Result<Vec<u8>> {
    let stream = file
        .read_future(glib::Priority::DEFAULT)
        .await
        .map_err(|_| {
            error(gettext(
                "This file could not be opened. Select it again using Attach Files.",
            ))
        })?;
    let mut bytes = Vec::new();
    loop {
        let chunk = stream
            .read_bytes_future(64 * 1024, glib::Priority::DEFAULT)
            .await
            .map_err(|_| error(gettext("This file could not be read")))?;
        if chunk.is_empty() {
            return Ok(bytes);
        }
        if bytes.len() + chunk.len() > MAX_FILE_BYTES {
            return Err(error(gettext("Choose a file smaller than 25 MB")));
        }
        bytes.extend_from_slice(chunk.as_ref());
    }
}

fn import_files(
    ui: &Rc<WindowUi>,
    backend: &Rc<Backend>,
    files: Vec<gio::File>,
    library_only: bool,
) {
    if files.is_empty() {
        return;
    }
    if ui.attachments.busy.get() > 0 {
        toast(ui, &gettext("Finish or cancel the current import first"));
        return;
    }
    if files.len() > if library_only { 32 } else { MAX_ATTACHMENTS } {
        toast(
            ui,
            &gettext("Select up to 8 attachments or 32 library files at a time"),
        );
        return;
    }
    let conversation = if library_only {
        None
    } else {
        match draft_id(ui, backend) {
            Ok(id) => Some(id),
            Err(error) => {
                toast(ui, &crate::i18n::error_message(&error));
                return;
            }
        }
    };
    ui.attachments.busy.set(1);
    let epoch = ui.attachments.import_epoch.get();
    update_status(ui, backend);
    let ui = ui.clone();
    let backend = backend.clone();
    glib::MainContext::default().spawn_local(async move {
        let mut imported = 0;
        for file in files {
            if ui.attachments.import_epoch.get() != epoch {
                break;
            }
            let name = file
                .basename()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| gettext("Document"));
            let result = async {
                let bytes = import_step(&ui, epoch, read_file(&file)).await?;
                let cache = backend.paths.cache_dir().to_path_buf();
                let name = name.clone();
                let worker = backend
                    .runtime
                    .spawn_blocking(move || import::parse(name, bytes, &cache));
                let asset = import_step(&ui, epoch, async {
                    worker
                        .await
                        .map_err(|_| error(gettext("This file could not be processed")))?
                })
                .await?;
                if ui.attachments.import_epoch.get() != epoch {
                    return Err(error(gettext("Import canceled")));
                }
                backend.conversation_repository.import_asset_from_uri(
                    asset,
                    conversation.as_deref(),
                    file.uri().as_str(),
                )?;
                Ok::<_, MooseError>(())
            }
            .await;
            match result {
                Ok(()) => imported += 1,
                Err(error) => toast(&ui, &format!("{name}: {error}")),
            }
        }
        ui.attachments.busy.set(0);
        refresh(&ui, &backend);
        conversation_list::refresh(&ui, &backend);
        library::refresh_if_visible(&ui, &backend);
        if imported > 0 {
            toast(
                &ui,
                &if library_only {
                    ngettext_f(
                        "{count} file added to the library",
                        "{count} files added to the library",
                        imported as u32,
                        &[("count", &imported.to_string())],
                    )
                } else {
                    ngettext_f(
                        "{count} file attached to the draft",
                        "{count} files attached to the draft",
                        imported as u32,
                        &[("count", &imported.to_string())],
                    )
                },
            );
        }
    });
}

fn paste_image(ui: &Rc<WindowUi>, backend: &Rc<Backend>) {
    if ui.attachments.busy.get() > 0 {
        toast(ui, &gettext("Finish or cancel the current import first"));
        return;
    }
    let epoch = ui.attachments.import_epoch.get();
    let conversation = match draft_id(ui, backend) {
        Ok(id) => id,
        Err(error) => {
            toast(ui, &crate::i18n::error_message(&error));
            return;
        }
    };
    ui.attachments.busy.set(ui.attachments.busy.get() + 1);
    update_status(ui, backend);
    let ui = ui.clone();
    let backend = backend.clone();
    glib::MainContext::default().spawn_local(async move {
        let result = import_step(&ui, epoch, async {
            let bytes = clipboard::read_image(&ui.entry.clipboard()).await?;
            let asset = backend
                .runtime
                .spawn_blocking(move || import::image(gettext("Pasted image.png"), &bytes))
                .await
                .map_err(|_| error(gettext("The image could not be prepared")))??;
            if ui.attachments.import_epoch.get() != epoch {
                return Err(error(gettext("Import canceled")));
            }
            backend
                .conversation_repository
                .import_asset(asset, Some(&conversation))?;
            Ok::<_, MooseError>(())
        })
        .await;
        if let Err(error) = result {
            toast(&ui, &crate::i18n::error_message(&error));
        }
        ui.attachments
            .busy
            .set(ui.attachments.busy.get().saturating_sub(1));
        refresh(&ui, &backend);
        conversation_list::refresh(&ui, &backend);
        library::refresh_if_visible(&ui, &backend);
    });
}

pub(super) fn refresh(ui: &WindowUi, backend: &Backend) {
    while let Some(child) = ui.attachments.previews.first_child() {
        ui.attachments.previews.remove(&child);
    }
    let id = backend.active_conversation_id.borrow().clone();
    let assets = id
        .as_deref()
        .map(|id| backend.conversation_repository.draft_assets(id))
        .transpose();
    let assets = match assets {
        Ok(assets) => assets.unwrap_or_default(),
        Err(error) => {
            toast(ui, &crate::i18n::error_message(&error));
            Vec::new()
        }
    };
    ui.attachments.count.set(assets.len());
    ui.attachments
        .has_images
        .set(assets.iter().any(|asset| asset.kind == "image"));
    for asset in assets {
        let row = composer::asset_widget(ui, backend, &asset);
        let remove = widgets::icon_button(
            "window-close-symbolic",
            &gettext_f("Remove {arg1}", &[("arg1", &(asset.name).to_string())]),
        );
        remove.set_valign(gtk::Align::Center);
        remove.add_css_class("flat");
        remove.add_css_class("circular");
        remove.add_css_class("moose-composer-attachment-remove");
        let repository = backend.conversation_repository.clone();
        let conversation = id.clone().unwrap_or_default();
        let asset_id = asset.id;
        let weak_row = row.downgrade();
        let previews = ui.attachments.previews.clone();
        let overlay = ui.toast_overlay.clone();
        let entry = ui.entry.clone();
        remove.connect_clicked(move |_| {
            match repository.detach_from_draft(&conversation, &asset_id) {
                Ok(()) => {
                    entry.grab_focus();
                    if let Some(child) = weak_row.upgrade().and_then(|row| row.parent()) {
                        previews.remove(&child);
                    }
                    previews.set_visible(previews.first_child().is_some());
                    entry.buffer().emit_by_name::<()>("changed", &[]);
                }
                Err(error) => overlay.add_toast(adw::Toast::new(&gettext_f(
                    "Attachment could not be removed: {error}",
                    &[("error", &(error).to_string())],
                ))),
            }
        });
        row.append(&remove);
        append_card(&ui.attachments.previews, &row);
    }
    ui.attachments
        .previews
        .set_visible(ui.attachments.count.get() > 0);
    update_status(ui, backend);
}

pub(super) fn sync_counts(ui: &WindowUi, backend: &Backend) {
    let assets = backend
        .active_conversation_id
        .borrow()
        .as_deref()
        .and_then(|id| backend.conversation_repository.draft_assets(id).ok())
        .unwrap_or_default();
    ui.attachments.count.set(assets.len());
    ui.attachments
        .has_images
        .set(assets.iter().any(|asset| asset.kind == "image"));
    update_status(ui, backend);
}

fn update_status(ui: &WindowUi, backend: &Backend) {
    library::sync_import(ui);
    let count = ui.attachments.count.get();
    let tooltip = if count > 0 {
        gettext_f(
            "Add Attachments · {count} of {MAX_ATTACHMENTS}",
            &[
                ("count", &(count).to_string()),
                ("MAX_ATTACHMENTS", &(MAX_ATTACHMENTS).to_string()),
            ],
        )
    } else {
        gettext("Add Attachments")
    };
    ui.attachments.button.set_tooltip_text(Some(&tooltip));
    ui.attachments
        .cancel
        .set_visible(ui.attachments.busy.get() > 0);
    ui.attachments.cancel.set_sensitive(true);
    let library = backend
        .active_conversation_id
        .borrow()
        .as_deref()
        .is_some_and(|id| {
            backend
                .conversation_repository
                .library_enabled(id)
                .unwrap_or(false)
        });
    let status = gettext(if ui.attachments.busy.get() > 0 {
        gettext_noop("Preparing files…")
    } else if ui.attachments.has_images.get() {
        match *ui.attachments.vision.borrow() {
            Some(true) => "",
            Some(false) => {
                gettext_noop("Select a model with image support to send these attachments")
            }
            None if ui.attachments.capability_failed.get() => {
                gettext_noop("Image support could not be checked. Refresh the model list to retry.")
            }
            None => gettext_noop("Checking image support…"),
        }
    } else if library {
        gettext_noop("Library search is on · Relevant excerpts will be sent with your question")
    } else if ui.attachments.count.get() > 0 {
        gettext_noop("Documents are saved in your library")
    } else {
        ""
    });
    ui.attachments.status.set_label(&status);
    ui.attachments.status.set_visible(!status.is_empty());
    update_send_button(ui);
}

pub(super) fn can_send(ui: &WindowUi) -> bool {
    ui.attachments.busy.get() == 0
        && (!ui.attachments.has_images.get() || *ui.attachments.vision.borrow() == Some(true))
}

pub(super) fn invalidate_capabilities(ui: &WindowUi) {
    ui.attachments
        .revision
        .set(ui.attachments.revision.get().wrapping_add(1));
    ui.attachments.capabilities.borrow_mut().clear();
    *ui.attachments.vision.borrow_mut() = None;
    ui.attachments.capability_failed.set(false);
    ui.attachments.capability.set_visible(false);
}

pub(super) fn refresh_capabilities(ui: &Rc<WindowUi>, backend: &Rc<Backend>) {
    let revision = ui.attachments.revision.get() + 1;
    ui.attachments.revision.set(revision);
    *ui.attachments.vision.borrow_mut() = None;
    ui.attachments.capability_failed.set(false);
    ui.attachments.capability.set_visible(false);
    ui.model_picker
        .set_tooltip_text(Some(&gettext("Active Model")));
    let Some(provider) = active_provider(backend) else {
        update_status(ui, backend);
        return;
    };
    let Some(model) = selected_model(&ui.model_picker, &ui.model_names) else {
        update_status(ui, backend);
        return;
    };
    let key = (
        crate::providers::policy::destination(&provider),
        model.clone(),
    );
    let cached = ui.attachments.capabilities.borrow().get(&key).copied();
    if let Some(vision) = cached {
        *ui.attachments.vision.borrow_mut() = Some(vision);
        ui.attachments.capability.set_visible(vision);
        ui.model_picker.set_tooltip_text(Some(&gettext(if vision {
            gettext_noop("Active Model · Supports Images")
        } else {
            gettext_noop("Active Model · Text Only")
        })));
        update_status(ui, backend);
        return;
    }
    update_status(ui, backend);
    let (sender, receiver) = mpsc::channel();
    let policy = backend.network_policy.clone();
    backend.runtime.spawn(async move {
        let result = async {
            ProviderClient::new(provider, policy)
                .await?
                .supports_vision(&model)
                .await
        }
        .await;
        let _ = sender.send(result);
    });
    let ui = ui.clone();
    let backend = backend.clone();
    glib::timeout_add_local(Duration::from_millis(100), move || {
        if ui.attachments.revision.get() != revision {
            return glib::ControlFlow::Break;
        }
        match receiver.try_recv() {
            Ok(Ok(vision)) => {
                ui.attachments
                    .capabilities
                    .borrow_mut()
                    .insert(key.clone(), vision);
                *ui.attachments.vision.borrow_mut() = Some(vision);
                ui.attachments.capability.set_visible(vision);
                ui.model_picker.set_tooltip_text(Some(&gettext(if vision {
                    gettext_noop("Active Model · Supports Images")
                } else {
                    gettext_noop("Active Model · Text Only")
                })));
                update_status(&ui, &backend);
                glib::ControlFlow::Break
            }
            Ok(Err(_)) | Err(mpsc::TryRecvError::Disconnected) => {
                ui.attachments.capability_failed.set(true);
                update_status(&ui, &backend);
                glib::ControlFlow::Break
            }
            Err(mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
        }
    });
}

fn menu_item(icon: &str, title: &str) -> gtk::Button {
    let button = gtk::Button::new();
    button.add_css_class("flat");
    let content = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    content.append(&gtk::Image::from_icon_name(icon));
    content.append(
        &gtk::Label::builder()
            .label(title)
            .xalign(0.0)
            .hexpand(true)
            .build(),
    );
    button.set_child(Some(&content));
    button
}

fn attachment_grid() -> gtk::FlowBox {
    let grid = gtk::FlowBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .homogeneous(true)
        .min_children_per_line(1)
        .max_children_per_line(3)
        .column_spacing(8)
        .row_spacing(8)
        .hexpand(true)
        .build();
    grid.add_css_class("moose-attachment-grid");
    grid
}

fn append_card(grid: &gtk::FlowBox, card: &gtk::Box) {
    let child = gtk::FlowBoxChild::new();
    child.set_focusable(false);
    child.set_child(Some(card));
    grid.insert(&child, -1);
}

fn asset_widget(ui: &WindowUi, backend: &Backend, asset: &Asset) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    row.set_size_request(220, -1);
    row.add_css_class("moose-attachment");
    let button = gtk::Button::new();
    button.add_css_class("flat");
    button.add_css_class("moose-attachment-preview");
    button.set_hexpand(true);
    let content = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    content.set_size_request(-1, 40);
    if asset.kind == "image"
        && let Ok(bytes) = backend.conversation_repository.asset_bytes(&asset.id)
        && let Ok(texture) = gdk::Texture::from_bytes(&glib::Bytes::from_owned(bytes))
    {
        let thumbnail = gtk::Overlay::builder()
            .child(&gtk::Box::new(gtk::Orientation::Horizontal, 0))
            .width_request(40)
            .height_request(40)
            .valign(gtk::Align::Center)
            .build();
        thumbnail.add_css_class("moose-attachment-thumbnail");
        thumbnail.set_overflow(gtk::Overflow::Hidden);
        let picture = gtk::Picture::for_paintable(&texture);
        picture.set_content_fit(gtk::ContentFit::Cover);
        picture.set_can_shrink(true);
        picture.set_can_target(false);
        thumbnail.add_overlay(&picture);
        thumbnail.set_measure_overlay(&picture, false);
        content.append(&thumbnail);
    }
    let details = gtk::Box::new(gtk::Orientation::Vertical, 3);
    details.set_hexpand(true);
    details.set_valign(gtk::Align::Center);
    let label = gtk::Label::builder()
        .label(&asset.name)
        .xalign(0.0)
        .hexpand(true)
        .width_chars(1)
        .max_width_chars(24)
        .ellipsize(gtk::pango::EllipsizeMode::Middle)
        .build();
    label.add_css_class("moose-attachment-name");
    let size = super::model_actions::format_download_size(asset.byte_size.max(0) as u64);
    let kind = gettext(match asset.kind.as_str() {
        "image" => gettext_noop("Image"),
        "pdf" => "PDF",
        _ => gettext_noop("Text"),
    });
    let description = if asset.kind == "pdf" {
        ngettext_f(
            "{kind} · {count} page · {size}",
            "{kind} · {count} pages · {size}",
            asset.page_count as u32,
            &[
                ("kind", &kind),
                ("count", &asset.page_count.to_string()),
                ("size", &size),
            ],
        )
    } else {
        format!("{kind} · {size}")
    };
    let metadata = gtk::Label::builder()
        .label(&description)
        .xalign(0.0)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .build();
    metadata.add_css_class("caption");
    metadata.add_css_class("dim-label");
    details.append(&label);
    details.append(&metadata);
    content.append(&details);
    button.set_child(Some(&content));
    button.set_tooltip_text(Some(&gettext_f(
        "Preview {arg1}\n{description}",
        &[
            ("arg1", &(asset.name).to_string()),
            ("description", &(description).to_string()),
        ],
    )));
    let repository = backend.conversation_repository.clone();
    let parent = ui.window.clone();
    let asset = asset.clone();
    button.connect_clicked(move |_| library::preview(&parent, &repository, &asset, 1));
    row.append(&button);
    row
}

fn sent_image_widget(
    ui: &WindowUi,
    backend: &Backend,
    asset: &Asset,
    frame: Option<(i32, i32)>,
    assets: &[Asset],
) -> Result<gtk::Box> {
    let bytes = backend.conversation_repository.asset_bytes(&asset.id)?;
    let texture = gdk::Texture::from_bytes(&glib::Bytes::from_owned(bytes))
        .map_err(|_| error(gettext("This image could not be displayed")))?;
    let viewport = image::preview(&texture, &asset.name, frame);
    let button = gtk::Button::new();
    button.add_css_class("flat");
    button.add_css_class("moose-sent-image");
    button.set_halign(gtk::Align::Center);
    button.set_child(Some(&viewport));
    button.set_cursor_from_name(Some("zoom-in"));
    button.set_tooltip_text(Some(&gettext_f(
        "Open image: {arg1}",
        &[("arg1", &(asset.name).to_string())],
    )));
    let repository = backend.conversation_repository.clone();
    let parent = ui.window.clone();
    let asset = asset.clone();
    let images = assets
        .iter()
        .filter(|asset| asset.kind == "image")
        .cloned()
        .collect::<Vec<_>>();
    button.connect_clicked(move |_| library::preview_images(&parent, &repository, &asset, &images));
    let card = gtk::Box::new(gtk::Orientation::Vertical, 0);
    card.set_valign(gtk::Align::Start);
    card.append(&button);
    Ok(card)
}

fn append_message_assets(ui: &WindowUi, backend: &Backend, row: &gtk::Box, assets: &[Asset]) {
    let section = gtk::Box::new(gtk::Orientation::Vertical, 8);
    section.add_css_class("moose-sent-attachments");
    let outgoing = row.has_css_class("moose-message-outgoing");
    let alignment = if outgoing {
        gtk::Align::End
    } else {
        gtk::Align::Start
    };
    section.set_halign(alignment);
    let mut images = assets.iter().filter(|asset| asset.kind == "image");
    let image_count = images.clone().count();
    if image_count == 1
        && let Some(asset) = images.next()
    {
        let card = sent_image_widget(ui, backend, asset, None, assets)
            .unwrap_or_else(|_| asset_widget(ui, backend, asset));
        card.set_halign(alignment);
        section.append(&card);
    }
    if image_count > 1 {
        let gallery = attachment_grid();
        gallery.set_hexpand(false);
        gallery.set_halign(alignment);
        gallery.set_max_children_per_line(image_count.min(4) as u32);
        gallery.set_column_spacing(6);
        gallery.set_row_spacing(6);
        gallery.add_css_class("moose-image-gallery");
        for asset in images {
            let card = sent_image_widget(ui, backend, asset, Some((96, 72)), assets)
                .unwrap_or_else(|_| asset_widget(ui, backend, asset));
            append_card(&gallery, &card);
        }
        section.append(&gallery);
    }
    if let Some(documents) = document::list(ui, backend, assets) {
        documents.set_halign(alignment);
        section.append(&documents);
    }
    let mut child = row.first_child();
    while let Some(widget) = child {
        child = widget.next_sibling();
        if let Some(group) = widget
            .downcast_ref::<adw::Bin>()
            .and_then(|widget| widget.child())
            .and_downcast::<gtk::Box>()
            .filter(|widget| widget.has_css_class("moose-message-user-group"))
        {
            group.prepend(&section);
            return;
        }
    }
    row.append(&section);
}

pub(super) fn append_stored(
    ui: &WindowUi,
    backend: &Backend,
    row: &gtk::Box,
    message: &str,
) -> Result<()> {
    let assets = backend.conversation_repository.message_assets(message)?;
    if !assets.is_empty() {
        append_message_assets(ui, backend, row, &assets);
    }
    let sources = backend.conversation_repository.response_sources(message)?;
    if !sources.is_empty() {
        let content = gtk::Box::new(gtk::Orientation::Vertical, 4);
        for (index, source) in sources.iter().enumerate() {
            let button = gtk::Button::with_label(&gettext_f(
                "[{arg1}] {arg2} · Page {arg3}",
                &[
                    ("arg1", &(index + 1).to_string()),
                    ("arg2", &(source.name).to_string()),
                    ("arg3", &(source.page).to_string()),
                ],
            ));
            button.add_css_class("flat");
            if let Some(label) = button.child().and_downcast::<gtk::Label>() {
                label.set_wrap(true);
                label.set_xalign(0.0);
            }
            button.set_tooltip_text(Some(&source.content));
            let repository = backend.conversation_repository.clone();
            let parent = ui.window.clone();
            let target_source = source.clone();
            button.connect_clicked(move |_| {
                if let Ok(asset) = repository.asset(&target_source.asset_id) {
                    library::preview(&parent, &repository, &asset, target_source.page);
                }
            });
            content.append(&button);
            let excerpt = gtk::Label::builder()
                .label(&source.content)
                .wrap(true)
                .selectable(true)
                .xalign(0.0)
                .build();
            excerpt.add_css_class("caption");
            excerpt.set_margin_start(12);
            excerpt.set_margin_end(12);
            excerpt.set_margin_bottom(8);
            content.append(&excerpt);
        }
        let expander = gtk::Expander::builder()
            .label(gettext_f(
                "Sources ({arg1})",
                &[("arg1", &(sources.len()).to_string())],
            ))
            .child(&content)
            .build();
        expander.set_tooltip_text(Some(&gettext(
            "Excerpts provided to the model. Check these sources to verify its answer.",
        )));
        row.append(&expander);
    }
    Ok(())
}

fn toast(ui: &WindowUi, message: &str) {
    ui.toast_overlay.add_toast(adw::Toast::new(message));
}
