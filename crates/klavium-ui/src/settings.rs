//! Окно настроек на egui/eframe.
//!
//! Открывается из трея; закрытие окна лишь скрывает его (приложение остаётся
//! в трее). Изменения применяются кнопкой «Сохранить» через колбэк приложения
//! (запись конфига, обновление движка, автозапуск).

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use egui::{Color32, RichText, Stroke};
use klavium_core::config::{AppConfig, SoundSpec};
use klavium_core::hotkeys::Hotkey;
use klavium_core::stats::StatsData;
use klavium_core::voice::{DownloadStatus, RecognitionTest, MODELS};

use crate::{theme, widgets};

/// Колбэк сохранения: приложение пишет файл и применяет изменения на лету.
pub type SaveHandler = Box<dyn Fn(AppConfig) -> Result<(), String> + 'static>;

/// Нажатие, пойманное глобальным хуком во время назначения хоткея.
#[derive(Debug, Clone, Copy)]
pub struct CapturedKey {
    /// Виртуальный код клавиши.
    pub vk_code: u32,
    /// Скан-код клавиши.
    pub scan_code: u32,
    /// Зажатые модификаторы.
    pub mods: klavium_core::hotkeys::Mods,
}

/// Очередь нажатий для захвата: наполняет приложение из хука, читает окно.
pub type CaptureQueue = Arc<Mutex<VecDeque<CapturedKey>>>;

/// Контекст запуска окна настроек.
pub struct SettingsContext {
    /// Стартовая копия конфигурации для редактирования.
    pub config: AppConfig,
    /// Колбэк сохранения.
    pub on_save: SaveHandler,
    /// Проиграть звук с заданными настройками (кнопка «Прослушать»).
    pub sound_preview: Box<dyn Fn(SoundSpec) + 'static>,
    /// Снимок статистики для вкладки «Статистика».
    pub stats_snapshot: Box<dyn Fn() -> StatsData + 'static>,
    /// Сброс статистики.
    pub stats_reset: Box<dyn Fn() + 'static>,
    /// Переключить голосовой ввод (вкл/выкл).
    pub voice_toggle: Box<dyn Fn() + 'static>,
    /// Статус голосового сервиса: (активен, текст состояния).
    pub voice_snapshot: Box<dyn Fn() -> (bool, String) + 'static>,
    /// Каталог данных приложения (для кнопки «Открыть папку»).
    pub data_dir: PathBuf,
    /// Пояснение к активному каталогу данных (fallback и т.п.).
    pub data_note: Option<String>,
    /// Очередь событий меню трея (заполняет поток-наблюдатель в приложении).
    pub tray_queue: crate::tray::TrayQueue,
    /// Смена каталога данных: `Some(path)` — выбрать, `None` — по умолчанию.
    pub data_dir_change: Box<dyn Fn(Option<PathBuf>) + 'static>,
    /// Статус переноса каталога данных (`None` — операций нет).
    pub data_move_status: Box<dyn Fn() -> Option<String> + 'static>,
    /// Запустить загрузку модели whisper.
    pub model_download_start: Box<dyn Fn(String) + 'static>,
    /// Снимок состояния загрузки модели.
    pub model_download_state: Box<dyn Fn() -> DownloadStatus + 'static>,
    /// Запустить установку Vosk (libvosk + русская модель).
    pub vosk_install_start: Box<dyn Fn() + 'static>,
    /// Снимок состояния установки Vosk.
    pub vosk_state: Box<dyn Fn() -> DownloadStatus + 'static>,
    /// Сообщить приложению о захвате клавиши: глобальный хук на это время
    /// клавиши не перехватывает, иначе окно не получит нажатие.
    pub capture_active: Box<dyn Fn(bool) + 'static>,
    /// Очередь нажатий от хука (работает и для Pause/PrintScreen/ScrollLock).
    pub capture_queue: CaptureQueue,
    /// Запустить тест распознавания.
    pub voice_test_start: Box<dyn Fn() + 'static>,
    /// Остановить тест распознавания.
    pub voice_test_stop: Box<dyn Fn() + 'static>,
    /// Снимок состояния теста распознавания.
    pub voice_test_state: Box<dyn Fn() -> RecognitionTest + 'static>,
    /// Открыть окно сразу при старте (диагностика/тесты).
    pub open_on_start: bool,
    /// Автоматически выйти через N секунд (дымовой режим).
    pub smoke_seconds: Option<u64>,
}

/// Запустить окно настроек (блокирует вызывающий поток, пока окно открыто).
///
/// Жизненный цикл: окно создаётся при открытии и **закрывается по-настоящему**
/// (в трей уходит только приложение целиком). Скрытое eframe-окно на Windows
/// неразбудимо корректно — цикл winit зависает и крутит CPU.
///
/// # Errors
///
/// Ошибка создания окна/цикла eframe.
pub fn run(context: SettingsContext) -> anyhow::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Klavium — настройки")
            .with_icon(egui::IconData {
                rgba: crate::tray::neutral_icon_rgba(),
                width: crate::tray::ICON_SIZE,
                height: crate::tray::ICON_SIZE,
            })
            .with_inner_size([780.0, 560.0])
            .with_min_inner_size([640.0, 420.0])
            .with_visible(context.open_on_start),
        // Окно живёт, пока открыто: закрытие — штатный возврат из run_native.
        run_and_return: true,
        ..Default::default()
    };
    eframe::run_native(
        "Klavium",
        options,
        Box::new(move |cc| Ok(Box::new(SettingsApp::new(cc, context)))),
    )
    .map_err(|error| anyhow::anyhow!("eframe: {error}"))?;
    Ok(())
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Section {
    Correction,
    Hotkeys,
    Sounds,
    Timeouts,
    Notifications,
    Statistics,
    General,
    Voice,
}

const SECTIONS: [(Section, &str); 8] = [
    (Section::Correction, "Коррекция"),
    (Section::Hotkeys, "Горячие клавиши"),
    (Section::Sounds, "Звуки"),
    (Section::Timeouts, "Таймауты"),
    (Section::Notifications, "Уведомления"),
    (Section::Statistics, "Статистика"),
    (Section::General, "Общие"),
    (Section::Voice, "Голос"),
];

/// Стартовая страница настроек для дымовых прогонов (`KLAVIUM_APP_SETTINGS_PAGE`).
fn initial_section() -> Section {
    match std::env::var("KLAVIUM_APP_SETTINGS_PAGE").as_deref() {
        Ok("sounds") => Section::Sounds,
        Ok("hotkeys") => Section::Hotkeys,
        Ok("timeouts") => Section::Timeouts,
        Ok("notifications") => Section::Notifications,
        Ok("statistics") => Section::Statistics,
        Ok("general") => Section::General,
        Ok("voice") => Section::Voice,
        _ => Section::Correction,
    }
}

/// Заголовок и пояснение страницы раздела.
const fn section_header(section: Section) -> (&'static str, &'static str) {
    match section {
        Section::Correction => (
            "Коррекция раскладки",
            "Автоматически заменяет слово, набранное в неверной раскладке.\nКлавиши меняются в разделе «Горячие клавиши».",
        ),
        Section::Hotkeys => (
            "Горячие клавиши",
            "Нажмите кнопку с текущим сочетанием и введите новое (Esc отменяет ввод).\nЕсли клавиша не ловится, впишите сочетание в поле ниже.",
        ),
        Section::Sounds => (
            "Звуки",
            "Сигналы при исправлении, смене раскладки и голосовом вводе.",
        ),
        Section::Timeouts => (
            "Таймауты",
            "Когда слово считается завершённым и как часто проверяется раскладка.",
        ),
        Section::Notifications => ("Уведомления", "Подсказка у каретки и её длительность."),
        Section::Statistics => ("Статистика", "Сколько слов исправлено и когда вы работали."),
        Section::General => ("Общие", "Каталог данных, автозапуск и журнал."),
        Section::Voice => (
            "Голосовой ввод",
            "Движок распознавания, модели и тест микрофона.",
        ),
    }
}

/// Пункт бокового меню: подсветка выбранного и полоска акцента слева.
fn menu_item(ui: &mut egui::Ui, title: &str, active: bool) -> bool {
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), theme::MENU_ROW_HEIGHT),
        egui::Sense::click(),
    );
    let radius = egui::CornerRadius::same(theme::RADIUS_CONTROL);
    if active {
        ui.painter().rect_filled(rect, radius, theme::ACCENT_TINT);
        let bar = egui::Rect::from_min_size(
            egui::pos2(rect.left() + 4.0, rect.top() + 8.0),
            egui::vec2(3.0, rect.height() - 16.0),
        );
        ui.painter()
            .rect_filled(bar, egui::CornerRadius::same(2), theme::ACCENT);
    } else if response.hovered() {
        ui.painter()
            .rect_filled(rect, radius, Color32::from_black_alpha(10));
    }
    let color = if active {
        theme::ACCENT
    } else {
        theme::TEXT_PRIMARY
    };
    ui.painter().text(
        egui::pos2(rect.left() + 16.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        title,
        egui::FontId::proportional(theme::TEXT_BODY),
        color,
    );
    response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .clicked()
}

