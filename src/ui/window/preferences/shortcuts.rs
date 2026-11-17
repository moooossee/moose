use crate::i18n::{gettext, gettext_f, gettext_noop};

use std::{cell::RefCell, collections::HashMap, rc::Rc};

use adw::prelude::*;
use gtk::{Align, Orientation, gdk};

use super::super::{
    Backend, WindowUi, reset_shortcut_values, save_shortcut_values, set_shortcut_capture_active,
    shortcut_values,
    shortcuts::{self as bindings, ShortcutCategory, ShortcutDefinition},
};

#[derive(Clone)]
struct ShortcutRow {
    definition: &'static ShortcutDefinition,
    row: adw::ActionRow,
    label: adw::ShortcutLabel,
}

pub(super) fn build(
    parent: &adw::PreferencesDialog,
    ui: &Rc<WindowUi>,
    backend: &Rc<Backend>,
) -> adw::PreferencesPage {
    let page = adw::PreferencesPage::builder()
        .name("shortcuts")
        .title(gettext("Shortcuts"))
        .icon_name("preferences-desktop-keyboard-symbolic")
        .build();
    let reset_all = gtk::Button::with_label(&gettext("Reset All"));
    reset_all.set_valign(Align::Center);
    reset_all.set_tooltip_text(Some(&gettext("Restore all default keyboard shortcuts")));
    let heading = adw::PreferencesGroup::builder()
        .title(gettext("Keyboard Shortcuts"))
        .description(gettext("Select an action to change its keyboard shortcut."))
        .header_suffix(&reset_all)
        .build();
    page.add(&heading);

    let values = shortcut_values(backend);
    let mut rows = Vec::new();
    for (category, title) in [
        (
            ShortcutCategory::Conversations,
            gettext_noop("Conversations"),
        ),
        (ShortcutCategory::Models, gettext_noop("Models")),
        (ShortcutCategory::Application, gettext_noop("Application")),
    ] {
        let group = adw::PreferencesGroup::builder()
            .title(gettext(title))
            .build();
        for definition in bindings::DEFINITIONS
            .iter()
            .filter(|definition| definition.category == category)
        {
            let row = adw::ActionRow::builder()
                .title(gettext(definition.title))
                .subtitle(gettext(definition.description))
                .title_lines(2)
                .subtitle_lines(2)
                .use_markup(false)
                .activatable(true)
                .focusable(true)
                .build();
            let label = shortcut_label();
            row.add_suffix(&label);
            let control = ShortcutRow {
                definition,
                row,
                label,
            };
            update_row(&control, value_for(&values, definition));

            let weak_parent = parent.downgrade();
            let weak_ui = Rc::downgrade(ui);
            let weak_reset = reset_all.downgrade();
            let target_backend = Rc::clone(backend);
            let target_label = control.label.clone();
            control.row.connect_activated(move |row| {
                let (Some(parent), Some(ui), Some(reset_all)) = (
                    weak_parent.upgrade(),
                    weak_ui.upgrade(),
                    weak_reset.upgrade(),
                ) else {
                    return;
                };
                let control = ShortcutRow {
                    definition,
                    row: row.clone(),
                    label: target_label.clone(),
                };
                show_capture_dialog(&parent, &ui, &target_backend, &control, &reset_all);
            });
            group.add(&control.row);
            rows.push(control);
        }
        page.add(&group);
    }
    update_reset_button(&reset_all, &values);

    let weak_parent = parent.downgrade();
    let weak_ui = Rc::downgrade(ui);
    let target_backend = Rc::clone(backend);
    reset_all.connect_clicked(move |button| {
        let (Some(parent), Some(ui)) = (weak_parent.upgrade(), weak_ui.upgrade()) else {
            return;
        };
        match reset_shortcut_values(&ui, &target_backend) {
            Ok(values) => {
                for control in &rows {
                    update_row(control, value_for(&values, control.definition));
                }
                update_reset_button(button, &values);
            }
            Err(message) => parent.add_toast(adw::Toast::new(&message)),
        }
    });

    page
}

