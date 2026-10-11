//! Окно настроек на egui/eframe.
//!
//! Открывается из трея; закрытие окна лишь скрывает его (приложение остаётся
//! в трее). Изменения применяются кнопкой «Сохранить» через колбэк приложения
//! (запись конфига, обновление движка, автозапуск).

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use egui::{Color32, RichText};
use klavium_core::config::AppConfig;
use klavium_core::stats::StatsData;
use klavium_core::voice::{DownloadStatus, RecognitionTest, MODELS};

/// Колбэк сохранения: приложение пишет файл и применяет изменения на лету.
pub type SaveHandler = Box<dyn Fn(AppConfig) -> Result<(), String> + 'static>;

/// Контекст запуска окна настроек.
pub struct SettingsContext {
    /// Стартовая копия конфигурации для редактирования.
    pub config: AppConfig,
    /// Колбэк сохранения.
    pub on_save: SaveHandler,
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

/// Запустить цикл приложения настроек (блокирует вызывающий поток до выхода).
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
    Sounds,
    Timeouts,
    Notifications,
    Statistics,
    General,
    Voice,
}

const SECTIONS: [(Section, &str); 7] = [
    (Section::Correction, "Коррекция"),
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
        Ok("timeouts") => Section::Timeouts,
        Ok("notifications") => Section::Notifications,
        Ok("statistics") => Section::Statistics,
        Ok("general") => Section::General,
        Ok("voice") => Section::Voice,
        _ => Section::Correction,
    }
}

struct SettingsApp {
    config: AppConfig,
    original: AppConfig,
    section: Section,
    visible: bool,
    quitting: bool,
    status: Option<(String, bool)>,
    on_save: SaveHandler,
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
    voice_test_start: Box<dyn Fn() + 'static>,
    voice_test_stop: Box<dyn Fn() + 'static>,
    voice_test_state: Box<dyn Fn() -> RecognitionTest + 'static>,
    smoke_seconds: Option<u64>,
    started: Instant,
}

impl SettingsApp {
    fn new(cc: &eframe::CreationContext<'_>, context: SettingsContext) -> Self {
        install_theme(&cc.egui_ctx);
        Self {
            original: context.config.clone(),
            config: context.config,
            section: initial_section(),
            visible: context.open_on_start,
            quitting: false,
            status: None,
            on_save: context.on_save,
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
            voice_test_start: context.voice_test_start,
            voice_test_stop: context.voice_test_stop,
            voice_test_state: context.voice_test_state,
            smoke_seconds: context.smoke_seconds,
            started: Instant::now(),
        }
    }

    fn show_window(&mut self, ctx: &egui::Context) {
        self.visible = true;
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        ctx.request_repaint();
    }

    fn hide_window(&mut self, ctx: &egui::Context) {
        self.visible = false;
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
    }

    fn save(&mut self) {
        match (self.on_save)(self.config.clone()) {
            Ok(()) => {
                self.original = self.config.clone();
                self.status = Some(("Сохранено".to_string(), false));
            }
            Err(error) => {
                self.status = Some((format!("Ошибка: {error}"), true));
            }
        }
    }

