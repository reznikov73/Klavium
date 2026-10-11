//! Оверлей-подсказки на нативном Win32 layered-окне.
//!
//! Одно скрытое окно (`WS_EX_LAYERED | NOACTIVATE | TOPMOST | TOOLWINDOW |
//! TRANSPARENT`) с «цветовым ключом» (магента = прозрачность) и GDI-отрисовкой:
//! - [`OverlayHandle::show_card`] — карточка у каретки: «было → станет»,
//!   уверенность и пилюли F8/Esc; автоскрытие 8 секунд;
//! - [`OverlayHandle::show_toast`] — короткий бейдж сверху по центру
//!   (смена раскладки, статусы); автоскрытие 2 секунды;
//! - [`OverlayHandle::show_badge`] — голосовой бейдж («ГОЛОС»/«ПАУЗА»,
//!   постоянный) и [`OverlayHandle::show_badge_timed`] — временный (ошибки).
//!
//! Все команды выполняются в выделенном потоке оверлея с собственным циклом
//! сообщений; клики сквозь окно (TRANSPARENT), фокус не забирается.
//!
//! Упрощения v1: без стрелки-указателя и без per-pixel alpha (плоские цвета
//! на цветовом ключе); DPI-масштабирование не учитывается.

use std::mem::size_of;
use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Mutex, PoisonError};
use std::thread;

use windows_sys::Win32::Foundation::{
    GetLastError, HMODULE, HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM,
};
use windows_sys::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, CreateFontW, CreatePen, CreateSolidBrush, DeleteDC,
    DeleteObject, DrawTextW, FillRect, GetDC, GetMonitorInfoW, MonitorFromPoint, ReleaseDC,
    RoundRect, SelectObject, SetBkMode, SetTextColor, BITMAPINFO, BITMAPINFOHEADER, BI_RGB,
    BLENDFUNCTION, CLEARTYPE_QUALITY, DEFAULT_CHARSET, DIB_RGB_COLORS, DT_CENTER, DT_END_ELLIPSIS,
    DT_NOPREFIX, DT_RIGHT, DT_SINGLELINE, DT_VCENTER, HDC, MONITORINFO, MONITOR_DEFAULTTONEAREST,
    PS_NULL,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Threading::GetCurrentThreadId;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW,
    GetSystemMetrics, KillTimer, PostQuitMessage, PostThreadMessageW, RegisterClassW, SetTimer,
    ShowWindow, TranslateMessage, UpdateLayeredWindow, MSG, SM_CXSCREEN, SM_CYSCREEN, SW_HIDE,
    SW_SHOWNOACTIVATE, ULW_COLORKEY, WM_APP, WM_DESTROY, WM_QUIT, WM_TIMER, WNDCLASSW,
    WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
};

/// Размер карточки-подсказки.
const CARD_SIZE: (i32, i32) = (460, 92);
/// Размер тоста-бейджа.
const TOAST_SIZE: (i32, i32) = (260, 44);
/// Размер голосового бейджа («ГОЛОС»/«ПАУЗА»).
const BADGE_SIZE: (i32, i32) = (150, 44);
/// Прозрачность бейджа (235/255).
const BADGE_ALPHA: u8 = 235;
/// Цвет «прозрачности» (хромакей).
const MAGENTA: u32 = 0x00FF_00FF;
const TIMER_AUTOHIDE: usize = 1;
const CARD_TIMEOUT_MS: u32 = 8000;
const TOAST_TIMEOUT_MS: u32 = 2000;

/// Видно ли сейчас окно карточки/тоста (для перехвата Esc в хуке).
static VISIBLE: AtomicBool = AtomicBool::new(false);
/// Виден ли голосовой бейдж.
static BADGE_VISIBLE: AtomicBool = AtomicBool::new(false);
/// Дескриптор окна карточки/тоста (для маршрутизации WM_TIMER).
static CARD_HWND: AtomicIsize = AtomicIsize::new(0);
/// Дескриптор окна голосового бейджа.
static BADGE_HWND: AtomicIsize = AtomicIsize::new(0);

