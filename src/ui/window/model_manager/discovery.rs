use crate::i18n::{gettext, gettext_noop};

use std::fs;

use super::{MODEL_FAMILIES, ModelFamily, ModelVariant};
use crate::models::model_names_match;

const GIB: u64 = 1024 * 1024 * 1024;

#[derive(Clone, Copy, Eq, PartialEq)]
pub(super) enum Task {
    Everyday,
    Coding,
    Images,
    Documents,
    All,
}

impl Task {
    pub(super) fn from_name(name: &str) -> Self {
        match name {
            "everyday" => Self::Everyday,
            "coding" => Self::Coding,
            "images" => Self::Images,
            "documents" => Self::Documents,
            _ => Self::All,
        }
    }

    pub(super) fn description(self) -> &'static str {
        match self {
            Self::Everyday => gettext_noop("For everyday questions, writing, and learning."),
            Self::Coding => gettext_noop("For writing, explaining, and troubleshooting code."),
            Self::Images => gettext_noop("For questions about photos, screenshots, and charts."),
            Self::Documents => {
                gettext_noop("For summarizing text and asking questions about documents.")
            }
            Self::All => gettext_noop("Explore every model family, including specialized models."),
        }
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub(super) enum Preference {
    Balanced,
    Lighter,
    Larger,
}

impl Preference {
    pub(super) fn from_index(index: u32) -> Self {
        match index {
            1 => Self::Lighter,
            2 => Self::Larger,
            _ => Self::Balanced,
        }
    }
}

pub(super) struct Filters {
    pub(super) task: Task,
    pub(super) preference: Preference,
    pub(super) max_download: Option<u64>,
    pub(super) images: bool,
    pub(super) tools: bool,
    pub(super) multilingual: bool,
}

impl Filters {
    pub(super) fn accepts(&self, family: &ModelFamily, variant: &ModelVariant) -> bool {
        if self
            .max_download
            .is_some_and(|limit| variant.download_bytes > limit)
            || ((self.images || self.task == Task::Images) && !supports_images(family, variant))
            || (self.tools && !supports_tools(family, variant))
            || (self.multilingual && !is_multilingual(family))
        {
            return false;
        }
        match self.task {
            Task::Everyday | Task::Documents => {
                family.tags.contains(&"chat")
                    && !family.tags.contains(&"specialized")
                    && !matches!(family.id, "codellama" | "deepseek-coder" | "qwen2.5-coder")
            }
            Task::Coding => family.tags.contains(&"coding"),
            Task::Images => !family.tags.contains(&"specialized"),
            Task::All => true,
        }
    }
}

#[derive(Clone, Copy, Default)]
pub(super) struct MemoryBudget {
    pub(super) available: Option<u64>,
}

impl MemoryBudget {
    pub(super) fn read(local: bool) -> Self {
        if !local {
            return Self::default();
        }
        let available = fs::read_to_string("/proc/meminfo").ok().and_then(|text| {
            text.lines().find_map(|line| {
                line.strip_prefix("MemAvailable:")?
                    .split_whitespace()
                    .next()?
                    .parse::<u64>()
                    .ok()?
                    .checked_mul(1024)
            })
        });
        Self { available }
    }

    fn pressure(self, download_bytes: u64) -> u8 {
        let Some(available) = self.available else {
            return 0;
        };
        let planning_size = Self::estimate_bytes(download_bytes);
        if planning_size <= available.saturating_mul(3) / 4 {
            0
        } else if planning_size <= available {
            1
        } else {
            2
        }
    }

    pub(super) fn estimate_bytes(download_bytes: u64) -> u64 {
        download_bytes
            .saturating_add(download_bytes / 4)
            .saturating_add(2 * GIB)
    }

