//! Детекция ошибочной раскладки и «умная» коррекция слов.
//!
//! Определение языка и коррекция раскладки:
//! определение языка, проверка осмысленности (словарь → эвристики → окончания),
//! поиск целевой раскладки и решение о коррекции с оценкой уверенности.
//!
//! Особенности реализации (без изменения поведения):
//! - строковые раскладки заменены на перечисление [`Layout`];
//! - поле `suggestions` отсутствует (потребителей нет);
//! - параметр `strong` отсутствует: он не влияет на
//!   результат (обе итоговые ветки возвращают `false`).
//!
//! Счётчики символов переводятся в доли через `as f64` — потеря точности
//! практически невозможна (строки на порядки короче предела мантиссы f64).

#![allow(clippy::cast_precision_loss)]

use crate::convert;
use crate::dicts::Dictionaries;
use crate::layout::Layout;

/// EN-символы, которые в русской раскладке являются БУКВАМИ
/// (`ё х ъ ж э б ю` и их Shift-варианты) — 7 клавиш × 2 = 14 символов.
/// В буфере слова такие символы НЕ завершают слово (см. конвейер Ф4).
pub const AMBIGUOUS_EN_CHARS: &str = "`~[{]};:'\",<>.";

/// Английские «окончания-признаки» слов.
const EN_ENDINGS: &[&str] = &[
    "ing", "ed", "er", "ly", "tion", "s", "es", "able", "ible", "ment", "ness", "less", "ful",
    "ous", "ive", "al", "ty", "ity", "ate", "ize", "ise", "est", "ence", "ance", "ant", "ent",
    "ship", "hood", "dom", "wise", "fold", "most", "like", "ward", "wards",
];

/// Русские «окончания-признаки» слов.
const RU_ENDINGS: &[&str] = &[
    "ый",
    "ий",
    "ой",
    "ая",
    "ое",
    "ые",
    "ие",
    "ого",
    "его",
    "ей",
    "их",
    "ых",
    "ому",
    "ему",
    "им",
    "ым",
    "ую",
    "ими",
    "ыми",
    "ом",
    "ем",
    "ет",
    "ют",
    "ут",
    "ишь",
    "ете",
    "ат",
    "ят",
    "л",
    "ла",
    "ло",
    "ли",
    "ал",
    "ял",
    "ало",
    "яло",
    "буду",
    "будешь",
    "будет",
    "будем",
    "будете",
    "будут",
    "ся",
    "сь",
    "ость",
    "есть",
    "ник",
    "щик",
    "чик",
    "ство",
];

/// Результат анализа слова корректором.
#[derive(Debug, Clone, PartialEq)]
pub struct Correction {
    /// Нужно ли исправление.
    pub needs_correction: bool,
    /// Исходный текст.
    pub original: String,
    /// Исправленный текст (равен исходному, если коррекция не нужна).
    pub corrected: String,
    /// Уверенность 0.0–1.0.
    pub confidence: f64,
    /// Целевая раскладка, если она определена.
    pub target_layout: Option<Layout>,
}

impl Correction {
    /// Результат «коррекция не нужна» для данного текста.
    fn no_correction(text: &str) -> Self {
        Self {
            needs_correction: false,
            original: text.to_string(),
            corrected: text.to_string(),
            confidence: 0.0,
            target_layout: None,
        }
    }
}

/// Русская буква (диапазоны `[а-яА-ЯёЁ]`).
const fn is_cyrillic_letter(c: char) -> bool {
    matches!(c, 'а'..='я' | 'А'..='Я' | 'ё' | 'Ё')
}

/// Определить язык текста по доле русских/английских букв.
///
/// Возвращает `(Some(Layout::En) | Some(Layout::Ru) | None, уверенность)`.
/// `None` соответствует 'unknown'/'mixed'.
#[must_use]
pub fn detect_language(text: &str) -> (Option<Layout>, f64) {
    let mut ru_count = 0usize;
    let mut en_count = 0usize;
    for c in text.chars() {
        if is_cyrillic_letter(c) {
            ru_count += 1;
        } else if c.is_ascii_alphabetic() {
            en_count += 1;
        }
    }

    let total = ru_count + en_count;
    if total == 0 {
        return (None, 0.0);
    }

    let ru_confidence = ru_count as f64 / total as f64;
    let en_confidence = en_count as f64 / total as f64;

    if ru_confidence > en_confidence {
        (Some(Layout::Ru), ru_confidence)
    } else if en_confidence > ru_confidence {
        (Some(Layout::En), en_confidence)
    } else {
        (None, 0.5)
    }
}

