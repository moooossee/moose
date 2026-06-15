use std::{cell::RefCell, rc::Rc};

use adw::prelude::*;
use gtk::{Align, Orientation};

use crate::{
    conversations::{
        DEFAULT_CONTEXT_MESSAGE_LIMIT, DEFAULT_TEMPERATURE, MAX_CONTEXT_MESSAGE_LIMIT,
    },
    error::Result,
    profiles::{ChatProfile, NewChatProfile},
};

use super::{
    Backend, ChatSettingsValues, WindowUi, restore_selected_provider_model, save_chat_settings,
    select_model_by_name, update_profile_indicator, widgets,
};

#[derive(Clone)]
struct SettingsControls {
    profile_values: Rc<RefCell<Vec<Option<ChatProfile>>>>,
    profile_row: adw::ComboRow,
    model_values: Rc<Vec<Option<String>>>,
    model_row: adw::ComboRow,
    temperature_row: adw::SpinRow,
    context_messages_row: adw::SpinRow,
    prompt_buffer: gtk::TextBuffer,
    top_p_row: adw::EntryRow,
    top_k_row: adw::EntryRow,
    seed_row: adw::EntryRow,
    num_ctx_row: adw::EntryRow,
}

pub(super) fn dialog(
    ui: &Rc<WindowUi>,
    backend: &Rc<Backend>,
    conversation_id: &str,
) -> Result<adw::PreferencesDialog> {
    let values = super::load_chat_settings(backend, conversation_id)?;
    let dialog = adw::PreferencesDialog::builder()
        .title("Chat Settings")
        .search_enabled(false)
        .build();
    dialog.set_content_width(680);
    dialog.set_content_height(760);
    let page = adw::PreferencesPage::builder()
        .title("Chat")
        .icon_name("preferences-system-symbolic")
        .build();

    let profiles = backend.profile_repository.list()?;
    let (profile_list, profile_values, selected_profile) =
        profile_choices(profiles, values.profile_id.as_deref());
    let (model_list, model_values, selected_model) =
        model_choices(ui, values.preferred_model.as_deref());
    let profile_group = adw::PreferencesGroup::builder()
        .title("Starting Point")
        .description(
            "Profiles apply a useful prompt and temperature. Every setting remains editable.",
        )
        .build();
    let profile_row = adw::ComboRow::builder()
        .title("Chat Profile")
        .model(&profile_list)
        .selected(selected_profile)
        .build();
    let profile_icon = gtk::Image::from_icon_name("avatar-default-symbolic");
    profile_icon.add_css_class("moose-settings-row-icon");
    profile_row.add_prefix(&profile_icon);
    update_profile_subtitle(&profile_row, &profile_values);
    let profile_actions = gtk::Box::builder()
        .orientation(Orientation::Horizontal)
        .spacing(8)
        .hexpand(true)
        .build();
    profile_actions.add_css_class("moose-settings-inline-actions");
    let profile_action_spacer = gtk::Box::builder().hexpand(true).build();
    let create_profile_button = action_button("list-add-symbolic", "Save as New Profile");
    create_profile_button.add_css_class("moose-settings-secondary-action");
    let delete_profile_button =
        widgets::icon_button("user-trash-symbolic", "Delete Selected Profile");
    delete_profile_button.add_css_class("moose-settings-inline-delete");
    delete_profile_button.add_css_class("destructive-action");
    delete_profile_button.set_sensitive(selected_profile_is_custom(&profile_row, &profile_values));
    profile_actions.append(&profile_action_spacer);
    profile_actions.append(&delete_profile_button);
    profile_actions.append(&create_profile_button);
    profile_group.add(&profile_row);
    profile_group.add(&profile_actions);

    let behavior_group = adw::PreferencesGroup::builder()
        .title("Response")
        .description("Control how this conversation uses its model and history.")
        .build();
    let model_row = adw::ComboRow::builder()
        .title("Preferred Model")
        .subtitle("Follow the chat model picker or pin this conversation to one model")
        .model(&model_list)
        .selected(selected_model)
        .build();
    let model_icon = gtk::Image::from_icon_name("computer-symbolic");
    model_icon.add_css_class("moose-settings-row-icon");
    model_row.add_prefix(&model_icon);
    let temperature_row = adw::SpinRow::with_range(0.0, 2.0, 0.05);
    temperature_row.set_title("Temperature");
    temperature_row.set_subtitle("Lower is focused and predictable; higher is more creative");
    temperature_row.set_digits(2);
    temperature_row.set_numeric(true);
    temperature_row.set_value(values.temperature);
    let temperature_icon = gtk::Image::from_icon_name("weather-clear-symbolic");
    temperature_icon.add_css_class("moose-settings-row-icon");
    temperature_row.add_prefix(&temperature_icon);
    let context_messages_row = adw::SpinRow::with_range(1.0, MAX_CONTEXT_MESSAGE_LIMIT as f64, 1.0);
    context_messages_row.set_title("Context Messages");
    context_messages_row.set_subtitle("Maximum recent messages included with your next request");
    context_messages_row.set_digits(0);
    context_messages_row.set_numeric(true);
    context_messages_row.set_snap_to_ticks(true);
    context_messages_row.set_value(values.context_messages as f64);
    let context_icon = gtk::Image::from_icon_name("view-list-symbolic");
    context_icon.add_css_class("moose-settings-row-icon");
    context_messages_row.add_prefix(&context_icon);

    behavior_group.add(&model_row);
    behavior_group.add(&temperature_row);
    behavior_group.add(&context_messages_row);

    let prompt_group = adw::PreferencesGroup::builder()
        .title("System Prompt")
        .description("Give the model lasting instructions for this conversation.")
        .build();
    let prompt_buffer = gtk::TextBuffer::new(None);
    prompt_buffer.set_text(&values.system_prompt);
    let prompt_view = gtk::TextView::builder()
        .buffer(&prompt_buffer)
        .accepts_tab(false)
        .bottom_margin(10)
        .left_margin(12)
        .right_margin(12)
        .top_margin(10)
        .wrap_mode(gtk::WrapMode::WordChar)
        .build();
    prompt_view.add_css_class("flat");
    prompt_view.add_css_class("moose-settings-text-view");
    let prompt_scrolled = gtk::ScrolledWindow::builder()
        .child(&prompt_view)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .max_content_height(180)
        .min_content_height(112)
        .propagate_natural_height(true)
        .vscrollbar_policy(gtk::PolicyType::Automatic)
        .build();
    prompt_scrolled.add_css_class("moose-settings-text");
    prompt_group.add(&prompt_scrolled);
    let prompt_footer = gtk::Box::builder()
        .orientation(Orientation::Horizontal)
        .spacing(8)
        .hexpand(true)
        .build();
    prompt_footer.add_css_class("moose-settings-prompt-footer");
    let prompt_hint = gtk::Label::builder()
        .label("Saved locally and sent before conversation messages")
        .halign(Align::Start)
        .hexpand(true)
        .xalign(0.0)
        .build();
    prompt_hint.add_css_class("dim-label");
    let prompt_count = gtk::Label::new(None);
    prompt_count.add_css_class("dim-label");
    update_prompt_count(&prompt_count, &prompt_buffer);
    prompt_footer.append(&prompt_hint);
    prompt_footer.append(&prompt_count);
    prompt_group.add(&prompt_footer);

    let target_profile_values = Rc::clone(&profile_values);
    let target_temperature_row = temperature_row.clone();
    let target_prompt_buffer = prompt_buffer.clone();
    let target_delete_profile_button = delete_profile_button.clone();
    profile_row.connect_selected_notify(move |row| {
        update_profile_subtitle(row, &target_profile_values);
        target_delete_profile_button
            .set_sensitive(selected_profile_is_custom(row, &target_profile_values));
        let selected = usize::try_from(row.selected()).unwrap_or(0);
        if let Some(profile) = target_profile_values
            .borrow()
            .get(selected)
            .cloned()
            .flatten()
        {
            target_temperature_row.set_value(profile.temperature);
            target_prompt_buffer.set_text(&profile.system_prompt);
        }
    });

    let advanced_row = adw::ExpanderRow::builder()
        .title("Advanced Ollama Options")
        .subtitle("Optional sampling and context-window overrides")
        .build();
    let advanced_icon = gtk::Image::from_icon_name("preferences-system-symbolic");
    advanced_icon.add_css_class("moose-settings-row-icon");
    advanced_row.add_prefix(&advanced_icon);
    let top_p_row = optional_entry("Top P", "Probability-based token selection", values.top_p);
    let top_k_row = optional_entry("Top K", "Maximum token choices per step", values.top_k);
    let seed_row = optional_entry(
        "Seed",
        "Fixed seed for more reproducible responses",
        values.seed,
    );
    let num_ctx_row = optional_entry(
        "Context Window",
        "Ollama token context size; larger values use more memory",
        values.num_ctx,
    );
    advanced_row.add_row(&top_p_row);
    advanced_row.add_row(&top_k_row);
    advanced_row.add_row(&seed_row);
    advanced_row.add_row(&num_ctx_row);
    behavior_group.add(&advanced_row);

    let action_group = adw::PreferencesGroup::new();
    let action_box = gtk::Box::builder()
        .orientation(Orientation::Horizontal)
        .spacing(8)
        .hexpand(true)
        .build();
    action_box.add_css_class("moose-settings-actions");
    let info_button = action_button("dialog-information-symbolic", "Settings Guide");
    let action_spacer = gtk::Box::builder().hexpand(true).build();
    let reset_button = action_button("edit-undo-symbolic", "Reset");
    let save_button = action_button("document-save-symbolic", "Save Changes");
    save_button.add_css_class("suggested-action");
    save_button.add_css_class("moose-settings-primary-action");
    action_box.append(&info_button);
    action_box.append(&action_spacer);
    action_box.append(&reset_button);
    action_box.append(&save_button);
    action_group.add(&action_box);

    page.add(&profile_group);
    page.add(&behavior_group);
    page.add(&prompt_group);
    page.add(&action_group);
    dialog.add(&page);

    let target_dialog = dialog.clone();
    info_button.connect_clicked(move |_| {
        show_settings_guide(&target_dialog);
    });

    let target_dialog = dialog.clone();
    let target_backend = Rc::clone(backend);
    let target_profile_list = profile_list.clone();
    let target_profile_values = Rc::clone(&profile_values);
    let target_profile_row = profile_row.clone();
    let target_temperature_row = temperature_row.clone();
    let target_prompt_buffer = prompt_buffer.clone();
    create_profile_button.connect_clicked(move |_| {
        show_create_profile_dialog(
            &target_dialog,
            &target_backend,
            &target_profile_list,
            &target_profile_values,
            &target_profile_row,
            target_temperature_row.value(),
            buffer_text(&target_prompt_buffer),
        );
    });

    let target_prompt_count = prompt_count.clone();
    prompt_buffer.connect_changed(move |buffer| {
        update_prompt_count(&target_prompt_count, buffer);
    });

    let target_dialog = dialog.clone();
    let target_backend = Rc::clone(backend);
    let target_profile_list = profile_list.clone();
    let target_profile_values = Rc::clone(&profile_values);
    let target_profile_row = profile_row.clone();
    let target_ui = Rc::clone(ui);
    let target_conversation_id = conversation_id.to_string();
    delete_profile_button.connect_clicked(move |_| {
        confirm_delete_profile(
            &target_dialog,
            &target_ui,
            &target_backend,
            &target_profile_list,
            &target_profile_values,
            &target_profile_row,
            &target_conversation_id,
        );
    });

    let target_profile_row = profile_row.clone();
    let target_model_row = model_row.clone();
    let target_temperature_row = temperature_row.clone();
    let target_context_messages_row = context_messages_row.clone();
    let target_prompt_buffer = prompt_buffer.clone();
    let target_top_p_row = top_p_row.clone();
    let target_top_k_row = top_k_row.clone();
    let target_seed_row = seed_row.clone();
    let target_num_ctx_row = num_ctx_row.clone();
    reset_button.connect_clicked(move |_| {
        target_profile_row.set_selected(0);
        target_model_row.set_selected(0);
        target_temperature_row.set_value(DEFAULT_TEMPERATURE);
        target_context_messages_row.set_value(DEFAULT_CONTEXT_MESSAGE_LIMIT as f64);
        target_prompt_buffer.set_text("");
        target_top_p_row.set_text("");
        target_top_k_row.set_text("");
        target_seed_row.set_text("");
        target_num_ctx_row.set_text("");
    });

    let target_dialog = dialog.clone();
    let target_ui = Rc::clone(ui);
    let target_backend = Rc::clone(backend);
    let target_conversation_id = conversation_id.to_string();
    let controls = SettingsControls {
        profile_values,
        profile_row,
        model_values,
        model_row,
        temperature_row,
        context_messages_row,
        prompt_buffer,
        top_p_row,
        top_k_row,
        seed_row,
        num_ctx_row,
    };
    save_button.connect_clicked(move |_| {
        let values = match values_from_controls(&controls) {
            Ok(values) => values,
            Err(message) => {
                target_dialog.add_toast(adw::Toast::new(&message));
                return;
            }
        };

        match save_chat_settings(&target_backend, &target_conversation_id, values.clone()) {
            Ok(()) => {
                if let Some(model) = values.preferred_model.as_deref() {
                    select_model_by_name(&target_ui, model);
                } else {
                    restore_selected_provider_model(&target_ui, &target_backend);
                }
                if let Err(error) = update_profile_indicator(
                    &target_ui,
                    &target_backend,
                    values.profile_id.as_deref(),
                ) {
                    target_ui.toast_overlay.add_toast(adw::Toast::new(&format!(
                        "Profile indicator could not be updated: {error}"
                    )));
                }
                target_ui
                    .toast_overlay
                    .add_toast(adw::Toast::new("Chat settings saved"));
                target_dialog.close();
            }
            Err(error) => {
                target_dialog.add_toast(adw::Toast::new(&format!(
                    "Chat settings could not be saved: {error}"
                )));
            }
        }
    });

    Ok(dialog)
}

