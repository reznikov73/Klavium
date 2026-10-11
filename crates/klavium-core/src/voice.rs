//! Обработка распознанного текста.
//!
//! Чистые функции без Win32: голосовая пунктуация, команды управления,
//! автозаглавные буквы. Особенности (для живого ASR):
//! - команды пунктуации ищутся регистронезависимо (иначе «Запятая»
//!   в начале фразы не заменялась бы);
//! - границы слов — по Unicode-алфавитно-цифровым символам, а не по
//!   классу `[а-яёa-z0-9]` (заглавные буквы — тоже границы).

/// Голосовые команды пунктуации (длинные фразы раньше коротких).
pub const PUNCT_COMMANDS: &[(&str, &str)] = &[
    ("восклицательный знак", "!"),
    ("вопросительный знак", "?"),
    ("точка с запятой", ";"),
    ("скобка открывается", "("),
    ("скобка закрывается", ")"),
    ("новая строка", "\n"),
    ("открывающая кавычка", "\""),
    ("закрывающая кавычка", "\""),
    ("запятая", ","),
    ("точка", "."),
    ("двоеточие", ":"),
    ("тире", " — "),
];

/// Команды остановки диктовки (точное совпадение фразы).
pub const CMD_STOP: &[&str] = &[
    "стоп",
    "останови",
    "выключи",
    "закончить диктовку",
    "конец диктовки",
];

/// Команды паузы (распознавание продолжается, текст не вставляется).
pub const CMD_PAUSE: &[&str] = &["пауза", "поставь на паузу", "подожди"];

/// Команды продолжения после паузы.
pub const CMD_RESUME: &[&str] = &["продолжить", "продолжай", "начать распознавание", "старт"];

/// Описание модели whisper в каталоге точности.
#[derive(Debug, Clone, Copy)]
pub struct ModelSpec {
    /// Имя файла (как ждёт whisper.cpp).
    pub file: &'static str,
    /// Название уровня (для UI).
    pub title: &'static str,
    /// Примерный размер файла.
    pub size: &'static str,
    /// Короткое пояснение.
    pub hint: &'static str,
}

/// Каталог моделей по уровню точности (от быстрой к максимальной).
pub const MODELS: &[ModelSpec] = &[
    ModelSpec {
        file: "ggml-tiny.bin",
        title: "Быстрая (tiny)",
        size: "75 МБ",
        hint: "черновая точность",
    },
    ModelSpec {
        file: "ggml-base.bin",
        title: "Базовая (base)",
        size: "142 МБ",
        hint: "немного точнее tiny",
    },
    ModelSpec {
        file: "ggml-small.bin",
        title: "Средняя (small)",
        size: "466 МБ",
        hint: "баланс скорости и точности",
    },
    ModelSpec {
        file: "ggml-medium.bin",
        title: "Высокая (medium)",
        size: "1,5 ГБ",
        hint: "хорошая точность, заметно медленнее",
    },
    ModelSpec {
        file: "ggml-large-v3-turbo.bin",
        title: "Максимум (large-v3-turbo)",
        size: "≈1,6 ГБ",
        hint: "лучшая точность для русского",
    },
];

/// Состояние загрузки модели (наполняет загрузчик, читает UI).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DownloadStatus {
    /// Загрузка идёт.
    pub active: bool,
    /// Имя файла, который качается/скачался.
    pub file: String,
    /// Прогресс 0..=100 (`None` — неизвестен).
    pub percent: Option<u8>,
    /// Ошибка (если была).
    pub error: Option<String>,
    /// Загрузка завершена успешно.
    pub done: bool,
}

/// Состояние теста распознавания (наполняет сервис, читает окно настроек).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RecognitionTest {
    /// Тест идёт.
    pub running: bool,
    /// Запрошен и ждёт готовности движка (стартует автоматически).
    pub waiting_engine: bool,
    /// Осталось секунд (при активном тесте).
    pub seconds_left: u32,
    /// Распознанные фразы (по порядку).
    pub results: Vec<String>,
    /// Ошибка запуска/работы теста.
    pub error: Option<String>,
}

