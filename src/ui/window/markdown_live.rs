use std::{
    cell::RefCell,
    ops::Range,
    time::{Duration, Instant},
};

use comrak::{
    Arena,
    nodes::{NodeCodeBlock, NodeValue},
    parse_document,
};
use gtk::prelude::*;
use gtk::{Align, Orientation};

use super::{
    code_view,
    markdown_view::{self, live_view::LiveContent},
    math_syntax, math_view,
};

pub(super) struct LiveMarkdown {
    root: gtk::Box,
    state: RefCell<LiveState>,
}

#[derive(Default)]
struct LiveState {
    source: String,
    committed: usize,
    current: Option<LiveBlock>,
    updated: Option<Instant>,
    deferred: bool,
}

enum LiveBlock {
    Text(LiveContent),
    Code(Box<code_view::LiveCodeBlock>),
}

struct ParsedBlock {
    range: Range<usize>,
    code: Option<NodeCodeBlock>,
    complete: bool,
}

impl LiveMarkdown {
    pub(super) fn new() -> Self {
        let root = gtk::Box::builder()
            .orientation(Orientation::Vertical)
            .spacing(8)
            .hexpand(true)
            .halign(Align::Fill)
            .vexpand(false)
            .valign(Align::Start)
            .build();
        root.add_css_class("moose-message-content");
        root.add_css_class("moose-markdown-content");
        Self {
            root,
            state: RefCell::new(LiveState::default()),
        }
    }

    pub(super) fn widget(&self) -> &gtk::Box {
        &self.root
    }

    pub(super) fn finish(&self, content: &str) {
        markdown_view::update(&self.root, content);
        *self.state.borrow_mut() = LiveState {
            source: content.to_string(),
            committed: content.len(),
            ..LiveState::default()
        };
    }

    pub(super) fn update(&self, content: &str) {
        let mut state = self.state.borrow_mut();
        if state.source == content && !state.deferred {
            return;
        }
        if state.current.as_ref().is_some_and(LiveBlock::has_selection) {
            state.deferred = true;
            return;
        }
        if content.starts_with(state.source.as_str())
            && state.updated.is_some_and(|updated| {
                updated.elapsed() < render_interval(content.len().saturating_sub(state.committed))
            })
        {
            return;
        }
        if let Some(appended) = content.strip_prefix(state.source.as_str()) {
            state.source.push_str(appended);
        } else {
            while let Some(child) = self.root.first_child() {
                self.root.remove(&child);
            }
            *state = LiveState::default();
            state.source.push_str(content);
        }
        state.updated = Some(Instant::now());
        state.deferred = false;

        let base = state.committed;
        let pending = &content[base..];
        let blocks = parse_blocks(pending);
        if blocks.is_empty() {
            if pending.trim().is_empty() {
                state.clear_current(&self.root);
                state.committed = content.len();
            } else {
                state.deferred = !state.update_text(&self.root, pending);
            }
            return;
        }

        for block in blocks {
            let source = &pending[block.range.clone()];
            if let Some(code) = block.code {
                if block.complete && math_view::is_math_language(&code.info) {
                    if state.current.as_ref().is_some_and(LiveBlock::has_selection) {
                        state.deferred = true;
                        break;
                    }
                    let widget = math_view::block(&code.literal);
                    state.clear_current(&self.root);
                    self.root.append(&widget);
                } else {
                    state.update_code(&self.root, &code);
                }
            } else if !state.update_text(&self.root, source) {
                state.deferred = true;
                break;
            }

            if block.complete {
                state.current.take();
                state.committed = base + block.range.end;
            } else {
                break;
            }
        }
    }
}

impl LiveState {
    fn clear_current(&mut self, root: &gtk::Box) {
        if let Some(block) = self.current.take() {
            root.remove(&block.widget());
        }
    }

    fn update_text(&mut self, root: &gtk::Box, source: &str) -> bool {
        if let Some(LiveBlock::Text(block)) = self.current.as_mut() {
            return block.update(source);
        }
        if self.current.as_ref().is_some_and(LiveBlock::has_selection) {
            return false;
        }
        let block = LiveContent::new(source);
        self.clear_current(root);
        root.append(&block.widget());
        self.current = Some(LiveBlock::Text(block));
        true
    }

