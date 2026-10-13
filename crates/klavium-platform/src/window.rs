//! Работа с окнами, фокусом и кареткой ввода.

use std::mem::size_of;
use std::sync::OnceLock;
use std::thread::sleep;
use std::time::Duration;

use windows_sys::Win32::Foundation::{HWND, LPARAM, POINT};
use windows_sys::Win32::Graphics::Gdi::ClientToScreen;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Threading::{
    AttachThreadInput, GetCurrentProcessId, GetCurrentThreadId,
};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::SetFocus;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AllowSetForegroundWindow, CreateWindowExW, DefWindowProcW, DispatchMessageW, EnumWindows,
    FindWindowW, GetCaretPos, GetClassNameW, GetCursorPos, GetForegroundWindow, GetGUIThreadInfo,
    GetWindowTextW, GetWindowThreadProcessId, MsgWaitForMultipleObjectsEx, PeekMessageW,
    PostMessageW, RegisterClassW, RegisterWindowMessageW, SetForegroundWindow, ShowWindow,
    TranslateMessage, GUITHREADINFO, MSG, PM_REMOVE, QS_ALLINPUT, SW_HIDE, SW_SHOWNOACTIVATE,
    WNDCLASSW,
};

/// Дескриптор активного (foreground) окна.
#[must_use]
pub fn foreground_window() -> Option<HWND> {
    let window = unsafe { GetForegroundWindow() };
    if window.is_null() {
        None
    } else {
        Some(window)
    }
}

fn read_window_string(
    window: HWND,
    read: unsafe extern "system" fn(HWND, *mut u16, i32) -> i32,
) -> String {
    let mut buffer = [0u16; 512];
    let length = unsafe { read(window, buffer.as_mut_ptr(), buffer.len() as i32) };
    if length <= 0 {
        return String::new();
    }
    String::from_utf16_lossy(&buffer[..length as usize])
}

/// Заголовок окна.
// HWND — непрозрачный дескриптор Win32 (не указатель на память).
#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[must_use]
pub fn window_title(window: HWND) -> String {
    read_window_string(window, GetWindowTextW)
}

/// Имя класса окна.
// HWND — непрозрачный дескриптор Win32 (не указатель на память).
#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[must_use]
pub fn window_class(window: HWND) -> String {
    read_window_string(window, GetClassNameW)
}

/// Идентификатор потока, создавшего окно.
// HWND — непрозрачный дескриптор Win32 (не указатель на память).
#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[must_use]
pub fn window_thread_id(window: HWND) -> u32 {
    unsafe { GetWindowThreadProcessId(window, std::ptr::null_mut()) }
}

/// Сохранённая цель ввода: окно и элемент фокуса.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InputTarget {
    /// Активное окно.
    pub window: HWND,
    /// Элемент в фокусе (может быть null).
    pub focus: HWND,
}

/// Снять текущую цель ввода (foreground-окно + элемент фокуса/каретки).
#[must_use]
pub fn capture_input_target() -> InputTarget {
    let window = unsafe { GetForegroundWindow() };
    let mut focus: HWND = std::ptr::null_mut();
    if !window.is_null() {
        if let Some(info) = gui_thread_info(window_thread_id(window)) {
            focus = if !info.hwndFocus.is_null() {
                info.hwndFocus
            } else {
                info.hwndCaret
            };
        }
    }
    InputTarget { window, focus }
}

fn gui_thread_info(thread_id: u32) -> Option<GUITHREADINFO> {
    if thread_id == 0 {
        return None;
    }
    let mut info: GUITHREADINFO = unsafe { std::mem::zeroed() };
    info.cbSize = size_of::<GUITHREADINFO>() as u32;
    let ok = unsafe { GetGUIThreadInfo(thread_id, &mut info) };
    if ok == 0 {
        None
    } else {
        Some(info)
    }
}