struct SettingsApp {
    config: AppConfig,
    original: AppConfig,
    section: Section,
    /// Действие, для которого сейчас ждём нажатие сочетания.
    capture: Option<HotkeyField>,
    status: Option<(String, bool)>,
    on_save: SaveHandler,
    sound_preview: Box<dyn Fn(SoundSpec) + 'static>,
    stats_snapshot: Box<dyn Fn() -> StatsData + 'static>,
    stats_reset: Box<dyn Fn() + 'static>,
    voice_toggle: Box<dyn Fn() + 'static>,
    voice_snapshot: Box<dyn Fn() -> (bool, String) + 'static>,
    data_dir: PathBuf,
    data_note: Option<String>,
    tray_queue: crate::tray::TrayQueue,
    data_dir_change: Box<dyn Fn(Option<PathBuf>) + 'static>,
    data_move_status: Box<dyn Fn() -> Option<String> + 'static>,
    model_download_start: Box<dyn Fn(String) + 'static>,
    model_download_state: Box<dyn Fn() -> DownloadStatus + 'static>,
    vosk_install_start: Box<dyn Fn() + 'static>,
    vosk_state: Box<dyn Fn() -> DownloadStatus + 'static>,
    capture_active: Box<dyn Fn(bool) + 'static>,
    capture_queue: CaptureQueue,
    capture_since: Option<Instant>,
    voice_test_start: Box<dyn Fn() + 'static>,
    voice_test_stop: Box<dyn Fn() + 'static>,
    voice_test_state: Box<dyn Fn() -> RecognitionTest + 'static>,
    smoke_seconds: Option<u64>,
    started: Instant,
    /// Иконка приложения в шапке сайдбара.
    icon: egui::TextureHandle,
    /// Статус в нижней панели (момент показа — для автоскрытия).
    status_at: Option<Instant>,
    /// Момент первого клика по «Сбросить статистику» (подтверждение).
    reset_armed: Option<Instant>,
}

impl SettingsApp {
    fn new(cc: &eframe::CreationContext<'_>, context: SettingsContext) -> Self {
        install_theme(&cc.egui_ctx);
        let icon = cc.egui_ctx.load_texture(
            "klavium_icon",
            egui::ColorImage::from_rgba_unmultiplied(
                [
                    crate::tray::ICON_SIZE as usize,
                    crate::tray::ICON_SIZE as usize,
                ],
                &crate::tray::neutral_icon_rgba(),
            ),
            egui::TextureOptions::LINEAR,
        );
        Self {
            original: context.config.clone(),
            config: context.config,
            section: initial_section(),
            capture: None,
            status: None,
            on_save: context.on_save,
            sound_preview: context.sound_preview,
            stats_snapshot: context.stats_snapshot,
            stats_reset: context.stats_reset,
            voice_toggle: context.voice_toggle,
            voice_snapshot: context.voice_snapshot,
            data_dir: context.data_dir,
            data_note: context.data_note,
            tray_queue: context.tray_queue,
            data_dir_change: context.data_dir_change,
            data_move_status: context.data_move_status,
            model_download_start: context.model_download_start,
            model_download_state: context.model_download_state,
            vosk_install_start: context.vosk_install_start,
            vosk_state: context.vosk_state,
            capture_active: context.capture_active,
            capture_queue: context.capture_queue,
            capture_since: None,
            voice_test_start: context.voice_test_start,
            voice_test_stop: context.voice_test_stop,
            voice_test_state: context.voice_test_state,
            smoke_seconds: context.smoke_seconds,
            started: Instant::now(),
            icon,
            status_at: None,
            reset_armed: None,
        }
    }

    fn save(&mut self) {
        self.cancel_capture();
        match (self.on_save)(self.config.clone()) {
            Ok(()) => {
                self.original = self.config.clone();
                self.set_status("Сохранено".to_string(), false);
            }
            Err(error) => {
                self.set_status(format!("Ошибка: {error}"), true);
            }
        }
    }

    /// Поставить статус в нижнюю панель с автоскрытием.
    fn set_status(&mut self, text: String, is_error: bool) {
        self.status = Some((text, is_error));
        self.status_at = Some(Instant::now());
    }

    /// Начать захват клавиши: глобальный хук на это время её не перехватывает.
    fn begin_capture(&mut self, field: HotkeyField) {
        if let Ok(mut queue) = self.capture_queue.lock() {
            queue.clear();
        }
        self.capture = Some(field);
        self.capture_since = Some(Instant::now());
        (self.capture_active)(true);
    }

    /// Прекратить захват клавиши.
    fn cancel_capture(&mut self) {
        self.capture = None;
        self.capture_since = None;
        if let Ok(mut queue) = self.capture_queue.lock() {
            queue.clear();
        }
        (self.capture_active)(false);
    }

    fn draw(&mut self, ctx: &egui::Context) {
        self.draw_sidebar(ctx);
        self.draw_bottom_bar(ctx);
        self.draw_content(ctx);
    }

