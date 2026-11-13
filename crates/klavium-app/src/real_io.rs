//! Реальная реализация [`EngineIo`] поверх klavium-platform.

use std::sync::atomic::{AtomicU8, Ordering};
use std::time::Duration;

use klavium_core::config::AppConfig;
use klavium_core::Layout;
use klavium_platform::{hook, inject, layout, window};

use crate::engine::{EngineIo, Modifiers, StoredTarget};

/// Прессеты темпа печати.
const SPEED_CAREFUL: u8 = 0;
const SPEED_FAST: u8 = 1;
const SPEED_INSTANT: u8 = 2;

/// Темп печати исправлений (меняется из настроек).
static SPEED_CORRECTION: AtomicU8 = AtomicU8::new(SPEED_CAREFUL);
/// Темп голосовой вставки.
static SPEED_VOICE: AtomicU8 = AtomicU8::new(SPEED_CAREFUL);

/// Применить пресеты темпа из конфига (старт приложения и сохранение настроек).
pub fn apply_typing_speed(config: &AppConfig) {
    SPEED_CORRECTION.store(
        parse_speed(&config.typing.speed_correction),
        Ordering::Relaxed,
    );
    SPEED_VOICE.store(parse_speed(&config.typing.speed_voice), Ordering::Relaxed);
}

fn parse_speed(value: &str) -> u8 {
    match value {
        "fast" => SPEED_FAST,
        "instant" => SPEED_INSTANT,
        _ => SPEED_CAREFUL,
    }
}

/// Задержка на символ для печати исправлений: (короткий, длинный текст).
///
/// `careful` — прежнее поведение (20/8 мс), `fast` — вдвое быстрее,
/// `instant` — почти без паузы (может терять символы в терминалах и RDP).
const fn correction_delay(speed: u8, long: bool) -> u64 {
    match (speed, long) {
        (SPEED_FAST, false) => 8,
        (SPEED_FAST, true) => 4,
        (SPEED_INSTANT, false) => 2,
        (SPEED_INSTANT, true) => 1,
        (_, false) => 20,
        (_, true) => 8,
    }
}

/// Задержка на символ для голосовой вставки: (короткая, длинная фраза).
///
/// `careful` — прежнее поведение (10/5 мс).
const fn voice_delay(speed: u8, long: bool) -> u64 {
    match (speed, long) {
        (SPEED_FAST, false) => 5,
        (SPEED_FAST, true) => 3,
        (SPEED_INSTANT, false) => 2,
        (SPEED_INSTANT, true) => 1,
        (_, false) => 10,
        (_, true) => 5,
    }
}

/// Обёртка над Win32-примитивами платформы.
pub struct PlatformIo;

impl EngineIo for PlatformIo {
    fn capture_target(&self) -> StoredTarget {
        let target = window::capture_input_target();
        StoredTarget {
            window: target.window as usize,
            focus: target.focus as usize,
        }
    }

    fn restore_target(&self, target: &StoredTarget) -> bool {
        let input = window::InputTarget {
            window: target.window as *mut core::ffi::c_void,
            focus: target.focus as *mut core::ffi::c_void,
        };
        window::restore_input_target(&input)
    }

    fn caret_pos(&self) -> Option<(i32, i32, i32)> {
        window::caret_screen_pos()
    }

    fn current_layout(&self) -> Option<Layout> {
        layout::current()
    }

    fn switch_layout(&self, target: Layout) -> bool {
        layout::switch_to(target)
    }

    fn send_backspaces(&self, count: u32) {
        let _ = inject::send_backspaces(count, Duration::from_millis(10));
    }

    fn send_text(&self, text: &str) {
        let speed = SPEED_CORRECTION.load(Ordering::Relaxed);
        let delay = Duration::from_millis(correction_delay(speed, text.chars().count() > 100));
        let _ = inject::send_unicode_text(text, delay);
    }

    fn send_voice_text(&self, text: &str) {
        let speed = SPEED_VOICE.load(Ordering::Relaxed);
        let delay = Duration::from_millis(voice_delay(speed, text.chars().count() > 60));
        let _ = inject::send_unicode_text(text, delay);
    }

    fn paste_text(&self, text: &str) -> bool {
        inject::paste_text(text).is_ok()
    }

    fn copy_selection(&self) -> Option<String> {
        inject::copy_selection()
    }

    fn modifiers(&self) -> Modifiers {
        current_modifiers()
    }

    fn foreground_exe(&self) -> Option<String> {
        klavium_platform::window::foreground_process_name()
    }
}

/// Текущее состояние модификаторов (физических клавиш) — для хоткеев и движка.
#[must_use]
pub fn current_modifiers() -> Modifiers {
    Modifiers {
        shift: is_any_pressed(&[hook::vk::SHIFT, hook::vk::LSHIFT, hook::vk::RSHIFT]),
        ctrl: is_any_pressed(&[hook::vk::CONTROL, hook::vk::LCONTROL, hook::vk::RCONTROL]),
        alt: is_any_pressed(&[hook::vk::MENU, hook::vk::LMENU, hook::vk::RMENU]),
        win: is_any_pressed(&[hook::vk::LWIN, hook::vk::RWIN]),
    }
}

fn is_any_pressed(vk_codes: &[u32]) -> bool {
    vk_codes.iter().any(|vk| hook::is_key_pressed(*vk))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_careful_matches_previous_behaviour() {
        // Прежние задержки: исправления 20/8 мс, голос 10/5 мс.
        assert_eq!(correction_delay(SPEED_CAREFUL, false), 20);
        assert_eq!(correction_delay(SPEED_CAREFUL, true), 8);
        assert_eq!(voice_delay(SPEED_CAREFUL, false), 10);
        assert_eq!(voice_delay(SPEED_CAREFUL, true), 5);
    }

    #[test]
    fn test_speed_presets_are_faster() {
        assert!(correction_delay(SPEED_FAST, false) < correction_delay(SPEED_CAREFUL, false));
        assert!(correction_delay(SPEED_INSTANT, false) < correction_delay(SPEED_FAST, false));
        assert!(voice_delay(SPEED_INSTANT, true) < voice_delay(SPEED_CAREFUL, true));
    }

    #[test]
    fn test_parse_speed_falls_back_to_careful() {
        assert_eq!(parse_speed("fast"), SPEED_FAST);
        assert_eq!(parse_speed("instant"), SPEED_INSTANT);
        assert_eq!(parse_speed("unknown"), SPEED_CAREFUL);
        assert_eq!(parse_speed(""), SPEED_CAREFUL);
    }
}