fn show_settings_guide(parent: &adw::PreferencesDialog) {
    let dialog = adw::Dialog::builder()
        .title("Chat Settings Guide")
        .content_width(560)
        .content_height(620)
        .build();
    let header_bar = adw::HeaderBar::builder()
        .show_start_title_buttons(false)
        .show_end_title_buttons(false)
        .build();
    let title = adw::WindowTitle::new(
        "Chat Settings Guide",
        "Understand how each option changes a conversation",
    );
    header_bar.set_title_widget(Some(&title));
    let close_button = widgets::icon_button("window-close-symbolic", "Close");
    let target_dialog = dialog.clone();
    close_button.connect_clicked(move |_| {
        target_dialog.close();
    });
    header_bar.pack_end(&close_button);

    let summary_group = adw::PreferencesGroup::new();
    let summary = adw::ActionRow::builder()
        .title("A good place to start")
        .subtitle("Keep the defaults first, then adjust one setting at a time when you need a different result.")
        .subtitle_lines(2)
        .build();
    let summary_icon = gtk::Image::from_icon_name("dialog-information-symbolic");
    summary_icon.add_css_class("moose-settings-guide-icon");
    summary.add_prefix(&summary_icon);
    summary_group.add(&summary);

    let conversation_group = adw::PreferencesGroup::builder()
        .title("Conversation")
        .description("The most useful settings for everyday chats.")
        .build();
    for (icon, title, description) in [
        (
            "avatar-default-symbolic",
            "Chat Profile",
            "Applies a useful temperature and system prompt as a starting point. You can still edit every setting.",
        ),
        (
            "computer-symbolic",
            "Preferred Model",
            "Pins this conversation to one installed model. Use Selected Model follows the model picker in the chat.",
        ),
        (
            "preferences-system-symbolic",
            "Temperature",
            "Controls creativity. Lower values are more focused and predictable; higher values are more varied.",
        ),
        (
            "view-list-symbolic",
            "Context Messages",
            "Sets the maximum number of user and assistant messages sent to the model, including your next message.",
        ),
        (
            "object-select-symbolic",
            "System Prompt",
            "Gives the model lasting instructions for this conversation, such as tone, role or response style.",
        ),
    ] {
        conversation_group.add(&guide_row(icon, title, description));
    }

    let advanced_group = adw::PreferencesGroup::builder()
        .title("Advanced Ollama Options")
        .description("Leave these empty to use the model defaults.")
        .build();
    for (icon, title, description) in [
        (
            "preferences-system-symbolic",
            "Top P",
            "Limits token choices by probability. Lower values make responses more focused.",
        ),
        (
            "preferences-system-symbolic",
            "Top K",
            "Limits each token choice to the most likely options. Lower values reduce variety.",
        ),
        (
            "object-select-symbolic",
            "Seed",
            "Uses a fixed random seed to make repeated requests more reproducible.",
        ),
        (
            "view-list-symbolic",
            "Context Window",
            "Sets Ollama's token context size. Larger values may use more memory and can be slower.",
        ),
    ] {
        advanced_group.add(&guide_row(icon, title, description));
    }

    let content = gtk::Box::builder()
        .orientation(Orientation::Vertical)
        .spacing(18)
        .build();
    content.add_css_class("moose-settings-guide-content");
    content.append(&summary_group);
    content.append(&conversation_group);
    content.append(&advanced_group);
    let scrolled = gtk::ScrolledWindow::builder()
        .child(&content)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vscrollbar_policy(gtk::PolicyType::Automatic)
        .vexpand(true)
        .build();

    let toolbar_view = adw::ToolbarView::builder()
        .top_bar_style(adw::ToolbarStyle::Flat)
        .content(&scrolled)
        .build();
    toolbar_view.add_top_bar(&header_bar);
    dialog.set_child(Some(&toolbar_view));
    dialog.present(Some(parent));
}

