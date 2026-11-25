//! Optional Windows OCR; language support is queried on the actual device.
use scn42_core::{OcrWord, Page};
use std::sync::atomic::AtomicBool;
/// Fit a page into the engine's limit while retaining aspect ratio.
fn fit_dimensions(width: u32, height: u32, max: u32) -> Option<(u32, u32)> {
    if width == 0 || height == 0 || max == 0 {
        return None;
    }
    let longest = width.max(height);
    if longest <= max {
        return Some((width, height));
    }
    Some((
        ((u64::from(width) * u64::from(max)) / u64::from(longest)).max(1) as u32,
        ((u64::from(height) * u64::from(max)) / u64::from(longest)).max(1) as u32,
    ))
}
/// Map a recognizer box back to source pixels, enclosing the original area.
fn map_box(
    rect: [f64; 4],
    rendered: (u32, u32),
    original: (u32, u32),
) -> Option<(u32, u32, u32, u32)> {
    let [x, y, w, h] = rect;
    if !rect.iter().all(|v| v.is_finite())
        || w <= 0.0
        || h <= 0.0
        || rendered.0 == 0
        || rendered.1 == 0
        || original.0 == 0
        || original.1 == 0
    {
        return None;
    }
    let sx = original.0 as f64 / rendered.0 as f64;
    let sy = original.1 as f64 / rendered.1 as f64;
    let left = (x * sx).floor().clamp(0.0, original.0 as f64) as u32;
    let top = (y * sy).floor().clamp(0.0, original.1 as f64) as u32;
    let right = ((x + w) * sx).ceil().clamp(0.0, original.0 as f64) as u32;
    let bottom = ((y + h) * sy).ceil().clamp(0.0, original.1 as f64) as u32;
    if left >= right || top >= bottom {
        None
    } else {
        Some((left, top, right - left, bottom - top))
    }
}
#[cfg(not(windows))]
pub fn recognize(_: &Page, _: &str, _: &AtomicBool) -> Result<(Vec<OcrWord>, String), String> {
    Err("Windows OCR is available only on Windows".into())
}
#[cfg(windows)]
pub fn recognize(
    page: &Page,
    language: &str,
    cancel: &AtomicBool,
) -> Result<(Vec<OcrWord>, String), String> {
    use std::sync::atomic::Ordering;
    use windows::{
        core::HSTRING,
        Globalization::Language,
        Graphics::Imaging::{BitmapPixelFormat, SoftwareBitmap},
        Media::Ocr::OcrEngine,
        Storage::Streams::DataWriter,
        Win32::System::WinRT::{RoInitialize, RoUninitialize, RO_INIT_MULTITHREADED},
    };
    unsafe { RoInitialize(RO_INIT_MULTITHREADED) }
        .map_err(|e| format!("Windows OCR initialization failed: {e}"))?;
    struct Apartment;
    impl Drop for Apartment {
        fn drop(&mut self) {
            unsafe { RoUninitialize() }
        }
    }
    let _apartment = Apartment;
    let engine = if language == "auto" {
        OcrEngine::TryCreateFromUserProfileLanguages()
            .map_err(|_| "No Windows OCR language is installed for your user profile".to_string())?
    } else {
        let lang = Language::CreateLanguage(&HSTRING::from(language))
            .map_err(|_| "Invalid Windows language tag (for example en-US)".to_string())?;
        if !OcrEngine::IsLanguageSupported(&lang).map_err(|e| e.to_string())? {
            return Err(format!("Windows OCR language {language} is not installed"));
        }
        OcrEngine::TryCreateFromLanguage(&lang)
            .map_err(|e| format!("Windows OCR language unavailable: {e}"))?
    };
    let actual = engine
        .RecognizerLanguage()
        .and_then(|l| l.LanguageTag())
        .map_err(|e| e.to_string())?
        .to_string();
    let max = OcrEngine::MaxImageDimension().map_err(|e| e.to_string())?;
    let (width, height) =
        fit_dimensions(page.width, page.height, max).ok_or("Invalid Windows OCR image limit")?;
    if cancel.load(Ordering::Relaxed) {
        return Err("Windows OCR cancelled".into());
    }
    let mut image =
        img42_core::ImageDocument::from_rgba(page.width, page.height, page.pixels.to_vec(), None)
            .map_err(|e| e.to_string())?;
    if (width, height) != (page.width, page.height) {
        image
            .resize_quality(width, height, "lanczos")
            .map_err(|e| e.to_string())?;
    }
    if cancel.load(Ordering::Relaxed) {
        return Err("Windows OCR cancelled".into());
    }
    let mut bgra = Vec::with_capacity(image.pixels().len());
    for p in image.pixels().chunks_exact(4) {
        let a = u32::from(p[3]);
        for &i in &[2usize, 1, 0] {
            bgra.push(((u32::from(p[i]) * a + 255 * (255 - a) + 127) / 255) as u8);
        }
        bgra.push(255);
    }
    if cancel.load(Ordering::Relaxed) {
        return Err("Windows OCR cancelled".into());
    }
    let writer = DataWriter::new().map_err(|e| e.to_string())?;
    writer.WriteBytes(&bgra).map_err(|e| e.to_string())?;
    let buffer = writer.DetachBuffer().map_err(|e| e.to_string())?;
    let bitmap = SoftwareBitmap::CreateCopyFromBuffer(
        &buffer,
        BitmapPixelFormat::Bgra8,
        width as i32,
        height as i32,
    )
    .map_err(|e| e.to_string())?;
    let operation = engine.RecognizeAsync(&bitmap).map_err(|e| e.to_string())?;
    let start = std::time::Instant::now();
    loop {
        let status = operation.Status().map_err(|e| e.to_string())?;
        if status.0 != 0 {
            break;
        }
        if cancel.load(Ordering::Relaxed) || start.elapsed() >= std::time::Duration::from_secs(60) {
            let _ = operation.Cancel();
            return Err("Windows OCR cancelled or timed out".into());
        }
        std::thread::sleep(std::time::Duration::from_millis(30));
    }
    let result = operation
        .join()
        .map_err(|e| format!("Windows OCR failed: {e}"))?;
    if cancel.load(Ordering::Relaxed) {
        return Err("Windows OCR cancelled".into());
    }
    let mut words = Vec::new();
    for line in result.Lines().map_err(|e| e.to_string())? {
        for word in line.Words().map_err(|e| e.to_string())? {
            let r = word.BoundingRect().map_err(|e| e.to_string())?;
            if !r.X.is_finite() || !r.Y.is_finite() || !r.Width.is_finite() || !r.Height.is_finite()
            {
                return Err("Invalid Windows OCR geometry".into());
            }
            let Some((x, y, word_width, word_height)) = map_box(
                [r.X as f64, r.Y as f64, r.Width as f64, r.Height as f64],
                (width, height),
                (page.width, page.height),
            ) else {
                continue;
            };
            let text = word.Text().map_err(|e| e.to_string())?.to_string();
            if text.is_empty() {
                continue;
            }
            words.push(OcrWord {
                text,
                x,
                y,
                width: word_width,
                height: word_height,
            });
            if words.len() > 100_000 {
                return Err("OCR word limit exceeded".into());
            }
        }
    }
    Ok((words, actual))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fit_and_map_preserve_original_geometry() {
        assert_eq!(fit_dimensions(2480, 3508, 2600), Some((1838, 2600)));
        assert_eq!(fit_dimensions(100, 200, 2600), Some((100, 200)));
        assert_eq!(fit_dimensions(0, 200, 2600), None);
        let b = map_box([100.25, 200.5, 80.0, 50.0], (1838, 2600), (2480, 3508)).unwrap();
        assert_eq!(b, (135, 270, 109, 68));
        assert_eq!(
            map_box([-10.0, -5.0, 20.0, 10.0], (100, 100), (200, 300)),
            Some((0, 0, 20, 15))
        );
        assert!(map_box([f64::NAN, 0.0, 1.0, 1.0], (1, 1), (1, 1)).is_none());
    }
}
