use std::{
    cell::RefCell,
    collections::HashMap,
    fs,
    rc::Rc,
    sync::{Arc, mpsc},
    time::Duration,
};

use adw::prelude::*;
use gtk::gio;
use serde::Deserialize;

use crate::{
    APPLICATION_ID, APPLICATION_NAME,
    chat::{ChatMessage, ChatRequest, ChatStreamEvent},
    conversations::{
        ConversationTitleUpdate, DEFAULT_CONVERSATION_TITLE, Message, MessageUpdate,
        NewConversation, NewMessage,
    },
    error::{MooseError, Result},
    ollama::{OllamaClient, OllamaModel, service::ManagedOllamaService},
    platform::AppPaths,
    providers::{MANAGED_OLLAMA_DEFAULT_PORT, Provider, managed_ollama_port_from_base_url},
    storage::{ConversationRepository, DownloadJobRepository, ProviderRepository, open_database},
};

mod chat_view;
mod code_view;
mod conversation_list;
mod first_run;
mod managed_install;
mod markdown_live;
mod markdown_view;
mod model_actions;
mod model_manager;
mod preferences;
mod provider_controls;
mod sidebar;
mod widgets;

use provider_controls::show_connect_external_dialog;

const CHAT_CSS: &str = include_str!("../../data/io.github.moooossee.Moose.css");
const TITLE_SYSTEM_PROMPT: &str = "You are an assistant that generates short chat titles based on the prompt. If you want to, you can add a single emoji. Format the response as a single JSON object.";
const TITLE_MAX_CHARS: usize = 30;
const SETTINGS_SELECTED_MODELS: &str = "selected-models";
const MANAGED_OLLAMA_READY_TIMEOUT: Duration = Duration::from_secs(20);
const STARTER_MODEL: &str = "llama3.2:1b";

type ManagedOllamaHandle = Arc<tokio::sync::Mutex<ManagedOllamaService>>;

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
    let first_run_guide = first_run::build();
    let new_chat_button = sidebar.new_chat_button.clone();
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

    let root_stack = gtk::Stack::builder().hexpand(true).vexpand(true).build();
    root_stack.set_transition_type(gtk::StackTransitionType::Crossfade);
    root_stack.add_named(&first_run_guide.root, Some("guide"));
    root_stack.add_named(&split_view, Some("app"));
    root_stack.set_visible_child_name("app");

    window.set_content(Some(&root_stack));
    window.set_size_request(768, 520);

    let model_names = Rc::new(RefCell::new(Vec::new()));
    let installed_models = Rc::new(RefCell::new(Vec::new()));
    let conversation_ids = Rc::new(RefCell::new(Vec::new()));
    let ui = Rc::new(WindowUi {
        window: window.clone(),
        root_stack,
        toast_overlay,
        content_stack,
        model_manager,
        provider_row: sidebar.provider_row,
        provider_status: sidebar.provider_status,
        provider_switch_button: sidebar.provider_switch_button,
        refresh_button: sidebar.refresh_button,
        model_picker: chat.model_picker,
        conversation_list: sidebar.conversation_list,
        messages: chat.messages,
        messages_scrolled: chat.messages_scrolled,
        chat_status_page: chat.status_page,
        message_stack: chat.message_stack,
        entry: chat.entry,
        send_button: chat.send_button,
        stop_button: chat.stop_button,
        model_names,
        installed_models,
        conversation_ids,
        first_run_guide,
        restoring_model_selection: RefCell::new(false),
        restoring_conversation_selection: RefCell::new(false),
    });

    bind_sidebar_visibility(&split_view, &sidebar_toggle_button);

    match Backend::new() {
        Ok(backend) => {
            let backend = Rc::new(backend);
            let provider = active_provider(&backend);
            apply_provider_state(&ui, &provider);
            bind_actions(
                &window,
                &ui,
                &backend,
                &new_chat_button,
                &model_manager_button,
                &preferences_button,
            );
            if provider.is_some() {
                refresh_models(&ui, &backend);
            } else {
                show_first_run_guide(&ui);
            }
            conversation_list::refresh(&ui, &backend);
        }
        Err(error) => {
            ui.provider_status.set_text("Storage Error");
            ui.send_button.set_sensitive(false);
            ui.stop_button.set_sensitive(false);
            ui.refresh_button.set_sensitive(false);
            ui.model_manager.refresh_button.set_sensitive(false);
            ui.model_manager.download_jobs_button.set_sensitive(false);
            ui.toast_overlay
                .add_toast(adw::Toast::new(&format!("Storage setup failed: {error}")));
        }
    }

    window
}

