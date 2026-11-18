use super::*;
use crate::{
    chat::{
        context::{ContextBudget, ContextUsage, request_overhead},
        runner::{context_overhead, conversation_messages},
    },
    ollama::{DEFAULT_CONTEXT_WINDOW, MAX_CONTEXT_WINDOW, effective_context_window},
};

const WARNING_FRACTION: f64 = 0.9;
const CRITICAL_FRACTION: f64 = 0.97;
const ADJUSTMENT_DURATION: Duration = Duration::from_secs(5);
const CONTEXT_WINDOW_HELP: &str =
    gettext_noop("Applies to the next message. Larger values use more memory.");
const LARGE_CONTEXT_WINDOW_THRESHOLD: i64 = 32_768;
const LARGE_CONTEXT_WINDOW_WARNING: &str = gettext_noop(
    "Requires model support and enough RAM or VRAM. May cause high resource use, slowdowns, or memory errors.",
);
const CONTEXT_WINDOW_STEPS: &[i64] = &[
    1024,
    2048,
    4096,
    8192,
    DEFAULT_CONTEXT_WINDOW,
    32_768,
    65_536,
    131_072,
    MAX_CONTEXT_WINDOW,
];

#[derive(Clone, Copy, Eq, PartialEq)]
enum Phase {
    NextMessage,
    Response,
}

impl Phase {
    fn label(self) -> &'static str {
        match self {
            Self::NextMessage => gettext_noop("Next Message"),
            Self::Response => gettext_noop("Current Response"),
        }
    }
}

pub(super) struct Controls {
    pub(super) button: gtk::MenuButton,
    label: gtk::Label,
    adjustment: gtk::Revealer,
    adjustment_timeout: Rc<RefCell<Option<gtk::glib::SourceId>>>,
    ring: gtk::DrawingArea,
    fraction: Rc<Cell<Option<f64>>>,
    progress: gtk::ProgressBar,
    summary: gtk::Label,
    phase: gtk::Label,
    description: gtk::Label,
    notice: gtk::Label,
    context_window: gtk::Scale,
    context_window_group: gtk::Box,
    context_window_value: gtk::Label,
    context_window_warning: gtk::Box,
    context_window_apply: gtk::Button,
    context_window_apply_stack: gtk::Stack,
    context_window_reset: gtk::Button,
    context_window_steps: RefCell<Vec<i64>>,
    pending_window: Cell<Option<i64>>,
    syncing_window: Cell<bool>,
    editor_target: RefCell<Option<EditorTarget>>,
    actions: gtk::Box,
    settings: gtk::Button,
    new_chat: gtk::Button,
    last_usage: Cell<Option<(ContextUsage, Phase)>>,
    scope: RefCell<Option<UsageScope>>,
    history: RefCell<Option<History>>,
    windows: RefCell<HashMap<(String, String, i64), usize>>,
    pending: RefCell<Option<gtk::glib::SourceId>>,
}

struct History {
    conversation_id: Option<String>,
    message_limit: i64,
    messages: Vec<crate::conversations::Message>,
}

#[derive(Eq, PartialEq)]
struct UsageScope {
    conversation_id: Option<String>,
    destination: String,
    model: Option<String>,
}

#[derive(Clone, Eq, PartialEq)]
struct EditorTarget {
    conversation_id: Option<String>,
    destination: String,
    value: Option<i64>,
}

