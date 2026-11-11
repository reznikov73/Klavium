//! Горячие клавиши: разбор спецификации и сопоставление с событием клавиши.
//!
//! Спецификация — модификаторы и клавиша через `+` (регистр не важен;
//! `-` и `_` тоже работают как разделитель: `Ctrl-Q`). Клавиша —
//! функциональная (`F8`), именованная (`Space`, `PageUp`, `Delete`) или
//! символ физической EN-раскладки (`Q`, `1`, `-`). Кириллица нормализуется
//! в ту же физическую клавишу (`Ф` → `A`), поэтому привязка не зависит от
//! текущей раскладки.
//!
//! Сопоставление **строгое**: лишний зажатый модификатор отменяет совпадение —
//! `F8` не срабатывает на `Alt+F8` и не отбирает чужие сочетания.
//! Пустая строка означает «не задано».

use std::fmt;

use thiserror::Error;

use crate::keys;

/// Виртуальный код `VK_F1`: остальные функциональные клавиши идут подряд.
const VK_F1: u32 = 0x70;
/// Сколько функциональных клавиш поддерживаем (`F1`–`F24`).
const MAX_FUNCTION_KEY: u8 = 24;

/// Состояние модификаторов при нажатии клавиши.
#[allow(clippy::struct_excessive_bools)]
// четыре независимых модификатора — данные, а не состояние
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct Mods {
    /// Shift нажат.
    pub shift: bool,
    /// Ctrl нажат.
    pub ctrl: bool,
    /// Alt нажат.
    pub alt: bool,
    /// Win нажат.
    pub win: bool,
}

impl Mods {
    /// Ни один модификатор не нажат.
    #[must_use]
    pub const fn none() -> Self {
        Self {
            shift: false,
            ctrl: false,
            alt: false,
            win: false,
        }
    }

    /// Ни один модификатор не нажат.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        !(self.shift || self.ctrl || self.alt || self.win)
    }
}

/// Модификатор, распознанный в спецификации.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Modifier {
    Ctrl,
    Shift,
    Alt,
    Win,
}

/// Именованная (не печатная) клавиша.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NamedKey {
    /// Пробел.
    Space,
    /// Enter.
    Enter,
    /// Tab.
    Tab,
    /// Esc.
    Escape,
    /// Backspace.
    Backspace,
    /// Insert.
    Insert,
    /// Delete.
    Delete,
    /// Home.
    Home,
    /// End.
    End,
    /// Page Up.
    PageUp,
    /// Page Down.
    PageDown,
    /// Стрелка влево.
    Left,
    /// Стрелка вправо.
    Right,
    /// Стрелка вверх.
    Up,
    /// Стрелка вниз.
    Down,
}

impl NamedKey {
    /// Виртуальный код клавиши (`VK_*`).
    #[must_use]
    pub const fn vk(self) -> u32 {
        match self {
            Self::Space => 0x20,
            Self::Enter => 0x0D,
            Self::Tab => 0x09,
            Self::Escape => 0x1B,
            Self::Backspace => 0x08,
            Self::Insert => 0x2D,
            Self::Delete => 0x2E,
            Self::Home => 0x24,
            Self::End => 0x23,
            Self::PageUp => 0x21,
            Self::PageDown => 0x22,
            Self::Left => 0x25,
            Self::Right => 0x27,
            Self::Up => 0x26,
            Self::Down => 0x28,
        }
    }

