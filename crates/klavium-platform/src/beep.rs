//! Системные звуковые сигналы (Console Beep).
//!
//! Паттерны: EN — один сигнал; RU — два (пауза 30 мс);
//! SWITCH — три (паузы 20 мс). Параметры — из конфигурации.

use std::thread;
use std::time::Duration;

use klavium_core::config::SoundSpec;
use windows_sys::Win32::System::Diagnostics::Debug::Beep;

/// Проиграть один сигнал (блокирует поток на длительность сигнала).
///
/// # Errors
///
/// Ошибка Win32 `Beep`.
pub fn beep(frequency: u32, duration_ms: u32) -> anyhow::Result<()> {
    let ok = unsafe { Beep(frequency, duration_ms) };
    if ok == 0 {
        anyhow::bail!("Beep: ошибка {}", unsafe {
            windows_sys::Win32::Foundation::GetLastError()
        });
    }
    Ok(())
}

/// Звук «английское слово»: один сигнал.
///
/// # Errors
///
/// Ошибка Win32 `Beep`.
pub fn play_en(spec: &SoundSpec) -> anyhow::Result<()> {
    beep(spec.frequency, spec.duration_ms)
}

/// Звук «русское слово»: два сигнала с паузой 30 мс.
///
/// # Errors
///
/// Ошибка Win32 `Beep`.
pub fn play_ru(spec: &SoundSpec) -> anyhow::Result<()> {
    beep(spec.frequency, spec.duration_ms)?;
    thread::sleep(Duration::from_millis(30));
    beep(spec.frequency, spec.duration_ms)
}

/// Звук «смена раскладки»: три сигнала с паузами 20 мс.
///
/// # Errors
///
/// Ошибка Win32 `Beep`.
pub fn play_switch(spec: &SoundSpec) -> anyhow::Result<()> {
    beep(spec.frequency, spec.duration_ms)?;
    thread::sleep(Duration::from_millis(20));
    beep(spec.frequency, spec.duration_ms)?;
    thread::sleep(Duration::from_millis(20));
    beep(spec.frequency, spec.duration_ms)
}
