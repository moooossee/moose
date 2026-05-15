use std::{cell::RefCell, fs, rc::Rc, sync::mpsc, time::Duration};

use adw::prelude::*;
use serde::Deserialize;

use crate::{
    APPLICATION_NAME,
    chat::{ChatMessage, ChatRequest, ChatStreamEvent},
    conversations::{
        ConversationTitleUpdate, DEFAULT_CONVERSATION_TITLE, MessageUpdate, NewConversation,
        NewMessage,
    },
    core::new_id,
    error::Result,
    ollama::{OllamaClient, OllamaModel, OllamaPullProgress},
    platform::AppPaths,
    providers::{Provider, validate_model_name},
    storage::{ConversationRepository, ProviderRepository, open_database},
};

mod chat_view;
mod conversation_list;
mod model_manager;
mod preferences;
mod sidebar;
mod widgets;

const CHAT_CSS: &str = include_str!("../../data/io.github.moooossee.Moose.css");
const TITLE_SYSTEM_PROMPT: &str = "You are an assistant that generates short chat titles based on the prompt. If you want to, you can add a single emoji. Format the response as a single JSON object.";
const TITLE_MAX_CHARS: usize = 30;

pub fn build(app: &adw::Application) -> adw::ApplicationWindow {
    install_chat_css();

    let window = adw::ApplicationWindow::builder()
        .application(app)
        .title(APPLICATION_NAME)
        .default_width(1120)
        .default_height(720)
        .build();

    let sidebar = sidebar::build();
    let chat = chat_view::build();
    let model_manager = model_manager::build();
    let new_chat_button = sidebar.new_chat_button.clone();
    let search_button = sidebar.search_button.clone();
    let model_manager_button = sidebar.model_manager_button.clone();

    let header_bar = adw::HeaderBar::new();
    let sidebar_toggle_button = widgets::icon_button("sidebar-show-symbolic", "Hide Sidebar");
    let preferences_button = widgets::icon_button("preferences-system-symbolic", "Preferences");
    sidebar_toggle_button.add_css_class("moose-header-button");
    preferences_button.add_css_class("moose-header-button");
    header_bar.pack_start(&sidebar_toggle_button);
    header_bar.pack_end(&preferences_button);

    let content_toolbar = adw::ToolbarView::new();
    let toast_overlay = adw::ToastOverlay::new();
    let content_stack = gtk::Stack::builder().hexpand(true).vexpand(true).build();
    content_stack.set_transition_type(gtk::StackTransitionType::Crossfade);
    content_stack.add_named(&chat.root, Some("chat"));
    content_stack.add_named(&model_manager.root, Some("models"));
    content_stack.set_visible_child_name("chat");
    toast_overlay.set_child(Some(&content_stack));
    content_toolbar.add_top_bar(&header_bar);
    content_toolbar.set_content(Some(&toast_overlay));

    let split_view = adw::OverlaySplitView::builder()
        .min_sidebar_width(232.0)
        .max_sidebar_width(292.0)
        .sidebar_width_fraction(0.21)
        .pin_sidebar(true)
        .show_sidebar(true)
        .enable_hide_gesture(true)
        .enable_show_gesture(true)
        .sidebar(&sidebar.root)
        .content(&content_toolbar)
        .build();

    window.set_content(Some(&split_view));
    window.set_size_request(768, 520);

    let model_names = Rc::new(RefCell::new(Vec::new()));
    let installed_models = Rc::new(RefCell::new(Vec::new()));
    let conversation_ids = Rc::new(RefCell::new(Vec::new()));
    let ui = Rc::new(WindowUi {
        window: window.clone(),
        toast_overlay,
        content_stack,
        model_manager,
        provider_row: sidebar.provider_row,
        provider_status: sidebar.provider_status,
        refresh_button: sidebar.refresh_button,
        model_picker: chat.model_picker,
        conversation_list: sidebar.conversation_list,
        messages: chat.messages,
        chat_status_page: chat.status_page,
        message_stack: chat.message_stack,
        entry: chat.entry,
        send_button: chat.send_button,
        stop_button: chat.stop_button,
        model_names,
        installed_models,
        conversation_ids,
        restoring_conversation_selection: RefCell::new(false),
    });

    bind_sidebar_visibility(&split_view, &sidebar_toggle_button);

    match Backend::new() {
        Ok(backend) => {
            let backend = Rc::new(backend);
            update_provider_summary(&ui, &backend.provider.borrow());
            bind_actions(
                &window,
                &ui,
                &backend,
                &new_chat_button,
                &search_button,
                &model_manager_button,
                &preferences_button,
            );
            refresh_models(&ui, &backend);
            conversation_list::refresh(&ui, &backend);
        }
        Err(error) => {
            ui.provider_status.set_text("Storage Error");
            ui.send_button.set_sensitive(false);
            ui.stop_button.set_sensitive(false);
            ui.refresh_button.set_sensitive(false);
            ui.model_manager.refresh_button.set_sensitive(false);
            ui.toast_overlay
                .add_toast(adw::Toast::new(&format!("Storage setup failed: {error}")));
        }
    }

    window
}

struct Backend {
    repository: ProviderRepository,
    conversation_repository: ConversationRepository,
    provider: RefCell<Provider>,
    runtime: tokio::runtime::Runtime,
    active_generation: RefCell<Option<tokio::task::JoinHandle<()>>>,
    active_model_pull: RefCell<Option<ActiveModelPull>>,
    active_model_delete: RefCell<Option<tokio::task::JoinHandle<()>>>,
    active_conversation_id: RefCell<Option<String>>,
    active_assistant_message_id: RefCell<Option<String>>,
    active_assistant_content: RefCell<String>,
}

struct ActiveModelPull {
    id: String,
    handle: tokio::task::JoinHandle<()>,
}

struct WindowUi {
    window: adw::ApplicationWindow,
    toast_overlay: adw::ToastOverlay,
    content_stack: gtk::Stack,
    model_manager: model_manager::ModelManager,
    provider_row: adw::ActionRow,
    provider_status: gtk::Label,
    refresh_button: gtk::Button,
    model_picker: gtk::DropDown,
    conversation_list: gtk::ListBox,
    messages: gtk::Box,
    chat_status_page: adw::StatusPage,
    message_stack: gtk::Stack,
    entry: gtk::TextView,
    send_button: gtk::Button,
    stop_button: gtk::Button,
    model_names: Rc<RefCell<Vec<String>>>,
    installed_models: Rc<RefCell<Vec<OllamaModel>>>,
    conversation_ids: Rc<RefCell<Vec<String>>>,
    restoring_conversation_selection: RefCell<bool>,
}

