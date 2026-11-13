use crate::i18n::gettext;

use comrak::{
    Arena, Options,
    nodes::{AstNode, NodeValue},
    parse_document,
};

use super::MAX_CONTENT_BYTES;

#[derive(Clone, Debug)]
pub enum Block {
    Heading(u8, String),
    Paragraph(String),
    Bullet(String),
    Quote(String),
    Code(String),
    Table(Table),
    Rule,
}

#[derive(Clone, Debug)]
pub struct Table {
    pub name: String,
    pub rows: Vec<Vec<String>>,
}

#[derive(Clone, Debug)]
pub struct Content {
    pub title: String,
    pub source: String,
    pub blocks: Vec<Block>,
}

impl Content {
    pub fn parse(title: &str, source: &str) -> Result<Self, String> {
        if source.trim().is_empty() || source.len() > MAX_CONTENT_BYTES {
            return Err(gettext(
                "Use a non-empty document with up to 512 KB of text.",
            ));
        }
        if source
            .chars()
            .any(|ch| ch == '\0' || (ch.is_control() && !matches!(ch, '\n' | '\r' | '\t')))
        {
            return Err(gettext(
                "The document contains unsupported control characters.",
            ));
        }
        let source = unwrap_fence(source);
        let arena = Arena::new();
        let mut options = Options::default();
        options.extension.table = true;
        options.extension.strikethrough = true;
        options.extension.tasklist = true;
        let root = parse_document(&arena, &source, &options);
        let mut blocks = Vec::new();
        let mut table_name = gettext("Sheet 1");
        for node in root.children() {
            append(node, &mut blocks, &mut table_name);
        }
        if blocks.is_empty() {
            blocks.push(Block::Paragraph(source.clone()));
        }
        if blocks.len() > 10_000 {
            return Err(gettext("The document contains too many sections."));
        }
        for block in &blocks {
            if let Block::Table(table) = block
                && (table.rows.len() > 20_000
                    || table.rows.iter().any(|row| {
                        row.len() > 100
                            || row.iter().any(|cell| cell.encode_utf16().count() > 32_767)
                    }))
            {
                return Err(gettext(
                    "Use tables with up to 100 columns, 20,000 rows, and 32,767 characters per cell.",
                ));
            }
        }
        let inferred = blocks.iter().find_map(|block| match block {
            Block::Heading(1, value) => Some(value.as_str()),
            _ => None,
        });
        let title = if title.trim().is_empty() || title == "Untitled Document" {
            inferred
                .map(str::to_string)
                .unwrap_or_else(|| gettext("Document"))
        } else {
            title.to_string()
        };
        Ok(Self {
            title: title.chars().take(120).collect(),
            source,
            blocks,
        })
    }

    pub fn tables(&self) -> Vec<Table> {
        self.blocks
            .iter()
            .filter_map(|block| match block {
                Block::Table(table) => Some(table.clone()),
                _ => None,
            })
            .collect()
    }

    pub fn plain_text(&self) -> String {
        self.blocks
            .iter()
            .map(|block| match block {
                Block::Heading(_, text)
                | Block::Paragraph(text)
                | Block::Quote(text)
                | Block::Code(text) => text.clone(),
                Block::Bullet(text) => format!("• {text}"),
                Block::Table(table) => table
                    .rows
                    .iter()
                    .map(|row| row.join("\t"))
                    .collect::<Vec<_>>()
                    .join("\n"),
                Block::Rule => "────────────────".into(),
            })
            .collect::<Vec<_>>()
            .join("\n\n")
    }

    pub fn slides(&self) -> Vec<(String, Vec<String>)> {
        let mut slides = Vec::new();
        let mut title = self.title.clone();
        let mut lines = Vec::new();
        for block in &self.blocks {
            match block {
                Block::Heading(level, value) if *level <= 2 => {
                    if !lines.is_empty() || (!slides.is_empty() && !title.is_empty()) {
                        slides.push((std::mem::take(&mut title), std::mem::take(&mut lines)));
                    }
                    title = value.clone();
                }
                Block::Paragraph(text)
                | Block::Quote(text)
                | Block::Code(text)
                | Block::Heading(_, text) => lines.push(text.clone()),
                Block::Bullet(text) => lines.push(format!("• {text}")),
                Block::Table(table) => lines.extend(table.rows.iter().map(|row| row.join("  ·  "))),
                Block::Rule => {}
            }
        }
        if !title.is_empty() || !lines.is_empty() {
            slides.push((title, lines));
        }
        slides
    }
}

fn append<'a>(node: &'a AstNode<'a>, blocks: &mut Vec<Block>, table_name: &mut String) {
    match &node.data().value {
        NodeValue::Heading(heading) => {
            let value = text(node);
            *table_name = value.clone();
            blocks.push(Block::Heading(heading.level, value));
        }
        NodeValue::Paragraph => blocks.push(Block::Paragraph(text(node))),
        NodeValue::Item(_) | NodeValue::TaskItem(_) => blocks.push(Block::Bullet(text(node))),
        NodeValue::BlockQuote => blocks.push(Block::Quote(text(node))),
        NodeValue::CodeBlock(code) => blocks.push(Block::Code(code.literal.trim_end().to_string())),
        NodeValue::ThematicBreak => blocks.push(Block::Rule),
        NodeValue::Table(_) => {
            let rows = node
                .children()
                .map(|row| row.children().map(text).collect::<Vec<_>>())
                .collect::<Vec<_>>();
            blocks.push(Block::Table(Table {
                name: table_name.clone(),
                rows,
            }));
        }
        NodeValue::HtmlBlock(block) => blocks.push(Block::Code(block.literal.clone())),
        _ => {
            for child in node.children() {
                append(child, blocks, table_name);
            }
        }
    }
}

