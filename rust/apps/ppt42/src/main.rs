#![cfg_attr(windows, windows_subsystem = "windows")]
use ppt42_core::Presentation;
use std::path::Path;
mod rich;
fn decode_image(bytes: &[u8]) -> Result<image::DynamicImage, String> {
    let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|e| e.to_string())?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(8192);
    limits.max_image_height = Some(8192);
    limits.max_alloc = Some(128 * 1024 * 1024);
    reader.limits(limits);
    reader.decode().map_err(|e| e.to_string())
}
fn object_point(o: &ppt42_core::SlideObject, x: f64, y: f64) -> win42::Point {
    let cx = o.x + o.width / 2.0;
    let cy = o.y + o.height / 2.0;
    let (sin, cos) = o.rotation_degrees.rem_euclid(360.0).to_radians().sin_cos();
    win42::Point {
        x: cx + cos * (x - cx) - sin * (y - cy),
        y: cy + sin * (x - cx) + cos * (y - cy),
    }
}

fn object_local_point(o: &ppt42_core::SlideObject, p: win42::Point) -> win42::Point {
    let cx = o.x + o.width / 2.0;
    let cy = o.y + o.height / 2.0;
    let (sin, cos) = o.rotation_degrees.rem_euclid(360.0).to_radians().sin_cos();
    win42::Point {
        x: cx + cos * (p.x - cx) + sin * (p.y - cy),
        y: cy - sin * (p.x - cx) + cos * (p.y - cy),
    }
}