enum ModelLoadEvent {
    Loaded {
        available: bool,
        status: String,
        models: Vec<OllamaModel>,
    },
    Failed(String),
}

enum ChatUiEvent {
    Token(String),
    Done,
    Failed(String),
}

enum ModelPullUiEvent {
    Progress(OllamaPullProgress),
    Done,
    Failed(String),
}

enum ModelDeleteUiEvent {
    Done,
    Failed(String),
}

enum TitleUiEvent {
    Generated(String),
    Failed,
}

enum AssistantMessageEnd {
    Complete,
    Cancelled,
    Failed,
}

#[derive(Deserialize)]
struct GeneratedTitle {
    title: String,
}

impl Backend {
    fn new() -> Result<Self> {
        let paths = AppPaths::new("moose")?;
        paths.create_all()?;
        let connection = Rc::new(open_database(paths.database_path())?);
        let repository = ProviderRepository::new(Rc::clone(&connection));
        let conversation_repository = ConversationRepository::new(connection);
        let provider = repository.ensure_default_provider()?;
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()?;

        Ok(Self {
            repository,
            conversation_repository,
            provider: RefCell::new(provider),
            runtime,
            active_generation: RefCell::new(None),
            active_model_pull: RefCell::new(None),
            active_model_delete: RefCell::new(None),
            active_conversation_id: RefCell::new(None),
            active_assistant_message_id: RefCell::new(None),
            active_assistant_content: RefCell::new(String::new()),
        })
    }

    fn client(&self) -> Result<OllamaClient> {
        OllamaClient::new(&self.provider.borrow().base_url)
    }

    fn abort_generation(&self) {
        if let Some(handle) = self.active_generation.borrow_mut().take() {
            handle.abort();
        }
    }

    fn cancel_generation(&self) -> Result<bool> {
        let was_active = if let Some(handle) = self.active_generation.borrow_mut().take() {
            handle.abort();
            true
        } else {
            false
        };
        persist_active_assistant_message(self, AssistantMessageEnd::Cancelled)?;
        Ok(was_active)
    }

    fn cancel_model_pull(&self) -> bool {
        if let Some(active) = self.active_model_pull.borrow_mut().take() {
            active.handle.abort();
            true
        } else {
            false
        }
    }

    fn finish_model_pull(&self, id: &str) -> bool {
        let mut active_model_pull = self.active_model_pull.borrow_mut();
        let is_current = active_model_pull
            .as_ref()
            .is_some_and(|active| active.id == id);
        if is_current {
            active_model_pull.take();
        }
        is_current
    }

    fn finish_model_delete(&self) {
        self.active_model_delete.borrow_mut().take();
    }
}

fn bind_sidebar_visibility(
    split_view: &adw::OverlaySplitView,
    sidebar_toggle_button: &gtk::Button,
) {
    let target_split_view = split_view.clone();
    sidebar_toggle_button.connect_clicked(move |_| {
        target_split_view.set_show_sidebar(!target_split_view.shows_sidebar());
    });

    let target_toggle_button = sidebar_toggle_button.clone();
    split_view.connect_show_sidebar_notify(move |split_view| {
        let tooltip = if split_view.shows_sidebar() {
            "Hide Sidebar"
        } else {
            "Show Sidebar"
        };
        target_toggle_button.set_tooltip_text(Some(tooltip));
    });
}