    /// Боковое меню: шапка с иконкой и версией, разделы, ссылка на папку данных.
    fn draw_sidebar(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("sections")
            .resizable(false)
            .exact_width(theme::SIDEBAR_WIDTH)
            .frame(
                egui::Frame::NONE
                    .fill(theme::BG_SIDEBAR)
                    .stroke(Stroke::new(theme::HAIRLINE, theme::BORDER_SUBTLE))
                    .inner_margin(egui::Margin::symmetric(12, 14)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.add(egui::Image::new((self.icon.id(), egui::vec2(28.0, 28.0))));
                    ui.vertical(|ui| {
                        ui.label(
                            RichText::new("Klavium")
                                .size(15.0)
                                .strong()
                                .color(theme::TEXT_PRIMARY),
                        );
                        ui.label(
                            RichText::new(format!(
                                "исправление раскладки · v{}",
                                env!("CARGO_PKG_VERSION")
                            ))
                            .size(theme::TEXT_CAPTION)
                            .color(theme::TEXT_SECONDARY),
                        );
                    });
                });
                ui.add_space(theme::SPACE_4);

                for (section, title) in SECTIONS {
                    if menu_item(ui, title, self.section == section) {
                        self.section = section;
                    }
                }

                ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
                    ui.label(
                        RichText::new(format!(
                            "Выход: трей → «Выход» или {}",
                            hotkey_label(&self.config.hotkeys.quit)
                        ))
                        .size(theme::TEXT_CAPTION)
                        .color(theme::TEXT_SECONDARY),
                    );
                    ui.add_space(theme::SPACE_2);
                    let link = egui::Button::new(
                        RichText::new("Открыть папку данных")
                            .size(theme::TEXT_BODY)
                            .color(theme::TEXT_SECONDARY),
                    )
                    .fill(Color32::TRANSPARENT)
                    .stroke(Stroke::NONE);
                    if ui.add(link).clicked() {
                        let _ = std::process::Command::new("explorer")
                            .arg(&self.data_dir)
                            .spawn();
                    }
                });
            });
    }

    /// Нижняя панель: сохранить/сбросить, статус, «Скрыть в трей».
    fn draw_bottom_bar(&mut self, ctx: &egui::Context) {
        let dirty = self.config != self.original;

        egui::TopBottomPanel::bottom("actions")
            .frame(
                egui::Frame::NONE
                    .fill(theme::BG_WINDOW)
                    .stroke(Stroke::new(theme::HAIRLINE, theme::BORDER_SUBTLE))
                    .inner_margin(egui::Margin::symmetric(20, 10)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    let save = egui::Button::new(
                        RichText::new("Сохранить")
                            .size(theme::TEXT_BODY)
                            .color(Color32::WHITE),
                    )
                    .fill(theme::ACCENT)
                    .stroke(Stroke::NONE)
                    .corner_radius(egui::CornerRadius::same(theme::RADIUS_CONTROL))
                    .min_size(egui::vec2(96.0, theme::BUTTON_HEIGHT));
                    if ui.add_enabled(dirty, save).clicked() {
                        self.save();
                    }

                    let reset = egui::Button::new(
                        RichText::new("Сбросить")
                            .size(theme::TEXT_BODY)
                            .color(theme::TEXT_PRIMARY),
                    )
                    .fill(Color32::TRANSPARENT)
                    .stroke(Stroke::new(theme::HAIRLINE, theme::BORDER_STRONG))
                    .corner_radius(egui::CornerRadius::same(theme::RADIUS_CONTROL))
                    .min_size(egui::vec2(88.0, theme::BUTTON_HEIGHT));
                    if ui.add_enabled(dirty, reset).clicked() {
                        self.config = self.original.clone();
                        self.set_status("Изменения сброшены".to_string(), false);
                    }

                    ui.add_space(theme::SPACE_3);
                    if let Some((text, is_error)) = &self.status {
                        let color = if *is_error {
                            theme::DANGER
                        } else {
                            theme::SUCCESS
                        };
                        ui.label(RichText::new(text).size(theme::TEXT_MONO).color(color));
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let hide = egui::Button::new(
                            RichText::new("Скрыть в трей")
                                .size(theme::TEXT_BODY)
                                .color(theme::TEXT_PRIMARY),
                        )
                        .fill(Color32::TRANSPARENT)
                        .stroke(Stroke::new(theme::HAIRLINE, theme::BORDER_STRONG))
                        .corner_radius(egui::CornerRadius::same(theme::RADIUS_CONTROL))
                        .min_size(egui::vec2(0.0, theme::BUTTON_HEIGHT));
                        if ui.add(hide).clicked() {
                            // Закрываем окно по-настоящему: приложение остаётся в трее,
                            // окно будет создано заново при следующем открытии.
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                    });
                });
            });
    }

    /// Контентная область: заголовок раздела и карточка с настройками.
    fn draw_content(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default()
            .frame(
                egui::Frame::NONE
                    .fill(theme::BG_WINDOW)
                    .inner_margin(egui::Margin::symmetric(
                        theme::CONTENT_PADDING_X,
                        theme::CONTENT_PADDING_Y,
                    )),
            )
            .show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        // Захват клавиши живёт только в своей секции: иначе следующее
                        // нажатие в другом разделе молча переназначит хоткей.
                        if self.section != Section::Hotkeys {
                            self.cancel_capture();
                        }
                        let (title, caption) = section_header(self.section);
                        widgets::page_header(ui, title, caption);
                        ui.add_space(theme::SPACE_3);
                        widgets::card(ui, |ui| match self.section {
                            Section::Correction => self.draw_correction(ui),
                            Section::Hotkeys => self.draw_hotkeys(ui, ctx),
                            Section::Sounds => self.draw_sounds(ui),
                            Section::Timeouts => self.draw_timeouts(ui),
                            Section::Notifications => self.draw_notifications(ui),
                            Section::Statistics => self.draw_statistics(ui),
                            Section::General => self.draw_general(ui),
                            Section::Voice => self.draw_voice(ui),
                        });
                    });
            });
    }

    fn draw_correction(&mut self, ui: &mut egui::Ui) {
        let apply = hotkey_label(&self.config.hotkeys.correction_apply);
        let hide = hotkey_label(&self.config.hotkeys.card_hide);

        let mut enabled = self.config.correction.enabled;
        if widgets::setting_row(
            ui,
            "Включить исправление раскладки",
            None,
            |ui| widgets::switch(ui, &mut enabled),
        ) {
            self.config.correction.enabled = enabled;
        }
        widgets::row_separator(ui);

        let mut auto_correct = self.config.correction.auto_correct;
        if widgets::setting_row(
            ui,
            "Исправлять автоматически",
            Some("Без подтверждения: слово заменяется сразу"),
            |ui| widgets::switch(ui, &mut auto_correct),
        ) {
            self.config.correction.auto_correct = auto_correct;
        }
        widgets::row_separator(ui);

        let mut show_notifications = self.config.correction.show_notifications;
        if widgets::setting_row(
            ui,
            "Показывать подсказку у каретки",
            None,
            |ui| widgets::switch(ui, &mut show_notifications),
        ) {
            self.config.correction.show_notifications = show_notifications;
        }
        widgets::row_separator(ui);

        let mut threshold = self.config.correction.threshold;
        if widgets::setting_row(
            ui,
            "Порог уверенности",
            Some("Ниже порога слово считается верным и не предлагается"),
            |ui| {
                ui.label(
                    RichText::new(format!("{:.0}%", threshold * 100.0))
                        .size(theme::TEXT_BODY)
                        .strong()
                        .color(theme::TEXT_PRIMARY),
                );
                ui.add(
                    egui::Slider::new(&mut threshold, 0.0..=1.0)
                        .fixed_decimals(2)
                        .show_value(false)
                        .trailing_fill(true),
                )
            },
        ) {
            self.config.correction.threshold = threshold;
        }

        ui.add_space(theme::SPACE_3);
        ui.label(
            RichText::new(format!(
                "Применить — {apply}, скрыть подсказку — {hide}. \
                 Клавиши меняются в разделе «Горячие клавиши»."
            ))
            .size(theme::TEXT_CAPTION)
            .color(theme::TEXT_SECONDARY),
        );
    }

    /// Секция «Горячие клавиши»: назначение по нажатию, проверка и конфликты.
    fn draw_hotkeys(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let mut start_capture = None;
        let mut stop_capture = false;
        let mut clear_field = None;
        let mut default_field = None;

        for (index, field) in HotkeyField::ALL.into_iter().enumerate() {
            let spec = field.spec(&self.config).to_string();
            let capturing = self.capture == Some(field);
            let (start, clear, reset) = Self::hotkey_row(ui, field, spec.as_str(), capturing);

            if start {
                start_capture = Some(field);
            }
            if clear {
                clear_field = Some(field);
            }
            if reset {
                default_field = Some(field);
            }

            draw_hotkey_state(ui, spec.as_str(), capturing, &mut stop_capture);
            if index + 1 < HotkeyField::ALL.len() {
                widgets::row_separator(ui);
            }
        }

        let conflicts = hotkey_conflicts(&self.config);
        if !conflicts.is_empty() {
            ui.add_space(theme::SPACE_2);
            widgets::badge(
                ui,
                &format!("Одинаковые сочетания: {}", conflicts.join("; ")),
                widgets::BadgeKind::Error,
            );
        }

        if stop_capture {
            self.cancel_capture();
        }
        if let Some(field) = clear_field {
            field.spec_mut(&mut self.config).clear();
            self.cancel_capture();
        }
        if let Some(field) = default_field {
            *field.spec_mut(&mut self.config) = field.default_spec().to_string();
            self.cancel_capture();
        }
        if let Some(field) = start_capture {
            self.begin_capture(field);
        }
        self.handle_capture(ctx);

        self.draw_manual_hotkeys(ui);
    }

    /// Поля ручного ввода сочетаний (когда клавишу не удаётся поймать).
    fn draw_manual_hotkeys(&mut self, ui: &mut egui::Ui) {
        ui.add_space(theme::SPACE_3);
        ui.label(
            RichText::new("Ввести вручную")
                .size(theme::TEXT_CARD_TITLE)
                .strong()
                .color(theme::TEXT_PRIMARY),
        );
        ui.add_space(theme::SPACE_1);
        ui.label(
            RichText::new(
                "Формат: Ctrl+Pause, Ctrl+Shift+Q, F8, Space. Пустое поле — клавиша не назначена.",
            )
            .size(theme::TEXT_CAPTION)
            .color(theme::TEXT_SECONDARY),
        );
        ui.add_space(theme::SPACE_2);
        for field in HotkeyField::ALL {
            ui.horizontal(|ui| {
                ui.label(RichText::new(field.title()).size(theme::TEXT_BODY));
                ui.add(
                    egui::TextEdit::singleline(field.spec_mut(&mut self.config))
                        .desired_width(190.0),
                );
            });
        }
    }

    /// Строка хоткея: название с подсказкой, кейкап сочетания и текстовые кнопки.
    ///
    /// Возвращает `(начать захват, очистить, по умолчанию)`.
    fn hotkey_row(
        ui: &mut egui::Ui,
        field: HotkeyField,
        spec: &str,
        capturing: bool,
    ) -> (bool, bool, bool) {
        let mut start = false;
        let mut clear = false;
        let mut reset = false;

        widgets::setting_row(ui, field.title(), Some(field.hint()), |ui| {
            // Справа налево: «По умолчанию», «Очистить», кейкап сочетания.
            if widgets::link_button(ui, "По умолчанию").clicked() {
                reset = true;
            }
            if widgets::link_button(ui, "Очистить").clicked() {
                clear = true;
            }
            let label = if capturing {
                "Нажмите сочетание…".to_string()
            } else {
                hotkey_label(spec)
            };
            let stroke = if capturing {
                Stroke::new(1.5_f32, theme::ACCENT)
            } else {
                Stroke::new(theme::HAIRLINE, theme::BORDER_STRONG)
            };
            let button = egui::Button::new(
                RichText::new(label)
                    .monospace()
                    .size(theme::TEXT_MONO)
                    .color(if capturing {
                        theme::ACCENT
                    } else {
                        theme::TEXT_PRIMARY
                    }),
            )
            .fill(theme::BG_CARD)
            .stroke(stroke)
            .corner_radius(egui::CornerRadius::same(theme::RADIUS_KEYCAP))
            .min_size(egui::vec2(170.0, 26.0));
            let response = ui.add(button);
            if response.clicked() {
                start = true;
            }
            response
        });

        (start, clear, reset)
    }

    /// Поймать следующее нажатие для выбранного действия.
    ///
    /// Сначала — событие окна (обычные клавиши), затем очередь от хука
    /// ([`CaptureQueue`]): `Pause`/`PrintScreen` окно может не получить.
    fn handle_capture(&mut self, ctx: &egui::Context) {
        let Some(field) = self.capture else {
            return;
        };
        if self
            .capture_since
            .is_some_and(|since| since.elapsed() > Duration::from_secs(10))
        {
            self.status = Some((
                "Время ожидания истекло — нажмите кнопку с сочетанием ещё раз".to_string(),
                false,
            ));
            self.cancel_capture();
            return;
        }

        // 1) Клавиша, доставленная окну (winit/egui).
        let window_key = ctx.input(|input| {
            input.events.iter().find_map(|event| match event {
                egui::Event::Key {
                    key,
                    pressed: true,
                    repeat: false,
                    modifiers,
                    ..
                } => Some((*key, *modifiers)),
                _ => None,
            })
        });
        if let Some((key, modifiers)) = window_key {
            if key == egui::Key::Escape && modifiers.is_none() {
                self.cancel_capture();
                return;
            }
            if let Ok(spec) = hotkey_from_egui(key, modifiers) {
                self.status = Some((format!("Назначено: {spec}"), false));
                *field.spec_mut(&mut self.config) = spec;
                self.cancel_capture();
                return;
            }
        }

        // 2) Клавиша из глобального хука (Pause, PrintScreen, key-up).
        let pressed = self
            .capture_queue
            .lock()
            .ok()
            .and_then(|mut queue| queue.pop_front());
        let Some(key) = pressed else {
            ctx.request_repaint();
            return;
        };
        if key.vk_code == 0x1B && key.mods.is_empty() {
            self.cancel_capture();
            return;
        }
        match Hotkey::from_key(key.vk_code, key.scan_code, key.mods) {
            Some(hotkey) => {
                *field.spec_mut(&mut self.config) = hotkey.to_spec();
                self.status = Some((format!("Назначено: {hotkey}"), false));
                self.cancel_capture();
            }
            None => {
                self.status = Some((
                    "Эту клавишу нельзя назначить — нажмите другую".to_string(),
                    true,
                ));
            }
        }
    }

    fn draw_sounds(&mut self, ui: &mut egui::Ui) {
        ui.label(
            RichText::new(
                "У каждого звука: тумблер, число сигналов, пауза между ними, частота и длительность. \
                 «Прослушать» играет с текущими значениями — можно до «Сохранить».",
            )
            .size(theme::TEXT_CAPTION)
            .color(theme::TEXT_SECONDARY),
        );
        ui.add_space(theme::SPACE_3);

        let preview = self.sound_preview.as_ref();
        let sounds = &mut self.config.sounds;
        let rows: [(&str, &mut SoundSpec); 5] = [
            ("Английское слово", &mut sounds.en),
            ("Русское слово", &mut sounds.ru),
            ("Смена раскладки", &mut sounds.switch),
            ("Старт голоса", &mut sounds.voice_start),
            ("Стоп голоса", &mut sounds.voice_stop),
        ];

        for (index, (title, spec)) in rows.into_iter().enumerate() {
            let enabled = &mut spec.enabled;
            widgets::setting_row(ui, title, None, |ui| widgets::switch(ui, enabled));
            ui.horizontal(|ui| {
                ui.add(
                    egui::DragValue::new(&mut spec.count)
                        .range(1..=10)
                        .suffix(" сигн."),
                )
                .on_hover_text("Сколько сигналов в последовательности");
                ui.add(
                    egui::DragValue::new(&mut spec.pause_ms)
                        .range(10..=1000)
                        .prefix("пауза ")
                        .suffix(" мс"),
                )
                .on_hover_text("Пауза между сигналами (при одном сигнале не важна)");
                ui.add(
                    egui::DragValue::new(&mut spec.frequency)
                        .range(200..=4000)
                        .suffix(" Гц"),
                )
                .on_hover_text("Частота сигнала");
                ui.add(
                    egui::DragValue::new(&mut spec.duration_ms)
                        .range(10..=2000)
                        .suffix(" мс"),
                )
                .on_hover_text("Длительность одного сигнала");
                if widgets::secondary_button(ui, "Прослушать").clicked() {
                    preview(spec.clone());
                }
            });
            if index + 1 < 5 {
                widgets::row_separator(ui);
            }
        }
    }

    fn draw_timeouts(&mut self, ui: &mut egui::Ui) {
        let word_start = &mut self.config.timeouts.word_start;
        widgets::setting_row(
            ui,
            "Пауза между словами",
            Some("После такой паузы новое слово считается отдельным"),
            |ui| {
                ui.label(
                    RichText::new(format!("{word_start:.1} с"))
                        .size(theme::TEXT_BODY)
                        .strong()
                        .color(theme::TEXT_PRIMARY),
                );
                ui.add(
                    egui::Slider::new(word_start, 0.5..=60.0)
                        .show_value(false)
                        .trailing_fill(true),
                )
            },
        );
        widgets::row_separator(ui);
        let layout_check = &mut self.config.timeouts.layout_check;
        widgets::setting_row(
            ui,
            "Интервал проверки раскладки",
            Some("Как часто сверяется раскладка активного окна"),
            |ui| {
                ui.label(
                    RichText::new(format!("{layout_check:.1} с"))
                        .size(theme::TEXT_BODY)
                        .strong()
                        .color(theme::TEXT_PRIMARY),
                );
                ui.add(
                    egui::Slider::new(layout_check, 0.1..=10.0)
                        .show_value(false)
                        .trailing_fill(true),
                )
            },
        );

        widgets::row_separator(ui);
        let speed_correction = &mut self.config.typing.speed_correction;
        widgets::setting_row(
            ui,
            "Скорость печати исправлений",
            Some("«Мгновенно» может терять символы в терминалах и RDP"),
            |ui| {
                widgets::segmented(
                    ui,
                    &[
                        ("careful", "Аккуратно"),
                        ("fast", "Быстро"),
                        ("instant", "Мгновенно"),
                    ],
                    speed_correction,
                )
            },
        );
        widgets::row_separator(ui);
        let speed_voice = &mut self.config.typing.speed_voice;
        widgets::setting_row(
            ui,
            "Скорость голосовой вставки",
            None,
            |ui| {
                widgets::segmented(
                    ui,
                    &[
                        ("careful", "Аккуратно"),
                        ("fast", "Быстро"),
                        ("instant", "Мгновенно"),
                    ],
                    speed_voice,
                )
            },
        );
    }

    fn draw_notifications(&mut self, ui: &mut egui::Ui) {
        let enabled = &mut self.config.notifications.enabled;
        widgets::setting_row(
            ui,
            "Показывать уведомления",
            Some("Тост о смене раскладки вверху экрана"),
            |ui| widgets::switch(ui, enabled),
        );
        widgets::row_separator(ui);
        let duration = &mut self.config.notifications.duration_ms;
        widgets::setting_row(ui, "Длительность", None, |ui| {
            ui.label(
                RichText::new(format!("{duration} мс"))
                    .size(theme::TEXT_BODY)
                    .strong()
                    .color(theme::TEXT_PRIMARY),
            );
            ui.add(
                egui::Slider::new(duration, 500..=10000)
                    .show_value(false)
                    .trailing_fill(true),
            )
        });
        ui.add_space(theme::SPACE_3);
        ui.horizontal(|ui| {
            ui.label(
                RichText::new("Превью")
                    .size(theme::TEXT_CAPTION)
                    .color(theme::TEXT_SECONDARY),
            );
            widgets::color_pill(ui, "ENGLISH", theme::ACCENT);
            widgets::color_pill(ui, "РУССКИЙ", theme::DANGER);
        });
    }

    fn draw_statistics(&mut self, ui: &mut egui::Ui) {
        let enabled = &mut self.config.statistics.enabled;
        widgets::setting_row(
            ui,
            "Вести статистику",
            Some("Слова, переключения раскладки и сессии"),
            |ui| widgets::switch(ui, enabled),
        );
        widgets::row_separator(ui);
        let reset_on_start = &mut self.config.statistics.reset_on_start;
        widgets::setting_row(
            ui,
            "Сбрасывать при запуске",
            None,
            |ui| widgets::switch(ui, reset_on_start),
        );

        ui.add_space(theme::SPACE_4);
        let snapshot = (self.stats_snapshot)();
        egui::Grid::new("stats_metrics")
            .num_columns(3)
            .min_col_width(150.0)
            .spacing([theme::SPACE_3, theme::SPACE_3])
            .show(ui, |ui| {
                widgets::metric_card(ui, "Слова EN", &snapshot.total_words.en.to_string(), None);
                widgets::metric_card(ui, "Слова RU", &snapshot.total_words.ru.to_string(), None);
                widgets::metric_card(ui, "Голосом", &snapshot.total_words.voice.to_string(), None);
                ui.end_row();
                widgets::metric_card(
                    ui,
                    "Переключения",
                    &snapshot.layout_switches.to_string(),
                    None,
                );
                widgets::metric_card(
                    ui,
                    "Прочие слова",
                    &snapshot.total_words.other.to_string(),
                    None,
                );
                widgets::metric_card(ui, "Сессий", &snapshot.sessions.len().to_string(), None);
                ui.end_row();
            });

        ui.add_space(theme::SPACE_4);
        Self::draw_sessions(ui, &snapshot);

        ui.add_space(theme::SPACE_4);
        let armed = self
            .reset_armed
            .is_some_and(|at| at.elapsed() < Duration::from_secs(3));
        let label = if armed {
            "Точно сбросить?"
        } else {
            "Сбросить статистику"
        };
        let button = egui::Button::new(
            RichText::new(label)
                .size(theme::TEXT_BODY)
                .color(theme::DANGER),
        )
        .fill(Color32::TRANSPARENT)
        .stroke(Stroke::new(theme::HAIRLINE, theme::DANGER))
        .corner_radius(egui::CornerRadius::same(theme::RADIUS_CONTROL))
        .min_size(egui::vec2(0.0, 30.0));
        if ui.add(button).clicked() {
            if armed {
                (self.stats_reset)();
                self.reset_armed = None;
                self.set_status("Статистика сброшена".to_string(), false);
            } else {
                self.reset_armed = Some(Instant::now());
            }
        }
    }

    /// Состояние диктовки: бейдж, кнопка включения и текст статуса движка.
    fn draw_voice_state(&self, ui: &mut egui::Ui, active: bool, status: &str) {
        ui.horizontal(|ui| {
            let (text, kind) = if active {
                ("диктовка активна", widgets::BadgeKind::Ok)
            } else {
                ("диктовка выключена", widgets::BadgeKind::Info)
            };
            widgets::badge(ui, text, kind);
            let label = if active {
                "Выключить"
            } else {
                "Включить"
            };
            if widgets::secondary_button(ui, label).clicked() {
                (self.voice_toggle)();
            }
        });
        ui.add_space(theme::SPACE_1);
        ui.label(
            RichText::new(status)
                .size(theme::TEXT_CAPTION)
                .color(theme::TEXT_SECONDARY),
        );
    }

    /// Компактная таблица последних сессий.
    fn draw_sessions(ui: &mut egui::Ui, snapshot: &StatsData) {
        ui.label(
            RichText::new("История сессий")
                .size(theme::TEXT_CARD_TITLE)
                .strong()
                .color(theme::TEXT_PRIMARY),
        );
        ui.add_space(theme::SPACE_2);
        if snapshot.sessions.is_empty() {
            ui.label(
                RichText::new("Пока нет данных")
                    .size(theme::TEXT_CAPTION)
                    .color(theme::TEXT_SECONDARY),
            );
            return;
        }
        egui::Grid::new("stats_sessions")
            .num_columns(3)
            .spacing([theme::SPACE_4, theme::SPACE_1])
            .show(ui, |ui| {
                for session in snapshot.sessions.iter().rev().take(8) {
                    let words = session.words.en
                        + session.words.ru
                        + session.words.other
                        + session.words.voice;
                    ui.label(
                        RichText::new(human_ago(session.started_at))
                            .monospace()
                            .size(theme::TEXT_MONO)
                            .color(theme::TEXT_SECONDARY),
                    );
                    ui.label(
                        RichText::new(format!("{} мин", session.duration_s / 60))
                            .monospace()
                            .size(theme::TEXT_MONO)
                            .color(theme::TEXT_SECONDARY),
                    );
                    ui.label(
                        RichText::new(format!("{words} сл."))
                            .monospace()
                            .size(theme::TEXT_MONO)
                            .color(theme::TEXT_PRIMARY),
                    );
                    ui.end_row();
                }
            });
    }

    fn draw_general(&mut self, ui: &mut egui::Ui) {
        let autostart = &mut self.config.autostart.enabled;
        widgets::setting_row(
            ui,
            "Запускать вместе с Windows",
            Some("Автозапуск в трее, без окна"),
            |ui| widgets::switch(ui, autostart),
        );
        widgets::row_separator(ui);

        let logging_enabled = &mut self.config.logging.enabled;
        widgets::setting_row(
            ui,
            "Вести журнал",
            Some("Файл logs\\app.log в каталоге данных"),
            |ui| widgets::switch(ui, logging_enabled),
        );
        widgets::row_separator(ui);

        let level = &mut self.config.logging.level;
        widgets::setting_row(ui, "Уровень журнала", None, |ui| {
            widgets::segmented(
                ui,
                &[
                    ("ERROR", "Ошибки"),
                    ("WARNING", "Предупр."),
                    ("INFO", "Инфо"),
                    ("DEBUG", "Отладка"),
                ],
                level,
            )
        });

        ui.add_space(theme::SPACE_4);
        ui.label(
            RichText::new("Каталог данных")
                .size(theme::TEXT_CARD_TITLE)
                .strong()
                .color(theme::TEXT_PRIMARY),
        );
        ui.add_space(theme::SPACE_1);
        ui.label(
            RichText::new(
                "Здесь хранятся настройки, статистика, журналы и модели. \
                 По умолчанию — папка data рядом с программой.",
            )
            .size(theme::TEXT_CAPTION)
            .color(theme::TEXT_SECONDARY),
        );
        ui.add_space(theme::SPACE_2);
        ui.label(
            RichText::new(self.data_dir.display().to_string())
                .monospace()
                .size(theme::TEXT_MONO)
                .color(theme::TEXT_PRIMARY),
        );
        if let Some(note) = &self.data_note {
            ui.add_space(theme::SPACE_1);
            widgets::badge(ui, note, widgets::BadgeKind::Warn);
        }
        ui.add_space(theme::SPACE_2);
        ui.horizontal(|ui| {
            if widgets::secondary_button(ui, "Выбрать папку…").clicked() {
                if let Some(dir) = rfd::FileDialog::new()
                    .set_directory(&self.data_dir)
                    .pick_folder()
                {
                    (self.data_dir_change)(Some(dir));
                }
            }
            if widgets::secondary_button(ui, "По умолчанию").clicked() {
                (self.data_dir_change)(None);
            }
            if widgets::secondary_button(ui, "Открыть").clicked() {
                let _ = std::process::Command::new("explorer")
                    .arg(&self.data_dir)
                    .spawn();
            }
        });
        if let Some(status) = (self.data_move_status)() {
            ui.add_space(theme::SPACE_1);
            widgets::badge(ui, &status, widgets::BadgeKind::Warn);
        }
    }

    fn draw_voice(&mut self, ui: &mut egui::Ui) {
        let hotkey = hotkey_label(&self.config.hotkeys.voice_toggle);
        let (active, status) = (self.voice_snapshot)();

        ui.label(
            RichText::new(format!(
                "Включение диктовки — {hotkey}. Клавиша меняется в разделе «Горячие клавиши»."
            ))
            .size(theme::TEXT_CAPTION)
            .color(theme::TEXT_SECONDARY),
        );
        ui.add_space(theme::SPACE_3);

        let voice = &mut self.config.voice;
        let enabled = &mut voice.enabled;
        widgets::setting_row(
            ui,
            "Голосовой ввод",
            Some("Диктовка в любое поле ввода"),
            |ui| widgets::switch(ui, enabled),
        );
        widgets::row_separator(ui);

        let language = &mut voice.language;
        widgets::setting_row(ui, "Язык распознавания", None, |ui| {
            widgets::segmented(
                ui,
                &[("auto", "Авто"), ("ru", "Русский"), ("en", "Английский")],
                language,
            )
        });
        widgets::row_separator(ui);

        let punct = &mut voice.punct_dict;
        widgets::setting_row(
            ui,
            "Голосовая пунктуация",
            Some("«запятая» → «,»"),
            |ui| widgets::switch(ui, punct),
        );
        widgets::row_separator(ui);

        let caps = &mut voice.auto_capitalize;
        widgets::setting_row(
            ui,
            "Автозаглавные буквы",
            Some("После точки"),
            |ui| widgets::switch(ui, caps),
        );
        widgets::row_separator(ui);

        let paste_long = &mut voice.paste_long_text;
        widgets::setting_row(
            ui,
            "Длинный текст через буфер",
            Some("Фраза вставляется одним Ctrl+V — быстрее и без ряби в окне"),
            |ui| widgets::switch(ui, paste_long),
        );

        ui.add_space(theme::SPACE_4);
        ui.label(
            RichText::new("Движок распознавания")
                .size(theme::TEXT_CARD_TITLE)
                .strong()
                .color(theme::TEXT_PRIMARY),
        );
        ui.add_space(theme::SPACE_2);
        draw_voice_engine(
            ui,
            voice,
            &self.data_dir,
            &*self.model_download_start,
            &*self.model_download_state,
            &*self.vosk_install_start,
            &*self.vosk_state,
        );

        ui.add_space(theme::SPACE_4);
        self.draw_voice_state(ui, active, &status);

        ui.add_space(theme::SPACE_4);
        ui.label(
            RichText::new("Тест распознавания")
                .size(theme::TEXT_CARD_TITLE)
                .strong()
                .color(theme::TEXT_PRIMARY),
        );
        ui.add_space(theme::SPACE_1);
        draw_voice_test(
            ui,
            &*self.voice_test_start,
            &*self.voice_test_stop,
            &*self.voice_test_state,
        );
    }
}

