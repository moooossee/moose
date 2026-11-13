use crate::i18n::gettext;

use super::{
    DocumentFormat,
    content::{Block, Content, Table, numeric, xml},
    office, opendocument, presentation,
};

pub struct RenderedDocument {
    pub title: String,
    pub bytes: Vec<u8>,
}

pub fn render(
    title: &str,
    source: &str,
    format: DocumentFormat,
) -> Result<RenderedDocument, String> {
    let content = Content::parse(title, source)?;
    let bytes = match format {
        DocumentFormat::Pdf => {
            return Err(gettext(
                "PDF documents must be rendered through the native print service.",
            ));
        }
        DocumentFormat::Docx => office::docx(&content)?,
        DocumentFormat::Xlsx => office::xlsx(&tables(&content)?)?,
        DocumentFormat::Pptx => presentation::pptx(&content)?,
        DocumentFormat::Odt | DocumentFormat::Odp => opendocument::render(&content, format)?,
        DocumentFormat::Ods => {
            let mut content = content.clone();
            if content.tables().is_empty() {
                content.blocks = tables(&content)?.into_iter().map(Block::Table).collect();
            }
            opendocument::render(&content, format)?
        }
        DocumentFormat::Markdown => content.source.as_bytes().to_vec(),
        DocumentFormat::Text => content.plain_text().into_bytes(),
        DocumentFormat::Csv => csv(&content)?,
        DocumentFormat::Html => html(&content).into_bytes(),
        DocumentFormat::Json => {
            let value: serde_json::Value = serde_json::from_str(&content.source).map_err(|_| gettext("The response is not valid JSON. Ask the model to correct the JSON and try again."))?;
            serde_json::to_vec_pretty(&value)
                .map_err(|_| gettext("The JSON document could not be formatted."))?
        }
        DocumentFormat::Xml => xml_document(&content).into_bytes(),
    };
    if bytes.len() > super::MAX_FILE_BYTES {
        return Err(gettext("The generated file exceeds the 32 MB limit."));
    }
    Ok(RenderedDocument {
        title: content.title,
        bytes,
    })
}

pub fn tables(content: &Content) -> Result<Vec<Table>, String> {
    let tables = content.tables();
    if !tables.is_empty() {
        return Ok(tables);
    }
    let rows = parse_csv(&content.source)?;
    if rows.first().is_none_or(|row| row.len() < 2) {
        return Err(gettext(
            "No table was found. Ask for a spreadsheet with a header row and columns, or choose a text document format.",
        ));
    }
    Ok(vec![Table {
        name: content.title.clone(),
        rows,
    }])
}

fn csv(content: &Content) -> Result<Vec<u8>, String> {
    let tables = content.tables();
    if tables.len() > 1 {
        return Err(gettext(
            "CSV supports one table. Choose Excel or OpenDocument Spreadsheet to keep multiple tables.",
        ));
    }
    let rows = if let Some(table) = tables.first() {
        table.rows.clone()
    } else {
        parse_csv(&content.source)?
    };
    let mut output = String::from("\u{feff}");
    for row in rows {
        let row = row
            .into_iter()
            .map(|value| {
                let trimmed = value.trim_start();
                let value = if (trimmed.starts_with(['=', '+', '@'])
                    || (trimmed.starts_with('-') && numeric(trimmed).is_none()))
                    && !trimmed.is_empty()
                {
                    format!("'{value}")
                } else {
                    value
                };
                format!("\"{}\"", value.replace('"', "\"\""))
            })
            .collect::<Vec<_>>()
            .join(",");
        output.push_str(&row);
        output.push_str("\r\n");
    }
    Ok(output.into_bytes())
}

