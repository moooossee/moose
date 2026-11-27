use super::*;
use crate::attachments::ocr::languages::{self, Download, Language};

pub(super) struct Manager {
    dialog: glib::WeakRef<adw::PreferencesDialog>,
    page: adw::PreferencesPage,
    search: gtk::SearchEntry,
    installed: gtk::ToggleButton,
    categories: adw::ToggleGroup,
    groups: Vec<adw::PreferencesGroup>,
    empty: Vec<adw::StatusPage>,
    rows: RefCell<Vec<(usize, adw::ActionRow)>>,
    status: gtk::Label,
    progress: gtk::ProgressBar,
    cancel: gtk::Button,
    download: RefCell<Option<Download>>,
    ui: std::rc::Weak<WindowUi>,
    backend: Rc<Backend>,
}

pub(super) fn button() -> gtk::Button {
    let button = gtk::Button::with_label(&gettext("Preferences…"));
    button.set_tooltip_text(Some(&gettext("Open Text Recognition preferences")));
    button.add_css_class("flat");
    button.set_valign(gtk::Align::Center);
    button.set_action_name(Some("win.ocr-languages"));
    button
}

pub(super) fn bind(ui: &Rc<WindowUi>, backend: &Rc<Backend>) {
    let action = gio::SimpleAction::new("ocr-languages", None);
    let target_ui = Rc::downgrade(ui);
    let backend = backend.clone();
    action.connect_activate(move |_, _| {
        let Some(ui) = target_ui.upgrade() else {
            return;
        };
        let dialog = preferences_dialog(&ui)
            .unwrap_or_else(|| super::super::preferences::dialog(&ui.window, &ui, &backend));
        let manager = ui.attachments.language_manager.borrow().clone();
        if let Some(manager) = manager {
            manager.populate();
            dialog.set_visible_page(&manager.page);
        }
        dialog.present(Some(&ui.window));
    });
    ui.window.add_action(&action);
}

pub(in crate::ui::window) fn preferences_dialog(ui: &WindowUi) -> Option<adw::PreferencesDialog> {
    ui.attachments
        .language_manager
        .borrow()
        .as_ref()
        .and_then(|manager| manager.dialog.upgrade())
}

