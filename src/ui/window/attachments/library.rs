use crate::i18n::{gettext, gettext_f, gettext_noop, ngettext_f};

use super::*;
use crate::attachments::FileEntry;

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
    let back = widgets::icon_button("go-previous-symbolic", &gettext("Back to Chat"));
    back.add_css_class("flat");
    back.add_css_class("moose-files-back");
    back.set_valign(gtk::Align::Center);
    let back_content = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    back_content.append(&gtk::Image::from_icon_name("go-previous-symbolic"));
    back_content.append(&gtk::Label::new(Some(&gettext("Chat"))));
    back.set_child(Some(&back_content));
    let titles = gtk::Box::new(gtk::Orientation::Vertical, 4);
    titles.set_hexpand(true);
    let title = gtk::Label::builder()
        .label(gettext("Files"))
        .xalign(0.0)
        .build();
    title.add_css_class("moose-files-title");
    let subtitle = gtk::Label::builder()
        .label(gettext("Files you upload and create in your chats."))
        .xalign(0.0)
        .wrap(true)
        .build();
    subtitle.add_css_class("dim-label");
    subtitle.add_css_class("moose-files-subtitle");
    titles.append(&title);
    titles.append(&subtitle);
    let add = gtk::Button::with_label(&gettext("Import…"));
    add.set_tooltip_text(Some(&gettext("Add files to your library")));
    add.set_valign(gtk::Align::Center);
    add.add_css_class("moose-files-import");
    let cancel = widgets::icon_button("process-stop-symbolic", &gettext("Cancel Import"));
    cancel.add_css_class("flat");
    cancel.set_valign(gtk::Align::Center);
    cancel.set_visible(false);
    let scope = adw::SwitchRow::builder()
        .title(gettext("Search Library in This Chat"))
        .subtitle(gettext("Include relevant text when you send a message."))
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
        .tooltip_text(gettext("Chat Library Settings"))
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
        .placeholder_text(gettext("Search files…"))
        .hexpand(true)
        .build();
    search.set_tooltip_text(Some(&gettext(
        "Search by file name or contents, or a generated document's chat",
    )));
    search.add_css_class("moose-files-search");
    content.append(&search);
    let filter_row = gtk::Box::new(gtk::Orientation::Horizontal, 4);
    filter_row.add_css_class("moose-files-filters");
    let mut filters = Vec::<gtk::ToggleButton>::new();
    for name in [
        gettext_noop("All"),
        gettext_noop("Library"),
        gettext_noop("Documents"),
        gettext_noop("Images"),
        gettext_noop("Generated"),
    ] {
        let filter = gtk::ToggleButton::with_label(&gettext(name));
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
        .min_children_per_line(2)
        .max_children_per_line(2)
        .column_spacing(18)
        .row_spacing(14)
        .valign(gtk::Align::Start)
        .hexpand(true)
        .build();
    grid.add_css_class("moose-files-grid");
    let empty = adw::StatusPage::builder()
        .icon_name("folder-documents-symbolic")
        .title(gettext("A Place for Your Files"))
        .description(gettext(
            "Import files or create a document in a chat. They will appear here automatically.",
        ))
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
    let more = gtk::Button::with_label(&gettext("Show More"));
    more.set_halign(gtk::Align::Center);
    more.add_css_class("flat");
    more.add_css_class("moose-files-more");
    let header_clamp = adw::Clamp::builder()
        .maximum_size(860)
        .tightening_threshold(650)
        .child(&content)
        .build();
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
    let page = gtk::Box::new(gtk::Orientation::Vertical, 0);
    page.append(&header_clamp);
    page.append(&clamp);
    let scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .child(&page)
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
                &gettext_f(
                    "Library setting could not be saved: {error}",
                    &[("error", &(error).to_string())],
                ),
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
        toast(
            ui,
            &gettext_f(
                "Draft could not be saved: {error}",
                &[("error", &(error).to_string())],
            ),
        );
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
    ui.files.add.set_label(&gettext(if busy {
        gettext_noop("Importing…")
    } else {
        gettext_noop("Import…")
    }));
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
        4 => "generated",
        _ => "all",
    };
    let limit = page.limit.get();
    let files =
        match backend
            .conversation_repository
            .files(page.search.text().as_str(), filter, limit + 1)
        {
            Ok(files) => files,
            Err(error) => {
                while let Some(row) = page.grid.first_child() {
                    page.grid.remove(&row);
                }
                page.grid.set_visible(false);
                page.more.set_visible(false);
                page.results.set_visible(false);
                page.empty.set_title(&gettext("Files Could Not Be Loaded"));
                page.empty
                    .set_description(Some(&gettext("Try opening Files again.")));
                page.empty.set_visible(true);
                toast(ui, &crate::i18n::error_message(&error));
                return;
            }
        };
    while let Some(row) = page.grid.first_child() {
        page.grid.remove(&row);
    }
    let searching = !page.search.text().trim().is_empty();
    let filtered = searching || filter != "all";
    page.empty
        .set_title(&gettext(if filter == "generated" && !searching {
            gettext_noop("No Generated Documents Yet")
        } else if filtered {
            gettext_noop("No Matching Files")
        } else {
            gettext_noop("A Place for Your Files")
        }));
    page.empty
        .set_description(Some(&gettext(if filter == "generated" && !searching {
            gettext_noop("Use Create Document in a chat. Your finished documents will appear here automatically.")
        } else if filtered {
            gettext_noop("Try another search or select All.")
        } else {
            gettext_noop("Import files or create a document in a chat. They will appear here automatically.")
        })));
    page.empty.set_visible(files.is_empty());
    page.grid.set_visible(!files.is_empty());
    page.more.set_visible(files.len() > limit);
    page.results.set_visible(!files.is_empty());
    let count = files.len().min(limit);
    page.results.set_label(&if files.len() > limit {
        ngettext_f(
            "Showing {count} file",
            "Showing {count} files",
            count as u32,
            &[("count", &count.to_string())],
        )
    } else {
        ngettext_f(
            "{count} file",
            "{count} files",
            count as u32,
            &[("count", &count.to_string())],
        )
    });
    for file in files.into_iter().take(limit) {
        let (filename, icon, kind, size) = match &file {
            FileEntry::Uploaded(asset) => (
                asset.name.clone(),
                if asset.kind == "image" {
                    "image-x-generic-symbolic"
                } else {
                    "text-x-generic-symbolic"
                },
                match asset.kind.as_str() {
                    "image" => gettext_noop("Image"),
                    "pdf" => "PDF",
                    _ => gettext_noop("Document"),
                },
                asset.byte_size,
            ),
            FileEntry::Generated { document, .. } => (
                document.filename(),
                document.format.icon(),
                document.format.label(),
                document.byte_size,
            ),
        };
        let content = gtk::Box::new(gtk::Orientation::Horizontal, 14);
        content.set_margin_end(28);
        let icon = gtk::Image::from_gicon(&gio::ThemedIcon::from_names(&[
            icon,
            "text-x-generic-symbolic",
        ]));
        icon.set_pixel_size(28);
        icon.set_halign(gtk::Align::Start);
        icon.set_valign(gtk::Align::Center);
        icon.add_css_class("moose-file-icon");
        let text = gtk::Box::new(gtk::Orientation::Vertical, 5);
        text.set_hexpand(true);
        text.set_valign(gtk::Align::Center);
        let name = gtk::Label::builder()
            .label(&filename)
            .xalign(0.0)
            .yalign(0.0)
            .wrap(true)
            .wrap_mode(gtk::pango::WrapMode::WordChar)
            .lines(2)
            .width_chars(1)
            .max_width_chars(36)
            .ellipsize(gtk::pango::EllipsizeMode::Middle)
            .build();
        name.add_css_class("moose-file-name");
        let details = gtk::Label::builder()
            .label(format!(
                "{} · {}",
                gettext(kind),
                super::super::model_actions::format_download_size(size.max(0) as u64),
            ))
            .xalign(0.0)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .build();
        details.add_css_class("moose-file-details");
        content.append(&icon);
        text.append(&name);
        text.append(&details);
        content.append(&text);
        let open = gtk::Button::builder()
            .child(&content)
            .hexpand(true)
            .vexpand(true)
            .tooltip_text(match &file {
                FileEntry::Generated {
                    conversation_title, ..
                } => gettext_f(
                    "{filename}\nGenerated in {conversation_title}\nPreview document",
                    &[
                        ("filename", &(filename).to_string()),
                        (
                            "conversation_title",
                            &crate::i18n::conversation_title(conversation_title),
                        ),
                    ],
                ),
                FileEntry::Uploaded(_) => gettext_f(
                    "{filename} — Open chat or preview",
                    &[("filename", &(filename).to_string())],
                ),
            })
            .build();
        open.add_css_class("moose-file-card");
        let menu = match file {
            FileEntry::Uploaded(asset) => {
                let menu = actions::menu(ui, backend, &asset);
                let target_ui = Rc::downgrade(ui);
                let target_backend = backend.clone();
                open.connect_clicked(move |_| {
                    if let Some(ui) = target_ui.upgrade() {
                        actions::open(&ui, &target_backend, &asset);
                    }
                });
                menu
            }
            FileEntry::Generated { document, .. } => {
                open.set_action_name(Some("message.preview-document"));
                open.set_action_target_value(Some(&document.id.to_variant()));
                generated_menu(&document)
            }
        };
        menu.set_halign(gtk::Align::End);
        menu.set_valign(gtk::Align::Center);
        menu.set_margin_end(8);
        let card = gtk::Overlay::builder().child(&open).build();
        card.add_css_class("moose-file-tile");
        card.add_overlay(&menu);
        let child = gtk::FlowBoxChild::new();
        child.set_focusable(false);
        child.set_child(Some(&card));
        page.grid.insert(&child, -1);
    }
}

