use std::{cell::RefCell, fs, rc::Rc, sync::mpsc, time::Duration};

use adw::prelude::*;
use gtk::{Align, Orientation};

use crate::{
    APPLICATION_ID, APPLICATION_NAME,
    chat::{ChatMessage, ChatRequest, ChatStreamEvent},
    error::Result,
    ollama::OllamaClient,
    platform::AppPaths,
    providers::{DEFAULT_OLLAMA_BASE_URL, NewProvider, Provider, ProviderKind, ProviderUpdate},
    storage::{ProviderRepository, open_database},
};

const CHAT_CSS: &str = include_str!("../../data/io.github.moooossee.Moose.css");

pub fn build(app: &adw::Application) -> adw::ApplicationWindow {
    install_chat_css();

    let window = adw::ApplicationWindow::builder()
        .application(app)
        .title(APPLICATION_NAME)
        .default_width(1120)
        .default_height(720)
        .build();

    let header_bar = adw::HeaderBar::new();
    let new_chat_button = icon_button("list-add-symbolic", "New Conversation");
    let search_button = icon_button("system-search-symbolic", "Search Conversations");
    let preferences_button = icon_button("preferences-system-symbolic", "Preferences");

    header_bar.pack_start(&new_chat_button);
    header_bar.pack_start(&search_button);
    header_bar.pack_end(&preferences_button);

    let toolbar_view = adw::ToolbarView::new();
    let toast_overlay = adw::ToastOverlay::new();
    let split_view = adw::NavigationSplitView::new();
    let sidebar = sidebar();
    let chat = chat();
    let sidebar_page = adw::NavigationPage::new(&sidebar.root, "Conversations");
    let content_page = adw::NavigationPage::new(&chat.root, "Chat");

    split_view.set_min_sidebar_width(260.0);
    split_view.set_max_sidebar_width(360.0);
    split_view.set_sidebar_width_fraction(0.28);
    split_view.set_sidebar(Some(&sidebar_page));
    split_view.set_content(Some(&content_page));
    toast_overlay.set_child(Some(&split_view));
    toolbar_view.add_top_bar(&header_bar);
    toolbar_view.set_content(Some(&toast_overlay));
    window.set_content(Some(&toolbar_view));
    window.set_size_request(768, 520);

    let model_names = Rc::new(RefCell::new(Vec::new()));
    let ui = Rc::new(WindowUi {
        toast_overlay,
        provider_row: sidebar.provider_row,
        provider_status: sidebar.provider_status,
        refresh_button: sidebar.refresh_button,
        model_picker: sidebar.model_picker,
        messages: chat.messages,
        message_stack: chat.message_stack,
        entry: chat.entry,
        send_button: chat.send_button,
        stop_button: chat.stop_button,
        model_names,
    });

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
                &preferences_button,
            );
            refresh_models(&ui, &backend);
        }
        Err(error) => {
            ui.provider_status.set_text("Storage Error");
            ui.send_button.set_sensitive(false);
            ui.stop_button.set_sensitive(false);
            ui.refresh_button.set_sensitive(false);
            ui.toast_overlay
                .add_toast(adw::Toast::new(&format!("Storage setup failed: {error}")));
        }
    }

    window
}

struct Backend {
    repository: ProviderRepository,
    provider: RefCell<Provider>,
    runtime: tokio::runtime::Runtime,
    active_generation: RefCell<Option<tokio::task::JoinHandle<()>>>,
}

struct WindowUi {
    toast_overlay: adw::ToastOverlay,
    provider_row: adw::ActionRow,
    provider_status: gtk::Label,
    refresh_button: gtk::Button,
    model_picker: gtk::DropDown,
    messages: gtk::Box,
    message_stack: gtk::Stack,
    entry: gtk::Entry,
    send_button: gtk::Button,
    stop_button: gtk::Button,
    model_names: Rc<RefCell<Vec<String>>>,
}

struct Sidebar {
    root: gtk::Box,
    provider_row: adw::ActionRow,
    provider_status: gtk::Label,
    refresh_button: gtk::Button,
    model_picker: gtk::DropDown,
}

