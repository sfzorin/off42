#![cfg_attr(windows, windows_subsystem = "windows")]
mod batch;
mod pdf_view;
use img42_core::ImageDocument;
use std::path::Path;
#[derive(Default)]
struct Img {
    batch: Option<batch::Worker>,
    batch_outcomes: Vec<String>,
    batch_done: Option<bool>,
    pending: Option<(u64, Vec<img42_core::SaveState>)>,
    next_save: u64,
    frames: Vec<Option<ImageDocument>>,
    delays: Vec<image::Delay>,
    animation_loop: img42_core::AnimationLoop,
    conversion_risk: bool,
    original_16bit: Vec<bool>,
    pending_16bit: Vec<bool>,
    print_pending: Option<std::sync::mpsc::Receiver<Result<win42::PrintJob, String>>>,
    pdf_pending: Option<(
        pdf_view::Receiver,
        std::sync::Arc<std::sync::atomic::AtomicBool>,
        std::sync::Arc<std::sync::atomic::AtomicUsize>,
    )>,
    pdf_prepared: Option<pdf_view::Prepared>,
    source_path: Option<std::path::PathBuf>,
    pending_open: Option<std::path::PathBuf>,
    pending_save_path: Option<std::path::PathBuf>,
    frame_index: usize,
    doc: Option<ImageDocument>,
    zoom: Option<f64>,
    tool: u8,
    gesture: Vec<(u32, u32)>,
    selection: Option<(u32, u32, u32, u32)>,
    mask: Option<Vec<bool>>,
    ink_color: [u8; 3],
    ink_width: u32,
}
impl Img {
    fn loses_precision(&self) -> bool {
        self.original_16bit.iter().enumerate().any(|(i, was_16)| {
            *was_16
                && !(if i == self.frame_index {
                    self.doc.as_ref()
                } else {
                    self.frames.get(i).and_then(Option::as_ref)
                })
                .is_some_and(ImageDocument::is_16bit)
        })
    }
}
impl win42::App for Img {
    fn poll_background(&mut self) -> bool {
        let mut changed = false;
        let mut done = None;
        if let Some(worker) = &self.batch {
            loop {
                match worker.rx.try_recv() {
                    Ok(batch::Event::Outcome(line)) => {
                        changed = true;
                        self.batch_outcomes.push(line);
                    }
                    Ok(batch::Event::Done(cancelled)) => {
                        changed = true;
                        done = Some(cancelled);
                        break;
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => break,
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        changed = true;
                        self.batch_outcomes
                            .push("Worker stopped unexpectedly".into());
                        done = Some(true);
                        break;
                    }
                }
            }
        }
        if let Some(cancelled) = done {
            self.batch_done = Some(cancelled);
            self.batch = None;
        }
        changed
    }
    fn begin_async_open(&mut self, path: &Path) -> Result<bool, String> {
        if !path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case("pdf"))
        {
            return Ok(false);
        }
        self.cancel_async_open();
        let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let progress = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let rx = pdf_view::start(path.to_path_buf(), cancel.clone(), progress.clone());
        self.pdf_pending = Some((rx, cancel, progress));
        Ok(true)
    }
    fn take_async_open_result(
        &mut self,
    ) -> Option<Result<suite42_core::session::FileIdentity, String>> {
        let result = self
            .pdf_pending
            .as_ref()
            .and_then(|(rx, _, _)| match rx.try_recv() {
                Ok(r) => Some(r),
                Err(std::sync::mpsc::TryRecvError::Empty) => None,
                Err(_) => Some(Err("PDF renderer stopped".into())),
            });
        if let Some(result) = result {
            self.pdf_pending = None;
            return Some(match result {
                Ok(prepared) => {
                    let identity = prepared.identity.clone();
                    self.pdf_prepared = Some(prepared);
                    Ok(identity)
                }
                Err(e) => Err(e),
            });
        }
        None
    }
    fn commit_async_open(&mut self) -> Result<u64, String> {
        let prepared = self.pdf_prepared.take().ok_or("No prepared PDF")?;
        if prepared.docs.is_empty() {
            return Err("PDF has no pages".into());
        }
        let count = prepared.docs.len();
        self.original_16bit = vec![false; count];
        self.frames = prepared.docs.into_iter().map(Some).collect();
        self.frame_index = 0;
        self.doc = self.frames[0].take();
        self.delays = vec![image::Delay::from_numer_denom_ms(0, 1); count];
        self.animation_loop = img42_core::AnimationLoop::Once;
        self.conversion_risk = true;
        self.source_path = Some(prepared.path);
        self.print_pending = None;
        self.zoom = None;
        self.selection = None;
        self.mask = None;
        self.gesture.clear();
        self.pending = None;
        self.pending_save_path = None;
        Ok(0)
    }
    fn cancel_async_open(&mut self) {
        if let Some((_, cancel, _)) = self.pdf_pending.take() {
            cancel.store(true, std::sync::atomic::Ordering::Relaxed);
        }
        self.pdf_prepared = None;
    }
    fn save_formats(&self) -> Vec<win42::FileFormat> {
        if self.frames.len() > 1 {
            return vec![
                win42::FileFormat {
                    id: "tiff",
                    title: "TIFF (all pages)",
                    extensions: &["tif", "tiff"],
                },
                win42::FileFormat {
                    id: "gif",
                    title: "Animated GIF (all frames)",
                    extensions: &["gif"],
                },
            ];
        }
        vec![
            win42::FileFormat {
                id: "png",
                title: "PNG image",
                extensions: &["png"],
            },
            win42::FileFormat {
                id: "jpeg",
                title: "JPEG image (white background)",
                extensions: &["jpg", "jpeg"],
            },
            win42::FileFormat {
                id: "tiff",
                title: "TIFF image",
                extensions: &["tif", "tiff"],
            },
            win42::FileFormat {
                id: "gif",
                title: "GIF image (single frame)",
                extensions: &["gif"],
            },
            win42::FileFormat {
                id: "webp",
                title: "WebP image",
                extensions: &["webp"],
            },
            win42::FileFormat {
                id: "bmp",
                title: "BMP image (white background)",
                extensions: &["bmp"],
            },
        ]
    }
    fn take_pending_open(&mut self) -> Option<std::path::PathBuf> {
        self.pending_open.take()
    }
    fn take_pending_print_job(&mut self) -> Option<Result<win42::PrintJob, String>> {
        let result = self
            .print_pending
            .as_ref()
            .and_then(|rx| rx.try_recv().ok());
        if result.is_some() {
            self.print_pending = None;
        }
        result
    }
    fn can_save_in_place(&self) -> bool {
        !self.conversion_risk && !self.loses_precision()
    }
    fn save_snapshot_to(
        &mut self,
        format: &str,
        path: &Path,
        is_current: bool,
    ) -> Result<Option<(u64, Vec<u8>)>, String> {
        if is_current && (self.conversion_risk || self.loses_precision()) {
            return Err("Saving here would discard source metadata or precision. Choose a different destination for the converted copy.".into());
        }
        let result = self.save_snapshot(format)?;
        self.pending_save_path = Some(path.to_path_buf());
        Ok(result)
    }
    fn save_snapshot(&mut self, format: &str) -> Result<Option<(u64, Vec<u8>)>, String> {
        let (bytes, states) = if self.frames.len() > 1 {
            let frames: Vec<&ImageDocument> = self
                .frames
                .iter()
                .enumerate()
                .map(|(i, f)| {
                    if i == self.frame_index {
                        self.doc.as_ref().unwrap()
                    } else {
                        f.as_ref().unwrap()
                    }
                })
                .collect();
            match format {
                "gif" => ImageDocument::save_gif_frames(&frames, &self.delays, self.animation_loop),
                "tiff" => ImageDocument::save_tiff_pages(&frames),
                _ => return Err("Choose TIFF or GIF to retain all pages".into()),
            }
            .map_err(|e| e.to_string())?
        } else {
            let (bytes, state) = self
                .doc
                .as_ref()
                .ok_or("No document")?
                .save_snapshot_format(format)
                .map_err(|e| e.to_string())?;
            (bytes, vec![state])
        };
        self.pending_16bit = self
            .frames
            .iter()
            .enumerate()
            .map(|(i, f)| {
                matches!(format, "tiff" | "png")
                    && (if i == self.frame_index {
                        self.doc.as_ref()
                    } else {
                        f.as_ref()
                    })
                    .is_some_and(ImageDocument::is_16bit)
            })
            .collect();
        self.next_save = self.next_save.wrapping_add(1);
        let revision = self.next_save;
        self.pending = Some((revision, states));
        Ok(Some((revision, bytes)))
    }

    fn saved_snapshot(&mut self, revision: u64) {
        if self.pending.as_ref().is_some_and(|(r, _)| *r == revision) {
            if let Some((_, states)) = self.pending.take() {
                self.conversion_risk = false;
                self.original_16bit = std::mem::take(&mut self.pending_16bit);
                if let Some(path) = self.pending_save_path.take() {
                    self.source_path = Some(path);
                }
                for (i, state) in states.into_iter().enumerate() {
                    let doc = if i == self.frame_index {
                        self.doc.as_mut()
                    } else {
                        self.frames.get_mut(i).and_then(Option::as_mut)
                    };
                    if let Some(doc) = doc {
                        doc.mark_saved_snapshot(state);
                    }
                }
            }
        }
    }

    fn name(&self) -> &'static str {
        "IMG42"
    }
    fn extension(&self) -> &'static str {
        "png"
    }
    fn format_name(&self) -> &'static str {
        "PNG / JPEG / BMP / TIFF / GIF / WebP / SVG / PDF image"
    }
    fn open_filter(&self) -> String {
        "*.png;*.jpg;*.jpeg;*.bmp;*.tif;*.tiff;*.gif;*.webp;*.svg;*.pdf".into()
    }
    fn file_actions(&self) -> &'static [win42::FileAction] {
        &[win42::FileAction {
            id: 40,
            title: "Export selected pixels (8-bit PNG)...",
            save: true,
            extension: "png",
            format: "PNG image",
            pattern: "*.png",
        }]
    }
    fn file_action(&mut self, id: u16, path: &Path) -> Result<(), String> {
        if id != 40 {
            return Err("Unknown action".into());
        }
        let mask = self.mask.as_ref().ok_or("Select a color first")?;
        let bytes = self
            .doc
            .as_ref()
            .ok_or("Open an image first")?
            .masked_png(mask)
            .map_err(|e| e.to_string())?;
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .map_err(|e| e.to_string())?;
        file.write_all(&bytes).map_err(|e| e.to_string())
    }
    fn actions(&self) -> &'static [(u16, &'static str)] {
        &[
            (10, "Rotate &Left"),
            (11, "Rotate &Right"),
            (15, "Flip &Horizontal"),
            (16, "Flip &Vertical"),
            (17, "&Resize..."),
            (18, "&Crop rectangle..."),
            (19, "&Adjust colors..."),
            (20, "Draw &line..."),
            (23, "Rectangle selection tool"),
            (24, "Ink tool (red)"),
            (25, "Crop to selection"),
            (26, "Rectangle markup tool"),
            (27, "Ellipse markup tool"),
            (28, "Arrow markup tool"),
            (29, "Markup color and width..."),
            (30, "Exposure and tone..."),
            (32, "Add text..."),
            (33, "Select color..."),
            (34, "Invert color selection"),
            (35, "Delete selected pixels"),
            (36, "Print all pages..."),
            (37, "Previous image in folder"),
            (38, "Next image in folder"),
            (39, "Lasso selection tool"),
            (40, "Batch convert folder..."),
            (41, "Cancel batch"),
            (42, "Batch outcomes..."),
        ]
    }
    fn input_folder_fields(&self, id: u16) -> Vec<(usize, &'static str)> {
        if id == 40 {
            vec![
                (0, "Choose the input image folder"),
                (1, "Choose the output folder"),
            ]
        } else {
            vec![]
        }
    }
    fn input_request(&self, id: u16) -> Option<win42::InputRequest> {
        if id == 40 {
            return Some(win42::InputRequest {
                title: "Batch conversion: JPEG is 8-bit on white",
                fields: vec![
                    ("Input folder path", String::new()),
                    ("Output folder path (must exist)", String::new()),
                    ("Format: png, tiff, jpeg", "png".into()),
                    ("Maximum width (0 = unchanged)", "0".into()),
                    ("Maximum height (0 = unchanged)", "0".into()),
                    (
                        "Resize: fit-no-upscale, fit, exact",
                        "fit-no-upscale".into(),
                    ),
                    ("Frames: first (PNG/JPEG), all (TIFF)", "first".into()),
                    ("JPEG quality (1–100)", "90".into()),
                ],
            });
        }
        if id == 33 {
            return Some(win42::InputRequest {
                title: "Select pixels by sampled color",
                fields: vec![
                    ("Sample X", "0".into()),
                    ("Sample Y", "0".into()),
                    ("RGB tolerance (0–255)", "20".into()),
                ],
            });
        }
        if id == 32 {
            return Some(win42::InputRequest {
                title: "Add text using markup color",
                fields: vec![
                    ("Text", "Text".into()),
                    ("Left (pixels)", "0".into()),
                    ("Top (pixels)", "0".into()),
                    ("Size (pixels)", "24".into()),
                ],
            });
        }
        let d = self.doc.as_ref()?;
        let (title, fields) = match id {
            17 => (
                "Resize image (Lanczos quality)",
                vec![
                    ("Width (pixels)", d.width().to_string()),
                    ("Height (pixels)", d.height().to_string()),
                ],
            ),
            18 => (
                "Crop rectangle",
                vec![
                    ("Left", "0".into()),
                    ("Top", "0".into()),
                    ("Width", d.width().to_string()),
                    ("Height", d.height().to_string()),
                ],
            ),
            19 => (
                "Adjust colors",
                vec![
                    ("Brightness (-255 to 255)", "0".into()),
                    ("Contrast (-100 to 100)", "0".into()),
                    ("Saturation (-100 to 100)", "0".into()),
                ],
            ),
            30 => (
                "Exposure and tone",
                vec![
                    ("Exposure (-400 to 400; 100 = 1 stop)", "0".into()),
                    ("Gamma (10 to 400; 100 = unchanged)", "100".into()),
                    ("Temperature (-100 to 100)", "0".into()),
                    ("Shadows (-100 to 100)", "0".into()),
                    ("Highlights (-100 to 100)", "0".into()),
                    ("Sharpness (0 to 100)", "0".into()),
                ],
            ),
            29 => (
                "Markup style",
                vec![
                    ("Red (0–255)", self.ink_color[0].to_string()),
                    ("Green (0–255)", self.ink_color[1].to_string()),
                    ("Blue (0–255)", self.ink_color[2].to_string()),
                    ("Width (1–99)", self.ink_width.max(1).to_string()),
                ],
            ),
            20 => (
                "Draw red line",
                vec![
                    ("Start X", "0".into()),
                    ("Start Y", "0".into()),
                    ("End X", d.width().saturating_sub(1).to_string()),
                    ("End Y", d.height().saturating_sub(1).to_string()),
                ],
            ),
            _ => return None,
        };
        Some(win42::InputRequest { title, fields })
    }
    fn input_action(&mut self, id: u16, values: &[String]) -> Result<(), String> {
        if id == 40 {
            if self.batch.is_some() {
                return Err("A batch is already running".into());
            }
            let get = |i: usize| {
                values
                    .get(i)
                    .map(|x| x.trim())
                    .ok_or("Missing batch option")
            };
            let format = get(2)?.to_ascii_lowercase();
            if format != "png" && format != "tiff" && format != "jpeg" {
                return Err("Choose png, tiff or jpeg".into());
            }
            let w: u32 = get(3)?.parse().map_err(|_| "Invalid width")?;
            let h: u32 = get(4)?.parse().map_err(|_| "Invalid height")?;
            let size = if w == 0 && h == 0 {
                None
            } else if w > 0 && h > 0 && w <= 16384 && h <= 16384 {
                Some((w, h))
            } else {
                return Err("Enter two dimensions from 1 to 16384, or two zeros".into());
            };
            let resize_mode = get(5)?.to_ascii_lowercase();
            if !["fit-no-upscale", "fit", "exact"].contains(&resize_mode.as_str()) {
                return Err("Choose fit-no-upscale, fit or exact".into());
            }
            let frames = get(6)?.to_ascii_lowercase();
            if !["first", "all"].contains(&frames.as_str()) {
                return Err("Choose first or all frames".into());
            }
            if frames == "all" && format != "tiff" {
                return Err("All frames requires TIFF; choose first for PNG or JPEG".into());
            }
            let jpeg_quality: u8 = get(7)?.parse().map_err(|_| "JPEG quality must be 1–100")?;
            if !(1..=100).contains(&jpeg_quality) {
                return Err("JPEG quality must be 1–100".into());
            }
            let options = batch::Options {
                input: get(0)?.into(),
                output: get(1)?.into(),
                format,
                size,
                resize_mode,
                frames,
                jpeg_quality,
            };
            let worker = batch::start(options)?;
            self.batch_outcomes.clear();
            self.batch_done = None;
            self.batch = Some(worker);
            return Ok(());
        }
        if id == 33 {
            let parse = |i: usize| {
                values
                    .get(i)
                    .ok_or("Missing value")?
                    .parse::<u32>()
                    .map_err(|_| "Invalid value")
            };
            let (x, y, t) = (parse(0)?, parse(1)?, parse(2)?);
            let d = self.doc.as_ref().ok_or("Open an image first")?;
            if x >= d.width() || y >= d.height() || t > 255 {
                return Err("Invalid sample or tolerance".into());
            }
            let i = ((y * d.width() + x) * 4) as usize;
            let target = &d.pixels()[i..i + 3];
            self.mask = Some(
                d.pixels()
                    .chunks_exact(4)
                    .map(|p| (0..3).all(|c| p[c].abs_diff(target[c]) <= t as u8))
                    .collect(),
            );
            return Ok(());
        }
        if id == 32 {
            let text = values.first().ok_or("Missing text")?;
            let parse = |i: usize| {
                values
                    .get(i)
                    .ok_or("Missing coordinate")?
                    .parse::<u32>()
                    .map_err(|_| "Invalid coordinate")
            };
            let (x, y, size) = (parse(1)?, parse(2)?, parse(3)?);
            return self
                .doc
                .as_mut()
                .ok_or("Open an image first")?
                .draw_text(x, y, size, text, self.ink_color)
                .map_err(|e| e.to_string());
        }
        let nums: Vec<i32> = values
            .iter()
            .map(|v| {
                v.parse::<i32>()
                    .map_err(|_| "Enter integer values".to_string())
            })
            .collect::<Result<_, _>>()?;
        if id == 29 {
            if let [r, g, b, w] = nums.as_slice() {
                if (0..=255).contains(r)
                    && (0..=255).contains(g)
                    && (0..=255).contains(b)
                    && (1..=99).contains(w)
                {
                    self.ink_color = [*r as u8, *g as u8, *b as u8];
                    self.ink_width = *w as u32;
                    return Ok(());
                }
            }
            return Err("Invalid markup style".into());
        }
        if id == 17 || id == 18 {
            self.selection = None;
            self.mask = None;
            self.gesture.clear();
        }
        let d = self.doc.as_mut().ok_or("Open an image first")?;
        let positive =
            |n: i32| u32::try_from(n).map_err(|_| "Coordinates must be nonnegative".to_string());
        match (id, nums.as_slice()) {
            (17, [w, h]) => d.resize_quality(positive(*w)?, positive(*h)?, "lanczos"),
            (18, [x, y, w, h]) => {
                d.crop(positive(*x)?, positive(*y)?, positive(*w)?, positive(*h)?)
            }
            (19, [b, c, s]) => d.adjust(*b, *c, *s),
            (30, [e, g, t, s, h, sharp]) => d.develop(*e, *g, *t, *s, *h, *sharp),
            (20, [x, y, x1, y1]) => d.draw_line(
                positive(*x)?,
                positive(*y)?,
                positive(*x1)?,
                positive(*y1)?,
                [255, 0, 0, 255],
            ),
            _ => return Err("Invalid request".into()),
        }
        .map_err(|e| e.to_string())
    }

    fn view_actions(&self) -> &'static [(u16, &'static str)] {
        &[(12, "Zoom &In"), (13, "Zoom &Out"), (14, "&Fit to Window")]
    }
    fn items(&self) -> Vec<String> {
        (0..self.frames.len())
            .map(|i| format!("Frame {}", i + 1))
            .collect()
    }
    fn list_visible(&self) -> bool {
        self.frames.len() > 1
    }
    fn text(&self) -> String {
        "Open an image. Animated GIF and WebP frames can be edited individually and saved together as GIF; GIF quantizes color and transparency. ICC profiles are retained in PNG, JPEG and TIFF; unsupported source metadata requires a converted copy.".into()
    }
    fn text_readonly(&self) -> bool {
        true
    }
    fn image(&self) -> Option<(u32, u32, Vec<u8>)> {
        let d = self.doc.as_ref()?;
        let mut pixels = d.pixels().to_vec();
        if let Some(mask) = &self.mask {
            for (p, selected) in pixels.chunks_exact_mut(4).zip(mask) {
                if *selected {
                    p[0] /= 2;
                    p[1] /= 2;
                    p[2] = p[2] / 2 + 127;
                }
            }
        }
        let rect = if self.tool == 1 && self.gesture.len() > 1 {
            Some(rectangle(self.gesture[0], *self.gesture.last().unwrap()))
        } else {
            self.selection
        };
        if let Some((x, y, w, h)) = rect.filter(|(x, y, w, h)| {
            x.checked_add(*w).is_some_and(|v| v <= d.width())
                && y.checked_add(*h).is_some_and(|v| v <= d.height())
        }) {
            for yy in y..y + h {
                for xx in x..x + w {
                    if xx == x || yy == y || xx + 1 == x + w || yy + 1 == y + h {
                        let i = ((yy * d.width() + xx) * 4) as usize;
                        pixels[i..i + 4].copy_from_slice(&[0, 120, 255, 255]);
                    }
                }
            }
        }
        Some((d.width(), d.height(), pixels))
    }
    fn canvas_event(&mut self, event: win42::CanvasEvent) -> Result<(), String> {
        use win42::CanvasEvent::*;
        let d = match self.doc.as_ref() {
            Some(d) => d,
            None => return Ok(()),
        };
        let point = |p: win42::Point| {
            (
                p.x.floor().clamp(0.0, (d.width() - 1) as f64) as u32,
                p.y.floor().clamp(0.0, (d.height() - 1) as f64) as u32,
            )
        };
        match event {
            PointerDown(p) if self.tool != 0 => {
                self.gesture.clear();
                self.gesture.push(point(p));
            }
            PointerMove(p) if !self.gesture.is_empty() => {
                let p = point(p);
                if self.gesture.last() != Some(&p) && self.gesture.len() < 4095 {
                    self.gesture.push(p);
                }
            }
            PointerUp(p) if !self.gesture.is_empty() => {
                self.gesture.push(point(p));
                if self.tool == 1 {
                    self.selection =
                        Some(rectangle(self.gesture[0], *self.gesture.last().unwrap()));
                } else if self.tool == 6 {
                    self.mask = Some(polygon_mask(d.width(), d.height(), &self.gesture)?);
                    self.selection = None;
                } else {
                    let start = self.gesture[0];
                    let end = *self.gesture.last().unwrap();
                    let points = if self.tool == 2 {
                        self.gesture.clone()
                    } else {
                        shape_points(self.tool, start, end, d.width(), d.height())
                    };
                    let [r, g, b] = self.ink_color;
                    self.doc
                        .as_mut()
                        .unwrap()
                        .draw_stroke(&points, [r, g, b, 255], self.ink_width.max(1))
                        .map_err(|e| e.to_string())?;
                }
                self.gesture.clear();
            }
            Cancel => self.gesture.clear(),
            _ => {}
        }
        Ok(())
    }
    fn image_zoom(&self) -> Option<f64> {
        self.zoom
    }
    fn status(&self) -> String {
        let completed = self
            .batch_outcomes
            .iter()
            .filter(|line| line.ends_with(": completed"))
            .count();
        if self.batch.is_some() {
            return format!(
                "Batch running: {} completed, {} other outcomes; Cancel batch or Batch outcomes",
                completed,
                self.batch_outcomes.len() - completed
            );
        }
        if let Some(cancelled) = self.batch_done {
            return format!(
                "Batch {}: {} completed, {} other outcomes; open Batch outcomes",
                if cancelled { "cancelled" } else { "finished" },
                completed,
                self.batch_outcomes.len() - completed
            );
        }
        if let Some((_, _, progress)) = &self.pdf_pending {
            return format!(
                "Opening PDF: {} pages rendered",
                progress.load(std::sync::atomic::Ordering::Relaxed)
            );
        }
        self.doc
            .as_ref()
            .map(|d| {
                format!(
                    "{} × {} pixels | {}",
                    d.width(),
                    d.height(),
                    self.zoom
                        .map(|z| format!("{:.0}%", 100.0 * z))
                        .unwrap_or_else(|| "Fit".into())
                )
            })
            .unwrap_or_else(|| "Open a PNG or JPEG image".into())
    }
    fn dirty(&self) -> bool {
        self.doc.as_ref().is_some_and(|d| d.is_dirty())
            || self.frames.iter().flatten().any(|d| d.is_dirty())
    }
    fn can_undo(&self) -> bool {
        self.doc.as_ref().is_some_and(|d| d.can_undo())
    }
    fn can_redo(&self) -> bool {
        self.doc.as_ref().is_some_and(|d| d.can_redo())
    }
    fn can_save(&self) -> bool {
        self.doc.is_some()
    }
    fn enabled(&self, id: u16) -> bool {
        match id {
            40 => self.batch.is_none(),
            41 => self.batch.is_some(),
            42 => !self.batch_outcomes.is_empty(),
            _ => self.doc.is_some(),
        }
    }
    fn open(&mut self, p: &Path) -> Result<(), String> {
        if p.extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case("pdf"))
        {
            return Err("PDF opening must use the asynchronous host".into());
        }
        let (docs, delays) = ImageDocument::open_frames(p).map_err(|e| e.to_string())?;
        let animation_loop = ImageDocument::animation_loop(p).map_err(|e| e.to_string())?;
        let conversion_risk = ImageDocument::has_conversion_risk(p).map_err(|e| e.to_string())?;
        self.source_path = Some(p.to_path_buf());
        self.print_pending = None;
        self.original_16bit = docs.iter().map(ImageDocument::is_16bit).collect();
        self.frames = docs.into_iter().map(Some).collect();
        self.frame_index = 0;
        self.doc = self.frames[0].take();
        self.delays = delays;
        self.animation_loop = animation_loop;
        self.conversion_risk = conversion_risk;
        self.zoom = None;
        self.selection = None;
        self.mask = None;
        self.gesture.clear();
        Ok(())
    }
    fn save(&mut self, p: &Path) -> Result<(), String> {
        if self.conversion_risk
            || self.loses_precision()
            || self.doc.as_ref().is_some_and(ImageDocument::is_16bit)
        {
            return Err("Use Save As for a converted 8-bit PNG copy".into());
        }
        if self.frames.len() > 1 {
            return Err("Use TIFF or GIF save to retain every frame".into());
        }
        self.doc
            .as_mut()
            .ok_or("Open an image first")?
            .save_as_png(p)
            .map_err(|e| e.to_string())
    }
    fn edit(&mut self, _: String) -> Result<(), String> {
        Ok(())
    }
    fn select(&mut self, i: usize) {
        if i < self.frames.len() && i != self.frame_index {
            self.frames[self.frame_index] = self.doc.take();
            self.doc = self.frames[i].take();
            self.frame_index = i;
            self.selection = None;
            self.mask = None;
            self.gesture.clear();
        }
    }
    fn selected(&self) -> usize {
        self.frame_index
    }
    fn action(&mut self, id: u16) -> Result<(), String> {
        if id == 41 {
            if let Some(worker) = &self.batch {
                worker
                    .cancel
                    .store(true, std::sync::atomic::Ordering::Relaxed);
            }
            return Ok(());
        }
        if id == 42 {
            return Err(format!("Batch outcomes (JPEG is 8-bit on white; compatible metadata may be retained, other metadata may be lost):\n{}", self.batch_outcomes.join("\n")));
        }
        if id == 37 || id == 38 {
            let source = self.source_path.as_ref().ok_or("Open an image first")?;
            let folder = source.parent().ok_or("Image has no folder")?;
            let mut paths: Vec<_> = std::fs::read_dir(folder)
                .map_err(|e| e.to_string())?
                .filter_map(Result::ok)
                .map(|e| e.path())
                .filter(|p| {
                    p.is_file()
                        && p.extension().and_then(|v| v.to_str()).is_some_and(|e| {
                            [
                                "png", "jpg", "jpeg", "bmp", "tif", "tiff", "gif", "webp", "svg",
                                "pdf",
                            ]
                            .iter()
                            .any(|x| e.eq_ignore_ascii_case(x))
                        })
                })
                .collect();
            paths.sort_by_key(|p| p.file_name().map(|n| n.to_string_lossy().to_lowercase()));
            let index = paths
                .iter()
                .position(|p| p == source)
                .ok_or("Current image is no longer in its folder")?;
            let next = if id == 37 {
                index.checked_sub(1)
            } else {
                index.checked_add(1).filter(|i| *i < paths.len())
            }
            .ok_or("No more images in this direction")?;
            self.pending_open = Some(paths[next].clone());
            return Ok(());
        }
        if id == 36 {
            if self.print_pending.is_some() {
                return Err("Print preparation already running".into());
            }
            let pages: Vec<_> = self
                .frames
                .iter()
                .enumerate()
                .map(|(i, f)| {
                    let d = if i == self.frame_index {
                        self.doc.as_ref().unwrap()
                    } else {
                        f.as_ref().unwrap()
                    };
                    (
                        d.width(),
                        d.height(),
                        d.pixels().to_vec(),
                        d.dpi().unwrap_or((150, 150)),
                    )
                })
                .collect();
            if pages.is_empty() {
                return Err("Open an image first".into());
            }
            let (tx, rx) = std::sync::mpsc::channel();
            std::thread::spawn(move || {
                let result = (|| -> Result<win42::PrintJob, String> {
                    let mut out = Vec::new();
                    for (w, h, pixels, (dx, dy)) in pages {
                        let width = w as f64 * 72.0 / dx as f64;
                        let height = h as f64 * 72.0 / dy as f64;
                        if width < 1.0 || height < 1.0 || width > 4096.0 || height > 4096.0 {
                            return Err("Page size is outside printable limits".into());
                        }
                        let geometry = win42::PrintGeometry {
                            width_twips: (width * 20.0).round() as i32,
                            height_twips: (height * 20.0).round() as i32,
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
                                pixel_width: w,
                                pixel_height: h,
                                rgba: pixels,
                            }],
                        };
                        out.push(win42::PrintPage { geometry, scene });
                    }
                    Ok(win42::PrintJob::Scenes(out))
                })();
                let _ = tx.send(result);
            });
            self.print_pending = Some(rx);
            return Ok(());
        }
        if id == 34 {
            let mask = self.mask.as_mut().ok_or("Select a color first")?;
            for selected in mask {
                *selected = !*selected;
            }
            return Ok(());
        }
        if id == 35 {
            let mask = self.mask.take().ok_or("Select a color first")?;
            return self
                .doc
                .as_mut()
                .ok_or("Open an image first")?
                .erase_mask(&mask)
                .map_err(|e| e.to_string());
        }
        if matches!(id, 4 | 5 | 10 | 11 | 15 | 16) {
            self.selection = None;
            self.mask = None;
            self.gesture.clear();
        }
        let d = self.doc.as_mut().ok_or("Open an image first")?;
        match id {
            4 => {
                d.undo();
            }
            5 => {
                d.redo();
            }
            10 => d.rotate_ccw().map_err(|e| e.to_string())?,
            11 => d.rotate_cw().map_err(|e| e.to_string())?,
            15 => d.flip_horizontal().map_err(|e| e.to_string())?,
            16 => d.flip_vertical().map_err(|e| e.to_string())?,
            12 => self.zoom = Some((self.zoom.unwrap_or(1.0) * 1.25).min(16.0)),
            13 => self.zoom = Some((self.zoom.unwrap_or(1.0) * 0.8).max(0.05)),
            14 => self.zoom = None,
            23 => {
                self.tool = 1;
                self.gesture.clear();
            }
            39 => {
                self.tool = 6;
                self.gesture.clear();
            }
            24 => {
                self.tool = 2;
                self.gesture.clear();
            }
            26..=28 => {
                self.tool = (id - 23) as u8;
                self.gesture.clear();
            }
            25 => {
                self.mask = None;
                let (x, y, w, h) = self.selection.take().ok_or("Select a rectangle first")?;
                d.crop(x, y, w, h).map_err(|e| e.to_string())?;
            }
            _ => {}
        }
        Ok(())
    }
}
/// Scanline even-odd polygon fill in image coordinates.
fn polygon_mask(width: u32, height: u32, points: &[(u32, u32)]) -> Result<Vec<bool>, String> {
    if points.len() < 3 || points.len() > 4096 {
        return Err("Draw a lasso with 3–4096 points".into());
    }
    let mut mask = vec![false; width as usize * height as usize];
    let min_y = points.iter().map(|p| p.1).min().unwrap();
    let max_y = points
        .iter()
        .map(|p| p.1)
        .max()
        .unwrap()
        .min(height.saturating_sub(1));
    for y in min_y..=max_y {
        let scan = y as f64 + 0.5;
        let mut intersections = Vec::new();
        for i in 0..points.len() {
            let (x1, y1) = points[i];
            let (x2, y2) = points[(i + 1) % points.len()];
            let (y1, y2) = (y1 as f64, y2 as f64);
            if (y1 <= scan && scan < y2) || (y2 <= scan && scan < y1) {
                intersections.push(x1 as f64 + (scan - y1) * (x2 as f64 - x1 as f64) / (y2 - y1));
            }
        }
        intersections.sort_by(f64::total_cmp);
        for pair in intersections.chunks_exact(2) {
            let x1 = (pair[0] - 0.5).ceil().max(0.0) as u32;
            let x2 = (pair[1] - 0.5).ceil().min(width as f64).max(0.0) as u32;
            for x in x1.min(width)..x2 {
                mask[(y * width + x) as usize] = true;
            }
        }
    }
    Ok(mask)
}
fn rectangle(a: (u32, u32), b: (u32, u32)) -> (u32, u32, u32, u32) {
    (
        a.0.min(b.0),
        a.1.min(b.1),
        a.0.abs_diff(b.0) + 1,
        a.1.abs_diff(b.1) + 1,
    )
}
fn shape_points(
    tool: u8,
    a: (u32, u32),
    b: (u32, u32),
    width: u32,
    height: u32,
) -> Vec<(u32, u32)> {
    let (x, y, w, h) = rectangle(a, b);
    let (right, bottom) = (x + w - 1, y + h - 1);
    match tool {
        3 => vec![(x, y), (right, y), (right, bottom), (x, bottom), (x, y)],
        4 => (0..=128)
            .map(|i| {
                let t = i as f64 * std::f64::consts::TAU / 128.0;
                (
                    (x as f64 + (w - 1) as f64 * (1.0 + t.cos()) / 2.0).round() as u32,
                    (y as f64 + (h - 1) as f64 * (1.0 + t.sin()) / 2.0).round() as u32,
                )
            })
            .collect(),
        5 => {
            let dx = b.0 as f64 - a.0 as f64;
            let dy = b.1 as f64 - a.1 as f64;
            let angle = dy.atan2(dx);
            let size = (dx.hypot(dy) * 0.25).clamp(3.0, 20.0);
            let tip = |ang: f64| {
                (
                    (b.0 as f64 - size * ang.cos())
                        .round()
                        .clamp(0.0, (width - 1) as f64) as u32,
                    (b.1 as f64 - size * ang.sin())
                        .round()
                        .clamp(0.0, (height - 1) as f64) as u32,
                )
            };
            vec![a, b, tip(angle - 0.5), b, tip(angle + 0.5)]
        }
        _ => vec![a, b],
    }
}
fn main() {
    win42::run(Box::new(Img::default()))
}

