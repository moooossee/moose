use crate::i18n::{gettext, gettext_f, gettext_noop};

use adw::prelude::*;
use gtk::{Align, Orientation, PolicyType, gio};
use std::{cell::Cell, rc::Rc};

use crate::{
    APPLICATION_ID,
    models::{ModelDownload, hugging_face_repository, model_names_match},
    ollama::{OllamaModel, OllamaPullProgress},
};

use super::widgets::icon_button;

mod discovery;
mod requirements;

use discovery::{Candidate, Filters, MemoryBudget, Preference, Task};

#[derive(Clone)]
pub(super) struct ModelManager {
    pub(super) root: gtk::Box,
    pub(super) pull_button: gtk::Button,
    pub(super) download_jobs_button: gtk::Button,
    pub(super) refresh_button: gtk::Button,
    pub(super) search_entry: gtk::SearchEntry,
    pub(super) task_filter: adw::ToggleGroup,
    pub(super) preference_filter: adw::ComboRow,
    pub(super) download_filter: adw::ComboRow,
    pub(super) images_filter: adw::SwitchRow,
    pub(super) tools_filter: adw::SwitchRow,
    pub(super) multilingual_filter: adw::SwitchRow,
    pub(super) reset_filters_button: gtk::Button,
    pub(super) changing_filters: Rc<Cell<bool>>,
    pub(super) pull_cancel_button: gtk::Button,
    busy: Rc<Cell<bool>>,
    filters_box: gtk::Box,
    filters_button: gtk::MenuButton,
    installed_group: adw::PreferencesGroup,
    memory_hint: gtk::Label,
    scroll: gtk::ScrolledWindow,
    pull_panel: gtk::Box,
    pull_row: adw::ActionRow,
    pull_activity: gtk::Stack,
    pull_icon: gtk::Image,
    pull_status: gtk::Label,
    pull_progress: gtk::ProgressBar,
    model_list: gtk::ListBox,
    available_model_list: gtk::ListBox,
    status_page: adw::StatusPage,
    stack: gtk::Stack,
}

struct ModelFamily {
    id: &'static str,
    title: &'static str,
    subtitle: &'static str,
    description: &'static str,
    tags: &'static [&'static str],
    variants: &'static [ModelVariant],
}

struct ModelVariant {
    name: &'static str,
    title: &'static str,
    description: &'static str,
    tags: &'static [&'static str],
    download_bytes: u64,
}

#[derive(Clone)]
struct ModelActions {
    download: Rc<dyn Fn(String)>,
    use_model: Rc<dyn Fn(String)>,
    download_control: gtk::Button,
}

