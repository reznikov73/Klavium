//! Словари слов и «запретных» биграмм.
//!
//! Данные словарей и биграмм (каталог `data/`) встроены в бинарник на
//! этапе компиляции (`include_str!`). Словари
//! отсортированы, поиск — бинарный: это даёт
//! компактную память и предсказуемую производительность.
//!
//! Русский словарь дополняется ё→е-нормализованными формами
//! («ёжик» == «ежик»).
//!
//! Из биграмм-таблиц используются только «запретные» пары (значение < 0) —
//! именно они влияют на результат.

use std::collections::HashMap;
use std::sync::OnceLock;

const ENGLISH_WORDS_TXT: &str = include_str!("../data/english_words.txt");
const RUSSIAN_WORDS_TXT: &str = include_str!("../data/russian_words.txt");
const RU_BIGRAMS_JSON: &str = include_str!("../data/ru_bigrams.json");
const EN_BIGRAMS_JSON: &str = include_str!("../data/en_bigrams.json");

/// Словари, загруженные из встроенных данных (создаются один раз за процесс).
pub struct Dictionaries {
    english_words: Vec<&'static str>,
    russian_words: Vec<&'static str>,
    ru_bad_bigrams: Vec<(char, char)>,
    en_bad_bigrams: Vec<(char, char)>,
}

static INSTANCE: OnceLock<Dictionaries> = OnceLock::new();

impl Dictionaries {
    /// Получить общий экземпляр словарей (строится лениво, один раз за процесс).
    #[must_use]
    pub fn get() -> &'static Self {
        INSTANCE.get_or_init(build)
    }

    /// Есть ли слово в английском словаре.
    #[must_use]
    pub fn contains_english(&self, word: &str) -> bool {
        self.english_words.binary_search(&word).is_ok()
    }

    /// Есть ли слово в русском словаре (с учётом ё→е-нормализации).
    #[must_use]
    pub fn contains_russian(&self, word: &str) -> bool {
        self.russian_words.binary_search(&word).is_ok()
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
        self.english_words.len()
    }

    /// Количество слов в русском словаре (для тестов и диагностики).
    #[must_use]
    pub const fn russian_word_count(&self) -> usize {
        self.russian_words.len()
    }
}

fn build() -> Dictionaries {
    Dictionaries {
        english_words: sorted_words(ENGLISH_WORDS_TXT.lines()),
        russian_words: build_russian_words(),
        ru_bad_bigrams: bad_bigrams(RU_BIGRAMS_JSON),
        en_bad_bigrams: bad_bigrams(EN_BIGRAMS_JSON),
    }
}

/// Разобрать список слов: обрезать пробелы, привести
/// к нижнему регистру (строки, где это необходимо), убрать пустые,
/// отсортировать, дедуплицировать.
fn sorted_words(lines: impl Iterator<Item = &'static str>) -> Vec<&'static str> {
    let mut words: Vec<&'static str> = Vec::new();
    for line in lines {
        let word = line.trim();
        if word.is_empty() {
            continue;
        }
        if word.chars().any(char::is_uppercase) {
            words.push(Box::leak(word.to_lowercase().into_boxed_str()));
        } else {
            words.push(word);
        }
    }
    words.sort_unstable();
    words.dedup();
    words
}

/// Русский словарь + ё→е-нормализованные формы.
fn build_russian_words() -> Vec<&'static str> {
    let mut words = sorted_words(RUSSIAN_WORDS_TXT.lines());
    let mut normalized: Vec<&'static str> = Vec::new();
    for word in &words {
        if word.contains('ё') {
            let replaced = Box::leak(word.replace('ё', "е").into_boxed_str());
            normalized.push(replaced);
        }
    }
    words.extend(normalized);
    words.sort_unstable();
    words.dedup();
    words
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
        for word in dicts.english_words.iter().chain(dicts.russian_words.iter()) {
            assert!(
                !word.chars().any(char::is_uppercase),
                "слово не в нижнем регистре: {word:?}"
            );
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
    fn test_bad_bigrams() {
        let dicts = Dictionaries::get();
        assert!(dicts.is_bad_ru_bigram(('ж', 'щ')));
        assert!(dicts.is_bad_en_bigram(('q', 'q')));
        assert!(dicts.is_bad_en_bigram(('q', 'w')));
        assert!(!dicts.is_bad_en_bigram(('t', 'h')));
    }
}
