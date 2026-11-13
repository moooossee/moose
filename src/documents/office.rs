use crate::i18n::gettext;

use super::{
    archive::Archive,
    content::{Block, Content, Table, formula, numeric, xml},
};

const WORD: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
const SHEET: &str = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";
const REL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";

pub fn content_types(main: &str, overrides: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/>{main}{overrides}</Types>"#
    )
}

pub fn relationships(entries: &[(&str, &str, &str)]) -> String {
    let entries = entries
        .iter()
        .map(|(id, kind, target)| {
            format!(
                r#"<Relationship Id="{}" Type="{REL}/{kind}" Target="{}"/>"#,
                xml(id),
                xml(target)
            )
        })
        .collect::<String>();
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">{entries}</Relationships>"#
    )
}

pub fn docx(content: &Content) -> Result<Vec<u8>, String> {
    let mut archive = Archive::new();
    archive.add("[Content_Types].xml", content_types(
        r#"<Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>"#,
        r#"<Override PartName="/word/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml"/>"#,
    ))?;
    archive.add(
        "_rels/.rels",
        relationships(&[("rId1", "officeDocument", "word/document.xml")]),
    )?;
    archive.add(
        "word/_rels/document.xml.rels",
        relationships(&[("rId1", "styles", "styles.xml")]),
    )?;
    let mut body = String::new();
    if !matches!(content.blocks.first(), Some(Block::Heading(1, title)) if title == &content.title)
    {
        body.push_str(&paragraph(&content.title, "Title", false));
    }
    for block in &content.blocks {
        match block {
            Block::Heading(level, text) => body.push_str(&paragraph(text, &format!("Heading{}", (*level).min(3)), false)),
            Block::Paragraph(text) => body.push_str(&paragraph(text, "Normal", false)),
            Block::Bullet(text) => body.push_str(&paragraph(&format!("• {text}"), "ListParagraph", false)),
            Block::Quote(text) => body.push_str(&paragraph(text, "Quote", false)),
            Block::Code(text) => body.push_str(&paragraph(text, "Code", false)),
            Block::Rule => body.push_str(r#"<w:p><w:pPr><w:pBdr><w:bottom w:val="single" w:sz="6" w:color="D0D0D0"/></w:pBdr></w:pPr></w:p>"#),
            Block::Table(table) => {
                let columns = table.rows.iter().map(Vec::len).max().unwrap_or(1).max(1);
                if columns > 100 { return Err(gettext("Tables can contain up to 100 columns.")); }
                body.push_str(r#"<w:tbl><w:tblPr><w:tblW w:w="0" w:type="auto"/><w:tblBorders><w:top w:val="single" w:sz="4" w:color="D0D0D0"/><w:left w:val="single" w:sz="4" w:color="D0D0D0"/><w:bottom w:val="single" w:sz="4" w:color="D0D0D0"/><w:right w:val="single" w:sz="4" w:color="D0D0D0"/><w:insideH w:val="single" w:sz="4" w:color="D0D0D0"/><w:insideV w:val="single" w:sz="4" w:color="D0D0D0"/></w:tblBorders></w:tblPr><w:tblGrid>"#);
                for _ in 0..columns { body.push_str(&format!(r#"<w:gridCol w:w="{}"/>"#, 9000 / columns)); }
                body.push_str("</w:tblGrid>");
                for (index, row) in table.rows.iter().enumerate() {
                    body.push_str("<w:tr>");
                    if index == 0 { body.push_str("<w:trPr><w:tblHeader/></w:trPr>"); }
                    for column in 0..columns {
                        body.push_str(&format!(r#"<w:tc><w:tcPr><w:tcW w:w="{}" w:type="dxa"/>{}</w:tcPr>{}</w:tc>"#, 9000 / columns, if index == 0 { r#"<w:shd w:fill="EDF2F7"/>"# } else { "" }, paragraph(row.get(column).map(String::as_str).unwrap_or_default(), "Normal", index == 0)));
                    }
                    body.push_str("</w:tr>");
                }
                body.push_str("</w:tbl><w:p/>");
            }
        }
    }
    body.push_str(r#"<w:sectPr><w:pgSz w:w="11906" w:h="16838"/><w:pgMar w:top="1134" w:right="1134" w:bottom="1134" w:left="1134" w:header="567" w:footer="567"/></w:sectPr>"#);
    archive.add("word/document.xml", format!(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document xmlns:w="{WORD}"><w:body>{body}</w:body></w:document>"#))?;
    let mut styles = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:styles xmlns:w="{WORD}"><w:docDefaults><w:rPrDefault><w:rPr><w:rFonts w:ascii="Calibri" w:hAnsi="Calibri"/><w:sz w:val="22"/></w:rPr></w:rPrDefault><w:pPrDefault><w:pPr><w:spacing w:after="160" w:line="276" w:lineRule="auto"/></w:pPr></w:pPrDefault></w:docDefaults><w:style w:type="paragraph" w:default="1" w:styleId="Normal"><w:name w:val="Normal"/></w:style>"#
    );
    for (id, size, color) in [
        ("Title", 40, "1C3557"),
        ("Heading1", 36, "1C3557"),
        ("Heading2", 28, "1C3557"),
        ("Heading3", 24, "1C3557"),
    ] {
        styles.push_str(&format!(r#"<w:style w:type="paragraph" w:styleId="{id}"><w:name w:val="{id}"/><w:basedOn w:val="Normal"/><w:pPr><w:keepNext/><w:spacing w:before="240" w:after="120"/></w:pPr><w:rPr><w:b/><w:color w:val="{color}"/><w:sz w:val="{size}"/></w:rPr></w:style>"#));
    }
    styles.push_str(r#"<w:style w:type="paragraph" w:styleId="ListParagraph"><w:name w:val="List Paragraph"/><w:basedOn w:val="Normal"/><w:pPr><w:ind w:left="360" w:hanging="180"/></w:pPr></w:style><w:style w:type="paragraph" w:styleId="Quote"><w:name w:val="Quote"/><w:basedOn w:val="Normal"/><w:pPr><w:ind w:left="360"/></w:pPr><w:rPr><w:i/><w:color w:val="555555"/></w:rPr></w:style><w:style w:type="paragraph" w:styleId="Code"><w:name w:val="Code"/><w:basedOn w:val="Normal"/><w:rPr><w:rFonts w:ascii="DejaVu Sans Mono" w:hAnsi="DejaVu Sans Mono"/><w:sz w:val="18"/></w:rPr></w:style></w:styles>"#);
    archive.add("word/styles.xml", styles)?;
    archive.finish()
}

fn paragraph(value: &str, style: &str, bold: bool) -> String {
    let text = value
        .split('\n')
        .map(|line| format!(r#"<w:t xml:space="preserve">{}</w:t>"#, xml(line)))
        .collect::<Vec<_>>()
        .join("<w:br/>");
    format!(
        r#"<w:p><w:pPr><w:pStyle w:val="{style}"/></w:pPr><w:r>{}{text}</w:r></w:p>"#,
        if bold { "<w:rPr><w:b/></w:rPr>" } else { "" }
    )
}

pub fn xlsx(tables: &[Table]) -> Result<Vec<u8>, String> {
    if tables.is_empty() || tables.len() > 64 {
        return Err(gettext(
            "A workbook needs between 1 and 64 tables or worksheets.",
        ));
    }
    let mut archive = Archive::new();
    let mut overrides = String::from(
        r#"<Override PartName="/xl/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml"/>"#,
    );
    let mut sheets = String::new();
    let mut relationships_data = Vec::new();
    let mut names = Vec::<String>::new();
    for (index, table) in tables.iter().enumerate() {
        let id = index + 1;
        let name = sheet_name(&table.name, &names);
        names.push(name.clone());
        sheets.push_str(&format!(
            r#"<sheet name="{}" sheetId="{id}" r:id="rId{id}"/>"#,
            xml(&name)
        ));
        overrides.push_str(&format!(r#"<Override PartName="/xl/worksheets/sheet{id}.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>"#));
        relationships_data.push((format!("rId{id}"), format!("worksheets/sheet{id}.xml")));
        archive.add(&format!("xl/worksheets/sheet{id}.xml"), worksheet(table)?)?;
    }
    archive.add("[Content_Types].xml", content_types(r#"<Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/>"#, &overrides))?;
    archive.add(
        "_rels/.rels",
        relationships(&[("rId1", "officeDocument", "xl/workbook.xml")]),
    )?;
    archive.add("xl/workbook.xml", format!(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><workbook xmlns="{SHEET}" xmlns:r="{REL}"><bookViews><workbookView/></bookViews><sheets>{sheets}</sheets><calcPr calcId="191029" fullCalcOnLoad="1"/></workbook>"#))?;
    let mut rels = relationships_data
        .iter()
        .map(|(id, target)| (id.as_str(), "worksheet", target.as_str()))
        .collect::<Vec<_>>();
    rels.push(("rStyles", "styles", "styles.xml"));
    archive.add("xl/_rels/workbook.xml.rels", relationships(&rels))?;
    archive.add("xl/styles.xml", format!(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><styleSheet xmlns="{SHEET}"><fonts count="2"><font><sz val="11"/><name val="Calibri"/></font><font><b/><sz val="11"/><color rgb="FFFFFFFF"/><name val="Calibri"/></font></fonts><fills count="3"><fill><patternFill patternType="none"/></fill><fill><patternFill patternType="gray125"/></fill><fill><patternFill patternType="solid"><fgColor rgb="FF1C3557"/><bgColor indexed="64"/></patternFill></fill></fills><borders count="1"><border><left/><right/><top/><bottom/><diagonal/></border></borders><cellStyleXfs count="1"><xf numFmtId="0" fontId="0" fillId="0" borderId="0"/></cellStyleXfs><cellXfs count="2"><xf numFmtId="0" fontId="0" fillId="0" borderId="0" xfId="0" applyAlignment="1"><alignment vertical="top" wrapText="1"/></xf><xf numFmtId="0" fontId="1" fillId="2" borderId="0" xfId="0" applyFont="1" applyFill="1" applyAlignment="1"><alignment vertical="center" wrapText="1"/></xf></cellXfs><cellStyles count="1"><cellStyle name="Normal" xfId="0" builtinId="0"/></cellStyles></styleSheet>"#))?;
    archive.finish()
}

fn worksheet(table: &Table) -> Result<String, String> {
    let columns = table.rows.iter().map(Vec::len).max().unwrap_or(0);
    if columns == 0 || columns > 100 || table.rows.len() > 20_000 {
        return Err(gettext(
            "Use tables with 1 to 100 columns and up to 20,000 rows.",
        ));
    }
    let mut rows = String::new();
    for (row_index, row) in table.rows.iter().enumerate() {
        let number = row_index + 1;
        rows.push_str(&format!(r#"<row r="{number}">"#));
        for (column, value) in row.iter().enumerate() {
            if value.encode_utf16().count() > 32_767 {
                return Err(gettext(
                    "A spreadsheet cell can contain up to 32,767 characters.",
                ));
            }
            let reference = format!("{}{number}", column_name(column));
            let style = usize::from(row_index == 0);
            if row_index > 0
                && let Some(formula) = formula(value)
            {
                rows.push_str(&format!(
                    r#"<c r="{reference}" s="{style}"><f>{}</f></c>"#,
                    xml(formula)
                ));
            } else if row_index > 0
                && let Some(value) = numeric(value)
            {
                rows.push_str(&format!(
                    r#"<c r="{reference}" s="{style}"><v>{value}</v></c>"#
                ));
            } else {
                rows.push_str(&format!(r#"<c r="{reference}" s="{style}" t="inlineStr"><is><t xml:space="preserve">{}</t></is></c>"#, xml(value)));
            }
        }
        rows.push_str("</row>");
    }
    Ok(format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><worksheet xmlns="{SHEET}"><sheetViews><sheetView workbookViewId="0"><pane ySplit="1" topLeftCell="A2" activePane="bottomLeft" state="frozen"/></sheetView></sheetViews><sheetFormatPr defaultRowHeight="18"/><cols><col min="1" max="{columns}" width="22" customWidth="1"/></cols><sheetData>{rows}</sheetData><autoFilter ref="A1:{}{}"/><pageMargins left="0.5" right="0.5" top="0.5" bottom="0.5" header="0.3" footer="0.3"/></worksheet>"#,
        column_name(columns - 1),
        table.rows.len()
    ))
}

pub fn column_name(mut index: usize) -> String {
    let mut result = String::new();
    loop {
        result.insert(0, (b'A' + (index % 26) as u8) as char);
        if index < 26 {
            break;
        }
        index = index / 26 - 1;
    }
    result
}

fn sheet_name(value: &str, existing: &[String]) -> String {
    let mut units = 0;
    let value = value
        .chars()
        .filter(|ch| !"[]:*?/\\".contains(*ch) && !ch.is_control())
        .take_while(|ch| {
            units += ch.len_utf16();
            units <= 25
        })
        .collect::<String>();
    let base = value.trim().trim_matches('\'');
    let base = if base.is_empty() {
        gettext("Sheet")
    } else {
        base.to_string()
    };
    let mut name = base.to_string();
    let mut suffix = 2;
    while existing
        .iter()
        .any(|entry| entry.eq_ignore_ascii_case(&name))
    {
        name = format!("{base} ({suffix})");
        suffix += 1;
    }
    name.chars().take(31).collect()
}
