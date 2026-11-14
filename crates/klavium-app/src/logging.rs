//! Журналирование.
//!
//! Файл: `logs\app.log` в каталоге данных приложения.
//! Уровень — `logging.level` (DEBUG/INFO/WARNING/ERROR).
//!
//! Файл ограничен по размеру: при превышении лимита он уходит в `app.log.1`,
//! прежние архивы сдвигаются, лишние удаляются. Без этого при уровне DEBUG
//! журнал растёт без предела — месяцы работы дают сотни мегабайт.

use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use klavium_core::config::AppConfig;

/// Предел размера текущего журнала: 5 МБ.
const LOG_LIMIT_BYTES: u64 = 5 * 1024 * 1024;
/// Сколько архивных файлов держим (`app.log.1` … `app.log.3`).
const LOG_KEEP_FILES: usize = 3;

/// Писатель журнала с ротацией по размеру.
///
/// Реализует [`Write`], поэтому подходит для `tracing_appender::non_blocking`:
/// запись идёт в фоновом потоке, а ротация случается на границе сообщения.
pub struct RotatingWriter {
    path: PathBuf,
    limit: u64,
    keep: usize,
    file: Option<File>,
    written: u64,
}

impl RotatingWriter {
    /// Открыть журнал (файл создаётся при необходимости).
    ///
    /// Существующий файл дополняется: размер учитывается, чтобы ротация
    /// сработала и после перезапуска, а не только в новом файле.
    #[must_use]
    pub fn new(path: PathBuf, limit: u64, keep: usize) -> Self {
        let mut writer = Self {
            path,
            limit,
            keep: keep.max(1),
            file: None,
            written: 0,
        };
        let _ = writer.open();
        writer
    }

    fn open(&mut self) -> io::Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        self.written = file.metadata().map_or(0, |meta| meta.len());
        self.file = Some(file);
        Ok(())
    }

    /// Сдвинуть архивы и начать новый файл: `app.log` → `app.log.1`.
    fn rotate(&mut self) {
        self.file = None;
        for index in (1..self.keep).rev() {
            let from = archive_path(&self.path, index);
            let to = archive_path(&self.path, index + 1);
            if from.exists() {
                let _ = std::fs::remove_file(&to);
                let _ = std::fs::rename(&from, &to);
            }
        }
        let first = archive_path(&self.path, 1);
        let _ = std::fs::remove_file(&first);
        let _ = std::fs::rename(&self.path, &first);
        let _ = self.open();
    }
}

/// Имя архивного файла: `app.log` → `app.log.N`.
fn archive_path(path: &Path, index: usize) -> PathBuf {
    let mut name = path.file_name().map_or_else(
        || std::ffi::OsString::from("app.log"),
        std::ffi::OsStr::to_os_string,
    );
    name.push(format!(".{index}"));
    path.with_file_name(name)
}

impl Write for RotatingWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if self.file.is_none() {
            self.open()?;
        }
        if self.written + buf.len() as u64 > self.limit {
            self.rotate();
        }
        let Some(file) = self.file.as_mut() else {
            return Ok(buf.len());
        };
        let written = file.write(buf)?;
        self.written += written as u64;
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.file.as_mut().map_or(Ok(()), std::io::Write::flush)
    }
}

/// Инициализировать журнал (вызывается один раз на старте).
///
/// # Errors
///
/// Ошибка создания каталога журнала.
pub fn init(config: &AppConfig, logs_dir: &Path) -> anyhow::Result<()> {
    if !config.logging.enabled {
        return Ok(());
    }

    std::fs::create_dir_all(logs_dir)?;

    let appender = RotatingWriter::new(logs_dir.join("app.log"), LOG_LIMIT_BYTES, LOG_KEEP_FILES);
    let (writer, guard) = tracing_appender::non_blocking(appender);

    let (level, level_name) = match config.logging.level.to_uppercase().as_str() {
        "DEBUG" => (tracing::Level::DEBUG, "debug"),
        "INFO" => (tracing::Level::INFO, "info"),
        "WARNING" | "WARN" => (tracing::Level::WARN, "warn"),
        _ => (tracing::Level::ERROR, "error"),
    };

    // Свои сообщения — по уровню из настроек; чужие (ureq, rustls, egui,
    // glutin и т.п.) — только WARNING, чтобы не засорять журнал.
    let filter = tracing_subscriber::EnvFilter::new(format!(
        "warn,klavium={level_name},klavium_app={level_name},klavium_core={level_name},klavium_platform={level_name},klavium_ui={level_name}"
    ));

    let _ = tracing_subscriber::fmt()
        .with_writer(writer)
        .with_ansi(false)
        .with_target(false)
        .with_max_level(level)
        .with_env_filter(filter)
        .try_init();

    // Держим писателя живым на всё время работы процесса.
    Box::leak(Box::new(guard));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("klavium-logtest-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("тестовый каталог");
        dir
    }

    #[test]
    fn test_rotates_by_size() {
        let dir = temp_dir("rotate");
        let path = dir.join("app.log");
        let mut writer = RotatingWriter::new(path.clone(), 100, 3);

        writer.write_all(&[b'x'; 90]).expect("запись");
        writer.flush().expect("сброс");
        writer.write_all(&[b'y'; 90]).expect("запись");
        writer.flush().expect("сброс");

        assert!(path.exists(), "текущий файл на месте");
        assert!(archive_path(&path, 1).exists(), "старый ушёл в .1");
        let current = std::fs::metadata(&path).expect("метаданные").len();
        assert!(current <= 100, "текущий файл не больше лимита: {current}");
    }

    #[test]
    fn test_keeps_only_configured_archives() {
        let dir = temp_dir("keep");
        let path = dir.join("app.log");
        let mut writer = RotatingWriter::new(path.clone(), 10, 2);

        for _ in 0..6 {
            writer.write_all(b"0123456789").expect("запись");
        }
        writer.flush().expect("сброс");

        assert!(path.exists());
        assert!(archive_path(&path, 1).exists());
        assert!(archive_path(&path, 2).exists());
        assert!(!archive_path(&path, 3).exists(), "лишние архивы не держим");
    }

    #[test]
    fn test_existing_file_size_counts_after_restart() {
        let dir = temp_dir("append");
        let path = dir.join("app.log");
        std::fs::write(&path, vec![b'z'; 60]).expect("подготовка");

        let mut writer = RotatingWriter::new(path.clone(), 100, 2);
        writer.write_all(&[b'z'; 50]).expect("запись");
        writer.flush().expect("сброс");

        assert!(
            archive_path(&path, 1).exists(),
            "60 + 50 больше лимита — ротация"
        );
    }
}
