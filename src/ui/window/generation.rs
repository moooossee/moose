use super::*;
use crate::{conversations::Message, storage::MessageDetails};
use std::time::Instant;

#[derive(Default)]
pub(super) struct Context {
    pub(super) conversation_id: String,
    pub(super) model: String,
    reasoning: String,
    started: Option<Instant>,
    pub(super) finished: Option<Instant>,
    reasoning_started: Option<Instant>,
    reasoning_finished: Option<Instant>,
    checkpoint: Option<Instant>,
    save_error_shown: bool,
    pub(super) pending_status: Option<&'static str>,
}

impl Context {
    fn elapsed_ms(&self) -> i64 {
        self.started
            .map(|start| {
                self.finished
                    .unwrap_or_else(Instant::now)
                    .duration_since(start)
                    .as_millis() as i64
            })
            .unwrap_or(0)
    }

    fn reasoning_ms(&self) -> i64 {
        self.reasoning_started
            .map(|start| {
                self.reasoning_finished
                    .or(self.finished)
                    .unwrap_or_else(Instant::now)
                    .duration_since(start)
                    .as_millis() as i64
            })
            .unwrap_or(0)
    }

    fn details(&self) -> MessageDetails {
        MessageDetails {
            model: self.model.clone(),
            reasoning: self.reasoning.clone(),
            reasoning_ms: self.reasoning_ms(),
            elapsed_ms: self.elapsed_ms(),
        }
    }
}

pub(super) fn is_visible(backend: &Backend) -> bool {
    backend.active_conversation_id.borrow().as_deref()
        == Some(backend.generation_context.borrow().conversation_id.as_str())
}

pub(super) fn sync_controls(ui: &WindowUi, backend: &Backend) {
    let running = backend.active_generation.borrow().is_some();
    let visible =
        is_visible(backend) && ui.content_stack.visible_child_name().as_deref() == Some("chat");
    ui.stop_button.set_sensitive(running);
    ui.stop_button.set_tooltip_text(Some(if visible {
        "Stop Response"
    } else {
        "Stop Background Response"
    }));
    ui.generation_banner.set_revealed(running && !visible);
    message_actions::set_enabled(ui, !running);
    ui.send_button.set_tooltip_text(Some(if running {
        "Wait for the current response, or stop it to send this draft"
    } else {
        "Send Message"
    }));
    update_send_button(ui);
}

pub(super) fn bind(ui: &Rc<WindowUi>, backend: &Rc<Backend>) {
    let target_ui = Rc::clone(ui);
    let target_backend = Rc::clone(backend);
    ui.content_stack
        .connect_visible_child_name_notify(move |_| sync_controls(&target_ui, &target_backend));
    let target_ui = Rc::clone(ui);
    let target_backend = Rc::clone(backend);
    ui.generation_banner.connect_button_clicked(move |_| {
        let id = target_backend
            .generation_context
            .borrow()
            .conversation_id
            .clone();
        open(&target_ui, &target_backend, &id);
    });
    let action = gio::SimpleAction::new("open-conversation", Some(gtk::glib::VariantTy::STRING));
    let target_ui = Rc::clone(ui);
    let target_backend = Rc::clone(backend);
    action.connect_activate(move |_, parameter| {
        if let Some(id) = parameter.and_then(|value| value.str()) {
            open(&target_ui, &target_backend, id);
            target_ui.window.present();
        }
    });
    if let Some(app) = ui.window.application() {
        app.add_action(&action);
    }
}

pub(super) fn open(ui: &Rc<WindowUi>, backend: &Rc<Backend>, id: &str) {
    match load_conversation(ui, backend, id) {
        Ok(()) => {
            show_chat(ui);
            sync_controls(ui, backend);
            workspace::sync_provider(ui, backend);
            reasoning::refresh(ui, backend);
            conversation_list::select(ui, id);
        }
        Err(error) => ui.toast_overlay.add_toast(adw::Toast::new(&format!(
            "Chat could not be opened: {error}"
        ))),
    }
}

pub(super) fn persist(backend: &Backend, status: &str) -> Result<()> {
    let Some(id) = backend.active_assistant_message_id.borrow().clone() else {
        return Ok(());
    };
    backend.conversation_repository.save_response(
        &id,
        &backend.active_assistant_content.borrow(),
        status,
        &backend.generation_context.borrow().details(),
    )
}