fn guide_row(icon_name: &str, title: &str, description: &str) -> adw::ActionRow {
    let row = adw::ActionRow::builder()
        .title(title)
        .subtitle(description)
        .subtitle_lines(3)
        .build();
    let icon = gtk::Image::from_icon_name(icon_name);
    icon.add_css_class("moose-settings-guide-icon");
    row.add_prefix(&icon);
    row
}

fn update_prompt_count(label: &gtk::Label, buffer: &gtk::TextBuffer) {
    let count = buffer_text(buffer).chars().count();
    label.set_label(&format!("{count} characters"));
}

fn action_button(icon_name: &str, label: &str) -> gtk::Button {
    let icon = gtk::Image::from_icon_name(icon_name);
    let text = gtk::Label::new(Some(label));
    let content = gtk::Box::builder()
        .orientation(Orientation::Horizontal)
        .spacing(7)
        .halign(Align::Center)
        .build();
    content.append(&icon);
    content.append(&text);
    let button = gtk::Button::builder().child(&content).build();
    button.set_tooltip_text(Some(label));
    button
}

fn show_create_profile_dialog(
    parent: &adw::PreferencesDialog,
    backend: &Rc<Backend>,
    profile_list: &gtk::StringList,
    profile_values: &Rc<RefCell<Vec<Option<ChatProfile>>>>,
    profile_row: &adw::ComboRow,
    temperature: f64,
    system_prompt: String,
) {
    let dialog = adw::Dialog::builder()
        .title("Create Profile")
        .content_width(520)
        .content_height(420)
        .build();
    let header_bar = adw::HeaderBar::new();
    let title = adw::WindowTitle::new("Create Profile", "Reuse this setup in other conversations");
    header_bar.set_title_widget(Some(&title));
    let cancel_button = gtk::Button::with_label("Cancel");
    let create_button = gtk::Button::with_label("Create");
    create_button.add_css_class("suggested-action");
    create_button.set_sensitive(false);
    header_bar.pack_start(&cancel_button);
    header_bar.pack_end(&create_button);

    let group = adw::PreferencesGroup::builder()
        .title("Profile Details")
        .description("Choose a short, recognizable name and describe when this profile is useful.")
        .build();
    let name_row = adw::EntryRow::builder().title("Name").build();
    let description_row = adw::EntryRow::builder().title("Short Description").build();
    let name_icon = gtk::Image::from_icon_name("avatar-default-symbolic");
    name_icon.add_css_class("moose-settings-row-icon");
    name_row.add_prefix(&name_icon);
    let description_icon = gtk::Image::from_icon_name("document-edit-symbolic");
    description_icon.add_css_class("moose-settings-row-icon");
    description_row.add_prefix(&description_icon);
    group.add(&name_row);
    group.add(&description_row);
    let page = adw::PreferencesPage::new();
    page.add(&group);
    let toolbar_view = adw::ToolbarView::builder().content(&page).build();
    toolbar_view.add_top_bar(&header_bar);
    let toast_overlay = adw::ToastOverlay::new();
    toast_overlay.set_child(Some(&toolbar_view));
    dialog.set_child(Some(&toast_overlay));

    let target_dialog = dialog.clone();
    cancel_button.connect_clicked(move |_| {
        target_dialog.close();
    });

    let target_create_button = create_button.clone();
    let target_description_row = description_row.clone();
    name_row.connect_text_notify(move |row| {
        target_create_button.set_sensitive(
            !row.text().trim().is_empty() && !target_description_row.text().trim().is_empty(),
        );
    });

    let target_create_button = create_button.clone();
    let target_name_row = name_row.clone();
    description_row.connect_text_notify(move |row| {
        target_create_button.set_sensitive(
            !target_name_row.text().trim().is_empty() && !row.text().trim().is_empty(),
        );
    });

    let target_dialog = dialog.clone();
    let target_toast_overlay = toast_overlay.clone();
    let target_backend = Rc::clone(backend);
    let target_profile_list = profile_list.clone();
    let target_profile_values = Rc::clone(profile_values);
    let target_profile_row = profile_row.clone();
    create_button.connect_clicked(move |_| {
        match target_backend.profile_repository.create(NewChatProfile {
            name: name_row.text().to_string(),
            description: description_row.text().to_string(),
            temperature,
            system_prompt: system_prompt.clone(),
        }) {
            Ok(profile) => {
                target_profile_list.append(&profile.name);
                target_profile_values.borrow_mut().push(Some(profile));
                target_profile_row.set_selected(target_profile_list.n_items() - 1);
                target_dialog.close();
            }
            Err(error) => target_toast_overlay.add_toast(adw::Toast::new(&format!(
                "Profile could not be created: {error}"
            ))),
        }
    });

    dialog.present(Some(parent));
}

