//! Воркер распознавания: живёт всё время работы приложения, принимает фразы
//! (16 кГц mono) и передаёт распознанный текст управляющему потоку.
//!
//! Движок выбирается настройкой `voice.engine` и **перезагружается на лету**:
//! воркер сравнивает настройки в цикле (или по команде `reload`) и пересоздаёт
//! движок без перезапуска приложения.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use klavium_core::config::AppConfig;
use klavium_core::Layout;

use super::engines::{self, EngineError, Recognizer};
use super::VoiceInbound;
use crate::runtime::SharedConfig;

/// Статус движка распознавания.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EngineStatus {
    /// Движок ещё инициализируется.
    Loading,
    /// Готов; описание движка.
    Ready(String),
    /// Модель/библиотека не найдены; где искали.
    NoModel(String),
    /// Ошибка инициализации.
    Failed(String),
}

/// Воркер распознавания с каналом фраз.
pub struct RecognitionWorker {
    phrase_tx: Sender<Vec<f32>>,
    shutdown: Arc<AtomicBool>,
    reload_requested: Arc<AtomicBool>,
    status: Arc<Mutex<EngineStatus>>,
    join: Option<thread::JoinHandle<()>>,
}

impl RecognitionWorker {
    /// Запустить воркер (инициализация движка — сразу в фоне).
    #[must_use]
    pub(crate) fn spawn(
        data_dir: PathBuf,
        config: SharedConfig,
        inbound: Sender<VoiceInbound>,
        status: Arc<Mutex<EngineStatus>>,
    ) -> Self {
        let (phrase_tx, phrase_rx) = std::sync::mpsc::channel::<Vec<f32>>();
        let keepalive = phrase_tx.clone();
        let shutdown = Arc::new(AtomicBool::new(false));
        let reload_requested = Arc::new(AtomicBool::new(false));
        let thread_shutdown = Arc::clone(&shutdown);
        let thread_reload = Arc::clone(&reload_requested);
        let thread_status = Arc::clone(&status);
        let join = thread::Builder::new()
            .name("voice-worker".to_string())
            .spawn(move || {
                worker_loop(
                    &phrase_rx,
                    keepalive,
                    &data_dir,
                    &config,
                    &inbound,
                    &thread_shutdown,
                    &thread_reload,
                    &thread_status,
                );
            })
            .ok();
        Self {
            phrase_tx,
            shutdown,
            reload_requested,
            status,
            join,
        }
    }

    /// Отправитель фраз для потока захвата (на сессию диктовки).
    #[must_use]
    pub fn phrase_sender(&self) -> Sender<Vec<f32>> {
        self.phrase_tx.clone()
    }

