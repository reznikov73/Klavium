//! Глобальный клавиатурный хук (`WH_KEYBOARD_LL`).
//!
//! Хук ставится в выделенном потоке с собственным циклом сообщений.
//! Callback вызывается в контексте этого потока для каждого события
//! клавиатуры и должен отрабатывать быстро (системный лимит
//! `LowLevelHooksTimeout`); тяжёлые действия — в отдельных потоках.
//! Решение [`HookDecision::Suppress`] подавляет клавишу: она не уходит
//! в активное приложение (и не далее по цепочке хуков).

use std::sync::mpsc;
use std::sync::Mutex;

use windows_sys::Win32::Foundation::{GetLastError, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Threading::GetCurrentThreadId;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;
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

unsafe extern "system" fn keyboard_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code < 0 {
        return CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam);
    }

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

            let _ = tx.send(Ok(unsafe { GetCurrentThreadId() }));

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

impl KeyboardHook {
    /// Снять хук и дождаться завершения потока хука.
    pub fn uninstall(mut self) {
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