#[cfg(test)]
mod selection_tests {
    use super::*;
    #[test]
    fn png_with_srgb_can_save_in_place() {
        use image::ImageEncoder;
        use win42::App;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("source.png");
        let mut png = Vec::new();
        image::codecs::png::PngEncoder::new(&mut png)
            .write_image(&[0, 0, 0, 255], 1, 1, image::ExtendedColorType::Rgba8)
            .unwrap();
        png.splice(
            33..33,
            [
                0, 0, 0, 1, b's', b'R', b'G', b'B', 0, 0xae, 0xce, 0x1c, 0xe9,
            ],
        );
        std::fs::write(&path, png).unwrap();
        let mut app = Img::default();
        app.open(&path).unwrap();
        assert!(app.can_save_in_place());
        let (_, out) = app.save_snapshot_to("png", &path, true).unwrap().unwrap();
        assert!(out.windows(4).any(|p| p == b"sRGB"));
    }
    #[test]
    fn png_with_unpreserved_color_metadata_needs_new_destination() {
        use image::ImageEncoder;
        use win42::App;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("source.png");
        let mut bytes = Vec::new();
        image::codecs::png::PngEncoder::new(&mut bytes)
            .write_image(&[0, 0, 0, 255], 1, 1, image::ExtendedColorType::Rgba8)
            .unwrap();
        let chunk = [
            0, 0, 0, 4, b's', b'B', b'I', b'T', 8, 8, 8, 8, 124, 8, 100, 136,
        ];
        bytes.splice(33..33, chunk);
        std::fs::write(&path, bytes).unwrap();
        let mut app = Img::default();
        app.open(&path).unwrap();
        assert!(!app.can_save_in_place());
        assert!(app.save_snapshot_to("png", &path, true).is_err());
        assert!(app
            .save_snapshot_to("png", &dir.path().join("copy.png"), false)
            .unwrap()
            .is_some());
    }
    #[test]
    fn png16_save_tracks_published_precision() {
        use image::ImageEncoder;
        use win42::App;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("photo.png");
        let samples = [1u16, 257, 4097, 65535];
        let bytes: Vec<u8> = samples.iter().flat_map(|v| v.to_ne_bytes()).collect();
        let mut input = Vec::new();
        image::codecs::png::PngEncoder::new(&mut input)
            .write_image(&bytes, 1, 1, image::ExtendedColorType::Rgba16)
            .unwrap();
        std::fs::write(&path, input).unwrap();
        let mut app = Img::default();
        app.open(&path).unwrap();
        assert!(app.doc.as_ref().unwrap().is_16bit());
        assert!(app.can_save_in_place());
        let (revision, encoded) = app.save_snapshot_to("png", &path, true).unwrap().unwrap();
        app.saved_snapshot(revision);
        assert!(app.can_save_in_place());
        assert_eq!(app.original_16bit, vec![true]);
        let saved = dir.path().join("saved.png");
        std::fs::write(&saved, encoded).unwrap();
        let reopened = ImageDocument::open(&saved).unwrap();
        assert!(reopened.is_16bit());
    }
    #[test]
    fn prepared_pdf_is_atomic_and_cancellable() {
        use win42::App;
        let original = ImageDocument::from_rgba(1, 1, vec![1, 2, 3, 255], None).unwrap();
        let next = ImageDocument::from_rgba(1, 1, vec![4, 5, 6, 255], None).unwrap();
        let mut app = Img::default();
        app.frames = vec![None];
        app.doc = Some(original);
        app.original_16bit = vec![false];
        let (tx, rx) = std::sync::mpsc::channel();
        let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let progress = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        app.pdf_pending = Some((rx, cancel, progress));
        let identity = suite42_core::session::FileIdentity::from_bytes(b"pdf", None);
        tx.send(Ok(pdf_view::Prepared {
            path: "x.pdf".into(),
            identity: identity.clone(),
            docs: vec![next],
        }))
        .unwrap();
        assert_eq!(app.take_async_open_result().unwrap().unwrap(), identity);
        assert_eq!(app.doc.as_ref().unwrap().pixels(), &[1, 2, 3, 255]);
        app.cancel_async_open();
        assert!(app.commit_async_open().is_err());
        assert_eq!(app.doc.as_ref().unwrap().pixels(), &[1, 2, 3, 255]);
        app.pdf_prepared = Some(pdf_view::Prepared {
            path: "x.pdf".into(),
            identity,
            docs: vec![ImageDocument::from_rgba(1, 1, vec![4, 5, 6, 255], None).unwrap()],
        });
        app.commit_async_open().unwrap();
        assert_eq!(app.doc.as_ref().unwrap().pixels(), &[4, 5, 6, 255]);
        assert!(!app.dirty());
    }
    #[test]
    fn lasso_fills_polygon_and_rejects_degenerate() {
        let m = polygon_mask(4, 4, &[(0, 0), (3, 0), (3, 3), (0, 3)]).unwrap();
        assert!(m[0]);
        assert!(m[10]);
        assert!(!m[15]);
        assert!(polygon_mask(4, 4, &[(0, 0), (1, 1)]).is_err());
    }
}

#[cfg(test)]
mod batch_option_tests {
    use super::*;
    use win42::App;
    #[test]
    fn rejects_invalid_jpeg_quality() {
        for quality in ["0", "101", "bad"] {
            let values = ["input", "output", "jpeg", "0", "0", "fit", "first", quality]
                .iter()
                .map(|s| s.to_string())
                .collect::<Vec<_>>();
            assert!(Img::default()
                .input_action(40, &values)
                .unwrap_err()
                .contains("quality"));
        }
    }
}
