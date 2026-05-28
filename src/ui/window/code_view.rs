use std::{cell::RefCell, path::Path, rc::Rc, time::Duration};

use comrak::nodes::NodeCodeBlock;
use gtk::prelude::*;
use gtk::{Align, Orientation, PolicyType, glib};
use sourceview5::prelude::*;

pub(super) struct LiveCodeBlock {
    root: gtk::Box,
    language_label: gtk::Label,
    scrolled: gtk::ScrolledWindow,
    buffer: sourceview5::Buffer,
    code: Rc<RefCell<String>>,
    language: String,
    height: i32,
}

pub(super) fn code_block(block: &NodeCodeBlock) -> gtk::Box {
    let code = block.literal.clone();
    let language = code_language(&block.info, &code);
    let copy_button = copy_button();
    bind_copy_button(&copy_button, code.clone());

    let (root, _) = code_shell(&language_label_text(&language), &copy_button);
    let height = code_height(&code, CodeHeightMode::Stable);
    let scrolled = gtk::ScrolledWindow::builder()
        .hexpand(true)
        .hscrollbar_policy(PolicyType::Automatic)
        .min_content_height(height)
        .propagate_natural_height(true)
        .build();
    let buffer = highlighted_code_buffer(&code, &language);
    let view = source_code_view(&buffer, code_line_count(&code) > 14);
    scrolled.set_child(Some(&view));
    scrolled.set_vscrollbar_policy(PolicyType::Automatic);
    scrolled.set_max_content_height(420);
    scrolled.add_css_class("moose-code-scroll");

    root.append(&scrolled);
    root
}

impl LiveCodeBlock {
    pub(super) fn new(content: &str, language: &str) -> Self {
        let code = Rc::new(RefCell::new(content.to_string()));
        let copy_button = copy_button();
        bind_live_copy_button(&copy_button, Rc::clone(&code));

        let (root, language_label) = code_shell(&language_label_text(language), &copy_button);
        let buffer = highlighted_code_buffer(content, language);
        let view = source_code_view(&buffer, false);
        let height = code_height(content, CodeHeightMode::Live);
        let scrolled = gtk::ScrolledWindow::builder()
            .child(&view)
            .hexpand(true)
            .hscrollbar_policy(PolicyType::Automatic)
            .vscrollbar_policy(PolicyType::Never)
            .min_content_height(height)
            .propagate_natural_height(true)
            .build();
        scrolled.add_css_class("moose-code-scroll");

        root.append(&scrolled);

        Self {
            root,
            language_label,
            scrolled,
            buffer,
            code,
            language: language.to_string(),
            height,
        }
    }

    pub(super) fn widget(&self) -> gtk::Widget {
        self.root.clone().upcast()
    }

    pub(super) fn update(&mut self, content: &str, language: &str) {
        if self.language != language {
            self.language = language.to_string();
            self.language_label
                .set_text(&language_label_text(&self.language));
            set_buffer_language(&self.buffer, &self.language);
        }
        self.buffer.set_text(content);
        *self.code.borrow_mut() = content.to_string();
        let next_height = code_height(content, CodeHeightMode::Live);
        if self.height != next_height {
            self.height = next_height;
            self.scrolled.set_min_content_height(next_height);
        }
    }
}

#[derive(Clone, Copy)]
enum CodeHeightMode {
    Stable,
    Live,
}

fn code_shell(label: &str, copy_button: &gtk::Button) -> (gtk::Box, gtk::Label) {
    let root = gtk::Box::builder()
        .orientation(Orientation::Vertical)
        .spacing(0)
        .hexpand(true)
        .halign(Align::Fill)
        .build();
    root.add_css_class("moose-code-block");

    let header = gtk::Box::builder()
        .orientation(Orientation::Horizontal)
        .spacing(8)
        .hexpand(true)
        .halign(Align::Fill)
        .build();
    header.add_css_class("moose-code-header");

    let language_label = gtk::Label::builder()
        .label(label)
        .halign(Align::Start)
        .hexpand(true)
        .xalign(0.0)
        .build();
    language_label.add_css_class("caption-heading");
    language_label.add_css_class("dim-label");
    language_label.add_css_class("moose-code-language");

    header.append(&language_label);
    header.append(copy_button);
    root.append(&header);

    (root, language_label)
}

fn copy_button() -> gtk::Button {
    let button = gtk::Button::from_icon_name("edit-copy-symbolic");
    button.add_css_class("flat");
    button.add_css_class("moose-code-copy");
    button.set_tooltip_text(Some("Copy code"));
    button
}

