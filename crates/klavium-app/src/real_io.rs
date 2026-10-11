//! Реальная реализация [`EngineIo`] поверх klavium-platform.

use std::time::Duration;

use klavium_core::Layout;
use klavium_platform::{hook, inject, layout, window};

use crate::engine::{EngineIo, Modifiers, StoredTarget};

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
        let delay = if text.chars().count() > 100 {
            Duration::from_millis(8)
        } else {
            Duration::from_millis(20)
        };
        let _ = inject::send_unicode_text(text, delay);
    }

    fn send_voice_text(&self, text: &str) {
        // Темп ввода: 10 мс/символ для коротких фраз, 5 мс — для длинных.
        let delay = if text.chars().count() > 60 {
            Duration::from_millis(5)
        } else {
            Duration::from_millis(10)
        };
        let _ = inject::send_unicode_text(text, delay);
    }

    fn modifiers(&self) -> Modifiers {
        current_modifiers()
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
