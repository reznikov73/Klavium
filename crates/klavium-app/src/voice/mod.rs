//! Голосовой ввод (Ф8): сессии диктовки, бейдж, вставка через движок коррекции.
//!
//! Потоки:
//! - `voice-manager` — состояние сессии, команды «стоп/пауза/продолжить»,
//!   пунктуация/автозаглавные, вставка, статистика и тест распознавания;
//! - `voice-worker` — движок распознавания (Whisper/Google/Vosk); переключается
//!   на лету при сохранении настроек;
//! - `voice-listen` (на сессию) — микрофон → VAD → фразы (16 кГц mono).
//!
//! [`VoiceService::toggle`] неблокирующий: поток хука только отправляет команду.

pub mod audio;
pub mod engines;
pub mod whisper;
pub mod worker;

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::Duration;

use klavium_core::voice as voice_text;
use klavium_core::voice::RecognitionTest;
use klavium_core::voice::VoiceCommand;
use klavium_platform::overlay::OverlayHandle;

use crate::engine::CorrectionEngine;
use crate::runtime::SharedConfig;
use crate::sound::{SoundKind, SoundService};
use crate::stats::StatsService;

use worker::{EngineStatus, RecognitionWorker};

/// Цвет бейджа диктовки (#7c3aed).
const BADGE_COLOR_ACTIVE: (u8, u8, u8) = (124, 58, 237);
/// Цвет бейджа ошибок (#dc2626).
const BADGE_COLOR_ERROR: (u8, u8, u8) = (220, 38, 38);
/// Автоскрытие временных бейджей (1.5 с).
const BADGE_TIMEOUT_MS: u32 = 1500;
/// Длительность теста распознавания в настройках (секунды).
const TEST_SECONDS: u32 = 15;
/// Период тика менеджера (автозапуск отложенного теста), мс.
const MANAGER_TICK_MS: u64 = 200;

/// Блокировка без паник при отравлении.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Сообщения управляющего потока сервиса.
pub(crate) enum VoiceInbound {
    /// Переключить диктовку.
    Toggle,
    /// Готовая фраза распознавания.
    Phrase(String),
    /// Не удалось открыть/читать микрофон.
    AudioFailed(String),
    /// Запустить тест распознавания (окно настроек).
    TestStart,
    /// Остановить тест распознавания.
    TestStop,
    /// Перечитать настройки и пересоздать движок (без перезапуска приложения).
    ReloadEngine,
    /// Остановить сервис.
    Shutdown,
}

/// Зависимости голосового сервиса.
pub struct VoiceDeps {
    /// Движок коррекции (куда уходит вставка).
    pub engine: Arc<CorrectionEngine>,
    /// Оверлей (бейдж).
    pub overlay: Arc<OverlayHandle>,
    /// Звуковой сервис.
    pub sound: Arc<SoundService>,
    /// Статистика.
    pub stats: Arc<StatsService>,
    /// Конфигурация.
    pub config: SharedConfig,
    /// Каталог данных (модели — в `models/`).
    pub data_dir: PathBuf,
}

/// Голосовой сервис; [`VoiceService::toggle`] безопасен из потока хука.
pub struct VoiceService {
    tx: Sender<VoiceInbound>,
    active: Arc<AtomicBool>,
    status: Arc<Mutex<EngineStatus>>,
    test: Arc<Mutex<RecognitionTest>>,
    manager: Mutex<Option<thread::JoinHandle<()>>>,
}

impl VoiceService {
    /// Запустить сервис (менеджер-поток и воркер распознавания).
    #[must_use]
    pub fn spawn(deps: VoiceDeps) -> Arc<Self> {
        let (tx, rx) = mpsc::channel::<VoiceInbound>();
        let active = Arc::new(AtomicBool::new(false));
        let status = Arc::new(Mutex::new(EngineStatus::Loading));
        let test = Arc::new(Mutex::new(RecognitionTest::default()));
        let worker_tx = tx.clone();
        let thread_active = Arc::clone(&active);
        let thread_status = Arc::clone(&status);
        let thread_test = Arc::clone(&test);
        let manager = thread::Builder::new()
            .name("voice-manager".to_string())
            .spawn(move || {
                manager_loop(
                    &rx,
                    &worker_tx,
                    &deps,
                    &thread_active,
                    thread_status,
                    &thread_test,
                );
            })
            .ok();
        Arc::new(Self {
            tx,
            active,
            status,
            test,
            manager: Mutex::new(manager),
        })
    }