struct Chat {
    root: gtk::Box,
    messages: gtk::Box,
    message_stack: gtk::Stack,
    entry: gtk::Entry,
    send_button: gtk::Button,
    stop_button: gtk::Button,
}

enum ModelLoadEvent {
    Loaded {
        available: bool,
        status: String,
        models: Vec<String>,
    },
    Failed(String),
}

enum ChatUiEvent {
    Token(String),
    Done,
    Failed(String),
}

impl Backend {
    fn new() -> Result<Self> {
        let paths = AppPaths::new("moose")?;
        paths.create_all()?;
        let connection = Rc::new(open_database(paths.database_path())?);
        let repository = ProviderRepository::new(connection);
        let provider = repository.ensure_default_provider()?;
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()?;

        Ok(Self {
            repository,
            provider: RefCell::new(provider),
            runtime,
            active_generation: RefCell::new(None),
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
}

fn bind_actions(
    window: &adw::ApplicationWindow,
    ui: &Rc<WindowUi>,
    backend: &Rc<Backend>,
    new_chat_button: &gtk::Button,
    search_button: &gtk::Button,
    preferences_button: &gtk::Button,
) {
    let target_ui = Rc::clone(ui);
    let target_backend = Rc::clone(backend);
    new_chat_button.connect_clicked(move |_| {
        target_backend.abort_generation();
        clear_messages(&target_ui);
    });

    let target_ui = Rc::clone(ui);
    search_button.connect_clicked(move |_| {
        target_ui
            .toast_overlay
            .add_toast(adw::Toast::new("Conversation search is not ready yet"));
    });

    let target_ui = Rc::clone(ui);
    let target_backend = Rc::clone(backend);
    ui.refresh_button.connect_clicked(move |_| {
        refresh_models(&target_ui, &target_backend);
    });

    let target_ui = Rc::clone(ui);
    let target_backend = Rc::clone(backend);
    ui.send_button.connect_clicked(move |_| {
        send_message(&target_ui, &target_backend);
    });

    let target_ui = Rc::clone(ui);
    let target_backend = Rc::clone(backend);
    ui.entry.connect_activate(move |_| {
        send_message(&target_ui, &target_backend);
    });

    let target_ui = Rc::clone(ui);
    let target_backend = Rc::clone(backend);
    ui.stop_button.connect_clicked(move |_| {
        target_backend.abort_generation();
        finish_generation(&target_ui);
        target_ui
            .toast_overlay
            .add_toast(adw::Toast::new("Generation cancelled"));
    });

    let target_ui = Rc::clone(ui);
    ui.model_picker.connect_selected_notify(move |dropdown| {
        target_ui
            .send_button
            .set_sensitive(selected_model(dropdown, &target_ui.model_names).is_some());
    });

    let parent = window.clone();
    let target_ui = Rc::clone(ui);
    let target_backend = Rc::clone(backend);
    preferences_button.connect_clicked(move |_| {
        preferences_dialog(&parent, &target_ui, &target_backend).present(Some(&parent));
    });
}

fn sidebar() -> Sidebar {
    let root = gtk::Box::builder()
        .orientation(Orientation::Vertical)
        .spacing(12)
        .margin_top(12)
        .margin_bottom(12)
        .margin_start(12)
        .margin_end(12)
        .build();

    let provider_group = gtk::ListBox::new();
    provider_group.add_css_class("boxed-list");
    provider_group.set_selection_mode(gtk::SelectionMode::None);

    let provider_status = status_label("Checking");
    let refresh_button = icon_button("view-refresh-symbolic", "Refresh Models");
    let provider_row = adw::ActionRow::builder()
        .title("Local Ollama")
        .subtitle(DEFAULT_OLLAMA_BASE_URL)
        .build();
    provider_row.add_suffix(&provider_status);
    provider_row.add_suffix(&refresh_button);
    provider_group.append(&provider_row);

    let model_picker = gtk::DropDown::from_strings(&["No model selected"]);
    model_picker.set_tooltip_text(Some("Active Model"));
    model_picker.set_sensitive(false);

    let model_box = gtk::Box::builder()
        .orientation(Orientation::Vertical)
        .spacing(6)
        .build();
    model_box.append(&section_label("Model"));
    model_box.append(&model_picker);

    let conversation_group = gtk::ListBox::new();
    conversation_group.add_css_class("boxed-list");
    conversation_group.set_selection_mode(gtk::SelectionMode::Single);
    conversation_group.append(
        &adw::ActionRow::builder()
            .title("New Conversation")
            .subtitle("Unsaved")
            .build(),
    );

    root.append(&section_label("Provider"));
    root.append(&provider_group);
    root.append(&model_box);
    root.append(&section_label("Conversations"));
    root.append(&conversation_group);

    Sidebar {
        root,
        provider_row,
        provider_status,
        refresh_button,
        model_picker,
    }
}

fn chat() -> Chat {
    let root = gtk::Box::builder()
        .orientation(Orientation::Vertical)
        .spacing(0)
        .build();

    let status_page = adw::StatusPage::builder()
        .icon_name(APPLICATION_ID)
        .title("No Conversation Selected")
        .description("Choose a model and start a conversation.")
        .hexpand(true)
        .vexpand(true)
        .build();

    let empty_clamp = adw::Clamp::builder()
        .maximum_size(860)
        .tightening_threshold(560)
        .hexpand(true)
        .vexpand(true)
        .child(&status_page)
        .build();

    let messages = gtk::Box::builder()
        .orientation(Orientation::Vertical)
        .spacing(8)
        .hexpand(true)
        .build();
    messages.add_css_class("moose-chat-column");

    let messages_clamp = adw::Clamp::builder()
        .maximum_size(980)
        .tightening_threshold(560)
        .hexpand(true)
        .vexpand(true)
        .child(&messages)
        .build();

    let scrolled = gtk::ScrolledWindow::builder()
        .hexpand(true)
        .vexpand(true)
        .child(&messages_clamp)
        .build();

    let message_stack = gtk::Stack::builder().hexpand(true).vexpand(true).build();
    message_stack.add_named(&empty_clamp, Some("empty"));
    message_stack.add_named(&scrolled, Some("messages"));
    message_stack.set_visible_child_name("empty");

    let composer = gtk::Box::builder()
        .orientation(Orientation::Horizontal)
        .spacing(6)
        .hexpand(true)
        .build();
    composer.add_css_class("moose-composer");

    let entry = gtk::Entry::builder()
        .placeholder_text("Message")
        .hexpand(true)
        .build();
    entry.add_css_class("flat");
    entry.add_css_class("moose-composer-entry");

    let stop_button = composer_button("media-playback-stop-symbolic", "Cancel Generation");
    let send_button = composer_button("mail-send-symbolic", "Send Message");

    send_button.add_css_class("suggested-action");
    stop_button.add_css_class("destructive-action");
    send_button.set_sensitive(false);
    stop_button.set_sensitive(false);
    composer.append(&entry);
    composer.append(&stop_button);
    composer.append(&send_button);

    let composer_clamp = adw::Clamp::builder()
        .maximum_size(980)
        .tightening_threshold(560)
        .margin_top(10)
        .margin_bottom(16)
        .margin_start(12)
        .margin_end(12)
        .hexpand(true)
        .child(&composer)
        .build();

    root.append(&message_stack);
    root.append(&composer_clamp);

    Chat {
        root,
        messages,
        message_stack,
        entry,
        send_button,
        stop_button,
    }
}

fn preferences_dialog(
    parent: &adw::ApplicationWindow,
    ui: &Rc<WindowUi>,
    backend: &Rc<Backend>,
) -> adw::PreferencesDialog {
    let dialog = adw::PreferencesDialog::builder()
        .title("Preferences")
        .search_enabled(false)
        .build();

    let provider = backend.provider.borrow().clone();
    let provider_page = adw::PreferencesPage::builder()
        .title("Provider")
        .icon_name("network-server-symbolic")
        .build();
    let provider_group = adw::PreferencesGroup::builder()
        .title("Ollama Provider")
        .build();
    let name_row = adw::EntryRow::builder()
        .title("Name")
        .text(&provider.name)
        .build();
    let url_row = adw::EntryRow::builder()
        .title("Base URL")
        .text(&provider.base_url)
        .build();
    let action_box = gtk::Box::builder()
        .orientation(Orientation::Horizontal)
        .spacing(6)
        .halign(Align::End)
        .margin_top(6)
        .margin_bottom(6)
        .build();
    let save_button = icon_button("document-save-symbolic", "Save Provider");
    let add_button = icon_button("list-add-symbolic", "Add Provider");
    let delete_button = icon_button("user-trash-symbolic", "Delete Provider");

    save_button.add_css_class("suggested-action");
    delete_button.add_css_class("destructive-action");
    action_box.append(&add_button);
    action_box.append(&delete_button);
    action_box.append(&save_button);
    provider_group.add(&name_row);
    provider_group.add(&url_row);
    provider_group.add(&action_box);
    provider_page.add(&provider_group);

    let privacy_page = adw::PreferencesPage::builder()
        .title("Privacy")
        .icon_name("changes-prevent-symbolic")
        .build();
    let privacy_group = adw::PreferencesGroup::builder().title("Local Data").build();
    privacy_group.add(
        &adw::ActionRow::builder()
            .title("Telemetry")
            .subtitle("Disabled")
            .build(),
    );
    privacy_group.add(
        &adw::ActionRow::builder()
            .title("Conversation Storage")
            .subtitle("Local application data")
            .build(),
    );
    privacy_page.add(&privacy_group);

    dialog.add(&provider_page);
    dialog.add(&privacy_page);

    let target_ui = Rc::clone(ui);
    let target_backend = Rc::clone(backend);
    let target_parent = parent.clone();
    let target_name_row = name_row.clone();
    let target_url_row = url_row.clone();
    save_button.connect_clicked(move |_| {
        let current = target_backend.provider.borrow().clone();
        match target_backend.repository.update(ProviderUpdate {
            id: current.id,
            name: target_name_row.text().to_string(),
            base_url: target_url_row.text().to_string(),
            is_default: true,
        }) {
            Ok(provider) => {
                *target_backend.provider.borrow_mut() = provider.clone();
                update_provider_summary(&target_ui, &provider);
                refresh_models(&target_ui, &target_backend);
                target_ui
                    .toast_overlay
                    .add_toast(adw::Toast::new("Provider saved"));
            }
            Err(error) => show_error(&target_parent, "Provider could not be saved", &error),
        }
    });

    let target_ui = Rc::clone(ui);
    let target_backend = Rc::clone(backend);
    let target_parent = parent.clone();
    let target_name_row = name_row.clone();
    let target_url_row = url_row.clone();
    add_button.connect_clicked(move |_| {
        let count = target_backend
            .repository
            .list()
            .map(|items| items.len() + 1);
        let name = count
            .map(|count| format!("Ollama Provider {count}"))
            .unwrap_or_else(|_| "Ollama Provider".to_string());
        match target_backend.repository.create(NewProvider {
            kind: ProviderKind::Ollama,
            name,
            base_url: DEFAULT_OLLAMA_BASE_URL.to_string(),
            is_managed: false,
            is_default: true,
        }) {
            Ok(provider) => {
                *target_backend.provider.borrow_mut() = provider.clone();
                target_name_row.set_text(&provider.name);
                target_url_row.set_text(&provider.base_url);
                update_provider_summary(&target_ui, &provider);
                refresh_models(&target_ui, &target_backend);
                target_ui
                    .toast_overlay
                    .add_toast(adw::Toast::new("Provider added"));
            }
            Err(error) => show_error(&target_parent, "Provider could not be added", &error),
        }
    });

    let target_ui = Rc::clone(ui);
    let target_backend = Rc::clone(backend);
    let target_parent = parent.clone();
    let target_name_row = name_row;
    let target_url_row = url_row;
    delete_button.connect_clicked(move |_| {
        let current = target_backend.provider.borrow().clone();
        match target_backend
            .repository
            .delete(&current.id)
            .and_then(|_| target_backend.repository.ensure_default_provider())
        {
            Ok(provider) => {
                *target_backend.provider.borrow_mut() = provider.clone();
                target_name_row.set_text(&provider.name);
                target_url_row.set_text(&provider.base_url);
                update_provider_summary(&target_ui, &provider);
                refresh_models(&target_ui, &target_backend);
                target_ui
                    .toast_overlay
                    .add_toast(adw::Toast::new("Provider removed"));
            }
            Err(error) => show_error(&target_parent, "Provider could not be removed", &error),
        }
    });

    dialog
}

fn refresh_models(ui: &Rc<WindowUi>, backend: &Rc<Backend>) {
    let client = match backend.client() {
        Ok(client) => client,
        Err(error) => {
            ui.provider_status.set_text("Invalid URL");
            ui.toast_overlay.add_toast(adw::Toast::new(&format!(
                "Provider URL is invalid: {error}"
            )));
            set_model_picker(ui, Vec::new());
            return;
        }
    };

    ui.provider_status.set_text("Checking");
    ui.refresh_button.set_sensitive(false);
    set_model_picker(ui, Vec::new());

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
                let names = models
                    .into_iter()
                    .filter(|model| model.supports_chat)
                    .map(|model| model.name)
                    .collect();
                let _ = sender.send(ModelLoadEvent::Loaded {
                    available: true,
                    status: health.message,
                    models: names,
                });
            }
            Err(error) => {
                let _ = sender.send(ModelLoadEvent::Failed(error.to_string()));
            }
        }
    });

    let target_ui = Rc::clone(ui);
    gtk::glib::timeout_add_local(Duration::from_millis(50), move || {
        match receiver.try_recv() {
            Ok(ModelLoadEvent::Loaded {
                available,
                status,
                models,
            }) => {
                target_ui.refresh_button.set_sensitive(true);
                target_ui.provider_status.set_text(if available {
                    "Connected"
                } else {
                    "Disconnected"
                });
                if !available {
                    target_ui
                        .toast_overlay
                        .add_toast(adw::Toast::new(&format!("Ollama unavailable: {status}")));
                }
                set_model_picker(&target_ui, models);
                gtk::glib::ControlFlow::Break
            }
            Ok(ModelLoadEvent::Failed(error)) => {
                target_ui.refresh_button.set_sensitive(true);
                target_ui.provider_status.set_text("Error");
                target_ui
                    .toast_overlay
                    .add_toast(adw::Toast::new(&format!("Model list failed: {error}")));
                set_model_picker(&target_ui, Vec::new());
                gtk::glib::ControlFlow::Break
            }
            Err(mpsc::TryRecvError::Empty) => gtk::glib::ControlFlow::Continue,
            Err(mpsc::TryRecvError::Disconnected) => {
                target_ui.refresh_button.set_sensitive(true);
                target_ui.provider_status.set_text("Disconnected");
                set_model_picker(&target_ui, Vec::new());
                gtk::glib::ControlFlow::Break
            }
        }
    });
}

