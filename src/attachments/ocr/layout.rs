use std::borrow::Cow;

pub(super) enum RegionKind<'a> {
    Field,
    Number(&'a str),
    Line(&'a str),
}

#[derive(Clone, Copy)]
pub(super) struct MergeOptions {
    pub offset: (usize, usize),
    pub final_pass: bool,
    pub confirm_isolated: bool,
}

pub(super) struct Merged<'a> {
    pub text: String,
    pub confidence: f64,
    pub repaired_lines: Vec<&'a str>,
}

#[derive(Default)]
pub(super) struct Layout(Vec<Line>);

#[derive(Clone)]
struct Line {
    top: f64,
    bottom: f64,
    key: String,
    evidence: usize,
    confidence: f64,
    spans: Vec<Span>,
    has_text: bool,
}

#[derive(Clone)]
struct Span {
    left: f64,
    right: f64,
    top: f64,
    bottom: f64,
    confidence: f64,
}

impl Layout {
    pub(super) fn has_split_rows(&self) -> bool {
        let mut lines: Vec<_> = self.0.iter().filter(|line| line.has_text).collect();
        if lines.len() < 4 {
            return false;
        }
        lines.sort_unstable_by(|a, b| a.top.total_cmp(&b.top));
        lines
            .windows(2)
            .filter(|pair| {
                let (a, b) = (pair[0], pair[1]);
                let overlap = a.bottom.min(b.bottom) - b.top;
                overlap * 2.0 >= (a.bottom - a.top).min(b.bottom - b.top)
                    && a.beside(b, 0.0).is_some()
            })
            .take(2)
            .count()
            == 2
    }

    pub(super) fn read(tsv: &str) -> Self {
        let mut layout = Self::default();
        for row in tsv.lines().filter_map(columns) {
            match row[0] {
                "4" => layout.line(row[7], row[9]),
                "5" => layout.word(row[11], 100.0, [row[6], row[7], row[8], row[9]]),
                _ => {}
            }
        }
        layout
    }

    pub(super) fn line(&mut self, top: &str, height: &str) {
        let (Ok(top), Ok(height)) = (top.parse::<f64>(), height.parse::<f64>()) else {
            return;
        };
        if top.is_finite() && height.is_finite() && height > 0.0 {
            self.0.push(Line {
                top,
                bottom: top + height,
                key: String::new(),
                evidence: 0,
                confidence: 0.0,
                spans: Vec::new(),
                has_text: false,
            });
        }
    }

    pub(super) fn word(&mut self, text: &str, confidence: f64, bounds: [&str; 4]) {
        if let Some(line) = self.0.last_mut() {
            line.has_text |= !text.trim().is_empty();
            let length = text.chars().filter(|ch| ch.is_alphanumeric()).count();
            line.key.extend(key(text));
            line.evidence += length;
            line.confidence += length as f64 * confidence;
            if let [Ok(left), Ok(top), Ok(width), Ok(height)] = bounds.map(str::parse::<f64>)
                && [left, top, width, height].into_iter().all(f64::is_finite)
                && width > 0.0
                && height > 0.0
            {
                line.spans.push(Span {
                    left,
                    right: left + width,
                    top,
                    bottom: top + height,
                    confidence,
                });
            }
        }
    }

    pub(super) fn follows_rotation(&self, rotation: i32) -> bool {
        let (mut matching, mut other) = (0, 0);
        for line in &self.0 {
            if !line.key.chars().any(|ch| ch.is_ascii_alphabetic()) {
                continue;
            }
            for pair in line.spans.windows(2) {
                let [a, b] = pair else { continue };
                let horizontal = a.left + a.right - b.left - b.right;
                let vertical = a.top + a.bottom - b.top - b.bottom;
                let direction = if horizontal.abs() >= vertical.abs() {
                    if horizontal > 0.0 { 180 } else { 0 }
                } else if vertical > 0.0 {
                    270
                } else {
                    90
                };
                if direction == rotation {
                    matching += 1;
                } else {
                    other += 1;
                }
            }
        }
        matching > other
    }

