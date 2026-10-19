use std::{cell::RefCell, rc::Rc};

use comrak::nodes::{AstNode, NodeValue};
use gtk::{glib, pango, prelude::*, subclass::prelude::*};

use super::{math_view, safe_uri};

const CURSOR_SPACE: i32 = 1;

mod imp {
    use std::cell::Cell;

    use super::*;

    #[derive(Default)]
    pub struct MarkdownTextView {
        pub min_width_chars: Cell<i32>,
        pub max_width_chars: Cell<i32>,
        pub measure_content_width: Cell<bool>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for MarkdownTextView {
        const NAME: &'static str = "MooseMarkdownTextView";
        type Type = super::MarkdownTextView;
        type ParentType = gtk::TextView;
    }

    impl ObjectImpl for MarkdownTextView {}

    impl WidgetImpl for MarkdownTextView {
        fn measure(&self, orientation: gtk::Orientation, for_size: i32) -> (i32, i32, i32, i32) {
            let (minimum, natural, minimum_baseline, natural_baseline) =
                self.parent_measure(orientation, for_size);
            let view = self.obj();
            if orientation == gtk::Orientation::Vertical {
                let layout = view.create_pango_layout(Some("M"));
                let attributes = pango::AttrList::new();
                attributes.insert(pango::AttrFloat::new_line_height(1.5));
                layout.set_attributes(Some(&attributes));
                let line_height =
                    layout.pixel_size().1.max(1) + view.top_margin() + view.bottom_margin();
                return (
                    minimum.max(line_height),
                    natural.max(line_height),
                    minimum_baseline,
                    natural_baseline,
                );
            }

            let context = view.pango_context();
            let metrics = context.metrics(None, None);
            let char_width =
                ((metrics.approximate_char_width() + pango::SCALE - 1) / pango::SCALE).max(1);
            let margins = view.left_margin() + view.right_margin() + CURSOR_SPACE;
            let minimum = minimum.max(char_width * self.min_width_chars.get() + margins);
            let maximum = char_width * self.max_width_chars.get() + margins;
            if !self.measure_content_width.get() {
                return (
                    minimum,
                    maximum.max(minimum),
                    minimum_baseline,
                    natural_baseline,
                );
            }

            let buffer = view.buffer();
            let mut indentation = 0;
            buffer.tag_table().foreach(|tag| {
                indentation = indentation.max(tag.left_margin() + tag.right_margin());
            });
            let natural = natural
                .max(view.text_width() + margins + indentation)
                .min(maximum)
                .max(minimum);
            (minimum, natural, minimum_baseline, natural_baseline)
        }
    }

    impl TextViewImpl for MarkdownTextView {}
}

glib::wrapper! {
    pub struct MarkdownTextView(ObjectSubclass<imp::MarkdownTextView>)
        @extends gtk::TextView, gtk::Widget,
        @implements gtk::Accessible, gtk::AccessibleText, gtk::Buildable,
                    gtk::ConstraintTarget, gtk::Scrollable;
}

impl MarkdownTextView {
    pub(super) fn new() -> Self {
        let view: Self = glib::Object::builder()
            .property("editable", false)
            .property("cursor-visible", false)
            .property("accepts-tab", false)
            .property("wrap-mode", gtk::WrapMode::WordChar)
            .property("hexpand", true)
            .build();
        view.imp().measure_content_width.set(true);
        view.set_width_chars(1, 120);
        view.add_css_class("moose-markdown-text");
        view.buffer().set_enable_undo(false);
        view.buffer().connect_changed(glib::clone!(
            #[weak]
            view,
            move |_| view.queue_resize()
        ));
        bind_links(&view);
        view
    }

    pub(super) fn set_streaming(&self, streaming: bool) {
        self.imp().measure_content_width.set(!streaming);
    }

    pub(super) fn sync_from(&self, source: &Self) {
        super::text_sync::update(self, source);
    }

    pub(super) fn set_width_chars(&self, minimum: i32, maximum: i32) {
        self.imp().min_width_chars.set(minimum.max(1));
        self.imp().max_width_chars.set(maximum.max(minimum).max(1));
        self.queue_resize();
    }