pub(super) fn build() -> Controls {
    let fraction = Rc::new(Cell::new(None::<f64>));
    let ring = gtk::DrawingArea::builder()
        .content_width(14)
        .content_height(14)
        .valign(gtk::Align::Center)
        .can_target(false)
        .build();
    let value = Rc::clone(&fraction);
    ring.set_draw_func(move |area, cr, width, height| {
        let color = area.color();
        let red = f64::from(color.red());
        let green = f64::from(color.green());
        let blue = f64::from(color.blue());
        let alpha = f64::from(color.alpha());
        let center_x = f64::from(width) / 2.0;
        let center_y = f64::from(height) / 2.0;
        let radius = f64::from(width.min(height)) / 2.0 - 1.5;
        let start = -std::f64::consts::FRAC_PI_2;
        cr.set_line_width(2.0);
        cr.set_source_rgba(red, green, blue, alpha * 0.18);
        cr.arc(center_x, center_y, radius, 0.0, std::f64::consts::TAU);
        let _ = cr.stroke();
        if let Some(fraction) = value.get().filter(|fraction| *fraction > 0.0) {
            cr.set_line_cap(gtk::cairo::LineCap::Round);
            cr.set_source_rgba(red, green, blue, alpha);
            let end = start + fraction.min(1.0) * std::f64::consts::TAU;
            cr.arc(center_x, center_y, radius, start, end);
            let _ = cr.stroke();
        }
    });
    let label = gtk::Label::builder()
        .label("—")
        .width_chars(5)
        .xalign(0.0)
        .build();
    let adjustment_label = gtk::Label::builder()
        .label(gettext("Context adjusted"))
        .css_classes(["caption"])
        .single_line_mode(true)
        .max_width_chars(18)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .build();
    let adjustment = gtk::Revealer::builder()
        .child(&adjustment_label)
        .transition_type(gtk::RevealerTransitionType::Crossfade)
        .transition_duration(150)
        .visible(false)
        .build();
    adjustment.connect_child_revealed_notify(|revealer| {
        if !revealer.reveals_child() && !revealer.is_child_revealed() {
            revealer.set_visible(false);
        }
    });
    let content = gtk::Box::new(gtk::Orientation::Horizontal, 5);
    content.append(&ring);
    content.append(&label);
    content.append(&adjustment);
    let title = gtk::Label::builder()
        .label(gettext("Context Usage"))
        .xalign(0.0)
        .hexpand(true)
        .css_classes(["heading"])
        .build();
    let phase = detail_label();
    phase.set_xalign(1.0);
    let heading = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    heading.append(&title);
    heading.append(&phase);
    let summary = gtk::Label::builder().xalign(0.0).build();
    let progress = gtk::ProgressBar::new();
    let window_title = gtk::Label::builder()
        .label(gettext("Context Window"))
        .css_classes(["caption", "dim-label"])
        .xalign(0.0)
        .hexpand(true)
        .build();
    let context_window_value = gtk::Label::builder()
        .xalign(1.0)
        .width_chars(9)
        .max_width_chars(12)
        .ellipsize(gtk::pango::EllipsizeMode::Middle)
        .css_classes(["caption", "moose-context-window-value"])
        .build();
    let window_heading = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    window_heading.append(&window_title);
    window_heading.append(&context_window_value);
    let context_window = gtk::Scale::with_range(
        gtk::Orientation::Horizontal,
        0.0,
        (CONTEXT_WINDOW_STEPS.len() - 1) as f64,
        1.0,
    );
    context_window.set_draw_value(false);
    context_window.set_round_digits(0);
    context_window.set_increments(1.0, 1.0);
    context_window.set_hexpand(true);
    context_window.add_css_class("moose-context-window-scale");
    let context_window_apply =
        widgets::icon_button("object-select-symbolic", &gettext("Apply Context Window"));
    context_window_apply.add_css_class("moose-context-window-action");
    let context_window_apply_stack = gtk::Stack::builder()
        .transition_type(gtk::StackTransitionType::Crossfade)
        .transition_duration(100)
        .build();
    context_window_apply_stack.add_named(&context_window_apply, Some("apply"));
    context_window_apply_stack.add_named(
        &gtk::Box::new(gtk::Orientation::Horizontal, 0),
        Some("empty"),
    );
    context_window_apply_stack.set_visible_child_name("empty");
    let default_tokens = format_tokens(DEFAULT_CONTEXT_WINDOW as usize);
    let context_window_reset = widgets::icon_button(
        "edit-undo-symbolic",
        &gettext_f(
            "Use Default ({tokens} tokens)",
            &[("tokens", &default_tokens)],
        ),
    );
    context_window_reset.add_css_class("moose-context-window-action");
    let window_controls = gtk::Box::new(gtk::Orientation::Horizontal, 4);
    window_controls.append(&context_window);
    window_controls.append(&context_window_reset);
    window_controls.append(&context_window_apply_stack);
    let context_window_group = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(2)
        .margin_top(4)
        .tooltip_text(gettext(CONTEXT_WINDOW_HELP))
        .visible(false)
        .build();
    context_window_group.append(&window_heading);
    context_window_group.append(&window_controls);
    let context_window_warning = warning_content();
    context_window_warning.set_visible(false);
    context_window_group.append(&context_window_warning);
    let description = detail_label();
    let notice = detail_label();
    notice.set_visible(false);
    let settings = gtk::Button::with_label(&gettext("Chat Settings"));
    settings.add_css_class("flat");
    let new_chat = gtk::Button::with_label(&gettext("New Chat"));
    new_chat.add_css_class("flat");
    let actions = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(6)
        .halign(gtk::Align::End)
        .visible(false)
        .build();
    actions.append(&settings);
    actions.append(&new_chat);
    let details = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(10)
        .margin_top(12)
        .margin_bottom(12)
        .margin_start(12)
        .margin_end(12)
        .build();
    details.append(&heading);
    details.append(&summary);
    details.append(&progress);
    details.append(&description);
    details.append(&context_window_group);
    details.append(&notice);
    details.append(&actions);
    let popover = gtk::Popover::builder()
        .child(&details)
        .position(gtk::PositionType::Top)
        .build();
    let button = gtk::MenuButton::builder()
        .child(&content)
        .popover(&popover)
        .always_show_arrow(false)
        .valign(gtk::Align::Center)
        .visible(false)
        .build();
    button.add_css_class("flat");
    button.add_css_class("moose-context-usage");
    Controls {
        button,
        label,
        adjustment,
        adjustment_timeout: Rc::new(RefCell::new(None)),
        ring,
        fraction,
        progress,
        summary,
        phase,
        description,
        notice,
        context_window,
        context_window_group,
        context_window_value,
        context_window_warning,
        context_window_apply,
        context_window_apply_stack,
        context_window_reset,
        context_window_steps: RefCell::new(CONTEXT_WINDOW_STEPS.to_vec()),
        pending_window: Cell::new(None),
        syncing_window: Cell::new(false),
        editor_target: RefCell::new(None),
        actions,
        settings,
        new_chat,
        last_usage: Cell::new(None),
        scope: RefCell::new(None),
        history: RefCell::new(None),
        windows: RefCell::new(HashMap::new()),
        pending: RefCell::new(None),
    }
}