    /// Переключить диктовку (вкл/выкл). Неблокирующе.
    pub fn toggle(&self) {
        let _ = self.tx.send(VoiceInbound::Toggle);
    }

    /// Попросить перечитать настройки и пересоздать движок (после сохранения).
    pub fn reload(&self) {
        let _ = self.tx.send(VoiceInbound::ReloadEngine);
    }

    /// Запустить тест распознавания (окно настроек).
    pub fn start_test(&self) {
        let _ = self.tx.send(VoiceInbound::TestStart);
    }

    /// Остановить тест распознавания.
    pub fn stop_test(&self) {
        let _ = self.tx.send(VoiceInbound::TestStop);
    }

    /// Снимок состояния теста (для окна настроек).
    #[must_use]
    pub fn test_status(&self) -> RecognitionTest {
        lock(&self.test).clone()
    }

    /// Активна ли диктовка (для окна настроек).
    #[must_use]
    pub fn is_active(&self) -> bool {
        self.active.load(Ordering::SeqCst)
    }

    /// Текст статуса движка для окна настроек.
    #[must_use]
    pub fn status_text(&self) -> String {
        let status = lock(&self.status).clone();
        match status {
            EngineStatus::Loading => "движок: модель загружается…".to_string(),
            EngineStatus::Ready(name) => format!("движок готов: {name}"),
            EngineStatus::NoModel(directory) => {
                format!("модель не найдена (папка: {directory})")
            }
            EngineStatus::Failed(reason) => format!("ошибка модели: {reason}"),
        }
    }

    /// Остановить сервис: завершить сессию и воркер распознавания.
    pub fn shutdown(&self) {
        let _ = self.tx.send(VoiceInbound::Shutdown);
        let join = lock(&self.manager).take();
        if let Some(join) = join {
            let _ = join.join();
        }
    }
}

/// Состояние запущенной сессии захвата (диктовка или тест).
struct Session {
    active: Arc<AtomicBool>,
    listen: thread::JoinHandle<()>,
}

/// Изменяемое состояние менеджера (сессии, пауза, контекст вставки).
struct ManagerState {
    session: Option<Session>,
    mic_test: Option<Session>,
    paused: bool,
    last_char: Option<char>,
}