fn send_message(ui: &Rc<WindowUi>, backend: &Rc<Backend>) {
    let prompt = ui.entry.text().trim().to_string();
    if prompt.is_empty() {
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

    backend.abort_generation();
    ui.entry.set_text("");
    ui.message_stack.set_visible_child_name("messages");
    append_message(&ui.messages, "You", &prompt);
    let assistant_label = append_message(&ui.messages, &model, "");
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
                    let current = assistant_label.text().to_string();
                    assistant_label.set_text(&(current + &token));
                }
                Ok(ChatUiEvent::Done) => {
                    finish_generation(&target_ui);
                    target_backend.active_generation.borrow_mut().take();
                    return gtk::glib::ControlFlow::Break;
                }
                Ok(ChatUiEvent::Failed(error)) => {
                    finish_generation(&target_ui);
                    target_backend.active_generation.borrow_mut().take();
                    target_ui
                        .toast_overlay
                        .add_toast(adw::Toast::new(&format!("Generation failed: {error}")));
                    return gtk::glib::ControlFlow::Break;
                }
                Err(mpsc::TryRecvError::Empty) => return gtk::glib::ControlFlow::Continue,
                Err(mpsc::TryRecvError::Disconnected) => {
                    finish_generation(&target_ui);
                    target_backend.active_generation.borrow_mut().take();
                    return gtk::glib::ControlFlow::Break;
                }
            }
        }
    });
}