fn bind_actions(
    window: &adw::ApplicationWindow,
    ui: &Rc<WindowUi>,
    backend: &Rc<Backend>,
    new_chat_button: &gtk::Button,
    search_button: &gtk::Button,
    model_manager_button: &gtk::Button,
    preferences_button: &gtk::Button,
) {
    let target_ui = Rc::clone(ui);
    let target_backend = Rc::clone(backend);
    new_chat_button.connect_clicked(move |_| {
        show_chat(&target_ui);
        if let Err(error) = target_backend.cancel_generation() {
            target_ui.toast_overlay.add_toast(adw::Toast::new(&format!(
                "Conversation could not be saved: {error}"
            )));
        }
        match create_empty_conversation(&target_backend) {
            Ok(conversation_id) => {
                clear_messages(&target_ui);
                set_chat_empty_state(&target_ui, "Empty Conversation", "Send a message to begin.");
                conversation_list::refresh(&target_ui, &target_backend);
                conversation_list::select(&target_ui, &conversation_id);
            }
            Err(error) => {
                target_ui.toast_overlay.add_toast(adw::Toast::new(&format!(
                    "Conversation could not be created: {error}"
                )));
            }
        }
    });

    let target_ui = Rc::clone(ui);
    search_button.connect_clicked(move |_| {
        target_ui
            .toast_overlay
            .add_toast(adw::Toast::new("Conversation search is not ready yet"));
    });

    let target_ui = Rc::clone(ui);
    let target_backend = Rc::clone(backend);
    model_manager_button.connect_clicked(move |_| {
        if target_backend.active_generation.borrow().is_some() {
            target_ui
                .toast_overlay
                .add_toast(adw::Toast::new("Finish the active generation first"));
            return;
        }
        show_model_manager(&target_ui);
    });

    let target_ui = Rc::clone(ui);
    let target_backend = Rc::clone(backend);
    ui.refresh_button.connect_clicked(move |_| {
        refresh_models(&target_ui, &target_backend);
    });

    let target_ui = Rc::clone(ui);
    let target_backend = Rc::clone(backend);
    ui.model_manager.refresh_button.connect_clicked(move |_| {
        refresh_models(&target_ui, &target_backend);
    });

    let parent = window.clone();
    let target_ui = Rc::clone(ui);
    let target_backend = Rc::clone(backend);
    ui.model_manager.pull_button.connect_clicked(move |_| {
        show_pull_dialog(&parent, &target_ui, &target_backend);
    });

    let target_ui = Rc::clone(ui);
    let target_backend = Rc::clone(backend);
    ui.model_manager
        .pull_cancel_button
        .connect_clicked(move |_| {
            if target_backend.cancel_model_pull() {
                target_ui.refresh_button.set_sensitive(true);
                model_manager::set_pull_finished(
                    &target_ui.model_manager,
                    "Download Cancelled",
                    "The model download was cancelled.",
                    0.0,
                );
                target_ui
                    .toast_overlay
                    .add_toast(adw::Toast::new("Model download cancelled"));
            }
        });

    let target_ui = Rc::clone(ui);
    let target_backend = Rc::clone(backend);
    ui.model_manager
        .search_entry
        .connect_search_changed(move |entry| {
            let query = entry.text().to_string();
            render_model_manager(&target_ui, &target_backend, &query);
        });

    let target_ui = Rc::clone(ui);
    let target_backend = Rc::clone(backend);
    ui.send_button.connect_clicked(move |_| {
        send_message(&target_ui, &target_backend);
    });

    let target_ui = Rc::clone(ui);
    let target_backend = Rc::clone(backend);
    let key_controller = gtk::EventControllerKey::new();
    key_controller.connect_key_pressed(move |_, key, _, state| {
        let is_enter = key == gtk::gdk::Key::Return
            || key == gtk::gdk::Key::KP_Enter
            || key == gtk::gdk::Key::ISO_Enter;
        if is_enter && !state.contains(gtk::gdk::ModifierType::SHIFT_MASK) {
            send_message(&target_ui, &target_backend);
            gtk::glib::Propagation::Stop
        } else {
            gtk::glib::Propagation::Proceed
        }
    });
    ui.entry.add_controller(key_controller);

    let target_ui = Rc::clone(ui);
    ui.entry.buffer().connect_changed(move |_| {
        update_send_button(&target_ui);
    });

    let target_ui = Rc::clone(ui);
    let target_backend = Rc::clone(backend);
    ui.stop_button
        .connect_clicked(move |_| match target_backend.cancel_generation() {
            Ok(true) => {
                finish_generation(&target_ui);
                conversation_list::refresh(&target_ui, &target_backend);
                target_ui
                    .toast_overlay
                    .add_toast(adw::Toast::new("Generation cancelled"));
            }
            Ok(false) => {}
            Err(error) => {
                finish_generation(&target_ui);
                target_ui.toast_overlay.add_toast(adw::Toast::new(&format!(
                    "Conversation could not be saved: {error}"
                )));
            }
        });

    let target_ui = Rc::clone(ui);
    ui.model_picker.connect_selected_notify(move |_| {
        update_send_button(&target_ui);
    });

    let target_ui = Rc::clone(ui);
    let target_backend = Rc::clone(backend);
    ui.conversation_list.connect_row_selected(move |_, row| {
        if *target_ui.restoring_conversation_selection.borrow() {
            return;
        }

        let Some(row) = row else {
            return;
        };

        if target_backend.active_generation.borrow().is_some() {
            target_ui
                .toast_overlay
                .add_toast(adw::Toast::new("Finish the active generation first"));
            return;
        }

        if let Err(error) = conversation_list::load_selected(&target_ui, &target_backend, row) {
            target_ui.toast_overlay.add_toast(adw::Toast::new(&format!(
                "Conversation could not be loaded: {error}"
            )));
        } else {
            show_chat(&target_ui);
        }
    });

    let parent = window.clone();
    let target_ui = Rc::clone(ui);
    let target_backend = Rc::clone(backend);
    preferences_button.connect_clicked(move |_| {
        preferences::dialog(&parent, &target_ui, &target_backend).present(Some(&parent));
    });
}

fn refresh_models(ui: &Rc<WindowUi>, backend: &Rc<Backend>) {
    if backend.active_model_pull.borrow().is_some() {
        ui.toast_overlay
            .add_toast(adw::Toast::new("Finish the active model download first"));
        return;
    }

    if backend.active_model_delete.borrow().is_some() {
        ui.toast_overlay
            .add_toast(adw::Toast::new("Finish the active model deletion first"));
        return;
    }

    let client = match backend.client() {
        Ok(client) => client,
        Err(error) => {
            ui.provider_status.set_text("Invalid URL");
            ui.toast_overlay.add_toast(adw::Toast::new(&format!(
                "Provider URL is invalid: {error}"
            )));
            set_model_picker(ui, Vec::new());
            set_installed_models(ui, backend, Vec::new());
            model_manager::set_unavailable(
                &ui.model_manager,
                "Invalid Provider URL",
                "The active provider URL could not be used.",
            );
            return;
        }
    };

    ui.provider_status.set_text("Checking");
    ui.refresh_button.set_sensitive(false);
    ui.model_manager.refresh_button.set_sensitive(false);
    set_model_picker(ui, Vec::new());
    set_installed_models(ui, backend, Vec::new());
    model_manager::set_loading(&ui.model_manager);

    let (sender, receiver) = mpsc::channel();
    backend.runtime.spawn(async move {
        let health = client.health().await;
        if !health.available {
            let _ = sender.send(ModelLoadEvent::Loaded {
                available: false,
                status: health.message,
                models: Vec::new(),
            });
            return;
        }

        match client.list_models().await {
            Ok(models) => {
                let _ = sender.send(ModelLoadEvent::Loaded {
                    available: true,
                    status: health.message,
                    models,
                });
            }
            Err(error) => {
                let _ = sender.send(ModelLoadEvent::Failed(error.to_string()));
            }
        }
    });

    let target_ui = Rc::clone(ui);
    let target_backend = Rc::clone(backend);
    gtk::glib::timeout_add_local(Duration::from_millis(50), move || {
        match receiver.try_recv() {
            Ok(ModelLoadEvent::Loaded {
                available,
                status,
                models,
            }) => {
                target_ui.refresh_button.set_sensitive(true);
                target_ui.model_manager.refresh_button.set_sensitive(true);
                target_ui.provider_status.set_text(if available {
                    "Connected"
                } else {
                    "Disconnected"
                });
                if !available {
                    target_ui
                        .toast_overlay
                        .add_toast(adw::Toast::new(&format!("Ollama unavailable: {status}")));
                    set_model_picker(&target_ui, Vec::new());
                    set_installed_models(&target_ui, &target_backend, Vec::new());
                    model_manager::set_unavailable(
                        &target_ui.model_manager,
                        "Ollama Unavailable",
                        "The active provider did not respond.",
                    );
                    return gtk::glib::ControlFlow::Break;
                }
                set_models(&target_ui, &target_backend, models);
                gtk::glib::ControlFlow::Break
            }
            Ok(ModelLoadEvent::Failed(error)) => {
                target_ui.refresh_button.set_sensitive(true);
                target_ui.model_manager.refresh_button.set_sensitive(true);
                target_ui.provider_status.set_text("Error");
                target_ui
                    .toast_overlay
                    .add_toast(adw::Toast::new(&format!("Model list failed: {error}")));
                set_model_picker(&target_ui, Vec::new());
                set_installed_models(&target_ui, &target_backend, Vec::new());
                model_manager::set_unavailable(
                    &target_ui.model_manager,
                    "Models Could Not Load",
                    "Ollama returned an error while listing local models.",
                );
                gtk::glib::ControlFlow::Break
            }
            Err(mpsc::TryRecvError::Empty) => gtk::glib::ControlFlow::Continue,
            Err(mpsc::TryRecvError::Disconnected) => {
                target_ui.refresh_button.set_sensitive(true);
                target_ui.model_manager.refresh_button.set_sensitive(true);
                target_ui.provider_status.set_text("Disconnected");
                set_model_picker(&target_ui, Vec::new());
                set_installed_models(&target_ui, &target_backend, Vec::new());
                model_manager::set_unavailable(
                    &target_ui.model_manager,
                    "Ollama Disconnected",
                    "The model refresh stopped before a response was received.",
                );
                gtk::glib::ControlFlow::Break
            }
        }
    });
}

