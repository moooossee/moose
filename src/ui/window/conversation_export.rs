use crate::i18n::{gettext, gettext_f, gettext_noop};

use std::rc::Rc;

use adw::prelude::*;
use gtk::gio;
use serde::Serialize;

use crate::{
    conversations::{Conversation, Message, MessageStatus},
    core::utc_now,
    error::{MooseError, Result},
    providers::Provider,
};

use super::{Backend, WindowUi};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ExportFormat {
    Markdown,
    Json,
    Text,
}

#[derive(Serialize)]
struct ExportDocument<'a> {
    version: u8,
    exported_at: &'a str,
    conversation: ExportConversation<'a>,
    messages: Vec<ExportMessage<'a>>,
    assets: &'a std::collections::BTreeMap<String, ExportAsset>,
}

#[derive(Serialize)]
struct ExportConversation<'a> {
    id: &'a str,
    title: &'a str,
    provider: ExportProvider<'a>,
    model: Option<&'a str>,
    created_at: &'a str,
    updated_at: &'a str,
    archived_at: Option<&'a str>,
    pinned_at: Option<&'a str>,
}

#[derive(Serialize)]
struct ExportProvider<'a> {
    id: &'a str,
    name: &'a str,
    kind: &'a str,
    managed: bool,
}

#[derive(Serialize)]
struct ExportMessage<'a> {
    id: &'a str,
    role: &'a str,
    content: &'a str,
    status: &'a str,
    created_at: &'a str,
    completed_at: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    details: Option<&'a crate::storage::MessageDetails>,
    attachments: &'a [crate::attachments::Asset],
    sources: &'a [crate::attachments::Source],
    documents: &'a [ExportCreatedDocument],
}

#[derive(Serialize)]
struct ExportCreatedDocument {
    metadata: crate::documents::CreatedDocument,
    data_base64: Option<String>,
}

#[derive(Serialize)]
struct ExportAsset {
    metadata: crate::attachments::Asset,
    data_base64: String,
}

struct ExportData {
    conversation: Conversation,
    provider: Provider,
    model: Option<String>,
    exported_at: String,
    messages: Vec<Message>,
    details: std::collections::HashMap<String, crate::storage::MessageDetails>,
    attachments: std::collections::HashMap<String, Vec<crate::attachments::Asset>>,
    sources: std::collections::HashMap<String, Vec<crate::attachments::Source>>,
    assets: std::collections::BTreeMap<String, ExportAsset>,
    documents: std::collections::HashMap<String, Vec<ExportCreatedDocument>>,
}

pub(super) fn show_dialog(ui: &Rc<WindowUi>, backend: &Rc<Backend>, conversation_id: &str) {
    if backend.active_generation.borrow().is_some()
        && backend.generation_context.borrow().conversation_id == conversation_id
    {
        ui.toast_overlay.add_toast(adw::Toast::new(&gettext(
            "Finish the active generation first",
        )));
        return;
    }

    let dialog = adw::AlertDialog::builder()
        .heading(gettext("Export Conversation"))
        .body(gettext("JSON includes copies of attachments and created documents. Markdown and plain text include their names. All formats include document source excerpts."))
        .close_response("cancel")
        .default_response("markdown")
        .build();
    dialog.add_response("cancel", &gettext("Cancel"));
    dialog.add_response("markdown", "Markdown");
    dialog.add_response("json", "JSON");
    dialog.add_response("text", &gettext("Plain Text"));
    dialog.set_response_appearance("markdown", adw::ResponseAppearance::Suggested);

    let target_ui = Rc::clone(ui);
    let target_backend = Rc::clone(backend);
    let target_conversation_id = conversation_id.to_string();
    dialog.connect_response(None, move |_, response| {
        let format = match response {
            "markdown" => ExportFormat::Markdown,
            "json" => ExportFormat::Json,
            "text" => ExportFormat::Text,
            _ => return,
        };
        export(&target_ui, &target_backend, &target_conversation_id, format);
    });
    dialog.present(Some(&ui.window));
}