fn highlighted_code_buffer(code: &str, language: &str) -> sourceview5::Buffer {
    let source_language = source_language(language);
    let buffer = match source_language.as_ref() {
        Some(language) => sourceview5::Buffer::with_language(language),
        None => sourceview5::Buffer::new(None::<&gtk::TextTagTable>),
    };

    if let Some(language) = source_language.as_ref() {
        buffer.set_language(Some(language));
    }
    if let Some(scheme) = source_style_scheme() {
        buffer.set_style_scheme(Some(&scheme));
    }
    buffer.set_highlight_syntax(source_language.is_some());
    buffer.set_text(code);
    buffer
}

fn set_buffer_language(buffer: &sourceview5::Buffer, language: &str) {
    let source_language = source_language(language);
    buffer.set_language(source_language.as_ref());
    buffer.set_highlight_syntax(source_language.is_some());
}

fn source_code_view(buffer: &sourceview5::Buffer, show_line_numbers: bool) -> sourceview5::View {
    let view = sourceview5::View::builder()
        .buffer(buffer)
        .editable(false)
        .cursor_visible(false)
        .monospace(true)
        .wrap_mode(gtk::WrapMode::None)
        .left_margin(12)
        .right_margin(12)
        .top_margin(10)
        .bottom_margin(10)
        .show_line_numbers(show_line_numbers)
        .tab_width(4)
        .build();
    view.add_css_class("moose-code-view");
    view
}

fn bind_copy_button(button: &gtk::Button, code: String) {
    button.connect_clicked(move |button| {
        if let Some(display) = gtk::gdk::Display::default() {
            display.clipboard().set_text(&code);
            button.set_icon_name("object-select-symbolic");
            let button = button.clone();
            glib::timeout_add_local_once(Duration::from_millis(1200), move || {
                button.set_icon_name("edit-copy-symbolic");
            });
        }
    });
}

fn bind_live_copy_button(button: &gtk::Button, code: Rc<RefCell<String>>) {
    button.connect_clicked(move |button| {
        if let Some(display) = gtk::gdk::Display::default() {
            let code = code.borrow();
            display.clipboard().set_text(code.as_str());
            button.set_icon_name("object-select-symbolic");
            let button = button.clone();
            glib::timeout_add_local_once(Duration::from_millis(1200), move || {
                button.set_icon_name("edit-copy-symbolic");
            });
        }
    });
}

fn code_height(code: &str, mode: CodeHeightMode) -> i32 {
    let line_count = code_line_count(code);
    let lines = match mode {
        CodeHeightMode::Stable => line_count.min(18),
        CodeHeightMode::Live => line_count,
    };
    let lines = lines as i32;
    (lines * 21 + 24).max(58)
}

fn code_line_count(code: &str) -> usize {
    let trailing_line = usize::from(code.ends_with('\n'));
    (code.lines().count() + trailing_line).max(1)
}

pub(super) fn code_language(info: &str, code: &str) -> String {
    let explicit = info
        .split_whitespace()
        .next()
        .unwrap_or("")
        .trim_matches(|value: char| {
            !value.is_ascii_alphanumeric() && value != '-' && value != '_' && value != '+'
        })
        .to_ascii_lowercase();

    if explicit.is_empty() {
        inferred_code_language(code).unwrap_or("").to_string()
    } else {
        explicit
    }
}

fn inferred_code_language(code: &str) -> Option<&'static str> {
    let trimmed = code.trim_start();
    let lower = trimmed.to_ascii_lowercase();

    if trimmed.starts_with('{') || trimmed.starts_with('[') {
        return Some("json");
    }
    if trimmed.starts_with("<!doctype html")
        || trimmed.starts_with("<html")
        || lower.contains("</html>")
    {
        return Some("html");
    }
    if lower.starts_with("select ")
        || lower.starts_with("with ")
        || lower.starts_with("insert ")
        || lower.starts_with("update ")
    {
        return Some("sql");
    }
    if lower.starts_with("#!/bin/")
        || lower.contains("\napt ")
        || lower.contains("\ncargo ")
        || lower.contains("\ngit ")
    {
        return Some("bash");
    }
    let looks_like_javascript_import =
        lower.starts_with("import {") || (lower.starts_with("import ") && lower.contains(" from "));

    if lower.contains("function ")
        || lower.contains("=>")
        || lower.contains("console.log")
        || lower.starts_with("const ")
        || lower.starts_with("let ")
        || looks_like_javascript_import
    {
        return Some("javascript");
    }
    if lower.contains("\ndef ")
        || lower.starts_with("def ")
        || lower.starts_with("class ")
        || (lower.starts_with("import ") && !looks_like_javascript_import)
        || lower.starts_with("from ")
    {
        return Some("python");
    }
    if lower.contains("\nfn ")
        || lower.starts_with("fn ")
        || lower.contains("\npub fn ")
        || lower.starts_with("pub fn ")
        || lower.contains("\nimpl ")
        || lower.starts_with("impl ")
        || lower.starts_with("pub struct ")
        || lower.starts_with("struct ")
        || lower.starts_with("#[derive")
        || lower.starts_with("use std::")
    {
        return Some("rust");
    }
    None
}

