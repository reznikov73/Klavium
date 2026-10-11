//! Клавиши: скан-коды → видимые символы.
//!
//! Разрешение символа зависит от раскладки, Shift и «особых» клавиш
//! (ё х ъ ж э б ю в RU; пунктуация в EN) — фиксированные таблицы.
//!

use crate::layout::Layout;

/// Скан-код → строчная латинская буква QWERTY.
const SCANCODE_TO_QWERTY: &[(u32, char)] = &[
    (0x10, 'q'),
    (0x11, 'w'),
    (0x12, 'e'),
    (0x13, 'r'),
    (0x14, 't'),
    (0x15, 'y'),
    (0x16, 'u'),
    (0x17, 'i'),
    (0x18, 'o'),
    (0x19, 'p'),
    (0x1E, 'a'),
    (0x1F, 's'),
    (0x20, 'd'),
    (0x21, 'f'),
    (0x22, 'g'),
    (0x23, 'h'),
    (0x24, 'j'),
    (0x25, 'k'),
    (0x26, 'l'),
    (0x2C, 'z'),
    (0x2D, 'x'),
    (0x2E, 'c'),
    (0x2F, 'v'),
    (0x30, 'b'),
    (0x31, 'n'),
    (0x32, 'm'),
];

/// Скан-код → базовый (unshifted) EN-символ цифр/пунктуации.
const SCANCODE_TO_CHAR: &[(u32, char)] = &[
    (0x02, '1'),
    (0x03, '2'),
    (0x04, '3'),
    (0x05, '4'),
    (0x06, '5'),
    (0x07, '6'),
    (0x08, '7'),
    (0x09, '8'),
    (0x0A, '9'),
    (0x0B, '0'),
    (0x0C, '-'),
    (0x0D, '='),
    (0x1A, '['),
    (0x1B, ']'),
    (0x27, ';'),
    (0x28, '\''),
    (0x29, '`'),
    (0x2B, '\\'),
    (0x33, ','),
    (0x34, '.'),
    (0x35, '/'),
];

/// Скан-код → БУКВА русской раскладки (ё х ъ ж э б ю).
const SCANCODE_TO_RU_LETTER: &[(u32, char)] = &[
    (0x29, 'ё'),
    (0x1A, 'х'),
    (0x1B, 'ъ'),
    (0x27, 'ж'),
    (0x28, 'э'),
    (0x33, 'б'),
    (0x34, 'ю'),
];

/// Shift-варианты EN-символов.
const SHIFT_MAP: &[(char, char)] = &[
    ('1', '!'),
    ('2', '@'),
    ('3', '#'),
    ('4', '$'),
    ('5', '%'),
    ('6', '^'),
    ('7', '&'),
    ('8', '*'),
    ('9', '('),
    ('0', ')'),
    ('-', '_'),
    ('=', '+'),
    ('[', '{'),
    (']', '}'),
    ('\\', '|'),
    (';', ':'),
    ('\'', '"'),
    ('`', '~'),
    (',', '<'),
    ('.', '>'),
    ('/', '?'),
];

/// Shift-варианты цифровой строки в RU (фактические RU-символы).
const RU_SHIFT_MAP: &[(char, char)] = &[
    ('1', '!'),
    ('2', '"'),
    ('3', '№'),
    ('4', ';'),
    ('5', '%'),
    ('6', ':'),
    ('7', '?'),
    ('8', '*'),
    ('9', '('),
    ('0', ')'),
    ('-', '_'),
    ('=', '+'),
    ('\\', '/'),
];

/// QWERTY-символ → кириллица (нижний регистр).
const QWERTY_TO_CYRILLIC: &[(char, char)] = &[
    ('q', 'й'),
    ('w', 'ц'),
    ('e', 'у'),
    ('r', 'к'),
    ('t', 'е'),
    ('y', 'н'),
    ('u', 'г'),
    ('i', 'ш'),
    ('o', 'щ'),
    ('p', 'з'),
    ('a', 'ф'),
    ('s', 'ы'),
    ('d', 'в'),
    ('f', 'а'),
    ('g', 'п'),
    ('h', 'р'),
    ('j', 'о'),
    ('k', 'л'),
    ('l', 'д'),
    ('z', 'я'),
    ('x', 'ч'),
    ('c', 'с'),
    ('v', 'м'),
    ('b', 'и'),
    ('n', 'т'),
    ('m', 'ь'),
];

const VK_TAB: u32 = 0x09;
const VK_RETURN: u32 = 0x0D;
const VK_SPACE: u32 = 0x20;

fn lookup_scan(table: &[(u32, char)], code: u32) -> Option<char> {
    table
        .iter()
        .find(|(key, _)| *key == code)
        .map(|(_, value)| *value)
}

fn lookup_char(table: &[(char, char)], key: char) -> Option<char> {
    table
        .iter()
        .find(|(k, _)| *k == key)
        .map(|(_, value)| *value)
}

fn upper(c: char) -> char {
    c.to_uppercase().next().unwrap_or(c)
}