    fn draw(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("sections")
            .resizable(false)
            .default_width(200.0)
            .show(ctx, |ui| {
                ui.add_space(12.0);
                ui.heading("Klavium");
                ui.label(RichText::new("исправление раскладки").small().weak());
                ui.add_space(12.0);

                for (section, title) in SECTIONS {
                    if ui
                        .selectable_label(self.section == section, title)
                        .clicked()
                    {
                        self.section = section;
                    }
                }

                ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
                    ui.add_space(6.0);
                    if ui.button("Открыть папку данных").clicked() {
                        let _ = std::process::Command::new("explorer")
                            .arg(&self.data_dir)
                            .spawn();
                    }
                });
            });

        egui::TopBottomPanel::bottom("actions").show(ctx, |ui| {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if ui.button("Сохранить").clicked() {
                    self.save();
                }
                if ui.button("Сбросить").clicked() {
                    self.config = self.original.clone();
                    self.status = Some(("Изменения сброшены".to_string(), false));
                }
                ui.add_space(12.0);
                if let Some((text, is_error)) = &self.status {
                    let color = if *is_error {
                        Color32::from_rgb(235, 87, 87)
                    } else {
                        Color32::from_rgb(87, 200, 120)
                    };
                    ui.colored_label(color, text);
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Скрыть в трей").clicked() {
                        self.hide_window(ctx);
                    }
                    ui.label(
                        RichText::new("Выход: трей → «Выход» или Ctrl+Shift+Q")
                            .small()
                            .weak(),
                    );
                });
            });
            ui.add_space(6.0);
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.add_space(4.0);
                match self.section {
                    Section::Correction => self.draw_correction(ui),
                    Section::Sounds => self.draw_sounds(ui),
                    Section::Timeouts => self.draw_timeouts(ui),
                    Section::Notifications => self.draw_notifications(ui),
                    Section::Statistics => self.draw_statistics(ui),
                    Section::General => self.draw_general(ui),
                    Section::Voice => self.draw_voice(ui),
                }
            });
        });
    }

    fn draw_correction(&mut self, ui: &mut egui::Ui) {
        ui.heading("Коррекция раскладки");
        ui.add_space(6.0);
        let correction = &mut self.config.correction;
        ui.checkbox(&mut correction.enabled, "Включить исправление раскладки");
        ui.checkbox(
            &mut correction.auto_correct,
            "Исправлять автоматически (без F8)",
        );
        ui.checkbox(
            &mut correction.show_notifications,
            "Показывать подсказку у каретки",
        );
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.label("Порог уверенности:");
            ui.add(
                egui::Slider::new(&mut correction.threshold, 0.0..=1.0)
                    .fixed_decimals(2)
                    .text(""),
            );
            ui.label(format!("{:.0}%", correction.threshold * 100.0));
        });
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.label("Хоткей исправления:");
            ui.text_edit_singleline(&mut correction.hotkey_correct);
        });
        ui.add_space(12.0);
        ui.label(
            RichText::new(
                "Подсказка появляется при уверенности выше порога; F8 применяет, Esc скрывает.",
            )
            .small()
            .weak(),
        );
    }

    fn draw_sounds(&mut self, ui: &mut egui::Ui) {
        ui.heading("Звуки");
        ui.add_space(4.0);
        ui.label(
            RichText::new("Частота (Гц) и длительность (мс). Проигрывание — в фазе сервисов.")
                .small()
                .weak(),
        );
        ui.add_space(8.0);
        let sounds = &mut self.config.sounds;
        let rows: [(&str, &mut klavium_core::config::SoundSpec); 5] = [
            ("Английское слово", &mut sounds.en),
            ("Русское слово", &mut sounds.ru),
            ("Смена раскладки", &mut sounds.switch),
            ("Старт голоса", &mut sounds.voice_start),
            ("Стоп голоса", &mut sounds.voice_stop),
        ];
        for (title, spec) in rows {
            ui.horizontal(|ui| {
                ui.label(format!("{title}:"));
                ui.add(
                    egui::DragValue::new(&mut spec.frequency)
                        .range(200..=4000)
                        .suffix(" Гц"),
                );
                ui.add(
                    egui::DragValue::new(&mut spec.duration_ms)
                        .range(10..=2000)
                        .suffix(" мс"),
                );
            });
        }
    }

    fn draw_timeouts(&mut self, ui: &mut egui::Ui) {
        ui.heading("Таймауты");
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.label("Пауза между словами (сек):");
            ui.add(
                egui::DragValue::new(&mut self.config.timeouts.word_start)
                    .range(0.5..=60.0)
                    .speed(0.1),
            );
        });
        ui.horizontal(|ui| {
            ui.label("Интервал проверки раскладки (сек):");
            ui.add(
                egui::DragValue::new(&mut self.config.timeouts.layout_check)
                    .range(0.1..=10.0)
                    .speed(0.1),
            );
        });
    }

    fn draw_notifications(&mut self, ui: &mut egui::Ui) {
        ui.heading("Уведомления");
        ui.add_space(8.0);
        let notifications = &mut self.config.notifications;
        ui.checkbox(&mut notifications.enabled, "Показывать уведомления");
        ui.horizontal(|ui| {
            ui.label("Длительность (мс):");
            ui.add(egui::DragValue::new(&mut notifications.duration_ms).range(500..=10000));
        });
    }

    fn draw_statistics(&mut self, ui: &mut egui::Ui) {
        ui.heading("Статистика");
        ui.add_space(8.0);
        let statistics = &mut self.config.statistics;
        ui.checkbox(&mut statistics.enabled, "Вести статистику");
        ui.checkbox(
            &mut statistics.reset_on_start,
            "Сбрасывать статистику при запуске",
        );
        ui.add_space(10.0);
        ui.separator();
        ui.add_space(8.0);

        let snapshot = (self.stats_snapshot)();
        ui.label(RichText::new("Слова по раскладкам (за всё время):").strong());
        ui.add_space(4.0);
        ui.label(format!("Английских: {}", snapshot.total_words.en));
        ui.label(format!("Русских: {}", snapshot.total_words.ru));
        ui.label(format!("Прочих: {}", snapshot.total_words.other));
        ui.label(format!("Голосом: {}", snapshot.total_words.voice));
        ui.add_space(6.0);
        ui.label(format!(
            "Переключений раскладки: {}",
            snapshot.layout_switches
        ));
        ui.label(format!("Сессий в истории: {}", snapshot.sessions.len()));
        ui.add_space(10.0);
        if ui.button("Сбросить статистику").clicked() {
            (self.stats_reset)();
        }
    }

    fn draw_general(&mut self, ui: &mut egui::Ui) {
        ui.heading("Общие");
        ui.add_space(8.0);
        ui.checkbox(
            &mut self.config.autostart.enabled,
            "Запускать вместе с Windows",
        );
        ui.add_space(12.0);
        ui.label(RichText::new("Логирование").strong());
        let logging = &mut self.config.logging;
        ui.checkbox(&mut logging.enabled, "Вести журнал");
        ui.horizontal(|ui| {
            ui.label("Уровень:");
            egui::ComboBox::from_id_salt("log_level")
                .selected_text(logging.level.clone())
                .show_ui(ui, |ui| {
                    for level in ["DEBUG", "INFO", "WARNING", "ERROR"] {
                        ui.selectable_value(&mut logging.level, level.to_string(), level);
                    }
                });
        });
        ui.add_space(12.0);
        ui.label(RichText::new("Каталог данных").strong());
        ui.label(
            RichText::new(
                "Здесь хранятся настройки, статистика, журналы и модели. По умолчанию — папка data рядом с программой.",
            )
            .small()
            .weak(),
        );
        ui.label(
            RichText::new(self.data_dir.display().to_string())
                .monospace()
                .small(),
        );
        if let Some(note) = &self.data_note {
            ui.colored_label(
                Color32::from_rgb(230, 180, 80),
                RichText::new(note.as_str()),
            );
        }
        ui.horizontal(|ui| {
            if ui.button("Выбрать папку…").clicked() {
                if let Some(dir) = rfd::FileDialog::new()
                    .set_directory(&self.data_dir)
                    .pick_folder()
                {
                    (self.data_dir_change)(Some(dir));
                }
            }
            if ui.button("По умолчанию (рядом с программой)").clicked() {
                (self.data_dir_change)(None);
            }
            if ui.button("Открыть").clicked() {
                let _ = std::process::Command::new("explorer")
                    .arg(&self.data_dir)
                    .spawn();
            }
        });
        if let Some(status) = (self.data_move_status)() {
            ui.colored_label(Color32::from_rgb(230, 180, 80), RichText::new(status));
        }
    }

    fn draw_voice(&mut self, ui: &mut egui::Ui) {
        ui.heading("Голосовой ввод");
        ui.add_space(4.0);
        ui.label(
            RichText::new("Диктовка: F9 — вкл/выкл; движок (Whisper/Google/Vosk) и тест — ниже.")
                .small()
                .weak(),
        );
        ui.add_space(8.0);
        let voice = &mut self.config.voice;
        ui.checkbox(&mut voice.enabled, "Включить голосовой ввод");
        ui.horizontal(|ui| {
            ui.label("Хоткей:");
            ui.text_edit_singleline(&mut voice.hotkey);
        });
        ui.horizontal(|ui| {
            ui.label("Язык:");
            egui::ComboBox::from_id_salt("voice_language")
                .selected_text(match voice.language.as_str() {
                    "ru" => "русский",
                    "en" => "английский",
                    _ => "авто (по раскладке)",
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(
                        &mut voice.language,
                        "auto".to_string(),
                        "авто (по раскладке)",
                    );
                    ui.selectable_value(&mut voice.language, "ru".to_string(), "русский");
                    ui.selectable_value(&mut voice.language, "en".to_string(), "английский");
                });
        });
        draw_voice_engine(
            ui,
            voice,
            &self.data_dir,
            &*self.model_download_start,
            &*self.model_download_state,
            &*self.vosk_install_start,
            &*self.vosk_state,
        );
        ui.add_space(8.0);
        draw_voice_test(
            ui,
            &*self.voice_test_start,
            &*self.voice_test_stop,
            &*self.voice_test_state,
        );
        ui.add_space(6.0);
        ui.checkbox(
            &mut voice.punct_dict,
            "Голосовая пунктуация («запятая» → «,»)",
        );
        ui.checkbox(
            &mut voice.auto_capitalize,
            "Автозаглавные буквы после точки",
        );
        self.draw_voice_status(ui);
    }

    /// Статус голосового сервиса и кнопка включения/выключения диктовки.
    fn draw_voice_status(&self, ui: &mut egui::Ui) {
        ui.add_space(12.0);
        ui.separator();
        ui.add_space(8.0);
        let (active, status) = (self.voice_snapshot)();
        ui.horizontal(|ui| {
            if active {
                ui.colored_label(Color32::from_rgb(87, 200, 120), "● диктовка активна");
            } else {
                ui.label(RichText::new("○ диктовка выключена").weak());
            }
            let button = if active {
                "Выключить (F9)"
            } else {
                "Включить (F9)"
            };
            if ui.button(button).clicked() {
                (self.voice_toggle)();
            }
        });
        ui.label(RichText::new(status).small().weak());
    }
}

