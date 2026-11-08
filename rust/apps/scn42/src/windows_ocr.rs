//! Scanner adapter for the shared native Windows recognizer.
use scn42_core::{OcrWord, Page};
use std::sync::atomic::AtomicBool;
pub fn recognize(
    page: &Page,
    language: &str,
    cancel: &AtomicBool,
) -> Result<(Vec<OcrWord>, String), String> {
    win42::ocr::recognize(
        &win42::ocr::Image {
            width: page.width,
            height: page.height,
            pixels: &page.pixels,
        },
        language,
        cancel,
    )
}
