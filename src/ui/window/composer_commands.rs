use crate::i18n::{gettext, gettext_f, gettext_noop};

use super::*;

#[derive(Clone, Copy, Eq, PartialEq)]
enum Tool {
    Web,
    Calculator,
}

impl Tool {
    fn name(self) -> &'static str {
        match self {
            Self::Web => "web",
            Self::Calculator => "calculator",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Web => gettext_noop("Web"),
            Self::Calculator => gettext_noop("Calculator"),
        }
    }

    fn icon(self) -> &'static str {
        match self {
            Self::Web => "moose-globe-symbolic",
            Self::Calculator => "accessories-calculator-symbolic",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value.to_ascii_lowercase().as_str() {
            "web" => Some(Self::Web),
            "calculator" | "calc" => Some(Self::Calculator),
            _ => None,
        }
    }
}

#[derive(Clone, Copy)]
struct Command {
    tool: Tool,
    enabled: bool,
}

struct CommandRow {
    command: Command,
    row: gtk::ListBoxRow,
    title: gtk::Label,
    description: gtk::Label,
    state: gtk::Label,
}

struct Chip {
    tool: Tool,
    button: gtk::Button,
    label: gtk::Label,
}

pub(super) struct Controls {
    pub menu: gtk::Revealer,
    pub active: gtk::Box,
    list: gtk::ListBox,
    rows: Vec<CommandRow>,
    chips: Vec<Chip>,
    dismissed: Cell<bool>,
    composing: Cell<bool>,
}

pub(super) fn build() -> Controls {
    let list = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::Single)
        .activate_on_single_click(true)
        .focusable(false)
        .build();
    list.add_css_class("boxed-list");
    list.add_css_class("moose-command-list");
    let mut rows = Vec::new();
    let active = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    active.add_css_class("moose-active-tools");
    active.set_visible(false);
    let mut chips = Vec::new();
    for tool in [Tool::Web, Tool::Calculator] {
        for enabled in [true, false] {
            let content = gtk::Box::new(gtk::Orientation::Horizontal, 12);
            content.add_css_class("moose-command-row");
            content.append(&gtk::Image::from_icon_name(tool.icon()));
            let labels = gtk::Box::new(gtk::Orientation::Vertical, 2);
            labels.set_hexpand(true);
            let title = gtk::Label::builder().xalign(0.0).build();
            title.add_css_class("monospace");
            let description = gtk::Label::builder()
                .xalign(0.0)
                .wrap(true)
                .wrap_mode(gtk::pango::WrapMode::WordChar)
                .width_chars(1)
                .build();
            description.add_css_class("dim-label");
            description.add_css_class("caption");
            labels.append(&title);
            labels.append(&description);
            content.append(&labels);
            let state = gtk::Label::builder()
                .label(gettext("On"))
                .visible(false)
                .build();
            state.add_css_class("accent");
            state.add_css_class("caption-heading");
            content.append(&state);
            let row = gtk::ListBoxRow::builder()
                .child(&content)
                .focusable(false)
                .focus_on_click(false)
                .build();
            list.append(&row);
            rows.push(CommandRow {
                command: Command { tool, enabled },
                row,
                title,
                description,
                state,
            });
        }
        let content = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        content.append(&gtk::Image::from_icon_name(tool.icon()));
        let label = gtk::Label::new(Some(&gettext(tool.label())));
        content.append(&label);
        let close = gtk::Image::from_icon_name("window-close-symbolic");
        close.set_pixel_size(12);
        content.append(&close);
        let button = gtk::Button::builder()
            .child(&content)
            .focus_on_click(false)
            .visible(false)
            .build();
        button.add_css_class("flat");
        button.add_css_class("moose-tool-chip");
        button.set_tooltip_text(Some(&gettext_f(
            "Turn off {arg1} in this chat",
            &[("arg1", &gettext(tool.label()))],
        )));
        active.append(&button);
        chips.push(Chip {
            tool,
            button,
            label,
        });
    }
    let content = gtk::Box::new(gtk::Orientation::Vertical, 6);
    let hint = gtk::Label::builder()
        .label(gettext(
            "Tools for this chat · ↑↓ to choose · Enter or Tab to select · Esc to dismiss",
        ))
        .xalign(0.0)
        .wrap(true)
        .build();
    hint.add_css_class("caption");
    hint.add_css_class("dim-label");
    content.append(&list);
    content.append(&hint);
    content.set_margin_bottom(8);
    let menu = gtk::Revealer::builder()
        .child(&content)
        .transition_type(gtk::RevealerTransitionType::SlideUp)
        .transition_duration(120)
        .build();
    Controls {
        menu,
        active,
        list,
        rows,
        chips,
        dismissed: Cell::new(false),
        composing: Cell::new(false),
    }
}

