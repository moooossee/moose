use std::{
    collections::HashMap,
    io::{Read, Write},
    net::TcpListener,
    thread,
    time::{Duration, Instant},
};

use super::super::{Backend, build_ui};
use super::*;
use crate::{platform::AppPaths, providers::NewProvider, storage::*};

fn iterate_until(mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !condition() {
        assert!(Instant::now() < deadline, "UI operation timed out");
        while glib::MainContext::default().pending() {
            glib::MainContext::default().iteration(false);
        }
        thread::sleep(Duration::from_millis(10));
    }
}

fn find_widget(
    widget: &gtk::Widget,
    predicate: &dyn Fn(&gtk::Widget) -> bool,
) -> Option<gtk::Widget> {
    if predicate(widget) {
        return Some(widget.clone());
    }
    let mut child = widget.first_child();
    while let Some(widget) = child {
        if let Some(found) = find_widget(&widget, predicate) {
            return Some(found);
        }
        child = widget.next_sibling();
    }
    None
}

fn button_with_tooltip(widget: &gtk::Widget, tooltip: &str) -> Option<gtk::Button> {
    find_widget(widget, &|widget| {
        widget.tooltip_text().as_deref() == Some(tooltip)
    })?
    .downcast()
    .ok()
}

fn test_backend(base_url: String) -> Rc<Backend> {
    let connection = Rc::new(open_in_memory_database().unwrap());
    let repository = ProviderRepository::new(Rc::clone(&connection));
    let mut provider = NewProvider::local_ollama(true);
    provider.base_url = base_url;
    let provider = repository.create(provider).unwrap();
    let paths = AppPaths::from_base_dirs(
        "/unused/data".into(),
        "/unused/cache".into(),
        "/unused/config".into(),
    );
    Rc::new(Backend {
        paths: paths.clone(),
        repository,
        conversation_repository: ConversationRepository::new(Rc::clone(&connection)),
        profile_repository: ProfileRepository::new(Rc::clone(&connection)),
        download_job_repository: DownloadJobRepository::new(connection),
        provider: std::cell::RefCell::new(Some(provider)),
        managed_ollama: std::sync::Arc::new(tokio::sync::Mutex::new(
            crate::ollama::service::ManagedOllamaService::new(&paths),
        )),
        managed_gpu: std::cell::RefCell::new(Default::default()),
        settings: None,
        network_policy: crate::providers::policy::NetworkPolicy::new(false),
        credential_operation: std::cell::Cell::new(false),
        model_load_revision: std::cell::Cell::new(0),
        selected_models: std::cell::RefCell::new(HashMap::new()),
        shortcuts: std::cell::RefCell::new(HashMap::new()),
        capturing_shortcut: std::cell::RefCell::new(false),
        runtime: tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap(),
        active_generation: std::cell::RefCell::new(None),
        active_model_pull: std::cell::RefCell::new(None),
        active_model_delete: std::cell::RefCell::new(None),
        active_conversation_id: std::cell::RefCell::new(None),
        active_assistant_message_id: std::cell::RefCell::new(None),
        active_assistant_content: std::cell::RefCell::new(String::new()),
        generation_context: std::cell::RefCell::new(super::super::generation::Context::default()),
    })
}