    pub(super) fn unrotate(&mut self, rotation: i32, [width, height]: [u32; 2]) {
        if rotation == 0 {
            return;
        }
        let (width, height) = (f64::from(width), f64::from(height));
        for line in &mut self.0 {
            if line.spans.is_empty() {
                continue;
            }
            let (mut top, mut bottom) = (f64::INFINITY, f64::NEG_INFINITY);
            for span in &mut line.spans {
                let (x, y, right, end) = (span.left, span.top, span.right, span.bottom);
                [span.left, span.top, span.right, span.bottom] = match rotation {
                    90 => [y, width - right, end, width - x],
                    180 => [width - right, height - end, width - x, height - y],
                    270 => [height - end, x, height - y, right],
                    _ => return,
                };
                top = top.min(span.top);
                bottom = bottom.max(span.bottom);
            }
            line.top = top;
            line.bottom = bottom;
        }
    }

    pub(super) fn column_order(&self, text: &str) -> Option<String> {
        if !could_contain_columns(self.0.iter()) {
            return None;
        }
        let lines = self.aligned(text)?;
        let top_to_bottom = lines.windows(2).all(|pair| pair[0].0.top <= pair[1].0.top);
        let sections = lines.chunk_by(|(previous, _), (next, _)| {
            !top_to_bottom
                || next.top - previous.bottom
                    <= 2.5 * (previous.bottom - previous.top).max(next.bottom - next.top)
        });
        let mut output = String::new();
        let mut end = 0;
        for section in sections {
            if let Some(ordered) = ordered_columns(section, section.len() != lines.len()) {
                let first = section.first()?.1.as_ptr().addr() - text.as_ptr().addr();
                let last = section.last()?.1;
                output.push_str(&text[end..first]);
                output.push_str(&ordered);
                end = last.as_ptr().addr() - text.as_ptr().addr() + last.len();
            }
        }
        (end != 0).then(|| {
            output.push_str(&text[end..]);
            output
        })
    }

    pub(super) fn has_unmerged_text(
        &self,
        text: &str,
        native: &Self,
        merged: &str,
        offset: (usize, usize),
        repaired_lines: &[&str],
    ) -> bool {
        let Some(lines) = self.aligned(text) else {
            return false;
        };
        let kept: std::collections::HashSet<_> =
            merged.split(['\n', '\t']).map(str::trim).collect();
        let mut source: Vec<_> = native.0.iter().filter(|line| line.has_text).collect();
        source.sort_by(|a, b| a.top.total_cmp(&b.top));
        let max_height = source
            .iter()
            .map(|line| line.bottom - line.top)
            .fold(0.0, f64::max);
        let (x, y) = (offset.0 as f64, offset.1 as f64);
        lines.into_iter().any(|(line, text)| {
            if line.evidence < 8
                || line.confidence < line.evidence as f64 * super::CLEAR_CONFIDENCE
                || repaired_lines.contains(&text)
                || kept.contains(text.trim())
            {
                return false;
            }
            let (top, bottom) = (line.top + y, line.bottom + y);
            let start = source.partition_point(|line| line.top + max_height <= top);
            source[start..]
                .iter()
                .take_while(|line| line.top < bottom)
                .all(|other| other.bottom <= top || line.beside(other, x).is_some())
        })
    }

    pub(super) fn isolated_line_position(
        &self,
        native: &str,
        [top, bottom]: [f64; 2],
    ) -> Option<usize> {
        let coalesced;
        let rows = match self.aligned(native) {
            Some(rows) => rows,
            None => {
                coalesced = self.physical_rows()?;
                coalesced.aligned(native)?
            }
        };
        if rows.is_empty()
            || !rows
                .windows(2)
                .all(|pair| pair[0].0.bottom <= pair[1].0.top)
            || rows
                .iter()
                .any(|(line, _)| line.top < bottom && top < line.bottom)
        {
            return None;
        }
        Some(
            rows.iter()
                .find(|(line, _)| line.top >= bottom)
                .map_or(native.len(), |(_, line)| {
                    line.as_ptr().addr() - native.as_ptr().addr()
                }),
        )
    }

    pub(super) fn merge<'text>(
        &self,
        text: &'text str,
        native: &Self,
        native_text: &str,
        options: MergeOptions,
        mut recover: impl FnMut([u32; 4], RegionKind<'_>) -> Option<(String, f64)>,
        discover: impl FnOnce(&[(usize, [usize; 4])]) -> Vec<(usize, String, bool, f64)>,
    ) -> Option<Merged<'text>> {
        let offset = options.offset;
        let coalesced;
        let native = match native.aligned(native_text) {
            Some(lines) => lines,
            None => {
                coalesced = native.physical_rows()?;
                coalesced.aligned(native_text)?
            }
        };
        if native.is_empty()
            || !native
                .windows(2)
                .all(|pair| pair[0].0.bottom <= pair[1].0.top)
        {
            return None;
        }

