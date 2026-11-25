//! Sequential all-or-nothing OCR jobs. Partial results never reach the document.
use scn42_core::{OcrWord, Page, ScanDocument};
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    mpsc, Arc,
};
pub type ResultPages = Vec<(Page, Vec<OcrWord>, String)>;
pub type Receiver = mpsc::Receiver<(u64, Result<ResultPages, String>)>;
pub fn run<F>(
    pages: Vec<Page>,
    language: &str,
    cancel: &AtomicBool,
    progress: &AtomicUsize,
    recognize: F,
) -> Result<ResultPages, String>
where
    F: FnMut(&Page, &str, &AtomicBool) -> Result<(Vec<OcrWord>, String), String>,
{
    run_with_budget(
        pages,
        language,
        cancel,
        progress,
        64 * 1024 * 1024,
        recognize,
    )
}
fn run_with_budget<F>(
    pages: Vec<Page>,
    language: &str,
    cancel: &AtomicBool,
    progress: &AtomicUsize,
    budget: usize,
    mut recognize: F,
) -> Result<ResultPages, String>
where
    F: FnMut(&Page, &str, &AtomicBool) -> Result<(Vec<OcrWord>, String), String>,
{
    if pages.is_empty() {
        return Err("No pages selected".into());
    }
    let mut out = Vec::with_capacity(pages.len());
    let mut retained = 0usize;
    for (i, page) in pages.into_iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            return Err("Recognition cancelled; no pages changed".into());
        }
        let (words, actual_language) = recognize(&page, language, cancel)
            .map_err(|e| format!("Selected page {}: {e}; no pages changed", i + 1))?;
        if cancel.load(Ordering::Relaxed) {
            return Err("Recognition cancelled; no pages changed".into());
        }
        let mut page_bytes = words
            .capacity()
            .checked_mul(std::mem::size_of::<OcrWord>())
            .and_then(|n| n.checked_add(actual_language.capacity()))
            .ok_or("OCR output exceeds batch memory limit; no pages changed")?;
        for word in &words {
            page_bytes = page_bytes
                .checked_add(word.text.capacity())
                .ok_or("OCR output exceeds batch memory limit; no pages changed")?;
        }
        retained = retained
            .checked_add(page_bytes)
            .ok_or("OCR output exceeds batch memory limit; no pages changed")?;
        if retained > budget {
            return Err("OCR output exceeds batch memory limit; no pages changed".into());
        }
        out.push((page, words, actual_language));
        progress.fetch_add(1, Ordering::Relaxed);
    }
    Ok(out)
}
pub fn start(
    generation: u64,
    pages: Vec<Page>,
    executable: String,
    language: String,
    use_windows: bool,
    cancel: Arc<AtomicBool>,
    progress: Arc<AtomicUsize>,
) -> Receiver {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let result = run(pages, &language, &cancel, &progress, |p, l, c| {
            if use_windows {
                crate::windows_ocr::recognize(p, l, c)
            } else {
                ScanDocument::recognize_page(p, &executable, l, c)
                    .map(|words| (words, l.to_owned()))
            }
        });
        let _ = tx.send((generation, result));
    });
    rx
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn batch_budget_stops_before_following_page_without_partial_commit() {
        let mut doc = ScanDocument::new();
        for _ in 0..3 {
            doc.add_rgba(1, 1, vec![255; 4]).unwrap();
        }
        let before = doc.pages().to_vec();
        let undo = doc.can_undo();
        let redo = doc.can_redo();
        let dirty = doc.is_dirty();
        let cancel = AtomicBool::new(false);
        let progress = AtomicUsize::new(0);
        let mut calls = 0;
        let one = std::mem::size_of::<OcrWord>() + 16 + 3;
        let result = run_with_budget(
            doc.pages().to_vec(),
            "eng",
            &cancel,
            &progress,
            one + 8,
            |_, _, _| {
                calls += 1;
                Ok((
                    vec![OcrWord {
                        text: "x".repeat(16),
                        x: 0,
                        y: 0,
                        width: 1,
                        height: 1,
                    }],
                    "eng".into(),
                ))
            },
        );
        assert!(result.unwrap_err().contains("batch memory limit"));
        assert_eq!(calls, 2);
        assert_eq!(progress.load(Ordering::Relaxed), 1);
        assert_eq!(doc.pages(), before);
        assert_eq!(doc.can_undo(), undo);
        assert_eq!(doc.can_redo(), redo);
        assert_eq!(doc.is_dirty(), dirty);
    }
    #[test]
    #[ignore = "requires SCN42_REAL_OCR_RGBA and locally installed Tesseract with English data"]
    fn real_tesseract_batch_words_geometry_and_history() {
        let path = std::env::var("SCN42_REAL_OCR_RGBA").expect("path to 900x220 RGBA fixture");
        let pixels = std::fs::read(path).unwrap();
        let mut doc = ScanDocument::new();
        doc.add_rgba(900, 220, pixels.clone()).unwrap();
        doc.add_rgba(900, 220, pixels).unwrap();
        let cancel = AtomicBool::new(false);
        let progress = AtomicUsize::new(0);
        let batch = run(
            doc.pages().to_vec(),
            "eng",
            &cancel,
            &progress,
            |p, l, c| {
                ScanDocument::recognize_page(p, "tesseract", l, c)
                    .map(|words| (words, l.to_owned()))
            },
        )
        .unwrap();
        assert_eq!(progress.load(Ordering::Relaxed), 2);
        for (_, words, _) in &batch {
            assert!(
                words
                    .iter()
                    .any(|w| w.text == "HELLO" && w.width > 0 && w.height > 0),
                "{words:?}"
            );
        }
        doc.apply_ocr_batch(batch).unwrap();
        assert!(doc
            .pages()
            .iter()
            .all(|p| p.ocr_language == "eng" && !p.words.is_empty()));
        assert!(doc.undo());
        assert!(doc.pages().iter().all(|p| p.words.is_empty()));
        assert!(doc.redo());
    }
    #[test]
    fn sequential_cancel_and_failure_never_return_partial_results() {
        let mut doc = ScanDocument::new();
        for _ in 0..3 {
            doc.add_rgba(1, 1, vec![255; 4]).unwrap();
        }
        let cancel = AtomicBool::new(false);
        let progress = AtomicUsize::new(0);
        let mut calls = 0;
        let result = run(
            doc.pages().to_vec(),
            "eng",
            &cancel,
            &progress,
            |_, _, c| {
                calls += 1;
                if calls == 2 {
                    c.store(true, Ordering::Relaxed);
                }
                Ok((vec![], "eng".into()))
            },
        );
        assert!(result.is_err());
        assert_eq!(calls, 2);
        assert_eq!(progress.load(Ordering::Relaxed), 1);
        cancel.store(false, Ordering::Relaxed);
        progress.store(0, Ordering::Relaxed);
        calls = 0;
        let result = run(
            doc.pages().to_vec(),
            "eng",
            &cancel,
            &progress,
            |_, _, _| {
                calls += 1;
                if calls == 2 {
                    Err("backend failure".into())
                } else {
                    Ok((vec![], "eng".into()))
                }
            },
        );
        assert!(result.unwrap_err().contains("page 2"));
        assert_eq!(calls, 2);
        assert_eq!(progress.load(Ordering::Relaxed), 1);
    }
}
