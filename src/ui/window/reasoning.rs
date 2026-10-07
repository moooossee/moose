use super::*;
use crate::chat::ThinkingValue;

#[derive(Default)]
pub(super) struct State {
    revision: u64,
    model: String,
    restoring: bool,
    values: Vec<ThinkingValue>,
    pub(super) cache: HashMap<(String, String), Vec<ThinkingValue>>,
}

pub(super) fn bind(ui: &Rc<WindowUi>, backend: &Rc<Backend>) {
    let target_ui = Rc::clone(ui);
    let target_backend = Rc::clone(backend);
    ui.model_picker.connect_selected_notify(move |_| {
        if !*target_ui.restoring_model_selection.borrow() {
            refresh(&target_ui, &target_backend);
        }
    });
    let target_ui = Rc::clone(ui);
    let backend = Rc::clone(backend);
    ui.thinking_picker.connect_selected_notify(move |_| {
        if target_ui.thinking_state.borrow().restoring {
            return;
        }
        let Some(model) = selected_model(&target_ui.model_picker, &target_ui.model_names) else {
            return;
        };
        let result = (|| -> Result<()> {
            workspace::save(&target_ui, &backend)?;
            let active = backend.active_conversation_id.borrow().clone();
            let id = match active {
                Some(id) => id,
                None => create_empty_conversation(&backend)?,
            };
            backend.conversation_repository.save_thinking_choice(
                &id,
                &model,
                selected(&target_ui).as_ref(),
            )?;
            backend.conversation_repository.remember_conversation(&id)?;
            Ok(())
        })();
        if let Err(error) = result {
            target_ui.toast_overlay.add_toast(adw::Toast::new(&format!(
                "Reasoning setting could not be saved: {error}"
            )));
        }
        conversation_list::refresh(&target_ui, &backend);
    });
}

pub(super) fn selected(ui: &WindowUi) -> Option<ThinkingValue> {
    let index = ui.thinking_picker.selected().checked_sub(1)? as usize;
    ui.thinking_state.borrow().values.get(index).cloned()
}

fn apply(ui: &WindowUi, backend: &Backend, model: &str, values: Vec<ThinkingValue>) {
    let unsupported = values == [ThinkingValue::Enabled(false)];
    let values = if unsupported { Vec::new() } else { values };
    let mut labels = vec![if unsupported {
        "Reasoning: —".to_string()
    } else {
        "Reasoning: Auto".to_string()
    }];
    labels.extend(
        values
            .iter()
            .map(|value| format!("Reasoning: {}", value.label())),
    );
    let saved = backend
        .active_conversation_id
        .borrow()
        .as_deref()
        .map(|id| backend.conversation_repository.thinking_choice(id, model))
        .transpose();
    let saved = match saved {
        Ok(saved) => saved.flatten(),
        Err(error) => {
            ui.toast_overlay.add_toast(adw::Toast::new(&format!(
                "Reasoning setting could not be loaded: {error}"
            )));
            None
        }
    };
    let index = saved
        .as_ref()
        .and_then(|saved| values.iter().position(|value| value == saved))
        .map(|index| index as u32 + 1)
        .unwrap_or(0);
    {
        let mut state = ui.thinking_state.borrow_mut();
        state.restoring = true;
        state.model = model.to_string();
        state.values = values;
    }
    let labels = labels.iter().map(String::as_str).collect::<Vec<_>>();
    ui.thinking_picker
        .set_model(Some(&gtk::StringList::new(&labels)));
    ui.thinking_picker.set_selected(index);
    ui.thinking_picker.set_sensitive(labels.len() > 1);
    ui.thinking_picker.set_tooltip_text(Some(if unsupported {
        "This model does not support reasoning"
    } else if labels.len() == 1 {
        "Ollama has not provided reasoning controls. The model’s default behavior will be used."
    } else {
        "Choose how this model reasons in this chat. Automatic uses the model’s default."
    }));
    ui.thinking_state.borrow_mut().restoring = false;
}

pub(super) fn refresh(ui: &Rc<WindowUi>, backend: &Rc<Backend>) {
    attachments::refresh_capabilities(ui, backend);
    let revision = {
        let mut state = ui.thinking_state.borrow_mut();
        state.revision += 1;
        state.revision
    };
    let Some(model) = selected_model(&ui.model_picker, &ui.model_names) else {
        apply(ui, backend, "", Vec::new());
        return;
    };
    let Some(provider) = active_provider(backend) else {
        return;
    };
    let key = (provider.base_url.clone(), model.clone());
    let cached = ui.thinking_state.borrow().cache.get(&key).cloned();
    if let Some(values) = cached {
        apply(ui, backend, &model, values);
        return;
    }
    apply(ui, backend, &model, Vec::new());
    ui.thinking_picker
        .set_tooltip_text(Some("Checking this model’s reasoning controls…"));
    let (sender, receiver) = mpsc::channel();
    let target_model = model.clone();
    let policy = backend.network_policy.clone();
    backend.runtime.spawn(async move {
        let result = async {
            ProviderClient::new(provider, policy)
                .await?
                .thinking_values(&target_model)
                .await
        }
        .await;
        let _ = sender.send(result);
    });
    let ui = Rc::clone(ui);
    let backend = Rc::clone(backend);
    gtk::glib::timeout_add_local(Duration::from_millis(80), move || {
        if ui.thinking_state.borrow().revision != revision {
            return gtk::glib::ControlFlow::Break;
        }
        match receiver.try_recv() {
            Ok(Ok(values)) => {
                ui.thinking_state
                    .borrow_mut()
                    .cache
                    .insert(key.clone(), values.clone());
                apply(&ui, &backend, &model, values);
                gtk::glib::ControlFlow::Break
            }
            Ok(Err(_)) | Err(mpsc::TryRecvError::Disconnected) => {
                apply(&ui, &backend, &model, Vec::new());
                ui.thinking_picker.set_tooltip_text(Some("Reasoning controls could not be loaded. The model’s default behavior will be used."));
                gtk::glib::ControlFlow::Break
            }
            Err(mpsc::TryRecvError::Empty) => gtk::glib::ControlFlow::Continue,
        }
    });
}

pub(super) fn selected_for_model(ui: &WindowUi, model: &str) -> Option<ThinkingValue> {
    if ui.thinking_state.borrow().model != model {
        return None;
    }
    selected(ui)
}

pub(super) fn reset(ui: &WindowUi) {
    {
        let mut state = ui.thinking_state.borrow_mut();
        state.revision += 1;
        state.restoring = true;
        state.model.clear();
        state.values.clear();
        state.cache.clear();
    }
    ui.thinking_picker
        .set_model(Some(&gtk::StringList::new(&["Reasoning: Auto"])));
    ui.thinking_picker.set_selected(0);
    ui.thinking_picker.set_sensitive(false);
    ui.thinking_picker
        .set_tooltip_text(Some("Select a model to see its reasoning controls"));
    ui.thinking_state.borrow_mut().restoring = false;
}