/// Состояние строки хоткея: ожидание нажатия, ошибка, предупреждение.
fn draw_hotkey_state(ui: &mut egui::Ui, spec: &str, capturing: bool, stop_capture: &mut bool) {
    if capturing {
        ui.horizontal(|ui| {
            ui.colored_label(Color32::from_rgb(87, 200, 120), "● ждём нажатия клавиши…");
            if ui.small_button("Отмена").clicked() {
                *stop_capture = true;
            }
        });
    } else if spec.trim().is_empty() {
        ui.label(
            RichText::new("не задано — действие доступно из меню трея")
                .small()
                .weak(),
        );
    } else {
        match Hotkey::parse(spec) {
            Err(error) => {
                ui.colored_label(
                    Color32::from_rgb(235, 87, 87),
                    format!("Сочетание не разобрано: {error}"),
                );
            }
            Ok(hotkey) if hotkey.is_risky() => {
                ui.colored_label(
                    Color32::from_rgb(230, 180, 80),
                    "Без модификаторов: сочетание будет перехватывать обычный набор.",
                );
            }
            Ok(_) => {}
        }
    }
}

/// Действие с настраиваемой горячей клавишей.
#[derive(Clone, Copy, PartialEq, Eq)]
enum HotkeyField {
    CorrectionApply,
    CorrectionUndo,
    CardHide,
    VoiceToggle,
    Quit,
}

