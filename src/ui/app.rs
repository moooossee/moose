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
