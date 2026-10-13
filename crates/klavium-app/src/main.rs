//! Klavium — фоновая утилита исправления раскладки (трей-приложение).
//!
//! Поток: single instance → конфиг → журнал → оверлей → сервисы (статистика,
//! звук, мониторинг раскладки, голос) → движок коррекции и хук → трей-иконка →
//! окно настроек (egui) на главном потоке. Приложение живёт в трее; «Выход» —
//! из меню; аварийный выход — Ctrl+Shift+Q.
//!
//! Дымовой режим (автотест): `KLAVIUM_APP_SMOKE=<сек>`, показать окно сразу:
//! `KLAVIUM_APP_SHOW_SETTINGS=1`; тост раскладки: `KLAVIUM_APP_TOAST_DEMO=en|ru`;
//! запрос теста распознавания сразу после старта: `KLAVIUM_APP_VOICE_TEST=1`.

// Приложение — фоновая трей-утилита без консольного окна (журнал — в `logs/app.log`).
#![windows_subsystem = "windows"]

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex, RwLock};

use klavium_app::config_io;
use klavium_app::engine::{CorrectionEngine, KeyInput};
use klavium_app::real_io::{self, PlatformIo};
use klavium_app::runtime::{self, SharedConfig};
use klavium_app::{logging, models, services, sound, stats, voice};
use klavium_core::config::AppConfig;
use klavium_platform::{autostart, hook, instance, overlay, window};
use klavium_ui::{settings, tray};

/// Журнал: старт приложения и выбранный каталог данных.
fn log_startup(paths: &config_io::DataPaths) {
    tracing::info!(
        target: "klavium",
        "Klavium запускается (версия {})",
        env!("CARGO_PKG_VERSION")
    );
    if let Some(note) = &paths.note {
        tracing::info!(
            target: "klavium",
            "Каталог данных: {} ({note})",
            paths.data_dir.display()
        );
    } else {
        tracing::info!(
            target: "klavium",
            "Каталог данных: {}",
            paths.data_dir.display()
        );
    }
}

/// Открыто ли окно настроек прямо сейчас (главный поток внутри eframe).
static UI_OPEN: AtomicBool = AtomicBool::new(false);

/// Попросить главный поток открыть окно настроек (если оно ещё не открыто).
fn request_ui_open(ui_tx: &std::sync::mpsc::Sender<()>) {
    if !UI_OPEN.load(Ordering::SeqCst) {
        let _ = ui_tx.send(());
    }
}

