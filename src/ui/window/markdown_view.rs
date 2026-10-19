pub(super) mod live_view;
mod table_view;
mod text_sync;
mod text_view;

use comrak::{
    Arena, Options,
    nodes::{AstNode, ListDelimType, ListType, NodeList, NodeMath, NodeValue},
    parse_document,
};
use gtk::prelude::*;
use gtk::{Align, Orientation};
use url::Url;

use super::{code_view, math_syntax, math_view};
use text_view::MarkdownTextView;

pub(super) fn render(content: &str) -> gtk::Box {
    let root = gtk::Box::builder()
        .orientation(Orientation::Vertical)
        .spacing(8)
        .hexpand(true)
        .halign(Align::Fill)
        .build();
    root.add_css_class("moose-message-content");
    root.add_css_class("moose-markdown-content");
    update(&root, content);
    root
}

pub(super) fn update(root: &gtk::Box, content: &str) {
    while let Some(child) = root.first_child() {
        root.remove(&child);
    }

    let arena = Arena::new();
    let prepared = math_syntax::prepare(content);
    let document = parse_document(&arena, &prepared.markdown, &markdown_options());
    for node in document.descendants() {
        let formula = match &node.data().value {
            NodeValue::Math(formula) => prepared.formulas.get(&formula.literal),
            _ => None,
        };
        if let Some(formula) = formula {
            node.data_mut().value = NodeValue::Math(NodeMath {
                dollar_math: true,
                display_math: formula.display,
                literal: formula.source.clone(),
            });
        } else {
            match &mut node.data_mut().value {
                NodeValue::CodeBlock(block) => {
                    block.literal = prepared.restore(&block.literal);
                    block.info = prepared.restore(&block.info);
                }
                NodeValue::Code(code) => code.literal = prepared.restore(&code.literal),
                NodeValue::HtmlBlock(block) => block.literal = prepared.restore(&block.literal),
                NodeValue::Text(text) => *text = prepared.restore(text).into(),
                NodeValue::HtmlInline(text) | NodeValue::FrontMatter(text) => {
                    *text = prepared.restore(text)
                }
                NodeValue::Link(link) | NodeValue::Image(link) => {
                    link.url = prepared.restore(&link.url);
                    link.title = prepared.restore(&link.title);
                }
                _ => {}
            }
        }
    }

    let mut renderer = Renderer::new(root);
    renderer.append_children(document, BlockStyle::default());

    if root.first_child().is_none() && !content.is_empty() {
        renderer.append_plain(content, BlockStyle::default());
    }
}

pub(super) fn constrain_width(root: &gtk::Box, width_chars: i32) {
    constrain_widget_width(root.upcast_ref(), width_chars);
}

fn constrain_widget_width(widget: &gtk::Widget, width_chars: i32) {
    if let Some(view) = widget.downcast_ref::<MarkdownTextView>() {
        view.set_width_chars(1, width_chars);
    } else if let Some(label) = widget.downcast_ref::<gtk::Label>() {
        label.set_width_chars(-1);
        label.set_max_width_chars(width_chars);
    }

    let mut child = widget.first_child();
    while let Some(widget) = child {
        child = widget.next_sibling();
        constrain_widget_width(&widget, width_chars);
    }
}

pub(super) fn markdown_options() -> Options<'static> {
    let mut options = Options::default();
    options.extension.autolink = true;
    options.extension.strikethrough = true;
    options.extension.table = true;
    options.extension.tagfilter = true;
    options.extension.tasklist = true;
    options.extension.math_code = true;
    options
}

#[derive(Clone, Copy, Default)]
struct BlockStyle {
    left_margin: i32,
    quote_depth: usize,
}

struct Renderer<'a> {
    parent: &'a gtk::Box,
    view: Option<MarkdownTextView>,
    pending_break: usize,
    prefix: Option<(String, i32)>,
}

impl<'r> Renderer<'r> {
    fn new(parent: &'r gtk::Box) -> Self {
        Self {
            parent,
            view: None,
            pending_break: 2,
            prefix: None,
        }
    }

    fn view(&mut self) -> MarkdownTextView {
        self.view
            .get_or_insert_with(|| {
                let view = MarkdownTextView::new();
                self.parent.append(&view);
                view
            })
            .clone()
    }

    fn begin_block(
        &mut self,
        style: BlockStyle,
        heading: Option<u8>,
    ) -> (MarkdownTextView, i32, gtk::TextTag) {
        let view = self.view();
        if view.buffer().char_count() > 0 {
            let start = view.buffer().char_count();
            view.append_text(&"\n".repeat(self.pending_break));
            if self.pending_break > 1 {
                let tag = view.tag("block-spacing", &[("scale", &0.35f64)]);
                view.apply_tag(&tag, start + 1);
            }
        }
        self.pending_break = 2;
        let start = view.buffer().char_count();
        let prefix = self.prefix.take();
        let name = format!(
            "block:{}:{}:{heading:?}",
            style.left_margin, style.quote_depth
        );
        let tag = view.tag(&name, &[("left-margin", &style.left_margin)]);
        if style.quote_depth > 0 {
            tag.set_paragraph_background_rgba(Some(&gtk::gdk::RGBA::new(0.5, 0.5, 0.5, 0.08)));
        }
        if let Some(level) = heading {
            tag.set_weight(700);
            tag.set_scale(match level {
                1 => 1.22,
                2 => 1.14,
                3 => 1.06,
                _ => 1.0,
            });
        }
        if let Some((prefix, width)) = prefix {
            let hanging = view.tag(&format!("list-indent:{width}"), &[("indent", &-width)]);
            view.append_text(&prefix);
            view.apply_tag(&hanging, start);
        }
        (view, start, tag)
    }

