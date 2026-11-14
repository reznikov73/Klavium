//! Встроенная справка: руководство пользователя в окне по F1.
//!
//! Текст живёт в `docs/USER-GUIDE.md` и вшивается в бинарник на сборке
//! (`include_str!`), поэтому справка открывается и в установленной копии,
//! где рядом с программой нет исходников и документации.
//!
//! Разметка — минимальная: заголовки `#`/`##`/`###`, пункты `-`, абзацы.
//! Таблицы и выделение внутри строк не поддерживаются: для справки хватает
//! структуры, а лишний разбор — лишние ошибки.

use egui::RichText;

use crate::theme;

/// Руководство пользователя, вшитое в бинарник.
pub const GUIDE: &str = include_str!("../../../docs/USER-GUIDE.md");

/// Блок разметки справки.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Block<'a> {
    /// Заголовок первого уровня (название руководства).
    Title(&'a str),
    /// Заголовок раздела.
    Heading(&'a str),
    /// Подзаголовок внутри раздела.
    Subheading(&'a str),
    /// Пункт списка.
    Bullet(&'a str),
    /// Абзац обычного текста.
    Paragraph(&'a str),
    /// Пустая строка — отступ между блоками.
    Gap,
}

/// Разобрать руководство на блоки (без учёта отступов внутри строк).
#[must_use]
pub fn parse(text: &str) -> Vec<Block<'_>> {
    text.lines()
        .map(|line| {
            let line = line.trim_end();
            if line.trim().is_empty() {
                return Block::Gap;
            }
            if let Some(rest) = line.strip_prefix("### ") {
                return Block::Subheading(rest.trim());
            }
            if let Some(rest) = line.strip_prefix("## ") {
                return Block::Heading(rest.trim());
            }
            if let Some(rest) = line.strip_prefix("# ") {
                return Block::Title(rest.trim());
            }
            if let Some(rest) = line.strip_prefix("- ") {
                return Block::Bullet(rest.trim());
            }
            Block::Paragraph(line.trim())
        })
        .collect()
}

/// Ширина текста справки: окно может быть шире, но строки не растягиваются.
const TEXT_WIDTH: f32 = 560.0;

/// Нарисовать окно справки; `open` — состояние окна (F1 переключает).
///
/// Закрытие крестиком тоже сбрасывает `open`, чтобы следующее F1 снова
/// открывало справку, а не мигало закрытым окном.
pub fn draw(ctx: &egui::Context, open: &mut bool) {
    if !*open {
        return;
    }
    let mut visible = true;
    egui::Window::new("Справка — Klavium")
        .open(&mut visible)
        .resizable(true)
        .default_size([620.0, 560.0])
        .min_width(420.0)
        .min_height(300.0)
        .show(ctx, |ui| {
            // Фиксируем ширину текста: иначе окно растягивается по самой
            // длинной строке и переносы не работают.
            ui.set_max_width(TEXT_WIDTH);
            ui.label(
                RichText::new("F1 — открыть и закрыть справку")
                    .size(theme::TEXT_CAPTION)
                    .color(theme::TEXT_SECONDARY),
            );
            ui.add_space(theme::SPACE_2);
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.set_max_width(TEXT_WIDTH);
                    for block in parse(GUIDE) {
                        draw_block(ui, &block);
                    }
                });
        });
    if !visible {
        *open = false;
    }
}

/// Строка текста с переносом по ширине окна.
fn text_label(ui: &mut egui::Ui, text: &str, size: f32, color: egui::Color32, strong: bool) {
    let mut rich = RichText::new(text).size(size).color(color);
    if strong {
        rich = rich.strong();
    }
    ui.add(egui::Label::new(rich).wrap());
}

/// Отрисовать один блок разметки.
fn draw_block(ui: &mut egui::Ui, block: &Block<'_>) {
    match block {
        Block::Title(text) => {
            ui.add_space(theme::SPACE_2);
            text_label(ui, text, theme::TEXT_TITLE + 3.0, theme::TEXT_PRIMARY, true);
            ui.add_space(theme::SPACE_2);
        }
        Block::Heading(text) => {
            ui.add_space(theme::SPACE_4);
            text_label(ui, text, theme::TEXT_TITLE, theme::TEXT_PRIMARY, true);
            ui.add_space(theme::SPACE_1);
        }
        Block::Subheading(text) => {
            ui.add_space(theme::SPACE_3);
            text_label(ui, text, theme::TEXT_CARD_TITLE, theme::TEXT_PRIMARY, true);
        }
        Block::Bullet(text) => {
            // Точка — часть строки: так перенос второй строки не уезжает под неё.
            let line = format!("•  {text}");
            text_label(ui, &line, theme::TEXT_BODY, theme::TEXT_PRIMARY, false);
            ui.add_space(theme::SPACE_1);
        }
        Block::Paragraph(text) => {
            text_label(ui, text, theme::TEXT_BODY, theme::TEXT_PRIMARY, false);
        }
        Block::Gap => {
            ui.add_space(theme::SPACE_1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_blocks() {
        let text = "# Заголовок\n\n## Раздел\n- пункт\n### Подраздел\nАбзац текста\n";
        assert_eq!(
            parse(text),
            vec![
                Block::Title("Заголовок"),
                Block::Gap,
                Block::Heading("Раздел"),
                Block::Bullet("пункт"),
                Block::Subheading("Подраздел"),
                Block::Paragraph("Абзац текста"),
            ]
        );
    }

    #[test]
    fn test_parse_bullet_keeps_inner_dashes() {
        assert_eq!(
            parse("- Ctrl+Alt+Z — отменить\n-- не пункт"),
            vec![
                Block::Bullet("Ctrl+Alt+Z — отменить"),
                Block::Paragraph("-- не пункт"),
            ]
        );
    }

    #[test]
    fn test_guide_starts_with_title_and_covers_sections() {
        let blocks = parse(GUIDE);
        assert!(matches!(blocks.first(), Some(Block::Title(_))));
        // Каждый раздел настроек должен быть описан в справке.
        for heading in [
            "Как это работает",
            "Коррекция раскладки",
            "Горячие клавиши",
            "Исправление выделенного",
            "Транслитерация",
            "Отмена исправления",
            "Словарь пользователя",
            "Правила для приложений",
            "Пауза",
            "Голосовой ввод",
            "Звуки",
            "Таймауты",
            "Уведомления",
            "Статистика",
            "Общие",
            "Диагностика",
            "Где хранятся данные",
            "Если что-то не работает",
        ] {
            assert!(
                blocks
                    .iter()
                    .any(|block| matches!(block, Block::Heading(text) if *text == heading)),
                "в справке нет раздела «{heading}»"
            );
        }
    }

    #[test]
    fn test_guide_mentions_key_hotkeys() {
        for hotkey in ["F8", "Ctrl+Alt+Z", "Ctrl+Alt+X", "Ctrl+Alt+T", "F1"] {
            assert!(GUIDE.contains(hotkey), "в справке нет сочетания {hotkey}");
        }
    }
}