fn main() -> anyhow::Result<()> {
    let Some(_guard) = instance::check_single_instance()? else {
        // Второй запуск: просим работающий экземпляр показать окно настроек
        // его собственными средствами (внешний ShowWindow ломает цикл winit).
        let _ = window::request_show_settings();
        return Ok(());
    };

    let paths = config_io::resolve();
    let config = config_io::load(&paths.config_path);
    if !paths.config_path.exists() {
        let _ = config_io::save(&paths.config_path, &config);
    }
    logging::init(&config, &paths.logs_dir())?;
    log_startup(&paths);

    sync_autostart(&config);

    let shared: SharedConfig = Arc::new(RwLock::new(config.clone()));

    // Сервисы: статистика, звук
    let stats_service = Arc::new(stats::StatsService::new(paths.stats_path()));
    if config.statistics.reset_on_start {
        stats_service.reset();
    }
    stats_service.spawn_autosave();
    let sound_service = sound::SoundService::spawn(Arc::clone(&shared));

    let overlay = Arc::new(overlay::start()?);

    let (events_tx, events_rx) = mpsc::channel();
    let engine = CorrectionEngine::spawn(
        Arc::new(PlatformIo),
        runtime::engine_config(&config),
        events_tx,
    );
    let events_thread = runtime::spawn_engine_event_loop(
        events_rx,
        Arc::clone(&overlay),
        runtime::Services {
            stats: Arc::clone(&stats_service),
            sound: Arc::clone(&sound_service),
            config: Arc::clone(&shared),
        },
    );

    let voice_service = voice::VoiceService::spawn(voice::VoiceDeps {
        engine: Arc::clone(&engine),
        overlay: Arc::clone(&overlay),
        sound: Arc::clone(&sound_service),
        stats: Arc::clone(&stats_service),
        config: Arc::clone(&shared),
        data_dir: paths.data_dir.clone(),
    });

    maybe_spawn_voice_auto(&voice_service);
    maybe_spawn_voice_test(&voice_service);

    // Канал «открыть окно настроек»: заполняется потоком трея и приёмником
    // сигналов от повторного запуска; главный поток ждёт его между открытиями.
    let (ui_tx, ui_rx) = mpsc::channel::<()>();

    // Трей живёт в отдельном потоке: иконке нужен собственный насос сообщений.
    let (tray_tx, tray_queue) = spawn_system_tray(
        Arc::clone(&voice_service),
        Arc::clone(&stats_service),
        ui_tx.clone(),
    );

    services::spawn_layout_monitor(
        Arc::clone(&overlay),
        Arc::clone(&sound_service),
        Arc::clone(&stats_service),
        Arc::clone(&shared),
        tray_tx,
    );

    maybe_spawn_toast_demo(&overlay);

    let keyboard_hook = install_hook(
        Arc::clone(&engine),
        Arc::clone(&overlay),
        Arc::clone(&voice_service),
        Arc::clone(&shared),
    )?;

    // Общие объекты окна настроек: живут всё время работы приложения и
    // переиспользуются при каждом открытии (окно создаётся и закрывается).
    let models_dir = paths.models_dir();
    let ui_runtime = UiRuntime {
        engine: Arc::clone(&engine),
        shared: Arc::clone(&shared),
        stats: Arc::clone(&stats_service),
        voice: Arc::clone(&voice_service),
        downloads: models::ModelDownloader::new(models_dir),
        vosk_install: models::VoskInstaller::new(paths.data_dir.clone()),
        data_move: Arc::new(DataDirShared::new(paths.data_dir.clone())),
        data_dir: paths.data_dir.clone(),
        data_note: paths.note.clone(),
        config_path: paths.config_path,
        tray_queue: Arc::clone(&tray_queue),
    };
    drop(ui_tx); // единственные отправители остаются в потоке трея

    // Окно настроек: ручной запуск — сразу, автозапуск — по требованию (клик
    // в трее или повторный запуск); на каждое открытие окно создаётся заново.
    let smoke = std::env::var("KLAVIUM_APP_SMOKE").is_ok();
    run_ui_loop(&ui_runtime, &ui_rx, should_open_settings() || smoke, smoke);

    keyboard_hook.uninstall();
    voice_service.shutdown();
    drop(engine);
    let _ = events_thread.join();
    overlay.shutdown();
    let _ = stats_service.save();
    tracing::info!(target: "klavium", "Klavium завершён");
    Ok(())
}

/// Всё для открытия окна настроек, что живёт весь срок работы приложения.
struct UiRuntime {
    engine: Arc<CorrectionEngine>,
    shared: SharedConfig,
    stats: Arc<stats::StatsService>,
    voice: Arc<voice::VoiceService>,
    downloads: Arc<models::ModelDownloader>,
    vosk_install: Arc<models::VoskInstaller>,
    data_move: Arc<DataDirShared>,
    data_dir: PathBuf,
    data_note: Option<String>,
    config_path: PathBuf,
    tray_queue: tray::TrayQueue,
}

/// Цикл «открыть/закрыть окно настроек» до завершения приложения.
///
/// Окно на каждое открытие создаётся заново; после закрытия eframe-цикл
/// освобождает окно только при следующем запуске — до этого скрываем его
/// сами (см. [`window::hide_settings_windows`]).
fn run_ui_loop(runtime: &UiRuntime, ui_rx: &mpsc::Receiver<()>, mut open_now: bool, smoke: bool) {
    loop {
        if open_now {
            UI_OPEN.store(true, Ordering::SeqCst);
            let open_result = run_settings(UiContext {
                config: config_io::load(&runtime.config_path),
                config_path: runtime.config_path.clone(),
                engine: Arc::clone(&runtime.engine),
                shared: Arc::clone(&runtime.shared),
                stats: Arc::clone(&runtime.stats),
                voice: Arc::clone(&runtime.voice),
                downloads: Arc::clone(&runtime.downloads),
                vosk_install: Arc::clone(&runtime.vosk_install),
                data_move: Arc::clone(&runtime.data_move),
                data_dir: runtime.data_dir.clone(),
                data_note: runtime.data_note.clone(),
                tray_queue: Arc::clone(&runtime.tray_queue),
            });
            UI_OPEN.store(false, Ordering::SeqCst);
            window::hide_settings_windows();
            if let Err(error) = open_result {
                tracing::error!(target: "klavium", "окно настроек: {error}");
            }
            if smoke {
                return; // дымовой режим: после закрытия окна завершаем процесс
            }
        }
        // Ждём запрос на открытие (клик «Настройки»/«Статистика» в трее,
        // повторный запуск приложения).
        if ui_rx.recv().is_err() {
            return;
        }
        open_now = true;
    }
}