fn detail_label() -> gtk::Label {
    gtk::Label::builder()
        .xalign(0.0)
        .wrap(true)
        .wrap_mode(gtk::pango::WrapMode::WordChar)
        .max_width_chars(34)
        .css_classes(["caption", "dim-label"])
        .build()
}

fn warning_content() -> gtk::Box {
    let icon = gtk::Image::from_icon_name("dialog-warning-symbolic");
    icon.set_valign(gtk::Align::Start);
    icon.add_css_class("dim-label");
    let label = detail_label();
    label.set_label(&gettext(LARGE_CONTEXT_WINDOW_WARNING));
    label.set_max_width_chars(30);
    label.set_hexpand(true);
    let warning = gtk::Box::builder().spacing(6).margin_top(4).build();
    warning.append(&icon);
    warning.append(&label);
    warning
}

pub(super) fn window_warning_button(value: Option<i64>) -> gtk::MenuButton {
    let content = warning_content();
    content.set_margin_top(12);
    content.set_margin_bottom(12);
    content.set_margin_start(12);
    content.set_margin_end(12);
    let popover = gtk::Popover::builder().child(&content).build();
    let icon = gtk::Image::from_icon_name("dialog-warning-symbolic");
    icon.add_css_class("dim-label");
    let button = gtk::MenuButton::builder()
        .child(&icon)
        .popover(&popover)
        .always_show_arrow(false)
        .valign(gtk::Align::Center)
        .tooltip_text(gettext(LARGE_CONTEXT_WINDOW_WARNING))
        .build();
    button.add_css_class("flat");
    button.add_css_class("circular");
    button.update_property(&[
        gtk::accessible::Property::Label(&gettext("Context Window Warning")),
        gtk::accessible::Property::Description(&gettext(LARGE_CONTEXT_WINDOW_WARNING)),
    ]);
    update_window_warning(&button, value);
    button
}

pub(super) fn update_window_warning(warning: &impl IsA<gtk::Widget>, value: Option<i64>) {
    warning.set_visible(effective_context_window(value) > LARGE_CONTEXT_WINDOW_THRESHOLD);
}