fn show_pull_dialog(parent: &adw::ApplicationWindow, ui: &Rc<WindowUi>, backend: &Rc<Backend>) {
    if backend.active_generation.borrow().is_some() {
        ui.toast_overlay
            .add_toast(adw::Toast::new("Finish the active generation first"));
        return;
    }

    if backend.active_model_pull.borrow().is_some() {
        ui.toast_overlay
            .add_toast(adw::Toast::new("A model download is already running"));
        return;
    }

    let model_row = adw::EntryRow::builder().title("Model Name").build();
    let group = adw::PreferencesGroup::new();
    group.add(&model_row);

    let dialog = adw::AlertDialog::builder()
        .heading("Pull Model")
        .body("Enter an Ollama model name, such as llama3.2:latest.")
        .extra_child(&group)
        .close_response("cancel")
        .default_response("pull")
        .build();
    dialog.add_response("cancel", "Cancel");
    dialog.add_response("pull", "Download");
    dialog.set_response_appearance("pull", adw::ResponseAppearance::Suggested);

    let target_parent = parent.clone();
    let target_ui = Rc::clone(ui);
    let target_backend = Rc::clone(backend);
    dialog.connect_response(Some("pull"), move |_, _| {
        let model_text = model_row.text().to_string();
        let model = match validate_model_name(&model_text) {
            Ok(model) => model,
            Err(_) => {
                target_ui
                    .toast_overlay
                    .add_toast(adw::Toast::new("Model name is invalid"));
                return;
            }
        };

        request_model_pull(&target_parent, &target_ui, &target_backend, model);
    });

    dialog.present(Some(parent));
}

fn request_model_pull(
    parent: &adw::ApplicationWindow,
    ui: &Rc<WindowUi>,
    backend: &Rc<Backend>,
    model: String,
) {
    let model = match validate_model_name(&model) {
        Ok(model) => model,
        Err(_) => {
            ui.toast_overlay
                .add_toast(adw::Toast::new("Model name is invalid"));
            return;
        }
    };

    if provider_is_local(&backend.provider.borrow()) {
        start_model_pull(ui, backend, model);
    } else {
        confirm_remote_model_pull(parent, ui, backend, model);
    }
}

fn confirm_remote_model_pull(
    parent: &adw::ApplicationWindow,
    ui: &Rc<WindowUi>,
    backend: &Rc<Backend>,
    model: String,
) {
    let provider = backend.provider.borrow().clone();
    let dialog = adw::AlertDialog::builder()
        .heading("Download on Remote Provider?")
        .body(format!(
            "The model will be downloaded by {} at {}.",
            provider.name, provider.base_url
        ))
        .close_response("cancel")
        .default_response("download")
        .build();
    dialog.add_response("cancel", "Cancel");
    dialog.add_response("download", "Download");
    dialog.set_response_appearance("download", adw::ResponseAppearance::Suggested);

    let target_ui = Rc::clone(ui);
    let target_backend = Rc::clone(backend);
    dialog.connect_response(Some("download"), move |_, _| {
        start_model_pull(&target_ui, &target_backend, model.clone());
    });

    dialog.present(Some(parent));
}

fn start_model_pull(ui: &Rc<WindowUi>, backend: &Rc<Backend>, model: String) {
    if backend.active_generation.borrow().is_some() {
        ui.toast_overlay
            .add_toast(adw::Toast::new("Finish the active generation first"));
        return;
    }

    if backend.active_model_pull.borrow().is_some() {
        ui.toast_overlay
            .add_toast(adw::Toast::new("A model download is already running"));
        return;
    }

    if backend.active_model_delete.borrow().is_some() {
        ui.toast_overlay
            .add_toast(adw::Toast::new("Finish the active model deletion first"));
        return;
    }

    let client = match backend.client() {
        Ok(client) => client,
        Err(error) => {
            ui.toast_overlay.add_toast(adw::Toast::new(&format!(
                "Provider URL is invalid: {error}"
            )));
            return;
        }
    };

    show_model_manager(ui);
    ui.refresh_button.set_sensitive(false);
    model_manager::set_pull_started(&ui.model_manager, &model);

    let (sender, receiver) = mpsc::channel();
    let target_model = model.clone();
    let pull_id = new_id();
    let handle = backend.runtime.spawn(async move {
        let progress_sender = sender.clone();
        let result = client
            .pull_model(&target_model, |progress| {
                let _ = progress_sender.send(ModelPullUiEvent::Progress(progress));
            })
            .await;

        match result {
            Ok(()) => {
                let _ = sender.send(ModelPullUiEvent::Done);
            }
            Err(error) => {
                let _ = sender.send(ModelPullUiEvent::Failed(error.to_string()));
            }
        }
    });
    *backend.active_model_pull.borrow_mut() = Some(ActiveModelPull {
        id: pull_id.clone(),
        handle,
    });

    let target_ui = Rc::clone(ui);
    let target_backend = Rc::clone(backend);
    gtk::glib::timeout_add_local(Duration::from_millis(80), move || {
        loop {
            match receiver.try_recv() {
                Ok(ModelPullUiEvent::Progress(progress)) => {
                    model_manager::set_pull_progress(&target_ui.model_manager, &model, &progress);
                }
                Ok(ModelPullUiEvent::Done) => {
                    if !target_backend.finish_model_pull(&pull_id) {
                        return gtk::glib::ControlFlow::Break;
                    }
                    target_ui.refresh_button.set_sensitive(true);
                    model_manager::set_pull_finished(
                        &target_ui.model_manager,
                        "Download Complete",
                        &format!("{model} is ready to use."),
                        1.0,
                    );
                    target_ui
                        .toast_overlay
                        .add_toast(adw::Toast::new("Model download complete"));
                    refresh_models(&target_ui, &target_backend);
                    return gtk::glib::ControlFlow::Break;
                }
                Ok(ModelPullUiEvent::Failed(error)) => {
                    if !target_backend.finish_model_pull(&pull_id) {
                        return gtk::glib::ControlFlow::Break;
                    }
                    target_ui.refresh_button.set_sensitive(true);
                    model_manager::set_pull_finished(
                        &target_ui.model_manager,
                        "Download Failed",
                        &error,
                        0.0,
                    );
                    target_ui
                        .toast_overlay
                        .add_toast(adw::Toast::new(&format!("Model download failed: {error}")));
                    return gtk::glib::ControlFlow::Break;
                }
                Err(mpsc::TryRecvError::Empty) => return gtk::glib::ControlFlow::Continue,
                Err(mpsc::TryRecvError::Disconnected) => {
                    if target_backend.finish_model_pull(&pull_id) {
                        target_ui.refresh_button.set_sensitive(true);
                        model_manager::set_pull_finished(
                            &target_ui.model_manager,
                            "Download Stopped",
                            "The model download stopped before completion.",
                            0.0,
                        );
                    }
                    return gtk::glib::ControlFlow::Break;
                }
            }
        }
    });
}

