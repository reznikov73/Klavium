//! Дымовой прогон распознавания без микрофона: аудиофайл → текст.
//!
//! Запуск:
//! `cargo run -p klavium-app --example voice_demo -- <audio.wav> [ru|en] [whisper|google|vosk] [модель]`
//!
//! Ожидается mono PCM16 WAV (частота ≠ 16 кГц — линейный ресемплинг).
//! Движок/модель переопределяют конфиг приложения; каталог данных — как у приложения
//! (можно задать `KLAVIUM_DATA_DIR`).

use std::path::Path;

use klavium_app::config_io;
use klavium_app::voice::audio::{resample_linear, TARGET_RATE};
use klavium_app::voice::engines::{self, EngineError};

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("usage: voice_demo <audio.wav> [ru|en] [whisper|google|vosk] [модель]");
        std::process::exit(2);
    }
    let wav = Path::new(&args[1]);
    let language = args.get(2).map_or("ru", String::as_str);

    let paths = config_io::resolve();
    let mut config = config_io::load(&paths.config_path);
    if let Some(engine) = args.get(3) {
        config.voice.engine.clone_from(engine);
    }
    if let Some(model) = args.get(4) {
        config.voice.model.clone_from(model);
    }

    let (samples, rate) = read_wav_pcm16(wav)?;
    let samples = if rate == TARGET_RATE {
        samples
    } else {
        resample_linear(&samples, rate, TARGET_RATE)
    };
    let seconds = f64::from(samples.len() as u32) / f64::from(TARGET_RATE);
    println!(
        "[voice_demo] {}: {seconds:.1} c аудио, движок={}, язык={language}",
        wav.display(),
        config.voice.engine
    );

    let started = std::time::Instant::now();
    let mut recognizer = match engines::create(&config, &paths.data_dir) {
        Ok(recognizer) => recognizer,
        Err(EngineError::Missing(where_)) => anyhow::bail!("движок не готов (нет: {where_})"),
        Err(EngineError::Failed(reason)) => anyhow::bail!("ошибка движка: {reason}"),
    };
    println!(
        "[voice_demo] движок: {} (инициализация {:.1} с)",
        recognizer.describe(),
        started.elapsed().as_secs_f32()
    );

    let started = std::time::Instant::now();
    let text = recognizer
        .recognize(&samples, language)
        .map_err(anyhow::Error::msg)?;
    println!(
        "[voice_demo] распознано за {:.1} с:",
        started.elapsed().as_secs_f32()
    );
    println!("{text}");
    Ok(())
}

/// Минимальный парсер WAV: RIFF/WAVE, ожидает PCM16 mono.
fn read_wav_pcm16(path: &Path) -> anyhow::Result<(Vec<f32>, u32)> {
    let bytes = std::fs::read(path)?;
    anyhow::ensure!(bytes.len() > 44, "слишком короткий файл");
    anyhow::ensure!(
        &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WAVE",
        "не WAV (RIFF/WAVE)"
    );

    let mut position = 12usize;
    let mut sample_rate = 0u32;
    let mut channels = 1u16;
    let mut bits = 16u16;
    let mut data: Option<&[u8]> = None;
    while position + 8 <= bytes.len() {
        let id = &bytes[position..position + 4];
        let size = u32::from_le_bytes(bytes[position + 4..position + 8].try_into()?) as usize;
        let body_start = position + 8;
        let body_end = (body_start + size).min(bytes.len());
        let body = &bytes[body_start..body_end];
        match id {
            b"fmt " => {
                anyhow::ensure!(body.len() >= 16, "короткий fmt-чанк");
                channels = u16::from_le_bytes(body[2..4].try_into()?);
                sample_rate = u32::from_le_bytes(body[4..8].try_into()?);
                bits = u16::from_le_bytes(body[14..16].try_into()?);
            }
            b"data" => data = Some(body),
            _ => {}
        }
        position = body_start + size + (size & 1);
    }
    let data = data.ok_or_else(|| anyhow::anyhow!("нет data-чанка"))?;
    anyhow::ensure!(channels == 1, "ожидается mono (каналов: {channels})");
    anyhow::ensure!(bits == 16, "ожидается PCM16 (бит: {bits})");
    let samples = data
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| f32::from(i16::from_le_bytes(*pair)) / 32768.0)
        .collect();
    Ok((samples, sample_rate))
}