/// Слова-маркеры типичных галлюцинаций whisper (фразы из обучающих субтитров,
/// всплывают на тишине/шуме).
const HALLUCINATION_MARKERS: &[&str] = &[
    "редактор субтитров",
    "синецкая",
    "сухиашвили",
    "dimatorzok",
    "dima torzok",
    "продолжение следует",
    "спасибо за просмотр",
    "подпишись на канал",
    "подписывайтесь на канал",
    "ставьте лайк",
    "не забудь подписаться",
    "субтитры создавал",
    "субтитры создавала",
    "субтитры подготовил",
];

/// Похоже ли распознанное на галлюцинацию/шум (титры-клише, скобочные вставки,
/// одни знаки без слов). Такие результаты не вставляем.
#[must_use]
pub fn is_likely_hallucination(text: &str) -> bool {
    let lower = text.trim().to_lowercase();
    if lower.is_empty() {
        return true;
    }
    if HALLUCINATION_MARKERS
        .iter()
        .any(|marker| lower.contains(marker))
    {
        return true;
    }
    // Если после выкидывания вставок в скобках не осталось букв/цифр
    // (например, «[Смех]», «♪», «, .») — это шум, а не речь.
    let without_brackets = remove_bracketed(&lower);
    !without_brackets.chars().any(char::is_alphanumeric)
}

/// Убрать содержимое групп в скобках (круглых/квадратных/фигурных).
fn remove_bracketed(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut depth = 0_usize;
    for c in text.chars() {
        match c {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth = depth.saturating_sub(1),
            _ => {
                if depth == 0 {
                    out.push(c);
                }
            }
        }
    }
    out
}

/// Распознанная голосовая команда.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VoiceCommand {
    /// Остановить диктовку.
    Stop,
    /// Поставить на паузу.
    Pause,
    /// Продолжить после паузы.
    Resume,
}

/// Классифицировать фразу как команду (регистр и хвостовые `.!?` не важны).
#[must_use]
pub fn classify_command(text: &str) -> Option<VoiceCommand> {
    let normalized = normalize_command(text);
    if CMD_STOP.contains(&normalized.as_str()) {
        return Some(VoiceCommand::Stop);
    }
    if CMD_PAUSE.contains(&normalized.as_str()) {
        return Some(VoiceCommand::Pause);
    }
    if CMD_RESUME.contains(&normalized.as_str()) {
        return Some(VoiceCommand::Resume);
    }
    None
}

fn normalize_command(text: &str) -> String {
    let lowered = text.trim().to_lowercase();
    lowered
        .trim_end_matches(|c: char| c.is_whitespace() || matches!(c, '.' | '!' | '?'))
        .trim_end()
        .to_string()
}

/// Заменить голосовые команды пунктуации на знаки и расставить пробелы
/// вокруг знаков.
#[must_use]
pub fn apply_punctuation(text: &str) -> String {
    let mut result = text.to_string();
    for (phrase, symbol) in PUNCT_COMMANDS {
        result = replace_phrase_all(&result, phrase, symbol);
    }
    normalize_punctuation_spacing(&result)
}

/// Заменить все вхождения фразы (слова через любой пробел, регистр не важен,
/// границы — не алфавитно-цифровые символы) на `symbol`.
fn replace_phrase_all(text: &str, phrase: &str, symbol: &str) -> String {
    let words: Vec<&str> = phrase.split_whitespace().collect();
    if words.is_empty() {
        return text.to_string();
    }
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut index = 0;
    while index < chars.len() {
        let left_ok = index == 0 || !chars[index - 1].is_alphanumeric();
        if left_ok {
            if let Some(end) = match_phrase_at(&chars, index, &words) {
                let right_ok = end == chars.len() || !chars[end].is_alphanumeric();
                if right_ok {
                    out.push_str(symbol);
                    index = end;
                    continue;
                }
            }
        }
        out.push(chars[index]);
        index += 1;
    }
    out
}

/// Попытаться сопоставить фразу (слова через `\s+`) начиная с позиции `start`;
/// вернуть индекс сразу после совпадения.
fn match_phrase_at(chars: &[char], start: usize, words: &[&str]) -> Option<usize> {
    let mut index = start;
    for (word_index, word) in words.iter().enumerate() {
        if word_index > 0 {
            let spaces_start = index;
            while index < chars.len() && chars[index].is_whitespace() {
                index += 1;
            }
            if index == spaces_start {
                return None;
            }
        }
        for word_char in word.chars() {
            if index >= chars.len() || !chars_equal_ignore_case(chars[index], word_char) {
                return None;
            }
            index += 1;
        }
    }
    Some(index)
}

