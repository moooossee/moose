use std::collections::HashMap;

#[derive(Clone)]
pub(super) struct Formula {
    pub(super) source: String,
    pub(super) display: bool,
    original: String,
}

pub(super) struct Prepared {
    pub(super) markdown: String,
    pub(super) formulas: HashMap<String, Formula>,
}

impl Prepared {
    fn push_formula(&mut self, prefix: &str, source: &str, original: &str, display: bool) {
        let key = format!("{prefix}{}", self.formulas.len());
        self.formulas.insert(
            key.clone(),
            Formula {
                source: source.to_string(),
                display,
                original: original.to_string(),
            },
        );
        self.markdown.push_str("$`");
        self.markdown.push_str(&key);
        self.markdown.push_str("`$");
    }

    pub(super) fn restore(&self, text: &str) -> String {
        let mut restored = text.to_string();
        for (key, formula) in &self.formulas {
            restored = restored.replace(&format!("$`{key}`$"), &formula.original);
        }
        restored
    }
}

pub(super) fn prepare(content: &str) -> Prepared {
    let mut prefix = "moose-formula:".to_string();
    while content.contains(&prefix) {
        prefix.push(':');
    }
    let mut result = Prepared {
        markdown: String::new(),
        formulas: HashMap::new(),
    };
    let mut offset = 0;
    while offset < content.len() {
        let tail = &content[offset..];
        if tail.starts_with('`') || tail.starts_with("~~~") {
            let marker = tail.as_bytes()[0];
            let length = tail.bytes().take_while(|byte| *byte == marker).count();
            let fence = &tail[..length];
            let end = code_end(content, offset, fence).unwrap_or_else(|| {
                if is_block_fence(content, offset, fence) {
                    content.len()
                } else {
                    offset + length
                }
            });
            result.markdown.push_str(&content[offset..end]);
            offset = end;
            continue;
        }
        if let Some(length) = environment_end(tail) {
            let source = &tail[..length];
            result.push_formula(&prefix, source, source, true);
            offset += length;
            continue;
        }
        let delimiter = if tail.starts_with("\\[") {
            Some(("\\[", "\\]", true))
        } else if tail.starts_with("\\(") {
            Some(("\\(", "\\)", false))
        } else if tail.starts_with("$$") {
            Some(("$$", "$$", true))
        } else if tail.starts_with("$`") {
            Some(("$`", "`$", false))
        } else if tail.starts_with('$')
            && tail[1..]
                .chars()
                .next()
                .is_some_and(|ch| !ch.is_whitespace())
        {
            Some(("$", "$", false))
        } else {
            None
        };
        if let Some((open, close, display)) = delimiter {
            let start = offset + open.len();
            if let Some(end) = find_close(content, start, close, open == "$") {
                result.push_formula(
                    &prefix,
                    &content[start..end],
                    &content[offset..end + close.len()],
                    display,
                );
                offset = end + close.len();
                continue;
            }
        }
        let ch = tail.chars().next().unwrap_or_default();
        result.markdown.push(ch);
        offset += ch.len_utf8();
        if ch == '\\' {
            if let Some(next) = content[offset..].chars().next() {
                result.markdown.push(next);
                offset += next.len_utf8();
            }
        }
    }
    result
}

fn environment_end(source: &str) -> Option<usize> {
    let name = source.strip_prefix("\\begin{")?.split('}').next()?;
    if !matches!(
        name.trim_end_matches('*'),
        "equation"
            | "align"
            | "aligned"
            | "gather"
            | "gathered"
            | "multline"
            | "displaymath"
            | "math"
            | "split"
            | "matrix"
            | "pmatrix"
            | "bmatrix"
            | "Bmatrix"
            | "vmatrix"
            | "Vmatrix"
            | "smallmatrix"
            | "cases"
            | "array"
    ) {
        return None;
    }
    let open = format!("\\begin{{{name}}}");
    let close = format!("\\end{{{name}}}");
    let mut depth = 1usize;
    let mut offset = open.len();
    while offset < source.len() {
        let tail = &source[offset..];
        let end = tail.find(&close)?;
        if let Some(start) = tail.find(&open).filter(|start| *start < end) {
            depth += 1;
            offset += start + open.len();
        } else {
            depth -= 1;
            offset += end + close.len();
            if depth == 0 {
                return Some(offset);
            }
        }
    }
    None
}

fn find_close(content: &str, start: usize, close: &str, dollar: bool) -> Option<usize> {
    let mut offset = start;
    while offset < content.len() {
        let tail = &content[offset..];
        if dollar && tail.starts_with('\n') {
            return None;
        }
        if tail.starts_with(close) && offset > start {
            let valid = !dollar
                || (content[..offset]
                    .chars()
                    .next_back()
                    .is_some_and(|ch| !ch.is_whitespace())
                    && !content[offset + close.len()..]
                        .chars()
                        .next()
                        .is_some_and(|ch| ch.is_ascii_digit()));
            if valid {
                return Some(offset);
            }
        }
        let ch = tail.chars().next()?;
        offset += ch.len_utf8();
        if ch == '\\' {
            if let Some(next) = content[offset..].chars().next() {
                offset += next.len_utf8();
            }
        }
    }
    None
}

fn code_end(content: &str, offset: usize, fence: &str) -> Option<usize> {
    let start = offset + fence.len();
    if is_block_fence(content, offset, fence) {
        let mut cursor = start;
        for line in content[start..].split_inclusive('\n') {
            if cursor > start && line.trim_start().starts_with(fence) {
                let trimmed = line.trim();
                if trimmed.bytes().all(|byte| byte == fence.as_bytes()[0]) {
                    return Some(cursor + line.len());
                }
            }
            cursor += line.len();
        }
        return None;
    }
    let mut cursor = start;
    while let Some(found) = content[cursor..].find(fence) {
        let at = cursor + found;
        let run = content[at..]
            .bytes()
            .take_while(|byte| *byte == fence.as_bytes()[0])
            .count();
        if run == fence.len() {
            return Some(at + run);
        }
        cursor = at + run;
    }
    None
}

fn is_block_fence(content: &str, offset: usize, fence: &str) -> bool {
    fence.len() >= 3
        && content[..offset]
            .rsplit('\n')
            .next()
            .is_some_and(|line| line.trim().is_empty())
}