fn confirm_model_delete(
    parent: &adw::ApplicationWindow,
    ui: &Rc<WindowUi>,
    backend: &Rc<Backend>,
    model: String,
) {
    let model = match validate_model_name(&model) {
        Ok(model) => model,
        Err(_) => {
            ui.toast_overlay
                .add_toast(adw::Toast::new("Model name is invalid"));
            return;
        }
    };

    if backend.active_generation.borrow().is_some() {
        ui.toast_overlay
            .add_toast(adw::Toast::new("Finish the active generation first"));
        return;
    }

    if backend.active_model_pull.borrow().is_some() {
        ui.toast_overlay
            .add_toast(adw::Toast::new("Finish the active model download first"));
        return;
    }

    if backend.active_model_delete.borrow().is_some() {
        ui.toast_overlay
            .add_toast(adw::Toast::new("A model deletion is already running"));
        return;
    }

    let provider = backend.provider.borrow().clone();
    let body = if provider_is_local(&provider) {
        format!("Delete \"{model}\" from Ollama? You can download it again later.")
    } else {
        format!(
            "Delete \"{model}\" from {} at {}? This changes the remote provider.",
            provider.name, provider.base_url
        )
    };

    let dialog = adw::AlertDialog::builder()
        .heading("Delete Model?")
        .body(body)
        .close_response("cancel")
        .default_response("cancel")
        .build();
    dialog.add_response("cancel", "Cancel");
    dialog.add_response("delete", "Delete");
    dialog.set_response_appearance("delete", adw::ResponseAppearance::Destructive);

    let target_ui = Rc::clone(ui);
    let target_backend = Rc::clone(backend);
    dialog.connect_response(Some("delete"), move |_, _| {
        start_model_delete(&target_ui, &target_backend, model.clone());
    });

    dialog.present(Some(parent));
}

fn start_model_delete(ui: &Rc<WindowUi>, backend: &Rc<Backend>, model: String) {
    if backend.active_generation.borrow().is_some() {
        ui.toast_overlay
            .add_toast(adw::Toast::new("Finish the active generation first"));
        return;
    }

    if backend.active_model_pull.borrow().is_some() {
        ui.toast_overlay
            .add_toast(adw::Toast::new("Finish the active model download first"));
        return;
    }

    if backend.active_model_delete.borrow().is_some() {
        ui.toast_overlay
            .add_toast(adw::Toast::new("A model deletion is already running"));
        return;
    }

    let client = match backend.client() {
        Ok(client) => client,
        Err(error) => {
            ui.toast_overlay.add_toast(adw::Toast::new(&format!(
                "Provider URL is invalid: {error}"
            )));
            return;
        }
    };

    show_model_manager(ui);
    ui.refresh_button.set_sensitive(false);
    model_manager::set_delete_started(&ui.model_manager, &model);

    let (sender, receiver) = mpsc::channel();
    let target_model = model.clone();
    let handle = backend.runtime.spawn(async move {
        match client.delete_model(&target_model).await {
            Ok(()) => {
                let _ = sender.send(ModelDeleteUiEvent::Done);
            }
            Err(error) => {
                let _ = sender.send(ModelDeleteUiEvent::Failed(error.to_string()));
            }
        }
    });
    *backend.active_model_delete.borrow_mut() = Some(handle);

    let target_ui = Rc::clone(ui);
    let target_backend = Rc::clone(backend);
    gtk::glib::timeout_add_local(Duration::from_millis(80), move || {
        match receiver.try_recv() {
            Ok(ModelDeleteUiEvent::Done) => {
                target_backend.finish_model_delete();
                target_ui.refresh_button.set_sensitive(true);
                model_manager::set_delete_finished(
                    &target_ui.model_manager,
                    "Model Deleted",
                    &format!("{model} was removed from Ollama."),
                    1.0,
                );
                target_ui
                    .toast_overlay
                    .add_toast(adw::Toast::new("Model deleted"));
                refresh_models(&target_ui, &target_backend);
                gtk::glib::ControlFlow::Break
            }
            Ok(ModelDeleteUiEvent::Failed(error)) => {
                target_backend.finish_model_delete();
                target_ui.refresh_button.set_sensitive(true);
                model_manager::set_delete_finished(
                    &target_ui.model_manager,
                    "Delete Failed",
                    &error,
                    0.0,
                );
                target_ui
                    .toast_overlay
                    .add_toast(adw::Toast::new(&format!("Model deletion failed: {error}")));
                gtk::glib::ControlFlow::Break
            }
            Err(mpsc::TryRecvError::Empty) => gtk::glib::ControlFlow::Continue,
            Err(mpsc::TryRecvError::Disconnected) => {
                target_backend.finish_model_delete();
                target_ui.refresh_button.set_sensitive(true);
                model_manager::set_delete_finished(
                    &target_ui.model_manager,
                    "Delete Stopped",
                    "The model deletion stopped before completion.",
                    0.0,
                );
                gtk::glib::ControlFlow::Break
            }
        }
    });
}