/// Запрос на показ карточки-подсказки.
#[derive(Debug, Clone)]
pub struct CardRequest {
    /// Исходный (ошибочный) текст.
    pub old_text: String,
    /// Предлагаемое исправление.
    pub new_text: String,
    /// Метка справа (например, «95%» или «2 слов · 95%»).
    pub meta: String,
    /// Позиция каретки `(x, y, высота строки)`; `None` — центр экрана.
    pub anchor: Option<(i32, i32, i32)>,
}

enum Command {
    ShowCard(CardRequest),
    ShowToast {
        text: String,
        color: u32,
    },
    ShowBadge {
        text: String,
        color: u32,
        timeout_ms: Option<u32>,
    },
    HideBadge,
    Hide,
}

/// Дескриптор оверлея; команды потокобезопасны.
pub struct OverlayHandle {
    tx: Sender<Command>,
    thread_id: u32,
    join: Mutex<Option<thread::JoinHandle<()>>>,
}

impl OverlayHandle {
    /// Показать карточку-подсказку у каретки (или в центре экрана).
    pub fn show_card(&self, card: CardRequest) {
        self.send(Command::ShowCard(card));
    }

    /// Показать короткий тост-бейдж сверху по центру (фон — `color`).
    pub fn show_toast(&self, text: &str, color: (u8, u8, u8)) {
        self.send(Command::ShowToast {
            text: text.to_string(),
            color: rgb(color.0, color.1, color.2),
        });
    }

    /// Показать голосовой бейдж сверху по центру (постоянный до [`Self::hide_badge`]).
    pub fn show_badge(&self, text: &str, color: (u8, u8, u8)) {
        self.send(Command::ShowBadge {
            text: text.to_string(),
            color: rgb(color.0, color.1, color.2),
            timeout_ms: None,
        });
    }

    /// Показать временный голосовой бейдж (автоскрытие через `timeout_ms`).
    pub fn show_badge_timed(&self, text: &str, color: (u8, u8, u8), timeout_ms: u32) {
        self.send(Command::ShowBadge {
            text: text.to_string(),
            color: rgb(color.0, color.1, color.2),
            timeout_ms: Some(timeout_ms),
        });
    }

    /// Скрыть голосовой бейдж.
    pub fn hide_badge(&self) {
        self.send(Command::HideBadge);
    }

    /// Скрыть оверлей.
    pub fn hide(&self) {
        self.send(Command::Hide);
    }

    /// Виден ли сейчас оверлей.
    #[must_use]
    pub fn is_visible(&self) -> bool {
        VISIBLE.load(Ordering::SeqCst)
    }

    /// Остановить поток оверлея (окно уничтожается).
    pub fn shutdown(&self) {
        self.send(Command::Hide);
        unsafe {
            PostThreadMessageW(self.thread_id, WM_QUIT, 0, 0);
        }
        if let Some(join) = self
            .join
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
        {
            let _ = join.join();
        }
    }

    fn send(&self, command: Command) {
        let _ = self.tx.send(command);
        unsafe {
            PostThreadMessageW(self.thread_id, WM_APP, 0, 0);
        }
    }
}

/// Запустить оверлей (создаёт поток с окном и циклом сообщений).
///
/// # Errors
///
/// Ошибка, если не удалось создать поток или окно оверлея (Win32).
pub fn start() -> anyhow::Result<OverlayHandle> {
    let (tx, rx) = mpsc::channel::<Command>();
    let (ready_tx, ready_rx) = mpsc::channel::<Result<u32, String>>();

    let join = thread::Builder::new()
        .name("overlay".to_string())
        .spawn(move || overlay_thread(rx, &ready_tx))?;

    match ready_rx.recv() {
        Ok(Ok(thread_id)) => Ok(OverlayHandle {
            tx,
            thread_id,
            join: Mutex::new(Some(join)),
        }),
        Ok(Err(message)) => {
            let _ = join.join();
            anyhow::bail!(message);
        }
        Err(_) => {
            let _ = join.join();
            anyhow::bail!("поток оверлея завершился до инициализации");
        }
    }
}

