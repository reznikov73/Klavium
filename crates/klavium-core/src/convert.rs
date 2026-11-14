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

/// Направление транслитерации (пункт ТЗ 7.7).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TranslitDirection {
    /// Кириллица → латиница.
    RuToLat,
    /// Латиница → кириллица.
    LatToRu,
}

/// Транслитерация: русский текст латиницей и обратно.
///
/// Это не конвертация раскладки (для неё — [`convert_text`]): здесь буквы
/// меняются на другие буквы по смыслу, «привет» → «privet».
///
/// Таблица — бытовая: основа ГОСТ 7.79-2000 (система B) с заменами для
/// читаемости: `й → y`, `х → kh`, `ц → ts`, `щ → shch`, `ы → y`, `э → e`,
/// `ъ` и `ь` не передаются. Обратный разбор жадный, по самому длинному
/// совпадению; диграфы ГОСТ побеждают одиночные буквы, поэтому «yo» — это
/// «ё» («yogurt» → «ёгурт», а не «йогурт»). Одиночная «y» перед гласной и
/// в конце слова после гласной — «й» («yes» → «йес», «may» → «май»),
/// иначе — «ы» («my» → «мы»). Кириллица во входе для `LatToRu`, латиница
/// во входе для `RuToLat`, а также цифры, пробелы и знаки остаются как есть.
///
/// Ограничение бытовой таблицы: `ъ` и `ь` теряются при прямом проходе
/// («мышь» → «mysh» → «мыш»), «э» и «е» сливаются в одну «e»
/// («экран» → «ekran» → «екран»).
///
/// # Примеры
///
/// ```
/// use klavium_core::convert::{translit, TranslitDirection};
///
/// assert_eq!(translit("привет", TranslitDirection::RuToLat), "privet");
/// assert_eq!(translit("privet", TranslitDirection::LatToRu), "привет");
/// assert_eq!(translit("ЩЁЛК", TranslitDirection::RuToLat), "SHCHYOLK");
/// ```
#[must_use]
pub fn translit(text: &str, direction: TranslitDirection) -> String {
    match direction {
        TranslitDirection::RuToLat => translit_ru_to_lat(text),
        TranslitDirection::LatToRu => translit_lat_to_ru(text),
    }
}

/// Выбрать направление транслитерации по составу букв.
///
/// Поровну кириллицы и латиницы (смешанный текст) или ни одной буквы —
/// `None`: преобразовывать нечего.
///
/// # Примеры
///
/// ```
/// use klavium_core::convert::{detect_translit_direction, TranslitDirection};
///
/// assert_eq!(detect_translit_direction("привет"), Some(TranslitDirection::RuToLat));
/// assert_eq!(detect_translit_direction("privet"), Some(TranslitDirection::LatToRu));
/// assert_eq!(detect_translit_direction("123"), None);
/// ```
#[must_use]
pub fn detect_translit_direction(text: &str) -> Option<TranslitDirection> {
    let mut cyrillic = 0_usize;
    let mut latin = 0_usize;
    for ch in text.chars() {
        if matches!(ch, 'а'..='я' | 'А'..='Я' | 'ё' | 'Ё') {
            cyrillic += 1;
        } else if ch.is_ascii_alphabetic() {
            latin += 1;
        }
    }
    match cyrillic.cmp(&latin) {
        std::cmp::Ordering::Greater => Some(TranslitDirection::RuToLat),
        std::cmp::Ordering::Less => Some(TranslitDirection::LatToRu),
        std::cmp::Ordering::Equal => None,
    }
}