    /// Каноническое имя для [`Hotkey::to_spec`].
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Space => "Space",
            Self::Enter => "Enter",
            Self::Tab => "Tab",
            Self::Escape => "Esc",
            Self::Backspace => "Backspace",
            Self::Insert => "Insert",
            Self::Delete => "Delete",
            Self::Home => "Home",
            Self::End => "End",
            Self::PageUp => "PageUp",
            Self::PageDown => "PageDown",
            Self::Left => "Left",
            Self::Right => "Right",
            Self::Up => "Up",
            Self::Down => "Down",
        }
    }

    /// Разобрать имя клавиши (регистр не важен, `-`/`_`/пробелы игнорируются).
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        let normalized: String = name
            .chars()
            .filter(|c| !matches!(c, '-' | '_' | ' '))
            .flat_map(char::to_lowercase)
            .collect();
        Some(match normalized.as_str() {
            "space" => Self::Space,
            "enter" | "return" => Self::Enter,
            "tab" => Self::Tab,
            "esc" | "escape" => Self::Escape,
            "backspace" | "back" => Self::Backspace,
            "insert" | "ins" => Self::Insert,
            "delete" | "del" => Self::Delete,
            "home" => Self::Home,
            "end" => Self::End,
            "pageup" | "pgup" => Self::PageUp,
            "pagedown" | "pgdn" => Self::PageDown,
            "left" | "arrowleft" => Self::Left,
            "right" | "arrowright" => Self::Right,
            "up" | "arrowup" => Self::Up,
            "down" | "arrowdown" => Self::Down,
            _ => return None,
        })
    }
}

/// Клавиша-триггер хоткея.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Trigger {
    /// Функциональная клавиша `F1`–`F24`.
    Function(u8),
    /// Именованная клавиша.
    Named(NamedKey),
    /// Символ физической EN-раскладки (нижний регистр).
    Char(char),
}

impl Trigger {
    /// Совпадает ли клавиша с событием.
    #[must_use]
    pub fn matches(self, vk_code: u32, scan_code: u32) -> bool {
        match self {
            Self::Function(number) => vk_code == VK_F1 + u32::from(number) - 1,
            Self::Named(key) => vk_code == key.vk(),
            Self::Char(ch) => keys::scan_code_of(ch) == Some(scan_code),
        }
    }
}

/// Горячая клавиша: клавиша-триггер и обязательные модификаторы.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Hotkey {
    trigger: Trigger,
    mods: Mods,
}

impl Hotkey {
    /// Разобрать спецификацию вида `Ctrl+Shift+Q`, `F8`, `Space`.
    ///
    /// # Errors
    ///
    /// Пустая строка, только модификаторы, несколько клавиш или неизвестная
    /// клавиша — [`HotkeyError`].
    pub fn parse(spec: &str) -> Result<Self, HotkeyError> {
        let tokens = split_tokens(spec);
        if tokens.is_empty() {
            return Err(HotkeyError::Empty);
        }

        let mut mods = Mods::default();
        let mut trigger = None;
        for token in tokens {
            match modifier_of(&token) {
                Some(Modifier::Ctrl) => mods.ctrl = true,
                Some(Modifier::Shift) => mods.shift = true,
                Some(Modifier::Alt) => mods.alt = true,
                Some(Modifier::Win) => mods.win = true,
                None => {
                    let parsed = parse_trigger(&token)
                        .ok_or_else(|| HotkeyError::UnknownKey(token.clone()))?;
                    if trigger.replace(parsed).is_some() {
                        return Err(HotkeyError::MultipleTriggers(token));
                    }
                }
            }
        }

        trigger
            .map(|trigger| Self { trigger, mods })
            .ok_or(HotkeyError::ModifiersOnly)
    }

    /// Совпадает ли событие клавиши с хоткеем (модификаторы — строго).
    #[must_use]
    pub fn matches(self, vk_code: u32, scan_code: u32, mods: Mods) -> bool {
        self.mods == mods && self.trigger.matches(vk_code, scan_code)
    }

    /// Клавиша-триггер.
    #[must_use]
    pub const fn trigger(self) -> Trigger {
        self.trigger
    }

    /// Обязательные модификаторы.
    #[must_use]
    pub const fn mods(self) -> Mods {
        self.mods
    }

    /// Хоткей без модификаторов.
    #[must_use]
    pub const fn is_bare(self) -> bool {
        self.mods.is_empty()
    }