fn chars_equal_ignore_case(a: char, b: char) -> bool {
    a.to_lowercase().eq(b.to_lowercase())
}

/// Привести пробелы вокруг знаков к каноничному виду.
fn normalize_punctuation_spacing(text: &str) -> String {
    let after_before = remove_space_before_punct(text);
    let after_after = add_space_after_punct(&after_before);
    let after_dedup = collapse_duplicate_punct(&after_after);
    collapse_spaces(&after_dedup)
}

const fn is_punct(c: char) -> bool {
    matches!(c, ',' | '.' | '!' | '?' | ';' | ':')
}

/// `\s+([,.!?;:])` → `\1`: убрать пробелы перед знаком.
fn remove_space_before_punct(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut index = 0;
    while index < chars.len() {
        if chars[index].is_whitespace() {
            let mut lookahead = index;
            while lookahead < chars.len() && chars[lookahead].is_whitespace() {
                lookahead += 1;
            }
            if lookahead < chars.len() && is_punct(chars[lookahead]) {
                index = lookahead;
                continue;
            }
        }
        out.push(chars[index]);
        index += 1;
    }
    out
}

/// `([,.!?;:])(?=[^\s\d\n])` → `\1 `: пробел после знака, если дальше не
/// пробел и не цифра.
fn add_space_after_punct(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    for (index, &c) in chars.iter().enumerate() {
        out.push(c);
        if is_punct(c) {
            if let Some(&next) = chars.get(index + 1) {
                if !next.is_whitespace() && !next.is_numeric() {
                    out.push(' ');
                }
            }
        }
    }
    out
}

/// `([,.!?;:])\s*\1+` → `\1`: схлопнуть повторы одного знака.
fn collapse_duplicate_punct(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut index = 0;
    while index < chars.len() {
        let c = chars[index];
        if !is_punct(c) {
            out.push(c);
            index += 1;
            continue;
        }
        out.push(c);
        let mut next = index + 1;
        loop {
            let mut lookahead = next;
            while lookahead < chars.len() && chars[lookahead].is_whitespace() {
                lookahead += 1;
            }
            if lookahead < chars.len() && chars[lookahead] == c {
                while lookahead < chars.len() && chars[lookahead] == c {
                    lookahead += 1;
                }
                next = lookahead;
            } else {
                break;
            }
        }
        index = next;
    }
    out
}

/// `[ \t]{2,}` → ` `: схлопнуть подряд идущие пробелы/табы.
fn collapse_spaces(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut previous_was_space = false;
    for c in text.chars() {
        if c == ' ' || c == '\t' {
            if !previous_was_space {
                out.push(' ');
            }
            previous_was_space = true;
        } else {
            previous_was_space = false;
            out.push(c);
        }
    }
    out
}

/// Сделать первую букву заглавной, если предыдущий значимый символ —
/// начало текста, точка, `!`, `?` или перевод строки.
#[must_use]
pub fn capitalize_after(last_char: Option<char>, text: &str) -> String {
    if !matches!(last_char, None | Some('.' | '!' | '?' | '\n')) {
        return text.to_string();
    }
    let Some((index, first)) = text.char_indices().find(|(_, c)| c.is_alphabetic()) else {
        return text.to_string();
    };
    let mut result = String::with_capacity(text.len());
    result.push_str(&text[..index]);
    result.extend(first.to_uppercase());
    result.push_str(&text[index + first.len_utf8()..]);
    result
}

/// Последний значимый (не пробельный) символ текста.
#[must_use]
pub fn last_significant_char(text: &str) -> Option<char> {
    text.chars().rev().find(|c| !c.is_whitespace())
}

