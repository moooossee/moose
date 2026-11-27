use super::*;
use crate::attachments::{
    ExtractedPage, NumericReview,
    ocr::{Job, PageTransform, PdfMode, valid_languages},
};

pub(super) async fn process<T: Send + 'static>(
    ui: &WindowUi,
    backend: &Backend,
    language: String,
    task: impl FnOnce(Job) -> Result<T> + Send + 'static,
) -> Result<T> {
    let job = Job::new(language, backend.paths.ocr_models_dir());
    let epoch = ui.attachments.import_epoch.get();
    ui.attachments.job.replace(Some(job.clone()));
    let worker_job = job.clone();
    let worker = backend.runtime.spawn_blocking(move || task(worker_job));
    while !worker.is_finished() {
        if ui.attachments.import_epoch.get() != epoch {
            job.cancel();
        }
        update_status(ui, backend);
        glib::timeout_future(Duration::from_millis(100)).await;
    }
    let result = worker
        .await
        .map_err(|_| error(gettext("This file could not be processed")));
    ui.attachments.job.take();
    job.check()?;
    result?
}

pub(super) fn bind(ui: &Rc<WindowUi>, backend: &Rc<Backend>) {
    let recognize = gio::SimpleAction::new("recognize-text", Some(glib::VariantTy::STRING));
    let target_ui = Rc::downgrade(ui);
    let target_backend = backend.clone();
    recognize.connect_activate(move |_, parameter| {
        let Some(ui) = target_ui.upgrade() else {
            return;
        };
        let Some(id) = parameter.and_then(|value| value.get::<String>()) else {
            return;
        };
        match target_backend.conversation_repository.asset(&id) {
            Ok(asset) => show(&ui, &target_backend, &asset),
            Err(error) => toast(&ui, &crate::i18n::error_message(&error)),
        }
    });
    ui.window.add_action(&recognize);
    let mode = gio::SimpleAction::new(
        "use-image-text",
        Some(<(String, bool)>::static_variant_type().as_ref()),
    );
    let target_ui = Rc::downgrade(ui);
    let target_backend = backend.clone();
    mode.connect_activate(move |_, parameter| {
        let Some(ui) = target_ui.upgrade() else {
            return;
        };
        let Some((id, enabled)) = parameter.and_then(|value| value.get::<(String, bool)>()) else {
            return;
        };
        let conversation = target_backend.active_conversation_id.borrow().clone();
        if let Some(conversation) = conversation {
            match target_backend.conversation_repository.set_draft_text_only(
                &conversation,
                &id,
                enabled,
            ) {
                Ok(()) => refresh(&ui, &target_backend),
                Err(error) => toast(&ui, &crate::i18n::error_message(&error)),
            }
        }
    });
    ui.window.add_action(&mode);
    let target_ui = Rc::downgrade(ui);
    ui.window.connect_close_request(move |_| {
        if let Some(ui) = target_ui.upgrade() {
            cancel_import(&ui);
            language_manager::cancel(&ui);
        }
        glib::Propagation::Proceed
    });
}

pub(super) fn button(asset: &Asset) -> gtk::Button {
    let button = widgets::icon_button("insert-text-symbolic", &gettext("Recognize Text…"));
    button.add_css_class("flat");
    button.set_valign(gtk::Align::Center);
    button.set_action_name(Some("win.recognize-text"));
    button.set_action_target_value(Some(&asset.id.to_variant()));
    button
}

pub(super) fn page_details(
    repository: &ConversationRepository,
    asset: &Asset,
    page: i64,
) -> gtk::Box {
    let details = gtk::Box::new(gtk::Orientation::Vertical, 4);
    let info = repository.asset_page_info(&asset.id, page);
    let status = match info.as_ref() {
        Ok(Some(info)) if !info.error.is_empty() => {
            gettext_f("Recognition incomplete: {error}", &[("error", &info.error)])
        }
        Ok(Some(info)) if info.method.starts_with("tesseract") => match info.confidence {
            Some(confidence) => gettext_f(
                "Recognized text · {language} · OCR score {confidence}/100",
                &[
                    ("language", &info.language),
                    ("confidence", &format!("{confidence:.0}")),
                ],
            ),
            None => gettext_f(
                "Recognized text · {language}",
                &[("language", &info.language)],
            ),
        },
        Err(failure) => crate::i18n::error_message(failure),
        _ => gettext("Extracted text"),
    };
    let label = gtk::Label::builder()
        .label(status)
        .wrap(true)
        .css_classes(["caption", "dim-label"])
        .build();
    details.append(&label);
    if let Ok(Some(info)) = info
        && !info.review.fields.is_empty()
    {
        details.append(&review_button(repository, asset, page, info.review));
    }
    details
}

