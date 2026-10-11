//! Файловые операции: каталог данных и конфигурация.
//!
//! Каталог данных (config.json, stats.json, logs\, models\) по умолчанию —
//! портативный, рядом с программой: `<папка программы>\data`. Порядок выбора:
//! 1. переменная `KLAVIUM_DATA_DIR` (тесты и спецзапуски);
//! 2. указатель `%APPDATA%\Klavium\location.txt` (папка, выбранная в настройках);
//! 3. `<папка программы>\data` — если она доступна для записи (с разовой
//!    миграцией данных из `%APPDATA%\Klavium`);
//! 4. `%APPDATA%\Klavium` — если рядом с программой писать нельзя.

use std::path::{Path, PathBuf};

use klavium_core::config::AppConfig;

/// Каталог данных и связанные пути.
#[derive(Debug, Clone)]
pub struct DataPaths {
    /// Активный каталог данных.
    pub data_dir: PathBuf,
    /// Путь конфигурации (`data_dir\config.json`).
    pub config_path: PathBuf,
    /// Пояснение для UI/журнала (миграция, fallback и т.п.).
    pub note: Option<String>,
}

impl DataPaths {
    fn new(data_dir: PathBuf, note: Option<String>) -> Self {
        let config_path = data_dir.join("config.json");
        Self {
            data_dir,
            config_path,
            note,
        }
    }

    /// Каталог журналов (`data_dir\logs`).
    #[must_use]
    pub fn logs_dir(&self) -> PathBuf {
        self.data_dir.join("logs")
    }

    /// Путь статистики (`data_dir\stats.json`).
    #[must_use]
    pub fn stats_path(&self) -> PathBuf {
        self.data_dir.join("stats.json")
    }

    /// Каталог моделей (`data_dir\models`).
    #[must_use]
    pub fn models_dir(&self) -> PathBuf {
        self.data_dir.join("models")
    }
}

/// Подпапка данных рядом с программой.
const DATA_SUBDIR: &str = "data";
/// Файл-указатель на выбранную вручную папку данных.
const POINTER_FILE: &str = "location.txt";

/// Старое/резервное место данных (`%APPDATA%\Klavium`).
#[must_use]
pub fn legacy_dir() -> PathBuf {
    std::env::var_os("APPDATA")
        .map_or_else(std::env::temp_dir, PathBuf::from)
        .join("Klavium")
}

/// Каталог данных рядом с программой (`<exe>\data`).
#[must_use]
pub fn portable_dir() -> Option<PathBuf> {
    std::env::current_exe()
        .ok()?
        .parent()
        .map(|parent| parent.join(DATA_SUBDIR))
}

/// Определить активный каталог данных (правила — в шапке файла).
#[must_use]
pub fn resolve() -> DataPaths {
    resolve_from(
        std::env::var_os("KLAVIUM_DATA_DIR").map(PathBuf::from),
        pointer_dir(),
        portable_dir(),
        &legacy_dir(),
    )
}

/// Каталог, выбранный в настройках (указатель), если он существует.
fn pointer_dir() -> Option<PathBuf> {
    let text = std::fs::read_to_string(legacy_dir().join(POINTER_FILE)).ok()?;
    let dir = PathBuf::from(text.trim());
    (dir.is_absolute() && dir.is_dir()).then_some(dir)
}

/// Тестируемая сердцевина [`resolve`].
fn resolve_from(
    env_dir: Option<PathBuf>,
    pointer: Option<PathBuf>,
    portable: Option<PathBuf>,
    legacy: &Path,
) -> DataPaths {
    if let Some(dir) = env_dir {
        return DataPaths::new(dir, None);
    }
    if let Some(dir) = pointer {
        return DataPaths::new(dir, None);
    }
    let Some(portable) = portable else {
        return DataPaths::new(legacy.to_path_buf(), None);
    };
    if portable.join("config.json").is_file() {
        return DataPaths::new(portable, None);
    }
    if ensure_writable_dir(&portable) {
        if legacy.join("config.json").is_file() {
            return match copy_data(legacy, &portable) {
                Ok(()) => DataPaths::new(
                    portable,
                    Some(format!("данные перенесены из {}", legacy.display())),
                ),
                Err(error) => DataPaths::new(
                    legacy.to_path_buf(),
                    Some(format!(
                        "не удалось перенести данные ({error}) — используется {}",
                        legacy.display()
                    )),
                ),
            };
        }
        return DataPaths::new(portable, None);
    }
    DataPaths::new(
        legacy.to_path_buf(),
        Some("папка рядом с программой недоступна для записи — используется %APPDATA%".to_string()),
    )
}

/// Убедиться, что каталог существует и доступен для записи (создать при необходимости).
fn ensure_writable_dir(dir: &Path) -> bool {
    if std::fs::create_dir_all(dir).is_err() {
        return false;
    }
    let probe = dir.join(".write_probe");
    if std::fs::write(&probe, b"1").is_err() {
        return false;
    }
    let _ = std::fs::remove_file(&probe);
    true
}

