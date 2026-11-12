//! Дизайн-система окна настроек: единый источник визуальных токенов.
//!
//! Тема приложения всегда светлая (независимо от темы Windows). Значения
//! согласованы с макетом: белые карточки на светло-сером фоне, тонкие
//! обводки, синий акцент. Оверлеи (`klavium-platform`) дублируют часть
//! значений — менять парно.

use egui::Color32;

/// Фон окна и контентной области.
pub const BG_WINDOW: Color32 = Color32::from_rgb(0xF5, 0xF5, 0xF7);
/// Фон бокового меню.
pub const BG_SIDEBAR: Color32 = Color32::from_rgb(0xEC, 0xEC, 0xF1);
/// Фон карточек, полей, кнопок.
pub const BG_CARD: Color32 = Color32::WHITE;
/// Фон карточки-строки под курсором.
pub const BG_CARD_HOVER: Color32 = Color32::from_rgb(0xF7, 0xF9, 0xFE);
/// Обводки карточек, разделители строк, рамка сайдбара.
pub const BORDER_SUBTLE: Color32 = Color32::from_rgb(0xE3, 0xE3, 0xE8);
/// Обводка полей ввода и кейкапов.
pub const BORDER_STRONG: Color32 = Color32::from_rgb(0xC8, 0xC8, 0xD0);
/// Основной текст.
pub const TEXT_PRIMARY: Color32 = Color32::from_rgb(0x1D, 0x1D, 0x1F);
/// Подписи, пояснения, caption.
pub const TEXT_SECONDARY: Color32 = Color32::from_rgb(0x6E, 0x6E, 0x73);
/// Неактивные элементы.
pub const TEXT_DISABLED: Color32 = Color32::from_rgb(0xAE, 0xAE, 0xB2);
/// Акцент: выбранный пункт меню, свитч, слайдер, ссылки.
pub const ACCENT: Color32 = Color32::from_rgb(0x25, 0x63, 0xEB);
/// Акцент под курсором.
pub const ACCENT_HOVER: Color32 = Color32::from_rgb(0x1D, 0x4E, 0xD8);
/// Фон выбранного пункта меню, информационные «пилюли».
pub const ACCENT_TINT: Color32 = Color32::from_rgb(0xE9, 0xEF, 0xFD);
/// Статус «сохранено», бейдж «готов».
pub const SUCCESS: Color32 = Color32::from_rgb(0x16, 0xA3, 0x4A);
/// Предупреждения (некритичные конфликты).
pub const WARNING: Color32 = Color32::from_rgb(0xD9, 0x77, 0x06);
/// Ошибки, опасные действия.
pub const DANGER: Color32 = Color32::from_rgb(0xDC, 0x26, 0x26);
/// Фон сообщений об ошибке.
pub const DANGER_TINT: Color32 = Color32::from_rgb(0xFD, 0xEC, 0xEC);
/// Фон полосы слайдера/прогресса.
pub const TRACK: Color32 = Color32::from_rgb(0xE9, 0xE9, 0xEE);
/// Трек выключенного свитча.
pub const SWITCH_OFF: Color32 = Color32::from_rgb(0xD9, 0xD9, 0xDE);

/// Толщина тонкой обводки (1 px).
pub const HAIRLINE: f32 = 1.0;

/// Радиус карточек.
pub const RADIUS_CARD: u8 = 10;
/// Радиус кнопок, полей, сегмент-контролов.
pub const RADIUS_CONTROL: u8 = 6;
/// Радиус кейкапов хоткеев.
pub const RADIUS_KEYCAP: u8 = 4;

/// Шаг сетки отступов.
pub const SPACE_1: f32 = 4.0;
/// Шаг сетки отступов.
pub const SPACE_2: f32 = 8.0;
/// Шаг сетки отступов.
pub const SPACE_3: f32 = 12.0;
/// Шаг сетки отступов.
pub const SPACE_4: f32 = 16.0;
/// Шаг сетки отступов.
pub const SPACE_6: f32 = 24.0;

/// Внутренний паддинг карточки.
pub const CARD_PADDING: i8 = 16;
/// Боковые поля контентной области.
pub const CONTENT_PADDING_X: i8 = 20;
/// Верхнее и нижнее поле контентной области.
pub const CONTENT_PADDING_Y: i8 = 16;
/// Ширина бокового меню.
pub const SIDEBAR_WIDTH: f32 = 220.0;
/// Высота пункта меню.
pub const MENU_ROW_HEIGHT: f32 = 34.0;
/// Высота кнопки нижней панели.
pub const BUTTON_HEIGHT: f32 = 32.0;

/// Заголовок страницы.
pub const TEXT_TITLE: f32 = 17.0;
/// Заголовок карточки.
pub const TEXT_CARD_TITLE: f32 = 14.0;
/// Основной текст.
pub const TEXT_BODY: f32 = 13.0;
/// Подписи и пояснения.
pub const TEXT_CAPTION: f32 = 11.5;
/// Моноширинный текст (кейкапы, пути, счётчики).
pub const TEXT_MONO: f32 = 12.0;
/// Крупное значение метрики.
pub const TEXT_METRIC: f32 = 22.0;
