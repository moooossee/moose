use std::collections::{HashMap, HashSet};

use gtk::gdk;

pub(super) const SETTINGS_SHORTCUTS: &str = "shortcuts";

#[derive(Clone, Copy)]
pub(super) struct ShortcutDefinition {
    pub(super) id: &'static str,
    pub(super) title: &'static str,
    pub(super) description: &'static str,
    pub(super) default: &'static str,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(super) struct ShortcutChord {
    ctrl: bool,
    alt: bool,
    shift: bool,
    super_key: bool,
    key: String,
}

pub(super) const DEFINITIONS: &[ShortcutDefinition] = &[
    ShortcutDefinition {
        id: "new-conversation",
        title: "New Conversation",
        description: "Create a new chat and move to the composer.",
        default: "Ctrl+N",
    },
    ShortcutDefinition {
        id: "focus-message",
        title: "Focus Message",
        description: "Move keyboard focus to the message composer.",
        default: "Ctrl+L",
    },
    ShortcutDefinition {
        id: "search-chats",
        title: "Search Chats",
        description: "Move keyboard focus to the chat search field.",
        default: "Ctrl+F",
    },
    ShortcutDefinition {
        id: "show-models",
        title: "Show Models",
        description: "Open the model manager.",
        default: "Ctrl+M",
    },
    ShortcutDefinition {
        id: "refresh-models",
        title: "Refresh Models",
        description: "Refresh the active provider model list.",
        default: "F5",
    },
    ShortcutDefinition {
        id: "preferences",
        title: "Preferences",
        description: "Open application preferences.",
        default: "Ctrl+,",
    },
    ShortcutDefinition {
        id: "toggle-sidebar",
        title: "Toggle Sidebar",
        description: "Show or hide the conversation sidebar.",
        default: "F9",
    },
    ShortcutDefinition {
        id: "stop-generation",
        title: "Stop Generation",
        description: "Cancel the active response.",
        default: "Esc",
    },
];

pub(super) fn defaults() -> HashMap<String, String> {
    DEFINITIONS
        .iter()
        .map(|definition| (definition.id.to_string(), definition.default.to_string()))
        .collect()
}

pub(super) fn merged_values(stored: HashMap<String, String>) -> HashMap<String, String> {
    let mut values = defaults();
    for definition in DEFINITIONS {
        if let Some(value) = stored.get(definition.id) {
            values.insert(definition.id.to_string(), value.to_string());
        }
    }
    normalize_values(values).unwrap_or_else(|_| defaults())
}

pub(super) fn normalize_values(
    values: HashMap<String, String>,
) -> Result<HashMap<String, String>, String> {
    let mut normalized = HashMap::new();
    let mut chords = HashSet::new();

    for definition in DEFINITIONS {
        let raw = values
            .get(definition.id)
            .map(String::as_str)
            .unwrap_or(definition.default);
        let Some(chord) = parse(raw)? else {
            normalized.insert(definition.id.to_string(), String::new());
            continue;
        };
        if !chords.insert(chord.clone()) {
            return Err(format!(
                "{} uses a shortcut that is already assigned",
                definition.title
            ));
        }
        normalized.insert(definition.id.to_string(), chord.label());
    }

    Ok(normalized)
}

pub(super) fn parse(value: &str) -> Result<Option<ShortcutChord>, String> {
    let value = normalize_input(value);
    if value.is_empty() {
        return Ok(None);
    }

    let mut ctrl = false;
    let mut alt = false;
    let mut shift = false;
    let mut super_key = false;
    let mut key = None;

    for part in value.split('+') {
        let part = part.trim();
        if part.is_empty() {
            return Err("Shortcut parts cannot be empty".to_string());
        }

        match part.to_ascii_lowercase().as_str() {
            "ctrl" | "control" | "primary" => ctrl = true,
            "alt" | "option" => alt = true,
            "shift" => shift = true,
            "super" | "meta" | "cmd" | "command" => super_key = true,
            _ => {
                if key.replace(normalize_key(part)?).is_some() {
                    return Err("Use only one key per shortcut".to_string());
                }
            }
        }
    }

    let Some(key) = key else {
        return Err("Add a key to the shortcut".to_string());
    };

    if !ctrl && !alt && !shift && !super_key && requires_modifier(&key) {
        return Err("Use Ctrl, Alt, Shift or Super with printable shortcuts".to_string());
    }

    Ok(Some(ShortcutChord {
        ctrl,
        alt,
        shift,
        super_key,
        key,
    }))
}

pub(super) fn event_chord(key: gdk::Key, state: gdk::ModifierType) -> Option<ShortcutChord> {
    let key = event_key_name(key)?;
    Some(ShortcutChord {
        ctrl: state.contains(gdk::ModifierType::CONTROL_MASK),
        alt: state.contains(gdk::ModifierType::ALT_MASK),
        shift: state.contains(gdk::ModifierType::SHIFT_MASK),
        super_key: state.contains(gdk::ModifierType::SUPER_MASK),
        key,
    })
}

impl ShortcutChord {
    pub(super) fn label(&self) -> String {
        let mut parts = Vec::new();
        if self.ctrl {
            parts.push("Ctrl".to_string());
        }
        if self.alt {
            parts.push("Alt".to_string());
        }
        if self.shift {
            parts.push("Shift".to_string());
        }
        if self.super_key {
            parts.push("Super".to_string());
        }
        parts.push(self.key.clone());
        parts.join("+")
    }
}

