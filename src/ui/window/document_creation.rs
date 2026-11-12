use super::*;
use crate::{
    conversations::{Message, MessageRole, MessageStatus},
    documents::{CreatedDocument, DocumentFormat, DocumentJob, DocumentRequest, content::Content},
};
use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};

mod pdf;

pub(super) fn bind(ui: &Rc<WindowUi>, backend: &Rc<Backend>) {
    let weak_ui = Rc::downgrade(ui);
    let target_backend = backend.clone();
    ui.create_document_button.connect_clicked(move |_| {
        if let Some(ui) = weak_ui.upgrade() {
            show_dialog(&ui, &target_backend, None);
        }
    });
    let action = gio::SimpleAction::new("create-document", Some(gtk::glib::VariantTy::STRING));
    let weak_ui = Rc::downgrade(ui);
    let target_backend = backend.clone();
    action.connect_activate(move |_, parameter| {
        if let (Some(ui), Some(message)) =
            (weak_ui.upgrade(), parameter.and_then(|value| value.str()))
        {
            show_dialog(&ui, &target_backend, Some(message.to_string()));
        }
    });
    ui.message_action_group.add_action(&action);
    for name in [
        "create-file",
        "preview-document",
        "open-document",
        "save-document",
    ] {
        let action = gio::SimpleAction::new(name, Some(gtk::glib::VariantTy::STRING));
        let weak_ui = Rc::downgrade(ui);
        let target_backend = backend.clone();
        action.connect_activate(move |_, parameter| {
            let (Some(ui), Some(id)) = (weak_ui.upgrade(), parameter.and_then(|value| value.str()))
            else {
                return;
            };
            if name == "create-file" {
                generate(&ui, &target_backend, id, None);
                return;
            }
            let document = match target_backend.conversation_repository.created_document(id) {
                Ok(Some(document)) => document,
                _ => {
                    toast(&ui, "This document is no longer available.");
                    return;
                }
            };
            match name {
                "preview-document" => preview(&ui, &target_backend, &document),
                "open-document" => open(&ui, &target_backend, &document),
                _ => save(&ui, &target_backend, &document),
            }
        });
        ui.message_action_group.add_action(&action);
    }
    sync_controls(ui, backend);
}

pub(super) fn sync_controls(ui: &WindowUi, backend: &Backend) {
    let running = backend.active_generation.borrow().is_some();
    ui.create_document_button
        .set_sensitive(!running && active_provider(backend).is_some());
    ui.create_document_button.set_tooltip_text(Some(if running {
        "Finish or stop the current response to create a document"
    } else {
        "Create a document, spreadsheet, or presentation"
    }));
}

pub(super) fn prepare_request(
    backend: &Backend,
    submission: &message_actions::Submission,
    prompt: &str,
    history: &[Message],
    explicit: Option<DocumentRequest>,
) -> Result<Option<DocumentJob>> {
    if let Some(request) = explicit {
        return Ok(Some(job(request)));
    }
    match submission {
        message_actions::Submission::Regenerate(id) => {
            return Ok(backend
                .conversation_repository
                .document_job(id)?
                .map(|mut job| {
                    job.content.clear();
                    job.state = "waiting".into();
                    job.error.clear();
                    job
                }));
        }
        message_actions::Submission::Continue(id) => {
            return backend.conversation_repository.document_job(id);
        }
        _ => {}
    }
    if let Some(request) = DocumentRequest::from_prompt(prompt) {
        return Ok(Some(job(request)));
    }
    let lower = prompt.trim().to_lowercase();
    let revision = [
        "update ",
        "change ",
        "revise ",
        "rewrite ",
        "add ",
        "remove ",
        "translate ",
        "shorten ",
        "modify ",
        "actualiza ",
        "cambia ",
        "modifica ",
        "añade ",
        "agrega ",
        "quita ",
        "elimina ",
        "traduce ",
        "mejora ",
        "acorta ",
    ]
    .iter()
    .any(|prefix| lower.starts_with(prefix));
    if revision
        && let Some(previous) = history
            .iter()
            .rev()
            .find(|message| message.role == MessageRole::Assistant)
    {
        return Ok(backend
            .conversation_repository
            .document_job(&previous.id)?
            .map(|previous| job(previous.request)));
    }
    Ok(None)
}

