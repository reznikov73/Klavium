//! Связка движка с UI и сервисами: события → карточка, звук, статистика, журнал.

use std::sync::{mpsc, Arc, PoisonError, RwLock};
use std::time::Duration;

use klavium_core::config::AppConfig;
use klavium_core::Layout;
use klavium_platform::overlay::{CardRequest, OverlayHandle};

use crate::engine::{EngineConfig, EngineEvent};
use crate::sound::{SoundKind, SoundService};
use crate::stats::StatsService;

/// Разделяемая конфигурация приложения (обновляется окном настроек).
pub type SharedConfig = Arc<RwLock<AppConfig>>;

/// Сервисы, нужные обработчику событий движка.
pub struct Services {
    /// Статистика.
    pub stats: Arc<StatsService>,
    /// Звуки.
    pub sound: Arc<SoundService>,
    /// Конфигурация.
    pub config: SharedConfig,
}

/// Собрать конфигурацию движка из настроек приложения.
#[must_use]
pub fn engine_config(config: &AppConfig) -> EngineConfig {
    EngineConfig {
        enabled: config.correction.enabled,
        auto_correct: config.correction.auto_correct,
        threshold: config.correction.threshold,
        hotkey_correct: config.correction.hotkey_correct.clone(),
        word_timeout: Duration::from_secs_f64(config.timeouts.word_start.max(0.1)),
        run_jobs_inline: false,
    }
}

/// Поток обработки событий движка (журнал + оверлей + сервисы).
pub fn spawn_engine_event_loop(
    events: mpsc::Receiver<EngineEvent>,
    overlay: Arc<OverlayHandle>,
    services: Services,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        while let Ok(event) = events.recv() {
            handle_event(event, &overlay, &services);
        }
    })
}

/// Обработать событие движка.
pub fn handle_event(event: EngineEvent, overlay: &OverlayHandle, services: &Services) {
    match event {
        EngineEvent::WordStarted { layout } => {
            let statistics_enabled = services
                .config
                .read()
                .unwrap_or_else(PoisonError::into_inner)
                .statistics
                .enabled;
            if statistics_enabled {
                match layout {
                    Some(Layout::En) => {
                        services.stats.add_word("en");
                        services.sound.play(SoundKind::En);
                    }
                    Some(Layout::Ru) => {
                        services.stats.add_word("ru");
                        services.sound.play(SoundKind::Ru);
                    }
                    None => {}
                }
            }
            tracing::debug!(target: "klavium", "слово: layout={:?}", layout);
        }
        EngineEvent::ErrorDetected {
            original,
            corrected,
            confidence,
            pending,
            screen,
        } => {
            println!(
                "[klavium] ошибка раскладки: «{original}» → «{corrected}» ({:.0}%), в очереди: {pending}",
                confidence * 100.0
            );
            tracing::info!(
                target: "klavium",
                "ошибка раскладки: «{}» → «{}» ({:.0}%), очередь: {}",
                original,
                corrected,
                confidence * 100.0,
                pending
            );
            let statistics_enabled = services
                .config
                .read()
                .unwrap_or_else(PoisonError::into_inner)
                .statistics
                .enabled;
            if statistics_enabled {
                services.stats.add_word("layout_error");
            }
            overlay.show_card(CardRequest {
                old_text: original,
                new_text: corrected,
                meta: format!("{:.0}%", confidence * 100.0),
                anchor: screen,
            });
        }
        EngineEvent::CorrectionApplied {
            original,
            corrected,
            batch,
        } => {
            println!(
                "[klavium] исправлено{}: «{original}» → «{corrected}»",
                if batch { " (пакет)" } else { "" }
            );
            tracing::info!(target: "klavium", "исправлено: «{original}» → «{corrected}», batch={batch}");
            overlay.hide();
        }
        EngineEvent::QueueCleared { reason } => {
            println!("[klavium] очередь очищена: {reason}");
            tracing::debug!(target: "klavium", "очередь очищена: {reason}");
            overlay.hide();
        }
    }
}
