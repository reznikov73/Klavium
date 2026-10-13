//! Журналирование.
//!
//! Файл: `logs\app.log` в каталоге данных приложения.
//! Уровень — `logging.level` (DEBUG/INFO/WARNING/ERROR).

use std::path::Path;

use klavium_core::config::AppConfig;

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

    let appender = tracing_appender::rolling::never(logs_dir, "app.log");
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
