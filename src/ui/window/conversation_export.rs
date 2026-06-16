use std::rc::Rc;

use adw::prelude::*;
use gtk::gio;
use serde::Serialize;

use crate::{
    conversations::{Conversation, Message},
    core::utc_now,
    error::Result,
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
}

struct ExportData {
    conversation: Conversation,
    provider: Provider,
    model: Option<String>,
    exported_at: String,
    messages: Vec<Message>,
}

pub(super) fn show_dialog(ui: &Rc<WindowUi>, backend: &Rc<Backend>, conversation_id: &str) {
    if backend.active_generation.borrow().is_some() {
        ui.toast_overlay
            .add_toast(adw::Toast::new("Finish the active generation first"));
        return;
    }

    let dialog = adw::AlertDialog::builder()
        .heading("Export Conversation")
        .body("Choose a local file format.")
        .close_response("cancel")
        .default_response("markdown")
        .build();
    dialog.add_response("cancel", "Cancel");
    dialog.add_response("markdown", "Markdown");
    dialog.add_response("json", "JSON");
    dialog.add_response("text", "Plain Text");
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
    let data = match load_export_data(ui, backend, conversation_id) {
        Ok(data) => data,
        Err(error) => {
            ui.toast_overlay.add_toast(adw::Toast::new(&format!(
                "Conversation could not be exported: {error}"
            )));
            return;
        }
    };

    let content = match format_content(&data, format) {
        Ok(content) => content,
        Err(error) => {
            ui.toast_overlay.add_toast(adw::Toast::new(&format!(
                "Conversation could not be exported: {error}"
            )));
            return;
        }
    };

    save(
        ui,
        default_filename(&data.conversation.title, format),
        content,
        format,
    );
}

fn load_export_data(ui: &WindowUi, backend: &Backend, conversation_id: &str) -> Result<ExportData> {
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

    Ok(ExportData {
        conversation,
        provider,
        model,
        exported_at: utc_now(),
        messages,
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
    output.push_str(&data.conversation.title);
    output.push_str("\n\n");
    output.push_str("Provider: ");
    output.push_str(&data.provider.name);
    output.push('\n');
    output.push_str("Model: ");
    output.push_str(data.model.as_deref().unwrap_or("Not set"));
    output.push('\n');
    output.push_str("Exported: ");
    output.push_str(&data.exported_at);
    output.push_str("\n\n");

    for message in &data.messages {
        output.push_str("## ");
        output.push_str(message_heading(message));
        output.push_str("\n\n");
        output.push_str(message.content.trim_end());
        output.push_str("\n\n");
    }

    output
}

fn text(data: &ExportData) -> String {
    let mut output = String::new();
    output.push_str(&data.conversation.title);
    output.push_str("\n\n");
    output.push_str("Provider: ");
    output.push_str(&data.provider.name);
    output.push('\n');
    output.push_str("Model: ");
    output.push_str(data.model.as_deref().unwrap_or("Not set"));
    output.push('\n');
    output.push_str("Exported: ");
    output.push_str(&data.exported_at);
    output.push_str("\n\n");

    for message in &data.messages {
        output.push_str(message_heading(message));
        output.push_str("\n\n");
        output.push_str(message.content.trim_end());
        output.push_str("\n\n");
    }

    output
}

fn json(data: &ExportData) -> Result<String> {
    let document = ExportDocument {
        version: 1,
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
            })
            .collect(),
    };

    Ok(serde_json::to_string_pretty(&document)?)
}

fn message_heading(message: &Message) -> &'static str {
    match message.role.as_str() {
        "user" => "You",
        "assistant" => "Assistant",
        "system" => "System",
        "tool" => "Tool",
        _ => "Message",
    }
}

fn save(ui: &Rc<WindowUi>, filename: String, content: String, format: ExportFormat) {
    let filter = file_filter(format);
    let dialog = gtk::FileDialog::builder()
        .title("Export Conversation")
        .accept_label("Export")
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
                toast_overlay.add_toast(adw::Toast::new(&format!(
                    "Export destination could not be opened: {error}"
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
            Ok(_) => toast_overlay.add_toast(adw::Toast::new("Conversation exported")),
            Err((_, error)) if error.matches(gio::IOErrorEnum::Cancelled) => {}
            Err((_, error)) => toast_overlay.add_toast(adw::Toast::new(&format!(
                "Conversation could not be saved: {error}"
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
            filter.set_name(Some("Plain Text"));
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
