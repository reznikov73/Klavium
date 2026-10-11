//! Захват микрофона (cpal), VAD-сегментация речи и ресемплинг в 16 кГц.
//!
//! Whisper работает с 16 кГц mono f32; устройство обычно даёт 44.1/48 кГц.
//! Сегментация — энергетическая (RMS по блокам 20 мс) с калибровкой шума
//! в первые полсекунды.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, Sample, SampleFormat, SizedSample};

/// Целевая частота дискретизации распознавания.
pub const TARGET_RATE: u32 = 16_000;

/// Длительность блока анализа VAD (мс).
pub const BLOCK_MS: u32 = 20;

/// Блоков калибровки шума (0.5 с).
const CALIBRATION_BLOCKS: usize = 25;

/// Сколько тишинных блоков оставляем в хвосте фразы (80 мс).
const TRIM_KEEP_BLOCKS: usize = 4;

/// Параметры VAD.
#[derive(Debug, Clone, Copy)]
pub struct VadParams {
    /// Порог RMS: блок выше — речь.
    pub threshold: f32,
    /// Тишина после речи, блоков (0.7 с при 20 мс).
    pub silence_blocks: usize,
    /// Минимальное число «речевых» блоков (короткие всхлипы — выбросить).
    pub min_speech_blocks: usize,
    /// Максимальная длина фразы, блоков (15 с при 20 мс).
    pub max_blocks: usize,
}

impl Default for VadParams {
    fn default() -> Self {
        Self {
            threshold: 0.006,
            silence_blocks: 35,
            min_speech_blocks: 8,
            max_blocks: 750,
        }
    }
}

/// Результат обработки блока VAD.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VadOutcome {
    /// Фраза продолжается (или ещё не началась).
    None,
    /// Фраза завершена — можно распознавать.
    Complete,
    /// Фраза слишком короткая — выброшена.
    TooShort,
}

/// Энергетический сегментатор речи (по блокам).
pub struct VadSegmenter {
    params: VadParams,
    in_phrase: bool,
    blocks: usize,
    speech_blocks: usize,
    silence_run: usize,
}

impl VadSegmenter {
    /// Новый сегментатор с параметрами.
    #[must_use]
    pub const fn new(params: VadParams) -> Self {
        Self {
            params,
            in_phrase: false,
            blocks: 0,
            speech_blocks: 0,
            silence_run: 0,
        }
    }

    /// Идёт ли сейчас фраза (блоки нужно накапливать).
    #[must_use]
    pub const fn in_phrase(&self) -> bool {
        self.in_phrase
    }

    /// Сколько тишинных блоков в хвосте (для обрезки перед распознаванием).
    #[must_use]
    pub const fn silence_run(&self) -> usize {
        self.silence_run
    }

    /// Обработать RMS очередного блока.
    pub fn push(&mut self, level: f32) -> VadOutcome {
        if !self.in_phrase {
            if level > self.params.threshold {
                self.in_phrase = true;
                self.blocks = 1;
                self.speech_blocks = 1;
                self.silence_run = 0;
            }
            return VadOutcome::None;
        }
        self.blocks += 1;
        if level > self.params.threshold {
            self.speech_blocks += 1;
            self.silence_run = 0;
        } else {
            self.silence_run += 1;
        }
        let done =
            self.silence_run >= self.params.silence_blocks || self.blocks >= self.params.max_blocks;
        if !done {
            return VadOutcome::None;
        }
        self.in_phrase = false;
        if self.speech_blocks >= self.params.min_speech_blocks {
            VadOutcome::Complete
        } else {
            VadOutcome::TooShort
        }
    }
}

/// RMS (среднеквадратичная амплитуда) блока.
#[must_use]
pub fn rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum: f64 = samples
        .iter()
        .map(|&sample| f64::from(sample) * f64::from(sample))
        .sum();
    let count = count_as_f64(samples.len());
    (sum / count).sqrt() as f32
}