pub(super) fn bind(ui: &Rc<WindowUi>, backend: &Rc<Backend>) {
    let weak_ui = Rc::downgrade(ui);
    let weak_backend = Rc::downgrade(backend);
    ui.context_usage
        .context_window_apply
        .connect_clicked(move |_| {
            if let (Some(ui), Some(backend)) = (weak_ui.upgrade(), weak_backend.upgrade()) {
                apply_context_window(&ui, &backend);
            }
        });
    let weak_ui = Rc::downgrade(ui);
    ui.context_usage
        .context_window
        .connect_value_changed(move |scale| {
            let Some(ui) = weak_ui.upgrade() else {
                return;
            };
            let controls = &ui.context_usage;
            if controls.syncing_window.get() {
                return;
            }
            let value = controls
                .context_window_steps
                .borrow()
                .get(scale.value().round() as usize)
                .copied();
            if let Some(value) = value {
                let default = controls
                    .editor_target
                    .borrow()
                    .as_ref()
                    .is_some_and(|target| target.value.is_none());
                controls
                    .pending_window
                    .set((!(default && value == DEFAULT_CONTEXT_WINDOW)).then_some(value));
                update_window_value(controls);
            }
        });
    let weak_ui = Rc::downgrade(ui);
    ui.context_usage
        .context_window_reset
        .connect_clicked(move |_| {
            if let Some(ui) = weak_ui.upgrade() {
                set_pending_window(&ui.context_usage, None);
            }
        });
    if let Some(popover) = ui.context_usage.button.popover() {
        let weak_ui = Rc::downgrade(ui);
        let weak_backend = Rc::downgrade(backend);
        popover.connect_show(move |_| {
            if let (Some(ui), Some(backend)) = (weak_ui.upgrade(), weak_backend.upgrade()) {
                sync_editor(&ui, &backend);
            }
        });
        let weak_ui = Rc::downgrade(ui);
        popover.connect_closed(move |_| {
            if let Some(ui) = weak_ui.upgrade() {
                ui.context_usage.editor_target.borrow_mut().take();
                ui.context_usage.pending_window.set(None);
                ui.context_usage
                    .context_window_apply_stack
                    .set_visible_child_name("empty");
            }
        });
    }
    let weak_ui = Rc::downgrade(ui);
    let weak_backend = Rc::downgrade(backend);
    ui.context_usage.settings.connect_clicked(move |_| {
        if let (Some(ui), Some(backend)) = (weak_ui.upgrade(), weak_backend.upgrade()) {
            ui.context_usage.button.popdown();
            show_chat_settings(&ui, &backend);
        }
    });
    let weak_ui = Rc::downgrade(ui);
    let weak_backend = Rc::downgrade(backend);
    ui.context_usage.new_chat.connect_clicked(move |_| {
        if let (Some(ui), Some(backend)) = (weak_ui.upgrade(), weak_backend.upgrade()) {
            ui.context_usage.button.popdown();
            new_conversation_action(&ui, &backend);
        }
    });
    let weak_ui = Rc::downgrade(ui);
    let weak_backend = Rc::downgrade(backend);
    ui.entry.buffer().connect_changed(move |_| {
        if let (Some(ui), Some(backend)) = (weak_ui.upgrade(), weak_backend.upgrade()) {
            schedule(&ui, &backend);
        }
    });
    let weak_ui = Rc::downgrade(ui);
    let weak_backend = Rc::downgrade(backend);
    ui.model_picker.connect_selected_notify(move |_| {
        if let (Some(ui), Some(backend)) = (weak_ui.upgrade(), weak_backend.upgrade()) {
            reset_usage(&ui.context_usage);
            schedule(&ui, &backend);
        }
    });
}

fn schedule(ui: &Rc<WindowUi>, backend: &Rc<Backend>) {
    if ui.restoring_draft.get() {
        return;
    }
    if let Some(source) = ui.context_usage.pending.borrow_mut().take() {
        source.remove();
    }
    let weak_ui = Rc::downgrade(ui);
    let weak_backend = Rc::downgrade(backend);
    let source = gtk::glib::timeout_add_local_once(Duration::from_millis(300), move || {
        if let (Some(ui), Some(backend)) = (weak_ui.upgrade(), weak_backend.upgrade()) {
            ui.context_usage.pending.borrow_mut().take();
            update(&ui, &backend);
        }
    });
    *ui.context_usage.pending.borrow_mut() = Some(source);
}

pub(super) fn refresh(ui: &WindowUi, backend: &Backend) {
    ui.context_usage.history.borrow_mut().take();
    update(ui, backend);
}

fn show_adjustment(controls: &Controls) {
    if let Some(source) = controls.adjustment_timeout.borrow_mut().take() {
        source.remove();
    }
    controls.adjustment.set_visible(true);
    controls.adjustment.set_reveal_child(true);
    let weak_adjustment = controls.adjustment.downgrade();
    let weak_timeout = Rc::downgrade(&controls.adjustment_timeout);
    let source = gtk::glib::timeout_add_local_once(ADJUSTMENT_DURATION, move || {
        if let Some(timeout) = weak_timeout.upgrade() {
            timeout.borrow_mut().take();
        }
        if let Some(adjustment) = weak_adjustment.upgrade() {
            adjustment.set_reveal_child(false);
        }
    });
    *controls.adjustment_timeout.borrow_mut() = Some(source);
}