    fn text_width(&self) -> i32 {
        let buffer = self.buffer();
        let text = buffer.slice(&buffer.start_iter(), &buffer.end_iter(), true);
        let layout = self.create_pango_layout(Some(&text));
        let attributes = pango::AttrList::new();
        let mut start = buffer.start_iter();
        let mut byte_offset = 0u32;

        while !start.is_end() {
            let mut end = start;
            if !end.forward_to_tag_toggle(None::<&gtk::TextTag>) {
                end = buffer.end_iter();
            }
            let next_offset = byte_offset + buffer.slice(&start, &end, true).len() as u32;
            let mut font = pango::FontDescription::new();
            let mut scale = 1.0;
            let mut tags = start.tags();
            tags.sort_by_key(|tag| tag.priority());
            for tag in tags {
                if let Some(description) = tag.font_desc() {
                    font.merge(Some(&description), true);
                }
                if tag.is_scale_set() {
                    scale *= tag.scale();
                }
            }

            let mut font_attribute = pango::AttrFontDesc::new(&font);
            font_attribute.set_start_index(byte_offset);
            font_attribute.set_end_index(next_offset);
            attributes.insert(font_attribute);
            let mut scale_attribute = pango::AttrFloat::new_scale(scale);
            scale_attribute.set_start_index(byte_offset);
            scale_attribute.set_end_index(next_offset);
            attributes.insert(scale_attribute);
            start = end;
            byte_offset = next_offset;
        }

        for (offset, (byte_index, character)) in text.char_indices().enumerate() {
            if character != '\u{fffc}' {
                continue;
            }
            let Some(anchor) = buffer.iter_at_offset(offset as i32).child_anchor() else {
                continue;
            };
            let Some(widget) = anchor
                .widgets()
                .into_iter()
                .find(|widget| widget.parent().as_ref() == Some(self.upcast_ref::<gtk::Widget>()))
            else {
                continue;
            };
            let (size, _) = widget.preferred_size();
            let bounds = pango::Rectangle::new(
                0,
                -size.height() * pango::SCALE,
                size.width() * pango::SCALE,
                size.height() * pango::SCALE,
            );
            let mut shape = pango::AttrShape::new(&bounds, &bounds);
            shape.set_start_index(byte_index as u32);
            shape.set_end_index((byte_index + character.len_utf8()) as u32);
            attributes.insert(shape);
        }

        layout.set_attributes(Some(&attributes));
        layout.pixel_size().0
    }

    pub(super) fn append_text(&self, text: &str) {
        let buffer = self.buffer();
        let start = buffer.char_count();
        buffer.insert(&mut buffer.end_iter(), text);
        buffer.remove_all_tags(&buffer.iter_at_offset(start), &buffer.end_iter());
    }

    pub(super) fn append_children<'a>(&self, node: &'a AstNode<'a>) {
        for child in node.children() {
            self.append_inline(child);
        }
    }

    pub(super) fn tag(
        &self,
        name: &str,
        properties: &[(&str, &dyn glib::value::ToValue)],
    ) -> gtk::TextTag {
        let table = self.buffer().tag_table();
        if let Some(tag) = table.lookup(name) {
            return tag;
        }
        let tag = gtk::TextTag::new(Some(name));
        tag.set_properties(properties);
        table.add(&tag);
        tag
    }

    pub(super) fn apply_tag(&self, tag: &gtk::TextTag, start: i32) {
        let buffer = self.buffer();
        buffer.apply_tag(tag, &buffer.iter_at_offset(start), &buffer.end_iter());
    }

    pub(super) fn append_inline<'a>(&self, node: &'a AstNode<'a>) {
        let start = self.buffer().char_count();
        let value = node.data().value.clone();
        let tag = match &value {
            NodeValue::Emph => Some(self.tag("emphasis", &[("style", &pango::Style::Italic)])),
            NodeValue::Strong => Some(self.tag("strong", &[("weight", &700i32)])),
            NodeValue::Strikethrough => {
                Some(self.tag("strikethrough", &[("strikethrough", &true)]))
            }
            NodeValue::Underline => {
                Some(self.tag("underline", &[("underline", &pango::Underline::Single)]))
            }
            NodeValue::Code(_) => Some(self.tag("code", &[("family", &"monospace")])),
            NodeValue::Subscript => {
                Some(self.tag("subscript", &[("rise", &-3000i32), ("scale", &0.833f64)]))
            }
            NodeValue::Superscript => {
                Some(self.tag("superscript", &[("rise", &6000i32), ("scale", &0.833f64)]))
            }
            NodeValue::Link(link) => safe_uri(&link.url).map(|uri| {
                self.tag(
                    &format!("link:{uri}"),
                    &[("underline", &pango::Underline::Single)],
                )
            }),
            _ => None,
        };

        match value {
            NodeValue::Text(text) => self.append_text(&text),
            NodeValue::Code(code) => self.append_text(&code.literal),
            NodeValue::SoftBreak | NodeValue::LineBreak => self.append_text("\n"),
            NodeValue::HtmlInline(text) => self.append_text(&text),
            NodeValue::EscapedTag(text) => self.append_text(text),
            NodeValue::WikiLink(link) => self.append_text(&link.url),
            NodeValue::Link(link) | NodeValue::Image(link) if node.first_child().is_none() => {
                self.append_text(&link.url);
            }
            NodeValue::Math(formula) => {
                if !formula.display_math
                    && let Some(widget) = math_view::inline(&formula.literal)
                {
                    self.append_widget(&widget);
                } else {
                    if self.buffer().end_iter().line_offset() != 0 {
                        self.append_text("\n");
                    }
                    self.append_widget(&math_view::block(&formula.literal));
                    self.append_text("\n");
                }
            }
            _ => self.append_children(node),
        }