/// Добавить пробел в конце фразы, если он нужен (чтобы фразы
/// не слипались при вставке).
#[must_use]
pub fn ensure_trailing_space(text: &str) -> String {
    match text.chars().last() {
        None => text.to_string(),
        Some(last) if last.is_whitespace() || matches!(last, '.' | '!' | '?' | ':' | ';' | '"') => {
            text.to_string()
        }
        Some(_) => format!("{text} "),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_punct_simple() {
        assert_eq!(apply_punctuation("привет запятая мир"), "привет, мир");
        assert_eq!(apply_punctuation("это точка"), "это.");
    }

    #[test]
    fn test_punct_longest_first() {
        // «точка с запятой» не должна развалиться на «точка» + «запятой»
        assert_eq!(
            apply_punctuation("привет точка с запятой мир"),
            "привет; мир"
        );
        assert_eq!(apply_punctuation("ура восклицательный знак"), "ура!");
        assert_eq!(apply_punctuation("да вопросительный знак"), "да?");
        assert_eq!(apply_punctuation("стоп точка точка"), "стоп.");
    }

    #[test]
    fn test_punct_case_insensitive_and_spacing() {
        assert_eq!(apply_punctuation("Привет Запятая мир"), "Привет, мир");
        assert_eq!(apply_punctuation("слово   с   запятой"), "слово с запятой");
        assert_eq!(
            apply_punctuation("слово точка с запятой конец"),
            "слово; конец"
        );
        // лишние пробелы вокруг знаков чистятся
        assert_eq!(
            apply_punctuation("конец точка  следующий"),
            "конец. следующий"
        );
    }

    #[test]
    fn test_punct_boundaries() {
        // внутри слова/после буквы команда не срабатывает
        assert_eq!(apply_punctuation("запятаянье"), "запятаянье");
        assert_eq!(apply_punctuation("вточка"), "вточка");
    }

    #[test]
    fn test_punct_dedup() {
        assert_eq!(apply_punctuation("итог точка точка"), "итог.");
        assert_eq!(apply_punctuation("итог точка  точка точка"), "итог.");
    }

    #[test]
    fn test_punct_dash_and_newline() {
        assert_eq!(apply_punctuation("он тире молодец"), "он — молодец");
        assert_eq!(
            apply_punctuation("строка новая строка два"),
            "строка \n два"
        );
    }

    #[test]
    fn test_classify_commands() {
        assert_eq!(classify_command("Стоп!"), Some(VoiceCommand::Stop));
        assert_eq!(classify_command(" останови... "), Some(VoiceCommand::Stop));
        assert_eq!(
            classify_command("Поставь на паузу"),
            Some(VoiceCommand::Pause)
        );
        assert_eq!(classify_command("продолжай"), Some(VoiceCommand::Resume));
        assert_eq!(classify_command("старт."), Some(VoiceCommand::Resume));
        assert_eq!(classify_command("привет"), None);
    }

    #[test]
    fn test_capitalize() {
        assert_eq!(capitalize_after(None, "привет мир"), "Привет мир");
        assert_eq!(capitalize_after(Some('.'), "пока"), "Пока");
        assert_eq!(capitalize_after(Some('!'), "ну и ну"), "Ну и ну");
        assert_eq!(capitalize_after(Some('\n'), "строчка"), "Строчка");
        // после обычной буквы — не трогаем
        assert_eq!(capitalize_after(Some('а'), "пока"), "пока");
        // без букв — не трогаем
        assert_eq!(capitalize_after(None, "123"), "123");
    }

    #[test]
    fn test_hallucination_filter() {
        // каноническая галлюцинация whisper на тишине
        assert!(is_likely_hallucination(
            ".Редактор субтитров А. Синецкая Корректор А. Сухиашвили"
        ));
        assert!(is_likely_hallucination("Продолжение следует"));
        assert!(is_likely_hallucination("[Смех]"));
        assert!(is_likely_hallucination("♪"));
        assert!(is_likely_hallucination(", ."));
        assert!(is_likely_hallucination("   "));
        // нормальная речь не отфильтровывается
        assert!(!is_likely_hallucination("привет, как дела?"));
        assert!(!is_likely_hallucination("включи субтитры"));
        assert!(!is_likely_hallucination("это тест диктовки"));
    }

    #[test]
    fn test_trailing_space_and_last_char() {
        assert_eq!(ensure_trailing_space("привет"), "привет ");
        assert_eq!(ensure_trailing_space("привет!"), "привет!");
        assert_eq!(ensure_trailing_space("привет "), "привет ");
        assert_eq!(last_significant_char("привет "), Some('т'));
        assert_eq!(last_significant_char("   "), None);
    }
}