impl HotkeyField {
    const ALL: [Self; 5] = [
        Self::CorrectionApply,
        Self::CorrectionUndo,
        Self::CardHide,
        Self::VoiceToggle,
        Self::Quit,
    ];

    const fn title(self) -> &'static str {
        match self {
            Self::CorrectionApply => "Применить исправление",
            Self::CorrectionUndo => "Отменить исправление",
            Self::CardHide => "Скрыть подсказку",
            Self::VoiceToggle => "Голосовой ввод",
            Self::Quit => "Выход из Klavium",
        }
    }

    const fn hint(self) -> &'static str {
        match self {
            Self::CorrectionApply => "Срабатывает, только когда есть что исправлять.",
            Self::CorrectionUndo => {
                "Возвращает исходное слово и раскладку после исправления (до минуты)."
            }
            Self::CardHide => {
                "Прячет карточку «было → станет»; без подсказки клавиша работает как обычно."
            }
            Self::VoiceToggle => "Включает и выключает диктовку (когда голосовой ввод включён).",
            Self::Quit => "Полностью закрывает программу.",
        }
    }

    const fn default_spec(self) -> &'static str {
        match self {
            Self::CorrectionApply => "F8",
            Self::CorrectionUndo => "Ctrl+Alt+Z",
            Self::CardHide => "Esc",
            Self::VoiceToggle => "F9",
            Self::Quit => "Ctrl+Shift+Q",
        }
    }

    const fn spec(self, config: &AppConfig) -> &str {
        match self {
            Self::CorrectionApply => config.hotkeys.correction_apply.as_str(),
            Self::CorrectionUndo => config.hotkeys.correction_undo.as_str(),
            Self::CardHide => config.hotkeys.card_hide.as_str(),
            Self::VoiceToggle => config.hotkeys.voice_toggle.as_str(),
            Self::Quit => config.hotkeys.quit.as_str(),
        }
    }

    const fn spec_mut(self, config: &mut AppConfig) -> &mut String {
        match self {
            Self::CorrectionApply => &mut config.hotkeys.correction_apply,
            Self::CorrectionUndo => &mut config.hotkeys.correction_undo,
            Self::CardHide => &mut config.hotkeys.card_hide,
            Self::VoiceToggle => &mut config.hotkeys.voice_toggle,
            Self::Quit => &mut config.hotkeys.quit,
        }
    }
}