    /// Хоткей перехватывает обычный набор: без модификаторов и не `F`-клавиша.
    ///
    /// Такой хоткей не даст печатать соответствующий символ — в настройках
    /// показываем предупреждение.
    #[must_use]
    pub const fn is_risky(self) -> bool {
        if !self.is_bare() {
            return false;
        }
        match self.trigger {
            Trigger::Function(_)
            | Trigger::Named(
                NamedKey::Insert
                | NamedKey::Delete
                | NamedKey::Home
                | NamedKey::End
                | NamedKey::PageUp
                | NamedKey::PageDown
                | NamedKey::Left
                | NamedKey::Right
                | NamedKey::Up
                | NamedKey::Down,
            ) => false,
            Trigger::Named(_) | Trigger::Char(_) => true,
        }
    }

    /// Каноническая запись для конфига и интерфейса.
    #[must_use]
    pub fn to_spec(self) -> String {
        self.to_string()
    }
}

impl fmt::Display for Hotkey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (pressed, name) in [
            (self.mods.ctrl, "Ctrl"),
            (self.mods.shift, "Shift"),
            (self.mods.alt, "Alt"),
            (self.mods.win, "Win"),
        ] {
            if pressed {
                formatter.write_str(name)?;
                formatter.write_str("+")?;
            }
        }
        match self.trigger {
            Trigger::Function(number) => write!(formatter, "F{number}"),
            Trigger::Named(key) => formatter.write_str(key.name()),
            Trigger::Char(ch) => write!(formatter, "{}", ch.to_ascii_uppercase()),
        }
    }
}

/// Ошибка разбора спецификации хоткея.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum HotkeyError {
    /// Пустая строка.
    #[error("пустое сочетание")]
    Empty,
    /// Указаны только модификаторы, клавиши нет.
    #[error("нужна клавиша, а не только модификаторы")]
    ModifiersOnly,
    /// В сочетании больше одной клавиши.
    #[error("в сочетании несколько клавиш: «{0}»")]
    MultipleTriggers(String),
    /// Неизвестная клавиша.
    #[error("неизвестная клавиша: «{0}»")]
    UnknownKey(String),
}

/// Разбить спецификацию на токены.
///
/// Разделители — `+`, а также `-`/`_`, если токен целиком не является
/// известной клавишей: `Ctrl-Q` — сочетание, `-` — клавиша «минус».
fn split_tokens(spec: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    for part in spec.split('+') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let lower = part.to_lowercase();
        let is_known = modifier_of(&lower).is_some() || parse_trigger(&lower).is_some();
        if !is_known && lower.len() > 1 && (lower.contains('-') || lower.contains('_')) {
            for sub in lower.split(['-', '_']) {
                if !sub.is_empty() {
                    tokens.push(sub.to_string());
                }
            }
        } else {
            tokens.push(lower);
        }
    }
    tokens
}

/// Распознать модификатор.
fn modifier_of(token: &str) -> Option<Modifier> {
    Some(match token {
        "ctrl" | "control" | "ctl" => Modifier::Ctrl,
        "shift" => Modifier::Shift,
        "alt" | "menu" => Modifier::Alt,
        "win" | "windows" | "cmd" | "super" | "meta" => Modifier::Win,
        _ => return None,
    })
}

