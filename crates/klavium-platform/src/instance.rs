//! Single instance — именованный мьютекс.
//!
//! Второй экземпляр приложения обнаруживает мьютекс и завершается.

use std::ptr;

use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, HANDLE};
use windows_sys::Win32::System::Threading::CreateMutexW;

/// Имя мьютекса Klavium (в рамках сессии пользователя).
pub const MUTEX_NAME: &str = "Klavium_TrayUtility_SingleInstance";

/// Владелец именованного мьютекса; при `Drop` мьютекс освобождается.
pub struct SingleInstanceGuard {
    handle: HANDLE,
}

impl Drop for SingleInstanceGuard {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.handle);
        }
    }
}

/// Проверить, запущена ли уже другая копия Klavium.
///
/// `Ok(Some(guard))` — мы единственный экземпляр; `Ok(None)` — уже запущен другой.
///
/// # Errors
///
/// Ошибка, если не удалось создать мьютекс (Win32).
pub fn check_single_instance() -> anyhow::Result<Option<SingleInstanceGuard>> {
    acquire_named(MUTEX_NAME)
}

/// Вариант с явным именем мьютекса (используется тестами и демо).
///
/// # Errors
///
/// Ошибка, если не удалось создать мьютекс (Win32).
pub fn acquire_named(name: &str) -> anyhow::Result<Option<SingleInstanceGuard>> {
    let wide: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
    let handle = unsafe { CreateMutexW(ptr::null(), 1, wide.as_ptr()) };
    if handle.is_null() {
        anyhow::bail!("CreateMutexW: ошибка {}", unsafe { GetLastError() });
    }
    if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
        unsafe {
            let _ = CloseHandle(handle);
        }
        return Ok(None);
    }
    Ok(Some(SingleInstanceGuard { handle }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_second_acquire_returns_none() {
        let name = format!("Klavium_Test_SingleInstance_{}", std::process::id());

        let first = acquire_named(&name).expect("первый захват");
        assert!(first.is_some(), "первый захват должен быть успешным");

        let second = acquire_named(&name).expect("второй захват");
        assert!(
            second.is_none(),
            "второй захват должен увидеть существующий мьютекс"
        );

        drop(first);
        let third = acquire_named(&name).expect("третий захват");
        assert!(third.is_some(), "после освобождения мьютекс снова доступен");
    }
}
