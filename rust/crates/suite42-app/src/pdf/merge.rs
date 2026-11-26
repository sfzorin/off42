//! Background merge publishes a new file only after all inputs and cancellation checks pass.
use std::{
    io::Write,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc, Arc,
    },
};
pub struct Worker {
    rx: mpsc::Receiver<Result<PathBuf, String>>,
    pub cancel: Arc<AtomicBool>,
    pub progress: Arc<AtomicUsize>,
    pub total: usize,
    shown: usize,
}
impl Drop for Worker {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}
impl Worker {
    pub fn start(paths: Vec<PathBuf>, output: PathBuf) -> Result<Self, String> {
        if paths.is_empty() || paths.len() > 128 {
            return Err("Choose between 1 and 128 PDF files".into());
        }
        let (tx, rx) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let progress = Arc::new(AtomicUsize::new(0));
        let (token, count) = (cancel.clone(), progress.clone());
        let total = paths.len();
        std::thread::Builder::new()
            .name("off42-pdf-merge".into())
            .spawn(move || {
                let result = publish(&paths, &output, &count, &token).map(|_| output);
                let _ = tx.send(result);
            })
            .map_err(|e| e.to_string())?;
        Ok(Self {
            rx,
            cancel,
            progress,
            total,
            shown: 0,
        })
    }
    pub fn progress_changed(&mut self) -> bool {
        let n = self.progress.load(Ordering::Relaxed);
        let changed = n != self.shown;
        self.shown = n;
        changed
    }
    pub fn poll(&self) -> Option<Result<PathBuf, String>> {
        match self.rx.try_recv() {
            Ok(r) => Some(r),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(_) => Some(Err("PDF merge worker stopped".into())),
        }
    }
}
fn publish(
    paths: &[PathBuf],
    output: &Path,
    progress: &AtomicUsize,
    cancel: &AtomicBool,
) -> Result<(), String> {
    if output
        .extension()
        .and_then(|e| e.to_str())
        .is_none_or(|e| !e.eq_ignore_ascii_case("pdf"))
    {
        return Err("Choose a file name ending in .pdf".into());
    }
    if output.exists() {
        return Err("The merge output already exists; choose a new file name".into());
    }
    let bytes = pdf42_core::merge::merge_files(paths, progress, cancel)?;
    let parent = output
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let mut temp = tempfile::NamedTempFile::new_in(
        suite42_core::win32_path(parent).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    temp.write_all(&bytes).map_err(|e| e.to_string())?;
    temp.flush().map_err(|e| e.to_string())?;
    temp.as_file().sync_all().map_err(|e| e.to_string())?;
    if cancel.load(Ordering::Relaxed) {
        return Err("PDF merge cancelled".into());
    }
    temp.persist_noclobber(suite42_core::win32_path(output).map_err(|e| e.to_string())?)
        .map_err(|e| e.error.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture(path: &Path) {
        use lopdf::{dictionary, Document, Object};
        let mut d = Document::with_version("1.7");
        let pages = d.new_object_id();
        let page=d.add_object(dictionary!{"Type"=>"Page","Parent"=>pages,"MediaBox"=>vec![0.into(),0.into(),100.into(),100.into()]});
        d.objects.insert(
            pages,
            Object::Dictionary(dictionary! {"Type"=>"Pages","Kids"=>vec![page.into()],"Count"=>1}),
        );
        let catalog = d.add_object(dictionary! {"Type"=>"Catalog","Pages"=>pages});
        d.trailer.set("Root", catalog);
        d.save(path).unwrap();
    }
    #[test]
    fn publication_is_no_clobber_and_cancel_or_failure_leave_no_partial_file() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.pdf");
        let b = dir.path().join("b.pdf");
        let broken = dir.path().join("broken.pdf");
        fixture(&a);
        fixture(&b);
        std::fs::write(&broken, b"broken").unwrap();
        let originals = (std::fs::read(&a).unwrap(), std::fs::read(&b).unwrap());
        let output = dir.path().join("merged.pdf");
        let progress = AtomicUsize::new(0);
        let cancel = AtomicBool::new(false);
        assert!(publish(&[a.clone(), broken], &output, &progress, &cancel).is_err());
        assert!(!output.exists());
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 3);
        assert!(publish(
            &[a.clone(), b.clone()],
            &output,
            &progress,
            &AtomicBool::new(true)
        )
        .is_err());
        assert!(!output.exists());
        publish(&[a.clone(), b.clone()], &output, &progress, &cancel).unwrap();
        assert_eq!(
            pdf42_core::PdfDocument::open(&output).unwrap().page_count(),
            2
        );
        let merged = std::fs::read(&output).unwrap();
        assert!(publish(&[a.clone()], &output, &progress, &cancel).is_err());
        assert_eq!(std::fs::read(&output).unwrap(), merged);
        assert_eq!(std::fs::read(a).unwrap(), originals.0);
        assert_eq!(std::fs::read(b).unwrap(), originals.1);
    }
}