const MODEL_FAMILIES: &[ModelFamily] = &[
    ModelFamily {
        id: "llama3.1",
        title: "Llama 3.1",
        subtitle: gettext_noop("Meta general chat models with tool-capable instruct variants"),
        description: gettext_noop(
            "Balanced general-purpose chat models from Meta. The 8B tag is the practical default for local machines, while 70B and 405B target larger systems.",
        ),
        tags: &["chat", "tools", "general", "meta"],
        variants: &[
            ModelVariant {
                name: "llama3.1:8b",
                title: "8B",
                description: gettext_noop("Recommended local default for everyday chat."),
                tags: &["recommended", "chat", "tools"],
                download_bytes: 4_920_753_328,
            },
            ModelVariant {
                name: "llama3.1:70b",
                title: "70B",
                description: gettext_noop("Larger model for stronger reasoning and writing."),
                tags: &["large", "chat", "tools"],
                download_bytes: 42_520_412_561,
            },
            ModelVariant {
                name: "llama3.1:405b",
                title: "405B",
                description: gettext_noop("Very large model for high-memory systems."),
                tags: &["very large", "chat"],
                download_bytes: 243_069_625_106,
            },
        ],
    },
    ModelFamily {
        id: "llama3.2",
        title: "Llama 3.2",
        subtitle: gettext_noop("Small Meta models for fast local chat"),
        description: gettext_noop(
            "Compact Llama models made for responsive local use. Good first downloads when you want speed and lower memory use.",
        ),
        tags: &["chat", "small", "fast", "meta"],
        variants: &[
            ModelVariant {
                name: "llama3.2:1b",
                title: "1B",
                description: gettext_noop("Very small and fast for quick responses."),
                tags: &["tiny", "fast", "chat"],
                download_bytes: 1_321_098_329,
            },
            ModelVariant {
                name: "llama3.2:3b",
                title: "3B",
                description: gettext_noop("Small but more capable than 1B."),
                tags: &["recommended", "small", "chat"],
                download_bytes: 2_019_393_189,
            },
        ],
    },
    ModelFamily {
        id: "llama3.3",
        title: "Llama 3.3",
        subtitle: gettext_noop("Popular 70B Meta model with strong quality for its size"),
        description: gettext_noop(
            "A newer Llama 70B option that Ollama lists among the popular models. Use this when you want high quality and have enough memory for a large model.",
        ),
        tags: &["chat", "tools", "large", "meta", "popular"],
        variants: &[
            ModelVariant {
                name: "llama3.3:70b",
                title: "70B",
                description: gettext_noop("Default Llama 3.3 70B tag."),
                tags: &["recommended", "large", "chat", "tools"],
                download_bytes: 42_520_413_916,
            },
            ModelVariant {
                name: "llama3.3:70b-instruct-q4_K_M",
                title: "70B Q4",
                description: gettext_noop("Quantized instruction variant for large local systems."),
                tags: &["large", "q4", "chat"],
                download_bytes: 42_520_413_916,
            },
        ],
    },
    ModelFamily {
        id: "llama3",
        title: "Llama 3",
        subtitle: gettext_noop("Older but still heavily used Meta chat models"),
        description: gettext_noop(
            "Still one of the high-pull Llama families on Ollama. The 8B variant remains a useful baseline when comparing newer local models.",
        ),
        tags: &["chat", "meta", "popular"],
        variants: &[
            ModelVariant {
                name: "llama3:8b",
                title: "8B",
                description: gettext_noop("Classic local Llama 3 baseline."),
                tags: &["popular", "chat"],
                download_bytes: 4_661_224_676,
            },
            ModelVariant {
                name: "llama3:8b-instruct-q4_K_M",
                title: "8B Q4",
                description: gettext_noop("Quantized instruct variant."),
                tags: &["q4", "chat"],
                download_bytes: 4_920_747_238,
            },
            ModelVariant {
                name: "llama3:70b",
                title: "70B",
                description: gettext_noop("Large Llama 3 variant."),
                tags: &["large", "chat"],
                download_bytes: 39_969_745_349,
            },
        ],
    },
    ModelFamily {
        id: "deepseek-r1",
        title: "DeepSeek R1",
        subtitle: gettext_noop("Popular reasoning model family"),
        description: gettext_noop(
            "Reasoning-focused models and distilled variants. Smaller tags are useful locally; larger tags need much more memory.",
        ),
        tags: &["chat", "reasoning", "thinking"],
        variants: &[
            ModelVariant {
                name: "deepseek-r1:1.5b",
                title: "1.5B",
                description: gettext_noop("Tiny reasoning distill for light machines."),
                tags: &["tiny", "reasoning"],
                download_bytes: 1_117_322_768,
            },
            ModelVariant {
                name: "deepseek-r1:7b",
                title: "7B",
                description: gettext_noop("Popular local reasoning default."),
                tags: &["recommended", "reasoning", "chat"],
                download_bytes: 4_683_075_440,
            },
            ModelVariant {
                name: "deepseek-r1:8b",
                title: "8B",
                description: gettext_noop("Qwen3-based refreshed distill."),
                tags: &["reasoning", "chat"],
                download_bytes: 5_225_376_047,
            },
            ModelVariant {
                name: "deepseek-r1:14b",
                title: "14B",
                description: gettext_noop("Stronger reasoning on mid-range hardware."),
                tags: &["medium", "reasoning"],
                download_bytes: 8_988_112_209,
            },
            ModelVariant {
                name: "deepseek-r1:32b",
                title: "32B",
                description: gettext_noop("High quality reasoning for large local setups."),
                tags: &["large", "reasoning"],
                download_bytes: 19_851_337_809,
            },
            ModelVariant {
                name: "deepseek-r1:70b",
                title: "70B",
                description: gettext_noop("Large reasoning model for high-memory systems."),
                tags: &["large", "reasoning"],
                download_bytes: 42_520_397_873,
            },
        ],
    },
    ModelFamily {
        id: "gemma3",
        title: "Gemma 3",
        subtitle: gettext_noop("Efficient Google models with small and vision-capable sizes"),
        description: gettext_noop(
            "A broad family that scales from tiny local assistants to larger, more capable models. Useful when you want efficient everyday chat.",
        ),
        tags: &["chat", "vision", "efficient", "google"],
        variants: &[
            ModelVariant {
                name: "gemma3:270m",
                title: "270M",
                description: gettext_noop("Very small instruction model."),
                tags: &["tiny", "fast"],
                download_bytes: 291_554_930,
            },
            ModelVariant {
                name: "gemma3:1b",
                title: "1B",
                description: gettext_noop("Small and responsive local chat."),
                tags: &["small", "chat"],
                download_bytes: 806_067_407,
            },
            ModelVariant {
                name: "gemma3:4b",
                title: "4B",
                description: gettext_noop("Recommended local balance for Gemma."),
                tags: &["recommended", "chat", "vision"],
                download_bytes: 3_350_272_403,
            },
            ModelVariant {
                name: "gemma3:12b",
                title: "12B",
                description: gettext_noop("Better quality for machines with more memory."),
                tags: &["medium", "chat", "vision"],
                download_bytes: 8_164_039_732,
            },
            ModelVariant {
                name: "gemma3:27b",
                title: "27B",
                description: gettext_noop("Large Gemma model for stronger answers."),
                tags: &["large", "chat", "vision"],
                download_bytes: 17_413_409_652,
            },
        ],
    },
    ModelFamily {
        id: "gemma3n",
        title: "Gemma 3n",
        subtitle: gettext_noop("Efficient Gemma models for everyday devices"),
        description: gettext_noop(
            "Compact Gemma 3n variants designed for efficient execution on laptops and everyday devices. Good when you want newer Gemma behavior without a huge model.",
        ),
        tags: &["chat", "small", "efficient", "google"],
        variants: &[
            ModelVariant {
                name: "gemma3n:e2b",
                title: "E2B",
                description: gettext_noop("Smaller efficient Gemma 3n variant."),
                tags: &["small", "efficient"],
                download_bytes: 7_836_324_540,
            },
            ModelVariant {
                name: "gemma3n:e4b",
                title: "E4B",
                description: gettext_noop("More capable efficient Gemma 3n variant."),
                tags: &["recommended", "small", "efficient"],
                download_bytes: 9_768_581_852,
            },
            ModelVariant {
                name: "gemma3n:e4b-it-q4_K_M",
                title: "E4B IT Q4",
                description: gettext_noop("Quantized instruction-tuned tag."),
                tags: &["q4", "chat"],
                download_bytes: 9_768_581_852,
            },
        ],
    },
    ModelFamily {
        id: "gemma2",
        title: "Gemma 2",
        subtitle: gettext_noop("Very popular efficient Google model family"),
        description: gettext_noop(
            "Older than Gemma 3, but still high on Ollama's popularity list. Useful for comparing small and medium Google models.",
        ),
        tags: &["chat", "google", "popular"],
        variants: &[
            ModelVariant {
                name: "gemma2:2b",
                title: "2B",
                description: gettext_noop("Small Gemma 2 model."),
                tags: &["small", "chat"],
                download_bytes: 1_629_518_495,
            },
            ModelVariant {
                name: "gemma2:9b",
                title: "9B",
                description: gettext_noop("Balanced Gemma 2 model."),
                tags: &["recommended", "chat"],
                download_bytes: 5_443_152_417,
            },
            ModelVariant {
                name: "gemma2:27b",
                title: "27B",
                description: gettext_noop("Large Gemma 2 model."),
                tags: &["large", "chat"],
                download_bytes: 15_628_387_458,
            },
        ],
    },
    ModelFamily {
        id: "gemma4",
        title: "Gemma 4",
        subtitle: gettext_noop("Newer Gemma family with local and larger variants"),
        description: gettext_noop(
            "Recent Gemma models with compact expert variants and larger instruction tags. Good to try when you want the newest Google family available in Ollama.",
        ),
        tags: &["chat", "google", "new"],
        variants: &[
            ModelVariant {
                name: "gemma4:e2b",
                title: "E2B",
                description: gettext_noop("Compact expert variant for lighter local use."),
                tags: &["small", "chat"],
                download_bytes: 4_587_156_505,
            },
            ModelVariant {
                name: "gemma4:e4b",
                title: "E4B",
                description: gettext_noop("Small expert variant with more capacity."),
                tags: &["small", "chat"],
                download_bytes: 6_583_656_505,
            },
            ModelVariant {
                name: "gemma4:12b",
                title: "12B",
                description: gettext_noop(
                    "Mid-sized Gemma 4 model for chat and image understanding.",
                ),
                tags: &["recommended", "medium", "chat", "vision"],
                download_bytes: 8_021_618_941,
            },
            ModelVariant {
                name: "gemma4:26b",
                title: "26B",
                description: gettext_noop("Larger instruction model."),
                tags: &["large", "chat"],
                download_bytes: 18_731_025_629,
            },
            ModelVariant {
                name: "gemma4:31b",
                title: "31B",
                description: gettext_noop("Large Gemma 4 variant for strong quality."),
                tags: &["large", "chat"],
                download_bytes: 20_385_458_234,
            },
        ],
    },
    ModelFamily {
        id: "translategemma",
        title: "TranslateGemma",
        subtitle: gettext_noop("Recent Gemma translation models"),
        description: gettext_noop(
            "A newer Gemma-based translation family for communication across many languages. Useful when translation is the main workflow.",
        ),
        tags: &["translation", "multilingual", "google", "new"],
        variants: &[
            ModelVariant {
                name: "translategemma:4b",
                title: "4B",
                description: gettext_noop("Small translation model."),
                tags: &["recommended", "translation"],
                download_bytes: 3_298_875_707,
            },
            ModelVariant {
                name: "translategemma:12b",
                title: "12B",
                description: gettext_noop("Medium translation model."),
                tags: &["translation", "medium"],
                download_bytes: 8_109_827_612,
            },
            ModelVariant {
                name: "translategemma:27b",
                title: "27B",
                description: gettext_noop("Large translation model."),
                tags: &["translation", "large"],
                download_bytes: 17_358_650_268,
            },
        ],
    },
    ModelFamily {
        id: "medgemma",
        title: "MedGemma",
        subtitle: gettext_noop("Recent specialized Gemma medical models"),
        description: gettext_noop(
            "Specialized Gemma variants for medical text and image comprehension. They are included for specialized workflows and should not replace professional medical judgment.",
        ),
        tags: &["specialized", "medical", "vision", "new"],
        variants: &[
            ModelVariant {
                name: "medgemma:4b",
                title: "4B",
                description: gettext_noop("Small specialized medical model."),
                tags: &["specialized", "vision"],
                download_bytes: 3_338_942_556,
            },
            ModelVariant {
                name: "medgemma:27b",
                title: "27B",
                description: gettext_noop("Large specialized medical model."),
                tags: &["large", "specialized"],
                download_bytes: 17_397_224_125,
            },
        ],
    },
    ModelFamily {
        id: "qwen2.5",
        title: "Qwen 2.5",
        subtitle: gettext_noop("Strong multilingual general models"),
        description: gettext_noop(
            "Popular multilingual chat models with many sizes. The 7B tag is a reliable local default, and larger tags improve quality when hardware allows.",
        ),
        tags: &["chat", "multilingual", "tools"],
        variants: &[
            ModelVariant {
                name: "qwen2.5:0.5b",
                title: "0.5B",
                description: gettext_noop("Tiny multilingual model."),
                tags: &["tiny", "fast"],
                download_bytes: 397_821_319,
            },
            ModelVariant {
                name: "qwen2.5:1.5b",
                title: "1.5B",
                description: gettext_noop("Small multilingual chat."),
                tags: &["small", "chat"],
                download_bytes: 986_061_892,
            },
            ModelVariant {
                name: "qwen2.5:3b",
                title: "3B",
                description: gettext_noop("Lightweight everyday chat."),
                tags: &["small", "chat"],
                download_bytes: 1_929_912_432,
            },
            ModelVariant {
                name: "qwen2.5:7b",
                title: "7B",
                description: gettext_noop("Recommended local default."),
                tags: &["recommended", "chat", "tools"],
                download_bytes: 4_683_087_332,
            },
            ModelVariant {
                name: "qwen2.5:14b",
                title: "14B",
                description: gettext_noop("Stronger multilingual answers."),
                tags: &["medium", "chat"],
                download_bytes: 8_988_124_069,
            },
            ModelVariant {
                name: "qwen2.5:32b",
                title: "32B",
                description: gettext_noop("Large local model for better quality."),
                tags: &["large", "chat"],
                download_bytes: 19_851_349_669,
            },
            ModelVariant {
                name: "qwen2.5:72b",
                title: "72B",
                description: gettext_noop("Very large multilingual model."),
                tags: &["very large", "chat"],
                download_bytes: 47_415_724_625,
            },
        ],
    },
    ModelFamily {
        id: "qwen3",
        title: "Qwen 3",
        subtitle: gettext_noop("Popular multilingual reasoning models"),
        description: gettext_noop(
            "Reasoning-capable Qwen models with small dense tags and larger mixture-of-experts tags. Strong choice for multilingual work.",
        ),
        tags: &["chat", "reasoning", "multilingual", "thinking"],
        variants: &[
            ModelVariant {
                name: "qwen3:0.6b",
                title: "0.6B",
                description: gettext_noop("Tiny Qwen 3 chat."),
                tags: &["tiny", "fast"],
                download_bytes: 522_653_767,
            },
            ModelVariant {
                name: "qwen3:1.7b",
                title: "1.7B",
                description: gettext_noop("Small reasoning model."),
                tags: &["small", "thinking"],
                download_bytes: 1_359_293_444,
            },
            ModelVariant {
                name: "qwen3:4b",
                title: "4B",
                description: gettext_noop("Recommended small reasoning model."),
                tags: &["recommended", "thinking"],
                download_bytes: 2_497_293_931,
            },
            ModelVariant {
                name: "qwen3:8b",
                title: "8B",
                description: gettext_noop("Balanced reasoning and chat."),
                tags: &["chat", "thinking"],
                download_bytes: 5_225_388_164,
            },
            ModelVariant {
                name: "qwen3:14b",
                title: "14B",
                description: gettext_noop("Stronger reasoning on mid-range hardware."),
                tags: &["medium", "thinking"],
                download_bytes: 9_276_198_565,
            },
            ModelVariant {
                name: "qwen3:30b-a3b",
                title: "30B A3B",
                description: gettext_noop("Mixture-of-experts model with active 3B path."),
                tags: &["moe", "thinking"],
                download_bytes: 18_556_699_314,
            },
            ModelVariant {
                name: "qwen3:32b",
                title: "32B",
                description: gettext_noop("Large dense Qwen 3 model."),
                tags: &["large", "thinking"],
                download_bytes: 20_201_253_829,
            },
        ],
    },
    ModelFamily {
        id: "qwen3.5",
        title: "Qwen 3.5",
        subtitle: gettext_noop("Newer Qwen family with compact and MoE tags"),
        description: gettext_noop(
            "Recent Qwen models with small local sizes and larger expert variants. Useful for multilingual chat, coding and reasoning.",
        ),
        tags: &["chat", "reasoning", "coding", "new"],
        variants: &[
            ModelVariant {
                name: "qwen3.5:0.8b",
                title: "0.8B",
                description: gettext_noop("Tiny fast model."),
                tags: &["tiny", "fast"],
                download_bytes: 1_322_069_277,
            },
            ModelVariant {
                name: "qwen3.5:2b",
                title: "2B",
                description: gettext_noop("Small general model."),
                tags: &["small", "chat"],
                download_bytes: 2_683_396_812,
            },
            ModelVariant {
                name: "qwen3.5:4b",
                title: "4B",
                description: gettext_noop("Practical recent Qwen model for local chat."),
                tags: &["recommended", "chat"],
                download_bytes: 3_324_173_934,
            },
            ModelVariant {
                name: "qwen3.5:9b",
                title: "9B",
                description: gettext_noop("Stronger recent Qwen model for local systems."),
                tags: &["chat", "thinking"],
                download_bytes: 6_550_825_550,
            },
            ModelVariant {
                name: "qwen3.5:27b",
                title: "27B",
                description: gettext_noop("Large dense model with coding variants."),
                tags: &["large", "coding"],
                download_bytes: 17_478_556_591,
            },
            ModelVariant {
                name: "qwen3.5:35b-a3b",
                title: "35B A3B",
                description: gettext_noop("Mixture-of-experts model for larger local setups."),
                tags: &["moe", "coding"],
                download_bytes: 22_069_592_181,
            },
        ],
    },
    ModelFamily {
        id: "qwen3.6",
        title: "Qwen 3.6",
        subtitle: gettext_noop("Recent Qwen upgrade for coding and thinking"),
        description: gettext_noop(
            "A newer Qwen generation shown in Ollama's recent model listings. It focuses on agentic coding and better thinking preservation.",
        ),
        tags: &["chat", "coding", "thinking", "new"],
        variants: &[
            ModelVariant {
                name: "qwen3.6:27b",
                title: "27B",
                description: gettext_noop("Dense recent Qwen model."),
                tags: &["large", "chat", "thinking"],
                download_bytes: 17_769_076_933,
            },
            ModelVariant {
                name: "qwen3.6:27b-q4_K_M",
                title: "27B Q4",
                description: gettext_noop("Quantized dense recent Qwen model."),
                tags: &["q4", "coding", "thinking"],
                download_bytes: 17_420_432_475,
            },
            ModelVariant {
                name: "qwen3.6:35b-a3b",
                title: "35B A3B",
                description: gettext_noop("Mixture-of-experts recent Qwen model."),
                tags: &["moe", "coding", "thinking"],
                download_bytes: 22_621_314_381,
            },
            ModelVariant {
                name: "qwen3.6:35b-a3b-q4_K_M",
                title: "35B A3B Q4",
                description: gettext_noop("Quantized MoE variant."),
                tags: &["q4", "moe", "coding"],
                download_bytes: 23_938_333_577,
            },
        ],
    },
    ModelFamily {
        id: "qwen3.8",
        title: "Qwen 3.8",
        subtitle: gettext_noop("Multimodal Qwen model for reasoning and coding"),
        description: gettext_noop(
            "A 27B model for chat, programming, research and image understanding. Suitable for local systems with more memory or a remote Ollama provider.",
        ),
        tags: &["chat", "coding", "vision", "thinking", "large"],
        variants: &[ModelVariant {
            name: "qwen3.8:27b",
            title: "27B",
            description: gettext_noop("Large Qwen model with image support and reasoning."),
            tags: &["recommended", "large", "vision", "thinking"],
            download_bytes: 17_741_872_154,
        }],
    },
    ModelFamily {
        id: "gpt-oss",
        title: "GPT OSS",
        subtitle: gettext_noop("Open-weight GPT-style models"),
        description: gettext_noop(
            "Open-weight models intended for capable local and workstation setups. The 20B tag is the practical local starting point.",
        ),
        tags: &["chat", "reasoning", "open weights"],
        variants: &[
            ModelVariant {
                name: "gpt-oss:20b",
                title: "20B",
                description: gettext_noop("Recommended local GPT OSS variant."),
                tags: &["recommended", "chat"],
                download_bytes: 13_793_441_566,
            },
            ModelVariant {
                name: "gpt-oss:120b",
                title: "120B",
                description: gettext_noop("Very large GPT OSS variant."),
                tags: &["very large", "chat"],
                download_bytes: 65_369_819_583,
            },
        ],
    },
    ModelFamily {
        id: "mistral",
        title: "Mistral",
        subtitle: gettext_noop("Fast general chat model"),
        description: gettext_noop(
            "Classic Mistral 7B family. Still useful for fast local chat and lightweight instruction tasks.",
        ),
        tags: &["chat", "fast"],
        variants: &[
            ModelVariant {
                name: "mistral:7b",
                title: "7B",
                description: gettext_noop("Default Mistral local chat model."),
                tags: &["recommended", "chat"],
                download_bytes: 4_372_824_384,
            },
            ModelVariant {
                name: "mistral:7b-instruct-v0.3-q4_K_M",
                title: "7B Instruct v0.3 Q4",
                description: gettext_noop("Quantized instruction variant."),
                tags: &["instruct", "q4", "chat"],
                download_bytes: 4_372_824_384,
            },
        ],
    },
    ModelFamily {
        id: "mistral-nemo",
        title: "Mistral Nemo",
        subtitle: gettext_noop("Popular 12B Mistral model with long context"),
        description: gettext_noop(
            "A high-pull Mistral family built with NVIDIA collaboration. Useful as a stronger local alternative to classic Mistral 7B.",
        ),
        tags: &["chat", "tools", "mistral", "popular"],
        variants: &[
            ModelVariant {
                name: "mistral-nemo:12b",
                title: "12B",
                description: gettext_noop("Default Mistral Nemo model."),
                tags: &["recommended", "medium", "chat"],
                download_bytes: 7_071_713_227,
            },
            ModelVariant {
                name: "mistral-nemo:12b-instruct-2407-q4_K_M",
                title: "12B Q4",
                description: gettext_noop("Quantized instruction variant."),
                tags: &["q4", "chat"],
                download_bytes: 7_477_217_229,
            },
        ],
    },
    ModelFamily {
        id: "mistral-small",
        title: "Mistral Small",
        subtitle: gettext_noop("Popular 22B and 24B Mistral Small models"),
        description: gettext_noop(
            "A widely used Mistral family below 70B. Good for stronger local chat when 7B models feel too small.",
        ),
        tags: &["chat", "tools", "mistral", "popular"],
        variants: &[
            ModelVariant {
                name: "mistral-small:22b",
                title: "22B",
                description: gettext_noop("Earlier Mistral Small tag."),
                tags: &["large", "chat"],
                download_bytes: 12_569_172_141,
            },
            ModelVariant {
                name: "mistral-small:24b",
                title: "24B",
                description: gettext_noop("Newer Mistral Small tag."),
                tags: &["recommended", "large", "chat"],
                download_bytes: 14_333_921_662,
            },
            ModelVariant {
                name: "mistral-small:24b-instruct-2501-q4_K_M",
                title: "24B Q4",
                description: gettext_noop("Quantized instruction variant."),
                tags: &["q4", "chat"],
                download_bytes: 14_333_921_662,
            },
        ],
    },
    ModelFamily {
        id: "mistral-small3.2",
        title: "Mistral Small 3.2",
        subtitle: gettext_noop("Modern 24B Mistral instruction model"),
        description: gettext_noop(
            "A newer 24B Mistral model for stronger general chat when you have enough memory.",
        ),
        tags: &["chat", "vision", "mistral"],
        variants: &[
            ModelVariant {
                name: "mistral-small3.2:24b",
                title: "24B",
                description: gettext_noop("Default 24B instruction model."),
                tags: &["large", "chat"],
                download_bytes: 15_177_384_862,
            },
            ModelVariant {
                name: "mistral-small3.2:24b-instruct-2506-q4_K_M",
                title: "24B Q4",
                description: gettext_noop("Quantized instruction variant."),
                tags: &["large", "q4", "chat"],
                download_bytes: 15_177_384_862,
            },
        ],
    },
    ModelFamily {
        id: "mistral-medium-3.5",
        title: "Mistral Medium 3.5",
        subtitle: gettext_noop("Recent 128B flagship Mistral model"),
        description: gettext_noop(
            "A recent Mistral model shown in Ollama's newest listings. It is large, but useful to expose for users with workstation or remote Ollama providers.",
        ),
        tags: &["chat", "vision", "tools", "thinking", "new"],
        variants: &[
            ModelVariant {
                name: "mistral-medium-3.5:128b",
                title: "128B",
                description: gettext_noop("Default Mistral Medium 3.5 tag."),
                tags: &["very large", "chat"],
                download_bytes: 80_241_475_099,
            },
            ModelVariant {
                name: "mistral-medium-3.5:128b-q4_K_M",
                title: "128B Q4",
                description: gettext_noop("Quantized large Mistral Medium tag."),
                tags: &["q4", "very large"],
                download_bytes: 80_241_475_099,
            },
        ],
    },
    ModelFamily {
        id: "ministral-3",
        title: "Ministral 3",
        subtitle: gettext_noop("Recent edge-friendly Mistral family"),
        description: gettext_noop(
            "A newer Mistral family designed for edge deployment with 3B, 8B and 14B sizes. Good fit for laptops and local-first use.",
        ),
        tags: &["chat", "vision", "tools", "new", "mistral"],
        variants: &[
            ModelVariant {
                name: "ministral-3:3b",
                title: "3B",
                description: gettext_noop("Small edge-friendly model."),
                tags: &["small", "fast"],
                download_bytes: 2_953_840_808,
            },
            ModelVariant {
                name: "ministral-3:8b",
                title: "8B",
                description: gettext_noop("Balanced local Ministral model."),
                tags: &["recommended", "chat"],
                download_bytes: 6_022_236_616,
            },
            ModelVariant {
                name: "ministral-3:14b",
                title: "14B",
                description: gettext_noop("Stronger local Ministral model."),
                tags: &["medium", "chat"],
                download_bytes: 9_082_537_546,
            },
        ],
    },
    ModelFamily {
        id: "muse-glimmer",
        title: "Muse Glimmer",
        subtitle: gettext_noop("Multimodal Meta model for chat and reasoning"),
        description: gettext_noop(
            "A 30B model for multilingual chat, programming and questions about images, screenshots and charts. Suitable for local systems with more memory or a remote Ollama provider.",
        ),
        tags: &["chat", "vision", "thinking", "tools", "meta"],
        variants: &[ModelVariant {
            name: "muse-glimmer:30b",
            title: "30B",
            description: gettext_noop(
                "Large local model with image understanding and reasoning support.",
            ),
            tags: &["recommended", "large", "vision", "thinking"],
            download_bytes: 18_157_010_252,
        }],
    },
    ModelFamily {
        id: "granite4.2",
        title: "Granite 4.2",
        subtitle: gettext_noop("Multilingual IBM models for chat and document questions"),
        description: gettext_noop(
            "Text models for summarization, document questions, coding and structured output. Supports Spanish and other languages, with built-in reasoning.",
        ),
        tags: &["chat", "documents", "multilingual", "thinking", "ibm"],
        variants: &[
            ModelVariant {
                name: "granite4.2:3b",
                title: "3B",
                description: gettext_noop(
                    "Small model for multilingual chat and document summaries.",
                ),
                tags: &["small", "chat", "documents", "thinking"],
                download_bytes: 2_244_023_965,
            },
            ModelVariant {
                name: "granite4.2:8b",
                title: "8B",
                description: gettext_noop(
                    "Recommended variant for document questions and everyday chat.",
                ),
                tags: &["recommended", "chat", "documents", "thinking"],
                download_bytes: 5_347_929_757,
            },
        ],
    },
    ModelFamily {
        id: "phi4",
        title: "Phi 4",
        subtitle: gettext_noop("Compact Microsoft model for reasoning and chat"),
        description: gettext_noop(
            "A compact 14B model that works well for practical local assistant tasks.",
        ),
        tags: &["chat", "reasoning", "microsoft"],
        variants: &[
            ModelVariant {
                name: "phi4:14b",
                title: "14B",
                description: gettext_noop("Default Phi 4 model."),
                tags: &["recommended", "chat"],
                download_bytes: 9_053_116_391,
            },
            ModelVariant {
                name: "phi4:14b-q4_K_M",
                title: "14B Q4",
                description: gettext_noop("Quantized Phi 4 variant."),
                tags: &["q4", "chat"],
                download_bytes: 9_053_116_391,
            },
        ],
    },
    ModelFamily {
        id: "phi4-mini",
        title: "Phi 4 Mini",
        subtitle: gettext_noop("Lightweight multilingual Microsoft model"),
        description: gettext_noop(
            "A compact 3.8B text model for everyday chat, mathematics and logical reasoning. Designed for responsive local use on systems with limited memory.",
        ),
        tags: &["chat", "small", "reasoning", "multilingual", "microsoft"],
        variants: &[ModelVariant {
            name: "phi4-mini:3.8b",
            title: "3.8B",
            description: gettext_noop(
                "Small local model for multilingual chat and practical reasoning.",
            ),
            tags: &["recommended", "small", "chat", "reasoning"],
            download_bytes: 2_491_876_774,
        }],
    },
    ModelFamily {
        id: "phi4-reasoning",
        title: "Phi 4 Reasoning",
        subtitle: gettext_noop("Microsoft reasoning model in a compact 14B size"),
        description: gettext_noop(
            "A reasoning-focused Phi 4 family. Useful when you want a smaller model for multi-step reasoning without jumping to very large models.",
        ),
        tags: &["chat", "reasoning", "thinking", "microsoft"],
        variants: &[
            ModelVariant {
                name: "phi4-reasoning:14b",
                title: "14B",
                description: gettext_noop("Default Phi 4 reasoning model."),
                tags: &["recommended", "reasoning"],
                download_bytes: 11_117_503_233,
            },
            ModelVariant {
                name: "phi4-reasoning:14b-q4_K_M",
                title: "14B Q4",
                description: gettext_noop("Quantized reasoning variant."),
                tags: &["q4", "reasoning"],
                download_bytes: 11_117_503_233,
            },
            ModelVariant {
                name: "phi4-reasoning:plus",
                title: "Plus",
                description: gettext_noop("Reasoning-plus variant."),
                tags: &["reasoning", "chat"],
                download_bytes: 11_117_503_233,
            },
        ],
    },
    ModelFamily {
        id: "phi3",
        title: "Phi 3",
        subtitle: gettext_noop("Small Microsoft models"),
        description: gettext_noop(
            "Older but useful compact Microsoft models for low-memory local machines.",
        ),
        tags: &["chat", "small", "microsoft"],
        variants: &[
            ModelVariant {
                name: "phi3:3.8b",
                title: "3.8B",
                description: gettext_noop("Small Phi 3 mini model."),
                tags: &["small", "chat"],
                download_bytes: 2_176_178_913,
            },
            ModelVariant {
                name: "phi3:14b",
                title: "14B",
                description: gettext_noop("Medium Phi 3 model."),
                tags: &["medium", "chat"],
                download_bytes: 7_897_127_458,
            },
        ],
    },
    ModelFamily {
        id: "qwen2.5-coder",
        title: "Qwen 2.5 Coder",
        subtitle: gettext_noop("Coding-specialized Qwen models"),
        description: gettext_noop(
            "Popular code-focused models for completion, explanation and software tasks. The 7B tag is a good local start.",
        ),
        tags: &["coding", "chat", "multilingual"],
        variants: &[
            ModelVariant {
                name: "qwen2.5-coder:1.5b",
                title: "1.5B",
                description: gettext_noop("Small code model."),
                tags: &["small", "coding"],
                download_bytes: 986_062_089,
            },
            ModelVariant {
                name: "qwen2.5-coder:7b",
                title: "7B",
                description: gettext_noop("Recommended local coding model."),
                tags: &["recommended", "coding"],
                download_bytes: 4_683_087_561,
            },
            ModelVariant {
                name: "qwen2.5-coder:14b",
                title: "14B",
                description: gettext_noop("Stronger coding model."),
                tags: &["medium", "coding"],
                download_bytes: 8_988_124_298,
            },
            ModelVariant {
                name: "qwen2.5-coder:32b",
                title: "32B",
                description: gettext_noop("Large coding model."),
                tags: &["large", "coding"],
                download_bytes: 19_851_349_898,
            },
        ],
    },
    ModelFamily {
        id: "qwen3-coder",
        title: "Qwen 3 Coder",
        subtitle: gettext_noop("Popular Qwen coding models for agentic tasks"),
        description: gettext_noop(
            "Qwen's newer coding family appears high in Ollama's popular list. The 30B A3B tag is the practical local starting point.",
        ),
        tags: &["coding", "tools", "popular"],
        variants: &[
            ModelVariant {
                name: "qwen3-coder:30b",
                title: "30B A3B",
                description: gettext_noop("Default local Qwen 3 Coder tag."),
                tags: &["recommended", "coding", "moe"],
                download_bytes: 18_556_700_761,
            },
            ModelVariant {
                name: "qwen3-coder:30b-a3b-q4_K_M",
                title: "30B A3B Q4",
                description: gettext_noop("Quantized local coding variant."),
                tags: &["q4", "coding", "moe"],
                download_bytes: 18_556_700_761,
            },
            ModelVariant {
                name: "qwen3-coder:480b",
                title: "480B A35B",
                description: gettext_noop("Very large Qwen coding model."),
                tags: &["very large", "coding", "moe"],
                download_bytes: 290_058_836_445,
            },
        ],
    },
    ModelFamily {
        id: "qwen3-coder-next",
        title: "Qwen 3 Coder Next",
        subtitle: gettext_noop("Recent coding-focused Qwen model"),
        description: gettext_noop(
            "A newer Qwen coding model from Ollama's recent list. Include it for users who want the freshest coding tag.",
        ),
        tags: &["coding", "tools", "new"],
        variants: &[
            ModelVariant {
                name: "qwen3-coder-next:q4_K_M",
                title: "Q4",
                description: gettext_noop("Quantized local coding tag."),
                tags: &["recommended", "q4", "coding"],
                download_bytes: 51_741_611_823,
            },
            ModelVariant {
                name: "qwen3-coder-next:q8_0",
                title: "Q8",
                description: gettext_noop("Higher precision local coding tag."),
                tags: &["q8", "coding"],
                download_bytes: 84_812_676_525,
            },
        ],
    },
    ModelFamily {
        id: "rnj-1",
        title: "Rnj-1",
        subtitle: gettext_noop("Compact coding and STEM model from Essential AI"),
        description: gettext_noop(
            "An 8B text model for code generation, programming explanations, mathematics and science. A smaller option alongside the larger coding families.",
        ),
        tags: &["chat", "coding", "math", "tools"],
        variants: &[ModelVariant {
            name: "rnj-1:8b",
            title: "8B",
            description: gettext_noop(
                "Local coding model for programming and technical questions.",
            ),
            tags: &["recommended", "coding", "math"],
            download_bytes: 5_114_440_677,
        }],
    },
    ModelFamily {
        id: "devstral-small-2",
        title: "Devstral Small 2",
        subtitle: gettext_noop("Mistral model for software engineering and image understanding"),
        description: gettext_noop(
            "A 24B model specialized in software engineering, code understanding and tool use, with image support. Requires Ollama 0.13.3 or later.",
        ),
        tags: &["coding", "vision", "tools", "mistral"],
        variants: &[ModelVariant {
            name: "devstral-small-2:24b",
            title: "24B",
            description: gettext_noop(
                "Coding-focused model for systems with more available memory.",
            ),
            tags: &["recommended", "large", "coding", "vision"],
            download_bytes: 15_177_374_099,
        }],
    },
    ModelFamily {
        id: "deepseek-coder",
        title: "DeepSeek Coder",
        subtitle: gettext_noop("Popular coding model family"),
        description: gettext_noop(
            "A high-pull coding model family on Ollama. Useful for users who want a lightweight code-specific model.",
        ),
        tags: &["coding", "popular"],
        variants: &[
            ModelVariant {
                name: "deepseek-coder:1.3b",
                title: "1.3B",
                description: gettext_noop("Small code model."),
                tags: &["small", "coding"],
                download_bytes: 776_080_839,
            },
            ModelVariant {
                name: "deepseek-coder:6.7b",
                title: "6.7B",
                description: gettext_noop("Balanced local code model."),
                tags: &["recommended", "coding"],
                download_bytes: 3_827_834_503,
            },
            ModelVariant {
                name: "deepseek-coder:33b",
                title: "33B",
                description: gettext_noop("Large code model."),
                tags: &["large", "coding"],
                download_bytes: 18_819_456_424,
            },
        ],
    },
    ModelFamily {
        id: "starcoder2",
        title: "StarCoder2",
        subtitle: gettext_noop("Popular open code model family"),
        description: gettext_noop(
            "A code-focused family with small, medium and larger tags. Good to expose alongside Qwen and DeepSeek coder models.",
        ),
        tags: &["coding", "popular"],
        variants: &[
            ModelVariant {
                name: "starcoder2:3b",
                title: "3B",
                description: gettext_noop("Small code model."),
                tags: &["small", "coding"],
                download_bytes: 1_709_901_728,
            },
            ModelVariant {
                name: "starcoder2:7b",
                title: "7B",
                description: gettext_noop("Balanced code model."),
                tags: &["recommended", "coding"],
                download_bytes: 4_042_813_088,
            },
            ModelVariant {
                name: "starcoder2:15b",
                title: "15B",
                description: gettext_noop("Larger code model."),
                tags: &["medium", "coding"],
                download_bytes: 9_065_413_313,
            },
        ],
    },
    ModelFamily {
        id: "codegemma",
        title: "CodeGemma",
        subtitle: gettext_noop("Google code models in compact sizes"),
        description: gettext_noop(
            "A popular coding family with 2B and 7B variants for code completion and instruction-style coding tasks.",
        ),
        tags: &["coding", "google", "popular"],
        variants: &[
            ModelVariant {
                name: "codegemma:2b",
                title: "2B",
                description: gettext_noop("Small code completion model."),
                tags: &["small", "coding"],
                download_bytes: 1_551_198_417,
            },
            ModelVariant {
                name: "codegemma:7b",
                title: "7B",
                description: gettext_noop("Recommended CodeGemma local tag."),
                tags: &["recommended", "coding"],
                download_bytes: 5_011_852_809,
            },
        ],
    },
    ModelFamily {
        id: "codellama",
        title: "Code Llama",
        subtitle: gettext_noop("Code-focused Llama family"),
        description: gettext_noop(
            "Established coding models for code generation and explanation. Useful as a fallback coding family.",
        ),
        tags: &["coding", "chat", "meta"],
        variants: &[
            ModelVariant {
                name: "codellama:7b",
                title: "7B",
                description: gettext_noop("Small coding model."),
                tags: &["small", "coding"],
                download_bytes: 3_825_910_662,
            },
            ModelVariant {
                name: "codellama:13b",
                title: "13B",
                description: gettext_noop("Medium coding model."),
                tags: &["medium", "coding"],
                download_bytes: 7_365_960_935,
            },
            ModelVariant {
                name: "codellama:34b",
                title: "34B",
                description: gettext_noop("Large coding model."),
                tags: &["large", "coding"],
                download_bytes: 19_052_049_085,
            },
        ],
    },
    ModelFamily {
        id: "llama3.2-vision",
        title: "Llama 3.2 Vision",
        subtitle: gettext_noop("Vision-capable Llama models"),
        description: gettext_noop(
            "Multimodal Llama variants. Moose can download them through Ollama; text chat support depends on how you use the provider.",
        ),
        tags: &["vision", "chat", "meta"],
        variants: &[
            ModelVariant {
                name: "llama3.2-vision:11b",
                title: "11B",
                description: gettext_noop("Smaller vision-capable Llama model."),
                tags: &["vision", "medium"],
                download_bytes: 7_816_589_186,
            },
            ModelVariant {
                name: "llama3.2-vision:90b",
                title: "90B",
                description: gettext_noop("Large vision-capable Llama model."),
                tags: &["vision", "large"],
                download_bytes: 54_622_563_330,
            },
        ],
    },
    ModelFamily {
        id: "qwen3-vl",
        title: "Qwen 3 VL",
        subtitle: gettext_noop("Vision-language Qwen models"),
        description: gettext_noop(
            "Vision-language Qwen variants with instruct and thinking tags. Useful for multimodal workflows once the UI supports image input.",
        ),
        tags: &["vision", "multilingual", "thinking"],
        variants: &[
            ModelVariant {
                name: "qwen3-vl:2b",
                title: "2B",
                description: gettext_noop("Small vision-language variant."),
                tags: &["small", "vision"],
                download_bytes: 1_889_508_332,
            },
            ModelVariant {
                name: "qwen3-vl:4b",
                title: "4B",
                description: gettext_noop("Compact vision-language model."),
                tags: &["small", "vision"],
                download_bytes: 3_336_619_180,
            },
            ModelVariant {
                name: "qwen3-vl:8b",
                title: "8B",
                description: gettext_noop("Balanced vision-language model."),
                tags: &["vision", "thinking"],
                download_bytes: 6_190_971_699,
            },
            ModelVariant {
                name: "qwen3-vl:30b-a3b",
                title: "30B A3B",
                description: gettext_noop("Larger mixture-of-experts vision model."),
                tags: &["vision", "moe"],
                download_bytes: 19_595_398_707,
            },
        ],
    },
    ModelFamily {
        id: "qwen2.5vl",
        title: "Qwen 2.5 VL",
        subtitle: gettext_noop("Popular Qwen vision-language model family"),
        description: gettext_noop(
            "Qwen's earlier flagship vision-language family remains popular on Ollama and includes practical 3B and 7B local sizes.",
        ),
        tags: &["vision", "multilingual", "popular"],
        variants: &[
            ModelVariant {
                name: "qwen2.5vl:3b",
                title: "3B",
                description: gettext_noop("Small vision-language model."),
                tags: &["small", "vision"],
                download_bytes: 3_200_627_168,
            },
            ModelVariant {
                name: "qwen2.5vl:7b",
                title: "7B",
                description: gettext_noop("Recommended local vision-language model."),
                tags: &["recommended", "vision"],
                download_bytes: 5_969_245_856,
            },
            ModelVariant {
                name: "qwen2.5vl:32b",
                title: "32B",
                description: gettext_noop("Large vision-language model."),
                tags: &["large", "vision"],
                download_bytes: 21_159_310_657,
            },
        ],
    },
    ModelFamily {
        id: "llava",
        title: "LLaVA",
        subtitle: gettext_noop("Classic high-pull vision-language model"),
        description: gettext_noop(
            "One of the most pulled multimodal model families in Ollama. Useful for users who want broad compatibility with older vision workflows.",
        ),
        tags: &["vision", "popular"],
        variants: &[
            ModelVariant {
                name: "llava:7b",
                title: "7B",
                description: gettext_noop("Small LLaVA vision model."),
                tags: &["recommended", "vision"],
                download_bytes: 4_733_363_377,
            },
            ModelVariant {
                name: "llava:13b",
                title: "13B",
                description: gettext_noop("Medium LLaVA vision model."),
                tags: &["medium", "vision"],
                download_bytes: 8_011_256_494,
            },
            ModelVariant {
                name: "llava:34b",
                title: "34B",
                description: gettext_noop("Large LLaVA vision model."),
                tags: &["large", "vision"],
                download_bytes: 20_166_497_526,
            },
        ],
    },
    ModelFamily {
        id: "minicpm-v",
        title: "MiniCPM-V",
        subtitle: gettext_noop("Popular compact multimodal model"),
        description: gettext_noop(
            "A popular vision-language model designed for multimodal understanding in a comparatively compact size.",
        ),
        tags: &["vision", "small", "popular"],
        variants: &[ModelVariant {
            name: "minicpm-v:8b",
            title: "8B",
            description: gettext_noop("Default MiniCPM-V local tag."),
            tags: &["recommended", "vision"],
            download_bytes: 5_473_838_466,
        }],
    },
    ModelFamily {
        id: "minicpm-v4.6",
        title: "MiniCPM-V 4.6",
        subtitle: gettext_noop("Small vision model for images and screenshots"),
        description: gettext_noop(
            "A compact multimodal model designed for efficient image understanding. Useful for questions about images and screenshots on lower-memory systems.",
        ),
        tags: &["chat", "vision", "small", "efficient"],
        variants: &[ModelVariant {
            name: "minicpm-v4.6:1b",
            title: "1B",
            description: gettext_noop(
                "Lightweight model for image questions and visual understanding.",
            ),
            tags: &["recommended", "small", "vision"],
            download_bytes: 1_637_848_812,
        }],
    },
    ModelFamily {
        id: "ornith-1.5",
        title: "Ornith 1.5",
        subtitle: gettext_noop("Local vision model for chat and image questions"),
        description: gettext_noop(
            "A 9B multimodal model for text conversations and questions about images. A practical local alternative for understanding screenshots, charts and visual content.",
        ),
        tags: &["chat", "vision"],
        variants: &[ModelVariant {
            name: "ornith-1.5:9b",
            title: "9B",
            description: gettext_noop("Balanced local model for chat and image understanding."),
            tags: &["recommended", "chat", "vision"],
            download_bytes: 6_550_813_657,
        }],
    },
    ModelFamily {
        id: "olmo2",
        title: "OLMo 2",
        subtitle: gettext_noop("Fully open 7B and 13B models"),
        description: gettext_noop(
            "Popular fully open models from Ai2. Good to include for users who prefer open training data and transparent model families.",
        ),
        tags: &["chat", "open", "popular"],
        variants: &[
            ModelVariant {
                name: "olmo2:7b",
                title: "7B",
                description: gettext_noop("Small OLMo 2 model."),
                tags: &["small", "open"],
                download_bytes: 4_472_032_405,
            },
            ModelVariant {
                name: "olmo2:13b",
                title: "13B",
                description: gettext_noop("Recommended OLMo 2 model."),
                tags: &["recommended", "open"],
                download_bytes: 8_354_361_686,
            },
            ModelVariant {
                name: "olmo2:13b-1124-instruct-q4_K_M",
                title: "13B Q4",
                description: gettext_noop("Quantized instruction tag."),
                tags: &["q4", "chat"],
                download_bytes: 8_354_361_686,
            },
        ],
    },
    ModelFamily {
        id: "mixtral",
        title: "Mixtral",
        subtitle: gettext_noop("Popular Mistral mixture-of-experts models"),
        description: gettext_noop(
            "Older but still widely used MoE models from Mistral. Useful for larger local and remote Ollama systems.",
        ),
        tags: &["chat", "tools", "moe", "popular"],
        variants: &[
            ModelVariant {
                name: "mixtral:8x7b",
                title: "8x7B",
                description: gettext_noop("Classic Mixtral MoE model."),
                tags: &["moe", "chat"],
                download_bytes: 26_443_602_516,
            },
            ModelVariant {
                name: "mixtral:8x22b",
                title: "8x22B",
                description: gettext_noop("Large Mixtral MoE model."),
                tags: &["large", "moe"],
                download_bytes: 79_521_279_497,
            },
        ],
    },
    ModelFamily {
        id: "glm-4.7-flash",
        title: "GLM 4.7 Flash",
        subtitle: gettext_noop("Mixture-of-experts model for reasoning and coding"),
        description: gettext_noop(
            "A text model with 30B total parameters and 3B active parameters per token for programming and multi-step reasoning. Requires a higher-memory system and Ollama 0.14.3 or later.",
        ),
        tags: &["chat", "coding", "thinking", "tools", "moe"],
        variants: &[ModelVariant {
            name: "glm-4.7-flash:q4_K_M",
            title: "30B A3B Q4",
            description: gettext_noop(
                "Quantized model for local coding and reasoning with lower memory use.",
            ),
            tags: &["recommended", "large", "q4", "coding", "thinking"],
            download_bytes: 19_019_270_897,
        }],
    },
    ModelFamily {
        id: "qwq",
        title: "QwQ",
        subtitle: gettext_noop("Qwen reasoning model"),
        description: gettext_noop(
            "A popular Qwen reasoning model. Useful as a dedicated thinking model alongside DeepSeek R1 and Qwen 3.",
        ),
        tags: &["chat", "reasoning", "thinking", "popular"],
        variants: &[
            ModelVariant {
                name: "qwq:32b",
                title: "32B",
                description: gettext_noop("Default QwQ reasoning model."),
                tags: &["recommended", "reasoning"],
                download_bytes: 19_851_349_657,
            },
            ModelVariant {
                name: "qwq:32b-q4_K_M",
                title: "32B Q4",
                description: gettext_noop("Quantized QwQ reasoning tag."),
                tags: &["q4", "thinking"],
                download_bytes: 19_851_349_657,
            },
        ],
    },
    ModelFamily {
        id: "deepseek-v3",
        title: "DeepSeek V3",
        subtitle: gettext_noop("Very large MoE model for remote providers"),
        description: gettext_noop(
            "A high-pull DeepSeek MoE model. It is too large for most local machines, but useful when Moose points to a powerful remote Ollama provider.",
        ),
        tags: &["chat", "moe", "very large", "popular"],
        variants: &[
            ModelVariant {
                name: "deepseek-v3:671b",
                title: "671B",
                description: gettext_noop("Default very large DeepSeek V3 tag."),
                tags: &["very large", "moe"],
                download_bytes: 404_430_200_769,
            },
            ModelVariant {
                name: "deepseek-v3:671b-q4_K_M",
                title: "671B Q4",
                description: gettext_noop("Quantized very large DeepSeek V3 tag."),
                tags: &["q4", "very large"],
                download_bytes: 404_430_200_769,
            },
        ],
    },
    ModelFamily {
        id: "laguna-xs-2.1",
        title: "Laguna XS 2.1",
        subtitle: gettext_noop("Updated local coding model from Poolside"),
        description: gettext_noop(
            "An updated Laguna model for programming and code understanding, with 33B total parameters and 3B active parameters per token. Supports reasoning and requires more memory than smaller coding models.",
        ),
        tags: &["chat", "coding", "thinking", "tools", "moe"],
        variants: &[ModelVariant {
            name: "laguna-xs-2.1:q4_K_M",
            title: "33B A3B Q4",
            description: gettext_noop(
                "Quantized Laguna XS 2.1 model for local programming assistance.",
            ),
            tags: &["recommended", "large", "q4", "coding", "thinking"],
            download_bytes: 20_274_303_147,
        }],
    },
    ModelFamily {
        id: "laguna-xs.2",
        title: "Laguna XS.2",
        subtitle: gettext_noop("Recent local MoE model for coding and long-horizon work"),
        description: gettext_noop(
            "A recent model in Ollama's newest list, designed for agentic coding with a compact active parameter count.",
        ),
        tags: &["coding", "thinking", "new", "moe"],
        variants: &[
            ModelVariant {
                name: "laguna-xs.2:q4_K_M",
                title: "Q4",
                description: gettext_noop("Quantized Laguna XS.2 tag."),
                tags: &["recommended", "q4", "coding"],
                download_bytes: 23_098_061_820,
            },
            ModelVariant {
                name: "laguna-xs.2:q8_0",
                title: "Q8",
                description: gettext_noop("Higher precision Laguna XS.2 tag."),
                tags: &["q8", "coding"],
                download_bytes: 37_486_621_690,
            },
        ],
    },
    ModelFamily {
        id: "lfm2",
        title: "LFM2",
        subtitle: gettext_noop("Recent hybrid model for on-device deployment"),
        description: gettext_noop(
            "A newer hybrid model family shown in Ollama's recent list. The 24B A2B tag offers an efficient active-parameter path.",
        ),
        tags: &["chat", "tools", "new", "moe"],
        variants: &[
            ModelVariant {
                name: "lfm2:24b-a2b",
                title: "24B A2B",
                description: gettext_noop("Default efficient LFM2 tag."),
                tags: &["recommended", "moe"],
                download_bytes: 14_415_742_358,
            },
            ModelVariant {
                name: "lfm2:24b-q4_K_M",
                title: "24B Q4",
                description: gettext_noop("Quantized LFM2 variant."),
                tags: &["q4", "chat"],
                download_bytes: 14_415_742_358,
            },
        ],
    },
    ModelFamily {
        id: "lfm2.5",
        title: "LFM2.5",
        subtitle: gettext_noop("Efficient local assistant from Liquid AI"),
        description: gettext_noop(
            "A text model designed for local assistants and instruction following. Its mixture-of-experts architecture has 8B total parameters and 1B active parameters per token.",
        ),
        tags: &["chat", "tools", "thinking", "efficient", "moe"],
        variants: &[ModelVariant {
            name: "lfm2.5:8b",
            title: "8B A1B",
            description: gettext_noop("Efficient local assistant model with reasoning support."),
            tags: &["recommended", "chat", "thinking", "moe"],
            download_bytes: 5_156_075_525,
        }],
    },
    ModelFamily {
        id: "lfm2.5-thinking",
        title: "LFM2.5 Thinking",
        subtitle: gettext_noop("Lightweight multilingual reasoning model"),
        description: gettext_noop(
            "A small text-only model designed for local deployment and reasoning. Supports Spanish and other languages for everyday questions on lower-memory devices.",
        ),
        tags: &["chat", "thinking", "small", "multilingual", "efficient"],
        variants: &[ModelVariant {
            name: "lfm2.5-thinking:1.2b",
            title: "1.2B",
            description: gettext_noop("Compact reasoning model for lightweight local use."),
            tags: &["recommended", "small", "thinking", "multilingual"],
            download_bytes: 731_163_903,
        }],
    },
    ModelFamily {
        id: "nemotron-3-super",
        title: "Nemotron 3 Super",
        subtitle: gettext_noop("Recent NVIDIA 120B MoE model"),
        description: gettext_noop(
            "A recent NVIDIA MoE model. Exposed for remote Ollama providers and high-memory systems.",
        ),
        tags: &["chat", "thinking", "tools", "new", "very large"],
        variants: &[
            ModelVariant {
                name: "nemotron-3-super:120b-a12b",
                title: "120B A12B",
                description: gettext_noop("Default Nemotron 3 Super MoE tag."),
                tags: &["moe", "very large"],
                download_bytes: 86_827_834_104,
            },
            ModelVariant {
                name: "nemotron-3-super:120b-a12b-q4_K_M",
                title: "120B Q4",
                description: gettext_noop("Quantized Nemotron 3 Super tag."),
                tags: &["q4", "very large"],
                download_bytes: 86_827_834_104,
            },
        ],
    },
    ModelFamily {
        id: "smollm2",
        title: "SmolLM2",
        subtitle: gettext_noop("Very small instruction models"),
        description: gettext_noop(
            "Tiny instruction models for low-resource machines and quick tests.",
        ),
        tags: &["chat", "small", "fast"],
        variants: &[
            ModelVariant {
                name: "smollm2:135m",
                title: "135M",
                description: gettext_noop("Tiny test model."),
                tags: &["tiny", "fast"],
                download_bytes: 270_898_672,
            },
            ModelVariant {
                name: "smollm2:360m",
                title: "360M",
                description: gettext_noop("Very small local model."),
                tags: &["tiny", "fast"],
                download_bytes: 725_566_512,
            },
            ModelVariant {
                name: "smollm2:1.7b",
                title: "1.7B",
                description: gettext_noop("Small instruction model."),
                tags: &["small", "chat"],
                download_bytes: 1_820_428_533,
            },
        ],
    },
];

