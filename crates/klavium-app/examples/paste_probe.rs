//! Проверка вставки через буфер обмена: печатает результат в stdout.
//!
//! Запуск (в активном окне Блокнота):
//! `cargo run --release -p klavium-app --example paste_probe -- "текст"`

fn main() {
    let text = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "проверка вставки через буфер".to_string());
    match klavium_platform::inject::paste_text(&text) {
        Ok(()) => println!("вставлено: {text:?}"),
        Err(error) => println!("ошибка: {error}"),
    }
    // Дать приложению обработать Ctrl+V.
    std::thread::sleep(std::time::Duration::from_millis(300));
}
