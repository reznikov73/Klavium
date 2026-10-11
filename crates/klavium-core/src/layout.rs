//! Раскладка клавиатуры.

use std::fmt;

/// Раскладка клавиатуры (типизированное представление).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Layout {
    /// Английская раскладка (QWERTY).
    En,
    /// Русская раскладка (ЙЦУКЕН).
    Ru,
}

impl Layout {
    /// Строковое имя раскладки (`"en"` / `"ru"`).
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::En => "en",
            Self::Ru => "ru",
        }
    }

    /// Противоположная раскладка.
    #[must_use]
    pub const fn opposite(self) -> Self {
        match self {
            Self::En => Self::Ru,
            Self::Ru => Self::En,
        }
    }
}

impl fmt::Display for Layout {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}