/// Замена одной кириллической буквы (строчной) латиницей.
///
/// Основа — ГОСТ 7.79-2000 (система B); отличия задокументированы
/// в [`translit`]. `None` — буква не кириллическая.
const fn ru_to_lat(ch: char) -> Option<&'static str> {
    Some(match ch {
        'а' => "a",
        'б' => "b",
        'в' => "v",
        'г' => "g",
        'д' => "d",
        'е' | 'э' => "e",
        'ё' => "yo",
        'ж' => "zh",
        'з' => "z",
        'и' => "i",
        'й' | 'ы' => "y",
        'к' => "k",
        'л' => "l",
        'м' => "m",
        'н' => "n",
        'о' => "o",
        'п' => "p",
        'р' => "r",
        'с' => "s",
        'т' => "t",
        'у' => "u",
        'ф' => "f",
        'х' => "kh",
        'ц' => "ts",
        'ч' => "ch",
        'ш' => "sh",
        'щ' => "shch",
        'ъ' | 'ь' => "",
        'ю' => "yu",
        'я' => "ya",
        _ => return None,
    })
}

/// Многобуквенные сочетания латиницы: разбор жадный, от самого длинного,
/// поэтому «shch» стоит раньше «sh».
const LAT_DIGRAPHS: [(&str, &str); 9] = [
    ("shch", "щ"),
    ("sh", "ш"),
    ("ch", "ч"),
    ("zh", "ж"),
    ("yo", "ё"),
    ("yu", "ю"),
    ("ya", "я"),
    ("kh", "х"),
    ("ts", "ц"),
];

/// Замена одной латинской буквы кириллицей.
///
/// `y` отдельно: см. [`y_replacement`]. Одиночная `c` — «ц» (как в ГОСТ),
/// `x` — «кс», `j` — «й»: документированные приближения.
const fn lat_to_ru(ch: char) -> Option<&'static str> {
    Some(match ch.to_ascii_lowercase() {
        'a' => "а",
        'b' => "б",
        'c' => "ц",
        'd' => "д",
        'e' => "е",
        'f' => "ф",
        'g' => "г",
        'h' => "х",
        'i' => "и",
        'j' => "й",
        'k' => "к",
        'l' => "л",
        'm' => "м",
        'n' => "н",
        'o' => "о",
        'p' => "п",
        'r' => "р",
        's' => "с",
        't' => "т",
        'u' => "у",
        'v' | 'w' => "в",
        'x' => "кс",
        'z' => "з",
        _ => return None,
    })
}

/// Латинская гласная (для правила одиночной «y»).
const fn is_latin_vowel(ch: char) -> bool {
    matches!(ch.to_ascii_lowercase(), 'a' | 'e' | 'i' | 'o' | 'u')
}

/// Одиночная «y»: перед гласной и в конце слова после гласной — «й»
/// («yes» → «йес», «may» → «май»), иначе — «ы» («my» → «мы»,
/// «bystro» → «быстро»). Диграфы «ya/yo/yu» разбираются раньше
/// и сюда не попадают.
fn y_replacement(chars: &[char], at: usize) -> &'static str {
    let next = chars.get(at + 1);
    let next_vowel = next.is_some_and(|c| is_latin_vowel(*c));
    let word_final = next.is_none_or(|c| !c.is_alphabetic());
    let prev_vowel = at > 0 && is_latin_vowel(chars[at - 1]);
    if next_vowel || (word_final && prev_vowel) {
        "й"
    } else {
        "ы"
    }
}

/// Прямой проход: кириллица → латиница, с сохранением регистра.
///
/// Одна заглавная буква может перейти в несколько латинских: «Ёлка» →
/// «Yolka», но «ЁЛКА» → «YOLKA» — регистр следующей буквы решает,
/// заглавными или строчными писать замену.
fn translit_ru_to_lat(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    for (index, &ch) in chars.iter().enumerate() {
        let Some(lower) = ch.to_lowercase().next() else {
            out.push(ch);
            continue;
        };
        match ru_to_lat(lower) {
            Some("") => {}
            Some(mapped) if ch.is_uppercase() => {
                let next_upper = chars.get(index + 1).is_some_and(|n| n.is_uppercase());
                if next_upper {
                    out.push_str(&mapped.to_uppercase());
                } else {
                    push_title(&mut out, mapped);
                }
            }
            Some(mapped) => out.push_str(mapped),
            None => out.push(ch),
        }
    }
    out
}