    /// Текущий статус движка.
    #[must_use]
    pub fn status(&self) -> EngineStatus {
        self.status
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Попросить воркер перечитать настройки и пересоздать движок.
    pub fn reload(&self) {
        self.reload_requested.store(true, Ordering::SeqCst);
    }

    /// Попросить воркер завершиться.
    pub fn stop(&self) {
        self.shutdown.store(true, Ordering::SeqCst);
    }

    /// Дождаться завершения воркера.
    pub fn join(&mut self) {
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

fn set_status(status: &Mutex<EngineStatus>, value: EngineStatus) {
    *status.lock().unwrap_or_else(PoisonError::into_inner) = value;
}

/// Настройки, влияющие на выбор движка (для отслеживания изменений).
#[derive(Debug, Clone, PartialEq, Eq)]
struct EngineSettings {
    engine: String,
    model: String,
    vosk_model: String,
}

impl EngineSettings {
    fn from(config: &AppConfig) -> Self {
        Self {
            engine: config.voice.engine.clone(),
            model: config.voice.model.clone(),
            vosk_model: config.voice.vosk_model_path.clone(),
        }
    }
}

/// Снять копию конфигурации.
fn config_snapshot(config: &SharedConfig) -> AppConfig {
    config
        .read()
        .unwrap_or_else(PoisonError::into_inner)
        .clone()
}

/// Создать движок и обновить статус (общий путь для старта и перезагрузки).
fn build_engine(
    config: &AppConfig,
    data_dir: &Path,
    status: &Mutex<EngineStatus>,
) -> Option<Box<dyn Recognizer>> {
    let started = Instant::now();
    match engines::create(config, data_dir) {
        Ok(engine) => {
            tracing::info!(
                target: "klavium",
                "Голос: движок «{}» готов за {:.1} с",
                engine.describe(),
                started.elapsed().as_secs_f32()
            );
            set_status(status, EngineStatus::Ready(engine.describe()));
            Some(engine)
        }
        Err(EngineError::Missing(where_)) => {
            tracing::warn!(target: "klavium", "Голос: движок не готов (нет: {where_})");
            set_status(status, EngineStatus::NoModel(where_));
            None
        }
        Err(EngineError::Failed(reason)) => {
            tracing::error!(target: "klavium", "Голос: ошибка движка: {reason}");
            set_status(status, EngineStatus::Failed(reason));
            None
        }
    }
}

/// Ждать остановки, сбрасывая приходящие фразы (движок недоступен).
fn drain_until_shutdown(phrase_rx: &Receiver<Vec<f32>>, shutdown: &AtomicBool) {
    while !shutdown.load(Ordering::SeqCst) {
        match phrase_rx.recv_timeout(Duration::from_millis(200)) {
            Ok(_) | Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn worker_loop(
    phrase_rx: &Receiver<Vec<f32>>,
    _keepalive: Sender<Vec<f32>>,
    data_dir: &Path,
    config: &SharedConfig,
    inbound: &Sender<VoiceInbound>,
    shutdown: &AtomicBool,
    reload_requested: &AtomicBool,
    status: &Mutex<EngineStatus>,
) {
    let snapshot = config_snapshot(config);
    let mut current = EngineSettings::from(&snapshot);
    let mut engine = build_engine(&snapshot, data_dir, status);
    if engine.is_none() {
        // Ждём смены настроек/команды перезагрузки (без постоянного ретрая).
        loop {
            if shutdown.load(Ordering::SeqCst) {
                return;
            }
            let requested = EngineSettings::from(&config_snapshot(config));
            let reload = reload_requested.swap(false, Ordering::SeqCst);
            if requested != current || reload {
                current = requested.clone();
                tracing::info!(target: "klavium", "Голос: перезагрузка движка (движок={})", current.engine);
                set_status(status, EngineStatus::Loading);
                let snapshot = config_snapshot(config);
                engine = build_engine(&snapshot, data_dir, status);
                if engine.is_some() {
                    break;
                }
            }
            match phrase_rx.recv_timeout(Duration::from_millis(200)) {
                Ok(_) | Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => return,
            }
        }
    }

    let mut engine = engine;
    while !shutdown.load(Ordering::SeqCst) {
        // Переключение движка на лету: настройки изменились или просили перезагрузку.
        let requested = EngineSettings::from(&config_snapshot(config));
        let reload = reload_requested.swap(false, Ordering::SeqCst);
        if requested != current || reload {
            current = requested.clone();
            tracing::info!(
                target: "klavium",
                "Голос: перезагружаю движок (движок={})",
                current.engine
            );
            set_status(status, EngineStatus::Loading);
            let snapshot = config_snapshot(config);
            engine = build_engine(&snapshot, data_dir, status);
        }

        match phrase_rx.recv_timeout(Duration::from_millis(200)) {
            Ok(samples) => {
                if samples.is_empty() {
                    continue;
                }
                let Some(active_engine) = engine.as_mut() else {
                    tracing::warn!(target: "klavium", "Голос: движок не готов — фраза пропущена");
                    continue;
                };
                let language = resolve_language(config);
                let started = Instant::now();
                match active_engine.recognize(&samples, &language) {
                    Ok(text) => {
                        let text = text.trim().to_string();
                        if text.is_empty() {
                            continue;
                        }
                        if klavium_core::voice::is_likely_hallucination(&text) {
                            tracing::info!(
                                target: "klavium",
                                "Голос: отфильтрован шум/галлюцинация: {text:?}"
                            );
                            continue;
                        }
                        tracing::info!(
                            target: "klavium",
                            "Голос: распознано {text:?} (язык={language}, {:.1} с)",
                            started.elapsed().as_secs_f32()
                        );
                        let _ = inbound.send(VoiceInbound::Phrase(text));
                    }
                    Err(error) => {
                        tracing::error!(target: "klavium", "Голос: ошибка распознавания: {error}");
                    }
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
    drain_until_shutdown(phrase_rx, shutdown);
    tracing::info!(target: "klavium", "Голос: воркер распознавания остановлен");
}

/// Язык распознавания: `ru`/`en` из настроек, иначе — по текущей раскладке.
fn resolve_language(config: &SharedConfig) -> String {
    let configured = {
        let guard = config.read().unwrap_or_else(PoisonError::into_inner);
        guard.voice.language.clone()
    };
    if configured == "ru" || configured == "en" {
        return configured;
    }
    match klavium_platform::layout::current() {
        Some(Layout::En) => "en".to_string(),
        Some(Layout::Ru) | None => "ru".to_string(),
    }
}