fn clear_adjustment(controls: &Controls) {
    if let Some(source) = controls.adjustment_timeout.borrow_mut().take() {
        source.remove();
    }
    controls.adjustment.set_reveal_child(false);
    controls.adjustment.set_visible(false);
}

fn reset_usage(controls: &Controls) {
    clear_adjustment(controls);
    controls.last_usage.set(None);
}

fn sync_scope(ui: &WindowUi, backend: &Backend, provider: &Provider) {
    let scope = UsageScope {
        conversation_id: backend.active_conversation_id.borrow().clone(),
        destination: crate::providers::policy::destination(provider),
        model: selected_model(&ui.model_picker, &ui.model_names),
    };
    if ui.context_usage.scope.borrow().as_ref() != Some(&scope) {
        reset_usage(&ui.context_usage);
        *ui.context_usage.scope.borrow_mut() = Some(scope);
    }
}

pub(super) fn refresh_live(ui: &WindowUi, backend: &Backend) {
    if let Some(usage) = generation::context_usage(backend) {
        ui.context_usage.settings.set_sensitive(false);
        ui.context_usage.context_window_group.set_sensitive(false);
        show(&ui.context_usage, usage, Phase::Response);
    }
}

pub(super) fn known_window(
    ui: &WindowUi,
    provider: &Provider,
    model: &str,
    configured: i64,
) -> Option<usize> {
    ui.context_usage
        .windows
        .borrow()
        .get(&(
            crate::providers::policy::destination(provider),
            model.to_string(),
            configured,
        ))
        .copied()
}

pub(super) fn remember_window(
    ui: &WindowUi,
    destination: &str,
    model: &str,
    configured: i64,
    window: usize,
) {
    if window > 0 {
        ui.context_usage.windows.borrow_mut().insert(
            (destination.to_string(), model.to_string(), configured),
            window,
        );
    }
}

pub(super) fn estimate_request(
    request: &ChatRequest,
    window: Option<usize>,
) -> Result<ContextUsage> {
    let mut usage = ContextUsage::estimate(request)?;
    let extra = context_overhead(request)?.saturating_sub(request_overhead(request)?);
    usage.tokens = usage.tokens.saturating_add(extra);
    usage.window = window;
    Ok(usage)
}

fn update(ui: &WindowUi, backend: &Backend) {
    if let Some(source) = ui.context_usage.pending.borrow_mut().take() {
        source.remove();
    }
    let controls = &ui.context_usage;
    controls
        .settings
        .set_sensitive(backend.active_generation.borrow().is_none());
    controls
        .context_window_group
        .set_sensitive(backend.active_generation.borrow().is_none());
    if controls
        .button
        .popover()
        .is_some_and(|popover| popover.is_visible())
    {
        sync_editor(ui, backend);
    }
    let Some(provider) = active_provider(backend) else {
        hide(controls);
        return;
    };
    sync_scope(ui, backend, &provider);
    if let Some(usage) = generation::context_usage(backend) {
        show(controls, usage, Phase::Response);
        return;
    }
    if !workspace::provider_matches(backend) {
        hide(controls);
        return;
    }
    let result = preview(ui, backend, &provider);
    match result {
        Ok(Some(usage)) => show(controls, usage, Phase::NextMessage),
        Ok(None) => hide(controls),
        Err(error) => show_error(controls, &error),
    }
}

fn set_pending_window(controls: &Controls, value: Option<i64>) {
    let effective = effective_context_window(value);
    let index = controls
        .context_window_steps
        .borrow()
        .iter()
        .position(|step| *step == effective);
    if let Some(index) = index {
        controls.syncing_window.set(true);
        controls.pending_window.set(value);
        controls.context_window.set_value(index as f64);
        controls.syncing_window.set(false);
        update_window_value(controls);
    }
}