fn export(ui: &Rc<WindowUi>, backend: &Rc<Backend>, conversation_id: &str, format: ExportFormat) {
    let data = match load_export_data(ui, backend, conversation_id, format) {
        Ok(data) => data,
        Err(error) => {
            ui.toast_overlay.add_toast(adw::Toast::new(&gettext_f(
                "Conversation could not be exported: {error}",
                &[("error", &(error).to_string())],
            )));
            return;
        }
    };

    let content = match format_content(&data, format) {
        Ok(content) => content,
        Err(error) => {
            ui.toast_overlay.add_toast(adw::Toast::new(&gettext_f(
                "Conversation could not be exported: {error}",
                &[("error", &(error).to_string())],
            )));
            return;
        }
    };

    save(
        ui,
        default_filename(
            &crate::i18n::conversation_title(&data.conversation.title),
            format,
        ),
        content,
        format,
    );
}

fn load_export_data(
    ui: &WindowUi,
    backend: &Backend,
    conversation_id: &str,
    format: ExportFormat,
) -> Result<ExportData> {
    let conversation = backend
        .conversation_repository
        .get(conversation_id)?
        .ok_or(crate::error::MooseError::ConversationNotFound)?;
    let provider = backend
        .repository
        .get(&conversation.provider_id)?
        .ok_or(crate::error::MooseError::ProviderNotConfigured)?;
    let settings = backend
        .conversation_repository
        .latest_generation_settings(conversation_id)?;
    let model = settings
        .and_then(|settings| settings.model)
        .or_else(|| conversation.model_id.clone())
        .or_else(|| active_selected_model(ui, backend, conversation_id));
    let messages = backend
        .conversation_repository
        .list_messages(conversation_id)?;

    let mut details = std::collections::HashMap::new();
    for message in &messages {
        if let Some(value) = backend
            .conversation_repository
            .message_details(&message.id)?
        {
            details.insert(message.id.clone(), value);
        }
    }
    let mut attachments = std::collections::HashMap::new();
    let mut sources = std::collections::HashMap::new();
    let mut assets = std::collections::BTreeMap::new();
    let mut documents = std::collections::HashMap::new();
    for message in &messages {
        let files = backend
            .conversation_repository
            .message_assets(&message.id)?;
        let references = backend
            .conversation_repository
            .response_sources(&message.id)?;
        if format == ExportFormat::Json {
            for id in files
                .iter()
                .map(|asset| &asset.id)
                .chain(references.iter().map(|source| &source.asset_id))
            {
                if !assets.contains_key(id) {
                    let metadata = backend.conversation_repository.asset(id)?;
                    let bytes = backend.conversation_repository.asset_bytes(id)?;
                    assets.insert(
                        id.clone(),
                        ExportAsset {
                            metadata,
                            data_base64: gtk::glib::base64_encode(&bytes).to_string(),
                        },
                    );
                }
            }
        }
        attachments.insert(message.id.clone(), files);
        sources.insert(message.id.clone(), references);
        let mut created = Vec::new();
        for metadata in backend
            .conversation_repository
            .message_documents(&message.id)?
        {
            let data_base64 = if format == ExportFormat::Json {
                Some(
                    gtk::glib::base64_encode(
                        &backend
                            .conversation_repository
                            .document_bytes(&metadata.id)?,
                    )
                    .to_string(),
                )
            } else {
                None
            };
            created.push(ExportCreatedDocument {
                metadata,
                data_base64,
            });
        }
        documents.insert(message.id.clone(), created);
    }
    Ok(ExportData {
        conversation,
        provider,
        model,
        exported_at: utc_now(),
        messages,
        details,
        attachments,
        sources,
        assets,
        documents,
    })
}

fn active_selected_model(
    ui: &WindowUi,
    backend: &Backend,
    conversation_id: &str,
) -> Option<String> {
    if backend.active_conversation_id.borrow().as_deref() != Some(conversation_id) {
        return None;
    }

    let selected = usize::try_from(ui.model_picker.selected()).ok()?;
    ui.model_names.borrow().get(selected).cloned()
}

fn format_content(data: &ExportData, format: ExportFormat) -> Result<String> {
    match format {
        ExportFormat::Markdown => Ok(markdown(data)),
        ExportFormat::Json => json(data),
        ExportFormat::Text => Ok(text(data)),
    }
}

