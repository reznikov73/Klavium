//! Движки распознавания речи: Whisper (офлайн) / Google (онлайн) / Vosk (офлайн).
//!
//! Выбор — в настройках (`voice.engine`), применяется при старте приложения.
//! Vosk использует `libvosk.dll` (ищется в `data\vosk\` и рядом с exe) и папку
//! модели (`voice.vosk_model_path` или автопоиск в `data\models\vosk\`).

use std::path::{Path, PathBuf};

use klavium_core::config::AppConfig;
use klavium_platform::vosk::VoskEngine;

use super::whisper::WhisperRecognizer;

/// Публичный ключ веб-API Google (тот же, что использует `SpeechRecognition`).
const GOOGLE_KEY: &str = "AIzaSyBOti4mM-6x9WDnZIjIeyEU21OpBXqWBgw";

/// URL архива libvosk для Windows.
const VOSK_LIB_URL: &str =
    "https://github.com/alphacep/vosk-api/releases/download/v0.3.45/vosk-win64-0.3.45.zip";

/// Движок распознавания фразы (16 кГц mono).
pub trait Recognizer {
    /// Распознать фразу; ошибки — строкой (пишутся в журнал, работа продолжается).
    ///
    /// # Errors
    ///
    /// Ошибка сети/движка/разбора ответа.
    fn recognize(&mut self, samples: &[f32], language: &str) -> Result<String, String>;

    /// Человекочитаемое имя движка (для статуса в настройках).
    fn describe(&self) -> String;
}

/// Ошибка создания движка.
pub enum EngineError {
    /// Нет модели/библиотеки; строка — где искали.
    Missing(String),
    /// Прочая ошибка инициализации.
    Failed(String),
}

/// Создать движок по настройкам.
///
/// # Errors
///
/// [`EngineError`], если модель/библиотека не найдены или не загрузились.
pub fn create(config: &AppConfig, data_dir: &Path) -> Result<Box<dyn Recognizer>, EngineError> {
    match config.voice.engine.as_str() {
        "google" => Ok(Box::new(GoogleRecognizer::new())),
        "vosk" => VoskRecognizer::create(config, data_dir)
            .map(|engine| Box::new(engine) as Box<dyn Recognizer>),
        _ => {
            let models_dir = data_dir.join("models");
            let path = super::whisper::resolve_model_path(&models_dir, &config.voice.model)
                .ok_or_else(|| EngineError::Missing(models_dir.display().to_string()))?;
            WhisperRecognizer::create(&path)
                .map(|engine| Box::new(engine) as Box<dyn Recognizer>)
                .map_err(EngineError::Failed)
        }
    }
}

/// URL архива libvosk (для установщика в настройках).
#[must_use]
pub const fn vosk_lib_url() -> &'static str {
    VOSK_LIB_URL
}

// --- Google (онлайн) ---------------------------------------------------------

/// Распознавание через веб-API Google (нужен интернет).
pub struct GoogleRecognizer {
    agent: ureq::Agent,
}

impl Default for GoogleRecognizer {
    fn default() -> Self {
        Self::new()
    }
}

impl GoogleRecognizer {
    /// Создать движок (сетевой агент с таймаутами).
    #[must_use]
    pub fn new() -> Self {
        let agent = ureq::AgentBuilder::new()
            .timeout_connect(std::time::Duration::from_secs(20))
            .timeout_read(std::time::Duration::from_secs(60))
            .build();
        Self { agent }
    }

    fn api_language(language: &str) -> &'static str {
        if language == "ru" {
            "ru-RU"
        } else {
            "en-US"
        }
    }
}

impl Recognizer for GoogleRecognizer {
    fn recognize(&mut self, samples: &[f32], language: &str) -> Result<String, String> {
        if samples.len() < 1600 {
            return Ok(String::new()); // короче 100 мс — не отправляем
        }
        let flac = encode_flac(samples).map_err(|error| format!("FLAC: {error}"))?;
        let url = format!(
            "https://www.google.com/speech-api/v2/recognize?client=chromium&lang={}&key={GOOGLE_KEY}",
            Self::api_language(language)
        );
        let response = self
            .agent
            .post(&url)
            .set("Content-Type", "audio/x-flac; rate=16000")
            .send_bytes(&flac)
            .map_err(|error| format!("сеть: {error}"))?;
        let body = response
            .into_string()
            .map_err(|error| format!("чтение ответа: {error}"))?;

        // Ответ — несколько JSON-строк; собираем все распознанные куски.
        let mut text = String::new();
        for line in body.lines() {
            let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
                continue;
            };
            let Some(results) = value.get("result").and_then(|value| value.as_array()) else {
                continue;
            };
            for result in results {
                let Some(transcript) = result
                    .get("alternative")
                    .and_then(|value| value.as_array())
                    .and_then(|alternatives| alternatives.first())
                    .and_then(|alternative| alternative.get("transcript"))
                    .and_then(|value| value.as_str())
                else {
                    continue;
                };
                if !text.is_empty() {
                    text.push(' ');
                }
                text.push_str(transcript);
            }
        }
        Ok(text)
    }

    fn describe(&self) -> String {
        "Google (онлайн)".to_string()
    }
}