fn update_window_value(controls: &Controls) {
    let pending = controls.pending_window.get();
    let value = effective_context_window(pending);
    let formatted = format_tokens(value as usize);
    controls.context_window_value.set_label(&formatted);
    update_window_warning(&controls.context_window_warning, pending);
    let tokens = gettext_f("{tokens} tokens", &[("tokens", &formatted)]);
    let value_text = if pending.is_none() {
        format!("{tokens} · {}", gettext("Default"))
    } else {
        tokens
    };
    controls
        .context_window_value
        .set_tooltip_text(Some(&value_text));
    controls.context_window.update_property(&[
        gtk::accessible::Property::Label(&gettext("Context Window")),
        gtk::accessible::Property::ValueText(&value_text),
        gtk::accessible::Property::Description(&gettext(CONTEXT_WINDOW_HELP)),
    ]);
    controls
        .context_window_group
        .set_tooltip_text(Some(&gettext(CONTEXT_WINDOW_HELP)));
    let changed = controls
        .editor_target
        .borrow()
        .as_ref()
        .is_some_and(|target| target.value != pending);
    controls
        .context_window_apply_stack
        .set_visible_child_name(if changed { "apply" } else { "empty" });
    controls
        .context_window_reset
        .set_sensitive(pending.is_some());
    if changed {
        controls.context_window_value.add_css_class("accent");
    } else {
        controls.context_window_value.remove_css_class("accent");
    }
}

fn sync_editor(ui: &WindowUi, backend: &Backend) {
    let controls = &ui.context_usage;
    let Some(provider) =
        active_provider(backend).filter(|provider| provider.kind == ProviderKind::Ollama)
    else {
        controls.context_window_group.set_visible(false);
        controls.editor_target.borrow_mut().take();
        return;
    };
    controls.context_window_group.set_visible(true);
    controls.context_window_group.set_sensitive(
        backend.active_generation.borrow().is_none() && workspace::provider_matches(backend),
    );
    let conversation_id = backend.active_conversation_id.borrow().clone();
    let settings = match conversation_id.as_deref().map_or(Ok(None), |id| {
        backend
            .conversation_repository
            .latest_generation_settings(id)
    }) {
        Ok(settings) => settings,
        Err(error) => {
            controls.context_window_group.set_sensitive(false);
            controls
                .context_window_group
                .set_tooltip_text(Some(&crate::i18n::error_message(&error)));
            controls.editor_target.borrow_mut().take();
            return;
        }
    };
    let target = EditorTarget {
        conversation_id,
        destination: crate::providers::policy::destination(&provider),
        value: settings
            .and_then(|settings| settings.num_ctx)
            .filter(|value| *value > 0)
            .map(|value| value.min(MAX_CONTEXT_WINDOW)),
    };
    if controls.editor_target.borrow().as_ref() != Some(&target) {
        let mut steps = CONTEXT_WINDOW_STEPS.to_vec();
        steps.push(effective_context_window(target.value));
        steps.sort_unstable();
        steps.dedup();
        controls.syncing_window.set(true);
        controls
            .context_window
            .set_range(0.0, (steps.len() - 1) as f64);
        *controls.context_window_steps.borrow_mut() = steps;
        controls.syncing_window.set(false);
        let value = target.value;
        *controls.editor_target.borrow_mut() = Some(target);
        set_pending_window(controls, value);
    }
}

fn apply_context_window(ui: &Rc<WindowUi>, backend: &Rc<Backend>) {
    if provider_change_is_blocked(ui, backend) {
        sync_editor(ui, backend);
        return;
    }
    let target = ui.context_usage.editor_target.borrow().clone();
    let Some(target) = target else {
        sync_editor(ui, backend);
        return;
    };
    let provider_matches = active_provider(backend).is_some_and(|provider| {
        provider.kind == ProviderKind::Ollama
            && crate::providers::policy::destination(&provider) == target.destination
    });
    if !provider_matches
        || backend.active_conversation_id.borrow().as_deref() != target.conversation_id.as_deref()
        || !workspace::provider_matches(backend)
    {
        sync_editor(ui, backend);
        return;
    }
    let value = ui.context_usage.pending_window.get();
    if value == target.value {
        return;
    }
    let was_new = target.conversation_id.is_none();
    let result = (|| -> Result<String> {
        let id = match target.conversation_id {
            Some(id) => id,
            None => create_empty_conversation(ui, backend)?,
        };
        let mut settings = load_chat_settings(backend, &id)?;
        if settings.num_ctx != value {
            settings.num_ctx = value;
            save_chat_settings(backend, &id, settings)?;
        }
        Ok(id)
    })();
    match result {
        Ok(id) => {
            ui.context_usage.editor_target.borrow_mut().take();
            if was_new {
                conversation_list::refresh(ui, backend);
                conversation_list::select(ui, &id);
            }
            refresh(ui, backend);
        }
        Err(error) => ui.toast_overlay.add_toast(adw::Toast::new(&gettext_f(
            "Context window could not be saved: {error}",
            &[("error", &crate::i18n::error_message(&error))],
        ))),
    }
}