fn show_capture_dialog(
    parent: &adw::PreferencesDialog,
    ui: &Rc<WindowUi>,
    backend: &Rc<Backend>,
    control: &ShortcutRow,
    reset_all: &gtk::Button,
) {
    let definition = control.definition;
    let values = shortcut_values(backend);
    let current_value = value_for(&values, definition).to_string();
    let candidate = Rc::new(RefCell::new(current_value.clone()));
    let dialog = adw::Dialog::builder()
        .title(gettext("Set Shortcut"))
        .content_width(480)
        .follows_content_size(true)
        .build();
    let header = adw::HeaderBar::builder()
        .show_start_title_buttons(false)
        .show_end_title_buttons(false)
        .build();
    let cancel = gtk::Button::with_mnemonic(&gettext("_Cancel"));
    let apply = gtk::Button::with_mnemonic(&gettext("_Set"));
    apply.add_css_class("suggested-action");
    apply.set_sensitive(false);
    header.pack_start(&cancel);
    header.pack_end(&apply);

    let content = gtk::Box::builder()
        .orientation(Orientation::Vertical)
        .spacing(18)
        .margin_top(24)
        .margin_bottom(24)
        .margin_start(24)
        .margin_end(24)
        .build();
    let icon = gtk::Image::from_icon_name("preferences-desktop-keyboard-symbolic");
    icon.set_pixel_size(48);
    icon.add_css_class("dim-label");
    let title = gtk::Label::builder()
        .label(gettext(definition.title))
        .css_classes(["title-2"])
        .justify(gtk::Justification::Center)
        .wrap(true)
        .build();
    let description = gtk::Label::builder()
        .label(gettext("Press a key combination, then choose Set."))
        .justify(gtk::Justification::Center)
        .wrap(true)
        .css_classes(["dim-label"])
        .build();
    let preview = shortcut_label();
    update_label(&preview, &current_value);
    preview.set_halign(Align::Center);
    preview.set_margin_top(16);
    preview.set_margin_bottom(16);
    let capture = gtk::Button::builder()
        .child(&preview)
        .tooltip_text(gettext(
            "Press a key combination while this control is focused",
        ))
        .build();
    update_capture_label(&capture, &current_value);
    capture.update_property(&[gtk::accessible::Property::Description(&gettext(
        "Press a key combination. Press Tab to move to the dialog controls.",
    ))]);
    let error = gtk::Label::builder()
        .wrap(true)
        .justify(gtk::Justification::Center)
        .css_classes(["error"])
        .accessible_role(gtk::AccessibleRole::Alert)
        .visible(false)
        .build();
    let hint = gtk::Label::builder()
        .label(gettext(
            "Press Tab to move between controls. Escape can be assigned as a shortcut.",
        ))
        .justify(gtk::Justification::Center)
        .wrap(true)
        .css_classes(["caption", "dim-label"])
        .build();
    let restore = gtk::Button::with_mnemonic(&gettext("Reset to _Default"));
    let disable = gtk::Button::with_mnemonic(&gettext("_Disable Shortcut"));
    let actions = gtk::Box::new(Orientation::Vertical, 6);
    actions.append(&restore);
    actions.append(&disable);
    content.append(&icon);
    content.append(&title);
    content.append(&description);
    content.append(&capture);
    content.append(&error);
    content.append(&hint);
    content.append(&actions);
    let scrolled = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .propagate_natural_height(true)
        .child(&content)
        .build();
    let toolbar = adw::ToolbarView::builder().content(&scrolled).build();
    toolbar.add_top_bar(&header);
    dialog.set_child(Some(&toolbar));

    let target_candidate = Rc::clone(&candidate);
    let target_backend = Rc::clone(backend);
    let target_preview = preview.clone();
    let target_error = error.clone();
    let target_apply = apply.clone();
    let weak_capture = capture.downgrade();
    let update_candidate = Rc::new(move |value: &str| {
        *target_candidate.borrow_mut() = value.to_string();
        update_label(&target_preview, value);
        if let Some(capture) = weak_capture.upgrade() {
            update_capture_label(&capture, value);
        }
        let mut values = shortcut_values(&target_backend);
        values.insert(definition.id.to_string(), value.to_string());
        match bindings::normalize_values(values) {
            Ok(_) => {
                target_error.set_visible(false);
                target_apply.set_sensitive(value != current_value);
            }
            Err(message) => {
                show_error(&target_error, &message);
                target_apply.set_sensitive(false);
            }
        }
    });

    let update = Rc::clone(&update_candidate);
    restore.connect_clicked(move |_| update(definition.default));
    let update = Rc::clone(&update_candidate);
    disable.connect_clicked(move |_| update(""));
    let weak_dialog = dialog.downgrade();
    cancel.connect_clicked(move |_| {
        if let Some(dialog) = weak_dialog.upgrade() {
            dialog.close();
        }
    });

    let weak_dialog = dialog.downgrade();
    let target_ui = Rc::clone(ui);
    let target_backend = Rc::clone(backend);
    let target_control = control.clone();
    let target_reset = reset_all.clone();
    let target_error = error.clone();
    apply.connect_clicked(move |_| {
        let Some(dialog) = weak_dialog.upgrade() else {
            return;
        };
        let value = candidate.borrow().clone();
        let mut values = shortcut_values(&target_backend);
        values.insert(definition.id.to_string(), value.clone());
        match save_shortcut_values(&target_ui, &target_backend, values) {
            Ok(()) => {
                update_row(&target_control, &value);
                update_reset_button(&target_reset, &shortcut_values(&target_backend));
                dialog.close();
            }
            Err(message) => show_error(&target_error, &message),
        }
    });

    let target_apply = apply.clone();
    let controller = gtk::EventControllerKey::new();
    controller.set_propagation_phase(gtk::PropagationPhase::Capture);
    controller.connect_key_pressed(move |_, key, _, state| {
        if matches!(key, gdk::Key::Tab | gdk::Key::ISO_Left_Tab)
            && !state.intersects(
                gdk::ModifierType::CONTROL_MASK
                    | gdk::ModifierType::ALT_MASK
                    | gdk::ModifierType::SUPER_MASK,
            )
        {
            return gtk::glib::Propagation::Proceed;
        }
        if is_modifier(key) {
            return gtk::glib::Propagation::Stop;
        }
        if let Some(chord) = bindings::event_chord(key, state) {
            update_candidate(&chord.label());
        } else {
            show_error(&error, &gettext("This key cannot be used as a shortcut"));
            target_apply.set_sensitive(false);
        }
        gtk::glib::Propagation::Stop
    });
    capture.add_controller(controller);

    let target_backend = Rc::clone(backend);
    dialog.connect_closed(move |_| set_shortcut_capture_active(&target_backend, false));
    set_shortcut_capture_active(backend, true);
    dialog.set_focus(Some(&capture));
    dialog.present(Some(parent));
}