    fn update_code(&mut self, root: &gtk::Box, code: &NodeCodeBlock) {
        if let Some(LiveBlock::Code(block)) = self.current.as_mut() {
            block.update(&code.literal, &code.info);
            return;
        }
        self.clear_current(root);
        let block = code_view::LiveCodeBlock::new(&code.literal, &code.info);
        root.append(&block.widget());
        self.current = Some(LiveBlock::Code(Box::new(block)));
    }
}

impl LiveBlock {
    fn widget(&self) -> gtk::Widget {
        match self {
            Self::Text(block) => block.widget(),
            Self::Code(block) => block.widget(),
        }
    }

    fn has_selection(&self) -> bool {
        match self {
            Self::Text(block) => block.has_selection(),
            Self::Code(block) => block.has_selection(),
        }
    }
}

fn parse_blocks(source: &str) -> Vec<ParsedBlock> {
    let arena = Arena::new();
    let document = parse_document(&arena, source, &markdown_view::markdown_options());
    let nodes = document.children().collect::<Vec<_>>();
    let mut lines = vec![0];
    for (index, byte) in source.bytes().enumerate() {
        if byte == b'\n' || (byte == b'\r' && source.as_bytes().get(index + 1) != Some(&b'\n')) {
            lines.push(index + 1);
        }
    }
    let starts = nodes
        .iter()
        .enumerate()
        .map(|(index, node)| {
            if index == 0 {
                0
            } else {
                lines
                    .get(node.data().sourcepos.start.line.saturating_sub(1))
                    .copied()
                    .unwrap_or(source.len())
            }
        })
        .collect::<Vec<_>>();

    let blocks = nodes
        .iter()
        .enumerate()
        .map(|(index, node)| {
            let start = starts[index];
            let end = starts.get(index + 1).copied().unwrap_or(source.len());
            let raw = &source[start..end];
            let data = node.data();
            let value = &data.value;
            let opening = lines
                .get(data.sourcepos.start.line.saturating_sub(1))
                .copied()
                .unwrap_or(end);
            let opening_terminated = source[opening..end].contains(['\n', '\r']);
            let terminated = raw.ends_with(['\n', '\r']);
            let complete = index + 1 < nodes.len()
                || match value {
                    NodeValue::CodeBlock(code) => code.fenced && code.closed && terminated,
                    NodeValue::Heading(_) | NodeValue::ThematicBreak => terminated,
                    NodeValue::Paragraph | NodeValue::Table(_) => ends_with_blank_line(raw),
                    _ => false,
                };
            let code = match value {
                NodeValue::CodeBlock(code) if !code.fenced || opening_terminated => {
                    Some(code.as_ref().clone())
                }
                _ => None,
            };
            ParsedBlock {
                range: start..end,
                code,
                complete,
            }
        })
        .collect::<Vec<_>>();
    if !source.contains(['$', '\\']) {
        return blocks;
    }
    let prepared = math_syntax::prepare(source);
    let mut ranges = prepared.ranges.iter().peekable();
    let mut merged: Vec<ParsedBlock> = Vec::with_capacity(blocks.len());
    for mut block in blocks {
        let boundary = block.range.start;
        while ranges.peek().is_some_and(|range| range.end <= boundary) {
            ranges.next();
        }
        let inside_formula = ranges
            .peek()
            .is_some_and(|range| range.start < boundary && boundary < range.end)
            || prepared.incomplete.is_some_and(|start| start < boundary);
        if prepared
            .incomplete
            .is_some_and(|start| block.range.end > start)
        {
            block.complete = false;
        }
        if inside_formula && let Some(previous) = merged.last_mut() {
            previous.range.end = block.range.end;
            previous.complete = block.complete;
            previous.code = None;
        } else {
            merged.push(block);
        }
    }
    merged
}

fn render_interval(bytes: usize) -> Duration {
    Duration::from_millis(match bytes {
        0..=16_384 => 80,
        16_385..=65_536 => 160,
        _ => 240,
    })
}

fn ends_with_blank_line(source: &str) -> bool {
    let Some(prefix) = source
        .strip_suffix('\n')
        .or_else(|| source.strip_suffix('\r'))
    else {
        return false;
    };
    let prefix = prefix.strip_suffix('\r').unwrap_or(prefix);
    prefix
        .rsplit(['\n', '\r'])
        .next()
        .is_some_and(|line| line.bytes().all(|byte| byte == b' ' || byte == b'\t'))
}
