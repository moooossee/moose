use super::*;
use std::path::Path;

pub(super) fn list(ui: &WindowUi, backend: &Backend, assets: &[Asset]) -> Option<adw::Clamp> {
    let mut documents = assets
        .iter()
        .filter(|asset| asset.kind != "image")
        .peekable();
    documents.peek()?;

    let list = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .activate_on_single_click(true)
        .build();
    list.add_css_class("boxed-list");

    for asset in documents {
        let size = model_actions::format_download_size(asset.byte_size.max(0) as u64);
        let kind = if asset.kind == "pdf" {
            "PDF".into()
        } else {
            Path::new(&asset.name)
                .extension()
                .and_then(|extension| extension.to_str())
                .filter(|extension| !extension.is_empty())
                .map(str::to_uppercase)
                .unwrap_or_else(|| "Document".into())
        };
        let description = if asset.kind == "pdf" && asset.page_count > 0 {
            format!(
                "{kind} · {} {} · {size}",
                asset.page_count,
                if asset.page_count == 1 {
                    "page"
                } else {
                    "pages"
                }
            )
        } else {
            format!("{kind} · {size}")
        };
        let row = adw::ActionRow::builder()
            .title(&asset.name)
            .subtitle(&description)
            .title_lines(1)
            .subtitle_lines(1)
            .use_markup(false)
            .activatable(true)
            .tooltip_text(format!("Preview {}\n{description}", asset.name))
            .build();

        let (guessed_type, uncertain) = gio::content_type_guess(Some(&asset.name), None::<&[u8]>);
        let content_type = if asset.kind == "pdf" || uncertain {
            gio::content_type_from_mime_type(&asset.mime_type).unwrap_or(guessed_type)
        } else {
            guessed_type
        };
        let icon = gtk::Image::from_gicon(&gio::content_type_get_symbolic_icon(&content_type));
        icon.set_pixel_size(24);
        icon.set_valign(gtk::Align::Center);
        row.add_prefix(&icon);

        let open = gtk::Image::from_icon_name("document-open-symbolic");
        open.add_css_class("dim-label");
        row.add_suffix(&open);

        let repository = backend.conversation_repository.clone();
        let parent = ui.window.downgrade();
        let asset = asset.clone();
        row.connect_activated(move |_| {
            if let Some(parent) = parent.upgrade() {
                library::preview(&parent, &repository, &asset, 1);
            }
        });
        list.append(&row);
    }

    Some(
        adw::Clamp::builder()
            .child(&list)
            .maximum_size(360)
            .tightening_threshold(360)
            .width_request(260)
            .build(),
    )
}
