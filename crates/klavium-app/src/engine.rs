//! Конвейер коррекции: буфер слов, очередь ошибок, единый воркер, F8 и пакетный режим.
//!
//! Порядок работы:
//! - слова копятся в буфере (буквы + «двусмысленные» символы);
//! - завершённое слово анализируется `smart_correction` (klavium-core);
//! - найденные ошибки попадают в очередь с учётом набранного «хвоста»
//!   (`text_after`) и разделителя (`terminator`);
//! - все исправления выполняются единым воркером — поток хука не блокируется;
//! - физический ввод подавляется на время коррекции;
//! - собственные инъекции помечены [`INJECT_EXTRA_INFO`] и не подавляются.
//!
//! Взаимодействие с ОС спрятано за трейтом [`EngineIo`]: реальная реализация —
//! [`crate::real_io::PlatformIo`], в тестах используется мок.

// Явные области блокировок состояния читаются лучше, чем микро-оптимизация
// времени жизни `MutexGuard`; nursery-правило отключено осознанно.
#![allow(clippy::significant_drop_tightening)]

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Mutex, MutexGuard, PoisonError, RwLock};
use std::thread;
use std::time::{Duration, Instant};

use klavium_core::detect::{detect_language, smart_correction, AMBIGUOUS_EN_CHARS};
use klavium_core::{convert, keys, Layout};
use klavium_platform::hook::INJECT_EXTRA_INFO;

/// Максимальная длина `text_after` у ожидающей ошибки (символов).
const MAX_TEXT_AFTER: usize = 500;
/// Максимальный возраст ошибки в очереди.
const MAX_ERROR_AGE: Duration = Duration::from_secs(60);
/// Как часто сверяем фокус внутри окна при работе с очередью.
const FOCUS_CHECK_INTERVAL: Duration = Duration::from_secs(1);
/// `VK_BACK`.
const VK_BACK: u32 = 0x08;

/// Цель ввода, пригодная для хранения между потоками (без сырых указателей).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StoredTarget {
    /// Дескриптор активного окна (HWND как usize; 0 — нет).
    pub window: usize,
    /// Дескриптор элемента фокуса (может быть 0).
    pub focus: usize,
}

/// Состояние модификаторов на момент события.
#[allow(clippy::struct_excessive_bools)]
// четыре независимых модификатора — данные, а не состояние
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Modifiers {
    /// Shift нажат.
    pub shift: bool,
    /// Ctrl нажат.
    pub ctrl: bool,
    /// Alt нажат.
    pub alt: bool,
    /// Win нажат.
    pub win: bool,
}

/// Абстракция над ОС — всё, что движку нужно снаружи.
pub trait EngineIo: Send + Sync {
    /// Снять текущую цель ввода (окно + фокус).
    fn capture_target(&self) -> StoredTarget;
    /// Вернуть фокус в сохранённую цель.
    fn restore_target(&self, target: &StoredTarget) -> bool;
    /// Позиция каретки ввода: `(x, y, высота строки)`.
    fn caret_pos(&self) -> Option<(i32, i32, i32)>;
    /// Текущая раскладка активного окна.
    fn current_layout(&self) -> Option<Layout>;
    /// Переключить раскладку активного окна.
    fn switch_layout(&self, target: Layout) -> bool;
    /// Отправить `count` нажатий Backspace.
    fn send_backspaces(&self, count: u32);
    /// Ввести текст (раскладко-независимо; `'\n'`/`'\t'` — Enter/Tab).
    fn send_text(&self, text: &str);
    /// Ввести текст с голосовой скоростью (по умолчанию — как [`Self::send_text`]).
    fn send_voice_text(&self, text: &str) {
        self.send_text(text);
    }
    /// Текущее состояние модификаторов.
    fn modifiers(&self) -> Modifiers;
}

/// Настройки движка.
#[derive(Debug, Clone)]
pub struct EngineConfig {
    /// Коррекция включена.
    pub enabled: bool,
    /// Автоматически исправлять без подтверждения.
    pub auto_correct: bool,
    /// Порог уверенности для авто/подсказки (по умолчанию 0.7).
    pub threshold: f64,
    /// Хоткей ручного исправления (по умолчанию `F8`).
    pub hotkey_correct: String,
    /// Пауза между словами одного набора (по умолчанию 5 с).
    pub word_timeout: Duration,
    /// Выполнять задания синхронно (используется тестами).
    pub run_jobs_inline: bool,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            auto_correct: false,
            threshold: 0.7,
            hotkey_correct: "F8".to_string(),
            word_timeout: Duration::from_secs(5),
            run_jobs_inline: false,
        }
    }
}

/// Событие клавиши (нормализованный вход движка).
#[derive(Debug, Clone, Copy)]
pub struct KeyInput {
    /// Виртуальный код клавиши.
    pub vk_code: u32,
    /// Скан-код.
    pub scan_code: u32,
    /// Нажатие (`true`) или отпускание.
    pub is_down: bool,
    /// Событие синтезировано.
    pub injected: bool,
    /// `dwExtraInfo` события.
    pub extra_info: usize,
}

impl KeyInput {
    /// Собрать вход движка из события клавиатурного хука.
    #[must_use]
    pub const fn from_hook(event: &klavium_platform::hook::KeyEvent) -> Self {
        Self {
            vk_code: event.vk_code,
            scan_code: event.scan_code,
            is_down: event.is_down,
            injected: event.injected,
            extra_info: event.extra_info,
        }
    }
}