fn resize_object(o: &mut ppt42_core::SlideObject, start: win42::Point, end: win42::Point) {
    let (sin, cos) = o.rotation_degrees.rem_euclid(360.0).to_radians().sin_cos();
    let (dx, dy) = (end.x - start.x, end.y - start.y);
    let width = (o.width + cos * dx + sin * dy).max(1.0);
    let height = (o.height - sin * dx + cos * dy).max(1.0);
    let (dw, dh) = (width - o.width, height - o.height);
    o.x += (cos * dw - sin * dh - dw) / 2.0;
    o.y += (sin * dw + cos * dh - dh) / 2.0;
    o.width = width;
    o.height = height;
}
struct Ppt {
    pending: Option<(u64, ppt42_core::SaveState)>,
    next_save: u64,
    deck: Presentation,
    pptx_source: Option<ppt42_core::pptx::Source>,
    odp_source: Option<ppt42_core::odp::Source>,
    selected_object: Option<u64>,
    selected_objects: Vec<u64>,
    drag: Option<Drag>,
    rubber_band: Option<(win42::Point, win42::Point, bool)>,
    presenting: bool,
    show: Option<Show>,
    rich_edit_object: Option<u64>,
    rich_draft: Option<ppt42_core::SlideObject>,
    rich_selection: (usize, usize),
}
struct Show {
    since: std::time::Instant,
    previous: Option<usize>,
    paused: Option<std::time::Instant>,
    transition_ms: u32,
}
enum Drag {
    Move(win42::Point, Vec<ppt42_core::SlideObject>),
    Resize(win42::Point, ppt42_core::SlideObject),
}
impl Ppt {
    fn rich_object(&self) -> Result<ppt42_core::SlideObject, String> {
        if let Some(object) = &self.rich_draft {
            return Ok(object.clone());
        }
        let id = self
            .rich_edit_object
            .ok_or("No text object is being edited")?;
        self.deck.slides()[self.deck.current()]
            .objects
            .iter()
            .find(|o| o.id == id)
            .cloned()
            .ok_or("Text object no longer exists".into())
    }
    fn save_rich_document(&mut self, doc: &doc42_core::rich::RichDocument) -> Result<(), String> {
        let mut object = self.rich_object()?;
        rich::update_object(&mut object, doc)?;
        self.rich_draft = Some(object);
        Ok(())
    }
    fn finish_rich(&mut self, commit: bool) -> Result<(), String> {
        let draft = self.rich_draft.take();
        self.rich_edit_object = None;
        if commit {
            if let Some(object) = draft {
                self.deck.update_object(object)?;
            }
        }
        Ok(())
    }
    fn format_rich_selection(&mut self, id: u16) -> Result<(), String> {
        let object = self.rich_object()?;
        let mut doc = rich::document(&object)?;
        let (paragraph, range, mut style, selection) =
            rich::selection_style(&doc, self.rich_selection)?;
        match id {
            59 => style.bold = !style.bold,
            60 => style.italic = !style.italic,
            61 => style.underline = !style.underline,
            _ => return Err("Unknown formatting action".into()),
        }
        doc.apply_text_style(paragraph, range, style, selection)
            .map_err(|e| e.to_string())?;
        self.save_rich_document(&doc)
    }
    fn format_rich_font(&mut self, font: win42::FontChoice) -> Result<(), String> {
        let object = self.rich_object()?;
        let mut doc = rich::document(&object)?;
        let (p, range, mut style, selection) = rich::selection_style(&doc, self.rich_selection)?;
        style.font = font.family;
        style.size_half_points = (font.points * 2.0).round().clamp(2.0, 1024.0) as u16;
        style.bold = font.bold;
        style.italic = font.italic;
        style.underline = font.underline;
        style.color =
            Some((font.color[0] as u32) << 16 | (font.color[1] as u32) << 8 | font.color[2] as u32);
        doc.apply_text_style(p, range, style, selection)
            .map_err(|e| e.to_string())?;
        self.save_rich_document(&doc)
    }
    fn format_rich_paragraph(&mut self, id: u16) -> Result<(), String> {
        use doc42_core::rich::{Alignment, Block, ListMarker, Selection};
        let object = self.rich_object()?;
        let mut doc = rich::document(&object)?;
        let start = doc
            .position_from_utf16(self.rich_selection.0.min(self.rich_selection.1))
            .map_err(|e| e.to_string())?;
        let end = doc
            .position_from_utf16(self.rich_selection.0.max(self.rich_selection.1))
            .map_err(|e| e.to_string())?;
        if start.paragraph != end.paragraph {
            return Err("Select within one paragraph".into());
        }
        let mut style = doc
            .state()
            .blocks
            .iter()
            .find_map(|b| match b {
                Block::Paragraph(p) if p.id == start.paragraph => Some(p.style.clone()),
                _ => None,
            })
            .ok_or("Paragraph missing")?;
        match id {
            43 => style.alignment = Alignment::Left,
            44 => style.alignment = Alignment::Center,
            45 => style.alignment = Alignment::Right,
            62 => {
                style.list = if style.list.is_some() {
                    None
                } else {
                    Some(ListMarker {
                        id: 1,
                        level: 0,
                        numbered: false,
                    })
                }
            }
            _ => return Err("Unknown paragraph action".into()),
        };
        doc.set_paragraph_style(
            start.paragraph,
            style,
            Selection {
                anchor: Some(start),
                focus: Some(end),
            },
        )
        .map_err(|e| e.to_string())?;
        self.save_rich_document(&doc)
    }
    fn new(deck: Presentation) -> Self {
        Self {
            deck,
            pptx_source: None,
            odp_source: None,
            pending: None,
            next_save: 0,
            selected_object: None,
            selected_objects: Vec::new(),
            rubber_band: None,
            drag: None,
            presenting: false,
            show: None,
            rich_edit_object: None,
            rich_draft: None,
            rich_selection: (0, 0),
        }
    }
}
impl Ppt {
    fn show_move(&mut self, index: usize, now: std::time::Instant) -> Result<(), String> {
        let old = self.deck.current();
        if index == old {
            return Ok(());
        }
        let paused = self.show.as_ref().is_some_and(|show| show.paused.is_some());
        self.deck.select(index)?;
        let timing = self.deck.slides()[index].timing;
        self.show = Some(Show {
            since: now,
            previous: (!paused && timing.transition != ppt42_core::Transition::Cut).then_some(old),
            paused: paused.then_some(now),
            transition_ms: if !paused && timing.transition != ppt42_core::Transition::Cut {
                timing.duration_ms
            } else {
                0
            },
        });
        Ok(())
    }
    fn show_tick(&mut self, now: std::time::Instant) -> bool {
        if !self.presenting {
            self.show = None;
            return false;
        }
        let Some(show) = &mut self.show else {
            return false;
        };
        if show.paused.is_some() {
            return false;
        }
        let timing = self.deck.slides()[self.deck.current()].timing;
        let elapsed = now.saturating_duration_since(show.since).as_millis();
        if show.previous.is_some() && elapsed < u128::from(timing.duration_ms) {
            return true;
        }
        if show.previous.take().is_some() {
            return true;
        }
        if timing
            .advance_ms
            .is_some_and(|ms| elapsed >= u128::from(ms) + u128::from(show.transition_ms))
        {
            let next = self.deck.current() + 1;
            if next < self.deck.slides().len() {
                let _ = self.show_move(next, now);
                return true;
            }
        }
        false
    }
    fn show_event(
        &mut self,
        event: win42::CanvasEvent,
        now: std::time::Instant,
    ) -> Result<(), String> {
        use win42::CanvasEvent::*;
        match event {
            Key { code: 27, .. } => {
                self.presenting = false;
                self.show = None;
            }
            Key { code: 80, .. } => {
                if let Some(show) = self.show.as_mut() {
                    if let Some(paused) = show.paused.take() {
                        show.since += now.saturating_duration_since(paused);
                    } else {
                        show.paused = Some(now);
                    }
                }
            }
            Key {
                code: 39 | 40 | 32 | 34,
                ..
            }
            | PointerUp(_) => {
                let i = self.deck.current();
                if i + 1 < self.deck.slides().len() {
                    self.show_move(i + 1, now)?;
                }
            }
            Key {
                code: 37 | 38 | 33, ..
            } => {
                let i = self.deck.current();
                if i > 0 {
                    self.show_move(i - 1, now)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
    fn selection(&self) -> Vec<u64> {
        let slide = &self.deck.slides()[self.deck.current()];
        let mut ids = self
            .selected_objects
            .iter()
            .copied()
            .filter(|id| slide.objects.iter().any(|o| o.id == *id))
            .collect::<Vec<_>>();
        if ids.is_empty() {
            if let Some(id) = self.selected_object {
                if slide.objects.iter().any(|o| o.id == id) {
                    ids.push(id);
                }
            }
        }
        ids
    }
    fn pointer_down(&mut self, p: win42::Point, add: bool) {
        let selected = self.selection();
        if !add && selected.len() == 1 {
            if let Some(object) = self.deck.slides()[self.deck.current()]
                .objects
                .iter()
                .find(|o| {
                    o.id == selected[0] && {
                        let handle = object_point(o, o.x + o.width, o.y + o.height);
                        (p.x - handle.x).abs() <= 7.0 && (p.y - handle.y).abs() <= 7.0
                    }
                })
                .cloned()
            {
                self.drag = Some(Drag::Resize(p, object));
                self.rubber_band = None;
                return;
            }
        }
        let object = self.deck.slides()[self.deck.current()]
            .objects
            .iter()
            .rev()
            .find(|o| {
                let p = object_local_point(o, p);
                p.x >= o.x && p.x <= o.x + o.width && p.y >= o.y && p.y <= o.y + o.height
            })
            .cloned();
        if let Some(object) = object {
            let mut selection = selected;
            let members = if let Some(group) = object.group_id {
                self.deck.slides()[self.deck.current()]
                    .objects
                    .iter()
                    .filter(|o| o.group_id == Some(group))
                    .map(|o| o.id)
                    .collect::<Vec<_>>()
            } else {
                vec![object.id]
            };
            if add {
                if selection.contains(&object.id) {
                    selection.retain(|id| !members.contains(id));
                    self.selected_object = selection.last().copied();
                    self.selected_objects = selection;
                    self.drag = None;
                    return;
                }
                for id in members {
                    if !selection.contains(&id) {
                        selection.push(id);
                    }
                }
            } else if !selection.contains(&object.id) {
                selection = members;
            }
            self.selected_object = Some(object.id);
            self.selected_objects = selection.clone();
            let objects = self.deck.slides()[self.deck.current()]
                .objects
                .iter()
                .filter(|o| selection.contains(&o.id))
                .cloned()
                .collect();
            self.drag = Some(Drag::Move(p, objects));
            self.rubber_band = None;
        } else {
            if !add {
                self.selected_object = None;
                self.selected_objects.clear();
            }
            self.drag = None;
            self.rubber_band = Some((p, p, add));
        }
    }
    fn scene_for(&self, index: usize, show_selection: bool) -> Option<win42::CanvasScene> {
        use win42::{CanvasScene, Point, SceneItem};
        let slide = &self.deck.slides()[index];
        let background = self
            .pptx_source
            .as_ref()
            .and_then(|s| s.background(slide.source_identity()))
            .unwrap_or([255, 255, 255]);
        let decorations = self
            .pptx_source
            .as_ref()
            .map(|s| s.decorations(slide.source_identity()))
            .unwrap_or(&[]);
        let mut items = vec![
            SceneItem::Rect {
                x: 0.0,
                y: 0.0,
                width: 720.0,
                height: 405.0,
                fill: background,
                stroke: [80, 80, 80],
            },
            SceneItem::TextBox {
                x: 35.0,
                y: 30.0,
                width: 650.0,
                height: 40.0,
                text: slide.title.clone(),
                color: [0, 0, 0],
                font: win42::SceneFont {
                    family: "Arial".into(),
                    points: 28.0,
                    bold: true,
                    italic: false,
                    underline: false,
                },
                align: win42::TextAlign::Left,
                wrap: true,
            },
            SceneItem::TextBox {
                x: 35.0,
                y: 75.0,
                width: 650.0,
                height: 290.0,
                text: slide.body.clone(),
                color: [0, 0, 0],
                font: win42::SceneFont {
                    family: "Arial".into(),
                    points: 18.0,
                    bold: false,
                    italic: false,
                    underline: false,
                },
                align: win42::TextAlign::Left,
                wrap: true,
            },
        ];
        for (editable, original) in decorations
            .iter()
            .map(|o| (false, o))
            .chain(slide.objects.iter().map(|o| (true, o)))
        {
            let o = self
                .drag
                .as_ref()
                .filter(|_| editable && show_selection && index == self.deck.current())
                .and_then(|drag| match drag {
                    Drag::Move(_, objects) => objects.iter().find(|o| o.id == original.id),
                    Drag::Resize(_, object) if object.id == original.id => Some(object),
                    _ => None,
                })
                .unwrap_or(original);
            let stroke = if editable && show_selection && self.selection().contains(&o.id) {
                [0, 100, 220]
            } else {
                o.stroke
            };
            let object_start = items.len();
            match o.kind {
                ppt42_core::ObjectKind::Rectangle | ppt42_core::ObjectKind::Text => {
                    if o.fill_visible {
                        items.push(SceneItem::Rect {
                            x: o.x,
                            y: o.y,
                            width: o.width,
                            height: o.height,
                            fill: o.fill,
                            stroke: if o.stroke_visible
                                || (editable && show_selection && self.selection().contains(&o.id))
                            {
                                stroke
                            } else {
                                o.fill
                            },
                        })
                    } else if o.stroke_visible
                        || (editable && show_selection && self.selection().contains(&o.id))
                    {
                        items.push(SceneItem::Path {
                            points: vec![
                                Point { x: o.x, y: o.y },
                                Point {
                                    x: o.x + o.width,
                                    y: o.y,
                                },
                                Point {
                                    x: o.x + o.width,
                                    y: o.y + o.height,
                                },
                                Point {
                                    x: o.x,
                                    y: o.y + o.height,
                                },
                            ],
                            color: stroke,
                            closed: true,
                        });
                    }
                }
                ppt42_core::ObjectKind::Line => items.push(SceneItem::Path {
                    points: vec![
                        Point { x: o.x, y: o.y },
                        Point {
                            x: o.x + o.width,
                            y: o.y + o.height,
                        },
                    ],
                    color: stroke,
                    closed: false,
                }),
                ppt42_core::ObjectKind::Ellipse => {
                    items.push(SceneItem::Ellipse {
                        x: o.x,
                        y: o.y,
                        width: o.width,
                        height: o.height,
                        fill: o.fill_visible.then_some(o.fill),
                        stroke: (o.stroke_visible
                            || (editable && show_selection && self.selection().contains(&o.id)))
                        .then_some(stroke),
                    });
                }
                ppt42_core::ObjectKind::Picture => {
                    if let Ok(image) = decode_image(&o.media) {
                        let pixels = image.to_rgba8();
                        items.push(SceneItem::ImageScaled {
                            x: o.x,
                            y: o.y,
                            width: o.width,
                            height: o.height,
                            pixel_width: pixels.width(),
                            pixel_height: pixels.height(),
                            rgba: pixels.into_raw(),
                        });
                        if editable && show_selection && self.selection().contains(&o.id) {
                            items.push(SceneItem::Path {
                                points: vec![
                                    Point { x: o.x, y: o.y },
                                    Point {
                                        x: o.x + o.width,
                                        y: o.y,
                                    },
                                    Point {
                                        x: o.x + o.width,
                                        y: o.y + o.height,
                                    },
                                    Point {
                                        x: o.x,
                                        y: o.y + o.height,
                                    },
                                ],
                                color: stroke,
                                closed: true,
                            });
                        }
                    }
                }
                ppt42_core::ObjectKind::Table => {
                    let rows = o.cells.len();
                    let cols = o.cells.first().map_or(0, Vec::len);
                    if rows > 0 && cols > 0 {
                        let w = o.width / cols as f64;
                        let h = o.height / rows as f64;
                        for (r, row) in o.cells.iter().enumerate() {
                            for (c, value) in row.iter().enumerate() {
                                let x = o.x + c as f64 * w;
                                let y = o.y + r as f64 * h;
                                items.push(SceneItem::Rect {
                                    x,
                                    y,
                                    width: w,
                                    height: h,
                                    fill: o.fill,
                                    stroke,
                                });
                                items.push(SceneItem::TextBox {
                                    x: x + 4.0,
                                    y: y + 4.0,
                                    width: (w - 8.0).max(1.0),
                                    height: (h - 8.0).max(1.0),
                                    text: value.clone(),
                                    color: [0, 0, 0],
                                    font: win42::SceneFont {
                                        family: "Arial".into(),
                                        points: 12.0,
                                        bold: false,
                                        italic: false,
                                        underline: false,
                                    },
                                    align: win42::TextAlign::Left,
                                    wrap: true,
                                });
                            }
                        }
                    }
                }
            }
            if !o.text.is_empty() {
                let style = &o.text_style;
                if o.paragraphs.is_empty() {
                    items.push(SceneItem::TextBox {
                        x: o.x + 5.0,
                        y: o.y + 5.0,
                        width: (o.width - 10.0).max(1.0),
                        height: (o.height - 10.0).max(1.0),
                        text: o.text.clone(),
                        color: style.color,
                        font: win42::SceneFont {
                            family: style.family.clone(),
                            points: style.points,
                            bold: style.bold,
                            italic: style.italic,
                            underline: style.underline,
                        },
                        align: match style.align {
                            ppt42_core::TextAlign::Left => win42::TextAlign::Left,
                            ppt42_core::TextAlign::Center => win42::TextAlign::Center,
                            ppt42_core::TextAlign::Right => win42::TextAlign::Right,
                        },
                        wrap: true,
                    });
                } else {
                    items.push(SceneItem::RichText {
                        x: o.x + 5.0,
                        y: o.y + 5.0,
                        width: (o.width - 10.0).max(1.0),
                        height: (o.height - 10.0).max(1.0),
                        paragraphs: o
                            .paragraphs
                            .iter()
                            .map(|p| win42::SceneParagraph {
                                align: match p.align {
                                    ppt42_core::TextAlign::Left => win42::TextAlign::Left,
                                    ppt42_core::TextAlign::Center => win42::TextAlign::Center,
                                    ppt42_core::TextAlign::Right => win42::TextAlign::Right,
                                },
                                bullet: p.bullet,
                                runs: p
                                    .runs
                                    .iter()
                                    .map(|r| win42::SceneRun {
                                        text: r.text.clone(),
                                        font: win42::SceneFont {
                                            family: r.style.family.clone(),
                                            points: r.style.points,
                                            bold: r.style.bold,
                                            italic: r.style.italic,
                                            underline: r.style.underline,
                                        },
                                        color: r.style.color,
                                    })
                                    .collect(),
                            })
                            .collect(),
                    });
                }
            }
            if editable
                && show_selection
                && self.selection().len() == 1
                && self.selection().contains(&o.id)
            {
                items.push(SceneItem::Rect {
                    x: o.x + o.width - 4.0,
                    y: o.y + o.height - 4.0,
                    width: 8.0,
                    height: 8.0,
                    fill: [255, 255, 255],
                    stroke: [0, 100, 220],
                });
            }
            if o.rotation_degrees.rem_euclid(360.0) != 0.0 {
                let children = items.split_off(object_start);
                items.push(SceneItem::Transform {
                    center: Point {
                        x: o.x + o.width / 2.0,
                        y: o.y + o.height / 2.0,
                    },
                    degrees: o.rotation_degrees,
                    items: children,
                });
            }
        }
        if show_selection {
            if let Some((a, b, _)) = self.rubber_band {
                items.push(SceneItem::Path {
                    points: vec![
                        Point { x: a.x, y: a.y },
                        Point { x: b.x, y: a.y },
                        Point { x: b.x, y: b.y },
                        Point { x: a.x, y: b.y },
                    ],
                    color: [0, 100, 220],
                    closed: true,
                });
            }
        }
        Some(CanvasScene {
            viewport: win42::Viewport::default(),
            items,
        })
    }
}
impl win42::App for Ppt {
    fn inline_rich_rect(&self) -> Option<win42::CanvasRect> {
        let o = self.rich_draft.as_ref()?;
        Some(win42::CanvasRect {
            x: o.x + 5.0,
            y: o.y + 5.0,
            width: (o.width - 10.0).max(1.0),
            height: (o.height - 10.0).max(1.0),
        })
    }
    fn finish_inline_rich(&mut self, commit: bool) -> Result<(), String> {
        self.finish_rich(commit)
    }
    fn uses_rich_edit(&self) -> bool {
        true
    }
    fn rich_text(&self) -> Option<Vec<u8>> {
        let object = self.rich_object().ok()?;
        rich::document(&object).ok()?.rich_edit_projection().ok()
    }
    fn edit_rich_text(&mut self, rtf: &[u8], selection: (usize, usize)) -> Result<(), String> {
        let object = self.rich_object()?;
        let mut doc = rich::document(&object)?;
        doc.apply_rich_edit_projection(rtf, selection)
            .map_err(|e| e.to_string())?;
        self.rich_selection = selection;
        self.save_rich_document(&doc)
    }
    fn set_rich_selection(&mut self, selection: (usize, usize)) {
        self.rich_selection = selection;
    }
    fn rich_selection_to_restore(&self) -> Option<(usize, usize)> {
        self.rich_edit_object.map(|_| self.rich_selection)
    }
    fn canvas_pointer_modifiers(&self) -> bool {
        true
    }
    fn clipboard_request(&self, id: u16) -> Option<win42::ClipboardRequest> {
        match id {
            56 => self.deck.copy_objects(&self.selection()).ok().map(|bytes| {
                win42::ClipboardRequest::WriteData {
                    format: "Suite42.Presentation.Objects.v1".into(),
                    bytes,
                }
            }),
            57 => Some(win42::ClipboardRequest::ReadData(
                "Suite42.Presentation.Objects.v1".into(),
            )),
            _ => None,
        }
    }
    fn clipboard_data(&mut self, id: u16, bytes: Vec<u8>) -> Result<(), String> {
        if id != 57 {
            return Err("Unknown clipboard command".into());
        }
        if (self.pptx_source.is_some() || self.odp_source.is_some())
            && Presentation::clipboard_has_groups(&bytes)?
        {
            return Err(
                "Pasting grouped objects into an existing office package is not supported yet"
                    .into(),
            );
        }
        let ids = self.deck.paste_objects(&bytes)?;
        self.selected_object = ids.last().copied();
        self.selected_objects = ids;
        Ok(())
    }
    fn presentation_size(&self) -> Option<(f64, f64)> {
        self.presenting.then_some((720.0, 405.0))
    }
    fn print_job(&self, id: u16) -> Option<win42::PrintJob> {
        if id != 40 {
            return None;
        }
        let (width, height) = if let Some(source) = &self.odp_source {
            source.size_points()?
        } else {
            self.pptx_source
                .as_ref()
                .map(|s| s.size_points())
                .unwrap_or((720.0, 405.0))
        };
        if !width.is_finite()
            || !height.is_finite()
            || width <= 0.0
            || height <= 0.0
            || width > 100000.0
            || height > 100000.0
        {
            return None;
        }
        let geometry = win42::PrintGeometry {
            width_twips: (width * 20.0).round() as i32,
            height_twips: (height * 20.0).round() as i32,
            margin_left_twips: 0,
            margin_top_twips: 0,
            margin_right_twips: 0,
            margin_bottom_twips: 0,
        };
        // Fit the logical slide without distorting rotated content.
        let sx = (width / 720.0).min(height / 405.0);
        let sy = sx;
        let mut pages = Vec::new();
        for index in 0..self.deck.slides().len() {
            let mut scene = self.scene_for(index, false)?;
            fn scale_items(items: &mut [win42::SceneItem], sx: f64, sy: f64) -> Option<()> {
                for item in items {
                    match item {
                        win42::SceneItem::OpacityLayer { bounds, items, .. } => {
                            bounds.x *= sx;
                            bounds.y *= sy;
                            bounds.width *= sx;
                            bounds.height *= sy;
                            scale_items(items, sx, sy)?;
                        }
                        win42::SceneItem::ClippedOffset {
                            clip,
                            offset,
                            items,
                        } => {
                            clip.x *= sx;
                            clip.y *= sy;
                            clip.width *= sx;
                            clip.height *= sy;
                            offset.x *= sx;
                            offset.y *= sy;
                            scale_items(items, sx, sy)?;
                        }
                        win42::SceneItem::Transform { center, items, .. } => {
                            if (sx - sy).abs() > 1e-9 {
                                return None;
                            }
                            center.x *= sx;
                            center.y *= sy;
                            scale_items(items, sx, sy)?;
                        }
                        win42::SceneItem::Rect {
                            x,
                            y,
                            width,
                            height,
                            ..
                        } => {
                            *x *= sx;
                            *y *= sy;
                            *width *= sx;
                            *height *= sy;
                        }
                        win42::SceneItem::Ellipse {
                            x,
                            y,
                            width,
                            height,
                            ..
                        } => {
                            *x *= sx;
                            *y *= sy;
                            *width *= sx;
                            *height *= sy;
                        }
                        win42::SceneItem::Text { x, y, .. } => {
                            *x *= sx;
                            *y *= sy;
                        }
                        win42::SceneItem::TextBox {
                            x,
                            y,
                            width,
                            height,
                            font,
                            ..
                        } => {
                            *x *= sx;
                            *y *= sy;
                            *width *= sx;
                            *height *= sy;
                            font.points *= sy;
                        }
                        win42::SceneItem::RichText {
                            x,
                            y,
                            width,
                            height,
                            paragraphs,
                        } => {
                            *x *= sx;
                            *y *= sy;
                            *width *= sx;
                            *height *= sy;
                            for p in paragraphs {
                                for r in &mut p.runs {
                                    r.font.points *= sy;
                                }
                            }
                        }
                        win42::SceneItem::Path { points, .. } => {
                            for p in points {
                                p.x *= sx;
                                p.y *= sy;
                            }
                        }
                        win42::SceneItem::Image {
                            x,
                            y,
                            width,
                            height,
                            rgba,
                        } => {
                            *x *= sx;
                            *y *= sy;
                            let nw = ((*width as f64 * sx).round() as u32).clamp(1, 4096);
                            let nh = ((*height as f64 * sy).round() as u32).clamp(1, 4096);
                            let image =
                                image::RgbaImage::from_raw(*width, *height, std::mem::take(rgba))?;
                            *rgba = image::imageops::resize(
                                &image,
                                nw,
                                nh,
                                image::imageops::FilterType::Triangle,
                            )
                            .into_raw();
                            *width = nw;
                            *height = nh;
                        }
                        win42::SceneItem::ImageScaled {
                            x,
                            y,
                            width,
                            height,
                            ..
                        } => {
                            *x *= sx;
                            *y *= sy;
                            *width *= sx;
                            *height *= sy;
                        }
                    }
                }
                Some(())
            }
            scale_items(&mut scene.items, sx, sy)?;
            pages.push(win42::PrintPage { geometry, scene });
        }
        Some(win42::PrintJob::Scenes(pages))
    }
    fn open_formats(&self) -> Vec<win42::FileFormat> {
        vec![
            win42::FileFormat {
                id: "pptx",
                title: "PowerPoint presentation",
                extensions: &["pptx"],
            },
            win42::FileFormat {
                id: "odp",
                title: "OpenDocument presentation",
                extensions: &["odp"],
            },
            win42::FileFormat {
                id: "json",
                title: "Suite42 slides JSON",
                extensions: &["json"],
            },
        ]
    }
    fn mark_file_action(&self) -> Option<u16> {
        Some(32)
    }
    fn file_actions(&self) -> &'static [win42::FileAction] {
        &[win42::FileAction {
            id: 32,
            title: "Insert Image…",
            save: false,
            extension: "png",
            format: "Images",
            pattern: "*.png;*.jpg;*.jpeg",
        }]
    }
    fn file_action(&mut self, id: u16, path: &Path) -> Result<(), String> {
        if id != 32 {
            return Err("Unknown file action".into());
        }
        let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
        if bytes.len() > 16 * 1024 * 1024 {
            return Err("Image exceeds size limit".into());
        }
        decode_image(&bytes)?;
        self.selected_object = Some(self.deck.insert_picture(bytes)?);
        self.selected_objects.clear();
        Ok(())
    }
    fn dialog_request(&self, id: u16) -> Option<win42::DialogRequest> {
        let object = self.selected_object.and_then(|id| {
            self.deck.slides()[self.deck.current()]
                .objects
                .iter()
                .find(|o| o.id == id)
        })?;
        match id {
            30 => Some(win42::DialogRequest::Color(object.fill)),
            31 => Some(win42::DialogRequest::Color(object.stroke)),
            42 => Some(win42::DialogRequest::Font),
            _ => None,
        }
    }
    fn dialog_result(&mut self, id: u16, result: win42::DialogResult) -> Result<(), String> {
        if id == 42 && self.rich_edit_object.is_some() {
            return match result {
                win42::DialogResult::Font(font) => self.format_rich_font(font),
                _ => Err("Expected a font".into()),
            };
        }
        let object_id = self.selected_object.ok_or("Select an object")?;
        let mut object = self.deck.slides()[self.deck.current()]
            .objects
            .iter()
            .find(|o| o.id == object_id)
            .cloned()
            .ok_or("Object no longer exists")?;
        match (id, result) {
            (30, win42::DialogResult::Color(color)) => object.fill = color,
            (31, win42::DialogResult::Color(color)) => object.stroke = color,
            (42, win42::DialogResult::Font(font)) => {
                let style = &mut object.text_style;
                style.family = font.family;
                style.points = font.points;
                style.bold = font.bold;
                style.italic = font.italic;
                style.underline = font.underline;
                style.color = font.color;
                for paragraph in &mut object.paragraphs {
                    for run in &mut paragraph.runs {
                        run.style = style.clone();
                    }
                }
            }
            _ => return Err("Unknown color action".into()),
        }
        self.deck.update_object(object)
    }
    fn input_request(&self, id: u16) -> Option<win42::InputRequest> {
        let slide = &self.deck.slides()[self.deck.current()];
        match id {
            66 => Some(win42::InputRequest {
                title: "Slide timing and transition",
                fields: vec![
                    (
                        "Advance after milliseconds (empty = manual)",
                        slide
                            .timing
                            .advance_ms
                            .map(|v| v.to_string())
                            .unwrap_or_default(),
                    ),
                    (
                        "Transition (cut, fade, or push-left)",
                        match slide.timing.transition {
                            ppt42_core::Transition::Cut => "cut",
                            ppt42_core::Transition::PushLeft => "push-left",
                            ppt42_core::Transition::Fade => "fade",
                        }
                        .into(),
                    ),
                    (
                        "Transition duration milliseconds",
                        slide.timing.duration_ms.to_string(),
                    ),
                ],
            }),
            24 => Some(win42::InputRequest {
                title: "Edit slide text",
                fields: vec![("Title", slide.title.clone()), ("Body", slide.body.clone())],
            }),
            25 => self
                .selected_object
                .and_then(|id| slide.objects.iter().find(|o| o.id == id))
                .map(|o| win42::InputRequest {
                    title: "Edit object text",
                    fields: vec![("Text", o.text.clone())],
                }),
            26 => self
                .selected_object
                .and_then(|id| slide.objects.iter().find(|o| o.id == id))
                .map(|o| win42::InputRequest {
                    title: "Object geometry",
                    fields: vec![
                        ("X", o.x.to_string()),
                        ("Y", o.y.to_string()),
                        ("Width", o.width.to_string()),
                        ("Height", o.height.to_string()),
                        ("Rotation (degrees)", o.rotation_degrees.to_string()),
                    ],
                }),
            33 => Some(win42::InputRequest {
                title: "Insert table",
                fields: vec![("Rows", "3".into()), ("Columns", "3".into())],
            }),
            65 => Some(win42::InputRequest {
                title: "Rotate selected objects",
                fields: vec![("Clockwise degrees", "90".into())],
            }),
            34 => self
                .selected_object
                .and_then(|id| {
                    slide
                        .objects
                        .iter()
                        .find(|o| o.id == id && matches!(o.kind, ppt42_core::ObjectKind::Table))
                })
                .map(|_| win42::InputRequest {
                    title: "Edit table cell",
                    fields: vec![
                        ("Row (1-based)", "1".into()),
                        ("Column (1-based)", "1".into()),
                        ("Text", String::new()),
                    ],
                }),
            _ => None,
        }
    }
    fn input_action(&mut self, id: u16, values: &[String]) -> Result<(), String> {
        match id {
            66 if values.len() == 3 => {
                let advance_ms = if values[0].trim().is_empty() {
                    None
                } else {
                    Some(
                        values[0]
                            .trim()
                            .parse()
                            .map_err(|_| "Invalid advance time")?,
                    )
                };
                let transition = match values[1].trim().to_ascii_lowercase().as_str() {
                    "cut" => ppt42_core::Transition::Cut,
                    "push-left" => ppt42_core::Transition::PushLeft,
                    "fade" => ppt42_core::Transition::Fade,
                    _ => return Err("Use cut, fade, or push-left".into()),
                };
                let duration_ms = values[2]
                    .trim()
                    .parse()
                    .map_err(|_| "Invalid transition duration")?;
                if self.odp_source.is_some() && ![250, 500, 1000].contains(&duration_ms) {
                    return Err(
                        "ODP transition duration must be 250, 500, or 1000 milliseconds".into(),
                    );
                }
                let slide = self.deck.slides()[self.deck.current()].source_identity();
                if self
                    .pptx_source
                    .as_ref()
                    .is_some_and(|s| !s.can_edit_timing(slide))
                    || self
                        .odp_source
                        .as_ref()
                        .is_some_and(|s| !s.can_edit_timing(slide))
                {
                    return Err("This slide transition cannot be safely edited".into());
                }
                self.deck.set_timing(ppt42_core::SlideTiming {
                    advance_ms,
                    transition,
                    duration_ms,
                })
            }
            65 if values.len() == 1 => {
                let angle: f64 = values[0].parse().map_err(|_| "Invalid rotation")?;
                if !angle.is_finite() {
                    return Err("Invalid rotation".into());
                }
                let ids = self.selection();
                let slide_id = self.deck.slides()[self.deck.current()].source_identity();
                if self
                    .pptx_source
                    .as_ref()
                    .is_some_and(|s| ids.iter().any(|id| !s.can_rotate(slide_id, *id)))
                {
                    return Err("This source geometry cannot be rotated safely".into());
                }
                let mut objects = self.deck.slides()[self.deck.current()]
                    .objects
                    .iter()
                    .filter(|o| ids.contains(&o.id))
                    .cloned()
                    .collect::<Vec<_>>();
                if objects.is_empty() {
                    return Err("Select objects first".into());
                }
                let mut left = f64::INFINITY;
                let mut right = f64::NEG_INFINITY;
                let mut top = f64::INFINITY;
                let mut bottom = f64::NEG_INFINITY;
                for o in &objects {
                    for (x, y) in [
                        (o.x, o.y),
                        (o.x + o.width, o.y),
                        (o.x, o.y + o.height),
                        (o.x + o.width, o.y + o.height),
                    ] {
                        let p = object_point(o, x, y);
                        left = left.min(p.x);
                        right = right.max(p.x);
                        top = top.min(p.y);
                        bottom = bottom.max(p.y);
                    }
                }
                let (cx, cy) = ((left + right) / 2.0, (top + bottom) / 2.0);
                let (sin, cos) = angle.rem_euclid(360.0).to_radians().sin_cos();
                for o in &mut objects {
                    let (x, y) = (o.x + o.width / 2.0 - cx, o.y + o.height / 2.0 - cy);
                    o.x = cx + cos * x - sin * y - o.width / 2.0;
                    o.y = cy + sin * x + cos * y - o.height / 2.0;
                    o.rotation_degrees = (o.rotation_degrees.rem_euclid(360.0)
                        + angle.rem_euclid(360.0))
                    .rem_euclid(360.0);
                }
                self.deck.update_objects(objects)
            }
            33 if values.len() == 2 => {
                let rows = values[0].parse().map_err(|_| "Invalid row count")?;
                let cols = values[1].parse().map_err(|_| "Invalid column count")?;
                self.selected_object = Some(self.deck.insert_table(rows, cols)?);
                self.selected_objects.clear();
                Ok(())
            }
            34 if values.len() == 3 => {
                let row: usize = values[0]
                    .parse::<usize>()
                    .map_err(|_| "Invalid row")?
                    .checked_sub(1)
                    .ok_or("Invalid row")?;
                let column: usize = values[1]
                    .parse::<usize>()
                    .map_err(|_| "Invalid column")?
                    .checked_sub(1)
                    .ok_or("Invalid column")?;
                let id = self.selected_object.ok_or("Select a table")?;
                if let Some(source) = &self.pptx_source {
                    if values[2].contains('\n')
                        || !source.can_edit_cell(
                            self.deck.slides()[self.deck.current()].source_identity(),
                            id,
                            row,
                            column,
                        )
                    {
                        return Err("This PPTX cell cannot be edited without changing unsupported text structure".into());
                    }
                }
                if let Some(source) = &self.odp_source {
                    if values[2].contains('\n')
                        || !source.can_edit_cell(
                            self.deck.slides()[self.deck.current()].source_identity(),
                            id,
                            row,
                            column,
                        )
                    {
                        return Err("This ODP cell cannot be edited without changing unsupported text structure".into());
                    }
                }
                let mut object = self.deck.slides()[self.deck.current()]
                    .objects
                    .iter()
                    .find(|o| o.id == id)
                    .cloned()
                    .ok_or("Object no longer exists")?;
                *object
                    .cells
                    .get_mut(row)
                    .and_then(|r| r.get_mut(column))
                    .ok_or("Cell out of range")? = values[2].clone();
                self.deck.update_object(object)
            }
            24 if values.len() == 2 => self.deck.edit(values[0].clone(), values[1].clone()),
            25 if values.len() == 1 => {
                if (self.pptx_source.is_some() || self.odp_source.is_some())
                    && values[0].contains('\n')
                {
                    return Err("Multiline PPTX text edits are not supported yet".into());
                }
                let id = self.selected_object.ok_or("Select an object")?;
                let mut object = self.deck.slides()[self.deck.current()]
                    .objects
                    .iter()
                    .find(|o| o.id == id)
                    .cloned()
                    .ok_or("Object no longer exists")?;
                object.text = values[0].clone();
                object.paragraphs.clear();
                self.deck.update_object(object)
            }
            26 if values.len() == 5 => {
                let id = self.selected_object.ok_or("Select an object")?;
                let mut object = self.deck.slides()[self.deck.current()]
                    .objects
                    .iter()
                    .find(|o| o.id == id)
                    .cloned()
                    .ok_or("Object no longer exists")?;
                let parsed = values
                    .iter()
                    .map(|v| v.parse::<f64>().map_err(|_| "Invalid geometry".to_string()))
                    .collect::<Result<Vec<_>, _>>()?;
                if parsed[4].rem_euclid(360.0) != object.rotation_degrees.rem_euclid(360.0)
                    && self.pptx_source.as_ref().is_some_and(|s| {
                        !s.can_rotate(
                            self.deck.slides()[self.deck.current()].source_identity(),
                            id,
                        )
                    })
                {
                    return Err("This source geometry cannot be rotated safely".into());
                }
                object.x = parsed[0];
                object.y = parsed[1];
                object.width = parsed[2];
                object.height = parsed[3];
                object.rotation_degrees = parsed[4].rem_euclid(360.0);
                self.deck.update_object(object)
            }
            _ => Err("Invalid text input".into()),
        }
    }
    fn canvas_scene(&self) -> Option<win42::CanvasScene> {
        let mut current = self.scene_for(self.deck.current(), !self.presenting)?;
        if let Some(show) = self.show.as_ref().filter(|_| self.presenting) {
            if let Some(previous) = show.previous {
                let now = show.paused.unwrap_or_else(std::time::Instant::now);
                let elapsed = now.saturating_duration_since(show.since).as_secs_f64();
                let duration =
                    self.deck.slides()[self.deck.current()].timing.duration_ms as f64 / 1000.0;
                let fraction = (elapsed / duration).clamp(0.0, 1.0);
                let old = self.scene_for(previous, false)?;
                let clip = win42::CanvasRect {
                    x: 0.0,
                    y: 0.0,
                    width: 720.0,
                    height: 405.0,
                };
                current.items = if self.deck.slides()[self.deck.current()].timing.transition
                    == ppt42_core::Transition::Fade
                {
                    let mut layers = old.items;
                    layers.push(win42::SceneItem::OpacityLayer {
                        bounds: clip,
                        opacity: (fraction * 255.0).round() as u8,
                        items: current.items,
                    });
                    layers
                } else {
                    vec![
                        win42::SceneItem::ClippedOffset {
                            clip,
                            offset: win42::Point {
                                x: -720.0 * fraction,
                                y: 0.0,
                            },
                            items: old.items,
                        },
                        win42::SceneItem::ClippedOffset {
                            clip,
                            offset: win42::Point {
                                x: 720.0 * (1.0 - fraction),
                                y: 0.0,
                            },
                            items: current.items,
                        },
                    ]
                };
            }
        }
        Some(current)
    }
    fn poll_background(&mut self) -> bool {
        self.show_tick(std::time::Instant::now())
    }
    fn background_poll_ms(&self) -> u32 {
        if self.presenting
            && self
                .show
                .as_ref()
                .is_some_and(|s| s.previous.is_some() && s.paused.is_none())
        {
            16
        } else {
            100
        }
    }
    fn canvas_event(&mut self, event: win42::CanvasEvent) -> Result<(), String> {
        use win42::CanvasEvent::*;
        if self.presenting {
            return self.show_event(event, std::time::Instant::now());
        }
        match event {
            DoubleClick(p) => {
                self.pointer_down(p, false);
                self.drag = None;
                if self.enabled(58) {
                    self.action(58)?;
                }
            }
            PointerDown(p) => self.pointer_down(p, false),
            PointerDownModified { point, modifiers } => self.pointer_down(point, modifiers != 0),
            PointerMove(p) => {
                if let Some(drag) = self.drag.as_mut() {
                    match drag {
                        Drag::Move(start, objects) => {
                            for object in objects {
                                object.x += p.x - start.x;
                                object.y += p.y - start.y;
                            }
                            *start = p;
                        }
                        Drag::Resize(start, object) => {
                            resize_object(object, *start, p);
                            *start = p;
                        }
                    }
                }
                if let Some((_, ref mut current, _)) = self.rubber_band {
                    *current = p;
                }
            }
            PointerUp(p) => {
                if let Some(drag) = self.drag.take() {
                    match drag {
                        Drag::Move(start, mut objects) => {
                            for object in &mut objects {
                                object.x += p.x - start.x;
                                object.y += p.y - start.y;
                            }
                            self.deck.update_objects(objects)?;
                        }
                        Drag::Resize(start, mut object) => {
                            resize_object(&mut object, start, p);
                            self.deck.update_object(object)?;
                        }
                    }
                }
                if let Some((a, _, add)) = self.rubber_band.take() {
                    let (left, right) = (a.x.min(p.x), a.x.max(p.x));
                    let (top, bottom) = (a.y.min(p.y), a.y.max(p.y));
                    let mut ids = if add { self.selection() } else { Vec::new() };
                    for o in &self.deck.slides()[self.deck.current()].objects {
                        if [
                            (o.x, o.y),
                            (o.x + o.width, o.y),
                            (o.x, o.y + o.height),
                            (o.x + o.width, o.y + o.height),
                        ]
                        .into_iter()
                        .map(|(x, y)| object_point(o, x, y))
                        .all(|p| p.x >= left && p.x <= right && p.y >= top && p.y <= bottom)
                            && !ids.contains(&o.id)
                        {
                            ids.push(o.id);
                        }
                    }
                    let groups = self.deck.slides()[self.deck.current()]
                        .objects
                        .iter()
                        .filter(|o| ids.contains(&o.id))
                        .filter_map(|o| o.group_id)
                        .collect::<Vec<_>>();
                    for o in &self.deck.slides()[self.deck.current()].objects {
                        if o.group_id.is_some_and(|id| groups.contains(&id)) && !ids.contains(&o.id)
                        {
                            ids.push(o.id);
                        }
                    }
                    self.selected_object = ids.last().copied();
                    self.selected_objects = ids;
                }
            }
            Cancel => {
                self.drag = None;
                self.rubber_band = None;
            }
            Key { code: 46, .. } => {
                if self.enabled(27) {
                    let ids = self.selection();
                    self.deck.delete_objects(&ids)?;
                    self.selected_object = None;
                    self.selected_objects.clear();
                }
            }
            _ => {}
        }
        Ok(())
    }
    fn save_formats(&self) -> Vec<win42::FileFormat> {
        if self.odp_source.is_some() {
            return vec![win42::FileFormat {
                id: "odp",
                title: "OpenDocument presentation",
                extensions: &["odp"],
            }];
        }
        if self.pptx_source.is_some() {
            return vec![win42::FileFormat {
                id: "pptx",
                title: "PowerPoint presentation",
                extensions: &["pptx"],
            }];
        }
        vec![
            win42::FileFormat {
                id: "json",
                title: "Suite42 slides JSON",
                extensions: &["json"],
            },
            win42::FileFormat {
                id: "odp",
                title: "OpenDocument presentation",
                extensions: &["odp"],
            },
            win42::FileFormat {
                id: "pptx",
                title: "PowerPoint presentation",
                extensions: &["pptx"],
            },
        ]
    }
    fn save_snapshot(&mut self, format: &str) -> Result<Option<(u64, Vec<u8>)>, String> {
        if self.rich_edit_object.is_some() {
            self.finish_rich(true)?;
        }
        let (native_bytes, state) = (&self.deck).save_snapshot().map_err(|e| e.to_string())?;
        let bytes = match (format, self.pptx_source.as_ref()) {
            ("json", None) if self.odp_source.is_none() => native_bytes,
            ("pptx", Some(source)) => ppt42_core::pptx::export(&self.deck, source)?,
            ("pptx", None) if self.odp_source.is_none() => {
                ppt42_core::pptx::export_new(&self.deck)?
            }
            ("odp", None) if self.odp_source.is_some() => {
                ppt42_core::odp::export_overlay(&self.deck, self.odp_source.as_ref().unwrap())?
            }
            ("odp", None) => ppt42_core::odp::export(&self.deck)?,
            _ => return Err("Unsupported save format".into()),
        };
        self.next_save = self.next_save.wrapping_add(1);
        let revision = self.next_save;
        self.pending = Some((revision, state));
        Ok(Some((revision, bytes)))
    }
    fn saved_snapshot(&mut self, revision: u64) {
        if self.pending.as_ref().is_some_and(|(r, _)| *r == revision) {
            if let Some((_, state)) = self.pending.take() {
                if let Some(doc) = Some(&mut self.deck) {
                    doc.mark_saved_snapshot(state);
                }
            }
        }
    }

    fn name(&self) -> &'static str {
        "PPT42"
    }
    fn extension(&self) -> &'static str {
        if self.odp_source.is_some() {
            "odp"
        } else if self.pptx_source.is_some() {
            "pptx"
        } else {
            "json"
        }
    }
    fn format_name(&self) -> &'static str {
        if self.odp_source.is_some() {
            "OpenDocument presentation"
        } else if self.pptx_source.is_some() {
            "PowerPoint presentation"
        } else {
            "Suite42 slides JSON"
        }
    }
    fn actions(&self) -> &'static [(u16, &'static str)] {
        &[
            (10, "&Add Slide"),
            (11, "&Delete Slide"),
            (12, "Move Slide &Up"),
            (13, "Move Slide D&own"),
            (14, "D&uplicate Slide"),
            (20, "Insert Rectangle"),
            (21, "Insert Ellipse"),
            (22, "Insert Line"),
            (23, "Insert Text Box"),
            (24, "Edit Slide Text…"),
            (25, "Edit Object Text…"),
            (26, "Object Geometry…"),
            (65, "Rotate Selected Objects…"),
            (27, "Delete Object"),
            (28, "Bring to Front"),
            (29, "Send to Back"),
            (30, "Fill Color…"),
            (31, "Line Color…"),
            (33, "Insert Table…"),
            (34, "Edit Table Cell…"),
            (40, "Print Slides…"),
            (41, "Start Slide Show"),
            (67, "Start Slide Show from Beginning"),
            (66, "Slide Timing and Transition…"),
            (42, "Text Font…"),
            (43, "Align Text Left"),
            (44, "Align Text Center"),
            (45, "Align Text Right"),
            (46, "Select All Objects"),
            (47, "Align Objects Left"),
            (48, "Align Objects Center"),
            (49, "Align Objects Right"),
            (50, "Align Objects Top"),
            (51, "Align Objects Middle"),
            (52, "Align Objects Bottom"),
            (53, "Distribute Horizontally"),
            (54, "Distribute Vertically"),
            (55, "Duplicate Objects"),
            (56, "Copy Objects"),
            (57, "Paste Objects"),
            (58, "Edit Rich Text / Done"),
            (59, "Bold Selection"),
            (60, "Italic Selection"),
            (61, "Underline Selection"),
            (62, "Toggle Paragraph Bullet"),
            (63, "Group Objects"),
            (64, "Ungroup Objects"),
        ]
    }
    fn items(&self) -> Vec<String> {
        self.deck
            .slides()
            .iter()
            .enumerate()
            .map(|(i, s)| format!("{}: {}", i + 1, s.title))
            .collect()
    }
    fn text(&self) -> String {
        let s = &self.deck.slides()[self.deck.current()];
        format!("{}\r\n{}", s.title, win42::to_native_text(&s.body))
    }
    fn status(&self) -> String {
        if self.presenting {
            let state = if self.show.as_ref().is_some_and(|show| show.paused.is_some()) {
                "Paused"
            } else {
                "Playing"
            };
            return format!(
                "Slide {} of {} | {} | arrows navigate, P pauses, Esc exits",
                self.deck.current() + 1,
                self.deck.slides().len(),
                state
            );
        }
        if self.rich_edit_object.is_some() {
            return "Editing text in place | Select text for formatting | click outside or Done to commit; Esc cancels".into();
        }
        if self.odp_source.is_some() {
            return format!("Slide {} of {} | ODP: supported objects editable; other content preserved but may not be displayed",self.deck.current()+1,self.deck.slides().len());
        }
        if self.pptx_source.is_some() {
            return format!("Slide {} of {} | PPTX: supported shapes editable; other content preserved but may not be displayed", self.deck.current()+1,self.deck.slides().len());
        }
        format!(
            "Slide {} of {} | first line is title; remaining text is body | Suite42 JSON",
            self.deck.current() + 1,
            self.deck.slides().len()
        )
    }
    fn dirty(&self) -> bool {
        self.deck.is_dirty()
            || self.rich_draft.as_ref().is_some_and(|draft| {
                self.deck.slides()[self.deck.current()]
                    .objects
                    .iter()
                    .find(|o| o.id == draft.id)
                    .is_none_or(|o| o != draft)
            })
    }
    fn can_undo(&self) -> bool {
        self.rich_draft.as_ref().is_some_and(|draft| {
            self.deck.slides()[self.deck.current()]
                .objects
                .iter()
                .find(|o| o.id == draft.id)
                .is_none_or(|o| o != draft)
        }) || self.deck.can_undo()
    }
    fn can_redo(&self) -> bool {
        self.rich_draft.is_none() && self.deck.can_redo()
    }
    fn selected(&self) -> usize {
        self.deck.current()
    }
    fn open(&mut self, p: &Path) -> Result<(), String> {
        if p.extension()
            .and_then(|s| s.to_str())
            .is_some_and(|s| s.eq_ignore_ascii_case("odp"))
        {
            let bytes = std::fs::read(p).map_err(|e| e.to_string())?;
            let (deck, source) = ppt42_core::odp::import_with_source(&bytes)?;
            *self = Ppt::new(deck);
            self.odp_source = Some(source);
            return Ok(());
        }
        if p.extension()
            .and_then(|s| s.to_str())
            .is_some_and(|s| s.eq_ignore_ascii_case("pptx"))
        {
            let bytes = std::fs::read(p).map_err(|e| e.to_string())?;
            let (deck, source) = ppt42_core::pptx::import(&bytes)?;
            *self = Ppt::new(deck);
            self.pptx_source = Some(source);
            return Ok(());
        }
        let presentation = Presentation::open(p)?;
        for slide in presentation.slides() {
            win42::validate_editable(&slide.title)?;
            win42::validate_editable(&slide.body)?;
            if slide.title.contains('\n') {
                return Err("Slide titles containing line breaks cannot be preserved by this editor. File was not opened.".into());
            }
        }
        *self = Ppt::new(presentation);
        Ok(())
    }
    fn save(&mut self, p: &Path) -> Result<(), String> {
        if self.rich_edit_object.is_some() {
            self.finish_rich(true)?;
        }
        if let Some(source) = &self.odp_source {
            use std::io::Write;
            let bytes = ppt42_core::odp::export_overlay(&self.deck, source)?;
            std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(p)
                .and_then(|mut f| f.write_all(&bytes))
                .map_err(|e| e.to_string())?;
            return Ok(());
        }
        if let Some(source) = &self.pptx_source {
            use std::io::Write;
            let bytes = ppt42_core::pptx::export(&self.deck, source)?;
            std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(p)
                .and_then(|mut f| f.write_all(&bytes))
                .map_err(|e| e.to_string())?;
            return Ok(());
        }
        self.deck.save_as(p)
    }
    fn edit(&mut self, s: String) -> Result<(), String> {
        let text = win42::from_native_text(&s);
        let (title, body) = text.split_once('\n').unwrap_or((&text, ""));
        self.deck.edit(title.into(), body.into())
    }
    fn select(&mut self, i: usize) {
        if self.rich_edit_object.is_some() {
            let _ = self.finish_rich(true);
        }
        let _ = self.deck.select(i);
        self.selected_object = None;
        self.selected_objects.clear();
        self.rich_edit_object = None;
        self.drag = None;
        self.rubber_band = None;
    }
    fn enabled(&self, id: u16) -> bool {
        if self.rich_edit_object.is_some() && id >= 10 && !matches!(id,42..=45|58..=62) {
            return false;
        }
        if self.pptx_source.is_some() && matches!(id, 24 | 28 | 29) {
            return false;
        }
        if id == 10 && self.pptx_source.as_ref().is_some_and(|s| !s.can_add()) {
            return false;
        }
        if id == 14
            && self.pptx_source.as_ref().is_some_and(|s| {
                !s.can_duplicate(self.deck.slides()[self.deck.current()].source_identity())
            })
        {
            return false;
        }
        if id == 11 && self.pptx_source.is_some() {
            let current = self.deck.slides()[self.deck.current()].id;
            if self
                .deck
                .slides()
                .iter()
                .any(|s| s.id != current && s.source_identity() == current)
            {
                return false;
            }
        }
        if self.pptx_source.is_some()
            && matches!(id, 27 | 30 | 31)
            && self.selection().iter().any(|object| {
                self.pptx_source.as_ref().unwrap().is_original_object(
                    self.deck.slides()[self.deck.current()].source_identity(),
                    *object,
                )
            })
        {
            return false;
        }
        if self.odp_source.is_some() && matches!(id, 24 | 28 | 29) {
            return false;
        }
        if self.odp_source.is_some()
            && matches!(id, 27 | 30 | 31)
            && self.selection().iter().any(|object| {
                self.odp_source.as_ref().unwrap().is_original_object(
                    self.deck.slides()[self.deck.current()].source_identity(),
                    *object,
                )
            })
        {
            return false;
        }
        if id == 11
            && self.odp_source.as_ref().is_some_and(|s| {
                !s.can_delete_slide(self.deck.slides()[self.deck.current()].source_identity())
            })
        {
            return false;
        }
        if id == 11 && self.pptx_source.as_ref().is_some_and(|s| !s.can_delete()) {
            return false;
        }
        let i = self.deck.current();
        let n = self.deck.slides().len();
        if self.rich_edit_object.is_none()
            && matches!(id, 25 | 42..=45)
            && (self.pptx_source.is_some() || self.odp_source.is_some())
            && self.selected_object.is_some_and(|id| {
                self.deck.slides()[i]
                    .objects
                    .iter()
                    .any(|o| o.id == id && !o.paragraphs.is_empty())
            })
        {
            return false;
        }
        match id {
            66 => {
                self.pptx_source
                    .as_ref()
                    .is_none_or(|s| s.can_edit_timing(self.deck.slides()[i].source_identity()))
                    && self
                        .odp_source
                        .as_ref()
                        .is_none_or(|s| s.can_edit_timing(self.deck.slides()[i].source_identity()))
            }
            58 => {
                self.rich_edit_object.is_some()
                    || self.selected_object.is_some_and(|id| {
                        self.deck.slides()[i].objects.iter().any(|o| {
                            o.id == id
                                && !matches!(
                                    o.kind,
                                    ppt42_core::ObjectKind::Picture | ppt42_core::ObjectKind::Table
                                )
                                && o.rotation_degrees.rem_euclid(360.0) == 0.0
                                && rich::document(o).is_ok_and(|d| d.rich_edit_projection().is_ok())
                        }) && self.pptx_source.as_ref().is_none_or(|s| {
                            s.can_edit_rich(self.deck.slides()[i].source_identity(), id)
                        }) && self.odp_source.as_ref().is_none_or(|s| {
                            s.can_edit_rich(self.deck.slides()[i].source_identity(), id)
                        })
                    })
            }
            59..=62 => self.rich_edit_object.is_some(),
            65 => {
                !self.selection().is_empty()
                    && self.pptx_source.as_ref().is_none_or(|s| {
                        self.selection()
                            .iter()
                            .all(|id| s.can_rotate(self.deck.slides()[i].source_identity(), *id))
                    })
            }
            46 => !self.deck.slides()[i].objects.is_empty(),
            47..=52 => self.selection().len() >= 2,
            53 | 54 => self.selection().len() >= 3,
            55 => {
                !self.selection().is_empty()
                    && ((self.pptx_source.is_none() && self.odp_source.is_none())
                        || !self.deck.slides()[i]
                            .objects
                            .iter()
                            .any(|o| self.selection().contains(&o.id) && o.group_id.is_some()))
            }
            63 => {
                self.pptx_source.is_none()
                    && self.odp_source.is_none()
                    && self.selection().len() >= 2
                    && self.deck.slides()[i]
                        .objects
                        .iter()
                        .filter(|o| self.selection().contains(&o.id))
                        .all(|o| o.group_id.is_none())
            }
            64 => {
                self.pptx_source.is_none()
                    && self.odp_source.is_none()
                    && self.deck.slides()[i]
                        .objects
                        .iter()
                        .any(|o| self.selection().contains(&o.id) && o.group_id.is_some())
            }
            56 => !self.selection().is_empty(),
            42..=45 => {
                self.selected_object.is_some_and(|id| {
                    self.deck.slides()[i].objects.iter().any(|o| {
                        o.id == id
                            && !o.text.is_empty()
                            && !matches!(
                                o.kind,
                                ppt42_core::ObjectKind::Picture | ppt42_core::ObjectKind::Table
                            )
                    })
                }) && self.pptx_source.as_ref().is_none_or(|s| {
                    s.can_edit_text(
                        self.deck.slides()[i].source_identity(),
                        self.selected_object.unwrap(),
                    )
                }) && self.odp_source.as_ref().is_none_or(|s| {
                    s.can_edit_text(
                        self.deck.slides()[i].source_identity(),
                        self.selected_object.unwrap(),
                    )
                })
            }
            40 => self
                .odp_source
                .as_ref()
                .is_none_or(|s| s.size_points().is_some()),
            10 => self.deck.can_add(),
            11 => n > 1,
            12 => i > 0,
            13 => i + 1 < n,
            14 => self.deck.can_duplicate(),
            25 if self.pptx_source.is_some() => self.selected_object.is_some_and(|id| {
                self.pptx_source
                    .as_ref()
                    .unwrap()
                    .can_edit_text(self.deck.slides()[i].source_identity(), id)
            }),
            25 if self.odp_source.is_some() => self.selected_object.is_some_and(|id| {
                self.odp_source
                    .as_ref()
                    .unwrap()
                    .can_edit_text(self.deck.slides()[i].source_identity(), id)
            }),
            25..=29 | 31 => self
                .selected_object
                .is_some_and(|id| self.deck.slides()[i].objects.iter().any(|o| o.id == id)),
            30 => self.selected_object.is_some_and(|id| {
                self.deck.slides()[i].objects.iter().any(|o| {
                    o.id == id
                        && matches!(
                            o.kind,
                            ppt42_core::ObjectKind::Rectangle | ppt42_core::ObjectKind::Text
                        )
                })
            }),
            34 => self.selected_object.is_some_and(|id| {
                self.deck.slides()[i]
                    .objects
                    .iter()
                    .any(|o| o.id == id && matches!(o.kind, ppt42_core::ObjectKind::Table))
            }),
            _ => true,
        }
    }
    fn action(&mut self, id: u16) -> Result<(), String> {
        if id >= 10 && !self.enabled(id) {
            return Err("Command unavailable".into());
        }
        let i = self.deck.current();
        match id {
            4 => {
                if self.rich_edit_object.is_some() {
                    self.finish_rich(true)?;
                }
                self.deck.undo();
                Ok(())
            }
            5 => {
                if self.rich_edit_object.is_some() {
                    self.finish_rich(true)?;
                }
                self.deck.redo();
                Ok(())
            }
            10 => self.deck.add(),
            11 => self.deck.delete(),
            12 => self.deck.move_slide(i, i - 1),
            13 => self.deck.move_slide(i, i + 1),
            14 => self.deck.duplicate(),
            46 => {
                self.selected_objects =
                    self.deck.slides()[i].objects.iter().map(|o| o.id).collect();
                self.selected_object = self.selected_objects.last().copied();
                Ok(())
            }
            47..=54 => {
                let mode = match id {
                    47 => ppt42_core::ObjectAlignment::Left,
                    48 => ppt42_core::ObjectAlignment::Center,
                    49 => ppt42_core::ObjectAlignment::Right,
                    50 => ppt42_core::ObjectAlignment::Top,
                    51 => ppt42_core::ObjectAlignment::Middle,
                    52 => ppt42_core::ObjectAlignment::Bottom,
                    53 => ppt42_core::ObjectAlignment::DistributeHorizontal,
                    _ => ppt42_core::ObjectAlignment::DistributeVertical,
                };
                self.deck.align_objects(&self.selection(), mode)
            }
            55 => {
                let ids = self.deck.duplicate_objects(&self.selection())?;
                self.selected_object = ids.last().copied();
                self.selected_objects = ids;
                Ok(())
            }
            58 => {
                if self.rich_edit_object.is_some() {
                    self.finish_rich(true)?;
                } else {
                    self.rich_edit_object = self.selected_object;
                    self.rich_draft = Some(self.rich_object()?);
                    self.rich_selection = (0, 0);
                }
                Ok(())
            }
            59..=61 => self.format_rich_selection(id),
            62 => self.format_rich_paragraph(id),
            63 => self.deck.group_objects(&self.selection()),
            64 => self.deck.ungroup_objects(&self.selection()),
            41 | 67 => {
                if id == 67 {
                    self.deck.select(0)?;
                }
                self.presenting = true;
                self.show = Some(Show {
                    since: std::time::Instant::now(),
                    previous: None,
                    paused: None,
                    transition_ms: 0,
                });
                self.selected_object = None;
                self.selected_objects.clear();
                self.rubber_band = None;
                self.drag = None;
                Ok(())
            }
            43..=45 => {
                if self.rich_edit_object.is_some() {
                    return self.format_rich_paragraph(id);
                }
                let object_id = self.selected_object.ok_or("Select an object")?;
                let mut object = self.deck.slides()[i]
                    .objects
                    .iter()
                    .find(|o| o.id == object_id)
                    .cloned()
                    .ok_or("Object no longer exists")?;
                object.text_style.align = match id {
                    43 => ppt42_core::TextAlign::Left,
                    44 => ppt42_core::TextAlign::Center,
                    _ => ppt42_core::TextAlign::Right,
                };
                for paragraph in &mut object.paragraphs {
                    paragraph.align = object.text_style.align;
                }
                self.deck.update_object(object)
            }
            27 => {
                let ids = self.selection();
                self.deck.delete_objects(&ids)?;
                self.selected_object = None;
                self.selected_objects.clear();
                Ok(())
            }
            28 | 29 => self
                .deck
                .arrange_object(self.selected_object.ok_or("Select an object")?, id == 28),
            20..=23 => {
                let kind = match id {
                    20 => ppt42_core::ObjectKind::Rectangle,
                    21 => ppt42_core::ObjectKind::Ellipse,
                    22 => ppt42_core::ObjectKind::Line,
                    _ => ppt42_core::ObjectKind::Text,
                };
                self.selected_object = Some(self.deck.insert_object(
                    kind,
                    if id == 23 {
                        "Text".into()
                    } else {
                        String::new()
                    },
                )?);
                self.selected_objects.clear();
                Ok(())
            }
            _ => Ok(()),
        }
    }
}
fn main() {
    win42::run(Box::new(Ppt::new(Presentation::new())))
}
#[cfg(test)]
mod tests {
    use super::*;
    use win42::App;