/// Вернуть фокус в сохранённую цель ввода (best effort).
#[must_use]
pub fn restore_input_target(target: &InputTarget) -> bool {
    let window = target.window;
    let focus = target.focus;
    if window.is_null() && focus.is_null() {
        return false;
    }

    let current = unsafe { GetForegroundWindow() };
    if !current.is_null() && (current == window || current == focus) {
        return true;
    }

    let current_thread = unsafe { GetCurrentThreadId() };
    let target_thread = thread_of(if !window.is_null() { window } else { focus });
    let foreground_thread = thread_of(current);

    let mut attached: Vec<(u32, u32)> = Vec::new();
    attach_thread(current_thread, target_thread, &mut attached);
    attach_thread(foreground_thread, target_thread, &mut attached);

    unsafe {
        let _ = AllowSetForegroundWindow(u32::MAX);
        if !window.is_null() {
            let _ = ShowWindow(window, SW_SHOWNOACTIVATE);
            let _ = SetForegroundWindow(window);
        }
        if !focus.is_null() && focus != window {
            let _ = SetFocus(focus);
        }
    }
    sleep(Duration::from_millis(50));

    for (from, to) in attached {
        unsafe {
            let _ = AttachThreadInput(from, to, 0);
        }
    }
    true
}

fn thread_of(window: HWND) -> u32 {
    if window.is_null() {
        0
    } else {
        window_thread_id(window)
    }
}

fn attach_thread(from: u32, to: u32, attached: &mut Vec<(u32, u32)>) {
    if from != 0 && to != 0 && from != to {
        let ok = unsafe { AttachThreadInput(from, to, 1) };
        if ok != 0 {
            attached.push((from, to));
        }
    }
}

/// Позиция каретки ввода в экранных координатах: `(x, y, высота строки)`.
#[must_use]
pub fn caret_screen_pos() -> Option<(i32, i32, i32)> {
    let window = unsafe { GetForegroundWindow() };
    if window.is_null() {
        return None;
    }

    if let Some(info) = gui_thread_info(window_thread_id(window)) {
        let caret_window = if !info.hwndCaret.is_null() {
            info.hwndCaret
        } else {
            info.hwndFocus
        };
        let caret_rect = info.rcCaret;
        if !caret_window.is_null()
            && (!info.hwndCaret.is_null() || caret_rect.bottom != 0 || caret_rect.right != 0)
        {
            let line_height = (caret_rect.bottom - caret_rect.top).max(16);
            let mut point = POINT {
                x: caret_rect.left,
                y: if caret_rect.bottom != 0 {
                    caret_rect.bottom
                } else {
                    caret_rect.top + line_height
                },
            };
            unsafe {
                let _ = ClientToScreen(caret_window, &mut point);
            }
            return Some((point.x, point.y, line_height));
        }

        let mut point = POINT { x: 0, y: 0 };
        let focus = if !info.hwndFocus.is_null() {
            info.hwndFocus
        } else {
            window
        };
        let ok = unsafe { GetCaretPos(&mut point) };
        if ok != 0 && (point.x != 0 || point.y != 0) {
            unsafe {
                let _ = ClientToScreen(focus, &mut point);
            }
            return Some((point.x, point.y, 20));
        }
    }
    None
}

/// Позиция курсора мыши (fallback для позиционирования подсказок).
#[must_use]
pub fn cursor_pos() -> (i32, i32) {
    let mut point = POINT { x: 0, y: 0 };
    unsafe {
        let _ = GetCursorPos(&mut point);
    }
    (point.x, point.y)
}

/// Класс скрытого окна-приёмника команд «показать настройки».
const CONTROL_CLASS: &str = "KlaviumControl";

/// Идентификатор именованного сообщения (одинаков во всех процессах сессии).
fn show_message_id() -> u32 {
    static MESSAGE: OnceLock<u32> = OnceLock::new();
    *MESSAGE.get_or_init(|| {
        let name: Vec<u16> = "Klavium.ShowSettings\0".encode_utf16().collect();
        unsafe { RegisterWindowMessageW(name.as_ptr()) }
    })
}

/// Колбэк, вызываемый окном-приёмником по сигналу «показать настройки».
static SHOW_CALLBACK: OnceLock<Box<dyn Fn() + Send + Sync>> = OnceLock::new();