fn confirm_delete_profile(
    parent: &adw::PreferencesDialog,
    ui: &Rc<WindowUi>,
    backend: &Rc<Backend>,
    profile_list: &gtk::StringList,
    profile_values: &Rc<RefCell<Vec<Option<ChatProfile>>>>,
    profile_row: &adw::ComboRow,
    conversation_id: &str,
) {
    let selected = usize::try_from(profile_row.selected()).unwrap_or(0);
    let Some(profile) = profile_values
        .borrow()
        .get(selected)
        .cloned()
        .flatten()
        .filter(|profile| !profile.is_builtin)
    else {
        return;
    };
    let dialog = adw::AlertDialog::builder()
        .heading("Delete Profile?")
        .body(format!(
            "Delete \"{}\"? Existing chats will keep their current settings.",
            profile.name
        ))
        .close_response("cancel")
        .default_response("cancel")
        .build();
    dialog.add_response("cancel", "Cancel");
    dialog.add_response("delete", "Delete");
    dialog.set_response_appearance("delete", adw::ResponseAppearance::Destructive);

    let target_parent = parent.clone();
    let target_ui = Rc::clone(ui);
    let target_backend = Rc::clone(backend);
    let target_profile_list = profile_list.clone();
    let target_profile_values = Rc::clone(profile_values);
    let target_profile_row = profile_row.clone();
    let target_conversation_id = conversation_id.to_string();
    dialog.connect_response(Some("delete"), move |_, _| {
        match target_backend.profile_repository.delete(&profile.id) {
            Ok(()) => {
                target_profile_row.set_selected(0);
                target_profile_list.remove(selected as u32);
                target_profile_values.borrow_mut().remove(selected);
                let indicator_result =
                    super::load_chat_settings(&target_backend, &target_conversation_id).and_then(
                        |settings| {
                            update_profile_indicator(
                                &target_ui,
                                &target_backend,
                                settings.profile_id.as_deref(),
                            )
                        },
                    );
                if let Err(error) = indicator_result {
                    target_parent.add_toast(adw::Toast::new(&format!(
                        "Profile indicator could not be updated: {error}"
                    )));
                }
                target_parent.add_toast(adw::Toast::new("Profile deleted"));
            }
            Err(error) => target_parent.add_toast(adw::Toast::new(&format!(
                "Profile could not be deleted: {error}"
            ))),
        }
    });
    dialog.present(Some(parent));
}