fn generated_menu(document: &crate::documents::CreatedDocument) -> gtk::MenuButton {
    let content = gtk::Box::new(gtk::Orientation::Vertical, 2);
    content.add_css_class("moose-attachment-menu");
    let popover = gtk::Popover::builder().child(&content).build();
    let button = gtk::MenuButton::builder()
        .icon_name("view-more-symbolic")
        .tooltip_text(gettext_f(
            "Actions for {arg1}",
            &[("arg1", &(document.filename()).to_string())],
        ))
        .popover(&popover)
        .build();
    button.add_css_class("flat");
    button.add_css_class("moose-file-actions");
    for (label, icon, action) in [
        (
            gettext_noop("Preview"),
            "document-open-symbolic",
            "preview-document",
        ),
        (gettext_noop("Open"), "go-next-symbolic", "open-document"),
        (
            gettext_noop("Save As…"),
            "document-save-as-symbolic",
            "save-document",
        ),
        (
            gettext_noop("Open Chat"),
            "go-jump-symbolic",
            "document-chat",
        ),
    ] {
        let item = menu_item(icon, &gettext(label));
        item.set_action_target_value(Some(&document.id.to_variant()));
        item.set_action_name(Some(&format!("message.{action}")));
        let weak_popover = popover.downgrade();
        item.connect_clicked(move |_| {
            if let Some(popover) = weak_popover.upgrade() {
                popover.popdown();
            }
        });
        content.append(&item);
    }
    button
}