    pub(super) fn guidance(self, download_bytes: u64) -> &'static str {
        if self.available.is_none() {
            return gettext_noop("Memory needs depend on the Ollama server");
        }
        match self.pressure(download_bytes) {
            0 => gettext_noop("Lower memory pressure expected"),
            1 => gettext_noop("May leave little memory for other apps"),
            _ => gettext_noop("May need more memory than is available"),
        }
    }
}

#[derive(Clone, Copy)]
pub(super) struct Candidate {
    pub(super) family: &'static ModelFamily,
    pub(super) variant: &'static ModelVariant,
    pub(super) memory: MemoryBudget,
}

impl Candidate {
    pub(super) fn capabilities(self) -> String {
        let mut features = vec![gettext(if supports_images(self.family, self.variant) {
            gettext_noop("Text and images")
        } else {
            gettext_noop("Text only")
        })];
        if supports_tools(self.family, self.variant) {
            features.push(gettext("Tools"));
        }
        if self.family.tags.contains(&"thinking") || self.variant.tags.contains(&"reasoning") {
            features.push(gettext("Reasoning"));
        }
        features.join(" · ")
    }

    pub(super) fn purpose(self) -> &'static str {
        if self.family.tags.contains(&"specialized") {
            gettext_noop("Specialized tasks")
        } else if self.family.tags.contains(&"translation") {
            gettext_noop("Translation")
        } else if self.family.tags.contains(&"coding") {
            gettext_noop("Coding and debugging")
        } else if supports_images(self.family, self.variant) {
            gettext_noop("Chat and images")
        } else if self.family.tags.contains(&"documents") {
            gettext_noop("Documents and summaries")
        } else if self.family.tags.contains(&"thinking") || self.family.tags.contains(&"reasoning")
        {
            gettext_noop("Complex questions")
        } else {
            gettext_noop("Chat and writing")
        }
    }
}

pub(super) fn discover(
    filters: &Filters,
    query: &str,
    installed_names: &[&str],
    memory: MemoryBudget,
) -> Vec<Candidate> {
    let mut candidates = MODEL_FAMILIES
        .iter()
        .filter_map(|family| {
            let family_match = family_matches(family, query);
            let mut variants = family
                .variants
                .iter()
                .filter(|variant| {
                    filters.accepts(family, variant)
                        && (!search_words(query).any(is_image_word)
                            || supports_images(family, variant))
                        && (family_match || variant_matches(variant, query))
                })
                .collect::<Vec<_>>();
            variants.sort_by_key(|variant| {
                let installed = installed_names
                    .iter()
                    .any(|name| model_names_match(name, variant.name));
                let preference = match filters.preference {
                    Preference::Lighter => variant.download_bytes,
                    Preference::Larger => u64::MAX - variant.download_bytes,
                    Preference::Balanced => {
                        let preferred = variant.tags.contains(&"recommended");
                        variant
                            .download_bytes
                            .saturating_add(if preferred { 0 } else { 16 * GIB })
                    }
                };
                (
                    memory.pressure(variant.download_bytes),
                    filters.preference == Preference::Balanced && !installed,
                    preference,
                )
            });
            variants.first().map(|variant| Candidate {
                family,
                variant,
                memory,
            })
        })
        .collect::<Vec<_>>();
    candidates.sort_by_key(|candidate| {
        let installed = installed_names
            .iter()
            .any(|name| model_names_match(name, candidate.variant.name));
        let preference = match filters.preference {
            Preference::Lighter => candidate.variant.download_bytes,
            _ => family_rank(candidate.family, filters.task),
        };
        (
            memory.pressure(candidate.variant.download_bytes),
            filters.preference == Preference::Balanced && !installed,
            preference,
            candidate.variant.download_bytes,
            candidate.family.title,
        )
    });
    candidates
}

