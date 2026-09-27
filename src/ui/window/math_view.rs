use std::cell::RefCell;
use std::collections::VecDeque;

use gtk::prelude::*;
use gtk::{Align, Orientation, PolicyType, cairo, gdk_pixbuf};
use latex_rust::{Dim, MathFont, MathStyle, SvgOptions};

#[derive(Clone)]
struct FormulaImage {
    mask: cairo::ImageSurface,
    width: i32,
    height: i32,
}

type CachedFormula = (String, bool, Option<FormulaImage>);

thread_local! {
    static FONT: Option<MathFont> = MathFont::stix_two_math().ok();
    static CACHE: RefCell<VecDeque<CachedFormula>> = const { RefCell::new(VecDeque::new()) };
}

pub(super) fn is_math_language(info: &str) -> bool {
    matches!(
        info.split_whitespace().next().unwrap_or_default(),
        "math" | "latex" | "tex"
    )
}

pub(super) fn inline(source: &str) -> Option<gtk::Button> {
    let image = formula_image(source, false)?;
    if image.width > 240 || image.height > 56 {
        return None;
    }
    let button = gtk::Button::builder()
        .child(&drawing(&image))
        .tooltip_text(format!("Copy formula: {}", source.trim()))
        .valign(Align::Center)
        .build();
    button.add_css_class("flat");
    button.add_css_class("moose-math-inline");
    button.update_property(&[gtk::accessible::Property::Label(&format!(
        "Copy formula: {}",
        source.trim()
    ))]);
    let source = source.to_string();
    button.connect_clicked(move |button| button.clipboard().set_text(&source));
    Some(button)
}

pub(super) fn block(source: &str) -> gtk::Box {
    let root = gtk::Box::builder()
        .orientation(Orientation::Vertical)
        .spacing(8)
        .hexpand(true)
        .width_request(280)
        .build();
    root.add_css_class("moose-math-block");

    let image = formula_image(source, true);
    let header = gtk::Box::builder()
        .orientation(Orientation::Horizontal)
        .spacing(8)
        .build();
    let title = gtk::Label::builder()
        .label(if image.is_some() {
            "Formula"
        } else {
            "Formula source"
        })
        .xalign(0.0)
        .hexpand(true)
        .build();
    title.add_css_class("dim-label");
    title.add_css_class("caption");
    header.append(&title);
    let copy = gtk::Button::builder()
        .icon_name("edit-copy-symbolic")
        .tooltip_text("Copy formula")
        .build();
    copy.add_css_class("flat");
    copy.add_css_class("moose-code-copy");
    let original = source.to_string();
    copy.connect_clicked(move |button| button.clipboard().set_text(&original));
    header.append(&copy);
    root.append(&header);

    let source_label = gtk::Label::builder()
        .label(source.trim())
        .selectable(true)
        .wrap(true)
        .wrap_mode(gtk::pango::WrapMode::WordChar)
        .width_chars(1)
        .max_width_chars(72)
        .xalign(0.0)
        .hexpand(true)
        .build();
    source_label.add_css_class("monospace");
    source_label.add_css_class("moose-math-source");
    if let Some(image) = image {
        let area = drawing(&image);
        area.set_halign(Align::Center);
        area.update_property(&[gtk::accessible::Property::Label(source.trim())]);
        let scroll = gtk::ScrolledWindow::builder()
            .child(&area)
            .hexpand(true)
            .hscrollbar_policy(PolicyType::Automatic)
            .vscrollbar_policy(PolicyType::Automatic)
            .propagate_natural_height(true)
            .min_content_width(1)
            .max_content_height(360)
            .build();
        scroll.add_css_class("moose-math-scroll");
        root.append(&scroll);
        let expander = gtk::Expander::builder()
            .label("LaTeX source")
            .child(&source_label)
            .build();
        expander.add_css_class("moose-math-details");
        root.append(&expander);
    } else {
        root.append(&source_label);
    }
    root
}