pub(super) fn bind(ui: &Rc<WindowUi>, backend: &Rc<Backend>) {
    let weak_ui = Rc::downgrade(ui);
    let target_backend = Rc::clone(backend);
    ui.commands.list.connect_row_activated(move |_, row| {
        if let Some(ui) = weak_ui.upgrade()
            && let Some(item) = ui.commands.rows.iter().find(|item| item.row == *row)
        {
            activate(&ui, &target_backend, item.command);
        }
    });
    let weak_ui = Rc::downgrade(ui);
    ui.commands.list.connect_row_selected(move |_, _| {
        if let Some(ui) = weak_ui.upgrade() {
            update_send_button(&ui);
        }
    });
    for chip in &ui.commands.chips {
        let weak_ui = Rc::downgrade(ui);
        let backend = Rc::clone(backend);
        let tool = chip.tool;
        chip.button.connect_clicked(move |_| {
            if let Some(ui) = weak_ui.upgrade() {
                apply(
                    &ui,
                    &backend,
                    &[Command {
                        tool,
                        enabled: false,
                    }],
                );
                ui.entry.grab_focus();
            }
        });
    }
    let weak_ui = Rc::downgrade(ui);
    ui.entry.buffer().connect_changed(move |_| {
        if let Some(ui) = weak_ui.upgrade() {
            ui.commands.dismissed.set(false);
            update_menu(&ui);
        }
    });
    let weak_ui = Rc::downgrade(ui);
    ui.entry.buffer().connect_cursor_position_notify(move |_| {
        if let Some(ui) = weak_ui.upgrade() {
            update_menu(&ui);
        }
    });
    let weak_ui = Rc::downgrade(ui);
    ui.entry.buffer().connect_has_selection_notify(move |_| {
        if let Some(ui) = weak_ui.upgrade() {
            update_menu(&ui);
        }
    });
    let weak_ui = Rc::downgrade(ui);
    ui.entry.connect_preedit_changed(move |_, text| {
        if let Some(ui) = weak_ui.upgrade() {
            ui.commands.composing.set(!text.is_empty());
            update_menu(&ui);
        }
    });
    let weak_ui = Rc::downgrade(ui);
    let target_backend = Rc::clone(backend);
    ui.entry.connect_has_focus_notify(move |_| {
        if let Some(ui) = weak_ui.upgrade() {
            refresh(&ui, &target_backend);
        }
    });
    let weak_ui = Rc::downgrade(ui);
    let target_backend = Rc::clone(backend);
    ui.stop_button.connect_sensitive_notify(move |_| {
        if let Some(ui) = weak_ui.upgrade() {
            refresh(&ui, &target_backend);
        }
    });
    refresh(ui, backend);
}

fn busy(backend: &Backend) -> Option<&'static str> {
    if backend.active_generation.borrow().is_some() {
        Some(gettext_noop(
            "Finish or stop the current response to change tools",
        ))
    } else if backend.credential_operation.get() {
        Some(gettext_noop("Finish the keyring operation to change tools"))
    } else {
        None
    }
}

