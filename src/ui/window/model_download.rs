use std::rc::Rc;

use adw::prelude::*;

use crate::{
    models::{ModelDownload, model_names_match},
    ollama::OllamaModel,
};

use super::{Backend, WindowUi, active_provider, model_actions};

struct DownloadFields {
    model: adw::EntryRow,
    variant: adw::EntryRow,
    options: adw::PreferencesGroup,
    variant_expander: adw::ExpanderRow,
    preview: gtk::Label,
    error: gtk::Label,
    download: gtk::glib::WeakRef<gtk::Button>,
    installed: Vec<OllamaModel>,
}

impl DownloadFields {
    fn reference(&self) -> Result<ModelDownload, &'static str> {
        ModelDownload::parse(&self.model.text())?.with_variant(&self.variant.text())
    }

    fn refresh(&self) {
        let parsed = ModelDownload::parse(&self.model.text());
        self.options.set_visible(
            parsed
                .as_ref()
                .is_ok_and(|model| model.hugging_face_repository.is_some()),
        );
        self.model.remove_css_class("error");
        self.variant.remove_css_class("error");
        let Some(download) = self.download.upgrade() else {
            return;
        };
        download.set_label("Download");

        match self.reference() {
            Ok(reference) => {
                let installed = self
                    .installed
                    .iter()
                    .find(|model| model_names_match(&model.name, &reference.name));
                let source = if reference.hugging_face_repository.is_some() {
                    "Hugging Face"
                } else {
                    "Ollama"
                };
                self.preview.set_label(&format!(
                    "{source}{}\n{}",
                    if installed.is_some() {
                        " · Already installed"
                    } else {
                        ""
                    },
                    reference.name
                ));
                self.preview.set_visible(true);
                self.variant_expander
                    .set_subtitle(reference.variant().unwrap_or("Automatic"));
                self.error.set_visible(false);
                download.set_sensitive(installed.is_none_or(|model| model.supports_chat));
                if installed.is_some_and(|model| model.supports_chat) {
                    download.set_label("New Chat");
                } else if installed.is_some() {
                    self.show_error("This model is already installed and does not support chat.");
                }
            }
            Err(error) => {
                download.set_sensitive(false);
                self.preview.set_visible(false);
                if !self.model.text().trim().is_empty() {
                    if parsed.is_ok() {
                        self.variant.add_css_class("error");
                    } else {
                        self.model.add_css_class("error");
                    }
                    self.show_error(error);
                } else {
                    self.error.set_visible(false);
                }
            }
        }
    }

    fn show_error(&self, message: &str) {
        self.error.set_label(message);
        self.error.set_visible(true);
    }
}

