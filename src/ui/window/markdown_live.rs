use std::cell::RefCell;

use gtk::prelude::*;
use gtk::{Align, Orientation};

use super::{code_view, markdown_view, math_syntax, math_view};

pub(super) struct LiveMarkdown {
    root: gtk::Box,
    blocks: RefCell<Vec<LiveBlock>>,
}

enum LiveBlock {
    Text { root: gtk::Box, content: String },
    Math { root: gtk::Box, content: String },
    Code(code_view::LiveCodeBlock),
}

struct LiveSegment {
    kind: LiveSegmentKind,
    content: String,
    language: String,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum LiveSegmentKind {
    Text,
    Code,
    Math,
}

impl LiveMarkdown {
    pub(super) fn new() -> Self {
        let root = gtk::Box::builder()
            .orientation(Orientation::Vertical)
            .spacing(8)
            .hexpand(true)
            .halign(Align::Fill)
            .build();
        root.add_css_class("moose-message-content");
        root.add_css_class("moose-markdown-content");

        Self {
            root,
            blocks: RefCell::new(Vec::new()),
        }
    }

    pub(super) fn widget(&self) -> &gtk::Box {
        &self.root
    }

    pub(super) fn update(&self, content: &str) {
        let segments = live_segments(content);
        let mut blocks = self.blocks.borrow_mut();
        let mut index = 0usize;

        while index < segments.len() {
            let mismatched = blocks
                .get(index)
                .is_some_and(|block| block.kind() != segments[index].kind);
            if mismatched {
                remove_live_blocks(&self.root, &mut blocks, index);
            }

            if let Some(block) = blocks.get_mut(index) {
                block.update(&segments[index]);
            } else {
                let block = LiveBlock::new(&segments[index]);
                self.root.append(&block.widget());
                blocks.push(block);
            }

            index += 1;
        }

        remove_live_blocks(&self.root, &mut blocks, segments.len());
    }
}

impl LiveBlock {
    fn new(segment: &LiveSegment) -> Self {
        match segment.kind {
            LiveSegmentKind::Text => {
                let root = gtk::Box::new(Orientation::Vertical, 8);
                root.set_hexpand(true);
                update_live_text(&root, &segment.content);
                Self::Text {
                    root,
                    content: segment.content.clone(),
                }
            }
            LiveSegmentKind::Math => {
                let root = gtk::Box::new(Orientation::Vertical, 8);
                root.set_hexpand(true);
                root.append(&math_view::block(&segment.content));
                Self::Math {
                    root,
                    content: segment.content.clone(),
                }
            }
            LiveSegmentKind::Code => Self::Code(code_view::LiveCodeBlock::new(
                &segment.content,
                &segment.language,
            )),
        }
    }

    fn kind(&self) -> LiveSegmentKind {
        match self {
            Self::Text { .. } => LiveSegmentKind::Text,
            Self::Code(_) => LiveSegmentKind::Code,
            Self::Math { .. } => LiveSegmentKind::Math,
        }
    }

    fn widget(&self) -> gtk::Widget {
        match self {
            Self::Text { root, .. } | Self::Math { root, .. } => root.clone().upcast(),
            Self::Code(block) => block.widget(),
        }
    }

    fn update(&mut self, segment: &LiveSegment) {
        match self {
            Self::Text { root, content } => {
                if *content != segment.content {
                    update_live_text(root, &segment.content);
                    content.clone_from(&segment.content);
                }
            }
            Self::Math { root, content } => {
                if *content != segment.content {
                    while let Some(child) = root.first_child() {
                        root.remove(&child);
                    }
                    root.append(&math_view::block(&segment.content));
                    content.clone_from(&segment.content);
                }
            }
            Self::Code(block) => block.update(&segment.content, &segment.language),
        }
    }
}

fn remove_live_blocks(root: &gtk::Box, blocks: &mut Vec<LiveBlock>, start: usize) {
    for block in blocks.drain(start..) {
        root.remove(&block.widget());
    }
}

fn update_live_text(root: &gtk::Box, content: &str) {
    if !math_syntax::prepare(content).formulas.is_empty() {
        markdown_view::update(root, content);
        return;
    }
    if let Some(label) = root
        .first_child()
        .and_then(|widget| widget.downcast::<gtk::Label>().ok())
    {
        if label.next_sibling().is_none() {
            label.set_text(content);
            return;
        }
    }
    while let Some(child) = root.first_child() {
        root.remove(&child);
    }
    root.append(&plain_live_label(content));
}

fn plain_live_label(content: &str) -> gtk::Label {
    let label = gtk::Label::builder()
        .label(content)
        .halign(Align::Fill)
        .hexpand(true)
        .selectable(true)
        .wrap(true)
        .wrap_mode(gtk::pango::WrapMode::WordChar)
        .natural_wrap_mode(gtk::NaturalWrapMode::Word)
        .width_chars(1)
        .max_width_chars(120)
        .xalign(0.0)
        .build();
    label.add_css_class("body");
    label.add_css_class("moose-markdown-label");
    label.add_css_class("moose-markdown-paragraph");
    label
}

fn live_segments(content: &str) -> Vec<LiveSegment> {
    let mut segments = Vec::new();
    let mut text = String::new();
    let mut lines = content.split_inclusive('\n').peekable();

    while let Some(line) = lines.next() {
        let Some(fence) = parse_opening_fence(line) else {
            text.push_str(line);
            continue;
        };

        push_live_text_segment(&mut segments, &mut text);

        let mut code = String::new();
        let mut closed = false;
        for code_line in lines.by_ref() {
            if is_closing_fence(code_line, fence.marker, fence.length) {
                closed = true;
                break;
            }
            code.push_str(code_line);
        }

        let language = code_view::code_language(&fence.info, &code);
        segments.push(LiveSegment {
            kind: if closed && math_view::is_math_language(&fence.info) {
                LiveSegmentKind::Math
            } else {
                LiveSegmentKind::Code
            },
            content: code,
            language,
        });
    }

    push_live_text_segment(&mut segments, &mut text);
    segments
}

struct CodeFence {
    marker: char,
    length: usize,
    info: String,
}

fn push_live_text_segment(segments: &mut Vec<LiveSegment>, text: &mut String) {
    if text.is_empty() {
        return;
    }

    segments.push(LiveSegment {
        kind: LiveSegmentKind::Text,
        content: std::mem::take(text),
        language: String::new(),
    });
}

fn parse_opening_fence(line: &str) -> Option<CodeFence> {
    let trimmed = line.trim_start_matches(' ');
    let marker = trimmed.chars().next()?;
    if marker != '`' && marker != '~' {
        return None;
    }

    let length = trimmed.chars().take_while(|value| *value == marker).count();
    if length < 3 {
        return None;
    }

    let info = trimmed[length..].trim().to_string();
    Some(CodeFence {
        marker,
        length,
        info,
    })
}

fn is_closing_fence(line: &str, marker: char, opening_length: usize) -> bool {
    let trimmed = line.trim_start_matches(' ');
    let length = trimmed.chars().take_while(|value| *value == marker).count();
    if length < opening_length {
        return false;
    }

    trimmed[length..].trim().is_empty()
}