pub(in crate::ui::window) fn preferences_page(
    dialog: &adw::PreferencesDialog,
    ui: &Rc<WindowUi>,
    backend: &Rc<Backend>,
) -> adw::PreferencesPage {
    library::set_languages(ui, backend, &library::languages(ui));
    let page = adw::PreferencesPage::builder()
        .name("text-recognition")
        .title(gettext("Text Recognition"))
        .icon_name("text-x-generic-symbolic")
        .build();
    let overview = adw::PreferencesGroup::builder()
        .title(gettext("Local Text Recognition"))
        .description(gettext("Extract text from images and scanned PDFs on this device. Install only the models you need; downloaded models work offline."))
        .build();
    let selection = adw::ActionRow::builder()
        .title(gettext("Selected Languages and Writing Systems"))
        .title_lines(2)
        .subtitle_lines(3)
        .use_markup(false)
        .build();
    library::bind_language_summary(ui, &selection);
    overview.add(&selection);
    page.add(&overview);
    let options = gtk::Box::new(gtk::Orientation::Vertical, 12);
    let categories = adw::ToggleGroup::builder()
        .homogeneous(true)
        .can_shrink(true)
        .build();
    let search = gtk::SearchEntry::builder()
        .placeholder_text(gettext("Search…"))
        .hexpand(true)
        .build();
    let installed = gtk::ToggleButton::with_label(&gettext("Installed"));
    installed.set_tooltip_text(Some(&gettext("Show only installed models")));
    let filters = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    filters.append(&search);
    filters.append(&installed);
    let status = gtk::Label::builder().xalign(0.0).wrap(true).build();
    status.add_css_class("caption");
    status.add_css_class("dim-label");
    let progress = gtk::ProgressBar::builder().visible(false).build();
    let cancel_button = gtk::Button::with_label(&gettext("Cancel Download"));
    cancel_button.set_visible(false);
    cancel_button.set_halign(gtk::Align::End);
    options.append(&categories);
    options.append(&filters);
    options.append(&status);
    options.append(&progress);
    options.append(&cancel_button);
    let controls = adw::PreferencesGroup::builder()
        .title(gettext("Recognition Models"))
        .build();
    controls.add(&options);
    page.add(&controls);
    let mut groups = Vec::new();
    let mut empty = Vec::new();
    for (name, tab, title, icon, description) in [
        (
            "languages",
            gettext_noop("Languages"),
            gettext_noop("Document Languages"),
            "preferences-desktop-locale-symbolic",
            gettext_noop(
                "Choose the languages used in your documents. Select installed languages to use them for recognition.",
            ),
        ),
        (
            "scripts",
            gettext_noop("Scripts"),
            gettext_noop("Writing Systems"),
            "font-x-generic-symbolic",
            gettext_noop(
                "Recognize text across languages that share a writing system, such as Latin, Arabic or Cyrillic. Select an installed writing system to use it.",
            ),
        ),
        (
            "tools",
            gettext_noop("Tools"),
            gettext_noop("Document Helpers"),
            "applications-utilities-symbolic",
            gettext_noop(
                "Optional tools help read scanned documents. They are used automatically when installed and do not need to be selected as a language.",
            ),
        ),
    ] {
        categories.add(
            adw::Toggle::builder()
                .name(name)
                .label(gettext(tab))
                .tooltip(gettext(title))
                .build(),
        );
        let group = adw::PreferencesGroup::builder()
            .title(gettext(title))
            .description(gettext(description))
            .build();
        let placeholder = adw::StatusPage::builder()
            .icon_name(icon)
            .title(gettext("No Matching Models"))
            .build();
        placeholder.add_css_class("compact");
        group.add(&placeholder);
        page.add(&group);
        groups.push(group);
        empty.push(placeholder);
    }
    categories.set_active_name(Some("languages"));
    let manager = Rc::new(Manager {
        dialog: dialog.downgrade(),
        page: page.clone(),
        search,
        installed,
        categories,
        groups,
        empty,
        rows: RefCell::new(Vec::new()),
        status,
        progress,
        cancel: cancel_button,
        download: RefCell::new(None),
        ui: Rc::downgrade(ui),
        backend: backend.clone(),
    });
    let target = Rc::downgrade(&manager);
    manager.search.connect_search_changed(move |_| {
        if let Some(manager) = target.upgrade() {
            manager.populate();
        }
    });
    let target = Rc::downgrade(&manager);
    manager.installed.connect_toggled(move |_| {
        if let Some(manager) = target.upgrade() {
            manager.populate();
        }
    });
    let target = Rc::downgrade(&manager);
    manager.categories.connect_active_name_notify(move |_| {
        if let Some(manager) = target.upgrade() {
            manager.update_summary();
        }
    });
    let target = Rc::downgrade(&manager);
    manager.cancel.connect_clicked(move |button| {
        if let Some(manager) = target.upgrade()
            && let Some(download) = manager.download.borrow().as_ref()
        {
            download.cancel();
            button.set_sensitive(false);
        }
    });
    let target = Rc::downgrade(&manager);
    dialog.connect_closed(move |_| {
        if let Some(manager) = target.upgrade() {
            if let Some(download) = manager.download.borrow().as_ref() {
                download.cancel();
            }
            if let Some(ui) = manager.ui.upgrade() {
                let mut active = ui.attachments.language_manager.borrow_mut();
                if active
                    .as_ref()
                    .is_some_and(|active| Rc::ptr_eq(active, &manager))
                {
                    active.take();
                }
            }
        }
    });
    ui.attachments
        .language_manager
        .replace(Some(manager.clone()));
    manager.populate();
    page
}

pub(super) fn cancel(ui: &WindowUi) {
    if let Some(manager) = ui.attachments.language_manager.borrow().as_ref()
        && let Some(download) = manager.download.borrow().as_ref()
    {
        download.cancel();
    }
}

fn category(language: &Language) -> usize {
    if !language.selectable() {
        2
    } else if language.code.starts_with("script/") {
        1
    } else {
        0
    }
}

