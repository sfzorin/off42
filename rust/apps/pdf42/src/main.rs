#![cfg_attr(windows, windows_subsystem = "windows")]
use pdf42_core::PdfDocument;
use std::path::Path;
#[cfg(windows)]
mod pdfium_edit;
#[cfg(windows)]
mod render;
#[cfg(windows)]
mod search;
struct Secret(String);
impl Drop for Secret {
    fn drop(&mut self) {
        unsafe {
            self.0.as_bytes_mut().fill(0);
        }
    }
}
enum OpenAttempt {
    Ready(PdfDocument, suite42_core::session::FileIdentity),
    Password {
        bytes: std::sync::Arc<[u8]>,
        identity: suite42_core::session::FileIdentity,
        incorrect: bool,
    },
}
struct PasswordChallenge {
    bytes: std::sync::Arc<[u8]>,
    identity: suite42_core::session::FileIdentity,
    incorrect: bool,
    prompt_pending: bool,
}
fn open_attempt(
    bytes: std::sync::Arc<[u8]>,
    identity: suite42_core::session::FileIdentity,
    password: Option<Secret>,
    cancel: &std::sync::atomic::AtomicBool,
) -> Result<OpenAttempt, String> {
    if cancel.load(std::sync::atomic::Ordering::Relaxed) {
        return Err("Open cancelled".into());
    }
    let result = if let Some(password) = password {
        match PdfDocument::try_open_with_password_bytes(&bytes, &password.0)? {
            Some(doc) => OpenAttempt::Ready(doc, identity),
            None => OpenAttempt::Password {
                bytes,
                identity,
                incorrect: true,
            },
        }
    } else {
        match PdfDocument::probe_open_bytes(&bytes)? {
            pdf42_core::PdfOpenProbe::Ready(doc) => OpenAttempt::Ready(doc, identity),
            pdf42_core::PdfOpenProbe::PasswordRequired => OpenAttempt::Password {
                bytes,
                identity,
                incorrect: false,
            },
        }
    };
    if cancel.load(std::sync::atomic::Ordering::Relaxed) {
        return Err("Open cancelled".into());
    }
    Ok(result)
}
#[derive(Default)]
struct Pdf {
    pending: Option<(u64, u64, pdf42_core::SaveState, bool)>,
    protected_original: Option<std::path::PathBuf>,
    protected_session: bool,
    encrypted_session: bool,
    encrypted_current: Option<std::path::PathBuf>,
    pending_destination: Option<std::path::PathBuf>,
    output_password: Option<Secret>,
    protected_readonly: bool,
    protected_open: Option<(
        std::sync::mpsc::Receiver<Result<OpenAttempt, String>>,
        std::sync::Arc<std::sync::atomic::AtomicBool>,
    )>,
    open_challenge: Option<PasswordChallenge>,
    opening_path: Option<std::path::PathBuf>,
    prepared_open: Option<(PdfDocument, suite42_core::session::FileIdentity)>,
    past_documents: Vec<(u64, PdfDocument)>,
    future_documents: Vec<(u64, PdfDocument)>,
    current_epoch: u64,
    saved_epoch: u64,
    next_epoch: u64,
    next_save: u64,
    doc: Option<PdfDocument>,
    #[cfg(windows)]
    renderer: Option<render::Worker>,
    #[cfg(windows)]
    generation: u64,
    #[cfg(windows)]
    rendered: Option<(u32, u32, Vec<u8>)>,
    #[cfg(windows)]
    render_error: Option<String>,
    #[cfg(windows)]
    flatten_pending: Option<(
        std::sync::mpsc::Receiver<Result<render::FlattenedPages, String>>,
        Vec<u8>,
        u64,
        usize,
        std::sync::Arc<std::sync::atomic::AtomicBool>,
    )>,
    operation_error: Option<String>,
    #[cfg(windows)]
    print_pending: Option<(
        std::sync::mpsc::Receiver<Result<win42::PrintJob, String>>,
        std::sync::Arc<std::sync::atomic::AtomicUsize>,
        std::sync::Arc<std::sync::atomic::AtomicBool>,
        usize,
    )>,
    #[cfg(windows)]
    print_ready: Option<Result<win42::PrintJob, String>>,
    #[cfg(windows)]
    export_pending: Option<(
        std::sync::mpsc::Receiver<Result<(), String>>,
        std::sync::Arc<std::sync::atomic::AtomicUsize>,
        std::sync::Arc<std::sync::atomic::AtomicBool>,
        usize,
        std::path::PathBuf,
    )>,
    export_first: usize,
    export_last: usize,
    export_dpi: u32,
    zoom: f64,
    pan: win42::Point,
    pan_drag: Option<(win42::Point, win42::Point)>,
    tool: Option<pdf42_core::AnnotationKind>,
    annotation_drag: Option<win42::Point>,
    ink_points: Vec<win42::Point>,
    annotation_contents: String,
    stamp_image: Option<pdf42_core::ImageStamp>,
    signature_asset: Option<Vec<[f64; 2]>>,
    capture_signature: bool,
    place_signature: bool,
    select_annotations: bool,
    redact_tool: bool,
    selected_annotation: Option<pdf42_core::AnnotationSelection>,
    #[cfg(windows)]
    search_hits: Vec<search::Hit>,
    #[cfg(windows)]
    search_index: usize,
    search_query: String,
    select_text: bool,
    attachment_name: String,
    #[cfg(windows)]
    selected_text: Option<search::Selection>,
    object_tool: bool,
    #[cfg(windows)]
    selected_object: Option<(pdfium_edit::Selected, Vec<u8>)>,
    #[cfg(windows)]
    object_inspect: Option<(
        std::sync::mpsc::Receiver<Result<pdfium_edit::Selected, String>>,
        Vec<u8>,
        u64,
        std::sync::Arc<std::sync::atomic::AtomicBool>,
    )>,
    #[cfg(windows)]
    object_edit: Option<(
        std::sync::mpsc::Receiver<Result<Vec<u8>, String>>,
        Vec<u8>,
        u64,
        usize,
        std::sync::Arc<std::sync::atomic::AtomicBool>,
    )>,
}
impl Pdf {
    #[cfg(windows)]
    fn page_point(&self, p: win42::Point) -> Result<[f64; 2], String> {
        let (width, height, _) = self
            .rendered
            .as_ref()
            .ok_or("Wait for the page to render")?;
        let doc = self.doc.as_ref().ok_or("Open a PDF first")?;
        let b = doc.page_box(doc.current())?;
        let u = (p.x / *width as f64).clamp(0., 1.);
        let v = (p.y / *height as f64).clamp(0., 1.);
        let (x, y) = match doc.rotation(doc.current())? {
            0 => (u, 1. - v),
            90 => (v, u),
            180 => (1. - u, v),
            270 => (1. - v, 1. - u),
            _ => return Err("Invalid page rotation".into()),
        };
        Ok([b[0] + x * (b[2] - b[0]), b[1] + y * (b[3] - b[1])])
    }
    fn install_flattened(
        &mut self,
        pages: Vec<pdf42_core::FlattenedPage>,
        page: usize,
    ) -> Result<(), String> {
        let replacement = PdfDocument::from_flattened_pages(pages, page)?;
        self.install_replacement(replacement)
    }
    fn install_replacement(&mut self, replacement: PdfDocument) -> Result<(), String> {
        let mut previous = self.doc.take().ok_or("Open a PDF first")?;
        previous.discard_redo();
        self.past_documents.push((self.current_epoch, previous));
        if self.past_documents.len() > 20 {
            self.past_documents.remove(0);
        }
        self.future_documents.clear();
        self.next_epoch = self.next_epoch.wrapping_add(1);
        self.current_epoch = self.next_epoch;
        self.doc = Some(replacement);
        self.selected_annotation = None;
        self.tool = None;
        self.select_annotations = false;
        self.select_text = false;
        self.object_tool = false;
        self.redact_tool = false;
        self.annotation_drag = None;
        self.ink_points.clear();
        self.capture_signature = false;
        self.place_signature = false;
        self.pan = win42::Point::default();
        Ok(())
    }
    fn branch(&mut self) {
        self.future_documents.clear();
    }
    fn undo_document(&mut self) {
        if self.doc.as_ref().is_some_and(|d| d.can_undo()) {
            self.doc.as_mut().unwrap().undo();
        } else if let Some((epoch, previous)) = self.past_documents.pop() {
            if let Some(current) = self.doc.replace(previous) {
                self.future_documents.push((self.current_epoch, current));
                self.current_epoch = epoch;
            }
        }
    }
    fn redo_document(&mut self) {
        if self.doc.as_ref().is_some_and(|d| d.can_redo()) {
            self.doc.as_mut().unwrap().redo();
        } else if let Some((epoch, next)) = self.future_documents.pop() {
            if let Some(current) = self.doc.replace(next) {
                self.past_documents.push((self.current_epoch, current));
                self.current_epoch = epoch;
            }
        }
    }
    fn request_render(&mut self) -> Result<(), String> {
        #[cfg(windows)]
        {
            if let Some((_, _, _, cancel)) = self.object_inspect.take() {
                cancel.store(true, std::sync::atomic::Ordering::Relaxed);
            }
            if let Some((_, _, _, _, cancel)) = self.object_edit.take() {
                cancel.store(true, std::sync::atomic::Ordering::Relaxed);
            }
            self.selected_object = None;
            if let Some((_, _, _, _, cancel)) = self.flatten_pending.take() {
                cancel.store(true, std::sync::atomic::Ordering::Relaxed);
            }
            self.selected_text = None;
            let doc = self.doc.as_ref().ok_or("Open a PDF first")?;
            let (bytes, _) = doc.save_snapshot()?;
            self.generation = self.generation.wrapping_add(1);
            self.rendered = None;
            self.render_error = None;
            let scale = if self.zoom == 0.0 { 1.0 } else { self.zoom };
            self.renderer
                .get_or_insert_with(render::Worker::new)
                .submit(render::Request {
                    generation: self.generation,
                    bytes,
                    page: doc.current() as u32,
                    scale,
                })?;
        }
        Ok(())
    }
}
impl win42::App for Pdf {
    fn supports_production(&self) -> bool {
        true
    }
    fn label_source(&self) -> Result<win42::LabelSource, String> {
        #[cfg(windows)]
        let text = self
            .selected_text
            .as_ref()
            .filter(|selection| {
                self.doc
                    .as_ref()
                    .is_some_and(|doc| doc.current() == selection.page)
            })
            .map(|selection| selection.text.clone())
            .unwrap_or_default();
        #[cfg(not(windows))]
        let text = String::new();
        Ok(win42::LabelSource::Text(text))
    }
    fn begin_async_open(&mut self, path: &Path) -> Result<bool, String> {
        self.cancel_async_open();
        self.opening_path = Some(path.to_path_buf());
        let path = path.to_path_buf();
        let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let token = cancel.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let result = (|| {
                if token.load(std::sync::atomic::Ordering::Relaxed) {
                    return Err("Open cancelled".into());
                }
                let meta = std::fs::metadata(&path).map_err(|e| e.to_string())?;
                if meta.len() > 256 * 1024 * 1024 {
                    return Err("PDF exceeds file limit".into());
                }
                use std::io::Read;
                let mut bytes = Vec::new();
                std::fs::File::open(&path)
                    .map_err(|e| e.to_string())?
                    .take(256 * 1024 * 1024 + 1)
                    .read_to_end(&mut bytes)
                    .map_err(|e| e.to_string())?;
                if bytes.len() > 256 * 1024 * 1024 {
                    return Err("PDF exceeds file limit".into());
                }
                let identity =
                    suite42_core::session::FileIdentity::from_bytes(&bytes, meta.modified().ok());
                open_attempt(bytes.into(), identity, None, &token)
            })();
            let _ = tx.send(result);
        });
        self.protected_open = Some((rx, cancel));
        Ok(true)
    }
    fn take_async_open_result(
        &mut self,
    ) -> Option<Result<suite42_core::session::FileIdentity, String>> {
        let (rx, _) = self.protected_open.as_ref()?;
        match rx.try_recv() {
            Ok(Ok(OpenAttempt::Ready(doc, id))) => {
                self.prepared_open = Some((doc, id.clone()));
                self.protected_open = None;
                Some(Ok(id))
            }
            Ok(Ok(OpenAttempt::Password {
                bytes,
                identity,
                incorrect,
            })) => {
                self.protected_open = None;
                self.open_challenge = Some(PasswordChallenge {
                    bytes,
                    identity,
                    incorrect,
                    prompt_pending: true,
                });
                None
            }
            Ok(Err(e)) => {
                self.protected_open = None;
                Some(Err(e))
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => None,
            Err(_) => {
                self.protected_open = None;
                Some(Err("PDF open worker stopped".into()))
            }
        }
    }
    fn take_async_open_password_request(&mut self) -> Option<win42::InputRequest> {
        let challenge = self.open_challenge.as_mut()?;
        if !challenge.prompt_pending {
            return None;
        }
        challenge.prompt_pending = false;
        Some(win42::InputRequest {
            title: if challenge.incorrect {
                "Incorrect password — try again"
            } else {
                "Open protected PDF"
            },
            fields: vec![("Password", String::new())],
        })
    }
    fn submit_async_open_password(&mut self, password: Option<&str>) -> Result<(), String> {
        let Some(password) = password else {
            self.cancel_async_open();
            return Ok(());
        };
        let challenge = self
            .open_challenge
            .take()
            .ok_or("No pending password challenge")?;
        let secret = Secret(password.to_owned());
        let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let token = cancel.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let result = open_attempt(challenge.bytes, challenge.identity, Some(secret), &token);
            let _ = tx.send(result);
        });
        self.protected_open = Some((rx, cancel));
        Ok(())
    }
    fn commit_async_open(&mut self) -> Result<u64, String> {
        let (doc, _) = self
            .prepared_open
            .take()
            .ok_or("No prepared protected PDF")?;
        #[cfg(windows)]
        let next_generation = {
            let (bytes, _) = doc.save_snapshot()?;
            let generation = self.generation.wrapping_add(1);
            let scale = if self.zoom == 0. { 1. } else { self.zoom };
            self.renderer
                .get_or_insert_with(render::Worker::new)
                .submit(render::Request {
                    generation,
                    bytes,
                    page: doc.current() as u32,
                    scale,
                })?;
            generation
        };
        self.encrypted_session = false;
        self.encrypted_current = None;
        self.pending_destination = None;
        self.output_password = None;
        self.protected_session = doc.was_password_protected();
        self.protected_readonly = !doc.allows_changes();
        self.doc = Some(doc);
        self.past_documents.clear();
        self.future_documents.clear();
        self.current_epoch = 0;
        self.saved_epoch = 0;
        self.next_epoch = 0;
        self.pending = None;
        self.protected_original = if self.protected_session {
            self.opening_path.take()
        } else {
            self.opening_path = None;
            None
        };
        self.operation_error = None;
        self.selected_annotation = None;
        self.tool = None;
        self.select_annotations = false;
        self.select_text = false;
        self.object_tool = false;
        self.redact_tool = false;
        self.annotation_drag = None;
        self.ink_points.clear();
        self.capture_signature = false;
        self.place_signature = false;
        self.pan = win42::Point::default();
        #[cfg(windows)]
        {
            if let Some((_, _, _, cancel)) = self.object_inspect.take() {
                cancel.store(true, std::sync::atomic::Ordering::Relaxed);
            }
            if let Some((_, _, _, _, cancel)) = self.object_edit.take() {
                cancel.store(true, std::sync::atomic::Ordering::Relaxed);
            }
            if let Some((_, _, _, _, cancel)) = self.flatten_pending.take() {
                cancel.store(true, std::sync::atomic::Ordering::Relaxed);
            }
            self.generation = next_generation;
            self.rendered = None;
            self.render_error = None;
            self.selected_text = None;
            self.selected_object = None;
            self.search_hits.clear();
        }
        Ok(0)
    }
    fn cancel_async_open(&mut self) {
        if let Some((_, token)) = self.protected_open.take() {
            token.store(true, std::sync::atomic::Ordering::Relaxed);
        }
        self.prepared_open = None;
        self.open_challenge = None;
        self.opening_path = None;
    }
    fn poll_background(&mut self) -> bool {
        #[cfg(windows)]
        {
            let mut changed = false;
            if let Some((rx, _, _, _)) = &self.object_inspect {
                if let Ok(result) = rx.try_recv() {
                    let (_, source, generation, _) = self.object_inspect.take().unwrap();
                    if generation == self.generation
                        && self
                            .doc
                            .as_ref()
                            .and_then(|d| d.save_snapshot().ok())
                            .is_some_and(|(bytes, _)| bytes == source)
                    {
                        match result {
                            Ok(selected) => {
                                self.selected_object = Some((selected, source));
                                self.operation_error = Some(
                                    "Text object selected. Choose Edit Selected Text Object."
                                        .into(),
                                );
                            }
                            Err(e) => self.operation_error = Some(e),
                        }
                    } else {
                        self.operation_error =
                            Some("Text selection cancelled because the document changed".into());
                    }
                    changed = true;
                }
            }
            if let Some((rx, _, _, _, _)) = &self.object_edit {
                if let Ok(result) = rx.try_recv() {
                    let (_, source, generation, page, _) = self.object_edit.take().unwrap();
                    if generation == self.generation
                        && self
                            .doc
                            .as_ref()
                            .and_then(|d| d.save_snapshot().ok())
                            .is_some_and(|(bytes, _)| bytes == source)
                    {
                        let result = result
                            .and_then(|bytes| PdfDocument::from_edited_snapshot(&bytes, page))
                            .and_then(|doc| self.install_replacement(doc));
                        match result {
                            Ok(()) => {
                                self.operation_error = None;
                                if let Err(e) = self.request_render() {
                                    self.operation_error = Some(e);
                                }
                            }
                            Err(e) => self.operation_error = Some(e),
                        }
                    } else {
                        self.operation_error =
                            Some("Text edit cancelled because the document changed".into());
                    }
                    changed = true;
                }
            }

            if let Some((rx, _, _, _, _)) = &self.export_pending {
                if let Ok(result) = rx.try_recv() {
                    let (_, _, _, _, path) = self.export_pending.take().unwrap();
                    self.operation_error = Some(match result {
                        Ok(()) => format!("Exported {}", path.display()),
                        Err(e) => e,
                    });
                    changed = true;
                } else {
                    changed = true;
                }
            }
            if let Some((rx, _, _, _)) = &self.print_pending {
                if let Ok(result) = rx.try_recv() {
                    self.print_pending = None;
                    self.print_ready = Some(result);
                    changed = true;
                } else {
                    changed = true;
                }
            }
            if let Some((rx, _, _, _, _)) = &self.flatten_pending {
                if let Ok(result) = rx.try_recv() {
                    let (_, source, generation, page, _) = self.flatten_pending.take().unwrap();
                    if generation == self.generation
                        && self
                            .doc
                            .as_ref()
                            .and_then(|d| d.save_snapshot().ok())
                            .is_some_and(|(bytes, _)| bytes == source)
                    {
                        let result = result.and_then(|pages| self.install_flattened(pages, page));
                        match result {
                            Ok(()) => {
                                self.search_hits.clear();
                                if let Err(e) = self.request_render() {
                                    self.operation_error = Some(e);
                                }
                            }
                            Err(e) => self.operation_error = Some(e),
                        }
                    } else {
                        self.operation_error = Some(
                            "Flattening cancelled because the document or view changed".into(),
                        );
                    }
                    changed = true;
                }
            }
            if let Some(worker) = &self.renderer {
                while let Ok(response) = worker.rx.try_recv() {
                    if response.generation == self.generation {
                        match response.result {
                            Ok(image) => self.rendered = Some(image),
                            Err(e) => self.render_error = Some(e),
                        }
                        changed = true;
                    }
                }
            }
            return changed;
        }
        #[cfg(not(windows))]
        {
            false
        }
    }
    fn take_pending_print_job(&mut self) -> Option<Result<win42::PrintJob, String>> {
        #[cfg(windows)]
        {
            self.print_ready.take()
        }
        #[cfg(not(windows))]
        {
            None
        }
    }
    fn canvas_scene(&self) -> Option<win42::CanvasScene> {
        #[cfg(windows)]
        {
            let (width, height, rgba) = self.rendered.as_ref()?;
            let mut items = vec![win42::SceneItem::Image {
                x: 0.,
                y: 0.,
                width: *width,
                height: *height,
                rgba: rgba.clone(),
            }];
            if let (Some(doc), Some(hit)) =
                (self.doc.as_ref(), self.search_hits.get(self.search_index))
            {
                if hit.page == doc.current() {
                    if let (Ok(b), Ok(rotation)) = (doc.page_box(hit.page), doc.rotation(hit.page))
                    {
                        let convert = |x: f64, y: f64| {
                            let x = (x - b[0]) / (b[2] - b[0]);
                            let y = (y - b[1]) / (b[3] - b[1]);
                            let (u, v) = match rotation {
                                90 => (y, x),
                                180 => (1. - x, y),
                                270 => (1. - y, 1. - x),
                                _ => (x, 1. - y),
                            };
                            win42::Point {
                                x: u * *width as f64,
                                y: v * *height as f64,
                            }
                        };
                        for rect in &hit.boxes {
                            items.push(win42::SceneItem::Path {
                                points: vec![
                                    convert(rect[0], rect[1]),
                                    convert(rect[2], rect[1]),
                                    convert(rect[2], rect[3]),
                                    convert(rect[0], rect[3]),
                                ],
                                color: [255, 0, 0],
                                closed: true,
                            });
                        }
                    }
                }
            }
            if let (Some(doc), Some(selection)) = (self.doc.as_ref(), self.selected_text.as_ref()) {
                if doc.current() == selection.page {
                    if let (Ok(b), Ok(rotation)) =
                        (doc.page_box(selection.page), doc.rotation(selection.page))
                    {
                        let cv = |x: f64, y: f64| {
                            let x = (x - b[0]) / (b[2] - b[0]);
                            let y = (y - b[1]) / (b[3] - b[1]);
                            let (u, v) = match rotation {
                                90 => (y, x),
                                180 => (1. - x, y),
                                270 => (1. - y, 1. - x),
                                _ => (x, 1. - y),
                            };
                            win42::Point {
                                x: u * *width as f64,
                                y: v * *height as f64,
                            }
                        };
                        for q in &selection.boxes {
                            items.push(win42::SceneItem::Path {
                                points: vec![
                                    cv(q[0], q[1]),
                                    cv(q[2], q[1]),
                                    cv(q[2], q[3]),
                                    cv(q[0], q[3]),
                                ],
                                color: [0, 80, 255],
                                closed: true,
                            });
                        }
                    }
                }
            }
            if self.ink_points.len() > 1 {
                items.push(win42::SceneItem::Path {
                    points: self.ink_points.clone(),
                    color: [255, 0, 0],
                    closed: false,
                });
            }
            Some(win42::CanvasScene {
                viewport: win42::Viewport {
                    zoom: 1.0,
                    pan: self.pan,
                },
                items,
            })
        }
        #[cfg(not(windows))]
        {
            None
        }
    }
    fn canvas_event(&mut self, event: win42::CanvasEvent) -> Result<(), String> {
        match event {
            win42::CanvasEvent::PointerDown(p) => {
                if self.protected_readonly && !self.select_text && !self.object_tool {
                    self.pan_drag = Some((p, self.pan));
                    return Ok(());
                }
                if self.tool.is_some()
                    || self.select_annotations
                    || self.redact_tool
                    || self.select_text
                    || self.object_tool
                    || self.place_signature
                {
                    self.annotation_drag = Some(p);
                    if self.tool == Some(pdf42_core::AnnotationKind::Ink) {
                        self.ink_points.clear();
                        self.ink_points.push(p);
                    }
                } else {
                    self.pan_drag = Some((p, self.pan));
                }
            }
            win42::CanvasEvent::PointerMove(p) => {
                if self.annotation_drag.is_some()
                    && self.tool == Some(pdf42_core::AnnotationKind::Ink)
                    && self.ink_points.len() < 100_000
                {
                    self.ink_points.push(p);
                }
                if let Some((anchor, _)) = self.pan_drag {
                    self.pan.x += p.x - anchor.x;
                    self.pan.y += p.y - anchor.y;
                }
            }
            win42::CanvasEvent::PointerUp(p) => {
                if let Some(anchor) = self.annotation_drag.take() {
                    #[cfg(windows)]
                    {
                        if self.redact_tool {
                            let (width, height, _) = self
                                .rendered
                                .as_ref()
                                .ok_or("Wait for the page to render")?;
                            let rect = [
                                anchor.x / *width as f64,
                                anchor.y / *height as f64,
                                p.x / *width as f64,
                                p.y / *height as f64,
                            ];
                            if (rect[0] - rect[2]).abs() < 0.001
                                || (rect[1] - rect[3]).abs() < 0.001
                            {
                                return Err("Drag a larger redaction area".into());
                            }
                            let doc = self.doc.as_ref().ok_or("Open a PDF first")?;
                            let page = doc.current();
                            let mut sizes = Vec::new();
                            for i in 0..doc.page_count() {
                                let b = doc.page_box(i)?;
                                let rotation = doc.rotation(i)?;
                                sizes.push(if rotation == 90 || rotation == 270 {
                                    (b[3] - b[1], b[2] - b[0])
                                } else {
                                    (b[2] - b[0], b[3] - b[1])
                                });
                            }
                            let (bytes, _) = doc.save_snapshot()?;
                            let cancel =
                                std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
                            let rx = render::flatten_async(
                                bytes.clone(),
                                sizes,
                                page,
                                rect,
                                cancel.clone(),
                            );
                            self.flatten_pending = Some((rx, bytes, self.generation, page, cancel));
                            self.operation_error = None;
                            self.redact_tool = false;
                            return Ok(());
                        }
                        let a = self.page_point(anchor)?;
                        let b = self.page_point(p)?;
                        if self.place_signature {
                            let shape = self
                                .signature_asset
                                .as_ref()
                                .ok_or("Draw a handwritten signature first")?;
                            let (left, right, bottom, top) = (
                                a[0].min(b[0]),
                                a[0].max(b[0]),
                                a[1].min(b[1]),
                                a[1].max(b[1]),
                            );
                            let scale = (right - left).min(top - bottom);
                            if scale < 2. {
                                return Err("Drag a larger signature area".into());
                            }
                            let points: Vec<_> = shape
                                .iter()
                                .map(|p| [left + p[0] * scale, bottom + p[1] * scale])
                                .collect();
                            let doc = self.doc.as_mut().ok_or("Open a PDF first")?;
                            doc.add_ink(doc.current(), &points)?;
                            self.search_hits.clear();
                            self.branch();
                            self.request_render()?;
                            return Ok(());
                        }
                        if self.object_tool {
                            if let Some((_, _, _, cancel)) = self.object_inspect.take() {
                                cancel.store(true, std::sync::atomic::Ordering::Relaxed);
                            }
                            if let Some((_, _, _, _, cancel)) = self.object_edit.take() {
                                cancel.store(true, std::sync::atomic::Ordering::Relaxed);
                            }

                            let doc = self.doc.as_ref().ok_or("Open a PDF first")?;
                            let (bytes, _) = doc.save_snapshot()?;
                            let cancel =
                                std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
                            let rx = pdfium_edit::inspect_async(
                                bytes.clone(),
                                doc.current(),
                                a,
                                cancel.clone(),
                            );
                            self.object_inspect = Some((rx, bytes, self.generation, cancel));
                            self.operation_error = Some("Inspecting text object…".into());
                            return Ok(());
                        }
                        if self.select_text {
                            let doc = self.doc.as_ref().ok_or("Open a PDF first")?;
                            let (bytes, _) = doc.save_snapshot()?;
                            self.selected_text = Some(search::select(
                                &bytes,
                                doc.current(),
                                [a[0], a[1], b[0], b[1]],
                            )?);
                            return Ok(());
                        }
                        if self.select_annotations {
                            let doc = self.doc.as_mut().ok_or("Open a PDF first")?;
                            self.selected_annotation = doc.hit_annotation(doc.current(), a);
                            if let Some(index) = self.selected_annotation {
                                if (a[0] - b[0]).abs() > 1. || (a[1] - b[1]).abs() > 1. {
                                    doc.move_annotation(index, [b[0] - a[0], b[1] - a[1]])?;
                                    self.branch();
                                    self.request_render()?;
                                }
                            }
                            return Ok(());
                        }
                        if self.tool == Some(pdf42_core::AnnotationKind::ImageStamp) {
                            let image = self
                                .stamp_image
                                .clone()
                                .ok_or("Choose a stamp image first")?;
                            let doc = self.doc.as_mut().ok_or("Open a PDF first")?;
                            doc.add_image_stamp(doc.current(), [a[0], a[1], b[0], b[1]], image)?;
                            self.search_hits.clear();
                            self.branch();
                            self.request_render()?;
                            return Ok(());
                        }
                        if self.tool == Some(pdf42_core::AnnotationKind::Ink) {
                            self.ink_points.push(p);
                            let points = self
                                .ink_points
                                .iter()
                                .map(|point| self.page_point(*point))
                                .collect::<Result<Vec<_>, _>>()?;
                            if self.capture_signature {
                                let mut bounds = [
                                    f64::INFINITY,
                                    f64::INFINITY,
                                    f64::NEG_INFINITY,
                                    f64::NEG_INFINITY,
                                ];
                                for p in &points {
                                    bounds[0] = bounds[0].min(p[0]);
                                    bounds[1] = bounds[1].min(p[1]);
                                    bounds[2] = bounds[2].max(p[0]);
                                    bounds[3] = bounds[3].max(p[1]);
                                }
                                let extent = (bounds[2] - bounds[0]).max(bounds[3] - bounds[1]);
                                if extent < 0.5 {
                                    return Err("Draw a longer handwritten signature".into());
                                }
                                self.signature_asset = Some(
                                    points
                                        .iter()
                                        .map(|p| {
                                            [
                                                (p[0] - bounds[0]) / extent,
                                                (p[1] - bounds[1]) / extent,
                                            ]
                                        })
                                        .collect(),
                                );
                                self.capture_signature = false;
                            }
                            self.ink_points.clear();
                            let doc = self.doc.as_mut().ok_or("Open a PDF first")?;
                            doc.add_ink(doc.current(), &points)?;
                            self.search_hits.clear();
                            self.branch();
                            self.request_render()?;
                            return Ok(());
                        }
                        let kind = self.tool.ok_or("Select an annotation tool")?;
                        let doc = self.doc.as_mut().ok_or("Open a PDF first")?;
                        self.search_hits.clear();
                        doc.add_annotation(
                            doc.current(),
                            kind,
                            [a[0], a[1], b[0], b[1]],
                            &self.annotation_contents,
                        )?;
                        self.branch();
                        self.request_render()?;
                    }
                    #[cfg(not(windows))]
                    let _ = (anchor, p);
                }
                self.pan_drag = None;
            }
            win42::CanvasEvent::Cancel => {
                self.annotation_drag = None;
                self.ink_points.clear();
                if let Some((_, original)) = self.pan_drag.take() {
                    self.pan = original;
                }
            }
            _ => {}
        }
        Ok(())
    }
    fn multiline_input(&self, id: u16) -> bool {
        matches!(id, 25 | 29 | 31 | 71 | 75)
    }
    fn password_input(&self, id: u16) -> bool {
        id == 76
    }
    fn input_request(&self, id: u16) -> Option<win42::InputRequest> {
        if id == 76 && self.doc.is_some() {
            return Some(win42::InputRequest {
                title: "Set password for encrypted PDF copy (AES-128)",
                fields: vec![("Password (1–32 printable ASCII characters)", String::new())],
            });
        }
        let doc = self.doc.as_ref()?;
        #[cfg(windows)]
        if id == 71 || id == 75 {
            let (object, _) = self.selected_object.as_ref()?;
            return Some(win42::InputRequest {
                title: if id == 75 {
                    "Replace object text with Noto (font appearance changes)"
                } else {
                    "Edit existing PDF text object (empty deletes it)"
                },
                fields: vec![("Text", object.text.clone())],
            });
        }
        if id == 59 {
            return Some(win42::InputRequest {
                title: "Image export range",
                fields: vec![
                    ("First page", self.export_first.max(1).to_string()),
                    (
                        "Last page",
                        if self.export_last == 0 {
                            doc.page_count()
                        } else {
                            self.export_last
                        }
                        .to_string(),
                    ),
                    (
                        "DPI (72–300)",
                        if self.export_dpi == 0 {
                            150
                        } else {
                            self.export_dpi
                        }
                        .to_string(),
                    ),
                ],
            });
        }
        if (54..=58).contains(&id) || id == 79 {
            let names = doc.bookmarks().ok()?;
            let mut fields = Vec::new();
            if id == 54 {
                fields.push(("Title", String::new()));
            } else {
                fields.push(("Bookmark number", "1".into()));
                if id == 79 {
                    fields.push(("Child title", String::new()));
                }
                if id == 56 {
                    fields.push(("New title", names.first().cloned().unwrap_or_default()));
                }
                if id == 58 {
                    fields.push(("Move to number", "1".into()));
                }
                fields.push((
                    "Bookmarks",
                    names
                        .iter()
                        .enumerate()
                        .map(|(i, n)| format!("{}: {}", i + 1, n))
                        .collect::<Vec<_>>()
                        .join("; "),
                ));
            }
            return Some(win42::InputRequest {
                title: match id {
                    54 => "Add bookmark",
                    55 => "Go to bookmark",
                    56 => "Rename bookmark",
                    57 => "Delete bookmark and children",
                    79 => "Add child bookmark",
                    _ => "Move bookmark",
                },
                fields,
            });
        }
        if id == 52 {
            return Some(win42::InputRequest {
                title: "Print PDF pages",
                fields: vec![
                    ("First page", "1".into()),
                    ("Last page", doc.page_count().to_string()),
                    ("DPI (72–300)", "150".into()),
                ],
            });
        }
        if id == 49 || id == 50 {
            let names = doc.attachments().ok()?;
            let first = names.first()?;
            let selected = if names.contains(&self.attachment_name) {
                self.attachment_name.clone()
            } else {
                first.clone()
            };
            let mut fields = vec![("Attachment name", selected.clone())];
            if id == 49 {
                fields.push(("Export path", selected));
            }
            fields.push(("Available attachments", names.join(", ")));
            return Some(win42::InputRequest {
                title: if id == 49 {
                    "Export attachment"
                } else {
                    "Delete attachment"
                },
                fields,
            });
        }
        if id == 36 {
            return Some(win42::InputRequest {
                title: "Find PDF text",
                fields: vec![("Search text", self.search_query.clone())],
            });
        }
        if id == 34 {
            let fields = doc.form_fields();
            let (name, _, value) = fields.first()?.clone();
            let kind = fields
                .iter()
                .map(|(n, k, _)| format!("{n} ({k})"))
                .collect::<Vec<_>>()
                .join(", ");
            return Some(win42::InputRequest {
                title: "Edit form field",
                fields: vec![
                    ("Field name", name),
                    ("Value", value),
                    ("Available fields", kind),
                ],
            });
        }
        if id == 31 {
            return Some(win42::InputRequest {
                title: "Edit note or text",
                fields: vec![(
                    "Text",
                    self.selected_annotation
                        .and_then(|a| doc.annotation_contents(a))
                        .unwrap_or_default(),
                )],
            });
        }
        if matches!(id, 25 | 26 | 29 | 30) {
            return Some(win42::InputRequest {
                title: match id {
                    25 => "Note text",
                    26 => "Link destination",
                    29 => "Text annotation",
                    _ => "Stamp text",
                },
                fields: vec![(
                    if id == 26 { "HTTP(S) URL" } else { "Text" },
                    self.annotation_contents.clone(),
                )],
            });
        }
        if id == 21 {
            let b = doc.page_box(doc.current()).ok()?;
            return Some(win42::InputRequest {
                title: "Crop page (PDF points)",
                fields: vec![
                    ("Left", b[0].to_string()),
                    ("Bottom", b[1].to_string()),
                    ("Right", b[2].to_string()),
                    ("Top", b[3].to_string()),
                ],
            });
        }
        if id != 14 {
            return None;
        }
        Some(win42::InputRequest {
            title: "Document properties",
            fields: vec![
                (
                    "Title",
                    doc.metadata("Title").ok().flatten().unwrap_or_default(),
                ),
                (
                    "Author",
                    doc.metadata("Author").ok().flatten().unwrap_or_default(),
                ),
                (
                    "Subject",
                    doc.metadata("Subject").ok().flatten().unwrap_or_default(),
                ),
                (
                    "Keywords",
                    doc.metadata("Keywords").ok().flatten().unwrap_or_default(),
                ),
            ],
        })
    }
    fn input_action(&mut self, id: u16, values: &[String]) -> Result<(), String> {
        if id == 76 && values.len() == 1 {
            let password = &values[0];
            if password.is_empty()
                || password.len() > 32
                || !password.bytes().all(|b| (32..=126).contains(&b))
            {
                return Err("Password must be 1–32 printable ASCII characters".into());
            }
            self.output_password = Some(Secret(password.clone()));
            self.operation_error =
                Some("Password set. Choose Password-protected PDF in Save As.".into());
            return Ok(());
        }
        if (id == 71 || id == 75) && values.len() == 1 {
            #[cfg(windows)]
            {
                let (selected, source) = self
                    .selected_object
                    .take()
                    .ok_or("Select a text object first")?;
                let doc = self.doc.as_ref().ok_or("Open a PDF first")?;
                if doc.save_snapshot()?.0 != source {
                    return Err("Document changed; select the text object again".into());
                }
                let page = selected.page;
                let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
                let rx = pdfium_edit::edit_async(
                    source.clone(),
                    selected,
                    values[0].clone(),
                    id == 75,
                    cancel.clone(),
                );
                self.object_edit = Some((rx, source, self.generation, page, cancel));
                self.operation_error = Some("Applying text edit…".into());
                return Ok(());
            }
            #[cfg(not(windows))]
            return Err("PDFium text editing requires Windows".into());
        }
        if id == 59 && values.len() == 3 {
            let first: usize = values[0]
                .trim()
                .parse()
                .map_err(|_| "Enter a valid first page")?;
            let last: usize = values[1]
                .trim()
                .parse()
                .map_err(|_| "Enter a valid last page")?;
            let dpi: u32 = values[2].trim().parse().map_err(|_| "Enter a valid DPI")?;
            if first == 0
                || first > last
                || last > self.doc.as_ref().ok_or("Open a PDF first")?.page_count()
                || !(72..=300).contains(&dpi)
            {
                return Err("Enter a valid page range and DPI between 72 and 300".into());
            }
            self.export_first = first;
            self.export_last = last;
            self.export_dpi = dpi;
            return Ok(());
        }
        if (54..=58).contains(&id) || id == 79 {
            let doc = self.doc.as_mut().ok_or("Open a PDF first")?;
            if id == 54 {
                let title = values.first().ok_or("Enter a title")?;
                doc.add_bookmark(title)?;
            } else {
                let index: usize = values
                    .first()
                    .ok_or("Enter a bookmark number")?
                    .trim()
                    .parse()
                    .map_err(|_| "Enter a valid bookmark number")?;
                let index = index.checked_sub(1).ok_or("Bookmark numbers start at 1")?;
                match id {
                    55 => doc.select_bookmark(index)?,
                    56 => doc.rename_bookmark(index, values.get(1).ok_or("Enter a title")?)?,
                    57 => doc.delete_bookmark(index)?,
                    79 => doc.add_child_bookmark(index, values.get(1).ok_or("Enter a title")?)?,
                    58 => {
                        let to: usize = values
                            .get(1)
                            .ok_or("Enter a destination")?
                            .trim()
                            .parse()
                            .map_err(|_| "Enter a valid destination")?;
                        doc.move_bookmark(
                            index,
                            to.checked_sub(1).ok_or("Bookmark numbers start at 1")?,
                        )?;
                    }
                    _ => {}
                }
            }
            if id != 55 {
                self.branch();
            }
            return self.request_render();
        }
        if id == 52 && values.len() == 3 {
            let first: usize = values[0]
                .trim()
                .parse()
                .map_err(|_| "Enter a valid first page")?;
            let last: usize = values[1]
                .trim()
                .parse()
                .map_err(|_| "Enter a valid last page")?;
            let dpi: u32 = values[2].trim().parse().map_err(|_| "Enter a valid DPI")?;
            if first == 0 || first > last || !(72..=300).contains(&dpi) {
                return Err("Enter an increasing page range and DPI between 72 and 300".into());
            }
            #[cfg(windows)]
            {
                let doc = self.doc.as_ref().ok_or("Open a PDF first")?;
                if last > doc.page_count() {
                    return Err("Page range exceeds the document".into());
                }
                let mut pages = Vec::new();
                for i in first - 1..last {
                    let b = doc.page_box(i)?;
                    let angle = doc.rotation(i)?;
                    let (w, h) = if angle == 90 || angle == 270 {
                        (b[3] - b[1], b[2] - b[0])
                    } else {
                        (b[2] - b[0], b[3] - b[1])
                    };
                    pages.push((i, w, h));
                }
                let (bytes, _) = doc.save_snapshot()?;
                let progress = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
                let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
                let total = pages.len();
                let rx = render::print_async(bytes, pages, dpi, progress.clone(), cancel.clone());
                self.print_pending = Some((rx, progress, cancel, total));
                return Ok(());
            }
            #[cfg(not(windows))]
            return Err("PDF printing requires Windows".into());
        }
        if (id == 49 && values.len() == 3) || (id == 50 && values.len() == 2) {
            self.attachment_name = values[0].clone();
            if id == 49 {
                return self
                    .doc
                    .as_ref()
                    .ok_or("Open a PDF first")?
                    .export_attachment(&values[0], Path::new(&values[1]));
            }
            self.doc
                .as_mut()
                .ok_or("Open a PDF first")?
                .delete_attachment(&values[0])?;
            self.branch();
            return self.request_render();
        }
        if id == 36 && values.len() == 1 {
            self.search_query = values[0].clone();
            #[cfg(windows)]
            {
                let doc = self.doc.as_ref().ok_or("Open a PDF first")?;
                let (bytes, _) = doc.save_snapshot()?;
                self.search_hits = search::find(&bytes, &self.search_query, doc.page_count())?;
                self.search_index = 0;
                let first = self
                    .search_hits
                    .first()
                    .ok_or("No matching text found")?
                    .page;
                self.doc.as_mut().unwrap().select(first)?;
                return self.request_render();
            }
            #[cfg(not(windows))]
            return Err("PDF text search is available on Windows with PDFium".into());
        }
        if id == 34 && values.len() == 3 {
            self.doc
                .as_mut()
                .ok_or("Open a PDF first")?
                .set_form_value(&values[0], &values[1])?;
            self.branch();
            return self.request_render();
        }
        if id == 31 && values.len() == 1 {
            let selected = self.selected_annotation.ok_or("Select a note first")?;
            self.doc
                .as_mut()
                .ok_or("Open a PDF first")?
                .set_note_text(selected, &values[0])?;
            self.branch();
            return self.request_render();
        }
        if matches!(id, 25 | 26 | 29 | 30) && values.len() == 1 {
            if values[0].trim().is_empty() {
                return Err("Enter a value".into());
            }
            if id == 26 && !(values[0].starts_with("https://") || values[0].starts_with("http://"))
            {
                return Err("Enter an http or https link".into());
            }
            self.annotation_contents = values[0].clone();
            self.place_signature = false;
            self.capture_signature = false;
            self.object_tool = false;
            self.select_text = false;
            self.select_annotations = false;
            self.redact_tool = false;
            self.selected_annotation = None;
            self.tool = Some(match id {
                25 => pdf42_core::AnnotationKind::Note,
                26 => pdf42_core::AnnotationKind::Link,
                29 => pdf42_core::AnnotationKind::FreeText,
                _ => pdf42_core::AnnotationKind::Stamp,
            });
            return Ok(());
        }
        if id == 21 && values.len() == 4 {
            let mut crop = [0.; 4];
            for (slot, value) in crop.iter_mut().zip(values) {
                *slot = value
                    .trim()
                    .parse()
                    .map_err(|_| "Enter numeric PDF point coordinates")?;
            }
            let doc = self.doc.as_mut().ok_or("Open a PDF first")?;
            doc.crop_page(doc.current(), crop)?;
            self.branch();
            return self.request_render();
        }
        if id != 14 || values.len() != 4 {
            return Err("Invalid properties".into());
        }
        let pairs = ["Title", "Author", "Subject", "Keywords"]
            .iter()
            .zip(values)
            .map(|(k, v)| ((*k).to_owned(), v.clone()))
            .collect::<Vec<_>>();
        self.doc
            .as_mut()
            .ok_or("Open a PDF first")?
            .set_metadata(&pairs)?;
        self.branch();
        self.request_render()
    }
    fn mark_file_action(&self) -> Option<u16> {
        Some(74)
    }
    fn file_actions(&self) -> &'static [win42::FileAction] {
        &[
            win42::FileAction {
                id: 74,
                title: "Choose Image Stamp…",
                save: false,
                extension: "",
                format: "PNG or JPEG image",
                pattern: "*.png;*.jpg;*.jpeg",
            },
            win42::FileAction {
                id: 60,
                title: "Export PNG Pages to ZIP…",
                save: true,
                extension: "zip",
                format: "ZIP archive",
                pattern: "*.zip",
            },
            win42::FileAction {
                id: 61,
                title: "Export JPEG Pages to ZIP…",
                save: true,
                extension: "zip",
                format: "ZIP archive",
                pattern: "*.zip",
            },
            win42::FileAction {
                id: 48,
                title: "Add Attachment…",
                save: false,
                extension: "",
                format: "All files",
                pattern: "*.*",
            },
            win42::FileAction {
                id: 33,
                title: "Extract Current Page…",
                save: true,
                extension: "pdf",
                format: "PDF document",
                pattern: "*.pdf",
            },
            win42::FileAction {
                id: 35,
                title: "Insert PDF Pages…",
                save: false,
                extension: "pdf",
                format: "PDF document",
                pattern: "*.pdf",
            },
        ]
    }
    fn file_action(&mut self, id: u16, path: &Path) -> Result<(), String> {
        if id == 74 {
            if std::fs::metadata(path).map_err(|e| e.to_string())?.len() > 16 * 1024 * 1024 {
                return Err("Stamp image exceeds 16 MB".into());
            }
            let data = std::fs::read(path).map_err(|e| e.to_string())?;
            if data.len() > 16 * 1024 * 1024 {
                return Err("Stamp image exceeds 16 MB".into());
            }
            let format = image::guess_format(&data).map_err(|e| e.to_string())?;
            if !matches!(format, image::ImageFormat::Png | image::ImageFormat::Jpeg) {
                return Err("Choose a PNG or JPEG stamp image".into());
            }
            let mut reader = image::ImageReader::with_format(std::io::Cursor::new(data), format);
            let mut limits = image::Limits::default();
            limits.max_image_width = Some(4096);
            limits.max_image_height = Some(4096);
            limits.max_alloc = Some(32 * 1024 * 1024);
            reader.limits(limits);
            let decoded = reader.decode().map_err(|e| e.to_string())?.to_rgba8();
            let (width, height) = decoded.dimensions();
            if width as u64 * height as u64 > 4_000_000 {
                return Err("Stamp image exceeds four million pixels".into());
            }
            self.stamp_image = Some(pdf42_core::ImageStamp {
                width,
                height,
                rgba: decoded.into_raw().into(),
            });
            self.tool = Some(pdf42_core::AnnotationKind::ImageStamp);
            self.place_signature = false;
            self.capture_signature = false;
            self.object_tool = false;
            self.select_annotations = false;
            self.select_text = false;
            self.redact_tool = false;
            self.operation_error=Some("Drag a rectangle to place the image stamp; its aspect ratio and transparency are retained.".into());
            return Ok(());
        }
        let doc = self.doc.as_mut().ok_or("Open a PDF first")?;
        match id {
            60 | 61 => {
                #[cfg(windows)]
                {
                    let first = self.export_first.max(1);
                    let last = if self.export_last == 0 {
                        doc.page_count()
                    } else {
                        self.export_last
                    };
                    if first > last || last > doc.page_count() {
                        return Err("Choose an image export range for this document".into());
                    }
                    let dpi = if self.export_dpi == 0 {
                        150
                    } else {
                        self.export_dpi
                    };
                    let mut pages = Vec::new();
                    for i in first - 1..last {
                        let b = doc.page_box(i)?;
                        let angle = doc.rotation(i)?;
                        let (w, h) = if angle == 90 || angle == 270 {
                            (b[3] - b[1], b[2] - b[0])
                        } else {
                            (b[2] - b[0], b[3] - b[1])
                        };
                        pages.push((i, w, h));
                    }
                    let (bytes, _) = doc.save_snapshot()?;
                    let progress = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
                    let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
                    let total = pages.len();
                    let rx = render::export_async(
                        bytes,
                        pages,
                        dpi,
                        id == 61,
                        path.to_path_buf(),
                        progress.clone(),
                        cancel.clone(),
                    );
                    self.export_pending = Some((rx, progress, cancel, total, path.to_path_buf()));
                    self.operation_error = None;
                    return Ok(());
                }
                #[cfg(not(windows))]
                {
                    Err("PDF image export requires Windows".into())
                }
            }
            48 => {
                doc.add_attachment(path)?;
                self.branch();
                self.request_render()
            }
            33 => doc.extract_page(doc.current(), path),
            35 => {
                doc.insert_pdf_after(doc.current(), path)?;
                self.branch();
                self.request_render()
            }
            _ => Err("Unsupported file action".into()),
        }
    }
    fn clipboard_request(&self, id: u16) -> Option<win42::ClipboardRequest> {
        #[cfg(windows)]
        if id == 44 {
            return self
                .selected_text
                .as_ref()
                .map(|s| win42::ClipboardRequest::WriteText(s.text.clone()));
        }
        let _ = id;
        None
    }
    fn open_formats(&self) -> Vec<win42::FileFormat> {
        vec![win42::FileFormat {
            id: "pdf",
            title: "PDF document",
            extensions: &["pdf"],
        }]
    }
    fn save_formats(&self) -> Vec<win42::FileFormat> {
        vec![
            win42::FileFormat {
                id: "pdf",
                title: if self.protected_session || self.encrypted_session {
                    "Decrypted PDF copy (password removed)"
                } else {
                    "PDF document"
                },
                extensions: &["pdf"],
            },
            win42::FileFormat {
                id: "pdf-encrypted",
                title: "Password-protected PDF (AES-128; all permissions; set password first)",
                extensions: &["pdf"],
            },
        ]
    }
    fn can_save_in_place(&self) -> bool {
        !self.protected_session && (!self.encrypted_session || self.output_password.is_some())
    }
    fn save_snapshot_to(
        &mut self,
        format: &str,
        path: &Path,
        is_current: bool,
    ) -> Result<Option<(u64, Vec<u8>)>, String> {
        let protected_source = self
            .protected_original
            .as_ref()
            .or(self.encrypted_current.as_ref());
        if let Some(original) = protected_source {
            let same_path = is_current
                || std::fs::canonicalize(path)
                    .ok()
                    .zip(std::fs::canonicalize(original).ok())
                    .is_some_and(|(a, b)| a == b);
            let same_identity = suite42_core::session::FileIdentity::read(path)
                .ok()
                .zip(suite42_core::session::FileIdentity::read(original).ok())
                .is_some_and(|(a, b)| a == b);
            if (same_path || same_identity) && (self.protected_session || format != "pdf-encrypted")
            {
                return Err("The protected PDF cannot be overwritten with this output. Choose another file.".into());
            }
        }
        let result = self.save_snapshot(format);
        if result.is_ok() {
            self.pending_destination = Some(path.to_path_buf());
        }
        result
    }
    fn save_snapshot(&mut self, format: &str) -> Result<Option<(u64, Vec<u8>)>, String> {
        if self.protected_readonly {
            return Err("Document permissions do not allow saving changes".into());
        }
        let doc = self.doc.as_ref().ok_or("No document")?;
        let encrypted = format == "pdf-encrypted";
        let (bytes, state) = match format {
            "pdf" => doc.save_snapshot()?,
            "pdf-encrypted" => doc.save_encrypted_snapshot(
                &self
                    .output_password
                    .as_ref()
                    .ok_or("Set a password for the encrypted copy first")?
                    .0,
            )?,
            _ => return Err("Unsupported save format".into()),
        };
        self.next_save = self.next_save.wrapping_add(1);
        let revision = self.next_save;
        self.pending = Some((revision, self.current_epoch, state, encrypted));
        Ok(Some((revision, bytes)))
    }
    fn saved_snapshot(&mut self, revision: u64) {
        if self
            .pending
            .as_ref()
            .is_some_and(|(r, _, _, _)| *r == revision)
        {
            if let Some((_, epoch, state, encrypted)) = self.pending.take() {
                let target = if epoch == self.current_epoch {
                    self.doc.as_mut()
                } else {
                    self.past_documents
                        .iter_mut()
                        .chain(self.future_documents.iter_mut())
                        .find(|(e, _)| *e == epoch)
                        .map(|(_, d)| d)
                };
                if let Some(doc) = target {
                    doc.mark_saved_snapshot(state);
                    self.saved_epoch = epoch;
                    self.protected_session = false;
                    self.protected_readonly = false;
                    self.protected_original = None;
                    self.encrypted_session = encrypted;
                    self.encrypted_current = if encrypted {
                        self.pending_destination.take()
                    } else {
                        self.pending_destination = None;
                        None
                    };
                }
            }
        }
    }

    fn name(&self) -> &'static str {
        "PDF42"
    }
    fn extension(&self) -> &'static str {
        "pdf"
    }
    fn format_name(&self) -> &'static str {
        "PDF document"
    }
    fn actions(&self) -> &'static [(u16, &'static str)] {
        &[
            (10, "Rotate &Right"),
            (11, "&Delete Page"),
            (12, "Move Page &Up"),
            (13, "Move Page D&own"),
            (14, "Document &Properties…"),
            (15, "Zoom In"),
            (16, "Zoom Out"),
            (17, "Pan Tool"),
            (18, "Draw Rectangle"),
            (19, "Draw Ellipse"),
            (20, "Draw Line"),
            (39, "Draw Freehand Ink"),
            (40, "Select Text"),
            (41, "Highlight Selected Text"),
            (42, "Underline Selected Text"),
            (43, "Strike Out Selected Text"),
            (44, "Copy Selected Text"),
            (49, "Export Attachment…"),
            (50, "Delete Attachment…"),
            (52, "Print Pages…"),
            (53, "Cancel Print Preparation"),
            (54, "Add Bookmark…"),
            (55, "Go to Bookmark…"),
            (56, "Rename Bookmark…"),
            (57, "Delete Bookmark…"),
            (58, "Move Bookmark…"),
            (79, "Add Child Bookmark…"),
            (59, "Set Image Export Range…"),
            (62, "Cancel Image Export"),
            (63, "Compress PDF Streams"),
            (70, "Select Existing Text Object"),
            (71, "Edit Selected Text Object…"),
            (75, "Replace Text Object Font with Noto…"),
            (76, "Set Password for Encrypted Copy…"),
            (77, "Draw Reusable Handwritten Signature (not digital)"),
            (78, "Place Handwritten Signature (not digital)"),
            (21, "Crop Page…"),
            (22, "Highlight Area"),
            (23, "Underline Area"),
            (24, "Strike Out Area"),
            (25, "Note…"),
            (26, "Link…"),
            (27, "Select Annotation"),
            (28, "Delete Selected Annotation"),
            (29, "Text…"),
            (30, "Stamp…"),
            (31, "Edit Selected Note or Text…"),
            (32, "Insert Blank Page"),
            (34, "Edit Form Field…"),
            (36, "Find Text…"),
            (37, "Next Search Result"),
            (
                38,
                "Flatten All Pages (removes text, forms, metadata) and Redact",
            ),
        ]
    }
    fn items(&self) -> Vec<String> {
        self.doc
            .as_ref()
            .map(|d| {
                (0..d.page_count())
                    .map(|i| format!("Page {} — {}°", i + 1, d.rotation(i).unwrap_or(0)))
                    .collect()
            })
            .unwrap_or_default()
    }
    fn text(&self) -> String {
        #[cfg(windows)]
        if let Some(error) = &self.render_error {
            return format!("Page rendering failed: {error}");
        }
        #[cfg(windows)]
        if self.doc.is_some() {
            return "Rendering page…".into();
        }
        "Open a PDF document.".into()
    }
    fn status(&self) -> String {
        if let Some(error) = &self.operation_error {
            return error.clone();
        }
        if self.protected_readonly {
            return "Protected PDF: document permissions make this session read-only".into();
        }
        if self.protected_session {
            return "Protected PDF: choose Save As decrypted or set a password for an encrypted copy; the original stays protected".into();
        }
        if self.encrypted_session {
            return "Encrypted PDF copy: Save preserves password protection".into();
        }
        if self.redact_tool {
            return "Drag the area to redact. All pages will be flattened at 150 DPI; text, forms, links and metadata will be removed.".into();
        }
        #[cfg(windows)]
        if let Some((_, progress, _, total)) = &self.print_pending {
            return format!(
                "Preparing print: {} of {} pages",
                progress.load(std::sync::atomic::Ordering::Relaxed),
                total
            );
        }
        #[cfg(windows)]
        if let Some((_, progress, _, total, _)) = &self.export_pending {
            return format!(
                "Exporting images: {} of {} pages",
                progress.load(std::sync::atomic::Ordering::Relaxed),
                total
            );
        }
        #[cfg(windows)]
        if self.flatten_pending.is_some() {
            return "Flattening all pages at 150 DPI…".into();
        }
        match &self.doc {
            Some(d) => format!(
                "Page {} of {} | {}° | Title: {} | Author: {} | Tool: {} | {}",
                d.current() + 1,
                d.page_count(),
                d.rotation(d.current()).unwrap_or(0),
                d.metadata("Title").ok().flatten().unwrap_or_default(),
                d.metadata("Author").ok().flatten().unwrap_or_default(),
                match self.tool {
                    None => "Pan",
                    Some(pdf42_core::AnnotationKind::Rectangle) => "Rectangle",
                    Some(pdf42_core::AnnotationKind::Ellipse) => "Ellipse",
                    Some(pdf42_core::AnnotationKind::Line) => "Line",
                    Some(pdf42_core::AnnotationKind::Ink) => "Ink",
                    Some(pdf42_core::AnnotationKind::Highlight) => "Highlight",
                    Some(pdf42_core::AnnotationKind::Underline) => "Underline",
                    Some(pdf42_core::AnnotationKind::StrikeOut) => "Strike out",
                    Some(pdf42_core::AnnotationKind::Note) => "Note",
                    Some(pdf42_core::AnnotationKind::Link) => "Link",
                    Some(pdf42_core::AnnotationKind::FreeText) => "Text",
                    Some(pdf42_core::AnnotationKind::Stamp) => "Stamp",
                    Some(pdf42_core::AnnotationKind::ImageStamp) => "Image stamp",
                },
                self.selected_annotation
                    .and_then(|selection| d.annotation_contents(selection))
                    .unwrap_or_default()
            ),
            None => "Open a PDF document".into(),
        }
    }
    fn can_save(&self) -> bool {
        self.doc.is_some() && !self.protected_readonly
    }
    fn selected(&self) -> usize {
        self.doc.as_ref().map_or(0, |d| d.current())
    }
    fn enabled(&self, id: u16) -> bool {
        let Some(d) = self.doc.as_ref() else {
            return false;
        };
        if self.protected_readonly
            && !matches!(
                id,
                15 | 16 | 17 | 36 | 37 | 40 | 44 | 52 | 53 | 55 | 59 | 60 | 61 | 62 | 70
            )
        {
            return false;
        }
        let n = d.page_count();
        let index = d.current();
        match id {
            10 => index < n,
            11 => d.can_reorder() && n > 1 && index < n,
            12 => d.can_reorder() && index > 0 && index < n,
            13 => d.can_reorder() && index + 1 < n,
            28 | 31 => self.selected_annotation.is_some(),
            32 | 35 => d.can_reorder(),
            33 => d.can_reorder(),
            38 => true,
            34 => !d.form_fields().is_empty(),
            78 => self.signature_asset.is_some(),
            71 | 75 => {
                #[cfg(windows)]
                {
                    self.selected_object.is_some() && self.object_edit.is_none()
                }
                #[cfg(not(windows))]
                {
                    false
                }
            }
            49 | 50 => d.attachments().is_ok_and(|v| !v.is_empty()),
            55..=58 | 79 => d.bookmarks().is_ok_and(|v| !v.is_empty()),
            60 | 61 => {
                #[cfg(windows)]
                {
                    self.export_pending.is_none()
                }
                #[cfg(not(windows))]
                {
                    false
                }
            }
            62 => {
                #[cfg(windows)]
                {
                    self.export_pending.is_some()
                }
                #[cfg(not(windows))]
                {
                    false
                }
            }
            52 => {
                #[cfg(windows)]
                {
                    self.print_pending.is_none()
                }
                #[cfg(not(windows))]
                {
                    false
                }
            }
            53 => {
                #[cfg(windows)]
                {
                    self.print_pending.is_some()
                }
                #[cfg(not(windows))]
                {
                    false
                }
            }
            41..=44 => {
                #[cfg(windows)]
                {
                    self.selected_text
                        .as_ref()
                        .is_some_and(|s| s.page == d.current())
                }
                #[cfg(not(windows))]
                {
                    false
                }
            }
            37 => {
                #[cfg(windows)]
                {
                    !self.search_hits.is_empty()
                }
                #[cfg(not(windows))]
                {
                    false
                }
            }
            _ => true,
        }
    }
    fn dirty(&self) -> bool {
        self.current_epoch != self.saved_epoch || self.doc.as_ref().is_some_and(|d| d.is_dirty())
    }
    fn can_undo(&self) -> bool {
        !self.past_documents.is_empty() || self.doc.as_ref().is_some_and(|d| d.can_undo())
    }
    fn can_redo(&self) -> bool {
        !self.future_documents.is_empty() || self.doc.as_ref().is_some_and(|d| d.can_redo())
    }
    fn open(&mut self, p: &Path) -> Result<(), String> {
        let doc = PdfDocument::open(p)?;
        self.cancel_async_open();
        self.doc = Some(doc);
        self.protected_session = false;
        self.protected_readonly = false;
        self.protected_original = None;
        self.encrypted_session = false;
        self.encrypted_current = None;
        self.pending_destination = None;
        self.output_password = None;
        self.past_documents.clear();
        self.future_documents.clear();
        self.current_epoch = 0;
        self.saved_epoch = 0;
        self.next_epoch = 0;
        self.pending = None;
        self.operation_error = None;
        #[cfg(windows)]
        {
            self.search_hits.clear();
        }
        self.selected_annotation = None;
        self.tool = None;
        self.object_tool = false;
        self.select_text = false;
        self.select_annotations = false;
        self.redact_tool = false;
        self.place_signature = false;
        self.capture_signature = false;
        self.annotation_drag = None;
        self.ink_points.clear();
        self.attachment_name.clear();
        self.export_first = 0;
        self.export_last = 0;
        self.export_dpi = 0;
        self.pan = win42::Point::default();
        self.request_render()
    }
    fn save(&mut self, p: &Path) -> Result<(), String> {
        if self.protected_session || self.encrypted_session {
            return Err("Use the password-protected Save or choose Save As a copy".into());
        }
        self.doc.as_mut().ok_or("Open a PDF first")?.save_as(p)?;
        self.saved_epoch = self.current_epoch;
        Ok(())
    }
    fn edit(&mut self, _: String) -> Result<(), String> {
        Ok(())
    }
    fn select(&mut self, i: usize) {
        self.selected_annotation = None;
        if let Some(d) = self.doc.as_mut() {
            let _ = d.select(i);
        }
        let _ = self.request_render();
    }
    fn text_readonly(&self) -> bool {
        true
    }
    fn action(&mut self, id: u16) -> Result<(), String> {
        if id >= 10 && !self.enabled(id) {
            return Err("Command unavailable".into());
        }
        if id == 37 {
            #[cfg(windows)]
            {
                self.search_index = (self.search_index + 1) % self.search_hits.len();
                let page = self.search_hits[self.search_index].page;
                self.doc.as_mut().ok_or("Open a PDF first")?.select(page)?;
                return self.request_render();
            }
            #[cfg(not(windows))]
            return Err("Search is unavailable".into());
        }
        if id == 77 || id == 78 {
            self.object_tool = false;
            self.select_text = false;
            self.select_annotations = false;
            self.redact_tool = false;
            self.capture_signature = id == 77;
            self.place_signature = id == 78;
            self.tool = if id == 77 {
                Some(pdf42_core::AnnotationKind::Ink)
            } else {
                None
            };
            self.operation_error=Some(if id==77 {"Draw a handwritten signature. This is not a digital signature."}else{"Drag to place the saved handwritten signature. This is not a digital signature."}.into());
            return Ok(());
        }
        if id == 70 {
            self.place_signature = false;
            self.capture_signature = false;
            self.object_tool = true;
            self.select_text = false;
            self.tool = None;
            self.select_annotations = false;
            self.redact_tool = false;
            self.operation_error = Some(
                "Click an existing text object, then choose Edit Selected Text Object.".into(),
            );
            return Ok(());
        }
        if id == 62 {
            #[cfg(windows)]
            if let Some((_, _, cancel, _, _)) = &self.export_pending {
                cancel.store(true, std::sync::atomic::Ordering::Relaxed);
            }
            return Ok(());
        }
        if id == 53 {
            #[cfg(windows)]
            if let Some((_, _, cancel, _)) = &self.print_pending {
                cancel.store(true, std::sync::atomic::Ordering::Relaxed);
            }
            return Ok(());
        }
        if id == 40 {
            self.place_signature = false;
            self.capture_signature = false;
            self.object_tool = false;
            self.select_text = true;
            self.tool = None;
            self.select_annotations = false;
            self.redact_tool = false;
            return Ok(());
        }
        if (41..=43).contains(&id) {
            #[cfg(windows)]
            {
                let selection = self.selected_text.take().ok_or("Select text first")?;
                let kind = match id {
                    41 => pdf42_core::AnnotationKind::Highlight,
                    42 => pdf42_core::AnnotationKind::Underline,
                    _ => pdf42_core::AnnotationKind::StrikeOut,
                };
                self.doc
                    .as_mut()
                    .ok_or("Open a PDF first")?
                    .add_text_markup(selection.page, kind, &selection.boxes)?;
                self.branch();
                return self.request_render();
            }
            #[cfg(not(windows))]
            return Err("Text selection requires Windows and PDFium".into());
        }
        if id == 38 {
            self.place_signature = false;
            self.capture_signature = false;
            self.operation_error = None;
            self.object_tool = false;
            self.redact_tool = true;
            self.select_text = false;
            self.select_annotations = false;
            self.tool = None;
            self.selected_annotation = None;
            return Ok(());
        }
        if id == 27 {
            self.place_signature = false;
            self.capture_signature = false;
            self.tool = None;
            self.select_text = false;
            self.redact_tool = false;
            self.object_tool = false;
            self.select_annotations = true;
            self.selected_annotation = None;
            return Ok(());
        }
        if id == 28 {
            let selected = self
                .selected_annotation
                .take()
                .ok_or("Select an annotation first")?;
            self.doc
                .as_mut()
                .ok_or("Open a PDF first")?
                .delete_annotation(selected)?;
            self.branch();
            return self.request_render();
        }
        if (17..=20).contains(&id) || (22..=24).contains(&id) || id == 39 {
            self.select_annotations = false;
            self.select_text = false;
            self.redact_tool = false;
            self.selected_annotation = None;
            self.object_tool = false;
            self.place_signature = false;
            self.capture_signature = false;
            self.tool = match id {
                18 => Some(pdf42_core::AnnotationKind::Rectangle),
                19 => Some(pdf42_core::AnnotationKind::Ellipse),
                20 => Some(pdf42_core::AnnotationKind::Line),
                39 => Some(pdf42_core::AnnotationKind::Ink),
                22 => Some(pdf42_core::AnnotationKind::Highlight),
                23 => Some(pdf42_core::AnnotationKind::Underline),
                24 => Some(pdf42_core::AnnotationKind::StrikeOut),
                _ => None,
            };
            self.annotation_drag = None;
            return Ok(());
        }
        if id == 15 || id == 16 {
            let current = if self.zoom == 0.0 { 1.0 } else { self.zoom };
            self.zoom = (if id == 15 {
                current * 1.25
            } else {
                current / 1.25
            })
            .clamp(0.25, 3.0);
            self.pan = win42::Point::default();
            return self.request_render();
        }
        self.selected_annotation = None;
        #[cfg(windows)]
        {
            self.search_hits.clear();
        }
        if id == 4 {
            self.undo_document();
            return self.request_render();
        }
        if id == 5 {
            self.redo_document();
            return self.request_render();
        }
        let d = self.doc.as_mut().ok_or("Open a PDF first")?;
        let index = d.current();
        match id {
            10 => d.rotate(index, 90)?,
            11 => d.delete(index)?,
            12 => d.move_page(index, index - 1)?,
            13 => d.move_page(index, index + 1)?,
            32 => d.insert_blank_after(index)?,
            63 => d.compress_streams(),
            _ => {}
        }
        self.branch();
        self.request_render()
    }
}
fn main() {
    win42::run(Box::new(Pdf::default()))
}
#[cfg(test)]
mod tests {
    use super::*;
    use win42::App;
    #[test]
    fn normal_open_password_retry_cancel_and_source_identity() {
        fn wait_challenge(app: &mut Pdf) -> win42::InputRequest {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
            loop {
                if let Some(result) = app.take_async_open_result() {
                    panic!("unexpected open result: {:?}", result.err());
                }
                if let Some(prompt) = app.take_async_open_password_request() {
                    return prompt;
                }
                assert!(std::time::Instant::now() < deadline);
                std::thread::yield_now();
            }
        }
        fn wait_ready(app: &mut Pdf) -> suite42_core::session::FileIdentity {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
            loop {
                if let Some(result) = app.take_async_open_result() {
                    return result.unwrap();
                }
                assert!(app.take_async_open_password_request().is_none());
                assert!(std::time::Instant::now() < deadline);
                std::thread::yield_now();
            }
        }
        let dir =
            std::env::temp_dir().join(format!("pdf42-normal-open-password-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let old = dir.join("old.pdf");
        let protected = dir.join("protected.pdf");
        let mut scan = scn42_core::ScanDocument::new();
        scan.add_rgba(1, 1, vec![255, 255, 255, 255]).unwrap();
        scan.export_pdf(&old).unwrap();
        let original = PdfDocument::open(&old).unwrap();
        let (encrypted, _) = original.save_encrypted_snapshot("pass").unwrap();
        std::fs::write(&protected, &encrypted).unwrap();
        let mut app = Pdf::default();
        app.open(&old).unwrap();
        app.action(10).unwrap();
        assert!(app.dirty());
        assert_eq!(app.doc.as_ref().unwrap().rotation(0).unwrap(), 90);
        assert!(app.begin_async_open(&protected).unwrap());
        app.cancel_async_open();
        assert!(app.opening_path.is_none());
        assert_eq!(app.doc.as_ref().unwrap().rotation(0).unwrap(), 90);
        assert!(app.begin_async_open(&protected).unwrap());
        let prompt = wait_challenge(&mut app);
        assert_eq!(prompt.title, "Open protected PDF");
        assert_eq!(app.doc.as_ref().unwrap().rotation(0).unwrap(), 90);
        app.submit_async_open_password(Some("wrong")).unwrap();
        let retry = wait_challenge(&mut app);
        assert_eq!(retry.title, "Incorrect password — try again");
        assert!(!app.status().contains("wrong"));
        assert_eq!(app.doc.as_ref().unwrap().rotation(0).unwrap(), 90);
        app.submit_async_open_password(None).unwrap();
        assert!(app.open_challenge.is_none());
        assert!(app.opening_path.is_none());
        assert!(app.dirty());
        assert!(app.begin_async_open(&protected).unwrap());
        let _ = wait_challenge(&mut app);
        std::fs::write(&protected, b"changed while waiting").unwrap();
        app.submit_async_open_password(Some("pass")).unwrap();
        let expected = wait_ready(&mut app);
        assert_ne!(
            suite42_core::session::FileIdentity::read(&protected).unwrap(),
            expected
        );
        app.cancel_async_open();
        assert_eq!(app.doc.as_ref().unwrap().rotation(0).unwrap(), 90);
        std::fs::write(&protected, &encrypted).unwrap();
        assert!(app.begin_async_open(&protected).unwrap());
        let _ = wait_challenge(&mut app);
        app.submit_async_open_password(Some("pass")).unwrap();
        let expected = wait_ready(&mut app);
        assert_eq!(
            suite42_core::session::FileIdentity::read(&protected).unwrap(),
            expected
        );
        app.commit_async_open().unwrap();
        assert!(app.protected_session);
        assert!(!app.can_save_in_place());
        assert_eq!(app.doc.as_ref().unwrap().rotation(0).unwrap(), 0);
        assert!(!app.dirty());
        assert!(app.begin_async_open(&old).unwrap());
        let expected = wait_ready(&mut app);
        assert_eq!(
            suite42_core::session::FileIdentity::read(&old).unwrap(),
            expected
        );
        app.commit_async_open().unwrap();
        assert!(!app.protected_session);
        assert!(app.can_save_in_place());
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn protected_save_requires_another_path_and_explicit_decrypted_copy() {
        use lopdf::encryption::crypt_filters::{Aes128CryptFilter, CryptFilter};
        use std::sync::Arc;
        let dir = std::env::temp_dir().join(format!("pdf42-protected-save-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let input = dir.join("protected.pdf");
        let output = dir.join("copy.pdf");
        let alias = dir.join("hardlink.pdf");
        let mut scan = scn42_core::ScanDocument::new();
        scan.add_rgba(1, 1, vec![255, 255, 255, 255]).unwrap();
        scan.export_pdf(&input).unwrap();
        let mut raw = lopdf::Document::load(&input).unwrap();
        raw.trailer.set(
            "ID",
            vec![
                lopdf::Object::string_literal("0123456789abcdef"),
                lopdf::Object::string_literal("0123456789abcdef"),
            ],
        );
        let filter: Arc<dyn CryptFilter> = Arc::new(Aes128CryptFilter);
        let state = lopdf::EncryptionState::try_from(lopdf::EncryptionVersion::V4 {
            document: &raw,
            encrypt_metadata: true,
            crypt_filters: std::collections::BTreeMap::from([(b"StdCF".to_vec(), filter)]),
            stream_filter: b"StdCF".to_vec(),
            string_filter: b"StdCF".to_vec(),
            owner_password: "owner",
            user_password: "pass",
            permissions: lopdf::Permissions::all(),
        })
        .unwrap();
        raw.encrypt(&state).unwrap();
        raw.save(&input).unwrap();
        std::fs::hard_link(&input, &alias).unwrap();
        let before = std::fs::read(&input).unwrap();
        let doc = PdfDocument::open_with_password_bytes(&before, "pass").unwrap();
        let mut app = Pdf::default();
        app.doc = Some(doc);
        app.protected_session = true;
        app.protected_original = Some(input.clone());
        assert!(!app.can_save_in_place());
        assert!(app.save_snapshot_to("pdf", &input, true).is_err());
        assert!(app.save_snapshot_to("pdf", &alias, false).is_err());
        let (rev, bytes) = app
            .save_snapshot_to("pdf", &output, false)
            .unwrap()
            .unwrap();
        std::fs::write(&output, &bytes).unwrap();
        app.saved_snapshot(rev);
        assert!(app.can_save_in_place());
        assert!(!lopdf::Document::load(&output).unwrap().is_encrypted());
        assert_eq!(std::fs::read(&input).unwrap(), before);
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn encrypted_save_as_stays_encrypted_on_later_save() {
        let dir = std::env::temp_dir().join(format!("pdf42-encrypted-save-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let input = dir.join("in.pdf");
        let output = dir.join("secure.pdf");
        let mut scan = scn42_core::ScanDocument::new();
        scan.add_rgba(1, 1, vec![255, 255, 255, 255]).unwrap();
        scan.export_pdf(&input).unwrap();
        let mut app = Pdf::default();
        app.open(&input).unwrap();
        app.output_password = Some(Secret("secret42".into()));
        let (rev, bytes) = app
            .save_snapshot_to("pdf-encrypted", &output, false)
            .unwrap()
            .unwrap();
        std::fs::write(&output, &bytes).unwrap();
        app.saved_snapshot(rev);
        assert!(app.encrypted_session);
        assert!(app.can_save_in_place());
        assert!(app.save_snapshot_to("pdf", &output, true).is_err());
        assert!(PdfDocument::open_with_password_bytes(&bytes, "wrong").is_err());
        assert!(PdfDocument::open_with_password_bytes(&bytes, "secret42")
            .unwrap()
            .was_password_protected());
        app.action(10).unwrap();
        let (rev, bytes2) = app
            .save_snapshot_to("pdf-encrypted", &output, true)
            .unwrap()
            .unwrap();
        std::fs::write(&output, &bytes2).unwrap();
        app.saved_snapshot(rev);
        assert_eq!(
            PdfDocument::open_with_password_bytes(&bytes2, "secret42")
                .unwrap()
                .rotation(0)
                .unwrap(),
            90
        );
        assert!(!app.dirty());
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn flatten_becomes_editable_and_history_tracks_saved_document_identity() {
        use std::io::Write;
        let dir = std::env::temp_dir().join(format!("pdf42-flatten-ui-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let input = dir.join("in.pdf");
        let mut scan = scn42_core::ScanDocument::new();
        scan.add_rgba(1, 1, vec![255, 255, 255, 255]).unwrap();
        scan.export_pdf(&input).unwrap();
        let mut source = lopdf::Document::load(&input).unwrap();
        source.catalog_mut().unwrap().set(
            "PrivateMarker",
            lopdf::Object::string_literal("SOURCE-SECRET-MARKER"),
        );
        source.save(&input).unwrap();
        let mut app = Pdf::default();
        app.open(&input).unwrap();
        assert!(!app.dirty());
        let mut encoder =
            flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(&vec![0; 300]).unwrap();
        let page = pdf42_core::FlattenedPage {
            width: 10,
            height: 10,
            compressed_rgb: encoder.finish().unwrap(),
            page_width: 100.,
            page_height: 100.,
        };
        app.install_flattened(vec![page], 0).unwrap();
        assert!(app.dirty());
        assert!(app.enabled(10));
        app.action(10).unwrap();
        assert_eq!(app.doc.as_ref().unwrap().rotation(0).unwrap(), 90);
        app.action(4).unwrap();
        assert_eq!(app.doc.as_ref().unwrap().rotation(0).unwrap(), 0);
        app.action(4).unwrap();
        assert!(!app.dirty());
        assert!(app.can_redo());
        app.action(5).unwrap();
        assert!(app.dirty());
        let (revision, bytes) = app.save_snapshot("pdf").unwrap().unwrap();
        app.saved_snapshot(revision);
        assert!(!app.dirty());
        app.action(4).unwrap();
        assert!(app.dirty());
        app.action(5).unwrap();
        assert!(!app.dirty());
        app.doc
            .as_mut()
            .unwrap()
            .add_annotation(
                0,
                pdf42_core::AnnotationKind::Rectangle,
                [1., 1., 10., 10.],
                "",
            )
            .unwrap();
        assert!(app.dirty());
        let (edited, _) = app.doc.as_ref().unwrap().save_snapshot().unwrap();
        let output = dir.join("out.pdf");
        std::fs::write(&output, edited).unwrap();
        let reopened = lopdf::Document::load(&output).unwrap();
        let p = *reopened.get_pages().values().next().unwrap();
        assert!(reopened.get_dictionary(p).unwrap().get(b"Annots").is_ok());
        assert!(!bytes
            .windows(b"SOURCE-SECRET-MARKER".len())
            .any(|v| v == b"SOURCE-SECRET-MARKER"));
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn command_boundaries_and_unsafe_structure_refuse_dispatch() {
        let dir = std::env::temp_dir().join(format!("pdf42-ui-boundary-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let input = dir.join("input.pdf");
        let unsafe_path = dir.join("unsafe.pdf");
        let _ = std::fs::remove_file(&input);
        let _ = std::fs::remove_file(&unsafe_path);
        let mut scan = scn42_core::ScanDocument::new();
        scan.add_rgba(1, 1, vec![255, 255, 255, 255]).unwrap();
        scan.add_rgba(1, 1, vec![0, 0, 0, 255]).unwrap();
        scan.export_pdf(&input).unwrap();
        let mut a = Pdf::default();
        for id in 10..=13 {
            assert!(!a.enabled(id));
            assert!(a.action(id).is_err());
        }
        a.open(&input).unwrap();
        assert!(!a.enabled(12));
        assert!(a.enabled(13));
        a.action(13).unwrap();
        assert!(a.can_undo());
        a.action(4).unwrap();
        assert_eq!(a.selected(), 0);
        assert!(a.can_redo());
        a.action(5).unwrap();
        assert_eq!(a.selected(), 1);
        assert!(!a.enabled(13));
        assert!(a.enabled(12));
        a.action(11).unwrap();
        assert!(!a.enabled(11));
        assert!(a.action(11).is_err());
        let mut doc = lopdf::Document::load(&input).unwrap();
        let page = *doc.get_pages().values().next().unwrap();
        doc.get_dictionary_mut(page)
            .unwrap()
            .set("Annots", lopdf::Object::Array(vec![]));
        doc.save(&unsafe_path).unwrap();
        let mut b = Pdf::default();
        b.open(&unsafe_path).unwrap();
        assert!(b.enabled(10));
        assert!(b.enabled(11));
        assert!(b.enabled(13));
        b.action(13).unwrap();
        assert_eq!(b.selected(), 1);
        b.action(4).unwrap();
        assert_eq!(b.selected(), 0);
        b.action(11).unwrap();
        assert_eq!(b.doc.as_ref().unwrap().page_count(), 1);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