fn language_label_text(language: &str) -> String {
    match language {
        "" | "text" | "txt" => "Text".to_string(),
        "bash" | "shell" | "sh" | "zsh" => "Shell".to_string(),
        "csharp" | "cs" => "C#".to_string(),
        "cpp" | "c++" => "C++".to_string(),
        "css" => "CSS".to_string(),
        "go" | "golang" => "Go".to_string(),
        "html" => "HTML".to_string(),
        "java" => "Java".to_string(),
        "javascript" | "js" | "jsx" => "JavaScript".to_string(),
        "json" => "JSON".to_string(),
        "kotlin" | "kt" => "Kotlin".to_string(),
        "markdown" | "md" => "Markdown".to_string(),
        "php" => "PHP".to_string(),
        "python" | "py" => "Python".to_string(),
        "ruby" | "rb" => "Ruby".to_string(),
        "rust" | "rs" => "Rust".to_string(),
        "sql" => "SQL".to_string(),
        "swift" => "Swift".to_string(),
        "toml" => "TOML".to_string(),
        "typescript" | "ts" | "tsx" => "TypeScript".to_string(),
        "xml" => "XML".to_string(),
        "yaml" | "yml" => "YAML".to_string(),
        value => value
            .split(['-', '_'])
            .filter(|part| !part.is_empty())
            .map(title_word)
            .collect::<Vec<_>>()
            .join(" "),
    }
}

fn title_word(value: &str) -> String {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return String::new();
    };
    format!("{}{}", first.to_uppercase(), chars.as_str())
}

fn source_language(language: &str) -> Option<sourceview5::Language> {
    let manager = sourceview5::LanguageManager::default();
    if language.is_empty() {
        return manager.language("text");
    }

    for candidate in language_candidates(language) {
        if let Some(language) = manager.language(candidate) {
            return Some(language);
        }
    }

    language_extension(language)
        .map(|extension| format!("code.{extension}"))
        .and_then(|filename| manager.guess_language(Some(Path::new(&filename)), None))
}

fn source_style_scheme() -> Option<sourceview5::StyleScheme> {
    let manager = sourceview5::StyleSchemeManager::default();
    let dark = adw::StyleManager::default().is_dark();
    let candidates = if dark {
        ["Adwaita-dark", "Adwaita", "classic"]
    } else {
        ["kate", "Adwaita", "classic"]
    };

    candidates
        .into_iter()
        .find_map(|scheme| manager.scheme(scheme))
}

fn language_candidates(language: &str) -> &'static [&'static str] {
    match language {
        "bash" | "shell" | "sh" | "zsh" => &["sh", "bash"],
        "c" => &["c"],
        "c++" | "cpp" => &["cpp", "c++"],
        "csharp" | "cs" => &["csharp", "cs"],
        "css" => &["css"],
        "go" | "golang" => &["go"],
        "html" => &["html"],
        "java" => &["java"],
        "javascript" | "js" | "jsx" => &["js", "javascript"],
        "json" => &["json"],
        "kotlin" | "kt" => &["kotlin"],
        "markdown" | "md" => &["markdown"],
        "php" => &["php"],
        "python" | "py" => &["python3", "python"],
        "ruby" | "rb" => &["ruby"],
        "rust" | "rs" => &["rust"],
        "sql" => &["sql"],
        "swift" => &["swift"],
        "toml" => &["toml"],
        "typescript" | "ts" | "tsx" => &["typescript", "typescript-js"],
        "xml" => &["xml"],
        "yaml" | "yml" => &["yaml"],
        _ => &[],
    }
}

fn language_extension(language: &str) -> Option<&'static str> {
    match language {
        "bash" | "shell" | "sh" | "zsh" => Some("sh"),
        "csharp" | "cs" => Some("cs"),
        "cpp" | "c++" => Some("cpp"),
        "javascript" | "js" | "jsx" => Some("js"),
        "markdown" | "md" => Some("md"),
        "go" | "golang" => Some("go"),
        "kotlin" | "kt" => Some("kt"),
        "python" | "py" => Some("py"),
        "ruby" | "rb" => Some("rb"),
        "rust" | "rs" => Some("rs"),
        "typescript" | "ts" | "tsx" => Some("ts"),
        "yaml" | "yml" => Some("yaml"),
        "c" => Some("c"),
        "css" => Some("css"),
        "html" => Some("html"),
        "java" => Some("java"),
        "json" => Some("json"),
        "php" => Some("php"),
        "sql" => Some("sql"),
        "swift" => Some("swift"),
        "toml" => Some("toml"),
        "xml" => Some("xml"),
        _ => None,
    }
}
