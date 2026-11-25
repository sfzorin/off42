#![cfg_attr(windows, windows_subsystem = "windows")]
mod ocr_batch;
mod ocr_settings;
mod pdf_import;
mod wia;
mod windows_ocr;
use scn42_core::ScanDocument;
use std::path::Path;
struct Scn {
    pending: Option<(u64, scn42_core::SaveState)>,
    next_save: u64,
    doc: ScanDocument,
    index: usize,
    window: usize,
    generation: u64,
    scan: Option<std::sync::mpsc::Receiver<wia::ResultMessage>>,
    scan_status: String,
    pdf_import: Option<(
        pdf_import::Receiver,
        std::sync::Arc<std::sync::atomic::AtomicBool>,
        std::sync::Arc<std::sync::atomic::AtomicUsize>,
    )>,
    pdf_first: u32,
    pdf_last: u32,
    pdf_dpi: u32,
    jpeg_quality: u8,
    compress_pdf: bool,
    tesseract_path: String,
    ocr_language: String,
    language_discovery: Option<std::sync::mpsc::Receiver<Result<String, String>>>,
    print_pending: Option<std::sync::mpsc::Receiver<(u64, Result<win42::PrintJob, String>)>>,
    ocr_displayed: usize,
    ocr: Option<(
        ocr_batch::Receiver,
        std::sync::Arc<std::sync::atomic::AtomicBool>,
        std::sync::Arc<std::sync::atomic::AtomicUsize>,
        usize,
    )>,
}
impl Scn {
    fn invalidate_ocr(&self) {
        if let Some((_, cancel, _, _)) = &self.ocr {
            cancel.store(true, std::sync::atomic::Ordering::Relaxed);
        }
    }
}
impl win42::App for Scn {
    fn take_pending_print_job(&mut self) -> Option<Result<win42::PrintJob, String>> {
        let result = self
            .print_pending
            .as_ref()
            .and_then(|rx| rx.try_recv().ok());
        if let Some((generation, result)) = result {
            self.print_pending = None;
            if generation == self.generation {
                Some(result)
            } else {
                Some(Err("Print cancelled because the project changed".into()))
            }
        } else {
            None
        }
    }
    fn set_native_window(&mut self, window: usize) {
        self.window = window;
    }
    fn poll_background(&mut self) -> bool {
        let import_result = self
            .pdf_import
            .as_ref()
            .and_then(|(rx, _, _)| rx.try_recv().ok());
        if let Some((generation, result)) = import_result {
            self.pdf_import = None;
            self.scan_status = if generation != self.generation {
                "PDF import discarded because the project changed".into()
            } else {
                match result.and_then(|pages| self.doc.add_pages_with_dpi(pages)) {
                    Ok(()) => {
                        self.invalidate_ocr();
                        self.generation = self.generation.wrapping_add(1);
                        self.index = self.doc.pages().len().saturating_sub(1);
                        "PDF pages imported".into()
                    }
                    Err(e) => e,
                }
            };
            return true;
        }
        if let Some(result) = self
            .language_discovery
            .as_ref()
            .and_then(|r| r.try_recv().ok())
        {
            self.language_discovery = None;
            self.scan_status = result.unwrap_or_else(|e| e);
            return true;
        }
        let ocr_result = self
            .ocr
            .as_ref()
            .and_then(|(rx, _, _, _)| match rx.try_recv() {
                Ok(v) => Some(v),
                Err(std::sync::mpsc::TryRecvError::Empty) => None,
                Err(_) => Some((
                    self.generation,
                    Err("OCR worker stopped; no pages changed".into()),
                )),
            });
        if let Some((generation, result)) = ocr_result {
            let cancelled = self
                .ocr
                .as_ref()
                .is_some_and(|(_, flag, _, _)| flag.load(std::sync::atomic::Ordering::Relaxed));
            self.ocr = None;
            self.scan_status = if cancelled {
                "Recognition cancelled; no pages changed".into()
            } else if generation != self.generation {
                "OCR result discarded because the project changed".into()
            } else {
                match result.and_then(|batch| self.doc.apply_ocr_batch(batch)) {
                    Ok(()) => {
                        self.invalidate_ocr();
                        self.generation = self.generation.wrapping_add(1);
                        "Recognition complete".into()
                    }
                    Err(e) => e,
                }
            };
            return true;
        }
        if let Some((_, _, progress, _)) = &self.ocr {
            let current = progress.load(std::sync::atomic::Ordering::Relaxed);
            if current != self.ocr_displayed {
                self.ocr_displayed = current;
                return true;
            }
        }
        let result = self.scan.as_ref().and_then(|rx| match rx.try_recv() {
            Ok(v) => Some(v),
            Err(std::sync::mpsc::TryRecvError::Empty) => None,
            Err(_) => Some((self.generation, Err("Scanner worker stopped".into()))),
        });
        let Some((generation, result)) = result else {
            return false;
        };
        self.scan = None;
        if generation != self.generation {
            self.scan_status = "Scan result discarded because the project changed".into();
            return true;
        }
        match result {
            Ok(pages) if pages.is_empty() => {
                self.scan_status = "Scan cancelled or no pages acquired".into()
            }
            Ok(pages) => match self.doc.add_pages_with_dpi(pages) {
                Ok(()) => {
                    self.invalidate_ocr();
                    self.generation = self.generation.wrapping_add(1);
                    self.index = self.doc.pages().len() - 1;
                    self.scan_status = "Pages acquired".into();
                }
                Err(e) => self.scan_status = e,
            },
            Err(e) => self.scan_status = e,
        }
        true
    }
    fn save_formats(&self) -> Vec<win42::FileFormat> {
        vec![win42::FileFormat {
            id: "scn42",
            title: "SCN42 project",
            extensions: &["scn42"],
        }]
    }
    fn save_snapshot(&mut self, format: &str) -> Result<Option<(u64, Vec<u8>)>, String> {
        if format != "scn42" {
            return Err("Unsupported save format".into());
        }
        let (bytes, state) = (&self.doc).save_snapshot();
        self.next_save = self.next_save.wrapping_add(1);
        let revision = self.next_save;
        self.pending = Some((revision, state));
        Ok(Some((revision, bytes)))
    }
    fn saved_snapshot(&mut self, revision: u64) {
        if self.pending.as_ref().is_some_and(|(r, _)| *r == revision) {
            if let Some((_, state)) = self.pending.take() {
                if let Some(doc) = Some(&mut self.doc) {
                    doc.mark_saved_snapshot(state);
                }
            }
        }
    }

