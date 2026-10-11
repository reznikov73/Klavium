//! Данные статистики.
//!
//! Храним: счётчики слов по раскладкам, число переключений раскладки,
//! время последнего сброса и последние сессии (до 100).

use serde::{Deserialize, Serialize};

/// Счётчики слов.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct WordCounts {
    /// Слова на английской раскладке.
    pub en: u64,
    /// Слова на русской раскладке.
    pub ru: u64,
    /// Прочие слова (включая ошибки раскладки).
    pub other: u64,
    /// Слова, введённые голосом.
    pub voice: u64,
}

/// Сессия работы приложения.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Session {
    /// Начало сессии (unix-время, сек).
    pub started_at: u64,
    /// Длительность, сек.
    pub duration_s: u64,
    /// Слова за сессию.
    pub words: WordCounts,
    /// Переключения раскладки за сессию.
    pub switches: u64,
}

/// Хранимые данные статистики.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct StatsData {
    /// Итоговые счётчики слов.
    pub total_words: WordCounts,
    /// Всего переключений раскладки.
    pub layout_switches: u64,
    /// Время последнего сброса (unix-время, сек; 0 — не сбрасывалось).
    pub last_reset: u64,
    /// Последние сессии (до 100).
    pub sessions: Vec<Session>,
}

/// Максимум хранимых сессий.
pub const MAX_SESSIONS: usize = 100;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_partial_json_defaults() {
        let json = r#"{"total_words":{"ru":42}}"#;
        let data: StatsData = serde_json::from_str(json).expect("частичный JSON");
        assert_eq!(data.total_words.ru, 42);
        assert_eq!(data.total_words.en, 0);
        assert_eq!(data.sessions, Vec::<Session>::new());
    }

    #[test]
    fn test_roundtrip() {
        let data = StatsData {
            total_words: WordCounts {
                en: 1,
                ru: 2,
                other: 3,
                voice: 4,
            },
            layout_switches: 5,
            last_reset: 6,
            sessions: vec![Session::default()],
        };
        let text = serde_json::to_string(&data).unwrap();
        let restored: StatsData = serde_json::from_str(&text).unwrap();
        assert_eq!(data, restored);
    }
}
