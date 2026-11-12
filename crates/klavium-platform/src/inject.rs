//! Инъекция ввода через `SendInput` (раскладко-независимый Unicode-путь).
//!
//! Все синтезированные события помечаются `dwExtraInfo = INJECT_EXTRA_INFO`,
//! чтобы собственный хук мог отличать их от физического ввода.

use std::mem::size_of;
use std::thread::sleep;
use std::time::Duration;

use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, KEYEVENTF_UNICODE,
    VIRTUAL_KEY, VK_BACK, VK_CONTROL, VK_LCONTROL, VK_LMENU, VK_LSHIFT, VK_LWIN, VK_RCONTROL,
    VK_RETURN, VK_RMENU, VK_RSHIFT, VK_RWIN, VK_TAB, VK_V,
};

use crate::hook::{is_key_pressed, INJECT_EXTRA_INFO};

/// Собрать событие клавиатуры с меткой Klavium.
fn key_input(vk: VIRTUAL_KEY, scan: u16, flags: u32) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: scan,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: INJECT_EXTRA_INFO,
            },
        },
    }
}

/// Отправить массив INPUT-событий (все события должны быть отправлены).
fn send_inputs(inputs: &[INPUT]) -> anyhow::Result<()> {
    let expected = inputs.len();
    let sent = unsafe { SendInput(expected as u32, inputs.as_ptr(), size_of::<INPUT>() as i32) };
    if sent as usize != expected {
        anyhow::bail!("SendInput: отправлено {sent} из {expected} событий");
    }
    Ok(())
}

/// Нажать клавишу по виртуальному коду.
///
/// # Errors
///
/// Ошибка, если `SendInput` не принял событие.
pub fn send_key_down(vk: VIRTUAL_KEY) -> anyhow::Result<()> {
    send_inputs(&[key_input(vk, 0, 0)])
}

/// Отпустить клавишу по виртуальному коду.
///
/// # Errors
///
/// Ошибка, если `SendInput` не принял событие.
pub fn send_key_up(vk: VIRTUAL_KEY) -> anyhow::Result<()> {
    send_inputs(&[key_input(vk, 0, KEYEVENTF_KEYUP)])
}

/// Нажатие и отпускание клавиши по виртуальному коду.
///
/// # Errors
///
/// Ошибка, если `SendInput` не принял события.
pub fn press_key(vk: VIRTUAL_KEY) -> anyhow::Result<()> {
    send_inputs(&[key_input(vk, 0, 0), key_input(vk, 0, KEYEVENTF_KEYUP)])
}

/// Нажать комбинацию: модификаторы → клавиша ↓↑ → модификаторы вверх.
///
/// Небольшие внутренние паузы повышают надёжность (смена раскладки, системные хоткеи).
///
/// # Errors
///
/// Ошибка, если `SendInput` не принял события.
pub fn send_hotkey(modifiers: &[VIRTUAL_KEY], key: VIRTUAL_KEY) -> anyhow::Result<()> {
    for &modifier in modifiers {
        send_key_down(modifier)?;
        sleep(Duration::from_millis(20));
    }
    press_key(key)?;
    for &modifier in modifiers.iter().rev() {
        sleep(Duration::from_millis(20));
        send_key_up(modifier)?;
    }
    Ok(())
}

/// Ввести один Unicode-символ раскладко-независимо
/// (`KEYEVENTF_UNICODE`, с корректной обработкой суррогатных пар).
///
/// # Errors
///
/// Ошибка, если `SendInput` не принял события.
pub fn send_unicode_char(c: char) -> anyhow::Result<()> {
    let mut buffer = [0u16; 2];
    let units = c.encode_utf16(&mut buffer);
    for unit in units.iter().copied() {
        send_inputs(&[
            key_input(0, unit, KEYEVENTF_UNICODE),
            key_input(0, unit, KEYEVENTF_UNICODE | KEYEVENTF_KEYUP),
        ])?;
    }
    Ok(())
}

/// Отпустить физически зажатые модификаторы перед инъекцией.
///
/// Хоткей с модификаторами (`Ctrl+Shift+Z` и т.п.) срабатывает, пока пальцы
/// ещё держат `Ctrl`/`Shift`. Без этого отпускания `Backspace` уходил бы
/// в активное приложение как `Ctrl+Shift+Backspace` — слово не стиралось,
/// и исправление «дописывало» текст рядом со старым.
pub fn release_modifiers() {
    const MODIFIERS: [VIRTUAL_KEY; 8] = [
        VK_LCONTROL,
        VK_RCONTROL,
        VK_LSHIFT,
        VK_RSHIFT,
        VK_LMENU,
        VK_RMENU,
        VK_LWIN,
        VK_RWIN,
    ];
    let mut released = false;
    for vk in MODIFIERS {
        if is_key_pressed(u32::from(vk)) && send_key_up(vk).is_ok() {
            released = true;
        }
    }
    if released {
        // Дать приложению обработать отпускание до первых Backspace.
        sleep(Duration::from_millis(25));
    }
}

/// Ввести текст: печатные символы — Unicode-событиями, `'\n'` → Enter, `'\t'` → Tab.
///
/// # Errors
///
/// Ошибка, если `SendInput` не принял события.
pub fn send_unicode_text(text: &str, delay: Duration) -> anyhow::Result<()> {
    release_modifiers();
    for ch in text.chars() {
        match ch {
            '\n' => press_key(VK_RETURN)?,
            '\t' => press_key(VK_TAB)?,
            _ => send_unicode_char(ch)?,
        }
        sleep(delay);
    }
    Ok(())
}

/// Отправить `count` нажатий Backspace с задержкой между ними.
///
/// # Errors
///
/// Ошибка, если `SendInput` не принял события.
pub fn send_backspaces(count: u32, delay: Duration) -> anyhow::Result<()> {
    release_modifiers();
    for _ in 0..count {
        press_key(VK_BACK)?;
        sleep(delay);
    }
    Ok(())
}

/// Вставить текст через буфер обмена (`Ctrl+V`).
///
/// Быстрый путь для длинных фраз: вместо посимвольного ввода — один `Ctrl+V`.
/// Буфер восстанавливается только если там всё ещё наш текст: если
/// пользователь успел скопировать своё, чужое содержимое не затирается.
///
/// # Errors
///
/// Ошибка буфера обмена или `SendInput` — вызывающий делает фолбэк
/// на посимвольный ввод.
pub fn paste_text(text: &str) -> anyhow::Result<()> {
    let mut clipboard = arboard::Clipboard::new()?;
    let previous = clipboard.get_text().ok();
    clipboard.set_text(text)?;

    release_modifiers();
    send_key_down(VK_CONTROL)?;
    sleep(Duration::from_millis(20));
    press_key(VK_V)?;
    sleep(Duration::from_millis(20));
    send_key_up(VK_CONTROL)?;
    sleep(Duration::from_millis(120));

    // Не затираем то, что пользователь успел скопировать за эти 120 мс.
    if clipboard.get_text().ok().as_deref() == Some(text) {
        match previous {
            Some(previous) => {
                let _ = clipboard.set_text(previous);
            }
            None => {
                let _ = clipboard.clear();
            }
        }
    }
    Ok(())
}
