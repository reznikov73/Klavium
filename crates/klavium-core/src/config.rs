//! Модель конфигурации приложения.
//!
//! Структуры сериализуются в JSON; отсутствующие поля/секции заполняются
//! значениями по умолчанию.
//! Файловые операции (путь к `config.json` в каталоге данных) — в слое приложения.

use serde::{Deserialize, Deserializer, Serialize};

/// Один звуковой сигнал: частота, длительность и рисунок последовательности
/// (сколько сигналов и с какой паузой между ними).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SoundSpec {
    /// Звук включён (выключенный — не играет в работе).
    pub enabled: bool,
    /// Частота, Гц.
    pub frequency: u32,
    /// Длительность одного сигнала, мс.
    pub duration_ms: u32,
    /// Количество сигналов в последовательности.
    pub count: u32,
    /// Пауза между сигналами, мс (при одном сигнале не используется).
    pub pause_ms: u32,
}

impl Default for SoundSpec {
    fn default() -> Self {
        Self {
            enabled: true,
            frequency: 1000,
            duration_ms: 100,
            count: 1,
            pause_ms: 30,
        }
    }
}

/// Звуки переключения/статусов.
///
/// Десериализация — вручную: в конфигах прежних версий у звуков нет полей
/// «включён/количество/пауза», и у каждого звука они должны дополниться его
/// собственным умолчанием — например, у «русского слова» два сигнала, у
/// «смены раскладки» три (паритет с прежним поведением).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
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
                enabled: true,
                frequency: 1000,
                duration_ms: 100,
                count: 1,
                pause_ms: 30,
            },
            ru: SoundSpec {
                enabled: true,
                frequency: 500,
                duration_ms: 100,
                count: 2,
                pause_ms: 30,
            },
            switch: SoundSpec {
                enabled: true,
                frequency: 1200,
                duration_ms: 50,
                count: 3,
                pause_ms: 20,
            },
            voice_start: SoundSpec {
                enabled: true,
                frequency: 1500,
                duration_ms: 80,
                count: 1,
                pause_ms: 30,
            },
            voice_stop: SoundSpec {
                enabled: true,
                frequency: 900,
                duration_ms: 120,
                count: 1,
                pause_ms: 30,
            },
        }
    }
}

/// Сырые значения звуков из файла: любое поле может отсутствовать.
#[derive(Deserialize, Default)]
#[serde(default)]
struct RawSoundsConfig {
    en: RawSoundSpec,
    ru: RawSoundSpec,
    switch: RawSoundSpec,
    voice_start: RawSoundSpec,
    voice_stop: RawSoundSpec,
}

/// Сырые значения одного звука (все поля опциональны).
#[derive(Deserialize, Default)]
#[serde(default)]
struct RawSoundSpec {
    enabled: Option<bool>,
    frequency: Option<u32>,
    duration_ms: Option<u32>,
    count: Option<u32>,
    pause_ms: Option<u32>,
}

impl RawSoundSpec {
    /// Дополнить отсутствующие поля умолчанием конкретного звука.
    fn merged(self, base: &SoundSpec) -> SoundSpec {
        SoundSpec {
            enabled: self.enabled.unwrap_or(base.enabled),
            frequency: self.frequency.unwrap_or(base.frequency),
            duration_ms: self.duration_ms.unwrap_or(base.duration_ms),
            count: self.count.unwrap_or(base.count),
            pause_ms: self.pause_ms.unwrap_or(base.pause_ms),
        }
    }
}

impl<'de> Deserialize<'de> for SoundsConfig {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = RawSoundsConfig::deserialize(deserializer)?;
        let base = Self::default();
        Ok(Self {
            en: raw.en.merged(&base.en),
            ru: raw.ru.merged(&base.ru),
            switch: raw.switch.merged(&base.switch),
            voice_start: raw.voice_start.merged(&base.voice_start),
            voice_stop: raw.voice_stop.merged(&base.voice_stop),
        })
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
        assert_eq!(config.sounds.ru.count, 2);
        assert_eq!(config.sounds.ru.pause_ms, 30);
        assert_eq!(config.sounds.switch.count, 3);
        assert_eq!(config.sounds.switch.pause_ms, 20);
        assert!(config.sounds.en.enabled);
        assert!(!config.autostart.enabled);
    }

    #[test]
    fn test_config_old_sounds_gain_per_kind_defaults() {
        // Конфиг прежних версий: у звуков только частота и длительность.
        // Рисунок последовательности дополняется умолчанием конкретного звука.
        let json = concat!(
            "{\"sounds\":{",
            "\"ru\":{\"frequency\":500,\"duration_ms\":100},",
            "\"switch\":{\"frequency\":1200,\"duration_ms\":50}}}"
        );
        let config: AppConfig = serde_json::from_str(json).expect("старый JSON");
        assert_eq!(config.sounds.ru.count, 2);
        assert_eq!(config.sounds.ru.pause_ms, 30);
        assert!(config.sounds.ru.enabled);
        assert_eq!(config.sounds.switch.count, 3);
        assert_eq!(config.sounds.switch.pause_ms, 20);
        assert_eq!(config.sounds.en.count, 1);
        assert_eq!(config.sounds.en.frequency, 1000);
    }

    #[test]
    fn test_config_sound_partial_override() {
        let json = r#"{"sounds":{"en":{"enabled":false,"count":4,"pause_ms":15}}}"#;
        let config: AppConfig = serde_json::from_str(json).expect("частичный звук");
        assert!(!config.sounds.en.enabled);
        assert_eq!(config.sounds.en.count, 4);
        assert_eq!(config.sounds.en.pause_ms, 15);
        assert_eq!(config.sounds.en.frequency, 1000); // из умолчания
        assert_eq!(config.sounds.en.duration_ms, 100);
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