fn markdown(data: &ExportData) -> String {
    let mut output = String::new();
    output.push_str("# ");
    output.push_str(&crate::i18n::conversation_title(&data.conversation.title));
    output.push_str("\n\n");
    output.push_str(&gettext("Provider: "));
    output.push_str(&data.provider.name);
    output.push('\n');
    output.push_str(&gettext("Model: "));
    output.push_str(data.model.as_deref().unwrap_or(&gettext("Not set")));
    output.push('\n');
    output.push_str(&gettext("Exported: "));
    output.push_str(&data.exported_at);
    output.push_str("\n\n");

    for message in &data.messages {
        output.push_str("## ");
        output.push_str(&gettext(message_heading(message)));
        output.push_str("\n\n");
        output.push_str(crate::i18n::message_text(&message.content).trim_end());
        output.push_str("\n\n");
        append_references(&mut output, data, message);
    }

    output
}

fn text(data: &ExportData) -> String {
    let mut output = String::new();
    output.push_str(&crate::i18n::conversation_title(&data.conversation.title));
    output.push_str("\n\n");
    output.push_str(&gettext("Provider: "));
    output.push_str(&data.provider.name);
    output.push('\n');
    output.push_str(&gettext("Model: "));
    output.push_str(data.model.as_deref().unwrap_or(&gettext("Not set")));
    output.push('\n');
    output.push_str(&gettext("Exported: "));
    output.push_str(&data.exported_at);
    output.push_str("\n\n");

    for message in &data.messages {
        output.push_str(&gettext(message_heading(message)));
        output.push_str("\n\n");
        output.push_str(crate::i18n::message_text(&message.content).trim_end());
        output.push_str("\n\n");
        append_references(&mut output, data, message);
    }

    output
}

fn append_references(output: &mut String, data: &ExportData, message: &Message) {
    if let Some(documents) = data.documents.get(&message.id) {
        for document in documents {
            output.push_str(&gettext_f(
                "Created document: {arg1} ({arg2})\n\n",
                &[
                    ("arg1", &(document.metadata.filename()).to_string()),
                    ("arg2", &gettext(document.metadata.format.label())),
                ],
            ));
        }
    }
    if message.status == MessageStatus::Incomplete {
        output.push_str(&MooseError::ResponseLimit.to_string());
        output.push_str("\n\n");
    }
    if let Some(details) = data.details.get(&message.id) {
        if !details.tool_notice.is_empty() {
            output.push_str(&details.tool_notice);
            output.push_str("\n\n");
        }
        for tool in &details.tools {
            output.push_str(&format!(
                "{}:\n{}\n\n",
                gettext(tool.label()),
                tool.summary()
            ));
        }
        for source in crate::tools::web::sources(&details.tools) {
            output.push_str(&gettext_f(
                "Web source [{arg1}]: {arg2}\n{arg3}\n{arg4} · Retrieved {arg5}{arg6}\n\n",
                &[
                    ("arg1", &(source.id).to_string()),
                    ("arg2", &(source.title).to_string()),
                    ("arg3", &(source.url).to_string()),
                    (
                        "arg4",
                        (if source.page_read {
                            "Page read"
                        } else {
                            "Search snippet"
                        }),
                    ),
                    ("arg5", &(source.retrieved_at).to_string()),
                    (
                        "arg6",
                        (if source.truncated {
                            " · Excerpt truncated"
                        } else {
                            ""
                        }),
                    ),
                ],
            ));
            for line in source.content.lines() {
                output.push_str("> ");
                output.push_str(line);
                output.push('\n');
            }
            output.push('\n');
        }
    }
    if let Some(assets) = data.attachments.get(&message.id) {
        for asset in assets {
            output.push_str(&gettext_f(
                "Attachment: {arg1} ({arg2})\n\n",
                &[
                    ("arg1", &(asset.name).to_string()),
                    ("arg2", &(asset.mime_type).to_string()),
                ],
            ));
        }
    }
    if let Some(sources) = data.sources.get(&message.id) {
        for (index, source) in sources.iter().enumerate() {
            output.push_str(&gettext_f(
                "Source [{arg1}]: {arg2} — page {arg3}\n\n",
                &[
                    ("arg1", &(index + 1).to_string()),
                    ("arg2", &(source.name).to_string()),
                    ("arg3", &(source.page).to_string()),
                ],
            ));
            for line in source.content.lines() {
                output.push_str("> ");
                output.push_str(line);
                output.push('\n');
            }
            output.push('\n');
        }
    }
}