fn profile_choices(
    profiles: Vec<ChatProfile>,
    selected_profile_id: Option<&str>,
) -> (gtk::StringList, Rc<RefCell<Vec<Option<ChatProfile>>>>, u32) {
    let mut labels = vec!["No Profile".to_string()];
    let mut values = vec![None];
    for profile in profiles {
        labels.push(profile.name.clone());
        values.push(Some(profile));
    }
    let selected = selected_profile_id
        .and_then(|profile_id| {
            values.iter().position(|profile| {
                profile
                    .as_ref()
                    .is_some_and(|profile| profile.id == profile_id)
            })
        })
        .unwrap_or(0) as u32;
    let label_refs = labels.iter().map(String::as_str).collect::<Vec<_>>();
    (
        gtk::StringList::new(&label_refs),
        Rc::new(RefCell::new(values)),
        selected,
    )
}

fn update_profile_subtitle(
    profile_row: &adw::ComboRow,
    profiles: &Rc<RefCell<Vec<Option<ChatProfile>>>>,
) {
    let selected = usize::try_from(profile_row.selected()).unwrap_or(0);
    let profiles = profiles.borrow();
    let subtitle = profiles
        .get(selected)
        .and_then(Option::as_ref)
        .map(|profile| profile.description.as_str())
        .unwrap_or("Use fully manual chat settings");
    profile_row.set_subtitle(subtitle);
}