fn job(request: DocumentRequest) -> DocumentJob {
    DocumentJob {
        request,
        content: String::new(),
        state: "waiting".into(),
        error: String::new(),
    }
}

fn show_dialog(ui: &Rc<WindowUi>, backend: &Rc<Backend>, message: Option<String>) {
    if message.is_none() {
        if require_active_provider(ui, backend).is_none() {
            return;
        }
        if backend.active_generation.borrow().is_some() {
            toast(
                ui,
                "Finish or stop the current response to create a document.",
            );
            return;
        }
    }
    let original = match &message {
        Some(id) => match backend.conversation_repository.get_message(id) {
            Ok(Some(message)) => Some(message),
            _ => {
                toast(ui, "This response is no longer available.");
                return;
            }
        },
        None => None,
    };
    let dialog = adw::Dialog::builder()
        .title("Create Document")
        .content_width(580)
        .content_height(if message.is_some() { 340 } else { 620 })
        .build();
    let header = adw::HeaderBar::builder()
        .show_start_title_buttons(false)
        .show_end_title_buttons(false)
        .build();
    let cancel = gtk::Button::with_label("Cancel");
    let create = gtk::Button::with_label("Create");
    create.add_css_class("suggested-action");
    header.pack_start(&cancel);
    header.pack_end(&create);
    let page = adw::PreferencesPage::new();
    let group = adw::PreferencesGroup::builder().description(if message.is_some() { "Turn this response into a file you can open and save." } else { "Describe what you need. Moose will write the content and create a file you can open and save." }).build();
    let title = adw::EntryRow::builder()
        .title("Document Name")
        .text("Untitled Document")
        .build();
    let formats = DocumentFormat::ALL
        .iter()
        .map(|format| format!("{} (.{})", format.label(), format.extension()))
        .collect::<Vec<_>>();
    let names = formats.iter().map(String::as_str).collect::<Vec<_>>();
    let format = adw::ComboRow::builder()
        .title("File Format")
        .model(&gtk::StringList::new(&names))
        .selected(0)
        .build();
    if let Some(original) = &original
        && let Ok(Some(previous)) = backend.conversation_repository.document_job(&original.id)
    {
        title.set_text(&previous.request.title);
        if let Some(index) = DocumentFormat::ALL
            .iter()
            .position(|format| *format == previous.request.format)
        {
            format.set_selected(index as u32);
        }
    }
    group.add(&title);
    group.add(&format);
    page.add(&group);
    let instructions = gtk::TextView::builder()
        .wrap_mode(gtk::WrapMode::WordChar)
        .top_margin(12)
        .bottom_margin(12)
        .left_margin(12)
        .right_margin(12)
        .accepts_tab(false)
        .build();
    instructions.buffer().set_text(&prompt_text(&ui.entry));
    if message.is_none() {
        let request_group = adw::PreferencesGroup::builder()
            .title("What Would You Like to Create?")
            .description(
                "For example: a project proposal, a monthly budget, or a presentation for a class.",
            )
            .build();
        let scroll = gtk::ScrolledWindow::builder()
            .child(&instructions)
            .min_content_height(160)
            .max_content_height(260)
            .propagate_natural_height(true)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .build();
        scroll.add_css_class("card");
        request_group.add(&scroll);
        page.add(&request_group);
        let examples = gtk::FlowBox::builder()
            .selection_mode(gtk::SelectionMode::None)
            .max_children_per_line(4)
            .column_spacing(6)
            .row_spacing(6)
            .build();
        for (label, choice, text) in [
            (
                "Report",
                DocumentFormat::Pdf,
                "Write a report about [topic], with a summary, key findings, and recommendations.",
            ),
            (
                "Letter",
                DocumentFormat::Docx,
                "Write a letter to [recipient] about [purpose].",
            ),
            (
                "Budget",
                DocumentFormat::Xlsx,
                "Create a monthly budget with categories, planned and actual amounts, and formulas for totals and differences.",
            ),
            (
                "Slides",
                DocumentFormat::Pptx,
                "Create a presentation about [topic] with an introduction, key points, and a conclusion.",
            ),
        ] {
            let button = gtk::Button::with_label(label);
            button.add_css_class("pill");
            let buffer = instructions.buffer();
            let target_format = format.clone();
            let entry = instructions.clone();
            button.connect_clicked(move |_| {
                buffer.set_text(text);
                if let Some(start) = text.find('[')
                    && let Some(end) = text[start..].find(']')
                {
                    let begin = text[..start].chars().count() as i32;
                    let finish = text[..start + end + 1].chars().count() as i32;
                    buffer.select_range(
                        &buffer.iter_at_offset(begin),
                        &buffer.iter_at_offset(finish),
                    );
                }
                if let Some(index) = DocumentFormat::ALL
                    .iter()
                    .position(|format| *format == choice)
                {
                    target_format.set_selected(index as u32);
                }
                entry.grab_focus();
            });
            examples.insert(&button, -1);
        }
        let examples_group = adw::PreferencesGroup::new();
        examples_group.add(&examples);
        page.add(&examples_group);
        create.set_sensitive(!prompt_text(&instructions).trim().is_empty());
        let button = create.downgrade();
        instructions.buffer().connect_changed(move |buffer| {
            if let Some(button) = button.upgrade() {
                let text = buffer.text(&buffer.start_iter(), &buffer.end_iter(), false);
                button.set_sensitive(!text.trim().is_empty());
            }
        });
    }
    let toolbar = adw::ToolbarView::builder().content(&page).build();
    toolbar.add_top_bar(&header);
    dialog.set_child(Some(&toolbar));
    dialog.set_default_widget(Some(&create));
    let target_dialog = dialog.downgrade();
    cancel.connect_clicked(move |_| {
        if let Some(dialog) = target_dialog.upgrade() {
            dialog.close();
        }
    });
    let target_dialog = dialog.downgrade();
    let parent = ui.window.clone();
    let ui = ui.clone();
    let backend = backend.clone();
    create.connect_clicked(move |_| {
        let choice = DocumentFormat::ALL
            .get(format.selected() as usize)
            .copied()
            .unwrap_or(DocumentFormat::Pdf);
        let request = DocumentRequest::new(title.text().as_str(), choice);
        if let Some(dialog) = target_dialog.upgrade() {
            dialog.close();
        }
        if let Some(message) = &message {
            generate(&ui, &backend, message, Some(request));
        } else {
            let description = prompt_text(&instructions);
            let name = if request.title == "Untitled Document" {
                String::new()
            } else {
                format!(" named “{}”", request.title)
            };
            let prompt = format!(
                "Create a {}{name}.\n\n{}",
                request.format.label(),
                description.trim()
            );
            ui.entry.buffer().set_text(&prompt);
            submit_message_with_document(
                &ui,
                &backend,
                message_actions::Submission::New(prompt),
                Some(request),
            );
        }
    });
    dialog.present(Some(&parent));
}