pub(super) fn build() -> ModelManager {
    let root = gtk::Box::builder()
        .orientation(Orientation::Vertical)
        .spacing(0)
        .hexpand(true)
        .vexpand(true)
        .build();
    root.add_css_class("moose-model-manager");

    let content = gtk::Box::builder()
        .orientation(Orientation::Vertical)
        .spacing(18)
        .hexpand(true)
        .vexpand(true)
        .build();
    content.add_css_class("moose-model-manager-content");

    let header = gtk::Box::builder()
        .orientation(Orientation::Vertical)
        .spacing(12)
        .halign(Align::Fill)
        .hexpand(true)
        .build();

    let title_row = gtk::Box::builder()
        .orientation(Orientation::Horizontal)
        .spacing(8)
        .build();

    let title = gtk::Label::builder()
        .label(gettext("Models"))
        .hexpand(true)
        .xalign(0.0)
        .build();
    title.add_css_class("title-2");

    let actions = gtk::Box::builder()
        .orientation(Orientation::Horizontal)
        .spacing(6)
        .halign(Align::End)
        .valign(Align::Center)
        .build();

    let pull_button = gtk::Button::builder()
        .label(gettext("Add Model…"))
        .tooltip_text(gettext("Download a model from Ollama or Hugging Face"))
        .build();
    pull_button.set_sensitive(false);

    let download_jobs_button = gtk::Button::builder()
        .label(gettext("Downloads"))
        .css_classes(["flat"])
        .build();
    let refresh_button = gtk::Button::builder()
        .label(gettext("Refresh Models"))
        .css_classes(["flat"])
        .build();
    refresh_button.set_sensitive(false);

    let options = gtk::Box::new(Orientation::Vertical, 0);
    options.append(&refresh_button);
    options.append(&download_jobs_button);
    let options_popover = gtk::Popover::builder().child(&options).build();
    for button in [&refresh_button, &download_jobs_button] {
        let weak_popover = options_popover.downgrade();
        button.connect_clicked(move |_| {
            if let Some(popover) = weak_popover.upgrade() {
                popover.popdown();
            }
        });
    }
    let options_button = gtk::MenuButton::builder()
        .icon_name("view-more-symbolic")
        .tooltip_text(gettext("More Options"))
        .popover(&options_popover)
        .css_classes(["flat"])
        .build();

    title_row.append(&title);
    actions.append(&pull_button);
    actions.append(&options_button);
    title_row.append(&actions);

    let search_entry = gtk::SearchEntry::builder()
        .placeholder_text(gettext("Search models or paste a link"))
        .tooltip_text(gettext("Search by name or task, or paste a model link"))
        .hexpand(true)
        .build();
    search_entry.set_sensitive(false);
    search_entry.add_css_class("moose-model-search");
    search_entry.update_property(&[gtk::accessible::Property::Label(&gettext(
        "Search models or paste a link",
    ))]);

    header.append(&title_row);

    let filters_box = gtk::Box::new(Orientation::Vertical, 12);
    let task_filter = adw::ToggleGroup::builder()
        .homogeneous(true)
        .can_shrink(true)
        .css_classes(["round"])
        .build();
    for (name, label, tooltip) in [
        (
            "all",
            gettext_noop("All"),
            gettext_noop("Browse every model family"),
        ),
        (
            "everyday",
            gettext_noop("Everyday"),
            gettext_noop("Questions, writing, and learning"),
        ),
        (
            "coding",
            gettext_noop("Coding"),
            gettext_noop("Write, explain, and troubleshoot code"),
        ),
        (
            "images",
            gettext_noop("Images"),
            gettext_noop("Ask questions about photos and screenshots"),
        ),
        (
            "documents",
            gettext_noop("Documents"),
            gettext_noop("Summarize text and work with documents"),
        ),
    ] {
        task_filter.add(
            adw::Toggle::builder()
                .name(name)
                .label(gettext(label))
                .tooltip(gettext(tooltip))
                .build(),
        );
    }
    task_filter.set_active_name(Some("all"));
    task_filter.update_property(&[gtk::accessible::Property::Label(&gettext(
        "Available model category",
    ))]);
    filters_box.append(&task_filter);

    let preference_filter = adw::ComboRow::builder()
        .title(gettext("Preference"))
        .model(&gtk::StringList::new(&[
            &gettext("Balanced"),
            &gettext("Lighter Downloads"),
            &gettext("Larger Models"),
        ]))
        .build();
    preference_filter.set_tooltip_text(Some(&gettext(
        "Choose a balance, smaller downloads, or larger versions. Speed depends on your hardware.",
    )));
    preference_filter.update_property(&[gtk::accessible::Property::Label(&gettext(
        "Model preference",
    ))]);
    let reset_filters_button = gtk::Button::builder()
        .label(gettext("Reset Filters"))
        .tooltip_text(gettext("Clear the search and restore the default filters"))
        .halign(Align::End)
        .css_classes(["flat"])
        .build();
    let filters_button = gtk::MenuButton::builder()
        .icon_name("moose-funnel-symbolic")
        .tooltip_text(gettext("Filters"))
        .build();
    filters_button.update_property(&[gtk::accessible::Property::Label(&gettext("Model filters"))]);
    let popover = gtk::Popover::new();
    let filter_content = gtk::Box::builder()
        .orientation(Orientation::Vertical)
        .spacing(12)
        .margin_top(12)
        .margin_bottom(12)
        .margin_start(12)
        .margin_end(12)
        .width_request(300)
        .build();
    let download_filter = adw::ComboRow::builder()
        .title(gettext("Download Size"))
        .model(&gtk::StringList::new(&[
            &gettext("Any size"),
            &gettext("Under 2 GB"),
            &gettext("Under 5 GB"),
            &gettext("Under 10 GB"),
            &gettext("Under 20 GB"),
        ]))
        .build();
    let images_filter = adw::SwitchRow::builder()
        .title(gettext("Image Input"))
        .subtitle(gettext("Models that can understand images"))
        .subtitle_lines(2)
        .build();
    let tools_filter = adw::SwitchRow::builder()
        .title(gettext("Tool Use"))
        .subtitle(gettext("Models with tool-calling support"))
        .subtitle_lines(2)
        .build();
    let multilingual_filter = adw::SwitchRow::builder()
        .title(gettext("Multiple Languages"))
        .subtitle(gettext("Language quality varies by model"))
        .subtitle_lines(2)
        .build();
    let filter_group = adw::PreferencesGroup::new();
    filter_group.add(&preference_filter);
    filter_group.add(&download_filter);
    filter_group.add(&images_filter);
    filter_group.add(&tools_filter);
    filter_group.add(&multilingual_filter);
    filter_content.append(&filter_group);
    let memory_hint = gtk::Label::builder()
        .xalign(0.0)
        .wrap(true)
        .width_chars(1)
        .css_classes(["caption", "dim-label"])
        .build();
    filter_content.append(&memory_hint);
    filter_content.append(&reset_filters_button);
    let filter_scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(PolicyType::Never)
        .max_content_height(440)
        .propagate_natural_height(true)
        .child(&filter_content)
        .build();
    popover.set_child(Some(&filter_scroll));
    filters_button.set_popover(Some(&popover));
    let search_bar = gtk::Box::builder()
        .orientation(Orientation::Horizontal)
        .css_classes(["linked"])
        .build();
    search_bar.append(&search_entry);
    search_bar.append(&filters_button);
    header.append(&search_bar);
    header.append(&filters_box);

    let pull_panel = gtk::Box::builder()
        .orientation(Orientation::Vertical)
        .hexpand(true)
        .visible(false)
        .css_classes(["card"])
        .build();
    let pull_row = adw::ActionRow::builder()
        .title(gettext("Model Download"))
        .subtitle(gettext("Preparing download…"))
        .title_lines(1)
        .subtitle_lines(1)
        .use_markup(false)
        .activatable(false)
        .selectable(false)
        .build();
    let pull_activity = gtk::Stack::builder()
        .valign(Align::Center)
        .width_request(24)
        .height_request(24)
        .build();
    let spinner = adw::Spinner::builder()
        .width_request(24)
        .height_request(24)
        .build();
    let pull_icon = gtk::Image::builder()
        .icon_name("folder-download-symbolic")
        .pixel_size(24)
        .build();
    pull_activity.add_named(&spinner, Some("working"));
    pull_activity.add_named(&pull_icon, Some("status"));
    pull_row.add_prefix(&pull_activity);

    let pull_cancel_button = icon_button("window-close-symbolic", &gettext("Cancel Download"));
    pull_cancel_button.add_css_class("circular");
    pull_cancel_button.set_valign(Align::Center);
    pull_cancel_button.set_sensitive(false);
    pull_cancel_button.set_visible(false);
    pull_cancel_button.update_property(&[gtk::accessible::Property::Label(&gettext(
        "Cancel download",
    ))]);

    let pull_status = gtk::Label::builder()
        .selectable(true)
        .wrap_mode(gtk::pango::WrapMode::WordChar)
        .width_chars(1)
        .xalign(0.0)
        .wrap(true)
        .build();
    let pull_copy_button = gtk::Button::builder()
        .label(gettext("Copy Details"))
        .halign(Align::End)
        .css_classes(["flat"])
        .build();
    let copy_status = pull_status.clone();
    pull_copy_button
        .connect_clicked(move |_| copy_status.clipboard().set_text(&copy_status.text()));
    let details = gtk::Box::builder()
        .orientation(Orientation::Vertical)
        .spacing(12)
        .width_request(280)
        .margin_top(6)
        .margin_bottom(6)
        .margin_start(6)
        .margin_end(6)
        .build();
    details.append(&pull_status);
    details.append(&pull_copy_button);
    let details_scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(PolicyType::Never)
        .max_content_height(280)
        .propagate_natural_height(true)
        .child(&details)
        .build();
    let details_popover = gtk::Popover::builder().child(&details_scroll).build();
    let details_button = gtk::MenuButton::builder()
        .icon_name("view-more-symbolic")
        .tooltip_text(gettext("Details"))
        .valign(Align::Center)
        .popover(&details_popover)
        .css_classes(["flat", "circular"])
        .build();
    details_button.update_property(&[gtk::accessible::Property::Label(&gettext(
        "Transfer details",
    ))]);
    pull_row.add_suffix(&details_button);
    pull_row.add_suffix(&pull_cancel_button);

    let pull_progress = gtk::ProgressBar::builder()
        .hexpand(true)
        .pulse_step(0.08)
        .margin_start(16)
        .margin_end(16)
        .margin_bottom(12)
        .build();
    pull_progress.update_property(&[gtk::accessible::Property::Label(&gettext(
        "Model download progress",
    ))]);
    pull_panel.append(&pull_row);
    pull_panel.append(&pull_progress);

    let model_list = gtk::ListBox::new();
    model_list.set_selection_mode(gtk::SelectionMode::None);
    model_list.add_css_class("boxed-list");
    let installed_group = adw::PreferencesGroup::builder()
        .title(gettext("Installed"))
        .build();
    installed_group.add(&model_list);

    let available_model_list = gtk::ListBox::new();
    available_model_list.set_selection_mode(gtk::SelectionMode::None);
    available_model_list.add_css_class("boxed-list");
    let available_group = adw::PreferencesGroup::builder()
        .title(gettext("Available"))
        .build();
    available_group.add(&available_model_list);

    let models_content = gtk::Box::builder()
        .orientation(Orientation::Vertical)
        .spacing(24)
        .hexpand(true)
        .vexpand(true)
        .build();
    models_content.add_css_class("moose-model-lists");
    models_content.append(&installed_group);
    models_content.append(&available_group);

    let status_page = adw::StatusPage::builder()
        .icon_name(APPLICATION_ID)
        .title(gettext("No Models Loaded"))
        .hexpand(true)
        .vexpand(true)
        .build();

    let empty_clamp = adw::Clamp::builder()
        .maximum_size(760)
        .tightening_threshold(520)
        .valign(Align::Center)
        .hexpand(true)
        .vexpand(true)
        .child(&status_page)
        .build();

    let stack = gtk::Stack::builder().hexpand(true).vexpand(true).build();
    stack.add_named(&empty_clamp, Some("empty"));
    stack.add_named(&models_content, Some("models"));
    stack.set_visible_child_name("empty");

    content.append(&header);
    content.append(&pull_panel);
    content.append(&stack);

    let clamp = adw::Clamp::builder()
        .maximum_size(820)
        .tightening_threshold(640)
        .hexpand(true)
        .vexpand(true)
        .child(&content)
        .build();

    let scrolled = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(PolicyType::Never)
        .child(&clamp)
        .hexpand(true)
        .vexpand(true)
        .build();
    root.append(&scrolled);

    ModelManager {
        root,
        pull_button,
        download_jobs_button,
        refresh_button,
        search_entry,
        task_filter,
        preference_filter,
        download_filter,
        images_filter,
        tools_filter,
        multilingual_filter,
        reset_filters_button,
        changing_filters: Rc::new(Cell::new(false)),
        pull_cancel_button,
        busy: Rc::new(Cell::new(false)),
        filters_box,
        filters_button,
        installed_group,
        memory_hint,
        scroll: scrolled,
        pull_panel,
        pull_row,
        pull_activity,
        pull_icon,
        pull_status,
        pull_progress,
        model_list,
        available_model_list,
        status_page,
        stack,
    }
}