        if let Some(tag) = tag {
            self.apply_tag(&tag, start);
        }
    }

    fn append_widget(&self, widget: &impl IsA<gtk::Widget>) {
        let buffer = self.buffer();
        let start = buffer.char_count();
        let anchor = buffer.create_child_anchor(&mut buffer.end_iter());
        buffer.remove_all_tags(&buffer.iter_at_offset(start), &buffer.end_iter());
        self.add_child_at_anchor(widget, &anchor);
    }
}

fn link_at_iter(iter: &gtk::TextIter) -> Option<String> {
    iter.tags().into_iter().find_map(|tag| {
        let name = tag.name()?;
        name.strip_prefix("link:").map(str::to_owned)
    })
}

fn link_at_position(view: &MarkdownTextView, x: f64, y: f64) -> Option<String> {
    let (x, y) = view.window_to_buffer_coords(gtk::TextWindowType::Widget, x as i32, y as i32);
    let (iter, _) = view.iter_at_position(x, y)?;
    link_at_iter(&iter)
}

fn open_link(view: &MarkdownTextView, uri: &str) {
    let parent = view
        .root()
        .and_then(|root| root.downcast::<gtk::Window>().ok());
    gtk::UriLauncher::new(uri).launch(parent.as_ref(), None::<&gtk::gio::Cancellable>, |_| {});
}

fn bind_links(view: &MarkdownTextView) {
    let pressed = Rc::new(RefCell::new(None));
    let click = gtk::GestureClick::builder()
        .button(1)
        .propagation_phase(gtk::PropagationPhase::Capture)
        .build();
    click.connect_pressed(glib::clone!(
        #[weak]
        view,
        #[strong]
        pressed,
        move |gesture, count, x, y| {
            *pressed.borrow_mut() = None;
            let modifiers = gesture.current_event_state();
            if count == 1
                && !modifiers.intersects(
                    gtk::gdk::ModifierType::SHIFT_MASK | gtk::gdk::ModifierType::CONTROL_MASK,
                )
                && !view.buffer().has_selection()
            {
                *pressed.borrow_mut() = link_at_position(&view, x, y).map(|uri| (uri, x, y));
            }
        }
    ));
    click.connect_released(glib::clone!(
        #[weak]
        view,
        #[strong]
        pressed,
        move |_, count, x, y| {
            let Some((uri, start_x, start_y)) = pressed.borrow_mut().take() else {
                return;
            };
            if count == 1
                && !view.buffer().has_selection()
                && !view.drag_check_threshold(start_x as i32, start_y as i32, x as i32, y as i32)
                && link_at_position(&view, x, y).as_deref() == Some(uri.as_str())
            {
                open_link(&view, &uri);
            }
        }
    ));
    click.connect_stopped(move |_| *pressed.borrow_mut() = None);
    view.add_controller(click);

    let motion = gtk::EventControllerMotion::new();
    motion.connect_motion(glib::clone!(
        #[weak]
        view,
        move |_, x, y| {
            view.set_cursor_from_name(Some(if link_at_position(&view, x, y).is_some() {
                "pointer"
            } else {
                "text"
            }));
        }
    ));
    view.add_controller(motion);

    view.set_has_tooltip(true);
    view.connect_query_tooltip(|view, x, y, keyboard, tooltip| {
        let uri = if keyboard {
            let buffer = view.buffer();
            link_at_iter(&buffer.iter_at_mark(&buffer.get_insert()))
        } else {
            link_at_position(view, f64::from(x), f64::from(y))
        };
        if let Some(uri) = uri {
            tooltip.set_text(Some(&uri));
            true
        } else {
            false
        }
    });

    let keys = gtk::EventControllerKey::new();
    keys.connect_key_pressed(glib::clone!(
        #[weak]
        view,
        #[upgrade_or]
        glib::Propagation::Proceed,
        move |_, key, _, modifiers| {
            if matches!(key, gtk::gdk::Key::Return | gtk::gdk::Key::KP_Enter)
                && modifiers.is_empty()
                && !view.buffer().has_selection()
            {
                let buffer = view.buffer();
                if let Some(uri) = link_at_iter(&buffer.iter_at_mark(&buffer.get_insert())) {
                    open_link(&view, &uri);
                    return glib::Propagation::Stop;
                }
            }
            glib::Propagation::Proceed
        }
    ));
    view.add_controller(keys);
}