fn unavailable(backend: &Backend, command: Command) -> Option<&'static str> {
    if let Some(reason) = busy(backend) {
        return Some(reason);
    }
    if !command.enabled {
        return None;
    }
    if !active_provider(backend).is_some_and(|provider| provider.kind.supports_tool_calling()) {
        return Some(gettext_noop(
            "Requires a compatible Ollama or Ollama Cloud model",
        ));
    }
    if command.tool == Tool::Web && backend.network_policy.local_only() {
        return Some(gettext_noop(
            "Blocked by Local Only in Preferences → Privacy",
        ));
    }
    None
}

pub(super) fn refresh(ui: &WindowUi, backend: &Backend) {
    let settings = backend
        .active_conversation_id
        .borrow()
        .as_deref()
        .map(|id| {
            Ok::<_, MooseError>((
                backend.conversation_repository.calculator_enabled(id)?,
                backend.conversation_repository.web_enabled(id)?,
            ))
        })
        .transpose();
    let loaded = settings.is_ok();
    let (calculator, web) = settings.ok().flatten().unwrap_or_default();
    let config = backend.repository.web_config().ok();
    for item in &ui.commands.rows {
        let enabled = match item.command.tool {
            Tool::Calculator => calculator,
            Tool::Web => web,
        };
        item.state.set_visible(item.command.enabled && enabled);
        let reason = if !loaded {
            Some(gettext(
                "Tool settings could not be loaded. Reopen this chat.",
            ))
        } else {
            unavailable(backend, item.command).map(gettext)
        };
        item.row.set_sensitive(reason.is_none());
        let description = reason.unwrap_or_else(|| {
            if !item.command.enabled {
                return gettext("Turn off this tool for the conversation");
            }
            gettext(match item.command.tool {
                Tool::Calculator => {
                    gettext_noop("Calculate on this device · Requires a model with tool support")
                }
                Tool::Web
                    if config.as_ref().is_none_or(|config| {
                        config.provider == crate::tools::web::SearchProvider::LinksOnly
                    }) =>
                {
                    gettext_noop("Read public links · Set up search in Preferences → Web")
                }
                Tool::Web => gettext_noop(
                    "Search and read public pages · Permission required before first use",
                ),
            })
        });
        item.description.set_text(&description);
    }
    for chip in &ui.commands.chips {
        let enabled = match chip.tool {
            Tool::Calculator => calculator,
            Tool::Web => web,
        };
        chip.button.set_visible(enabled);
        chip.button.set_sensitive(loaded && busy(backend).is_none());
        let blocked = (chip.tool == Tool::Web && backend.network_policy.local_only())
            || !active_provider(backend)
                .is_some_and(|provider| provider.kind.supports_tool_calling());
        chip.label.set_text(&format!(
            "{}{}",
            gettext(chip.tool.label()),
            if blocked {
                gettext(" · Unavailable")
            } else {
                String::new()
            }
        ));
    }
    ui.commands.active.set_visible(calculator || web);
    update_menu(ui);
}

fn matches(command: Command, query: &str) -> bool {
    if let Some(separator) = query.find(char::is_whitespace) {
        let Some(tool) = Tool::parse(&query[..separator]) else {
            return false;
        };
        let argument = query[separator..].trim();
        command.tool == tool && (if command.enabled { "on" } else { "off" }).starts_with(argument)
    } else {
        command.enabled
            && (command.tool.name().starts_with(query)
                || (command.tool == Tool::Calculator && "calc".starts_with(query)))
    }
}