pub(super) fn generate(
    ui: &Rc<WindowUi>,
    backend: &Rc<Backend>,
    message: &str,
    override_request: Option<DocumentRequest>,
) {
    let result = (|| -> Result<(DocumentRequest, String)> {
        let original = backend
            .conversation_repository
            .get_message(message)?
            .ok_or(MooseError::MessageNotFound)?;
        if original.status != MessageStatus::Complete {
            return Err(crate::attachments::error(
                "Finish or retry the response before creating its document.",
            ));
        }
        let existing = backend.conversation_repository.document_job(message)?;
        if existing.as_ref().is_some_and(|job| job.state == "creating") {
            return Err(crate::attachments::error(
                "This document is already being created.",
            ));
        }
        let content = existing
            .as_ref()
            .filter(|job| !job.content.is_empty())
            .map(|job| job.content.clone())
            .unwrap_or(original.content);
        if content.trim().is_empty() || content.trim() == "No response generated." {
            let error = "The model did not provide document content. Retry the response.";
            backend
                .conversation_repository
                .set_document_state(message, "failed", error)?;
            return Err(crate::attachments::error(error));
        }
        let request = override_request
            .or_else(|| existing.as_ref().map(|job| job.request.clone()))
            .ok_or_else(|| crate::attachments::error("Choose a document format first."))?;
        backend
            .conversation_repository
            .save_document_request(message, &request, &content)?;
        backend
            .conversation_repository
            .set_document_state(message, "creating", "")?;
        Ok((request, content))
    })();
    let (request, content) = match result {
        Ok(value) => value,
        Err(error) => {
            toast(ui, &error.to_string());
            message_actions::refresh_message(ui, backend, message);
            return;
        }
    };
    message_actions::refresh_message(ui, backend, message);
    let ui = ui.clone();
    let backend = backend.clone();
    let message = message.to_string();
    gtk::glib::MainContext::default().spawn_local(async move {
        gtk::glib::timeout_future(Duration::from_millis(30)).await;
        let source = content.clone();
        let result = if request.format == DocumentFormat::Pdf {
            Content::parse(&request.title, &source).and_then(|content| {
                let bytes = pdf::render(
                    &ui.window,
                    &backend.paths.cache_dir().join("created-documents"),
                    &content,
                )?;
                Ok(crate::documents::RenderedDocument {
                    title: content.title,
                    bytes,
                })
            })
        } else {
            let title = request.title.clone();
            let format = request.format;
            match backend
                .runtime
                .spawn_blocking(move || crate::documents::render(&title, &source, format))
                .await
            {
                Ok(result) => result,
                Err(_) => Err("Document creation was interrupted. Try again.".into()),
            }
        };
        let result = result.and_then(|output| {
            backend
                .conversation_repository
                .create_document(
                    &message,
                    &output.title,
                    request.format,
                    &content,
                    &output.bytes,
                )
                .map_err(|error| error.to_string())
        });
        match result {
            Ok(_) => {
                let _ = backend
                    .conversation_repository
                    .set_document_state(&message, "complete", "");
            }
            Err(error) => {
                let _ = backend
                    .conversation_repository
                    .set_document_state(&message, "failed", &error);
                toast(&ui, &error);
            }
        }
        message_actions::refresh_message(&ui, &backend, &message);
    });
}

