//! Звуковой сервис: сигналы через системный Beep в выделенном потоке.
//!
//! Рисунок каждого звука (число сигналов, пауза, частота, длительность)
//! и тумблер «включён» берутся из конфигурации на лету. Отдельный вид
//! запроса — проигрывание произвольных настроек (кнопка «Прослушать»).

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

/// Запрос на проигрывание: рабочий звук по виду или произвольные настройки.
enum PlayRequest {
    /// Звук события приложения (параметры — из конфигурации).
    Kind(SoundKind),
    /// Произвольные параметры (кнопка «Прослушать» в настройках).
    Spec(SoundSpec),
}

/// Сервис воспроизведения (неблокирующий: сигналы играет отдельный поток).
pub struct SoundService {
    tx: Sender<PlayRequest>,
}

impl SoundService {
    /// Запустить сервис с общим доступом к конфигурации.
    #[must_use]
    pub fn spawn(config: SharedConfig) -> Arc<Self> {
        let (tx, rx) = mpsc::channel::<PlayRequest>();
        let _ = thread::Builder::new()
            .name("sound".to_string())
            .spawn(move || {
                while let Ok(request) = rx.recv() {
                    match request {
                        PlayRequest::Kind(kind) => play_kind(&config, kind),
                        // «Прослушать»: играет и выключенный звук, чтобы можно
                        // было послушать его перед включением тумблера.
                        PlayRequest::Spec(spec) => {
                            let _ = beep::play_spec(&spec);
                        }
                    }
                }
            });
        Arc::new(Self { tx })
    }

    /// Проиграть сигнал события (неблокирующе).
    pub fn play(&self, kind: SoundKind) {
        let _ = self.tx.send(PlayRequest::Kind(kind));
    }

    /// Проиграть звук с заданными настройками (неблокирующе).
    pub fn play_spec(&self, spec: SoundSpec) {
        let _ = self.tx.send(PlayRequest::Spec(spec));
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
    if spec.enabled {
        let _ = beep::play_spec(&spec);
    }
}
