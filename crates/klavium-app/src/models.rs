//! Загрузка моделей whisper (кнопка «Скачать» в настройках).
//!
//! Модели качаются с huggingface (ggerganov/whisper.cpp) в каталог данных
//! `models\`; во время загрузки пишется `<файл>.part`, по завершении —
//! переименование. Прогресс читает UI через [`ModelDownloader::status`].

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::Duration;

use klavium_core::voice::DownloadStatus;

/// Блокировка без паник при отравлении.
fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Загрузчик моделей: одна загрузка за раз, в фоновом потоке.
pub struct ModelDownloader {
    models_dir: PathBuf,
    state: Mutex<DownloadStatus>,
}

impl ModelDownloader {
    /// Создать загрузчик для каталога моделей.
    #[must_use]
    pub fn new(models_dir: PathBuf) -> Arc<Self> {
        Arc::new(Self {
            models_dir,
            state: Mutex::new(DownloadStatus::default()),
        })
    }

    /// Снимок состояния (для UI).
    #[must_use]
    pub fn status(&self) -> DownloadStatus {
        lock(&self.state).clone()
    }

    /// Запустить загрузку файла (повторный вызов во время загрузки — игнор).
    pub fn start(self: &Arc<Self>, file: &str) {
        {
            let mut state = lock(&self.state);
            if state.active {
                return;
            }
            *state = DownloadStatus {
                active: true,
                file: file.to_string(),
                ..DownloadStatus::default()
            };
        }
        let downloader = Arc::clone(self);
        let file = file.to_string();
        let spawned = thread::Builder::new()
            .name("model-download".to_string())
            .spawn(move || downloader.run(&file));
        if spawned.is_err() {
            let mut state = lock(&self.state);
            state.active = false;
            state.error = Some("не удалось создать поток загрузки".to_string());
        }
    }

    fn run(&self, file: &str) {
        let result = self.download(file);
        let mut state = lock(&self.state);
        state.active = false;
        match result {
            Ok(()) => {
                state.done = true;
                state.percent = Some(100);
            }
            Err(error) => state.error = Some(error.to_string()),
        }
    }

    fn download(&self, file: &str) -> anyhow::Result<()> {
        std::fs::create_dir_all(&self.models_dir)?;
        let url = format!("https://huggingface.co/ggerganov/whisper.cpp/resolve/main/{file}");
        let target = self.models_dir.join(file);
        download_to(&url, &target, &self.state)
    }
}

/// Скачать URL в файл с прогрессом (пишется `<файл>.part`, затем переименование).
fn download_to(url: &str, target: &Path, state: &Mutex<DownloadStatus>) -> anyhow::Result<()> {
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(30))
        .timeout_read(Duration::from_secs(120))
        .build();
    tracing::info!(target: "klavium", "Загрузка: {url}");
    let response = agent
        .get(url)
        .call()
        .map_err(|error| anyhow::anyhow!("сеть: {error}"))?;
    let total: Option<u64> = response
        .header("Content-Length")
        .and_then(|value| value.parse().ok());
    let part = target.with_file_name(format!(
        "{}.part",
        target
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("download")
    ));
    let mut reader = response.into_reader();
    let mut writer = std::fs::File::create(&part)?;
    let mut buffer = vec![0_u8; 256 * 1024];
    let mut written: u64 = 0;
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        writer.write_all(&buffer[..read])?;
        written += u64::try_from(read).unwrap_or(0);
        if let Some(total) = total.filter(|total| *total > 0) {
            let percent = (written * 100 / total).min(100);
            lock(state).percent = Some(u8::try_from(percent).unwrap_or(100));
        }
    }
    drop(writer);
    std::fs::rename(&part, target)?;
    tracing::info!(target: "klavium", "Загружено: {}", target.display());
    Ok(())
}

// --- Установка Vosk ----------------------------------------------------------

/// Установщик Vosk: libvosk.dll + русская модель (скачивание и распаковка).
pub struct VoskInstaller {
    data_dir: PathBuf,
    state: Mutex<DownloadStatus>,
}