/// Подпись сочетания для интерфейса (каноническая запись или «не задано»).
fn hotkey_label(spec: &str) -> String {
    let spec = spec.trim();
    if spec.is_empty() {
        return "не задано".to_string();
    }
    Hotkey::parse(spec).map_or_else(|_| spec.to_string(), Hotkey::to_spec)
}

/// Пары действий с одинаковым сочетанием — для предупреждения в настройках.
fn hotkey_conflicts(config: &AppConfig) -> Vec<String> {
    let mut seen: Vec<(Hotkey, HotkeyField)> = Vec::new();
    let mut conflicts = Vec::new();
    for field in HotkeyField::ALL {
        let Ok(hotkey) = Hotkey::parse(field.spec(config)) else {
            continue;
        };
        if let Some((_, other)) = seen.iter().find(|(existing, _)| *existing == hotkey) {
            conflicts.push(format!("«{}» и «{}»", other.title(), field.title()));
        } else {
            seen.push((hotkey, field));
        }
    }
    conflicts
}

/// Собрать спецификацию из события окна (клавиша + модификаторы).
fn hotkey_from_egui(key: egui::Key, modifiers: egui::Modifiers) -> Result<String, String> {
    let mut spec = String::new();
    // На Windows `command` — это Ctrl; Win-клавиша в egui не попадает.
    if modifiers.ctrl || modifiers.command {
        spec.push_str("Ctrl+");
    }
    if modifiers.shift {
        spec.push_str("Shift+");
    }
    if modifiers.alt {
        spec.push_str("Alt+");
    }
    spec.push_str(egui_key_token(key));
    Hotkey::parse(&spec)
        .map(Hotkey::to_spec)
        .map_err(|error| format!("Эту клавишу нельзя назначить: {error}"))
}

/// Имя клавиши окна в терминах спецификации хоткея.
fn egui_key_token(key: egui::Key) -> &'static str {
    match key {
        egui::Key::Minus => "-",
        egui::Key::Equals => "=",
        egui::Key::Comma => ",",
        egui::Key::Period => ".",
        egui::Key::Semicolon => ";",
        egui::Key::Quote => "'",
        egui::Key::Backtick => "`",
        egui::Key::Slash => "/",
        egui::Key::Backslash => "\\",
        egui::Key::OpenBracket => "[",
        egui::Key::CloseBracket => "]",
        other => other.name(),
    }
}

/// Сколько времени назад была сессия (без часовых поясов — только разница).
fn human_ago(started_at: u64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let diff = now.saturating_sub(started_at);
    if diff < 3600 {
        format!("{} мин назад", diff / 60)
    } else if diff < 86_400 {
        format!("{} ч назад", diff / 3600)
    } else {
        format!("{} дн назад", diff / 86_400)
    }
}

/// Поднять и сфокусировать уже открытое окно настроек (клик «Настройки»
/// или «Статистика» в трее).
fn show_window(ctx: &egui::Context) {
    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
    ctx.request_repaint();
}

impl eframe::App for SettingsApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Статус в нижней панели живёт 4 секунды.
        if self
            .status_at
            .is_some_and(|at| at.elapsed() > Duration::from_secs(4))
        {
            self.status = None;
            self.status_at = None;
        }
        if self.status.is_some() {
            ctx.request_repaint_after(Duration::from_millis(250));
        }

        // События меню трея (передаёт поток-наблюдатель из приложения)
        while let Some(id) = crate::tray::pop_queued_event(&self.tray_queue) {
            match id.as_str() {
                crate::tray::MENU_SETTINGS => show_window(ctx),
                crate::tray::MENU_STATS => {
                    self.section = Section::Statistics;
                    show_window(ctx);
                }
                crate::tray::MENU_VOICE => (self.voice_toggle)(),
                crate::tray::MENU_QUIT => {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
                _ => {}
            }
        }

        // Дымовой режим: авто-выход по таймеру
        if let Some(seconds) = self.smoke_seconds {
            if self.started.elapsed() >= Duration::from_secs(seconds) {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }

        // Всегда просыпаемся периодически: опрос меню трея, живая статистика
        // и дымовой таймер должны работать независимо от потока событий окна.
        ctx.request_repaint_after(Duration::from_millis(200));

        self.draw(ctx);
    }
}

impl Drop for SettingsApp {
    fn drop(&mut self) {
        // Окно закрыли во время захвата — вернуть хуку обычный режим.
        (self.capture_active)(false);
    }
}