fn overlay_thread(rx: mpsc::Receiver<Command>, ready: &Sender<Result<u32, String>>) {
    let class_name: Vec<u16> = "KlaviumOverlay\0".encode_utf16().collect();
    let hinstance = unsafe { GetModuleHandleW(std::ptr::null()) };

    let window_class = WNDCLASSW {
        style: 0,
        lpfnWndProc: Some(wnd_proc),
        cbClsExtra: 0,
        cbWndExtra: 0,
        hInstance: hinstance,
        hIcon: std::ptr::null_mut(),
        hCursor: std::ptr::null_mut(),
        hbrBackground: std::ptr::null_mut(),
        lpszMenuName: std::ptr::null(),
        lpszClassName: class_name.as_ptr(),
    };
    // Повторная регистрация (второй оверлей в процессе) — ошибка, игнорируем.
    unsafe {
        RegisterClassW(&window_class);
    }

    let hwnd = create_layered_window(&class_name, hinstance);
    if hwnd.is_null() {
        let _ = ready.send(Err(format!("CreateWindowExW: ошибка {}", unsafe {
            GetLastError()
        })));
        return;
    }
    let badge_hwnd = create_layered_window(&class_name, hinstance);
    if badge_hwnd.is_null() {
        unsafe {
            DestroyWindow(hwnd);
        }
        let _ = ready.send(Err(format!(
            "CreateWindowExW (бейдж): ошибка {}",
            unsafe { GetLastError() }
        )));
        return;
    }
    CARD_HWND.store(hwnd as isize, Ordering::SeqCst);
    BADGE_HWND.store(badge_hwnd as isize, Ordering::SeqCst);
    let _ = ready.send(Ok(unsafe { GetCurrentThreadId() }));

    let mut message: MSG = unsafe { std::mem::zeroed() };
    loop {
        let got = unsafe { GetMessageW(&mut message, std::ptr::null_mut(), 0, 0) };
        if got <= 0 {
            break;
        }
        if message.message == WM_APP {
            while let Ok(command) = rx.try_recv() {
                handle_command(hwnd, badge_hwnd, command);
            }
        } else {
            unsafe {
                TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
    }

    unsafe {
        DestroyWindow(badge_hwnd);
        DestroyWindow(hwnd);
    }
}

/// Создать скрытое layered-окно оверлея (общий класс).
fn create_layered_window(class_name: &[u16], hinstance: HMODULE) -> HWND {
    unsafe {
        CreateWindowExW(
            WS_EX_LAYERED | WS_EX_NOACTIVATE | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_TRANSPARENT,
            class_name.as_ptr(),
            class_name.as_ptr(),
            WS_POPUP,
            0,
            0,
            0,
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            hinstance,
            std::ptr::null(),
        )
    }
}

unsafe extern "system" fn wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_TIMER => {
            let _ = ShowWindow(hwnd, SW_HIDE);
            if hwnd as isize == CARD_HWND.load(Ordering::SeqCst) {
                VISIBLE.store(false, Ordering::SeqCst);
            } else {
                BADGE_VISIBLE.store(false, Ordering::SeqCst);
            }
            0
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            0
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

fn handle_command(hwnd: HWND, badge_hwnd: HWND, command: Command) {
    match command {
        Command::ShowCard(card) => {
            let (width, height) = CARD_SIZE;
            let position = card_position(width, height, card.anchor);
            draw_card(hwnd, position, width, height, &card);
            unsafe {
                SetTimer(hwnd, TIMER_AUTOHIDE, CARD_TIMEOUT_MS, None);
            }
            VISIBLE.store(true, Ordering::SeqCst);
        }
        Command::ShowToast { text, color } => {
            let (width, height) = TOAST_SIZE;
            let screen_width = unsafe { GetSystemMetrics(SM_CXSCREEN) };
            let position = ((screen_width - width) / 2, 96);
            draw_toast(hwnd, position, width, height, &text, color);
            unsafe {
                SetTimer(hwnd, TIMER_AUTOHIDE, TOAST_TIMEOUT_MS, None);
            }
            VISIBLE.store(true, Ordering::SeqCst);
        }
        Command::ShowBadge {
            text,
            color,
            timeout_ms,
        } => {
            let (width, height) = BADGE_SIZE;
            let screen_width = unsafe { GetSystemMetrics(SM_CXSCREEN) };
            let x = ((screen_width - width) / 2).max(12);
            draw_badge(badge_hwnd, (x, 16), width, height, &text, color);
            unsafe {
                KillTimer(badge_hwnd, TIMER_AUTOHIDE);
                if let Some(ms) = timeout_ms {
                    SetTimer(badge_hwnd, TIMER_AUTOHIDE, ms, None);
                }
            }
            BADGE_VISIBLE.store(true, Ordering::SeqCst);
        }
        Command::HideBadge => {
            unsafe {
                KillTimer(badge_hwnd, TIMER_AUTOHIDE);
                let _ = ShowWindow(badge_hwnd, SW_HIDE);
            }
            BADGE_VISIBLE.store(false, Ordering::SeqCst);
        }
        Command::Hide => {
            unsafe {
                KillTimer(hwnd, TIMER_AUTOHIDE);
                let _ = ShowWindow(hwnd, SW_HIDE);
            }
            VISIBLE.store(false, Ordering::SeqCst);
        }
    }
}

/// Позиция карточки относительно каретки (с учётом рабочей области монитора).
fn card_position(width: i32, height: i32, anchor: Option<(i32, i32, i32)>) -> (i32, i32) {
    let (anchor_x, anchor_y, line_height) = anchor.unwrap_or_else(|| {
        let screen_width = unsafe { GetSystemMetrics(SM_CXSCREEN) };
        let screen_height = unsafe { GetSystemMetrics(SM_CYSCREEN) };
        (screen_width / 2, screen_height / 2, 20)
    });

    let monitor = unsafe {
        MonitorFromPoint(
            POINT {
                x: anchor_x,
                y: anchor_y,
            },
            MONITOR_DEFAULTTONEAREST,
        )
    };
    let mut info: MONITORINFO = unsafe { std::mem::zeroed() };
    info.cbSize = size_of::<MONITORINFO>() as u32;
    let mut work = RECT {
        left: 0,
        top: 0,
        right: 1920,
        bottom: 1080,
    };
    if !monitor.is_null() && unsafe { GetMonitorInfoW(monitor, &mut info) } != 0 {
        work = info.rcWork;
    }

    let mut x = anchor_x - 24;
    let below = anchor_y + 8;
    let y = if below + height <= work.bottom {
        below
    } else {
        (anchor_y - line_height - height - 8).max(work.top)
    };
    x = x.clamp(work.left + 8, work.right - width - 8);
    (x, y.max(work.top + 8))
}

/// Промежуточный холст: DIB + память-DC; `present` показывает окно, `Drop` чистит.
struct Canvas {
    screen_dc: HDC,
    mem_dc: HDC,
    dib: *mut core::ffi::c_void,
    old_bitmap: *mut core::ffi::c_void,
    width: i32,
    height: i32,
}

impl Canvas {
    fn new(width: i32, height: i32) -> Option<Self> {
        unsafe {
            let screen_dc = GetDC(std::ptr::null_mut());
            if screen_dc.is_null() {
                return None;
            }
            let mem_dc = CreateCompatibleDC(screen_dc);
            if mem_dc.is_null() {
                let _ = ReleaseDC(std::ptr::null_mut(), screen_dc);
                return None;
            }

            let mut info: BITMAPINFO = std::mem::zeroed();
            info.bmiHeader = BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width,
                biHeight: -height, // top-down
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB,
                biSizeImage: 0,
                biXPelsPerMeter: 0,
                biYPelsPerMeter: 0,
                biClrUsed: 0,
                biClrImportant: 0,
            };
            let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
            let dib = CreateDIBSection(
                mem_dc,
                &info,
                DIB_RGB_COLORS,
                &mut bits,
                std::ptr::null_mut(),
                0,
            );
            if dib.is_null() {
                let _ = DeleteDC(mem_dc);
                let _ = ReleaseDC(std::ptr::null_mut(), screen_dc);
                return None;
            }
            let old_bitmap = SelectObject(mem_dc, dib);

            // Фон — магента (будет «прозрачным»)
            let magenta_brush = CreateSolidBrush(MAGENTA);
            let full = RECT {
                left: 0,
                top: 0,
                right: width,
                bottom: height,
            };
            FillRect(mem_dc, &full, magenta_brush);
            let _ = DeleteObject(magenta_brush);

            let _ = SetBkMode(mem_dc, 1); // TRANSPARENT

            Some(Self {
                screen_dc,
                mem_dc,
                dib,
                old_bitmap,
                width,
                height,
            })
        }
    }

    /// Показать содержимое холста в окне (позиция — в экранных координатах).
    fn present(self, hwnd: HWND, position: (i32, i32), alpha: u8) {
        unsafe {
            let destination = POINT {
                x: position.0,
                y: position.1,
            };
            let source = POINT { x: 0, y: 0 };
            let size = SIZE {
                cx: self.width,
                cy: self.height,
            };
            let blend = BLENDFUNCTION {
                BlendOp: 0, // AC_SRC_OVER
                BlendFlags: 0,
                SourceConstantAlpha: alpha,
                AlphaFormat: 0,
            };
            UpdateLayeredWindow(
                hwnd,
                self.screen_dc,
                &destination,
                &size,
                self.mem_dc,
                &source,
                MAGENTA,
                &blend,
                ULW_COLORKEY,
            );
            let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        }
    }
}

impl Drop for Canvas {
    fn drop(&mut self) {
        unsafe {
            SelectObject(self.mem_dc, self.old_bitmap);
            let _ = DeleteObject(self.dib);
            let _ = DeleteDC(self.mem_dc);
            let _ = ReleaseDC(std::ptr::null_mut(), self.screen_dc);
        }
    }
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

fn rgb(r: u8, g: u8, b: u8) -> u32 {
    u32::from(r) | (u32::from(g) << 8) | (u32::from(b) << 16)
}

/// Толщина шрифта (значение FW_NORMAL Win32).
const WEIGHT_NORMAL: i32 = 400;
/// Полужирное начертание (значение FW_SEMIBOLD Win32).
const WEIGHT_SEMIBOLD: i32 = 600;

fn create_font(height: i32, weight: i32) -> *mut core::ffi::c_void {
    let face = wide("Segoe UI");
    unsafe {
        CreateFontW(
            height,
            0,
            0,
            0,
            weight,
            0,
            0,
            0,
            u32::from(DEFAULT_CHARSET),
            0,
            0,
            u32::from(CLEARTYPE_QUALITY),
            0,
            face.as_ptr(),
        )
    }
}

fn draw_text(dc: HDC, text: &str, rect: &mut RECT, flags: u32) {
    let text = wide(text);
    unsafe {
        DrawTextW(dc, text.as_ptr(), -1, rect, flags);
    }
}

fn draw_card(hwnd: HWND, position: (i32, i32), width: i32, height: i32, card: &CardRequest) {
    let Some(canvas) = Canvas::new(width, height) else {
        return;
    };
    unsafe {
        let dc = canvas.mem_dc;
        let null_pen = CreatePen(PS_NULL, 0, 0);
        let old_pen = SelectObject(dc, null_pen);

        // Карточка
        let card_brush = CreateSolidBrush(rgb(24, 26, 30));
        let old_brush = SelectObject(dc, card_brush);
        RoundRect(dc, 0, 0, width, height, 14, 14);

        // Акцентная полоса слева
        let accent_brush = CreateSolidBrush(rgb(64, 150, 255));
        SelectObject(dc, accent_brush);
        RoundRect(dc, 10, 14, 16, height - 14, 6, 6);

        let font_small = create_font(-13, WEIGHT_NORMAL);
        let font_normal = create_font(-16, WEIGHT_SEMIBOLD);
        let old_font = SelectObject(dc, font_small);

        // «было: ...»
        SetTextColor(dc, rgb(150, 153, 158));
        let mut old_rect = RECT {
            left: 28,
            top: 10,
            right: width - 130,
            bottom: 34,
        };
        draw_text(
            dc,
            &format!("было:  {}", card.old_text),
            &mut old_rect,
            DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS | DT_NOPREFIX,
        );

        // meta справа
        let mut meta_rect = RECT {
            left: width - 140,
            top: 10,
            right: width - 16,
            bottom: 34,
        };
        draw_text(
            dc,
            &card.meta,
            &mut meta_rect,
            DT_SINGLELINE | DT_VCENTER | DT_RIGHT | DT_NOPREFIX,
        );

        // «станет: ...»
        SelectObject(dc, font_normal);
        SetTextColor(dc, rgb(240, 242, 245));
        let mut new_rect = RECT {
            left: 28,
            top: 32,
            right: width - 16,
            bottom: 58,
        };
        draw_text(
            dc,
            &format!("станет:  {}", card.new_text),
            &mut new_rect,
            DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS | DT_NOPREFIX,
        );

        // Пилюли
        let pill_brush = CreateSolidBrush(rgb(45, 47, 54));
        SelectObject(dc, pill_brush);
        RoundRect(dc, 28, 62, 148, 86, 12, 12);
        RoundRect(dc, 156, 62, 262, 86, 12, 12);

        SelectObject(dc, font_small);
        SetTextColor(dc, rgb(220, 222, 228));
        let mut pill1 = RECT {
            left: 28,
            top: 62,
            right: 148,
            bottom: 86,
        };
        draw_text(
            dc,
            "F8 — исправить",
            &mut pill1,
            DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX,
        );
        let mut pill2 = RECT {
            left: 156,
            top: 62,
            right: 262,
            bottom: 86,
        };
        draw_text(
            dc,
            "Esc — скрыть",
            &mut pill2,
            DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX,
        );

        SelectObject(dc, old_font);
        SelectObject(dc, old_brush);
        SelectObject(dc, old_pen);
        let _ = DeleteObject(font_small);
        let _ = DeleteObject(font_normal);
        let _ = DeleteObject(card_brush);
        let _ = DeleteObject(accent_brush);
        let _ = DeleteObject(pill_brush);
        let _ = DeleteObject(null_pen);
    }
    canvas.present(hwnd, position, 255);
}

/// Тост-бейдж раскладки: цветная «таблетка» с белым текстом сверху по центру.
fn draw_toast(hwnd: HWND, position: (i32, i32), width: i32, height: i32, text: &str, color: u32) {
    let Some(canvas) = Canvas::new(width, height) else {
        return;
    };
    unsafe {
        let dc = canvas.mem_dc;
        let null_pen = CreatePen(PS_NULL, 0, 0);
        let old_pen = SelectObject(dc, null_pen);

        let card_brush = CreateSolidBrush(color);
        let old_brush = SelectObject(dc, card_brush);
        RoundRect(dc, 0, 0, width, height, 12, 12);

        let font = create_font(-16, WEIGHT_SEMIBOLD);
        let old_font = SelectObject(dc, font);
        SetTextColor(dc, rgb(240, 242, 245));
        let mut rect = RECT {
            left: 8,
            top: 0,
            right: width - 8,
            bottom: height,
        };
        draw_text(
            dc,
            text,
            &mut rect,
            DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX,
        );

        SelectObject(dc, old_font);
        SelectObject(dc, old_brush);
        SelectObject(dc, old_pen);
        let _ = DeleteObject(font);
        let _ = DeleteObject(card_brush);
        let _ = DeleteObject(null_pen);
    }
    canvas.present(hwnd, position, 255);
}

/// Голосовой бейдж: цветная «таблетка» с белым жирным текстом сверху по центру.
fn draw_badge(hwnd: HWND, position: (i32, i32), width: i32, height: i32, text: &str, color: u32) {
    let Some(canvas) = Canvas::new(width, height) else {
        return;
    };
    unsafe {
        let dc = canvas.mem_dc;
        let null_pen = CreatePen(PS_NULL, 0, 0);
        let old_pen = SelectObject(dc, null_pen);

        let brush = CreateSolidBrush(color);
        let old_brush = SelectObject(dc, brush);
        RoundRect(dc, 0, 0, width, height, 14, 14);

        let font = create_font(-16, WEIGHT_SEMIBOLD);
        let old_font = SelectObject(dc, font);
        SetTextColor(dc, rgb(255, 255, 255));
        let mut rect = RECT {
            left: 8,
            top: 0,
            right: width - 8,
            bottom: height,
        };
        draw_text(
            dc,
            text,
            &mut rect,
            DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX,
        );

        SelectObject(dc, old_font);
        SelectObject(dc, old_brush);
        SelectObject(dc, old_pen);
        let _ = DeleteObject(font);
        let _ = DeleteObject(brush);
        let _ = DeleteObject(null_pen);
    }
    canvas.present(hwnd, position, BADGE_ALPHA);
}
