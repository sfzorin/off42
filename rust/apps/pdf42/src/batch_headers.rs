//! Copies selected PDFs with continuous Bates numbers. Each successful file is published atomically.
use pdf42_core::{decorations::HeaderFooter, PdfDocument, PdfOpenProbe};
use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc, Arc,
    },
};
const MAX_BYTES: u64 = 256 * 1024 * 1024;
const MAX_OUTPUT: usize = 512 * 1024 * 1024;
const MAX_PAGES: usize = 1000;
pub struct Entry {
    pub input: PathBuf,
    pub output: Option<PathBuf>,
    pub result: String,
    pub range: Option<(u64, u64)>,
}
pub struct Outcome {
    pub entries: Vec<Entry>,
    pub next: u64,
    pub cancelled: bool,
}
impl Outcome {
    pub fn report(&self) -> win42::ReportRequest {
        let written = self.entries.iter().filter(|e| e.output.is_some()).count();
        win42::ReportRequest {
            title: win42::localization::text("Batch Headers & Bates Numbers"),
            summary: format!(
                "{} / {} PDFs saved as copies. {} Next Bates number: {}.",
                written,
                self.entries.len(),
                if self.cancelled {
                    "Cancelled."
                } else {
                    "Finished."
                },
                self.next
            ),
            columns: vec!["File", "Result", "Bates numbers", "Output"],
            column_colors: vec![],
            rows: self
                .entries
                .iter()
                .map(|e| {
                    vec![
                        e.input
                            .file_name()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .into_owned(),
                        e.result.clone(),
                        e.range.map(|(a, b)| format!("{a}–{b}")).unwrap_or_default(),
                        e.output
                            .as_ref()
                            .map(|p| p.display().to_string())
                            .unwrap_or_default(),
                    ]
                })
                .collect(),
            highlight_controls: false,
            compact_first_column: false,
            allow_keep: false,
            keeping: false,
        }
    }
}
pub struct Worker {
    rx: mpsc::Receiver<Result<Outcome, String>>,
    pub cancel: Arc<AtomicBool>,
    pub progress: Arc<AtomicUsize>,
    pub total: usize,
    pub settings: HeaderFooter,
    shown: usize,
}
impl Drop for Worker {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}
impl Worker {
    pub fn start(
        mut paths: Vec<PathBuf>,
        folder: PathBuf,
        settings: HeaderFooter,
    ) -> Result<Self, String> {
        prepare(&mut paths, &folder)?;
        let (tx, rx) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let progress = Arc::new(AtomicUsize::new(0));
        let (token, count, spec) = (cancel.clone(), progress.clone(), settings.clone());
        let total = paths.len();
        std::thread::Builder::new()
            .name("off42-pdf-bates".into())
            .spawn(move || {
                let result = process(&paths, &folder, &spec, &count, &token);
                let _ = tx.send(Ok(result));
            })
            .map_err(|e| e.to_string())?;
        Ok(Self {
            rx,
            cancel,
            progress,
            total,
            settings,
            shown: 0,
        })
    }
    pub fn progress_changed(&mut self) -> bool {
        let n = self.progress.load(Ordering::Relaxed);
        let changed = n != self.shown;
        self.shown = n;
        changed
    }
    pub fn poll(&self) -> Option<Result<Outcome, String>> {
        match self.rx.try_recv() {
            Ok(r) => Some(r),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(_) => Some(Err("PDF batch worker stopped".into())),
        }
    }
}
fn prepare(paths: &mut [PathBuf], folder: &Path) -> Result<(), String> {
    if paths.is_empty() || paths.len() > 128 {
        return Err("Choose between 1 and 128 PDF files".into());
    }
    if !suite42_core::win32_path(folder)
        .map_err(|e| e.to_string())?
        .is_dir()
    {
        return Err("Choose an existing output folder".into());
    }
    paths.sort_by(|a, b| {
        win42::logical_file_order(
            a.file_name().unwrap_or_default(),
            b.file_name().unwrap_or_default(),
        )
        .then_with(|| a.cmp(b))
    });
    let mut names = std::collections::HashSet::new();
    for path in paths {
        let name = path.file_name().ok_or("Invalid PDF file name")?;
        if !names.insert(name.to_string_lossy().to_lowercase()) {
            return Err(
                "Selected PDFs have duplicate output names; choose files with different names"
                    .into(),
            );
        }
        if suite42_core::win32_path(&folder.join(name))
            .map_err(|e| e.to_string())?
            .symlink_metadata()
            .is_ok()
        {
            return Err(format!(
                "Output already exists: {}. Choose another folder.",
                name.to_string_lossy()
            ));
        }
    }
    Ok(())
}
fn process(
    paths: &[PathBuf],
    folder: &Path,
    settings: &HeaderFooter,
    progress: &AtomicUsize,
    cancel: &AtomicBool,
) -> Outcome {
    let (mut input_bytes, mut output_bytes, mut pages) = (0u64, 0usize, 0usize);
    let mut next = settings.bates_start;
    let mut entries = Vec::new();
    for path in paths {
        let result = (|| -> Result<(PathBuf, u64), String> {
            if cancel.load(Ordering::Relaxed) {
                return Err("Cancelled; no copy written".into());
            }
            let remaining = MAX_BYTES.saturating_sub(input_bytes);
            let mut bytes = Vec::new();
            let file =
                std::fs::File::open(suite42_core::win32_path(path).map_err(|e| e.to_string())?)
                    .map_err(|e| e.to_string())?;
            file.take(remaining + 1)
                .read_to_end(&mut bytes)
                .map_err(|e| e.to_string())?;
            input_bytes = input_bytes.saturating_add(bytes.len() as u64);
            if input_bytes > MAX_BYTES {
                return Err("Batch input exceeds 256 MiB".into());
            }
            if cancel.load(Ordering::Relaxed) {
                return Err("Cancelled; no copy written".into());
            }
            let mut doc = match PdfDocument::probe_open_bytes(&bytes)? {
                PdfOpenProbe::Ready(doc) => doc,
                PdfOpenProbe::PasswordRequired => {
                    return Err(
                        "Password-protected PDF; open and save an unlocked copy first".into(),
                    )
                }
            };
            pages = pages.saturating_add(doc.page_count());
            if pages > MAX_PAGES {
                return Err("Batch exceeds 1000 pages".into());
            }
            let mut spec = settings.clone();
            spec.bates_start = next;
            spec.file_name = path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
            let after = doc.add_header_footer(&(0..doc.page_count()).collect::<Vec<_>>(), &spec)?;
            if cancel.load(Ordering::Relaxed) {
                return Err("Cancelled; no copy written".into());
            }
            let (bytes, _) = doc.save_snapshot()?;
            output_bytes = output_bytes.saturating_add(bytes.len());
            if output_bytes > MAX_OUTPUT {
                return Err("Batch output exceeds 512 MiB".into());
            }
            let output = folder.join(path.file_name().ok_or("Invalid PDF file name")?);
            let mut temp = tempfile::NamedTempFile::new_in(
                suite42_core::win32_path(folder).map_err(|e| e.to_string())?,
            )
            .map_err(|e| e.to_string())?;
            temp.write_all(&bytes).map_err(|e| e.to_string())?;
            temp.flush().map_err(|e| e.to_string())?;
            temp.as_file().sync_all().map_err(|e| e.to_string())?;
            if cancel.load(Ordering::Relaxed) {
                return Err("Cancelled; no copy written".into());
            }
            temp.persist_noclobber(suite42_core::win32_path(&output).map_err(|e| e.to_string())?)
                .map_err(|e| e.error.to_string())?;
            Ok((output, after))
        })();
        let entry = match result {
            Ok((output, after)) => {
                let range = Some((next, after - 1));
                next = after;
                Entry {
                    input: path.clone(),
                    output: Some(output),
                    result: "Saved".into(),
                    range,
                }
            }
            Err(error) => Entry {
                input: path.clone(),
                output: None,
                result: error,
                range: None,
            },
        };
        entries.push(entry);
        progress.store(entries.len(), Ordering::Relaxed);
    }
    Outcome {
        entries,
        next,
        cancelled: cancel.load(Ordering::Relaxed),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn fixture(path: &Path, count: usize) {
        use lopdf::{dictionary, Document, Object};
        let mut d = Document::with_version("1.7");
        let pages = d.new_object_id();
        let kids=(0..count).map(|_|d.add_object(dictionary!{"Type"=>"Page","Parent"=>pages,"MediaBox"=>vec![0.into(),0.into(),300.into(),300.into()]}).into()).collect::<Vec<Object>>();
        d.objects.insert(
            pages,
            Object::Dictionary(dictionary! {"Type"=>"Pages","Kids"=>kids,"Count"=>count as i64}),
        );
        let root = d.add_object(dictionary! {"Type"=>"Catalog","Pages"=>pages});
        d.trailer.set("Root", root);
        d.save(path).unwrap();
    }
    fn extracted_text(doc: &lopdf::Document, number: u32) -> String {
        let page = doc.get_pages()[&number];
        let mut text = doc.extract_text(&[number]).unwrap();
        if let Ok(resources) = doc.get_dictionary(page).unwrap().get(b"Resources") {
            let resources = doc.dereference(resources).unwrap().1.as_dict().unwrap();
            if let Ok(objects) = resources.get(b"XObject") {
                for (_, value) in doc
                    .dereference(objects)
                    .unwrap()
                    .1
                    .as_dict()
                    .unwrap()
                    .iter()
                {
                    let id = value.as_reference().unwrap();
                    let form = doc.get_object(id).unwrap().as_stream().unwrap();
                    if form.dict.get(b"Subtype").unwrap().as_name().unwrap() != b"Form" {
                        continue;
                    }
                    let mut projected = doc.clone();
                    let page = projected.get_dictionary_mut(page).unwrap();
                    page.set("Contents", id);
                    page.set("Resources", form.dict.get(b"Resources").unwrap().clone());
                    text.push_str(&projected.extract_text(&[number]).unwrap());
                }
            }
        }
        text
    }
    #[test]
    fn natural_order_copies_continue_only_successful_numbers_and_preserve_sources() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("output");
        std::fs::create_dir(&out).unwrap();
        let a = dir.path().join("file2.pdf");
        let b = dir.path().join("file10.pdf");
        let bad = dir.path().join("file3.pdf");
        fixture(&a, 2);
        fixture(&b, 1);
        std::fs::write(&bad, b"broken").unwrap();
        let originals = [std::fs::read(&a).unwrap(), std::fs::read(&b).unwrap()];
        let mut paths = vec![b.clone(), bad, a.clone()];
        prepare(&mut paths, &out).unwrap();
        assert_eq!(paths[0], a);
        assert_eq!(paths[2], b);
        let mut settings = HeaderFooter::default();
        settings.texts[4] = "<<file>> <<bates>>".into();
        settings.bates_start = 42;
        let outcome = process(
            &paths,
            &out,
            &settings,
            &AtomicUsize::new(0),
            &AtomicBool::new(false),
        );
        assert_eq!(outcome.next, 45);
        assert_eq!(outcome.entries[0].range, Some((42, 43)));
        assert!(outcome.entries[1].output.is_none());
        assert_eq!(outcome.entries[2].range, Some((44, 44)));
        for (i, name) in ["file2.pdf", "file10.pdf"].iter().enumerate() {
            let d = lopdf::Document::load(out.join(name)).unwrap();
            let text = d
                .get_pages()
                .keys()
                .map(|n| extracted_text(&d, *n))
                .collect::<Vec<_>>()
                .join("\n");
            assert!(
                text.contains(if i == 0 { "000042" } else { "000044" }),
                "{text}"
            );
            assert!(text.contains(name));
        }
        assert_eq!(std::fs::read(&a).unwrap(), originals[0]);
        assert_eq!(std::fs::read(&b).unwrap(), originals[1]);
        assert!(prepare(&mut paths, &out).is_err());
    }
    #[test]
    fn cancel_and_output_races_do_not_consume_numbers_or_leave_temp_files() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("output");
        std::fs::create_dir(&out).unwrap();
        let a = dir.path().join("a.pdf");
        fixture(&a, 1);
        let paths = vec![a.clone()];
        let settings = HeaderFooter::default();
        let result = process(
            &paths,
            &out,
            &settings,
            &AtomicUsize::new(0),
            &AtomicBool::new(true),
        );
        assert!(result.cancelled);
        assert_eq!(result.next, 1);
        assert_eq!(std::fs::read_dir(&out).unwrap().count(), 0);
        std::fs::write(out.join("a.pdf"), b"newer document").unwrap();
        let result = process(
            &paths,
            &out,
            &settings,
            &AtomicUsize::new(0),
            &AtomicBool::new(false),
        );
        assert_eq!(result.next, 1);
        assert!(result.entries[0].output.is_none());
        assert_eq!(std::fs::read(out.join("a.pdf")).unwrap(), b"newer document");
        assert_eq!(std::fs::read_dir(&out).unwrap().count(), 1);
        let mut duplicate = vec![a, dir.path().join("elsewhere/a.pdf")];
        assert!(prepare(&mut duplicate, &out).is_err());
    }
}
