use crate::i18n::gettext;

use scraper::{Html, Selector};

const MAX_EXCERPT: usize = 8000;

pub fn clipped(value: &str, limit: usize) -> String {
    let end = value.floor_char_boundary(value.len().min(limit));
    value[..end].to_string()
}

pub fn clean(value: &str, limit: usize) -> String {
    clipped(
        &value.split_whitespace().collect::<Vec<_>>().join(" "),
        limit,
    )
}

pub fn snippet(value: &str, limit: usize) -> String {
    let document = Html::parse_fragment(value);
    clean(
        &document.root_element().text().collect::<Vec<_>>().join(" "),
        limit,
    )
}

pub fn page(bytes: &[u8], content_type: &str) -> Result<(String, String, bool), String> {
    let text = std::str::from_utf8(bytes)
        .map_err(|_| gettext("This page uses an unsupported text encoding. Try another source."))?;
    if content_type.starts_with("text/plain") {
        let text = text.trim();
        if text.is_empty() || text.contains('\0') {
            return Err(gettext(
                "This page does not contain readable text. Try another source.",
            ));
        }
        return Ok((
            String::new(),
            clipped(text, MAX_EXCERPT),
            text.len() > MAX_EXCERPT,
        ));
    }
    let mut document = Html::parse_document(text);
    let ignored = Selector::parse("script, style, noscript, template, svg, canvas, iframe, nav, footer, header, form, aside, [hidden], [aria-hidden='true']")
        .map_err(|_| gettext("Page extraction is unavailable."))?;
    let ids: Vec<_> = document
        .select(&ignored)
        .map(|element| element.id())
        .collect();
    for id in ids {
        if let Some(mut node) = document.tree.get_mut(id) {
            node.detach();
        }
    }
    let title = Selector::parse("title")
        .ok()
        .and_then(|selector| document.root_element().select(&selector).next())
        .map(|element| clean(&element.text().collect::<Vec<_>>().join(" "), 240))
        .unwrap_or_default();
    let root = ["main", "[role='main']", "article", "body"]
        .iter()
        .find_map(|query| {
            Selector::parse(query)
                .ok()
                .and_then(|selector| document.root_element().select(&selector).next())
        })
        .unwrap_or_else(|| document.root_element());
    let mut output = String::new();
    let mut truncated = false;
    for text in root.text() {
        let text = clean(text, MAX_EXCERPT + 1);
        if text.is_empty() {
            continue;
        }
        if output.len() + text.len() + 1 > MAX_EXCERPT {
            output.push_str(&clipped(&text, MAX_EXCERPT.saturating_sub(output.len())));
            truncated = true;
            break;
        }
        output.push_str(&text);
        output.push('\n');
    }
    if output.trim().len() < 40 {
        return Err(gettext(
            "This page has too little readable text. It may require JavaScript or sign-in. Try another source.",
        ));
    }
    Ok((title, output.trim().to_string(), truncated))
}