/// Линейный ресемплинг в `to_rate` (для речи достаточно; whisper ждёт 16 кГц).
#[must_use]
pub fn resample_linear(input: &[f32], from_rate: u32, to_rate: u32) -> Vec<f32> {
    if from_rate == to_rate || input.is_empty() {
        return input.to_vec();
    }
    let ratio = f64::from(to_rate) / f64::from(from_rate);
    let out_len = (count_as_f64(input.len()) * ratio).round() as usize;
    let mut out = Vec::with_capacity(out_len);
    for index in 0..out_len {
        let position = count_as_f64(index) / ratio;
        let base = position.floor() as usize;
        let frac = (position - count_as_f64(base)) as f32;
        let first = input.get(base).copied().unwrap_or(0.0);
        let second = input.get(base + 1).copied().unwrap_or(first);
        out.push(f32::mul_add(second - first, frac, first));
    }
    out
}

/// Длина буфера как f64 без потери точности (звуковые буферы ≪ 2³² сэмплов).
fn count_as_f64(value: usize) -> f64 {
    f64::from(value as u32)
}

/// Длина небольшого набора как f32 (для средних значений).
fn count_as_f32(value: usize) -> f32 {
    f32::from(value as u16)
}

/// Запустить сессию захвата: блокируется до `active == false`.
///
/// Фразы (16 кГц mono) уходят в `phrase_tx`. Ошибки открытия устройства
/// возвращаются как `Err` — их показывает вызывающий поток бейджем.
///
/// # Errors
///
/// Ошибка, если не удалось открыть микрофон или настроить поток cpal.
pub fn run_capture_session(
    active: &AtomicBool,
    phrase_tx: &Sender<Vec<f32>>,
) -> anyhow::Result<()> {
    let host = cpal::default_host();
    let device = host
        .default_input_device()
        .ok_or_else(|| anyhow::anyhow!("нет устройства ввода по умолчанию"))?;
    let supported = device
        .default_input_config()
        .map_err(|error| anyhow::anyhow!("конфигурация ввода: {error}"))?;
    let sample_format = supported.sample_format();
    let config: cpal::StreamConfig = supported.into();
    let rate = config.sample_rate;
    let channels = config.channels as usize;

    let buffer: Arc<Mutex<Vec<f32>>> = Arc::new(Mutex::new(Vec::new()));
    let error_callback = |error: cpal::Error| {
        tracing::error!(target: "klavium", "Голос: ошибка аудиопотока: {error}");
    };
    let stream = match sample_format {
        SampleFormat::F32 => {
            build_stream::<f32>(&device, &config, Arc::clone(&buffer), error_callback)?
        }
        SampleFormat::I16 => {
            build_stream::<i16>(&device, &config, Arc::clone(&buffer), error_callback)?
        }
        SampleFormat::U16 => {
            build_stream::<u16>(&device, &config, Arc::clone(&buffer), error_callback)?
        }
        other => anyhow::bail!("неподдерживаемый формат сэмплов: {other:?}"),
    };
    stream
        .play()
        .map_err(|error| anyhow::anyhow!("запуск потока: {error}"))?;
    tracing::info!(target: "klavium", "Голос: микрофон открыт ({rate} Гц, {channels} кан.)");

    let mut segmenter = VadSegmenter::new(VadParams::default());
    let mut calibrated = false;
    let mut calibration: Vec<f32> = Vec::with_capacity(CALIBRATION_BLOCKS);
    let mut pending: Vec<f32> = Vec::new();
    let mut phrase: Vec<f32> = Vec::new();
    let block_len = (rate as usize * BLOCK_MS as usize) / 1000;

    while active.load(Ordering::SeqCst) {
        thread::sleep(Duration::from_millis(u64::from(BLOCK_MS) / 2));
        let chunk: Vec<f32> = {
            let mut guard = buffer.lock().unwrap_or_else(PoisonError::into_inner);
            std::mem::take(&mut *guard)
        };
        if !chunk.is_empty() {
            pending.extend_from_slice(&chunk);
        }
        while pending.len() >= block_len {
            let block: Vec<f32> = pending.drain(..block_len).collect();
            let level = rms(&block);
            if !calibrated {
                calibration.push(level);
                if calibration.len() >= CALIBRATION_BLOCKS {
                    let average = calibration.iter().sum::<f32>() / count_as_f32(calibration.len());
                    let threshold = (average * 3.0).clamp(0.004, 0.05);
                    segmenter = VadSegmenter::new(VadParams {
                        threshold,
                        ..VadParams::default()
                    });
                    calibrated = true;
                }
                continue;
            }
            let was_in_phrase = segmenter.in_phrase();
            let outcome = segmenter.push(level);
            if was_in_phrase || segmenter.in_phrase() {
                phrase.extend_from_slice(&block);
            }
            match outcome {
                VadOutcome::None => {}
                VadOutcome::TooShort => phrase.clear(),
                VadOutcome::Complete => {
                    let trim = segmenter.silence_run().saturating_sub(TRIM_KEEP_BLOCKS);
                    let keep = phrase.len().saturating_sub(trim * block_len);
                    if keep > 0 {
                        let resampled = resample_linear(&phrase[..keep], rate, TARGET_RATE);
                        if !resampled.is_empty() {
                            let _ = phrase_tx.send(resampled);
                        }
                    }
                    phrase.clear();
                }
            }
        }
    }

    drop(stream);
    Ok(())
}

