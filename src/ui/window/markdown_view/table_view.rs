use comrak::nodes::{AstNode, NodeValue, TableAlignment};
use gtk::{Align, PolicyType, prelude::*};

use super::text_view::MarkdownTextView;

pub(super) fn render<'a>(
    node: &'a AstNode<'a>,
    alignments: &[TableAlignment],
) -> gtk::ScrolledWindow {
    let grid = gtk::Grid::builder()
        .hexpand(true)
        .halign(Align::Start)
        .build();
    grid.add_css_class("moose-markdown-table");

    for (row_index, row) in node.children().enumerate() {
        let header = matches!(&row.data().value, NodeValue::TableRow(true));
        for (column_index, cell) in row.children().enumerate() {
            let view = MarkdownTextView::new();
            view.set_width_chars(12, 36);
            view.set_justification(match alignments.get(column_index) {
                Some(TableAlignment::Center) => gtk::Justification::Center,
                Some(TableAlignment::Right) => gtk::Justification::Right,
                _ => gtk::Justification::Left,
            });
            view.add_css_class("moose-markdown-table-cell");
            if header {
                view.add_css_class("moose-markdown-table-header");
            }
            view.append_children(cell);
            grid.attach(&view, column_index as i32, row_index as i32, 1, 1);
        }
    }

    let scrolled = gtk::ScrolledWindow::builder()
        .child(&grid)
        .hexpand(true)
        .hscrollbar_policy(PolicyType::Automatic)
        .vscrollbar_policy(PolicyType::Never)
        .build();
    scrolled.add_css_class("moose-markdown-table-scroll");
    scrolled
}
