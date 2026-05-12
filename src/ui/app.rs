use adw::prelude::*;
use gtk::gio;

use crate::{APPLICATION_ID, ui::window};

pub fn run() -> gtk::glib::ExitCode {
    let app = adw::Application::builder()
        .application_id(APPLICATION_ID)
        .flags(gio::ApplicationFlags::empty())
        .build();

    app.connect_activate(|app| {
        let window = window::build(app);
        window.present();
    });

    app.run()
}
