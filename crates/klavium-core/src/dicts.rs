//! Словари слов и «запретных» биграмм.
//!
//! Словари собираются на этапе компиляции в компактные FST (`build.rs`):
//! общие префиксы и суффиксы сжимаются, поиск идёт прямо по байтам в
//! бинарнике — без `Vec<&str>` с указателями (это экономило ~8,5 МБ кучи
//! на 532 тыс. слов и заметный кусок размера exe).
//!
//! Русский словарь дополняется ё→е-нормализованными формами
//! («ёжик» == «ежик») — это делает `build.rs`.
//!
//! Из биграмм-таблиц используются только «запретные» пары (значение < 0) —
//! именно они влияют на результат.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime};

use fst::Set;

const ENGLISH_WORDS_FST: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/english_words.fst"));
const RUSSIAN_WORDS_FST: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/russian_words.fst"));
const RU_BIGRAMS_JSON: &str = include_str!("../data/ru_bigrams.json");
const EN_BIGRAMS_JSON: &str = include_str!("../data/en_bigrams.json");

/// Счётчики слов, сгенерированные `build.rs`.
mod counts {
    include!(concat!(env!("OUT_DIR"), "/dict_counts.rs"));
}

/// Словари, загруженные из встроенных данных (создаются один раз за процесс).
pub struct Dictionaries {
    english_words: Set<&'static [u8]>,
    russian_words: Set<&'static [u8]>,
    ru_bad_bigrams: Vec<(char, char)>,
    en_bad_bigrams: Vec<(char, char)>,
}

static INSTANCE: OnceLock<Dictionaries> = OnceLock::new();

/// Путь к словарю пользователя (задаётся приложением после выбора каталога данных).
static USER_PATH: OnceLock<PathBuf> = OnceLock::new();
/// Кэш словаря пользователя: файл перечитывается не чаще раза в минуту.
static USER_CACHE: OnceLock<Mutex<Option<UserCache>>> = OnceLock::new();

/// Словарь пользователя: слова, которые не исправлять, и стоп-лист.
#[derive(Debug, Default)]
struct UserDict {
    /// Слова, которые считаются осмысленными (не исправляются).
    words: HashSet<String>,
    /// Слова, которые нельзя исправлять никогда (`!слово`).
    forbidden: HashSet<String>,
}

/// Снимок словаря пользователя вместе с меткой файла.
struct UserCache {
    /// Время изменения файла на момент загрузки.
    modified: Option<SystemTime>,
    /// Когда последний раз проверяли файл.
    checked_at: Instant,
    /// Разобранный словарь.
    dict: Arc<UserDict>,
}

/// Задать путь к словарю пользователя (вызывается один раз на старте).
pub fn set_user_dict_path(path: PathBuf) {
    let _ = USER_PATH.set(path);
}

/// Нормализовать слово для словаря (нижний регистр, без пробелов по краям).
fn normalize(word: &str) -> String {
    word.trim().to_lowercase()
}

/// Разобрать файл словаря: `#` — комментарий, `!слово` — стоп-лист.
fn load_user_dict(path: &Path) -> UserDict {
    let mut dict = UserDict::default();
    let Ok(text) = std::fs::read_to_string(path) else {
        return dict;
    };
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(rest) = line.strip_prefix('!') {
            let word = normalize(rest);
            if !word.is_empty() {
                dict.forbidden.insert(word);
            }
        } else {
            let word = normalize(line);
            if !word.is_empty() {
                dict.words.insert(word);
            }
        }
    }
    dict
}

/// Актуальный словарь пользователя (читывается при изменении файла).
fn user_dict_snapshot() -> Arc<UserDict> {
    let Some(path) = USER_PATH.get() else {
        return Arc::new(UserDict::default());
    };
    let cache = USER_CACHE.get_or_init(|| Mutex::new(None));
    let mut guard = cache
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let now = Instant::now();
    let modified = std::fs::metadata(path)
        .and_then(|meta| meta.modified())
        .ok();

    if let Some(cache) = guard.as_mut() {
        // Не чаще раза в минуту проверяем файл; внутри минуты — только кэш.
        if now.saturating_duration_since(cache.checked_at) < Duration::from_secs(60) {
            return Arc::clone(&cache.dict);
        }
        if modified == cache.modified {
            cache.checked_at = now;
            return Arc::clone(&cache.dict);
        }
    }

    let dict = Arc::new(load_user_dict(path));
    *guard = Some(UserCache {
        modified,
        checked_at: now,
        dict: Arc::clone(&dict),
    });
    dict
}

impl Dictionaries {
    /// Слово из пользовательского словаря — считается осмысленным, не исправляем.
    #[must_use]
    pub fn user_contains(&self, word: &str) -> bool {
        let dict = user_dict_snapshot();
        dict.words.contains(&normalize(word))
    }

    /// Стоп-слово — не исправлять никогда.
    #[must_use]
    pub fn is_forbidden(&self, word: &str) -> bool {
        let dict = user_dict_snapshot();
        dict.forbidden.contains(&normalize(word))
    }

    /// Сколько слов в словаре пользователя (для настроек и диагностики).
    #[must_use]
    pub fn user_word_count(&self) -> usize {
        user_dict_snapshot().words.len()
    }

