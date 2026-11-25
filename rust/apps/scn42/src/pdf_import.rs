//! Background PDF import; project updates happen only after the complete job succeeds.
use std::sync::{
    atomic::{AtomicBool, AtomicUsize},
    mpsc, Arc,
};
pub type Pages = Vec<(u32, u32, Vec<u8>, u32, u32)>;
pub type Receiver = mpsc::Receiver<(u64, Result<Pages, String>)>;
pub fn start(
    path: std::path::PathBuf,
    generation: u64,
    first: u32,
    last: u32,
    dpi: u32,
    cancel: Arc<AtomicBool>,
    progress: Arc<AtomicUsize>,
) -> Receiver {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let result = import(path, first, last, dpi, &cancel, &progress);
        let _ = tx.send((generation, result));
    });
    rx
}
#[cfg(not(windows))]
fn import(
    _: std::path::PathBuf,
    _: u32,
    _: u32,
    _: u32,
    _: &AtomicBool,
    _: &AtomicUsize,
) -> Result<Pages, String> {
    Err("PDF import requires the Windows PDF renderer".into())
}
#[cfg(windows)]
fn import(
    path: std::path::PathBuf,
    first: u32,
    last: u32,
    dpi: u32,
    cancel: &AtomicBool,
    progress: &AtomicUsize,
) -> Result<Pages, String> {
    use std::io::Read;
    use std::sync::atomic::Ordering;
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    if file.metadata().map_err(|e| e.to_string())?.len() > 64 * 1024 * 1024 {
        return Err("PDF exceeds 64 MB import limit".into());
    }
    let mut bytes = Vec::new();
    file.take(64 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > 64 * 1024 * 1024 {
        return Err("PDF exceeds 64 MB import limit".into());
    }
    let session = suite42_pdf_raster::Session::open(&bytes)?;
    let count = session.page_count()?;
    let end = if last == 0 { count } else { last };
    if first == 0 || end < first || end > count || end - first + 1 > 32 {
        return Err(format!(
            "Select 1–32 valid pages; this PDF contains {count} pages"
        ));
    }
    let mut pages = Vec::new();
    let mut total = 0usize;
    for page in first..=end {
        if cancel.load(Ordering::Relaxed) {
            return Err("PDF import cancelled".into());
        }
        let (w, h, pixels) = session.render_page(page - 1, dpi)?;
        total = total
            .checked_add(pixels.len())
            .ok_or("PDF import exceeds memory limit")?;
        if total > 16 * 1024 * 1024 {
            return Err(
                "PDF import exceeds 16 MB page limit; select fewer pages or lower DPI".into(),
            );
        }
        pages.push((w, h, pixels, dpi, dpi));
        progress.fetch_add(1, Ordering::Relaxed);
    }
    if cancel.load(Ordering::Relaxed) {
        return Err("PDF import cancelled".into());
    }
    Ok(pages)
}