pub(super) fn set_loading(manager: &ModelManager) {
    clear_model_lists(manager);
    manager.pull_button.set_sensitive(false);
    manager.refresh_button.set_sensitive(false);
    manager.search_entry.set_sensitive(false);
    manager.filters_button.set_sensitive(false);
    manager.filters_box.set_sensitive(false);
    manager.status_page.set_title(&gettext("Loading Models"));
    manager.status_page.set_description(None);
    manager.stack.set_visible_child_name("empty");
}

pub(super) fn set_unavailable(manager: &ModelManager, title: &str, description: &str) {
    clear_model_lists(manager);
    manager.pull_button.set_sensitive(false);
    manager.refresh_button.set_sensitive(true);
    manager.search_entry.set_sensitive(false);
    manager.filters_button.set_sensitive(false);
    manager.filters_box.set_sensitive(false);
    manager.status_page.set_title(title);
    manager.status_page.set_description(Some(description));
    manager.stack.set_visible_child_name("empty");
}

pub(super) fn clear_download_job(manager: &ModelManager) {
    manager.busy.set(false);
    manager.pull_panel.set_visible(false);
    manager.pull_cancel_button.set_sensitive(false);
    manager.pull_cancel_button.set_visible(false);
    manager.pull_row.set_title(&gettext("Model Download"));
    manager.pull_row.set_subtitle("");
    manager.pull_status.set_label("");
    manager.pull_progress.set_fraction(0.0);
}

