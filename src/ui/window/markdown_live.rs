use std::cell::RefCell;

use gtk::prelude::*;
use gtk::{Align, Orientation};

use super::{code_view, markdown_view};

pub(super) struct LiveMarkdown {
    root: gtk::Box,
    blocks: RefCell<Vec<LiveBlock>>,
}

enum LiveBlock {
    Text { label: gtk::Label },
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

    pub(super) fn finish(&self, content: &str) {
        {
            let mut blocks = self.blocks.borrow_mut();
            remove_live_blocks(&self.root, &mut blocks, 0);
        }
        markdown_view::update(&self.root, content);
    }
}

impl LiveBlock {
    fn new(segment: &LiveSegment) -> Self {
        match segment.kind {
            LiveSegmentKind::Text => {
                let label = plain_live_label(&segment.content);
                Self::Text { label }
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
        }
    }

    fn widget(&self) -> gtk::Widget {
        match self {
            Self::Text { label } => label.clone().upcast(),
            Self::Code(block) => block.widget(),
        }
    }

    fn update(&mut self, segment: &LiveSegment) {
        match self {
            Self::Text { label } => label.set_text(&segment.content),
            Self::Code(block) => block.update(&segment.content, &segment.language),
        }
    }
}

fn remove_live_blocks(root: &gtk::Box, blocks: &mut Vec<LiveBlock>, start: usize) {
    for block in blocks.drain(start..) {
        root.remove(&block.widget());
    }
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
        .width_chars(120)
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
        while let Some(code_line) = lines.next() {
            if is_closing_fence(code_line, fence.marker, fence.length) {
                break;
            }
            code.push_str(code_line);
        }

        let language = code_view::code_language(&fence.info, &code);
        segments.push(LiveSegment {
            kind: LiveSegmentKind::Code,
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
