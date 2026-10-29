use adw::prelude::*;
use gtk::gio;

use crate::{APPLICATION_ID, ui::window};

pub fn run() -> gtk::glib::ExitCode {
    if let Ok(paths) = crate::platform::AppPaths::new("moose") {
        crate::logging::init(&paths);
    }
    let app = adw::Application::builder()
        .application_id(APPLICATION_ID)
        .flags(gio::ApplicationFlags::empty())
        .build();

    app.connect_startup(|_| {
        if let Some(display) = gtk::gdk::Display::default() {
            gtk::IconTheme::for_display(&display)
                .add_search_path(concat!(env!("CARGO_MANIFEST_DIR"), "/data/icons"));
        }
    });

    app.connect_activate(|app| {
        if let Some(window) = app.active_window() {
            window.present();
            return;
        }

        let window = window::build(app);
        window.present();
    });

    app.run()
}