pub(super) fn finish_pull(manager: &ModelManager) {
    clear_download_job(manager);
    manager.pull_button.set_sensitive(true);
    manager.refresh_button.set_sensitive(true);
}

pub(super) fn set_models(
    manager: &ModelManager,
    parent: &adw::ApplicationWindow,
    models: &[OllamaModel],
    query: &str,
    on_pull: Rc<dyn Fn(String)>,
    on_delete: Rc<dyn Fn(String)>,
    on_add: Rc<dyn Fn(String)>,
    on_use: Rc<dyn Fn(String)>,
    local: bool,
) {
    clear_model_lists(manager);
    manager.pull_button.set_sensitive(!manager.busy.get());
    manager.refresh_button.set_sensitive(!manager.busy.get());
    manager.search_entry.set_sensitive(true);
    manager.filters_button.set_sensitive(true);
    manager.filters_box.set_sensitive(true);

    let filters = Filters {
        task: Task::from_name(
            manager
                .task_filter
                .active_name()
                .as_deref()
                .unwrap_or("all"),
        ),
        preference: Preference::from_index(manager.preference_filter.selected()),
        max_download: match manager.download_filter.selected() {
            1 => Some(2_000_000_000),
            2 => Some(5_000_000_000),
            3 => Some(10_000_000_000),
            4 => Some(20_000_000_000),
            _ => None,
        },
        images: manager.images_filter.is_active(),
        tools: manager.tools_filter.is_active(),
        multilingual: manager.multilingual_filter.is_active(),
    };
    let extra_count = usize::from(filters.preference != Preference::Balanced)
        + usize::from(filters.max_download.is_some())
        + usize::from(filters.images)
        + usize::from(filters.tools)
        + usize::from(filters.multilingual);
    if extra_count == 0 {
        manager.filters_button.remove_css_class("suggested-action");
        manager
            .filters_button
            .set_tooltip_text(Some(&gettext("Filters")));
    } else {
        manager.filters_button.add_css_class("suggested-action");
        manager.filters_button.set_tooltip_text(Some(&gettext_f(
            "Filters ({extra_count} active)",
            &[("extra_count", &(extra_count).to_string())],
        )));
    }
    manager
        .task_filter
        .set_tooltip_text(Some(&gettext(filters.task.description())));
    manager.reset_filters_button.set_sensitive(
        !query.trim().is_empty()
            || filters.task != Task::All
            || filters.preference != Preference::Balanced
            || extra_count > 0,
    );
    let memory = MemoryBudget::read(local);
    manager.memory_hint.set_label(&memory.available.map_or_else(
        || gettext("Memory guidance is unavailable for this connection."),
        |bytes| {
            gettext_f(
                "{arg1_formatted} GiB available · Memory estimates are approximate",
                &[(
                    "arg1_formatted",
                    &format!("{:.1}", bytes as f64 / 1_073_741_824.0),
                )],
            )
        },
    ));
    manager.memory_hint.set_tooltip_text(Some(&gettext("Memory guidance uses download size with a rough runtime allowance. Context length, graphics memory, and other apps affect actual requirements. It does not predict response speed.")));

    let download_reference = ModelDownload::parse(query).ok().filter(|reference| {
        reference.hugging_face_repository.is_some()
            || query.contains(':')
            || query.contains('/')
            || query.trim().starts_with("ollama ")
    });
    let query = query.trim().to_ascii_lowercase();
    let filtered_models = models
        .iter()
        .filter(|model| {
            model_matches_query(model, &query)
                || download_reference
                    .as_ref()
                    .is_some_and(|reference| model_names_match(&model.name, &reference.name))
        })
        .collect::<Vec<_>>();

    if filtered_models.is_empty() {
        let row = empty_row(&gettext(if models.is_empty() {
            gettext_noop("No installed models")
        } else {
            gettext_noop("No installed matches")
        }));
        manager.model_list.append(&row);
    } else {
        for model in filtered_models {
            manager.model_list.append(&model_row(
                parent,
                model,
                Rc::clone(&on_delete),
                Rc::clone(&on_use),
            ));
        }
    }
    manager.installed_group.set_visible(!models.is_empty());

    let installed_names = models
        .iter()
        .map(|model| model.name.as_str())
        .collect::<Vec<_>>();
    let candidates = discovery::discover(&filters, &query, &installed_names, memory);
    let direct_reference = download_reference.filter(|reference| {
        !model_is_installed(&reference.name, &installed_names)
            && !candidates
                .iter()
                .any(|candidate| model_names_match(candidate.variant.name, &reference.name))
    });
    let has_reference = direct_reference.is_some();
    if let Some(reference) = direct_reference {
        manager
            .available_model_list
            .append(&download_reference_row(reference, on_add));
    }
    if candidates.is_empty() && !has_reference {
        let reset = gtk::Button::builder()
            .label(gettext("Clear Filters"))
            .halign(Align::Center)
            .css_classes(["pill"])
            .build();
        let weak_reset = manager.reset_filters_button.downgrade();
        reset.connect_clicked(move |_| {
            if let Some(button) = weak_reset.upgrade() {
                button.emit_clicked();
            }
        });
        let empty = adw::StatusPage::builder()
            .icon_name("system-search-symbolic")
            .title(gettext("No Matching Models"))
            .description(gettext(
                "Try another task or clear your search and filters.",
            ))
            .child(&reset)
            .css_classes(["compact"])
            .build();
        manager.available_model_list.append(&empty);
    }
    for candidate in candidates {
        manager.available_model_list.append(&model_family_row(
            parent,
            candidate,
            &installed_names,
            ModelActions {
                download: Rc::clone(&on_pull),
                use_model: Rc::clone(&on_use),
                download_control: manager.pull_button.clone(),
            },
        ));
    }

    manager.stack.set_visible_child_name("models");
}