/// Событие движка для слоя UI (пока — лог; Ф5 добавит оверлеи).
#[derive(Debug, Clone)]
pub enum EngineEvent {
    /// Обнаружена ошибка раскладки (показывается подсказка).
    ErrorDetected {
        /// Исходное слово.
        original: String,
        /// Предлагаемое исправление.
        corrected: String,
        /// Уверенность 0.0–1.0.
        confidence: f64,
        /// Сколько ошибок ждёт в очереди.
        pending: usize,
        /// Позиция каретки `(x, y, высота строки)` для показа подсказки (Ф5).
        screen: Option<(i32, i32, i32)>,
    },
    /// Коррекция выполнена.
    CorrectionApplied {
        /// Что исправлялось (слово или span).
        original: String,
        /// Итоговый введённый текст (без уже набранного хвоста).
        corrected: String,
        /// Пакетный режим (несколько ошибок за раз).
        batch: bool,
    },
    /// Очередь ошибок очищена (смена окна/поля и т.п.).
    QueueCleared {
        /// Причина.
        reason: String,
    },
    /// Началось новое слово (для статистики и звуков раскладки).
    WordStarted {
        /// Раскладка, под которой слово началось.
        layout: Option<Layout>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum KeyType {
    #[default]
    Other,
    Alpha,
    Ambiguous,
    Digit,
    Space,
    Punctuation,
}

// Поля `confidence`, `target_layout`, `screen` понадобятся в Ф5–Ф7
// (подсказка у каретки, статистика); до этого момента чтений нет.
#[allow(dead_code)]
#[derive(Clone)]
struct PendingError {
    id: u64,
    original: String,
    corrected: String,
    confidence: f64,
    target_layout: Option<Layout>,
    screen: Option<(i32, i32, i32)>,
    input_target: StoredTarget,
    terminator: Option<String>,
    text_after: String,
    timestamp: Instant,
}

#[derive(Default)]
struct InnerState {
    current_word: String,
    word_start_layout: Option<Layout>,
    input_context: VecDeque<String>,
    pending_tail: String,
    pending_errors: Vec<PendingError>,
    last_key_time: Option<Instant>,
    last_key_type: KeyType,
    last_window: usize,
    last_focus: usize,
    last_focus_check: Option<Instant>,
    next_id: u64,
}

#[derive(Clone)]
enum Job {
    ApplyPending,
    Correct { id: u64 },
    VoiceInsert { text: String },
}

/// Движок коррекции. Создаётся один раз и разделяется между потоками.
pub struct CorrectionEngine {
    state: Mutex<InnerState>,
    suppress: AtomicBool,
    io: std::sync::Arc<dyn EngineIo>,
    config: RwLock<EngineConfig>,
    events: Sender<EngineEvent>,
    jobs: Mutex<Option<Sender<Job>>>,
}

impl CorrectionEngine {
    /// Создать движок (и, если не включён inline-режим, поток-воркер).
    #[must_use]
    pub fn spawn(
        io: std::sync::Arc<dyn EngineIo>,
        config: EngineConfig,
        events: Sender<EngineEvent>,
    ) -> std::sync::Arc<Self> {
        let inline = config.run_jobs_inline;
        let engine = std::sync::Arc::new(Self {
            state: Mutex::new(InnerState::default()),
            suppress: AtomicBool::new(false),
            io,
            config: RwLock::new(config),
            events,
            jobs: Mutex::new(None),
        });

        if !inline {
            let (tx, rx) = mpsc::channel::<Job>();
            // Weak-ссылка: воркер НЕ удерживает движок живым — иначе на выходе
            // получается цикл (движок → очередь → воркер → движок) и процесс виснет.
            let worker = std::sync::Arc::downgrade(&engine);
            let spawned = thread::Builder::new()
                .name("correction-worker".to_string())
                .spawn(move || {
                    while let Ok(job) = rx.recv() {
                        if let Some(engine) = worker.upgrade() {
                            engine.run_job(job);
                        }
                    }
                });
            if spawned.is_ok() {
                *engine.lock_jobs() = Some(tx);
            }
            // при ошибке создания потока остаёмся в синхронном режиме
        }
        engine
    }

    /// Сколько ошибок ждёт в очереди.
    #[must_use]
    pub fn pending_errors_count(&self) -> usize {
        self.lock_state().pending_errors.len()
    }

    /// Идёт ли сейчас коррекция (физический ввод подавляется).
    #[must_use]
    pub fn is_correcting(&self) -> bool {
        self.suppress.load(Ordering::SeqCst)
    }

    /// Обновить настройки движка на лету (из окна настроек).
    pub fn update_config(&self, config: EngineConfig) {
        *self.config.write().unwrap_or_else(PoisonError::into_inner) = config;
    }

    /// Снимок текущих настроек.
    fn config(&self) -> EngineConfig {
        self.config
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Запросить ручное исправление очереди (F8). Неблокирующе.
    pub fn request_apply(&self) {
        self.submit_job(Job::ApplyPending);
    }

    /// Вставить продиктованный текст (голосовая вставка). Неблокирующе.
    ///
    /// Выполняется в воркере под тем же подавлением физического ввода, что и
    /// коррекция, — вставка не пересекается с авто-правкой и F8.
    pub fn submit_voice_text(&self, text: &str) {
        self.submit_job(Job::VoiceInsert {
            text: text.to_string(),
        });
    }

    /// Обработать событие клавиши (вызывается из потока хука; быстро).
    ///
    /// Возвращает `true`, если событие нужно подавить.
    #[must_use]
    pub fn process_key(&self, key: &KeyInput) -> bool {
        if key.extra_info == INJECT_EXTRA_INFO {
            return false;
        }
        if !key.is_down {
            return false;
        }
        if self.is_correcting() {
            return true;
        }

        if key.vk_code == VK_BACK {
            self.track_backspace();
            return false;
        }
        if is_modifier_vk(key.vk_code) {
            return false;
        }

        let modifiers = self.io.modifiers();
        let config = self.config();
        if crate::hotkeys::matches(
            &config.hotkey_correct,
            key.vk_code,
            key.scan_code,
            &modifiers,
        ) {
            self.request_apply();
            return true;
        }

        let layout = self.io.current_layout();
        let Some(ch) = keys::resolve_char(key.vk_code, key.scan_code, layout, modifiers.shift)
        else {
            return false;
        };
        self.track_typed_char(ch);
        self.handle_char(ch, layout);
        false
    }

    // ==================== внутреннее ====================

    fn lock_state(&self) -> MutexGuard<'_, InnerState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn lock_jobs(&self) -> MutexGuard<'_, Option<Sender<Job>>> {
        self.jobs.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn submit_job(&self, job: Job) {
        let sender = self.lock_jobs().clone();
        match sender {
            Some(tx) => {
                let _ = tx.send(job);
            }
            None => self.run_job(job),
        }
    }

    fn run_job(&self, job: Job) {
        match job {
            Job::ApplyPending => self.apply_pending(),
            Job::Correct { id } => {
                let _ = self.perform_correction(id);
            }
            Job::VoiceInsert { text } => self.perform_voice_insert(&text),
        }
    }

    /// Добавить символ к `text_after` всех ожидающих ошибок.
    fn track_typed_char(&self, ch: char) {
        self.check_input_target_changed();
        let mut state = self.lock_state();
        if state.pending_errors.is_empty() {
            return;
        }
        for error in &mut state.pending_errors {
            error.text_after.push(ch);
        }
        state
            .pending_errors
            .retain(|error| error.text_after.chars().count() <= MAX_TEXT_AFTER);
    }

    /// Учесть вставленный текст целиком (голосовая вставка) в `text_after`
    /// ожидающих ошибок — иначе F8 удалит не тот фрагмент.
    fn track_typed_text(&self, text: &str) {
        self.check_input_target_changed();
        let mut state = self.lock_state();
        if state.pending_errors.is_empty() {
            return;
        }
        for error in &mut state.pending_errors {
            error.text_after.push_str(text);
        }
        state
            .pending_errors
            .retain(|error| error.text_after.chars().count() <= MAX_TEXT_AFTER);
    }

    /// Backspace: снять символ с `text_after` и из буферов слова.
    fn track_backspace(&self) {
        self.check_input_target_changed();
        let mut state = self.lock_state();
        state
            .pending_errors
            .retain_mut(|error| error.text_after.pop().is_some());
        if !state.pending_tail.is_empty() {
            state.pending_tail.pop();
        } else if !state.current_word.is_empty() {
            state.current_word.pop();
        }
    }

    /// Если окно/поле ввода сменилось — очередь ошибок неактуальна.
    fn check_input_target_changed(&self) {
        {
            let state = self.lock_state();
            if state.pending_errors.is_empty() {
                return;
            }
        }
        let current = self.io.capture_target();
        let mut state = self.lock_state();
        let Some(first) = state.pending_errors.first() else {
            return;
        };
        let stored_window = first.input_target.window;
        let stored_focus = first.input_target.focus;

        if current.window != stored_window {
            self.clear_queue_locked(&mut state, "фокус перешёл в другое окно");
            return;
        }

        let now = Instant::now();
        let focus_check_due = state
            .last_focus_check
            .is_none_or(|t| now.saturating_duration_since(t) >= FOCUS_CHECK_INTERVAL);
        if stored_focus != 0 && focus_check_due {
            state.last_focus_check = Some(now);
            if current.focus != 0 && current.focus != stored_focus {
                self.clear_queue_locked(&mut state, "фокус перешёл в другое поле");
            }
        }
    }

    fn clear_queue_locked(&self, state: &mut InnerState, reason: &str) {
        state.pending_errors.clear();
        state.input_context.clear();
        state.pending_tail.clear();
        state.current_word.clear();
        state.last_focus = 0;
        let _ = self.events.send(EngineEvent::QueueCleared {
            reason: reason.to_string(),
        });
    }

    /// Смена окна/поля — буферы слова и очередь сбрасываются.
    fn check_focus_and_clear_buffers(&self) {
        let current = self.io.capture_target();
        if current.window == 0 {
            return;
        }
        let mut state = self.lock_state();
        if state.last_window != current.window {
            state.current_word.clear();
            state.pending_tail.clear();
            state.pending_errors.clear();
            state.last_window = current.window;
            state.last_focus = 0;
            return;
        }
        if current.focus != 0
            && state.last_focus != 0
            && current.focus != state.last_focus
            && (!state.current_word.is_empty() || !state.pending_errors.is_empty())
        {
            state.current_word.clear();
            state.pending_tail.clear();
            state.pending_errors.clear();
        }
        state.last_focus = current.focus;
    }

    /// Обработка символа: буфер слова, пауза, завершение слова.
    fn handle_char(&self, ch: char, layout: Option<Layout>) {
        self.check_focus_and_clear_buffers();
        let now = Instant::now();
        let word_timeout = self.config().word_timeout;
        let key_type = classify_char(ch, layout);
        let word_char = is_word_key_type(key_type);

        let mut complete_needed = false;
        let mut starts_new_word = false;
        {
            let mut state = self.lock_state();
            let since_last = state
                .last_key_time
                .map_or(Duration::MAX, |t| now.saturating_duration_since(t));
            if word_char {
                let previous_is_word = is_word_key_type(state.last_key_type);
                let pause = !state.current_word.is_empty() && since_last > word_timeout;
                if !previous_is_word || pause {
                    complete_needed = true;
                    starts_new_word = true;
                }
            } else {
                state.pending_tail.push(ch);
                if !state.current_word.is_empty() {
                    complete_needed = true;
                }
            }
        }
        if complete_needed {
            self.complete_word();
        }

        {
            let mut state = self.lock_state();
            if word_char {
                state.current_word.push(ch);
                if starts_new_word {
                    state.word_start_layout = layout;
                }
            }
            state.last_key_time = Some(now);
            state.last_key_type = key_type;
        }
        if word_char && starts_new_word {
            let _ = self.events.send(EngineEvent::WordStarted { layout });
        }
    }

    /// Завершить текущее слово: анализ, постановка ошибки в очередь.
    fn complete_word(&self) {
        let (word, context, terminator, word_layout) = {
            let mut state = self.lock_state();
            if state.current_word.is_empty() {
                state.pending_tail.clear();
                return;
            }
            let word = std::mem::take(&mut state.current_word);
            // Контекст — снапшот ДО добавления текущего слова.
            let context: Vec<String> = state.input_context.iter().cloned().collect();
            state.input_context.push_back(word.clone());
            while state.input_context.len() > 10 {
                state.input_context.pop_front();
            }
            let terminator = if state.pending_tail.is_empty() {
                None
            } else {
                Some(std::mem::take(&mut state.pending_tail))
            };
            let word_layout = state.word_start_layout.take();
            (word, context, terminator, word_layout)
        };

        if word.chars().count() < 2 {
            return;
        }
        let config = self.config();
        if !config.enabled {
            return;
        }

        let word_layout = word_layout.or_else(|| self.io.current_layout());
        let context_refs: Vec<&str> = context.iter().map(String::as_str).collect();
        let result = smart_correction(&word, &context_refs, word_layout);
        if !result.needs_correction {
            return;
        }

        let screen = self.io.caret_pos();
        let input_target = self.io.capture_target();

        let (id, pending) = {
            let mut state = self.lock_state();
            let id = state.next_id;
            state.next_id += 1;
            state.pending_errors.push(PendingError {
                id,
                original: word.clone(),
                corrected: result.corrected.clone(),
                confidence: result.confidence,
                target_layout: result.target_layout,
                screen,
                input_target,
                terminator,
                text_after: String::new(),
                timestamp: Instant::now(),
            });
            (id, state.pending_errors.len())
        };

        // Диспатч: порог → авто/подсказка.
        if result.confidence < config.threshold {
            return;
        }
        if config.auto_correct {
            self.submit_job(Job::Correct { id });
        } else {
            let _ = self.events.send(EngineEvent::ErrorDetected {
                original: word,
                corrected: result.corrected,
                confidence: result.confidence,
                pending,
                screen,
            });
        }
    }

    /// F8: исправить очередь для текущего поля (одиночная или пакетная коррекция).
    fn apply_pending(&self) {
        self.cleanup_old_errors();

        let current = self.io.capture_target();
        {
            let mut state = self.lock_state();
            if state.pending_errors.is_empty() {
                return;
            }
            if current.window != 0 {
                let mismatch = state
                    .pending_errors
                    .iter()
                    .any(|e| e.input_target.window != 0 && e.input_target.window != current.window);
                if mismatch {
                    self.clear_queue_locked(&mut state, "очередь относится к другому окну");
                    return;
                }
            }
        }

        let ids = self.same_field_errors(&current);
        if ids.is_empty() {
            return;
        }
        if ids.len() == 1 {
            let _ = self.perform_correction(ids[0]);
        } else {
            let _ = self.perform_batch(&ids);
        }
    }

    fn cleanup_old_errors(&self) {
        let mut state = self.lock_state();
        state
            .pending_errors
            .retain(|error| error.timestamp.elapsed() < MAX_ERROR_AGE);
    }

    /// Ошибки, относящиеся к текущей цели ввода.
    fn same_field_errors(&self, current: &StoredTarget) -> Vec<u64> {
        let state = self.lock_state();
        let errors = &state.pending_errors;
        let Some(first) = errors.first() else {
            return Vec::new();
        };
        if errors.iter().all(|e| e.input_target == first.input_target) {
            return errors.iter().map(|e| e.id).collect();
        }
        errors
            .iter()
            .filter(|e| e.input_target == *current)
            .map(|e| e.id)
            .collect()
    }

    fn error_snapshot(&self, id: u64) -> Option<PendingError> {
        let state = self.lock_state();
        state.pending_errors.iter().find(|e| e.id == id).cloned()
    }

    fn errors_snapshot(&self, ids: &[u64]) -> Vec<PendingError> {
        let state = self.lock_state();
        state
            .pending_errors
            .iter()
            .filter(|e| ids.contains(&e.id))
            .cloned()
            .collect()
    }

    fn remove_error(&self, id: u64) {
        let mut state = self.lock_state();
        state.pending_errors.retain(|e| e.id != id);
    }

    /// Одиночная коррекция: удалить слово (+разделитель+хвост), сменить
    /// раскладку и ввести исправленное, восстановив хвост.
    /// Голосовая вставка: вывод текста от имени пользователя под подавлением
    /// физического ввода; продиктованное учитывается в `text_after` ошибок.
    fn perform_voice_insert(&self, text: &str) {
        if text.is_empty() {
            return;
        }
        let target = self.io.capture_target();
        let _guard = SuppressGuard::new(&self.suppress);
        let _ = self.io.restore_target(&target);
        self.io.send_voice_text(text);
        self.track_typed_text(text);
    }

    fn perform_correction(&self, id: u64) -> bool {
        let Some(error) = self.error_snapshot(id) else {
            return false;
        };
        if error.original.is_empty()
            || error.corrected.is_empty()
            || error.original == error.corrected
        {
            self.remove_error(id);
            return false;
        }

        let target_layout = detect_target_layout(&error.original, &error.corrected);
        let mut terminator = error.terminator.clone();
        let mut text_after = error.text_after.clone();
        if let Some(target) = target_layout {
            let source = target.opposite();
            if let Some(text) = &terminator {
                let converted = convert::convert_punct(text, source.as_str(), target.as_str());
                if converted != *text {
                    terminator = Some(converted);
                }
            }
            if !text_after.is_empty() {
                let converted =
                    convert::convert_punct(&text_after, source.as_str(), target.as_str());
                if converted != text_after {
                    text_after = converted;
                }
            }
        }

        let _guard = SuppressGuard::new(&self.suppress);
        let original_layout = self.io.current_layout();
        let _ = self.io.restore_target(&error.input_target);

        let mut count = error.original.chars().count() as u32;
        if let Some(text) = &terminator {
            count += text.chars().count() as u32;
        }
        count += text_after.chars().count() as u32;
        self.io.send_backspaces(count);

        if let Some(target) = target_layout {
            if Some(target) != original_layout {
                let _ = self.io.switch_layout(target);
            }
        }

        self.io.send_text(&error.corrected);
        if let Some(text) = &terminator {
            self.io.send_text(text);
        }
        if !text_after.is_empty() {
            self.io.send_text(&text_after);
        }

        {
            let term = terminator.unwrap_or_default();
            let old_suffix = format!("{}{}", error.original, term);
            let new_suffix = format!("{}{}", error.corrected, term);
            let mut state = self.lock_state();
            if old_suffix != new_suffix {
                for other in &mut state.pending_errors {
                    if other.text_after.ends_with(&old_suffix) {
                        let cut = other.text_after.len() - old_suffix.len();
                        other.text_after.truncate(cut);
                        other.text_after.push_str(&new_suffix);
                    }
                }
            }
            state.pending_errors.retain(|e| e.id != id);
        }

        let _ = self.events.send(EngineEvent::CorrectionApplied {
            original: error.original,
            corrected: error.corrected,
            batch: false,
        });
        true
    }

    /// Пакетная коррекция: весь span от первой ошибки до курсора за один проход.
    fn perform_batch(&self, ids: &[u64]) -> bool {
        let errors = self.errors_snapshot(ids);
        if errors.len() < 2 {
            if let Some(error) = errors.first() {
                let id = error.id;
                return self.perform_correction(id);
            }
            return false;
        }

        let first_target = errors[0].input_target;
        if errors.iter().any(|e| e.input_target != first_target) {
            return false; // ошибки из разных полей — не трогаем
        }

        let original_span = original_span_for(&errors);
        let (corrected_text, chain_ok) = build_corrected_text(&errors);
        if !chain_ok {
            // Цепочка разорвана — фолбэк на последнее слово.
            let last_id = errors[errors.len() - 1].id;
            return self.perform_correction(last_id);
        }
        if original_span == corrected_text {
            for error in &errors {
                self.remove_error(error.id);
            }
            return true;
        }

        let target_layout = detect_target_layout(&errors[0].original, &errors[0].corrected);
        let mut final_text = corrected_text;
        if let Some(target) = target_layout {
            let source = target.opposite();
            let converted = convert::convert_punct(&final_text, source.as_str(), target.as_str());
            if converted != final_text {
                final_text = converted;
            }
        }

        let _guard = SuppressGuard::new(&self.suppress);
        let original_layout = self.io.current_layout();
        let _ = self.io.restore_target(&first_target);
        self.io
            .send_backspaces(original_span.chars().count() as u32);
        if let Some(target) = target_layout {
            if Some(target) != original_layout {
                let _ = self.io.switch_layout(target);
            }
        }
        self.io.send_text(&final_text);

        {
            let mut state = self.lock_state();
            state.pending_errors.retain(|e| !ids.contains(&e.id));
        }

        let _ = self.events.send(EngineEvent::CorrectionApplied {
            original: original_span,
            corrected: final_text,
            batch: true,
        });
        true
    }
}

/// На время жизни подавляет физический ввод (снимает флаг в `Drop`).
struct SuppressGuard<'a>(&'a AtomicBool);

impl<'a> SuppressGuard<'a> {
    fn new(flag: &'a AtomicBool) -> Self {
        flag.store(true, Ordering::SeqCst);
        Self(flag)
    }
}

impl Drop for SuppressGuard<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

const fn is_modifier_vk(vk: u32) -> bool {
    matches!(
        vk,
        0x10 | 0x11 | 0x12 | 0x5B | 0x5C | 0xA0 | 0xA1 | 0xA2 | 0xA3 | 0xA4 | 0xA5
    )
}

const fn is_word_key_type(key_type: KeyType) -> bool {
    matches!(key_type, KeyType::Alpha | KeyType::Ambiguous)
}

fn classify_char(c: char, layout: Option<Layout>) -> KeyType {
    if c.is_alphabetic() {
        return KeyType::Alpha;
    }
    if layout != Some(Layout::Ru) && AMBIGUOUS_EN_CHARS.contains(c) {
        return KeyType::Ambiguous;
    }
    if c.is_ascii_digit() {
        return KeyType::Digit;
    }
    if c.is_whitespace() || c == '\n' || c == '\r' || c == '\t' {
        return KeyType::Space;
    }
    if ".,!?;:()[]{}\"'\u{ab}\u{bb}".contains(c) {
        return KeyType::Punctuation;
    }
    KeyType::Other
}

const fn is_cyrillic(c: char) -> bool {
    matches!(c, 'а'..='я' | 'А'..='Я' | 'ё' | 'Ё')
}

/// Целевая раскладка по различию языков оригинала и исправления.
fn detect_target_layout(original: &str, corrected: &str) -> Option<Layout> {
    let (original_lang, _) = detect_language(original);
    let (corrected_lang, _) = detect_language(corrected);
    if original_lang == Some(Layout::En) && corrected_lang == Some(Layout::Ru) {
        return Some(Layout::Ru);
    }
    if original_lang == Some(Layout::Ru) && corrected_lang == Some(Layout::En) {
        return Some(Layout::En);
    }

    let original_en = original.chars().any(|c| c.is_ascii_alphabetic());
    let original_ru = original.chars().any(is_cyrillic);
    let corrected_en = corrected.chars().any(|c| c.is_ascii_alphabetic());
    let corrected_ru = corrected.chars().any(is_cyrillic);

    if original_en && !original_ru && corrected_ru && !corrected_en {
        return Some(Layout::Ru);
    }
    if original_ru && !original_en && corrected_en && !corrected_ru {
        return Some(Layout::En);
    }
    None
}

/// Исходный фрагмент от первой ошибки до курсора.
fn original_span_for(errors: &[PendingError]) -> String {
    let first = &errors[0];
    let mut span = first.original.clone();
    if let Some(text) = &first.terminator {
        span.push_str(text);
    }
    span.push_str(&first.text_after);
    span
}

/// Восстановить исправленный текст из цепочки ошибок.
fn build_corrected_text(errors: &[PendingError]) -> (String, bool) {
    let mut chain_ok = true;
    let mut result = errors[0].corrected.clone();
    if let Some(text) = &errors[0].terminator {
        result.push_str(text);
    }

    for pair in errors.windows(2) {
        let previous_after = &pair[0].text_after;
        let current = &pair[1];

        let mut suffix = current.original.clone();
        if let Some(text) = &current.terminator {
            suffix.push_str(text);
        }
        suffix.push_str(&current.text_after);

        let stripped = if suffix.is_empty() {
            None
        } else {
            previous_after.strip_suffix(&suffix)
        };
        if stripped.is_none() {
            chain_ok = false;
        }
        let prefix = stripped.unwrap_or("");

        result.push_str(prefix);
        result.push_str(&current.corrected);
        if let Some(text) = &current.terminator {
            result.push_str(text);
        }
    }

    result.push_str(&errors[errors.len() - 1].text_after);
    (result, chain_ok)
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc::Receiver;
    use std::sync::Arc;

    use super::*;

    #[derive(Debug, Clone, PartialEq)]
    enum Op {
        Restore,
        Backspaces(u32),
        Switch(Layout),
        Text(String),
    }

    struct MockIoState {
        layout: Option<Layout>,
        target: StoredTarget,
        ops: Vec<Op>,
        modifiers: Modifiers,
    }

    struct MockIo(Mutex<MockIoState>);

    impl MockIo {
        fn new() -> Self {
            Self(Mutex::new(MockIoState {
                layout: Some(Layout::En),
                target: StoredTarget {
                    window: 1,
                    focus: 2,
                },
                ops: Vec::new(),
                modifiers: Modifiers::default(),
            }))
        }

        fn ops(&self) -> Vec<Op> {
            self.0.lock().unwrap().ops.clone()
        }

        fn set_layout(&self, layout: Layout) {
            self.0.lock().unwrap().layout = Some(layout);
        }

        fn set_target_window(&self, window: usize) {
            self.0.lock().unwrap().target.window = window;
        }
    }

    impl EngineIo for MockIo {
        fn capture_target(&self) -> StoredTarget {
            self.0.lock().unwrap().target
        }

        fn restore_target(&self, _target: &StoredTarget) -> bool {
            self.0.lock().unwrap().ops.push(Op::Restore);
            true
        }

        fn caret_pos(&self) -> Option<(i32, i32, i32)> {
            Some((0, 0, 20))
        }

        fn current_layout(&self) -> Option<Layout> {
            self.0.lock().unwrap().layout
        }

        fn switch_layout(&self, target: Layout) -> bool {
            let mut state = self.0.lock().unwrap();
            state.ops.push(Op::Switch(target));
            state.layout = Some(target);
            true
        }

        fn send_backspaces(&self, count: u32) {
            self.0.lock().unwrap().ops.push(Op::Backspaces(count));
        }

        fn send_text(&self, text: &str) {
            self.0.lock().unwrap().ops.push(Op::Text(text.to_string()));
        }

        fn modifiers(&self) -> Modifiers {
            self.0.lock().unwrap().modifiers
        }
    }

    fn make_engine() -> (Arc<CorrectionEngine>, Arc<MockIo>, Receiver<EngineEvent>) {
        let io = Arc::new(MockIo::new());
        let (tx, rx) = mpsc::channel();
        let config = EngineConfig {
            run_jobs_inline: true,
            ..EngineConfig::default()
        };
        let engine = CorrectionEngine::spawn(io.clone(), config, tx);
        (engine, io, rx)
    }

    fn key_for(ch: char) -> KeyInput {
        let (vk, scan) = match ch {
            'g' => (0x47, 0x22),
            'h' => (0x48, 0x23),
            'b' => (0x42, 0x30),
            'd' => (0x44, 0x20),
            't' => (0x54, 0x14),
            'n' => (0x4E, 0x31),
            'c' => (0x43, 0x2E),
            ' ' => (0x20, 0x39),
            other => panic!("нет маппинга в тесте для {other:?}"),
        };
        KeyInput {
            vk_code: vk,
            scan_code: scan,
            is_down: true,
            injected: false,
            extra_info: 0,
        }
    }

    fn press(engine: &CorrectionEngine, ch: char) {
        let key = key_for(ch);
        let _ = engine.process_key(&key);
    }

    fn press_str(engine: &CorrectionEngine, text: &str) {
        for ch in text.chars() {
            press(engine, ch);
        }
    }

    fn backspace_key() -> KeyInput {
        KeyInput {
            vk_code: 0x08,
            scan_code: 0x0E,
            is_down: true,
            injected: false,
            extra_info: 0,
        }
    }

    /// Прочитать следующее «значимое» событие (пропуская `WordStarted`).
    fn recv_meaningful(events: &Receiver<EngineEvent>) -> EngineEvent {
        loop {
            let event = events.recv_timeout(Duration::from_secs(1)).unwrap();
            if !matches!(event, EngineEvent::WordStarted { .. }) {
                return event;
            }
        }
    }

    #[test]
    fn test_single_word_error_then_f8() {
        let (engine, io, events) = make_engine();
        press_str(&engine, "ghbdtn ");
        assert_eq!(engine.pending_errors_count(), 1);

        let event = recv_meaningful(&events);
        match event {
            EngineEvent::ErrorDetected {
                original,
                corrected,
                pending,
                ..
            } => {
                assert_eq!(original, "ghbdtn");
                assert_eq!(corrected, "привет");
                assert_eq!(pending, 1);
            }
            other => panic!("неожиданное событие: {other:?}"),
        }

        engine.request_apply();
        assert_eq!(engine.pending_errors_count(), 0);
        assert_eq!(
            io.ops(),
            vec![
                Op::Restore,
                Op::Backspaces(7),
                Op::Switch(Layout::Ru),
                Op::Text("привет".to_string()),
                Op::Text(" ".to_string()),
            ]
        );

        let applied = recv_meaningful(&events);
        assert!(matches!(
            applied,
            EngineEvent::CorrectionApplied { batch: false, .. }
        ));

        // Повторный F8 ничего не делает
        engine.request_apply();
        assert_eq!(io.ops().len(), 5);
    }

    #[test]
    fn test_voice_insert_types_and_tracks() {
        let (engine, io, _events) = make_engine();
        press_str(&engine, "ghbdtn ");
        assert_eq!(engine.pending_errors_count(), 1);

        // Вставка от имени пользователя: восстановление цели + текст; подавление снято.
        engine.submit_voice_text("привет ");
        let ops = io.ops();
        assert!(matches!(ops.last(), Some(Op::Text(text)) if text == "привет "));
        assert!(!engine.is_correcting());
        assert_eq!(engine.pending_errors_count(), 1);

        // F8 после вставки удаляет слово вместе с «хвостом» (продиктованным текстом).
        engine.request_apply();
        assert_eq!(engine.pending_errors_count(), 0);
        assert!(io.ops().contains(&Op::Backspaces(14)));
        assert!(matches!(io.ops().last(), Some(Op::Text(text)) if text == "привет "));
    }

    #[test]
    fn test_f8_hotkey_key_triggers_apply() {
        let (engine, io, _events) = make_engine();
        press_str(&engine, "ghbdtn ");
        let f8 = KeyInput {
            vk_code: 0x77,
            scan_code: 0x42,
            is_down: true,
            injected: false,
            extra_info: 0,
        };
        let suppressed = engine.process_key(&f8);
        assert!(suppressed, "F8 должен подавляться");
        assert!(io.ops().contains(&Op::Backspaces(7)));
    }

    #[test]
    fn test_batch_correction_two_errors() {
        let (engine, io, events) = make_engine();
        press_str(&engine, "ghbdtn ");
        press_str(&engine, "ntcn ");
        assert_eq!(engine.pending_errors_count(), 2);

        engine.request_apply();
        assert_eq!(engine.pending_errors_count(), 0);
        assert_eq!(
            io.ops(),
            vec![
                Op::Restore,
                Op::Backspaces(12), // "ghbdtn " + "ntcn "
                Op::Switch(Layout::Ru),
                Op::Text("привет тест ".to_string()),
            ]
        );
        let mut batch_flag = None;
        while let Ok(event) = events.recv_timeout(Duration::from_secs(1)) {
            if let EngineEvent::CorrectionApplied { batch, .. } = event {
                batch_flag = Some(batch);
                break;
            }
        }
        assert_eq!(batch_flag, Some(true));
    }

    #[test]
    fn test_backspace_trims_text_after() {
        let (engine, io, _events) = make_engine();
        press_str(&engine, "ghbdtn ");
        press_str(&engine, "ntc");
        let _ = engine.process_key(&backspace_key());

        engine.request_apply();
        assert_eq!(engine.pending_errors_count(), 0);
        assert_eq!(
            io.ops(),
            vec![
                Op::Restore,
                Op::Backspaces(9), // "ghbdtn "(7) + "nt"(2)
                Op::Switch(Layout::Ru),
                Op::Text("привет".to_string()),
                Op::Text(" ".to_string()),
                Op::Text("nt".to_string()),
            ]
        );
    }

    #[test]
    fn test_focus_change_clears_queue() {
        let (engine, io, events) = make_engine();
        press_str(&engine, "ghbdtn ");
        assert_eq!(engine.pending_errors_count(), 1);

        io.set_target_window(999);
        press(&engine, 'g'); // первое же событие заметит смену окна

        assert_eq!(engine.pending_errors_count(), 0);
        let first = recv_meaningful(&events);
        assert!(matches!(first, EngineEvent::ErrorDetected { .. }));
        let cleared = recv_meaningful(&events);
        assert!(matches!(cleared, EngineEvent::QueueCleared { .. }));

        engine.request_apply();
        assert_eq!(io.ops(), Vec::<Op>::new());
    }

    #[test]
    fn test_meaningful_word_is_not_queued() {
        let (engine, io, events) = make_engine();
        io.set_layout(Layout::Ru);
        press_str(&engine, "ghbdtn "); // в RU-раскладке это «привет»

        assert_eq!(engine.pending_errors_count(), 0);
        engine.request_apply();
        assert_eq!(io.ops(), Vec::<Op>::new());
        while let Ok(event) = events.recv_timeout(Duration::from_millis(50)) {
            assert!(
                matches!(event, EngineEvent::WordStarted { .. }),
                "лишнее событие: {event:?}"
            );
        }
    }

    #[test]
    fn test_own_injections_are_passed_through() {
        let (engine, _io, _events) = make_engine();
        let mut key = key_for('g');
        key.extra_info = INJECT_EXTRA_INFO;
        assert!(!engine.process_key(&key));
    }

    fn pending(
        id: u64,
        original: &str,
        corrected: &str,
        terminator: Option<&str>,
        text_after: &str,
    ) -> PendingError {
        PendingError {
            id,
            original: original.to_string(),
            corrected: corrected.to_string(),
            confidence: 0.9,
            target_layout: None,
            screen: None,
            input_target: StoredTarget::default(),
            terminator: terminator.map(str::to_string),
            text_after: text_after.to_string(),
            timestamp: Instant::now(),
        }
    }

    #[test]
    fn test_build_corrected_text_chain_ok() {
        let errors = vec![
            pending(0, "ghbdtn", "привет", Some(" "), "ntcn "),
            pending(1, "ntcn", "тест", Some(" "), ""),
        ];
        let (text, ok) = build_corrected_text(&errors);
        assert!(ok);
        assert_eq!(text, "привет тест ");
    }

    #[test]
    fn test_build_corrected_text_chain_broken() {
        let errors = vec![
            pending(0, "ghbdtn", "привет", Some(" "), "xyz"),
            pending(1, "ntcn", "тест", Some(" "), ""),
        ];
        let (_, ok) = build_corrected_text(&errors);
        assert!(!ok);
    }

    #[test]
    fn test_detect_target_layout() {
        assert_eq!(detect_target_layout("ghbdtn", "привет"), Some(Layout::Ru));
        assert_eq!(detect_target_layout("руддщ", "hello"), Some(Layout::En));
        assert_eq!(detect_target_layout("abc", "abc"), None);
    }

    #[test]
    fn test_classify_char() {
        assert_eq!(classify_char('a', Some(Layout::En)), KeyType::Alpha);
        assert_eq!(classify_char(',', Some(Layout::En)), KeyType::Ambiguous);
        assert_eq!(classify_char(',', Some(Layout::Ru)), KeyType::Punctuation);
        assert_eq!(classify_char('5', Some(Layout::En)), KeyType::Digit);
        assert_eq!(classify_char(' ', Some(Layout::En)), KeyType::Space);
        assert_eq!(classify_char('#', Some(Layout::En)), KeyType::Other);
    }
}