        let mut additions = Vec::new();
        let mut fields = vec![None; native.len()];
        let mut matched_labels = vec![false; native.len()];
        let mut evidence = 0;
        let mut confidence = 0.0;
        let mut field_slots = 0;
        let mut last_short_slot = None;
        let mut anchored = false;
        let mut isolated_number = None;
        for (line, text) in self.aligned(text)? {
            let top = line.top + offset.1 as f64;
            let bottom = line.bottom + offset.1 as f64;
            let slot = native.partition_point(|(native, _)| native.bottom <= top);
            if let Some(&(bounds, label)) = native.get(slot)
                && bounds.top < bottom
            {
                let center = (top + bottom) / 2.0;
                if text.split_whitespace().eq(label.split_whitespace())
                    && (bounds.top..bounds.bottom).contains(&center)
                    && let (Some(word), Some(source)) = (line.spans.first(), bounds.spans.first())
                    && word.left + (offset.0 as f64) < source.right
                    && word.right + offset.0 as f64 > source.left
                {
                    matched_labels[slot] = true;
                }
                if (bounds.top..bounds.bottom).contains(&center)
                    && native
                        .get(slot + 1)
                        .is_none_or(|(next, _)| next.top >= bottom)
                    && let Some((value, before, field)) =
                        line.adjacent_field(text, bounds, label, offset.0 as f64)
                {
                    let first = field.first()?;
                    let last = field.last()?;
                    let region = [
                        (first.left + offset.0 as f64) as u32,
                        top as u32,
                        (last.right - first.left).ceil() as u32,
                        (bottom - top).ceil() as u32,
                    ];
                    let reread = field.len() > 1 || first.confidence < super::CLEAR_CONFIDENCE;
                    let (value, certainty) = if !reread {
                        (value.to_owned(), first.confidence)
                    } else {
                        let Some(field) = recover(region, RegionKind::Field) else {
                            continue;
                        };
                        field
                    };
                    let weight = super::alphanumeric_count(&value);
                    if fields[slot].replace((value, before)).is_some() {
                        return None;
                    }
                    evidence += weight;
                    confidence += weight as f64 * certainty;
                    field_slots += 1;
                    anchored |= line.spans.len() > 1 || reread;
                    if !anchored
                        && fields[slot].as_ref().is_some_and(|(value, _)| {
                            value.chars().filter(|ch| ch.is_numeric()).count() >= 2
                        })
                    {
                        isolated_number = Some((slot, region));
                    }
                } else if (bounds.top..bounds.bottom).contains(&center)
                    && native
                        .get(slot + 1)
                        .is_none_or(|(next, _)| next.top >= bottom)
                    && fields[slot].is_none()
                    && line.evidence >= 12
                    && line.confidence >= line.evidence as f64 * super::CLEAR_CONFIDENCE
                    && let Some(before) = line.beside(bounds, offset.0 as f64)
                {
                    fields[slot] = Some((text.to_owned(), before));
                    evidence += line.evidence;
                    confidence += line.confidence;
                    anchored = true;
                }
                continue;
            }
            if line.evidence < 4 {
                if line.spans.len() != 1
                    || line.spans[0].confidence < super::CLEAR_CONFIDENCE
                    || !text.chars().any(char::is_numeric)
                {
                    continue;
                }
                if last_short_slot != Some(slot) {
                    field_slots += 1;
                    last_short_slot = Some(slot);
                }
            } else if line.confidence < line.evidence as f64 * super::MIN_LINE_CONFIDENCE {
                continue;
            }
            if additions
                .last()
                .is_some_and(|(previous, _, _)| *previous > slot)
            {
                return None;
            }
            additions.push((slot, line, text));
            evidence += line.evidence;
            confidence += line.confidence;
        }
        let mut missing = Vec::new();
        for (slot, &(line, _)) in native.iter().enumerate() {
            if !matched_labels[slot] || fields[slot].is_some() {
                continue;
            }
            let margin = (line.bottom - line.top) / 2.0;
            if slot > 0 && native[slot - 1].0.bottom > line.top - margin
                || native
                    .get(slot + 1)
                    .is_some_and(|(next, _)| next.top < line.bottom + margin)
            {
                continue;
            }
            let (Some(first), Some(last)) = (line.spans.first(), line.spans.last()) else {
                continue;
            };
            missing.push((
                slot,
                [
                    first.left.round() as usize,
                    line.top.round() as usize,
                    last.right.round() as usize,
                    line.bottom.round() as usize,
                ],
            ));
        }
        for (slot, value, before, certainty) in discover(&missing) {
            let weight = super::alphanumeric_count(&value);
            evidence += weight;
            confidence += weight as f64 * certainty;
            fields[slot] = Some((value, before));
            anchored = true;
        }
        if options.confirm_isolated
            && !anchored
            && let [(_, line, text)] = additions.as_slice()
        {
            let bounds = line.region(offset)?;
            recover(bounds, RegionKind::Line(text))?;
            anchored = true;
        }
        if evidence < 12 && !anchored && field_slots < 2 {
            if let Some((slot, bounds)) = isolated_number {
                let (confirmed, _) = recover(bounds, RegionKind::Field)?;
                if fields[slot].as_ref()?.0 != confirmed {
                    return None;
                }
            } else if let [(_, line, text)] = additions.as_slice() {
                let bounds = line.region(offset)?;
                recover(bounds, RegionKind::Line(text))?;
            } else {
                return None;
            }
        }