struct Backend {
    paths: AppPaths,
    repository: ProviderRepository,
    conversation_repository: ConversationRepository,
    download_job_repository: DownloadJobRepository,
    provider: RefCell<Option<Provider>>,
    managed_ollama: ManagedOllamaHandle,
    settings: Option<gio::Settings>,
    selected_models: RefCell<HashMap<String, String>>,
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
    root_stack: gtk::Stack,
    toast_overlay: adw::ToastOverlay,
    content_stack: gtk::Stack,
    model_manager: model_manager::ModelManager,
    provider_row: adw::ActionRow,
    provider_status: gtk::Label,
    provider_switch_button: gtk::Button,
    refresh_button: gtk::Button,
    model_picker: gtk::DropDown,
    conversation_list: gtk::ListBox,
    messages: gtk::Box,
    messages_scrolled: gtk::ScrolledWindow,
    chat_status_page: adw::StatusPage,
    message_stack: gtk::Stack,
    entry: gtk::TextView,
    send_button: gtk::Button,
    stop_button: gtk::Button,
    model_names: Rc<RefCell<Vec<String>>>,
    installed_models: Rc<RefCell<Vec<OllamaModel>>>,
    conversation_ids: Rc<RefCell<Vec<String>>>,
    first_run_guide: first_run::FirstRunGuide,
    restoring_model_selection: RefCell<bool>,
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

enum TitleUiEvent {
    Generated(String),
    Failed,
}

enum AssistantMessageEnd {
    Complete,
    Cancelled,
    Failed,
}

struct PendingExchange {
    user: Message,
    assistant: Message,
}

#[derive(Deserialize)]
struct GeneratedTitle {
    title: String,
}

fn app_settings() -> Option<gio::Settings> {
    gio::SettingsSchemaSource::default()
        .and_then(|source| source.lookup(APPLICATION_ID, true))
        .map(|schema| gio::Settings::new_full(&schema, gio::SettingsBackend::NONE, None))
}

impl Backend {
    fn new() -> Result<Self> {
        let paths = AppPaths::new("moose")?;
        paths.create_all()?;
        let connection = Rc::new(open_database(paths.database_path())?);
        let repository = ProviderRepository::new(Rc::clone(&connection));
        let conversation_repository = ConversationRepository::new(Rc::clone(&connection));
        let download_job_repository = DownloadJobRepository::new(connection);
        let provider = repository.ensure_default_provider()?;
        let managed_ollama = Arc::new(tokio::sync::Mutex::new(ManagedOllamaService::new(&paths)));
        let settings = app_settings();
        let selected_models = settings
            .as_ref()
            .map(|settings| settings.get(SETTINGS_SELECTED_MODELS))
            .unwrap_or_default();
        download_job_repository.fail_active_jobs("Download interrupted.")?;
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()?;

        Ok(Self {
            paths,
            repository,
            conversation_repository,
            download_job_repository,
            provider: RefCell::new(provider),
            managed_ollama,
            settings,
            selected_models: RefCell::new(selected_models),
            runtime,
            active_generation: RefCell::new(None),
            active_model_pull: RefCell::new(None),
            active_model_delete: RefCell::new(None),
            active_conversation_id: RefCell::new(None),
            active_assistant_message_id: RefCell::new(None),
            active_assistant_content: RefCell::new(String::new()),
        })
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

    fn cancel_model_pull(&self) -> Option<String> {
        if let Some(active) = self.active_model_pull.borrow_mut().take() {
            active.handle.abort();
            Some(active.id)
        } else {
            None
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

    fn stop_managed_ollama(&self) {
        let managed_ollama = Arc::clone(&self.managed_ollama);
        self.runtime.spawn(async move {
            managed_ollama.lock().await.shutdown();
        });
    }
}

async fn prepared_ollama_client(
    paths: AppPaths,
    managed_ollama: ManagedOllamaHandle,
    provider: Provider,
) -> Result<OllamaClient> {
    if !provider.is_managed {
        return OllamaClient::new(&provider.base_url);
    }

    let port = managed_ollama_port_from_base_url(&provider.base_url)?;
    let mut service = managed_ollama.lock().await;
    if service.config().base_url != provider.base_url {
        service.shutdown();
        *service = ManagedOllamaService::new_with_port(&paths, port)?;
    }
    service.ensure_ready(MANAGED_OLLAMA_READY_TIMEOUT).await?;
    OllamaClient::new(&service.config().base_url)
}

fn active_provider(backend: &Backend) -> Option<Provider> {
    backend.provider.borrow().clone()
}

fn require_active_provider(ui: &WindowUi, backend: &Backend) -> Option<Provider> {
    let provider = active_provider(backend);
    if provider.is_none() {
        ui.toast_overlay
            .add_toast(adw::Toast::new("Create or connect an instance first"));
    }
    provider
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
    ui.provider_switch_button.connect_clicked(move |button| {
        provider_controls::show_switcher(button, &target_ui, &target_backend);
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
        model_actions::show_pull_dialog(&parent, &target_ui, &target_backend);
    });

    let parent = window.clone();
    let target_ui = Rc::clone(ui);
    let target_backend = Rc::clone(backend);
    ui.model_manager
        .download_jobs_button
        .connect_clicked(move |_| {
            model_actions::show_download_jobs_dialog(&parent, &target_ui, &target_backend);
        });

    let target_ui = Rc::clone(ui);
    let target_backend = Rc::clone(backend);
    ui.model_manager
        .pull_cancel_button
        .connect_clicked(move |_| {
            if let Some(job_id) = target_backend.cancel_model_pull() {
                if let Err(error) = target_backend.download_job_repository.cancel(&job_id) {
                    target_ui.toast_overlay.add_toast(adw::Toast::new(&format!(
                        "Download job could not be saved: {error}"
                    )));
                }
                target_ui.refresh_button.set_sensitive(true);
                model_manager::set_pull_finished(
                    &target_ui.model_manager,
                    "Download Cancelled",
                    "The model download was cancelled.",
                    0.0,
                );
                model_manager::clear_download_job(&target_ui.model_manager);
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
    let target_backend = Rc::clone(backend);
    ui.model_picker.connect_selected_notify(move |_| {
        if *target_ui.restoring_model_selection.borrow() {
            return;
        }

        if let Some(model) = selected_model(&target_ui.model_picker, &target_ui.model_names) {
            save_selected_model(&target_ui, &target_backend, model);
        }

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

    let target_stack = ui.first_run_guide.stack.clone();
    ui.first_run_guide.start_button.connect_clicked(move |_| {
        target_stack.set_visible_child_name("instances");
    });

    let target_ui = Rc::clone(ui);
    let target_backend = Rc::clone(backend);
    ui.first_run_guide.create_button.connect_clicked(move |_| {
        managed_install::show_dialog(&target_ui, &target_backend);
    });

    let target_ui = Rc::clone(ui);
    let target_backend = Rc::clone(backend);
    ui.first_run_guide.connect_button.connect_clicked(move |_| {
        show_connect_external_dialog(&target_ui, &target_backend);
    });
}

fn provider_change_is_blocked(ui: &Rc<WindowUi>, backend: &Rc<Backend>) -> bool {
    if backend.active_generation.borrow().is_some() {
        ui.toast_overlay
            .add_toast(adw::Toast::new("Finish the active generation first"));
        return true;
    }

    if backend.active_model_pull.borrow().is_some() {
        ui.toast_overlay
            .add_toast(adw::Toast::new("Finish the active model download first"));
        return true;
    }

    if backend.active_model_delete.borrow().is_some() {
        ui.toast_overlay
            .add_toast(adw::Toast::new("Finish the active model deletion first"));
        return true;
    }

    false
}

fn apply_active_provider(ui: &Rc<WindowUi>, backend: &Rc<Backend>, provider: Provider) {
    *backend.provider.borrow_mut() = Some(provider.clone());
    if !provider.is_managed {
        backend.stop_managed_ollama();
    }
    reset_active_conversation(ui, backend);
    apply_provider_state(ui, &Some(provider.clone()));
    model_manager::clear_download_job(&ui.model_manager);
    refresh_models(ui, backend);
    conversation_list::refresh(ui, backend);
}

fn clear_active_provider(ui: &Rc<WindowUi>, backend: &Rc<Backend>) {
    *backend.provider.borrow_mut() = None;
    backend.stop_managed_ollama();
    reset_active_conversation(ui, backend);
    apply_provider_state(ui, &None);
    set_model_picker(ui, Vec::new(), None);
    set_installed_models(ui, backend, Vec::new());
    model_manager::set_unavailable(
        &ui.model_manager,
        "No Instance",
        "Create or connect an Ollama instance to manage models.",
    );
    ui.model_manager.refresh_button.set_sensitive(false);
    ui.model_manager.pull_button.set_sensitive(false);
    show_first_run_guide(ui);
}

fn reset_active_conversation(ui: &WindowUi, backend: &Backend) {
    backend.active_conversation_id.borrow_mut().take();
    backend.active_assistant_message_id.borrow_mut().take();
    backend.active_assistant_content.borrow_mut().clear();
    ui.conversation_list.unselect_all();
    clear_messages(ui);
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

    let Some(provider) = active_provider(backend) else {
        apply_provider_state(ui, &None);
        set_model_picker(ui, Vec::new(), None);
        set_installed_models(ui, backend, Vec::new());
        model_manager::set_unavailable(
            &ui.model_manager,
            "No Instance",
            "Create or connect an Ollama instance to refresh models.",
        );
        ui.model_manager.refresh_button.set_sensitive(false);
        ui.model_manager.pull_button.set_sensitive(false);
        show_first_run_guide(ui);
        return;
    };
    let provider_is_managed = provider.is_managed;
    ui.provider_status.set_text(if provider.is_managed {
        "Starting"
    } else {
        "Checking"
    });
    ui.refresh_button.set_sensitive(false);
    ui.model_manager.refresh_button.set_sensitive(false);
    set_model_picker(ui, Vec::new(), None);
    set_installed_models(ui, backend, Vec::new());
    model_manager::set_loading(&ui.model_manager);

    let (sender, receiver) = mpsc::channel();
    let paths = backend.paths.clone();
    let managed_ollama = Arc::clone(&backend.managed_ollama);
    backend.runtime.spawn(async move {
        let client = match prepared_ollama_client(paths, managed_ollama, provider).await {
            Ok(client) => client,
            Err(error) => {
                let _ = sender.send(ModelLoadEvent::Failed(error.to_string()));
                return;
            }
        };
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
                    if provider_is_managed {
                        "Ready"
                    } else {
                        "Connected"
                    }
                } else {
                    "Disconnected"
                });
                if !available {
                    if !provider_is_managed {
                        target_ui
                            .toast_overlay
                            .add_toast(adw::Toast::new(&format!("Ollama unavailable: {status}")));
                    }
                    set_model_picker(&target_ui, Vec::new(), None);
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
                set_model_picker(&target_ui, Vec::new(), None);
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
                set_model_picker(&target_ui, Vec::new(), None);
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

    let Some(provider) = require_active_provider(ui, backend) else {
        return;
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
    let pending_exchange = match save_pending_exchange(backend, &conversation_id, &prompt) {
        Ok(pending_exchange) => pending_exchange,
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
            conversation_id.clone(),
            prompt.clone(),
            model.clone(),
        );
    }

    backend.abort_generation();
    let assistant_message_id = pending_exchange.assistant.id.clone();
    *backend.active_assistant_message_id.borrow_mut() = Some(assistant_message_id);
    backend.active_assistant_content.borrow_mut().clear();
    clear_prompt(&ui.entry);
    ui.message_stack.set_visible_child_name("messages");
    chat_view::append_message(
        &ui.messages,
        "You",
        &prompt,
        Some(&pending_exchange.user.created_at),
    );
    let assistant_message = chat_view::append_streaming_message(
        &ui.messages,
        &model,
        Some(&pending_exchange.assistant.created_at),
    );
    scroll_chat_to_bottom(ui);
    ui.send_button.set_sensitive(false);
    ui.stop_button.set_sensitive(true);

    let (sender, receiver) = mpsc::channel();
    let paths = backend.paths.clone();
    let managed_ollama = Arc::clone(&backend.managed_ollama);
    let handle = backend.runtime.spawn(async move {
        let client = match prepared_ollama_client(paths, managed_ollama, provider).await {
            Ok(client) => client,
            Err(error) => {
                let _ = sender.send(ChatUiEvent::Failed(error.to_string()));
                return;
            }
        };
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
    gtk::glib::timeout_add_local(Duration::from_millis(80), move || {
        let mut content_changed = false;

        loop {
            match receiver.try_recv() {
                Ok(ChatUiEvent::Token(token)) => {
                    let mut content = target_backend.active_assistant_content.borrow_mut();
                    content.push_str(&token);
                    content_changed = true;
                }
                Ok(ChatUiEvent::Done) => {
                    if target_backend
                        .active_assistant_content
                        .borrow()
                        .trim()
                        .is_empty()
                    {
                        *target_backend.active_assistant_content.borrow_mut() =
                            "No response generated.".to_string();
                    }
                    let display_content = target_backend.active_assistant_content.borrow().clone();
                    chat_view::set_streaming_message_content(&assistant_message, &display_content);
                    scroll_chat_to_bottom(&target_ui);
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
                    let display_content = {
                        let content = target_backend.active_assistant_content.borrow();
                        message_content_with_live_state(content.as_str(), "Response failed.")
                    };
                    chat_view::set_streaming_message_content(&assistant_message, &display_content);
                    scroll_chat_to_bottom(&target_ui);
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
                Err(mpsc::TryRecvError::Empty) => {
                    if content_changed {
                        let should_scroll =
                            chat_view::should_stick_to_bottom(&target_ui.messages_scrolled);
                        let content = target_backend.active_assistant_content.borrow();
                        chat_view::update_streaming_message_content(
                            &assistant_message,
                            content.as_str(),
                        );
                        if should_scroll {
                            scroll_chat_to_bottom(&target_ui);
                        }
                    }
                    return gtk::glib::ControlFlow::Continue;
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    let display_content = {
                        let content = target_backend.active_assistant_content.borrow();
                        message_content_with_live_state(content.as_str(), "Response cancelled.")
                    };
                    chat_view::set_streaming_message_content(&assistant_message, &display_content);
                    scroll_chat_to_bottom(&target_ui);
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

fn message_content_with_live_state(content: &str, state: &str) -> String {
    if content.trim().is_empty() {
        state.to_string()
    } else {
        format!("{content}\n\n{state}")
    }
}

fn create_empty_conversation(backend: &Backend) -> Result<String> {
    let provider = active_provider(backend).ok_or(MooseError::ProviderNotConfigured)?;
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
        scroll_chat_to_bottom(ui);
    }

    Ok(())
}

fn ensure_active_conversation(backend: &Backend) -> Result<(String, bool)> {
    if let Some(conversation_id) = backend.active_conversation_id.borrow().clone() {
        let should_generate_title = should_generate_conversation_title(backend, &conversation_id)?;
        return Ok((conversation_id, should_generate_title));
    }

    let provider = active_provider(backend).ok_or(MooseError::ProviderNotConfigured)?;
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
    conversation_id: String,
    prompt: String,
    fallback_model: String,
) {
    let (sender, receiver) = mpsc::channel();
    let Some(provider) = active_provider(backend) else {
        return;
    };
    let paths = backend.paths.clone();
    let managed_ollama = Arc::clone(&backend.managed_ollama);
    backend.runtime.spawn(async move {
        let event = match prepared_ollama_client(paths, managed_ollama, provider).await {
            Ok(client) => match generate_model_title(client, &fallback_model, &prompt).await {
                Ok(title) => TitleUiEvent::Generated(title),
                Err(_) => TitleUiEvent::Failed,
            },
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

fn save_pending_exchange(
    backend: &Backend,
    conversation_id: &str,
    prompt: &str,
) -> Result<PendingExchange> {
    let user = backend
        .conversation_repository
        .create_message(NewMessage::user(conversation_id, prompt))?;
    let assistant_message = backend
        .conversation_repository
        .create_message(NewMessage::assistant_streaming(conversation_id))?;
    Ok(PendingExchange {
        user,
        assistant: assistant_message,
    })
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

fn scroll_chat_to_bottom(ui: &WindowUi) {
    chat_view::scroll_to_bottom(&ui.messages_scrolled);
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

fn save_selected_model(ui: &WindowUi, backend: &Backend, model: String) {
    let Some(provider_id) = active_provider(backend).map(|provider| provider.id) else {
        return;
    };

    let selected_models = {
        let mut selected_models = backend.selected_models.borrow_mut();
        selected_models.insert(provider_id, model);
        selected_models.clone()
    };

    let Some(settings) = backend.settings.as_ref() else {
        return;
    };

    if settings
        .set(SETTINGS_SELECTED_MODELS, selected_models)
        .is_err()
    {
        ui.toast_overlay
            .add_toast(adw::Toast::new("Selected model could not be saved"));
    }
}

fn set_models(ui: &Rc<WindowUi>, backend: &Rc<Backend>, models: Vec<OllamaModel>) {
    let is_empty = models.is_empty();
    let chat_models = models
        .iter()
        .filter(|model| model.supports_chat)
        .map(|model| model.name.clone())
        .collect();
    let selected_model = active_provider(backend)
        .and_then(|provider| backend.selected_models.borrow().get(&provider.id).cloned());
    set_model_picker(ui, chat_models, selected_model.as_deref());
    set_installed_models(ui, backend, models);
    if is_empty {
        show_no_models_state(ui, backend);
    } else if ui.message_stack.visible_child_name().as_deref() == Some("empty") {
        set_chat_empty_state(
            ui,
            "No Conversation Selected",
            "Choose a model and start a conversation.",
        );
    }
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
        model_actions::request_model_pull(&target_parent, &target_ui, &target_backend, model);
    });
    let target_parent = ui.window.clone();
    let target_ui = Rc::clone(ui);
    let target_backend = Rc::clone(backend);
    let on_delete = Rc::new(move |model: String| {
        model_actions::confirm_model_delete(&target_parent, &target_ui, &target_backend, model);
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

fn set_model_picker(ui: &WindowUi, models: Vec<String>, selected_model: Option<&str>) {
    let is_empty = models.is_empty();
    *ui.model_names.borrow_mut() = models;
    *ui.restoring_model_selection.borrow_mut() = true;

    if is_empty {
        let list = gtk::StringList::new(&["No model selected"]);
        ui.model_picker.set_model(Some(&list));
        ui.model_picker.set_selected(0);
        ui.model_picker.set_sensitive(false);
        *ui.restoring_model_selection.borrow_mut() = false;
        update_send_button(ui);
        return;
    }

    let (list, selected) = {
        let borrowed = ui.model_names.borrow();
        let labels = borrowed.iter().map(String::as_str).collect::<Vec<_>>();
        let selected = selected_model
            .and_then(|model| borrowed.iter().position(|candidate| candidate == model))
            .unwrap_or(0);

        (gtk::StringList::new(&labels), selected)
    };

    ui.model_picker.set_model(Some(&list));
    ui.model_picker.set_selected(selected as u32);
    ui.model_picker.set_sensitive(true);
    *ui.restoring_model_selection.borrow_mut() = false;
    update_send_button(ui);
}

fn set_chat_empty_state(ui: &WindowUi, title: &str, description: &str) {
    chat_view::set_empty_state(&ui.chat_status_page, title, description);
    ui.chat_status_page.set_child(None::<&gtk::Widget>);
}

fn show_no_models_state(ui: &Rc<WindowUi>, backend: &Rc<Backend>) {
    chat_view::set_empty_state(
        &ui.chat_status_page,
        "No Models Installed",
        "Download Llama 3.2 1B to start, or browse the model library.",
    );

    let download_button = gtk::Button::with_label("Download Llama 3.2 1B");
    download_button.add_css_class("suggested-action");
    download_button.add_css_class("moose-empty-action");
    let browse_button = gtk::Button::with_label("Browse Models");
    browse_button.add_css_class("flat");
    browse_button.add_css_class("moose-empty-action");

    let actions = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(6)
        .halign(gtk::Align::Center)
        .build();
    actions.append(&download_button);
    actions.append(&browse_button);

    let target_parent = ui.window.clone();
    let target_ui = Rc::clone(ui);
    let target_backend = Rc::clone(backend);
    download_button.connect_clicked(move |_| {
        model_actions::request_model_pull(
            &target_parent,
            &target_ui,
            &target_backend,
            STARTER_MODEL.to_string(),
        );
    });

    let target_ui = Rc::clone(ui);
    browse_button.connect_clicked(move |_| {
        show_model_manager(&target_ui);
    });

    ui.chat_status_page.set_child(Some(&actions));
    ui.message_stack.set_visible_child_name("empty");
    show_chat(ui);
}

fn show_chat(ui: &WindowUi) {
    ui.root_stack.set_visible_child_name("app");
    ui.content_stack.set_visible_child_name("chat");
}

fn show_model_manager(ui: &WindowUi) {
    ui.root_stack.set_visible_child_name("app");
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

fn show_first_run_guide(ui: &WindowUi) {
    first_run::reset(&ui.first_run_guide);
    ui.root_stack.set_visible_child_name("guide");
}

fn apply_provider_state(ui: &WindowUi, provider: &Option<Provider>) {
    match provider {
        Some(provider) => {
            update_provider_summary(ui, provider);
            ui.refresh_button.set_sensitive(true);
            ui.model_manager.refresh_button.set_sensitive(true);
            ui.model_manager.pull_button.set_sensitive(true);
            ui.model_manager.download_jobs_button.set_sensitive(true);
            update_send_button(ui);
        }
        None => {
            ui.provider_row.set_title("No Instance");
            ui.provider_row.set_subtitle("First-run guide required");
            ui.provider_row.set_tooltip_text(None);
            ui.provider_status.set_text("Not Set");
            ui.refresh_button.set_sensitive(false);
            ui.model_manager.refresh_button.set_sensitive(false);
            ui.model_manager.pull_button.set_sensitive(false);
            ui.model_manager.download_jobs_button.set_sensitive(false);
            ui.send_button.set_sensitive(false);
            ui.stop_button.set_sensitive(false);
        }
    }
}

fn update_provider_summary(ui: &WindowUi, provider: &Provider) {
    ui.provider_row.set_title(&provider.name);
    let subtitle = if provider.is_managed {
        match managed_ollama_port_from_base_url(&provider.base_url) {
            Ok(MANAGED_OLLAMA_DEFAULT_PORT) => "Managed by Moose".to_string(),
            Ok(port) => format!("Managed on 127.0.0.1:{port}"),
            Err(_) => "Managed by Moose".to_string(),
        }
    } else {
        String::new()
    };
    ui.provider_row.set_subtitle(&subtitle);
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
