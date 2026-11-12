//! Замер бюджета потока хука: сколько времени занимает обработка одной клавиши.
//!
//! Сравниваются четыре варианта:
//! - «вычисления» (мок ввода) — сколько занимает сама детекция по словарям;
//! - «реальный Win32» ([`PlatformIo`]) — как было в приложении: раскладка,
//!   цель ввода и каретка запрашиваются у системы на каждое нажатие.
//!
//! В каждом варианте — два режима: `run_jobs_inline` (детекция в колбэке, как
//! до выноса в воркер) и обычный (в колбэке остаётся только отправка воркеру).
//!
//! Запуск: `cargo run --release -p klavium-app --example hook_budget`
//!
//! Ориентир: системный лимит `LowLevelHooksTimeout` ≈ 300 мс; бюджет — p99 < 3 мс,
//! максимум < 50 мс. Превышение лимита приводит к тому, что Windows молча
//! снимает хук (коррекция и хоткеи перестают работать до перезапуска).

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use klavium_app::engine::{
    CorrectionEngine, EngineConfig, EngineIo, KeyInput, Modifiers, StoredTarget,
};
use klavium_app::real_io::PlatformIo;
use klavium_core::Layout;

/// Сколько клавиш прогоняем в каждом варианте.
const KEYS: usize = 10_000;

/// Мок ввода: раскладка EN, инъекции — в счётчик (на экран ничего не идёт).
struct ScriptedIo {
    injected: AtomicUsize,
}

impl ScriptedIo {
    const fn new() -> Self {
        Self {
            injected: AtomicUsize::new(0),
        }
    }
}

impl EngineIo for ScriptedIo {
    fn capture_target(&self) -> StoredTarget {
        StoredTarget {
            window: 1,
            focus: 2,
        }
    }

    fn restore_target(&self, _target: &StoredTarget) -> bool {
        true
    }

    fn caret_pos(&self) -> Option<(i32, i32, i32)> {
        Some((100, 100, 18))
    }

    fn current_layout(&self) -> Option<Layout> {
        Some(Layout::En)
    }

    fn switch_layout(&self, _target: Layout) -> bool {
        true
    }

    fn send_backspaces(&self, count: u32) {
        self.injected.fetch_add(count as usize, Ordering::Relaxed);
    }

    fn send_text(&self, text: &str) {
        self.injected
            .fetch_add(text.chars().count(), Ordering::Relaxed);
    }

    fn modifiers(&self) -> Modifiers {
        Modifiers::none()
    }
}

/// Скан-коды «ghbdtn » (EN-раскладка).
const WORD: [u32; 7] = [0x22, 0x23, 0x30, 0x20, 0x14, 0x31, 0x39];

fn main() {
    println!("Klavium — бюджет потока хука");
    println!("Клавиш в прогоне: {KEYS} («ghbdtn » циклами), лимит системы ≈ 300 мс\n");

    report(
        "вычисления, было (детекция в колбэке)",
        &run_inline_mock(),
        "мок",
    );
    report(
        "вычисления, стало (детекция в воркере)",
        &run_async_mock(),
        "мок",
    );
    report(
        "Win32, было (детекция в колбэке)",
        &run_inline_platform(),
        "Win32",
    );
    report(
        "Win32, стало (детекция в воркере)",
        &run_async_platform(),
        "Win32",
    );
}

fn run_inline_mock() -> Vec<u128> {
    run(Arc::new(ScriptedIo::new()), true)
}

fn run_async_mock() -> Vec<u128> {
    run(Arc::new(ScriptedIo::new()), false)
}

fn run_inline_platform() -> Vec<u128> {
    run(Arc::new(PlatformIo), true)
}

fn run_async_platform() -> Vec<u128> {
    run(Arc::new(PlatformIo), false)
}

/// Прогнать клавиши и вернуть длительности обработки (мкс), отсортированные.
fn run(io: Arc<dyn EngineIo>, inline: bool) -> Vec<u128> {
    let (tx, _rx) = mpsc::channel();
    let config = EngineConfig {
        run_jobs_inline: inline,
        ..EngineConfig::default()
    };
    let engine = CorrectionEngine::spawn(io, config, tx);

    let mut durations = Vec::with_capacity(KEYS);
    for index in 0..KEYS {
        let scan = WORD[index % WORD.len()];
        let key = KeyInput {
            vk_code: 0,
            scan_code: scan,
            is_down: true,
            injected: false,
            extra_info: 0,
        };
        let started = Instant::now();
        let _ = engine.process_key(&key);
        durations.push(started.elapsed().as_micros());

        // В асинхронном режиме даём воркеру разгружать очередь.
        if !inline && index % 64 == 63 {
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    durations.sort_unstable();
    durations
}

fn report(title: &str, durations: &[u128], kind: &str) {
    let n = durations.len();
    if n == 0 {
        println!("{title}: нет данных");
        return;
    }
    // Проценты считаем целочисленно — без потери точности при касте.
    let p = |percent: usize| durations[(n * percent / 100).min(n - 1)];
    let over = durations.iter().filter(|d| **d > 50_000).count();
    println!("{title} [{kind}]:");
    println!(
        "  min {} мкс · p50 {} мкс · p95 {} мкс · p99 {} мкс · max {} мкс · дольше 50 мс: {over}",
        durations[0],
        p(50),
        p(95),
        p(99),
        durations[n - 1]
    );
}