pub(super) fn append(backend: &Backend, row: &gtk::Box, message: &Message) -> Result<()> {
    let documents = backend
        .conversation_repository
        .message_documents(&message.id)?;
    let job = backend.conversation_repository.document_job(&message.id)?;
    if documents.is_empty() && job.is_none() {
        return Ok(());
    }
    if !documents.is_empty() {
        let section = gtk::Box::new(gtk::Orientation::Vertical, 8);
        section.set_margin_top(8);
        section.set_margin_bottom(8);
        for document in &documents {
            section.append(&card(document));
        }
        row.append(&section);
    }
    if let Some(job) = job
        && (job.state == "creating"
            || !job.error.is_empty()
            || !documents
                .iter()
                .any(|document| document.format == job.request.format))
    {
        let working = message.status == MessageStatus::Streaming || job.state == "creating";
        let pending = adw::ActionRow::builder()
            .title(job.request.format.label())
            .subtitle(if working {
                if job.state == "creating" {
                    "Creating your file…"
                } else {
                    "Writing your document…"
                }
            } else if !job.error.is_empty() {
                &job.error
            } else if message.status == MessageStatus::Complete {
                "Ready to create your file"
            } else {
                "Continue or retry the response to finish this document"
            })
            .subtitle_lines(3)
            .use_markup(false)
            .build();
        if working {
            let spinner = gtk::Spinner::builder()
                .spinning(true)
                .valign(gtk::Align::Center)
                .build();
            pending.add_prefix(&spinner);
        } else {
            pending.add_prefix(&gtk::Image::from_icon_name(job.request.format.icon()));
            if message.status == MessageStatus::Complete {
                let button = gtk::Button::with_label(if job.error.is_empty() {
                    "Create File"
                } else {
                    "Retry Response"
                });
                button.set_valign(gtk::Align::Center);
                button.set_action_name(Some(if job.error.is_empty() {
                    "message.create-file"
                } else {
                    "message.regenerate"
                }));
                button.set_action_target_value(Some(&message.id.to_variant()));
                pending.add_suffix(&button);
            }
        }
        pending.add_css_class("card");
        pending.set_margin_top(8);
        pending.set_margin_bottom(8);
        row.insert_child_after(&pending, row.first_child().as_ref());
    }
    Ok(())
}

