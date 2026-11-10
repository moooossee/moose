use super::*;

pub(super) fn previews() -> gtk::FlowBox {
    let grid = gtk::FlowBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .homogeneous(false)
        .min_children_per_line(1)
        .max_children_per_line(MAX_ATTACHMENTS as u32)
        .column_spacing(6)
        .row_spacing(6)
        .hexpand(true)
        .valign(gtk::Align::Start)
        .build();
    grid.add_css_class("moose-attachment-grid");
    grid
}

pub(super) fn asset_widget(ui: &WindowUi, backend: &Backend, asset: &Asset) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    row.set_halign(gtk::Align::Start);
    row.add_css_class("moose-composer-attachment");
    let content = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    let texture = if asset.kind == "image" {
        backend
            .conversation_repository
            .asset_bytes(&asset.id)
            .ok()
            .and_then(|bytes| gdk::Texture::from_bytes(&glib::Bytes::from_owned(bytes)).ok())
    } else {
        None
    };
    if let Some(texture) = texture {
        let thumbnail = gtk::Overlay::builder()
            .child(&gtk::Box::new(gtk::Orientation::Horizontal, 0))
            .width_request(28)
            .height_request(28)
            .valign(gtk::Align::Center)
            .build();
        thumbnail.add_css_class("moose-composer-attachment-thumbnail");
        thumbnail.set_overflow(gtk::Overflow::Hidden);
        let picture = gtk::Picture::builder()
            .paintable(&texture)
            .content_fit(gtk::ContentFit::Cover)
            .can_shrink(true)
            .can_target(false)
            .alternative_text(&asset.name)
            .build();
        thumbnail.add_overlay(&picture);
        thumbnail.set_measure_overlay(&picture, false);
        content.append(&thumbnail);
    } else {
        let icon = gtk::Image::from_icon_name(if asset.kind == "image" {
            "image-x-generic-symbolic"
        } else {
            "text-x-generic-symbolic"
        });
        icon.set_pixel_size(16);
        icon.set_size_request(28, 28);
        icon.add_css_class("dim-label");
        content.append(&icon);
    }
    let name = gtk::Label::builder()
        .label(&asset.name)
        .xalign(0.0)
        .single_line_mode(true)
        .width_chars(1)
        .max_width_chars(18)
        .ellipsize(gtk::pango::EllipsizeMode::Middle)
        .build();
    name.add_css_class("caption");
    content.append(&name);
    let size = model_actions::format_download_size(asset.byte_size.max(0) as u64);
    let button = gtk::Button::builder()
        .child(&content)
        .tooltip_text(format!("Preview {}\n{size}", asset.name))
        .build();
    button.add_css_class("flat");
    button.add_css_class("moose-composer-attachment-preview");
    let repository = backend.conversation_repository.clone();
    let parent = ui.window.clone();
    let asset = asset.clone();
    button.connect_clicked(move |_| library::preview(&parent, &repository, &asset, 1));
    row.append(&button);
    row
}
