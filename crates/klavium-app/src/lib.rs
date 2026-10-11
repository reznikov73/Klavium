//! klavium-app — библиотека приложения Klavium.
//!
//! Содержит конвейер коррекции ([`engine`]), его привязку к Win32
//! ([`real_io`]) и разбор хоткеев ([`hotkeys`]). Бинарник `klavium`
//! и демо-пример `hook_demo` используют эти модули.

pub mod config_io;
pub mod engine;
pub mod hotkeys;
pub mod logging;
pub mod models;
pub mod real_io;
pub mod runtime;
pub mod services;
pub mod sound;
pub mod stats;
pub mod voice;