pub(super) fn set_pull_started(manager: &ModelManager, model: &str) {
    manager.busy.set(true);
    manager.scroll.vadjustment().set_value(0.0);
    manager.pull_panel.set_visible(true);
    manager.pull_button.set_sensitive(false);
    manager.refresh_button.set_sensitive(false);
    manager.pull_cancel_button.set_sensitive(true);
    manager.pull_cancel_button.set_visible(true);
    manager.pull_activity.set_visible_child_name("working");
    manager.pull_row.set_title(&download_model_title(model));
    manager
        .pull_row
        .set_subtitle(&gettext("Preparing download…"));
    manager.pull_status.set_label(&gettext_f(
        "Model: {model}\nPreparing download",
        &[("model", (model))],
    ));
    manager.pull_progress.set_visible(true);
    manager.pull_progress.set_fraction(0.0);
}

pub(super) fn set_pull_progress(
    manager: &ModelManager,
    model: &str,
    progress: &OllamaPullProgress,
) {
    manager.pull_panel.set_visible(true);
    manager.pull_row.set_title(&download_model_title(model));
    let status = progress.status.to_ascii_lowercase();
    let finalizing = status.contains("verifying")
        || status.contains("writing manifest")
        || status.contains("removing unused");
    let phase = gettext(if progress.done {
        gettext_noop("Ready to use")
    } else if status.contains("verifying") {
        gettext_noop("Verifying download…")
    } else if finalizing {
        gettext_noop("Finishing up…")
    } else if status.contains("manifest") {
        gettext_noop("Preparing download…")
    } else {
        gettext_noop("Downloading…")
    });
    let subtitle = if progress.done || finalizing {
        manager.pull_progress.set_fraction(1.0);
        phase.to_string()
    } else if let (Some(completed), Some(total)) = (progress.completed_bytes, progress.total_bytes)
        && total > 0
    {
        let completed = completed.min(total);
        manager
            .pull_progress
            .set_fraction(completed as f64 / total as f64);
        gettext_f(
            "Downloading · {arg1} of {arg2}",
            &[
                ("arg1", &(format_size(Some(completed))).to_string()),
                ("arg2", &(format_size(Some(total))).to_string()),
            ],
        )
    } else {
        manager.pull_progress.pulse();
        phase.to_string()
    };
    manager.pull_row.set_subtitle(&subtitle);
    let mut details = gettext_f(
        "Model: {model}\nStatus: {arg1}\n{subtitle}",
        &[
            ("model", (model)),
            ("arg1", &(progress.status).to_string()),
            ("subtitle", &(subtitle).to_string()),
        ],
    );
    if let Some(digest) = &progress.digest {
        details.push_str(&gettext_f("\nDigest: {digest}", &[("digest", digest)]));
    }
    manager.pull_status.set_label(&details);
    if progress.done {
        manager.pull_icon.set_icon_name(Some("emblem-ok-symbolic"));
        manager.pull_activity.set_visible_child_name("status");
        manager.pull_cancel_button.set_sensitive(false);
        manager.pull_cancel_button.set_visible(false);
    }
}

