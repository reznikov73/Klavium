//! Автозапуск через реестр (`HKCU\Software\Microsoft\Windows\CurrentVersion\Run`).
//!
//! Значение `Klavium` указывает на путь к исполняемому файлу в кавычках.

use windows_sys::Win32::Foundation::WIN32_ERROR;
use windows_sys::Win32::System::Registry::{
    RegCloseKey, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW, HKEY,
    HKEY_CURRENT_USER, KEY_QUERY_VALUE, KEY_SET_VALUE, REG_SZ,
};

/// Имя значения в ключе автозапуска.
pub const AUTOSTART_VALUE_NAME: &str = "Klavium";

const RUN_KEY_PATH: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
const ERROR_SUCCESS: WIN32_ERROR = 0;
const ERROR_FILE_NOT_FOUND: WIN32_ERROR = 2;

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

fn open_run_key(access: u32) -> Option<HKEY> {
    let path = wide(RUN_KEY_PATH);
    let mut key: HKEY = std::ptr::null_mut();
    let result = unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, path.as_ptr(), 0, access, &mut key) };
    if result == ERROR_SUCCESS {
        Some(key)
    } else {
        None
    }
}

fn run_value_present(name: &str) -> bool {
    let Some(key) = open_run_key(KEY_QUERY_VALUE) else {
        return false;
    };
    let value_name = wide(name);
    let result = unsafe {
        RegQueryValueExW(
            key,
            value_name.as_ptr(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    unsafe {
        let _ = RegCloseKey(key);
    }
    result == ERROR_SUCCESS
}

/// Прочитать текущую команду автозапуска (если есть).
fn read_run_value(name: &str) -> Option<String> {
    let key = open_run_key(KEY_QUERY_VALUE)?;
    let value_name = wide(name);
    let mut buffer = [0u16; 1024];
    let mut size = (buffer.len() * 2) as u32;
    let result = unsafe {
        RegQueryValueExW(
            key,
            value_name.as_ptr(),
            std::ptr::null(),
            std::ptr::null_mut(),
            buffer.as_mut_ptr().cast::<u8>(),
            &mut size,
        )
    };
    unsafe {
        let _ = RegCloseKey(key);
    }
    if result != ERROR_SUCCESS {
        return None;
    }
    let length = (size as usize / 2).min(buffer.len());
    let text: Vec<u16> = buffer[..length]
        .iter()
        .copied()
        .take_while(|c| *c != 0)
        .collect();
    String::from_utf16(&text).ok()
}

/// Совпадает ли запись автозапуска с текущей командой (exe + `--silent`).
#[must_use]
pub fn autostart_is_current() -> bool {
    let Ok(exe) = std::env::current_exe() else {
        return false;
    };
    let expected = format!("\"{}\" --silent", exe.display());
    read_run_value(AUTOSTART_VALUE_NAME).as_deref() == Some(expected.as_str())
}

fn set_run_value(name: &str, command: &str) -> anyhow::Result<()> {
    let key = open_run_key(KEY_SET_VALUE)
        .ok_or_else(|| anyhow::anyhow!("не удалось открыть ключ Run для записи"))?;
    let value_name = wide(name);
    // REG_SZ — это UTF-16 с завершающим нулём (иначе в реестре хранился бы мусор).
    let data: Vec<u16> = command.encode_utf16().chain(std::iter::once(0)).collect();
    let result = unsafe {
        RegSetValueExW(
            key,
            value_name.as_ptr(),
            0,
            REG_SZ,
            data.as_ptr().cast::<u8>(),
            (data.len() * 2) as u32,
        )
    };
    unsafe {
        let _ = RegCloseKey(key);
    }
    if result != ERROR_SUCCESS {
        anyhow::bail!("RegSetValueExW: ошибка {result}");
    }
    Ok(())
}

fn delete_run_value(name: &str) -> anyhow::Result<()> {
    let key = open_run_key(KEY_SET_VALUE)
        .ok_or_else(|| anyhow::anyhow!("не удалось открыть ключ Run для записи"))?;
    let value_name = wide(name);
    let result = unsafe { RegDeleteValueW(key, value_name.as_ptr()) };
    unsafe {
        let _ = RegCloseKey(key);
    }
    if result != ERROR_SUCCESS && result != ERROR_FILE_NOT_FOUND {
        anyhow::bail!("RegDeleteValueW: ошибка {result}");
    }
    Ok(())
}

/// Включён ли автозапуск Klavium.
#[must_use]
pub fn is_autostart_enabled() -> bool {
    run_value_present(AUTOSTART_VALUE_NAME)
}

/// Включить автозапуск (запись указывает на текущий исполняемый файл).
///
/// # Errors
///
/// Ошибка, если не удалось определить путь к exe или записать в реестр.
pub fn enable_autostart() -> anyhow::Result<()> {
    let exe = std::env::current_exe()?;
    // `--silent`: при автозапуске окно настроек не открываем (живём в трее).
    let command = format!("\"{}\" --silent", exe.display());
    set_run_value(AUTOSTART_VALUE_NAME, &command)
}

/// Выключить автозапуск.
///
/// # Errors
///
/// Ошибка, если не удалось удалить значение из реестра.
pub fn disable_autostart() -> anyhow::Result<()> {
    delete_run_value(AUTOSTART_VALUE_NAME)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_autostart_roundtrip() {
        // Используем тестовое имя значения и подчищаем за собой.
        let name = "Klavium_SelfTest";
        let _ = delete_run_value(name);
        assert!(!run_value_present(name));

        set_run_value(name, "\"C:\\test\\klavium.exe\"").expect("запись тестового значения");
        assert!(run_value_present(name));
        assert_eq!(
            read_run_value(name).as_deref(),
            Some("\"C:\\test\\klavium.exe\"")
        );

        delete_run_value(name).expect("удаление тестового значения");
        assert!(!run_value_present(name));
    }

    #[test]
    fn test_unknown_value_absent() {
        assert!(!run_value_present("Klavium_Definitely_Not_Set_123"));
    }
}
