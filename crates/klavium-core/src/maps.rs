//! Таблицы соответствия символов между раскладками QWERTY (EN) и ЙЦУКЕН (RU).
//!
//! Канонический источник данных — [`RU_TO_EN_PAIRS`]:
//! таблица пар «русский символ → английский символ».
//! Обратное отображение строится из той же таблицы, поэтому прямое и обратное
//! преобразования согласованы по построению (инвариант проверяется тестом).
//!
//! В таблицу входят:
//! - 33 строчные буквы RU ↔ EN;
//! - 33 прописные буквы RU ↔ EN;
//! - 5 символьных пар: `@`↔`"`, `#`↔`№`, `$`↔`;`, `^`↔`:`, `&`↔`?`.

/// Каноническая таблица RU → EN: `(русский символ, английский символ)`.
///
/// 33 строчные буквы + 33 прописные + 5 символьных пар = 71 пара.
pub const RU_TO_EN_PAIRS: &[(char, char)] = &[
    // Строчные буквы
    ('а', 'f'),
    ('б', ','),
    ('в', 'd'),
    ('г', 'u'),
    ('д', 'l'),
    ('е', 't'),
    ('ё', '`'),
    ('ж', ';'),
    ('з', 'p'),
    ('и', 'b'),
    ('й', 'q'),
    ('к', 'r'),
    ('л', 'k'),
    ('м', 'v'),
    ('н', 'y'),
    ('о', 'j'),
    ('п', 'g'),
    ('р', 'h'),
    ('с', 'c'),
    ('т', 'n'),
    ('у', 'e'),
    ('ф', 'a'),
    ('х', '['),
    ('ц', 'w'),
    ('ч', 'x'),
    ('ш', 'i'),
    ('щ', 'o'),
    ('ъ', ']'),
    ('ы', 's'),
    ('ь', 'm'),
    ('э', '\''),
    ('ю', '.'),
    ('я', 'z'),
    // Прописные буквы
    ('А', 'F'),
    ('Б', '<'),
    ('В', 'D'),
    ('Г', 'U'),
    ('Д', 'L'),
    ('Е', 'T'),
    ('Ё', '~'),
    ('Ж', ':'),
    ('З', 'P'),
    ('И', 'B'),
    ('Й', 'Q'),
    ('К', 'R'),
    ('Л', 'K'),
    ('М', 'V'),
    ('Н', 'Y'),
    ('О', 'J'),
    ('П', 'G'),
    ('Р', 'H'),
    ('С', 'C'),
    ('Т', 'N'),
    ('У', 'E'),
    ('Ф', 'A'),
    ('Х', '{'),
    ('Ц', 'W'),
    ('Ч', 'X'),
    ('Ш', 'I'),
    ('Щ', 'O'),
    ('Ъ', '}'),
    ('Ы', 'S'),
    ('Ь', 'M'),
    ('Э', '"'),
    ('Ю', '>'),
    ('Я', 'Z'),
    // Символьные пары
    ('"', '@'),
    ('№', '#'),
    (';', '$'),
    (':', '^'),
    ('?', '&'),
];

/// Преобразовать один символ из RU-раскладки в EN.
#[must_use]
pub fn ru_to_en(c: char) -> Option<char> {
    RU_TO_EN_PAIRS
        .iter()
        .find_map(|&(ru, en)| (ru == c).then_some(en))
}

/// Преобразовать один символ из EN-раскладки в RU.
#[must_use]
pub fn en_to_ru(c: char) -> Option<char> {
    RU_TO_EN_PAIRS
        .iter()
        .find_map(|&(ru, en)| (en == c).then_some(ru))
}

/// Проверить, является ли символ частью раскладки EN (есть отображение EN → RU).
#[must_use]
pub fn is_en_char(c: char) -> bool {
    en_to_ru(c).is_some()
}