/// Похоже ли слово на осмысленное английское (эвристика).
#[must_use]
pub fn is_meaningful_english(word: &str) -> bool {
    let word = word.to_lowercase();

    // 1. Гласные
    if !word
        .chars()
        .any(|c| matches!(c, 'a' | 'e' | 'i' | 'o' | 'u'))
    {
        return false;
    }

    // 2. Биграммы
    if !check_bigrams_en(&word) {
        return false;
    }

    // 3. Окончания
    if EN_ENDINGS.iter().any(|ending| word.ends_with(ending)) {
        return true;
    }

    // 4. Словарь
    Dictionaries::get().contains_english(&word)
}

/// Похоже ли слово на осмысленное русское.
///
/// Словарь проверяется первым (с ё/е-нормализацией), затем — базовые
/// эвристики (гласные, запретные биграммы) и окончания.
#[must_use]
pub fn is_meaningful_russian(word: &str) -> bool {
    let word = word.to_lowercase();
    let dicts = Dictionaries::get();

    // 1. Словарь (с учётом ё/е-нормализации: «ёжик» == «ежик»)
    if dicts.contains_russian(&word) {
        return true;
    }
    if word.contains('ё') && dicts.contains_russian(&word.replace('ё', "е")) {
        return true;
    }

    // 2. Гласные
    if !word
        .chars()
        .any(|c| matches!(c, 'а' | 'е' | 'ё' | 'и' | 'о' | 'у' | 'ы' | 'э' | 'ю' | 'я'))
    {
        return false;
    }

    // 3. Биграммы
    if !check_bigrams_ru(&word) {
        return false;
    }

    // 4. Окончания
    RU_ENDINGS.iter().any(|ending| word.ends_with(ending))
}

/// Проверить, что в русском слове нет запретных биграмм.
fn check_bigrams_ru(word: &str) -> bool {
    let dicts = Dictionaries::get();
    !word
        .chars()
        .zip(word.chars().skip(1))
        .any(|pair| dicts.is_bad_ru_bigram(pair))
}

/// Проверить, что в английском слове нет запретных биграмм.
fn check_bigrams_en(word: &str) -> bool {
    let dicts = Dictionaries::get();
    !word
        .chars()
        .zip(word.chars().skip(1))
        .any(|pair| dicts.is_bad_en_bigram(pair))
}

/// Определить, допущена ли ошибка раскладки; вернуть целевую раскладку.
///
/// Определение ошибки раскладки с явным порогом
/// уверенности (по умолчанию 0.7).
#[must_use]
pub fn detect_layout_error(
    text: &str,
    current_layout: Option<Layout>,
    confidence_threshold: f64,
) -> Option<Layout> {
    // Слишком короткий текст не анализируем
    if text.trim().chars().count() < 2 {
        return None;
    }

    let (detected_lang, confidence) = detect_language(text);

    // Если текущая раскладка неизвестна — берём её из детекции
    let current = current_layout.or(detected_lang);

    // СЛУЧАЙ 1: раскладка и язык не совпадают
    if let (Some(cur), Some(detected)) = (current, detected_lang) {
        if cur != detected {
            return Some(detected);
        }
    }

    // СЛУЧАЙ 2: совпадают, но текст может быть ошибкой («ghbdtn»)
    let cur = current?;
    if confidence >= confidence_threshold {
        let is_meaningful = match cur {
            Layout::Ru => is_meaningful_russian(text),
            Layout::En => is_meaningful_english(text),
        };
        if is_meaningful {
            return None;
        }

        let converted = match cur {
            Layout::En => convert::convert_text(text),
            Layout::Ru => convert::convert_text_reverse(text),
        };
        let (new_lang, new_conf) = detect_language(&converted);
        if new_lang == Some(cur.opposite()) && new_conf > 0.8 {
            return Some(cur.opposite());
        }
    }

    None
}