fn hide(controls: &Controls) {
    controls.button.popdown();
    controls.button.set_visible(false);
    reset_usage(controls);
}

fn preview(ui: &WindowUi, backend: &Backend, provider: &Provider) -> Result<Option<ContextUsage>> {
    let id = backend.active_conversation_id.borrow().clone();
    let settings = id
        .as_deref()
        .map(|id| load_chat_settings(backend, id))
        .transpose()?
        .unwrap_or_default();
    let Ok(model) = request_model(ui, &settings) else {
        return Ok(None);
    };
    let num_ctx =
        (provider.kind == ProviderKind::Ollama).then(|| effective_context_window(settings.num_ctx));
    let window = model_context_window(ui, provider, &model, settings.num_ctx);
    let mut request = ChatRequest::streaming(&model, Vec::new())?;
    request.options = Some(ChatOptions {
        num_ctx,
        ..ChatOptions::default()
    });
    if provider.kind.supports_tool_calling() {
        if settings.calculator {
            request.tools.push(crate::tools::calculator());
        }
        if settings.web {
            request
                .tools
                .extend(backend.repository.web_config()?.definitions());
        }
    }
    let mut cached = ui.context_usage.history.borrow_mut();
    if cached.as_ref().is_none_or(|history| {
        history.conversation_id != id || history.message_limit != settings.context_messages
    }) {
        let mut messages = id
            .as_deref()
            .map(|id| backend.conversation_repository.list_messages(id))
            .transpose()?
            .unwrap_or_default();
        backend.conversation_repository.include_tool_context(
            &mut messages,
            settings.context_messages.saturating_sub(1) as usize,
        )?;
        *cached = Some(History {
            conversation_id: id.clone(),
            message_limit: settings.context_messages,
            messages,
        });
    }
    let history = cached
        .as_ref()
        .map(|history| history.messages.as_slice())
        .unwrap_or_default();
    let prompt = prompt_text(&ui.entry);
    request.messages = if let Some(id) = id {
        attachments::prepare(
            ui,
            backend,
            &id,
            &message_actions::Submission::New(prompt.clone()),
            &prompt,
            history,
            &settings,
            &request,
            attachments::Preparation::Preview,
        )?
        .0
    } else {
        build_conversation_context(
            history,
            &prompt,
            settings.context_messages as usize,
            Some(&settings.system_prompt),
        )
    };
    let overhead = context_overhead(&request)?;
    if provider.kind == ProviderKind::Ollama {
        ContextBudget::new(window.and_then(|window| i64::try_from(window).ok()))
            .fit(&mut request.messages, overhead)?;
    }
    let mut usage = estimate_request(&request, window)?;
    usage.omitted_messages = history
        .len()
        .saturating_sub(conversation_messages(&request).saturating_sub(1));
    Ok(Some(usage))
}

fn show(controls: &Controls, usage: ContextUsage, phase: Phase) {
    if usage.omitted_messages == 0 {
        clear_adjustment(controls);
    }
    let previous = controls.last_usage.replace(Some((usage, phase)));
    if previous == Some((usage, phase)) {
        controls.button.set_visible(true);
        return;
    }
    if previous.is_some_and(|(previous, _)| usage.omitted_messages > previous.omitted_messages) {
        show_adjustment(controls);
    }
    let fraction = usage.fraction();
    let full = fraction.is_some_and(|fraction| fraction >= 1.0);
    let prefix = if usage.estimated { "~" } else { "" };
    let amount = format!("{prefix}{}", format_tokens(usage.tokens));
    let summary = if let Some(window) = usage.window {
        gettext_f(
            "{used} of {capacity} tokens",
            &[("used", &amount), ("capacity", &format_tokens(window))],
        )
    } else {
        gettext_f("{used} tokens", &[("used", &amount)])
    };
    let label = usage_label(usage);
    if controls.label.text().as_str() != label {
        controls.label.set_label(&label);
    }
    if controls.fraction.replace(fraction) != fraction {
        controls.ring.queue_draw();
    }
    controls.summary.set_label(&summary);
    controls.phase.set_label(&gettext(phase.label()));
    controls.progress.set_visible(fraction.is_some());
    controls
        .progress
        .set_fraction(fraction.unwrap_or(0.0).min(1.0));
    let description = match (phase, usage.estimated) {
        (Phase::NextMessage, _) => gettext("Estimated context included in your next message."),
        (Phase::Response, true) => gettext(
            "Includes the current response and reasoning. Your next draft is counted afterward.",
        ),
        (Phase::Response, false) => {
            gettext("Input tokens reported by Ollama. The count grows as it responds.")
        }
    };
    let description = if usage.window.is_none() {
        format!(
            "{description}\n\n{}",
            gettext("This provider does not report a context limit.")
        )
    } else {
        description
    };
    controls.description.set_label(&description);
    let mut notices = Vec::new();
    if usage.omitted_messages > 0 {
        notices.push(gettext(match phase {
            Phase::NextMessage => gettext_noop(
                "Older messages were left out to make room for your next message. They remain saved in this chat.",
            ),
            Phase::Response => gettext_noop(
                "Older messages were left out to make room for this response. They remain saved in this chat.",
            ),
        }));
    }
    if full {
        notices.push(gettext("There is no room left in this context. Shorten your next message, adjust Chat Settings, or start a new chat."));
    } else if fraction.is_some_and(|fraction| fraction >= WARNING_FRACTION) {
        notices.push(gettext(
            "Context is nearly full. Older messages may be left out to make room for a response.",
        ));
    }
    let notice = notices.join("\n\n");
    controls.notice.set_label(&notice);
    controls.notice.set_visible(!notice.is_empty());
    controls
        .actions
        .set_visible(fraction.is_some_and(|fraction| fraction >= WARNING_FRACTION));
    set_severity(controls, fraction);
    let mut tooltip = gettext_f("Context usage: {usage}", &[("usage", &summary)]);
    tooltip.push_str(&format!("\n{}", gettext(phase.label())));
    if !notice.is_empty() {
        tooltip.push('\n');
        tooltip.push_str(&notice);
    }
    set_accessible_text(controls, &tooltip);
    controls.button.set_visible(true);
}

