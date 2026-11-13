//! Правила для приложений: где коррекцию выключать или оставлять только
//! по горячей клавише.
//!
//! Правило ссылается на **имя процесса** активного окна (`klavium.exe`,
//! `notepad.exe`) без учёта регистра и без расширения. Поддерживается `*`
//! как любой набор символов: `*code*` накроет `vscode`, `code`, `code-insiders`.
//!
//! Порядок важен: действует **первое** совпавшее правило, поэтому узкие
//! шаблоны ставятся выше широких.

use serde::{Deserialize, Serialize};

/// Что делать в приложении, подходящем под правило.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AppRuleMode {
    /// Не трогать: ни подсказок, ни автоисправления, ни реакции на хоткей.
    Off,
    /// Только по горячей клавише: слово анализируется и встаёт в очередь,
    /// но подсказка не показывается и автоисправление не срабатывает.
    Manual,
}

impl AppRuleMode {
    /// Подпись для интерфейса.
    #[must_use]
    pub const fn title(self) -> &'static str {
        match self {
            Self::Off => "Не исправлять",
            Self::Manual => "Только по хоткею",
        }
    }
}

/// Правило для одного приложения.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppRule {
    /// Имя процесса активного окна (`notepad`, `code`, `*team*`).
    pub pattern: String,
    /// Режим коррекции в этом приложении.
    pub mode: AppRuleMode,
}

impl AppRule {
    /// Создать правило.
    #[must_use]
    pub fn new(pattern: impl Into<String>, mode: AppRuleMode) -> Self {
        Self {
            pattern: normalize(pattern.into()),
            mode,
        }
    }

    /// Подходит ли процесс под правило.
    #[must_use]
    pub fn matches(&self, exe: &str) -> bool {
        let pattern = normalize(self.pattern.clone());
        if pattern.is_empty() {
            return false;
        }
        let exe = normalize(exe.to_string());
        glob_match(&pattern, &exe)
    }
}

/// Привести имя процесса к сравнимому виду: нижний регистр, без пути и `.exe`.
#[must_use]
pub fn normalize(name: impl Into<String>) -> String {
    let raw = name.into();
    let trimmed = raw.trim();
    // Путь вида `C:\Program Files\App\app.exe` → `app.exe`.
    let file = trimmed
        .rsplit(['\\', '/'])
        .next()
        .unwrap_or(trimmed)
        .to_lowercase();
    file.strip_suffix(".exe")
        .map_or_else(|| file.clone(), std::string::ToString::to_string)
}

/// Сопоставление с `*` (любая последовательность, включая пустую).
fn glob_match(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let t: Vec<char> = text.chars().collect();
    let (mut pi, mut ti) = (0usize, 0usize);
    // Последняя позиция `*` и место в тексте, где он начал «съедать».
    let mut star: Option<usize> = None;
    let mut star_ti = 0usize;

    while ti < t.len() {
        if pi < p.len() && (p[pi] == t[ti] || p[pi] == '?') {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            star_ti = ti;
            pi += 1;
        } else if let Some(star_pos) = star {
            pi = star_pos + 1;
            star_ti += 1;
            ti = star_ti;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

/// Режим для процесса: первое совпавшее правило или `None` (обычная работа).
#[must_use]
pub fn mode_for(rules: &[AppRule], exe: &str) -> Option<AppRuleMode> {
    rules.iter().find(|rule| rule.matches(exe)).map(|r| r.mode)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_strips_path_and_exe() {
        assert_eq!(normalize("C:\\Program Files\\App\\App.EXE"), "app");
        assert_eq!(normalize("notepad.exe"), "notepad");
        assert_eq!(normalize("  Code  "), "code");
    }

    #[test]
    fn exact_rule_matches() {
        let rule = AppRule::new("notepad", AppRuleMode::Off);
        assert!(rule.matches("notepad.exe"));
        assert!(!rule.matches("notepad++"));
    }

    #[test]
    fn wildcard_rule_matches() {
        let rule = AppRule::new("*code*", AppRuleMode::Manual);
        assert!(rule.matches("code"));
        assert!(rule.matches("vscode"));
        assert!(rule.matches("code-insiders"));
        assert!(!rule.matches("notepad"));
    }

    #[test]
    fn wildcard_edges() {
        let rule = AppRule::new("*term*", AppRuleMode::Off);
        assert!(rule.matches("terminal"));
        assert!(rule.matches("termius"));
        assert!(!rule.matches("notepad"));

        let rule = AppRule::new("win*", AppRuleMode::Off);
        assert!(rule.matches("winword"));
        assert!(!rule.matches("notepad"));
    }

    #[test]
    fn first_rule_wins() {
        let rules = vec![
            AppRule::new("code", AppRuleMode::Off),
            AppRule::new("*", AppRuleMode::Manual),
        ];
        assert_eq!(mode_for(&rules, "code"), Some(AppRuleMode::Off));
        assert_eq!(mode_for(&rules, "notepad"), Some(AppRuleMode::Manual));
        assert_eq!(mode_for(&[], "notepad"), None);
    }

    #[test]
    fn empty_pattern_ignored() {
        let rule = AppRule::new("   ", AppRuleMode::Off);
        assert!(!rule.matches("notepad"));
    }
}