fn review_button(
    repository: &ConversationRepository,
    asset: &Asset,
    page: i64,
    review: NumericReview,
) -> gtk::MenuButton {
    let list = gtk::Box::new(gtk::Orientation::Vertical, 4);
    let choices = gtk::Box::new(gtk::Orientation::Vertical, 2);
    list.append(&choices);
    let picture = gtk::Picture::builder()
        .width_request(240)
        .height_request(72)
        .visible(false)
        .build();
    list.append(&picture);
    let message = gtk::Label::builder().wrap(true).visible(false).build();
    list.append(&message);
    for field in review.fields {
        let title = if let Some(alternative) = &field.alternative {
            gettext_f(
                "{value} / {alternative} · OCR passes disagree",
                &[("value", &field.text), ("alternative", alternative)],
            )
        } else {
            gettext_f(
                "{value} · OCR score {score}/100",
                &[("value", &field.text), ("score", &field.score.to_string())],
            )
        };
        let button = gtk::Button::with_label(&title);
        choices.append(&button);
        let repository = repository.clone();
        let asset = asset.clone();
        let picture = picture.clone();
        let message = message.clone();
        let choices = choices.downgrade();
        button.connect_clicked(move |_| {
            let Some(choices) = choices.upgrade() else {
                return;
            };
            let bytes = repository.asset_original_bytes(&asset.id);
            let image = asset.kind == "image";
            let tiff = asset.mime_type == "image/tiff";
            let bounds = field.bounds;
            let dimensions = review.dimensions;
            let message = message.clone();
            let picture = picture.clone();
            choices.set_sensitive(false);
            picture.set_visible(false);
            message.set_label(&gettext("Loading source region…"));
            message.set_visible(true);
            glib::spawn_future_local(async move {
                let result = gio::spawn_blocking(move || {
                    let job = Job::new(String::new(), Default::default());
                    source_region(&bytes?, image, tiff, page, dimensions, bounds, &job)
                })
                .await
                .ok()
                .and_then(Result::ok)
                .and_then(|png| gdk::Texture::from_bytes(&glib::Bytes::from_owned(png)).ok());
                choices.set_sensitive(true);
                message.set_visible(result.is_none());
                if let Some(texture) = result {
                    picture.set_paintable(Some(&texture));
                    picture.set_visible(true);
                } else {
                    message.set_label(&gettext("This region could not be displayed"));
                }
            });
        });
    }
    gtk::MenuButton::builder()
        .label(gettext("Review uncertain numbers"))
        .halign(gtk::Align::Center)
        .popover(&gtk::Popover::builder().child(&list).build())
        .build()
}

fn source_region(
    bytes: &[u8],
    image: bool,
    tiff: bool,
    page: i64,
    [width, height]: [u32; 2],
    [x, y, w, h]: [u32; 4],
    job: &Job,
) -> Result<Vec<u8>> {
    let unavailable = || error(gettext("This region is unavailable"));
    if w == 0 || h == 0 || x.saturating_add(w) > width || y.saturating_add(h) > height {
        return Err(unavailable());
    }
    let pad = h.div_ceil(4);
    let left = x.saturating_sub(pad);
    let top = y.saturating_sub(pad);
    let right = x.saturating_add(w).saturating_add(pad).min(width);
    let bottom = y.saturating_add(h).saturating_add(pad).min(height);
    if image {
        let source = import::prepared_image(bytes, job)?;
        if [source.width() as u32, source.height() as u32] != [width, height] {
            return Err(unavailable());
        }
        source
            .new_subpixbuf(
                left as i32,
                top as i32,
                (right - left) as i32,
                (bottom - top) as i32,
            )
            .save_to_bufferv("png", &[])
            .map_err(|_| unavailable())
    } else {
        ocr::page_region(
            bytes,
            tiff,
            page,
            width.max(height),
            [left, top, right - left, bottom - top],
            &glib::user_cache_dir(),
            job,
        )
    }
}

