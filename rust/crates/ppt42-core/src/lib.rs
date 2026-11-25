//! Small native slide-deck model; this is not PPTX compatibility.
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    fs,
    io::Read,
    path::Path,
};
use suite42_core::{write_new, SnapshotHistory, TextValue};
pub mod odp;
pub mod pptx;
const MAX_SLIDES: usize = 200;
const MAX_TEXT: usize = 32_768;
const MAX_FILE: u64 = 512 * 1024;
const MAX_MEDIA_FILE: u64 = 64 * 1024 * 1024;
const HISTORY_LIMIT: usize = 10;
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Slide {
    pub id: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    origin: Option<u64>,
    #[serde(skip)]
    pub title: String,
    #[serde(skip)]
    pub body: String,
    title_text: TextValue,
    body_text: TextValue,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub objects: Vec<SlideObject>,
    #[serde(default = "first_object_id")]
    next_object_id: u64,
    #[serde(default)]
    pub timing: SlideTiming,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Transition {
    #[default]
    Cut,
    PushLeft,
    Fade,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SlideTiming {
    pub advance_ms: Option<u32>,
    pub transition: Transition,
    pub duration_ms: u32,
}
impl Default for SlideTiming {
    fn default() -> Self {
        Self {
            advance_ms: None,
            transition: Transition::Cut,
            duration_ms: 500,
        }
    }
}
fn first_object_id() -> u64 {
    1
}
/// Editable objects use slide-local stable identities and slide coordinates.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SlideObject {
    pub id: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group_id: Option<u64>,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    #[serde(default)]
    pub rotation_degrees: f64,
    pub kind: ObjectKind,
    pub fill: [u8; 3],
    pub stroke: [u8; 3],
    #[serde(default = "enabled", skip_serializing_if = "is_enabled")]
    pub fill_visible: bool,
    #[serde(default = "enabled", skip_serializing_if = "is_enabled")]
    pub stroke_visible: bool,
    pub text: String,
    #[serde(default, skip_serializing_if = "TextStyle::is_default")]
    pub text_style: TextStyle,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub paragraphs: Vec<RichParagraph>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub media: Vec<u8>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cells: Vec<Vec<String>>,
}
fn enabled() -> bool {
    true
}
fn is_enabled(value: &bool) -> bool {
    *value
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TextStyle {
    pub family: String,
    pub points: f64,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub color: [u8; 3],
    pub align: TextAlign,
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum TextAlign {
    Left,
    Center,
    Right,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RichRun {
    pub text: String,
    pub style: TextStyle,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RichParagraph {
    pub runs: Vec<RichRun>,
    pub align: TextAlign,
    pub bullet: bool,
}
impl Default for TextStyle {
    fn default() -> Self {
        Self {
            family: "Arial".into(),
            points: 16.0,
            bold: false,
            italic: false,
            underline: false,
            color: [0, 0, 0],
            align: TextAlign::Left,
        }
    }
}
impl TextStyle {
    fn is_default(&self) -> bool {
        self == &Self::default()
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum ObjectKind {
    Rectangle,
    Ellipse,
    Line,
    Text,
    Picture,
    Table,
}
#[derive(Clone, Copy, Debug)]
pub enum ObjectAlignment {
    Left,
    Center,
    Right,
    Top,
    Middle,
    Bottom,
    DistributeHorizontal,
    DistributeVertical,
}
impl Slide {
    pub fn source_identity(&self) -> u64 {
        self.origin.unwrap_or(self.id)
    }
    fn new(id: u64, title: String, body: String) -> Self {
        Self {
            id,
            origin: None,
            title_text: TextValue::from_raw(title.clone()),
            body_text: TextValue::from_raw(body.clone()),
            title,
            body,
            objects: Vec::new(),
            next_object_id: 1,
            timing: SlideTiming::default(),
        }
    }
    fn edited(&self, title: &str, body: &str) -> Self {
        let title_text = self.title_text.with_edited(title);
        let body_text = self.body_text.with_edited(body);
        Self {
            id: self.id,
            origin: self.origin,
            title: title_text.raw().to_owned(),
            body: body_text.raw().to_owned(),
            title_text,
            body_text,
            objects: self.objects.clone(),
            next_object_id: self.next_object_id,
            timing: self.timing,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Deck {
    version: u32,
    next_id: u64,
    pub slides: Vec<Slide>,
}
impl Deck {
    fn valid(&self) -> bool {
        let mut ids = HashSet::new();
        self.version == 2
            && self.next_id > 0
            && !self.slides.is_empty()
            && self.slides.len() <= MAX_SLIDES
            && self.slides.iter().all(|s| {
                s.id > 0
                    && s.id < self.next_id
                    && ids.insert(s.id)
                    && s.title.len() <= MAX_TEXT
                    && s.body.len() <= MAX_TEXT
                    && s.title_text.policy_consistent()
                    && s.body_text.policy_consistent()
                    && s.title_text.raw() == s.title
                    && s.body_text.raw() == s.body
                    && s.timing
                        .advance_ms
                        .is_none_or(|ms| (100..=86_400_000).contains(&ms))
                    && (100..=10_000).contains(&s.timing.duration_ms)
                    && {
                        let mut object_ids = HashSet::new();
                        s.next_object_id > 0
                            && s.objects.iter().all(|o| {
                                o.id > 0
                                    && o.id < s.next_object_id
                                    && object_ids.insert(o.id)
                                    && o.x.is_finite()
                                    && o.y.is_finite()
                                    && o.width.is_finite()
                                    && o.height.is_finite()
                                    && o.rotation_degrees.is_finite()
                                    && o.width > 0.0
                                    && o.height > 0.0
                                    && o.text.len() <= MAX_TEXT
                                    && o.text_style.points.is_finite()
                                    && (1.0..=512.0).contains(&o.text_style.points)
                                    && o.text_style.family.len() <= 256
                                    && (o.paragraphs.is_empty()
                                        || (o.paragraphs.len() <= 2048
                                            && o.paragraphs.iter().all(|p| {
                                                p.runs.len() <= 4096
                                                    && p.runs.iter().all(|r| {
                                                        r.text.len() <= MAX_TEXT
                                                            && r.style.points.is_finite()
                                                            && (1.0..=512.0)
                                                                .contains(&r.style.points)
                                                            && r.style.family.len() <= 256
                                                    })
                                            })
                                            && o.paragraphs
                                                .iter()
                                                .map(|p| {
                                                    p.runs
                                                        .iter()
                                                        .map(|r| r.text.as_str())
                                                        .collect::<String>()
                                                })
                                                .collect::<Vec<_>>()
                                                .join("\n")
                                                == o.text))
                                    && o.media.len() <= 16 * 1024 * 1024
                                    && o.cells.len() <= 100
                                    && o.cells.iter().all(|r| {
                                        r.len() <= 100 && r.iter().all(|c| c.len() <= MAX_TEXT)
                                    })
                            })
                    }
            })
            && serde_json::to_vec(self).is_ok_and(|bytes| bytes.len() as u64 <= self.file_limit())
    }
    fn file_limit(&self) -> u64 {
        if self
            .slides
            .iter()
            .flat_map(|s| &s.objects)
            .any(|o| !o.media.is_empty())
        {
            MAX_MEDIA_FILE
        } else {
            MAX_FILE
        }
    }
}
#[derive(Clone)]
struct State {
    deck: Deck,
    selected: u64,
}
// Selection is captured for undo/redo, but does not make a saved deck dirty.
impl PartialEq for State {
    fn eq(&self, other: &Self) -> bool {
        self.deck == other.deck
    }
}
pub struct SaveState(State);

pub struct Presentation {
    history: SnapshotHistory<State>,
}
impl Default for Presentation {
    fn default() -> Self {
        Self::new()
    }
}
impl Presentation {
    pub fn new() -> Self {
        Self {
            history: SnapshotHistory::new(
                State {
                    deck: Deck {
                        version: 2,
                        next_id: 2,
                        slides: vec![Slide::new(1, String::new(), String::new())],
                    },
                    selected: 1,
                },
                HISTORY_LIMIT,
            ),
        }
    }
    pub fn slides(&self) -> &[Slide] {
        &self.history.current().deck.slides
    }
    pub fn current(&self) -> usize {
        self.slides()
            .iter()
            .position(|s| s.id == self.history.current().selected)
            .expect("valid selection")
    }
    pub fn select(&mut self, index: usize) -> Result<(), String> {
        let id = self
            .slides()
            .get(index)
            .ok_or("slide index out of range")?
            .id;
        let mut state = self.history.current().clone();
        state.selected = id;
        self.history.replace_current(state);
        Ok(())
    }
    pub fn edit(&mut self, title: String, body: String) -> Result<(), String> {
        let mut state = self.history.current().clone();
        let index = self.current();
        state.deck.slides[index] = state.deck.slides[index].edited(&title, &body);
        if !state.deck.valid() {
            return Err("invalid slide text or deck exceeds size limit".into());
        }
        self.history.apply(state);
        Ok(())
    }
    pub fn set_timing(&mut self, timing: SlideTiming) -> Result<(), String> {
        let mut state = self.history.current().clone();
        state.deck.slides[self.current()].timing = timing;
        if !state.deck.valid() {
            return Err("Invalid slide timing".into());
        }
        self.history.apply(state);
        Ok(())
    }
    /// Import raw text and establish line-ending policy for the selected slide.
    pub fn edit_raw(&mut self, title: String, body: String) -> Result<(), String> {
        let mut state = self.history.current().clone();
        let index = self.current();
        let id = state.deck.slides[index].id;
        let objects = state.deck.slides[index].objects.clone();
        let next_object_id = state.deck.slides[index].next_object_id;
        let timing = state.deck.slides[index].timing;
        state.deck.slides[index] = Slide::new(id, title, body);
        state.deck.slides[index].objects = objects;
        state.deck.slides[index].next_object_id = next_object_id;
        state.deck.slides[index].timing = timing;
        if !state.deck.valid() {
            return Err("invalid slide text or deck exceeds size limit".into());
        }
        self.history.apply(state);
        Ok(())
    }
    pub fn insert_object(&mut self, kind: ObjectKind, text: String) -> Result<u64, String> {
        self.insert_content(kind, text, Vec::new(), Vec::new())
    }
    pub fn insert_picture(&mut self, bytes: Vec<u8>) -> Result<u64, String> {
        if bytes.is_empty() {
            return Err("Empty image".into());
        }
        self.insert_content(ObjectKind::Picture, String::new(), bytes, Vec::new())
    }
    pub fn insert_table(&mut self, rows: usize, columns: usize) -> Result<u64, String> {
        if rows == 0 || columns == 0 || rows > 100 || columns > 100 {
            return Err("Invalid table size".into());
        }
        self.insert_content(
            ObjectKind::Table,
            String::new(),
            Vec::new(),
            vec![vec![String::new(); columns]; rows],
        )
    }
    fn insert_content(
        &mut self,
        kind: ObjectKind,
        text: String,
        media: Vec<u8>,
        cells: Vec<Vec<String>>,
    ) -> Result<u64, String> {
        let mut state = self.history.current().clone();
        let slide = &mut state.deck.slides[self.current()];
        let id = slide.next_object_id;
        slide.next_object_id = id.checked_add(1).ok_or("object identity exhausted")?;
        slide.objects.push(SlideObject {
            id,
            group_id: None,
            x: 100.0,
            y: 120.0,
            width: 180.0,
            height: 90.0,
            rotation_degrees: 0.0,
            kind,
            fill: [220, 230, 245],
            stroke: [40, 70, 110],
            fill_visible: true,
            stroke_visible: true,
            text,
            text_style: TextStyle::default(),
            paragraphs: Vec::new(),
            media,
            cells,
        });
        if !state.deck.valid() {
            return Err("deck exceeds limits".into());
        }
        self.history.apply(state);
        Ok(id)
    }
    pub fn update_object(&mut self, object: SlideObject) -> Result<(), String> {
        self.update_objects(vec![object])
    }
    pub fn update_objects(&mut self, objects: Vec<SlideObject>) -> Result<(), String> {
        let mut state = self.history.current().clone();
        let mut seen = HashSet::new();
        for object in objects {
            if !seen.insert(object.id) {
                return Err("duplicate object identity".into());
            }
            let target = state.deck.slides[self.current()]
                .objects
                .iter_mut()
                .find(|o| o.id == object.id)
                .ok_or("object not found")?;
            *target = object;
        }
        if !state.deck.valid() {
            return Err("invalid object or deck exceeds limits".into());
        }
        self.history.apply(state);
        Ok(())
    }
    pub fn align_objects(&mut self, ids: &[u64], mode: ObjectAlignment) -> Result<(), String> {
        let selected = self.slides()[self.current()]
            .objects
            .iter()
            .filter(|o| ids.contains(&o.id))
            .cloned()
            .collect::<Vec<_>>();
        if selected.len() != ids.len() || selected.len() < 2 {
            return Err("Select at least two objects".into());
        }
        let left = selected.iter().map(|o| o.x).fold(f64::INFINITY, f64::min);
        let top = selected.iter().map(|o| o.y).fold(f64::INFINITY, f64::min);
        let right = selected
            .iter()
            .map(|o| o.x + o.width)
            .fold(f64::NEG_INFINITY, f64::max);
        let bottom = selected
            .iter()
            .map(|o| o.y + o.height)
            .fold(f64::NEG_INFINITY, f64::max);
        let mut objects = selected;
        match mode {
            ObjectAlignment::Left => {
                for o in &mut objects {
                    o.x = left
                }
            }
            ObjectAlignment::Center => {
                for o in &mut objects {
                    o.x = (left + right - o.width) / 2.0
                }
            }
            ObjectAlignment::Right => {
                for o in &mut objects {
                    o.x = right - o.width
                }
            }
            ObjectAlignment::Top => {
                for o in &mut objects {
                    o.y = top
                }
            }
            ObjectAlignment::Middle => {
                for o in &mut objects {
                    o.y = (top + bottom - o.height) / 2.0
                }
            }
            ObjectAlignment::Bottom => {
                for o in &mut objects {
                    o.y = bottom - o.height
                }
            }
            ObjectAlignment::DistributeHorizontal | ObjectAlignment::DistributeVertical => {
                if objects.len() < 3 {
                    return Err("Select at least three objects to distribute".into());
                }
                let horizontal = matches!(mode, ObjectAlignment::DistributeHorizontal);
                objects.sort_by(|a, b| {
                    if horizontal {
                        a.x.total_cmp(&b.x)
                    } else {
                        a.y.total_cmp(&b.y)
                    }
                });
                let total = objects
                    .iter()
                    .map(|o| if horizontal { o.width } else { o.height })
                    .sum::<f64>();
                let (start, end) = if horizontal {
                    (left, right)
                } else {
                    (top, bottom)
                };
                let gap = (end - start - total) / (objects.len() - 1) as f64;
                let mut pos = start;
                for o in &mut objects {
                    if horizontal {
                        o.x = pos;
                        pos += o.width + gap
                    } else {
                        o.y = pos;
                        pos += o.height + gap
                    }
                }
            }
        }
        self.update_objects(objects)
    }
    pub fn duplicate_objects(&mut self, ids: &[u64]) -> Result<Vec<u64>, String> {
        let mut state = self.history.current().clone();
        let slide = &mut state.deck.slides[self.current()];
        let selected = slide
            .objects
            .iter()
            .filter(|o| ids.contains(&o.id))
            .cloned()
            .collect::<Vec<_>>();
        if selected.is_empty() || selected.len() != ids.len() {
            return Err("Invalid selection".into());
        }
        let mut created = Vec::new();
        let mut groups = HashMap::new();
        for mut object in selected {
            if let Some(old) = object.group_id {
                let count = ids
                    .iter()
                    .filter(|id| {
                        slide
                            .objects
                            .iter()
                            .any(|o| o.id == **id && o.group_id == Some(old))
                    })
                    .count();
                if count >= 2 {
                    let group = if let Some(id) = groups.get(&old) {
                        *id
                    } else {
                        let id = slide.next_object_id;
                        slide.next_object_id = slide
                            .next_object_id
                            .checked_add(1)
                            .ok_or("object identity exhausted")?;
                        groups.insert(old, id);
                        id
                    };
                    object.group_id = Some(group)
                } else {
                    object.group_id = None
                }
            }
            object.id = slide.next_object_id;
            slide.next_object_id = slide
                .next_object_id
                .checked_add(1)
                .ok_or("object identity exhausted")?;
            object.x += 12.0;
            object.y += 12.0;
            created.push(object.id);
            slide.objects.push(object);
        }
        if !state.deck.valid() {
            return Err("deck exceeds limits".into());
        }
        self.history.apply(state);
        Ok(created)
    }
    pub fn copy_objects(&self, ids: &[u64]) -> Result<Vec<u8>, String> {
        let selected = self.slides()[self.current()]
            .objects
            .iter()
            .filter(|o| ids.contains(&o.id))
            .cloned()
            .collect::<Vec<_>>();
        if selected.is_empty() || selected.len() != ids.len() {
            return Err("Invalid selection".into());
        }
        let bytes = serde_json::to_vec(&selected).map_err(|e| e.to_string())?;
        if bytes.len() as u64 > MAX_MEDIA_FILE {
            return Err("Clipboard exceeds size limit".into());
        }
        Ok(bytes)
    }
    pub fn paste_objects(&mut self, bytes: &[u8]) -> Result<Vec<u64>, String> {
        if bytes.len() as u64 > MAX_MEDIA_FILE {
            return Err("Clipboard exceeds size limit".into());
        }
        let objects: Vec<SlideObject> =
            serde_json::from_slice(bytes).map_err(|_| "Invalid presentation clipboard")?;
        if objects.is_empty() {
            return Err("Clipboard is empty".into());
        }
        let mut state = self.history.current().clone();
        let slide = &mut state.deck.slides[self.current()];
        let mut ids = Vec::new();
        let mut groups = HashMap::new();
        let counts =
            objects
                .iter()
                .filter_map(|o| o.group_id)
                .fold(HashMap::new(), |mut map, id| {
                    *map.entry(id).or_insert(0usize) += 1;
                    map
                });
        for mut object in objects {
            if let Some(old) = object.group_id {
                if counts.get(&old).copied().unwrap_or(0) >= 2 {
                    let group = if let Some(id) = groups.get(&old) {
                        *id
                    } else {
                        let id = slide.next_object_id;
                        slide.next_object_id = slide
                            .next_object_id
                            .checked_add(1)
                            .ok_or("object identity exhausted")?;
                        groups.insert(old, id);
                        id
                    };
                    object.group_id = Some(group)
                } else {
                    object.group_id = None
                }
            }
            object.id = slide.next_object_id;
            slide.next_object_id = slide
                .next_object_id
                .checked_add(1)
                .ok_or("object identity exhausted")?;
            object.x += 12.0;
            object.y += 12.0;
            ids.push(object.id);
            slide.objects.push(object);
        }
        if !state.deck.valid() {
            return Err("Invalid clipboard objects or deck exceeds limits".into());
        }
        self.history.apply(state);
        Ok(ids)
    }
    pub fn clipboard_has_groups(bytes: &[u8]) -> Result<bool, String> {
        if bytes.len() as u64 > MAX_MEDIA_FILE {
            return Err("Clipboard exceeds size limit".into());
        }
        let objects: Vec<SlideObject> =
            serde_json::from_slice(bytes).map_err(|_| "Invalid presentation clipboard")?;
        Ok(objects.iter().any(|o| o.group_id.is_some()))
    }
    pub fn group_objects(&mut self, ids: &[u64]) -> Result<(), String> {
        if ids.len() < 2 {
            return Err("Select at least two objects".into());
        }
        let mut state = self.history.current().clone();
        let slide = &mut state.deck.slides[self.current()];
        if ids.iter().collect::<HashSet<_>>().len() != ids.len()
            || ids.iter().any(|id| {
                !slide
                    .objects
                    .iter()
                    .any(|o| o.id == *id && o.group_id.is_none())
            })
        {
            return Err("Select ungrouped objects".into());
        }
        let group = slide.next_object_id;
        slide.next_object_id = group.checked_add(1).ok_or("object identity exhausted")?;
        for object in &mut slide.objects {
            if ids.contains(&object.id) {
                object.group_id = Some(group);
            }
        }
        if !state.deck.valid() {
            return Err("deck exceeds limits".into());
        }
        self.history.apply(state);
        Ok(())
    }
    pub fn ungroup_objects(&mut self, ids: &[u64]) -> Result<(), String> {
        let mut state = self.history.current().clone();
        let slide = &mut state.deck.slides[self.current()];
        let groups = slide
            .objects
            .iter()
            .filter(|o| ids.contains(&o.id))
            .filter_map(|o| o.group_id)
            .collect::<HashSet<_>>();
        if groups.is_empty() {
            return Err("Select grouped objects".into());
        }
        for object in &mut slide.objects {
            if object.group_id.is_some_and(|id| groups.contains(&id)) {
                object.group_id = None;
            }
        }
        self.history.apply(state);
        Ok(())
    }
    pub fn delete_object(&mut self, id: u64) -> Result<(), String> {
        self.delete_objects(&[id])
    }
    pub fn delete_objects(&mut self, ids: &[u64]) -> Result<(), String> {
        let mut state = self.history.current().clone();
        let objects = &mut state.deck.slides[self.current()].objects;
        if ids.is_empty()
            || ids.iter().collect::<HashSet<_>>().len() != ids.len()
            || ids.iter().any(|id| !objects.iter().any(|o| o.id == *id))
        {
            return Err("Invalid object selection".into());
        }
        objects.retain(|o| !ids.contains(&o.id));
        let counts =
            objects
                .iter()
                .filter_map(|o| o.group_id)
                .fold(HashMap::new(), |mut map, id| {
                    *map.entry(id).or_insert(0usize) += 1;
                    map
                });
        for object in objects.iter_mut() {
            if object
                .group_id
                .is_some_and(|id| counts.get(&id).copied().unwrap_or(0) < 2)
            {
                object.group_id = None;
            }
        }
        self.history.apply(state);
        Ok(())
    }
    pub fn arrange_object(&mut self, id: u64, front: bool) -> Result<(), String> {
        let mut state = self.history.current().clone();
        let objects = &mut state.deck.slides[self.current()].objects;
        let index = objects
            .iter()
            .position(|o| o.id == id)
            .ok_or("object not found")?;
        let object = objects.remove(index);
        if front {
            objects.push(object);
        } else {
            objects.insert(0, object);
        }
        self.history.apply(state);
        Ok(())
    }
    fn deck_with_new_slide(&self) -> Result<State, String> {
        if self.slides().len() >= MAX_SLIDES {
            return Err("slide limit exceeded".into());
        }
        let mut state = self.history.current().clone();
        let id = state.deck.next_id;
        state.deck.next_id = id.checked_add(1).ok_or("slide identity exhausted")?;
        state.deck.slides.insert(
            self.current() + 1,
            Slide::new(id, String::new(), String::new()),
        );
        state.selected = id;
        if !state.deck.valid() {
            return Err("deck exceeds size limit".into());
        }
        Ok(state)
    }
    pub fn can_add(&self) -> bool {
        self.deck_with_new_slide().is_ok()
    }
    pub fn add(&mut self) -> Result<(), String> {
        let state = self.deck_with_new_slide()?;
        self.history.apply(state);
        Ok(())
    }
    fn deck_with_duplicate(&self) -> Result<State, String> {
        if self.slides().len() >= MAX_SLIDES {
            return Err("slide limit exceeded".into());
        }
        let mut state = self.history.current().clone();
        let id = state.deck.next_id;
        state.deck.next_id = id.checked_add(1).ok_or("slide identity exhausted")?;
        let mut slide = state.deck.slides[self.current()].clone();
        slide.origin = Some(slide.origin.unwrap_or(slide.id));
        slide.id = id;
        state.deck.slides.insert(self.current() + 1, slide);
        state.selected = id;
        if !state.deck.valid() {
            return Err("deck exceeds size limit".into());
        }
        Ok(state)
    }
    pub fn can_duplicate(&self) -> bool {
        self.deck_with_duplicate().is_ok()
    }
    pub fn duplicate(&mut self) -> Result<(), String> {
        let state = self.deck_with_duplicate()?;
        self.history.apply(state);
        Ok(())
    }
    pub fn delete(&mut self) -> Result<(), String> {
        if self.slides().len() == 1 {
            return Err("cannot delete last slide".into());
        }
        let mut state = self.history.current().clone();
        let i = self.current();
        state.deck.slides.remove(i);
        state.selected = state.deck.slides[i.min(state.deck.slides.len() - 1)].id;
        self.history.apply(state);
        Ok(())
    }
    pub fn move_slide(&mut self, from: usize, to: usize) -> Result<(), String> {
        let mut state = self.history.current().clone();
        if from >= state.deck.slides.len() || to >= state.deck.slides.len() {
            return Err("slide index out of range".into());
        }
        let slide = state.deck.slides.remove(from);
        state.selected = slide.id;
        state.deck.slides.insert(to, slide);
        self.history.apply(state);
        Ok(())
    }
    pub fn undo(&mut self) -> bool {
        self.history.undo()
    }
    pub fn redo(&mut self) -> bool {
        self.history.redo()
    }
    pub fn can_undo(&self) -> bool {
        self.history.can_undo()
    }
    pub fn can_redo(&self) -> bool {
        self.history.can_redo()
    }
    pub fn is_dirty(&self) -> bool {
        self.history.is_dirty()
    }
    pub fn save_snapshot(&self) -> Result<(Vec<u8>, SaveState), String> {
        let state = self.history.snapshot();
        let bytes = serde_json::to_vec(&state.deck).map_err(|e| e.to_string())?;
        if bytes.len() as u64 > state.deck.file_limit() {
            return Err("deck exceeds file limit".into());
        }
        Ok((bytes, SaveState(state)))
    }
    pub fn mark_saved_snapshot(&mut self, state: SaveState) {
        self.history.mark_saved_snapshot(state.0);
    }
    pub fn save_as(&mut self, path: &Path) -> Result<(), String> {
        let (bytes, state) = self.save_snapshot()?;
        write_new(path, &bytes).map_err(|e| e.to_string())?;
        self.mark_saved_snapshot(state);
        Ok(())
    }
    pub fn open(path: &Path) -> Result<Self, String> {
        if fs::metadata(path).map_err(|e| e.to_string())?.len() > MAX_MEDIA_FILE {
            return Err("deck exceeds file limit".into());
        }
        let mut bytes = Vec::new();
        fs::File::open(path)
            .map_err(|e| e.to_string())?
            .take(MAX_MEDIA_FILE + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        if bytes.len() as u64 > MAX_MEDIA_FILE {
            return Err("deck exceeds file limit".into());
        }
        let mut deck: Deck = serde_json::from_slice(&bytes)
            .map_err(|e| format!("invalid or unsupported version 2 deck: {e}"))?;
        for slide in &mut deck.slides {
            slide.title = slide.title_text.raw().to_owned();
            slide.body = slide.body_text.raw().to_owned();
            if let Some(max_id) = slide.objects.iter().map(|o| o.id).max() {
                slide.next_object_id = slide
                    .next_object_id
                    .max(max_id.checked_add(1).ok_or("object identity exhausted")?);
            }
        }
        if !deck.valid() {
            return Err("unsupported version or invalid deck".into());
        }
        let selected = deck.slides[0].id;
        Ok(Self {
            history: SnapshotHistory::new(State { deck, selected }, HISTORY_LIMIT),
        })
    }
}
#[cfg(test)]
mod tests {
    #[test]
    fn grouped_objects_roundtrip_and_clipboard_preserve_independent_identity() {
        let mut deck = super::Presentation::new();
        let a = deck
            .insert_object(super::ObjectKind::Rectangle, "a".into())
            .unwrap();
        let b = deck
            .insert_object(super::ObjectKind::Ellipse, "b".into())
            .unwrap();
        deck.group_objects(&[a, b]).unwrap();
        let group = deck.slides()[0].objects[0].group_id;
        assert!(group.is_some());
        assert_eq!(deck.slides()[0].objects[1].group_id, group);
        for reopened in [
            super::pptx::import(&super::pptx::export_new(&deck).unwrap())
                .unwrap()
                .0,
            super::odp::import(&super::odp::export(&deck).unwrap()).unwrap(),
        ] {
            assert!(reopened.slides()[0].objects[0].group_id.is_some());
            assert_eq!(
                reopened.slides()[0].objects[0].group_id,
                reopened.slides()[0].objects[1].group_id
            );
        }
        let copy = deck.copy_objects(&[a, b]).unwrap();
        let ids = deck.paste_objects(&copy).unwrap();
        let cloned = deck.slides()[0]
            .objects
            .iter()
            .filter(|o| ids.contains(&o.id))
            .collect::<Vec<_>>();
        assert_ne!(cloned[0].group_id, group);
        assert_eq!(cloned[0].group_id, cloned[1].group_id);
        assert!(deck.undo());
        assert_eq!(deck.slides()[0].objects.len(), 2);
        deck.ungroup_objects(&[a, b]).unwrap();
        assert!(deck.slides()[0]
            .objects
            .iter()
            .all(|o| o.group_id.is_none()));
        assert!(deck.undo());
        assert!(deck.slides()[0].objects[0].group_id.is_some());
    }
    #[test]
    fn alignment_and_duplicate_are_atomic_history_steps() {
        let mut deck = super::Presentation::new();
        let a = deck
            .insert_object(super::ObjectKind::Rectangle, String::new())
            .unwrap();
        let b = deck
            .insert_object(super::ObjectKind::Ellipse, String::new())
            .unwrap();
        let mut second = deck.slides()[0].objects[1].clone();
        second.x = 300.0;
        deck.update_object(second).unwrap();
        deck.align_objects(&[a, b], super::ObjectAlignment::Left)
            .unwrap();
        assert_eq!(deck.slides()[0].objects[1].x, 100.0);
        assert!(deck.undo());
        assert_eq!(deck.slides()[0].objects[1].x, 300.0);
        assert!(deck.redo());
        let created = deck.duplicate_objects(&[a, b]).unwrap();
        assert_eq!(created.len(), 2);
        assert_eq!(deck.slides()[0].objects.len(), 4);
        assert!(deck.undo());
        assert_eq!(deck.slides()[0].objects.len(), 2);
    }
    #[test]
    fn object_edit_history_and_persistence() {
        let mut deck = super::Presentation::new();
        let id = deck
            .insert_object(super::ObjectKind::Rectangle, "hello".into())
            .unwrap();
        let mut object = deck.slides()[0].objects[0].clone();
        object.x = 245.0;
        deck.update_object(object).unwrap();
        assert!(deck.undo());
        assert_eq!(deck.slides()[0].objects[0].x, 100.0);
        assert!(deck.redo());
        let (bytes, _) = deck.save_snapshot().unwrap();
        let path =
            std::env::temp_dir().join(format!("ppt42-object-{}-{}.json", std::process::id(), id));
        std::fs::write(&path, bytes).unwrap();
        let opened = super::Presentation::open(&path).unwrap();
        std::fs::remove_file(path).unwrap();
        assert_eq!(opened.slides()[0].objects[0].x, 245.0);
        assert_eq!(opened.slides()[0].objects[0].text, "hello");
    }
    use super::*;
    #[test]
    fn timing_native_snapshot_and_legacy_default() {
        let mut deck = Presentation::new();
        let timing = SlideTiming {
            advance_ms: Some(1500),
            transition: Transition::Fade,
            duration_ms: 700,
        };
        deck.set_timing(timing).unwrap();
        let (bytes, _) = deck.save_snapshot().unwrap();
        let parsed: Deck = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(parsed.slides[0].timing, timing);
        let mut json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        json["slides"][0].as_object_mut().unwrap().remove("timing");
        let legacy: Deck = serde_json::from_value(json).unwrap();
        assert_eq!(legacy.slides[0].timing, SlideTiming::default());
    }
    #[test]
    fn duplicate_preserves_identity_policy_history_and_reopen() {
        let mut p = Presentation::new();
        p.edit_raw("same".into(), "a\r\nb".into()).unwrap();
        let original = p.slides()[0].id;
        p.duplicate().unwrap();
        let copied = p.slides()[1].id;
        assert_ne!(original, copied);
        assert_eq!(p.current(), 1);
        assert_eq!(p.slides()[1].body, "a\r\nb");
        assert!(p.undo());
        assert_eq!(p.current(), 0);
        assert!(p.redo());
        assert_eq!(p.current(), 1);
        let path =
            std::env::temp_dir().join(format!("ppt42-duplicate-{}.json", std::process::id()));
        let _ = fs::remove_file(&path);
        p.save_as(&path).unwrap();
        let mut q = Presentation::open(&path).unwrap();
        q.select(1).unwrap();
        q.edit("same".into(), "a\nb\nc".into()).unwrap();
        assert_eq!(q.slides()[1].body, "a\r\nb\r\nc");
        assert_eq!(q.slides()[1].id, copied);
        assert!(p.undo());
        p.edit("branch".into(), "".into()).unwrap();
        assert!(!p.can_redo());
        fs::remove_file(path).unwrap();
    }
    #[test]
    fn duplicate_refuses_size_budget_without_mutation() {
        let mut p = Presentation::new();
        p.edit_raw("".into(), "\u{1}".repeat(MAX_TEXT)).unwrap();
        while p.can_duplicate() {
            p.duplicate().unwrap();
        }
        let n = p.slides().len();
        let selected = p.current();
        let undo = p.can_undo();
        assert!(p.duplicate().is_err());
        assert_eq!(p.slides().len(), n);
        assert_eq!(p.current(), selected);
        assert_eq!(p.can_undo(), undo);
    }
    #[test]
    fn escaped_budget_rejection_preserves_state_and_history() {
        let mut p = Presentation::new();
        let payload = "\u{0001}".repeat(MAX_TEXT);
        for _ in 0..1 {
            p.edit(payload.clone(), payload.clone()).unwrap();
            p.add().unwrap();
        }
        let before = p.slides().to_vec();
        let current = p.current();
        let undo = p.can_undo();
        let redo = p.can_redo();
        assert!(p.edit(payload.clone(), payload).is_err());
        assert_eq!(p.slides(), before);
        assert_eq!(p.current(), current);
        assert_eq!(p.can_undo(), undo);
        assert_eq!(p.can_redo(), redo);
        let path = std::env::temp_dir().join(format!("ppt42-budget-{}.json", std::process::id()));
        let _ = fs::remove_file(&path);
        p.save_as(&path).unwrap();
        assert_eq!(Presentation::open(&path).unwrap().slides(), p.slides());
        fs::remove_file(path).unwrap();
    }
    #[test]
    fn add_capability_tracks_limits_and_undo() {
        let mut p = Presentation::new();
        for _ in 1..MAX_SLIDES {
            assert!(p.can_add());
            p.add().unwrap();
        }
        assert!(!p.can_add());
        assert!(p.add().is_err());
        assert!(p.undo());
        assert!(p.can_add());
        assert!(p.add().is_ok());
        let mut p = Presentation::new();
        let data = "\u{0001}".repeat(MAX_TEXT);
        p.edit(data.clone(), data.clone()).unwrap();
        p.add().unwrap();
        let title = data[..21000].to_string();
        p.edit(title.clone(), String::new()).unwrap();
        let used = serde_json::to_vec(&p.history.current().deck).unwrap().len();
        let body = "a".repeat(MAX_FILE as usize - used - 5);
        p.edit(title, body).unwrap();
        assert!(!p.can_add());
        assert!(p.add().is_err());
        assert!(p.undo());
        assert!(p.can_add());
    }
    #[test]
    fn identical_slides_keep_identity_and_newline_policy_through_history() {
        let mut p = Presentation::new();
        p.edit_raw("first\r\nline".into(), "body\r\nline".into())
            .unwrap();
        let crlf = p.slides()[0].id;
        p.add().unwrap();
        p.edit_raw("second\nline".into(), "body\nline".into())
            .unwrap();
        let lf = p.slides()[1].id;
        p.select(0).unwrap();
        p.edit("same".into(), "same".into()).unwrap();
        p.select(1).unwrap();
        p.edit("same".into(), "same".into()).unwrap();
        p.move_slide(0, 1).unwrap();
        assert_eq!(p.slides()[1].id, crlf);
        assert_eq!(p.slides()[0].id, lf);
        assert_eq!(p.current(), 1);
        p.edit("title\nnext".into(), "body\nnext".into()).unwrap();
        assert_eq!(p.slides()[1].title, "title\r\nnext");
        p.select(0).unwrap();
        p.edit("title\nnext".into(), "body\nnext".into()).unwrap();
        assert_eq!(p.slides()[0].title, "title\nnext");
        assert!(p.undo());
        assert_eq!(p.current(), 0);
        assert_eq!(p.slides()[0].title, "same");
        assert!(p.redo());
        assert_eq!(p.slides()[0].title, "title\nnext");
        let path = std::env::temp_dir().join(format!("ppt42-identity-{}.json", std::process::id()));
        let _ = fs::remove_file(&path);
        p.save_as(&path).unwrap();
        let mut reopened = Presentation::open(&path).unwrap();
        assert_eq!(reopened.slides(), p.slides());
        reopened.select(1).unwrap();
        reopened
            .edit("again\nline".into(), "again\nline".into())
            .unwrap();
        assert_eq!(reopened.slides()[1].title, "again\r\nline");
        assert!(p.undo());
        p.edit("branch\nline".into(), "branch\nline".into())
            .unwrap();
        assert!(!p.can_redo());
        assert_eq!(p.slides()[0].id, lf);
        assert_eq!(p.slides()[0].title, "branch\nline");
        fs::remove_file(path).unwrap();
    }
    #[test]
    fn rejects_unknown_text_fields_and_contradictory_policy_on_open() {
        let dir = std::env::temp_dir().join(format!("ppt42-schema-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("deck.json");
        let mut p = Presentation::new();
        p.edit_raw("title\r\nline".into(), "body\nline".into())
            .unwrap();
        let valid = serde_json::to_value(&p.history.current().deck).unwrap();
        for (field, unknown) in [
            ("title_text", true),
            ("body_text", true),
            ("title_text", false),
            ("body_text", false),
        ] {
            let mut value = valid.clone();
            let text = &mut value["slides"][0][field];
            if unknown {
                text["future"] = serde_json::json!(42);
            } else {
                text["crlf"] = serde_json::json!(field == "body_text");
            }
            let bytes = serde_json::to_vec(&value).unwrap();
            fs::write(&path, &bytes).unwrap();
            assert!(Presentation::open(&path).is_err());
            assert_eq!(fs::read(&path).unwrap(), bytes);
        }
        // Either retained policy is valid when no newline remains.
        let mut value = valid;
        value["slides"][0]["title_text"]["raw"] = serde_json::json!("no newline");
        for flag in [true, false] {
            value["slides"][0]["title_text"]["crlf"] = serde_json::json!(flag);
            fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
            assert!(Presentation::open(&path).is_ok());
        }
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn invalid_ids_and_exhaustion_are_rejected() {
        let mut p = Presentation::new();
        p.add().unwrap();
        let mut state = p.history.current().clone();
        state.deck.slides[1].id = state.deck.slides[0].id;
        assert!(!state.deck.valid());
        state = p.history.current().clone();
        state.deck.next_id = u64::MAX;
        assert!(state.deck.valid());
        p.history.replace_current(state);
        assert!(!p.can_add());
        assert!(p.add().is_err());
    }
    #[test]
    fn state_and_roundtrip() {
        let mut p = Presentation::new();
        p.edit("First".into(), "text".into()).unwrap();
        p.add().unwrap();
        p.edit("Second".into(), "body".into()).unwrap();
        p.move_slide(1, 0).unwrap();
        assert_eq!(p.slides()[0].title, "Second");
        assert!(p.undo());
        assert_eq!(p.slides()[0].title, "First");
        assert!(p.redo());
        let path = std::env::temp_dir().join(format!("ppt42-test-{}.json", std::process::id()));
        let _ = fs::remove_file(&path);
        p.save_as(&path).unwrap();
        assert!(!p.is_dirty());
        let r = Presentation::open(&path).unwrap();
        assert_eq!(r.slides(), p.slides());
        assert!(p.save_as(&path).is_err());
        fs::remove_file(path).unwrap();
        assert!(p.delete().is_ok());
        assert!(p.delete().is_err());
    }
}