fn json(data: &ExportData) -> Result<String> {
    let document = ExportDocument {
        version: 5,
        assets: &data.assets,
        exported_at: &data.exported_at,
        conversation: ExportConversation {
            id: &data.conversation.id,
            title: &data.conversation.title,
            provider: ExportProvider {
                id: &data.provider.id,
                name: &data.provider.name,
                kind: data.provider.kind.as_str(),
                managed: data.provider.is_managed,
            },
            model: data.model.as_deref(),
            created_at: &data.conversation.created_at,
            updated_at: &data.conversation.updated_at,
            archived_at: data.conversation.archived_at.as_deref(),
            pinned_at: data.conversation.pinned_at.as_deref(),
        },
        messages: data
            .messages
            .iter()
            .map(|message| ExportMessage {
                id: &message.id,
                role: message.role.as_str(),
                content: &message.content,
                status: message.status.as_str(),
                created_at: &message.created_at,
                completed_at: message.completed_at.as_deref(),
                details: data.details.get(&message.id),
                attachments: data
                    .attachments
                    .get(&message.id)
                    .map(Vec::as_slice)
                    .unwrap_or_default(),
                sources: data
                    .sources
                    .get(&message.id)
                    .map(Vec::as_slice)
                    .unwrap_or_default(),
                documents: data
                    .documents
                    .get(&message.id)
                    .map(Vec::as_slice)
                    .unwrap_or_default(),
            })
            .collect(),
    };

    Ok(serde_json::to_string_pretty(&document)?)
}

fn message_heading(message: &Message) -> &'static str {
    match message.role.as_str() {
        "user" => gettext_noop("You"),
        "assistant" => gettext_noop("Assistant"),
        "system" => gettext_noop("System"),
        "tool" => gettext_noop("Tool"),
        _ => gettext_noop("Message"),
    }
}

fn save(ui: &Rc<WindowUi>, filename: String, content: String, format: ExportFormat) {
    let filter = file_filter(format);
    let dialog = gtk::FileDialog::builder()
        .title(gettext("Export Conversation"))
        .accept_label(gettext("Export"))
        .initial_name(filename)
        .default_filter(&filter)
        .build();
    let parent = ui.window.clone();
    let toast_overlay = ui.toast_overlay.clone();

    gtk::glib::MainContext::default().spawn_local(async move {
        let file = match dialog.save_future(Some(&parent)).await {
            Ok(file) => file,
            Err(error) if error.matches(gio::IOErrorEnum::Cancelled) => return,
            Err(error) => {
                toast_overlay.add_toast(adw::Toast::new(&gettext_f(
                    "Export destination could not be opened: {error}",
                    &[("error", &(error).to_string())],
                )));
                return;
            }
        };

        match file
            .replace_contents_future(
                content.into_bytes(),
                None,
                false,
                gio::FileCreateFlags::REPLACE_DESTINATION,
            )
            .await
        {
            Ok(_) => toast_overlay.add_toast(adw::Toast::new(&gettext("Conversation exported"))),
            Err((_, error)) if error.matches(gio::IOErrorEnum::Cancelled) => {}
            Err((_, error)) => toast_overlay.add_toast(adw::Toast::new(&gettext_f(
                "Conversation could not be saved: {error}",
                &[("error", &(error).to_string())],
            ))),
        }
    });
}

fn file_filter(format: ExportFormat) -> gtk::FileFilter {
    let filter = gtk::FileFilter::new();
    match format {
        ExportFormat::Markdown => {
            filter.set_name(Some("Markdown"));
            filter.add_suffix("md");
            filter.add_mime_type("text/markdown");
        }
        ExportFormat::Json => {
            filter.set_name(Some("JSON"));
            filter.add_suffix("json");
            filter.add_mime_type("application/json");
        }
        ExportFormat::Text => {
            filter.set_name(Some(&gettext("Plain Text")));
            filter.add_suffix("txt");
            filter.add_mime_type("text/plain");
        }
    }
    filter
}

fn default_filename(title: &str, format: ExportFormat) -> String {
    let mut stem = title
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_') {
                character.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-");

    if stem.is_empty() {
        stem = "conversation".to_string();
    }

    if stem.len() > 80 {
        stem.truncate(80);
        stem = stem.trim_end_matches('-').to_string();
    }

    format!("{stem}.{}", extension(format))
}

fn extension(format: ExportFormat) -> &'static str {
    match format {
        ExportFormat::Markdown => "md",
        ExportFormat::Json => "json",
        ExportFormat::Text => "txt",
    }
}
