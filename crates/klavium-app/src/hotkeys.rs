//! Разбор и сопоставление хоткеев.
//!
//! Спецификация: триггер + модификаторы через `+` (регистр не важен,
//! `-` тоже допустим). Триггер — функциональная клавиша (`F8`), именованная
//! клавиша (`Space`, `Enter`, `Tab`, `Esc`, `Backspace`) или символ
//! по физической EN-раскладке (`Q`).

use crate::engine::Modifiers;

/// Проверить, что событие клавиши соответствует спецификации хоткея.
#[must_use]
pub fn matches(spec: &str, vk_code: u32, scan_code: u32, modifiers: &Modifiers) -> bool {
    let parts: Vec<String> = spec
        .replace('-', "+")
        .split('+')
        .map(|part| part.trim().to_lowercase())
        .filter(|part| !part.is_empty())
        .collect();
    if parts.is_empty() {
        return false;
    }

    let mut trigger: Option<&str> = None;
    let mut need = Modifiers::default();
    for part in &parts {
        match canonical_modifier(part) {
            Some("ctrl") => need.ctrl = true,
            Some("shift") => need.shift = true,
            Some("alt") => need.alt = true,
            Some("win") => need.win = true,
            _ => {
                if trigger.is_some() {
                    return false; // несколько триггеров — не поддерживаем
                }
                trigger = Some(part.as_str());
            }
        }
    }
    let Some(trigger) = trigger else {
        return false;
    };

    if (need.ctrl && !modifiers.ctrl)
        || (need.shift && !modifiers.shift)
        || (need.alt && !modifiers.alt)
        || (need.win && !modifiers.win)
    {
        return false;
    }

    trigger_matches(trigger, vk_code, scan_code)
}

fn canonical_modifier(token: &str) -> Option<&'static str> {
    match token {
        "ctrl" | "control" => Some("ctrl"),
        "shift" => Some("shift"),
        "alt" => Some("alt"),
        "win" | "windows" | "cmd" => Some("win"),
        _ => None,
    }
}

fn trigger_matches(trigger: &str, vk_code: u32, scan_code: u32) -> bool {
    if let Some(vk) = named_key_vk(trigger) {
        return vk_code == vk;
    }
    if let Some(vk) = function_key_vk(trigger) {
        return vk_code == vk;
    }
    let single = {
        let mut chars = trigger.chars();
        match (chars.next(), chars.next()) {
            (Some(first), None) => Some(first),
            _ => None,
        }
    };
    if let Some(first) = single {
        if let Some(qwerty) = klavium_core::keys::qwerty_char(scan_code) {
            return qwerty.to_ascii_lowercase() == first;
        }
    }
    false
}

fn named_key_vk(name: &str) -> Option<u32> {
    match name {
        "space" => Some(0x20),
        "enter" | "return" => Some(0x0D),
        "tab" => Some(0x09),
        "esc" | "escape" => Some(0x1B),
        "backspace" => Some(0x08),
        _ => None,
    }
}

fn function_key_vk(trigger: &str) -> Option<u32> {
    let number: u32 = trigger.strip_prefix('f')?.parse().ok()?;
    if (1..=24).contains(&number) {
        Some(0x70 + number - 1)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_function_key() {
        assert!(matches("F8", 0x77, 0x42, &Modifiers::default()));
        assert!(matches("f8", 0x77, 0x42, &Modifiers::default()));
        assert!(!matches("F8", 0x78, 0x43, &Modifiers::default()));
        // Посторонние нажатые модификаторы не мешают результату
        let ctrl = Modifiers {
            ctrl: true,
            ..Modifiers::default()
        };
        assert!(matches("F8", 0x77, 0x42, &ctrl));
    }

    #[test]
    fn test_modifiers_required() {
        let ctrl = Modifiers {
            ctrl: true,
            ..Modifiers::default()
        };
        assert!(matches("Ctrl+Q", 0x51, 0x10, &ctrl));
        assert!(!matches("Ctrl+Q", 0x51, 0x10, &Modifiers::default()));
        assert!(matches("control-q", 0x51, 0x10, &ctrl));
    }

    #[test]
    fn test_aliases_and_named_keys() {
        let win = Modifiers {
            win: true,
            ..Modifiers::default()
        };
        assert!(matches("Win+Space", 0x20, 0x39, &win));
        assert!(matches("windows+space", 0x20, 0x39, &win));
        assert!(!matches("Win+Space", 0x20, 0x39, &Modifiers::default()));
        assert!(matches("Esc", 0x1B, 0x01, &Modifiers::default()));
    }

    #[test]
    fn test_invalid_specs() {
        assert!(!matches("", 0x77, 0x42, &Modifiers::default()));
        assert!(!matches("ctrl+shift", 0x10, 0x2A, &Modifiers::default()));
        assert!(!matches("F8+Q", 0x77, 0x10, &Modifiers::default()));
    }
}
