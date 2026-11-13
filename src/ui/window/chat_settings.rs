use crate::i18n::{gettext, gettext_f, gettext_noop, ngettext_f};

use std::{cell::RefCell, rc::Rc};

use adw::prelude::*;
use gtk::{Align, Orientation, gio};

use crate::{
    conversations::{
        DEFAULT_CONTEXT_MESSAGE_LIMIT, DEFAULT_TEMPERATURE, MAX_CONTEXT_MESSAGE_LIMIT,
    },
    error::Result,
    profiles::{ChatProfile, NewChatProfile},
    providers::ProviderKind,
};

use super::{
    Backend, ChatSettingsValues, WindowUi, active_provider, restore_selected_provider_model,
    save_chat_settings, select_model_by_name, update_profile_indicator, widgets,
};

#[derive(Clone)]
struct SettingsControls {
    calculator_row: adw::SwitchRow,
    web_row: adw::SwitchRow,
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
) -> Result<adw::Dialog> {
    let values = super::load_chat_settings(backend, conversation_id)?;
    let dialog = adw::Dialog::builder()
        .title(gettext("Chat Settings"))
        .content_width(620)
        .content_height(660)
        .build();
    let navigation = adw::NavigationView::new();
    let toast_overlay = adw::ToastOverlay::new();
    let page = adw::PreferencesPage::new();
    let (main_page, header) = settings_page(&gettext("Chat Settings"), &page);
    navigation.add(&main_page);

    let actions = gio::SimpleActionGroup::new();
    let create_profile_action = gio::SimpleAction::new("create-profile", None);
    let delete_profile_action = gio::SimpleAction::new("delete-profile", None);
    let reset_action = gio::SimpleAction::new("reset", None);
    actions.add_action(&create_profile_action);
    actions.add_action(&delete_profile_action);
    actions.add_action(&reset_action);
    dialog.insert_action_group("settings", Some(&actions));
    let menu = gio::Menu::new();
    let profile_menu = gio::Menu::new();
    profile_menu.append(
        Some(&gettext("Save as Profile…")),
        Some("settings.create-profile"),
    );
    profile_menu.append(
        Some(&gettext("Delete Profile…")),
        Some("settings.delete-profile"),
    );
    menu.append_section(None, &profile_menu);
    let reset_menu = gio::Menu::new();
    reset_menu.append(Some(&gettext("Reset to Defaults")), Some("settings.reset"));
    menu.append_section(None, &reset_menu);
    let menu_button = gtk::MenuButton::builder()
        .icon_name("view-more-symbolic")
        .tooltip_text(gettext("More Options"))
        .menu_model(&menu)
        .build();
    header.pack_end(&menu_button);

    let save_button = gtk::Button::with_label(&gettext("Save Changes"));
    let cancel_button = gtk::Button::with_label(&gettext("Cancel"));
    cancel_button.add_css_class("flat");
    cancel_button.set_halign(Align::Center);
    let footer = gtk::Box::builder()
        .orientation(Orientation::Vertical)
        .spacing(8)
        .margin_top(12)
        .margin_bottom(20)
        .margin_start(24)
        .margin_end(24)
        .build();
    footer.append(&widgets::primary_action(&save_button));
    footer.append(&cancel_button);
    let toolbar = adw::ToolbarView::builder().content(&navigation).build();
    toolbar.add_bottom_bar(&footer);
    toast_overlay.set_child(Some(&toolbar));
    dialog.set_child(Some(&toast_overlay));

    let target_dialog = dialog.downgrade();
    cancel_button.connect_clicked(move |_| {
        if let Some(dialog) = target_dialog.upgrade() {
            dialog.close();
        }
    });

    let profiles = backend.profile_repository.list()?;
    let (profile_list, profile_values, selected_profile) =
        profile_choices(profiles, values.profile_id.as_deref());
    let (model_list, model_values, selected_model) =
        model_choices(ui, values.preferred_model.as_deref());
    let profile_group = adw::PreferencesGroup::builder()
        .title(gettext("Conversation"))
        .build();
    let profile_row = adw::ComboRow::builder()
        .title(gettext("Profile"))
        .subtitle_lines(2)
        .use_markup(false)
        .model(&profile_list)
        .selected(selected_profile)
        .build();
    update_profile_subtitle(&profile_row, &profile_values);
    delete_profile_action.set_enabled(selected_profile_is_custom(&profile_row, &profile_values));
    profile_group.add(&profile_row);

    let behavior_group = adw::PreferencesGroup::builder()
        .title(gettext("Response"))
        .build();
    let model_row = adw::ComboRow::builder()
        .title(gettext("Preferred Model"))
        .subtitle(gettext("Choose a model to always use in this chat"))
        .subtitle_lines(2)
        .use_markup(false)
        .expression(gtk::PropertyExpression::new(
            gtk::StringObject::static_type(),
            gtk::Expression::NONE,
            "string",
        ))
        .enable_search(true)
        .search_match_mode(gtk::StringFilterMatchMode::Substring)
        .model(&model_list)
        .selected(selected_model)
        .build();
    profile_group.add(&model_row);
    page.add(&profile_group);
    let temperature_row = adw::SpinRow::with_range(0.0, 2.0, 0.05);
    temperature_row.set_title(&gettext("Temperature"));
    temperature_row.set_subtitle(&gettext("Lower is more focused; higher is more creative"));
    temperature_row.set_subtitle_lines(2);
    temperature_row.set_digits(2);
    temperature_row.set_numeric(true);
    temperature_row.set_value(values.temperature);
    let context_messages_row = adw::SpinRow::with_range(1.0, MAX_CONTEXT_MESSAGE_LIMIT as f64, 1.0);
    context_messages_row.set_title(&gettext("Context Messages"));
    context_messages_row.set_subtitle(&gettext(
        "Maximum recent messages, keeping questions and answers together",
    ));
    context_messages_row.set_subtitle_lines(2);
    context_messages_row.set_digits(0);
    context_messages_row.set_numeric(true);
    context_messages_row.set_snap_to_ticks(true);
    context_messages_row.set_value(values.context_messages as f64);
    behavior_group.add(&temperature_row);
    let provider_kind = active_provider(backend).map(|provider| provider.api_kind());
    if provider_kind == Some(ProviderKind::DeepSeek) {
        temperature_row.set_subtitle(&gettext(
            "Applies when reasoning is off; higher is more creative",
        ));
    }
    let sampling = !matches!(
        provider_kind,
        Some(ProviderKind::OpenAi | ProviderKind::Anthropic)
    );
    temperature_row.set_visible(sampling);
    behavior_group.add(&context_messages_row);

    let tools_group = adw::PreferencesGroup::builder()
        .title(gettext("Tools"))
        .description(gettext(
            "Allow tools in this chat. Requires a model with tool support.",
        ))
        .build();
    let calculator_row = adw::SwitchRow::builder()
        .title(gettext("Calculator"))
        .subtitle(gettext("Run calculations on this device"))
        .subtitle_lines(2)
        .active(values.calculator)
        .build();
    calculator_row.add_prefix(&gtk::Image::from_icon_name(
        "accessories-calculator-symbolic",
    ));
    if !provider_kind.is_some_and(ProviderKind::supports_tool_calling) {
        calculator_row.set_sensitive(false);
        calculator_row.set_subtitle(&gettext(
            "Available with compatible Ollama and Ollama Cloud models",
        ));
    } else if active_provider(backend).is_some_and(|provider| provider.is_remote()) {
        calculator_row.set_subtitle(&gettext(
            "Calculate on this device and share results with this chat’s provider",
        ));
    }
    tools_group.add(&calculator_row);
    let web_row = adw::SwitchRow::builder()
        .title(gettext("Web"))
        .subtitle(gettext("Search and read public pages with your permission"))
        .subtitle_lines(2)
        .active(values.web)
        .build();
    web_row.add_prefix(&gtk::Image::from_icon_name("moose-globe-symbolic"));
    if !provider_kind.is_some_and(ProviderKind::supports_tool_calling) {
        web_row.set_sensitive(false);
        web_row.set_subtitle(&gettext(
            "Available with compatible Ollama and Ollama Cloud models",
        ));
    } else if backend.network_policy.local_only() {
        web_row.set_subtitle(&gettext("Blocked by Local Only in Preferences → Privacy"));
    }
    tools_group.add(&web_row);
    let web_config = backend.repository.web_config()?;
    let search_row = navigation_row(
        &gettext("Web Settings"),
        &crate::i18n::web_description(&web_config),
        None,
    );
    let search_group = adw::PreferencesGroup::builder()
        .description(gettext("Manage search and web permissions for all chats."))
        .build();
    search_group.add(&search_row);
    let tools_content = adw::PreferencesPage::new();
    tools_content.add(&tools_group);
    tools_content.add(&search_group);
    let (tools_page, _) = settings_page(&gettext("Tools"), &tools_content);
    let target_ui = Rc::clone(ui);
    let target_backend = Rc::clone(backend);
    let target_dialog = dialog.downgrade();
    let target_search_row = search_row.downgrade();
    search_row.connect_activated(move |_| {
        let Some(dialog) = target_dialog.upgrade() else {
            return;
        };
        let web_dialog = super::web_tools::show_settings(&dialog, &target_ui, &target_backend);
        let row = target_search_row.clone();
        let backend = Rc::clone(&target_backend);
        web_dialog.connect_closed(move |_| {
            if let Some(row) = row.upgrade()
                && let Ok(config) = backend.repository.web_config()
            {
                row.set_subtitle(&crate::i18n::web_description(&config));
            }
        });
    });

    let prompt_group = adw::PreferencesGroup::builder()
        .title(gettext("System Prompt"))
        .description(gettext(
            "Instructions sent with every message in this chat.",
        ))
        .build();
    let prompt_buffer = gtk::TextBuffer::new(None);
    let displayed_prompt = profile_values
        .borrow()
        .iter()
        .flatten()
        .find(|profile| {
            Some(profile.id.as_str()) == values.profile_id.as_deref()
                && profile.system_prompt == values.system_prompt
        })
        .map(crate::i18n::profile_system_prompt)
        .unwrap_or_else(|| values.system_prompt.clone());
    prompt_buffer.set_text(&displayed_prompt);
    let prompt_view = gtk::TextView::builder()
        .buffer(&prompt_buffer)
        .accepts_tab(false)
        .bottom_margin(12)
        .left_margin(12)
        .right_margin(12)
        .top_margin(12)
        .wrap_mode(gtk::WrapMode::WordChar)
        .build();
    let prompt_scrolled = gtk::ScrolledWindow::builder()
        .child(&prompt_view)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .max_content_height(360)
        .min_content_height(240)
        .propagate_natural_height(true)
        .vscrollbar_policy(gtk::PolicyType::Automatic)
        .build();
    prompt_scrolled.add_css_class("card");
    prompt_group.add(&prompt_scrolled);
    let prompt_count = gtk::Label::new(None);
    prompt_count.add_css_class("dim-label");
    prompt_count.add_css_class("caption");
    update_prompt_count(&prompt_count, &prompt_buffer);
    prompt_group.set_header_suffix(Some(&prompt_count));
    let prompt_content = adw::PreferencesPage::new();
    prompt_content.add(&prompt_group);
    let (prompt_page, _) = settings_page(&gettext("System Prompt"), &prompt_content);

    let target_profile_values = Rc::clone(&profile_values);
    let target_temperature_row = temperature_row.clone();
    let target_prompt_buffer = prompt_buffer.clone();
    let target_delete_profile_action = delete_profile_action.downgrade();
    profile_row.connect_selected_notify(move |row| {
        update_profile_subtitle(row, &target_profile_values);
        if let Some(action) = target_delete_profile_action.upgrade() {
            action.set_enabled(selected_profile_is_custom(row, &target_profile_values));
        }
        let selected = usize::try_from(row.selected()).unwrap_or(0);
        if let Some(profile) = target_profile_values
            .borrow()
            .get(selected)
            .cloned()
            .flatten()
        {
            target_temperature_row.set_value(profile.temperature);
            target_prompt_buffer.set_text(&crate::i18n::profile_system_prompt(&profile));
        }
    });

    let advanced_group = adw::PreferencesGroup::builder()
        .title(gettext("Ollama Options"))
        .description(gettext(
            "Context Window defaults to 16,384 tokens. Other empty fields use the model defaults.",
        ))
        .build();
    let top_p_row = optional_entry(
        &gettext("Top P"),
        &gettext("Probability-based token selection"),
        values.top_p,
    );
    let top_k_row = optional_entry(
        &gettext("Top K"),
        &gettext("Maximum token choices per step"),
        values.top_k,
    );
    let seed_row = optional_entry(
        &gettext("Seed"),
        &gettext("Fixed seed for more reproducible responses"),
        values.seed,
    );
    let default_context_window = (provider_kind == Some(ProviderKind::Ollama))
        .then_some(crate::ollama::DEFAULT_CONTEXT_WINDOW);
    let num_ctx_row = optional_entry(
        &gettext("Context Window"),
        &gettext("Defaults to 16,384 tokens. Larger values use more memory."),
        values.num_ctx.or(default_context_window),
    );
    advanced_group.add(&top_p_row);
    advanced_group.add(&top_k_row);
    advanced_group.add(&seed_row);
    advanced_group.add(&num_ctx_row);
    advanced_group.set_visible(provider_kind == Some(ProviderKind::Ollama));
    let advanced_content = adw::PreferencesPage::new();
    let guide_group = adw::PreferencesGroup::new();
    let guide_row = navigation_row(
        &gettext("Settings Guide"),
        &gettext("Learn what each parameter does"),
        Some("dialog-information-symbolic"),
    );
    connect_page(&guide_row, &navigation, &settings_guide());
    guide_group.add(&guide_row);
    advanced_content.add(&guide_group);
    advanced_content.add(&behavior_group);
    advanced_content.add(&advanced_group);
    let (advanced_page, _) = settings_page(&gettext("Advanced"), &advanced_content);

    let customize_group = adw::PreferencesGroup::builder()
        .title(gettext("Customize"))
        .build();
    let prompt_row = navigation_row(
        &gettext("System Prompt"),
        "",
        Some("document-edit-symbolic"),
    );
    update_prompt_summary(&prompt_row, &prompt_buffer);
    connect_page(&prompt_row, &navigation, &prompt_page);
    customize_group.add(&prompt_row);
    let tools_row = navigation_row(
        &gettext("Tools"),
        "",
        Some("applications-utilities-symbolic"),
    );
    update_tools_summary(&tools_row, &calculator_row, &web_row);
    connect_page(&tools_row, &navigation, &tools_page);
    customize_group.add(&tools_row);
    let advanced_row = navigation_row(
        &gettext("Advanced"),
        &gettext("Response style and conversation history"),
        Some("preferences-system-symbolic"),
    );
    connect_page(&advanced_row, &navigation, &advanced_page);
    customize_group.add(&advanced_row);
    if active_provider(backend).is_some_and(|p| p.is_remote()) {
        let privacy_content = adw::PreferencesPage::new();
        privacy_content.add(&super::privacy::conversation_group(ui, backend));
        let (privacy_page, _) = settings_page(&gettext("Sharing Permissions"), &privacy_content);
        let privacy_row = navigation_row(
            &gettext("Sharing Permissions"),
            &gettext("Manage access for this chat’s provider"),
            Some("changes-prevent-symbolic"),
        );
        connect_page(&privacy_row, &navigation, &privacy_page);
        customize_group.add(&privacy_row);
    }
    page.add(&customize_group);

    let target_dialog = dialog.downgrade();
    let target_backend = Rc::clone(backend);
    let target_profile_list = profile_list.clone();
    let target_profile_values = Rc::clone(&profile_values);
    let target_profile_row = profile_row.clone();
    let target_temperature_row = temperature_row.clone();
    let target_prompt_buffer = prompt_buffer.clone();
    create_profile_action.connect_activate(move |_, _| {
        let Some(dialog) = target_dialog.upgrade() else {
            return;
        };
        show_create_profile_dialog(
            &dialog,
            &target_backend,
            &target_profile_list,
            &target_profile_values,
            &target_profile_row,
            target_temperature_row.value(),
            buffer_text(&target_prompt_buffer),
        );
    });

    let target_prompt_count = prompt_count.downgrade();
    let target_prompt_row = prompt_row.downgrade();
    prompt_buffer.connect_changed(move |buffer| {
        if let Some(label) = target_prompt_count.upgrade() {
            update_prompt_count(&label, buffer);
        }
        if let Some(row) = target_prompt_row.upgrade() {
            update_prompt_summary(&row, buffer);
        }
    });
    for row in [&calculator_row, &web_row] {
        let target_tools_row = tools_row.downgrade();
        let target_calculator_row = calculator_row.downgrade();
        let target_web_row = web_row.downgrade();
        row.connect_active_notify(move |_| {
            if let (Some(row), Some(calculator), Some(web)) = (
                target_tools_row.upgrade(),
                target_calculator_row.upgrade(),
                target_web_row.upgrade(),
            ) {
                update_tools_summary(&row, &calculator, &web);
            }
        });
    }

    let target_dialog = dialog.downgrade();
    let target_toast_overlay = toast_overlay.downgrade();
    let target_backend = Rc::clone(backend);
    let target_profile_list = profile_list.clone();
    let target_profile_values = Rc::clone(&profile_values);
    let target_profile_row = profile_row.clone();
    let target_ui = Rc::clone(ui);
    let target_conversation_id = conversation_id.to_string();
    delete_profile_action.connect_activate(move |_, _| {
        let (Some(dialog), Some(overlay)) =
            (target_dialog.upgrade(), target_toast_overlay.upgrade())
        else {
            return;
        };
        confirm_delete_profile(
            &dialog,
            &overlay,
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
    let target_calculator_row = calculator_row.clone();
    let target_web_row = web_row.clone();
    let target_toast_overlay = toast_overlay.downgrade();
    reset_action.connect_activate(move |_, _| {
        target_profile_row.set_selected(0);
        target_model_row.set_selected(0);
        target_temperature_row.set_value(DEFAULT_TEMPERATURE);
        target_context_messages_row.set_value(DEFAULT_CONTEXT_MESSAGE_LIMIT as f64);
        target_prompt_buffer.set_text("");
        target_top_p_row.set_text("");
        target_top_k_row.set_text("");
        target_seed_row.set_text("");
        target_num_ctx_row.set_text(
            &default_context_window
                .map(|value| value.to_string())
                .unwrap_or_default(),
        );
        target_calculator_row.set_active(false);
        target_web_row.set_active(false);
        if let Some(overlay) = target_toast_overlay.upgrade() {
            overlay.add_toast(adw::Toast::new(&gettext(
                "Defaults restored. Save to apply changes.",
            )));
        }
    });

    let target_dialog = dialog.downgrade();
    let target_toast_overlay = toast_overlay.downgrade();
    let target_navigation = navigation.downgrade();
    let target_ui = Rc::clone(ui);
    let target_backend = Rc::clone(backend);
    let target_conversation_id = conversation_id.to_string();
    let controls = SettingsControls {
        calculator_row,
        web_row,
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
        let (Some(dialog), Some(overlay)) =
            (target_dialog.upgrade(), target_toast_overlay.upgrade())
        else {
            return;
        };
        if super::provider_change_is_blocked(&target_ui, &target_backend) {
            return;
        }
        let values = match values_from_controls(&controls) {
            Ok(values) => values,
            Err((row, message)) => {
                if let Some(navigation) = target_navigation.upgrade()
                    && navigation.visible_page().as_ref() != Some(&advanced_page)
                {
                    navigation.replace(&[main_page.clone(), advanced_page.clone()]);
                }
                row.add_css_class("error");
                row.grab_focus();
                overlay.add_toast(adw::Toast::new(&message));
                return;
            }
        };

        match save_chat_settings(&target_backend, &target_conversation_id, values.clone()) {
            Ok(()) => {
                super::composer_commands::refresh(&target_ui, &target_backend);
                if let Some(model) = values.preferred_model.as_deref() {
                    select_model_by_name(&target_ui, model);
                } else {
                    restore_selected_provider_model(&target_ui, &target_backend);
                }
                super::reasoning::refresh(&target_ui, &target_backend);
                if let Err(error) = update_profile_indicator(
                    &target_ui,
                    &target_backend,
                    values.profile_id.as_deref(),
                ) {
                    target_ui
                        .toast_overlay
                        .add_toast(adw::Toast::new(&gettext_f(
                            "Profile indicator could not be updated: {error}",
                            &[("error", &(error).to_string())],
                        )));
                }
                target_ui
                    .toast_overlay
                    .add_toast(adw::Toast::new(&gettext("Chat settings saved")));
                dialog.close();
            }
            Err(error) => {
                overlay.add_toast(adw::Toast::new(&gettext_f(
                    "Chat settings could not be saved: {error}",
                    &[("error", &(error).to_string())],
                )));
            }
        }
    });

    Ok(dialog)
}

fn settings_page(
    title: &str,
    content: &adw::PreferencesPage,
) -> (adw::NavigationPage, adw::HeaderBar) {
    let header = adw::HeaderBar::new();
    let toolbar = adw::ToolbarView::builder().content(content).build();
    toolbar.add_top_bar(&header);
    (adw::NavigationPage::new(&toolbar, title), header)
}

fn navigation_row(title: &str, subtitle: &str, icon: Option<&str>) -> adw::ActionRow {
    let row = adw::ActionRow::builder()
        .title(title)
        .subtitle(subtitle)
        .subtitle_lines(1)
        .use_markup(false)
        .activatable(true)
        .build();
    if let Some(icon) = icon {
        row.add_prefix(&gtk::Image::from_icon_name(icon));
    }
    row.add_suffix(&gtk::Image::from_icon_name("go-next-symbolic"));
    row
}

fn connect_page(
    row: &adw::ActionRow,
    navigation: &adw::NavigationView,
    page: &adw::NavigationPage,
) {
    let navigation = navigation.downgrade();
    let page = page.clone();
    row.connect_activated(move |_| {
        if let Some(navigation) = navigation.upgrade() {
            navigation.push(&page);
        }
    });
}

fn settings_guide() -> adw::NavigationPage {
    let content = adw::PreferencesPage::new();
    let conversation_group = adw::PreferencesGroup::builder()
        .title(gettext("Conversation"))
        .description(gettext(
            "Start with the defaults, then adjust one setting at a time.",
        ))
        .build();
    for (title, description) in [
        (
            gettext_noop("Profile"),
            gettext_noop(
                "Applies a temperature and system prompt as a starting point. You can still edit every setting.",
            ),
        ),
        (
            gettext_noop("Preferred Model"),
            gettext_noop(
                "Keeps this conversation on one model. Use Selected Model follows the model picker in the chat.",
            ),
        ),
        (
            gettext_noop("System Prompt"),
            gettext_noop(
                "Gives the model lasting instructions for this conversation, such as its role, tone or response style.",
            ),
        ),
        (
            gettext_noop("Temperature"),
            gettext_noop(
                "Controls response variety. Lower values are more focused and predictable; higher values are more varied. Some providers or reasoning modes use their own defaults.",
            ),
        ),
        (
            gettext_noop("Context Messages"),
            gettext_noop(
                "Sets the maximum number of recent user and assistant messages sent to the model, including your next message. Questions and answers stay together. With Ollama, Moose also estimates their size and leaves out older exchanges when needed to make room for reasoning and the response. All messages stay saved in the chat.",
            ),
        ),
    ] {
        conversation_group.add(&parameter_explanation(
            &gettext(title),
            &gettext(description),
        ));
    }
    content.add(&conversation_group);

    let ollama_group = adw::PreferencesGroup::builder()
        .title(gettext("Ollama Options"))
        .description(gettext("Available with Ollama. Context Window defaults to 16,384 tokens; other empty fields use the model defaults."))
        .build();
    for (title, description) in [
        (
            gettext_noop("Top P"),
            gettext_noop(
                "Limits token choices by their combined probability. Lower values reduce variety by keeping a smaller set of likely choices.",
            ),
        ),
        (
            gettext_noop("Top K"),
            gettext_noop(
                "Limits each token choice to the most likely options. Lower values reduce variety.",
            ),
        ),
        (
            gettext_noop("Seed"),
            gettext_noop(
                "Uses a fixed random seed to make repeated requests more reproducible. Identical results are not guaranteed.",
            ),
        ),
        (
            gettext_noop("Context Window"),
            gettext_noop(
                "Sets Ollama’s context size in tokens, shared by the conversation, reasoning, and response. Moose uses 16,384 when this field is empty. Up to half the window, capped at 8,192 tokens, is reserved for reasoning and the response, with an additional safety margin. Text and image sizes are estimates, so very long responses can still reach the limit. Larger windows use more memory; Moose does not increase this setting automatically.",
            ),
        ),
    ] {
        ollama_group.add(&parameter_explanation(
            &gettext(title),
            &gettext(description),
        ));
    }
    content.add(&ollama_group);
    settings_page(&gettext("Settings Guide"), &content).0
}

fn parameter_explanation(title: &str, description: &str) -> adw::ActionRow {
    adw::ActionRow::builder()
        .title(title)
        .subtitle(description)
        .subtitle_lines(0)
        .use_markup(false)
        .build()
}

fn update_prompt_count(label: &gtk::Label, buffer: &gtk::TextBuffer) {
    let count = buffer.char_count();
    label.set_label(&ngettext_f(
        "{count} character",
        "{count} characters",
        count as u32,
        &[("count", &count.to_string())],
    ));
}

fn update_prompt_summary(row: &adw::ActionRow, buffer: &gtk::TextBuffer) {
    let text = buffer_text(buffer);
    let summary = text.lines().find(|line| !line.trim().is_empty());
    row.set_subtitle(
        &summary
            .map(|line| line.trim().chars().take(80).collect::<String>())
            .unwrap_or_else(|| gettext("Add instructions for this chat")),
    );
}

fn update_tools_summary(row: &adw::ActionRow, calculator: &adw::SwitchRow, web: &adw::SwitchRow) {
    row.set_subtitle(&gettext(match (calculator.is_active(), web.is_active()) {
        (true, true) => gettext_noop("Calculator · Web"),
        (true, false) => gettext_noop("Calculator"),
        (false, true) => gettext_noop("Web"),
        (false, false) => gettext_noop("None enabled"),
    }));
}

fn show_create_profile_dialog(
    parent: &adw::Dialog,
    backend: &Rc<Backend>,
    profile_list: &gtk::StringList,
    profile_values: &Rc<RefCell<Vec<Option<ChatProfile>>>>,
    profile_row: &adw::ComboRow,
    temperature: f64,
    system_prompt: String,
) {
    let dialog = adw::Dialog::builder()
        .title(gettext("Save as Profile"))
        .content_width(520)
        .content_height(420)
        .build();
    let header_bar = adw::HeaderBar::new();
    let cancel_button = gtk::Button::with_label(&gettext("Cancel"));
    let create_button = gtk::Button::with_label(&gettext("Save"));
    create_button.add_css_class("suggested-action");
    create_button.set_sensitive(false);
    header_bar.pack_start(&cancel_button);
    header_bar.pack_end(&create_button);

    let group = adw::PreferencesGroup::builder()
        .title(gettext("Profile Details"))
        .description(gettext(
            "Reuse this chat’s instructions and temperature in other conversations.",
        ))
        .build();
    let name_row = adw::EntryRow::builder().title(gettext("Name")).build();
    let description_row = adw::EntryRow::builder()
        .title(gettext("Short Description"))
        .build();
    group.add(&name_row);
    group.add(&description_row);
    let page = adw::PreferencesPage::new();
    page.add(&group);
    let toolbar_view = adw::ToolbarView::builder().content(&page).build();
    toolbar_view.add_top_bar(&header_bar);
    let toast_overlay = adw::ToastOverlay::new();
    toast_overlay.set_child(Some(&toolbar_view));
    dialog.set_child(Some(&toast_overlay));
    dialog.set_default_widget(Some(&create_button));
    dialog.set_focus(Some(&name_row));

    let target_dialog = dialog.downgrade();
    cancel_button.connect_clicked(move |_| {
        if let Some(dialog) = target_dialog.upgrade() {
            dialog.close();
        }
    });

    let target_create_button = create_button.downgrade();
    let target_description_row = description_row.downgrade();
    name_row.connect_text_notify(move |row| {
        if let (Some(button), Some(description)) = (
            target_create_button.upgrade(),
            target_description_row.upgrade(),
        ) {
            button.set_sensitive(
                !row.text().trim().is_empty() && !description.text().trim().is_empty(),
            );
        }
    });

    let target_create_button = create_button.downgrade();
    let target_name_row = name_row.downgrade();
    description_row.connect_text_notify(move |row| {
        if let (Some(button), Some(name)) =
            (target_create_button.upgrade(), target_name_row.upgrade())
        {
            button.set_sensitive(!name.text().trim().is_empty() && !row.text().trim().is_empty());
        }
    });

    let target_dialog = dialog.downgrade();
    let target_toast_overlay = toast_overlay.downgrade();
    let target_backend = Rc::clone(backend);
    let target_profile_list = profile_list.clone();
    let target_profile_values = Rc::clone(profile_values);
    let target_profile_row = profile_row.clone();
    create_button.connect_clicked(move |_| {
        let (Some(dialog), Some(overlay)) =
            (target_dialog.upgrade(), target_toast_overlay.upgrade())
        else {
            return;
        };
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
                dialog.close();
            }
            Err(error) => overlay.add_toast(adw::Toast::new(&gettext_f(
                "Profile could not be created: {error}",
                &[("error", &(error).to_string())],
            ))),
        }
    });

    dialog.present(Some(parent));
}