fn selected_profile_is_custom(
    profile_row: &adw::ComboRow,
    profiles: &Rc<RefCell<Vec<Option<ChatProfile>>>>,
) -> bool {
    let selected = usize::try_from(profile_row.selected()).unwrap_or(0);
    profiles
        .borrow()
        .get(selected)
        .and_then(Option::as_ref)
        .is_some_and(|profile| !profile.is_builtin)
}

fn model_choices(
    ui: &WindowUi,
    preferred_model: Option<&str>,
) -> (gtk::StringList, Rc<Vec<Option<String>>>, u32) {
    let mut labels = vec!["Use Selected Model".to_string()];
    let mut values = vec![None];

    for model in ui.model_names.borrow().iter() {
        labels.push(model.clone());
        values.push(Some(model.clone()));
    }

    if let Some(model) = preferred_model
        && !values
            .iter()
            .any(|candidate| candidate.as_deref() == Some(model))
    {
        labels.push(format!("{model} (Unavailable)"));
        values.push(Some(model.to_string()));
    }

    let selected = preferred_model
        .and_then(|model| {
            values
                .iter()
                .position(|candidate| candidate.as_deref() == Some(model))
        })
        .unwrap_or(0) as u32;
    let label_refs = labels.iter().map(String::as_str).collect::<Vec<_>>();
    (gtk::StringList::new(&label_refs), Rc::new(values), selected)
}

