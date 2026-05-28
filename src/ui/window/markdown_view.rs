use comrak::{
    Arena, Options,
    nodes::{
        AstNode, ListType, NodeCode, NodeHeading, NodeLink, NodeList, NodeValue, TableAlignment,
    },
    parse_document,
};
use gtk::prelude::*;
use gtk::{Align, Orientation, PolicyType, glib};
use url::Url;

use super::code_view;

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
    let document = parse_document(&arena, content, &markdown_options());

    for child in document.children() {
        append_block(root, child);
    }

    if root.first_child().is_none() && !content.is_empty() {
        append_paragraph(root, &escape(content));
    }
}

pub(super) fn constrain_labels(root: &gtk::Box, width_chars: i32) {
    constrain_widget_labels(root.upcast_ref::<gtk::Widget>(), width_chars);
}

fn constrain_widget_labels(widget: &gtk::Widget, width_chars: i32) {
    if let Some(label) = widget.downcast_ref::<gtk::Label>() {
        label.set_width_chars(-1);
        label.set_max_width_chars(width_chars);
    }

    let mut child = widget.first_child();
    while let Some(widget) = child {
        child = widget.next_sibling();
        constrain_widget_labels(&widget, width_chars);
    }
}

fn markdown_options() -> Options<'static> {
    let mut options = Options::default();
    options.extension.autolink = true;
    options.extension.strikethrough = true;
    options.extension.table = true;
    options.extension.tagfilter = true;
    options.extension.tasklist = true;
    options
}

fn append_block<'a>(parent: &gtk::Box, node: &'a AstNode<'a>) {
    let value = node.data().value.clone();

    match value {
        NodeValue::Document => append_children(parent, node),
        NodeValue::Paragraph => append_paragraph(parent, &inline_markup_children(node)),
        NodeValue::Heading(heading) => append_heading(parent, node, heading),
        NodeValue::CodeBlock(block) => parent.append(&code_view::code_block(&block)),
        NodeValue::BlockQuote | NodeValue::MultilineBlockQuote(_) => append_quote(parent, node),
        NodeValue::List(list) => append_list(parent, node, list),
        NodeValue::Item(_) => append_children(parent, node),
        NodeValue::Table(table) => append_table(parent, node, &table.alignments),
        NodeValue::ThematicBreak => parent.append(&separator()),
        NodeValue::HtmlBlock(block) => append_plain_block(parent, &block.literal),
        NodeValue::FrontMatter(value) => append_plain_block(parent, &value),
        NodeValue::DescriptionList
        | NodeValue::DescriptionItem(_)
        | NodeValue::DescriptionTerm
        | NodeValue::DescriptionDetails
        | NodeValue::FootnoteDefinition(_)
        | NodeValue::TableRow(_)
        | NodeValue::TableCell
        | NodeValue::TaskItem(_)
        | NodeValue::Alert(_)
        | NodeValue::Subtext
        | NodeValue::BlockDirective(_) => append_children(parent, node),
        _ => {
            let markup = inline_markup(node);
            if !markup.trim().is_empty() {
                append_paragraph(parent, &markup);
            } else {
                append_children(parent, node);
            }
        }
    }
}

fn append_children<'a>(parent: &gtk::Box, node: &'a AstNode<'a>) {
    for child in node.children() {
        append_block(parent, child);
    }
}

fn append_paragraph(parent: &gtk::Box, markup: &str) {
    if markup.trim().is_empty() {
        return;
    }

    let label = markdown_label(markup);
    label.add_css_class("body");
    label.add_css_class("moose-markdown-paragraph");
    parent.append(&label);
}

fn append_heading<'a>(parent: &gtk::Box, node: &'a AstNode<'a>, heading: NodeHeading) {
    let label = markdown_label(&format!("<b>{}</b>", inline_markup_children(node)));
    label.add_css_class("heading");
    label.add_css_class("moose-markdown-heading");
    label.add_css_class(&format!(
        "moose-markdown-heading-{}",
        heading.level.clamp(1, 6)
    ));
    parent.append(&label);
}

fn append_quote<'a>(parent: &gtk::Box, node: &'a AstNode<'a>) {
    let quote = gtk::Box::builder()
        .orientation(Orientation::Vertical)
        .spacing(6)
        .hexpand(true)
        .halign(Align::Fill)
        .build();
    quote.add_css_class("moose-markdown-quote");
    append_children(&quote, node);
    parent.append(&quote);
}