fn shortcut_label() -> adw::ShortcutLabel {
    let label = adw::ShortcutLabel::new("");
    label.set_disabled_text(&gettext("Disabled"));
    label.set_valign(Align::Center);
    label.set_halign(Align::End);
    label
}

fn update_label(label: &adw::ShortcutLabel, value: &str) {
    let accelerator = bindings::parse(value)
        .ok()
        .flatten()
        .map(|chord| chord.accelerator())
        .unwrap_or_default();
    label.set_accelerator(&accelerator);
}

fn update_row(control: &ShortcutRow, value: &str) {
    update_label(&control.label, value);
    let shortcut = display_label(value);
    control
        .row
        .update_property(&[gtk::accessible::Property::Description(&gettext_f(
            "Current shortcut: {shortcut}. Activate to change it.",
            &[("shortcut", &shortcut)],
        ))]);
}

fn update_capture_label(button: &gtk::Button, value: &str) {
    button.update_property(&[gtk::accessible::Property::Label(&gettext_f(
        "Shortcut: {shortcut}",
        &[("shortcut", &display_label(value))],
    ))]);
}

fn display_label(value: &str) -> String {
    bindings::parse(value)
        .ok()
        .flatten()
        .map(|chord| chord.display_label())
        .unwrap_or_else(|| gettext("Disabled"))
}

fn update_reset_button(button: &gtk::Button, values: &HashMap<String, String>) {
    button.set_sensitive(
        bindings::DEFINITIONS
            .iter()
            .any(|definition| value_for(values, definition) != definition.default),
    );
}

fn value_for<'a>(values: &'a HashMap<String, String>, definition: &ShortcutDefinition) -> &'a str {
    values
        .get(definition.id)
        .map(String::as_str)
        .unwrap_or(definition.default)
}

fn show_error(label: &gtk::Label, message: &str) {
    label.set_label(message);
    label.set_visible(true);
}

fn is_modifier(key: gdk::Key) -> bool {
    matches!(
        key,
        gdk::Key::Shift_L
            | gdk::Key::Shift_R
            | gdk::Key::Control_L
            | gdk::Key::Control_R
            | gdk::Key::Alt_L
            | gdk::Key::Alt_R
            | gdk::Key::Super_L
            | gdk::Key::Super_R
            | gdk::Key::Meta_L
            | gdk::Key::Meta_R
            | gdk::Key::Hyper_L
            | gdk::Key::Hyper_R
            | gdk::Key::ISO_Level3_Shift
            | gdk::Key::ISO_Level5_Shift
            | gdk::Key::Mode_switch
            | gdk::Key::Caps_Lock
            | gdk::Key::Num_Lock
            | gdk::Key::Scroll_Lock
    )
}
