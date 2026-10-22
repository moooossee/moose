use std::{
    cell::{OnceCell, RefCell},
    time::Duration,
};

use comrak::{
    format_commonmark,
    nodes::{AstNode, NodeValue, TableAlignment},
};
use gtk::{Align, Orientation, PolicyType, glib, prelude::*, subclass::prelude::*};

use super::text_view::MarkdownTextView;

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct TableView {
        pub content: OnceCell<(gtk::Box, gtk::ScrolledWindow, gtk::Grid)>,
        pub markdown: RefCell<String>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for TableView {
        const NAME: &'static str = "MooseMarkdownTableView";
        type Type = super::TableView;
        type ParentType = gtk::Widget;
    }

    impl ObjectImpl for TableView {
        fn dispose(&self) {
            while let Some(child) = self.obj().first_child() {
                child.unparent();
            }
        }
    }

    impl WidgetImpl for TableView {
        fn request_mode(&self) -> gtk::SizeRequestMode {
            gtk::SizeRequestMode::HeightForWidth
        }

        fn measure(&self, orientation: Orientation, for_size: i32) -> (i32, i32, i32, i32) {
            let Some((content, scrolled, grid)) = self.content.get() else {
                return (0, 0, -1, -1);
            };
            let (minimum, natural, minimum_baseline, natural_baseline) =
                content.measure(orientation, for_size);
            if orientation == Orientation::Horizontal {
                return (minimum, natural, minimum_baseline, natural_baseline);
            }
            let (minimum_width, natural_width, _, _) = grid.measure(Orientation::Horizontal, -1);
            let mut height = grid
                .measure(
                    Orientation::Vertical,
                    for_size.clamp(minimum_width, natural_width),
                )
                .0;
            if for_size >= 0 && for_size < minimum_width {
                height += scrolled.hscrollbar().measure(Orientation::Vertical, -1).0;
            }
            (
                minimum + height,
                natural + height,
                minimum_baseline,
                natural_baseline,
            )
        }

        fn size_allocate(&self, width: i32, height: i32, baseline: i32) {
            if let Some((content, _, grid)) = self.content.get() {
                let width = width.min(grid.measure(Orientation::Horizontal, -1).1);
                content.allocate(width, height, baseline, None);
            }
        }

        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            if let Some((content, _, _)) = self.content.get() {
                self.obj().snapshot_child(content, snapshot);
            }
        }
    }
}

glib::wrapper! {
    pub struct TableView(ObjectSubclass<imp::TableView>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl TableView {
    pub(super) fn content(&self) -> &gtk::Box {
        &self.imp().content.get().unwrap().0
    }

    pub(super) fn sync_source(&self, source: &Self) {
        self.imp()
            .markdown
            .replace(source.imp().markdown.borrow().clone());
    }
}

pub(super) fn render<'a>(node: &'a AstNode<'a>, alignments: &[TableAlignment]) -> TableView {
    let table: TableView = glib::Object::builder()
        .property("hexpand", true)
        .property("vexpand", false)
        .property("valign", Align::Start)
        .build();
    let mut markdown = String::new();
    format_commonmark(node, &super::markdown_options(), &mut markdown).unwrap();
    table.imp().markdown.replace(markdown);

    let grid = gtk::Grid::builder()
        .hexpand(true)
        .halign(Align::Start)
        .valign(Align::Start)
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
        .vexpand(true)
        .overlay_scrolling(false)
        .propagate_natural_width(true)
        .hscrollbar_policy(PolicyType::Automatic)
        .vscrollbar_policy(PolicyType::External)
        .build();
    scrolled.add_css_class("moose-markdown-table-scroll");

    let copy = gtk::Button::builder()
        .icon_name("edit-copy-symbolic")
        .tooltip_text("Copy table")
        .halign(Align::End)
        .build();
    copy.add_css_class("flat");
    copy.add_css_class("moose-table-copy");
    copy.update_property(&[gtk::accessible::Property::Label("Copy table")]);
    copy.connect_clicked(glib::clone!(
        #[weak]
        table,
        move |button| {
            button.clipboard().set_text(&table.imp().markdown.borrow());
            button.set_icon_name("object-select-symbolic");
            let weak = button.downgrade();
            glib::timeout_add_local_once(Duration::from_millis(1200), move || {
                if let Some(button) = weak.upgrade() {
                    button.set_icon_name("edit-copy-symbolic");
                }
            });
        }
    ));

    let header = gtk::Box::builder()
        .orientation(Orientation::Horizontal)
        .spacing(8)
        .hexpand(true)
        .build();
    header.add_css_class("moose-table-header");
    let title = gtk::Label::builder()
        .label("Spreadsheet")
        .hexpand(true)
        .xalign(0.0)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .build();
    title.add_css_class("caption-heading");
    title.add_css_class("moose-table-title");
    header.append(&gtk::Image::from_icon_name("x-office-spreadsheet-symbolic"));
    header.append(&title);
    header.append(&gtk::Separator::new(Orientation::Vertical));
    header.append(&copy);

    let content = gtk::Box::new(Orientation::Vertical, 0);
    content.add_css_class("moose-table-block");
    content.append(&header);
    content.append(&scrolled);
    content.set_parent(&table);
    table.imp().content.set((content, scrolled, grid)).unwrap();
    table
}
