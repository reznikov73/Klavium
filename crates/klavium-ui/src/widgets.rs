//! Библиотека компонентов окна настроек.
//!
//! Все элементы рисуются из токенов [`crate::theme`]: карточки, строки
//! настроек, свитчи, кейкапы хоткеев, бейджи, прогресс, сегмент-контролы и
//! карточки метрик. Секции не должны собирать эти элементы вручную.

use egui::{Color32, Response, RichText, Sense, Stroke, Ui};

use crate::theme;

/// Вид статусного бейджа.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum BadgeKind {
    /// Информация (синий).
    Info,
    /// Успех (зелёный).
    Ok,
    /// Предупреждение (оранжевый).
    Warn,
    /// Ошибка (красный).
    Error,
}

impl BadgeKind {
    /// Цвет фона пилюли.
    const fn background(self) -> Color32 {
        match self {
            Self::Info => theme::ACCENT_TINT,
            Self::Ok => Color32::from_rgb(0xE7, 0xF6, 0xEC),
            Self::Warn => Color32::from_rgb(0xFD, 0xF3, 0xE3),
            Self::Error => theme::DANGER_TINT,
        }
    }

    /// Цвет текста пилюли.
    const fn foreground(self) -> Color32 {
        match self {
            Self::Info => theme::ACCENT,
            Self::Ok => theme::SUCCESS,
            Self::Warn => theme::WARNING,
            Self::Error => theme::DANGER,
        }
    }
}

/// Карточка: белая панель с тонкой обводкой и скруглением.
pub fn card<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    egui::Frame::NONE
        .fill(theme::BG_CARD)
        .stroke(Stroke::new(theme::HAIRLINE, theme::BORDER_SUBTLE))
        .corner_radius(egui::CornerRadius::same(theme::RADIUS_CARD))
        .inner_margin(egui::Margin::same(theme::CARD_PADDING))
        .show(ui, add)
        .inner
}

/// Карточка с заголовком и необязательным пояснением.
pub fn card_titled(ui: &mut Ui, title: &str, caption: Option<&str>, add: impl FnOnce(&mut Ui)) {
    card(ui, |ui| {
        ui.label(
            RichText::new(title)
                .size(theme::TEXT_CARD_TITLE)
                .strong()
                .color(theme::TEXT_PRIMARY),
        );
        if let Some(caption) = caption {
            ui.add_space(theme::SPACE_1);
            ui.label(
                RichText::new(caption)
                    .size(theme::TEXT_CAPTION)
                    .color(theme::TEXT_SECONDARY),
            );
        }
        ui.add_space(theme::SPACE_3);
        add(ui);
    });
}

/// Заголовок страницы раздела: крупный заголовок и пояснение.
pub fn page_header(ui: &mut Ui, title: &str, caption: &str) {
    ui.label(
        RichText::new(title)
            .size(theme::TEXT_TITLE)
            .strong()
            .color(theme::TEXT_PRIMARY),
    );
    ui.add_space(theme::SPACE_1);
    ui.label(
        RichText::new(caption)
            .size(theme::TEXT_CAPTION)
            .color(theme::TEXT_SECONDARY),
    );
}

/// Строка настройки: подпись слева, элемент управления справа.
///
/// `hint` выводится второй строкой под подписью. Возвращает `true`, если
/// элемент управления ответил изменением.
pub fn setting_row(
    ui: &mut Ui,
    label: &str,
    hint: Option<&str>,
    add: impl FnOnce(&mut Ui) -> Response,
) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.label(
                RichText::new(label)
                    .size(theme::TEXT_BODY)
                    .color(theme::TEXT_PRIMARY),
            );
            if let Some(hint) = hint {
                ui.label(
                    RichText::new(hint)
                        .size(theme::TEXT_CAPTION)
                        .color(theme::TEXT_SECONDARY),
                );
            }
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            changed = add(ui).changed();
        });
    });
    changed
}

/// Тонкий разделитель между строками карточки.
pub fn row_separator(ui: &mut Ui) {
    ui.add_space(theme::SPACE_2);
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 1.0), Sense::hover());
    ui.painter()
        .rect_filled(rect, egui::CornerRadius::ZERO, theme::BORDER_SUBTLE);
    ui.add_space(theme::SPACE_2);
}

