//! Трей-иконка и её меню (события меню — через глобальный канал muda).

use std::collections::VecDeque;
use std::sync::{Arc, Mutex, PoisonError};

use anyhow::Result;
use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, TrayIconBuilder};

use klavium_core::Layout;

/// Идентификатор пункта «Настройки».
pub const MENU_SETTINGS: &str = "klavium.settings";
/// Идентификатор пункта «Статистика».
pub const MENU_STATS: &str = "klavium.stats";
/// Идентификатор пункта «Голосовой ввод».
pub const MENU_VOICE: &str = "klavium.voice";
/// Идентификатор пункта «Выход».
pub const MENU_QUIT: &str = "klavium.quit";

/// Команды потоку трея (иконка живёт только в его потоке).
#[derive(Debug, Clone, Copy)]
pub enum TrayCommand {
    /// Обновить иконку и подсказку по текущей раскладке.
    SetLayout(Option<Layout>),
}

/// Иконки трея (сгенерированы `tools/gen_icons.py`): EN — синяя, RU — красная,
/// нейтральная — тёмно-синяя с «K»; цвета: #2563eb / #dc2626.
const ICON_EN: &[u8] = include_bytes!("../resources/tray_en.bin");
const ICON_RU: &[u8] = include_bytes!("../resources/tray_ru.bin");
const ICON_NEUTRAL: &[u8] = include_bytes!("../resources/tray_neutral.bin");
/// Размер иконок трея (RGBA, квадрат).
pub const ICON_SIZE: u32 = 64;

fn icon_for(layout: Option<Layout>) -> Icon {
    let rgba = match layout {
        Some(Layout::Ru) => ICON_RU,
        Some(Layout::En) => ICON_EN,
        None => ICON_NEUTRAL,
    };
    Icon::from_rgba(rgba.to_vec(), ICON_SIZE, ICON_SIZE).expect("иконка трея повреждена")
}

fn tooltip_for(layout: Option<Layout>) -> String {
    let language = match layout {
        Some(Layout::Ru) => "русский",
        Some(Layout::En) => "английский",
        None => "…",
    };
    format!("Klavium — раскладка: {language}")
}

/// Нейтральная иконка (RGBA) — для окна настроек.
#[must_use]
pub fn neutral_icon_rgba() -> Vec<u8> {
    ICON_NEUTRAL.to_vec()
}

/// Дескриптор трей-иконки.
pub struct TrayHandle {
    tray: tray_icon::TrayIcon,
}

impl TrayHandle {
    /// Обновить иконку и подсказку трея по текущей раскладке
    /// (EN — синяя, RU — красная).
    pub fn set_layout_icon(&self, layout: Option<Layout>) {
        let _ = self.tray.set_icon(Some(icon_for(layout)));
        let _ = self.tray.set_tooltip(Some(tooltip_for(layout)));
    }
}

/// Создать и отобразить трей-иконку с меню «Настройки» / «Выход».
///
/// # Errors
///
/// Ошибка, если не удалось создать иконку или меню трея.
pub fn create_tray() -> Result<TrayHandle> {
    let menu = Menu::new();
    let settings_item = MenuItem::with_id(MENU_SETTINGS, "Настройки", true, None);
    let stats_item = MenuItem::with_id(MENU_STATS, "Статистика", true, None);
    let voice_item = MenuItem::with_id(MENU_VOICE, "Голосовой ввод", true, None);
    let quit_item = MenuItem::with_id(MENU_QUIT, "Выход", true, None);
    menu.append(&settings_item)?;
    menu.append(&stats_item)?;
    menu.append(&voice_item)?;
    menu.append(&PredefinedMenuItem::separator())?;
    menu.append(&quit_item)?;

    let initial = klavium_platform::layout::current();
    let tray = TrayIconBuilder::new()
        .with_icon(icon_for(initial))
        .with_menu(Box::new(menu))
        .with_tooltip(tooltip_for(initial))
        .build()?;

    Ok(TrayHandle { tray })
}

/// Неблокирующий опрос событий меню: возвращает id нажатого пункта.
#[must_use]
pub fn poll_menu_event() -> Option<String> {
    MenuEvent::receiver()
        .try_recv()
        .ok()
        .map(|event| event.id.0)
}

/// Очередь событий меню, которые поток-наблюдатель передаёт в окно настроек.
pub type TrayQueue = Arc<Mutex<VecDeque<String>>>;

/// Забрать очередное событие меню для UI (потокобезопасно).
#[must_use]
pub fn pop_queued_event(queue: &TrayQueue) -> Option<String> {
    queue
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .pop_front()
}