fn update_menu(ui: &WindowUi) {
    let text = prompt_text(&ui.entry);
    let buffer = ui.entry.buffer();
    let query = text.strip_prefix('/').map(str::to_ascii_lowercase);
    let eligible = !ui.restoring_draft.get()
        && !is_composing(ui)
        && ui.entry.has_focus()
        && !buffer.has_selection()
        && buffer.cursor_position() == buffer.char_count()
        && text.len() <= 48
        && !text.contains(['\n', '\r'])
        && !ui.commands.dismissed.get();
    let mut first = None;
    for item in &ui.commands.rows {
        let visible = eligible
            && query
                .as_deref()
                .is_some_and(|query| matches(item.command, query));
        item.row.set_visible(visible);
        if visible && first.is_none() {
            first = Some(&item.row);
        }
        let explicit = query
            .as_deref()
            .is_some_and(|query| query.contains(char::is_whitespace));
        item.title.set_text(&format!(
            "/{}{}",
            item.command.tool.name(),
            if !item.command.enabled {
                " off"
            } else if explicit {
                " on"
            } else {
                ""
            }
        ));
    }
    if ui
        .commands
        .list
        .selected_row()
        .is_none_or(|row| !row.is_visible())
    {
        ui.commands.list.select_row(first);
    }
    ui.commands.menu.set_reveal_child(first.is_some());
    update_send_button(ui);
}

fn dismiss(ui: &WindowUi) {
    ui.commands.dismissed.set(true);
    ui.commands.menu.set_reveal_child(false);
    update_send_button(ui);
}

pub(super) fn can_apply(ui: &WindowUi) -> bool {
    ui.commands.menu.reveals_child()
        && ui
            .commands
            .list
            .selected_row()
            .is_some_and(|row| row.is_sensitive())
}

pub(super) fn reset(ui: &WindowUi) {
    ui.commands.composing.set(false);
    ui.commands.dismissed.set(false);
    ui.commands.menu.set_reveal_child(false);
}

pub(super) fn is_composing(ui: &WindowUi) -> bool {
    ui.entry.has_focus() && ui.commands.composing.get()
}

pub(super) fn handle_key(
    ui: &Rc<WindowUi>,
    backend: &Rc<Backend>,
    key: gtk::gdk::Key,
    modifiers: gtk::gdk::ModifierType,
) -> bool {
    use gtk::gdk::{Key, ModifierType};
    if !ui.entry.has_focus()
        || !ui.commands.menu.reveals_child()
        || modifiers.intersects(
            ModifierType::CONTROL_MASK
                | ModifierType::ALT_MASK
                | ModifierType::SUPER_MASK
                | ModifierType::SHIFT_MASK,
        )
    {
        return false;
    }
    match key {
        Key::Escape => dismiss(ui),
        Key::Up | Key::KP_Up | Key::Down | Key::KP_Down => {
            let visible: Vec<_> = ui
                .commands
                .rows
                .iter()
                .filter(|item| item.row.is_visible())
                .collect();
            if visible.is_empty() {
                return false;
            }
            let selected = ui.commands.list.selected_row();
            let current = visible
                .iter()
                .position(|item| Some(&item.row) == selected.as_ref())
                .unwrap_or(0);
            let next = if matches!(key, Key::Up | Key::KP_Up) {
                (current + visible.len() - 1) % visible.len()
            } else {
                (current + 1) % visible.len()
            };
            ui.commands.list.select_row(Some(&visible[next].row));
        }
        Key::Return | Key::KP_Enter | Key::ISO_Enter | Key::Tab => {
            activate_selected(ui, backend);
        }
        _ => return false,
    }
    true
}

fn activate_selected(ui: &Rc<WindowUi>, backend: &Rc<Backend>) {
    if let Some(row) = ui.commands.list.selected_row()
        && let Some(item) = ui.commands.rows.iter().find(|item| item.row == row)
    {
        activate(ui, backend, item.command);
    }
}

fn activate(ui: &Rc<WindowUi>, backend: &Rc<Backend>, command: Command) {
    if apply(ui, backend, &[command]) {
        dismiss(ui);
        remove_prefix(ui, ui.entry.buffer().char_count());
        ui.entry.grab_focus();
    }
}