        let mut output = String::with_capacity(native_text.len() + text.len());
        let mut repaired_lines = Vec::new();
        let mut additions = additions
            .into_iter()
            .map(|(slot, line, text)| {
                let (updated, gain) = line.repair_numbers(text, offset, &mut recover);
                if gain > 0.0 {
                    repaired_lines.push(text);
                }
                confidence += gain;
                (slot, updated)
            })
            .peekable();
        let mut slot = 0;
        for line in native_text.lines() {
            if line.trim().is_empty() {
                output.push_str(line);
            } else {
                append_at(&mut output, &mut additions, slot);
                if let Some((value, true)) = &fields[slot] {
                    output.push_str(value);
                    output.push('\t');
                }
                output.push_str(line);
                if let Some((value, false)) = &fields[slot] {
                    output.push('\t');
                    output.push_str(value);
                }
                slot += 1;
            }
            output.push('\n');
        }
        append_at(&mut output, &mut additions, slot);
        drop(additions);
        Some(Merged {
            text: output,
            confidence: confidence / evidence as f64,
            repaired_lines,
        })
    }

    fn aligned<'a, 'text>(&'a self, text: &'text str) -> Option<Vec<(&'a Line, &'text str)>> {
        let mut lines = text.lines().filter(|line| !line.trim().is_empty());
        let mut aligned = Vec::new();
        for bounds in self.0.iter().filter(|line| line.has_text) {
            let line = lines.next()?;
            if !key(line).eq(bounds.key.chars()) {
                return None;
            }
            aligned.push((bounds, line));
        }
        lines.next().is_none().then_some(aligned)
    }

    fn physical_rows(&self) -> Option<Self> {
        let mut sorted: Vec<_> = self.0.iter().filter(|line| line.has_text).collect();
        sorted.sort_by(|a, b| a.top.total_cmp(&b.top));
        let mut rows = Vec::new();
        for band in sorted.chunk_by(|a, b| {
            let overlap = a.bottom.min(b.bottom) - a.top.max(b.top);
            overlap * 2.0 >= (a.bottom - a.top).min(b.bottom - b.top)
        }) {
            let mut fragments = band.to_vec();
            fragments.sort_by(|a, b| {
                let left = |line: &Line| line.spans.first().map_or(f64::INFINITY, |span| span.left);
                left(a).total_cmp(&left(b))
            });
            let (first, rest) = fragments.split_first()?;
            let mut row = (**first).clone();
            for fragment in rest {
                if row.beside(fragment, 0.0) != Some(true) {
                    return None;
                }
                row.top = row.top.min(fragment.top);
                row.bottom = row.bottom.max(fragment.bottom);
                row.key.push_str(&fragment.key);
                row.evidence += fragment.evidence;
                row.confidence += fragment.confidence;
                row.spans.extend_from_slice(&fragment.spans);
            }
            rows.push(row);
        }
        Some(Self(rows))
    }
}

impl Line {
    fn region(&self, (x, y): (usize, usize)) -> Option<[u32; 4]> {
        let mut left = f64::INFINITY;
        let mut right = f64::NEG_INFINITY;
        for span in &self.spans {
            left = left.min(span.left);
            right = right.max(span.right);
        }
        if right <= left || right - left > 2048.0 || self.bottom - self.top > 512.0 {
            return None;
        }
        Some([
            (left.floor() as u32).saturating_add(x as u32),
            (self.top.floor() as u32).saturating_add(y as u32),
            (right.ceil() - left.floor()) as u32,
            (self.bottom.ceil() - self.top.floor()) as u32,
        ])
    }