pub(super) fn append_live(ui: &WindowUi, backend: &Backend, message: &Message) {
    let model = backend.generation_context.borrow().model.clone();
    let live = chat_view::append_streaming_message(&ui.messages, &model, Some(&message.created_at));
    if let Some(row) = ui.messages.last_child() {
        row.set_widget_name(&message.id);
    }
    *ui.streaming_message.borrow_mut() = Some(live);
    update_live(ui, backend);
}

fn update_live(ui: &WindowUi, backend: &Backend) {
    if !is_visible(backend) {
        return;
    }
    if let Some(live) = ui.streaming_message.borrow().as_ref() {
        let context = backend.generation_context.borrow();
        chat_view::update_streaming_message(
            live,
            &backend.active_assistant_content.borrow(),
            &context.reasoning,
            (context.elapsed_ms() / 1000) as u64,
            context.reasoning_ms(),
        );
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn start(
    ui: &Rc<WindowUi>,
    backend: &Rc<Backend>,
    provider: Provider,
    conversation_id: String,
    model: String,
    request: ChatRequest,
    assistant: Message,
    clear_draft: bool,
) {
    let assistant_id = assistant.id.clone();
    *backend.active_assistant_message_id.borrow_mut() = Some(assistant_id.clone());
    backend.active_assistant_content.borrow_mut().clear();
    *backend.generation_context.borrow_mut() = Context {
        conversation_id: conversation_id.clone(),
        model,
        started: Some(Instant::now()),
        checkpoint: Some(Instant::now()),
        ..Context::default()
    };
    if clear_draft {
        clear_prompt(&ui.entry);
        if let Err(error) = workspace::save(ui, backend) {
            ui.toast_overlay.add_toast(adw::Toast::new(&format!(
                "Draft could not be cleared: {error}"
            )));
        }
    }
    attachments::refresh(ui, backend);
    ui.message_stack.set_visible_child_name("messages");
    let (sender, receiver) = mpsc::channel();
    let paths = backend.paths.clone();
    let managed_ollama = Arc::clone(&backend.managed_ollama);
    let managed_gpu = backend.managed_gpu.borrow().clone();
    let handle = backend.runtime.spawn(async move {
        let result = async {
            let client =
                prepared_ollama_client(paths, managed_ollama, managed_gpu, provider).await?;
            client
                .stream_chat(request, |event| {
                    let event = match event {
                        ChatStreamEvent::Token(token) => ChatUiEvent::Token(token),
                        ChatStreamEvent::Thinking(token) => ChatUiEvent::Thinking(token),
                        ChatStreamEvent::Done => ChatUiEvent::Done,
                    };
                    let _ = sender.send(event);
                })
                .await
        }
        .await;
        if let Err(error) = result {
            let _ = sender.send(ChatUiEvent::Failed(error.to_string()));
        }
    });
    *backend.active_generation.borrow_mut() = Some(handle);
    message_actions::refresh(ui, backend);
    scroll_chat_to_bottom(ui);
    sync_controls(ui, backend);
    conversation_list::refresh(ui, backend);
    let ui = Rc::clone(ui);
    let backend = Rc::clone(backend);
    gtk::glib::timeout_add_local(Duration::from_millis(80), move || {
        if backend.active_assistant_message_id.borrow().as_deref() != Some(&assistant_id) {
            return gtk::glib::ControlFlow::Break;
        }
        let mut changed = false;
        let mut end = None;
        for _ in 0..512 {
            match receiver.try_recv() {
                Ok(ChatUiEvent::Token(token)) => {
                    if !token.is_empty() {
                        let mut context = backend.generation_context.borrow_mut();
                        if context.reasoning_started.is_some()
                            && context.reasoning_finished.is_none()
                        {
                            context.reasoning_finished = Some(Instant::now());
                        }
                        backend
                            .active_assistant_content
                            .borrow_mut()
                            .push_str(&token);
                        changed = true;
                    }
                }
                Ok(ChatUiEvent::Thinking(token)) => {
                    let mut context = backend.generation_context.borrow_mut();
                    context.reasoning_started.get_or_insert_with(Instant::now);
                    context.reasoning_finished = None;
                    context.reasoning.push_str(&token);
                    changed = true;
                }
                Ok(ChatUiEvent::Done) => {
                    end = Some((AssistantMessageEnd::Complete, None));
                    break;
                }
                Ok(ChatUiEvent::Failed(error)) => {
                    end = Some((AssistantMessageEnd::Failed, Some(error)));
                    break;
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    end = Some((
                        AssistantMessageEnd::Failed,
                        Some("The connection closed before the response finished".into()),
                    ));
                    break;
                }
                Err(mpsc::TryRecvError::Empty) => break,
            }
        }
        if let Some((status, error)) = end {
            let completed = matches!(status, AssistantMessageEnd::Complete);
            if completed && backend.active_assistant_content.borrow().trim().is_empty() {
                *backend.active_assistant_content.borrow_mut() = "No response generated.".into();
            }
            backend.active_generation.borrow_mut().take();
            if let Err(error) = persist_active_assistant_message(&backend, status) {
                ui.toast_overlay.add_toast(adw::Toast::new(&format!(
                    "Response could not be saved: {error}"
                )));
            }
            finish_generation(&ui);
            if is_visible(&backend) {
                message_actions::refresh(&ui, &backend);
            }
            workspace::sync_provider(&ui, &backend);
            sync_controls(&ui, &backend);
            conversation_list::refresh(&ui, &backend);
            if let Some(error) = error {
                ui.toast_overlay
                    .add_toast(adw::Toast::new(&format!("Generation failed: {error}")));
            }
            notify(&ui, &backend, &conversation_id, completed);
            return gtk::glib::ControlFlow::Break;
        }
        let should_scroll = changed
            && is_visible(&backend)
            && chat_view::should_stick_to_bottom(&ui.messages_scrolled);
        update_live(&ui, &backend);
        if should_scroll {
            scroll_chat_to_bottom(&ui);
        }
        let checkpoint_due = backend
            .generation_context
            .borrow()
            .checkpoint
            .is_none_or(|last| last.elapsed() >= Duration::from_secs(1));
        if checkpoint_due {
            if let Err(error) = persist(&backend, "streaming") {
                let mut context = backend.generation_context.borrow_mut();
                if !context.save_error_shown {
                    ui.toast_overlay.add_toast(adw::Toast::new(&format!(
                        "Response could not be saved: {error}"
                    )));
                    context.save_error_shown = true;
                }
            }
            backend.generation_context.borrow_mut().checkpoint = Some(Instant::now());
        }
        gtk::glib::ControlFlow::Continue
    });
}

fn notify(ui: &Rc<WindowUi>, backend: &Rc<Backend>, conversation_id: &str, completed: bool) {
    let title = if completed {
        "Response ready"
    } else {
        "Response interrupted"
    };
    if !is_visible(backend) || ui.content_stack.visible_child_name().as_deref() != Some("chat") {
        let toast = adw::Toast::builder()
            .title(title)
            .button_label("Open")
            .timeout(8)
            .build();
        let target_ui = Rc::clone(ui);
        let target_backend = Rc::clone(backend);
        let id = conversation_id.to_string();
        toast.connect_button_clicked(move |_| open(&target_ui, &target_backend, &id));
        ui.toast_overlay.add_toast(toast);
    }
    if !ui.window.is_active() {
        if let Some(app) = ui.window.application() {
            let notification = gio::Notification::new(title);
            if let Ok(Some(conversation)) = backend.conversation_repository.get(conversation_id) {
                notification.set_body(Some(&conversation.title));
            }
            notification.set_default_action_and_target_value(
                "app.open-conversation",
                Some(&conversation_id.to_variant()),
            );
            app.send_notification(Some(conversation_id), &notification);
        }
    }
}

pub(super) fn retry_pending(backend: &Backend) -> Result<()> {
    let status = backend.generation_context.borrow().pending_status;
    if let Some(status) = status {
        persist(backend, status)?;
        backend.active_assistant_message_id.borrow_mut().take();
        backend.active_assistant_content.borrow_mut().clear();
        backend.generation_context.borrow_mut().pending_status = None;
    }
    Ok(())
}