    fn name(&self) -> &'static str {
        "SCN42"
    }
    fn extension(&self) -> &'static str {
        "scn42"
    }
    fn format_name(&self) -> &'static str {
        "SCN42 project"
    }
    fn open_filter(&self) -> String {
        "*.scn42".into()
    }
    fn supports_new(&self) -> bool {
        true
    }
    fn new_document(&mut self) -> Result<(), String> {
        self.invalidate_ocr();
        self.generation = self.generation.wrapping_add(1);
        self.doc = ScanDocument::new();
        self.index = 0;
        Ok(())
    }
    fn file_actions(&self) -> &'static [win42::FileAction] {
        &[
            win42::FileAction {
                id: 33,
                title: "Import PDF pages...",
                save: false,
                extension: "pdf",
                format: "PDF",
                pattern: "*.pdf",
            },
            win42::FileAction {
                id: 20,
                title: "&Import Image (convert to 8-bit)...",
                save: false,
                extension: "png",
                format: "PNG / JPEG / BMP / TIFF / GIF / WebP / SVG image",
                pattern: "*.png;*.jpg;*.jpeg;*.bmp;*.tif;*.tiff;*.gif;*.webp;*.svg",
            },
            win42::FileAction {
                id: 22,
                title: "Export &Recognized Text...",
                save: true,
                extension: "txt",
                format: "UTF-8 recognized text",
                pattern: "*.txt",
            },
            win42::FileAction {
                id: 30,
                title: "Export all pages as TIFF...",
                save: true,
                extension: "tif",
                format: "Multi-page TIFF",
                pattern: "*.tif;*.tiff",
            },
            win42::FileAction {
                id: 31,
                title: "Export current page as PNG...",
                save: true,
                extension: "png",
                format: "PNG image",
                pattern: "*.png",
            },
            win42::FileAction {
                id: 32,
                title: "Export current page as JPEG...",
                save: true,
                extension: "jpg",
                format: "JPEG image",
                pattern: "*.jpg;*.jpeg",
            },
            win42::FileAction {
                id: 21,
                title: "&Export PDF...",
                save: true,
                extension: "pdf",
                format: "PDF",
                pattern: "*.pdf",
            },
        ]
    }
    fn file_action(&mut self, id: u16, p: &Path) -> Result<(), String> {
        if id == 20 {
            self.invalidate_ocr();
            self.generation = self.generation.wrapping_add(1);
        }
        match id {
            33 => {
                if self.pdf_import.is_some() {
                    return Err("PDF import is already running".into());
                }
                let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
                let progress = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
                let rx = pdf_import::start(
                    p.to_path_buf(),
                    self.generation,
                    self.pdf_first,
                    self.pdf_last,
                    self.pdf_dpi,
                    cancel.clone(),
                    progress.clone(),
                );
                self.pdf_import = Some((rx, cancel, progress));
                self.scan_status = "Importing PDF pages…".into();
                Ok(())
            }
            20 => {
                let (images, _) =
                    img42_core::ImageDocument::open_frames(p).map_err(|e| e.to_string())?;
                let dpi =
                    img42_core::ImageDocument::source_page_dpi(p).map_err(|e| e.to_string())?;
                self.doc.add_pages_with_dpi(
                    images
                        .iter()
                        .enumerate()
                        .map(|(i, img)| {
                            let (x, y) = dpi.get(i).copied().flatten().unwrap_or((150, 150));
                            (img.width(), img.height(), img.pixels().to_vec(), x, y)
                        })
                        .collect(),
                )?;
                self.index = self.doc.pages().len() - 1;
                Ok(())
            }
            30 | 31 | 32 => {
                let pages: Vec<_> = if id == 30 {
                    self.doc.pages().iter().collect()
                } else {
                    vec![self.doc.pages().get(self.index).ok_or("No page")?]
                };
                if pages.is_empty() {
                    return Err("No pages to export".into());
                }
                let images: Vec<_> = pages
                    .iter()
                    .map(|page| {
                        img42_core::ImageDocument::from_rgba(
                            page.width,
                            page.height,
                            page.pixels.to_vec(),
                            Some((page.dpi_x, page.dpi_y)),
                        )
                        .map_err(|e| e.to_string())
                    })
                    .collect::<Result<_, _>>()?;
                let bytes = if id == 30 {
                    let refs: Vec<_> = images.iter().collect();
                    img42_core::ImageDocument::save_tiff_pages(&refs)
                        .map_err(|e| e.to_string())?
                        .0
                } else {
                    images[0]
                        .save_snapshot_format_quality(
                            if id == 31 { "png" } else { "jpeg" },
                            self.jpeg_quality,
                        )
                        .map_err(|e| e.to_string())?
                        .0
                };
                use std::io::Write;
                let mut file = std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(p)
                    .map_err(|e| e.to_string())?;
                file.write_all(&bytes).map_err(|e| e.to_string())
            }
            21 => self.doc.export_pdf_compressed(p, self.compress_pdf),
            22 => {
                use std::io::Write;
                let text = self.doc.recognized_text();
                let mut file = std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(p)
                    .map_err(|e| e.to_string())?;
                file.write_all(text.as_bytes()).map_err(|e| e.to_string())
            }
            _ => Err("Unknown file action".into()),
        }
    }
    fn actions(&self) -> &'static [(u16, &'static str)] {
        &[
            (10, "Rotate &Right"),
            (11, "&Delete Page"),
            (12, "Move Page &Up"),
            (13, "Move Page D&own"),
            (14, "Flip &Horizontal"),
            (15, "Flip &Vertical"),
            (16, "Convert to &Grayscale"),
            (17, "Clean &Black and White"),
            (18, "&Crop Page..."),
            (19, "From &Scanner..."),
            (23, "Recognize &Text..."),
            (24, "Cancel Recognition"),
            (37, "Recognize page range..."),
            (38, "Recognize using Windows OCR..."),
            (25, "Find Installed OCR Languages..."),
            (26, "Page Resolution..."),
            (27, "Print all pages..."),
            (28, "Deskew page..."),
            (29, "Perspective correction..."),
            (34, "PDF import settings..."),
            (35, "Cancel PDF import"),
            (36, "Export quality settings..."),
        ]
    }
    fn input_request(&self, id: u16) -> Option<win42::InputRequest> {
        if id == 36 {
            return Some(win42::InputRequest {
                title: "Export quality and compression",
                fields: vec![
                    ("JPEG quality (1–100)", self.jpeg_quality.to_string()),
                    (
                        "Compress PDF streams (1 yes, 0 no)",
                        (self.compress_pdf as u8).to_string(),
                    ),
                ],
            });
        }
        if id == 34 {
            return Some(win42::InputRequest {
                title: "PDF import page range and DPI",
                fields: vec![
                    ("First page (1-based)", self.pdf_first.to_string()),
                    ("Last page (0 = all)", self.pdf_last.to_string()),
                    ("Raster DPI (36–600)", self.pdf_dpi.to_string()),
                ],
            });
        }
        if id == 25 {
            return Some(win42::InputRequest {
                title: "Find installed Tesseract languages",
                fields: vec![("Tesseract executable path", self.tesseract_path.clone())],
            });
        }
        if id == 38 {
            return Some(win42::InputRequest {
                title: "Windows OCR (large pages are scaled; installed language required)",
                fields: vec![
                    ("Language tag (auto, en-US, ru-RU...)", "auto".into()),
                    ("First page (1-based)", "1".into()),
                    ("Last page (0 = all)", "0".into()),
                ],
            });
        }
        if id == 37 {
            return Some(win42::InputRequest {
                title: "Recognize page range (all-or-nothing)",
                fields: vec![
                    ("Tesseract executable", self.tesseract_path.clone()),
                    ("Language codes (e.g. eng+rus)", self.ocr_language.clone()),
                    ("First page (1-based)", "1".into()),
                    ("Last page (0 = all)", "0".into()),
                ],
            });
        }
        if id == 23 {
            return Some(win42::InputRequest {
                title: "Recognize text (Tesseract language codes)",
                fields: vec![
                    (
                        "Tesseract executable path (install separately)",
                        self.tesseract_path.clone(),
                    ),
                    (
                        "Installed language codes (e.g. eng+rus)",
                        self.ocr_language.clone(),
                    ),
                ],
            });
        }
        let p = self.doc.pages().get(self.index)?;
        if id == 28 {
            return Some(win42::InputRequest {
                title: "Deskew page (-45 to 45 degrees)",
                fields: vec![("Angle (degrees)", "0".into())],
            });
        }
        if id == 29 {
            let (w, h) = (p.width.saturating_sub(1), p.height.saturating_sub(1));
            return Some(win42::InputRequest {
                title: "Perspective: clockwise source corners",
                fields: vec![
                    ("Top left X", "0".into()),
                    ("Top left Y", "0".into()),
                    ("Top right X", w.to_string()),
                    ("Top right Y", "0".into()),
                    ("Bottom right X", w.to_string()),
                    ("Bottom right Y", h.to_string()),
                    ("Bottom left X", "0".into()),
                    ("Bottom left Y", h.to_string()),
                    ("Output width", p.width.to_string()),
                    ("Output height", p.height.to_string()),
                ],
            });
        }
        if id == 26 {
            return Some(win42::InputRequest {
                title: "Physical page resolution (default 150 DPI)",
                fields: vec![
                    ("Horizontal DPI", p.dpi_x.to_string()),
                    ("Vertical DPI", p.dpi_y.to_string()),
                ],
            });
        }
        (id == 18).then(|| win42::InputRequest {
            title: "Crop page",
            fields: vec![
                ("Left", "0".into()),
                ("Top", "0".into()),
                ("Width", p.width.to_string()),
                ("Height", p.height.to_string()),
            ],
        })
    }
    fn input_action(&mut self, id: u16, values: &[String]) -> Result<(), String> {
        if id == 36 {
            let nums: Vec<u32> = values
                .iter()
                .map(|v| v.parse::<u32>().map_err(|_| "Enter integers"))
                .collect::<Result<_, _>>()?;
            if let [q, c] = nums.as_slice() {
                if (1..=100).contains(q) && *c <= 1 {
                    self.jpeg_quality = *q as u8;
                    self.compress_pdf = *c == 1;
                    return Ok(());
                }
            }
            return Err("Invalid export settings".into());
        }
        if id == 34 {
            let nums: Vec<u32> = values
                .iter()
                .map(|v| v.parse::<u32>().map_err(|_| "Enter nonnegative integers"))
                .collect::<Result<_, _>>()?;
            if let [first, last, dpi] = nums.as_slice() {
                if *first > 0 && (*last == 0 || last >= first) && (36..=600).contains(dpi) {
                    self.pdf_first = *first;
                    self.pdf_last = *last;
                    self.pdf_dpi = *dpi;
                    return Ok(());
                }
            }
            return Err("Invalid range or DPI".into());
        }
        if id == 25 {
            let executable = values.first().ok_or("Missing executable path")?.clone();
            if executable.trim().is_empty() {
                return Err("Enter the installed Tesseract executable path".into());
            }
            self.tesseract_path = executable.clone();
            let (tx, rx) = std::sync::mpsc::channel();
            std::thread::spawn(move || {
                use std::process::{Command, Stdio};
                let result = (|| -> Result<String, String> {
                    let mut child = Command::new(&executable)
                        .arg("--list-langs")
                        .stdout(Stdio::piped())
                        .stderr(Stdio::null())
                        .spawn()
                        .map_err(|e| format!("Cannot start {executable}: {e}"))?;
                    let start = std::time::Instant::now();
                    loop {
                        if child.try_wait().map_err(|e| e.to_string())?.is_some() {
                            break;
                        }
                        if start.elapsed() > std::time::Duration::from_secs(10) {
                            let _ = child.kill();
                            let _ = child.wait();
                            return Err("Language discovery timed out".into());
                        }
                        std::thread::sleep(std::time::Duration::from_millis(50));
                    }
                    let out = child.wait_with_output().map_err(|e| e.to_string())?;
                    if !out.status.success() {
                        return Err("Tesseract could not list installed languages".into());
                    }
                    Ok(format!(
                        "Installed OCR languages: {}",
                        String::from_utf8_lossy(&out.stdout)
                            .lines()
                            .skip(1)
                            .collect::<Vec<_>>()
                            .join(", ")
                    ))
                })();
                let _ = tx.send(result);
            });
            self.language_discovery = Some(rx);
            self.scan_status = "Finding installed OCR languages…".into();
            return Ok(());
        }
        if id == 23 || id == 37 || id == 38 {
            if self.ocr.is_some() {
                return Err("Recognition already running".into());
            }
            let use_windows = id == 38;
            let executable = if use_windows {
                String::new()
            } else {
                values.first().ok_or("Missing executable path")?.clone()
            };
            let language = values
                .get(if use_windows { 0 } else { 1 })
                .ok_or("Missing language")?
                .clone();
            let (first, last) = if id == 23 {
                (self.index, self.index + 1)
            } else {
                let first = values
                    .get(if use_windows { 1 } else { 2 })
                    .ok_or("Missing first page")?
                    .parse::<usize>()
                    .map_err(|_| "Invalid first page")?;
                let last = values
                    .get(if use_windows { 2 } else { 3 })
                    .ok_or("Missing last page")?
                    .parse::<usize>()
                    .map_err(|_| "Invalid last page")?;
                if first == 0
                    || first > self.doc.pages().len()
                    || (last != 0 && (last < first || last > self.doc.pages().len()))
                {
                    return Err("Invalid page range".into());
                }
                (
                    first - 1,
                    if last == 0 {
                        self.doc.pages().len()
                    } else {
                        last
                    },
                )
            };
            if first >= last || last > self.doc.pages().len() {
                return Err("No pages selected".into());
            }
            if !use_windows {
                ocr_settings::save(&ocr_settings::Settings {
                    executable: executable.clone(),
                    language: language.clone(),
                })
                .map_err(|e| format!("Cannot save OCR settings: {e}"))?;
                self.tesseract_path = executable.clone();
                self.ocr_language = language.clone();
            }
            let pages = self.doc.pages()[first..last].to_vec();
            let total = pages.len();
            let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
            let progress = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
            let rx = ocr_batch::start(
                self.generation,
                pages,
                executable,
                language,
                use_windows,
                cancel.clone(),
                progress.clone(),
            );
            self.ocr_displayed = 0;
            self.ocr = Some((rx, cancel, progress, total));
            self.scan_status = "Recognizing text…".into();
            return Ok(());
        }
        if id == 28 {
            let angle = values
                .first()
                .ok_or("Missing angle")?
                .parse::<f64>()
                .map_err(|_| "Invalid angle")?;
            self.invalidate_ocr();
            self.generation = self.generation.wrapping_add(1);
            return self.doc.deskew(self.index, angle);
        }
        self.invalidate_ocr();
        self.generation = self.generation.wrapping_add(1);
        let n: Vec<u32> = values
            .iter()
            .map(|v| {
                v.parse::<u32>()
                    .map_err(|_| "Enter nonnegative integers".to_string())
            })
            .collect::<Result<_, _>>()?;
        match (id, n.as_slice()) {
            (18, [x, y, w, h]) => self.doc.crop(self.index, *x, *y, *w, *h),
            (26, [x, y]) => self.doc.set_dpi(self.index, *x, *y),
            (29, [x1, y1, x2, y2, x3, y3, x4, y4, w, h]) => self.doc.perspective(
                self.index,
                [
                    (*x1 as f64, *y1 as f64),
                    (*x2 as f64, *y2 as f64),
                    (*x3 as f64, *y3 as f64),
                    (*x4 as f64, *y4 as f64),
                ],
                *w,
                *h,
            ),
            _ => Err("Invalid crop request".into()),
        }
    }
    fn items(&self) -> Vec<String> {
        self.doc
            .pages()
            .iter()
            .enumerate()
            .map(|(i, p)| format!("Page {} — {} × {}", i + 1, p.width, p.height))
            .collect()
    }
    fn can_save(&self) -> bool {
        true
    }
    fn selected(&self) -> usize {
        self.index
    }
    fn text(&self) -> String {
        "Import PNG, JPEG or BMP images to add pages. Use From Scanner on Windows or import images. Text export uses an installed Tesseract executable.".into()
    }
    fn text_readonly(&self) -> bool {
        true
    }
    fn image(&self) -> Option<(u32, u32, Vec<u8>)> {
        self.doc
            .pages()
            .get(self.index)
            .map(|p| (p.width, p.height, p.pixels.to_vec()))
    }
    fn status(&self) -> String {
        let progress = self
            .pdf_import
            .as_ref()
            .map(|(_, _, p)| {
                format!(
                    " | {} PDF pages rendered",
                    p.load(std::sync::atomic::Ordering::Relaxed)
                )
            })
            .unwrap_or_default();
        let ocr_progress = self
            .ocr
            .as_ref()
            .map(|(_, _, p, total)| {
                format!(
                    " | OCR {}/{} pages",
                    p.load(std::sync::atomic::Ordering::Relaxed),
                    total
                )
            })
            .unwrap_or_default();
        format!(
            "{} pages | {}{}{}",
            self.doc.pages().len(),
            self.scan_status,
            progress,
            ocr_progress
        )
    }
    fn dirty(&self) -> bool {
        self.doc.is_dirty()
    }
    fn can_undo(&self) -> bool {
        self.doc.can_undo()
    }
    fn can_redo(&self) -> bool {
        self.doc.can_redo()
    }
    fn enabled(&self, id: u16) -> bool {
        let n = self.doc.pages().len();
        match id {
            10 | 11 | 14 | 15 | 16 | 17 | 18 | 21 | 22 => n > 0,
            19 => self.scan.is_none(),
            23 | 37 => n > 0 && self.ocr.is_none(),
            38 => cfg!(windows) && n > 0 && self.ocr.is_none(),
            24 => self.ocr.is_some(),
            25 => self.language_discovery.is_none(),
            26 | 28 | 29 | 30 | 31 | 32 => n > 0,
            27 => n > 0 && self.print_pending.is_none(),
            33 => self.pdf_import.is_none(),
            35 => self.pdf_import.is_some(),
            12 => n > 0 && self.index > 0,
            13 => self.index + 1 < n,
            _ => true,
        }
    }
    fn open(&mut self, p: &Path) -> Result<(), String> {
        let doc = ScanDocument::open_project(p)?;
        self.invalidate_ocr();
        self.generation = self.generation.wrapping_add(1);
        self.doc = doc;
        self.index = 0;
        Ok(())
    }
    fn save(&mut self, p: &Path) -> Result<(), String> {
        self.doc.save_project(p)
    }
    fn edit(&mut self, _: String) -> Result<(), String> {
        Ok(())
    }
    fn select(&mut self, i: usize) {
        self.index = i
    }
    fn action(&mut self, id: u16) -> Result<(), String> {
        if id >= 10 && !self.enabled(id) {
            return Err("Command unavailable".into());
        }
        if matches!(id, 4 | 5 | 10..=17) {
            self.invalidate_ocr();
            self.generation = self.generation.wrapping_add(1);
        }
        match id {
            35 => {
                if let Some((_, cancel, _)) = &self.pdf_import {
                    cancel.store(true, std::sync::atomic::Ordering::Relaxed);
                }
                self.invalidate_ocr();
                self.generation = self.generation.wrapping_add(1);
                self.scan_status = "Cancelling PDF import…".into();
            }
            4 => {
                self.doc.undo();
            }
            5 => {
                self.doc.redo();
            }
            10 => self.doc.rotate(self.index)?,
            11 => self.doc.delete(self.index)?,
            14 => self.doc.flip(self.index, true)?,
            15 => self.doc.flip(self.index, false)?,
            16 => self.doc.document_cleanup(self.index, None)?,
            17 => self.doc.document_cleanup(self.index, Some(128))?,
            27 => {
                let pages = self.doc.pages().to_vec();
                let generation = self.generation;
                let (tx, rx) = std::sync::mpsc::channel();
                std::thread::spawn(move || {
                    let result = (|| -> Result<win42::PrintJob, String> {
                        let mut out = Vec::new();
                        for page in pages {
                            let width = page.width as f64 * 72.0 / page.dpi_x as f64;
                            let height = page.height as f64 * 72.0 / page.dpi_y as f64;
                            if width < 1.0 || height < 1.0 || width > 4096.0 || height > 4096.0 {
                                return Err("Page size is outside printable limits".into());
                            }
                            let geometry = win42::PrintGeometry {
                                width_twips: (width * 20.0) as i32,
                                height_twips: (height * 20.0) as i32,
                                margin_left_twips: 0,
                                margin_top_twips: 0,
                                margin_right_twips: 0,
                                margin_bottom_twips: 0,
                            };
                            let scene = win42::CanvasScene {
                                viewport: win42::Viewport::default(),
                                items: vec![win42::SceneItem::ImageScaled {
                                    x: 0.0,
                                    y: 0.0,
                                    width,
                                    height,
                                    pixel_width: page.width,
                                    pixel_height: page.height,
                                    rgba: page.pixels.to_vec(),
                                }],
                            };
                            out.push(win42::PrintPage { geometry, scene });
                        }
                        Ok(win42::PrintJob::Scenes(out))
                    })();
                    let _ = tx.send((generation, result));
                });
                self.print_pending = Some(rx);
            }
            24 => {
                self.invalidate_ocr();
                self.generation = self.generation.wrapping_add(1);
                if let Some((_, cancel, _, _)) = &self.ocr {
                    cancel.store(true, std::sync::atomic::Ordering::Relaxed);
                }
                self.scan_status = "Cancelling recognition…".into();
            }
            19 => {
                self.invalidate_ocr();
                self.generation = self.generation.wrapping_add(1);
                let (byte_budget, page_budget) = self.doc.acquisition_capacity();
                if byte_budget == 0 || page_budget == 0 {
                    return Err("Document is at the acquisition limit".into());
                }
                self.scan = Some(wia::start(
                    self.generation,
                    self.window,
                    byte_budget,
                    page_budget,
                ));
                self.scan_status = "Scanner dialog open; cancel in that dialog".into();
            }
            12 if self.index > 0 => {
                self.doc.move_page(self.index, self.index - 1)?;
                self.index -= 1
            }
            13 if self.index + 1 < self.doc.pages().len() => {
                self.doc.move_page(self.index, self.index + 1)?;
                self.index += 1
            }
            _ => {}
        }
        self.index = self.index.min(self.doc.pages().len().saturating_sub(1));
        Ok(())
    }
}
fn main() {
    let settings = ocr_settings::load();
    win42::run(Box::new(Scn {
        pending: None,
        next_save: 0,
        doc: ScanDocument::new(),
        index: 0,
        window: 0,
        generation: 0,
        scan: None,
        scan_status: String::new(),
        pdf_import: None,
        pdf_first: 1,
        pdf_last: 0,
        pdf_dpi: 150,
        jpeg_quality: 90,
        compress_pdf: true,
        tesseract_path: settings.executable,
        ocr_language: settings.language,
        language_discovery: None,
        print_pending: None,
        ocr_displayed: 0,
        ocr: None,
    }))
}
#[cfg(test)]
mod tests {
    use super::*;
    use win42::App;
    #[test]
    fn new_and_open_cancel_stale_batch() {
        for reopen in [false, true] {
            let mut app = Scn {
                pending: None,
                next_save: 0,
                doc: ScanDocument::new(),
                index: 0,
                window: 0,
                generation: 1,
                scan: None,
                scan_status: String::new(),
                pdf_import: None,
                pdf_first: 1,
                pdf_last: 0,
                pdf_dpi: 150,
                jpeg_quality: 90,
                compress_pdf: true,
                tesseract_path: "tesseract".into(),
                ocr_language: "eng".into(),
                language_discovery: None,
                print_pending: None,
                ocr_displayed: 0,
                ocr: None,
            };
            app.doc.add_rgba(1, 1, vec![255; 4]).unwrap();
            let source = app.doc.pages()[0].clone();
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("source.scn42");
            app.doc.save_project(&path).unwrap();
            let (tx, rx) = std::sync::mpsc::channel();
            let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
            app.ocr = Some((
                rx,
                cancel.clone(),
                std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0)),
                1,
            ));
            if reopen {
                app.open(&path).unwrap();
            } else {
                app.new_document().unwrap();
            }
            assert!(cancel.load(std::sync::atomic::Ordering::Relaxed));
            let words = vec![scn42_core::OcrWord {
                text: "late".into(),
                x: 0,
                y: 0,
                width: 1,
                height: 1,
            }];
            tx.send((1, Ok(vec![(source, words, "eng".into())])))
                .unwrap();
            assert!(app.poll_background());
            assert!(app.doc.pages().iter().all(|p| p.words.is_empty()));
        }
    }
    #[test]
    fn deleted_page_cancels_running_batch_and_discards_all_results() {
        let mut app = Scn {
            pending: None,
            next_save: 0,
            doc: ScanDocument::new(),
            index: 0,
            window: 0,
            generation: 1,
            scan: None,
            scan_status: String::new(),
            pdf_import: None,
            pdf_first: 1,
            pdf_last: 0,
            pdf_dpi: 150,
            jpeg_quality: 90,
            compress_pdf: true,
            tesseract_path: "tesseract".into(),
            ocr_language: "eng".into(),
            language_discovery: None,
            print_pending: None,
            ocr_displayed: 0,
            ocr: None,
        };
        app.doc.add_rgba(1, 1, vec![255; 4]).unwrap();
        app.doc.add_rgba(1, 1, vec![250; 4]).unwrap();
        let old = app.doc.pages().to_vec();
        let (tx, rx) = std::sync::mpsc::channel();
        let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let progress = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(1));
        app.ocr = Some((rx, cancel.clone(), progress, 2));
        assert!(app.poll_background());
        assert!(app.status().contains("OCR 1/2 pages"));
        app.action(11).unwrap();
        assert!(cancel.load(std::sync::atomic::Ordering::Relaxed));
        let words = vec![scn42_core::OcrWord {
            text: "late".into(),
            x: 0,
            y: 0,
            width: 1,
            height: 1,
        }];
        tx.send((
            1,
            Ok(vec![
                (old[0].clone(), words.clone(), "eng".into()),
                (old[1].clone(), words, "eng".into()),
            ]),
        ))
        .unwrap();
        assert!(app.poll_background());
        assert_eq!(app.doc.pages().len(), 1);
        assert!(app.doc.pages()[0].words.is_empty());
    }
    #[test]
    fn adapter_project_and_export_are_distinct() {
        let dir = std::env::temp_dir().join(format!("scn42-adapter-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir(&dir).unwrap();
        let mut app = Scn {
            pending: None,
            next_save: 0,
            doc: ScanDocument::new(),
            index: 0,
            window: 0,
            generation: 0,
            scan: None,
            scan_status: String::new(),
            pdf_import: None,
            pdf_first: 1,
            pdf_last: 0,
            pdf_dpi: 150,
            jpeg_quality: 90,
            compress_pdf: true,
            tesseract_path: "tesseract".into(),
            ocr_language: "eng".into(),
            language_discovery: None,
            print_pending: None,
            ocr_displayed: 0,
            ocr: None,
        };
        app.doc
            .add_rgba(2, 1, vec![1, 2, 3, 4, 5, 6, 7, 8])
            .unwrap();
        app.action(14).unwrap();
        let pixels = app.doc.pages()[0].pixels.clone();
        app.file_action(21, &dir.join("out.pdf")).unwrap();
        assert!(app.dirty());
        app.save(&dir.join("project.scn42")).unwrap();
        assert!(!app.dirty());
        app.action(11).unwrap();
        assert!(app.dirty());
        app.open(&dir.join("project.scn42")).unwrap();
        assert!(!app.dirty());
        assert_eq!(app.doc.pages()[0].pixels, pixels);
        let before = app.doc.pages().to_vec();
        std::fs::write(dir.join("bad.scn42"), b"broken").unwrap();
        assert!(app.open(&dir.join("bad.scn42")).is_err());
        assert_eq!(app.doc.pages(), before);
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn stale_acquisition_does_not_mutate_new_project() {
        let mut a = Scn {
            pending: None,
            next_save: 0,
            doc: ScanDocument::new(),
            index: 0,
            window: 0,
            generation: 1,
            scan: None,
            scan_status: String::new(),
            pdf_import: None,
            pdf_first: 1,
            pdf_last: 0,
            pdf_dpi: 150,
            jpeg_quality: 90,
            compress_pdf: true,
            tesseract_path: "tesseract".into(),
            ocr_language: "eng".into(),
            language_discovery: None,
            print_pending: None,
            ocr_displayed: 0,
            ocr: None,
        };
        let (tx, rx) = std::sync::mpsc::channel();
        a.scan = Some(rx);
        a.new_document().unwrap();
        tx.send((1, Ok(vec![(1, 1, vec![0, 0, 0, 255], 150, 150)])))
            .unwrap();
        assert!(a.poll_background());
        assert!(a.doc.pages().is_empty());
    }
    #[test]
    fn stale_pdf_import_cannot_modify_new_project() {
        let mut app = Scn {
            pending: None,
            next_save: 0,
            doc: ScanDocument::new(),
            index: 0,
            window: 0,
            generation: 1,
            scan: None,
            scan_status: String::new(),
            pdf_import: None,
            pdf_first: 1,
            pdf_last: 0,
            pdf_dpi: 150,
            jpeg_quality: 90,
            compress_pdf: true,
            tesseract_path: "tesseract".into(),
            ocr_language: "eng".into(),
            language_discovery: None,
            print_pending: None,
            ocr_displayed: 0,
            ocr: None,
        };
        let (tx, rx) = std::sync::mpsc::channel();
        let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let progress = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        app.pdf_import = Some((rx, cancel, progress));
        app.new_document().unwrap();
        tx.send((1, Ok(vec![(1, 1, vec![1, 2, 3, 255], 150, 150)])))
            .unwrap();
        assert!(app.poll_background());
        assert!(app.doc.pages().is_empty());
    }
    #[test]
    fn boundary_commands_match_actions() {
        let mut a = Scn {
            pending: None,
            next_save: 0,
            doc: ScanDocument::new(),
            index: 0,
            window: 0,
            generation: 0,
            scan: None,
            scan_status: String::new(),
            pdf_import: None,
            pdf_first: 1,
            pdf_last: 0,
            pdf_dpi: 150,
            jpeg_quality: 90,
            compress_pdf: true,
            tesseract_path: "tesseract".into(),
            ocr_language: "eng".into(),
            language_discovery: None,
            print_pending: None,
            ocr_displayed: 0,
            ocr: None,
        };
        for id in 10..=13 {
            assert!(!a.enabled(id));
            assert!(a.action(id).is_err());
        }
        a.doc.add_rgba(1, 1, vec![255, 0, 0, 255]).unwrap();
        assert!(a.enabled(10));
        assert!(a.enabled(11));
        assert!(!a.enabled(12));
        assert!(!a.enabled(13));
        assert!(a.action(12).is_err());
        a.doc.add_rgba(1, 1, vec![0, 0, 0, 255]).unwrap();
        assert!(a.enabled(13));
        a.action(13).unwrap();
        assert!(a.enabled(12));
        assert!(!a.enabled(13));
    }
}