impl VoskInstaller {
    /// Создать установщик для каталога данных.
    #[must_use]
    pub fn new(data_dir: PathBuf) -> Arc<Self> {
        Arc::new(Self {
            data_dir,
            state: Mutex::new(DownloadStatus::default()),
        })
    }

    /// Снимок состояния (для UI).
    #[must_use]
    pub fn status(&self) -> DownloadStatus {
        lock(&self.state).clone()
    }

    /// Запустить установку (повторный вызов во время работы — игнор).
    pub fn start(self: &Arc<Self>) {
        {
            let mut state = lock(&self.state);
            if state.active {
                return;
            }
            *state = DownloadStatus {
                active: true,
                file: "Vosk: библиотека (1/2)".to_string(),
                ..DownloadStatus::default()
            };
        }
        let installer = Arc::clone(self);
        let spawned = thread::Builder::new()
            .name("vosk-install".to_string())
            .spawn(move || installer.run());
        if spawned.is_err() {
            let mut state = lock(&self.state);
            state.active = false;
            state.error = Some("не удалось создать поток установки".to_string());
        }
    }

    fn run(&self) {
        let result = self.install();
        let mut state = lock(&self.state);
        state.active = false;
        match result {
            Ok(()) => {
                state.done = true;
                state.percent = Some(100);
                state.file = "Vosk установлен".to_string();
            }
            Err(error) => state.error = Some(error.to_string()),
        }
    }

    fn install(&self) -> anyhow::Result<()> {
        // 1) libvosk.dll
        let vosk_dir = self.data_dir.join("vosk");
        if !vosk_dir.join("libvosk.dll").is_file() {
            lock(&self.state).file = "Vosk: библиотека (1/2)".to_string();
            lock(&self.state).percent = None;
            let archive = temp_zip_path("vosk-lib");
            download_to(crate::voice::engines::vosk_lib_url(), &archive, &self.state)?;
            let unpacked = temp_unpack_dir("vosk-lib");
            extract_zip(&archive, &unpacked)?;
            let dll = find_file(&unpacked, "libvosk.dll")
                .ok_or_else(|| anyhow::anyhow!("libvosk.dll не найден в архиве"))?;
            let source_dir = dll
                .parent()
                .ok_or_else(|| anyhow::anyhow!("не найден каталог libvosk.dll"))?;
            std::fs::create_dir_all(&vosk_dir)?;
            // Копируем все DLL из архива: libvosk + MinGW-зависимости
            // (libstdc++-6, libgcc_s_seh-1, libwinpthread-1).
            for entry in std::fs::read_dir(source_dir)?.flatten() {
                let path = entry.path();
                let is_dll = path
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("dll"));
                if is_dll && path.is_file() {
                    if let Some(name) = path.file_name() {
                        std::fs::copy(&path, vosk_dir.join(name))?;
                    }
                }
            }
            let _ = std::fs::remove_file(&archive);
            let _ = std::fs::remove_dir_all(&unpacked);
        }
        // 2) русская модель (оригинальная структура — файлами с HuggingFace).
        // Файлы сохраняются в именованную подпапку `vosk-model-small-ru-0.22`.
        let models_root = self.data_dir.join("models").join("vosk");
        let ru_dir = models_root.join(VOSK_MODEL_RU_DIR);
        // «Установлено» — модель в именованной подпапке (актуальный вариант)
        // либо прямо в корне `models\vosk` (раскладка старых сборок; её
        // понимает автопоиск `find_vosk_model`, поэтому не перекачиваем).
        let ru_installed = ru_dir.join("am").join("final.mdl").is_file()
            || models_root.join("am").join("final.mdl").is_file();
        if !ru_installed {
            lock(&self.state).file = "Vosk: русская модель (2/2)".to_string();
            lock(&self.state).percent = None;
            download_hf_tree(VOSK_MODEL_REPO, VOSK_MODEL_RU_PATH, &ru_dir, &self.state)?;
        }
        Ok(())
    }
}

