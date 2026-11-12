//! Сборка компактных словарей (FST) на этапе компиляции.
//!
//! Берём `data/english_words.txt` и `data/russian_words.txt`, приводим слова
//! к нижнему регистру, для русского добавляем ё→е-нормализованные формы
//! и пишем два `fst::Set` в `OUT_DIR`. Дальше `dicts.rs` встраивает их через
//! `include_bytes!` — поиск идёт по байтам в бинарнике, без кучи под словарь.
//!
//! Так словарь занимает меньше места, чем текстовый файл (FST сжимает общие
//! префиксы и суффиксы), а `Vec<&str>` с указателями (8,5 МБ на 532 тыс. слов)
//! больше не нужен вовсе.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

fn main() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let data = manifest.join("data");
    let out = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR"));

    println!("cargo:rerun-if-changed=data/english_words.txt");
    println!("cargo:rerun-if-changed=data/russian_words.txt");

    let english = read_words(&data.join("english_words.txt"), false);
    let russian = read_words(&data.join("russian_words.txt"), true);

    write_fst(&english, &out.join("english_words.fst"));
    write_fst(&russian, &out.join("russian_words.fst"));

    // Счётчики слов — чтобы `english_word_count()` не потерял смысл.
    let counts = format!(
        "/// Количество слов в английском словаре (сгенерировано build.rs).\n\
         pub const ENGLISH_WORD_COUNT: usize = {};\n\
         /// Количество слов в русском словаре (сгенерировано build.rs).\n\
         pub const RUSSIAN_WORD_COUNT: usize = {};\n",
        with_separators(english.len()),
        with_separators(russian.len())
    );
    std::fs::write(out.join("dict_counts.rs"), counts).expect("запись dict_counts.rs");
}

/// Число с разделителями разрядов (`370_105`), чтобы clippy не ругался на литерал.
fn with_separators(value: usize) -> String {
    let digits = value.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, ch) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            out.push('_');
        }
        out.push(ch);
    }
    out
}

/// Прочитать словарь: нижний регистр, без пустых строк, сортировка и дедупликация.
///
/// Для русского добавляются формы с «е» вместо «ё» («ёжик» == «ежик»).
fn read_words(path: &Path, add_yo_normalized: bool) -> Vec<String> {
    let text = std::fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("не читается {}: {error}", path.display()));
    let mut words: BTreeSet<String> = BTreeSet::new();
    for line in text.lines() {
        let word = line.trim().to_lowercase();
        if word.is_empty() {
            continue;
        }
        if add_yo_normalized && word.contains('ё') {
            words.insert(word.replace('ё', "е"));
        }
        words.insert(word);
    }
    words.into_iter().collect()
}

/// Записать словарь в FST (ключи — слова в UTF-8).
fn write_fst(words: &[String], path: &Path) {
    let mut builder = fst::SetBuilder::memory();
    for word in words {
        builder
            .insert(word)
            .unwrap_or_else(|error| panic!("FST: не вставить {word:?}: {error}"));
    }
    let bytes = builder
        .into_inner()
        .unwrap_or_else(|error| panic!("FST: не собрать {}: {error}", path.display()));
    std::fs::write(path, bytes)
        .unwrap_or_else(|error| panic!("FST: не записать {}: {error}", path.display()));
}
