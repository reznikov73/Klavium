//! Обёртка над `libvosk.dll` (офлайн-распознавание Vosk).
//!
//! Библиотека загружается динамически (`libloading`) — жёсткой зависимости от
//! DLL при сборке нет; файл ищется в `data\vosk\` или рядом с программой.

use std::ffi::{c_char, c_int, c_void, CStr, CString};
use std::path::Path;

use libloading::os::windows::{
    Library, LOAD_LIBRARY_SEARCH_DEFAULT_DIRS, LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR,
};

/// Точки входа libvosk.
struct VoskApi {
    _library: Library,
    set_log_level: unsafe extern "C" fn(c_int),
    model_new: unsafe extern "C" fn(*const c_char) -> *mut c_void,
    model_free: unsafe extern "C" fn(*mut c_void),
    recognizer_new: unsafe extern "C" fn(*mut c_void, f32) -> *mut c_void,
    recognizer_free: unsafe extern "C" fn(*mut c_void),
    accept_waveform: unsafe extern "C" fn(*mut c_void, *const c_char, c_int) -> c_int,
    final_result: unsafe extern "C" fn(*mut c_void) -> *const c_char,
}

impl VoskApi {
    unsafe fn load(library: Library) -> Result<Self, String> {
        macro_rules! symbol {
            ($name:literal, $ty:ty) => {{
                let pointer = unsafe {
                    library
                        .get::<$ty>(concat!($name, "\0").as_bytes())
                        .map_err(|error| format!("libvosk: {error}"))?
                };
                *pointer
            }};
        }
        let api = Self {
            set_log_level: symbol!("vosk_set_log_level", unsafe extern "C" fn(c_int)),
            model_new: symbol!(
                "vosk_model_new",
                unsafe extern "C" fn(*const c_char) -> *mut c_void
            ),
            model_free: symbol!("vosk_model_free", unsafe extern "C" fn(*mut c_void)),
            recognizer_new: symbol!(
                "vosk_recognizer_new",
                unsafe extern "C" fn(*mut c_void, f32) -> *mut c_void
            ),
            recognizer_free: symbol!("vosk_recognizer_free", unsafe extern "C" fn(*mut c_void)),
            accept_waveform: symbol!(
                "vosk_recognizer_accept_waveform",
                unsafe extern "C" fn(*mut c_void, *const c_char, c_int) -> c_int
            ),
            final_result: symbol!(
                "vosk_recognizer_final_result",
                unsafe extern "C" fn(*mut c_void) -> *const c_char
            ),
            _library: library,
        };
        Ok(api)
    }
}

/// Движок Vosk: загруженная библиотека + модель.
pub struct VoskEngine {
    api: VoskApi,
    model: *mut c_void,
    model_name: String,
}

impl VoskEngine {
    /// Загрузить `libvosk.dll` и модель из папки.
    ///
    /// # Errors
    ///
    /// Строка с описанием ошибки загрузки библиотеки или модели.
    pub fn load(library_path: &Path, model_dir: &Path) -> Result<Self, String> {
        // Соседние зависимости (libstdc++-6, libgcc_s_seh-1, libwinpthread-1)
        // ищем рядом с самой библиотекой.
        let library = unsafe {
            Library::load_with_flags(
                library_path,
                LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_DEFAULT_DIRS,
            )
        }
        .map_err(|error| format!("libvosk: {error}"))?;
        let api = unsafe { VoskApi::load(library) }?;
        unsafe {
            (api.set_log_level)(-1); // без логов Kaldi в консоль
        }
        let model_name = model_dir
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("vosk")
            .to_string();
        let path = CString::new(model_dir.to_string_lossy().as_bytes())
            .map_err(|error| format!("путь модели: {error}"))?;
        let model = unsafe { (api.model_new)(path.as_ptr()) };
        if model.is_null() {
            return Err("не удалось загрузить модель Vosk".to_string());
        }
        Ok(Self {
            api,
            model,
            model_name,
        })
    }

    /// Имя модели (папки).
    #[must_use]
    pub fn model_name(&self) -> &str {
        &self.model_name
    }

    /// Распознать фразу (16 кГц mono) и вернуть JSON-ответ Vosk.
    ///
    /// # Errors
    ///
    /// Строка с описанием ошибки распознавания.
    pub fn recognize_json(&mut self, samples: &[f32]) -> Result<String, String> {
        let mut bytes = Vec::with_capacity(samples.len() * 2);
        for sample in samples {
            let value = (sample.clamp(-1.0, 1.0) * 32767.0) as i16;
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        let recognizer = unsafe { (self.api.recognizer_new)(self.model, 16_000.0) };
        if recognizer.is_null() {
            return Err("vosk: не удалось создать распознаватель".to_string());
        }
        unsafe {
            let _ =
                (self.api.accept_waveform)(recognizer, bytes.as_ptr().cast(), bytes.len() as c_int);
        }
        let pointer = unsafe { (self.api.final_result)(recognizer) };
        let json = if pointer.is_null() {
            String::new()
        } else {
            unsafe { CStr::from_ptr(pointer) }
                .to_string_lossy()
                .into_owned()
        };
        unsafe {
            (self.api.recognizer_free)(recognizer);
        }
        Ok(json)
    }
}

impl Drop for VoskEngine {
    fn drop(&mut self) {
        unsafe {
            (self.api.model_free)(self.model);
        }
    }
}
