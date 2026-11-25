use std::{
    collections::HashSet,
    io::Write,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc, Arc,
    },
};
pub fn parse_pages(value: &str, count: usize) -> Result<Vec<usize>, String> {
    if value.len() > 4096 || count == 0 || count > 1000 {
        return Err("Invalid page selection".into());
    }
    let mut selected = HashSet::new();
    let mut output = vec![];
    let number = |s: &str| {
        s.trim()
            .parse::<usize>()
            .ok()
            .filter(|n| (1..=count).contains(n))
            .ok_or("Page numbers must be within the current document")
    };
    for item in value.split(',') {
        let (first, last) = if let Some((a, b)) = item.split_once('-') {
            (number(a)?, number(b)?)
        } else {
            let n = number(item)?;
            (n, n)
        };
        let mut index = first;
        loop {
            if !selected.insert(index) {
                return Err("A page may appear only once; remove overlapping ranges".into());
            }
            output.push(index - 1);
            if index == last {
                break;
            }
            if first < last {
                index += 1;
            } else {
                index -= 1;
            }
        }
    }
    Ok(output)
}
pub struct Worker {
    receiver: mpsc::Receiver<Result<(), String>>,
    pub progress: Arc<AtomicUsize>,
    pub cancel: Arc<AtomicBool>,
    pub total: usize,
    reported_progress: usize,
}
impl Drop for Worker {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}
impl Worker {
    pub fn progress_changed(&mut self) -> bool {
        let value = self.progress.load(Ordering::Relaxed);
        let changed = self.reported_progress != value;
        self.reported_progress = value;
        changed
    }
    pub fn poll(&self) -> Option<Result<(), String>> {
        match self.receiver.try_recv() {
            Ok(value) => Some(value),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => Some(Err("PDF export worker stopped".into())),
        }
    }
    pub fn start(
        prepared: pdf42_core::PageExtraction,
        indices: Vec<usize>,
        path: PathBuf,
        split: bool,
    ) -> Result<Self, String> {
        let (sender, receiver) = mpsc::channel();
        let progress = Arc::new(AtomicUsize::new(0));
        let cancel = Arc::new(AtomicBool::new(false));
        let total = indices.len();
        let (worker_progress, worker_cancel) = (progress.clone(), cancel.clone());
        std::thread::Builder::new()
            .name("off42-pdf-page-export".into())
            .spawn(move || {
                let result = export(
                    &prepared,
                    &indices,
                    &path,
                    split,
                    &worker_progress,
                    &worker_cancel,
                );
                let _ = sender.send(result);
            })
            .map_err(|e| e.to_string())?;
        Ok(Self {
            receiver,
            progress,
            cancel,
            total,
            reported_progress: 0,
        })
    }
}
fn export(
    prepared: &pdf42_core::PageExtraction,
    indices: &[usize],
    destination: &Path,
    split: bool,
    progress: &AtomicUsize,
    cancel: &AtomicBool,
) -> Result<(), String> {
    let check = || {
        if cancel.load(Ordering::Relaxed) {
            Err("PDF page export cancelled".to_string())
        } else {
            Ok(())
        }
    };
    check()?;
    if indices.is_empty() || indices.len() > 1000 {
        return Err("Choose a valid page selection".into());
    }
    let folder = destination
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let mut temporary = tempfile::NamedTempFile::new_in(
        suite42_core::win32_path(folder).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    if split {
        let mut zip = zip::ZipWriter::new(temporary.as_file_mut());
        let mut size = 0usize;
        for (position, &index) in indices.iter().enumerate() {
            check()?;
            let data = prepared.extract(&[index])?;
            size = size
                .checked_add(data.len())
                .filter(|n| *n <= 512 * 1024 * 1024)
                .ok_or("Split output exceeds 512 MB; export fewer pages")?;
            zip.start_file(
                format!("{:04}-page-{:04}.pdf", position + 1, index + 1),
                zip::write::SimpleFileOptions::default()
                    .compression_method(zip::CompressionMethod::Deflated),
            )
            .map_err(|e| e.to_string())?;
            zip.write_all(&data).map_err(|e| e.to_string())?;
            progress.store(position + 1, Ordering::Relaxed);
        }
        zip.finish().map_err(|e| e.to_string())?;
    } else {
        let data = prepared.extract(indices)?;
        check()?;
        temporary.write_all(&data).map_err(|e| e.to_string())?;
        progress.store(indices.len(), Ordering::Relaxed);
    }
    check()?;
    temporary.flush().map_err(|e| e.to_string())?;
    temporary.as_file().sync_all().map_err(|e| e.to_string())?;
    // No partial destination and no overwrite, including a race after the picker.
    temporary
        .persist_noclobber(suite42_core::win32_path(destination).map_err(|e| e.to_string())?)
        .map_err(|e| e.error.to_string())?;
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn page_lists_retain_requested_order_and_reject_duplicates_and_bad_bounds() {
        assert_eq!(parse_pages(" 5-3,1 ", 5).unwrap(), vec![4, 3, 2, 0]);
        for invalid in [
            "", "0", "6", "1,1", "1-3,2", "1-0", "1-2-3", "1,,2", "1.5", "-1",
        ] {
            assert!(parse_pages(invalid, 5).is_err(), "{invalid}");
        }
    }
    #[test]
    fn split_snapshot_preserves_pages_and_refuses_overwrite_or_cancelled_publication() {
        use std::io::{self, Read};
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("source.pdf");
        let mut scan = scn42_core::ScanDocument::new();
        scan.add_rgba(20, 10, vec![255; 20 * 10 * 4]).unwrap();
        scan.add_rgba(10, 20, vec![255; 20 * 10 * 4]).unwrap();
        scan.export_pdf(&source).unwrap();
        let source_bytes = std::fs::read(&source).unwrap();
        let pdf = pdf42_core::PdfDocument::open(&source).unwrap();
        let prepared = pdf.prepare_page_extraction().unwrap();
        let progress = AtomicUsize::new(0);
        let cancel = AtomicBool::new(false);
        let destination = directory.path().join("split.zip");
        export(&prepared, &[1, 0], &destination, true, &progress, &cancel).unwrap();
        let saved = std::fs::read(&destination).unwrap();
        let mut archive = zip::ZipArchive::new(io::Cursor::new(&saved)).unwrap();
        assert_eq!(archive.len(), 2);
        for (index, filename) in [(1, "0001-page-0002.pdf"), (0, "0002-page-0001.pdf")] {
            let mut data = vec![];
            archive
                .by_name(filename)
                .unwrap()
                .read_to_end(&mut data)
                .unwrap();
            let output = lopdf::Document::load_mem(&data).unwrap();
            assert_eq!(output.get_pages().len(), 1);
            let reopened = pdf42_core::PdfDocument::probe_open_bytes(&data).unwrap();
            let pdf42_core::PdfOpenProbe::Ready(reopened) = reopened else {
                panic!("unexpected password");
            };
            assert_eq!(reopened.page_box(0).unwrap(), pdf.page_box(index).unwrap());
        }
        assert_eq!(progress.load(Ordering::Relaxed), 2);
        assert!(export(&prepared, &[0], &destination, true, &progress, &cancel).is_err());
        assert_eq!(std::fs::read(&destination).unwrap(), saved);
        let cancelled = directory.path().join("cancelled.zip");
        cancel.store(true, Ordering::Relaxed);
        assert!(export(&prepared, &[0], &cancelled, true, &progress, &cancel).is_err());
        assert!(!cancelled.exists());
        assert_eq!(std::fs::read(&source).unwrap(), source_bytes);
        assert!(std::fs::read_dir(directory.path())
            .unwrap()
            .all(|entry| matches!(
                entry.unwrap().file_name().to_str(),
                Some("source.pdf" | "split.zip")
            )));
    }
}
