//! Глобальный клавиатурный хук (`WH_KEYBOARD_LL`).
//!
//! Хук ставится в выделенном потоке с собственным циклом сообщений.
//! Callback вызывается в контексте этого потока для каждого события
//! клавиатуры и должен отрабатывать быстро (системный лимит
//! `LowLevelHooksTimeout`); тяжёлые действия — в отдельных потоках.
//! Решение [`HookDecision::Suppress`] подавляет клавишу: она не уходит
//! в активное приложение (и не далее по цепочке хуков).

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::{GetLastError, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::SystemInformation::GetTickCount;
use windows_sys::Win32::System::Threading::GetCurrentThreadId;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, GetLastInputInfo, LASTINPUTINFO,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetMessageW, PostThreadMessageW, SetWindowsHookExW,
    TranslateMessage, UnhookWindowsHookEx, KBDLLHOOKSTRUCT, MSG, WH_KEYBOARD_LL, WM_KEYDOWN,
    WM_KEYUP, WM_QUIT, WM_SYSKEYDOWN, WM_SYSKEYUP,
};

/// Флаг `LLKHF_INJECTED` из `KBDLLHOOKSTRUCT.flags` (событие синтезировано).
const LLKHF_INJECTED: u32 = 0x10;

/// Дополнительная метка инъекций Klavium (`dwExtraInfo` в `SendInput`).
///
/// Хук получает её в [`KeyEvent::extra_info`] — так потребитель отличает
/// собственный синтезированный ввод от физического.
pub const INJECT_EXTRA_INFO: usize = 0x4B4C_564D; // "KLVM"

/// Коды виртуальных клавиш (для сравнения с [`KeyEvent::vk_code`]).
pub mod vk {
    /// `VK_SHIFT`.
    pub const SHIFT: u32 = 0x10;
    /// `VK_CONTROL`.
    pub const CONTROL: u32 = 0x11;
    /// `VK_MENU` (Alt).
    pub const MENU: u32 = 0x12;
    /// `VK_LSHIFT`.
    pub const LSHIFT: u32 = 0xA0;
    /// `VK_RSHIFT`.
    pub const RSHIFT: u32 = 0xA1;
    /// `VK_LCONTROL`.
    pub const LCONTROL: u32 = 0xA2;
    /// `VK_RCONTROL`.
    pub const RCONTROL: u32 = 0xA3;
    /// `VK_LMENU`.
    pub const LMENU: u32 = 0xA4;
    /// `VK_RMENU`.
    pub const RMENU: u32 = 0xA5;
    /// `VK_LWIN`.
    pub const LWIN: u32 = 0x5B;
    /// `VK_RWIN`.
    pub const RWIN: u32 = 0x5C;
    /// `VK_ESCAPE`.
    pub const ESCAPE: u32 = 0x1B;
    /// `VK_Q`.
    pub const Q: u32 = 0x51;
    /// `VK_F8` (хоткей исправления).
    pub const F8: u32 = 0x77;
    /// `VK_F9` (хоткей голосового ввода).
    pub const F9: u32 = 0x78;
    /// `VK_F10`.
    pub const F10: u32 = 0x79;
}

/// Событие клавиатуры из низкоуровневого хука.
#[derive(Debug, Clone, Copy)]
pub struct KeyEvent {
    /// Виртуальный код клавиши.
    pub vk_code: u32,
    /// Аппаратный скан-код.
    pub scan_code: u32,
    /// `true` — нажатие (key down), `false` — отпускание.
    pub is_down: bool,
    /// Событие с участием Alt (`WM_SYSKEY*`).
    pub is_sys: bool,
    /// Событие синтезировано (флаг `LLKHF_INJECTED`).
    pub injected: bool,
    /// `dwExtraInfo` из события (у инъекций Klavium — [`INJECT_EXTRA_INFO`]).
    pub extra_info: usize,
}

/// Решение обработчика по событию.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookDecision {
    /// Пропустить событие дальше (в активное приложение).
    Propagate,
    /// Подавить событие (клавиша не уйдёт в приложение).
    Suppress,
}

type HookCallback = Box<dyn Fn(KeyEvent) -> HookDecision + Send + 'static>;

static CALLBACK: Mutex<Option<HookCallback>> = Mutex::new(None);