/// Обратный проход: латиница → кириллица, жадный разбор по самому длинному
/// совпадению, с сохранением регистра.
fn translit_lat_to_ru(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut index = 0;
    while index < chars.len() {
        let ch = chars[index];
        if ch.is_ascii_alphabetic() {
            if let Some((len, mapped)) = match_digraph(&chars, index) {
                push_mapped(&mut out, mapped, &chars[index..index + len]);
                index += len;
                continue;
            }
            if ch.eq_ignore_ascii_case(&'y') {
                push_mapped(&mut out, y_replacement(&chars, index), &[ch]);
                index += 1;
                continue;
            }
            if let Some(mapped) = lat_to_ru(ch) {
                push_mapped(&mut out, mapped, &[ch]);
                index += 1;
                continue;
            }
        }
        out.push(ch);
        index += 1;
    }
    out
}

/// Совпавшее многобуквенное сочетание и его длина (регистронезависимо).
fn match_digraph(chars: &[char], at: usize) -> Option<(usize, &'static str)> {
    LAT_DIGRAPHS.iter().find_map(|&(latin, mapped)| {
        let len = latin.chars().count();
        if at + len > chars.len() {
            return None;
        }
        let matched = latin
            .chars()
            .enumerate()
            .all(|(i, expected)| chars[at + i].to_ascii_lowercase() == expected);
        matched.then_some((len, mapped))
    })
}

/// Первая буква замены — заглавная, остальные как есть («Ё» → «Yo»).
fn push_title(out: &mut String, mapped: &str) {
    let mut rest = mapped.chars();
    if let Some(first) = rest.next() {
        out.extend(first.to_uppercase());
        out.push_str(rest.as_str());
    }
}