fn optional_entry<T: ToString>(title: &str, subtitle: &str, value: Option<T>) -> adw::EntryRow {
    let row = adw::EntryRow::builder()
        .title(title)
        .text(value.map(|value| value.to_string()).unwrap_or_default())
        .build();
    row.set_tooltip_text(Some(subtitle));
    row
}

fn values_from_controls(
    controls: &SettingsControls,
) -> std::result::Result<ChatSettingsValues, String> {
    let selected_profile = usize::try_from(controls.profile_row.selected()).unwrap_or(0);
    let profile_id = controls
        .profile_values
        .borrow()
        .get(selected_profile)
        .and_then(Option::as_ref)
        .map(|profile| profile.id.clone());
    let selected_model = usize::try_from(controls.model_row.selected()).unwrap_or(0);
    let preferred_model = controls.model_values.get(selected_model).cloned().flatten();

    Ok(ChatSettingsValues {
        profile_id,
        preferred_model,
        temperature: controls.temperature_row.value(),
        top_p: parse_optional_probability(&controls.top_p_row, "Top P")?,
        top_k: parse_optional_non_negative_i64(&controls.top_k_row, "Top K")?,
        seed: parse_optional_i64(&controls.seed_row, "Seed")?,
        num_ctx: parse_optional_non_negative_i64(&controls.num_ctx_row, "Context Window")?,
        context_messages: controls.context_messages_row.value().round() as i64,
        system_prompt: buffer_text(&controls.prompt_buffer),
    })
}

