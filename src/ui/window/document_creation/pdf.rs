use std::{
    cell::RefCell,
    fs,
    os::unix::fs::{DirBuilderExt, OpenOptionsExt},
    path::{Path, PathBuf},
    rc::Rc,
};

use gtk::{gdk, gsk, pango, prelude::*};

use crate::documents::{
    MAX_FILE_BYTES,
    content::{Block, Content, Table, xml},
};

struct TemporaryFile(PathBuf);

impl Drop for TemporaryFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

pub(super) fn render(
    parent: &adw::ApplicationWindow,
    directory: &Path,
    content: &Content,
) -> Result<Vec<u8>, String> {
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(directory)
        .map_err(|_| "The document cache could not be opened.")?;
    let path = TemporaryFile(directory.join(format!("{}.pdf", crate::core::new_id())));
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&path.0)
        .map_err(|_| "A temporary PDF could not be created.")?;
    let operation = gtk::PrintOperation::new();
    operation.set_allow_async(false);
    operation.set_unit(gtk::Unit::Points);
    operation.set_export_filename(&path.0);
    operation.set_job_name(&content.title);
    let setup = gtk::PageSetup::new();
    setup.set_paper_size(&gtk::PaperSize::new(Some("iso_a4")));
    setup.set_top_margin(48.0, gtk::Unit::Points);
    setup.set_bottom_margin(48.0, gtk::Unit::Points);
    setup.set_left_margin(48.0, gtk::Unit::Points);
    setup.set_right_margin(48.0, gtk::Unit::Points);
    operation.set_default_page_setup(Some(&setup));
    let pages = Rc::new(RefCell::new(Vec::<(f64, f64)>::new()));
    let node = Rc::new(RefCell::new(None::<gsk::RenderNode>));
    let error = Rc::new(RefCell::new(None::<String>));
    let markup = markup(content);
    let target_pages = pages.clone();
    let target_node = node.clone();
    let target_error = error.clone();
    operation.connect_begin_print(move |operation, context| {
        let layout = context.create_pango_layout();
        layout.set_width((context.width() * f64::from(pango::SCALE)) as i32);
        layout.set_wrap(pango::WrapMode::WordChar);
        layout.set_font_description(Some(&pango::FontDescription::from_string("Sans 11")));
        layout.set_spacing(3 * pango::SCALE);
        layout.set_markup(&markup);
        let height = context.height() - 26.0;
        let mut ranges = Vec::new();
        let mut start = 0.0;
        let mut iter = layout.iter();
        let end;
        loop {
            let (_, rect) = iter.line_extents();
            let y = f64::from(rect.y()) / f64::from(pango::SCALE);
            let bottom = y + f64::from(rect.height()) / f64::from(pango::SCALE);
            if bottom - start > height && y > start {
                ranges.push((start, y));
                start = y;
            }
            if !iter.next_line() {
                end = bottom;
                break;
            }
        }
        ranges.push((start, end));
        if ranges.len() > 300 {
            *target_error.borrow_mut() = Some(
                "The PDF exceeds the limit of 300 pages. Split the document into smaller parts."
                    .into(),
            );
            operation.cancel();
            return;
        }
        let snapshot = gtk::Snapshot::new();
        snapshot.append_layout(&layout, &gdk::RGBA::BLACK);
        *target_node.borrow_mut() = snapshot.to_node();
        if target_node.borrow().is_none() {
            *target_error.borrow_mut() =
                Some("The PDF text could not be rendered. Try another format.".into());
            operation.cancel();
            return;
        }
        operation.set_n_pages(ranges.len() as i32);
        *target_pages.borrow_mut() = ranges;
    });
    let target_pages = pages.clone();
    let target_error = error.clone();
    operation.connect_draw_page(move |operation, context, page| {
        let pages = target_pages.borrow();
        let Some(&(start, end)) = pages.get(page as usize) else {
            return;
        };
        let cr = context.cairo_context();
        if cr.save().is_ok() {
            cr.rectangle(
                0.0,
                0.0,
                context.width(),
                (end - start).min(context.height() - 26.0),
            );
            cr.clip();
            cr.translate(0.0, -start);
            if let Some(node) = node.borrow().as_ref() {
                node.draw(&cr);
            }
            let _ = cr.restore();
        }
        let footer = context.create_pango_layout();
        footer.set_font_description(Some(&pango::FontDescription::from_string("Sans 9")));
        footer.set_text(&format!("{} / {}", page + 1, pages.len()));
        let snapshot = gtk::Snapshot::new();
        snapshot.append_layout(&footer, &gdk::RGBA::new(0.4, 0.4, 0.4, 1.0));
        if let Some(node) = snapshot.to_node() {
            cr.translate(0.0, context.height() - 15.0);
            node.draw(&cr);
        }
        if let Err(error) = cr.status() {
            *target_error.borrow_mut() = Some(format!("The PDF page could not be drawn: {error}"));
            operation.cancel();
        }
    });
    let result = operation
        .run(gtk::PrintOperationAction::Export, Some(parent))
        .map_err(|error| format!("PDF generation failed: {error}"))?;
    if let Some(error) = error.borrow_mut().take() {
        return Err(error);
    }
    if result != gtk::PrintOperationResult::Apply {
        return Err("PDF generation did not finish.".into());
    }
    let bytes = fs::read(&path.0).map_err(|_| "The generated PDF could not be read.")?;
    if !bytes.starts_with(b"%PDF-") || bytes.len() > MAX_FILE_BYTES {
        return Err("The PDF is invalid or exceeds the size limit.".into());
    }
    Ok(bytes)
}