fn provider_is_local(provider: &Provider) -> bool {
    reqwest::Url::parse(&provider.base_url)
        .ok()
        .and_then(|url| url.host_str().map(str::to_string))
        .map(|host| {
            let host = host.trim_matches(['[', ']']);
            matches!(host, "localhost" | "127.0.0.1" | "::1")
        })
        .unwrap_or(false)
}

fn send_message(ui: &Rc<WindowUi>, backend: &Rc<Backend>) {
    let prompt = prompt_text(&ui.entry).trim().to_string();
    if prompt.is_empty() {
        return;
    }

    if backend.active_generation.borrow().is_some() {
        ui.toast_overlay
            .add_toast(adw::Toast::new("Generation is already running"));
        return;
    }

    if backend.active_model_pull.borrow().is_some() {
        ui.toast_overlay
            .add_toast(adw::Toast::new("Finish the active model download first"));
        return;
    }

    if backend.active_model_delete.borrow().is_some() {
        ui.toast_overlay
            .add_toast(adw::Toast::new("Finish the active model deletion first"));
        return;
    }

    let Some(model) = selected_model(&ui.model_picker, &ui.model_names) else {
        ui.toast_overlay
            .add_toast(adw::Toast::new("Select an installed model first"));
        return;
    };

    let client = match backend.client() {
        Ok(client) => client,
        Err(error) => {
            ui.toast_overlay.add_toast(adw::Toast::new(&format!(
                "Provider URL is invalid: {error}"
            )));
            return;
        }
    };

    let request =
        match ChatRequest::streaming(model.clone(), vec![ChatMessage::user(prompt.clone())]) {
            Ok(request) => request,
            Err(error) => {
                ui.toast_overlay.add_toast(adw::Toast::new(&format!(
                    "Message could not be sent: {error}"
                )));
                return;
            }
        };
    let (conversation_id, should_generate_title) = match ensure_active_conversation(backend) {
        Ok(result) => result,
        Err(error) => {
            ui.toast_overlay.add_toast(adw::Toast::new(&format!(
                "Conversation could not be saved: {error}"
            )));
            return;
        }
    };
    let assistant_message_id = match save_pending_exchange(backend, &conversation_id, &prompt) {
        Ok(assistant_message_id) => assistant_message_id,
        Err(error) => {
            ui.toast_overlay.add_toast(adw::Toast::new(&format!(
                "Message could not be saved: {error}"
            )));
            return;
        }
    };
    conversation_list::refresh(ui, backend);
    if should_generate_title {
        generate_conversation_title(
            ui,
            backend,
            client.clone(),
            conversation_id.clone(),
            prompt.clone(),
            model.clone(),
        );
    }

    backend.abort_generation();
    *backend.active_assistant_message_id.borrow_mut() = Some(assistant_message_id);
    backend.active_assistant_content.borrow_mut().clear();
    clear_prompt(&ui.entry);
    ui.message_stack.set_visible_child_name("messages");
    chat_view::append_message(&ui.messages, "You", &prompt);
    let assistant_label = chat_view::append_message(&ui.messages, &model, "");
    ui.send_button.set_sensitive(false);
    ui.stop_button.set_sensitive(true);

    let (sender, receiver) = mpsc::channel();
    let handle = backend.runtime.spawn(async move {
        let result = client
            .stream_chat(request, |event| match event {
                ChatStreamEvent::Token(token) => {
                    let _ = sender.send(ChatUiEvent::Token(token));
                }
                ChatStreamEvent::Done => {
                    let _ = sender.send(ChatUiEvent::Done);
                }
            })
            .await;

        if let Err(error) = result {
            let _ = sender.send(ChatUiEvent::Failed(error.to_string()));
        }
    });
    *backend.active_generation.borrow_mut() = Some(handle);

    let target_ui = Rc::clone(ui);
    let target_backend = Rc::clone(backend);
    gtk::glib::timeout_add_local(Duration::from_millis(30), move || {
        loop {
            match receiver.try_recv() {
                Ok(ChatUiEvent::Token(token)) => {
                    let mut content = target_backend.active_assistant_content.borrow_mut();
                    content.push_str(&token);
                    assistant_label.set_text(&content);
                }
                Ok(ChatUiEvent::Done) => {
                    finish_generation(&target_ui);
                    target_backend.active_generation.borrow_mut().take();
                    if let Err(error) = persist_active_assistant_message(
                        &target_backend,
                        AssistantMessageEnd::Complete,
                    ) {
                        target_ui.toast_overlay.add_toast(adw::Toast::new(&format!(
                            "Conversation could not be saved: {error}"
                        )));
                    }
                    conversation_list::refresh(&target_ui, &target_backend);
                    return gtk::glib::ControlFlow::Break;
                }
                Ok(ChatUiEvent::Failed(error)) => {
                    finish_generation(&target_ui);
                    target_backend.active_generation.borrow_mut().take();
                    if let Err(save_error) = persist_active_assistant_message(
                        &target_backend,
                        AssistantMessageEnd::Failed,
                    ) {
                        target_ui.toast_overlay.add_toast(adw::Toast::new(&format!(
                            "Conversation could not be saved: {save_error}"
                        )));
                    }
                    conversation_list::refresh(&target_ui, &target_backend);
                    target_ui
                        .toast_overlay
                        .add_toast(adw::Toast::new(&format!("Generation failed: {error}")));
                    return gtk::glib::ControlFlow::Break;
                }
                Err(mpsc::TryRecvError::Empty) => return gtk::glib::ControlFlow::Continue,
                Err(mpsc::TryRecvError::Disconnected) => {
                    finish_generation(&target_ui);
                    target_backend.active_generation.borrow_mut().take();
                    if let Err(error) = persist_active_assistant_message(
                        &target_backend,
                        AssistantMessageEnd::Cancelled,
                    ) {
                        target_ui.toast_overlay.add_toast(adw::Toast::new(&format!(
                            "Conversation could not be saved: {error}"
                        )));
                    }
                    conversation_list::refresh(&target_ui, &target_backend);
                    return gtk::glib::ControlFlow::Break;
                }
            }
        }
    });
}

fn create_empty_conversation(backend: &Backend) -> Result<String> {
    let provider = backend.provider.borrow().clone();
    let conversation = backend.conversation_repository.create(NewConversation {
        provider_id: provider.id,
        model_id: None,
        title: DEFAULT_CONVERSATION_TITLE.to_string(),
    })?;
    let conversation_id = conversation.id;
    *backend.active_conversation_id.borrow_mut() = Some(conversation_id.clone());
    backend.active_assistant_message_id.borrow_mut().take();
    backend.active_assistant_content.borrow_mut().clear();
    Ok(conversation_id)
}