fn family_rank(family: &ModelFamily, task: Task) -> u64 {
    let preferred: &[&str] = match task {
        Task::Coding => &[
            "qwen2.5-coder",
            "qwen3.5",
            "qwen3-coder",
            "rnj-1",
            "devstral-small-2",
        ],
        Task::Images => &[
            "gemma3",
            "qwen3-vl",
            "ministral-3",
            "qwen2.5vl",
            "minicpm-v4.6",
        ],
        Task::Documents => &["granite4.2", "qwen3", "gemma3", "qwen2.5", "phi4-mini"],
        _ => &[
            "gemma3",
            "qwen3.5",
            "qwen3",
            "llama3.2",
            "phi4-mini",
            "granite4.2",
        ],
    };
    preferred
        .iter()
        .position(|id| *id == family.id)
        .map_or(100, |index| index as u64)
}

pub(super) fn supports_images(family: &ModelFamily, variant: &ModelVariant) -> bool {
    match family.id {
        "gemma3" => matches!(variant.title, "4B" | "12B" | "27B"),
        "gemma4" | "qwen3.5" | "qwen3.6" => true,
        _ => variant.tags.contains(&"vision") || family.tags.contains(&"vision"),
    }
}

fn supports_tools(family: &ModelFamily, variant: &ModelVariant) -> bool {
    variant.tags.contains(&"tools") || family.tags.contains(&"tools")
}

fn is_multilingual(family: &ModelFamily) -> bool {
    family.tags.contains(&"multilingual")
        || matches!(
            family.id,
            "gemma3"
                | "gemma4"
                | "qwen3.5"
                | "qwen3.6"
                | "qwen3.8"
                | "llama3.1"
                | "llama3.2"
                | "llama3.3"
                | "ministral-3"
        )
}

fn family_matches(family: &ModelFamily, query: &str) -> bool {
    let text = format!(
        "{} {} {} {} {} {} {}",
        family.id,
        family.title,
        family.subtitle,
        family.description,
        gettext(family.subtitle),
        gettext(family.description),
        family.tags.join(" ")
    )
    .to_lowercase();
    search_words(query).all(|word| {
        text.contains(word)
            || match word {
                "photo" | "photos" | "picture" | "pictures" | "image" | "images" | "screenshot"
                | "screenshots" | "vision" => family
                    .variants
                    .iter()
                    .any(|variant| supports_images(family, variant)),
                "code" | "programming" | "program" | "debug" | "debugging" => {
                    family.tags.contains(&"coding")
                }
                "pdf" | "pdfs" | "document" | "documents" | "summarize" | "summary"
                | "summaries" | "writing" | "write" | "email" | "emails" => {
                    family.tags.contains(&"chat") && !family.tags.contains(&"specialized")
                }
                "translate" | "languages" => {
                    is_multilingual(family) || family.tags.contains(&"translation")
                }
                "math" | "reason" => {
                    family.tags.contains(&"reasoning")
                        || family.tags.contains(&"thinking")
                        || family.tags.contains(&"math")
                }
                "light" | "lightweight" | "quick" => {
                    family.tags.contains(&"small")
                        || family.tags.contains(&"fast")
                        || family.tags.contains(&"efficient")
                }
                _ => false,
            }
    })
}

fn variant_matches(variant: &ModelVariant, query: &str) -> bool {
    let text = format!(
        "{} {} {} {} {}",
        variant.name,
        variant.title,
        variant.description,
        gettext(variant.description),
        variant.tags.join(" ")
    )
    .to_lowercase();
    search_words(query).all(|word| text.contains(word))
}

fn search_words(query: &str) -> impl Iterator<Item = &str> {
    query.split_whitespace().filter(|word| {
        !matches!(
            *word,
            "a" | "an"
                | "and"
                | "the"
                | "for"
                | "to"
                | "with"
                | "me"
                | "my"
                | "help"
                | "about"
                | "of"
        )
    })
}

fn is_image_word(word: &str) -> bool {
    matches!(
        word,
        "photo"
            | "photos"
            | "picture"
            | "pictures"
            | "image"
            | "images"
            | "screenshot"
            | "screenshots"
            | "vision"
    )
}