/// Тик (`GetTickCount`, мс) последнего события, дошедшего до хука.
///
/// По нему сторож видит, что в системе был ввод, а хук его не получил:
/// Windows молча снимает `WH_KEYBOARD_LL`, если колбэк превысил системный
/// таймаут (`LowLevelHooksTimeout`, обычно 300 мс), — и тогда «немые»
/// клавиши (Pause, PrintScreen) и горячие клавиши перестают работать
/// до перезапуска приложения.
static LAST_EVENT_TICK: AtomicU32 = AtomicU32::new(0);

/// Сколько колбэк может работать, прежде чем напишем предупреждение.
const SLOW_CALLBACK_MS: u128 = 50;

/// Расхождение «система видела ввод» и «хук видел событие», после которого
/// считаем хук потерянным (мс).
const HOOK_LOST_MS: i32 = 5000;

/// Не переустанавливаем хук чаще, чем раз в столько (мс).
const REINSTALL_COOLDOWN_MS: u32 = 300_000;

unsafe extern "system" fn keyboard_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code < 0 {
        return CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam);
    }

    let started = Instant::now();
    LAST_EVENT_TICK.store(unsafe { GetTickCount() }, Ordering::Relaxed);

    let message = wparam as u32;
    let (is_down, is_sys) = match message {
        WM_KEYDOWN => (true, false),
        WM_SYSKEYDOWN => (true, true),
        WM_KEYUP => (false, false),
        WM_SYSKEYUP => (false, true),
        _ => (false, false),
    };

    let kb = &*(lparam as *const KBDLLHOOKSTRUCT);
    let event = KeyEvent {
        vk_code: kb.vkCode,
        scan_code: kb.scanCode,
        is_down,
        is_sys,
        injected: (kb.flags & LLKHF_INJECTED) != 0,
        extra_info: kb.dwExtraInfo,
    };

    // try_lock: не блокируем хук, если замок занят (установка/снятие хука)
    let suppress = match CALLBACK.try_lock() {
        Ok(guard) => match guard.as_ref() {
            Some(callback) => callback(event) == HookDecision::Suppress,
            None => false,
        },
        Err(_) => false,
    };

    if suppress {
        return 1;
    }
    let elapsed = started.elapsed();
    if elapsed.as_millis() >= SLOW_CALLBACK_MS {
        tracing::warn!(
            target: "klavium",
            "хук: колбэк занял {} мс (vk={:#04X}) — риск снятия хука системой",
            elapsed.as_millis(),
            event.vk_code
        );
    }
    CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam)
}

/// Установленный клавиатурный хук; снимается через [`KeyboardHook::uninstall`].
pub struct KeyboardHook {
    thread_id: u32,
    join: Option<std::thread::JoinHandle<()>>,
}

/// Установить глобальный клавиатурный хук.
///
/// Callback вызывается на потоке хука (быстро, без блокировок).
///
/// # Errors
///
/// Ошибка, если хук уже установлен или не удалось создать поток/хук (Win32).
pub fn install(
    callback: impl Fn(KeyEvent) -> HookDecision + Send + 'static,
) -> anyhow::Result<KeyboardHook> {
    {
        let mut slot = CALLBACK
            .lock()
            .map_err(|_| anyhow::anyhow!("замок callback отравлен"))?;
        if slot.is_some() {
            anyhow::bail!("клавиатурный хук уже установлен");
        }
        *slot = Some(Box::new(callback));
    }

    let (tx, rx) = mpsc::channel::<Result<u32, String>>();
    let join = std::thread::Builder::new()
        .name("keyboard-hook".to_string())
        .spawn(move || {
            let module = unsafe { GetModuleHandleW(std::ptr::null()) };
            let hook = unsafe { SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_proc), module, 0) };
            if hook.is_null() {
                if let Ok(mut slot) = CALLBACK.lock() {
                    *slot = None;
                }
                let _ = tx.send(Err(format!("SetWindowsHookExW: ошибка {}", unsafe {
                    GetLastError()
                })));
                return;
            }

            let thread_id = unsafe { GetCurrentThreadId() };
            LAST_EVENT_TICK.store(unsafe { GetTickCount() }, Ordering::Relaxed);
            tracing::info!(target: "klavium", "хук: установлен (поток {thread_id})");
            let _ = tx.send(Ok(thread_id));
            spawn_watchdog(thread_id);

            let mut message: MSG = unsafe { std::mem::zeroed() };
            loop {
                let got = unsafe { GetMessageW(&mut message, std::ptr::null_mut(), 0, 0) };
                if got <= 0 {
                    break;
                }
                unsafe {
                    TranslateMessage(&message);
                    DispatchMessageW(&message);
                }
            }

            unsafe {
                UnhookWindowsHookEx(hook);
            }
            WATCHDOG_STOP.store(true, Ordering::Relaxed);
            if let Ok(mut slot) = CALLBACK.lock() {
                *slot = None;
            }
        })?;

    match rx.recv() {
        Ok(Ok(thread_id)) => Ok(KeyboardHook {
            thread_id,
            join: Some(join),
        }),
        Ok(Err(message)) => {
            let _ = join.join();
            anyhow::bail!(message);
        }
        Err(_) => {
            let _ = join.join();
            anyhow::bail!("поток хука завершился до установки");
        }
    }
}