    fn append_plain(&mut self, content: &str, style: BlockStyle) {
        if content.is_empty() && self.prefix.is_none() {
            return;
        }
        let (view, start, tag) = self.begin_block(style, None);
        view.append_text(content);
        view.apply_tag(&tag, start);
    }

    fn append_paragraph<'a>(
        &mut self,
        node: &'a AstNode<'a>,
        style: BlockStyle,
        heading: Option<u8>,
    ) {
        let (view, start, tag) = self.begin_block(style, heading);
        view.append_children(node);
        view.apply_tag(&tag, start);
    }

    fn append_widget(&mut self, widget: &impl IsA<gtk::Widget>, style: BlockStyle) {
        if self.prefix.is_some() {
            self.append_plain("", style);
        }
        widget.set_margin_start(style.left_margin);
        self.parent.append(widget);
        self.view = None;
        self.pending_break = 2;
    }

    fn append_children<'a>(&mut self, node: &'a AstNode<'a>, style: BlockStyle) {
        for child in node.children() {
            self.append_block(child, style);
        }
    }

    fn append_block<'a>(&mut self, node: &'a AstNode<'a>, style: BlockStyle) {
        let value = node.data().value.clone();
        match value {
            NodeValue::Paragraph => self.append_paragraph(node, style, None),
            NodeValue::Heading(heading) => self.append_paragraph(node, style, Some(heading.level)),
            NodeValue::CodeBlock(block) => {
                if math_view::is_math_language(&block.info) {
                    self.append_widget(&math_view::block(&block.literal), style);
                } else {
                    self.append_widget(&code_view::code_block(&block), style);
                }
            }
            NodeValue::Math(formula) => {
                self.append_widget(&math_view::block(&formula.literal), style)
            }
            NodeValue::BlockQuote | NodeValue::MultilineBlockQuote(_) => {
                self.append_children(
                    node,
                    BlockStyle {
                        left_margin: style.left_margin + 16,
                        quote_depth: style.quote_depth + 1,
                    },
                );
            }
            NodeValue::List(list) => self.append_list(node, list, style),
            NodeValue::Table(table) => {
                self.append_widget(&table_view::render(node, &table.alignments), style)
            }
            NodeValue::ThematicBreak => {
                let separator = gtk::Separator::new(Orientation::Horizontal);
                separator.add_css_class("moose-markdown-separator");
                self.append_widget(&separator, style);
            }
            NodeValue::HtmlBlock(block) => self.append_plain(&block.literal, style),
            NodeValue::FrontMatter(content) => self.append_plain(&content, style),
            _ if node.first_child().is_some() => self.append_children(node, style),
            _ => {
                let (view, start, tag) = self.begin_block(style, None);
                view.append_inline(node);
                view.apply_tag(&tag, start);
            }
        }
    }

    fn append_list<'a>(&mut self, node: &'a AstNode<'a>, list: NodeList, style: BlockStyle) {
        if self.prefix.is_some() {
            self.append_plain("", style);
        }
        if list.tight && style.left_margin > 0 {
            self.pending_break = 1;
        }
        let start = list.start;
        for (index, item) in node.children().enumerate() {
            if index > 0 {
                self.pending_break = if list.tight { 1 } else { 2 };
            }
            let marker = match &item.data().value {
                NodeValue::TaskItem(task) => {
                    if task.symbol.is_some() {
                        "☑ ".to_string()
                    } else {
                        "☐ ".to_string()
                    }
                }
                _ => match list.list_type {
                    ListType::Ordered => {
                        let delimiter = match list.delimiter {
                            ListDelimType::Period => '.',
                            ListDelimType::Paren => ')',
                        };
                        format!("{}{delimiter} ", start.saturating_add(index))
                    }
                    ListType::Bullet => "- ".to_string(),
                },
            };
            let width = self
                .parent
                .create_pango_layout(Some(&marker))
                .pixel_size()
                .0
                .max(1);
            self.prefix = Some((marker, width));
            let item_style = BlockStyle {
                left_margin: style.left_margin + width + 12,
                ..style
            };
            self.append_children(item, item_style);
            if self.prefix.is_some() {
                self.append_plain("", item_style);
            }
        }
        self.pending_break = 2;
    }
}

fn safe_uri(value: &str) -> Option<String> {
    let url = Url::parse(value).ok()?;
    match url.scheme() {
        "http" | "https" | "mailto" => Some(url.to_string()),
        _ => None,
    }
}