/// Распознать клавишу-триггер.
fn parse_trigger(token: &str) -> Option<Trigger> {
    if let Some(rest) = token.strip_prefix('f') {
        if let Ok(number) = rest.parse::<u8>() {
            if (1..=MAX_FUNCTION_KEY).contains(&number) {
                return Some(Trigger::Function(number));
            }
        }
    }
    if let Some(key) = NamedKey::from_name(token) {
        return Some(Trigger::Named(key));
    }
    let mut chars = token.chars();
    match (chars.next(), chars.next()) {
        (Some(ch), None) => keys::to_qwerty(ch).map(Trigger::Char),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(spec: &str) -> Hotkey {
        Hotkey::parse(spec).expect("спецификация должна разбираться")
    }

    #[test]
    fn test_function_key() {
        let f8 = parse("F8");
        assert!(f8.matches(0x77, 0x42, Mods::none()));
        assert!(!f8.matches(0x78, 0x43, Mods::none()));
        assert_eq!(f8.to_spec(), "F8");
        assert_eq!(parse("f8").to_spec(), "F8");
    }

    #[test]
    fn test_modifiers_are_strict() {
        let plain = parse("F8");
        let ctrl = Mods {
            ctrl: true,
            ..Mods::none()
        };
        // Лишний модификатор отменяет совпадение: чужие сочетания не отбираем.
        assert!(plain.matches(0x77, 0x42, Mods::none()));
        assert!(!plain.matches(0x77, 0x42, ctrl));

        let combo = parse("Ctrl+Q");
        assert!(combo.matches(0x51, 0x10, ctrl));
        assert!(!combo.matches(0x51, 0x10, Mods::none()));
        assert_eq!(combo.to_spec(), "Ctrl+Q");
    }

    #[test]
    fn test_layout_independent_letters() {
        // «Ф» — та же физическая клавиша, что EN «A» (скан-код 0x1E).
        let cyrillic = parse("Ф");
        assert_eq!(cyrillic, parse("A"));
        assert!(cyrillic.matches(0x41, 0x1E, Mods::none()));
        assert!(!cyrillic.matches(
            0x41,
            0x1E,
            Mods {
                shift: true,
                ..Mods::none()
            }
        ));
    }

    #[test]
    fn test_named_keys() {
        assert!(parse("Space").matches(0x20, 0x39, Mods::none()));
        assert!(parse("Esc").matches(0x1B, 0x01, Mods::none()));
        assert!(parse("PageUp").matches(0x21, 0x49, Mods::none()));
        assert!(parse("page-up").matches(0x21, 0x49, Mods::none()));
        assert!(parse("Delete").matches(0x2E, 0x53, Mods::none()));
        assert_eq!(parse("Escape").to_spec(), "Esc");
    }

    #[test]
    fn test_minus_key() {
        // «-» — это клавиша, а не пустой разделитель.
        let minus = parse("-");
        assert_eq!(minus.trigger(), Trigger::Char('-'));
        assert!(minus.matches(0xBD, 0x0C, Mods::none()));

        let ctrl_minus = parse("Ctrl+-");
        assert!(ctrl_minus.matches(
            0xBD,
            0x0C,
            Mods {
                ctrl: true,
                ..Mods::none()
            }
        ));
    }

    #[test]
    fn test_hyphen_separator() {
        assert_eq!(parse("Ctrl-Q"), parse("Ctrl+Q"));
        assert_eq!(parse("control-q").to_spec(), "Ctrl+Q");
    }

    #[test]
    fn test_order_and_display() {
        assert_eq!(parse("shift+ctrl+q").to_spec(), "Ctrl+Shift+Q");
        assert_eq!(parse("Q+Shift+Ctrl"), parse("Ctrl+Shift+Q"));
    }

    #[test]
    fn test_cyrillic_uppercase_normalized() {
        assert_eq!(parse("Ы"), parse("S"));
    }

    #[test]
    fn test_invalid_specs() {
        assert_eq!(Hotkey::parse(""), Err(HotkeyError::Empty));
        assert_eq!(Hotkey::parse("   "), Err(HotkeyError::Empty));
        assert_eq!(Hotkey::parse("ctrl+shift"), Err(HotkeyError::ModifiersOnly));
        assert!(matches!(
            Hotkey::parse("F8+Q"),
            Err(HotkeyError::MultipleTriggers(_))
        ));
        assert!(matches!(
            Hotkey::parse("Ctrl+Foo"),
            Err(HotkeyError::UnknownKey(_))
        ));
        assert!(matches!(
            Hotkey::parse("F25"),
            Err(HotkeyError::UnknownKey(_))
        ));
    }

    #[test]
    fn test_risky_bare_keys() {
        assert!(parse("Q").is_risky());
        assert!(parse("Space").is_risky());
        assert!(!parse("Ctrl+Q").is_risky());
        assert!(!parse("F8").is_risky());
        assert!(!parse("PageUp").is_risky());
    }
}