pub(super) fn preview_text(
    parent: &adw::ApplicationWindow,
    repository: &ConversationRepository,
    asset: &Asset,
) {
    let dialog = adw::Dialog::builder()
        .title(&asset.name)
        .content_width(760)
        .content_height(560)
        .build();
    let root = gtk::Box::new(gtk::Orientation::Vertical, 8);
    root.append(&page_details(repository, asset, 1));
    let text = repository
        .asset_page(&asset.id, 1)
        .unwrap_or_else(|failure| crate::i18n::error_message(&failure));
    let view = gtk::TextView::builder()
        .editable(false)
        .wrap_mode(gtk::WrapMode::WordChar)
        .left_margin(20)
        .right_margin(20)
        .top_margin(12)
        .bottom_margin(16)
        .vexpand(true)
        .build();
    let text = if text.trim().is_empty() {
        gettext("No readable text was found")
    } else {
        text
    };
    view.buffer().set_text(&text);
    root.append(
        &gtk::ScrolledWindow::builder()
            .child(&view)
            .vexpand(true)
            .build(),
    );
    let toolbar = adw::ToolbarView::builder().content(&root).build();
    toolbar.add_top_bar(&adw::HeaderBar::new());
    dialog.set_child(Some(&toolbar));
    dialog.present(Some(parent));
}

pub(super) fn show(ui: &Rc<WindowUi>, backend: &Rc<Backend>, asset: &Asset) {
    if ui.attachments.busy.get() > 0 {
        toast(ui, &gettext("Finish or cancel the current import first"));
        return;
    }
    if !asset.supports_ocr() {
        return;
    }
    library::set_languages(ui, backend, &library::languages(ui));
    let content = gtk::Box::new(gtk::Orientation::Vertical, 12);
    let group = adw::PreferencesGroup::new();
    let file = adw::ActionRow::builder()
        .title(gettext("File"))
        .subtitle(&asset.name)
        .subtitle_lines(2)
        .use_markup(false)
        .build();
    file.add_prefix(&gtk::Image::from_icon_name(if asset.kind == "image" {
        "image-x-generic-symbolic"
    } else if asset.kind == "pdf" {
        "application-pdf-symbolic"
    } else {
        "x-office-document-symbolic"
    }));
    group.add(&file);
    let languages = library::language_summary(ui);
    group.add(&languages);
    content.append(&group);
    let labels = [
        gettext("Automatic"),
        gettext("Rotate 90° clockwise"),
        gettext("Rotate 180°"),
        gettext("Rotate 90° counterclockwise"),
        gettext("Flip horizontally"),
        gettext("Flip vertically"),
    ];
    let orientation = adw::ComboRow::builder()
        .title(gettext("Orientation"))
        .model(&gtk::StringList::new(
            &labels.each_ref().map(String::as_str),
        ))
        .build();
    group.add(&orientation);
    let options = adw::PreferencesGroup::new();
    let all = adw::SwitchRow::builder()
        .title(gettext("All Pages"))
        .active(true)
        .build();
    let page = adw::SpinRow::with_range(1.0, asset.page_count.max(1) as f64, 1.0);
    page.set_title(&gettext("Page"));
    page.set_numeric(true);
    page.set_snap_to_ticks(true);
    if asset.is_paged_document() && asset.page_count > 1 {
        options.add(&all);
        let requested_pages = all.downgrade();
        orientation.connect_selected_notify(move |row| {
            if row.selected() != 0
                && let Some(all) = requested_pages.upgrade()
            {
                all.set_active(false);
            }
        });
        all.bind_property("active", &page, "sensitive")
            .invert_boolean()
            .sync_create()
            .build();
        options.add(&page);
        content.append(&options);
    }
    let use_text = adw::SwitchRow::builder()
        .title(gettext("Use Recognized Text"))
        .subtitle(gettext("Send the extracted text with the current draft."))
        .subtitle_lines(2)
        .title_lines(2)
        .active(true)
        .build();
    if asset.kind == "image" {
        options.add(&use_text);
        content.append(&options);
    }
    let dialog = adw::AlertDialog::builder()
        .heading(gettext("Recognize Text"))
        .body(gettext(
            "Extract text on this device using the languages selected in Preferences.",
        ))
        .extra_child(&content)
        .close_response("cancel")
        .default_response("recognize")
        .build();
    dialog.add_response("cancel", &gettext("Cancel"));
    dialog.add_response("recognize", &gettext("Recognize"));
    dialog.set_response_appearance("recognize", adw::ResponseAppearance::Suggested);
    dialog.set_response_enabled("recognize", valid_languages(&library::languages(ui)));
    let weak_dialog = dialog.downgrade();
    let target_ui = Rc::downgrade(ui);
    languages.connect_subtitle_notify(move |_| {
        if let Some(dialog) = weak_dialog.upgrade()
            && let Some(ui) = target_ui.upgrade()
        {
            dialog.set_response_enabled("recognize", valid_languages(&library::languages(&ui)));
        }
    });
    let target_ui = Rc::downgrade(ui);
    let backend = backend.clone();
    let asset = asset.clone();
    dialog.connect_response(Some("recognize"), move |_, _| {
        let Some(ui) = target_ui.upgrade() else {
            return;
        };
        if ui.attachments.busy.get() > 0 {
            toast(&ui, &gettext("Finish or cancel the current import first"));
            return;
        }
        page.update();
        let selected =
            (asset.is_paged_document() && !all.is_active()).then(|| page.value().round() as usize);
        let language = library::languages(&ui);
        let transform = match orientation.selected() {
            1 => Some(PageTransform::Clockwise),
            2 => Some(PageTransform::UpsideDown),
            3 => Some(PageTransform::Counterclockwise),
            4 => Some(PageTransform::MirrorHorizontal),
            5 => Some(PageTransform::MirrorVertical),
            _ => None,
        };
        start(
            &ui,
            &backend,
            asset.clone(),
            language,
            selected,
            use_text.is_active(),
            transform,
        );
    });
    dialog.present(Some(&ui.window));
}

