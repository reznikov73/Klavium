//! Модель конфигурации приложения.
//!
//! Структуры сериализуются в JSON; отсутствующие поля/секции заполняются
//! значениями по умолчанию.
//! Файловые операции (путь к `config.json` в каталоге данных) — в слое приложения.

use serde::{Deserialize, Serialize};

/// Один звуковой сигнал (частота и длительность).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SoundSpec {
    /// Частота, Гц.
    pub frequency: u32,
    /// Длительность, мс.
    pub duration_ms: u32,
}

impl Default for SoundSpec {
    fn default() -> Self {
        Self {
            frequency: 1000,
            duration_ms: 100,
        }
    }
}

/// Звуки переключения/статусов.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SoundsConfig {
    /// Слово на английской раскладке.
    pub en: SoundSpec,
    /// Слово на русской раскладке.
    pub ru: SoundSpec,
    /// Смена раскладки.
    pub switch: SoundSpec,
    /// Старт голосового ввода.
    pub voice_start: SoundSpec,
    /// Остановка голосового ввода.
    pub voice_stop: SoundSpec,
}

impl Default for SoundsConfig {
    fn default() -> Self {
        Self {
            en: SoundSpec {
                frequency: 1000,
                duration_ms: 100,
            },
            ru: SoundSpec {
                frequency: 500,
                duration_ms: 100,
            },
            switch: SoundSpec {
                frequency: 1200,
                duration_ms: 50,
            },
            voice_start: SoundSpec {
                frequency: 1500,
                duration_ms: 80,
            },
            voice_stop: SoundSpec {
                frequency: 900,
                duration_ms: 120,
            },
        }
    }
}

/// Таймауты.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TimeoutsConfig {
    /// Пауза между словами одного набора (сек).
    pub word_start: f64,
    /// Интервал проверки раскладки (сек).
    pub layout_check: f64,
}

impl Default for TimeoutsConfig {
    fn default() -> Self {
        Self {
            word_start: 5.0,
            layout_check: 0.5,
        }
    }
}

/// Уведомления.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct NotificationsConfig {
    /// Показывать уведомления.
    pub enabled: bool,
    /// Длительность показа (мс).
    pub duration_ms: u32,
}

impl Default for NotificationsConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            duration_ms: 2000,
        }
    }
}

/// Автозапуск.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct AutostartConfig {
    /// Запускать вместе с Windows.
    pub enabled: bool,
}

/// Статистика.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct StatisticsConfig {
    /// Вести статистику.
    pub enabled: bool,
    /// Сбрасывать при старте.
    pub reset_on_start: bool,
}

impl Default for StatisticsConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            reset_on_start: false,
        }
    }
}

/// Коррекция раскладки.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CorrectionConfig {
    /// Коррекция включена.
    pub enabled: bool,
    /// Порог уверенности 0.0–1.0.
    pub threshold: f64,
    /// Автоматически исправлять без подтверждения.
    pub auto_correct: bool,
    /// Хоткей исправления.
    pub hotkey_correct: String,
    /// Хоткей показа подсказки.
    pub hotkey_show: String,
    /// Показывать подсказки.
    pub show_notifications: bool,
}

impl Default for CorrectionConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            threshold: 0.7,
            auto_correct: false,
            hotkey_correct: "F8".to_string(),
            hotkey_show: "F8".to_string(),
            show_notifications: true,
        }
    }
}

/// Голосовой ввод (реализация — фаза Ф8).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct VoiceConfig {
    /// Голосовой ввод включён.
    pub enabled: bool,
    /// Движок распознавания.
    pub engine: String,
    /// Файл модели: имя в папке `models` рядом с данными приложения или
    /// путь (пусто — автопоиск `ggml-small.bin`, `ggml-large-v3-turbo.bin` и др.).
    pub model: String,
    /// Язык (`auto`/`ru`/`en`).
    pub language: String,
    /// Хоткей.
    pub hotkey: String,
    /// Голосовая пунктуация.
    pub punct_dict: bool,
    /// Автозаглавные буквы.
    pub auto_capitalize: bool,
    /// Путь к модели Vosk.
    pub vosk_model_path: String,
}

impl Default for VoiceConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            engine: "whisper".to_string(),
            model: String::new(),
            language: "auto".to_string(),
            hotkey: "F9".to_string(),
            punct_dict: true,
            auto_capitalize: true,
            vosk_model_path: String::new(),
        }
    }
}

/// Логирование.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct LoggingConfig {
    /// Логирование включено.
    pub enabled: bool,
    /// Уровень (DEBUG/INFO/...).
    pub level: String,
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            level: "DEBUG".to_string(),
        }
    }
}

/// Полная конфигурация Klavium.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    /// Звуки.
    pub sounds: SoundsConfig,
    /// Таймауты.
    pub timeouts: TimeoutsConfig,
    /// Уведомления.
    pub notifications: NotificationsConfig,
    /// Автозапуск.
    pub autostart: AutostartConfig,
    /// Статистика.
    pub statistics: StatisticsConfig,
    /// Коррекция.
    pub correction: CorrectionConfig,
    /// Голос.
    pub voice: VoiceConfig,
    /// Логирование.
    pub logging: LoggingConfig,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_defaults() {
        let config = AppConfig::default();
        assert_eq!(config.correction.threshold, 0.7);
        assert_eq!(config.correction.hotkey_correct, "F8");
        assert_eq!(config.sounds.ru.frequency, 500);
        assert!(!config.autostart.enabled);
    }

    #[test]
    fn test_config_partial_json_merge() {
        // Частичный JSON: незаданные поля заполняются умолчаниями
        let json = r#"{"correction":{"threshold":0.85}}"#;
        let config: AppConfig = serde_json::from_str(json).expect("частичный JSON");
        assert_eq!(config.correction.threshold, 0.85);
        assert!(config.correction.enabled);
        assert_eq!(config.correction.hotkey_correct, "F8");
        assert_eq!(config.sounds.switch.duration_ms, 50);
    }

    #[test]
    fn test_config_roundtrip() {
        let config = AppConfig::default();
        let text = serde_json::to_string_pretty(&config).expect("сериализация");
        let restored: AppConfig = serde_json::from_str(&text).expect("десериализация");
        assert_eq!(config, restored);
    }
}
