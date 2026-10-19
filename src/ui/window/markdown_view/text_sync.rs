use std::collections::HashSet;

use gtk::prelude::*;

use super::text_view::MarkdownTextView;
use crate::ui::window::streaming_text;

struct Run {
    start: i32,
    end: i32,
    tags: Vec<gtk::TextTag>,
    names: Vec<String>,
}

struct Anchor {
    offset: usize,
    widget: gtk::Widget,
    key: String,
}

struct Snapshot {
    text: String,
    runs: Vec<Run>,
    anchors: Vec<Anchor>,
}

impl Snapshot {
    fn new(view: &MarkdownTextView) -> Self {
        let buffer = view.buffer();
        let text = buffer
            .slice(&buffer.start_iter(), &buffer.end_iter(), true)
            .to_string();
        let mut runs = Vec::new();
        let mut iter = buffer.start_iter();
        while !iter.is_end() {
            let start = iter.offset();
            let tags = iter.tags();
            let mut names = tags
                .iter()
                .filter_map(|tag| tag.name().map(|name| name.to_string()))
                .collect::<Vec<_>>();
            names.sort();
            if !iter.forward_to_tag_toggle(None::<&gtk::TextTag>) {
                iter = buffer.end_iter();
            }
            runs.push(Run {
                start,
                end: iter.offset(),
                tags,
                names,
            });
        }
        let mut anchors = Vec::new();
        for (offset, (_, character)) in text.char_indices().enumerate() {
            if character != '\u{fffc}' {
                continue;
            }
            if let Some(anchor) = buffer.iter_at_offset(offset as i32).child_anchor()
                && let Some(widget) = anchor.widgets().into_iter().find(|widget| {
                    widget.parent().as_ref() == Some(view.upcast_ref::<gtk::Widget>())
                })
            {
                anchors.push(Anchor {
                    offset,
                    key: widget.widget_name().to_string(),
                    widget,
                });
            }
        }
        Self {
            text,
            runs,
            anchors,
        }
    }
}

pub(super) fn update(target: &MarkdownTextView, source: &MarkdownTextView) {
    let previous = Snapshot::new(target);
    let next = Snapshot::new(source);
    let bytes = streaming_text::shared_prefix(&previous.text, &next.text);
    let mut shared = next.text[..bytes].chars().count();
    for (anchors, other) in [
        (&previous.anchors, &next.anchors),
        (&next.anchors, &previous.anchors),
    ] {
        for anchor in anchors
            .iter()
            .filter(|anchor| anchor.offset < shared)
            .collect::<Vec<_>>()
        {
            if !other
                .iter()
                .any(|other| other.offset == anchor.offset && other.key == anchor.key)
            {
                shared = shared.min(anchor.offset);
            }
        }
    }
    let buffer = target.buffer();
    let _notifications = buffer.freeze_notify();
    let old_count = buffer.char_count() as usize;
    if shared < old_count {
        buffer.delete(
            &mut buffer.iter_at_offset(shared as i32),
            &mut buffer.end_iter(),
        );
    }
    let offsets = next
        .text
        .char_indices()
        .map(|(index, _)| index)
        .chain(std::iter::once(next.text.len()))
        .collect::<Vec<_>>();
    let mut cursor = shared;
    for anchor in next.anchors.iter().filter(|anchor| anchor.offset >= shared) {
        buffer.insert(
            &mut buffer.end_iter(),
            &next.text[offsets[cursor]..offsets[anchor.offset]],
        );
        source.remove(&anchor.widget);
        let child = buffer.create_child_anchor(&mut buffer.end_iter());
        target.add_child_at_anchor(&anchor.widget, &child);
        cursor = anchor.offset + 1;
    }
    buffer.insert(&mut buffer.end_iter(), &next.text[offsets[cursor]..]);

    let from = (shared as i32).min(shared_styles(&previous.runs, &next.runs));
    if from >= buffer.char_count() {
        prune(&buffer.tag_table(), &next.runs);
        return;
    }
    buffer.remove_all_tags(&buffer.iter_at_offset(from), &buffer.end_iter());
    let table = buffer.tag_table();
    let source_table = source.buffer().tag_table();
    for run in next.runs.iter().filter(|run| run.end > from) {
        for tag in &run.tags {
            let existing = tag.name().and_then(|name| table.lookup(&name));
            let tag = existing.unwrap_or_else(|| {
                source_table.remove(tag);
                table.add(tag);
                tag.clone()
            });
            buffer.apply_tag(
                &tag,
                &buffer.iter_at_offset(run.start.max(from)),
                &buffer.iter_at_offset(run.end),
            );
        }
    }
    prune(&table, &next.runs);
    target.queue_resize();
}

fn prune(table: &gtk::TextTagTable, runs: &[Run]) {
    let names = runs
        .iter()
        .flat_map(|run| run.names.iter().map(String::as_str))
        .collect::<HashSet<_>>();
    let mut unused = Vec::new();
    table.foreach(|tag| {
        if tag
            .name()
            .is_some_and(|name| !names.contains(name.as_str()))
        {
            unused.push(tag.clone());
        }
    });
    for tag in unused {
        table.remove(&tag);
    }
}

fn shared_styles(previous: &[Run], next: &[Run]) -> i32 {
    let mut left = 0;
    let mut right = 0;
    let mut offset = 0;
    while let (Some(old), Some(new)) = (previous.get(left), next.get(right)) {
        if old.names != new.names {
            break;
        }
        offset = old.end.min(new.end);
        if old.end == offset {
            left += 1;
        }
        if new.end == offset {
            right += 1;
        }
    }
    offset
}
