//! Типографика: кавычки, тире и многоточие под язык текста.
//!
//! Перекодировка раскладки работает по позициям клавиш и не знает, что
//! `"` в русском тексте — это «ёлочки», а дефис с пробелами — длинное тире.
//! Этот модуль доводит результат до привычного вида.
//!
//! Правила намеренно простые и предсказуемые: текст не переписывается
//! по смыслу, меняются только парные кавычки, отдельно стоящий дефис
//! и три точки подряд.

use crate::layout::Layout;

/// Привести типографику к языку текста.
///
/// Для русского: `"` → `«»` (по парности), ` - ` → ` — `, `...` → `…`.
/// Для английского — наоборот: `«»` → `"`, `—` → `-`, `…` → `...`.
/// Непарная кавычка остаётся как была; цифры и код не трогаются.
///
/// # Примеры
///
/// ```
/// use klavium_core::typography::typography;
/// use klavium_core::Layout;
///
/// assert_eq!(typography("\"привет\"", Layout::Ru), "«привет»");
/// assert_eq!(typography("слово - другое", Layout::Ru), "слово — другое");
/// assert_eq!(typography("«привет»", Layout::En), "\"привет\"");
/// ```
#[must_use]
pub fn typography(text: &str, layout: Layout) -> String {
    match layout {
        Layout::Ru => to_russian(text),
        Layout::En => to_english(text),
    }
}

/// Русская типографика: ёлочки, длинное тире, многоточие.
fn to_russian(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 8);
    let mut open_quote = true;
    let chars: Vec<char> = text.chars().collect();
    let mut index = 0;
    while index < chars.len() {
        let ch = chars[index];
        match ch {
            '"' => {
                out.push(if open_quote { '«' } else { '»' });
                open_quote = !open_quote;
            }
            '.' => {
                // Серия точек: многоточие только если их ровно три.
                let mut len = 1;
                while chars.get(index + len) == Some(&'.') {
                    len += 1;
                }
                if len == 3 {
                    out.push('…');
                } else {
                    for _ in 0..len {
                        out.push('.');
                    }
                }
                index += len;
                continue;
            }
            '-' if is_dash(&chars, index) => out.push('—'),
            _ => out.push(ch),
        }
        index += 1;
    }
    out
}

/// Английская типографика: прямые кавычки, дефис, три точки.
fn to_english(text: &str) -> String {
    text.chars()
        .map(|ch| match ch {
            '«' | '»' => '"',
            '—' | '–' => '-',
            '…' => '.',
            other => other,
        })
        .collect()
}

/// Дефис, который должен стать тире: стоит после пробела или в начале
/// текста (реплика диалога) и за ним — пробел.
fn is_dash(chars: &[char], at: usize) -> bool {
    let before = at.checked_sub(1).and_then(|i| chars.get(i));
    let after = chars.get(at + 1);
    let line_start = matches!(before, None | Some(&'\n'));
    let before_ok = line_start || matches!(before, Some(&c) if c == ' ');
    let after_space = matches!(after, Some(&c) if c == ' ');
    before_ok && after_space
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_russian_quotes_pairs() {
        assert_eq!(typography("\"привет\"", Layout::Ru), "«привет»");
        assert_eq!(
            typography("\"один\" и \"два\"", Layout::Ru),
            "«один» и «два»"
        );
        // Непарная кавычка становится открывающей и остаётся одна.
        assert_eq!(typography("\"привет", Layout::Ru), "«привет");
    }

    #[test]
    fn test_russian_dash_and_ellipsis() {
        assert_eq!(typography("слово - другое", Layout::Ru), "слово — другое");
        // Реплика диалога: тире в начале текста.
        assert_eq!(typography("- Ах, привет", Layout::Ru), "— Ах, привет");
        // Дефис внутри слова не трогаем.
        assert_eq!(
            typography("интернет-магазин", Layout::Ru),
            "интернет-магазин"
        );
        assert_eq!(typography("подожди...", Layout::Ru), "подожди…");
        assert_eq!(typography("подожди....", Layout::Ru), "подожди....");
    }

    #[test]
    fn test_english_typography() {
        assert_eq!(typography("«привет»", Layout::En), "\"привет\"");
        assert_eq!(typography("слово — другое", Layout::En), "слово - другое");
        assert_eq!(typography("подожди…", Layout::En), "подожди.");
    }

    #[test]
    fn test_plain_text_unchanged() {
        let text = "обычный текст без кавычек, тире и прочего 123";
        assert_eq!(typography(text, Layout::Ru), text);
        assert_eq!(typography(text, Layout::En), text);
    }
}
