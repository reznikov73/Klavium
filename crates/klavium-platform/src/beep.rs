//! Системные звуковые сигналы (Console Beep).
//!
//! Один сигнал — [`beep`]; последовательность по настройкам звука (число
//! сигналов, пауза между ними, частота, длительность) — [`play_spec`].

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

/// Проиграть звук по его настройкам: `count` сигналов с паузой `pause_ms`.
///
/// Тумблер (`enabled`) здесь не проверяется: кнопка «Прослушать» должна
/// играть и выключенный звук. Рабочее проигрывание проверяет `enabled`
/// на уровне звукового сервиса.
///
/// # Errors
///
/// Ошибка Win32 `Beep`.
pub fn play_spec(spec: &SoundSpec) -> anyhow::Result<()> {
    let count = spec.count.clamp(1, 10);
    for index in 0..count {
        if index > 0 {
            thread::sleep(Duration::from_millis(u64::from(spec.pause_ms)));
        }
        beep(spec.frequency, spec.duration_ms)?;
    }
    Ok(())
}