    /// Получить общий экземпляр словарей (строится лениво, один раз за процесс).
    #[must_use]
    pub fn get() -> &'static Self {
        INSTANCE.get_or_init(build)
    }

    /// Есть ли слово в английском словаре.
    #[must_use]
    pub fn contains_english(&self, word: &str) -> bool {
        self.english_words.contains(word)
    }
    /// Есть ли слово в русском словаре (с учётом ё→е-нормализации).
    #[must_use]
    pub fn contains_russian(&self, word: &str) -> bool {
        self.russian_words.contains(word)
    }

    /// Является ли пара символов запретной русской биграммой.
    #[must_use]
    pub fn is_bad_ru_bigram(&self, pair: (char, char)) -> bool {
        self.ru_bad_bigrams.binary_search(&pair).is_ok()
    }

    /// Является ли пара символов запретной английской биграммой.
    #[must_use]
    pub fn is_bad_en_bigram(&self, pair: (char, char)) -> bool {
        self.en_bad_bigrams.binary_search(&pair).is_ok()
    }

    /// Количество слов в английском словаре (для тестов и диагностики).
    #[must_use]
    pub const fn english_word_count(&self) -> usize {
        counts::ENGLISH_WORD_COUNT
    }

    /// Количество слов в русском словаре (для тестов и диагностики).
    #[must_use]
    pub const fn russian_word_count(&self) -> usize {
        counts::RUSSIAN_WORD_COUNT
    }
}

fn build() -> Dictionaries {
    Dictionaries {
        english_words: Set::new(ENGLISH_WORDS_FST).expect("FST английского словаря повреждён"),
        russian_words: Set::new(RUSSIAN_WORDS_FST).expect("FST русского словаря повреждён"),
        ru_bad_bigrams: bad_bigrams(RU_BIGRAMS_JSON),
        en_bad_bigrams: bad_bigrams(EN_BIGRAMS_JSON),
    }
}

/// Извлечь «запретные» биграммы (значение < 0) из JSON-таблицы.
fn bad_bigrams(json: &str) -> Vec<(char, char)> {
    let map: HashMap<String, f64> =
        serde_json::from_str(json).expect("встроенный JSON биграмм должен быть валидным");
    let mut pairs: Vec<(char, char)> = map
        .into_iter()
        .filter(|(_, value)| *value < 0.0)
        .filter_map(|(key, _)| {
            let mut chars = key.chars();
            Some((chars.next()?, chars.next()?))
        })
        .collect();
    pairs.sort_unstable();
    pairs.dedup();
    pairs
}

#[cfg(test)]
mod tests {
    use super::*;
    use fst::Streamer;

    #[test]
    fn test_user_dict_parsing() {
        let dir = std::env::temp_dir().join("klavium-user-dict-test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("user_dict.txt");
        std::fs::write(&path, "# комментарий\nИван\n!тест\n\n  Пётр  \n!\n").unwrap();
        let dict = load_user_dict(&path);
        assert!(dict.words.contains("иван"), "слова: {:?}", dict.words);
        assert!(dict.words.contains("пётр"));
        assert!(dict.forbidden.contains("тест"));
        assert_eq!(dict.words.len(), 2);
        assert_eq!(dict.forbidden.len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_user_dict_missing_file_is_empty() {
        let dict = load_user_dict(Path::new("C:\\nonexistent\\klavium\\user_dict.txt"));
        assert!(dict.words.is_empty());
        assert!(dict.forbidden.is_empty());
    }

    #[test]
    fn test_dictionaries_non_empty() {
        let dicts = Dictionaries::get();
        assert!(
            dicts.english_word_count() > 300_000,
            "en: {}",
            dicts.english_word_count()
        );
        assert!(
            dicts.russian_word_count() > 150_000,
            "ru: {}",
            dicts.russian_word_count()
        );
    }

    #[test]
    fn test_words_are_lowercase() {
        let dicts = Dictionaries::get();
        for set in [&dicts.english_words, &dicts.russian_words] {
            let mut stream = set.stream();
            while let Some(key) = stream.next() {
                let word = std::str::from_utf8(key).expect("слово в UTF-8");
                assert!(
                    !word.chars().any(char::is_uppercase),
                    "слово не в нижнем регистре: {word:?}"
                );
            }
        }
    }

    #[test]
    fn test_contains_basics() {
        let dicts = Dictionaries::get();
        assert!(dicts.contains_english("hello"));
        assert!(dicts.contains_english("the"));
        assert!(!dicts.contains_english("привет"));
        assert!(dicts.contains_russian("привет"));
        assert!(dicts.contains_russian("бабушка"));
        assert!(!dicts.contains_russian("hello"));
    }

    #[test]
    fn test_yo_normalized_forms() {
        // У любого слова с «ё» должна находиться е-форма (её добавляет build.rs).
        let dicts = Dictionaries::get();
        let mut stream = dicts.russian_words.stream();
        while let Some(key) = stream.next() {
            let word = std::str::from_utf8(key).expect("слово в UTF-8");
            if word.contains('ё') {
                let normalized = word.replace('ё', "е");
                assert!(
                    dicts.contains_russian(&normalized),
                    "нет е-формы для {word:?}"
                );
                return;
            }
        }
    }

    #[test]
    fn test_bad_bigrams() {
        let dicts = Dictionaries::get();
        assert!(dicts.is_bad_ru_bigram(('ж', 'щ')));
        assert!(dicts.is_bad_en_bigram(('q', 'q')));
        assert!(dicts.is_bad_en_bigram(('q', 'w')));
        assert!(!dicts.is_bad_en_bigram(('t', 'h')));
    }
}