/// Репозиторий-зеркало моделей Vosk на `HuggingFace` (оригинальные структуры).
const VOSK_MODEL_REPO: &str = "Derur/vosk-models";
/// Путь русской small-модели в репозитории.
const VOSK_MODEL_RU_PATH: &str = "stt/ru/vosk-model-small-ru-0.22";
/// Имя папки русской модели на диске.
const VOSK_MODEL_RU_DIR: &str = "vosk-model-small-ru-0.22";

/// Скачать папку модели с `HuggingFace` (файлами, по списку из API).
fn download_hf_tree(
    repo: &str,
    subdir: &str,
    destination: &Path,
    state: &Mutex<DownloadStatus>,
) -> anyhow::Result<()> {
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(30))
        .timeout_read(Duration::from_secs(120))
        .build();
    let api_url =
        format!("https://huggingface.co/api/models/{repo}/tree/main/{subdir}?recursive=true");
    tracing::info!(target: "klavium", "Список файлов модели: {api_url}");
    let body = agent
        .get(&api_url)
        .call()
        .map_err(|error| anyhow::anyhow!("сеть: {error}"))?
        .into_string()?;
    let entries: Vec<serde_json::Value> = serde_json::from_str(&body)?;
    let files: Vec<(String, u64)> = entries
        .iter()
        .filter(|entry| entry.get("type").and_then(|t| t.as_str()) == Some("file"))
        .filter_map(|entry| {
            let path = entry.get("path")?.as_str()?.to_string();
            let size = entry
                .get("size")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0);
            Some((path, size))
        })
        .collect();
    anyhow::ensure!(!files.is_empty(), "список файлов модели пуст");
    let total: u64 = files.iter().map(|(_, size)| *size).sum();
    let mut done: u64 = 0;
    for (path, size) in files {
        let relative = path
            .strip_prefix(subdir)
            .unwrap_or(&path)
            .trim_start_matches('/');
        let target = destination.join(relative);
        if target.is_file() {
            done += size;
            continue;
        }
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let url = format!("https://huggingface.co/{repo}/resolve/main/{path}");
        tracing::info!(target: "klavium", "Загрузка: {url}");
        fetch_url(&agent, &url, &target)?;
        done += size;
        if let Some(percent) = (done * 100).checked_div(total) {
            let percent = percent.min(100);
            lock(state).percent = Some(u8::try_from(percent).unwrap_or(100));
        }
    }
    Ok(())
}

/// Скачать URL в файл (без прогресса; для файлов моделей).
fn fetch_url(agent: &ureq::Agent, url: &str, target: &Path) -> anyhow::Result<()> {
    let response = agent
        .get(url)
        .call()
        .map_err(|error| anyhow::anyhow!("сеть: {error}"))?;
    let part = target.with_file_name(format!(
        "{}.part",
        target
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("download")
    ));
    let mut reader = response.into_reader();
    let mut writer = std::fs::File::create(&part)?;
    std::io::copy(&mut reader, &mut writer)?;
    drop(writer);
    std::fs::rename(&part, target)?;
    Ok(())
}

fn temp_zip_path(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("klavium-{name}.zip"))
}

fn temp_unpack_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("klavium-{name}-unpack"));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

fn extract_zip(archive: &Path, destination: &Path) -> anyhow::Result<()> {
    std::fs::create_dir_all(destination)?;
    let file = std::fs::File::open(archive)?;
    let mut zip = zip::ZipArchive::new(file)?;
    zip.extract(destination)?;
    Ok(())
}

/// Найти файл с именем `name` рекурсивно.
fn find_file(dir: &Path, name: &str) -> Option<PathBuf> {
    for entry in std::fs::read_dir(dir).ok()?.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if let Some(found) = find_file(&path, name) {
                return Some(found);
            }
        } else if entry.file_name() == std::ffi::OsStr::new(name) {
            return Some(path);
        }
    }
    None
}