/// Вставить замену с регистром исходных букв: всё заглавное — заменяем
/// целиком заглавными («SHCH» → «Щ», «X» → «КС»), первая заглавная —
/// заглавная первая буква замены («Shch» → «Щ», «Yolka» → «Ёлка»).
fn push_mapped(out: &mut String, mapped: &str, source: &[char]) {
    if source.iter().all(|c| c.is_uppercase()) {
        out.push_str(&mapped.to_uppercase());
    } else if source[0].is_uppercase() {
        push_title(out, mapped);
    } else {
        out.push_str(mapped);
    }
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

/// Перекодировать в другую раскладку одиночные буквы-слова.
///
/// Слова из двух и более букв движок исправляет поштучно, а одиночная
/// буква в очередь не попадает: «z» — это и слово «я», и переменная
/// в формуле. Когда раскладка фразы уже признана неверной (пакетная
/// правка, исправление слова с хвостом), такие буквы нужно дотянуть —
/// этот проход трогает только односимвольные буквенные токены и ничего
/// больше.
///
/// # Примеры
///
/// ```
/// use klavium_core::convert::convert_single_letters;
/// use klavium_core::Layout;
///
/// assert_eq!(
///     convert_single_letters("ghbdtn z ghbitk", Layout::En, Layout::Ru),
///     "ghbdtn я ghbitk"
/// );
/// ```
#[must_use]
pub fn convert_single_letters(text: &str, from: Layout, to: Layout) -> String {
    let mapper: fn(char) -> Option<char> = match (from, to) {
        (Layout::Ru, Layout::En) => maps::ru_to_en,
        (Layout::En, Layout::Ru) => maps::en_to_ru,
        _ => return text.to_string(),
    };

    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut index = 0;
    while index < chars.len() {
        if !chars[index].is_alphabetic() {
            out.push(chars[index]);
            index += 1;
            continue;
        }
        let start = index;
        while index < chars.len() && chars[index].is_alphabetic() {
            index += 1;
        }
        // Слово из двух и более букв не трогаем — его судьбу решает движок.
        if index - start != 1 {
            out.extend(&chars[start..index]);
            continue;
        }
        let ch = chars[start];
        let converted = mapper(ch).unwrap_or(ch);
        if ch.is_uppercase() && converted.is_alphabetic() {
            out.extend(converted.to_uppercase());
        } else {
            out.push(converted);
        }
    }
    out
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

    #[test]
    fn test_translit_ru_to_lat_words() {
        let cases = [
            ("привет", "privet"),
            ("мама", "mama"),
            ("спасибо", "spasibo"),
            ("хорошо", "khorosho"),
            ("щука", "shchuka"),
            ("ёжик", "yozhik"),
            ("йогурт", "yogurt"),
            ("мышь", "mysh"),
            ("съезд", "sezd"),
            ("объект", "obekt"),
            ("царь", "tsar"),
            ("экран", "ekran"),
            ("цена", "tsena"),
            ("язык", "yazyk"),
            ("лучший", "luchshiy"),
            ("журнал", "zhurnal"),
            ("чай", "chay"),
            ("фильм", "film"),
            ("вода", "voda"),
            ("солнце", "solntse"),
            ("сердце", "serdtse"),
            ("юла", "yula"),
            ("подъезд", "podezd"),
            ("зима", "zima"),
            ("лето", "leto"),
            ("кот", "kot"),
        ];
        for (input, expected) in cases {
            assert_eq!(translit(input, TranslitDirection::RuToLat), expected);
        }
    }

    #[test]
    fn test_translit_ru_to_lat_case() {
        assert_eq!(translit("Привет", TranslitDirection::RuToLat), "Privet");
        assert_eq!(translit("ПРИВЕТ", TranslitDirection::RuToLat), "PRIVET");
        assert_eq!(translit("Ёлка", TranslitDirection::RuToLat), "Yolka");
        assert_eq!(translit("ЁЛКА", TranslitDirection::RuToLat), "YOLKA");
        assert_eq!(translit("ЩЁЛК", TranslitDirection::RuToLat), "SHCHYOLK");
        assert_eq!(translit("Щука", TranslitDirection::RuToLat), "Shchuka");
        // ъ и ь не передаются, в любом регистре
        assert_eq!(translit("Мышь", TranslitDirection::RuToLat), "Mysh");
    }

    #[test]
    fn test_translit_ru_to_lat_mixed_input() {
        // Латиница, цифры и знаки в русском тексте остаются как есть
        assert_eq!(
            translit("привет world 123", TranslitDirection::RuToLat),
            "privet world 123"
        );
        assert_eq!(translit("Нью-Йорк", TranslitDirection::RuToLat), "Nyu-York");
        assert_eq!(translit("", TranslitDirection::RuToLat), "");
    }

    #[test]
    fn test_translit_lat_to_ru_words() {
        let cases = [
            ("privet", "привет"),
            ("mama", "мама"),
            ("spasibo", "спасибо"),
            ("khorosho", "хорошо"),
            ("shchuka", "щука"),
            ("yozhik", "ёжик"),
            ("mysh", "мыш"),
            ("sezd", "сезд"),
            ("tsena", "цена"),
            ("yazyk", "язык"),
            ("luchshiy", "лучший"),
            ("zhurnal", "журнал"),
            ("chay", "чай"),
            ("film", "филм"),
            ("voda", "вода"),
            ("solntse", "солнце"),
            ("serdtse", "сердце"),
            ("yula", "юла"),
            ("zima", "зима"),
            ("leto", "лето"),
            ("kot", "кот"),
            ("bystro", "быстро"),
            ("may", "май"),
            ("geroy.", "герой."),
            ("yes", "йес"),
            ("my", "мы"),
            ("box", "бокс"),
            ("extra", "екстра"),
        ];
        for (input, expected) in cases {
            assert_eq!(
                translit(input, TranslitDirection::LatToRu),
                expected,
                "{input}"
            );
        }
    }

    #[test]
    fn test_translit_lat_to_ru_case() {
        assert_eq!(translit("Privet", TranslitDirection::LatToRu), "Привет");
        assert_eq!(translit("PRIVET", TranslitDirection::LatToRu), "ПРИВЕТ");
        assert_eq!(translit("Yolka", TranslitDirection::LatToRu), "Ёлка");
        assert_eq!(translit("YOGA", TranslitDirection::LatToRu), "ЁГА");
        assert_eq!(translit("SHCHUKA", TranslitDirection::LatToRu), "ЩУКА");
        assert_eq!(translit("Shchuka", TranslitDirection::LatToRu), "Щука");
        assert_eq!(translit("BOX", TranslitDirection::LatToRu), "БОКС");
    }

    #[test]
    fn test_translit_lat_to_ru_digraph_traps() {
        // Жадный разбор: сначала «shch», потом «sh», затем одиночные
        assert_eq!(translit("eshchyo", TranslitDirection::LatToRu), "ещё");
        assert_eq!(translit("shchina", TranslitDirection::LatToRu), "щина");
        // «yo» — всегда «ё», даже когда имелось в виду «йо»
        assert_eq!(translit("yogurt", TranslitDirection::LatToRu), "ёгурт");
        assert_eq!(translit("mayor", TranslitDirection::LatToRu), "маёр");
        // Одиночная «y» перед гласной — «й», после согласной — «ы»
        assert_eq!(translit("may", TranslitDirection::LatToRu), "май");
        assert_eq!(translit("system", TranslitDirection::LatToRu), "сыстем");
    }

    #[test]
    fn test_translit_lat_to_ru_mixed_input() {
        // Кириллица, цифры и знаки в латинском тексте остаются как есть
        assert_eq!(
            translit("privet привет 123", TranslitDirection::LatToRu),
            "привет привет 123"
        );
        assert_eq!(translit("Nyu-York", TranslitDirection::LatToRu), "Ню-Ёрк");
        assert_eq!(translit("", TranslitDirection::LatToRu), "");
    }

    #[test]
    fn test_translit_round_trip() {
        // Круговой прогон без ъ/ь/э — исходник восстанавливается
        let words = [
            "привет",
            "мама",
            "спасибо",
            "хорошо",
            "щука",
            "ёжик",
            "язык",
            "журнал",
            "чай",
            "чипсы",
            "вода",
            "солнце",
            "сердце",
            "юла",
        ];
        for word in words {
            let latin = translit(word, TranslitDirection::RuToLat);
            let back = translit(&latin, TranslitDirection::LatToRu);
            assert_eq!(back, word, "{word} → {latin} → {back}");
        }
    }

    #[test]
    fn test_convert_single_letters() {
        // Перекодируются только односимвольные буквенные токены
        assert_eq!(
            convert_single_letters("ghbdtn z ghbitk", Layout::En, Layout::Ru),
            "ghbdtn я ghbitk"
        );
        assert_eq!(
            convert_single_letters("привет я пришел", Layout::Ru, Layout::En),
            "привет z пришел"
        );
        assert_eq!(convert_single_letters("Z", Layout::En, Layout::Ru), "Я");
        // Цифры, знаки и пробелы не трогаются, регистр сохраняется
        assert_eq!(
            convert_single_letters("1 + v = 5", Layout::En, Layout::Ru),
            "1 + м = 5"
        );
        assert_eq!(
            convert_single_letters("ab z", Layout::En, Layout::Ru),
            "ab я"
        );
        assert_eq!(convert_single_letters("", Layout::En, Layout::Ru), "");
    }

    #[test]
    fn test_detect_translit_direction() {
        assert_eq!(
            detect_translit_direction("привет"),
            Some(TranslitDirection::RuToLat)
        );
        assert_eq!(
            detect_translit_direction("Привет 123"),
            Some(TranslitDirection::RuToLat)
        );
        assert_eq!(
            detect_translit_direction("privet"),
            Some(TranslitDirection::LatToRu)
        );
        assert_eq!(detect_translit_direction("123 !?"), None);
        assert_eq!(detect_translit_direction(""), None);
        // Смесь поровну — не преобразовываем
        assert_eq!(detect_translit_direction("ghbdtn привет"), None);
    }
}