/// Умная коррекция текста с учётом контекста и осмысленности.
///
/// Умная коррекция: решение о необходимости замены и её результат.
/// `context` — последние набранные слова (учитываются до 3 последних);
/// `current_layout` — раскладка, под которой набран текст (`None` — 'unknown').
#[must_use]
pub fn smart_correction(
    text: &str,
    context: &[&str],
    current_layout: Option<Layout>,
) -> Correction {
    let mut result = Correction::no_correction(text);

    if text.chars().count() < 2 {
        return result;
    }

    // «Смысл раньше пунктуации»: токен с двусмысленными символами — отдельная ветка
    if text.chars().any(|c| AMBIGUOUS_EN_CHARS.contains(c)) {
        return ambiguous_token_correction(text, result);
    }

    let cleaned_original: String = text.chars().filter(|c| c.is_alphabetic()).collect();
    if cleaned_original.is_empty() {
        return result;
    }

    let is_original_ru = is_meaningful_russian(&cleaned_original);
    let is_original_en = is_meaningful_english(&cleaned_original);
    let (original_lang, _) = detect_language(&cleaned_original);

    // Оригинал уже осмысленный в своей раскладке — не исправляем
    if matches!(
        (original_lang, is_original_ru, is_original_en),
        (Some(Layout::Ru), true, _) | (Some(Layout::En), _, true)
    ) {
        return result;
    }

    // Определяем целевую раскладку
    let Some(correct_layout) = detect_layout_error(text, current_layout, 0.7) else {
        return result;
    };
    if Some(correct_layout) == current_layout {
        return result;
    }
    result.target_layout = Some(correct_layout);

    // Конвертируем и проверяем осмысленность результата
    let corrected = convert::convert_between(text, current_layout, correct_layout);
    let cleaned_corrected: String = corrected.chars().filter(|c| c.is_alphabetic()).collect();
    if cleaned_corrected.is_empty() {
        return result;
    }

    let is_corrected_meaningful = match correct_layout {
        Layout::Ru => is_meaningful_russian(&cleaned_corrected),
        Layout::En => is_meaningful_english(&cleaned_corrected),
    };
    let (corrected_lang, _) = detect_language(&cleaned_corrected);

    // Принимаем решение об исправлении
    let (needs_correction, base_confidence, confidence_cap) =
        if is_corrected_meaningful && !(is_original_ru || is_original_en) {
            // СЛУЧАЙ 1: конвертированный осмысленный, оригинал нет
            let mut base = 0.9;
            if corrected_lang == Some(correct_layout) {
                base += 0.05;
            }
            (true, base, 0.99)
        } else {
            // СЛУЧАЙ 2: эвристика по доле гласных (кап 0.45)
            let vowels = match correct_layout {
                Layout::Ru => "аеёиоуыэюя",
                Layout::En => "aeiou",
            };
            let lower = cleaned_corrected.to_lowercase();
            let vowel_count = lower.chars().filter(|c| vowels.contains(*c)).count();
            let vowel_ratio = vowel_count as f64 / cleaned_corrected.chars().count() as f64;
            let mut base = vowel_ratio * 0.6;
            if is_corrected_meaningful {
                base += 0.4;
            }
            (base >= 0.3, base, 0.45)
        };

    let final_confidence = f64::max(0.3, f64::min(base_confidence, confidence_cap));

    if needs_correction && final_confidence >= 0.3 {
        result.needs_correction = true;
        result.corrected = corrected;
        result.confidence = final_confidence;

        // Контекст: +0.1, если одно из последних 3 слов — на целевой раскладке
        if context_has_layout(context, correct_layout) {
            result.confidence = f64::min(0.99, result.confidence + 0.1);
        }
    }

    result
}

/// Есть ли среди последних (до 3) слов контекста слово на заданной раскладке.
fn context_has_layout(context: &[&str], layout: Layout) -> bool {
    let start = context.len().saturating_sub(3);
    context[start..].iter().any(|ctx| {
        let cleaned: String = ctx.chars().filter(|c| c.is_alphabetic()).collect();
        detect_language(&cleaned).0 == Some(layout)
    })
}