fn finish_generation(ui: &WindowUi) {
    ui.stop_button.set_sensitive(false);
    ui.send_button
        .set_sensitive(selected_model(&ui.model_picker, &ui.model_names).is_some());
}

fn selected_model(
    dropdown: &gtk::DropDown,
    model_names: &Rc<RefCell<Vec<String>>>,
) -> Option<String> {
    let selected = usize::try_from(dropdown.selected()).ok()?;
    model_names.borrow().get(selected).cloned()
}

fn set_model_picker(ui: &WindowUi, models: Vec<String>) {
    let is_empty = models.is_empty();
    *ui.model_names.borrow_mut() = models;

    if is_empty {
        let list = gtk::StringList::new(&["No model selected"]);
        ui.model_picker.set_model(Some(&list));
        ui.model_picker.set_selected(0);
        ui.model_picker.set_sensitive(false);
        ui.send_button.set_sensitive(false);
        return;
    }

    let borrowed = ui.model_names.borrow();
    let labels = borrowed.iter().map(String::as_str).collect::<Vec<_>>();
    let list = gtk::StringList::new(&labels);
    ui.model_picker.set_model(Some(&list));
    ui.model_picker.set_selected(0);
    ui.model_picker.set_sensitive(true);
    ui.send_button.set_sensitive(true);
}

