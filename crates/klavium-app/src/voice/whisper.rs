//! Движок распознавания Whisper (whisper.cpp через whisper-rs, офлайн).

use std::path::{Path, PathBuf};

use super::engines::Recognizer;

/// Порог «не речь»: сегменты выше него отбрасываются (0.6 — умолчание whisper).
const NO_SPEECH_THRESHOLD: f32 = 0.6;

/// Выбрать файл модели: `configured` — имя в `models_dir` или путь.
///
/// Пусто — автопоиск: лучшая из загруженных по каталогу уровней точности
/// (large-v3-turbo → medium → small → base → tiny), затем любой `ggml*.bin`.
#[must_use]
pub fn resolve_model_path(models_dir: &Path, configured: &str) -> Option<PathBuf> {
    let configured = configured.trim();
    if !configured.is_empty() {
        let candidate = Path::new(configured);
        let path = if candidate.is_absolute() {
            candidate.to_path_buf()
        } else {
            models_dir.join(candidate)
        };
        return path.is_file().then_some(path);
    }
    let preferred = klavium_core::voice::MODELS
        .iter()
        .rev()
        .map(|spec| models_dir.join(spec.file))
        .find(|candidate| candidate.is_file());
    if let Some(path) = preferred {
        return Some(path);
    }
    let mut fallback: Vec<PathBuf> = std::fs::read_dir(models_dir)
        .ok()?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or_default();
            path.is_file()
                && name.starts_with("ggml")
                && Path::new(name)
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("bin"))
        })
        .collect();
    fallback.sort();
    fallback.into_iter().next()
}

/// Распознавание Whisper: модель загружается один раз, состояние — в воркере.
pub struct WhisperRecognizer {
    state: whisper_rs::WhisperState,
    name: String,
}

impl WhisperRecognizer {
    /// Загрузить модель и подготовить состояние.
    ///
    /// # Errors
    ///
    /// Строка с описанием ошибки загрузки модели.
    pub fn create(model_path: &Path) -> Result<Self, String> {
        whisper_rs::install_logging_hooks();
        let context = whisper_rs::WhisperContext::new_with_params(
            model_path,
            whisper_rs::WhisperContextParameters::default(),
        )
        .map_err(|error| format!("загрузка модели: {error}"))?;
        let state = context
            .create_state()
            .map_err(|error| format!("состояние: {error}"))?;
        let name = model_path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default()
            .to_string();
        Ok(Self { state, name })
    }
}

impl Recognizer for WhisperRecognizer {
    fn recognize(&mut self, samples: &[f32], language: &str) -> Result<String, String> {
        transcribe(&mut self.state, samples, language, worker_threads())
            .map_err(|error| error.to_string())
    }

    fn describe(&self) -> String {
        format!("Whisper ({})", self.name)
    }
}

/// Число потоков для whisper (не больше 8).
fn worker_threads() -> i32 {
    std::thread::available_parallelism()
        .map_or(4, |n| n.get().min(8))
        .try_into()
        .unwrap_or(4)
}

/// Распознать фразу (16 кГц mono f32) и собрать текст сегментов.
fn transcribe(
    state: &mut whisper_rs::WhisperState,
    samples: &[f32],
    language: &str,
    threads: i32,
) -> Result<String, whisper_rs::WhisperError> {
    let mut params =
        whisper_rs::FullParams::new(whisper_rs::SamplingStrategy::Greedy { best_of: 1 });
    params.set_language(Some(language));
    params.set_translate(false);
    params.set_no_context(true);
    params.set_print_special(false);
    params.set_print_progress(false);
    params.set_print_realtime(false);
    params.set_print_timestamps(false);
    params.set_n_threads(threads);
    // Защита от галлюцинаций на тишине/шуме: скрыть не-речевые токены (♪ и т.п.)
    // и отбросить сегменты, которые сама модель пометила как «не речь».
    params.set_suppress_nst(true);
    params.set_no_speech_thold(NO_SPEECH_THRESHOLD);
    state.full(params, samples)?;

    let mut text = String::new();
    for segment in state.as_iter() {
        if segment.no_speech_probability() > NO_SPEECH_THRESHOLD {
            continue;
        }
        if let Ok(part) = segment.to_str() {
            text.push_str(part);
        }
    }
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("klavium-voice-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("тестовый каталог");
        dir
    }

    #[test]
    fn test_resolve_model_prefers_small() {
        let dir = temp_dir("models");
        std::fs::write(dir.join("ggml-tiny.bin"), b"x").expect("файл");
        std::fs::write(dir.join("ggml-small.bin"), b"x").expect("файл");

        let resolved = resolve_model_path(&dir, "").expect("модель");
        assert_eq!(
            resolved.file_name().and_then(|name| name.to_str()),
            Some("ggml-small.bin")
        );

        let custom = resolve_model_path(&dir, "ggml-tiny.bin").expect("модель");
        assert_eq!(
            custom.file_name().and_then(|name| name.to_str()),
            Some("ggml-tiny.bin")
        );

        assert!(resolve_model_path(&dir, "ggml-big.bin").is_none());
        assert!(resolve_model_path(&dir, "").is_some());

        let _ = std::fs::remove_dir_all(&dir);
    }
}
