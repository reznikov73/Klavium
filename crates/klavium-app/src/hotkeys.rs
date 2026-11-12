//! Хоткеи приложения: разобранные сочетания для горячего пути хука.
//!
//! Строки конфига разбираются один раз — при загрузке и при сохранении
//! настроек, — а хук сравнивает готовые [`Hotkey`]: без разбора строк
//! и аллокаций на каждое нажатие клавиши (системный лимит
//! `LowLevelHooksTimeout` не прощает лишней работы в колбэке).

use klavium_core::config::AppConfig;
use klavium_core::hotkeys::Hotkey;

/// Разобранные хоткеи приложения.
#[derive(Debug, Clone, Copy, Default)]
pub struct CompiledHotkeys {
    /// Применить отложенное исправление раскладки.
    pub correction_apply: Option<Hotkey>,
    /// Отменить последнее исправление.
    pub correction_undo: Option<Hotkey>,
    /// Скрыть карточку «было → станет».
    pub card_hide: Option<Hotkey>,
    /// Включить/выключить голосовой ввод.
    pub voice_toggle: Option<Hotkey>,
    /// Полностью закрыть Klavium.
    pub quit: Option<Hotkey>,
    /// Голосовой ввод включён — хук проверяет это без чтения общего конфига.
    pub voice_enabled: bool,
}

impl CompiledHotkeys {
    /// Собрать хоткеи из конфигурации.
    ///
    /// Неразбираемые и пустые значения пропускаются: действие останется
    /// доступным из меню трея и из самого окна настроек.
    #[must_use]
    pub fn from_config(config: &AppConfig) -> Self {
        Self {
            correction_apply: parse_spec(&config.hotkeys.correction_apply),
            correction_undo: parse_spec(&config.hotkeys.correction_undo),
            card_hide: parse_spec(&config.hotkeys.card_hide),
            voice_toggle: parse_spec(&config.hotkeys.voice_toggle),
            quit: parse_spec(&config.hotkeys.quit),
            voice_enabled: config.voice.enabled,
        }
    }
}

/// Разобрать спецификацию; пустая строка — «не задано».
fn parse_spec(spec: &str) -> Option<Hotkey> {
    if spec.trim().is_empty() {
        None
    } else {
        Hotkey::parse(spec).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use klavium_core::hotkeys::Mods;

    #[test]
    fn test_defaults_compile() {
        let compiled = CompiledHotkeys::from_config(&AppConfig::default());
        let f8 = compiled.correction_apply.expect("F8");
        assert!(f8.matches(0x77, 0x42, Mods::none()));
        let esc = compiled.card_hide.expect("Esc");
        assert!(esc.matches(0x1B, 0x01, Mods::none()));
        let f9 = compiled.voice_toggle.expect("F9");
        assert!(f9.matches(0x78, 0x43, Mods::none()));
        let quit = compiled.quit.expect("Ctrl+Shift+Q");
        assert!(quit.matches(
            0x51,
            0x10,
            Mods {
                ctrl: true,
                shift: true,
                ..Mods::none()
            }
        ));
    }

    #[test]
    fn test_empty_and_invalid_specs_are_skipped() {
        let mut config = AppConfig::default();
        config.hotkeys.correction_apply.clear();
        config.hotkeys.voice_toggle = "не клавиша".to_string();
        let compiled = CompiledHotkeys::from_config(&config);
        assert!(compiled.correction_apply.is_none());
        assert!(compiled.voice_toggle.is_none());
        assert!(compiled.card_hide.is_some());
    }
}
