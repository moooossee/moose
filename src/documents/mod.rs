use serde::{Deserialize, Serialize};

#[cfg(feature = "gui")]
mod archive;
#[cfg(feature = "gui")]
pub mod content;
#[cfg(feature = "gui")]
mod office;
#[cfg(feature = "gui")]
mod opendocument;
#[cfg(feature = "gui")]
mod presentation;
#[cfg(feature = "gui")]
mod render;

#[cfg(feature = "gui")]
pub use render::{RenderedDocument, render};

pub const MAX_CONTENT_BYTES: usize = 512 * 1024;
pub const MAX_FILE_BYTES: usize = 32 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DocumentFormat {
    Pdf,
    Docx,
    Xlsx,
    Pptx,
    Odt,
    Ods,
    Odp,
    Markdown,
    Text,
    Csv,
    Html,
    Json,
    Xml,
}

impl DocumentFormat {
    pub const ALL: [Self; 13] = [
        Self::Pdf,
        Self::Docx,
        Self::Xlsx,
        Self::Pptx,
        Self::Odt,
        Self::Ods,
        Self::Odp,
        Self::Markdown,
        Self::Text,
        Self::Csv,
        Self::Html,
        Self::Json,
        Self::Xml,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Pdf => "PDF Document",
            Self::Docx => "Word Document",
            Self::Xlsx => "Excel Workbook",
            Self::Pptx => "PowerPoint Presentation",
            Self::Odt => "OpenDocument Text",
            Self::Ods => "OpenDocument Spreadsheet",
            Self::Odp => "OpenDocument Presentation",
            Self::Markdown => "Markdown",
            Self::Text => "Plain Text",
            Self::Csv => "CSV Table",
            Self::Html => "HTML Document",
            Self::Json => "JSON Data",
            Self::Xml => "XML Document",
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            Self::Pdf => "pdf",
            Self::Docx => "docx",
            Self::Xlsx => "xlsx",
            Self::Pptx => "pptx",
            Self::Odt => "odt",
            Self::Ods => "ods",
            Self::Odp => "odp",
            Self::Markdown => "md",
            Self::Text => "txt",
            Self::Csv => "csv",
            Self::Html => "html",
            Self::Json => "json",
            Self::Xml => "xml",
        }
    }

    pub fn mime_type(self) -> &'static str {
        match self {
            Self::Pdf => "application/pdf",
            Self::Docx => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
            Self::Xlsx => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
            Self::Pptx => {
                "application/vnd.openxmlformats-officedocument.presentationml.presentation"
            }
            Self::Odt => "application/vnd.oasis.opendocument.text",
            Self::Ods => "application/vnd.oasis.opendocument.spreadsheet",
            Self::Odp => "application/vnd.oasis.opendocument.presentation",
            Self::Markdown => "text/markdown",
            Self::Text => "text/plain",
            Self::Csv => "text/csv",
            Self::Html => "text/html",
            Self::Json => "application/json",
            Self::Xml => "application/xml",
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            Self::Pdf => "application-pdf-symbolic",
            Self::Xlsx | Self::Ods | Self::Csv => "x-office-spreadsheet-symbolic",
            Self::Pptx | Self::Odp => "x-office-presentation-symbolic",
            _ => "x-office-document-symbolic",
        }
    }

    pub fn is_spreadsheet(self) -> bool {
        matches!(self, Self::Xlsx | Self::Ods | Self::Csv)
    }

    pub fn is_presentation(self) -> bool {
        matches!(self, Self::Pptx | Self::Odp)
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|format| format.extension() == value)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DocumentRequest {
    pub title: String,
    pub format: DocumentFormat,
}

impl DocumentRequest {
    pub fn new(title: &str, format: DocumentFormat) -> Self {
        let title = title
            .chars()
            .filter(|ch| !ch.is_control())
            .take(120)
            .collect::<String>();
        let suffix = format!(".{}", format.extension());
        let title = if title.to_ascii_lowercase().ends_with(&suffix) {
            title[..title.len() - suffix.len()].trim().to_string()
        } else {
            title
        };
        Self {
            title: if title.trim().is_empty() {
                "Untitled Document".into()
            } else {
                title.trim().to_string()
            },
            format,
        }
    }

