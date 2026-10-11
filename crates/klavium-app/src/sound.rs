//! Звуковой сервис: сигналы через системный Beep в выделенном потоке.
//!
//! Паттерны: EN — один сигнал; RU — два (пауза 30 мс);
//! SWITCH — три (паузы 20 мс). Параметры берутся из конфигурации на лету.

use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, PoisonError};
use std::thread;

use klavium_core::config::SoundSpec;
use klavium_platform::beep;

use crate::runtime::SharedConfig;

/// Тип звукового сигнала.
#[derive(Debug, Clone, Copy)]
pub enum SoundKind {
    /// Английское слово.
    En,
    /// Русское слово.
    Ru,
    /// Смена раскладки.
    Switch,
    /// Старт голосовой диктовки.
    VoiceStart,
    /// Остановка голосовой диктовки.
    VoiceStop,
}

/// Сервис воспроизведения (неблокирующий: сигналы играет отдельный поток).
pub struct SoundService {
    tx: Sender<SoundKind>,
}

impl SoundService {
    /// Запустить сервис с общим доступом к конфигурации.
    #[must_use]
    pub fn spawn(config: SharedConfig) -> Arc<Self> {
        let (tx, rx) = mpsc::channel::<SoundKind>();
        let _ = thread::Builder::new()
            .name("sound".to_string())
            .spawn(move || {
                while let Ok(kind) = rx.recv() {
                    play_kind(&config, kind);
                }
            });
        Arc::new(Self { tx })
    }

    /// Проиграть сигнал (неблокирующе).
    pub fn play(&self, kind: SoundKind) {
        let _ = self.tx.send(kind);
    }
}

fn play_kind(config: &SharedConfig, kind: SoundKind) {
    let spec: SoundSpec = {
        let guard = config.read().unwrap_or_else(PoisonError::into_inner);
        match kind {
            SoundKind::En => guard.sounds.en.clone(),
            SoundKind::Ru => guard.sounds.ru.clone(),
            SoundKind::Switch => guard.sounds.switch.clone(),
            SoundKind::VoiceStart => guard.sounds.voice_start.clone(),
            SoundKind::VoiceStop => guard.sounds.voice_stop.clone(),
        }
    };

    let _ = match kind {
        SoundKind::En => beep::play_en(&spec),
        SoundKind::Ru => beep::play_ru(&spec),
        SoundKind::Switch => beep::play_switch(&spec),
        // Голосовые сигналы: одиночные (старт/стоп — из конфига).
        SoundKind::VoiceStart | SoundKind::VoiceStop => {
            beep::beep(spec.frequency, spec.duration_ms)
        }
    };
}