fn confirm_delete_profile(
    parent: &adw::Dialog,
    toast_overlay: &adw::ToastOverlay,
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
        .heading(gettext("Delete Profile?"))
        .body(gettext_f(
            "Delete \"{arg1}\"? Existing chats will keep their current settings.",
            &[("arg1", &(profile.name).to_string())],
        ))
        .close_response("cancel")
        .default_response("cancel")
        .build();
    dialog.add_response("cancel", &gettext("Cancel"));
    dialog.add_response("delete", &gettext("Delete"));
    dialog.set_response_appearance("delete", adw::ResponseAppearance::Destructive);

    let target_toast_overlay = toast_overlay.downgrade();
    let target_ui = Rc::clone(ui);
    let target_backend = Rc::clone(backend);
    let target_profile_list = profile_list.clone();
    let target_profile_values = Rc::clone(profile_values);
    let target_profile_row = profile_row.clone();
    let target_conversation_id = conversation_id.to_string();
    dialog.connect_response(Some("delete"), move |_, _| {
        let Some(overlay) = target_toast_overlay.upgrade() else {
            return;
        };
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
                    overlay.add_toast(adw::Toast::new(&gettext_f(
                        "Profile indicator could not be updated: {error}",
                        &[("error", &(error).to_string())],
                    )));
                }
                overlay.add_toast(adw::Toast::new(&gettext("Profile deleted")));
            }
            Err(error) => overlay.add_toast(adw::Toast::new(&gettext_f(
                "Profile could not be deleted: {error}",
                &[("error", &(error).to_string())],
            ))),
        }
    });
    dialog.present(Some(parent));
}

