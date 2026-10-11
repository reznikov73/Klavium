//! Раскладка клавиатуры: чтение текущей и переключение.
//!
//! Каскад способов: прямое API-переключение, затем
//! эмуляция системных горячих клавиш (с запоминанием рабочей комбинации),
//! затем `LoadKeyboardLayout`.
//!
//! ВНИМАНИЕ: [`switch_to`] содержит паузы (до ~1 секунды) и НЕ должен
//! вызываться из потока клавиатурного хука — только из рабочих потоков.

use std::ffi::c_void;
use std::sync::Mutex;
use std::thread::sleep;
use std::time::Duration;

use klavium_core::Layout;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    ActivateKeyboardLayout, GetKeyboardLayout, LoadKeyboardLayoutW, VK_CONTROL, VK_LCONTROL,
    VK_LMENU, VK_LSHIFT, VK_LWIN, VK_MENU, VK_RCONTROL, VK_RMENU, VK_RSHIFT, VK_RWIN, VK_SHIFT,
    VK_SPACE,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{SendMessageW, WM_INPUTLANGCHANGEREQUEST};

use crate::{inject, window};

const EN_LANG_ID: u32 = 0x0409;
const RU_LANG_ID: u32 = 0x0419;

fn lang_id_of(hkl: *mut c_void) -> u32 {
    (hkl as usize & 0xFFFF) as u32
}

/// Текущая раскладка активного окна (`None`, если неизвестна).
#[must_use]
pub fn current() -> Option<Layout> {
    let hwnd = window::foreground_window()?;
    let thread_id = window::window_thread_id(hwnd);
    if thread_id == 0 {
        return None;
    }
    let hkl = unsafe { GetKeyboardLayout(thread_id) };
    if hkl.is_null() {
        return None;
    }
    match lang_id_of(hkl) {
        RU_LANG_ID => Some(Layout::Ru),
        EN_LANG_ID => Some(Layout::En),
        _ => None,
    }
}

/// Комбинации переключения раскладки (запоминается первая успешная).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Combo {
    AltShift,
    CtrlShift,
    WinSpace,
}

static KNOWN_COMBO: Mutex<Option<Combo>> = Mutex::new(None);

/// Переключить раскладку активного окна на целевую. `true` — успех.
#[must_use]
pub fn switch_to(target: Layout) -> bool {
    let target_lang = match target {
        Layout::En => EN_LANG_ID,
        Layout::Ru => RU_LANG_ID,
    };

    // Попытка 1: прямое переключение через Windows API
    if let Some(hwnd) = window::foreground_window() {
        let thread_id = window::window_thread_id(hwnd);
        if thread_id != 0 {
            let hkl = unsafe { GetKeyboardLayout(thread_id) };
            if !hkl.is_null() && lang_id_of(hkl) == target_lang {
                return true;
            }

            let lparam = ((target_lang << 16) | 0x1) as isize;
            let result = unsafe { SendMessageW(hwnd, WM_INPUTLANGCHANGEREQUEST, 0, lparam) };
            if result != 0 && wait_for_layout(target, Duration::from_millis(100)) {
                return true;
            }

            let activated = unsafe {
                ActivateKeyboardLayout(
                    (target_lang << 16) as usize as *mut c_void,
                    0x0001, // KLF_SETFORPROCESS
                )
            };
            if !activated.is_null() && wait_for_layout(target, Duration::from_millis(100)) {
                return true;
            }
        }
    }

    // Попытка 2: эмуляция горячих клавиш (с запоминанием успешной комбинации)
    for combo in ordered_combos() {
        release_modifiers();
        sleep(Duration::from_millis(50));

        let sent = match combo {
            Combo::AltShift => inject::send_hotkey(&[VK_LMENU], VK_SHIFT),
            Combo::CtrlShift => inject::send_hotkey(&[VK_LCONTROL], VK_SHIFT),
            Combo::WinSpace => inject::send_hotkey(&[VK_LWIN], VK_SPACE),
        };
        if sent.is_err() {
            continue;
        }

        sleep(Duration::from_millis(300));
        if current() == Some(target) {
            if let Ok(mut known) = KNOWN_COMBO.lock() {
                *known = Some(combo);
            }
            return true;
        }
    }

    // Попытка 3: LoadKeyboardLayoutW + ActivateKeyboardLayout
    let layout_name: Vec<u16> = match target {
        Layout::En => "00000409",
        Layout::Ru => "00000419",
    }
    .encode_utf16()
    .chain(std::iter::once(0))
    .collect();
    let hkl = unsafe { LoadKeyboardLayoutW(layout_name.as_ptr(), 0x0001) };
    if !hkl.is_null() {
        let _ = unsafe { ActivateKeyboardLayout(hkl, 0) };
        if wait_for_layout(target, Duration::from_millis(100)) {
            return true;
        }
    }

    false
}

fn ordered_combos() -> [Combo; 3] {
    let known = KNOWN_COMBO.lock().ok().and_then(|guard| *guard);
    match known {
        Some(Combo::AltShift) => [Combo::AltShift, Combo::CtrlShift, Combo::WinSpace],
        Some(Combo::CtrlShift) => [Combo::CtrlShift, Combo::AltShift, Combo::WinSpace],
        Some(Combo::WinSpace) => [Combo::WinSpace, Combo::AltShift, Combo::CtrlShift],
        None => [Combo::AltShift, Combo::CtrlShift, Combo::WinSpace],
    }
}

fn wait_for_layout(target: Layout, timeout: Duration) -> bool {
    let attempts = (timeout.as_millis() / 20).max(1) as u32;
    for _ in 0..attempts {
        if current() == Some(target) {
            return true;
        }
        sleep(Duration::from_millis(20));
    }
    false
}

fn release_modifiers() {
    for vk in [
        VK_LSHIFT,
        VK_RSHIFT,
        VK_LCONTROL,
        VK_RCONTROL,
        VK_LMENU,
        VK_RMENU,
        VK_LWIN,
        VK_RWIN,
        VK_SHIFT,
        VK_CONTROL,
        VK_MENU,
    ] {
        let _ = inject::send_key_up(vk);
    }
}
