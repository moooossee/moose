use std::rc::Rc;

use adw::prelude::*;

use super::{Backend, WindowUi, widgets};

pub(super) fn details_dialog(heading: &str, details: &str) -> adw::AlertDialog {
    let text = gtk::Label::builder()
        .label(details)
        .selectable(true)
        .wrap(true)
        .wrap_mode(gtk::pango::WrapMode::WordChar)
        .xalign(0.0)
        .yalign(0.0)
        .hexpand(true)
        .build();
    let scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vscrollbar_policy(gtk::PolicyType::Automatic)
        .min_content_height(160)
        .max_content_height(400)
        .propagate_natural_height(true)
        .child(&text)
        .build();
    let dialog = adw::AlertDialog::builder()
        .heading(heading)
        .extra_child(&scroll)
        .close_response("close")
        .default_response("close")
        .build();
    dialog.add_response("copy", "Copy Details");
    dialog.add_response("close", "Close");
    let full_details = details.to_string();
    dialog.connect_response(Some("copy"), move |_, _| {
        text.clipboard().set_text(&full_details)
    });
    dialog
}

pub(super) fn show_logs(ui: &Rc<WindowUi>, backend: &Rc<Backend>, ollama: bool) {
    let path = if ollama {
        backend.paths.ollama_log_path()
    } else {
        backend.paths.app_log_path()
    };
    let heading = if ollama { "Ollama Log" } else { "Moose Log" };
    let contents = crate::logging::recent(&path)
        .unwrap_or_else(|error| format!("The log could not be read: {error}"));
    let details = format!(
        "Moose {}\n{}\n\n{}",
        env!("CARGO_PKG_VERSION"),
        path.display(),
        contents
    );
    let dialog = adw::Dialog::builder()
        .title(heading)
        .content_width(820)
        .content_height(560)
        .build();
    let header = adw::HeaderBar::new();
    let title = adw::WindowTitle::new(heading, &format!("Moose {}", env!("CARGO_PKG_VERSION")));
    header.set_title_widget(Some(&title));
    let copy = widgets::icon_button("edit-copy-symbolic", "Copy Log");
    header.pack_start(&copy);
    copy.connect_clicked(move |button| button.clipboard().set_text(&details));
    let buffer = gtk::TextBuffer::new(None);
    buffer.set_enable_undo(false);
    buffer.set_text(&contents);
    let text = gtk::TextView::builder()
        .buffer(&buffer)
        .editable(false)
        .cursor_visible(false)
        .monospace(true)
        .wrap_mode(gtk::WrapMode::WordChar)
        .left_margin(18)
        .right_margin(18)
        .top_margin(18)
        .bottom_margin(18)
        .hexpand(true)
        .vexpand(true)
        .build();
    let scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vscrollbar_policy(gtk::PolicyType::Automatic)
        .child(&text)
        .vexpand(true)
        .build();
    let location = path.to_string_lossy();
    let footer = gtk::Label::builder()
        .label(location.as_ref())
        .tooltip_text(location.as_ref())
        .ellipsize(gtk::pango::EllipsizeMode::Middle)
        .selectable(true)
        .xalign(0.0)
        .margin_start(18)
        .margin_end(18)
        .margin_top(10)
        .margin_bottom(10)
        .css_classes(["caption", "dim-label"])
        .build();
    let toolbar = adw::ToolbarView::builder().content(&scroll).build();
    toolbar.add_top_bar(&header);
    toolbar.add_bottom_bar(&footer);
    dialog.set_child(Some(&toolbar));
    dialog.present(Some(&ui.window));
}

pub(super) fn show_error(parent: &impl IsA<gtk::Widget>, heading: &str, details: &str) {
    crate::logging::record("ERROR", heading, details);
    details_dialog(heading, details).present(Some(parent));
}