/// Скопировать содержимое каталога данных (без указателя; существующее не перезаписываем).
fn copy_data(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        if entry.file_name() == std::ffi::OsStr::new(POINTER_FILE) {
            continue;
        }
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            if !target.exists() {
                copy_dir_recursive(&entry.path(), &target)?;
            }
        } else if !target.exists() {
            std::fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

/// Рекурсивное копирование каталога (существующие файлы не трогаем).
fn copy_dir_recursive(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir_recursive(&entry.path(), &target)?;
        } else if !target.exists() {
            std::fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

/// Сменить каталог данных: скопировать файлы и записать (или убрать) указатель.
///
/// `None` — вернуть каталог по умолчанию (рядом с программой).
/// Старое содержимое не удаляется — остаётся как резервная копия.
///
/// # Errors
///
/// Ошибка создания/записи каталога, копирования файлов или указателя.
pub fn set_data_dir(current: &Path, target: Option<&Path>) -> anyhow::Result<PathBuf> {
    let new_dir = match target {
        Some(dir) => dir.to_path_buf(),
        None => portable_dir()
            .ok_or_else(|| anyhow::anyhow!("не удалось определить папку программы"))?,
    };
    anyhow::ensure!(new_dir.is_absolute(), "нужен абсолютный путь");
    anyhow::ensure!(ensure_writable_dir(&new_dir), "папка недоступна для записи");
    if new_dir != current {
        copy_data(current, &new_dir)?;
    }
    let pointer = legacy_dir().join(POINTER_FILE);
    let is_portable_default = portable_dir().is_some_and(|portable| portable == new_dir);
    if target.is_some() && !is_portable_default {
        std::fs::create_dir_all(legacy_dir())?;
        std::fs::write(&pointer, new_dir.to_string_lossy().as_bytes())?;
    } else {
        let _ = std::fs::remove_file(&pointer);
    }
    Ok(new_dir)
}

/// Загрузить конфигурацию; при отсутствии/ошибке — умолчания.
#[must_use]
pub fn load(path: &std::path::Path) -> AppConfig {
    let Ok(text) = std::fs::read_to_string(path) else {
        return AppConfig::default();
    };
    serde_json::from_str(&text).unwrap_or_else(|error| {
        eprintln!(
            "[config] ошибка разбора {}: {error}; используются умолчания",
            path.display()
        );
        AppConfig::default()
    })
}

/// Сохранить конфигурацию (pretty JSON; создаёт каталог при необходимости).
///
/// # Errors
///
/// Ошибка сериализации или записи файла.
pub fn save(path: &std::path::Path, config: &AppConfig) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let text = serde_json::to_string_pretty(config)?;
    std::fs::write(path, text)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("klavium-pathtest-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("тестовый каталог");
        dir
    }

    #[test]
    fn test_env_wins() {
        let base = temp("env");
        let env_dir = base.join("env-data");
        let paths = resolve_from(
            Some(env_dir.clone()),
            None,
            Some(base.join("portable")),
            &base.join("legacy"),
        );
        assert_eq!(paths.data_dir, env_dir);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn test_pointer_wins_over_portable() {
        let base = temp("pointer");
        let chosen = base.join("chosen");
        std::fs::create_dir_all(&chosen).unwrap();
        let portable = base.join("portable");
        std::fs::create_dir_all(&portable).unwrap();
        std::fs::write(portable.join("config.json"), "{}").unwrap();
        let paths = resolve_from(
            None,
            Some(chosen.clone()),
            Some(portable),
            &base.join("legacy"),
        );
        assert_eq!(paths.data_dir, chosen);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn test_migration_from_legacy() {
        let base = temp("migration");
        let legacy = base.join("legacy");
        std::fs::create_dir_all(legacy.join("logs")).unwrap();
        std::fs::write(legacy.join("config.json"), "{}").unwrap();
        std::fs::write(legacy.join("stats.json"), "{}").unwrap();
        std::fs::write(legacy.join("logs").join("app.log"), "x").unwrap();

        let portable = base.join("portable");
        let paths = resolve_from(None, None, Some(portable.clone()), &legacy);
        assert_eq!(paths.data_dir, portable);
        assert!(paths
            .note
            .as_deref()
            .is_some_and(|note| note.starts_with("данные перенесены")));
        assert!(portable.join("config.json").is_file());
        assert!(portable.join("logs").join("app.log").is_file());
        // старое место не трогаем — остаётся резервом
        assert!(legacy.join("config.json").is_file());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn test_fallback_when_portable_unwritable() {
        let base = temp("fallback");
        // «портативный каталог» — файл: создать каталог на его месте нельзя
        let portable = base.join("portable-file");
        std::fs::write(&portable, "x").unwrap();
        let legacy = base.join("legacy");
        std::fs::create_dir_all(&legacy).unwrap();
        let paths = resolve_from(None, None, Some(portable), &legacy);
        assert_eq!(paths.data_dir, legacy);
        assert!(paths.note.is_some());
        let _ = std::fs::remove_dir_all(&base);
    }
}