    #[test]
    fn paused_manual_navigation_waits_for_explicit_resume() {
        use std::time::{Duration, Instant};
        let mut app = Ppt::new(Presentation::new());
        for index in 0..3 {
            if index > 0 {
                app.deck.add().unwrap();
            }
            app.deck
                .set_timing(ppt42_core::SlideTiming {
                    advance_ms: Some(200),
                    transition: ppt42_core::Transition::PushLeft,
                    duration_ms: 500,
                })
                .unwrap();
        }
        app.deck.select(0).unwrap();
        let start = Instant::now();
        app.presenting = true;
        app.show = Some(Show {
            since: start,
            previous: None,
            paused: None,
            transition_ms: 0,
        });
        let key = |code| win42::CanvasEvent::Key { code, modifiers: 0 };
        app.show_event(key(80), start + Duration::from_millis(50))
            .unwrap();
        app.show_event(key(39), start + Duration::from_millis(100))
            .unwrap();
        assert_eq!(app.deck.current(), 1);
        assert!(app.show.as_ref().unwrap().paused.is_some());
        assert!(app.show.as_ref().unwrap().previous.is_none());
        app.show_event(key(37), start + Duration::from_millis(200))
            .unwrap();
        assert_eq!(app.deck.current(), 0);
        app.show_event(
            win42::CanvasEvent::PointerUp(win42::Point { x: 0.0, y: 0.0 }),
            start + Duration::from_millis(300),
        )
        .unwrap();
        assert_eq!(app.deck.current(), 1);
        assert!(!app.show_tick(start + Duration::from_secs(20)));
        assert_eq!(app.deck.current(), 1);
        app.show_event(key(80), start + Duration::from_secs(20))
            .unwrap();
        assert!(!app.show_tick(start + Duration::from_secs(20) + Duration::from_millis(199)));
        assert!(app.show_tick(start + Duration::from_secs(20) + Duration::from_millis(200)));
        assert_eq!(app.deck.current(), 2);
        assert!(app.show.as_ref().unwrap().paused.is_none());
    }

