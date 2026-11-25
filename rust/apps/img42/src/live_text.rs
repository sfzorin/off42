//! Reading-only recognized words, tied to immutable pixels and the current viewing session.
use img42_core::{ImageDocument, ImageSnapshot};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc, Arc,
};
use suite42_core::ocr::Word;
struct Worker {
    cancel: Arc<AtomicBool>,
    rx: mpsc::Receiver<Result<Vec<Word>, String>>,
}
impl Drop for Worker {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}
#[derive(Default)]
pub struct LiveText {
    source: Option<ImageSnapshot>,
    worker: Option<Worker>,
    words: Vec<Word>,
    selection: Option<(usize, usize)>,
    dragging: bool,
    pub language: String,
    message: String,
}
impl LiveText {
    pub fn reset(&mut self) {
        let language = std::mem::take(&mut self.language);
        *self = Self {
            language,
            ..Self::default()
        };
    }
    pub fn valid(&self, doc: &ImageDocument) -> bool {
        self.source.as_ref().is_some_and(|s| doc.matches_image(s))
    }
    pub fn busy(&self) -> bool {
        self.worker.is_some()
    }
    pub fn ready(&self, doc: &ImageDocument) -> bool {
        self.valid(doc) && !self.busy() && !self.words.is_empty()
    }
    pub fn selected(&self, doc: &ImageDocument) -> bool {
        self.ready(doc) && self.selection.is_some()
    }
    pub fn clear(&mut self) {
        self.selection = None;
        self.dragging = false;
    }
    pub fn cancel(&mut self) {
        self.worker = None;
        self.message = "Text recognition cancelled".into();
    }
    pub fn start(&mut self, doc: &ImageDocument, language: &str) {
        self.reset();
        self.language = language.into();
        let source = doc.capture_image();
        let pixels = source.clone();
        let cancel = Arc::new(AtomicBool::new(false));
        let token = cancel.clone();
        let (tx, rx) = mpsc::channel();
        let language = language.to_string();
        std::thread::spawn(move || {
            let result = win42::ocr::recognize(
                &win42::ocr::Image {
                    width: pixels.width(),
                    height: pixels.height(),
                    pixels: pixels.pixels(),
                },
                &language,
                &token,
            )
            .map(|(words, _)| words);
            let _ = tx.send(result);
        });
        self.source = Some(source);
        self.worker = Some(Worker { cancel, rx });
        self.message = "Recognizing image text…".into();
    }
    pub fn poll(&mut self, doc: Option<&ImageDocument>) -> bool {
        let Some(doc) = doc else {
            let changed = self.source.is_some();
            self.reset();
            return changed;
        };
        if !self.valid(doc) {
            let language = if self.language.is_empty() {
                "auto".to_string()
            } else {
                self.language.clone()
            };
            #[cfg(windows)]
            self.start(doc, &language);
            #[cfg(not(windows))]
            {
                let _ = language;
                self.reset();
                self.source = Some(doc.capture_image());
            }
            return true;
        }
        let result = self.worker.as_ref().and_then(|w| match w.rx.try_recv() {
            Ok(v) => Some(v),
            Err(mpsc::TryRecvError::Disconnected) => Some(Err("Image text worker stopped".into())),
            Err(mpsc::TryRecvError::Empty) => None,
        });
        if let Some(result) = result {
            self.worker = None;
            self.message = match result {
                Ok(words) => {
                    self.words = words;
                    format!(
                        "{} recognized words · drag to select text",
                        self.words.len()
                    )
                }
                Err(e) => e,
            };
            return true;
        }
        false
    }
    pub fn status(&self, doc: &ImageDocument) -> &str {
        if self.valid(doc) {
            &self.message
        } else {
            ""
        }
    }
    fn hit(&self, p: win42::Point) -> Option<usize> {
        if !p.x.is_finite() || !p.y.is_finite() {
            return None;
        }
        self.words.iter().position(|w| {
            p.x >= w.x as f64
                && p.y >= w.y as f64
                && p.x < f64::from(w.x + w.width)
                && p.y < f64::from(w.y + w.height)
        })
    }
    pub fn event(&mut self, doc: &ImageDocument, event: win42::CanvasEvent) -> bool {
        use win42::CanvasEvent::*;
        if !self.ready(doc) {
            return false;
        }
        match event {
            PointerDown(p) | DoubleClick(p) => {
                self.selection = self.hit(p).map(|i| (i, i));
                self.dragging = self.selection.is_some();
                true
            }
            PointerMove(p) | PointerUp(p) if self.dragging => {
                if let Some(i) = self.hit(p) {
                    self.selection.as_mut().unwrap().1 = i;
                }
                if matches!(event, PointerUp(_)) {
                    self.dragging = false;
                }
                true
            }
            Cancel => {
                self.dragging = false;
                true
            }
            Key { code: 27, .. } => {
                self.clear();
                true
            }
            _ => false,
        }
    }
    pub fn select_all(&mut self, doc: &ImageDocument) {
        if self.ready(doc) {
            self.selection = Some((0, self.words.len() - 1));
            self.dragging = false;
        }
    }
    fn range(&self, all: bool) -> Option<std::ops::RangeInclusive<usize>> {
        if all {
            return (!self.words.is_empty()).then(|| 0..=self.words.len() - 1);
        }
        self.selection.map(|(a, b)| a.min(b)..=a.max(b))
    }
    pub fn text(&self, doc: &ImageDocument, all: bool) -> Option<String> {
        if !self.ready(doc) {
            return None;
        }
        let range = self.range(all)?;
        let mut out = String::new();
        let mut previous: Option<&Word> = None;
        for word in &self.words[range] {
            if let Some(last) = previous {
                let delta = (f64::from(word.y) + f64::from(word.height) / 2.
                    - f64::from(last.y)
                    - f64::from(last.height) / 2.)
                    .abs();
                out.push(if delta > f64::from(word.height.max(last.height)) * 0.6 {
                    '\n'
                } else {
                    ' '
                });
            }
            out.push_str(&word.text);
            previous = Some(word);
        }
        Some(out)
    }
    pub fn paint_selection(&self, doc: &ImageDocument, pixels: &mut [u8]) {
        if !self.ready(doc) {
            return;
        }
        let Some(range) = self.range(false) else {
            return;
        };
        for word in &self.words[range] {
            for y in word.y..word.y.saturating_add(word.height).min(doc.height()) {
                for x in word.x..word.x.saturating_add(word.width).min(doc.width()) {
                    let i = (y as usize * doc.width() as usize + x as usize) * 4;
                    for (c, color) in [50u8, 130, 245].iter().enumerate() {
                        pixels[i + c] = ((u16::from(pixels[i + c]) + u16::from(*color)) / 2) as u8;
                    }
                }
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn word(text: &str, x: u32, y: u32) -> Word {
        Word {
            text: text.into(),
            x,
            y,
            width: 25,
            height: 10,
        }
    }
    #[test]
    fn selection_reverse_drag_lines_and_stale_pixels_never_modify_the_image() {
        let mut doc = ImageDocument::from_rgba(100, 70, vec![255; 100 * 70 * 4], None).unwrap();
        let source = doc.capture_image();
        let mut live = LiveText {
            source: Some(source.clone()),
            words: vec![
                word("ALPHA", 0, 0),
                word("BRAVO", 30, 0),
                word("中文", 0, 20),
            ],
            ..LiveText::default()
        };
        use win42::{CanvasEvent::*, Point};
        assert!(live.event(&doc, DoubleClick(Point { x: 35., y: 5. })));
        assert_eq!(live.text(&doc, false).unwrap(), "BRAVO");
        live.event(&doc, PointerDown(Point { x: 5., y: 25. }));
        live.event(&doc, PointerMove(Point { x: 5., y: 5. }));
        live.event(&doc, PointerUp(Point { x: 5., y: 5. }));
        assert_eq!(live.text(&doc, false).unwrap(), "ALPHA BRAVO\n中文");
        let mut shown = doc.pixels().to_vec();
        live.paint_selection(&doc, &mut shown);
        assert_ne!(shown, doc.pixels());
        assert!(!doc.is_dirty());
        assert!(!doc.can_undo());
        live.event(
            &doc,
            Key {
                code: 27,
                modifiers: 0,
            },
        );
        assert!(live.text(&doc, false).is_none());
        live.select_all(&doc);
        assert!(live.selected(&doc));
        doc.draw_line(0, 0, 10, 10, [0, 0, 0, 255]).unwrap();
        assert!(!doc.matches_image(&source));
        assert!(live.text(&doc, true).is_none());
        assert!(!live.event(&doc, DoubleClick(Point { x: 35., y: 5. })));
        doc.undo();
        assert!(doc.matches_image(&source));
        live.reset();
        assert!(live.text(&doc, true).is_none());
    }
}

#[cfg(test)]
mod app_tests {
    use super::*;
    use win42::App;
    #[test]
    fn frame_navigation_and_drawing_modes_clear_reading_selection_without_dirtying_pixels() {
        let first = ImageDocument::from_rgba(100, 70, vec![255; 100 * 70 * 4], None).unwrap();
        let second = ImageDocument::from_rgba(100, 70, vec![200; 100 * 70 * 4], None).unwrap();
        let source = first.capture_image();
        let mut app = crate::Img {
            doc: Some(first),
            frames: vec![None, Some(second)],
            live_text: LiveText {
                source: Some(source),
                words: vec![Word {
                    text: "readable".into(),
                    x: 5,
                    y: 5,
                    width: 50,
                    height: 20,
                }],
                selection: Some((0, 0)),
                ..LiveText::default()
            },
            ..crate::Img::default()
        };
        assert!(app.clipboard_request(44).is_some());
        assert!(!app.dirty());
        app.action(24).unwrap();
        assert!(app.clipboard_request(44).is_none());
        app.canvas_event(win42::CanvasEvent::PointerDown(win42::Point {
            x: 10.,
            y: 10.,
        }))
        .unwrap();
        app.canvas_event(win42::CanvasEvent::PointerUp(win42::Point {
            x: 40.,
            y: 15.,
        }))
        .unwrap();
        assert!(app.dirty());
        assert!(app.clipboard_request(45).is_none());
        app.action(4).unwrap();
        assert!(!app.dirty());
        app.select(1);
        assert!(app.clipboard_request(45).is_none());
        assert!(app.live_text.source.is_none());
        app.select(0);
        assert!(app.clipboard_request(45).is_none());
        assert!(!app.dirty());
    }
}