impl Manager {
    fn populate(self: &Rc<Self>) {
        let Some(ui) = self.ui.upgrade() else { return };
        for (category, row) in self.rows.take() {
            self.groups[category].remove(&row);
        }
        let directory = self.backend.paths.ocr_models_dir();
        let query = self.search.text().trim().to_lowercase();
        let selected = library::languages(&ui);
        let busy = self.download.borrow().is_some();
        let mut counts = [0; 3];
        for language in &languages::catalog().models {
            let installed_size = language.installed_size(&directory);
            let installed = installed_size.is_some();
            if (self.installed.is_active() && !installed)
                || (!language.name.to_lowercase().contains(&query)
                    && !language.code.to_lowercase().contains(&query))
            {
                continue;
            }
            let category = category(language);
            counts[category] += 1;
            let size = model_actions::format_download_size(
                installed_size.unwrap_or_else(|| language.download_size()),
            );
            let row = adw::ActionRow::builder()
                .title(gettext(&language.name))
                .subtitle(format!(
                    "{} · {}",
                    size,
                    gettext(if installed {
                        gettext_noop("Installed")
                    } else {
                        gettext_noop("Not installed")
                    })
                ))
                .use_markup(false)
                .subtitle_lines(2)
                .build();
            if language.selectable() {
                let selected_button = gtk::CheckButton::new();
                selected_button
                    .set_active(installed && selected.split('+').any(|code| code == language.code));
                selected_button.set_sensitive(installed && !busy);
                selected_button.set_tooltip_text(Some(&gettext(if category == 1 {
                    gettext_noop("Use this writing system for text recognition")
                } else {
                    gettext_noop("Use this language for text recognition")
                })));
                row.add_prefix(&selected_button);
                let target = Rc::downgrade(self);
                selected_button.connect_toggled(move |button| {
                    if let Some(manager) = target.upgrade()
                        && let Some(ui) = manager.ui.upgrade()
                    {
                        let current = library::languages(&ui);
                        let mut codes = current
                            .split('+')
                            .filter(|code| !code.is_empty() && *code != language.code)
                            .map(str::to_string)
                            .collect::<Vec<_>>();
                        if button.is_active() {
                            codes.push(language.code.clone());
                        }
                        library::set_languages(&ui, &manager.backend, &codes.join("+"));
                    }
                });
                row.set_activatable_widget(Some(&selected_button));
            } else {
                row.set_title(&gettext("Orientation and Script Detection"));
                row.set_subtitle(&gettext_f(
                    "Detects rotated pages and writing systems. Used automatically when installed. · {size}",
                    &[("size", &size)],
                ));
                row.set_subtitle_lines(0);
                let icon = gtk::Image::from_icon_name("object-rotate-right-symbolic");
                icon.set_pixel_size(24);
                icon.add_css_class("dim-label");
                row.add_prefix(&icon);
                let state = gtk::Label::new(Some(&gettext(if installed {
                    gettext_noop("Enabled")
                } else {
                    gettext_noop("Optional")
                })));
                state.add_css_class("caption");
                state.add_css_class(if installed { "success" } else { "dim-label" });
                row.add_suffix(&state);
            }
            if installed_size.is_some_and(|size| size != language.download_size()) {
                let upgrade = gtk::Button::with_label(&gettext("Upgrade"));
                upgrade.set_tooltip_text(Some(&gettext_f(
                    "Download the higher-accuracy model · {size}",
                    &[(
                        "size",
                        &model_actions::format_download_size(language.download_size()),
                    )],
                )));
                upgrade.set_valign(gtk::Align::Center);
                upgrade.set_sensitive(!busy);
                let target = Rc::downgrade(self);
                upgrade.connect_clicked(move |_| {
                    if let Some(manager) = target.upgrade() {
                        manager.install(language);
                    }
                });
                row.add_suffix(&upgrade);
            }
            let action = if installed {
                let button = widgets::icon_button(
                    "user-trash-symbolic",
                    &gettext_f("Remove {name}", &[("name", &language.name)]),
                );
                button.add_css_class("flat");
                button
            } else {
                gtk::Button::with_label(&gettext("Install"))
            };
            action.set_valign(gtk::Align::Center);
            action.set_sensitive(!busy);
            row.add_suffix(&action);
            let target = Rc::downgrade(self);
            action.connect_clicked(move |_| {
                if let Some(manager) = target.upgrade() {
                    if installed {
                        manager.remove(language);
                    } else {
                        manager.install(language);
                    }
                }
            });
            self.groups[category].add(&row);
            self.rows.borrow_mut().push((category, row));
        }
        for (index, placeholder) in self.empty.iter().enumerate() {
            placeholder.set_visible(counts[index] == 0);
            placeholder.set_title(&gettext(match index {
                0 => gettext_noop("No Matching Languages"),
                1 => gettext_noop("No Matching Writing Systems"),
                _ => gettext_noop("No Matching Tools"),
            }));
            placeholder.set_description(Some(&gettext(if self.installed.is_active() {
                gettext_noop("Turn off Installed to browse models you can download.")
            } else {
                gettext_noop("Try another search.")
            })));
        }
        self.update_summary();
    }