/// Собрать входной поток cpal для конкретного формата сэмплов.
fn build_stream<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    buffer: Arc<Mutex<Vec<f32>>>,
    error_callback: impl FnMut(cpal::Error) + Send + 'static,
) -> anyhow::Result<cpal::Stream>
where
    T: SizedSample,
    f32: FromSample<T>,
{
    let channels = config.channels as usize;
    let channels_f = f32::from(config.channels);
    let stream = device.build_input_stream(
        *config,
        move |data: &[T], _: &cpal::InputCallbackInfo| {
            let mut guard = buffer.lock().unwrap_or_else(PoisonError::into_inner);
            for frame in data.chunks(channels.max(1)) {
                let sum: f32 = frame.iter().map(|sample| f32::from_sample(*sample)).sum();
                guard.push(sum / channels_f);
            }
        },
        error_callback,
        None,
    )?;
    Ok(stream)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rms() {
        assert_eq!(rms(&[]), 0.0);
        let constant = vec![0.5_f32; 100];
        assert!((rms(&constant) - 0.5).abs() < 1e-6);
        let silent = vec![0.0_f32; 100];
        assert_eq!(rms(&silent), 0.0);
    }

    #[test]
    fn test_resample_linear() {
        let input: Vec<f32> = (0..480u16).map(|index| f32::from(index) / 480.0).collect();
        let output = resample_linear(&input, 48_000, 16_000);
        assert_eq!(output.len(), 160);
        assert!(output[0].abs() < 1e-6);
        assert!(output[159] > output[158]);

        let constant = vec![0.25_f32; 480];
        let out = resample_linear(&constant, 48_000, 16_000);
        assert!(out.iter().all(|&value| (value - 0.25).abs() < 1e-6));

        // без изменений частоты — копия
        assert_eq!(resample_linear(&input, 16_000, 16_000), input);
    }

    #[test]
    fn test_vad_segmentation() {
        let mut vad = VadSegmenter::new(VadParams {
            threshold: 0.01,
            silence_blocks: 35,
            min_speech_blocks: 8,
            max_blocks: 750,
        });
        for _ in 0..10 {
            assert_eq!(vad.push(0.001), VadOutcome::None);
        }
        assert!(!vad.in_phrase());
        for index in 0..20 {
            let outcome = vad.push(0.05);
            if index < 19 {
                assert_eq!(outcome, VadOutcome::None);
            }
            assert!(vad.in_phrase());
        }
        let mut completed_at = None;
        for index in 0..40 {
            if vad.push(0.001) == VadOutcome::Complete {
                completed_at = Some(index);
                break;
            }
        }
        assert_eq!(completed_at, Some(34));
        assert_eq!(vad.silence_run(), 35);
    }

    #[test]
    fn test_vad_too_short() {
        let mut vad = VadSegmenter::new(VadParams {
            threshold: 0.01,
            silence_blocks: 35,
            min_speech_blocks: 8,
            max_blocks: 750,
        });
        for _ in 0..3 {
            let _ = vad.push(0.05);
        }
        let mut outcome = VadOutcome::None;
        for _ in 0..35 {
            outcome = vad.push(0.001);
            if outcome != VadOutcome::None {
                break;
            }
        }
        assert_eq!(outcome, VadOutcome::TooShort);
    }

    #[test]
    fn test_vad_max_length() {
        let mut vad = VadSegmenter::new(VadParams {
            threshold: 0.01,
            silence_blocks: 35,
            min_speech_blocks: 8,
            max_blocks: 50,
        });
        let mut completed = false;
        for _ in 0..60 {
            if vad.push(0.05) == VadOutcome::Complete {
                completed = true;
                break;
            }
        }
        assert!(completed);
    }
}