/// Найти файл модели: `configured` — имя/путь (пусто — лучшая из загруженных).
fn find_model(models_dir: &std::path::Path, configured: &str) -> Option<std::path::PathBuf> {
    let configured = configured.trim();
    if !configured.is_empty() {
        let path = std::path::Path::new(configured);
        let path = if path.is_absolute() {
            path.to_path_buf()
        } else {
            models_dir.join(configured)
        };
        return path.is_file().then_some(path);
    }
    MODELS
        .iter()
        .rev()
        .map(|spec| models_dir.join(spec.file))
        .find(|candidate| candidate.is_file())
}

/// Подпись выбора модели для комбобокса.
fn model_choice_label(configured: &str) -> String {
    let configured = configured.trim();
    if configured.is_empty() {
        return "Авто — лучшая из загруженных".to_string();
    }
    chosen_spec(configured).map_or_else(
        || format!("Другая: {configured}"),
        |spec| format!("{} — {}", spec.title, spec.size),
    )
}

/// Спецификация выбранного файла модели (если он из каталога точности).
fn chosen_spec(configured: &str) -> Option<&'static klavium_core::voice::ModelSpec> {
    let name = configured.trim();
    MODELS.iter().find(|spec| spec.file == name)
}

/// Блок «Vosk»: статус библиотеки/модели и установка в один клик.
fn draw_voice_vosk(
    ui: &mut egui::Ui,
    voice: &mut klavium_core::config::VoiceConfig,
    data_dir: &std::path::Path,
    install_start: &dyn Fn(),
    install_state: &dyn Fn() -> DownloadStatus,
) {
    let vosk_dir = data_dir.join("vosk");
    let models_root = data_dir.join("models").join("vosk");
    let library_ok = vosk_dir.join("libvosk.dll").is_file() || exe_side_libvosk().is_some();
    let model = first_subdir(&models_root);
    match (library_ok, model.as_ref()) {
        (true, Some(dir)) => {
            let name = dir.file_name().and_then(|n| n.to_str()).unwrap_or("модель");
            ui.colored_label(
                Color32::from_rgb(87, 200, 120),
                format!("✓ Vosk готов: {name}"),
            );
        }
        (false, _) => {
            ui.colored_label(
                Color32::from_rgb(235, 87, 87),
                "✗ нет libvosk.dll — нажмите «Установить Vosk»",
            );
        }
        (true, None) => {
            ui.colored_label(
                Color32::from_rgb(235, 87, 87),
                "✗ нет русской модели — нажмите «Установить Vosk»",
            );
        }
    }
    let state = install_state();
    ui.horizontal(|ui| {
        if ui
            .add_enabled(
                !state.active,
                egui::Button::new("Установить Vosk (libvosk + рус. модель, ≈60 МБ)"),
            )
            .clicked()
        {
            install_start();
        }
        if ui.button("Открыть папку моделей").clicked() {
            let _ = std::fs::create_dir_all(&models_root);
            let _ = std::process::Command::new("explorer")
                .arg(&models_root)
                .spawn();
        }
    });
    if state.active {
        let percent = state.percent.unwrap_or(0);
        ui.add(
            egui::ProgressBar::new(f32::from(percent) / 100.0)
                .desired_width(320.0)
                .text(format!("{} — {percent}%", state.file)),
        );
    } else if let Some(error) = state.error.clone() {
        ui.colored_label(
            Color32::from_rgb(235, 87, 87),
            format!("ошибка установки: {error}"),
        );
    } else if state.done {
        ui.colored_label(Color32::from_rgb(87, 200, 120), "✓ Vosk установлен");
    }
    ui.horizontal(|ui| {
        ui.label("Папка модели (пусто — авто):");
        ui.add(
            egui::TextEdit::singleline(&mut voice.vosk_model_path)
                .hint_text("автопоиск в models\\vosk (это норма)")
                .desired_width(250.0),
        );
    });
    if voice.vosk_model_path.trim().is_empty() {
        match &model {
            Some(dir) => {
                let name = dir.file_name().and_then(|n| n.to_str()).unwrap_or("модель");
                ui.label(
                    RichText::new(format!("используется: {name}"))
                        .small()
                        .weak(),
                );
            }
            None => {
                ui.label(
                    RichText::new("используется: (модель не найдена)")
                        .small()
                        .weak(),
                );
            }
        }
    }
}

/// Текст для проверки распознавания (читать вслух, с короткими паузами).
const VOICE_TEST_SAMPLE: &str = "Тест распознавания речи: сегодня пятнадцатое октября, погода — плюс семь градусов; экспресс-доставка в подъезд номер четыре, объявление и съёмка.";

/// Блок «Тест распознавания»: прочитать сложный текст и увидеть результат.
fn draw_voice_test(
    ui: &mut egui::Ui,
    test_start: &dyn Fn(),
    test_stop: &dyn Fn(),
    test_state: &dyn Fn() -> RecognitionTest,
) {
    ui.label(
        RichText::new("Прочитайте вслух (фразы разделяйте короткими паузами):")
            .size(theme::TEXT_CAPTION)
            .color(theme::TEXT_SECONDARY),
    );
    ui.label(
        RichText::new(VOICE_TEST_SAMPLE)
            .size(theme::TEXT_CAPTION)
            .italics()
            .color(theme::TEXT_PRIMARY),
    );
    ui.add_space(theme::SPACE_2);
    let state = test_state();
    ui.horizontal(|ui| {
        if state.running {
            if widgets::secondary_button(ui, "Стоп").clicked() {
                test_stop();
            }
            ui.label(
                RichText::new(format!("осталось ~{} с", state.seconds_left))
                    .size(theme::TEXT_CAPTION)
                    .color(theme::TEXT_SECONDARY),
            );
        } else if state.waiting_engine {
            if widgets::secondary_button(ui, "Отмена").clicked() {
                test_stop();
            }
            ui.label(
                RichText::new("ждём загрузку движка…")
                    .size(theme::TEXT_CAPTION)
                    .color(theme::TEXT_SECONDARY),
            );
        } else if widgets::secondary_button(ui, "Начать тест (15 с)").clicked() {
            test_start();
        }
        if !state.results.is_empty() && !state.running {
            ui.label(
                RichText::new("(новый тест очистит прошлый результат)")
                    .size(theme::TEXT_CAPTION)
                    .color(theme::TEXT_SECONDARY),
            );
        }
    });
    if state.waiting_engine {
        ui.label(
            RichText::new(
                "Движок ещё загружается — тест начнётся автоматически, как только он будет готов.",
            )
            .size(theme::TEXT_CAPTION)
            .color(theme::TEXT_SECONDARY),
        );
    }
    if !state.results.is_empty() {
        ui.add_space(theme::SPACE_2);
        ui.label(
            RichText::new("Распознано")
                .size(theme::TEXT_CAPTION)
                .strong()
                .color(theme::TEXT_PRIMARY),
        );
        for line in &state.results {
            ui.label(
                RichText::new(line)
                    .monospace()
                    .size(theme::TEXT_MONO)
                    .color(theme::TEXT_PRIMARY),
            );
        }
    }
    if let Some(error) = &state.error {
        ui.add_space(theme::SPACE_1);
        widgets::badge(ui, error, widgets::BadgeKind::Error);
    }
    if state.running {
        ui.label(
            RichText::new("Говорите — распознанные фразы появятся здесь; тест остановится сам.")
                .size(theme::TEXT_CAPTION)
                .color(theme::TEXT_SECONDARY),
        );
    }
}

/// Первая (по алфавиту) подпапка каталога.
fn first_subdir(root: &std::path::Path) -> Option<std::path::PathBuf> {
    let mut dirs: Vec<std::path::PathBuf> = std::fs::read_dir(root)
        .ok()?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect();
    dirs.sort();
    dirs.into_iter().next()
}

/// `libvosk.dll` рядом с exe (альтернативное место).
fn exe_side_libvosk() -> Option<std::path::PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let path = exe.parent()?.join("libvosk.dll");
    path.is_file().then_some(path)
}

/// Выбор движка распознавания и параметры выбранного.
fn draw_voice_engine(
    ui: &mut egui::Ui,
    voice: &mut klavium_core::config::VoiceConfig,
    data_dir: &std::path::Path,
    model_download_start: &dyn Fn(String),
    model_download_state: &dyn Fn() -> DownloadStatus,
    vosk_install_start: &dyn Fn(),
    vosk_state: &dyn Fn() -> DownloadStatus,
) {
    ui.horizontal(|ui| {
        ui.label(
            RichText::new("Движок")
                .size(theme::TEXT_BODY)
                .color(theme::TEXT_PRIMARY),
        );
        widgets::segmented(
            ui,
            &[
                ("whisper", "Whisper"),
                ("google", "Google"),
                ("vosk", "Vosk"),
            ],
            &mut voice.engine,
        );
    });
    match voice.engine.as_str() {
        "google" => {
            ui.add_space(theme::SPACE_1);
            ui.label(
                RichText::new(
                    "Онлайн-распознавание Google: нужен интернет. Язык — из настроек выше («авто» — по раскладке).",
                )
                .size(theme::TEXT_CAPTION)
                .color(theme::TEXT_SECONDARY),
            );
        }
        "vosk" => draw_voice_vosk(ui, voice, data_dir, vosk_install_start, vosk_state),
        _ => draw_voice_model(
            ui,
            voice,
            data_dir,
            model_download_start,
            model_download_state,
        ),
    }
    ui.label(
        RichText::new("Смена движка применяется сразу после сохранения настроек.")
            .size(theme::TEXT_CAPTION)
            .color(theme::TEXT_SECONDARY),
    );
}