    fn repair_numbers<'a>(
        &self,
        text: &'a str,
        (x, y): (usize, usize),
        recover: &mut impl FnMut([u32; 4], RegionKind<'_>) -> Option<(String, f64)>,
    ) -> (Cow<'a, str>, f64) {
        let words = text.split_whitespace();
        if words.clone().count() != self.spans.len() {
            return (Cow::Borrowed(text), 0.0);
        }
        let mut output = String::new();
        let mut cursor = 0;
        let mut gain = 0.0;
        for (word, span) in words.zip(&self.spans) {
            if span.confidence >= super::CLEAR_CONFIDENCE
                || !(3..=32).contains(&word.len())
                || !word.bytes().all(|digit| digit.is_ascii_digit())
            {
                continue;
            }
            let region = [
                (span.left.floor() as u32).saturating_add(x as u32),
                (span.top.floor() as u32).saturating_add(y as u32),
                (span.right.ceil() - span.left.floor()) as u32,
                (span.bottom.ceil() - span.top.floor()) as u32,
            ];
            if let Some((replacement, score)) = recover(region, RegionKind::Number(word))
                && score > span.confidence
            {
                let position = word.as_ptr().addr() - text.as_ptr().addr();
                output.push_str(&text[cursor..position]);
                output.push_str(&replacement);
                cursor = position + word.len();
                gain += (score - span.confidence) * word.len() as f64;
            }
        }
        if cursor == 0 {
            (Cow::Borrowed(text), 0.0)
        } else {
            output.push_str(&text[cursor..]);
            (Cow::Owned(output), gain)
        }
    }

    fn beside(&self, other: &Line, offset: f64) -> Option<bool> {
        let first = self.spans.first()?;
        let last = self.spans.last()?;
        let other_first = other.spans.first()?;
        let other_last = other.spans.last()?;
        if ![self, other].iter().all(|line| {
            line.spans
                .windows(2)
                .all(|pair| pair[0].left <= pair[1].left)
        }) {
            return None;
        }
        if last.right + offset <= other_first.left {
            Some(true)
        } else if first.left + offset >= other_last.right {
            Some(false)
        } else {
            None
        }
    }

    fn adjacent_field<'a>(
        &self,
        text: &'a str,
        native: &Line,
        label: &str,
        offset: f64,
    ) -> Option<(&'a str, bool, &[Span])> {
        let left = native.spans.first()?.left;
        let right = native.spans.last()?.right;
        if ![self, native].iter().all(|line| {
            line.spans
                .windows(2)
                .all(|pair| pair[0].right <= pair[1].left)
        }) || self.spans.len() != text.split_whitespace().count()
        {
            return None;
        }
        let before_end = self
            .spans
            .partition_point(|span| span.right + offset <= left);
        let after_start = self
            .spans
            .partition_point(|span| span.left + offset < right);
        let (field, anchors, value, label_read, before) = match (before_end, after_start) {
            (0, after) if after < self.spans.len() => {
                let split = text.split_whitespace().nth(after)?;
                let index = split.as_ptr().addr() - text.as_ptr().addr();
                (
                    &self.spans[after..],
                    &self.spans[..after],
                    &text[index..],
                    &text[..index],
                    false,
                )
            }
            (before, after) if before != 0 && after == self.spans.len() => {
                let split = text.split_whitespace().nth(before);
                let index = split.map_or(text.len(), |word| {
                    word.as_ptr().addr() - text.as_ptr().addr()
                });
                (
                    &self.spans[..before],
                    &self.spans[before..],
                    text[..index].trim_end(),
                    &text[index..],
                    true,
                )
            }
            _ => return None,
        };
        if !value.chars().any(char::is_numeric) || field.len() > 16 {
            return None;
        }
        if anchors.is_empty() {
            return (field.len() == 1).then_some((value, before, field));
        }
        (anchors.len() == native.spans.len()
            && label_read.split_whitespace().eq(label.split_whitespace())
            && anchors.iter().zip(&native.spans).all(|(ocr, source)| {
                ocr.left + offset < source.right && source.left < ocr.right + offset
            }))
        .then_some((value, before, field))
    }
}

fn could_contain_columns<'a>(lines: impl Iterator<Item = &'a Line>) -> bool {
    lines.filter(|line| line.spans.len() >= 8).nth(2).is_some()
}