    fn update_summary(&self) {
        let category = match self.categories.active_name().as_deref() {
            Some("scripts") => 1,
            Some("tools") => 2,
            _ => 0,
        };
        for (index, group) in self.groups.iter().enumerate() {
            group.set_visible(index == category);
        }
        if self.download.borrow().is_some() {
            return;
        }
        let rows = self.rows.borrow();
        let count = rows.iter().filter(|(index, _)| *index == category).count() as u32;
        let (singular, plural) = match category {
            0 => (
                gettext_noop("{count} language"),
                gettext_noop("{count} languages"),
            ),
            1 => (
                gettext_noop("{count} writing system"),
                gettext_noop("{count} writing systems"),
            ),
            _ => (gettext_noop("{count} tool"), gettext_noop("{count} tools")),
        };
        self.status.set_label(&ngettext_f(
            singular,
            plural,
            count,
            &[("count", &count.to_string())],
        ));
    }

    fn remove(self: &Rc<Self>, language: &Language) {
        let Some(ui) = self.ui.upgrade() else { return };
        if ui.attachments.busy.get() > 0 {
            self.status.set_label(&gettext(
                "Finish or cancel text extraction before removing OCR languages",
            ));
            return;
        }
        match languages::remove(&language.code, &self.backend.paths.ocr_models_dir()) {
            Ok(()) => {
                library::set_languages(&ui, &self.backend, &library::languages(&ui));
                self.populate();
            }
            Err(failure) => self.status.set_label(&crate::i18n::error_message(&failure)),
        }
    }

    fn install(self: &Rc<Self>, language: &'static Language) {
        if self.download.borrow().is_some() {
            return;
        }
        let download = Download::default();
        self.download.replace(Some(download.clone()));
        self.progress.set_fraction(0.0);
        self.progress.set_visible(true);
        self.cancel.set_visible(true);
        self.cancel.set_sensitive(true);
        self.populate();
        self.page.scroll_to_top();
        let target = self.clone();
        let directory = self.backend.paths.ocr_models_dir();
        let worker_download = download.clone();
        let worker = self.backend.runtime.spawn(async move {
            languages::install(&language.code, &directory, &worker_download).await
        });
        let size = language.download_size();
        glib::MainContext::default().spawn_local(async move {
            while !worker.is_finished() {
                target
                    .progress
                    .set_fraction((download.bytes() as f64 / size as f64).min(1.0));
                target.status.set_label(&gettext_f(
                    "Downloading {name}… {downloaded} / {total}",
                    &[
                        ("name", &language.name),
                        (
                            "downloaded",
                            &model_actions::format_download_size(download.bytes()),
                        ),
                        ("total", &model_actions::format_download_size(size)),
                    ],
                ));
                glib::timeout_future(Duration::from_millis(100)).await;
            }
            let result = worker
                .await
                .map_err(|_| error(gettext("The OCR language download stopped unexpectedly")))
                .and_then(|result| result);
            target.download.take();
            target.progress.set_visible(false);
            target.cancel.set_visible(false);
            if result.is_ok()
                && language.selectable()
                && let Some(ui) = target.ui.upgrade()
                && library::languages(&ui).is_empty()
            {
                library::set_languages(&ui, &target.backend, &language.code);
            }
            target.populate();
            if let Err(failure) = result {
                target
                    .status
                    .set_label(&crate::i18n::error_message(&failure));
            }
        });
    }
}