fn buffer_text(buffer: &gtk::TextBuffer) -> String {
    let (start, end) = buffer.bounds();
    buffer.text(&start, &end, true).to_string()
}

fn parse_optional_probability(
    row: &adw::EntryRow,
    field: &str,
) -> std::result::Result<Option<f64>, String> {
    let Some(value) = parse_optional_f64(row, field)? else {
        return Ok(None);
    };

    if (0.0..=1.0).contains(&value) {
        Ok(Some(value))
    } else {
        Err(format!("{field} must be between 0 and 1"))
    }
}

fn parse_optional_f64(
    row: &adw::EntryRow,
    field: &str,
) -> std::result::Result<Option<f64>, String> {
    let text = row.text();
    let text = text.trim();
    if text.is_empty() {
        return Ok(None);
    }

    let value = text
        .parse::<f64>()
        .map_err(|_| format!("{field} must be a number"))?;
    if value.is_finite() && value >= 0.0 {
        Ok(Some(value))
    } else {
        Err(format!("{field} must be 0 or greater"))
    }
}

fn parse_optional_non_negative_i64(
    row: &adw::EntryRow,
    field: &str,
) -> std::result::Result<Option<i64>, String> {
    let Some(value) = parse_optional_i64(row, field)? else {
        return Ok(None);
    };

    if value >= 0 {
        Ok(Some(value))
    } else {
        Err(format!("{field} must be 0 or greater"))
    }
}

fn parse_optional_i64(
    row: &adw::EntryRow,
    field: &str,
) -> std::result::Result<Option<i64>, String> {
    let text = row.text();
    let text = text.trim();
    if text.is_empty() {
        return Ok(None);
    }

    text.parse::<i64>()
        .map(Some)
        .map_err(|_| format!("{field} must be a whole number"))
}