fn append_message(messages: &gtk::Box, role: &str, content: &str) -> gtk::Label {
    let is_user = role == "You";
    let text_alignment = if is_user { 1.0 } else { 0.0 };
    let justification = if is_user {
        gtk::Justification::Right
    } else {
        gtk::Justification::Left
    };
    let row = gtk::Box::builder()
        .orientation(Orientation::Vertical)
        .spacing(6)
        .halign(Align::Fill)
        .hexpand(true)
        .build();
    let role_label = gtk::Label::builder()
        .label(role)
        .halign(Align::Fill)
        .xalign(text_alignment)
        .justify(justification)
        .build();
    let content_label = gtk::Label::builder()
        .label(content)
        .halign(Align::Fill)
        .hexpand(true)
        .xalign(text_alignment)
        .justify(justification)
        .wrap(true)
        .wrap_mode(gtk::pango::WrapMode::Char)
        .natural_wrap_mode(gtk::NaturalWrapMode::None)
        .width_chars(120)
        .selectable(true)
        .build();

    row.add_css_class("moose-message");
    if is_user {
        role_label.add_css_class("moose-message-user");
    }
    role_label.add_css_class("caption-heading");
    role_label.add_css_class("dim-label");
    content_label.add_css_class("body");
    content_label.add_css_class("moose-message-content");
    row.append(&role_label);
    row.append(&content_label);
    messages.append(&row);
    content_label
}