fn append_plain_block(parent: &gtk::Box, content: &str) {
    append_paragraph(parent, &escape(content));
}

fn append_list<'a>(parent: &gtk::Box, node: &'a AstNode<'a>, list: NodeList) {
    let list_box = gtk::Box::builder()
        .orientation(Orientation::Vertical)
        .spacing(5)
        .hexpand(true)
        .halign(Align::Fill)
        .build();
    list_box.add_css_class("moose-markdown-list");

    let start = if list.start == 0 { 1 } else { list.start };

    for (index, item) in node.children().enumerate() {
        let marker = match list.list_type {
            ListType::Ordered => format!("{}.", start + index),
            ListType::Bullet => "-".to_string(),
        };
        append_list_item(&list_box, item, &marker);
    }

    parent.append(&list_box);
}

fn append_list_item<'a>(parent: &gtk::Box, item: &'a AstNode<'a>, marker: &str) {
    let row = gtk::Box::builder()
        .orientation(Orientation::Horizontal)
        .spacing(8)
        .hexpand(true)
        .halign(Align::Fill)
        .build();
    row.add_css_class("moose-markdown-list-item");

    let content = gtk::Box::builder()
        .orientation(Orientation::Vertical)
        .spacing(5)
        .hexpand(true)
        .halign(Align::Fill)
        .build();

    let children = item.children().collect::<Vec<_>>();
    if let Some((checked, task_node)) = task_item(&children) {
        let check = gtk::CheckButton::builder()
            .active(checked)
            .sensitive(false)
            .valign(Align::Start)
            .build();
        check.add_css_class("moose-markdown-task");
        row.append(&check);
        for child in task_node.children() {
            append_block(&content, child);
        }
        for child in children.into_iter().skip(1) {
            append_block(&content, child);
        }
    } else {
        row.append(&list_marker(marker));
        for child in children {
            append_block(&content, child);
        }
    }

    row.append(&content);
    parent.append(&row);
}

fn task_item<'a>(children: &[&'a AstNode<'a>]) -> Option<(bool, &'a AstNode<'a>)> {
    let node = children.first().copied()?;
    match &node.data().value {
        NodeValue::TaskItem(task) => Some((task.symbol.is_some(), node)),
        _ => None,
    }
}

fn list_marker(marker: &str) -> gtk::Label {
    let label = gtk::Label::builder()
        .label(marker)
        .halign(Align::Start)
        .valign(Align::Start)
        .width_chars(3)
        .xalign(1.0)
        .build();
    label.add_css_class("dim-label");
    label.add_css_class("moose-markdown-list-marker");
    label
}

fn append_table<'a>(parent: &gtk::Box, node: &'a AstNode<'a>, alignments: &[TableAlignment]) {
    let grid = gtk::Grid::builder()
        .column_spacing(0)
        .row_spacing(0)
        .hexpand(true)
        .halign(Align::Start)
        .build();
    grid.add_css_class("moose-markdown-table");

    for (row_index, row) in node.children().enumerate() {
        let header = matches!(row.data().value.clone(), NodeValue::TableRow(true));
        for (column_index, cell) in row.children().enumerate() {
            let label = table_cell_label(
                &inline_markup_children(cell),
                alignments.get(column_index).copied(),
            );
            if header {
                label.add_css_class("moose-markdown-table-header");
            }
            grid.attach(&label, column_index as i32, row_index as i32, 1, 1);
        }
    }

    let scrolled = gtk::ScrolledWindow::builder()
        .child(&grid)
        .hexpand(true)
        .hscrollbar_policy(PolicyType::Automatic)
        .vscrollbar_policy(PolicyType::Never)
        .build();
    scrolled.add_css_class("moose-markdown-table-scroll");
    parent.append(&scrolled);
}

fn table_xalign(alignment: Option<TableAlignment>) -> f32 {
    match alignment {
        Some(TableAlignment::Center) => 0.5,
        Some(TableAlignment::Right) => 1.0,
        _ => 0.0,
    }
}

fn separator() -> gtk::Separator {
    let separator = gtk::Separator::new(Orientation::Horizontal);
    separator.add_css_class("moose-markdown-separator");
    separator
}