/// Проверить, является ли символ частью раскладки RU (есть отображение RU → EN).
#[must_use]
pub fn is_ru_char(c: char) -> bool {
    ru_to_en(c).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn test_table_size() {
        assert_eq!(RU_TO_EN_PAIRS.len(), 71);
    }

    #[test]
    fn test_canonical_pairs_are_bidirectional() {
        for &(ru, en) in RU_TO_EN_PAIRS {
            assert_eq!(ru_to_en(ru), Some(en), "ru_to_en({ru:?})");
            assert_eq!(en_to_ru(en), Some(ru), "en_to_ru({en:?})");
        }
    }

    #[test]
    fn test_table_has_no_duplicate_keys() {
        let mut rus = HashSet::new();
        let mut ens = HashSet::new();
        for &(ru, en) in RU_TO_EN_PAIRS {
            assert!(rus.insert(ru), "дубликат RU-символа: {ru:?}");
            assert!(ens.insert(en), "дубликат EN-символа: {en:?}");
        }
    }

    #[test]
    fn test_ru_to_en_basic() {
        assert_eq!(ru_to_en('а'), Some('f'));
        assert_eq!(ru_to_en('я'), Some('z'));
        assert_eq!(ru_to_en('ж'), Some(';'));
        assert_eq!(ru_to_en('ё'), Some('`'));
        assert_eq!(ru_to_en('б'), Some(','));
        assert_eq!(ru_to_en('ю'), Some('.'));
        assert_eq!(ru_to_en('э'), Some('\''));
    }

    #[test]
    fn test_en_to_ru_basic() {
        assert_eq!(en_to_ru('f'), Some('а'));
        assert_eq!(en_to_ru('q'), Some('й'));
        assert_eq!(en_to_ru('z'), Some('я'));
        assert_eq!(en_to_ru(','), Some('б'));
        assert_eq!(en_to_ru('.'), Some('ю'));
        assert_eq!(en_to_ru('`'), Some('ё'));
    }

    #[test]
    fn test_uppercase_mapping() {
        assert_eq!(en_to_ru('A'), Some('Ф'));
        assert_eq!(en_to_ru('Q'), Some('Й'));
        assert_eq!(ru_to_en('Ф'), Some('A'));
        assert_eq!(ru_to_en('Э'), Some('"'));
        assert_eq!(en_to_ru('~'), Some('Ё'));
        assert_eq!(en_to_ru('>'), Some('Ю'));
    }

    #[test]
    fn test_symbol_pairs() {
        assert_eq!(en_to_ru('@'), Some('"'));
        assert_eq!(ru_to_en('"'), Some('@'));
        assert_eq!(en_to_ru('#'), Some('№'));
        assert_eq!(ru_to_en('№'), Some('#'));
        assert_eq!(en_to_ru('$'), Some(';'));
        assert_eq!(ru_to_en(';'), Some('$'));
        assert_eq!(en_to_ru('^'), Some(':'));
        assert_eq!(ru_to_en(':'), Some('^'));
        assert_eq!(en_to_ru('&'), Some('?'));
        assert_eq!(ru_to_en('?'), Some('&'));
    }

    #[test]
    fn test_no_mapping_returns_none() {
        assert_eq!(en_to_ru('1'), None);
        assert_eq!(ru_to_en('1'), None);
        assert_eq!(en_to_ru(' '), None);
        assert_eq!(ru_to_en(' '), None);
        assert_eq!(en_to_ru('б'), None);
        assert_eq!(ru_to_en('f'), None);
        assert_eq!(en_to_ru('!'), None);
    }

    #[test]
    fn test_is_en_char() {
        assert!(is_en_char('a'));
        assert!(is_en_char('Q'));
        assert!(is_en_char('@'));
        assert!(!is_en_char('а'));
        assert!(!is_en_char('1'));
    }

    #[test]
    fn test_is_ru_char() {
        assert!(is_ru_char('а'));
        assert!(is_ru_char('Й'));
        assert!(is_ru_char('№'));
        assert!(!is_ru_char('a'));
        assert!(!is_ru_char('1'));
    }
}