/// Блок «Модель» (Whisper): выбор точности, статус файла и загрузка.
fn draw_voice_model(
    ui: &mut egui::Ui,
    voice: &mut klavium_core::config::VoiceConfig,
    data_dir: &std::path::Path,
    download_start: &dyn Fn(String),
    download_state: &dyn Fn() -> DownloadStatus,
) {
    ui.horizontal(|ui| {
        ui.label("Модель:");
        egui::ComboBox::from_id_salt("voice_model")
            .selected_text(model_choice_label(&voice.model))
            .width(320.0)
            .show_ui(ui, |ui| {
                ui.selectable_value(
                    &mut voice.model,
                    String::new(),
                    "Авто — лучшая из загруженных",
                );
                for spec in MODELS {
                    ui.selectable_value(
                        &mut voice.model,
                        spec.file.to_string(),
                        format!("{} — {} ({})", spec.title, spec.size, spec.hint),
                    );
                }
            });
    });
    let models_dir = data_dir.join("models");
    let found = find_model(&models_dir, &voice.model);
    let file_for_download: Option<String> = if found.is_none() {
        let name = voice.model.trim();
        if name.is_empty() {
            MODELS.first().map(|spec| spec.file.to_string())
        } else {
            Some(name.to_string())
        }
    } else {
        None
    };
    if let Some(path) = &found {
        ui.colored_label(
            Color32::from_rgb(87, 200, 120),
            format!("✓ модель найдена: {}", path.display()),
        );
    } else {
        let detail = chosen_spec(&voice.model).map_or(String::new(), |spec| {
            format!(" ({} — {})", spec.size, spec.hint)
        });
        ui.colored_label(
            Color32::from_rgb(235, 87, 87),
            format!("✗ файл модели не найден{detail}"),
        );
    }
    let download = download_state();
    ui.horizontal(|ui| {
        if ui.button("Открыть папку моделей").clicked() {
            let _ = std::fs::create_dir_all(&models_dir);
            let _ = std::process::Command::new("explorer")
                .arg(&models_dir)
                .spawn();
        }
        if let Some(file) = &file_for_download {
            if ui
                .add_enabled(
                    !download.active,
                    egui::Button::new(format!("Скачать {file}")),
                )
                .clicked()
            {
                download_start(file.clone());
            }
        }
    });
    if download.active {
        let percent = download.percent.unwrap_or(0);
        ui.add(
            egui::ProgressBar::new(f32::from(percent) / 100.0)
                .desired_width(320.0)
                .text(format!("загрузка {} — {percent}%", download.file)),
        );
    } else if let Some(error) = download.error.clone() {
        ui.colored_label(
            Color32::from_rgb(235, 87, 87),
            format!("ошибка загрузки {}: {error}", download.file),
        );
    } else if download.done {
        ui.colored_label(
            Color32::from_rgb(87, 200, 120),
            format!("✓ загружено: {}", download.file),
        );
    }
    ui.label(
        RichText::new("Новая модель загрузится сразу после сохранения настроек.")
            .small()
            .weak(),
    );
}

/// Установить светлую тему и шрифты окна настроек.
///
/// Тема фиксированно светлая (тёмная — в будущих версиях), независимо от
/// темы Windows: палитра из [`crate::theme`].
fn install_theme(ctx: &egui::Context) {
    install_fonts(ctx);

    let mut style = (*ctx.style()).clone();
    style.visuals = egui::Visuals::light();

    let visuals = &mut style.visuals;
    visuals.panel_fill = theme::BG_WINDOW;
    visuals.window_fill = theme::BG_CARD;
    visuals.window_stroke = Stroke::new(theme::HAIRLINE, theme::BORDER_SUBTLE);
    visuals.window_corner_radius = egui::CornerRadius::same(theme::RADIUS_CARD);
    visuals.extreme_bg_color = theme::BG_CARD;
    visuals.faint_bg_color = theme::BG_WINDOW;
    visuals.override_text_color = Some(theme::TEXT_PRIMARY);
    visuals.selection.bg_fill = theme::ACCENT_TINT;
    visuals.selection.stroke = Stroke::new(theme::HAIRLINE, theme::TEXT_PRIMARY);
    visuals.hyperlink_color = theme::ACCENT;
    visuals.slider_trailing_fill = true;

    let widgets = &mut visuals.widgets;
    widgets.noninteractive.bg_fill = theme::BG_WINDOW;
    widgets.noninteractive.bg_stroke = Stroke::new(theme::HAIRLINE, theme::BORDER_SUBTLE);
    widgets.noninteractive.fg_stroke = Stroke::new(theme::HAIRLINE, theme::TEXT_PRIMARY);
    widgets.noninteractive.corner_radius = egui::CornerRadius::same(theme::RADIUS_CARD);

    widgets.inactive.bg_fill = theme::BG_CARD;
    widgets.inactive.bg_stroke = Stroke::new(theme::HAIRLINE, theme::BORDER_STRONG);
    widgets.inactive.fg_stroke = Stroke::new(theme::HAIRLINE, theme::TEXT_PRIMARY);
    widgets.inactive.corner_radius = egui::CornerRadius::same(theme::RADIUS_CONTROL);

    widgets.hovered.bg_fill = theme::BG_CARD_HOVER;
    widgets.hovered.bg_stroke = Stroke::new(theme::HAIRLINE, theme::ACCENT);
    widgets.hovered.fg_stroke = Stroke::new(theme::HAIRLINE, theme::TEXT_PRIMARY);
    widgets.hovered.corner_radius = egui::CornerRadius::same(theme::RADIUS_CONTROL);

    widgets.active.bg_fill = theme::BG_CARD;
    widgets.active.bg_stroke = Stroke::new(theme::HAIRLINE, theme::ACCENT);
    widgets.active.fg_stroke = Stroke::new(1.5_f32, theme::TEXT_PRIMARY);
    widgets.active.corner_radius = egui::CornerRadius::same(theme::RADIUS_CONTROL);

    widgets.open = widgets.inactive;

    style.spacing.item_spacing = egui::vec2(theme::SPACE_2, theme::SPACE_2);
    style.spacing.button_padding = egui::vec2(10.0, 5.0);
    style.spacing.interact_size.y = 26.0;

    style.text_styles = [
        (
            egui::TextStyle::Heading,
            egui::FontId::new(theme::TEXT_TITLE, egui::FontFamily::Proportional),
        ),
        (
            egui::TextStyle::Body,
            egui::FontId::new(theme::TEXT_BODY, egui::FontFamily::Proportional),
        ),
        (
            egui::TextStyle::Button,
            egui::FontId::new(theme::TEXT_BODY, egui::FontFamily::Proportional),
        ),
        (
            egui::TextStyle::Small,
            egui::FontId::new(theme::TEXT_CAPTION, egui::FontFamily::Proportional),
        ),
        (
            egui::TextStyle::Monospace,
            egui::FontId::new(theme::TEXT_MONO, egui::FontFamily::Monospace),
        ),
    ]
    .into();

    ctx.set_style(style);
}

/// Подключить системный шрифт с цепочкой фолбэков.
///
/// Порядок: Segoe UI Variable → Segoe UI → встроенные шрифты egui. Ошибки
/// не выводятся: при отсутствии файлов окно просто остаётся на встроенном
/// шрифте (на этой машине, например, `SegUIVar.ttf` отсутствует).
fn install_fonts(ctx: &egui::Context) {
    const CANDIDATES: [&str; 2] = [
        "C:\\Windows\\Fonts\\SegUIVar.ttf",
        "C:\\Windows\\Fonts\\segoeui.ttf",
    ];
    let Some(bytes) = CANDIDATES.iter().find_map(|path| std::fs::read(path).ok()) else {
        return;
    };

    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "segoe_ui".to_string(),
        Arc::new(egui::FontData::from_owned(bytes)),
    );
    if let Some(family) = fonts.families.get_mut(&egui::FontFamily::Proportional) {
        family.insert(0, "segoe_ui".to_string());
    }
    if let Some(family) = fonts.families.get_mut(&egui::FontFamily::Monospace) {
        family.push("segoe_ui".to_string());
    }
    ctx.set_fonts(fonts);
}