fn start(
    ui: &Rc<WindowUi>,
    backend: &Rc<Backend>,
    asset: Asset,
    language: String,
    selected: Option<usize>,
    use_text: bool,
    transform: Option<PageTransform>,
) {
    ui.attachments.busy.set(1);
    update_status(ui, backend);
    let ui = ui.clone();
    let backend = backend.clone();
    let conversation = backend.active_conversation_id.borrow().clone();
    glib::MainContext::default().spawn_local(async move {
        let result = async {
            let bytes = backend
                .conversation_repository
                .asset_original_bytes(&asset.id)?;
            let cache = backend.paths.cache_dir().to_path_buf();
            let image = asset.kind == "image";
            let tiff = asset.mime_type == "image/tiff";
            let mut pages: Vec<ExtractedPage> = process(&ui, &backend, language, move |job| {
                if image {
                    let page = match transform {
                        Some(transform) => {
                            import::recognize_image_oriented(&bytes, &cache, &job, Some(transform))
                        }
                        None => import::recognize_image(&bytes, &cache, &job),
                    }?;
                    Ok(vec![page])
                } else {
                    let mode = transform.map_or(PdfMode::Recognize(selected), |orientation| {
                        PdfMode::RecognizeOriented(selected, orientation)
                    });
                    if tiff {
                        ocr::tiff(&bytes, &cache, &job, mode)
                    } else {
                        ocr::pdf(&bytes, &cache, &job, mode)
                    }
                }
            })
            .await?;
            for (index, page) in pages.iter_mut().enumerate() {
                if selected.is_some_and(|selected| selected != index + 1)
                    || (page.error.is_empty() && !page.text.trim().is_empty())
                {
                    continue;
                }
                let previous = backend
                    .conversation_repository
                    .asset_page(&asset.id, index as i64 + 1)?;
                if page.error.is_empty() && !previous.trim().is_empty() {
                    page.error = gettext("No readable text was found. Previous text was kept.");
                }
                page.text = previous;
                if let Some(previous) = backend
                    .conversation_repository
                    .asset_page_info(&asset.id, index as i64 + 1)?
                {
                    page.method = previous.method;
                    page.language = previous.language;
                    page.confidence = previous.confidence;
                    page.review = previous.review;
                }
            }
            backend
                .conversation_repository
                .replace_asset_pages(&asset.id, &pages, selected)?;
            let recognized = backend.conversation_repository.asset(&asset.id)?;
            if image
                && use_text
                && recognized.has_text
                && let Some(conversation) = conversation
            {
                backend.conversation_repository.set_draft_text_only(
                    &conversation,
                    &asset.id,
                    true,
                )?;
            }
            if let Some(page) = pages.iter().find(|page| !page.error.is_empty()) {
                toast(&ui, &page.error);
            } else {
                toast(
                    &ui,
                    &gettext(if recognized.has_text {
                        gettext_noop("Recognized text is ready")
                    } else {
                        gettext_noop("No readable text was found")
                    }),
                );
            }
            Ok::<_, MooseError>(())
        }
        .await;
        if let Err(error) = result {
            toast(&ui, &crate::i18n::error_message(&error));
        }
        ui.attachments.busy.set(0);
        refresh(&ui, &backend);
        library::refresh_if_visible(&ui, &backend);
    });
}