pub(super) fn show_dialog(ui: &Rc<WindowUi>, backend: &Rc<Backend>, initial: &str) {
    if !model_actions::can_manage_models(ui, backend) {
        return;
    }
    if backend.active_model_pull.borrow().is_some() {
        ui.toast_overlay
            .add_toast(adw::Toast::new("A model download is already running"));
        return;
    }
    let Some(provider) = active_provider(backend) else {
        return;
    };

    let dialog = adw::Dialog::builder()
        .title("Download Model")
        .content_width(520)
        .follows_content_size(true)
        .build();
    let header = adw::HeaderBar::new();
    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(16)
        .margin_start(24)
        .margin_end(24)
        .margin_top(8)
        .margin_bottom(24)
        .build();

    let model = adw::EntryRow::builder()
        .title("Model Name or Link")
        .text(initial)
        .activates_default(true)
        .build();
    let model_group = adw::PreferencesGroup::builder()
        .description("Use an Ollama name like llama3.2:1b, a Hugging Face GGUF link, or an ollama run command.")
        .build();
    model_group.add(&model);
    content.append(&model_group);

    let variant = adw::EntryRow::builder()
        .title("Quantization or Filename")
        .tooltip_text("Overrides the variant in the model name or link")
        .activates_default(true)
        .build();
    let variant_expander = adw::ExpanderRow::builder()
        .title("Variant")
        .subtitle("Automatic")
        .build();
    variant_expander.add_row(&variant);
    let options = adw::PreferencesGroup::builder().visible(false).build();
    options.add(&variant_expander);
    let variant_hint = gtk::Label::builder()
        .label("Optional. Enter a quantization such as Q4_K_M or an exact GGUF filename.")
        .wrap(true)
        .width_chars(1)
        .xalign(0.0)
        .margin_start(12)
        .margin_end(12)
        .margin_top(8)
        .margin_bottom(12)
        .css_classes(["caption", "dim-label"])
        .build();
    variant_expander.add_row(&variant_hint);
    content.append(&options);

    let preview = gtk::Label::builder()
        .selectable(true)
        .wrap(true)
        .wrap_mode(gtk::pango::WrapMode::WordChar)
        .width_chars(1)
        .xalign(0.0)
        .visible(false)
        .css_classes(["caption", "dim-label"])
        .build();
    let error = gtk::Label::builder()
        .wrap(true)
        .width_chars(1)
        .xalign(0.0)
        .visible(false)
        .css_classes(["caption", "error"])
        .build();
    content.append(&preview);
    content.append(&error);

    let browse = gtk::LinkButton::with_label(
        "https://huggingface.co/models?library=gguf",
        "Browse Hugging Face Models",
    );
    browse.set_halign(gtk::Align::Start);
    content.append(&browse);

    let destination = gtk::Label::builder()
        .label(if provider.is_managed {
            format!("Downloads to {} on this device.", provider.name)
        } else {
            format!("Downloads to {} at {}.", provider.name, provider.base_url)
        })
        .wrap(true)
        .wrap_mode(gtk::pango::WrapMode::WordChar)
        .width_chars(1)
        .xalign(0.0)
        .css_classes(["caption", "dim-label"])
        .build();
    content.append(&destination);

    let cancel = gtk::Button::with_label("Cancel");
    let download = gtk::Button::with_label("Download");
    download.add_css_class("suggested-action");
    let actions = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .halign(gtk::Align::End)
        .margin_top(4)
        .build();
    actions.append(&cancel);
    actions.append(&download);
    content.append(&actions);

    let scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .propagate_natural_height(true)
        .child(&content)
        .build();
    let toolbar = adw::ToolbarView::builder().content(&scroll).build();
    toolbar.add_top_bar(&header);
    dialog.set_child(Some(&toolbar));
    dialog.set_default_widget(Some(&download));
    dialog.set_focus(Some(&model));

    let fields = Rc::new(DownloadFields {
        model,
        variant,
        options,
        variant_expander,
        preview,
        error,
        download: download.downgrade(),
        installed: ui.installed_models.borrow().clone(),
    });
    let weak_fields = Rc::downgrade(&fields);
    fields.model.connect_text_notify(move |_| {
        if let Some(fields) = weak_fields.upgrade() {
            fields.refresh();
        }
    });
    let weak_fields = Rc::downgrade(&fields);
    fields.variant.connect_text_notify(move |_| {
        if let Some(fields) = weak_fields.upgrade() {
            fields.refresh();
        }
    });
    fields.refresh();

    let weak_dialog = dialog.downgrade();
    cancel.connect_clicked(move |_| {
        if let Some(dialog) = weak_dialog.upgrade() {
            dialog.close();
        }
    });
    let weak_dialog = dialog.downgrade();
    let weak_ui = Rc::downgrade(ui);
    let weak_backend = Rc::downgrade(backend);
    download.connect_clicked(move |_| {
        let (Some(dialog), Some(ui), Some(backend)) = (
            weak_dialog.upgrade(),
            weak_ui.upgrade(),
            weak_backend.upgrade(),
        ) else {
            return;
        };
        let reference = match fields.reference() {
            Ok(reference) => reference,
            Err(error) => {
                fields.show_error(error);
                return;
            }
        };
        if active_provider(&backend).is_none_or(|active| active.id != provider.id) {
            fields.show_error(
                "The connection changed. Reopen this dialog to choose the destination.",
            );
            return;
        }
        if !model_actions::can_manage_models(&ui, &backend) {
            return;
        }
        if backend.active_model_pull.borrow().is_some()
            || backend.active_model_delete.borrow().is_some()
        {
            fields.show_error("Wait for the current model operation to finish.");
            return;
        }
        let installed = ui
            .installed_models
            .borrow()
            .iter()
            .find(|model| model.supports_chat && model_names_match(&model.name, &reference.name))
            .map(|model| model.name.clone());
        dialog.close();
        if let Some(model) = installed {
            super::new_conversation_with_model(&ui, &backend, Some(&model));
        } else {
            model_actions::start_model_pull(&ui, &backend, reference.name);
        }
    });

    dialog.present(Some(&ui.window));
}