fn card(document: &CreatedDocument) -> gtk::Box {
    let card = gtk::Box::new(gtk::Orientation::Vertical, 0);
    card.add_css_class("card");
    let header = adw::ActionRow::builder()
        .title(document.filename())
        .subtitle(format!(
            "{} · {}",
            document.format.label(),
            model_actions::format_download_size(document.byte_size.max(0) as u64)
        ))
        .title_lines(1)
        .use_markup(false)
        .build();
    let icon = gtk::Image::from_icon_name(document.format.icon());
    icon.set_pixel_size(32);
    header.add_prefix(&icon);
    card.append(&header);
    let actions = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    actions.set_margin_start(12);
    actions.set_margin_end(12);
    actions.set_margin_bottom(12);
    for (label, action) in [
        ("Preview", "preview-document"),
        ("Open", "open-document"),
        ("Save As…", "save-document"),
    ] {
        let button = gtk::Button::with_label(label);
        button.set_hexpand(true);
        button.set_action_name(Some(&format!("message.{action}")));
        button.set_action_target_value(Some(&document.id.to_variant()));
        actions.append(&button);
    }
    card.append(&actions);
    card
}

fn preview(ui: &Rc<WindowUi>, backend: &Rc<Backend>, document: &CreatedDocument) {
    let content = match backend
        .conversation_repository
        .document_content(&document.id)
    {
        Ok(content) => content,
        Err(error) => {
            toast(ui, &error.to_string());
            return;
        }
    };
    let dialog = adw::Dialog::builder()
        .title(document.filename())
        .content_width(820)
        .content_height(700)
        .build();
    let header = adw::HeaderBar::new();
    let save_button = gtk::Button::with_label("Save As…");
    save_button.add_css_class("suggested-action");
    header.pack_end(&save_button);
    let document_copy = document.clone();
    let target_ui = ui.clone();
    let target_backend = backend.clone();
    save_button.connect_clicked(move |_| save(&target_ui, &target_backend, &document_copy));
    let page = gtk::Box::new(gtk::Orientation::Vertical, 18);
    page.set_margin_start(24);
    page.set_margin_end(24);
    page.set_margin_top(20);
    page.set_margin_bottom(24);
    let title = gtk::Label::builder()
        .label(&document.title)
        .xalign(0.0)
        .wrap(true)
        .selectable(true)
        .build();
    title.add_css_class("title-1");
    page.append(&title);
    let description = gtk::Label::builder()
        .label("Content preview · Open the file to see its final layout")
        .xalign(0.0)
        .wrap(true)
        .build();
    description.add_css_class("dim-label");
    page.append(&description);
    if matches!(
        document.format,
        DocumentFormat::Json | DocumentFormat::Xml | DocumentFormat::Csv | DocumentFormat::Text
    ) {
        let text = backend
            .conversation_repository
            .document_bytes(&document.id)
            .ok()
            .and_then(|bytes| String::from_utf8(bytes).ok())
            .unwrap_or(content);
        let view = gtk::TextView::builder()
            .editable(false)
            .cursor_visible(false)
            .monospace(true)
            .wrap_mode(gtk::WrapMode::WordChar)
            .build();
        view.buffer().set_text(text.trim_start_matches('\u{feff}'));
        page.append(&view);
    } else {
        page.append(&markdown_view::render(&content));
    }
    let clamp = adw::Clamp::builder()
        .child(&page)
        .maximum_size(780)
        .tightening_threshold(600)
        .build();
    let scroll = gtk::ScrolledWindow::builder()
        .child(&clamp)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .build();
    let toolbar = adw::ToolbarView::builder().content(&scroll).build();
    toolbar.add_top_bar(&header);
    dialog.set_child(Some(&toolbar));
    dialog.present(Some(&ui.window));
}