impl eframe::App for SettingsApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // События меню трея (передаёт поток-наблюдатель из приложения)
        while let Some(id) = crate::tray::pop_queued_event(&self.tray_queue) {
            match id.as_str() {
                crate::tray::MENU_SETTINGS => self.show_window(ctx),
                crate::tray::MENU_STATS => {
                    self.section = Section::Statistics;
                    self.show_window(ctx);
                }
                crate::tray::MENU_VOICE => (self.voice_toggle)(),
                crate::tray::MENU_QUIT => {
                    self.quitting = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
                _ => {}
            }
        }

        // Закрытие окна пользователем — скрываем в трей, не выходим
        if ctx.input(|i| i.viewport().close_requested()) && !self.quitting {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.hide_window(ctx);
        }

        // Дымовой режим: авто-выход по таймеру
        if let Some(seconds) = self.smoke_seconds {
            if self.started.elapsed() >= Duration::from_secs(seconds) {
                self.quitting = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }

        // Всегда просыпаемся периодически: опрос меню трея, живая статистика
        // и дымовой таймер должны работать независимо от потока событий окна.
        ctx.request_repaint_after(Duration::from_millis(200));

        if self.visible {
            self.draw(ctx);
        }
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
    ui.separator();
    ui.label(RichText::new("Тест распознавания").strong());
    ui.label(
        RichText::new("Прочитайте вслух (фразы разделяйте короткими паузами):")
            .small()
            .weak(),
    );
    ui.label(RichText::new(VOICE_TEST_SAMPLE).italics());
    let state = test_state();
    ui.horizontal(|ui| {
        if state.running {
            if ui.button("⏹ Стоп").clicked() {
                test_stop();
            }
            ui.label(RichText::new(format!("осталось ~{} с", state.seconds_left)).small());
        } else if state.waiting_engine {
            if ui.button("⏹ Отмена").clicked() {
                test_stop();
            }
            ui.label(RichText::new("ждём загрузку движка…").small().weak());
        } else if ui.button("🎤 Начать тест (15 с)").clicked() {
            test_start();
        }
        if !state.results.is_empty() && !state.running {
            ui.label(
                RichText::new("(новый тест очистит прошлый результат)")
                    .small()
                    .weak(),
            );
        }
    });
    if state.waiting_engine {
        ui.label(
            RichText::new(
                "Движок ещё загружается — тест начнётся автоматически, как только он будет готов.",
            )
            .small()
            .weak(),
        );
    }
    if !state.results.is_empty() {
        ui.add_space(4.0);
        ui.label(RichText::new("Распознано:").strong());
        for line in &state.results {
            ui.label(RichText::new(line));
        }
    }
    if let Some(error) = &state.error {
        ui.colored_label(Color32::from_rgb(235, 87, 87), error);
    }
    if state.running {
        ui.label(
            RichText::new("Говорите — распознанные фразы появятся здесь; тест остановится сам.")
                .small()
                .weak(),
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
        ui.label("Движок распознавания:");
        egui::ComboBox::from_id_salt("voice_engine_kind")
            .selected_text(match voice.engine.as_str() {
                "google" => "Google (онлайн)",
                "vosk" => "Vosk (офлайн)",
                _ => "Whisper (офлайн)",
            })
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut voice.engine, "whisper".to_string(), "Whisper (офлайн)");
                ui.selectable_value(&mut voice.engine, "google".to_string(), "Google (онлайн)");
                ui.selectable_value(&mut voice.engine, "vosk".to_string(), "Vosk (офлайн)");
            });
    });
    match voice.engine.as_str() {
        "google" => {
            ui.label(
                RichText::new(
                    "Онлайн-распознавание Google: нужен интернет. Язык — из настроек выше («авто» — по раскладке).",
                )
                .small()
                .weak(),
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
            .small()
            .weak(),
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

fn install_theme(ctx: &egui::Context) {
    // Тема — системная (тёмная/светлая по настройкам Windows): eframe сам
    // применяет палитру по системе, а не наши переопределения.
    // Кириллический шрифт из системы (с фолбэком на встроенные)
    if let Ok(bytes) = std::fs::read("C:\\Windows\\Fonts\\segoeui.ttf") {
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
}