fn markup(content: &Content) -> String {
    let mut value = String::new();
    if !matches!(content.blocks.first(), Some(Block::Heading(1, title)) if title == &content.title)
    {
        value.push_str(&format!(
            "<span size=\"24000\" weight=\"bold\" foreground=\"#1c3557\">{}</span>\n\n",
            xml(&content.title)
        ));
    }
    for block in &content.blocks {
        match block {
            Block::Heading(level, text) => value.push_str(&format!(
                "<span size=\"{}\" weight=\"bold\" foreground=\"#1c3557\">{}</span>\n\n",
                match level {
                    1 => 24000,
                    2 => 19000,
                    _ => 15000,
                },
                xml(text)
            )),
            Block::Paragraph(text) => value.push_str(&format!("{}\n\n", xml(text))),
            Block::Bullet(text) => value.push_str(&format!("• {}\n", xml(text))),
            Block::Quote(text) => value.push_str(&format!(
                "<span style=\"italic\" foreground=\"#555555\">{}</span>\n\n",
                xml(text)
            )),
            Block::Code(text) => value.push_str(&format!(
                "<span font_family=\"monospace\" size=\"10000\">{}</span>\n\n",
                xml(text)
            )),
            Block::Rule => value.push_str("────────────────────────────────────────\n\n"),
            Block::Table(table) => {
                value.push_str(&table_markup(table));
            }
        }
    }
    value.trim_end().to_string()
}

fn table_markup(table: &Table) -> String {
    let columns = table.rows.iter().map(Vec::len).max().unwrap_or(0);
    if columns == 0 {
        return String::new();
    }
    let widths = (0..columns)
        .map(|column| {
            table
                .rows
                .iter()
                .filter_map(|row| row.get(column))
                .map(|cell| {
                    cell.lines()
                        .map(|line| line.chars().count())
                        .max()
                        .unwrap_or(0)
                })
                .max()
                .unwrap_or(0)
                .clamp(6, 28)
        })
        .collect::<Vec<_>>();
    if widths.iter().sum::<usize>() + (columns - 1) * 3 > 80 {
        let mut result = String::new();
        for (index, row) in table.rows.iter().enumerate().skip(1) {
            result.push_str(&format!("<b>{index}</b>\n"));
            for (column, cell) in row.iter().enumerate() {
                let name = table.rows[0]
                    .get(column)
                    .map(String::as_str)
                    .unwrap_or("Column");
                result.push_str(&format!("<b>{}</b>: {}\n", xml(name), xml(cell)));
            }
            result.push('\n');
        }
        if table.rows.len() == 1 {
            result.push_str(&xml(&table.rows[0].join(" · ")));
            result.push('\n');
        }
        return result;
    }
    let mut result = String::new();
    for (index, row) in table.rows.iter().enumerate() {
        let cells = widths
            .iter()
            .enumerate()
            .map(|(column, width)| {
                wrap_cell(
                    row.get(column).map(String::as_str).unwrap_or_default(),
                    *width,
                )
            })
            .collect::<Vec<_>>();
        let height = cells.iter().map(Vec::len).max().unwrap_or(1);
        for line in 0..height {
            let text = cells
                .iter()
                .zip(&widths)
                .map(|(lines, width)| {
                    let text = lines.get(line).map(String::as_str).unwrap_or_default();
                    format!(
                        "{text}{}",
                        " ".repeat(width.saturating_sub(text.chars().count()))
                    )
                })
                .collect::<Vec<_>>()
                .join(" │ ");
            result.push_str(&format!(
                "<span font_family=\"monospace\" size=\"9500\"{}>{}</span>\n",
                if index == 0 { " weight=\"bold\"" } else { "" },
                xml(&text)
            ));
        }
        if index == 0 {
            result.push_str(&format!(
                "<span font_family=\"monospace\" size=\"9500\">{}</span>\n",
                widths
                    .iter()
                    .map(|width| "─".repeat(*width))
                    .collect::<Vec<_>>()
                    .join("─┼─")
            ));
        }
    }
    result.push('\n');
    result
}

fn wrap_cell(value: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    for line in value.split('\n') {
        let mut text = String::new();
        for ch in line.chars() {
            if text.chars().count() == width {
                lines.push(std::mem::take(&mut text));
            }
            text.push(ch);
        }
        lines.push(text);
    }
    lines
}