fn normalize_input(value: &str) -> String {
    let value = value.trim();
    let mut output = String::new();
    let mut bracket = String::new();
    let mut in_bracket = false;

    for character in value.chars() {
        match character {
            '<' if !in_bracket => {
                in_bracket = true;
                bracket.clear();
            }
            '>' if in_bracket => {
                output.push_str(&bracket);
                output.push('+');
                in_bracket = false;
            }
            _ if in_bracket => bracket.push(character),
            _ => output.push(character),
        }
    }

    if in_bracket {
        value.to_string()
    } else {
        output
    }
}

fn normalize_key(value: &str) -> Result<String, String> {
    let lower = value.to_ascii_lowercase();
    let key = match lower.as_str() {
        "escape" | "esc" => "Esc".to_string(),
        "return" | "enter" => "Enter".to_string(),
        "space" => "Space".to_string(),
        "tab" => "Tab".to_string(),
        "backspace" => "Backspace".to_string(),
        "delete" | "del" => "Delete".to_string(),
        "insert" | "ins" => "Insert".to_string(),
        "home" => "Home".to_string(),
        "end" => "End".to_string(),
        "pageup" | "page-up" | "page up" => "PageUp".to_string(),
        "pagedown" | "page-down" | "page down" => "PageDown".to_string(),
        "up" | "arrowup" | "arrow-up" => "Up".to_string(),
        "down" | "arrowdown" | "arrow-down" => "Down".to_string(),
        "left" | "arrowleft" | "arrow-left" => "Left".to_string(),
        "right" | "arrowright" | "arrow-right" => "Right".to_string(),
        _ if is_function_key(&lower) => lower.to_ascii_uppercase(),
        _ if value.chars().count() == 1 => normalize_character_key(value),
        _ => return Err(format!("Unsupported key: {value}")),
    };
    Ok(key)
}

fn normalize_character_key(value: &str) -> String {
    let character = value.chars().next().unwrap_or_default();
    if character.is_ascii_alphabetic() {
        character.to_ascii_uppercase().to_string()
    } else {
        character.to_string()
    }
}

fn is_function_key(value: &str) -> bool {
    let Some(number) = value.strip_prefix('f') else {
        return false;
    };
    number
        .parse::<u8>()
        .is_ok_and(|number| (1..=24).contains(&number))
}

fn requires_modifier(key: &str) -> bool {
    key == "Space" || key.chars().count() == 1
}

fn event_key_name(key: gdk::Key) -> Option<String> {
    if let Some(character) = key.to_unicode() {
        if character == ' ' {
            return Some("Space".to_string());
        }
        if !character.is_control() {
            return Some(normalize_character_key(&character.to_string()));
        }
    }

    match key {
        gdk::Key::Escape => Some("Esc".to_string()),
        gdk::Key::Return | gdk::Key::KP_Enter | gdk::Key::ISO_Enter => Some("Enter".to_string()),
        gdk::Key::Tab | gdk::Key::ISO_Left_Tab => Some("Tab".to_string()),
        gdk::Key::BackSpace => Some("Backspace".to_string()),
        gdk::Key::Delete => Some("Delete".to_string()),
        gdk::Key::Insert => Some("Insert".to_string()),
        gdk::Key::Home => Some("Home".to_string()),
        gdk::Key::End => Some("End".to_string()),
        gdk::Key::Page_Up => Some("PageUp".to_string()),
        gdk::Key::Page_Down => Some("PageDown".to_string()),
        gdk::Key::Up => Some("Up".to_string()),
        gdk::Key::Down => Some("Down".to_string()),
        gdk::Key::Left => Some("Left".to_string()),
        gdk::Key::Right => Some("Right".to_string()),
        gdk::Key::F1 => Some("F1".to_string()),
        gdk::Key::F2 => Some("F2".to_string()),
        gdk::Key::F3 => Some("F3".to_string()),
        gdk::Key::F4 => Some("F4".to_string()),
        gdk::Key::F5 => Some("F5".to_string()),
        gdk::Key::F6 => Some("F6".to_string()),
        gdk::Key::F7 => Some("F7".to_string()),
        gdk::Key::F8 => Some("F8".to_string()),
        gdk::Key::F9 => Some("F9".to_string()),
        gdk::Key::F10 => Some("F10".to_string()),
        gdk::Key::F11 => Some("F11".to_string()),
        gdk::Key::F12 => Some("F12".to_string()),
        gdk::Key::F13 => Some("F13".to_string()),
        gdk::Key::F14 => Some("F14".to_string()),
        gdk::Key::F15 => Some("F15".to_string()),
        gdk::Key::F16 => Some("F16".to_string()),
        gdk::Key::F17 => Some("F17".to_string()),
        gdk::Key::F18 => Some("F18".to_string()),
        gdk::Key::F19 => Some("F19".to_string()),
        gdk::Key::F20 => Some("F20".to_string()),
        gdk::Key::F21 => Some("F21".to_string()),
        gdk::Key::F22 => Some("F22".to_string()),
        gdk::Key::F23 => Some("F23".to_string()),
        gdk::Key::F24 => Some("F24".to_string()),
        _ => None,
    }
}
