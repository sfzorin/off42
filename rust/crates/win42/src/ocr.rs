//! Offline recognition shared by PDF42 and SCN42. Native code is statically linked.
use suite42_core::ocr::Word as OcrWord;
#[derive(Clone, Copy)]
pub struct Image<'a> {
    pub width: u32,
    pub height: u32,
    pub pixels: &'a [u8],
}
#[cfg(not(windows))]
use std::sync::atomic::AtomicBool;
/// Fit a page into the engine's limit while retaining aspect ratio.
#[cfg(any(windows, test))]
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
#[cfg(any(windows, test))]
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
pub fn recognize(_: &Image<'_>, _: &str, _: &AtomicBool) -> Result<(Vec<OcrWord>, String), String> {
    Err("Built-in OCR is available only on Windows".into())
}
#[cfg(windows)]
mod native;
#[cfg(windows)]
pub use native::{installed_languages, recognize};

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

#[cfg(not(windows))]
pub fn installed_languages() -> Result<Vec<(String, String)>, String> {
    Err("Built-in OCR is available only on Windows".into())
}