fn open(ui: &Rc<WindowUi>, backend: &Rc<Backend>, document: &CreatedDocument) {
    let result = (|| -> Result<std::path::PathBuf> {
        let bytes = backend
            .conversation_repository
            .document_bytes(&document.id)?;
        let directory = backend
            .paths
            .cache_dir()
            .join("opened-documents")
            .join(crate::core::new_id());
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&directory)?;
        let path = directory.join(document.filename());
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)?;
        file.write_all(&bytes)?;
        Ok(path)
    })();
    match result {
        Ok(path) => {
            let launcher = gtk::FileLauncher::new(Some(&gio::File::for_path(path)));
            let ui = ui.clone();
            let parent = ui.window.clone();
            launcher.launch(Some(&parent), None::<&gio::Cancellable>, move |result| {
                if let Err(error) = result
                    && !error.matches(gio::IOErrorEnum::Cancelled)
                {
                    toast(&ui, &format!("Document could not be opened: {error}"));
                }
            });
        }
        Err(error) => toast(ui, &error.to_string()),
    }
}

fn save(ui: &Rc<WindowUi>, backend: &Rc<Backend>, document: &CreatedDocument) {
    let bytes = match backend.conversation_repository.document_bytes(&document.id) {
        Ok(bytes) => bytes,
        Err(error) => {
            toast(ui, &error.to_string());
            return;
        }
    };
    let filter = gtk::FileFilter::new();
    filter.set_name(Some(document.format.label()));
    filter.add_suffix(document.format.extension());
    let filters = gio::ListStore::new::<gtk::FileFilter>();
    filters.append(&filter);
    let dialog = gtk::FileDialog::builder()
        .title("Save Document")
        .accept_label("Save")
        .initial_name(document.filename())
        .filters(&filters)
        .default_filter(&filter)
        .build();
    let ui = ui.clone();
    gtk::glib::MainContext::default().spawn_local(async move {
        let file = match dialog.save_future(Some(&ui.window)).await {
            Ok(file) => file,
            Err(error)
                if error.matches(gio::IOErrorEnum::Cancelled)
                    || error.matches(gtk::DialogError::Dismissed) =>
            {
                return;
            }
            Err(error) => {
                toast(&ui, &format!("Save location could not be opened: {error}"));
                return;
            }
        };
        match file
            .replace_contents_future(
                bytes,
                None,
                false,
                gio::FileCreateFlags::REPLACE_DESTINATION,
            )
            .await
        {
            Ok(_) => toast(&ui, "Document saved"),
            Err((_, error)) if error.matches(gio::IOErrorEnum::Cancelled) => {}
            Err((_, error)) => toast(&ui, &format!("Document could not be saved: {error}")),
        }
    });
}

fn toast(ui: &WindowUi, text: &str) {
    ui.toast_overlay.add_toast(adw::Toast::new(text));
}