fn manager_loop(
    rx: &Receiver<VoiceInbound>,
    worker_tx: &Sender<VoiceInbound>,
    deps: &VoiceDeps,
    active_flag: &Arc<AtomicBool>,
    status: Arc<Mutex<EngineStatus>>,
    test_snapshot: &Arc<Mutex<RecognitionTest>>,
) {
    let worker = RecognitionWorker::spawn(
        deps.data_dir.clone(),
        Arc::clone(&deps.config),
        worker_tx.clone(),
        status,
    );
    let mut state = ManagerState {
        session: None,
        mic_test: None,
        paused: false,
        last_char: None,
    };

    loop {
        match rx.recv_timeout(Duration::from_millis(MANAGER_TICK_MS)) {
            Ok(VoiceInbound::Toggle) => {
                handle_toggle(
                    &worker,
                    worker_tx,
                    deps,
                    active_flag,
                    &mut state,
                    test_snapshot,
                );
            }
            Ok(VoiceInbound::Phrase(text)) => {
                handle_phrase(text, deps, active_flag, &mut state, test_snapshot);
            }
            Ok(VoiceInbound::AudioFailed(reason)) => {
                handle_audio_failed(&reason, deps, active_flag, &mut state, test_snapshot);
            }
            Ok(VoiceInbound::TestStart) => {
                handle_test_start(&worker, worker_tx, &mut state.mic_test, test_snapshot);
            }
            Ok(VoiceInbound::TestStop) => {
                stop_test_session(&mut state.mic_test, test_snapshot);
            }
            Ok(VoiceInbound::ReloadEngine) => worker.reload(),
            Ok(VoiceInbound::Shutdown) => {
                stop_session(&mut state.session, deps, active_flag);
                stop_test_session(&mut state.mic_test, test_snapshot);
                worker.stop();
                break;
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
        maybe_auto_start_test(&worker, worker_tx, &mut state.mic_test, test_snapshot);
    }
}

/// F9: во время теста (или ожидания движка) отменяем тест; иначе старт/стоп диктовки.
fn handle_toggle(
    worker: &RecognitionWorker,
    worker_tx: &Sender<VoiceInbound>,
    deps: &VoiceDeps,
    active_flag: &Arc<AtomicBool>,
    state: &mut ManagerState,
    test_snapshot: &Arc<Mutex<RecognitionTest>>,
) {
    if state.mic_test.is_some() || lock(test_snapshot).waiting_engine {
        stop_test_session(&mut state.mic_test, test_snapshot);
        return;
    }
    if state.session.is_some() {
        stop_session(&mut state.session, deps, active_flag);
        state.paused = false;
        return;
    }
    match worker.status() {
        EngineStatus::Ready(_) => {
            state.session = start_session(worker, deps, active_flag, worker_tx);
            state.paused = false;
            state.last_char = None;
        }
        not_ready => show_not_ready_badge(deps, &not_ready),
    }
}

/// Фраза: в тесте — в результат; команды «стоп/пауза/продолжить»; иначе вставка.
fn handle_phrase(
    text: String,
    deps: &VoiceDeps,
    active_flag: &Arc<AtomicBool>,
    state: &mut ManagerState,
    test_snapshot: &Arc<Mutex<RecognitionTest>>,
) {
    if state.mic_test.is_some() {
        lock(test_snapshot).results.push(text);
        return;
    }
    if state.session.is_none() {
        return;
    }
    match voice_text::classify_command(&text) {
        Some(VoiceCommand::Stop) => {
            tracing::info!(target: "klavium", "Голос: команда «стоп»");
            stop_session(&mut state.session, deps, active_flag);
            state.paused = false;
            return;
        }
        Some(VoiceCommand::Pause) => {
            state.paused = true;
            deps.overlay.show_badge("ПАУЗА", BADGE_COLOR_ACTIVE);
            return;
        }
        Some(VoiceCommand::Resume) => {
            state.paused = false;
            deps.overlay.show_badge("ГОЛОС", BADGE_COLOR_ACTIVE);
            return;
        }
        None => {}
    }
    if state.paused {
        return;
    }
    insert_phrase(&text, deps, &mut state.last_char);
}

/// Микрофон отвалился: в тесте — ошибка в окне; в диктовке — стоп и бейдж.
fn handle_audio_failed(
    reason: &str,
    deps: &VoiceDeps,
    active_flag: &Arc<AtomicBool>,
    state: &mut ManagerState,
    test_snapshot: &Arc<Mutex<RecognitionTest>>,
) {
    if state.mic_test.is_some() {
        stop_test_session(&mut state.mic_test, test_snapshot);
        lock(test_snapshot).error = Some(format!("микрофон: {reason}"));
        return;
    }
    if state.session.is_none() {
        return;
    }
    tracing::error!(target: "klavium", "Голос: ошибка микрофона: {reason}");
    stop_session(&mut state.session, deps, active_flag);
    state.paused = false;
    deps.overlay
        .show_badge_timed("НЕТ МИКРОФОНА", BADGE_COLOR_ERROR, BADGE_TIMEOUT_MS);
}

/// Бейдж «движок не готов» (загрузка/нет модели/ошибка).
fn show_not_ready_badge(deps: &VoiceDeps, status: &EngineStatus) {
    let text = match status {
        EngineStatus::Loading => "МОДЕЛЬ ГРУЗИТСЯ",
        EngineStatus::NoModel(_) => "НЕТ МОДЕЛИ",
        EngineStatus::Failed(_) => "ОШИБКА МОДЕЛИ",
        EngineStatus::Ready(_) => return,
    };
    deps.overlay
        .show_badge_timed(text, BADGE_COLOR_ERROR, BADGE_TIMEOUT_MS);
}

/// Запуск теста распознавания: сброс, старт захвата, таймер автостопа.
///
/// Если движок ещё загружается, тест ставится в ожидание и стартует сам
/// (см. [`maybe_auto_start_test`]); если модель недоступна — показываем причину.
fn handle_test_start(
    worker: &RecognitionWorker,
    worker_tx: &Sender<VoiceInbound>,
    mic_test: &mut Option<Session>,
    test_snapshot: &Arc<Mutex<RecognitionTest>>,
) {
    match worker.status() {
        EngineStatus::Ready(_) => {}
        EngineStatus::Loading => {
            {
                let mut snapshot = lock(test_snapshot);
                snapshot.running = false;
                snapshot.waiting_engine = true;
                snapshot.seconds_left = 0;
                snapshot.results.clear();
                snapshot.error = None;
            }
            tracing::info!(target: "klavium", "Голос: тест отложен — движок ещё загружается");
            return;
        }
        not_ready => {
            if let Some(message) = engine_not_ready_reason(&not_ready) {
                set_test_error(test_snapshot, message);
            }
            return;
        }
    }
    {
        let mut snapshot = lock(test_snapshot);
        snapshot.running = true;
        snapshot.waiting_engine = false;
        snapshot.seconds_left = TEST_SECONDS;
        snapshot.results.clear();
        snapshot.error = None;
    }
    if let Some(old) = mic_test.take() {
        old.active.store(false, Ordering::SeqCst);
        let _ = old.listen.join();
    }
    *mic_test = start_test_session(worker, worker_tx);
    if mic_test.is_some() {
        spawn_test_timer(worker_tx, Arc::clone(test_snapshot));
    } else {
        set_test_error(
            test_snapshot,
            "не удалось запустить захват микрофона".to_string(),
        );
    }
}

/// Тик менеджера: отложенный тест стартует сам, когда движок доготовился.
fn maybe_auto_start_test(
    worker: &RecognitionWorker,
    worker_tx: &Sender<VoiceInbound>,
    mic_test: &mut Option<Session>,
    test_snapshot: &Arc<Mutex<RecognitionTest>>,
) {
    if !lock(test_snapshot).waiting_engine {
        return;
    }
    match worker.status() {
        EngineStatus::Loading => {}
        EngineStatus::Ready(_) => {
            tracing::info!(target: "klavium", "Голос: движок готов — запускаю отложенный тест");
            handle_test_start(worker, worker_tx, mic_test, test_snapshot);
        }
        not_ready => {
            if let Some(message) = engine_not_ready_reason(&not_ready) {
                set_test_error(test_snapshot, message);
            }
        }
    }
}

/// Показать ошибку теста (снимает ожидание и прошлый результат).
fn set_test_error(test_snapshot: &Arc<Mutex<RecognitionTest>>, message: String) {
    let mut snapshot = lock(test_snapshot);
    snapshot.running = false;
    snapshot.waiting_engine = false;
    snapshot.seconds_left = 0;
    snapshot.results.clear();
    snapshot.error = Some(message);
}

/// Причина «движок не готов» (None — движок в порядке или ещё грузится).
fn engine_not_ready_reason(status: &EngineStatus) -> Option<String> {
    match status {
        EngineStatus::Loading | EngineStatus::Ready(_) => None,
        EngineStatus::NoModel(directory) => Some(format!(
            "движок не готов: модель не найдена (папка: {directory}) — выберите модель или скачайте её"
        )),
        EngineStatus::Failed(reason) => Some(format!("движок не готов: {reason}")),
    }
}

fn start_session(
    worker: &RecognitionWorker,
    deps: &VoiceDeps,
    active_flag: &Arc<AtomicBool>,
    worker_tx: &Sender<VoiceInbound>,
) -> Option<Session> {
    let active = Arc::new(AtomicBool::new(true));
    let phrase_tx = worker.phrase_sender();
    let listen_active = Arc::clone(&active);
    let error_tx = worker_tx.clone();
    let listen = thread::Builder::new()
        .name("voice-listen".to_string())
        .spawn(move || {
            if let Err(error) = audio::run_capture_session(&listen_active, &phrase_tx) {
                let _ = error_tx.send(VoiceInbound::AudioFailed(error.to_string()));
            }
        });
    let listen = match listen {
        Ok(join) => join,
        Err(error) => {
            tracing::error!(target: "klavium", "Голос: не удалось запустить поток захвата: {error}");
            deps.overlay
                .show_badge_timed("НЕТ МИКРОФОНА", BADGE_COLOR_ERROR, BADGE_TIMEOUT_MS);
            return None;
        }
    };
    active_flag.store(true, Ordering::SeqCst);
    deps.sound.play(SoundKind::VoiceStart);
    deps.overlay.show_badge("ГОЛОС", BADGE_COLOR_ACTIVE);
    tracing::info!(target: "klavium", "Голосовой ввод включён");
    Some(Session { active, listen })
}

fn stop_session(session: &mut Option<Session>, deps: &VoiceDeps, active_flag: &Arc<AtomicBool>) {
    let Some(session) = session.take() else {
        return;
    };
    session.active.store(false, Ordering::SeqCst);
    let _ = session.listen.join();
    active_flag.store(false, Ordering::SeqCst);
    deps.sound.play(SoundKind::VoiceStop);
    deps.overlay.hide_badge();
    tracing::info!(target: "klavium", "Голосовой ввод выключен");
}

/// Тестовая сессия: тот же захват, но без бейджа/звуков (результаты — в UI).
fn start_test_session(
    worker: &RecognitionWorker,
    worker_tx: &Sender<VoiceInbound>,
) -> Option<Session> {
    let active = Arc::new(AtomicBool::new(true));
    let phrase_tx = worker.phrase_sender();
    let listen_active = Arc::clone(&active);
    let error_tx = worker_tx.clone();
    let listen = thread::Builder::new()
        .name("voice-test-listen".to_string())
        .spawn(move || {
            if let Err(error) = audio::run_capture_session(&listen_active, &phrase_tx) {
                let _ = error_tx.send(VoiceInbound::AudioFailed(error.to_string()));
            }
        });
    match listen {
        Ok(join) => {
            tracing::info!(
                target: "klavium",
                "Голос: тест распознавания запущен ({TEST_SECONDS} с)"
            );
            Some(Session {
                active,
                listen: join,
            })
        }
        Err(error) => {
            tracing::error!(
                target: "klavium",
                "Голос: не удалось запустить тестовый захват: {error}"
            );
            None
        }
    }
}

fn stop_test_session(test: &mut Option<Session>, snapshot: &Arc<Mutex<RecognitionTest>>) {
    if let Some(session) = test.take() {
        session.active.store(false, Ordering::SeqCst);
        let _ = session.listen.join();
        tracing::info!(target: "klavium", "Голос: тест распознавания остановлен");
    }
    let mut state = lock(snapshot);
    state.running = false;
    state.waiting_engine = false;
    state.seconds_left = 0;
}

/// Обратный отсчёт теста и автостоп по истечении времени.
fn spawn_test_timer(worker_tx: &Sender<VoiceInbound>, snapshot: Arc<Mutex<RecognitionTest>>) {
    let tx = worker_tx.clone();
    let _ = thread::Builder::new()
        .name("voice-test-timer".to_string())
        .spawn(move || {
            for remaining in (1..=TEST_SECONDS).rev() {
                {
                    let mut state = lock(&snapshot);
                    if !state.running {
                        return; // тест остановили раньше
                    }
                    state.seconds_left = remaining;
                }
                thread::sleep(std::time::Duration::from_secs(1));
            }
            let _ = tx.send(VoiceInbound::TestStop);
        });
}

/// Обработать распознанную фразу: пунктуация, автозаглавные, вставка, статистика.
fn insert_phrase(text: &str, deps: &VoiceDeps, last_char: &mut Option<char>) {
    let (punct, capitalize) = {
        let guard = deps.config.read().unwrap_or_else(PoisonError::into_inner);
        (guard.voice.punct_dict, guard.voice.auto_capitalize)
    };
    let mut processed = text.to_string();
    if punct {
        processed = voice_text::apply_punctuation(&processed);
    }
    let mut processed = processed.trim().to_string();
    if processed.is_empty() {
        return;
    }
    if capitalize {
        processed = voice_text::capitalize_after(*last_char, &processed);
    }
    let processed = voice_text::ensure_trailing_space(&processed);
    *last_char = voice_text::last_significant_char(&processed);
    for _word in processed.split_whitespace() {
        deps.stats.add_word("voice");
    }
    tracing::info!(target: "klavium", "Голосовая вставка: {processed:?}");
    deps.engine.submit_voice_text(&processed);
}