pub(super) fn preview(
    parent: &adw::ApplicationWindow,
    repository: &ConversationRepository,
    asset: &Asset,
    initial_page: i64,
) {
    preview_asset(parent, repository, asset, initial_page, &[]);
}

pub(super) fn preview_images(
    parent: &adw::ApplicationWindow,
    repository: &ConversationRepository,
    asset: &Asset,
    images: &[Asset],
) {
    preview_asset(parent, repository, asset, 1, images);
}

fn preview_asset(
    parent: &adw::ApplicationWindow,
    repository: &ConversationRepository,
    asset: &Asset,
    initial_page: i64,
    images: &[Asset],
) {
    let current_asset = Rc::new(RefCell::new(asset.clone()));
    let dialog = adw::Dialog::builder()
        .title(&asset.name)
        .content_width(760)
        .content_height(560)
        .build();
    let header = adw::HeaderBar::new();
    let save = gtk::Button::with_label(&gettext("Save Copy…"));
    header.pack_start(&save);
    let root = gtk::Box::new(gtk::Orientation::Vertical, 8);
    if asset.kind == "image" {
        match repository.asset_bytes(&asset.id).and_then(|bytes| {
            gdk::Texture::from_bytes(&glib::Bytes::from_owned(bytes))
                .map_err(|_| error(gettext("This image could not be displayed")))
        }) {
            Ok(texture) => {
                dialog.set_content_width(960);
                dialog.set_content_height(720);
                let picture = gtk::Picture::for_paintable(&texture);
                picture.set_content_fit(gtk::ContentFit::ScaleDown);
                picture.set_can_shrink(true);
                picture.set_alternative_text(Some(&asset.name));
                picture.set_hexpand(true);
                picture.set_vexpand(true);
                let scroll = gtk::ScrolledWindow::builder()
                    .hscrollbar_policy(gtk::PolicyType::Never)
                    .vscrollbar_policy(gtk::PolicyType::Never)
                    .hexpand(true)
                    .vexpand(true)
                    .child(&picture)
                    .build();
                let fit = gtk::ToggleButton::builder()
                    .icon_name("zoom-fit-best-symbolic")
                    .tooltip_text(gettext("Fit Image to Window"))
                    .active(true)
                    .build();
                let actual = gtk::ToggleButton::builder()
                    .label("100%")
                    .tooltip_text(gettext("View at Actual Size"))
                    .group(&fit)
                    .build();
                let controls = gtk::Box::new(gtk::Orientation::Horizontal, 0);
                controls.add_css_class("linked");
                controls.append(&fit);
                controls.append(&actual);
                header.pack_end(&controls);
                let image_stack = gtk::Stack::builder().vexpand(true).build();
                image_stack.add_named(&scroll, Some("image"));
                let failed = gtk::Label::builder()
                    .label(gettext("This image could not be displayed."))
                    .wrap(true)
                    .margin_start(24)
                    .margin_end(24)
                    .build();
                image_stack.add_named(&failed, Some("error"));
                root.append(&image_stack);
                if let Some(initial) = images
                    .iter()
                    .position(|image| image.id == asset.id)
                    .filter(|_| images.len() > 1)
                {
                    let previous =
                        widgets::icon_button("go-previous-symbolic", &gettext("Previous Image"));
                    let next = widgets::icon_button("go-next-symbolic", &gettext("Next Image"));
                    previous.set_sensitive(initial > 0);
                    next.set_sensitive(initial + 1 < images.len());
                    let counter = gtk::Label::builder()
                        .label(gettext_f(
                            "{current} of {total}",
                            &[
                                ("current", &(initial + 1).to_string()),
                                ("total", &images.len().to_string()),
                            ],
                        ))
                        .width_chars(7)
                        .css_classes(["dim-label"])
                        .build();
                    let navigation = gtk::Box::builder()
                        .orientation(gtk::Orientation::Horizontal)
                        .spacing(8)
                        .halign(gtk::Align::Center)
                        .margin_bottom(12)
                        .build();
                    navigation.append(&previous);
                    navigation.append(&counter);
                    navigation.append(&next);
                    root.append(&navigation);
                    let index = Cell::new(initial);
                    let images = images.to_vec();
                    let repository = repository.clone();
                    let selected = current_asset.clone();
                    let weak_dialog = dialog.downgrade();
                    let weak_previous = previous.downgrade();
                    let weak_next = next.downgrade();
                    let target_picture = picture.clone();
                    let target_scroll = scroll.clone();
                    let target_save = save.clone();
                    let navigate: Rc<dyn Fn(isize)> = Rc::new(move |step| {
                        let Some(target) = index
                            .get()
                            .checked_add_signed(step)
                            .filter(|target| *target < images.len())
                        else {
                            return;
                        };
                        let image = &images[target];
                        let texture = repository.asset_bytes(&image.id).and_then(|bytes| {
                            gdk::Texture::from_bytes(&glib::Bytes::from_owned(bytes))
                                .map_err(|_| error(gettext("This image could not be displayed")))
                        });
                        let available = texture.is_ok();
                        match texture {
                            Ok(texture) => {
                                target_picture.set_paintable(Some(&texture));
                                image_stack.set_visible_child_name("image");
                            }
                            Err(_) => {
                                target_picture.set_paintable(None::<&gdk::Texture>);
                                image_stack.set_visible_child_name("error");
                            }
                        }
                        target_picture.set_alternative_text(Some(&image.name));
                        fit.set_active(true);
                        target_scroll.hadjustment().set_value(0.0);
                        target_scroll.vadjustment().set_value(0.0);
                        target_save.set_sensitive(available);
                        controls.set_sensitive(available);
                        if let Some(dialog) = weak_dialog.upgrade() {
                            dialog.set_title(&image.name);
                        }
                        counter.set_label(&gettext_f(
                            "{current} of {total}",
                            &[
                                ("current", &(target + 1).to_string()),
                                ("total", &images.len().to_string()),
                            ],
                        ));
                        if let Some(previous) = weak_previous.upgrade() {
                            previous.set_sensitive(target > 0);
                        }
                        if let Some(next) = weak_next.upgrade() {
                            next.set_sensitive(target + 1 < images.len());
                        }
                        selected.replace(image.clone());
                        index.set(target);
                    });
                    for (button, step) in [(&previous, -1), (&next, 1)] {
                        let navigate = navigate.clone();
                        button.connect_clicked(move |_| navigate(step));
                    }
                    let keys = gtk::EventControllerKey::builder()
                        .propagation_phase(gtk::PropagationPhase::Capture)
                        .build();
                    keys.connect_key_pressed(move |_, key, _, modifiers| {
                        if modifiers.intersects(
                            gdk::ModifierType::CONTROL_MASK
                                | gdk::ModifierType::ALT_MASK
                                | gdk::ModifierType::SUPER_MASK
                                | gdk::ModifierType::SHIFT_MASK,
                        ) {
                            return glib::Propagation::Proceed;
                        }
                        let step = match key {
                            gdk::Key::Left => -1,
                            gdk::Key::Right => 1,
                            _ => return glib::Propagation::Proceed,
                        };
                        navigate(step);
                        glib::Propagation::Stop
                    });
                    dialog.add_controller(keys);
                }
                actual.connect_toggled(move |button| {
                    picture.set_can_shrink(!button.is_active());
                    let policy = if button.is_active() {
                        gtk::PolicyType::Automatic
                    } else {
                        gtk::PolicyType::Never
                    };
                    scroll.set_policy(policy, policy);
                });
            }
            Err(error) => root.append(&gtk::Label::new(Some(&crate::i18n::error_message(&error)))),
        }
    } else {
        let page = gtk::SpinButton::with_range(1.0, asset.page_count.max(1) as f64, 1.0);
        page.set_value(initial_page.clamp(1, asset.page_count.max(1)) as f64);
        let page_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        page_row.set_halign(gtk::Align::Center);
        page_row.append(&gtk::Label::new(Some(&gettext("Page"))));
        page_row.append(&page);
        page_row.append(&gtk::Label::new(Some(&gettext_f(
            "of {total}",
            &[("total", &asset.page_count.to_string())],
        ))));
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
                buffer.set_text(&gettext("No selectable text on this page."))
            }
            Ok(text) => buffer.set_text(&text),
            Err(error) => buffer.set_text(&gettext_f(
                "Page could not be loaded: {error}",
                &[("error", &(error).to_string())],
            )),
        };
        load(page.value_as_int() as i64);
        page.connect_value_changed(move |page| load(page.value_as_int() as i64));
        if asset.kind == "pdf" {
            let hint = gtk::Label::new(Some(&gettext(
                "Extracted text · Original layout is available in the saved PDF",
            )));
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
    let parent_window = parent.clone();
    save.connect_clicked(move |_| {
        let asset = current_asset.borrow().clone();
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
            .title(gettext("Save Attachment"))
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
                    .map_err(|(_, error)| crate::i18n::error_message(&error)),
                Err(error) => Err(crate::i18n::error_message(&error)),
            };
            if let Err(error) = result {
                let dialog = adw::AlertDialog::builder()
                    .heading(gettext("File Could Not Be Saved"))
                    .body(&error)
                    .build();
                dialog.add_response("close", &gettext("Close"));
                dialog.present(Some(&parent));
            }
        });
    });
    dialog.present(Some(parent));
}