fn apply(ui: &Rc<WindowUi>, backend: &Rc<Backend>, commands: &[Command]) -> bool {
    workspace::sync_provider(ui, backend);
    for command in commands {
        if let Some(reason) = unavailable(backend, *command) {
            ui.toast_overlay
                .add_toast(adw::Toast::new(&gettext(reason)));
            refresh(ui, backend);
            return false;
        }
    }
    if !workspace::provider_matches(backend) {
        ui.toast_overlay.add_toast(adw::Toast::new(&gettext(
            "Switch to this chat’s provider before changing tools",
        )));
        return false;
    }
    let was_new = backend.active_conversation_id.borrow().is_none();
    let result = (|| -> Result<()> {
        let current = backend.active_conversation_id.borrow().clone();
        let id = match current {
            Some(id) => id,
            None => create_empty_conversation(backend)?,
        };
        let mut calculator = backend.conversation_repository.calculator_enabled(&id)?;
        let mut web = backend.conversation_repository.web_enabled(&id)?;
        for command in commands {
            match command.tool {
                Tool::Calculator => calculator = command.enabled,
                Tool::Web => web = command.enabled,
            }
        }
        backend
            .conversation_repository
            .save_chat_tools(&id, calculator, web)
    })();
    match result {
        Ok(()) => {
            refresh(ui, backend);
            if was_new {
                conversation_list::refresh(ui, backend);
            }
            let text = commands
                .iter()
                .map(|command| {
                    format!(
                        "{} {}",
                        gettext(command.tool.label()),
                        if command.enabled {
                            "enabled"
                        } else {
                            "disabled"
                        }
                    )
                })
                .collect::<Vec<_>>()
                .join(" · ");
            ui.toast_overlay.add_toast(adw::Toast::new(&text));
            true
        }
        Err(error) => {
            show_error(
                &ui.window,
                &gettext("Tool settings could not be saved"),
                &error,
            );
            false
        }
    }
}

fn remove_prefix(ui: &WindowUi, characters: i32) {
    let buffer = ui.entry.buffer();
    buffer.begin_user_action();
    let mut start = buffer.start_iter();
    let mut end = buffer.iter_at_offset(characters);
    buffer.delete(&mut start, &mut end);
    buffer.end_user_action();
}

pub(super) fn submit(ui: &Rc<WindowUi>, backend: &Rc<Backend>) -> bool {
    if ui.commands.menu.reveals_child() {
        activate_selected(ui, backend);
        return true;
    }
    let text = prompt_text(&ui.entry);
    let mut remaining = text.as_str();
    let mut commands: Vec<Command> = Vec::new();
    loop {
        let candidate = remaining.trim_start();
        let token_end = candidate
            .find(char::is_whitespace)
            .unwrap_or(candidate.len());
        let token = &candidate[..token_end];
        let Some(tool) = token.strip_prefix('/').and_then(Tool::parse) else {
            break;
        };
        remaining = candidate[token_end..].trim_start();
        let argument_end = remaining
            .find(char::is_whitespace)
            .unwrap_or(remaining.len());
        let argument = &remaining[..argument_end];
        let enabled = !argument.eq_ignore_ascii_case("off");
        if argument.eq_ignore_ascii_case("on") || argument.eq_ignore_ascii_case("off") {
            remaining = remaining[argument_end..].trim_start();
        }
        let command = Command { tool, enabled };
        if let Some(previous) = commands.iter_mut().find(|previous| previous.tool == tool) {
            *previous = command;
        } else {
            commands.push(command);
        }
    }
    if commands.is_empty() {
        return false;
    }
    if !apply(ui, backend, &commands) {
        return true;
    }
    let consumed = text[..text.len() - remaining.len()].chars().count() as i32;
    let command_only = remaining.is_empty();
    dismiss(ui);
    remove_prefix(ui, consumed);
    command_only
}