    #[test]
    fn slideshow_advance_push_pause_and_cancel() {
        use std::time::{Duration, Instant};
        let mut app = Ppt::new(Presentation::new());
        app.deck
            .set_timing(ppt42_core::SlideTiming {
                advance_ms: Some(200),
                ..Default::default()
            })
            .unwrap();
        app.deck.add().unwrap();
        app.deck
            .set_timing(ppt42_core::SlideTiming {
                transition: ppt42_core::Transition::PushLeft,
                duration_ms: 500,
                advance_ms: Some(200),
            })
            .unwrap();
        app.deck.select(0).unwrap();
        let start = Instant::now();
        app.presenting = true;
        app.show = Some(Show {
            since: start,
            previous: None,
            paused: None,
            transition_ms: 0,
        });
        assert!(!app.show_tick(start + Duration::from_millis(199)));
        assert!(app.show_tick(start + Duration::from_millis(200)));
        assert_eq!(app.deck.current(), 1);
        assert_eq!(app.show.as_ref().unwrap().previous, Some(0));
        assert!(app.show_tick(start + Duration::from_millis(400)));
        app.show.as_mut().unwrap().paused = Some(start + Duration::from_millis(400));
        assert!(!app.show_tick(start + Duration::from_secs(5)));
        app.canvas_event(win42::CanvasEvent::Key {
            code: 27,
            modifiers: 0,
        })
        .unwrap();
        assert!(!app.show_tick(start + Duration::from_secs(6)));
        assert!(app.show.is_none());
        app.deck.select(0).unwrap();
        app.deck.select(1).unwrap();
        app.deck
            .set_timing(ppt42_core::SlideTiming {
                transition: ppt42_core::Transition::Fade,
                ..Default::default()
            })
            .unwrap();
        app.deck.select(0).unwrap();
        app.presenting = true;
        app.show_move(1, Instant::now()).unwrap();
        let scene = app.canvas_scene().unwrap();
        assert!(scene.items.iter().any(
            |item| matches!(item, win42::SceneItem::OpacityLayer { items, .. } if !items.is_empty())
        ));
    }