fn clear_messages(ui: &WindowUi) {
    while let Some(child) = ui.messages.first_child() {
        ui.messages.remove(&child);
    }
    ui.entry.set_text("");
    ui.stop_button.set_sensitive(false);
    ui.send_button
        .set_sensitive(selected_model(&ui.model_picker, &ui.model_names).is_some());
    ui.message_stack.set_visible_child_name("empty");
}

fn update_provider_summary(ui: &WindowUi, provider: &Provider) {
    ui.provider_row.set_title(&provider.name);
    ui.provider_row.set_subtitle(&provider.base_url);
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

fn icon_button(icon_name: &str, tooltip: &str) -> gtk::Button {
    let button = gtk::Button::from_icon_name(icon_name);
    button.add_css_class("flat");
    button.set_tooltip_text(Some(tooltip));
    button
}

fn composer_button(icon_name: &str, tooltip: &str) -> gtk::Button {
    let button = gtk::Button::from_icon_name(icon_name);
    button.add_css_class("circular");
    button.add_css_class("moose-composer-button");
    button.set_tooltip_text(Some(tooltip));
    button
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

fn section_label(text: &str) -> gtk::Label {
    let label = gtk::Label::builder()
        .label(text)
        .halign(Align::Start)
        .xalign(0.0)
        .build();
    label.add_css_class("heading");
    label
}

fn status_label(text: &str) -> gtk::Label {
    let label = gtk::Label::builder()
        .label(text)
        .halign(Align::Center)
        .valign(Align::Center)
        .build();
    label.add_css_class("dim-label");
    label
}
