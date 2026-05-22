use adw::prelude::*;

use crate::APPLICATION_ID;

pub(super) struct FirstRunGuide {
    pub(super) root: gtk::Box,
    pub(super) stack: gtk::Stack,
    pub(super) start_button: gtk::Button,
    pub(super) create_button: gtk::Button,
    pub(super) connect_button: gtk::Button,
}

pub(super) fn build() -> FirstRunGuide {
    let root = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .hexpand(true)
        .vexpand(true)
        .build();
    root.add_css_class("moose-first-run");

    let stack = gtk::Stack::builder().hexpand(true).vexpand(true).build();
    stack.set_transition_type(gtk::StackTransitionType::SlideLeftRight);

    let start_button = gtk::Button::builder()
        .label("Start Guide")
        .halign(gtk::Align::Center)
        .valign(gtk::Align::Center)
        .build();
    start_button.add_css_class("suggested-action");
    start_button.add_css_class("moose-guide-start");

    let welcome_page = adw::StatusPage::builder()
        .icon_name(APPLICATION_ID)
        .title("Welcome to Moose")
        .description("Set up an Ollama instance and get cozy with local chat.")
        .hexpand(true)
        .vexpand(true)
        .child(&start_button)
        .build();

    let create_button = gtk::Button::with_label("Create Ollama Instance");
    let connect_button = gtk::Button::with_label("Connect External Instance");
    create_button.add_css_class("suggested-action");
    create_button.add_css_class("moose-guide-choice");
    connect_button.add_css_class("moose-guide-choice");

    let title = gtk::Label::builder()
        .label("Instances")
        .halign(gtk::Align::Center)
        .justify(gtk::Justification::Center)
        .build();
    title.add_css_class("title-1");

    let description = gtk::Label::builder()
        .label("Instances are Ollama providers Moose uses for models and chat.")
        .halign(gtk::Align::Center)
        .justify(gtk::Justification::Center)
        .wrap(true)
        .wrap_mode(gtk::pango::WrapMode::WordChar)
        .build();
    description.add_css_class("dim-label");

    let choices = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(10)
        .hexpand(true)
        .build();
    choices.append(&create_button);
    choices.append(&connect_button);

    let instances_content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(18)
        .halign(gtk::Align::Fill)
        .valign(gtk::Align::Center)
        .hexpand(true)
        .vexpand(true)
        .build();
    instances_content.add_css_class("moose-guide-content");
    instances_content.append(&title);
    instances_content.append(&description);
    instances_content.append(&choices);

    let instances_clamp = adw::Clamp::builder()
        .maximum_size(440)
        .tightening_threshold(360)
        .hexpand(true)
        .vexpand(true)
        .child(&instances_content)
        .build();

    stack.add_named(&welcome_page, Some("welcome"));
    stack.add_named(&instances_clamp, Some("instances"));
    stack.set_visible_child_name("welcome");
    root.append(&stack);

    FirstRunGuide {
        root,
        stack,
        start_button,
        create_button,
        connect_button,
    }
}

pub(super) fn reset(guide: &FirstRunGuide) {
    guide.stack.set_visible_child_name("welcome");
}