unsafe extern "system" fn control_wnd_proc(
    window: HWND,
    message: u32,
    wparam: usize,
    lparam: isize,
) -> isize {
    if message == show_message_id() {
        if let Some(callback) = SHOW_CALLBACK.get() {
            callback();
        }
        return 0;
    }
    unsafe { DefWindowProcW(window, message, wparam, lparam) }
}

/// Завести скрытое окно-приёмник в текущем потоке (нужен поток с насосом
/// сообщений — у Klavium это поток трея) и повесить на него колбэк показа.
///
/// Так повторный запуск приложения просит работающий экземпляр показать окно
/// **его собственными средствами**: внешний `ShowWindow` ломает цикл winit на
/// Windows (окно зависает чёрным и крутит CPU на 100%).
pub fn install_show_control(on_show: impl Fn() + Send + Sync + 'static) -> bool {
    let class: Vec<u16> = CONTROL_CLASS
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    let instance = unsafe { GetModuleHandleW(std::ptr::null()) };
    unsafe {
        let mut window_class: WNDCLASSW = std::mem::zeroed();
        window_class.lpfnWndProc = Some(control_wnd_proc);
        window_class.hInstance = instance;
        window_class.lpszClassName = class.as_ptr();
        // Повторная регистрация класса в том же процессе безвредна.
        RegisterClassW(&window_class);

        // HWND_MESSAGE (-3): «окно только для сообщений» — невидимо и вне Z-порядка.
        let message_only: HWND = -3_isize as HWND;
        let window = CreateWindowExW(
            0,
            class.as_ptr(),
            class.as_ptr(),
            0,
            0,
            0,
            0,
            0,
            message_only,
            std::ptr::null_mut(),
            instance,
            std::ptr::null(),
        );
        if window.is_null() {
            return false;
        }
    }
    let _ = SHOW_CALLBACK.set(Box::new(on_show));
    true
}

/// Попросить работающий экземпляр показать окно настроек (повторный запуск).
#[must_use]
pub fn request_show_settings() -> bool {
    let class: Vec<u16> = CONTROL_CLASS
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    let window = unsafe { FindWindowW(class.as_ptr(), std::ptr::null()) };
    if window.is_null() {
        return false;
    }
    let posted = unsafe { PostMessageW(window, show_message_id(), 0, 0) };
    posted != 0
}

/// Обработать сообщения текущего потока (ждать до 100 мс).
///
/// Нужен потоку трей-иконки: у него нет цикла winit, а иконке и контекстному
/// меню требуется насос сообщений на своём потоке.
pub fn pump_thread_messages() {
    unsafe {
        let _ = MsgWaitForMultipleObjectsEx(0, std::ptr::null(), 100, QS_ALLINPUT, 0);
    }
    let mut message: MSG = unsafe { std::mem::zeroed() };
    while unsafe { PeekMessageW(&mut message, std::ptr::null_mut(), 0, 0, PM_REMOVE) } != 0 {
        unsafe {
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
}

unsafe extern "system" fn hide_settings_cb(window: HWND, _param: LPARAM) -> i32 {
    let mut pid = 0_u32;
    unsafe {
        GetWindowThreadProcessId(window, &mut pid);
    }
    if pid != unsafe { GetCurrentProcessId() } {
        return 1;
    }
    let class = window_class(window);
    let title = window_title(window);
    if class == "Window Class" && title.starts_with("Klavium — настройки") {
        unsafe {
            let _ = ShowWindow(window, SW_HIDE);
        }
    }
    1 // продолжаем перечисление
}

/// Скрыть окна настроек, оставшиеся после закрытия окна eframe.
///
/// eframe в режиме `run_and_return` освобождает окно только при следующем
/// запуске цикла — до этого оно остаётся видимым «зомби», поэтому прячем сами.
/// Вызывать из того же потока, что создавал окно (главный поток).
pub fn hide_settings_windows() {
    unsafe {
        let _ = EnumWindows(Some(hide_settings_cb), 0);
    }
}
