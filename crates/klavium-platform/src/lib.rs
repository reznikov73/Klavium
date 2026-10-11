//! klavium-platform — обёртки над Win32 API.
//!
//! Единственный крейт со «живым» `unsafe`: клавиатурный хук, инъекция ввода,
//! окна/фокус/каретка, раскладка и single instance. Вся остальная логика —
//! в `klavium-core` (чистой) и `klavium-app` (сборка).
//!
//! Модули:
//! - `hook` — глобальный клавиатурный хук (`WH_KEYBOARD_LL`) с подавлением.
//! - `inject` — инъекция ввода через `SendInput` (раскладко-независимый Unicode).
//! - `window` — активное окно, фокус, каретка.
//! - `layout` — текущая раскладка и каскад переключения.
//! - `instance` — single instance (именованный мьютекс).
//! - `autostart` — автозапуск (реестр; реализация — фаза Ф6).

pub mod autostart;
pub mod beep;
pub mod hook;
pub mod inject;
pub mod instance;
pub mod layout;
pub mod overlay;
pub mod vosk;
pub mod window;