fn text<'a>(node: &'a AstNode<'a>) -> String {
    let mut value = String::new();
    append_text(node, &mut value);
    value.trim().to_string()
}

fn append_text<'a>(node: &'a AstNode<'a>, value: &mut String) {
    match &node.data().value {
        NodeValue::Text(text) => value.push_str(text),
        NodeValue::Code(code) => value.push_str(&code.literal),
        NodeValue::CodeBlock(code) => value.push_str(&code.literal),
        NodeValue::SoftBreak | NodeValue::LineBreak => value.push('\n'),
        NodeValue::Link(link) => {
            let start = value.len();
            for child in node.children() {
                append_text(child, value);
            }
            if (link.url.starts_with("https://")
                || link.url.starts_with("http://")
                || link.url.starts_with("mailto:"))
                && &value[start..] != link.url.as_str()
                && &value[start..] != link.url.trim_start_matches("mailto:")
            {
                value.push_str(&format!(" ({})", link.url));
            }
        }
        _ => {
            for child in node.children() {
                append_text(child, value);
            }
            if matches!(&node.data().value, NodeValue::Paragraph) && !value.ends_with('\n') {
                value.push('\n');
            }
        }
    }
}

pub fn unwrap_fence(source: &str) -> String {
    let trimmed = source.trim();
    if let Some((first, rest)) = trimmed.split_once('\n')
        && first.starts_with("```")
        && matches!(
            first
                .trim_start_matches('`')
                .trim()
                .to_ascii_lowercase()
                .as_str(),
            "" | "markdown" | "md" | "text" | "csv" | "json" | "xml" | "html"
        )
        && let Some(body) = rest.strip_suffix("```")
    {
        return body.trim().to_string();
    }
    trimmed.to_string()
}

pub fn xml(value: &str) -> String {
    value
        .chars()
        .filter(|ch| {
            matches!(*ch, '\n' | '\r' | '\t')
                || (*ch >= '\u{20}' && *ch != '\u{fffe}' && *ch != '\u{ffff}')
        })
        .map(|ch| match ch {
            '&' => "&amp;".into(),
            '<' => "&lt;".into(),
            '>' => "&gt;".into(),
            '"' => "&quot;".into(),
            '\'' => "&apos;".into(),
            _ => ch.to_string(),
        })
        .collect()
}

pub fn numeric(value: &str) -> Option<f64> {
    let trimmed = value.trim();
    if trimmed.is_empty()
        || (trimmed.len() > 1 && trimmed.starts_with('0') && !trimmed.starts_with("0."))
        || trimmed.chars().filter(|ch| ch.is_ascii_digit()).count() > 15
    {
        return None;
    }
    trimmed
        .parse::<f64>()
        .ok()
        .filter(|value| value.is_finite())
}

pub fn formula(value: &str) -> Option<&str> {
    let value = value.trim().strip_prefix('=')?;
    if value.is_empty() || value.len() > 2048 || value.contains(['[', ']', '!', '|', '\\']) {
        return None;
    }
    let upper = value.to_ascii_uppercase();
    if [
        "HYPERLINK",
        "WEBSERVICE",
        "FILTERXML",
        "RTD",
        "DDE",
        "CALL",
        "EXEC",
        "IMPORT",
        "IMAGE",
    ]
    .iter()
    .any(|name| upper.contains(name))
    {
        return None;
    }
    let allowed = [
        "SUM",
        "SUMIF",
        "SUMIFS",
        "AVERAGE",
        "AVERAGEIF",
        "MIN",
        "MAX",
        "COUNT",
        "COUNTA",
        "COUNTIF",
        "COUNTIFS",
        "IF",
        "IFERROR",
        "ROUND",
        "ROUNDUP",
        "ROUNDDOWN",
        "ABS",
        "AND",
        "OR",
        "NOT",
        "TODAY",
        "DATE",
        "DAY",
        "MONTH",
        "YEAR",
        "CONCAT",
        "CONCATENATE",
        "LEN",
        "TRIM",
        "LEFT",
        "RIGHT",
        "MID",
        "UPPER",
        "LOWER",
        "TEXT",
        "INDEX",
        "MATCH",
        "VLOOKUP",
        "HLOOKUP",
        "XLOOKUP",
        "SUBTOTAL",
        "PRODUCT",
        "SUMPRODUCT",
        "MEDIAN",
        "MOD",
        "POWER",
        "SQRT",
    ];
    let mut token = String::new();
    let mut quoted = false;
    let mut depth = 0usize;
    for ch in value.chars() {
        if ch == '"' {
            quoted = !quoted;
            token.clear();
            continue;
        }
        if quoted {
            continue;
        }
        if ch == '(' {
            depth += 1;
        } else if ch == ')' {
            depth = depth.checked_sub(1)?;
        }
        if ch.is_ascii_alphanumeric() || ch == '_' {
            token.push(ch);
        } else {
            if ch == '('
                && !token.is_empty()
                && !allowed.contains(&token.to_ascii_uppercase().as_str())
            {
                return None;
            }
            if !" .,:+-*/^%=()<>&$;".contains(ch) {
                return None;
            }
            if !ch.is_ascii_whitespace() {
                token.clear();
            }
        }
    }
    (!quoted && depth == 0).then_some(value)
}