fn parse_csv(source: &str) -> Result<Vec<Vec<String>>, String> {
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut closed = false;
    let mut chars = source.trim_start_matches('\u{feff}').chars().peekable();
    while let Some(ch) = chars.next() {
        if quoted {
            if ch == '"' && chars.peek() == Some(&'"') {
                field.push('"');
                chars.next();
            } else if ch == '"' {
                quoted = false;
                closed = true;
            } else {
                field.push(ch);
            }
        } else {
            match ch {
                '"' if field.is_empty() && !closed => quoted = true,
                ',' => {
                    row.push(std::mem::take(&mut field));
                    closed = false;
                }
                '\n' | '\r' => {
                    if ch == '\r' && chars.peek() == Some(&'\n') {
                        chars.next();
                    }
                    row.push(std::mem::take(&mut field));
                    rows.push(std::mem::take(&mut row));
                    closed = false;
                }
                _ if closed || ch == '"' => {
                    return Err(gettext(
                        "The response contains malformed CSV. Ask the model to return only a CSV table.",
                    ));
                }
                _ => field.push(ch),
            }
        }
        if row.len() > 100 || rows.len() > 20_000 {
            return Err(gettext("Use up to 100 columns and 20,000 rows."));
        }
    }
    if quoted {
        return Err(gettext(
            "The CSV response ends inside a quoted field. Continue or retry the response.",
        ));
    }
    if !field.is_empty() || !row.is_empty() || closed {
        row.push(field);
        rows.push(row);
    }
    let columns = rows.first().map_or(0, Vec::len);
    if columns > 100 || rows.len() > 20_000 {
        return Err(gettext("Use up to 100 columns and 20,000 rows."));
    }
    if columns == 0 || rows.iter().any(|row| row.len() != columns) {
        return Err(gettext(
            "Each CSV row must have the same number of columns.",
        ));
    }
    Ok(rows)
}

fn html(content: &Content) -> String {
    let mut body = String::new();
    for block in &content.blocks {
        match block {
            Block::Heading(level, text) => {
                body.push_str(&format!("<h{level}>{}</h{level}>", xml(text)))
            }
            Block::Paragraph(text) => {
                body.push_str(&format!("<p>{}</p>", xml(text).replace('\n', "<br>")))
            }
            Block::Bullet(text) => body.push_str(&format!("<ul><li>{}</li></ul>", xml(text))),
            Block::Quote(text) => body.push_str(&format!("<blockquote>{}</blockquote>", xml(text))),
            Block::Code(text) => body.push_str(&format!("<pre><code>{}</code></pre>", xml(text))),
            Block::Rule => body.push_str("<hr>"),
            Block::Table(table) => {
                body.push_str("<table>");
                for (index, row) in table.rows.iter().enumerate() {
                    body.push_str("<tr>");
                    let tag = if index == 0 { "th" } else { "td" };
                    for cell in row {
                        body.push_str(&format!("<{tag}>{}</{tag}>", xml(cell)));
                    }
                    body.push_str("</tr>");
                }
                body.push_str("</table>");
            }
        }
    }
    format!(
        r#"<!doctype html><html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><meta http-equiv="Content-Security-Policy" content="default-src 'none'; style-src 'unsafe-inline'"><title>{}</title><style>body{{font:16px/1.6 system-ui,sans-serif;color:#20242b;background:#fff;max-width:850px;margin:48px auto;padding:0 24px}}h1,h2,h3{{color:#1c3557;line-height:1.25}}table{{border-collapse:collapse;width:100%;margin:20px 0}}td,th{{border:1px solid #d8dee5;padding:9px 12px;text-align:left;vertical-align:top}}th{{background:#edf2f7}}pre{{background:#f4f5f7;padding:16px;white-space:pre-wrap;overflow-wrap:anywhere}}blockquote{{border-left:3px solid #9ba9b6;margin-left:0;padding-left:18px;color:#56616d}}@media print{{body{{margin:0;max-width:none}}h1,h2,h3{{break-after:avoid}}tr,pre{{break-inside:avoid}}}}</style></head><body>{body}</body></html>"#,
        xml(&content.title)
    )
}

fn xml_document(content: &Content) -> String {
    let mut body = String::new();
    for block in &content.blocks {
        match block {
            Block::Heading(level, text) => body.push_str(&format!(
                "<heading level=\"{level}\">{}</heading>",
                xml(text)
            )),
            Block::Paragraph(text)
            | Block::Quote(text)
            | Block::Code(text)
            | Block::Bullet(text) => {
                body.push_str(&format!("<paragraph>{}</paragraph>", xml(text)))
            }
            Block::Table(table) => {
                body.push_str(&format!("<table name=\"{}\">", xml(&table.name)));
                for row in &table.rows {
                    body.push_str("<row>");
                    for cell in row {
                        body.push_str(&format!("<cell>{}</cell>", xml(cell)));
                    }
                    body.push_str("</row>");
                }
                body.push_str("</table>");
            }
            Block::Rule => body.push_str("<separator/>"),
        }
    }
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><document><title>{}</title><body>{body}</body></document>",
        xml(&content.title)
    )
}