fn usage_label(usage: ContextUsage) -> String {
    let prefix = if usage.estimated { "~" } else { "" };
    match usage.fraction() {
        Some(fraction) if fraction >= 1.0 => gettext("Full"),
        Some(fraction) if fraction > 0.0 && fraction < 0.01 => "<1%".to_string(),
        Some(fraction) => format!("{prefix}{:.0}%", (fraction * 100.0).floor()),
        None if usage.tokens >= 1_000_000 => {
            format!("{prefix}{:.1}M", usage.tokens as f64 / 1_000_000.0)
        }
        None if usage.tokens >= 1000 => {
            format!("{prefix}{:.1}k", usage.tokens as f64 / 1000.0)
        }
        None => format!("{prefix}{}", usage.tokens),
    }
}

fn show_error(controls: &Controls, error: &MooseError) {
    reset_usage(controls);
    let full = matches!(error, MooseError::ContextTooLarge);
    let fraction = full.then_some(1.0);
    let label = if full { gettext("Full") } else { "—".into() };
    controls.label.set_label(&label);
    controls.fraction.set(fraction);
    controls.ring.queue_draw();
    controls
        .phase
        .set_label(&gettext(Phase::NextMessage.label()));
    controls.progress.set_visible(false);
    if full {
        let title = gettext("Context full");
        let notice = gettext(
            "Shorten your message, use fewer attachments, adjust Chat Settings, or start a new chat.",
        );
        controls.summary.set_label(&title);
        controls.description.set_label(&gettext("The current message, instructions, or attachments are estimated to leave too little room for a response. Ollama checks the exact size when you send."));
        controls.notice.set_label(&notice);
        set_accessible_text(controls, &format!("{title}\n{notice}"));
    } else {
        controls.summary.set_label(&gettext("Usage unavailable"));
        controls
            .description
            .set_label(&crate::i18n::error_message(error));
        set_accessible_text(controls, &gettext("Context usage is unavailable"));
    }
    controls.notice.set_visible(full);
    controls.actions.set_visible(full);
    set_severity(controls, fraction);
    controls.button.set_visible(true);
}

fn set_severity(controls: &Controls, fraction: Option<f64>) {
    let classes: &[&str] = match fraction {
        Some(value) if value >= CRITICAL_FRACTION => &["error"],
        Some(value) if value >= WARNING_FRACTION => &["warning"],
        _ => &[],
    };
    controls.ring.set_css_classes(classes);
    controls.label.set_css_classes(classes);
}

fn set_accessible_text(controls: &Controls, text: &str) {
    controls.button.set_tooltip_text(Some(text));
    controls
        .button
        .update_property(&[gtk::accessible::Property::Label(text)]);
}

fn format_tokens(tokens: usize) -> String {
    let digits = tokens.to_string();
    let mut formatted = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            formatted.push(',');
        }
        formatted.push(digit);
    }
    formatted
}