/// Всё, что нужно окну настроек (собирается в `main`).
struct UiContext {
    config: AppConfig,
    config_path: PathBuf,
    engine: Arc<CorrectionEngine>,
    shared: SharedConfig,
    stats: Arc<stats::StatsService>,
    voice: Arc<voice::VoiceService>,
    downloads: Arc<models::ModelDownloader>,
    vosk_install: Arc<models::VoskInstaller>,
    data_move: Arc<DataDirShared>,
    data_dir: PathBuf,
    data_note: Option<String>,
    tray_queue: tray::TrayQueue,
}

/// Асинхронный перенос каталога данных (кнопки в «Общих»).
struct DataDirShared {
    state: Mutex<Option<String>>,
    current: PathBuf,
}

impl DataDirShared {
    const fn new(current: PathBuf) -> Self {
        Self {
            state: Mutex::new(None),
            current,
        }
    }

    /// Запустить перенос в фоне (`None` — вернуть папку по умолчанию).
    fn start(self: &Arc<Self>, target: Option<PathBuf>) {
        {
            let mut guard = self
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if guard.is_some() {
                return;
            }
            *guard = Some("перенос данных…".to_string());
        }
        let shared = Arc::clone(self);
        let _ = std::thread::Builder::new()
            .name("data-move".to_string())
            .spawn(move || {
                let text = match config_io::set_data_dir(&shared.current, target.as_deref()) {
                    Ok(dir) => format!("готово: {}. Перезапустите Klavium.", dir.display()),
                    Err(error) => format!("ошибка переноса: {error}"),
                };
                *shared
                    .state
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(text);
            });
    }

    /// Текущий статус для окна настроек.
    fn status(&self) -> Option<String> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
}

/// Собрать контекст и запустить окно настроек (блокирует до выхода).
fn run_settings(ui: UiContext) -> anyhow::Result<()> {
    let save_engine = Arc::clone(&ui.engine);
    let save_shared = Arc::clone(&ui.shared);
    let stats_snapshot = Arc::clone(&ui.stats);
    let stats_reset = Arc::clone(&ui.stats);
    let voice_toggle = Arc::clone(&ui.voice);
    let voice_snapshot = Arc::clone(&ui.voice);
    let voice_reload = Arc::clone(&ui.voice);
    let voice_test_start = Arc::clone(&ui.voice);
    let voice_test_stop = Arc::clone(&ui.voice);
    let voice_test_state = Arc::clone(&ui.voice);
    let data_move_change = Arc::clone(&ui.data_move);
    let data_move_status = Arc::clone(&ui.data_move);
    let downloads_start = Arc::clone(&ui.downloads);
    let downloads_state = Arc::clone(&ui.downloads);
    let vosk_install_start = Arc::clone(&ui.vosk_install);
    let vosk_install_state = Arc::clone(&ui.vosk_install);
    let settings_context = settings::SettingsContext {
        config: ui.config,
        on_save: Box::new(move |new_config: AppConfig| -> Result<(), String> {
            config_io::save(&ui.config_path, &new_config).map_err(|error| error.to_string())?;
            save_engine.update_config(runtime::engine_config(&new_config));
            *save_shared
                .write()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = new_config.clone();
            sync_autostart(&new_config);
            voice_reload.reload(); // движок перечитает настройки на лету
            tracing::info!(target: "klavium", "настройки сохранены");
            Ok(())
        }),
        stats_snapshot: Box::new(move || stats_snapshot.snapshot()),
        stats_reset: Box::new(move || stats_reset.reset()),
        voice_toggle: Box::new(move || voice_toggle.toggle()),
        voice_snapshot: Box::new(move || {
            (voice_snapshot.is_active(), voice_snapshot.status_text())
        }),
        voice_test_start: Box::new(move || voice_test_start.start_test()),
        voice_test_stop: Box::new(move || voice_test_stop.stop_test()),
        voice_test_state: Box::new(move || voice_test_state.test_status()),
        data_dir: ui.data_dir,
        data_note: ui.data_note,
        tray_queue: ui.tray_queue,
        data_dir_change: Box::new(move |target: Option<PathBuf>| data_move_change.start(target)),
        data_move_status: Box::new(move || data_move_status.status()),
        model_download_start: Box::new(move |file: String| downloads_start.start(&file)),
        model_download_state: Box::new(move || downloads_state.status()),
        vosk_install_start: Box::new(move || vosk_install_start.start()),
        vosk_state: Box::new(move || vosk_install_state.status()),
        open_on_start: true,
        smoke_seconds: std::env::var("KLAVIUM_APP_SMOKE")
            .ok()
            .and_then(|value| value.parse::<u64>().ok()),
    };
    settings::run(settings_context)
}