#[test]
#[ignore = "Requires a GTK display"]
fn message_actions_complete_the_edit_regenerate_retry_and_fork_flow() {
    adw::init().unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base_url = format!("http://{}/api", listener.local_addr().unwrap());
    let (sender, requests) = std::sync::mpsc::channel();
    let server = thread::spawn(move || {
        for (index, answer) in [
            "Alternative answer",
            "Edited answer",
            "",
            "Recovered answer",
        ]
        .iter()
        .enumerate()
        {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(10)))
                .unwrap();
            let mut bytes = Vec::new();
            let mut buffer = [0; 4096];
            let body = loop {
                let count = stream.read(&mut buffer).unwrap();
                assert!(count > 0);
                bytes.extend_from_slice(&buffer[..count]);
                if let Some(end) = bytes.windows(4).position(|chunk| chunk == b"\r\n\r\n") {
                    let headers = String::from_utf8_lossy(&bytes[..end]).to_lowercase();
                    let length: usize = headers
                        .lines()
                        .find_map(|line| line.strip_prefix("content-length:"))
                        .unwrap()
                        .trim()
                        .parse()
                        .unwrap();
                    if bytes.len() >= end + 4 + length {
                        break serde_json::from_slice::<serde_json::Value>(
                            &bytes[end + 4..end + 4 + length],
                        )
                        .unwrap();
                    }
                }
            };
            sender.send(body).unwrap();
            let (status, body) = if index == 2 {
                (
                    "500 Internal Server Error",
                    "{\"error\":\"Test failure\"}".to_string(),
                )
            } else {
                (
                    "200 OK",
                    format!(
                        "{{\"message\":{{\"content\":\"{answer}\"}},\"done\":false}}\n{{\"done\":true}}\n"
                    ),
                )
            };
            write!(stream, "HTTP/1.1 {status}\r\nContent-Type: application/x-ndjson\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
        }
    });
    let app = adw::Application::builder()
        .application_id("io.github.moooossee.Moose.MessageActionsTest")
        .flags(gio::ApplicationFlags::NON_UNIQUE)
        .build();
    app.register(None::<&gio::Cancellable>).unwrap();
    let ui = build_ui(&app);
    let backend = test_backend(base_url);
    bind(&ui, &backend);
    let provider_id = backend.provider.borrow().as_ref().unwrap().id.clone();
    let conversation = backend
        .conversation_repository
        .create(crate::conversations::NewConversation {
            provider_id,
            model_id: None,
            title: "Message actions".into(),
        })
        .unwrap();
    backend
        .conversation_repository
        .set_remote_permissions(
            &conversation.id,
            backend.provider.borrow().as_ref().unwrap(),
            crate::providers::policy::RemotePermissions {
                messages: true,
                files: false,
            },
        )
        .unwrap();
    let (user, original) = backend
        .conversation_repository
        .create_exchange(&conversation.id, "Explain branching")
        .unwrap();
    backend
        .conversation_repository
        .update_message(crate::conversations::MessageUpdate::completed(
            &original.id,
            "Original answer with **Markdown**.\n\n```rust\nlet version = 1;\n```",
        ))
        .unwrap();
    super::super::set_model_picker(&ui, vec!["test:latest".into()], Some("test:latest"));
    load_conversation(&ui, &backend, &conversation.id).unwrap();
    ui.entry.buffer().set_text("Unsent draft");
    ui.window.present();
    let regenerate = button_with_tooltip(ui.messages.upcast_ref(), "Regenerate Response").unwrap();
    regenerate.emit_clicked();
    assert!(backend.active_generation.borrow().is_some());
    assert!(!ui.message_action_group.is_action_enabled("version"));
    iterate_until(|| backend.active_generation.borrow().is_none());
    assert_eq!(super::super::prompt_text(&ui.entry), "Unsent draft");
    assert!(ui.message_action_group.is_action_enabled("version"));
    assert_eq!(
        ui.message_stack.visible_child_name().as_deref(),
        Some("messages")
    );
    let sent = requests.recv_timeout(Duration::from_secs(1)).unwrap();
    assert_eq!(sent["messages"].as_array().unwrap().len(), 1);
    assert_eq!(sent["messages"][0]["content"], "Explain branching");
    assert_eq!(
        backend
            .conversation_repository
            .list_messages(&conversation.id)
            .unwrap()[1]
            .content,
        "Alternative answer"
    );
    let previous = button_with_tooltip(ui.messages.upcast_ref(), "Previous Version").unwrap();
    previous.emit_clicked();
    assert_eq!(
        backend
            .conversation_repository
            .list_messages(&conversation.id)
            .unwrap()[1]
            .id,
        original.id
    );
    let previous = button_with_tooltip(ui.messages.upcast_ref(), "Previous Version").unwrap();
    assert!(!previous.is_sensitive());
    set_enabled(&ui, false);
    set_enabled(&ui, true);
    assert!(!previous.is_sensitive());
    assert_eq!(super::super::prompt_text(&ui.entry), "Unsent draft");
    let edit = button_with_tooltip(ui.messages.upcast_ref(), "Edit Message").unwrap();
    edit.emit_clicked();
    assert!(ui.window.visible_dialog().is_none());
    let cancel = button_with_tooltip(ui.messages.upcast_ref(), "Cancel editing (Esc)").unwrap();
    cancel.emit_clicked();
    edit.emit_clicked();
    let editor = find_widget(ui.messages.upcast_ref(), &|widget| {
        widget.is::<gtk::TextView>()
    })
    .unwrap()
    .downcast::<gtk::TextView>()
    .unwrap();
    let send = find_widget(ui.messages.upcast_ref(), &|widget| {
        widget
            .downcast_ref::<gtk::Button>()
            .is_some_and(|button| button.label().as_deref() == Some("Save & Send"))
    })
    .unwrap()
    .downcast::<gtk::Button>()
    .unwrap();
    assert!(!send.is_sensitive());
    editor.buffer().set_text("   ");
    assert!(!send.is_sensitive());
    editor.buffer().set_text("Explain versions instead");
    assert!(send.is_sensitive());
    send.emit_clicked();
    iterate_until(|| backend.active_generation.borrow().is_none());
    let sent = requests.recv_timeout(Duration::from_secs(1)).unwrap();
    assert_eq!(sent["messages"].as_array().unwrap().len(), 1);
    assert_eq!(sent["messages"][0]["content"], "Explain versions instead");
    assert_eq!(super::super::prompt_text(&ui.entry), "Unsent draft");
    let regenerate = button_with_tooltip(ui.messages.upcast_ref(), "Regenerate Response").unwrap();
    regenerate.emit_clicked();
    iterate_until(|| backend.active_generation.borrow().is_none());
    requests.recv_timeout(Duration::from_secs(1)).unwrap();
    let failed = backend
        .conversation_repository
        .list_messages(&conversation.id)
        .unwrap()
        .pop()
        .unwrap();
    assert_eq!(failed.status, MessageStatus::Failed);
    let retry = button_with_tooltip(ui.messages.upcast_ref(), "Retry Response").unwrap();
    retry.emit_clicked();
    iterate_until(|| backend.active_generation.borrow().is_none());
    requests.recv_timeout(Duration::from_secs(1)).unwrap();
    let answer = backend
        .conversation_repository
        .list_messages(&conversation.id)
        .unwrap()
        .pop()
        .unwrap();
    assert_eq!(answer.content, "Recovered answer");
    ui.message_action_group
        .activate_action("fork", Some(&answer.id.to_variant()));
    assert_ne!(
        backend.active_conversation_id.borrow().as_deref(),
        Some(conversation.id.as_str())
    );
    assert_eq!(super::super::prompt_text(&ui.entry), "Unsent draft");
    assert!(
        backend
            .conversation_repository
            .get(&conversation.id)
            .unwrap()
            .is_some()
    );
    backend
        .conversation_repository
        .select_message_version(&user.id)
        .unwrap();
    load_conversation(&ui, &backend, &conversation.id).unwrap();
    ui.window.destroy();
    server.join().unwrap();
}
