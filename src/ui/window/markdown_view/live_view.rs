use gtk::prelude::*;

use super::{render, table_view::TableView, text_view::MarkdownTextView};
use crate::ui::window::{chat_scroll::ChatScroll, code_view, streaming_text};

pub(in crate::ui::window) struct LiveContent {
    root: gtk::Box,
    source: String,
}

impl LiveContent {
    pub(in crate::ui::window) fn new(source: &str) -> Self {
        let root = render(source);
        configure(root.upcast_ref(), true);
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
        if source.starts_with(&self.source)
            && source.chars().next().is_some_and(char::is_alphabetic)
            && source.chars().all(|character| {
                character.is_alphanumeric()
                    || matches!(character, ' ' | ',' | '!' | '?' | ':' | ';' | '¿' | '¡')
            })
            && let Some(view) = self.root.first_child().and_downcast::<MarkdownTextView>()
            && view.next_sibling().is_none()
        {
            let buffer = view.buffer();
            let mut previous = buffer
                .text(&buffer.start_iter(), &buffer.end_iter(), true)
                .to_string();
            if previous == self.source.trim_end() {
                streaming_text::update(&buffer, &mut previous, source.trim_end());
                self.source.clear();
                self.source.push_str(source);
                return true;
            }
        }
        if has_selection(self.root.upcast_ref()) {
            return false;
        }
        let next = render(source);
        configure(next.upcast_ref(), true);
        sync_box(&self.root, &next);
        self.source.clear();
        self.source.push_str(source);
        true
    }

    pub(in crate::ui::window) fn has_selection(&self) -> bool {
        has_selection(self.root.upcast_ref())
    }
}

pub(in crate::ui::window) fn finish(root: &gtk::Box, next: &gtk::Box) {
    if has_selection(root.upcast_ref()) {
        let root = root.downgrade();
        let next = next.clone();
        gtk::glib::timeout_add_local(std::time::Duration::from_millis(100), move || {
            let Some(root) = root.upgrade().filter(|root| root.parent().is_some()) else {
                return gtk::glib::ControlFlow::Break;
            };
            if has_selection(root.upcast_ref()) {
                return gtk::glib::ControlFlow::Continue;
            }
            finish(&root, &next);
            gtk::glib::ControlFlow::Break
        });
        return;
    }
    for group in children(root.upcast_ref()) {
        if let Some(group) = group.downcast_ref::<gtk::Box>()
            && group.has_css_class("moose-markdown-content")
        {
            let mut sibling = group.prev_sibling();
            while let Some(child) = group.first_child() {
                group.remove(&child);
                root.insert_child_after(&child, sibling.as_ref());
                sibling = Some(child);
            }
            root.remove(group);
        }
    }
    sync_box(root, next);
    configure(root.upcast_ref(), false);
}

fn configure(widget: &gtk::Widget, streaming: bool) {
    if let Some(view) = widget.downcast_ref::<MarkdownTextView>() {
        view.set_streaming(streaming && !view.has_css_class("moose-markdown-table-cell"));
    }
    if streaming
        && let Some(scrolled) = widget.downcast_ref::<gtk::ScrolledWindow>()
        && scrolled.has_css_class("moose-code-scroll")
        && let Some(view) = scrolled.child().and_downcast::<sourceview5::View>()
    {
        ChatScroll::bind_code(scrolled, &view.buffer());
    }
    for child in children(widget) {
        configure(&child, streaming);
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
    let mut index = 0;
    let mut sibling = None;
    for widget in children(source.upcast_ref()) {
        if let Some(offset) = previous[index..]
            .iter()
            .position(|old| sync_widget(old, &widget))
        {
            for old in &previous[index..index + offset] {
                target.remove(old);
            }
            index += offset;
            sibling = Some(previous[index].clone());
            index += 1;
        } else {
            source.remove(&widget);
            target.insert_child_after(&widget, sibling.as_ref());
            sibling = Some(widget);
        }
    }
    for widget in previous.iter().skip(index) {
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
