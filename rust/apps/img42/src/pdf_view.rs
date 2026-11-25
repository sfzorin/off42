//! Cancellable PDF preparation through the shared Windows renderer.
use img42_core::ImageDocument;
use std::sync::{
    atomic::{AtomicBool, AtomicUsize},
    mpsc, Arc,
};
use suite42_core::session::FileIdentity;
pub struct Prepared {
    pub path: std::path::PathBuf,
    pub identity: FileIdentity,
    pub docs: Vec<ImageDocument>,
}
pub type Receiver = mpsc::Receiver<Result<Prepared, String>>;
pub fn start(
    path: std::path::PathBuf,
    cancel: Arc<AtomicBool>,
    progress: Arc<AtomicUsize>,
) -> Receiver {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let result = prepare(path, &cancel, &progress);
        let _ = tx.send(result);
    });
    rx
}
#[cfg(not(windows))]
fn prepare(_: std::path::PathBuf, _: &AtomicBool, _: &AtomicUsize) -> Result<Prepared, String> {
    Err("PDF viewing requires the Windows PDF renderer".into())
}
#[cfg(windows)]
fn prepare(
    path: std::path::PathBuf,
    cancel: &AtomicBool,
    progress: &AtomicUsize,
) -> Result<Prepared, String> {
    use std::io::Read;
    use std::sync::atomic::Ordering;
    let mut file = std::fs::File::open(&path).map_err(|e| e.to_string())?;
    let metadata = file.metadata().map_err(|e| e.to_string())?;
    if metadata.len() > 64 * 1024 * 1024 {
        return Err("PDF exceeds 64 MB limit".into());
    }
    let mut bytes = Vec::new();
    (&mut file)
        .take(64 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    let after = file.metadata().map_err(|e| e.to_string())?;
    if bytes.len() > 64 * 1024 * 1024 {
        return Err("PDF exceeds 64 MB limit".into());
    }
    if bytes.len() as u64 != metadata.len()
        || metadata.len() != after.len()
        || metadata.modified().ok() != after.modified().ok()
    {
        return Err("PDF changed during open; try again".into());
    }
    let before = FileIdentity::from_bytes(&bytes, after.modified().ok());
    let pdf = suite42_pdf_raster::Session::open(&bytes)?;
    let count = pdf.page_count()?;
    if count == 0 || count > 32 {
        return Err("PDF viewing supports 1–32 pages; use SCN42 to import a page range".into());
    }
    let mut docs = Vec::new();
    let mut total = 0usize;
    for page in 0..count {
        if cancel.load(Ordering::Relaxed) {
            return Err("PDF opening cancelled".into());
        }
        let (w, h, pixels) = pdf.render_page(page, 96)?;
        total = total.checked_add(pixels.len()).ok_or("PDF is too large")?;
        if total > 64 * 1024 * 1024 {
            return Err("PDF viewing exceeds 64 MB raster limit; use SCN42 to select pages".into());
        }
        docs.push(
            ImageDocument::from_rgba(w, h, pixels, Some((96, 96))).map_err(|e| e.to_string())?,
        );
        progress.fetch_add(1, Ordering::Relaxed);
    }
    if cancel.load(Ordering::Relaxed) {
        return Err("PDF opening cancelled".into());
    }
    Ok(Prepared {
        path,
        identity: before,
        docs,
    })
}