/// Флаг остановки сторожа.
static WATCHDOG_STOP: AtomicBool = AtomicBool::new(false);

/// Сторож хука: если в системе был ввод, а хук его не получил, —
/// просит поток хука переустановить хук.
///
/// `GetLastInputInfo` обновляется и от мыши, поэтому при работе без
/// клавиатуры возможны ложные срабатывания; они безвредны (переустановка
/// занимает микросекунды) и ограничены паузой [`REINSTALL_COOLDOWN_MS`].
fn spawn_watchdog(thread_id: u32) {
    WATCHDOG_STOP.store(false, Ordering::Relaxed);
    let spawn = std::thread::Builder::new()
        .name("hook-watchdog".to_string())
        .spawn(move || {
            let cooldown = Duration::from_millis(u64::from(REINSTALL_COOLDOWN_MS));
            let mut last_warning = Instant::now()
                .checked_sub(cooldown)
                .unwrap_or_else(Instant::now);
            while !WATCHDOG_STOP.load(Ordering::Relaxed) {
                std::thread::sleep(Duration::from_millis(1000));
                let Some(system_tick) = last_input_tick() else {
                    continue;
                };
                let our_tick = LAST_EVENT_TICK.load(Ordering::Relaxed);
                // Знаковая разница: отрицательная — системный ввод старее
                // нашего последнего события (это нормально).
                let lag = system_tick.wrapping_sub(our_tick) as i32;
                if lag < HOOK_LOST_MS || last_warning.elapsed() < cooldown {
                    continue;
                }
                last_warning = Instant::now();
                // Переустановка «на месте» (Unhook + SetWindowsHookEx в том же
                // потоке) в проверке убивала хук — поэтому только сообщаем
                // в журнал: проблема видна, лечится перезапуском приложения.
                tracing::warn!(
                    target: "klavium",
                    "хук: клавиатура не доходит ({lag} мс) — перезапустите Klavium"
                );
                let _ = thread_id;
            }
        });
    if spawn.is_err() {
        tracing::error!(target: "klavium", "хук: не удалось запустить сторожевой поток");
    }
}

/// Тик последнего ввода в системе (`GetLastInputInfo`), если доступен.
fn last_input_tick() -> Option<u32> {
    let mut info = LASTINPUTINFO {
        cbSize: u32::try_from(std::mem::size_of::<LASTINPUTINFO>()).ok()?,
        dwTime: 0,
    };
    if unsafe { GetLastInputInfo(&mut info) } == 0 {
        None
    } else {
        Some(info.dwTime)
    }
}

impl KeyboardHook {
    /// Снять хук и дождаться завершения потока хука.
    pub fn uninstall(mut self) {
        WATCHDOG_STOP.store(true, Ordering::Relaxed);
        unsafe {
            PostThreadMessageW(self.thread_id, WM_QUIT, 0, 0);
        }
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

/// Нажата ли клавиша прямо сейчас (`GetAsyncKeyState`).
#[must_use]
pub fn is_key_pressed(vk_code: u32) -> bool {
    unsafe { (GetAsyncKeyState(vk_code as i32) as u16) & 0x8000 != 0 }
}