fn ordered_columns(lines: &[(&Line, &str)], require_continuation: bool) -> Option<String> {
    if !could_contain_columns(lines.iter().map(|&(line, _)| line)) {
        return None;
    }
    let mut spans: Vec<_> = lines.iter().flat_map(|(line, _)| &line.spans).collect();
    spans.sort_unstable_by(|a, b| a.left.total_cmp(&b.left));
    let first = spans.first()?.left;
    let mut right = first;
    let mut widest = (0.0, 0.0);
    for span in spans {
        if span.left - right > widest.0 {
            widest = (span.left - right, (span.left + right) / 2.0);
        }
        right = right.max(span.right);
    }
    let (gap, boundary) = widest;
    let mut heights: Vec<_> = lines
        .iter()
        .map(|(line, _)| line.bottom - line.top)
        .collect();
    let middle = heights.len() / 2;
    let height = *heights.select_nth_unstable_by(middle, f64::total_cmp).1;
    if gap < (0.04 * (right - first)).max(2.5 * height) {
        return None;
    }
    let mut left_column = Vec::new();
    let mut right_column = Vec::new();
    let mut aligned_rows = 0;
    let mut continuation = false;
    for &(line, text) in lines {
        if !line
            .spans
            .windows(2)
            .all(|pair| pair[0].left <= pair[1].left)
            || line.spans.len() != text.split_whitespace().count()
        {
            return None;
        }
        let split = line.spans.partition_point(|span| span.left < boundary);
        if split == 0 {
            right_column.push(text);
            continuation |= aligned_rows != 0;
        } else if split == line.spans.len() {
            left_column.push(text);
            continuation |= aligned_rows != 0;
        } else {
            let start = text.split_whitespace().nth(split)?;
            let offset = start.as_ptr().addr() - text.as_ptr().addr();
            left_column.push(text[..offset].trim_end());
            right_column.push(&text[offset..]);
            aligned_rows += usize::from(split >= 4 && line.spans.len() - split >= 4);
        }
    }
    (aligned_rows >= 3 && (!require_continuation || continuation))
        .then(|| format!("{}\n\n{}", left_column.join("\n"), right_column.join("\n")))
}

fn append_at<'a>(
    output: &mut String,
    additions: &mut std::iter::Peekable<impl Iterator<Item = (usize, Cow<'a, str>)>>,
    slot: usize,
) {
    while let Some((_, text)) = additions.next_if(|(position, _)| *position == slot) {
        output.push_str(&text);
        output.push('\n');
    }
}

fn key(text: &str) -> impl Iterator<Item = char> + '_ {
    text.chars().filter(|ch| !ch.is_whitespace())
}

pub(super) fn columns(row: &str) -> Option<[&str; 12]> {
    let mut fields = row.splitn(12, '\t');
    let mut columns = [""; 12];
    for column in &mut columns {
        *column = fields.next()?;
    }
    Some(columns)
}

pub(super) fn space_symbols(text: &str, boxes: &str) -> Option<String> {
    let mut symbols = text.char_indices().filter(|(_, ch)| !ch.is_whitespace());
    let mut output = String::with_capacity(text.len());
    let mut previous: Option<(i64, i64)> = None;
    let mut cursor = 0;
    for line in boxes.lines() {
        let mut columns = line.split_whitespace();
        let (position, symbol) = symbols.next()?;
        let mut value = columns.next()?.chars();
        if value.next() != Some(symbol) || value.next().is_some() {
            return None;
        }
        let [left, bottom, right, top] =
            std::array::from_fn(|_| columns.next().and_then(|n| n.parse::<i32>().ok()));
        let [Some(left), Some(bottom), Some(right), Some(top)] = [left, bottom, right, top] else {
            return None;
        };
        if right <= left || top <= bottom || columns.next()? != "0" {
            return None;
        }
        let height = i64::from(top) - i64::from(bottom);
        output.push_str(&text[cursor..position]);
        if cursor == position
            && let Some((end, previous_height)) = previous
        {
            let gap = i64::from(left) - end;
            if 20 * gap >= 7 * height.max(previous_height) {
                output.push(' ');
            }
        }
        output.push(symbol);
        previous = Some((i64::from(right), height));
        cursor = position + symbol.len_utf8();
    }
    if symbols.next().is_some() {
        return None;
    }
    output.push_str(&text[cursor..]);
    (output != text).then_some(output)
}
