use super::{fit_dimensions, map_box, Image, OcrWord};
use sha2::{Digest, Sha256};
use std::{
    ffi::{c_char, c_int, c_void, CStr, CString},
    path::PathBuf,
    sync::atomic::{AtomicBool, Ordering},
};
type Handle = *mut c_void;
// An app may invalidate analysis rapidly while drawing. Serialize engines so cancelled
// workers cannot initialize simultaneous model sets or race Tesseract's shared caches.
static ENGINE_GATE: std::sync::Mutex<()> = std::sync::Mutex::new(());
fn engine_lease(cancel: &AtomicBool) -> Result<std::sync::MutexGuard<'static, ()>, String> {
    let start = std::time::Instant::now();
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err("Recognition cancelled".into());
        }
        match ENGINE_GATE.try_lock() {
            Ok(lease) => return Ok(lease),
            Err(std::sync::TryLockError::Poisoned(_)) => {
                return Err("OCR worker state is unavailable".into())
            }
            Err(std::sync::TryLockError::WouldBlock) => {}
        }
        if start.elapsed() > std::time::Duration::from_secs(60) {
            return Err("OCR worker is busy; retry recognition".into());
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

extern "C" {
    fn TessBaseAPICreate() -> Handle;
    fn TessBaseAPIDelete(api: Handle);
    fn off42_ocr_init(api: Handle, path: *const c_char, languages: *const c_char) -> c_int;
    fn TessBaseAPISetImage(
        api: Handle,
        data: *const u8,
        width: c_int,
        height: c_int,
        channels: c_int,
        stride: c_int,
    );
    fn TessBaseAPISetPageSegMode(api: Handle, mode: c_int);
    fn TessBaseAPISetSourceResolution(api: Handle, dpi: c_int);
    fn TessBaseAPIRecognize(api: Handle, monitor: Handle) -> c_int;
    fn TessBaseAPIGetIterator(api: Handle) -> Handle;
    fn TessResultIteratorDelete(iterator: Handle);
    fn TessResultIteratorNext(iterator: Handle, level: c_int) -> c_int;
    fn TessResultIteratorGetUTF8Text(iterator: Handle, level: c_int) -> *mut c_char;
    fn TessResultIteratorGetPageIteratorConst(iterator: Handle) -> Handle;
    fn TessPageIteratorBoundingBox(
        iterator: Handle,
        level: c_int,
        left: *mut c_int,
        top: *mut c_int,
        right: *mut c_int,
        bottom: *mut c_int,
    ) -> c_int;
    fn TessDeleteText(text: *const c_char);
    fn TessMonitorCreate() -> Handle;
    fn TessMonitorDelete(monitor: Handle);
    fn TessMonitorSetCancelFunc(
        monitor: Handle,
        callback: unsafe extern "C" fn(Handle, c_int) -> bool,
    );
    fn TessMonitorSetCancelThis(monitor: Handle, data: Handle);
    fn TessMonitorSetDeadlineMSecs(monitor: Handle, milliseconds: c_int);
}
struct Owned(Handle, unsafe extern "C" fn(Handle));
impl Drop for Owned {
    fn drop(&mut self) {
        unsafe { (self.1)(self.0) }
    }
}
struct Text(*mut c_char);
impl Drop for Text {
    fn drop(&mut self) {
        unsafe { TessDeleteText(self.0) }
    }
}
unsafe extern "C" fn cancelled(data: Handle, _: c_int) -> bool {
    // Tesseract calls synchronously during Recognize; the borrowed token outlives the monitor.
    unsafe { (&*data.cast::<AtomicBool>()).load(Ordering::Relaxed) }
}
fn data_dir() -> Result<PathBuf, String> {
    if let Some(path) = std::env::var_os("OFF42_OCR_DATA") {
        return Ok(path.into());
    }
    Ok(std::env::current_exe()
        .map_err(|e| e.to_string())?
        .parent()
        .ok_or("Missing OCR application directory")?
        .join("ocr/tessdata"))
}
#[derive(serde::Deserialize)]
struct Model {
    tag: String,
    code: String,
    name: String,
    bytes: u64,
    sha256: String,
}
#[derive(serde::Deserialize)]
struct Models {
    models: Vec<Model>,
}
fn models() -> Models {
    serde_json::from_str(include_str!("../../../../native/ocr/models.json"))
        .expect("Pinned OCR model manifest")
}
fn select(language: &str) -> Result<Vec<Model>, String> {
    let requested = if language == "auto" {
        "eng+rus+tur"
    } else {
        language
    };
    if requested.len() > 128 {
        return Err("Too many OCR languages".into());
    }
    let catalog = models();
    let mut selected = Vec::new();
    for part in requested.split('+') {
        let lower = part.to_ascii_lowercase().replace('_', "-");
        let base = lower.split('-').next().unwrap_or("");
        let tag = match lower.as_str() {
            "zh-tw" | "zh-hk" => "zh-hant",
            "zh-cn" | "zh-sg" => "zh-hans",
            _ => lower.as_str(),
        };
        let model = catalog
            .models
            .iter()
            .find(|m| m.code == lower || m.tag.eq_ignore_ascii_case(tag))
            .or_else(|| catalog.models.iter().find(|m| m.tag == base))
            .ok_or_else(|| format!("Unknown built-in OCR language: {part}"))?;
        if !selected.iter().any(|m: &Model| m.code == model.code) {
            selected.push(Model {
                tag: model.tag.clone(),
                code: model.code.clone(),
                name: model.name.clone(),
                bytes: model.bytes,
                sha256: model.sha256.clone(),
            });
        }
    }
    if selected.is_empty() || selected.len() > 3 {
        return Err("Choose one to three OCR languages".into());
    }
    Ok(selected)
}
fn verify(path: &std::path::Path, model: &Model) -> Result<(), String> {
    let path = path.join(format!("{}.traineddata", model.code));
    let file = std::fs::File::open(&path).map_err(|_| {
        format!(
            "Missing OCR model {}. Extract the complete OFF42 ZIP.",
            model.name
        )
    })?;
    if file.metadata().map_err(|e| e.to_string())?.len() != model.bytes {
        return Err(format!("Invalid OCR model {}", model.name));
    }
    use std::io::Read;
    let mut hash = Sha256::new();
    let mut reader = file.take(model.bytes + 1);
    let mut buf = [0u8; 65536];
    loop {
        let n = reader.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        hash.update(&buf[..n]);
    }
    if format!("{:x}", hash.finalize()) != model.sha256 {
        return Err(format!("OCR model {} failed its checksum", model.name));
    }
    Ok(())
}
pub fn installed_languages() -> Result<Vec<(String, String)>, String> {
    let path = data_dir()?;
    let mut available = Vec::new();
    for model in models().models {
        verify(&path, &model)?;
        available.push((model.tag, model.name));
    }
    available.sort();
    Ok(available)
}
pub fn recognize(
    page: &Image<'_>,
    language: &str,
    cancel: &AtomicBool,
) -> Result<(Vec<OcrWord>, String), String> {
    let check = || {
        if cancel.load(Ordering::Relaxed) {
            Err("Recognition cancelled".to_string())
        } else {
            Ok(())
        }
    };
    check()?;
    let _lease = engine_lease(cancel)?;
    let expected = u64::from(page.width) * u64::from(page.height) * 4;
    if page.width == 0
        || page.height == 0
        || expected > 256 * 1024 * 1024
        || expected != page.pixels.len() as u64
    {
        return Err("Invalid OCR image or memory limit exceeded".into());
    }
    let selected = select(language)?;
    let path = data_dir()?;
    for model in &selected {
        check()?;
        verify(&path, model)?;
    }
    let language = selected
        .iter()
        .map(|m| m.code.as_str())
        .collect::<Vec<_>>()
        .join("+");
    let langs = CString::new(language.as_str()).map_err(|_| "Invalid OCR language")?;
    let path = CString::new(path.to_str().ok_or("Invalid OCR model path")?)
        .map_err(|_| "Invalid OCR model path")?;
    let (width, height) =
        fit_dimensions(page.width, page.height, 4000).ok_or("Invalid OCR image")?;
    let scaled = if (width, height) != (page.width, page.height) {
        let original = image::RgbaImage::from_raw(page.width, page.height, page.pixels.to_vec())
            .ok_or("Invalid OCR pixels")?;
        Some(image::imageops::resize(
            &original,
            width,
            height,
            image::imageops::FilterType::Lanczos3,
        ))
    } else {
        None
    };
    let pixels = scaled
        .as_ref()
        .map_or(page.pixels, |p| p.as_raw().as_slice());
    let gray = pixels
        .chunks_exact(4)
        .map(|p| {
            let a = u32::from(p[3]);
            let l = (u32::from(p[0]) * 299 + u32::from(p[1]) * 587 + u32::from(p[2]) * 114 + 500)
                / 1000;
            ((l * a + 255 * (255 - a) + 127) / 255) as u8
        })
        .collect::<Vec<_>>();
    check()?;
    // All native handles and the backing pixels stay on this worker thread and are released on every error.
    unsafe {
        let api = Owned(TessBaseAPICreate(), TessBaseAPIDelete);
        if api.0.is_null() {
            return Err("Cannot create OCR engine".into());
        }
        if off42_ocr_init(api.0, path.as_ptr(), langs.as_ptr()) != 0 {
            return Err("Cannot initialize built-in OCR models".into());
        }
        check()?;
        TessBaseAPISetPageSegMode(api.0, 3); // PSM_AUTO, without the separate orientation model.
        TessBaseAPISetImage(
            api.0,
            gray.as_ptr(),
            width as i32,
            height as i32,
            1,
            width as i32,
        );
        TessBaseAPISetSourceResolution(api.0, 180);
        let monitor = Owned(TessMonitorCreate(), TessMonitorDelete);
        if monitor.0.is_null() {
            return Err("Cannot create OCR monitor".into());
        }
        TessMonitorSetCancelThis(monitor.0, (cancel as *const AtomicBool).cast_mut().cast());
        TessMonitorSetCancelFunc(monitor.0, cancelled);
        TessMonitorSetDeadlineMSecs(monitor.0, 60_000);
        if TessBaseAPIRecognize(api.0, monitor.0) != 0 {
            return Err("Recognition cancelled, timed out or failed".into());
        }
        check()?;
        let iterator = Owned(TessBaseAPIGetIterator(api.0), TessResultIteratorDelete);
        if iterator.0.is_null() {
            return Ok((Vec::new(), language));
        }
        let mut words = Vec::new();
        let mut bytes = 0usize;
        loop {
            check()?;
            let text = Text(TessResultIteratorGetUTF8Text(iterator.0, 3)); // RIL_WORD
            if !text.0.is_null() {
                let value = CStr::from_ptr(text.0)
                    .to_str()
                    .map_err(|_| "Invalid OCR UTF-8")?
                    .trim();
                bytes = bytes
                    .checked_add(value.len())
                    .filter(|v| *v <= 8 * 1024 * 1024)
                    .ok_or("OCR text limit exceeded")?;
                if value.len() > 4096 || value.chars().any(|c| c.is_control()) {
                    return Err("Invalid OCR word".into());
                }
                let (mut left, mut top, mut right, mut bottom) = (0, 0, 0, 0);
                if !value.is_empty()
                    && TessPageIteratorBoundingBox(
                        TessResultIteratorGetPageIteratorConst(iterator.0),
                        3,
                        &mut left,
                        &mut top,
                        &mut right,
                        &mut bottom,
                    ) != 0
                {
                    if let Some((x, y, w, h)) = map_box(
                        [
                            left as f64,
                            top as f64,
                            (right - left) as f64,
                            (bottom - top) as f64,
                        ],
                        (width, height),
                        (page.width, page.height),
                    ) {
                        if words.len() >= 100_000 {
                            return Err("OCR word limit exceeded".into());
                        }
                        words.push(OcrWord {
                            text: value.into(),
                            x,
                            y,
                            width: w,
                            height: h,
                        });
                    }
                }
            }
            if TessResultIteratorNext(iterator.0, 3) == 0 {
                break;
            }
        }
        Ok((words, language))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn language_aliases_and_limits() {
        assert_eq!(
            select("en-US+rus+tr")
                .unwrap()
                .iter()
                .map(|m| m.code.as_str())
                .collect::<Vec<_>>(),
            ["eng", "rus", "tur"]
        );
        assert_eq!(select("zh-TW").unwrap()[0].code, "chi_tra");
        assert_eq!(select("auto").unwrap().len(), 3);
        assert!(select("../../eng").is_err());
        assert!(select("eng+fra+rus+tur").is_err());
    }
}

#[cfg(test)]
mod engine_tests {
    use super::*;
    #[test]
    fn pinned_engine_recognizes_real_pixels_and_honors_cancel() {
        let pixels = image::load_from_memory(include_bytes!("../../fixtures/ocr-english.png"))
            .unwrap()
            .to_rgba8();
        let image = Image {
            width: pixels.width(),
            height: pixels.height(),
            pixels: pixels.as_raw(),
        };
        let cancel = AtomicBool::new(false);
        let (words, language) = recognize(&image, "en-US", &cancel).unwrap();
        let text = words
            .iter()
            .map(|w| w.text.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        assert_eq!(language, "eng");
        assert!(text.contains("offline text recognition"), "{text}");
        assert!(text.contains("1234567890"), "{text}");
        assert!(words.iter().all(|w| w.width > 0
            && w.height > 0
            && w.x + w.width <= image.width
            && w.y + w.height <= image.height));
        assert_eq!(installed_languages().unwrap().len(), 20);
        cancel.store(true, Ordering::Relaxed);
        assert!(recognize(&image, "eng", &cancel)
            .unwrap_err()
            .contains("cancelled"));
    }
}
