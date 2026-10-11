//! klavium-core — чистая логика без зависимостей от Windows.
//!
//! Модули:
//! - `maps` — таблицы соответствия символов RU <-> EN (QWERTY <-> ЙЦУКЕН).
//! - `layout` — раскладка клавиатуры ([`Layout`]).
//! - `convert` — конвертация текста, набранного не в той раскладке.
//! - `dicts` — словари слов и запретных биграмм (встроенные данные).
//! - `detect` — детекция языка/ошибочной раскладки и «умная» коррекция.
//! - `keys` — скан-коды клавиш → символы (раскладка + Shift).
//! - `config` — модель конфигурации приложения (сериализуемая).
//! - `stats` — данные статистики (сериализуемые).
//!
//! # Архитектурный принцип
//!
//! Этот крейт **не зависит** от Win32, GUI или какой-либо платформы.
//! Он может и должен тестироваться полностью изолированно — это гарантирует,
//! что ядро программы корректно работает независимо от окружения.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod config;
pub mod convert;
pub mod detect;
pub mod dicts;
pub mod keys;
pub mod layout;
pub mod maps;
pub mod stats;
pub mod voice;

pub use layout::Layout;

#[cfg(test)]
mod tests {
    use super::*;

    /// Базовая проверка: ghbdtn -> привет
    #[test]
    fn test_convert_ghbdtn() {
        assert_eq!(convert::convert_text("ghbdtn"), "привет");
    }

    /// Регистр сохраняется: Ghbdtn -> Привет
    #[test]
    fn test_convert_uppercase() {
        assert_eq!(convert::convert_text("Ghbdtn"), "Привет");
    }

    /// Обратное направление: руддщ -> hello
    #[test]
    fn test_convert_reverse_hello() {
        assert_eq!(convert::convert_text_reverse("руддщ"), "hello");
    }

    /// Проверка: текст уже на правильной раскладке — не должен измениться
    #[test]
    fn test_convert_already_correct() {
        assert_eq!(convert::convert_text("привет"), "привет");
    }

    /// Проверка: пустая строка
    #[test]
    fn test_convert_empty() {
        assert_eq!(convert::convert_text(""), "");
    }

    /// Проверка: цифры и спецсимволы без отображения проходят без изменений
    #[test]
    fn test_convert_special_chars() {
        assert_eq!(convert::convert_text("123!%*()"), "123!%*()");
    }
}