fn download_model_title(model: &str) -> String {
    MODEL_FAMILIES
        .iter()
        .find_map(|family| {
            family
                .variants
                .iter()
                .find(|variant| model_names_match(variant.name, model))
                .map(|variant| format!("{} · {}", family.title, variant.title))
        })
        .unwrap_or_else(|| model.to_string())
}

pub(super) fn set_delete_started(manager: &ModelManager, model: &str) {
    manager.busy.set(true);
    manager.scroll.vadjustment().set_value(0.0);
    manager.pull_panel.set_visible(true);
    manager.pull_button.set_sensitive(false);
    manager.refresh_button.set_sensitive(false);
    manager.pull_cancel_button.set_sensitive(false);
    manager.pull_cancel_button.set_visible(false);
    manager.pull_activity.set_visible_child_name("working");
    manager.pull_row.set_title(&download_model_title(model));
    manager.pull_row.set_subtitle(&gettext("Removing model…"));
    manager.pull_status.set_label(&gettext_f(
        "Model: {model}\nRemoving model from Ollama",
        &[("model", (model))],
    ));
    manager.pull_progress.set_visible(false);
    manager.pull_progress.set_fraction(0.0);
}

pub(super) fn set_delete_finished(
    manager: &ModelManager,
    title: &str,
    status: &str,
    fraction: f64,
) {
    manager.busy.set(false);
    manager.pull_panel.set_visible(true);
    manager.pull_row.set_title(title);
    manager.pull_row.set_subtitle(&gettext(if fraction >= 1.0 {
        gettext_noop("Model removed")
    } else {
        gettext_noop("Open details for more information")
    }));
    manager.pull_icon.set_icon_name(Some(if fraction >= 1.0 {
        "emblem-ok-symbolic"
    } else {
        "dialog-warning-symbolic"
    }));
    manager.pull_activity.set_visible_child_name("status");
    manager.pull_status.set_label(status);
    manager.pull_progress.set_visible(false);
    manager.pull_progress.set_fraction(fraction.clamp(0.0, 1.0));
    manager.pull_cancel_button.set_sensitive(false);
    manager.pull_cancel_button.set_visible(false);
    manager.pull_button.set_sensitive(true);
    manager.refresh_button.set_sensitive(true);
}

fn clear_model_lists(manager: &ModelManager) {
    while let Some(child) = manager.model_list.first_child() {
        manager.model_list.remove(&child);
    }

    while let Some(child) = manager.available_model_list.first_child() {
        manager.available_model_list.remove(&child);
    }
}

fn empty_row(title: &str) -> adw::ActionRow {
    let row = adw::ActionRow::builder()
        .title(title)
        .sensitive(false)
        .build();
    row.add_css_class("moose-model-row-item");
    row
}