fn load_conversation(ui: &WindowUi, backend: &Backend, conversation_id: &str) -> Result<()> {
    let messages = backend
        .conversation_repository
        .list_messages(conversation_id)?;

    clear_messages(ui);
    *backend.active_conversation_id.borrow_mut() = Some(conversation_id.to_string());
    backend.active_assistant_message_id.borrow_mut().take();
    backend.active_assistant_content.borrow_mut().clear();

    for message in &messages {
        chat_view::append_stored_message(&ui.messages, message);
    }

    if messages.is_empty() {
        set_chat_empty_state(ui, "Empty Conversation", "Send a message to begin.");
        ui.message_stack.set_visible_child_name("empty");
    } else {
        ui.message_stack.set_visible_child_name("messages");
    }

    Ok(())
}

fn ensure_active_conversation(backend: &Backend) -> Result<(String, bool)> {
    if let Some(conversation_id) = backend.active_conversation_id.borrow().clone() {
        let should_generate_title = should_generate_conversation_title(backend, &conversation_id)?;
        return Ok((conversation_id, should_generate_title));
    }

    let provider = backend.provider.borrow().clone();
    let conversation = backend.conversation_repository.create(NewConversation {
        provider_id: provider.id,
        model_id: None,
        title: DEFAULT_CONVERSATION_TITLE.to_string(),
    })?;
    let conversation_id = conversation.id;
    *backend.active_conversation_id.borrow_mut() = Some(conversation_id.clone());
    Ok((conversation_id, true))
}

fn should_generate_conversation_title(backend: &Backend, conversation_id: &str) -> Result<bool> {
    let Some(conversation) = backend.conversation_repository.get(conversation_id)? else {
        return Ok(false);
    };

    if !conversation.title.starts_with(DEFAULT_CONVERSATION_TITLE) {
        return Ok(false);
    }

    Ok(backend
        .conversation_repository
        .list_messages(conversation_id)?
        .is_empty())
}

fn generate_conversation_title(
    ui: &Rc<WindowUi>,
    backend: &Rc<Backend>,
    client: OllamaClient,
    conversation_id: String,
    prompt: String,
    fallback_model: String,
) {
    let (sender, receiver) = mpsc::channel();
    backend.runtime.spawn(async move {
        let event = match generate_model_title(client, &fallback_model, &prompt).await {
            Ok(title) => TitleUiEvent::Generated(title),
            Err(_) => TitleUiEvent::Failed,
        };
        let _ = sender.send(event);
    });

    let target_ui = Rc::clone(ui);
    let target_backend = Rc::clone(backend);
    gtk::glib::timeout_add_local(Duration::from_millis(80), move || {
        match receiver.try_recv() {
            Ok(TitleUiEvent::Generated(title)) => {
                let _ = apply_generated_conversation_title(
                    &target_ui,
                    &target_backend,
                    &conversation_id,
                    &title,
                );
                gtk::glib::ControlFlow::Break
            }
            Ok(TitleUiEvent::Failed) | Err(mpsc::TryRecvError::Disconnected) => {
                gtk::glib::ControlFlow::Break
            }
            Err(mpsc::TryRecvError::Empty) => gtk::glib::ControlFlow::Continue,
        }
    });
}

async fn generate_model_title(
    client: OllamaClient,
    fallback_model: &str,
    prompt: &str,
) -> Result<String> {
    let mut request = ChatRequest::streaming_with_temperature(
        fallback_model,
        vec![
            ChatMessage::system(TITLE_SYSTEM_PROMPT),
            ChatMessage::user(format!(
                "Generate a concise title for this chat prompt. Return only JSON using this shape: {{\"title\":\"string\"}}.\n\nPrompt:\n{prompt}"
            )),
        ],
        0.2,
    )?;
    request.format = Some("json".to_string());
    let mut response = String::new();
    client
        .stream_chat(request, |event| {
            if let ChatStreamEvent::Token(token) = event {
                response.push_str(&token);
            }
        })
        .await?;
    parse_generated_title(&response)
}

fn parse_generated_title(response: &str) -> Result<String> {
    let trimmed = response.trim();
    let json = if trimmed.starts_with('{') && trimmed.ends_with('}') {
        trimmed
    } else if let (Some(start), Some(end)) = (trimmed.find('{'), trimmed.rfind('}')) {
        &trimmed[start..=end]
    } else {
        trimmed
    };
    let generated: GeneratedTitle = serde_json::from_str(json)?;
    let title = generated
        .title
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let title = title.trim();
    if title.is_empty() {
        return Err(crate::error::MooseError::InvalidConversationTitle);
    }
    Ok(truncate_title(title))
}

fn truncate_title(title: &str) -> String {
    let count = title.chars().count();
    if count <= TITLE_MAX_CHARS {
        return title.to_string();
    }

    let mut truncated = title
        .chars()
        .take(TITLE_MAX_CHARS.saturating_sub(3))
        .collect::<String>();
    truncated.push_str("...");
    truncated
}

fn apply_generated_conversation_title(
    ui: &Rc<WindowUi>,
    backend: &Rc<Backend>,
    conversation_id: &str,
    title: &str,
) -> Result<()> {
    let Some(conversation) = backend.conversation_repository.get(conversation_id)? else {
        return Ok(());
    };

    if !conversation.title.starts_with(DEFAULT_CONVERSATION_TITLE) {
        return Ok(());
    }

    let title = numbered_conversation_title(backend, conversation_id, title)?;
    backend
        .conversation_repository
        .update_title(ConversationTitleUpdate {
            id: conversation_id.to_string(),
            title,
        })?;
    conversation_list::refresh(ui, backend);
    if backend.active_conversation_id.borrow().as_deref() == Some(conversation_id) {
        conversation_list::select(ui, conversation_id);
    }
    Ok(())
}

fn numbered_conversation_title(
    backend: &Backend,
    conversation_id: &str,
    title: &str,
) -> Result<String> {
    let existing_titles = backend
        .conversation_repository
        .list_recent(500)?
        .into_iter()
        .filter(|conversation| conversation.id != conversation_id)
        .map(|conversation| conversation.title)
        .collect::<Vec<_>>();

    if !existing_titles.iter().any(|existing| existing == title) {
        return Ok(title.to_string());
    }

    for number in 2.. {
        let suffix = format!(" {number}");
        let base_limit = TITLE_MAX_CHARS.saturating_sub(suffix.chars().count());
        let mut candidate = title.chars().take(base_limit).collect::<String>();
        candidate.push_str(&suffix);
        if !existing_titles
            .iter()
            .any(|existing| existing == &candidate)
        {
            return Ok(candidate);
        }
    }

    Ok(title.to_string())
}