/// Установить клавиатурный хук: аварийный выход, Esc-скрытие карточки,
/// хоткей голосового ввода, движок.
fn install_hook(
    engine: Arc<CorrectionEngine>,
    overlay: Arc<overlay::OverlayHandle>,
    voice: Arc<voice::VoiceService>,
    config: SharedConfig,
) -> anyhow::Result<hook::KeyboardHook> {
    hook::install(move |event| {
        if event.extra_info == hook::INJECT_EXTRA_INFO {
            return hook::HookDecision::Propagate;
        }
        if !event.is_down {
            return hook::HookDecision::Propagate;
        }

        // Аварийный выход
        if event.vk_code == hook::vk::Q
            && hook::is_key_pressed(hook::vk::CONTROL)
            && hook::is_key_pressed(hook::vk::SHIFT)
        {
            std::process::exit(0);
        }

        // Esc скрывает видимую карточку (в приложение не уходит)
        if event.vk_code == hook::vk::ESCAPE && overlay.is_visible() {
            overlay.hide();
            return hook::HookDecision::Suppress;
        }

        // Хоткей голосового ввода (по умолчанию F9), если фича включена
        let voice_hotkey_hit = {
            let guard = config
                .read()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if guard.voice.enabled {
                let modifiers = real_io::current_modifiers();
                klavium_app::hotkeys::matches(
                    &guard.voice.hotkey,
                    event.vk_code,
                    event.scan_code,
                    &modifiers,
                )
            } else {
                false
            }
        };
        if voice_hotkey_hit {
            voice.toggle();
            return hook::HookDecision::Suppress;
        }

        let key = KeyInput::from_hook(&event);
        if engine.process_key(&key) {
            hook::HookDecision::Suppress
        } else {
            hook::HookDecision::Propagate
        }
    })
}

/// Дымовой режим: автоматически включить диктовку (`KLAVIUM_APP_VOICE=1`)
/// — проверка голосового тракта без реального F9.
fn maybe_spawn_voice_auto(voice: &Arc<voice::VoiceService>) {
    if std::env::var("KLAVIUM_APP_VOICE").is_err() {
        return;
    }
    let voice_auto = Arc::clone(voice);
    let _ = std::thread::Builder::new()
        .name("voice-auto".to_string())
        .spawn(move || {
            std::thread::sleep(std::time::Duration::from_secs(10));
            voice_auto.toggle();
        });
}

/// Дымовой режим: сразу запросить тест распознавания (`KLAVIUM_APP_VOICE_TEST=1`)
/// — проверка отложенного старта, пока движок ещё загружается.
fn maybe_spawn_voice_test(voice: &Arc<voice::VoiceService>) {
    if std::env::var("KLAVIUM_APP_VOICE_TEST").is_err() {
        return;
    }
    voice.start_test();
}

