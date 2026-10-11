//! Замер скорости whisper при разном числе потоков (диагностика).
//!
//! Запуск: `cargo run --release -p klavium-app --example whisper_speed -- <модель.bin> <аудио.wav> [язык]`
//!
//! Грузит модель один раз, затем распознаёт один и тот же файл
//! последовательно с 1/2/3/4/6/8 потоками и печатает время.

use std::path::Path;

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        eprintln!("usage: whisper_speed <model.bin> <audio.wav> [lang]");
        std::process::exit(2);
    }
    let model = Path::new(&args[1]);
    let wav = Path::new(&args[2]);
    let language = args.get(3).map_or("en", String::as_str);

    let (samples, rate) = read_wav_pcm16(wav)?;
    let samples = if rate == 16_000 {
        samples
    } else {
        klavium_app::voice::audio::resample_linear(&samples, rate, 16_000)
    };
    let seconds = f64::from(samples.len() as u32) / f64::from(16_000_u32);
    println!("аудио: {seconds:.1} с | модель: {}", model.display());

    let started = std::time::Instant::now();
    whisper_rs::install_logging_hooks();
    let ctx = whisper_rs::WhisperContext::new_with_params(
        model,
        whisper_rs::WhisperContextParameters::default(),
    )
    .map_err(|e| anyhow::anyhow!("загрузка модели: {e}"))?;
    println!("загрузка модели: {:.1} с", started.elapsed().as_secs_f32());
    let mut state = ctx.create_state()?;

    for threads in [1_i32, 2, 4, 6] {
        let started = std::time::Instant::now();
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
        params.set_suppress_nst(true);
        state.full(params, &samples)?;
        println!(
            "потоков {threads:>2}: {:>7.2} с",
            started.elapsed().as_secs_f32()
        );
    }
    Ok(())
}

/// Минимальный парсер WAV: RIFF/WAVE, PCM16 mono.
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