fn drawing(image: &FormulaImage) -> gtk::DrawingArea {
    let area = gtk::DrawingArea::builder()
        .content_width(image.width)
        .content_height(image.height)
        .accessible_role(gtk::AccessibleRole::Img)
        .build();
    let mask = image.mask.clone();
    area.set_draw_func(move |area, context, _, _| {
        let color = area.color();
        context.set_source_rgba(
            f64::from(color.red()),
            f64::from(color.green()),
            f64::from(color.blue()),
            f64::from(color.alpha()),
        );
        context.scale(0.5, 0.5);
        let _ = context.mask_surface(&mask, 0.0, 0.0);
    });
    area
}

fn formula_image(source: &str, display: bool) -> Option<FormulaImage> {
    let source = source.trim();
    CACHE.with(|cache| {
        let cached = cache
            .borrow()
            .iter()
            .position(|(text, style, _)| text == source && *style == display);
        if let Some(index) = cached {
            let entry = cache.borrow_mut().remove(index)?;
            let image = entry.2.clone();
            cache.borrow_mut().push_back(entry);
            return image;
        }
        if !bounded_source(source) {
            return None;
        }
        let image = std::panic::catch_unwind(|| rasterize(source, display))
            .ok()
            .flatten();
        let mut cache = cache.borrow_mut();
        if cache.len() >= 24 {
            cache.pop_front();
        }
        cache.push_back((source.to_string(), display, image.clone()));
        image
    })
}

fn bounded_source(source: &str) -> bool {
    if source.is_empty() || source.len() > 8192 || source.matches('\\').count() > 256 {
        return false;
    }
    let mut depth = 0usize;
    for ch in source.chars() {
        match ch {
            '{' => {
                depth += 1;
                if depth > 32 {
                    return false;
                }
            }
            '}' => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    ![
        "\\def",
        "\\gdef",
        "\\edef",
        "\\newcommand",
        "\\renewcommand",
    ]
    .iter()
    .any(|command| source.contains(command))
}

fn rasterize(source: &str, display: bool) -> Option<FormulaImage> {
    let (svg, width, height) = FONT.with(|font| {
        let font = font.as_ref()?;
        let ast = latex_rust::parse(source).ok()?;
        let style = if display {
            MathStyle::Display
        } else {
            MathStyle::Text
        };
        let tree = latex_rust::layout(&ast, font, style).ok()?;
        let font_size = if display { 18 } else { 16 };
        let width = f32::from_bits(tree.width.to_ieee32_bits()) * font_size as f32;
        let height = f32::from_bits((&tree.height + &tree.depth).to_ieee32_bits()) * font_size as f32;
        if !width.is_finite()
            || !height.is_finite()
            || width < 0.0
            || height < 0.0
            || width > 8192.0
            || height > 2048.0
            || width * height > 524_288.0
        {
            return None;
        }
        let options = SvgOptions {
            font_size_pt: Dim::from_i64(font_size),
            display,
            ..SvgOptions::new()
        };
        let svg = latex_rust::render_svg(&tree, font, &options).ok()?;
        let start = svg.find("<svg ")?;
        let end = start + svg[start..].find('>')? + 1;
        let width = width.ceil() as i32 + 12;
        let height = height.ceil() as i32 + 12;
        let padded = format!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{width}\" height=\"{height}\" viewBox=\"-6 -6 {width} {height}\">{}",
            &svg[end..]
        );
        Some((padded, width, height))
    })?;
    let loader = gdk_pixbuf::PixbufLoader::with_type("svg").ok()?;
    loader.set_size(width * 2, height * 2);
    loader.write(svg.as_bytes()).ok()?;
    loader.close().ok()?;
    let pixbuf = loader.pixbuf()?;
    if !pixbuf.has_alpha() || pixbuf.n_channels() != 4 {
        return None;
    }
    let mut mask =
        cairo::ImageSurface::create(cairo::Format::A8, pixbuf.width(), pixbuf.height()).ok()?;
    let stride = mask.stride() as usize;
    let pixels = pixbuf.read_pixel_bytes();
    {
        let mut data = mask.data().ok()?;
        for y in 0..pixbuf.height() as usize {
            for x in 0..pixbuf.width() as usize {
                data[y * stride + x] = pixels[y * pixbuf.rowstride() as usize + x * 4 + 3];
            }
        }
    }
    mask.mark_dirty();
    Some(FormulaImage {
        mask,
        width,
        height,
    })
}
