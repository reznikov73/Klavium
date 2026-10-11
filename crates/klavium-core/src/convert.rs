//! Конвертация текста между раскладками.
//!
//! Конвертация текста между раскладками:
//! посимвольное преобразование по каноническим таблицам из [`crate::maps`],
//! с сохранением регистра и отдельной конвертацией знаков препинания.

use crate::layout::Layout;
use crate::maps;

/// Конвертировать текст, набранный не в той раскладке: EN → RU.
///
/// # Примеры
///
/// ```
/// use klavium_core::convert::convert_text;
///
/// assert_eq!(convert_text("ghbdtn"), "привет");
/// assert_eq!(convert_text("Ghbdtn"), "Привет");
/// ```
#[must_use]
pub fn convert_text(input: &str) -> String {
    convert_between(input, Some(Layout::En), Layout::Ru)
}

/// Конвертировать в обратную сторону (RU → EN).
///
/// # Примеры
///
/// ```
/// use klavium_core::convert::convert_text_reverse;
///
/// assert_eq!(convert_text_reverse("руддщ"), "hello");
/// ```
#[must_use]
pub fn convert_text_reverse(input: &str) -> String {
    convert_between(input, Some(Layout::Ru), Layout::En)
}

/// Конвертировать текст между раскладками (посимвольно, с сохранением регистра).
///
/// Конвертация текста из одной раскладки в другую:
/// - совпадающие раскладки — текст возвращается без изменений;
/// - неизвестная исходная раскладка (`None`) — текст возвращается без изменений
///   (поведение 'unknown');
/// - иначе — посимвольная замена по канонической таблице [`crate::maps`].
#[must_use]
pub fn convert_between(text: &str, from: Option<Layout>, to: Layout) -> String {
    if from == Some(to) {
        return text.to_string();
    }

    let mapper: fn(char) -> Option<char> = match (from, to) {
        (Some(Layout::Ru), Layout::En) => maps::ru_to_en,
        (Some(Layout::En), Layout::Ru) => maps::en_to_ru,
        _ => return text.to_string(),
    };

    convert_with(text, mapper)
}

/// Конвертировать ТОЛЬКО знаки препинания между раскладками.
///
/// Буквы не трогает; неизвестные комбинации раскладок и символы без
/// отображения возвращает как есть.
#[must_use]
pub fn convert_punct(text: &str, from_layout: &str, to_layout: &str) -> String {
    if text.is_empty() || from_layout == to_layout {
        return text.to_string();
    }
    match (from_layout, to_layout) {
        ("en", "ru") => text
            .chars()
            .map(|c| match c {
                '`' => 'ё',
                '~' => 'Ё',
                '@' => '"',
                '#' => '№',
                '$' => ';',
                '^' => ':',
                '&' => '?',
                '/' => '.',
                '?' => ',',
                '|' => '/',
                other => other,
            })
            .collect(),
        ("ru", "en") => text
            .chars()
            .map(|c| match c {
                'ё' => '`',
                'Ё' => '~',
                '"' => '@',
                '№' => '#',
                ';' => '$',
                ':' => '^',
                '?' => '&',
                '.' => '/',
                ',' => '?',
                '/' => '|',
                other => other,
            })
            .collect(),
        _ => text.to_string(),
    }
}

/// Общий посимвольный проход с сохранением регистра.
///
/// Семантика:
/// `if char.isupper() and converted.isalpha(): converted = converted.upper()`.
fn convert_with(input: &str, mapper: fn(char) -> Option<char>) -> String {
    input
        .chars()
        .map(|c| {
            let converted = mapper(c).unwrap_or(c);
            if c.is_uppercase() && converted.is_alphabetic() {
                converted.to_uppercase().next().unwrap_or(converted)
            } else {
                converted
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_convert_simple() {
        assert_eq!(convert_text("ghbdtn"), "привет");
        assert_eq!(convert_text("hello"), "руддщ");
    }

    #[test]
    fn test_convert_mixed() {
        assert_eq!(convert_text("ghbdtn 123"), "привет 123");
    }

    #[test]
    fn test_convert_phrase_with_comma() {
        assert_eq!(convert_text("hello, world"), "руддщб цщкдв");
    }

    #[test]
    fn test_convert_ambiguous_token() {
        assert_eq!(convert_text(",f,eirf"), "бабушка");
    }

    #[test]
    fn test_convert_apostrophe() {
        assert_eq!(convert_text("can't"), "сфтэе");
    }

    #[test]
    fn test_convert_single_symbols() {
        assert_eq!(convert_text("@"), "\"");
        assert_eq!(convert_text("&"), "?");
        assert_eq!(convert_text("."), "ю");
        assert_eq!(convert_text(","), "б");
    }

    #[test]
    fn test_convert_empty_and_unmapped() {
        assert_eq!(convert_text(""), "");
        assert_eq!(convert_text("123!%*()"), "123!%*()");
    }

    #[test]
    fn test_convert_uppercase() {
        assert_eq!(convert_text("Ghbdtn"), "Привет");
    }

    #[test]
    fn test_convert_keyboard_rows() {
        assert_eq!(convert_text("qwerty"), "йцукен");
        assert_eq!(convert_text_reverse("ЙЦУКЕН"), "QWERTY");
    }

    #[test]
    fn test_convert_reverse_simple() {
        assert_eq!(convert_text_reverse("руддщ"), "hello");
        assert_eq!(convert_text_reverse("привет"), "ghbdtn");
        assert_eq!(convert_text_reverse("ёжик"), "`;br");
        assert_eq!(convert_text_reverse("№"), "#");
        assert_eq!(convert_text_reverse("ж"), ";");
    }

    #[test]
    fn test_convert_reverse_uppercase() {
        assert_eq!(convert_text_reverse("Руддщ"), "Hello");
    }

    #[test]
    fn test_convert_between() {
        assert_eq!(
            convert_between("ghbdtn", Some(Layout::En), Layout::Ru),
            "привет"
        );
        assert_eq!(
            convert_between("руддщ", Some(Layout::Ru), Layout::En),
            "hello"
        );
        // Неизвестная исходная раскладка — без изменений (поведение 'unknown')
        assert_eq!(convert_between("ghbdtn", None, Layout::Ru), "ghbdtn");
        // Совпадающие раскладки — без изменений
        assert_eq!(
            convert_between("ghbdtn", Some(Layout::En), Layout::En),
            "ghbdtn"
        );
    }

    #[test]
    fn test_convert_punct_en_to_ru() {
        assert_eq!(convert_punct("`", "en", "ru"), "ё");
        assert_eq!(convert_punct("/", "en", "ru"), ".");
        assert_eq!(convert_punct("?", "en", "ru"), ",");
        assert_eq!(convert_punct("|", "en", "ru"), "/");
        assert_eq!(convert_punct("abc", "en", "ru"), "abc");
    }

    #[test]
    fn test_convert_punct_ru_to_en() {
        assert_eq!(convert_punct("ё", "ru", "en"), "`");
        assert_eq!(convert_punct(".", "ru", "en"), "/");
        assert_eq!(convert_punct("?", "ru", "en"), "&");
        assert_eq!(convert_punct(",", "ru", "en"), "?");
    }

    #[test]
    fn test_convert_punct_same_or_unknown_layout() {
        assert_eq!(convert_punct("?.", "ru", "ru"), "?.");
        assert_eq!(convert_punct("x", "zz", "yy"), "x");
    }
}