fn markdown_label(markup: &str) -> gtk::Label {
    let label = gtk::Label::builder()
        .halign(Align::Fill)
        .hexpand(true)
        .selectable(true)
        .wrap(true)
        .wrap_mode(gtk::pango::WrapMode::WordChar)
        .natural_wrap_mode(gtk::NaturalWrapMode::Word)
        .width_chars(120)
        .xalign(0.0)
        .build();
    label.set_markup(markup);
    label.add_css_class("moose-markdown-label");
    bind_label_links(&label);
    label
}

fn table_cell_label(markup: &str, alignment: Option<TableAlignment>) -> gtk::Label {
    let label = gtk::Label::builder()
        .halign(Align::Fill)
        .selectable(true)
        .wrap(true)
        .wrap_mode(gtk::pango::WrapMode::WordChar)
        .natural_wrap_mode(gtk::NaturalWrapMode::Word)
        .width_chars(12)
        .max_width_chars(36)
        .xalign(table_xalign(alignment))
        .build();
    label.set_markup(markup);
    label.add_css_class("moose-markdown-label");
    label.add_css_class("moose-markdown-table-cell");
    bind_label_links(&label);
    label
}

fn bind_label_links(label: &gtk::Label) {
    label.connect_activate_link(|_, uri| {
        let Some(uri) = safe_uri(uri) else {
            return glib::Propagation::Stop;
        };
        gtk::UriLauncher::new(&uri).launch(
            None::<&gtk::Window>,
            None::<&gtk::gio::Cancellable>,
            |_| {},
        );
        glib::Propagation::Stop
    });
}

fn inline_markup_children<'a>(node: &'a AstNode<'a>) -> String {
    node.children().map(inline_markup).collect::<String>()
}

fn inline_markup<'a>(node: &'a AstNode<'a>) -> String {
    let value = node.data().value.clone();

    match value {
        NodeValue::Text(text) => escape(&text),
        NodeValue::Code(NodeCode { literal, .. }) => {
            format!(
                "<span font_family=\"monospace\">{}</span>",
                escape(&literal)
            )
        }
        NodeValue::SoftBreak => "\n".to_string(),
        NodeValue::LineBreak => "\n".to_string(),
        NodeValue::Emph => format!("<i>{}</i>", inline_markup_children(node)),
        NodeValue::Strong => format!("<b>{}</b>", inline_markup_children(node)),
        NodeValue::Strikethrough => format!("<s>{}</s>", inline_markup_children(node)),
        NodeValue::Link(link) => link_markup(node, &link),
        NodeValue::Image(link) => image_markup(node, &link),
        NodeValue::HtmlInline(value) => escape(&value),
        NodeValue::Math(value) => escape(&value.literal),
        NodeValue::WikiLink(link) => escape(&link.url),
        NodeValue::Underline => format!("<u>{}</u>", inline_markup_children(node)),
        NodeValue::Subscript => format!(
            "<span rise=\"-3000\" size=\"smaller\">{}</span>",
            inline_markup_children(node)
        ),
        NodeValue::Superscript => format!(
            "<span rise=\"6000\" size=\"smaller\">{}</span>",
            inline_markup_children(node)
        ),
        NodeValue::Highlight
        | NodeValue::Insert
        | NodeValue::SpoileredText
        | NodeValue::Escaped => inline_markup_children(node),
        NodeValue::EscapedTag(value) => escape(value),
        NodeValue::TaskItem(task) => {
            let state = if task.symbol.is_some() {
                "[x] "
            } else {
                "[ ] "
            };
            format!("{}{}", state, inline_markup_children(node))
        }
        _ => inline_markup_children(node),
    }
}

fn link_markup<'a>(node: &'a AstNode<'a>, link: &NodeLink) -> String {
    let text = inline_markup_children(node);
    let text = if text.is_empty() {
        escape(&link.url)
    } else {
        text
    };

    match safe_uri(&link.url) {
        Some(uri) => format!("<a href=\"{}\">{}</a>", escape(&uri), text),
        None => text,
    }
}

fn image_markup<'a>(node: &'a AstNode<'a>, link: &NodeLink) -> String {
    let text = inline_markup_children(node);
    if text.is_empty() {
        escape(&link.url)
    } else {
        text
    }
}

fn safe_uri(value: &str) -> Option<String> {
    let url = Url::parse(value).ok()?;
    match url.scheme() {
        "http" | "https" | "mailto" => Some(url.to_string()),
        _ => None,
    }
}

fn escape(value: &str) -> String {
    glib::markup_escape_text(value).to_string()
}