/// Кодировать сэмплы 16 кГц mono в FLAC (формат сервиса Google).
fn encode_flac(samples: &[f32]) -> Result<Vec<u8>, String> {
    use flacenc::component::BitRepr;
    use flacenc::error::Verify;

    let samples_i32: Vec<i32> = samples
        .iter()
        .map(|sample| (sample.clamp(-1.0, 1.0) * 32767.0) as i32)
        .collect();
    let config = flacenc::config::Encoder::default()
        .into_verified()
        .map_err(|error| format!("{error:?}"))?;
    let source = flacenc::source::MemSource::from_samples(&samples_i32, 1, 16, 16_000);
    let stream = flacenc::encode_with_fixed_block_size(&config, source, config.block_size)
        .map_err(|error| format!("{error:?}"))?;
    let mut sink = flacenc::bitsink::ByteSink::new();
    stream
        .write(&mut sink)
        .map_err(|error| format!("{error:?}"))?;
    Ok(sink.as_slice().to_vec())
}

// --- Vosk (офлайн) -----------------------------------------------------------

/// Распознавание через Vosk (офлайн, модель Kaldi).
pub struct VoskRecognizer {
    engine: VoskEngine,
}

impl VoskRecognizer {
    fn create(config: &AppConfig, data_dir: &Path) -> Result<Self, EngineError> {
        let library = find_libvosk(data_dir).ok_or_else(|| {
            EngineError::Missing(
                data_dir
                    .join("vosk")
                    .join("libvosk.dll")
                    .display()
                    .to_string(),
            )
        })?;
        let model = find_vosk_model(data_dir, config).ok_or_else(|| {
            EngineError::Missing(data_dir.join("models").join("vosk").display().to_string())
        })?;
        VoskEngine::load(&library, &model)
            .map(|engine| Self { engine })
            .map_err(EngineError::Failed)
    }
}

impl Recognizer for VoskRecognizer {
    fn recognize(&mut self, samples: &[f32], _language: &str) -> Result<String, String> {
        let json = self.engine.recognize_json(samples)?;
        let value: serde_json::Value =
            serde_json::from_str(&json).map_err(|error| format!("vosk json: {error}"))?;
        Ok(value
            .get("text")
            .and_then(|text| text.as_str())
            .unwrap_or_default()
            .trim()
            .to_string())
    }

    fn describe(&self) -> String {
        format!("Vosk ({})", self.engine.model_name())
    }
}

/// Найти `libvosk.dll`: сначала в `data\vosk\`, затем рядом с exe.
#[must_use]
pub fn find_libvosk(data_dir: &Path) -> Option<PathBuf> {
    let mut candidates = vec![data_dir.join("vosk").join("libvosk.dll")];
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.join("libvosk.dll"));
        }
    }
    candidates.into_iter().find(|path| path.is_file())
}

/// Найти папку модели Vosk: `voice.vosk_model_path` или первая папка в `data\models\vosk\`.
#[must_use]
pub fn find_vosk_model(data_dir: &Path, config: &AppConfig) -> Option<PathBuf> {
    let configured = config.voice.vosk_model_path.trim();
    if !configured.is_empty() {
        let path = PathBuf::from(configured);
        let path = if path.is_absolute() {
            path
        } else {
            data_dir.join(path)
        };
        return path.is_dir().then_some(path);
    }
    let root = data_dir.join("models").join("vosk");
    // Установщик может положить файлы модели прямо в `models\vosk`
    // (am/conf/graph в корне) — такую раскладку распознаём до поиска подпапок.
    if root.join("am").join("final.mdl").is_file() {
        return Some(root);
    }
    let mut entries: Vec<PathBuf> = std::fs::read_dir(root)
        .ok()?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect();
    entries.sort();
    let preferred = entries
        .iter()
        .find(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.contains("ru"))
        })
        .cloned();
    preferred.or_else(|| entries.into_iter().next())
}
