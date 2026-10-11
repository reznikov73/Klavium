//! Сервисы уровня приложения: мониторинг раскладки.
//!
//! Поток опрашивает раскладку активного окна с интервалом `timeouts.layout_check`
//! и при смене: звук, тост-уведомление и счётчик переключений.

use std::sync::mpsc::Sender;
use std::sync::{Arc, PoisonError};
use std::thread;
use std::time::Duration;

use klavium_core::Layout;
use klavium_platform::layout;
use klavium_platform::overlay::OverlayHandle;
use klavium_ui::tray::TrayCommand;

use crate::runtime::SharedConfig;
use crate::sound::{SoundKind, SoundService};
use crate::stats::StatsService;

/// Запустить поток-мониторинг раскладки.
pub fn spawn_layout_monitor(
    overlay: Arc<OverlayHandle>,
    sound: Arc<SoundService>,
    stats: Arc<StatsService>,
    config: SharedConfig,
    tray_commands: Sender<TrayCommand>,
) {
    let _ = thread::Builder::new()
        .name("layout-monitor".to_string())
        .spawn(move || {
            let mut last: Option<Layout> = None;
            loop {
                let interval = {
                    let guard = config.read().unwrap_or_else(PoisonError::into_inner);
                    guard.timeouts.layout_check
                };
                thread::sleep(Duration::from_secs_f64(interval.max(0.1)));

                let current = layout::current();
                if current != last {
                    if last.is_some() {
                        if let Some(value) = current {
                            let _ = tray_commands.send(TrayCommand::SetLayout(Some(value)));
                            stats.add_switch();
                            sound.play(SoundKind::Switch);
                            let notify = {
                                let guard = config.read().unwrap_or_else(PoisonError::into_inner);
                                guard.notifications.enabled
                            };
                            if notify {
                                overlay.show_toast(layout_label(value), layout_color(value));
                            }
                            tracing::info!(target: "klavium", "раскладка: {}", value.as_str());
                        }
                    }
                    last = current;
                }
            }
        });
}

/// Метка раскладки для тоста.
pub const fn layout_label(value: Layout) -> &'static str {
    match value {
        Layout::En => "ENGLISH",
        Layout::Ru => "РУССКИЙ",
    }
}

/// Цвет тоста раскладки: русский — красный, английский — синий (как иконки трея).
pub const fn layout_color(value: Layout) -> (u8, u8, u8) {
    match value {
        Layout::En => (37, 99, 235),
        Layout::Ru => (220, 38, 38),
    }
}
