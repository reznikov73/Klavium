//! Временный пробник: сколько «мусора» эвристика считает осмысленным.
//!
//! Берём английские слова, перекодируем в русскую раскладку (это ровно то,
//! как выглядит ошибка раскладки) и считаем долю тех, что признаны
//! осмысленными по-русски. Чем меньше — тем точнее детекция. Метрика нужна,
//! чтобы правки словарных эвристик не проходили «на глаз»: любое правило,
//! которое начинает считать мусор словом, здесь видно сразу.
//!
//! Запуск: `cargo run --release -p klavium-core --example stem_probe`.

// Доли считаются через `as f64`: выборки на порядки короче предела мантиссы.
#![allow(clippy::cast_precision_loss)]

use std::time::Instant;

use klavium_core::convert::convert_text;
use klavium_core::detect::{is_meaningful_english, is_meaningful_russian};

fn main() {
    let text = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/data/english_words.txt"
    ))
    .expect("нужен файл data/english_words.txt");
    let words: Vec<&str> = text.lines().filter(|w| w.len() >= 4).collect();

    // Каждое 17-е слово — равномерная выборка по алфавиту.
    let sample: Vec<&str> = words.iter().step_by(17).copied().collect();

    let mut false_positive = 0_usize;
    let mut examples: Vec<(&str, String)> = Vec::new();
    let started = Instant::now();
    for word in &sample {
        let converted = convert_text(word);
        if is_meaningful_russian(&converted) {
            false_positive += 1;
            if examples.len() < 10 {
                examples.push((word, converted));
            }
        }
    }
    let elapsed = started.elapsed();

    println!("выборка EN-слов: {}", sample.len());
    println!(
        "ложно «осмысленных» по-русски: {} ({:.3}%)",
        false_positive,
        100.0 * false_positive as f64 / sample.len() as f64
    );
    println!(
        "время: {:.1} мс ({:.2} мкс на слово)",
        elapsed.as_secs_f64() * 1000.0,
        elapsed.as_secs_f64() * 1e6 / sample.len() as f64
    );
    for (word, converted) in &examples {
        println!("  {word} → {converted}");
    }

    // Контроль обратного направления: русские слова в EN-раскладке.
    let russian = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/data/russian_words.txt"
    ))
    .expect("нужен файл data/russian_words.txt");
    let ru_words: Vec<&str> = russian.lines().filter(|w| w.chars().count() >= 4).collect();
    let ru_sample: Vec<&str> = ru_words.iter().step_by(23).copied().collect();
    let mut ru_false_positive = 0_usize;
    for word in &ru_sample {
        let converted = klavium_core::convert::convert_text_reverse(word);
        if is_meaningful_english(&converted) {
            ru_false_positive += 1;
        }
    }
    println!(
        "обратно (RU→EN): {}/{} ({:.3}%)",
        ru_false_positive,
        ru_sample.len(),
        100.0 * ru_false_positive as f64 / ru_sample.len() as f64
    );

    // Типовой мусор от промаха по соседним клавишам.
    let junk = [
        "фыва",
        "фывап",
        "пролдж",
        "йцукен",
        "ячсмить",
        "олджэ",
        "жээ",
        "ааа",
        "ююю",
        "ушка",
        "еирф",
        "вкен",
        "пщщ",
        "ршд",
        "щшг",
    ];
    let meaningful_junk: Vec<&str> = junk
        .iter()
        .copied()
        .filter(|w| is_meaningful_russian(w))
        .collect();
    println!("мусор, признанный осмысленным: {meaningful_junk:?}");
}
