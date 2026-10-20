use std::{cell::RefCell, rc::Rc};

use adw::prelude::*;

use crate::{
    error::Result,
    providers::{ApiProtocol, CustomAuthentication, CustomModel, CustomProviderConfig},
};

use super::widgets;

struct ModelRow {
    entry: adw::EntryRow,
    images: gtk::CheckButton,
}

type ModelRows = Rc<RefCell<Vec<ModelRow>>>;

#[derive(Clone)]
pub(super) struct CustomProviderFields {
    pub connection_group: adw::PreferencesGroup,
    pub models_group: adw::PreferencesGroup,
    pub authentication: adw::ComboRow,
    protocol: adw::ComboRow,
    header: adw::EntryRow,
    discover_models: adw::SwitchRow,
    models: ModelRows,
}

impl CustomProviderFields {
    pub fn new(config: &CustomProviderConfig) -> Self {
        let connection_group = adw::PreferencesGroup::builder()
            .title("API Compatibility")
            .description("Choose the API format supported by your provider.")
            .build();
        let protocol = adw::ComboRow::builder()
            .title("API Format")
            .model(&gtk::StringList::new(
                &ApiProtocol::ALL.map(ApiProtocol::label),
            ))
            .selected(
                ApiProtocol::ALL
                    .iter()
                    .position(|value| *value == config.protocol)
                    .unwrap_or(0) as u32,
            )
            .build();
        let authentication = adw::ComboRow::builder()
            .title("Authentication")
            .model(&gtk::StringList::new(&[
                "Bearer token",
                "API key header",
                "None",
            ]))
            .selected(match &config.authentication {
                CustomAuthentication::Bearer => 0,
                CustomAuthentication::Header(_) => 1,
                CustomAuthentication::None => 2,
            })
            .build();
        let header = adw::EntryRow::builder()
            .title("API Key Header")
            .text(match &config.authentication {
                CustomAuthentication::Header(name) => name.as_str(),
                _ => "x-api-key",
            })
            .visible(authentication.selected() == 1)
            .build();
        connection_group.add(&protocol);
        connection_group.add(&authentication);
        connection_group.add(&header);

        let models_group = adw::PreferencesGroup::builder()
            .title("Models")
            .description("Add exact model IDs from your provider. Enable Images only for models that accept image input. Saved IDs remain available if discovery is unavailable.")
            .build();
        let discover_models = adw::SwitchRow::builder()
            .title("Discover Models Automatically")
            .subtitle("Turn off if your provider does not offer a model catalog")
            .active(config.discover_models)
            .build();
        models_group.add(&discover_models);
        let add_button = gtk::Button::with_label("Add Model");
        add_button.set_valign(gtk::Align::Center);
        models_group.set_header_suffix(Some(&add_button));
        let models = Rc::new(RefCell::new(Vec::new()));
        for model in &config.models {
            add_model(&models_group, &models, model);
        }
        let target_group = models_group.downgrade();
        let target_models = Rc::clone(&models);
        add_button.connect_clicked(move |_| {
            if let Some(group) = target_group.upgrade() {
                add_model(
                    &group,
                    &target_models,
                    &CustomModel {
                        id: String::new(),
                        vision: false,
                    },
                );
            }
        });
        let target_header = header.clone();
        authentication.connect_selected_notify(move |row| {
            target_header.set_visible(row.selected() == 1);
        });
        let target_authentication = authentication.clone();
        let target_header = header.clone();
        protocol.connect_selected_notify(move |row| {
            match ApiProtocol::ALL[row.selected() as usize].authentication() {
                CustomAuthentication::Bearer => target_authentication.set_selected(0),
                CustomAuthentication::Header(name) => {
                    target_header.set_text(&name);
                    target_authentication.set_selected(1);
                }
                CustomAuthentication::None => target_authentication.set_selected(2),
            }
        });
        Self {
            connection_group,
            models_group,
            protocol,
            authentication,
            header,
            discover_models,
            models,
        }
    }

    pub fn set_visible(&self, visible: bool) {
        self.connection_group.set_visible(visible);
        self.models_group.set_visible(visible);
    }

    pub fn clear_key_on_destination_change(&self, key: &gtk::PasswordEntry) {
        let target_key = key.downgrade();
        self.protocol.connect_selected_notify(move |_| {
            if let Some(key) = target_key.upgrade() {
                key.set_text("");
            }
        });
        let target_key = key.downgrade();
        self.header.connect_text_notify(move |_| {
            if let Some(key) = target_key.upgrade() {
                key.set_text("");
            }
        });
    }

    pub fn config(&self) -> Result<CustomProviderConfig> {
        let protocol = ApiProtocol::ALL[self.protocol.selected() as usize];
        let config = CustomProviderConfig {
            protocol,
            authentication: match self.authentication.selected() {
                0 => CustomAuthentication::Bearer,
                1 => CustomAuthentication::Header(self.header.text().trim().to_ascii_lowercase()),
                _ => CustomAuthentication::None,
            },
            discover_models: self.discover_models.is_active(),
            models: self
                .models
                .borrow()
                .iter()
                .map(|row| {
                    let id = row.entry.text();
                    let id = id.as_str();
                    CustomModel {
                        id: if protocol == ApiProtocol::Gemini {
                            id.strip_prefix("models/").unwrap_or(id).to_string()
                        } else {
                            id.to_string()
                        },
                        vision: row.images.is_active(),
                    }
                })
                .collect(),
        };
        config.validate()?;
        Ok(config)
    }
}

fn add_model(group: &adw::PreferencesGroup, models: &ModelRows, model: &CustomModel) {
    if models.borrow().len() >= 1000 {
        return;
    }
    let entry = adw::EntryRow::builder()
        .title("Model ID")
        .text(&model.id)
        .build();
    let images = gtk::CheckButton::builder()
        .label("Images")
        .active(model.vision)
        .valign(gtk::Align::Center)
        .tooltip_text("This model supports image input")
        .build();
    let remove = widgets::icon_button("user-trash-symbolic", "Remove Model");
    remove.set_valign(gtk::Align::Center);
    entry.add_suffix(&images);
    entry.add_suffix(&remove);
    group.add(&entry);
    models.borrow_mut().push(ModelRow {
        entry: entry.clone(),
        images,
    });
    let target_group = group.downgrade();
    let target_entry = entry.downgrade();
    let target_models = Rc::downgrade(models);
    remove.connect_clicked(move |_| {
        if let (Some(group), Some(entry), Some(models)) = (
            target_group.upgrade(),
            target_entry.upgrade(),
            target_models.upgrade(),
        ) {
            group.remove(&entry);
            models.borrow_mut().retain(|row| row.entry != entry);
        }
    });
    if model.id.is_empty() {
        entry.grab_focus();
    }
}