    #[test]
    fn rotation_hit_resize_and_group_undo() {
        let mut app = Ppt::new(Presentation::new());
        let a = app
            .deck
            .insert_object(ppt42_core::ObjectKind::Rectangle, String::new())
            .unwrap();
        let b = app
            .deck
            .insert_object(ppt42_core::ObjectKind::Ellipse, String::new())
            .unwrap();
        let mut second = app.deck.slides()[0]
            .objects
            .iter()
            .find(|o| o.id == b)
            .unwrap()
            .clone();
        second.x += 200.0;
        app.deck.update_object(second).unwrap();
        app.selected_object = Some(a);
        app.selected_objects = vec![a, b];
        let original = app.deck.slides()[0].objects.clone();
        app.input_action(65, &["90".into()]).unwrap();
        let rotated = app.deck.slides()[0].objects.clone();
        assert_eq!(rotated[0].rotation_degrees, 90.0);
        assert_eq!(rotated[1].rotation_degrees, 90.0);
        assert_ne!(rotated[0].y, original[0].y);
        assert!(app.deck.undo());
        assert_eq!(app.deck.slides()[0].objects, original);
        assert!(app.deck.redo());
        let mut object = rotated[0].clone();
        let corner = object_point(&object, object.x, object.y);
        let end = object_point(&object, object.x + object.width, object.y + object.height);
        resize_object(
            &mut object,
            end,
            win42::Point {
                x: end.x - 10.0,
                y: end.y + 20.0,
            },
        );
        let after = object_point(&object, object.x, object.y);
        assert!((corner.x - after.x).abs() < 1e-9 && (corner.y - after.y).abs() < 1e-9);
        let local = object_local_point(
            &object,
            object_point(&object, object.x + 5.0, object.y + 5.0),
        );
        assert!((local.x - object.x - 5.0).abs() < 1e-9);
    }

