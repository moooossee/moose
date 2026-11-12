use super::{
    DocumentFormat,
    archive::Archive,
    content::{Block, Content, formula, numeric, xml},
    presentation,
};

const NAMESPACES: &str = r###"xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0" xmlns:table="urn:oasis:names:tc:opendocument:xmlns:table:1.0" xmlns:style="urn:oasis:names:tc:opendocument:xmlns:style:1.0" xmlns:fo="urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0" xmlns:draw="urn:oasis:names:tc:opendocument:xmlns:drawing:1.0" xmlns:svg="urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0" xmlns:presentation="urn:oasis:names:tc:opendocument:xmlns:presentation:1.0" xmlns:of="urn:oasis:names:tc:opendocument:xmlns:of:1.2""###;

pub fn render(content: &Content, format: DocumentFormat) -> Result<Vec<u8>, String> {
    let body = match format {
        DocumentFormat::Odt => text_document(content),
        DocumentFormat::Ods => spreadsheet(content)?,
        DocumentFormat::Odp => presentation_document(content)?,
        _ => return Err("This OpenDocument format is not supported.".into()),
    };
    let mut archive = Archive::new();
    archive.add("mimetype", format.mime_type())?;
    archive.add("META-INF/manifest.xml", format!(r###"<?xml version="1.0" encoding="UTF-8"?><manifest:manifest xmlns:manifest="urn:oasis:names:tc:opendocument:xmlns:manifest:1.0" manifest:version="1.3"><manifest:file-entry manifest:full-path="/" manifest:version="1.3" manifest:media-type="{}"/><manifest:file-entry manifest:full-path="content.xml" manifest:media-type="text/xml"/><manifest:file-entry manifest:full-path="styles.xml" manifest:media-type="text/xml"/></manifest:manifest>"###, format.mime_type()))?;
    archive.add("content.xml", format!(r###"<?xml version="1.0" encoding="UTF-8"?><office:document-content {NAMESPACES} office:version="1.3"><office:automatic-styles><style:style style:name="HeaderCell" style:family="table-cell"><style:table-cell-properties fo:background-color="#edf2f7"/><style:text-properties fo:font-weight="bold"/></style:style><style:style style:name="SlideTitle" style:family="paragraph"><style:text-properties fo:font-size="28pt" fo:font-weight="bold" fo:color="#1c3557"/></style:style><style:style style:name="SlideBody" style:family="paragraph"><style:paragraph-properties fo:margin-bottom="0.2cm"/><style:text-properties fo:font-size="18pt"/></style:style></office:automatic-styles><office:body>{body}</office:body></office:document-content>"###))?;
    archive.add("styles.xml", format!(r###"<?xml version="1.0" encoding="UTF-8"?><office:document-styles {NAMESPACES} office:version="1.3"><office:font-face-decls><style:font-face style:name="Liberation Sans" svg:font-family="'Liberation Sans'"/></office:font-face-decls><office:styles><style:default-style style:family="paragraph"><style:paragraph-properties fo:margin-bottom="0.2cm" fo:line-height="120%"/><style:text-properties fo:font-size="11pt" style:font-name="Liberation Sans"/></style:default-style></office:styles><office:automatic-styles><style:page-layout style:name="Page"><style:page-layout-properties fo:page-width="{}" fo:page-height="{}" style:print-orientation="{}" fo:margin="1.8cm"/></style:page-layout></office:automatic-styles><office:master-styles><style:master-page style:name="Default" style:page-layout-name="Page"/></office:master-styles></office:document-styles>"###, if format == DocumentFormat::Odp { "33.87cm" } else { "21cm" }, if format == DocumentFormat::Odp { "19.05cm" } else { "29.7cm" }, if format == DocumentFormat::Odp { "landscape" } else { "portrait" }))?;
    archive.finish()
}

fn text_document(content: &Content) -> String {
    let mut result = String::from("<office:text>");
    if !matches!(content.blocks.first(), Some(Block::Heading(1, title)) if title == &content.title)
    {
        result.push_str(&format!(
            r###"<text:h text:outline-level="1">{}</text:h>"###,
            xml(&content.title)
        ));
    }
    for (index, block) in content.blocks.iter().enumerate() {
        match block {
            Block::Heading(level, text) => result.push_str(&format!(
                r###"<text:h text:outline-level="{level}">{}</text:h>"###,
                xml(text)
            )),
            Block::Paragraph(text) | Block::Quote(text) | Block::Code(text) => {
                result.push_str(&format!("<text:p>{}</text:p>", text_xml(text)))
            }
            Block::Bullet(text) => {
                result.push_str(&format!("<text:p>• {}</text:p>", text_xml(text)))
            }
            Block::Rule => result.push_str("<text:p>────────────────</text:p>"),
            Block::Table(table) => {
                result.push_str(&format!(
                    r###"<table:table table:name="Table{}">"###,
                    index + 1
                ));
                for (row_index, row) in table.rows.iter().enumerate() {
                    result.push_str("<table:table-row>");
                    for cell in row {
                        result.push_str(&format!(r###"<table:table-cell office:value-type="string"{}><text:p>{}</text:p></table:table-cell>"###, if row_index == 0 { r###" table:style-name="HeaderCell""### } else { "" }, text_xml(cell)));
                    }
                    result.push_str("</table:table-row>");
                }
                result.push_str("</table:table>");
            }
        }
    }
    result.push_str("</office:text>");
    result
}

fn spreadsheet(content: &Content) -> Result<String, String> {
    let tables = content.tables();
    if tables.is_empty() || tables.len() > 64 {
        return Err("A spreadsheet needs between 1 and 64 Markdown tables.".into());
    }
    let mut result = String::from("<office:spreadsheet>");
    for (index, table) in tables.iter().enumerate() {
        result.push_str(&format!(
            r###"<table:table table:name="{}">"###,
            xml(&format!(
                "{} ({})",
                table.name.chars().take(80).collect::<String>(),
                index + 1
            ))
        ));
        for (row_index, row) in table.rows.iter().enumerate() {
            result.push_str("<table:table-row>");
            for value in row {
                if row_index > 0
                    && let Some(value) = numeric(value)
                {
                    result.push_str(&format!(r###"<table:table-cell office:value-type="float" office:value="{value}"><text:p>{value}</text:p></table:table-cell>"###));
                } else if row_index > 0
                    && let Some(value) = formula(value)
                {
                    result.push_str(&format!(r###"<table:table-cell table:formula="of:={}" office:value-type="float" office:value="0"><text:p/></table:table-cell>"###, xml(&odf_formula(value))));
                } else {
                    result.push_str(&format!(r###"<table:table-cell office:value-type="string"{}><text:p>{}</text:p></table:table-cell>"###, if row_index == 0 { r###" table:style-name="HeaderCell""### } else { "" }, text_xml(value)));
                }
            }
            result.push_str("</table:table-row>");
        }
        result.push_str("</table:table>");
    }
    result.push_str("</office:spreadsheet>");
    Ok(result)
}

fn odf_formula(value: &str) -> String {
    let mut result = String::new();
    let mut token = String::new();
    let mut quoted = false;
    let flush = |token: &mut String, result: &mut String| {
        if token.chars().any(|ch| ch.is_ascii_alphabetic())
            && token.chars().any(|ch| ch.is_ascii_digit())
            && token
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || ch == '$')
        {
            result.push_str(&format!("[.{token}]"));
        } else {
            result.push_str(token);
        }
        token.clear();
    };
    for ch in value.chars() {
        if ch == '"' {
            flush(&mut token, &mut result);
            quoted = !quoted;
            result.push(ch);
        } else if quoted {
            result.push(ch);
        } else if ch.is_ascii_alphanumeric() || ch == '$' {
            token.push(ch);
        } else {
            flush(&mut token, &mut result);
            result.push(if ch == ',' { ';' } else { ch });
        }
    }
    flush(&mut token, &mut result);
    result.replace("]:[.", ":.")
}

fn presentation_document(content: &Content) -> Result<String, String> {
    let mut result = String::from("<office:presentation>");
    for (index, (title, lines)) in presentation::slides(content)?.iter().enumerate() {
        result.push_str(&format!(r###"<draw:page draw:name="Slide {}" draw:master-page-name="Default"><draw:frame svg:x="1.8cm" svg:y="1.2cm" svg:width="30cm" svg:height="2.4cm"><draw:text-box><text:p text:style-name="SlideTitle">{}</text:p></draw:text-box></draw:frame><draw:frame svg:x="1.8cm" svg:y="4cm" svg:width="30cm" svg:height="13cm"><draw:text-box>"###, index + 1, text_xml(title)));
        for line in lines {
            result.push_str(&format!(
                r###"<text:p text:style-name="SlideBody">{}</text:p>"###,
                text_xml(line)
            ));
        }
        result.push_str("</draw:text-box></draw:frame></draw:page>");
    }
    result.push_str("</office:presentation>");
    Ok(result)
}

fn text_xml(value: &str) -> String {
    xml(value)
        .replace('\n', "<text:line-break/>")
        .replace('\t', "<text:tab/>")
}
