//! PDF page recognition pipeline: render each page, recognize it with the shell's OCR engine
//! and map words back to PDF coordinates. Rendering and recognition are passed in by the shell.
use crate::platform::OcrImage;
use pdf42_core::ocr::{Page, Text};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
/// Rendering resolution for a page: the requested DPI, reduced to fit the engine's image limits.
pub fn dpi(bounds: [f64; 4], requested: u32) -> Result<u32, String> {
    if !(36..=300).contains(&requested) {
        return Err("OCR resolution must be 36–300 DPI".into());
    }
    let (w, h) = (bounds[2] - bounds[0], bounds[3] - bounds[1]);
    if !w.is_finite() || !h.is_finite() || w <= 0.0 || h <= 0.0 {
        return Err("Invalid OCR page geometry".into());
    }
    let fit = (72.0 * (3_900_000.0 / (w * h)).sqrt())
        .min(72.0 * 9900.0 / w.max(h))
        .floor();
    if fit < 36.0 {
        return Err("Page exceeds OCR image limits at the minimum resolution".into());
    }
    Ok(requested.min(fit as u32))
}
pub fn run<R, O>(
    pages: &[(usize, [f64; 4])],
    language: &str,
    resolution: u32,
    cancel: &AtomicBool,
    progress: &AtomicUsize,
    mut render: R,
    mut recognize: O,
) -> Result<Vec<Page>, String>
where
    R: FnMut(usize, u32) -> Result<(u32, u32, Vec<u8>), String>,
    O: FnMut(
        &OcrImage<'_>,
        &str,
        &AtomicBool,
    ) -> Result<(Vec<suite42_core::ocr::Word>, String), String>,
{
    let mut output = Vec::new();
    let (mut words_total, mut bytes_total) = (0usize, 0usize);
    for &(index, bounds) in pages {
        if cancel.load(Ordering::Relaxed) {
            return Err("Recognition cancelled; no pages changed".into());
        }
        let resolution = dpi(bounds, resolution)?;
        let (width, height, pixels) = render(index, resolution)
            .map_err(|e| format!("Page {}: {e}; no pages changed", index + 1))?;
        if width == 0
            || height == 0
            || u64::from(width) * u64::from(height) > 4_000_000
            || pixels.len() as u64 != u64::from(width) * u64::from(height) * 4
        {
            return Err("Invalid OCR page image".into());
        }
        if cancel.load(Ordering::Relaxed) {
            return Err("Recognition cancelled; no pages changed".into());
        }
        let (words, actual) = recognize(
            &OcrImage {
                width,
                height,
                pixels: &pixels,
            },
            language,
            cancel,
        )
        .map_err(|e| format!("Page {}: {e}; no pages changed", index + 1))?;
        let mut texts = Vec::new();
        for word in words {
            words_total = words_total
                .checked_add(1)
                .filter(|n| *n <= 100_000)
                .ok_or("OCR exceeds its word limit; recognize fewer pages")?;
            bytes_total = bytes_total
                .checked_add(word.text.len())
                .filter(|n| *n <= 16 * 1024 * 1024)
                .ok_or("OCR exceeds its text limit; recognize fewer pages")?;
            if word.width == 0
                || word.height == 0
                || word.x.checked_add(word.width).is_none_or(|x| x > width)
                || word.y.checked_add(word.height).is_none_or(|y| y > height)
            {
                return Err("Invalid OCR word geometry".into());
            }
            let sx = (bounds[2] - bounds[0]) / f64::from(width);
            let sy = (bounds[3] - bounds[1]) / f64::from(height);
            texts.push(Text {
                text: word.text,
                rect: [
                    bounds[0] + f64::from(word.x) * sx,
                    bounds[3] - f64::from(word.y + word.height) * sy,
                    bounds[0] + f64::from(word.x + word.width) * sx,
                    bounds[3] - f64::from(word.y) * sy,
                ],
            });
        }
        if cancel.load(Ordering::Relaxed) {
            return Err("Recognition cancelled; no pages changed".into());
        }
        output.push(Page {
            index,
            language: actual,
            words: texts,
        });
        progress.fetch_add(1, Ordering::Relaxed);
    }
    Ok(output)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unrotated_media_coordinates_scaling_cancel_and_errors_are_atomic() {
        let cancel = AtomicBool::new(false);
        let progress = AtomicUsize::new(0);
        let pages = [(2, [10., 20., 210., 320.])];
        let result = run(
            &pages,
            "auto",
            180,
            &cancel,
            &progress,
            |_, dpi| {
                assert!(dpi <= 180);
                Ok((100, 150, vec![255; 100 * 150 * 4]))
            },
            |_, _, _| {
                Ok((
                    vec![suite42_core::ocr::Word {
                        text: "Hello".into(),
                        x: 10,
                        y: 15,
                        width: 20,
                        height: 10,
                    }],
                    "en-US".into(),
                ))
            },
        )
        .unwrap();
        assert_eq!(result[0].index, 2);
        assert_eq!(result[0].words[0].rect, [30., 270., 70., 290.]);
        assert_eq!(progress.load(Ordering::Relaxed), 1);
        let mut calls = 0;
        let result = run(
            &[(0, [0., 0., 100., 100.]), (1, [0., 0., 100., 100.])],
            "auto",
            180,
            &cancel,
            &AtomicUsize::new(0),
            |_, _| Ok((1, 1, vec![255; 4])),
            |_, _, _| {
                calls += 1;
                if calls == 2 {
                    Err("engine error".into())
                } else {
                    Ok((Vec::new(), "en-US".into()))
                }
            },
        );
        assert!(result.is_err());
        assert_eq!(calls, 2);
        cancel.store(true, Ordering::Relaxed);
        assert!(run(
            &pages,
            "auto",
            180,
            &cancel,
            &AtomicUsize::new(0),
            |_, _| panic!("render after cancellation"),
            |_, _, _| panic!()
        )
        .is_err());
        assert_eq!(dpi([0., 0., 600., 800.], 300).unwrap(), 205);
        assert!(dpi([0., 0., 1e8, 1e8], 180).is_err());
    }
}