/// Переключатель вкл/выкл (трек 40×22, белый ползунок).
pub fn switch(ui: &mut Ui, on: &mut bool) -> Response {
    let size = egui::vec2(40.0, 22.0);
    let (rect, mut response) = ui.allocate_exact_size(size, Sense::click());
    if response.clicked() {
        *on = !*on;
        response.mark_changed();
    }
    let enabled = ui.is_enabled();
    let progress = ui.ctx().animate_bool(response.id, *on);
    let radius = rect.height() / 2.0;

    let track = if !enabled {
        theme::TRACK
    } else if *on {
        theme::ACCENT
    } else if response.hovered() {
        theme::BORDER_STRONG
    } else {
        theme::SWITCH_OFF
    };
    ui.painter()
        .rect_filled(rect, egui::CornerRadius::same(11), track);

    let travel = (2.0_f32).mul_add(-radius, rect.width());
    let knob_x = travel.mul_add(progress, rect.left() + radius);
    let knob = if enabled {
        Color32::WHITE
    } else {
        Color32::from_rgb(0xF4, 0xF4, 0xF5)
    };
    ui.painter()
        .circle_filled(egui::pos2(knob_x, rect.center().y), radius - 2.0, knob);

    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Кейкап — «клавиша» с моноширинным текстом.
pub fn keycap(ui: &mut Ui, text: &str) -> Response {
    egui::Frame::NONE
        .fill(theme::BG_CARD)
        .stroke(Stroke::new(theme::HAIRLINE, theme::BORDER_STRONG))
        .corner_radius(egui::CornerRadius::same(theme::RADIUS_KEYCAP))
        .inner_margin(egui::Margin::symmetric(6, 4))
        .show(ui, |ui| {
            ui.label(
                RichText::new(text)
                    .monospace()
                    .size(theme::TEXT_MONO)
                    .color(theme::TEXT_PRIMARY),
            );
        })
        .response
}

/// Статусный бейдж-пилюля.
pub fn badge(ui: &mut Ui, text: &str, kind: BadgeKind) {
    egui::Frame::NONE
        .fill(kind.background())
        .corner_radius(egui::CornerRadius::same(11))
        .inner_margin(egui::Margin::symmetric(10, 3))
        .show(ui, |ui| {
            ui.label(
                RichText::new(text)
                    .size(theme::TEXT_CAPTION)
                    .strong()
                    .color(kind.foreground()),
            );
        });
}

/// Полоса прогресса (высота 6, скругление 3).
pub fn progress(ui: &mut Ui, fraction: f32) {
    let fraction = fraction.clamp(0.0, 1.0);
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 6.0), Sense::hover());
    ui.painter()
        .rect_filled(rect, egui::CornerRadius::same(3), theme::TRACK);
    if fraction > 0.0 {
        let filled =
            egui::Rect::from_min_size(rect.min, egui::vec2(rect.width() * fraction, rect.height()));
        ui.painter()
            .rect_filled(filled, egui::CornerRadius::same(3), theme::ACCENT);
    }
}

/// Сегмент-контрол: набор взаимоисключающих вариантов.
///
/// `options` — пары (значение, подпись); выбранное значение пишется в
/// `current`. Возвращает `true`, если выбор изменился.
pub fn segmented(ui: &mut Ui, options: &[(&str, &str)], current: &mut String) -> bool {
    let mut changed = false;
    egui::Frame::NONE
        .fill(theme::TRACK)
        .corner_radius(egui::CornerRadius::same(theme::RADIUS_CONTROL))
        .inner_margin(egui::Margin::same(3))
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            ui.horizontal(|ui| {
                for (value, title) in options {
                    let selected = current == value;
                    let text = RichText::new(*title)
                        .size(theme::TEXT_CAPTION)
                        .color(if selected {
                            theme::TEXT_PRIMARY
                        } else {
                            theme::TEXT_SECONDARY
                        });
                    let button = egui::Button::new(text)
                        .fill(if selected {
                            theme::BG_CARD
                        } else {
                            Color32::TRANSPARENT
                        })
                        .stroke(if selected {
                            Stroke::new(theme::HAIRLINE, theme::BORDER_SUBTLE)
                        } else {
                            Stroke::NONE
                        })
                        .corner_radius(egui::CornerRadius::same(theme::RADIUS_KEYCAP))
                        .min_size(egui::vec2(0.0, 22.0));
                    if ui.add(button).clicked() && !selected {
                        *current = (*value).to_string();
                        changed = true;
                    }
                }
            });
        });
    changed
}

/// Карточка метрики для раздела «Статистика».
pub fn metric_card(ui: &mut Ui, label: &str, value: &str, hint: Option<&str>) {
    card(ui, |ui| {
        ui.label(
            RichText::new(value)
                .size(theme::TEXT_METRIC)
                .strong()
                .color(theme::TEXT_PRIMARY),
        );
        ui.label(
            RichText::new(label)
                .size(theme::TEXT_CAPTION)
                .color(theme::TEXT_SECONDARY),
        );
        if let Some(hint) = hint {
            ui.label(
                RichText::new(hint)
                    .size(theme::TEXT_CAPTION)
                    .color(theme::TEXT_DISABLED),
            );
        }
    });
}