fn model_row(
    parent: &adw::ApplicationWindow,
    model: &OllamaModel,
    on_delete: Rc<dyn Fn(String)>,
    on_use: Rc<dyn Fn(String)>,
) -> adw::ActionRow {
    let row = adw::ActionRow::builder()
        .title(&model.name)
        .title_lines(1)
        .subtitle(if model.supports_chat {
            format_size(model.size_bytes)
        } else {
            gettext_f(
                "{arg1} · Not a chat model",
                &[("arg1", &(format_size(model.size_bytes)).to_string())],
            )
        })
        .subtitle_lines(1)
        .use_markup(false)
        .build();
    row.add_css_class("moose-model-row-item");
    row.set_tooltip_text(Some(&format!("{}\n{}", model.name, model_subtitle(model))));

    if model.supports_chat {
        let use_button = gtk::Button::builder()
            .label(gettext("Use"))
            .valign(Align::Center)
            .build();
        let name = model.name.clone();
        use_button.connect_clicked(move |_| on_use(name.clone()));
        row.add_suffix(&use_button);
    }

    let options = gtk::Box::new(Orientation::Vertical, 0);
    let popover = gtk::Popover::builder().child(&options).build();
    let options_button = gtk::MenuButton::builder()
        .icon_name("view-more-symbolic")
        .tooltip_text(gettext("Model Options"))
        .valign(Align::Center)
        .css_classes(["flat"])
        .popover(&popover)
        .build();

    if let Some(repository) = hugging_face_repository(&model.name) {
        let open_button = gtk::Button::builder()
            .label(gettext("Open on Hugging Face"))
            .css_classes(["flat"])
            .build();
        let url = format!("https://huggingface.co/{repository}");
        let parent = parent.downgrade();
        let weak_popover = popover.downgrade();
        open_button.connect_clicked(move |_| {
            if let Some(popover) = weak_popover.upgrade() {
                popover.popdown();
            }
            if let Some(parent) = parent.upgrade() {
                gtk::UriLauncher::new(&url).launch(
                    Some(&parent),
                    None::<&gio::Cancellable>,
                    |_| {},
                );
            }
        });
        options.append(&open_button);
    }

    let delete_button = gtk::Button::builder()
        .label(gettext("Remove…"))
        .css_classes(["flat", "destructive-action"])
        .build();
    let model_name = model.name.clone();
    let weak_popover = popover.downgrade();
    delete_button.connect_clicked(move |_| {
        if let Some(popover) = weak_popover.upgrade() {
            popover.popdown();
        }
        on_delete(model_name.clone());
    });
    options.append(&delete_button);
    row.add_suffix(&options_button);
    row
}

fn download_reference_row(reference: ModelDownload, on_add: Rc<dyn Fn(String)>) -> adw::ActionRow {
    let row = adw::ActionRow::builder()
        .title(gettext(if reference.hugging_face_repository.is_some() {
            gettext_noop("Download from Hugging Face")
        } else {
            gettext_noop("Download Model")
        }))
        .subtitle(&reference.name)
        .subtitle_lines(2)
        .use_markup(false)
        .activatable(true)
        .build();
    row.add_css_class("moose-model-row-item");
    row.add_prefix(&gtk::Image::from_icon_name("folder-download-symbolic"));
    row.add_suffix(&gtk::Image::from_icon_name("go-next-symbolic"));
    row.connect_activated(move |_| on_add(reference.name.clone()));
    row
}

fn model_family_row(
    parent: &adw::ApplicationWindow,
    candidate: Candidate,
    installed_names: &[&str],
    actions: ModelActions,
) -> adw::ActionRow {
    let family = candidate.family;
    let row = adw::ActionRow::builder()
        .title(family.title)
        .subtitle(format!(
            "{} · {}",
            gettext(candidate.purpose()),
            format_size(Some(candidate.variant.download_bytes)),
        ))
        .subtitle_lines(1)
        .use_markup(false)
        .activatable(true)
        .build();
    row.set_tooltip_text(Some(&gettext_f(
        "{arg1} · View details and other versions",
        &[("arg1", (candidate.variant.name))],
    )));
    row.add_suffix(&model_action(candidate.variant, installed_names, &actions));
    row.add_suffix(&gtk::Image::from_icon_name("go-next-symbolic"));

    let target_parent = parent.clone();
    let installed_names = installed_names
        .iter()
        .map(|name| (*name).to_string())
        .collect::<Vec<_>>();
    row.connect_activated(move |_| {
        let installed_names = installed_names
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>();
        show_model_family_dialog(
            &target_parent,
            candidate,
            installed_names.as_slice(),
            actions.clone(),
        );
    });

    row
}

fn show_model_family_dialog(
    parent: &adw::ApplicationWindow,
    candidate: Candidate,
    installed_names: &[&str],
    actions: ModelActions,
) {
    let family = candidate.family;
    let dialog = adw::Dialog::builder()
        .title(family.title)
        .content_width(640)
        .build();

    let header_bar = adw::HeaderBar::builder()
        .show_start_title_buttons(false)
        .show_end_title_buttons(false)
        .build();
    let title = adw::WindowTitle::new(family.title, &gettext("Model Details"));
    header_bar.set_title_widget(Some(&title));

    let ollama_button = icon_button("moose-globe-symbolic", &gettext("Open on Ollama"));
    let ollama_url = format!("https://ollama.com/library/{}", family.id);
    let target_parent = parent.clone();
    ollama_button.connect_clicked(move |_| {
        let launcher = gtk::UriLauncher::new(&ollama_url);
        launcher.launch(Some(&target_parent), None::<&gio::Cancellable>, |_| {});
    });
    header_bar.pack_start(&ollama_button);

    let close_button = icon_button("window-close-symbolic", &gettext("Close"));
    let target_dialog = dialog.clone();
    close_button.connect_clicked(move |_| {
        target_dialog.close();
    });
    header_bar.pack_end(&close_button);

    let content = gtk::Box::builder()
        .orientation(Orientation::Vertical)
        .spacing(24)
        .hexpand(true)
        .build();
    content.add_css_class("moose-model-dialog-content");

    let description = gtk::Label::builder()
        .label(gettext(family.description))
        .xalign(0.0)
        .wrap(true)
        .width_chars(1)
        .build();
    description.add_css_class("dim-label");

    let variant_list = gtk::ListBox::new();
    variant_list.set_selection_mode(gtk::SelectionMode::None);
    variant_list.add_css_class("boxed-list");
    let weak_dialog = dialog.downgrade();
    let on_pull = actions.download;
    let on_pull: Rc<dyn Fn(String)> = Rc::new(move |name| {
        if let Some(dialog) = weak_dialog.upgrade() {
            dialog.close();
        }
        on_pull(name);
    });
    let weak_dialog = dialog.downgrade();
    let on_use = actions.use_model;
    let on_use: Rc<dyn Fn(String)> = Rc::new(move |name| {
        if let Some(dialog) = weak_dialog.upgrade() {
            dialog.close();
        }
        on_use(name);
    });
    let actions = ModelActions {
        download: on_pull,
        use_model: on_use,
        download_control: actions.download_control,
    };
    for variant in std::iter::once(candidate.variant).chain(
        family
            .variants
            .iter()
            .filter(|variant| variant.name != candidate.variant.name),
    ) {
        variant_list.append(&model_variant_row(
            Candidate {
                variant,
                ..candidate
            },
            variant.name == candidate.variant.name,
            installed_names,
            &actions,
        ));
    }
    let versions = adw::PreferencesGroup::builder()
        .title(gettext("Versions"))
        .build();
    versions.add(&variant_list);
    let technical = requirements::build(parent, candidate);
    let technical_group = adw::PreferencesGroup::new();
    technical_group.add(&technical);
    content.append(&description);
    content.append(&versions);
    content.append(&technical_group);
    let scrolled = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(PolicyType::Never)
        .min_content_height(300)
        .max_content_height(560)
        .propagate_natural_height(true)
        .child(&content)
        .build();

    let toolbar_view = adw::ToolbarView::builder()
        .top_bar_style(adw::ToolbarStyle::Flat)
        .content(&scrolled)
        .build();
    toolbar_view.add_top_bar(&header_bar);

    dialog.set_child(Some(&toolbar_view));
    dialog.present(Some(parent));
}

fn model_variant_row(
    candidate: Candidate,
    selected: bool,
    installed_names: &[&str],
    actions: &ModelActions,
) -> adw::ActionRow {
    let variant = candidate.variant;
    let row = adw::ActionRow::builder()
        .title(if selected {
            gettext_f("{arg1} · Recommended", &[("arg1", (variant.title))])
        } else {
            variant.title.to_string()
        })
        .subtitle(format!(
            "{} · {}",
            format_size(Some(variant.download_bytes)),
            candidate.capabilities(),
        ))
        .subtitle_lines(1)
        .use_markup(false)
        .build();
    row.set_tooltip_text(Some(&format!(
        "{}\n{}\n{}",
        variant.name,
        gettext(variant.description),
        gettext(candidate.memory.guidance(variant.download_bytes))
    )));
    let action = model_action(variant, installed_names, actions);
    row.set_activatable_widget(Some(&action));
    row.add_suffix(&action);
    row
}

fn model_action(
    variant: &ModelVariant,
    installed_names: &[&str],
    actions: &ModelActions,
) -> gtk::Button {
    let installed = model_is_installed(variant.name, installed_names);
    let button = gtk::Button::builder()
        .label(gettext(if installed {
            gettext_noop("Use")
        } else {
            gettext_noop("Download")
        }))
        .valign(Align::Center)
        .build();
    let tooltip = if installed {
        gettext_f("Start a new chat with {model}", &[("model", variant.name)])
    } else {
        gettext_f("Download {model}", &[("model", variant.name)])
    };
    button.set_tooltip_text(Some(&tooltip));
    let accessible_label = if installed {
        gettext_f("Use {model}", &[("model", variant.name)])
    } else {
        gettext_f("Download {model}", &[("model", variant.name)])
    };
    button.update_property(&[gtk::accessible::Property::Label(&accessible_label)]);
    if !installed {
        actions
            .download_control
            .bind_property("sensitive", &button, "sensitive")
            .sync_create()
            .build();
    }
    let actions = actions.clone();
    let name = variant.name.to_string();
    button.connect_clicked(move |_| {
        if installed {
            (actions.use_model)(name.clone());
        } else {
            (actions.download)(name.clone());
        }
    });
    button
}

fn model_is_installed(model_name: &str, installed_names: &[&str]) -> bool {
    installed_names
        .iter()
        .any(|installed| model_names_match(model_name, installed))
}

fn model_subtitle(model: &OllamaModel) -> String {
    let mut parts = Vec::new();

    if hugging_face_repository(&model.name).is_some() {
        parts.push("Hugging Face".to_string());
    }

    if let Some(family) = model.family.as_deref().filter(|value| !value.is_empty()) {
        parts.push(family.to_string());
    }

    if let Some(parameter_size) = model
        .parameter_size
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        parts.push(parameter_size.to_string());
    }

    if let Some(quantization_level) = model
        .quantization_level
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        parts.push(quantization_level.to_string());
    }

    if let Some(modified_at) = model
        .modified_at
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        parts.push(gettext_f(
            "Modified {arg1}",
            &[("arg1", &(compact_timestamp(modified_at)).to_string())],
        ));
    }

    if parts.is_empty() {
        gettext("Installed locally")
    } else {
        parts.join(" - ")
    }
}

fn compact_timestamp(value: &str) -> String {
    value
        .split_once('T')
        .map(|(date, _)| date)
        .unwrap_or(value)
        .to_string()
}

fn format_size(size_bytes: Option<u64>) -> String {
    let Some(size_bytes) = size_bytes else {
        return gettext("Unknown size");
    };

    let units = ["B", "KB", "MB", "GB", "TB"];
    let mut size = size_bytes as f64;
    let mut unit_index = 0;

    while size >= 1000.0 && unit_index + 1 < units.len() {
        size /= 1000.0;
        unit_index += 1;
    }

    if unit_index == 0 {
        format!("{size_bytes} {}", units[unit_index])
    } else if size >= 10.0 {
        format!("{size:.0} {}", units[unit_index])
    } else {
        format!("{size:.1} {}", units[unit_index])
    }
}

fn model_matches_query(model: &OllamaModel, query: &str) -> bool {
    if query.is_empty() {
        return true;
    }

    contains_query(&model.name, query)
        || (hugging_face_repository(&model.name).is_some()
            && contains_query(&gettext("hugging face huggingface hf.co"), query))
        || model
            .family
            .as_deref()
            .is_some_and(|family| contains_query(family, query))
        || model
            .families
            .iter()
            .any(|family| contains_query(family, query))
        || model
            .parameter_size
            .as_deref()
            .is_some_and(|parameter_size| contains_query(parameter_size, query))
        || model
            .quantization_level
            .as_deref()
            .is_some_and(|quantization_level| contains_query(quantization_level, query))
}

fn contains_query(value: &str, query: &str) -> bool {
    value.to_ascii_lowercase().contains(query)
}
