use super::*;

pub(super) fn bind(ui: &Rc<WindowUi>, backend: &Rc<Backend>) {
    let weak_ui = Rc::downgrade(ui);
    let backend = Rc::clone(backend);
    ui.window.connect_close_request(move |_| {
        if let Some(ui) = weak_ui.upgrade() {
            if let Err(error) = save(&ui, &backend) {
                ui.toast_overlay.add_toast(adw::Toast::new(&format!(
                    "Draft could not be saved: {error}"
                )));
                return gtk::glib::Propagation::Stop;
            }
            if let Err(error) = backend.cancel_generation() {
                finish_generation(&ui);
                ui.toast_overlay.add_toast(adw::Toast::new(&format!(
                    "Response could not be saved: {error}"
                )));
                return gtk::glib::Propagation::Stop;
            }
        }
        gtk::glib::Propagation::Proceed
    });
}

pub(super) fn schedule_save(ui: &Rc<WindowUi>, backend: &Rc<Backend>) {
    if ui.restoring_draft.get() {
        return;
    }
    if let Some(source) = ui.draft_source.borrow_mut().take() {
        source.remove();
    }
    ui.draft_label.set_label("Saving…");
    let target_ui = Rc::clone(ui);
    let backend = Rc::clone(backend);
    let source = gtk::glib::timeout_add_local_once(Duration::from_millis(350), move || {
        target_ui.draft_source.borrow_mut().take();
        let was_new = backend.active_conversation_id.borrow().is_none();
        if save(&target_ui, &backend).is_err() {
            target_ui.draft_label.set_label("Draft not saved");
        }
        if was_new {
            conversation_list::refresh(&target_ui, &backend);
        }
    });
    *ui.draft_source.borrow_mut() = Some(source);
}

pub(super) fn save(ui: &WindowUi, backend: &Backend) -> Result<()> {
    if let Some(source) = ui.draft_source.borrow_mut().take() {
        source.remove();
    }
    let content = prompt_text(&ui.entry);
    let active = backend.active_conversation_id.borrow().clone();
    let id = match active {
        Some(id) => id,
        None if content.is_empty() => {
            ui.draft_label.set_label("");
            return Ok(());
        }
        None => create_empty_conversation(backend)?,
    };
    backend.conversation_repository.save_draft(&id, &content)?;
    backend.conversation_repository.remember_conversation(&id)?;
    conversation_list::update_draft(ui, &id, &content);
    ui.draft_label.set_label(if content.is_empty() {
        ""
    } else {
        "Draft saved"
    });
    Ok(())
}

pub(super) fn restore(ui: &WindowUi, backend: &Backend) -> Result<()> {
    let id = backend.active_conversation_id.borrow().clone();
    let draft = id
        .as_deref()
        .map(|id| backend.conversation_repository.draft(id))
        .transpose()?
        .unwrap_or_default();
    ui.restoring_draft.set(true);
    ui.entry.buffer().set_text(&draft);
    ui.restoring_draft.set(false);
    ui.draft_label
        .set_label(if draft.is_empty() { "" } else { "Draft saved" });
    if let Some(id) = id {
        backend.conversation_repository.remember_conversation(&id)?;
    }
    attachments::refresh(ui, backend);
    update_send_button(ui);
    Ok(())
}

pub(super) fn restore_last(ui: &Rc<WindowUi>, backend: &Rc<Backend>) {
    let result = (|| -> Result<()> {
        let Some(id) = backend.conversation_repository.last_conversation()? else {
            return Ok(());
        };
        if let Some(conversation) = backend.conversation_repository.get(&id)? {
            if let Some(provider) = backend.repository.get(&conversation.provider_id)? {
                *backend.provider.borrow_mut() = Some(provider.clone());
                apply_provider_state(ui, &Some(provider));
            }
        }
        load_conversation(ui, backend, &id)?;
        conversation_list::select(ui, &id);
        Ok(())
    })();
    if let Err(error) = result {
        ui.toast_overlay.add_toast(adw::Toast::new(&format!(
            "Workspace could not be restored: {error}"
        )));
    }
}

pub(super) fn provider_matches(backend: &Backend) -> bool {
    let Some(id) = backend.active_conversation_id.borrow().clone() else {
        return true;
    };
    match backend.conversation_repository.get(&id) {
        Ok(Some(conversation)) => {
            active_provider(backend).is_some_and(|provider| provider.id == conversation.provider_id)
        }
        _ => false,
    }
}

pub(super) fn sync_provider(ui: &Rc<WindowUi>, backend: &Rc<Backend>) {
    if provider_matches(backend)
        || backend.active_generation.borrow().is_some()
        || backend.active_model_pull.borrow().is_some()
        || backend.active_model_delete.borrow().is_some()
    {
        return;
    }
    let result = (|| -> Result<()> {
        let Some(id) = backend.active_conversation_id.borrow().clone() else {
            return Ok(());
        };
        let Some(conversation) = backend.conversation_repository.get(&id)? else {
            return Ok(());
        };
        let provider = backend
            .repository
            .get(&conversation.provider_id)?
            .ok_or(MooseError::ProviderNotConfigured)?;
        *backend.provider.borrow_mut() = Some(provider.clone());
        if !provider.is_managed {
            backend.stop_managed_ollama();
        }
        apply_provider_state(ui, &Some(provider));
        refresh_models(ui, backend);
        Ok(())
    })();
    if let Err(error) = result {
        ui.toast_overlay.add_toast(adw::Toast::new(&format!(
            "Chat instance could not be loaded: {error}"
        )));
    }
}
