//! Сервис статистики: счётчики в памяти + сохранение в `stats.json`.
//!
//! Автосейв — фоновым потоком (раз в 30 с при изменениях) и при выходе.

// Явные области блокировок читаются лучше микро-оптимизации (как в engine.rs).
#![allow(clippy::significant_drop_tightening)]

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use klavium_core::stats::{Session, StatsData, MAX_SESSIONS};

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Сервис статистики.
pub struct StatsService {
    data: Mutex<StatsData>,
    current: Mutex<Session>,
    started: Instant,
    dirty: AtomicBool,
    path: PathBuf,
}

impl StatsService {
    /// Создать сервис (читает файл; при отсутствии — пустые данные).
    #[must_use]
    pub fn new(path: PathBuf) -> Self {
        let data = std::fs::read_to_string(&path)
            .ok()
            .and_then(|text| serde_json::from_str::<StatsData>(&text).ok())
            .unwrap_or_default();
        let current = Session {
            started_at: now_unix(),
            ..Session::default()
        };
        Self {
            data: Mutex::new(data),
            current: Mutex::new(current),
            started: Instant::now(),
            dirty: AtomicBool::new(false),
            path,
        }
    }

    /// Добавить слово в счётчики (`en`/`ru`/`voice`; прочее — `other`).
    pub fn add_word(&self, category: &str) {
        let mut data = lock(&self.data);
        let mut current = lock(&self.current);
        for counts in [&mut data.total_words, &mut current.words] {
            match category {
                "en" => counts.en += 1,
                "ru" => counts.ru += 1,
                "voice" => counts.voice += 1,
                _ => counts.other += 1,
            }
        }
        self.dirty.store(true, Ordering::Relaxed);
    }

    /// Зафиксировать переключение раскладки.
    pub fn add_switch(&self) {
        lock(&self.data).layout_switches += 1;
        lock(&self.current).switches += 1;
        self.dirty.store(true, Ordering::Relaxed);
    }

    /// Сбросить счётчики (включая голосовые).
    pub fn reset(&self) {
        let mut data = lock(&self.data);
        *data = StatsData {
            last_reset: now_unix(),
            ..StatsData::default()
        };
        self.dirty.store(true, Ordering::Relaxed);
    }

    /// Снимок статистики для UI.
    #[must_use]
    pub fn snapshot(&self) -> StatsData {
        lock(&self.data).clone()
    }

    /// Сохранить на диск (закрывает текущую сессию в список сессий).
    ///
    /// # Errors
    ///
    /// Ошибка сериализации или записи файла.
    pub fn save(&self) -> anyhow::Result<()> {
        let mut data = lock(&self.data);
        let current = {
            let mut current = lock(&self.current);
            current.duration_s = self.started.elapsed().as_secs();
            current.clone()
        };

        let replaces_last = data
            .sessions
            .last()
            .is_some_and(|last| last.started_at == current.started_at);
        if replaces_last {
            if let Some(last) = data.sessions.last_mut() {
                *last = current;
            }
        } else {
            data.sessions.push(current);
            if data.sessions.len() > MAX_SESSIONS {
                let extra = data.sessions.len() - MAX_SESSIONS;
                data.sessions.drain(0..extra);
            }
        }

        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&self.path, serde_json::to_string_pretty(&*data)?)?;
        self.dirty.store(false, Ordering::Relaxed);
        Ok(())
    }

    /// Фоновый автосейв (раз в 30 с при изменениях).
    pub fn spawn_autosave(self: &Arc<Self>) {
        let service = Arc::clone(self);
        let spawned = thread::Builder::new()
            .name("stats-autosave".to_string())
            .spawn(move || loop {
                thread::sleep(Duration::from_secs(30));
                if service.dirty.load(Ordering::Relaxed) {
                    let _ = service.save();
                }
            });
        let _ = spawned;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path() -> PathBuf {
        std::env::temp_dir().join(format!("klavium_stats_test_{}.json", std::process::id()))
    }

    #[test]
    fn test_counts_and_reset() {
        let path = temp_path();
        let _ = std::fs::remove_file(&path);
        let service = StatsService::new(path.clone());

        service.add_word("en");
        service.add_word("ru");
        service.add_word("ru");
        service.add_word("voice");
        service.add_word("layout_error"); // уйдёт в other
        service.add_switch();

        let snapshot = service.snapshot();
        assert_eq!(snapshot.total_words.en, 1);
        assert_eq!(snapshot.total_words.ru, 2);
        assert_eq!(snapshot.total_words.voice, 1);
        assert_eq!(snapshot.total_words.other, 1);
        assert_eq!(snapshot.layout_switches, 1);

        service.reset();
        let snapshot = service.snapshot();
        assert_eq!(snapshot.total_words.ru, 0);
        assert_eq!(snapshot.total_words.voice, 0); // фикс: голос тоже сбрасывается
        assert!(snapshot.last_reset > 0);

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_save_and_reload() {
        let path = temp_path();
        let _ = std::fs::remove_file(&path);
        let service = StatsService::new(path.clone());
        service.add_word("ru");
        service.add_switch();
        service.save().expect("сохранение");

        let reloaded = StatsService::new(path.clone());
        let snapshot = reloaded.snapshot();
        assert_eq!(snapshot.total_words.ru, 1);
        assert_eq!(snapshot.layout_switches, 1);
        assert_eq!(snapshot.sessions.len(), 1);

        let _ = std::fs::remove_file(&path);
    }
}
