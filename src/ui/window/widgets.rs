use adw::prelude::*;
use gtk::Align;

pub(super) fn icon_button(icon_name: &str, tooltip: &str) -> gtk::Button {
    let button = gtk::Button::from_icon_name(icon_name);
    button.add_css_class("flat");
    button.set_tooltip_text(Some(tooltip));
    button
}

pub(super) fn composer_button(icon_name: &str, tooltip: &str) -> gtk::Button {
    let button = gtk::Button::from_icon_name(icon_name);
    button.add_css_class("circular");
    button.add_css_class("moose-composer-button");
    if let Some(image) = button.child().and_downcast::<gtk::Image>() {
        image.set_pixel_size(18);
    }
    button.set_tooltip_text(Some(tooltip));
    button
}

pub(super) fn string_list_factory(
    width_chars: i32,
    show_selection: bool,
) -> gtk::SignalListItemFactory {
    let factory = gtk::SignalListItemFactory::new();
    factory.connect_setup(move |_, object| {
        let Some(item) = object.downcast_ref::<gtk::ListItem>() else {
            return;
        };
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        row.set_hexpand(show_selection);
        row.set_margin_top(if show_selection { 4 } else { 0 });
        row.set_margin_bottom(if show_selection { 4 } else { 0 });
        let label = gtk::Label::builder()
            .xalign(0.0)
            .hexpand(show_selection)
            .width_chars(if show_selection {
                width_chars.min(20)
            } else {
                1
            })
            .max_width_chars(width_chars)
            .single_line_mode(!show_selection)
            .ellipsize(if show_selection {
                gtk::pango::EllipsizeMode::None
            } else {
                gtk::pango::EllipsizeMode::End
            })
            .wrap(show_selection)
            .wrap_mode(gtk::pango::WrapMode::WordChar)
            .natural_wrap_mode(gtk::NaturalWrapMode::Word)
            .build();
        row.append(&label);
        if show_selection {
            let check = gtk::Image::from_icon_name("object-select-symbolic");
            check.set_valign(Align::Center);
            item.bind_property("selected", &check, "opacity")
                .transform_to(|_, selected: bool| Some(if selected { 1.0_f64 } else { 0.0_f64 }))
                .sync_create()
                .build();
            row.append(&check);
        }
        item.set_child(Some(&row));
    });
    factory.connect_bind(|_, object| {
        let Some(item) = object.downcast_ref::<gtk::ListItem>() else {
            return;
        };
        let Some(value) = item.item().and_downcast::<gtk::StringObject>() else {
            return;
        };
        let Some(label) = item
            .child()
            .and_then(|row| row.first_child())
            .and_downcast::<gtk::Label>()
        else {
            return;
        };
        let text = value.string();
        label.set_label(&text);
        label.set_tooltip_text(Some(&text));
    });
    factory
}

pub(super) fn section_label(text: &str) -> gtk::Label {
    let label = gtk::Label::builder()
        .label(text)
        .halign(Align::Start)
        .xalign(0.0)
        .build();
    label.add_css_class("heading");
    label
}

pub(super) fn status_label(text: &str) -> gtk::Label {
    let label = gtk::Label::builder()
        .label(text)
        .halign(Align::Center)
        .valign(Align::Center)
        .build();
    label.add_css_class("dim-label");
    label
}
