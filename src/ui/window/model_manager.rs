use adw::prelude::*;
use gtk::{Align, Orientation, PolicyType, gio};
use std::rc::Rc;

use crate::{
    APPLICATION_ID,
    ollama::{OllamaModel, OllamaPullProgress},
};

use super::widgets::{icon_button, section_label};

#[derive(Clone)]
pub(super) struct ModelManager {
    pub(super) root: gtk::Box,
    pub(super) pull_button: gtk::Button,
    pub(super) download_jobs_button: gtk::Button,
    pub(super) refresh_button: gtk::Button,
    pub(super) search_entry: gtk::SearchEntry,
    pub(super) pull_cancel_button: gtk::Button,
    pull_panel: gtk::Box,
    pull_title: gtk::Label,
    pull_status: gtk::Label,
    pull_progress: gtk::ProgressBar,
    pull_progress_label: gtk::Label,
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
}

const MODEL_FAMILIES: &[ModelFamily] = &[
    ModelFamily {
        id: "llama3.1",
        title: "Llama 3.1",
        subtitle: "Meta general chat models with tool-capable instruct variants",
        description: "Balanced general-purpose chat models from Meta. The 8B tag is the practical default for local machines, while 70B and 405B target larger systems.",
        tags: &["chat", "tools", "general", "meta"],
        variants: &[
            ModelVariant {
                name: "llama3.1:8b",
                title: "8B",
                description: "Recommended local default for everyday chat.",
                tags: &["recommended", "chat", "tools"],
            },
            ModelVariant {
                name: "llama3.1:70b",
                title: "70B",
                description: "Larger model for stronger reasoning and writing.",
                tags: &["large", "chat", "tools"],
            },
            ModelVariant {
                name: "llama3.1:405b",
                title: "405B",
                description: "Very large model for high-memory systems.",
                tags: &["very large", "chat"],
            },
        ],
    },
    ModelFamily {
        id: "llama3.2",
        title: "Llama 3.2",
        subtitle: "Small Meta models for fast local chat",
        description: "Compact Llama models made for responsive local use. Good first downloads when you want speed and lower memory use.",
        tags: &["chat", "small", "fast", "meta"],
        variants: &[
            ModelVariant {
                name: "llama3.2:1b",
                title: "1B",
                description: "Very small and fast for quick responses.",
                tags: &["tiny", "fast", "chat"],
            },
            ModelVariant {
                name: "llama3.2:3b",
                title: "3B",
                description: "Small but more capable than 1B.",
                tags: &["recommended", "small", "chat"],
            },
        ],
    },
    ModelFamily {
        id: "llama3.3",
        title: "Llama 3.3",
        subtitle: "Popular 70B Meta model with strong quality for its size",
        description: "A newer Llama 70B option that Ollama lists among the popular models. Use this when you want high quality and have enough memory for a large model.",
        tags: &["chat", "tools", "large", "meta", "popular"],
        variants: &[
            ModelVariant {
                name: "llama3.3:70b",
                title: "70B",
                description: "Default Llama 3.3 70B tag.",
                tags: &["recommended", "large", "chat", "tools"],
            },
            ModelVariant {
                name: "llama3.3:70b-instruct-q4_K_M",
                title: "70B Q4",
                description: "Quantized instruction variant for large local systems.",
                tags: &["large", "q4", "chat"],
            },
        ],
    },
    ModelFamily {
        id: "llama3",
        title: "Llama 3",
        subtitle: "Older but still heavily used Meta chat models",
        description: "Still one of the high-pull Llama families on Ollama. The 8B variant remains a useful baseline when comparing newer local models.",
        tags: &["chat", "meta", "popular"],
        variants: &[
            ModelVariant {
                name: "llama3:8b",
                title: "8B",
                description: "Classic local Llama 3 baseline.",
                tags: &["popular", "chat"],
            },
            ModelVariant {
                name: "llama3:8b-instruct-q4_K_M",
                title: "8B Q4",
                description: "Quantized instruct variant.",
                tags: &["q4", "chat"],
            },
            ModelVariant {
                name: "llama3:70b",
                title: "70B",
                description: "Large Llama 3 variant.",
                tags: &["large", "chat"],
            },
        ],
    },
    ModelFamily {
        id: "deepseek-r1",
        title: "DeepSeek R1",
        subtitle: "Popular reasoning model family",
        description: "Reasoning-focused models and distilled variants. Smaller tags are useful locally; larger tags need much more memory.",
        tags: &["chat", "reasoning", "thinking"],
        variants: &[
            ModelVariant {
                name: "deepseek-r1:1.5b",
                title: "1.5B",
                description: "Tiny reasoning distill for light machines.",
                tags: &["tiny", "reasoning"],
            },
            ModelVariant {
                name: "deepseek-r1:7b",
                title: "7B",
                description: "Popular local reasoning default.",
                tags: &["recommended", "reasoning", "chat"],
            },
            ModelVariant {
                name: "deepseek-r1:8b",
                title: "8B",
                description: "Qwen3-based refreshed distill.",
                tags: &["reasoning", "chat"],
            },
            ModelVariant {
                name: "deepseek-r1:14b",
                title: "14B",
                description: "Stronger reasoning on mid-range hardware.",
                tags: &["medium", "reasoning"],
            },
            ModelVariant {
                name: "deepseek-r1:32b",
                title: "32B",
                description: "High quality reasoning for large local setups.",
                tags: &["large", "reasoning"],
            },
            ModelVariant {
                name: "deepseek-r1:70b",
                title: "70B",
                description: "Large reasoning model for high-memory systems.",
                tags: &["large", "reasoning"],
            },
        ],
    },
    ModelFamily {
        id: "gemma3",
        title: "Gemma 3",
        subtitle: "Efficient Google models with small and vision-capable sizes",
        description: "A broad family that scales from tiny local assistants to larger, more capable models. Useful when you want efficient everyday chat.",
        tags: &["chat", "vision", "efficient", "google"],
        variants: &[
            ModelVariant {
                name: "gemma3:270m",
                title: "270M",
                description: "Very small instruction model.",
                tags: &["tiny", "fast"],
            },
            ModelVariant {
                name: "gemma3:1b",
                title: "1B",
                description: "Small and responsive local chat.",
                tags: &["small", "chat"],
            },
            ModelVariant {
                name: "gemma3:4b",
                title: "4B",
                description: "Recommended local balance for Gemma.",
                tags: &["recommended", "chat", "vision"],
            },
            ModelVariant {
                name: "gemma3:12b",
                title: "12B",
                description: "Better quality for machines with more memory.",
                tags: &["medium", "chat", "vision"],
            },
            ModelVariant {
                name: "gemma3:27b",
                title: "27B",
                description: "Large Gemma model for stronger answers.",
                tags: &["large", "chat", "vision"],
            },
        ],
    },
    ModelFamily {
        id: "gemma3n",
        title: "Gemma 3n",
        subtitle: "Efficient Gemma models for everyday devices",
        description: "Compact Gemma 3n variants designed for efficient execution on laptops and everyday devices. Good when you want newer Gemma behavior without a huge model.",
        tags: &["chat", "small", "efficient", "google"],
        variants: &[
            ModelVariant {
                name: "gemma3n:e2b",
                title: "E2B",
                description: "Smaller efficient Gemma 3n variant.",
                tags: &["small", "efficient"],
            },
            ModelVariant {
                name: "gemma3n:e4b",
                title: "E4B",
                description: "More capable efficient Gemma 3n variant.",
                tags: &["recommended", "small", "efficient"],
            },
            ModelVariant {
                name: "gemma3n:e4b-it-q4_K_M",
                title: "E4B IT Q4",
                description: "Quantized instruction-tuned tag.",
                tags: &["q4", "chat"],
            },
        ],
    },
    ModelFamily {
        id: "gemma2",
        title: "Gemma 2",
        subtitle: "Very popular efficient Google model family",
        description: "Older than Gemma 3, but still high on Ollama's popularity list. Useful for comparing small and medium Google models.",
        tags: &["chat", "google", "popular"],
        variants: &[
            ModelVariant {
                name: "gemma2:2b",
                title: "2B",
                description: "Small Gemma 2 model.",
                tags: &["small", "chat"],
            },
            ModelVariant {
                name: "gemma2:9b",
                title: "9B",
                description: "Balanced Gemma 2 model.",
                tags: &["recommended", "chat"],
            },
            ModelVariant {
                name: "gemma2:27b",
                title: "27B",
                description: "Large Gemma 2 model.",
                tags: &["large", "chat"],
            },
        ],
    },
    ModelFamily {
        id: "gemma4",
        title: "Gemma 4",
        subtitle: "Newer Gemma family with local and larger variants",
        description: "Recent Gemma models with compact expert variants and larger instruction tags. Good to try when you want the newest Google family available in Ollama.",
        tags: &["chat", "google", "new"],
        variants: &[
            ModelVariant {
                name: "gemma4:e2b",
                title: "E2B",
                description: "Compact expert variant for lighter local use.",
                tags: &["small", "chat"],
            },
            ModelVariant {
                name: "gemma4:e4b",
                title: "E4B",
                description: "Small expert variant with more capacity.",
                tags: &["small", "chat"],
            },
            ModelVariant {
                name: "gemma4:26b",
                title: "26B",
                description: "Larger instruction model.",
                tags: &["large", "chat"],
            },
            ModelVariant {
                name: "gemma4:31b",
                title: "31B",
                description: "Large Gemma 4 variant for strong quality.",
                tags: &["large", "chat"],
            },
        ],
    },
    ModelFamily {
        id: "translategemma",
        title: "TranslateGemma",
        subtitle: "Recent Gemma translation models",
        description: "A newer Gemma-based translation family for communication across many languages. Useful when translation is the main workflow.",
        tags: &["translation", "multilingual", "google", "new"],
        variants: &[
            ModelVariant {
                name: "translategemma:4b",
                title: "4B",
                description: "Small translation model.",
                tags: &["recommended", "translation"],
            },
            ModelVariant {
                name: "translategemma:12b",
                title: "12B",
                description: "Medium translation model.",
                tags: &["translation", "medium"],
            },
            ModelVariant {
                name: "translategemma:27b",
                title: "27B",
                description: "Large translation model.",
                tags: &["translation", "large"],
            },
        ],
    },
    ModelFamily {
        id: "medgemma",
        title: "MedGemma",
        subtitle: "Recent specialized Gemma medical models",
        description: "Specialized Gemma variants for medical text and image comprehension. They are included for specialized workflows and should not replace professional medical judgment.",
        tags: &["specialized", "medical", "vision", "new"],
        variants: &[
            ModelVariant {
                name: "medgemma:4b",
                title: "4B",
                description: "Small specialized medical model.",
                tags: &["specialized", "vision"],
            },
            ModelVariant {
                name: "medgemma:27b",
                title: "27B",
                description: "Large specialized medical model.",
                tags: &["large", "specialized"],
            },
        ],
    },
    ModelFamily {
        id: "qwen2.5",
        title: "Qwen 2.5",
        subtitle: "Strong multilingual general models",
        description: "Popular multilingual chat models with many sizes. The 7B tag is a reliable local default, and larger tags improve quality when hardware allows.",
        tags: &["chat", "multilingual", "tools"],
        variants: &[
            ModelVariant {
                name: "qwen2.5:0.5b",
                title: "0.5B",
                description: "Tiny multilingual model.",
                tags: &["tiny", "fast"],
            },
            ModelVariant {
                name: "qwen2.5:1.5b",
                title: "1.5B",
                description: "Small multilingual chat.",
                tags: &["small", "chat"],
            },
            ModelVariant {
                name: "qwen2.5:3b",
                title: "3B",
                description: "Lightweight everyday chat.",
                tags: &["small", "chat"],
            },
            ModelVariant {
                name: "qwen2.5:7b",
                title: "7B",
                description: "Recommended local default.",
                tags: &["recommended", "chat", "tools"],
            },
            ModelVariant {
                name: "qwen2.5:14b",
                title: "14B",
                description: "Stronger multilingual answers.",
                tags: &["medium", "chat"],
            },
            ModelVariant {
                name: "qwen2.5:32b",
                title: "32B",
                description: "Large local model for better quality.",
                tags: &["large", "chat"],
            },
            ModelVariant {
                name: "qwen2.5:72b",
                title: "72B",
                description: "Very large multilingual model.",
                tags: &["very large", "chat"],
            },
        ],
    },
    ModelFamily {
        id: "qwen3",
        title: "Qwen 3",
        subtitle: "Popular multilingual reasoning models",
        description: "Reasoning-capable Qwen models with small dense tags and larger mixture-of-experts tags. Strong choice for multilingual work.",
        tags: &["chat", "reasoning", "multilingual", "thinking"],
        variants: &[
            ModelVariant {
                name: "qwen3:0.6b",
                title: "0.6B",
                description: "Tiny Qwen 3 chat.",
                tags: &["tiny", "fast"],
            },
            ModelVariant {
                name: "qwen3:1.7b",
                title: "1.7B",
                description: "Small reasoning model.",
                tags: &["small", "thinking"],
            },
            ModelVariant {
                name: "qwen3:4b",
                title: "4B",
                description: "Recommended small reasoning model.",
                tags: &["recommended", "thinking"],
            },
            ModelVariant {
                name: "qwen3:8b",
                title: "8B",
                description: "Balanced reasoning and chat.",
                tags: &["chat", "thinking"],
            },
            ModelVariant {
                name: "qwen3:14b",
                title: "14B",
                description: "Stronger reasoning on mid-range hardware.",
                tags: &["medium", "thinking"],
            },
            ModelVariant {
                name: "qwen3:30b-a3b",
                title: "30B A3B",
                description: "Mixture-of-experts model with active 3B path.",
                tags: &["moe", "thinking"],
            },
            ModelVariant {
                name: "qwen3:32b",
                title: "32B",
                description: "Large dense Qwen 3 model.",
                tags: &["large", "thinking"],
            },
        ],
    },
    ModelFamily {
        id: "qwen3.5",
        title: "Qwen 3.5",
        subtitle: "Newer Qwen family with compact and MoE tags",
        description: "Recent Qwen models with small local sizes and larger expert variants. Useful for multilingual chat, coding and reasoning.",
        tags: &["chat", "reasoning", "coding", "new"],
        variants: &[
            ModelVariant {
                name: "qwen3.5:0.8b",
                title: "0.8B",
                description: "Tiny fast model.",
                tags: &["tiny", "fast"],
            },
            ModelVariant {
                name: "qwen3.5:2b",
                title: "2B",
                description: "Small general model.",
                tags: &["small", "chat"],
            },
            ModelVariant {
                name: "qwen3.5:4b",
                title: "4B",
                description: "Practical recent Qwen model for local chat.",
                tags: &["recommended", "chat"],
            },
            ModelVariant {
                name: "qwen3.5:9b",
                title: "9B",
                description: "Stronger recent Qwen model for local systems.",
                tags: &["chat", "thinking"],
            },
            ModelVariant {
                name: "qwen3.5:27b",
                title: "27B",
                description: "Large dense model with coding variants.",
                tags: &["large", "coding"],
            },
            ModelVariant {
                name: "qwen3.5:35b-a3b",
                title: "35B A3B",
                description: "Mixture-of-experts model for larger local setups.",
                tags: &["moe", "coding"],
            },
        ],
    },
    ModelFamily {
        id: "qwen3.6",
        title: "Qwen 3.6",
        subtitle: "Recent Qwen upgrade for coding and thinking",
        description: "A newer Qwen generation shown in Ollama's recent model listings. It focuses on agentic coding and better thinking preservation.",
        tags: &["chat", "coding", "thinking", "new"],
        variants: &[
            ModelVariant {
                name: "qwen3.6:27b",
                title: "27B",
                description: "Dense recent Qwen model.",
                tags: &["large", "chat", "thinking"],
            },
            ModelVariant {
                name: "qwen3.6:27b-q4_K_M",
                title: "27B Q4",
                description: "Quantized dense recent Qwen model.",
                tags: &["q4", "coding", "thinking"],
            },
            ModelVariant {
                name: "qwen3.6:35b-a3b",
                title: "35B A3B",
                description: "Mixture-of-experts recent Qwen model.",
                tags: &["moe", "coding", "thinking"],
            },
            ModelVariant {
                name: "qwen3.6:35b-a3b-q4_K_M",
                title: "35B A3B Q4",
                description: "Quantized MoE variant.",
                tags: &["q4", "moe", "coding"],
            },
        ],
    },
    ModelFamily {
        id: "gpt-oss",
        title: "GPT OSS",
        subtitle: "Open-weight GPT-style models",
        description: "Open-weight models intended for capable local and workstation setups. The 20B tag is the practical local starting point.",
        tags: &["chat", "reasoning", "open weights"],
        variants: &[
            ModelVariant {
                name: "gpt-oss:20b",
                title: "20B",
                description: "Recommended local GPT OSS variant.",
                tags: &["recommended", "chat"],
            },
            ModelVariant {
                name: "gpt-oss:120b",
                title: "120B",
                description: "Very large GPT OSS variant.",
                tags: &["very large", "chat"],
            },
        ],
    },
    ModelFamily {
        id: "mistral",
        title: "Mistral",
        subtitle: "Fast general chat model",
        description: "Classic Mistral 7B family. Still useful for fast local chat and lightweight instruction tasks.",
        tags: &["chat", "fast"],
        variants: &[
            ModelVariant {
                name: "mistral:7b",
                title: "7B",
                description: "Default Mistral local chat model.",
                tags: &["recommended", "chat"],
            },
            ModelVariant {
                name: "mistral:7b-instruct-v0.3-q4_K_M",
                title: "7B Instruct v0.3 Q4",
                description: "Quantized instruction variant.",
                tags: &["instruct", "q4", "chat"],
            },
        ],
    },
    ModelFamily {
        id: "mistral-nemo",
        title: "Mistral Nemo",
        subtitle: "Popular 12B Mistral model with long context",
        description: "A high-pull Mistral family built with NVIDIA collaboration. Useful as a stronger local alternative to classic Mistral 7B.",
        tags: &["chat", "tools", "mistral", "popular"],
        variants: &[
            ModelVariant {
                name: "mistral-nemo:12b",
                title: "12B",
                description: "Default Mistral Nemo model.",
                tags: &["recommended", "medium", "chat"],
            },
            ModelVariant {
                name: "mistral-nemo:12b-instruct-2407-q4_K_M",
                title: "12B Q4",
                description: "Quantized instruction variant.",
                tags: &["q4", "chat"],
            },
        ],
    },
    ModelFamily {
        id: "mistral-small",
        title: "Mistral Small",
        subtitle: "Popular 22B and 24B Mistral Small models",
        description: "A widely used Mistral family below 70B. Good for stronger local chat when 7B models feel too small.",
        tags: &["chat", "tools", "mistral", "popular"],
        variants: &[
            ModelVariant {
                name: "mistral-small:22b",
                title: "22B",
                description: "Earlier Mistral Small tag.",
                tags: &["large", "chat"],
            },
            ModelVariant {
                name: "mistral-small:24b",
                title: "24B",
                description: "Newer Mistral Small tag.",
                tags: &["recommended", "large", "chat"],
            },
            ModelVariant {
                name: "mistral-small:24b-instruct-2501-q4_K_M",
                title: "24B Q4",
                description: "Quantized instruction variant.",
                tags: &["q4", "chat"],
            },
        ],
    },
    ModelFamily {
        id: "mistral-small3.2",
        title: "Mistral Small 3.2",
        subtitle: "Modern 24B Mistral instruction model",
        description: "A newer 24B Mistral model for stronger general chat when you have enough memory.",
        tags: &["chat", "vision", "mistral"],
        variants: &[
            ModelVariant {
                name: "mistral-small3.2:24b",
                title: "24B",
                description: "Default 24B instruction model.",
                tags: &["large", "chat"],
            },
            ModelVariant {
                name: "mistral-small3.2:24b-instruct-2506-q4_K_M",
                title: "24B Q4",
                description: "Quantized instruction variant.",
                tags: &["large", "q4", "chat"],
            },
        ],
    },
    ModelFamily {
        id: "mistral-medium-3.5",
        title: "Mistral Medium 3.5",
        subtitle: "Recent 128B flagship Mistral model",
        description: "A recent Mistral model shown in Ollama's newest listings. It is large, but useful to expose for users with workstation or remote Ollama providers.",
        tags: &["chat", "vision", "tools", "thinking", "new"],
        variants: &[
            ModelVariant {
                name: "mistral-medium-3.5:128b",
                title: "128B",
                description: "Default Mistral Medium 3.5 tag.",
                tags: &["very large", "chat"],
            },
            ModelVariant {
                name: "mistral-medium-3.5:128b-q4_K_M",
                title: "128B Q4",
                description: "Quantized large Mistral Medium tag.",
                tags: &["q4", "very large"],
            },
        ],
    },
    ModelFamily {
        id: "ministral-3",
        title: "Ministral 3",
        subtitle: "Recent edge-friendly Mistral family",
        description: "A newer Mistral family designed for edge deployment with 3B, 8B and 14B sizes. Good fit for laptops and local-first use.",
        tags: &["chat", "vision", "tools", "new", "mistral"],
        variants: &[
            ModelVariant {
                name: "ministral-3:3b",
                title: "3B",
                description: "Small edge-friendly model.",
                tags: &["small", "fast"],
            },
            ModelVariant {
                name: "ministral-3:8b",
                title: "8B",
                description: "Balanced local Ministral model.",
                tags: &["recommended", "chat"],
            },
            ModelVariant {
                name: "ministral-3:14b",
                title: "14B",
                description: "Stronger local Ministral model.",
                tags: &["medium", "chat"],
            },
        ],
    },
    ModelFamily {
        id: "phi4",
        title: "Phi 4",
        subtitle: "Compact Microsoft model for reasoning and chat",
        description: "A compact 14B model that works well for practical local assistant tasks.",
        tags: &["chat", "reasoning", "microsoft"],
        variants: &[
            ModelVariant {
                name: "phi4:14b",
                title: "14B",
                description: "Default Phi 4 model.",
                tags: &["recommended", "chat"],
            },
            ModelVariant {
                name: "phi4:14b-q4_K_M",
                title: "14B Q4",
                description: "Quantized Phi 4 variant.",
                tags: &["q4", "chat"],
            },
        ],
    },
    ModelFamily {
        id: "phi4-reasoning",
        title: "Phi 4 Reasoning",
        subtitle: "Microsoft reasoning model in a compact 14B size",
        description: "A reasoning-focused Phi 4 family. Useful when you want a smaller model for multi-step reasoning without jumping to very large models.",
        tags: &["chat", "reasoning", "thinking", "microsoft"],
        variants: &[
            ModelVariant {
                name: "phi4-reasoning:14b",
                title: "14B",
                description: "Default Phi 4 reasoning model.",
                tags: &["recommended", "reasoning"],
            },
            ModelVariant {
                name: "phi4-reasoning:14b-q4_K_M",
                title: "14B Q4",
                description: "Quantized reasoning variant.",
                tags: &["q4", "reasoning"],
            },
            ModelVariant {
                name: "phi4-reasoning:plus",
                title: "Plus",
                description: "Reasoning-plus variant.",
                tags: &["reasoning", "chat"],
            },
        ],
    },
    ModelFamily {
        id: "phi3",
        title: "Phi 3",
        subtitle: "Small Microsoft models",
        description: "Older but useful compact Microsoft models for low-memory local machines.",
        tags: &["chat", "small", "microsoft"],
        variants: &[
            ModelVariant {
                name: "phi3:3.8b",
                title: "3.8B",
                description: "Small Phi 3 mini model.",
                tags: &["small", "chat"],
            },
            ModelVariant {
                name: "phi3:14b",
                title: "14B",
                description: "Medium Phi 3 model.",
                tags: &["medium", "chat"],
            },
        ],
    },
    ModelFamily {
        id: "qwen2.5-coder",
        title: "Qwen 2.5 Coder",
        subtitle: "Coding-specialized Qwen models",
        description: "Popular code-focused models for completion, explanation and software tasks. The 7B tag is a good local start.",
        tags: &["coding", "chat", "multilingual"],
        variants: &[
            ModelVariant {
                name: "qwen2.5-coder:1.5b",
                title: "1.5B",
                description: "Small code model.",
                tags: &["small", "coding"],
            },
            ModelVariant {
                name: "qwen2.5-coder:7b",
                title: "7B",
                description: "Recommended local coding model.",
                tags: &["recommended", "coding"],
            },
            ModelVariant {
                name: "qwen2.5-coder:14b",
                title: "14B",
                description: "Stronger coding model.",
                tags: &["medium", "coding"],
            },
            ModelVariant {
                name: "qwen2.5-coder:32b",
                title: "32B",
                description: "Large coding model.",
                tags: &["large", "coding"],
            },
        ],
    },
    ModelFamily {
        id: "qwen3-coder",
        title: "Qwen 3 Coder",
        subtitle: "Popular Qwen coding models for agentic tasks",
        description: "Qwen's newer coding family appears high in Ollama's popular list. The 30B A3B tag is the practical local starting point.",
        tags: &["coding", "tools", "popular"],
        variants: &[
            ModelVariant {
                name: "qwen3-coder:30b",
                title: "30B A3B",
                description: "Default local Qwen 3 Coder tag.",
                tags: &["recommended", "coding", "moe"],
            },
            ModelVariant {
                name: "qwen3-coder:30b-a3b-q4_K_M",
                title: "30B A3B Q4",
                description: "Quantized local coding variant.",
                tags: &["q4", "coding", "moe"],
            },
            ModelVariant {
                name: "qwen3-coder:480b",
                title: "480B A35B",
                description: "Very large Qwen coding model.",
                tags: &["very large", "coding", "moe"],
            },
        ],
    },
    ModelFamily {
        id: "qwen3-coder-next",
        title: "Qwen 3 Coder Next",
        subtitle: "Recent coding-focused Qwen model",
        description: "A newer Qwen coding model from Ollama's recent list. Include it for users who want the freshest coding tag.",
        tags: &["coding", "tools", "new"],
        variants: &[
            ModelVariant {
                name: "qwen3-coder-next:q4_K_M",
                title: "Q4",
                description: "Quantized local coding tag.",
                tags: &["recommended", "q4", "coding"],
            },
            ModelVariant {
                name: "qwen3-coder-next:q8_0",
                title: "Q8",
                description: "Higher precision local coding tag.",
                tags: &["q8", "coding"],
            },
        ],
    },
    ModelFamily {
        id: "deepseek-coder",
        title: "DeepSeek Coder",
        subtitle: "Popular coding model family",
        description: "A high-pull coding model family on Ollama. Useful for users who want a lightweight code-specific model.",
        tags: &["coding", "popular"],
        variants: &[
            ModelVariant {
                name: "deepseek-coder:1.3b",
                title: "1.3B",
                description: "Small code model.",
                tags: &["small", "coding"],
            },
            ModelVariant {
                name: "deepseek-coder:6.7b",
                title: "6.7B",
                description: "Balanced local code model.",
                tags: &["recommended", "coding"],
            },
            ModelVariant {
                name: "deepseek-coder:33b",
                title: "33B",
                description: "Large code model.",
                tags: &["large", "coding"],
            },
        ],
    },
    ModelFamily {
        id: "starcoder2",
        title: "StarCoder2",
        subtitle: "Popular open code model family",
        description: "A code-focused family with small, medium and larger tags. Good to expose alongside Qwen and DeepSeek coder models.",
        tags: &["coding", "popular"],
        variants: &[
            ModelVariant {
                name: "starcoder2:3b",
                title: "3B",
                description: "Small code model.",
                tags: &["small", "coding"],
            },
            ModelVariant {
                name: "starcoder2:7b",
                title: "7B",
                description: "Balanced code model.",
                tags: &["recommended", "coding"],
            },
            ModelVariant {
                name: "starcoder2:15b",
                title: "15B",
                description: "Larger code model.",
                tags: &["medium", "coding"],
            },
        ],
    },
    ModelFamily {
        id: "codegemma",
        title: "CodeGemma",
        subtitle: "Google code models in compact sizes",
        description: "A popular coding family with 2B and 7B variants for code completion and instruction-style coding tasks.",
        tags: &["coding", "google", "popular"],
        variants: &[
            ModelVariant {
                name: "codegemma:2b",
                title: "2B",
                description: "Small code completion model.",
                tags: &["small", "coding"],
            },
            ModelVariant {
                name: "codegemma:7b",
                title: "7B",
                description: "Recommended CodeGemma local tag.",
                tags: &["recommended", "coding"],
            },
        ],
    },
    ModelFamily {
        id: "codellama",
        title: "Code Llama",
        subtitle: "Code-focused Llama family",
        description: "Established coding models for code generation and explanation. Useful as a fallback coding family.",
        tags: &["coding", "chat", "meta"],
        variants: &[
            ModelVariant {
                name: "codellama:7b",
                title: "7B",
                description: "Small coding model.",
                tags: &["small", "coding"],
            },
            ModelVariant {
                name: "codellama:13b",
                title: "13B",
                description: "Medium coding model.",
                tags: &["medium", "coding"],
            },
            ModelVariant {
                name: "codellama:34b",
                title: "34B",
                description: "Large coding model.",
                tags: &["large", "coding"],
            },
        ],
    },
    ModelFamily {
        id: "llama3.2-vision",
        title: "Llama 3.2 Vision",
        subtitle: "Vision-capable Llama models",
        description: "Multimodal Llama variants. Moose can download them through Ollama; text chat support depends on how you use the provider.",
        tags: &["vision", "chat", "meta"],
        variants: &[
            ModelVariant {
                name: "llama3.2-vision:11b",
                title: "11B",
                description: "Smaller vision-capable Llama model.",
                tags: &["vision", "medium"],
            },
            ModelVariant {
                name: "llama3.2-vision:90b",
                title: "90B",
                description: "Large vision-capable Llama model.",
                tags: &["vision", "large"],
            },
        ],
    },
    ModelFamily {
        id: "qwen3-vl",
        title: "Qwen 3 VL",
        subtitle: "Vision-language Qwen models",
        description: "Vision-language Qwen variants with instruct and thinking tags. Useful for multimodal workflows once the UI supports image input.",
        tags: &["vision", "multilingual", "thinking"],
        variants: &[
            ModelVariant {
                name: "qwen3-vl:2b",
                title: "2B",
                description: "Small vision-language variant.",
                tags: &["small", "vision"],
            },
            ModelVariant {
                name: "qwen3-vl:4b",
                title: "4B",
                description: "Compact vision-language model.",
                tags: &["small", "vision"],
            },
            ModelVariant {
                name: "qwen3-vl:8b",
                title: "8B",
                description: "Balanced vision-language model.",
                tags: &["vision", "thinking"],
            },
            ModelVariant {
                name: "qwen3-vl:30b-a3b",
                title: "30B A3B",
                description: "Larger mixture-of-experts vision model.",
                tags: &["vision", "moe"],
            },
        ],
    },
    ModelFamily {
        id: "qwen2.5vl",
        title: "Qwen 2.5 VL",
        subtitle: "Popular Qwen vision-language model family",
        description: "Qwen's earlier flagship vision-language family remains popular on Ollama and includes practical 3B and 7B local sizes.",
        tags: &["vision", "multilingual", "popular"],
        variants: &[
            ModelVariant {
                name: "qwen2.5vl:3b",
                title: "3B",
                description: "Small vision-language model.",
                tags: &["small", "vision"],
            },
            ModelVariant {
                name: "qwen2.5vl:7b",
                title: "7B",
                description: "Recommended local vision-language model.",
                tags: &["recommended", "vision"],
            },
            ModelVariant {
                name: "qwen2.5vl:32b",
                title: "32B",
                description: "Large vision-language model.",
                tags: &["large", "vision"],
            },
        ],
    },
    ModelFamily {
        id: "llava",
        title: "LLaVA",
        subtitle: "Classic high-pull vision-language model",
        description: "One of the most pulled multimodal model families in Ollama. Useful for users who want broad compatibility with older vision workflows.",
        tags: &["vision", "popular"],
        variants: &[
            ModelVariant {
                name: "llava:7b",
                title: "7B",
                description: "Small LLaVA vision model.",
                tags: &["recommended", "vision"],
            },
            ModelVariant {
                name: "llava:13b",
                title: "13B",
                description: "Medium LLaVA vision model.",
                tags: &["medium", "vision"],
            },
            ModelVariant {
                name: "llava:34b",
                title: "34B",
                description: "Large LLaVA vision model.",
                tags: &["large", "vision"],
            },
        ],
    },
    ModelFamily {
        id: "minicpm-v",
        title: "MiniCPM-V",
        subtitle: "Popular compact multimodal model",
        description: "A popular vision-language model designed for multimodal understanding in a comparatively compact size.",
        tags: &["vision", "small", "popular"],
        variants: &[ModelVariant {
            name: "minicpm-v:8b",
            title: "8B",
            description: "Default MiniCPM-V local tag.",
            tags: &["recommended", "vision"],
        }],
    },
    ModelFamily {
        id: "olmo2",
        title: "OLMo 2",
        subtitle: "Fully open 7B and 13B models",
        description: "Popular fully open models from Ai2. Good to include for users who prefer open training data and transparent model families.",
        tags: &["chat", "open", "popular"],
        variants: &[
            ModelVariant {
                name: "olmo2:7b",
                title: "7B",
                description: "Small OLMo 2 model.",
                tags: &["small", "open"],
            },
            ModelVariant {
                name: "olmo2:13b",
                title: "13B",
                description: "Recommended OLMo 2 model.",
                tags: &["recommended", "open"],
            },
            ModelVariant {
                name: "olmo2:13b-1124-instruct-q4_K_M",
                title: "13B Q4",
                description: "Quantized instruction tag.",
                tags: &["q4", "chat"],
            },
        ],
    },
    ModelFamily {
        id: "mixtral",
        title: "Mixtral",
        subtitle: "Popular Mistral mixture-of-experts models",
        description: "Older but still widely used MoE models from Mistral. Useful for larger local and remote Ollama systems.",
        tags: &["chat", "tools", "moe", "popular"],
        variants: &[
            ModelVariant {
                name: "mixtral:8x7b",
                title: "8x7B",
                description: "Classic Mixtral MoE model.",
                tags: &["moe", "chat"],
            },
            ModelVariant {
                name: "mixtral:8x22b",
                title: "8x22B",
                description: "Large Mixtral MoE model.",
                tags: &["large", "moe"],
            },
        ],
    },
    ModelFamily {
        id: "qwq",
        title: "QwQ",
        subtitle: "Qwen reasoning model",
        description: "A popular Qwen reasoning model. Useful as a dedicated thinking model alongside DeepSeek R1 and Qwen 3.",
        tags: &["chat", "reasoning", "thinking", "popular"],
        variants: &[
            ModelVariant {
                name: "qwq:32b",
                title: "32B",
                description: "Default QwQ reasoning model.",
                tags: &["recommended", "reasoning"],
            },
            ModelVariant {
                name: "qwq:32b-q4_K_M",
                title: "32B Q4",
                description: "Quantized QwQ reasoning tag.",
                tags: &["q4", "thinking"],
            },
        ],
    },
    ModelFamily {
        id: "deepseek-v3",
        title: "DeepSeek V3",
        subtitle: "Very large MoE model for remote providers",
        description: "A high-pull DeepSeek MoE model. It is too large for most local machines, but useful when Moose points to a powerful remote Ollama provider.",
        tags: &["chat", "moe", "very large", "popular"],
        variants: &[
            ModelVariant {
                name: "deepseek-v3:671b",
                title: "671B",
                description: "Default very large DeepSeek V3 tag.",
                tags: &["very large", "moe"],
            },
            ModelVariant {
                name: "deepseek-v3:671b-q4_K_M",
                title: "671B Q4",
                description: "Quantized very large DeepSeek V3 tag.",
                tags: &["q4", "very large"],
            },
        ],
    },
    ModelFamily {
        id: "laguna-xs.2",
        title: "Laguna XS.2",
        subtitle: "Recent local MoE model for coding and long-horizon work",
        description: "A recent model in Ollama's newest list, designed for agentic coding with a compact active parameter count.",
        tags: &["coding", "thinking", "new", "moe"],
        variants: &[
            ModelVariant {
                name: "laguna-xs.2:q4_K_M",
                title: "Q4",
                description: "Quantized Laguna XS.2 tag.",
                tags: &["recommended", "q4", "coding"],
            },
            ModelVariant {
                name: "laguna-xs.2:q8_0",
                title: "Q8",
                description: "Higher precision Laguna XS.2 tag.",
                tags: &["q8", "coding"],
            },
        ],
    },
    ModelFamily {
        id: "lfm2",
        title: "LFM2",
        subtitle: "Recent hybrid model for on-device deployment",
        description: "A newer hybrid model family shown in Ollama's recent list. The 24B A2B tag offers an efficient active-parameter path.",
        tags: &["chat", "tools", "new", "moe"],
        variants: &[
            ModelVariant {
                name: "lfm2:24b-a2b",
                title: "24B A2B",
                description: "Default efficient LFM2 tag.",
                tags: &["recommended", "moe"],
            },
            ModelVariant {
                name: "lfm2:24b-q4_K_M",
                title: "24B Q4",
                description: "Quantized LFM2 variant.",
                tags: &["q4", "chat"],
            },
        ],
    },
    ModelFamily {
        id: "nemotron-3-super",
        title: "Nemotron 3 Super",
        subtitle: "Recent NVIDIA 120B MoE model",
        description: "A recent NVIDIA MoE model. Exposed for remote Ollama providers and high-memory systems.",
        tags: &["chat", "thinking", "tools", "new", "very large"],
        variants: &[
            ModelVariant {
                name: "nemotron-3-super:120b-a12b",
                title: "120B A12B",
                description: "Default Nemotron 3 Super MoE tag.",
                tags: &["moe", "very large"],
            },
            ModelVariant {
                name: "nemotron-3-super:120b-a12b-q4_K_M",
                title: "120B Q4",
                description: "Quantized Nemotron 3 Super tag.",
                tags: &["q4", "very large"],
            },
        ],
    },
    ModelFamily {
        id: "smollm2",
        title: "SmolLM2",
        subtitle: "Very small instruction models",
        description: "Tiny instruction models for low-resource machines and quick tests.",
        tags: &["chat", "small", "fast"],
        variants: &[
            ModelVariant {
                name: "smollm2:135m",
                title: "135M",
                description: "Tiny test model.",
                tags: &["tiny", "fast"],
            },
            ModelVariant {
                name: "smollm2:360m",
                title: "360M",
                description: "Very small local model.",
                tags: &["tiny", "fast"],
            },
            ModelVariant {
                name: "smollm2:1.7b",
                title: "1.7B",
                description: "Small instruction model.",
                tags: &["small", "chat"],
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
        .spacing(16)
        .hexpand(true)
        .vexpand(true)
        .build();
    content.add_css_class("moose-model-manager-content");

    let header = gtk::Box::builder()
        .orientation(Orientation::Vertical)
        .spacing(12)
        .halign(Align::Center)
        .hexpand(true)
        .build();
    header.add_css_class("moose-model-manager-header");

    let title_row = gtk::Box::builder()
        .orientation(Orientation::Horizontal)
        .spacing(8)
        .halign(Align::Center)
        .build();

    let title = gtk::Label::builder()
        .label("Models")
        .halign(Align::Center)
        .xalign(0.0)
        .build();
    title.add_css_class("title-2");

    let actions = gtk::Box::builder()
        .orientation(Orientation::Horizontal)
        .spacing(6)
        .halign(Align::Center)
        .build();

    let pull_button = icon_button("folder-download-symbolic", "Pull Model");
    pull_button.add_css_class("suggested-action");
    pull_button.set_sensitive(false);

    let download_jobs_button = icon_button("view-list-symbolic", "Download Jobs");

    let refresh_button = icon_button("view-refresh-symbolic", "Refresh Models");
    refresh_button.set_sensitive(false);

    title_row.append(&title);
    actions.append(&refresh_button);
    actions.append(&download_jobs_button);
    actions.append(&pull_button);

    let search_entry = gtk::SearchEntry::builder()
        .placeholder_text("Search Models")
        .halign(Align::Center)
        .build();
    search_entry.set_size_request(420, -1);
    search_entry.set_sensitive(false);
    search_entry.add_css_class("moose-model-search");

    header.append(&title_row);
    header.append(&actions);
    header.append(&search_entry);

    let installed_label = section_label("Installed Models");
    installed_label.add_css_class("moose-model-section");

    let pull_panel = gtk::Box::builder()
        .orientation(Orientation::Vertical)
        .spacing(8)
        .hexpand(true)
        .visible(false)
        .build();
    pull_panel.add_css_class("moose-pull-panel");

    let pull_header = gtk::Box::builder()
        .orientation(Orientation::Horizontal)
        .spacing(8)
        .hexpand(true)
        .build();

    let pull_title = gtk::Label::builder()
        .label("Downloading Model")
        .halign(Align::Start)
        .hexpand(true)
        .xalign(0.0)
        .build();
    pull_title.add_css_class("heading");

    let pull_cancel_button = icon_button("process-stop-symbolic", "Cancel Download");
    pull_cancel_button.add_css_class("destructive-action");
    pull_cancel_button.set_sensitive(false);

    pull_header.append(&pull_title);
    pull_header.append(&pull_cancel_button);

    let pull_status = gtk::Label::builder()
        .label("Preparing download")
        .halign(Align::Start)
        .xalign(0.0)
        .wrap(true)
        .build();
    pull_status.add_css_class("dim-label");

    let pull_progress = gtk::ProgressBar::builder()
        .hexpand(true)
        .pulse_step(0.08)
        .build();
    pull_progress.add_css_class("moose-pull-progress");

    let pull_progress_label = gtk::Label::builder()
        .label("")
        .halign(Align::Start)
        .xalign(0.0)
        .build();
    pull_progress_label.add_css_class("caption");
    pull_progress_label.add_css_class("dim-label");

    pull_panel.append(&pull_header);
    pull_panel.append(&pull_status);
    pull_panel.append(&pull_progress);
    pull_panel.append(&pull_progress_label);

    let model_list = gtk::ListBox::new();
    model_list.set_selection_mode(gtk::SelectionMode::None);
    model_list.add_css_class("moose-model-list");

    let available_label = section_label("Popular and Recent Models");
    available_label.add_css_class("moose-model-section");

    let available_model_list = gtk::ListBox::new();
    available_model_list.set_selection_mode(gtk::SelectionMode::None);
    available_model_list.add_css_class("moose-model-list");

    let models_content = gtk::Box::builder()
        .orientation(Orientation::Vertical)
        .spacing(8)
        .hexpand(true)
        .vexpand(true)
        .build();
    models_content.add_css_class("moose-model-lists");
    models_content.append(&installed_label);
    models_content.append(&model_list);
    models_content.append(&available_label);
    models_content.append(&available_model_list);

    let status_page = adw::StatusPage::builder()
        .icon_name(APPLICATION_ID)
        .title("No Models Loaded")
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

    let scrolled = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(PolicyType::Never)
        .vscrollbar_policy(PolicyType::Automatic)
        .hexpand(true)
        .vexpand(true)
        .child(&stack)
        .build();
    scrolled.add_css_class("moose-model-scroll");

    content.append(&header);
    content.append(&pull_panel);
    content.append(&scrolled);

    let clamp = adw::Clamp::builder()
        .maximum_size(900)
        .tightening_threshold(640)
        .hexpand(true)
        .vexpand(true)
        .child(&content)
        .build();

    root.append(&clamp);

    ModelManager {
        root,
        pull_button,
        download_jobs_button,
        refresh_button,
        search_entry,
        pull_cancel_button,
        pull_panel,
        pull_title,
        pull_status,
        pull_progress,
        pull_progress_label,
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
    manager.status_page.set_title("Loading Models");
    manager.status_page.set_description(None);
    manager.stack.set_visible_child_name("empty");
}

pub(super) fn set_unavailable(manager: &ModelManager, title: &str, description: &str) {
    clear_model_lists(manager);
    manager.pull_button.set_sensitive(false);
    manager.refresh_button.set_sensitive(true);
    manager.search_entry.set_sensitive(false);
    manager.status_page.set_title(title);
    manager.status_page.set_description(Some(description));
    manager.stack.set_visible_child_name("empty");
}

pub(super) fn clear_download_job(manager: &ModelManager) {
    manager.pull_panel.set_visible(false);
    manager.pull_cancel_button.set_sensitive(false);
    manager.pull_progress.set_fraction(0.0);
    manager.pull_progress_label.set_label("");
}

pub(super) fn set_models(
    manager: &ModelManager,
    parent: &adw::ApplicationWindow,
    models: &[OllamaModel],
    query: &str,
    on_pull: Rc<dyn Fn(String)>,
    on_delete: Rc<dyn Fn(String)>,
) {
    clear_model_lists(manager);
    manager.pull_button.set_sensitive(true);
    manager.refresh_button.set_sensitive(true);
    manager.search_entry.set_sensitive(true);

    let query = query.trim().to_ascii_lowercase();
    let filtered_models = models
        .iter()
        .filter(|model| model_matches_query(model, &query))
        .collect::<Vec<_>>();

    if filtered_models.is_empty() {
        let row = empty_row(if models.is_empty() {
            "No installed models"
        } else {
            "No installed matches"
        });
        manager.model_list.append(&row);
    } else {
        for model in filtered_models {
            manager
                .model_list
                .append(&model_row(model, Rc::clone(&on_delete)));
        }
    }

    let installed_names = models
        .iter()
        .map(|model| model.name.as_str())
        .collect::<Vec<_>>();
    let filtered_available = MODEL_FAMILIES
        .iter()
        .filter(|family| model_family_matches_query(family, &query))
        .collect::<Vec<_>>();

    if filtered_available.is_empty() {
        manager
            .available_model_list
            .append(&empty_row("No available matches"));
    } else {
        for family in filtered_available {
            manager.available_model_list.append(&model_family_row(
                parent,
                family,
                &installed_names,
                Rc::clone(&on_pull),
            ));
        }
    }

    manager.stack.set_visible_child_name("models");
}

pub(super) fn set_pull_started(manager: &ModelManager, model: &str) {
    manager.pull_panel.set_visible(true);
    manager.pull_button.set_sensitive(false);
    manager.refresh_button.set_sensitive(false);
    manager.pull_cancel_button.set_sensitive(true);
    manager
        .pull_title
        .set_label(&format!("Downloading {model}"));
    manager.pull_status.set_label("Starting download");
    manager.pull_progress.set_fraction(0.0);
    manager
        .pull_progress_label
        .set_label("Waiting for progress");
}

pub(super) fn set_pull_progress(
    manager: &ModelManager,
    model: &str,
    progress: &OllamaPullProgress,
) {
    manager.pull_panel.set_visible(true);
    manager
        .pull_title
        .set_label(&format!("Downloading {model}"));
    manager.pull_status.set_label(&progress.status);

    match (progress.completed_bytes, progress.total_bytes) {
        (Some(completed), Some(total)) if total > 0 => {
            let fraction = (completed as f64 / total as f64).clamp(0.0, 1.0);
            manager.pull_progress.set_fraction(fraction);
            manager.pull_progress_label.set_label(&format!(
                "{} of {}",
                format_size(Some(completed)),
                format_size(Some(total))
            ));
        }
        _ => {
            manager.pull_progress.pulse();
            manager
                .pull_progress_label
                .set_label("Waiting for progress");
        }
    }
}

pub(super) fn set_pull_finished(manager: &ModelManager, title: &str, status: &str, fraction: f64) {
    manager.pull_panel.set_visible(true);
    manager.pull_title.set_label(title);
    manager.pull_status.set_label(status);
    manager.pull_progress.set_fraction(fraction.clamp(0.0, 1.0));
    manager.pull_progress_label.set_label("");
    manager.pull_cancel_button.set_sensitive(false);
    manager.pull_button.set_sensitive(true);
    manager.refresh_button.set_sensitive(true);
}

pub(super) fn set_delete_started(manager: &ModelManager, model: &str) {
    manager.pull_panel.set_visible(true);
    manager.pull_button.set_sensitive(false);
    manager.refresh_button.set_sensitive(false);
    manager.pull_cancel_button.set_sensitive(false);
    manager.pull_title.set_label(&format!("Deleting {model}"));
    manager.pull_status.set_label("Removing model from Ollama");
    manager.pull_progress.set_fraction(0.0);
    manager.pull_progress_label.set_label("");
}

pub(super) fn set_delete_finished(
    manager: &ModelManager,
    title: &str,
    status: &str,
    fraction: f64,
) {
    manager.pull_panel.set_visible(true);
    manager.pull_title.set_label(title);
    manager.pull_status.set_label(status);
    manager.pull_progress.set_fraction(fraction.clamp(0.0, 1.0));
    manager.pull_progress_label.set_label("");
    manager.pull_cancel_button.set_sensitive(false);
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

fn model_row(model: &OllamaModel, on_delete: Rc<dyn Fn(String)>) -> adw::ActionRow {
    let row = adw::ActionRow::builder()
        .title(&model.name)
        .subtitle(&model_subtitle(model))
        .subtitle_lines(2)
        .build();
    row.add_css_class("moose-model-row-item");
    row.set_tooltip_text(Some(&model.name));

    let meta = gtk::Box::builder()
        .orientation(Orientation::Horizontal)
        .spacing(6)
        .halign(Align::End)
        .valign(Align::Center)
        .build();

    let capability = if model.supports_chat { "Chat" } else { "Other" };
    meta.append(&pill_label(capability));
    meta.append(&pill_label(&format_size(model.size_bytes)));
    row.add_suffix(&meta);

    let delete_button = icon_button("user-trash-symbolic", &format!("Delete {}", model.name));
    delete_button.add_css_class("destructive-action");
    let model_name = model.name.clone();
    delete_button.connect_clicked(move |_| {
        on_delete(model_name.clone());
    });
    row.add_suffix(&delete_button);
    row
}

fn model_family_row(
    parent: &adw::ApplicationWindow,
    family: &'static ModelFamily,
    installed_names: &[&str],
    on_pull: Rc<dyn Fn(String)>,
) -> adw::ActionRow {
    let installed_count = family
        .variants
        .iter()
        .filter(|variant| model_is_installed(variant.name, installed_names))
        .count();
    let row = adw::ActionRow::builder()
        .title(family.title)
        .subtitle(&format!(
            "{} - {} variations",
            family.subtitle,
            family.variants.len()
        ))
        .subtitle_lines(2)
        .build();
    row.add_css_class("moose-model-row-item");
    row.set_activatable(true);
    row.set_tooltip_text(Some("Show variations"));

    if installed_count > 0 {
        row.add_suffix(&pill_label(&format!("{installed_count} installed")));
    }
    row.add_suffix(&pill_label(&format!("{} tags", family.variants.len())));

    let chevron = gtk::Image::from_icon_name("go-next-symbolic");
    row.add_suffix(&chevron);

    let target_parent = parent.clone();
    let target_on_pull = Rc::clone(&on_pull);
    let installed_names = installed_names
        .iter()
        .map(|name| (*name).to_string())
        .collect::<Vec<_>>();
    let click = gtk::GestureClick::new();
    click.connect_released(move |_, _, _, _| {
        let installed_names = installed_names
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>();
        show_model_family_dialog(
            &target_parent,
            family,
            installed_names.as_slice(),
            Rc::clone(&target_on_pull),
        );
    });
    row.add_controller(click);

    row
}

fn show_model_family_dialog(
    parent: &adw::ApplicationWindow,
    family: &'static ModelFamily,
    installed_names: &[&str],
    on_pull: Rc<dyn Fn(String)>,
) {
    let dialog = adw::Dialog::builder()
        .title(family.title)
        .content_width(640)
        .build();

    let header_bar = adw::HeaderBar::builder()
        .show_start_title_buttons(false)
        .show_end_title_buttons(false)
        .build();
    let title = adw::WindowTitle::new(family.title, family.subtitle);
    header_bar.set_title_widget(Some(&title));

    let ollama_button = icon_button("web-browser-symbolic", "Open on Ollama");
    let ollama_url = format!("https://ollama.com/library/{}", family.id);
    let target_parent = parent.clone();
    ollama_button.connect_clicked(move |_| {
        let launcher = gtk::UriLauncher::new(&ollama_url);
        launcher.launch(Some(&target_parent), None::<&gio::Cancellable>, |_| {});
    });
    header_bar.pack_start(&ollama_button);

    let close_button = icon_button("window-close-symbolic", "Close");
    let target_dialog = dialog.clone();
    close_button.connect_clicked(move |_| {
        target_dialog.close();
    });
    header_bar.pack_end(&close_button);

    let content = gtk::Box::builder()
        .orientation(Orientation::Vertical)
        .spacing(12)
        .hexpand(true)
        .build();
    content.add_css_class("moose-model-dialog-content");

    let description = gtk::Label::builder()
        .label(family.description)
        .halign(Align::Center)
        .xalign(0.5)
        .justify(gtk::Justification::Center)
        .wrap(true)
        .build();
    description.add_css_class("dim-label");

    let tag_row = gtk::FlowBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .column_spacing(6)
        .row_spacing(6)
        .halign(Align::Start)
        .hexpand(true)
        .build();
    tag_row.add_css_class("moose-model-tag-flow");
    for tag in family.tags {
        tag_row.append(&pill_label(tag));
    }

    let variant_list = gtk::ListBox::new();
    variant_list.set_selection_mode(gtk::SelectionMode::None);
    variant_list.add_css_class("moose-model-list");
    variant_list.add_css_class("moose-model-variant-list");
    for variant in family.variants {
        variant_list.append(&model_variant_row(
            variant,
            installed_names,
            Rc::clone(&on_pull),
        ));
    }

    let scrolled = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(PolicyType::Never)
        .vscrollbar_policy(PolicyType::Automatic)
        .min_content_height(260)
        .max_content_height(420)
        .child(&variant_list)
        .build();
    scrolled.add_css_class("moose-model-dialog-scroll");

    content.append(&description);
    content.append(&section_label("Tags"));
    content.append(&tag_row);
    content.append(&section_label("Variations"));
    content.append(&scrolled);

    let toolbar_view = adw::ToolbarView::builder()
        .top_bar_style(adw::ToolbarStyle::Flat)
        .content(&content)
        .build();
    toolbar_view.add_top_bar(&header_bar);

    dialog.set_child(Some(&toolbar_view));
    dialog.present(Some(parent));
}

fn model_variant_row(
    variant: &ModelVariant,
    installed_names: &[&str],
    on_pull: Rc<dyn Fn(String)>,
) -> gtk::ListBoxRow {
    let row = gtk::ListBoxRow::builder()
        .activatable(false)
        .selectable(false)
        .build();
    row.add_css_class("moose-model-variant-row");
    row.set_tooltip_text(Some(variant.name));

    let content = gtk::Box::builder()
        .orientation(Orientation::Horizontal)
        .spacing(12)
        .margin_top(10)
        .margin_bottom(10)
        .margin_start(12)
        .margin_end(12)
        .hexpand(true)
        .build();

    let copy = gtk::Box::builder()
        .orientation(Orientation::Vertical)
        .spacing(6)
        .hexpand(true)
        .valign(Align::Center)
        .build();

    let title = gtk::Label::builder()
        .label(variant.name)
        .halign(Align::Start)
        .xalign(0.0)
        .hexpand(true)
        .wrap(true)
        .wrap_mode(gtk::pango::WrapMode::WordChar)
        .build();
    title.add_css_class("moose-model-variant-title");

    let description = gtk::Label::builder()
        .label(&format!("{} - {}", variant.title, variant.description))
        .halign(Align::Start)
        .xalign(0.0)
        .hexpand(true)
        .wrap(true)
        .wrap_mode(gtk::pango::WrapMode::WordChar)
        .build();
    description.add_css_class("moose-model-variant-description");

    let tag_row = gtk::FlowBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .column_spacing(6)
        .row_spacing(6)
        .halign(Align::Start)
        .hexpand(true)
        .build();
    tag_row.add_css_class("moose-model-tag-flow");
    for tag in variant.tags {
        tag_row.append(&pill_label(tag));
    }

    copy.append(&title);
    copy.append(&description);
    copy.append(&tag_row);
    content.append(&copy);

    let action_box = gtk::Box::builder()
        .orientation(Orientation::Horizontal)
        .halign(Align::End)
        .valign(Align::Center)
        .build();

    if model_is_installed(variant.name, installed_names) {
        action_box.append(&pill_label("Installed"));
    } else {
        let download_button = icon_button(
            "folder-download-symbolic",
            &format!("Download {}", variant.name),
        );
        download_button.add_css_class("suggested-action");
        let model_name = variant.name.to_string();
        download_button.connect_clicked(move |_| {
            on_pull(model_name.clone());
        });
        action_box.append(&download_button);
    }

    content.append(&action_box);
    row.set_child(Some(&content));

    row
}

fn model_is_installed(model_name: &str, installed_names: &[&str]) -> bool {
    installed_names.iter().any(|name| *name == model_name)
}

fn pill_label(text: &str) -> gtk::Label {
    let label = gtk::Label::builder()
        .label(text)
        .halign(Align::Center)
        .valign(Align::Center)
        .build();
    label.add_css_class("moose-model-pill");
    label
}

fn model_subtitle(model: &OllamaModel) -> String {
    let mut parts = Vec::new();

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
        parts.push(format!("Modified {}", compact_timestamp(modified_at)));
    }

    if parts.is_empty() {
        "Installed locally".to_string()
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
        return "Unknown size".to_string();
    };

    let units = ["B", "KB", "MB", "GB", "TB"];
    let mut size = size_bytes as f64;
    let mut unit_index = 0;

    while size >= 1024.0 && unit_index + 1 < units.len() {
        size /= 1024.0;
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

fn model_family_matches_query(family: &ModelFamily, query: &str) -> bool {
    if query.is_empty() {
        return true;
    }

    contains_query(family.id, query)
        || contains_query(family.title, query)
        || contains_query(family.subtitle, query)
        || contains_query(family.description, query)
        || family.tags.iter().any(|tag| contains_query(tag, query))
        || family.variants.iter().any(|variant| {
            contains_query(variant.name, query)
                || contains_query(variant.title, query)
                || contains_query(variant.description, query)
                || variant.tags.iter().any(|tag| contains_query(tag, query))
        })
}

fn contains_query(value: &str, query: &str) -> bool {
    value.to_ascii_lowercase().contains(query)
}