    #[test]
    fn rich_draft_save_captures_revision_and_cancel_is_lossless() {
        let mut app = Ppt::new(Presentation::new());
        let id = app
            .deck
            .insert_object(ppt42_core::ObjectKind::Text, "hello".into())
            .unwrap();
        let (_, baseline) = app.deck.save_snapshot().unwrap();
        app.deck.mark_saved_snapshot(baseline);
        app.selected_object = Some(id);
        app.action(58).unwrap();
        app.set_rich_selection((0, 5));
        app.action(59).unwrap();
        assert!(app.dirty());
        assert!(app.can_undo());
        let (revision, bytes) = app.save_snapshot("json").unwrap().unwrap();
        assert!(app.rich_draft.is_none());
        assert!(std::str::from_utf8(&bytes).unwrap().contains("paragraphs"));
        let mut later = app.deck.slides()[0].objects[0].clone();
        later.x += 5.0;
        app.deck.update_object(later).unwrap();
        app.saved_snapshot(revision);
        assert!(app.dirty());
        app.selected_object = Some(id);
        app.action(58).unwrap();
        app.set_rich_selection((0, 5));
        app.action(60).unwrap();
        app.finish_inline_rich(false).unwrap();
        assert!(
            !app.deck.slides()[0].objects[0].paragraphs[0].runs[0]
                .style
                .italic
        );
    }
    #[test]
    fn rich_selection_edit_is_single_presentation_history_and_roundtrips_pptx() {
        let mut a = Ppt::new(Presentation::new());
        let id = a
            .deck
            .insert_object(ppt42_core::ObjectKind::Text, "hello world".into())
            .unwrap();
        a.selected_object = Some(id);
        a.action(58).unwrap();
        a.set_rich_selection((0, 5));
        a.action(59).unwrap();
        let projection = a.rich_text().unwrap();
        a.edit_rich_text(&projection, (0, 5)).unwrap();
        a.action(62).unwrap();
        assert!(a.deck.slides()[0].objects[0].paragraphs.is_empty());
        a.action(58).unwrap();
        let object = &a.deck.slides()[0].objects[0];
        assert_eq!(object.paragraphs[0].runs.len(), 2);
        assert!(object.paragraphs[0].runs[0].style.bold);
        assert!(object.paragraphs[0].bullet);
        assert!(!object.paragraphs[0].runs[1].style.bold);
        let bytes = ppt42_core::pptx::export_new(&a.deck).unwrap();
        let (opened, _) = ppt42_core::pptx::import(&bytes).unwrap();
        let runs = &opened.slides()[0].objects[0].paragraphs[0].runs;
        assert!(runs[0].style.bold);
        assert!(!runs[1].style.bold);
        assert!(opened.slides()[0].objects[0].paragraphs[0].bullet);
        let (source_deck, source) = ppt42_core::pptx::import(&bytes).unwrap();
        let mut source_app = Ppt::new(source_deck);
        source_app.pptx_source = Some(source);
        source_app.selected_object = Some(source_app.deck.slides()[0].objects[0].id);
        assert!(source_app.enabled(58));
        source_app.action(58).unwrap();
        source_app.set_rich_selection((6, 11));
        source_app.action(60).unwrap();
        source_app.action(58).unwrap();
        let bytes =
            ppt42_core::pptx::export(&source_app.deck, source_app.pptx_source.as_ref().unwrap())
                .unwrap();
        let (reopened, _) = ppt42_core::pptx::import(&bytes).unwrap();
        assert!(reopened.slides()[0].objects[0].paragraphs[0]
            .runs
            .iter()
            .any(|r| r.text == "world" && r.style.italic));
        let odp = ppt42_core::odp::export(&a.deck).unwrap();
        let (deck, source) = ppt42_core::odp::import_with_source(&odp).unwrap();
        let mut source_app = Ppt::new(deck);
        source_app.odp_source = Some(source);
        source_app.selected_object = Some(source_app.deck.slides()[0].objects[0].id);
        assert!(source_app.enabled(58));
        source_app.action(58).unwrap();
        source_app.set_rich_selection((6, 11));
        source_app.action(60).unwrap();
        source_app.action(58).unwrap();
        let odp = ppt42_core::odp::export_overlay(
            &source_app.deck,
            source_app.odp_source.as_ref().unwrap(),
        )
        .unwrap();
        let (opened, _) = ppt42_core::odp::import_with_source(&odp).unwrap();
        assert!(opened.slides()[0].objects[0].paragraphs[0]
            .runs
            .iter()
            .any(|r| r.text == "world" && r.style.italic));
        a.action(4).unwrap();
        assert!(a.deck.slides()[0].objects[0].paragraphs.is_empty());
        a.action(5).unwrap();
        assert!(
            a.deck.slides()[0].objects[0].paragraphs[0].runs[0]
                .style
                .bold
        );
        a.action(58).unwrap();
        a.set_rich_selection((0, 5));
        a.action(60).unwrap();
        a.finish_inline_rich(false).unwrap();
        assert!(
            !a.deck.slides()[0].objects[0].paragraphs[0].runs[0]
                .style
                .italic
        );
    }
    #[test]
    fn multiselect_drag_resize_and_history() {
        let mut a = Ppt::new(Presentation::new());
        let one = a
            .deck
            .insert_object(ppt42_core::ObjectKind::Rectangle, String::new())
            .unwrap();
        let two = a
            .deck
            .insert_object(ppt42_core::ObjectKind::Rectangle, String::new())
            .unwrap();
        let mut second = a.deck.slides()[0].objects[1].clone();
        second.x = 350.0;
        a.deck.update_object(second).unwrap();
        a.canvas_event(win42::CanvasEvent::PointerDown(win42::Point {
            x: 120.0,
            y: 140.0,
        }))
        .unwrap();
        a.canvas_event(win42::CanvasEvent::PointerUp(win42::Point {
            x: 120.0,
            y: 140.0,
        }))
        .unwrap();
        a.canvas_event(win42::CanvasEvent::PointerDownModified {
            point: win42::Point { x: 370.0, y: 140.0 },
            modifiers: 1,
        })
        .unwrap();
        a.canvas_event(win42::CanvasEvent::PointerUp(win42::Point {
            x: 370.0,
            y: 140.0,
        }))
        .unwrap();
        assert_eq!(a.selection().len(), 2);
        a.canvas_event(win42::CanvasEvent::PointerDown(win42::Point {
            x: 120.0,
            y: 140.0,
        }))
        .unwrap();
        a.canvas_event(win42::CanvasEvent::PointerUp(win42::Point {
            x: 140.0,
            y: 150.0,
        }))
        .unwrap();
        assert_eq!(a.deck.slides()[0].objects[0].x, 120.0);
        assert_eq!(a.deck.slides()[0].objects[1].x, 370.0);
        a.action(4).unwrap();
        assert_eq!(a.deck.slides()[0].objects[0].x, 100.0);
        assert_eq!(a.deck.slides()[0].objects[1].x, 350.0);
        a.action(5).unwrap();
        a.action(47).unwrap();
        assert_eq!(
            a.deck.slides()[0].objects[0].x,
            a.deck.slides()[0].objects[1].x
        );
        a.action(4).unwrap();
        assert_eq!(a.deck.slides()[0].objects[1].x, 370.0);
        a.selected_objects = vec![one];
        a.selected_object = Some(one);
        let before = a.deck.slides()[0].objects[0].width;
        let o = a.deck.slides()[0].objects[0].clone();
        a.canvas_event(win42::CanvasEvent::PointerDown(win42::Point {
            x: o.x + o.width,
            y: o.y + o.height,
        }))
        .unwrap();
        a.canvas_event(win42::CanvasEvent::PointerUp(win42::Point {
            x: o.x + o.width + 25.0,
            y: o.y + o.height + 10.0,
        }))
        .unwrap();
        assert_eq!(a.deck.slides()[0].objects[0].width, before + 25.0);
        a.action(4).unwrap();
        assert_eq!(a.deck.slides()[0].objects[0].width, before);
        assert_ne!(one, two);
    }
    #[test]
    fn duplicate_menu_dispatch_tracks_history() {
        let mut app = Ppt::new(Presentation::new());
        app.edit("Title\r\nBody".into()).unwrap();
        assert!(app.enabled(14));
        app.action(14).unwrap();
        assert_eq!(app.selected(), 1);
        assert_eq!(app.text(), "Title\r\nBody");
        assert_ne!(app.deck.slides()[0].id, app.deck.slides()[1].id);
        app.action(4).unwrap();
        assert_eq!(app.selected(), 0);
        app.action(5).unwrap();
        assert_eq!(app.selected(), 1);
    }
    #[test]
    fn slide_editor_preserves_crlf() {
        let mut p = Presentation::new();
        p.edit_raw("title".into(), "a\r\nb".into()).unwrap();
        let mut a = Ppt::new(p);
        assert_eq!(a.text(), "title\r\na\r\nb");
        a.edit("title\r\na".into()).unwrap();
        a.select(0);
        a.edit("title\r\na\r\nc".into()).unwrap();
        assert_eq!(a.deck.slides()[0].body, "a\r\nc");
    }
}
#[cfg(test)]
mod regression {
    use super::*;
    use win42::App;
    #[test]
    fn policy_survives_reorder_undo_redo_and_context_switch() {
        let mut d = Presentation::new();
        d.edit_raw("one".into(), "a\r\nb".into()).unwrap();
        d.add().unwrap();
        d.edit("two".into(), "x\ny".into()).unwrap();
        let path = std::env::temp_dir().join(format!("ppt42-policy-{}.json", std::process::id()));
        let _ = std::fs::remove_file(&path);
        d.save_as(&path).unwrap();
        let mut a = Ppt::new(Presentation::open(&path).unwrap());
        a.select(0);
        a.edit("one\r\na".into()).unwrap();
        a.action(13).unwrap();
        a.action(4).unwrap();
        a.action(5).unwrap();
        a.select(0);
        a.select(1);
        a.edit("one\r\na\r\nc".into()).unwrap();
        assert_eq!(a.deck.slides()[1].body, "a\r\nc");
        assert_eq!(a.deck.slides()[0].body, "x\ny");
        let out =
            std::env::temp_dir().join(format!("ppt42-policy-out-{}.json", std::process::id()));
        let _ = std::fs::remove_file(&out);
        a.save(&out).unwrap();
        let re = Presentation::open(&out).unwrap();
        assert_eq!(re.slides()[1].body, "a\r\nc");
        assert_eq!(re.slides()[0].body, "x\ny");
        std::fs::remove_file(path).unwrap();
        std::fs::remove_file(out).unwrap();
    }
    #[test]
    fn rejected_and_noop_edit_do_not_desynchronize_policy_history() {
        let mut d = Presentation::new();
        d.edit_raw("one".into(), "a\r\nb".into()).unwrap();
        let path = std::env::temp_dir().join(format!("ppt42-branch-{}.json", std::process::id()));
        let _ = std::fs::remove_file(&path);
        d.save_as(&path).unwrap();
        let mut a = Ppt::new(Presentation::open(&path).unwrap());
        a.edit("one\r\na".into()).unwrap();
        assert!(a.edit(format!("one\r\n{}", "x".repeat(32769))).is_err());
        a.edit("one\r\na".into()).unwrap();
        a.action(4).unwrap();
        assert_eq!(a.deck.slides()[0].body, "a\r\nb");
        a.edit("one\r\nbranch".into()).unwrap();
        assert!(!a.can_redo());
        a.edit("one\r\nbranch\r\nnext".into()).unwrap();
        assert_eq!(a.deck.slides()[0].body, "branch\r\nnext");
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn bounded_policy_history_stays_aligned() {
        let mut d = Presentation::new();
        d.edit_raw("one".into(), "a\r\nb".into()).unwrap();
        let p = std::env::temp_dir().join(format!("ppt42-limit-{}.json", std::process::id()));
        let _ = std::fs::remove_file(&p);
        d.save_as(&p).unwrap();
        let mut a = Ppt::new(Presentation::open(&p).unwrap());
        for i in 0..15 {
            a.edit(format!("one\r\nvalue{i}")).unwrap();
        }
        for _ in 0..10 {
            a.action(4).unwrap();
        }
        assert!(!a.can_undo());
        a.edit("one\r\nlast\r\nline".into()).unwrap();
        assert_eq!(a.deck.slides()[0].body, "last\r\nline");
        assert!(!a.can_redo());
        std::fs::remove_file(p).unwrap();
    }
    #[test]
    fn boundary_commands_are_refused() {
        let mut a = Ppt::new(Presentation::new());
        for id in [11, 12, 13] {
            assert!(!a.enabled(id));
            assert!(a.action(id).is_err());
        }
        a.action(10).unwrap();
        assert!(a.enabled(12));
        assert!(!a.enabled(13));
        a.action(12).unwrap();
        assert!(!a.enabled(12));
        assert!(a.enabled(13));
    }
}
#[cfg(test)]
mod identity_regression {
    use super::*;
    use win42::App;
    #[test]
    fn identical_visible_slides_keep_identity_and_newlines_through_reorder_history_and_save() {
        let mut deck = Presentation::new();
        deck.edit_raw("same".into(), "a\r\nb".into()).unwrap();
        deck.add().unwrap();
        deck.edit_raw("same".into(), "a\nb".into()).unwrap();
        let crlf_id = deck.slides()[0].id;
        let lf_id = deck.slides()[1].id;
        let mut app = Ppt::new(deck);
        app.select(0);
        app.edit("same\r\none".into()).unwrap();
        app.select(1);
        app.edit("same\r\none".into()).unwrap();
        assert_eq!(app.deck.slides()[0].body, app.deck.slides()[1].body);
        app.select(0);
        app.action(13).unwrap();
        assert_eq!(app.deck.slides()[1].id, crlf_id);
        app.action(4).unwrap();
        assert_eq!(app.deck.slides()[0].id, crlf_id);
        app.action(5).unwrap();
        assert_eq!(app.deck.slides()[0].id, lf_id);
        let path = std::env::temp_dir().join(format!("ppt42-identity-{}.json", std::process::id()));
        let _ = std::fs::remove_file(&path);
        app.save(&path).unwrap();
        let mut reopened = Ppt::new(Presentation::open(&path).unwrap());
        reopened.select(0);
        reopened.edit("same\r\none\r\nmore".into()).unwrap();
        reopened.select(1);
        reopened.edit("same\r\none\r\nmore".into()).unwrap();
        assert_eq!(
            (
                reopened.deck.slides()[0].id,
                reopened.deck.slides()[0].body.as_str()
            ),
            (lf_id, "one\nmore")
        );
        assert_eq!(
            (
                reopened.deck.slides()[1].id,
                reopened.deck.slides()[1].body.as_str()
            ),
            (crlf_id, "one\r\nmore")
        );
        reopened.action(4).unwrap();
        reopened.action(5).unwrap();
        reopened.action(4).unwrap();
        reopened.edit("same\r\nbranch\r\nend".into()).unwrap();
        assert!(!reopened.can_redo());
        assert_eq!(reopened.deck.slides()[1].body, "branch\r\nend");
        std::fs::remove_file(path).unwrap();
    }
}
