use gtk::prelude::*;

use super::{render, table_view::TableView, text_view::MarkdownTextView};
use crate::ui::window::{chat_scroll::ChatScroll, code_view};

pub(in crate::ui::window) struct LiveContent {
    root: gtk::Box,
    source: String,
}

impl LiveContent {
    pub(in crate::ui::window) fn new(source: &str) -> Self {
        let root = render(source);
        configure(root.upcast_ref());
        Self {
            root,
            source: source.to_string(),
        }
    }

    pub(in crate::ui::window) fn widget(&self) -> gtk::Widget {
        self.root.clone().upcast()
    }

    pub(in crate::ui::window) fn update(&mut self, source: &str) -> bool {
        if self.source == source {
            return true;
        }
        if has_selection(self.root.upcast_ref()) {
            return false;
        }
        let next = render(source);
        configure(next.upcast_ref());
        sync_box(&self.root, &next);
        self.source.clear();
        self.source.push_str(source);
        true
    }

    pub(in crate::ui::window) fn has_selection(&self) -> bool {
        has_selection(self.root.upcast_ref())
    }
}

fn configure(widget: &gtk::Widget) {
    if let Some(view) = widget.downcast_ref::<MarkdownTextView>() {
        view.set_streaming(!view.has_css_class("moose-markdown-table-cell"));
    }
    if let Some(scrolled) = widget.downcast_ref::<gtk::ScrolledWindow>()
        && scrolled.has_css_class("moose-code-scroll")
        && let Some(view) = scrolled.child().and_downcast::<sourceview5::View>()
    {
        ChatScroll::bind_code(scrolled, &view.buffer());
    }
    for child in children(widget) {
        configure(&child);
    }
}

fn has_selection(widget: &gtk::Widget) -> bool {
    if let Some(view) = widget.downcast_ref::<gtk::TextView>()
        && view.buffer().has_selection()
    {
        return true;
    }
    if let Some(label) = widget.downcast_ref::<gtk::Label>()
        && label.selection_bounds().is_some()
    {
        return true;
    }
    children(widget).iter().any(has_selection)
}

fn children(widget: &gtk::Widget) -> Vec<gtk::Widget> {
    let mut result = Vec::new();
    let mut child = widget.first_child();
    while let Some(widget) = child {
        child = widget.next_sibling();
        result.push(widget);
    }
    result
}

fn sync_widget(target: &gtk::Widget, source: &gtk::Widget) -> bool {
    if target.type_() != source.type_() {
        return false;
    }
    let mut old_classes = target.css_classes();
    let mut classes = source.css_classes();
    old_classes.sort();
    classes.sort();
    if old_classes != classes {
        return false;
    }
    if source.has_css_class("moose-math-block") || source.has_css_class("moose-math-inline") {
        return target.widget_name() == source.widget_name();
    }
    target.set_margin_start(source.margin_start());
    target.set_margin_end(source.margin_end());
    target.set_margin_top(source.margin_top());
    target.set_margin_bottom(source.margin_bottom());
    target.set_halign(source.halign());
    target.set_hexpand(source.hexpands());
    if let (Some(target), Some(source)) = (
        target.downcast_ref::<TableView>(),
        source.downcast_ref::<TableView>(),
    ) {
        target.sync_source(source);
        sync_box(target.content(), source.content());
        target.queue_resize();
    } else if let (Some(target), Some(source)) = (
        target.downcast_ref::<MarkdownTextView>(),
        source.downcast_ref::<MarkdownTextView>(),
    ) {
        target.set_justification(source.justification());
        target.sync_from(source);
    } else if let (Some(target), Some(source)) = (
        target.downcast_ref::<sourceview5::View>(),
        source.downcast_ref::<sourceview5::View>(),
    ) {
        code_view::sync_source_view(target, source);
    } else if let (Some(target), Some(source)) = (
        target.downcast_ref::<gtk::Box>(),
        source.downcast_ref::<gtk::Box>(),
    ) {
        target.set_spacing(source.spacing());
        sync_box(target, source);
    } else if let (Some(target), Some(source)) = (
        target.downcast_ref::<gtk::Grid>(),
        source.downcast_ref::<gtk::Grid>(),
    ) {
        sync_grid(target, source);
    } else if let (Some(target), Some(source)) = (
        target.downcast_ref::<gtk::Viewport>(),
        source.downcast_ref::<gtk::Viewport>(),
    ) {
        if let Some(next) = source.child() {
            if target
                .child()
                .is_none_or(|child| !sync_widget(&child, &next))
            {
                source.set_child(None::<&gtk::Widget>);
                target.set_child(Some(&next));
            }
        } else {
            target.set_child(None::<&gtk::Widget>);
        }
    } else if let (Some(target), Some(source)) = (
        target.downcast_ref::<gtk::ScrolledWindow>(),
        source.downcast_ref::<gtk::ScrolledWindow>(),
    ) {
        target.set_min_content_height(source.min_content_height());
        target.set_max_content_height(source.max_content_height());
        if let Some(next) = source.child() {
            if target
                .child()
                .is_none_or(|child| !sync_widget(&child, &next))
            {
                source.set_child(None::<&gtk::Widget>);
                target.set_child(Some(&next));
            }
        } else {
            target.set_child(None::<&gtk::Widget>);
        }
    } else if let (Some(target), Some(source)) = (
        target.downcast_ref::<gtk::Label>(),
        source.downcast_ref::<gtk::Label>(),
    ) {
        if target.text() != source.text() {
            target.set_text(&source.text());
        }
    } else if !target.is::<gtk::Button>() && !target.is::<gtk::Separator>() {
        return false;
    }
    true
}

fn sync_box(target: &gtk::Box, source: &gtk::Box) {
    let previous = children(target.upcast_ref());
    let next = children(source.upcast_ref());
    for (index, widget) in next.iter().enumerate() {
        if let Some(old) = previous.get(index) {
            if sync_widget(old, widget) {
                continue;
            }
            let sibling = old.prev_sibling();
            source.remove(widget);
            target.remove(old);
            target.insert_child_after(widget, sibling.as_ref());
        } else {
            source.remove(widget);
            target.append(widget);
        }
    }
    for widget in previous.iter().skip(next.len()) {
        target.remove(widget);
    }
}

fn sync_grid(target: &gtk::Grid, source: &gtk::Grid) {
    let mut retained = Vec::new();
    for widget in children(source.upcast_ref()) {
        let (column, row, width, height) = source.query_child(&widget);
        retained.push((column, row));
        if let Some(old) = target.child_at(column, row) {
            if sync_widget(&old, &widget) {
                continue;
            }
            target.remove(&old);
        }
        source.remove(&widget);
        target.attach(&widget, column, row, width, height);
    }
    for widget in children(target.upcast_ref()) {
        let (column, row, _, _) = target.query_child(&widget);
        if !retained.contains(&(column, row)) {
            target.remove(&widget);
        }
    }
}