fn save_pending_exchange(backend: &Backend, conversation_id: &str, prompt: &str) -> Result<String> {
    backend
        .conversation_repository
        .create_message(NewMessage::user(conversation_id, prompt))?;
    let assistant_message = backend
        .conversation_repository
        .create_message(NewMessage::assistant_streaming(conversation_id))?;
    Ok(assistant_message.id)
}

fn persist_active_assistant_message(backend: &Backend, end: AssistantMessageEnd) -> Result<()> {
    let Some(message_id) = backend.active_assistant_message_id.borrow_mut().take() else {
        backend.active_assistant_content.borrow_mut().clear();
        return Ok(());
    };

    let content = backend.active_assistant_content.borrow().clone();
    let update = match end {
        AssistantMessageEnd::Complete => MessageUpdate::completed(message_id, content),
        AssistantMessageEnd::Cancelled => MessageUpdate::cancelled(message_id, content),
        AssistantMessageEnd::Failed => MessageUpdate::failed(message_id, content),
    };
    backend.conversation_repository.update_message(update)?;
    backend.active_assistant_content.borrow_mut().clear();
    Ok(())
}

fn finish_generation(ui: &WindowUi) {
    ui.stop_button.set_sensitive(false);
    update_send_button(ui);
}

fn prompt_text(entry: &gtk::TextView) -> String {
    let buffer = entry.buffer();
    let (start, end) = buffer.bounds();
    buffer.text(&start, &end, true).to_string()
}

fn clear_prompt(entry: &gtk::TextView) {
    entry.buffer().set_text("");
}

fn prompt_is_ready(entry: &gtk::TextView) -> bool {
    !prompt_text(entry).trim().is_empty()
}

fn update_send_button(ui: &WindowUi) {
    let can_send = selected_model(&ui.model_picker, &ui.model_names).is_some()
        && prompt_is_ready(&ui.entry)
        && !ui.stop_button.is_sensitive();
    ui.send_button.set_sensitive(can_send);
}

fn selected_model(
    dropdown: &gtk::DropDown,
    model_names: &Rc<RefCell<Vec<String>>>,
) -> Option<String> {
    let selected = usize::try_from(dropdown.selected()).ok()?;
    model_names.borrow().get(selected).cloned()
}

fn set_models(ui: &Rc<WindowUi>, backend: &Rc<Backend>, models: Vec<OllamaModel>) {
    let chat_models = models
        .iter()
        .filter(|model| model.supports_chat)
        .map(|model| model.name.clone())
        .collect();
    set_model_picker(ui, chat_models);
    set_installed_models(ui, backend, models);
}

fn set_installed_models(ui: &Rc<WindowUi>, backend: &Rc<Backend>, models: Vec<OllamaModel>) {
    *ui.installed_models.borrow_mut() = models;
    let query = ui.model_manager.search_entry.text().to_string();
    render_model_manager(ui, backend, &query);
}

fn render_model_manager(ui: &Rc<WindowUi>, backend: &Rc<Backend>, query: &str) {
    let installed_models = ui.installed_models.borrow();
    let target_parent = ui.window.clone();
    let target_ui = Rc::clone(ui);
    let target_backend = Rc::clone(backend);
    let on_pull = Rc::new(move |model: String| {
        request_model_pull(&target_parent, &target_ui, &target_backend, model);
    });
    let target_parent = ui.window.clone();
    let target_ui = Rc::clone(ui);
    let target_backend = Rc::clone(backend);
    let on_delete = Rc::new(move |model: String| {
        confirm_model_delete(&target_parent, &target_ui, &target_backend, model);
    });
    model_manager::set_models(
        &ui.model_manager,
        &ui.window,
        installed_models.as_slice(),
        query,
        on_pull,
        on_delete,
    );
}

fn set_model_picker(ui: &WindowUi, models: Vec<String>) {
    let is_empty = models.is_empty();
    *ui.model_names.borrow_mut() = models;

    if is_empty {
        let list = gtk::StringList::new(&["No model selected"]);
        ui.model_picker.set_model(Some(&list));
        ui.model_picker.set_selected(0);
        ui.model_picker.set_sensitive(false);
        update_send_button(ui);
        return;
    }

    let borrowed = ui.model_names.borrow();
    let labels = borrowed.iter().map(String::as_str).collect::<Vec<_>>();
    let list = gtk::StringList::new(&labels);
    ui.model_picker.set_model(Some(&list));
    ui.model_picker.set_selected(0);
    ui.model_picker.set_sensitive(true);
    update_send_button(ui);
}

fn set_chat_empty_state(ui: &WindowUi, title: &str, description: &str) {
    chat_view::set_empty_state(&ui.chat_status_page, title, description);
}

fn show_chat(ui: &WindowUi) {
    ui.content_stack.set_visible_child_name("chat");
}

fn show_model_manager(ui: &WindowUi) {
    ui.content_stack.set_visible_child_name("models");
}

fn clear_messages(ui: &WindowUi) {
    while let Some(child) = ui.messages.first_child() {
        ui.messages.remove(&child);
    }
    clear_prompt(&ui.entry);
    ui.stop_button.set_sensitive(false);
    update_send_button(ui);
    set_chat_empty_state(
        ui,
        "No Conversation Selected",
        "Choose a model and start a conversation.",
    );
    ui.message_stack.set_visible_child_name("empty");
}

fn update_provider_summary(ui: &WindowUi, provider: &Provider) {
    ui.provider_row.set_title(&provider.name);
    ui.provider_row.set_subtitle("");
    ui.provider_row.set_tooltip_text(Some(&provider.base_url));
}

fn show_error(parent: &adw::ApplicationWindow, heading: &str, error: &dyn std::error::Error) {
    let dialog = adw::AlertDialog::builder()
        .heading(heading)
        .body(error.to_string())
        .close_response("ok")
        .default_response("ok")
        .build();
    dialog.add_response("ok", "OK");
    dialog.present(Some(parent));
}

fn install_chat_css() {
    let provider = gtk::CssProvider::new();
    let css = option_env!("MOOSE_STYLE_PATH")
        .and_then(|path| fs::read_to_string(path).ok())
        .unwrap_or_else(|| CHAT_CSS.to_string());
    provider.load_from_string(&css);
    if let Some(display) = gtk::gdk::Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
}
