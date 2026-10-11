//! Демонстрация платформы, конвейера и оверлея (Ф3–Ф5) — ручная проверка.
//!
//! Запуск:
//!
//! ```text
//! cargo run -p klavium-app --example hook_demo
//! ```
//!
//! Чек-лист:
//! 1. Откройте Блокнот (или любое приложение с текстовым полем).
//! 2. Наберите в неверной раскладке «ghbdtn» и пробел — у каретки появится
//!    карточка «было → станет», в консоли — строка об ошибке.
//! 3. F8 — слово заменится на «привет», карточка скроется.
//! 4. Esc — скрыть карточку (клавиша не уйдёт в приложение).
//! 5. Сделайте 2–3 ошибки подряд, затем F8 — исправятся все (пакетно).
//! 6. F9 — переключение раскладки (с тостом), F10 — каретка/окно в консоль.
//! 7. Ctrl+Shift+Q — выход.
//!
//! Переменные окружения:
//! - `KLAVIUM_DEMO_SMOKE=<сек>` — выйти по таймеру (автотест);
//! - `KLAVIUM_DEMO_SHOWCARD=1` — показать тестовую карточку сразу после старта.
//!
//! ВАЖНО: демо ставит реальный глобальный хук и синтезирует ввод —
//! запускайте его осознанно и не во время важного набора текста.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::time::Duration;

use klavium_app::engine::{CorrectionEngine, EngineConfig, EngineEvent, KeyInput};
use klavium_app::real_io::PlatformIo;
use klavium_core::Layout;
use klavium_platform::overlay::{self, CardRequest, OverlayHandle};
use klavium_platform::{hook, instance, layout, window};

fn main() -> anyhow::Result<()> {
    println!("Klavium — демо платформы, конвейера и оверлея (Ф3–Ф5)");
    println!("Наберите «ghbdtn» + пробел — появится карточка у каретки; F8 исправит, Esc скроет.");
    println!("F9 — раскладка, F10 — каретка, Ctrl+Shift+Q — выход.");

    let _guard = instance::acquire_named("Klavium_HookDemo_SingleInstance")?
        .ok_or_else(|| anyhow::anyhow!("демо уже запущено (или остался висеть прошлый процесс)"))?;

    let overlay = Arc::new(overlay::start()?);

    let (events_tx, events_rx) = mpsc::channel::<EngineEvent>();
    let engine = CorrectionEngine::spawn(Arc::new(PlatformIo), EngineConfig::default(), events_tx);

    let printer = start_event_printer(events_rx, Arc::clone(&overlay));

    let quit = Arc::new(AtomicBool::new(false));
    let quit_hook = Arc::clone(&quit);
    let engine_hook = Arc::clone(&engine);
    let overlay_hook = Arc::clone(&overlay);

    let keyboard_hook = hook::install(move |event| {
        if event.extra_info == hook::INJECT_EXTRA_INFO {
            return hook::HookDecision::Propagate;
        }
        if !event.is_down {
            return hook::HookDecision::Propagate;
        }

        // Выход: Ctrl+Shift+Q
        if event.vk_code == hook::vk::Q
            && hook::is_key_pressed(hook::vk::CONTROL)
            && hook::is_key_pressed(hook::vk::SHIFT)
        {
            quit_hook.store(true, Ordering::SeqCst);
            return hook::HookDecision::Suppress;
        }

        // Esc скрывает видимую карточку
        if event.vk_code == hook::vk::ESCAPE && overlay_hook.is_visible() {
            overlay_hook.hide();
            return hook::HookDecision::Suppress;
        }

        // Отладочные помощники
        match event.vk_code {
            hook::vk::F9 => {
                let overlay = Arc::clone(&overlay_hook);
                std::thread::spawn(move || {
                    let target = match layout::current() {
                        Some(Layout::En) => Layout::Ru,
                        _ => Layout::En,
                    };
                    let ok = layout::switch_to(target);
                    let color = match target {
                        Layout::Ru => (220, 38, 38),
                        Layout::En => (37, 99, 235),
                    };
                    overlay.show_toast(&format!("Раскладка: {}", target_name(target)), color);
                    println!("[demo] переключение раскладки → {target}: {ok}");
                });
                return hook::HookDecision::Suppress;
            }
            hook::vk::F10 => {
                let caret = window::caret_screen_pos();
                let cursor = window::cursor_pos();
                let title = window::foreground_window()
                    .map(window::window_title)
                    .unwrap_or_default();
                println!("[demo] каретка={caret:?}, мышь={cursor:?}, окно={title:?}");
                return hook::HookDecision::Suppress;
            }
            _ => {}
        }

        let key = KeyInput::from_hook(&event);
        if engine_hook.process_key(&key) {
            hook::HookDecision::Suppress
        } else {
            hook::HookDecision::Propagate
        }
    })?;

    println!("[demo] хук установлен. Ctrl+Shift+Q — выход.");

    if std::env::var("KLAVIUM_DEMO_SHOWCARD").is_ok() {
        overlay.show_card(CardRequest {
            old_text: "ghbdtn".to_string(),
            new_text: "привет".to_string(),
            meta: "95%".to_string(),
            anchor: Some((260, 300, 20)),
        });
        println!("[demo] тестовая карточка показана (KLAVIUM_DEMO_SHOWCARD)");
    }

    // Дымовой режим для автотеста: KLAVIUM_DEMO_SMOKE=<секунды> — выйти по таймеру.
    let smoke_seconds = std::env::var("KLAVIUM_DEMO_SMOKE")
        .ok()
        .and_then(|value| value.parse::<u64>().ok());
    let started = std::time::Instant::now();

    while !quit.load(Ordering::SeqCst) {
        if let Some(seconds) = smoke_seconds {
            if started.elapsed() >= Duration::from_secs(seconds) {
                println!("[demo] smoke: выход по таймеру ({seconds} c)");
                break;
            }
        }
        std::thread::sleep(Duration::from_millis(100));
    }

    keyboard_hook.uninstall();
    drop(engine);
    let _ = printer.join();
    overlay.shutdown();
    println!("[demo] хук снят, выход.");
    Ok(())
}

const fn target_name(layout: Layout) -> &'static str {
    match layout {
        Layout::En => "ENGLISH",
        Layout::Ru => "РУССКИЙ",
    }
}

/// Поток вывода событий движка: консоль + карточка/тосты оверлея.
fn start_event_printer(
    events_rx: mpsc::Receiver<EngineEvent>,
    overlay: Arc<OverlayHandle>,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        while let Ok(event) = events_rx.recv() {
            handle_event(event, &overlay);
        }
    })
}

fn handle_event(event: EngineEvent, overlay: &OverlayHandle) {
    match event {
        EngineEvent::ErrorDetected {
            original,
            corrected,
            confidence,
            pending,
            screen,
        } => {
            println!(
                "[demo] ошибка раскладки: «{original}» → «{corrected}» ({:.0}%), в очереди: {pending} — F8 для исправления",
                confidence * 100.0
            );
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
                "[demo] исправлено{}: «{original}» → «{corrected}»",
                if batch { " (пакет)" } else { "" }
            );
            overlay.hide();
        }
        EngineEvent::QueueCleared { reason } => {
            println!("[demo] очередь очищена: {reason}");
            overlay.hide();
        }
        EngineEvent::WordStarted { .. } => {}
    }
}