fn profile_choices(
    profiles: Vec<ChatProfile>,
    selected_profile_id: Option<&str>,
) -> (gtk::StringList, Rc<RefCell<Vec<Option<ChatProfile>>>>, u32) {
    let mut labels = vec![gettext("No Profile")];
    let mut values = vec![None];
    for profile in profiles {
        labels.push(crate::i18n::profile_name(&profile));
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
        .map(crate::i18n::profile_description)
        .unwrap_or_else(|| gettext("Choose a profile to apply instructions and response settings"));
    profile_row.set_subtitle(&subtitle);
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
    let mut labels = vec![gettext("Use Selected Model")];
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
        labels.push(gettext_f("{model} (Unavailable)", &[("model", (model))]));
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
    row.connect_text_notify(|row| row.remove_css_class("error"));
    row
}

fn values_from_controls(
    controls: &SettingsControls,
) -> std::result::Result<ChatSettingsValues, (adw::EntryRow, String)> {
    let selected_profile = usize::try_from(controls.profile_row.selected()).unwrap_or(0);
    let profile_id = controls
        .profile_values
        .borrow()
        .get(selected_profile)
        .and_then(Option::as_ref)
        .map(|profile| profile.id.clone());
    let selected_model = usize::try_from(controls.model_row.selected()).unwrap_or(0);
    let preferred_model = controls.model_values.get(selected_model).cloned().flatten();
    let displayed_prompt = buffer_text(&controls.prompt_buffer);
    let system_prompt = controls
        .profile_values
        .borrow()
        .get(selected_profile)
        .and_then(Option::as_ref)
        .map(|profile| crate::i18n::profile_prompt_value(profile, &displayed_prompt))
        .unwrap_or(displayed_prompt);

    Ok(ChatSettingsValues {
        calculator: controls.calculator_row.is_active(),
        web: controls.web_row.is_active(),
        profile_id,
        preferred_model,
        temperature: controls.temperature_row.value(),
        top_p: parse_optional_probability(&controls.top_p_row, &gettext("Top P"))
            .map_err(|message| (controls.top_p_row.clone(), message))?,
        top_k: parse_optional_non_negative_i64(&controls.top_k_row, &gettext("Top K"))
            .map_err(|message| (controls.top_k_row.clone(), message))?,
        seed: parse_optional_i64(&controls.seed_row, &gettext("Seed"))
            .map_err(|message| (controls.seed_row.clone(), message))?,
        num_ctx: parse_optional_positive_i64(&controls.num_ctx_row, &gettext("Context Window"))
            .map_err(|message| (controls.num_ctx_row.clone(), message))?,
        context_messages: controls.context_messages_row.value().round() as i64,
        system_prompt,
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
        Err(gettext_f(
            "{field} must be between 0 and 1",
            &[("field", (field))],
        ))
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
        .map_err(|_| gettext_f("{field} must be a number", &[("field", (field))]))?;
    if value.is_finite() && value >= 0.0 {
        Ok(Some(value))
    } else {
        Err(gettext_f(
            "{field} must be 0 or greater",
            &[("field", (field))],
        ))
    }
}

fn parse_optional_positive_i64(
    row: &adw::EntryRow,
    field: &str,
) -> std::result::Result<Option<i64>, String> {
    let value = parse_optional_i64(row, field)?;
    if value.is_some_and(|value| value <= 0) {
        Err(gettext_f(
            "{field} must be greater than 0",
            &[("field", (field))],
        ))
    } else {
        Ok(value)
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
        Err(gettext_f(
            "{field} must be 0 or greater",
            &[("field", (field))],
        ))
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
        .map_err(|_| gettext_f("{field} must be a whole number", &[("field", (field))]))
}