/// Коррекция токена с «двусмысленными» символами:
/// сначала осмысленность ЦЕЛИКОМ, затем (при отсутствии смысла) — тишина.
fn ambiguous_token_correction(text: &str, mut result: Correction) -> Correction {
    // (А) оригинал осмыслен как EN-текст («can't» → «cant») — не трогаем
    let cleaned_original: String = text.chars().filter(|c| c.is_alphabetic()).collect();
    if !cleaned_original.is_empty() && is_meaningful_english(&cleaned_original) {
        return result;
    }

    // (Б) конвертация целиком en→ru и проверка осмысленности
    let candidate = convert::convert_text(text);
    let cleaned_candidate: String = candidate.chars().filter(|c| c.is_alphabetic()).collect();
    if cleaned_candidate.chars().count() < 2 {
        return result;
    }
    if !is_meaningful_russian(&cleaned_candidate) {
        return result;
    }

    let confidence = if Dictionaries::get().contains_russian(&candidate.to_lowercase()) {
        0.95
    } else {
        0.85
    };

    result.needs_correction = true;
    result.original = text.to_string();
    result.corrected = candidate;
    result.confidence = confidence;
    result.target_layout = Some(Layout::Ru);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_language() {
        assert_eq!(detect_language("ghbdtn"), (Some(Layout::En), 1.0));
        assert_eq!(detect_language("привет"), (Some(Layout::Ru), 1.0));
        assert_eq!(detect_language("123"), (None, 0.0));
        let (lang, conf) = detect_language("ghпр");
        assert_eq!(lang, None);
        assert!((conf - 0.5).abs() < 1e-9);
    }

    #[test]
    fn test_ambiguous_token_babushka() {
        let r = smart_correction(",f,eirf", &[], Some(Layout::En));
        assert!(r.needs_correction);
        assert_eq!(r.corrected, "бабушка");
        assert_eq!(r.target_layout, Some(Layout::Ru));
    }

    #[test]
    fn test_ambiguous_token_ignores_layout() {
        // Токен «,f,eirf» при layout=ru тоже → «бабушка» (ветка неделима)
        let r = smart_correction(",f,eirf", &[], Some(Layout::Ru));
        assert!(r.needs_correction);
        assert_eq!(r.corrected, "бабушка");
    }

    #[test]
    fn test_ghbdtn_to_privet() {
        let r = smart_correction("ghbdtn", &[], Some(Layout::En));
        assert!(r.needs_correction);
        assert_eq!(r.corrected, "привет");
        assert!(r.confidence > 0.9, "conf={}", r.confidence);
    }

    #[test]
    fn test_ruddshch_to_hello() {
        let r = smart_correction("руддщ", &[], Some(Layout::Ru));
        assert!(r.needs_correction);
        assert_eq!(r.corrected, "hello");
    }

    #[test]
    fn test_yo_ye_equivalent_meaningful() {
        assert!(is_meaningful_russian("ёжик"));
        assert!(is_meaningful_russian("ежик"));
    }

    #[test]
    fn test_yo_mixed_not_corrected() {
        let r = smart_correction("ёжik", &[], Some(Layout::En));
        assert!(!r.needs_correction || r.corrected == "ёжik");
    }

    #[test]
    fn test_apostrophe_word_not_corrected() {
        let r = smart_correction("can't", &[], Some(Layout::En));
        assert!(!r.needs_correction);
    }

    #[test]
    fn test_ambiguous_phrase_not_corrected() {
        let r = smart_correction("hello,world", &[], Some(Layout::En));
        assert!(!r.needs_correction);
    }

    #[test]
    fn test_ambiguous_junk_silent() {
        let r = smart_correction("eirf,", &[], Some(Layout::En));
        assert!(!r.needs_correction);
    }

    #[test]
    fn test_heuristic_confidence_capped() {
        let r = smart_correction("eirf", &[], Some(Layout::En));
        if r.needs_correction {
            assert!(r.confidence <= 0.45, "conf={}", r.confidence);
        }
    }

    #[test]
    fn test_dictionary_confidence_beats_heuristic() {
        let r1 = smart_correction(",f,eirf", &[], Some(Layout::En));
        let r2 = smart_correction("eirf", &[], Some(Layout::En));
        assert!(r1.confidence > r2.confidence);
    }

    #[test]
    fn test_short_text_ignored() {
        assert!(!smart_correction("f", &[], Some(Layout::En)).needs_correction);
        assert!(!smart_correction("", &[], Some(Layout::En)).needs_correction);
    }

    #[test]
    fn test_context_boost() {
        let r = smart_correction("ghbdtn", &["привет", "мир"], Some(Layout::En));
        assert!(r.needs_correction);
        assert!((r.confidence - 0.99).abs() < 1e-9, "conf={}", r.confidence);
    }
}