    pub fn from_prompt(prompt: &str) -> Option<Self> {
        let lower = prompt.trim().to_lowercase();
        if [
            "how ",
            "how do ",
            "how can ",
            "cómo ",
            "como ",
            "explain ",
            "explica ",
            "what is ",
            "qué es ",
            "que es ",
            "explícame ",
            "explicame ",
            "dime ",
        ]
        .iter()
        .any(|prefix| lower.starts_with(prefix))
        {
            return None;
        }
        let words = lower
            .split(|ch: char| !ch.is_alphanumeric())
            .filter(|word| !word.is_empty())
            .collect::<Vec<_>>();
        if !words.iter().any(|word| {
            matches!(
                *word,
                "create"
                    | "make"
                    | "generate"
                    | "prepare"
                    | "export"
                    | "convert"
                    | "save"
                    | "write"
                    | "crea"
                    | "creame"
                    | "créame"
                    | "crear"
                    | "genera"
                    | "generame"
                    | "genérame"
                    | "haz"
                    | "hazme"
                    | "prepara"
                    | "prepárame"
                    | "preparame"
                    | "exporta"
                    | "convierte"
                    | "guarda"
                    | "escribe"
            )
        }) {
            return None;
        }
        if words
            .iter()
            .any(|word| matches!(*word, "function" | "función" | "funcion" | "script"))
            && !words
                .iter()
                .any(|word| matches!(*word, "file" | "archivo" | "document" | "documento"))
        {
            return None;
        }
        let parse_format = |word: &&str| match *word {
            "pdf" => Some(DocumentFormat::Pdf),
            "word" | "docx" => Some(DocumentFormat::Docx),
            "excel" | "xlsx" | "spreadsheet" | "workbook" => Some(DocumentFormat::Xlsx),
            "powerpoint" | "pptx" | "presentation" | "presentación" | "presentacion" | "slides"
            | "diapositivas" => Some(DocumentFormat::Pptx),
            "odt" => Some(DocumentFormat::Odt),
            "ods" => Some(DocumentFormat::Ods),
            "odp" => Some(DocumentFormat::Odp),
            "markdown" | "md" => Some(DocumentFormat::Markdown),
            "txt" => Some(DocumentFormat::Text),
            "csv" => Some(DocumentFormat::Csv),
            "html" => Some(DocumentFormat::Html),
            "json" => Some(DocumentFormat::Json),
            "xml" => Some(DocumentFormat::Xml),
            _ => None,
        };
        let converting = words
            .iter()
            .any(|word| matches!(*word, "convert" | "convierte" | "export" | "exporta"));
        let format = if converting {
            words.iter().rev().find_map(parse_format)
        } else {
            words.iter().find_map(parse_format)
        }
        .or_else(|| {
            words
                .iter()
                .any(|word| matches!(*word, "document" | "documento" | "archivo" | "file"))
                .then_some(DocumentFormat::Pdf)
        })?;
        Some(Self::new("Untitled Document", format))
    }

    pub fn instruction(&self) -> String {
        let structure = match self.format {
            DocumentFormat::Json => {
                "Return only valid JSON. Do not wrap it in a code fence or add prose outside the JSON."
            }
            DocumentFormat::Csv => {
                "Return only RFC 4180 CSV with a header row. Quote fields containing commas, quotes, or line breaks. Do not use code fences."
            }
            format if format.is_spreadsheet() => {
                "Use Markdown tables for worksheets. Put a level-two heading before each worksheet. Keep each table rectangular. Write numbers without currency symbols or digit separators when numeric cells are needed. Formula cells may start with = and use ordinary Excel formulas and A1 references. Do not produce macros, external workbook references, or scripts."
            }
            format if format.is_presentation() => {
                "Use a level-one heading for the presentation title and a level-two heading for each slide title. Use short paragraphs or bullet points for slide content. Keep each slide concise and split long content across more slides."
            }
            _ => {
                "Write the complete document in Markdown with clear headings, paragraphs, lists, and tables where useful. Use a level-one heading for its title. Do not wrap the entire document in a code fence."
            }
        };
        let name = if self.title == "Untitled Document" {
            "Choose a concise title for the document.".to_string()
        } else {
            format!(
                "Use this document title: {}.",
                serde_json::to_string(&self.title).unwrap_or_default()
            )
        };
        format!(
            "The user is creating a {}. {name} Moose will generate the actual downloadable file from your response. Return only the finished document content in the user's language, without a conversational introduction, instructions for saving, a claim that a file was saved, or a closing offer. Preserve relevant facts from the conversation. Never invent facts about the user; use clearly marked placeholders for missing personal details. {structure}",
            self.format.label()
        )
    }
}

#[derive(Clone, Debug)]
pub struct DocumentJob {
    pub request: DocumentRequest,
    pub content: String,
    pub state: String,
    pub error: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct CreatedDocument {
    pub id: String,
    pub message_id: String,
    pub title: String,
    pub format: DocumentFormat,
    pub byte_size: i64,
    pub created_at: String,
}

impl CreatedDocument {
    pub fn filename(&self) -> String {
        filename(&self.title, self.format)
    }
}

pub fn filename(title: &str, format: DocumentFormat) -> String {
    let name = title
        .chars()
        .filter(|ch| !ch.is_control())
        .map(|ch| {
            if matches!(ch, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') {
                '_'
            } else {
                ch
            }
        })
        .take(100)
        .collect::<String>();
    let name = name.trim().trim_matches('.').trim();
    let name = &name[..name.floor_char_boundary(name.len().min(160))];
    format!(
        "{}.{}",
        if name.is_empty() { "Document" } else { name },
        format.extension()
    )
}
