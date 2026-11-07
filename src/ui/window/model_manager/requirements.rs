use std::rc::Rc;

use adw::prelude::*;
use gtk::gio;

use super::{Candidate, MemoryBudget, format_size};

const GIB: u64 = 1024 * 1024 * 1024;

struct RequirementRows {
    model: adw::ActionRow,
    ram: adw::ActionRow,
    vram: adw::ActionRow,
    storage: adw::ActionRow,
    available: adw::ActionRow,
}

impl RequirementRows {
    fn update(&self, candidate: Candidate) {
        let bytes = candidate.variant.download_bytes;
        let ram = MemoryBudget::estimate_bytes(bytes).div_ceil(GIB);
        self.model.set_subtitle(candidate.variant.name);
        self.ram.set_subtitle(&format!(
            "About {ram} GiB of available RAM for CPU-only use. Leave additional memory for the system and other apps."
        ));
        self.vram.set_subtitle(&format!(
            "Model files are about {}. Full GPU execution also needs VRAM for context and runtime. The exact VRAM minimum is not verified.",
            format_size(Some(bytes)),
        ));
        self.storage.set_subtitle(&format!(
            "About {} for model files. Ollama and its runtime need additional disk space. Shared files may reduce the additional space used.",
            format_size(Some(bytes)),
        ));
        self.available
            .set_subtitle(&candidate.memory.available.map_or_else(
                || {
                    "Not available for this connection. Check the hardware of the Ollama server."
                        .to_string()
                },
                |available| {
                    format!(
                        "{:.1} GiB available on this device. {}.",
                        available as f64 / GIB as f64,
                        candidate.memory.guidance(bytes),
                    )
                },
            ));
    }
}

pub(super) fn build(parent: &adw::ApplicationWindow, candidate: Candidate) -> adw::ExpanderRow {
    let technical = adw::ExpanderRow::builder()
        .title("Technical Details")
        .build();
    let rows = Rc::new(RequirementRows {
        model: detail_row("Model", ""),
        ram: detail_row("RAM for CPU Use (Estimated)", ""),
        vram: detail_row("VRAM for Full GPU Use", ""),
        storage: detail_row("Disk Space", ""),
        available: detail_row("Available System RAM", ""),
    });
    rows.update(candidate);

    let variants = std::iter::once(candidate.variant)
        .chain(
            candidate
                .family
                .variants
                .iter()
                .filter(|variant| variant.name != candidate.variant.name),
        )
        .collect::<Vec<_>>();
    if variants.len() > 1 {
        let names = variants
            .iter()
            .map(|variant| variant.title)
            .collect::<Vec<_>>();
        let version = adw::ComboRow::builder()
            .title("Requirements for Version")
            .model(&gtk::StringList::new(&names))
            .selected(0)
            .build();
        let version_rows = Rc::clone(&rows);
        version.connect_selected_notify(move |row| {
            if let Some(variant) = variants.get(row.selected() as usize) {
                version_rows.update(Candidate {
                    variant,
                    ..candidate
                });
            }
        });
        technical.add_row(&version);
    }

    technical.add_row(&rows.model);
    technical.add_row(&detail_row(
        "CPU",
        "64-bit x86_64 or ARM64 CPU supported by Ollama. CPU-only execution is available; speed depends on the processor and model.",
    ));
    technical.add_row(&rows.ram);
    technical.add_row(&rows.vram);
    technical.add_row(&rows.storage);
    technical.add_row(&rows.available);
    technical.add_row(&detail_row(
        "Context Window",
        &format!(
            "Moose defaults to {} tokens. Chat settings can override this value; longer contexts and images can use substantially more memory.",
            crate::ollama::DEFAULT_CONTEXT_WINDOW,
        ),
    ));
    let gpu = detail_row(
        "GPU Compatibility",
        "A dedicated GPU is optional. Acceleration requires a supported GPU and driver; RAM and VRAM can also be used together. View Ollama's supported hardware.",
    );
    gpu.set_activatable(true);
    gpu.add_suffix(&gtk::Image::from_icon_name("external-link-symbolic"));
    let weak_parent = parent.downgrade();
    gpu.connect_activated(move |_| {
        if let Some(parent) = weak_parent.upgrade() {
            gtk::UriLauncher::new("https://docs.ollama.com/gpu").launch(
                Some(&parent),
                None::<&gio::Cancellable>,
                |_| {},
            );
        }
    });
    technical.add_row(&gpu);
    technical.add_row(&detail_row(
        "About These Estimates",
        "The RAM budget uses model file size with runtime headroom. It is a planning estimate, not a verified minimum or a guarantee that the model will fit. Requirements depend on context, architecture, images, and backend settings.",
    ));
    technical.add_row(&detail_row(
        "Model Tags",
        &candidate.family.tags.join(" · "),
    ));
    technical
}

fn detail_row(title: &str, subtitle: &str) -> adw::ActionRow {
    adw::ActionRow::builder()
        .title(title)
        .subtitle(subtitle)
        .subtitle_lines(0)
        .use_markup(false)
        .build()
}