/// Дымовой режим: показать тост раскладки (`KLAVIUM_APP_TOAST_DEMO=en|ru`)
/// — проверка цветов уведомления без реального переключения языка.
fn maybe_spawn_toast_demo(overlay: &Arc<overlay::OverlayHandle>) {
    let Ok(kind) = std::env::var("KLAVIUM_APP_TOAST_DEMO") else {
        return;
    };
    let layout = if kind.eq_ignore_ascii_case("ru") {
        klavium_core::Layout::Ru
    } else {
        klavium_core::Layout::En
    };
    let overlay = Arc::clone(overlay);
    let _ = std::thread::Builder::new()
        .name("toast-demo".to_string())
        .spawn(move || {
            for _ in 0..40 {
                std::thread::sleep(std::time::Duration::from_millis(1200));
                tracing::info!(target: "klavium", "тост-демо: показ {kind}");
                overlay.show_toast(
                    services::layout_label(layout),
                    services::layout_color(layout),
                );
            }
        });
}

/// Поток трей-иконки: создаёт иконку и меню, качает свои сообщения Win32
/// (иконке нужен насос на своём потоке), выполняет команды смены раскладки,
/// а «Выход»/«Голосовой ввод»/«Настройки» обрабатывает независимо от UI.
///
/// Возвращает канал команд для смены иконки и очередь событий для окна.
fn spawn_system_tray(
    voice: Arc<voice::VoiceService>,
    stats: Arc<stats::StatsService>,
    ui_tx: std::sync::mpsc::Sender<()>,
) -> (std::sync::mpsc::Sender<tray::TrayCommand>, tray::TrayQueue) {
    let queue: tray::TrayQueue = Arc::new(Mutex::new(std::collections::VecDeque::new()));
    let thread_queue = Arc::clone(&queue);
    let (commands_tx, commands_rx) = mpsc::channel::<tray::TrayCommand>();
    let spawned = std::thread::Builder::new()
        .name("system-tray".to_string())
        .spawn(move || {
            let Ok(tray) = tray::create_tray() else {
                tracing::error!(target: "klavium", "не удалось создать иконку трея");
                return;
            };
            // Приёмник сигналов «открыть настройки» от других процессов (повторный запуск).
            {
                let control_queue = Arc::clone(&thread_queue);
                let control_ui_tx = ui_tx.clone();
                window::install_show_control(move || {
                    control_queue
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .push_back(tray::MENU_SETTINGS.to_string());
                    request_ui_open(&control_ui_tx);
                });
            }
            loop {
                window::pump_thread_messages();
                while let Ok(command) = commands_rx.try_recv() {
                    match command {
                        tray::TrayCommand::SetLayout(layout) => tray.set_layout_icon(layout),
                    }
                }
                while let Some(id) = tray::poll_menu_event() {
                    match id.as_str() {
                        tray::MENU_QUIT => {
                            tracing::info!(target: "klavium", "Выход из меню трея");
                            voice.shutdown();
                            let _ = stats.save();
                            std::thread::sleep(std::time::Duration::from_millis(100));
                            std::process::exit(0);
                        }
                        tray::MENU_VOICE => voice.toggle(),
                        _ => {
                            thread_queue
                                .lock()
                                .unwrap_or_else(std::sync::PoisonError::into_inner)
                                .push_back(id);
                            // Окно уже открыто — оно само прочитает очередь;
                            // иначе просим главный поток открыть его (без внешних
                            // ShowWindow: они ломают цикл winit).
                            request_ui_open(&ui_tx);
                        }
                    }
                }
            }
        });
    if spawned.is_err() {
        tracing::error!(target: "klavium", "не удалось запустить поток трея");
    }
    (commands_tx, queue)
}

/// Открывать окно настроек сразу: ручной запуск — да; автозапуск (`--silent`) — нет.
fn should_open_settings() -> bool {
    if std::env::var("KLAVIUM_APP_SHOW_SETTINGS").is_ok() {
        return true;
    }
    !std::env::args().any(|arg| arg == "--silent") && std::env::var("KLAVIUM_APP_SILENT").is_err()
}

/// Синхронизировать автозапуск с настройкой (без лишних записей в реестр).
fn sync_autostart(config: &AppConfig) {
    if config.autostart.enabled {
        if !autostart::is_autostart_enabled() || !autostart::autostart_is_current() {
            let _ = autostart::enable_autostart();
        }
    } else if autostart::is_autostart_enabled() {
        let _ = autostart::disable_autostart();
    }
}