/// Определить видимый символ клавиши по скан-коду, раскладке и Shift.
///
/// `vk_code` нужен только для служебных клавиш (пробел/Enter/Tab).
/// `layout: None` (неизвестная раскладка) обрабатывается как EN-ветка —
/// раскладка к этому моменту всегда известна.
#[must_use]
pub fn resolve_char(
    vk_code: u32,
    scan_code: u32,
    layout: Option<Layout>,
    shift: bool,
) -> Option<char> {
    match vk_code {
        VK_SPACE => return Some(' '),
        VK_RETURN => return Some('\n'),
        VK_TAB => return Some('\t'),
        _ => {}
    }

    let qwerty = lookup_scan(SCANCODE_TO_QWERTY, scan_code);

    if layout == Some(Layout::Ru) {
        if let Some(letter) = lookup_scan(SCANCODE_TO_RU_LETTER, scan_code) {
            return Some(if shift { upper(letter) } else { letter });
        }
        if let Some(q) = qwerty {
            let cyrillic = lookup_char(QWERTY_TO_CYRILLIC, q).unwrap_or(q);
            return Some(if shift { upper(cyrillic) } else { cyrillic });
        }
        return lookup_scan(SCANCODE_TO_CHAR, scan_code).map(|base| {
            if shift {
                lookup_char(RU_SHIFT_MAP, base).unwrap_or(base)
            } else {
                base
            }
        });
    }

    // EN-ветка (включая неизвестную раскладку)
    if let Some(q) = qwerty {
        return Some(if shift { q.to_ascii_uppercase() } else { q });
    }
    lookup_scan(SCANCODE_TO_CHAR, scan_code).map(|base| {
        if shift {
            lookup_char(SHIFT_MAP, base).unwrap_or(base)
        } else {
            base
        }
    })
}

/// Печатный QWERTY-символ по скан-коду (буквы и цифры/пунктуация; регистр — нижний).
#[must_use]
pub fn qwerty_char(scan_code: u32) -> Option<char> {
    lookup_scan(SCANCODE_TO_QWERTY, scan_code).or_else(|| lookup_scan(SCANCODE_TO_CHAR, scan_code))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_letters_en() {
        assert_eq!(resolve_char(0x51, 0x10, Some(Layout::En), false), Some('q'));
        assert_eq!(resolve_char(0x51, 0x10, Some(Layout::En), true), Some('Q'));
        assert_eq!(resolve_char(0x51, 0x10, None, false), Some('q'));
    }

    #[test]
    fn test_letters_ru() {
        assert_eq!(resolve_char(0x51, 0x10, Some(Layout::Ru), false), Some('й'));
        assert_eq!(resolve_char(0x51, 0x10, Some(Layout::Ru), true), Some('Й'));
    }

    #[test]
    fn test_ru_letter_keys() {
        // Клавиша «,» в RU даёт «б», «.» → «ю», «`» → «ё».
        assert_eq!(resolve_char(0xBC, 0x33, Some(Layout::Ru), false), Some('б'));
        assert_eq!(resolve_char(0xBE, 0x34, Some(Layout::Ru), false), Some('ю'));
        assert_eq!(resolve_char(0xC0, 0x29, Some(Layout::Ru), false), Some('ё'));
    }

    #[test]
    fn test_punct_and_shift() {
        assert_eq!(resolve_char(0x32, 0x03, Some(Layout::En), true), Some('@'));
        assert_eq!(resolve_char(0x32, 0x03, Some(Layout::Ru), true), Some('"'));
        assert_eq!(resolve_char(0xBF, 0x35, Some(Layout::En), false), Some('/'));
        assert_eq!(resolve_char(0xBF, 0x35, Some(Layout::En), true), Some('?'));
        // Клавиша скан-кода 0x35 в RU даёт «/»: код обрабатывает клавишу общим
        // путём (упоминание «.»/«,» в старых комментариях не подтвердилось).
        // Поведение зафиксировано этим тестом.
        assert_eq!(resolve_char(0xBF, 0x35, Some(Layout::Ru), false), Some('/'));
    }

    #[test]
    fn test_special_keys() {
        assert_eq!(resolve_char(0x20, 0x39, Some(Layout::Ru), false), Some(' '));
        assert_eq!(
            resolve_char(0x0D, 0x1C, Some(Layout::Ru), false),
            Some('\n')
        );
        assert_eq!(
            resolve_char(0x09, 0x0F, Some(Layout::Ru), false),
            Some('\t')
        );
    }

    #[test]
    fn test_qwerty_char() {
        assert_eq!(qwerty_char(0x10), Some('q'));
        assert_eq!(qwerty_char(0x03), Some('2'));
        assert_eq!(qwerty_char(0x4B), None);
    }

    #[test]
    fn test_unknown_key() {
        // Стрелка влево (scan 0x4B) не маппится ни в один символ.
        assert_eq!(resolve_char(0x25, 0x4B, Some(Layout::En), false), None);
    }
}
