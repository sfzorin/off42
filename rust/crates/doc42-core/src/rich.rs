//! Structured document state. Text positions are UTF-8 byte offsets.
mod annotations;
pub use annotations::ProjectedNote;
mod docx;
mod html;
mod links;
mod markdown;
mod odt;
mod rtf;
mod rtf_field;
mod rtf_lists;
mod rtf_picture;
mod rtf_read;
mod table_edit;
mod toc;
use serde::{Deserialize, Serialize};
use std::fmt;
use suite42_core::{SnapshotHistory, TextValue};
const MAX_BYTES: usize = 16 * 1024 * 1024;
const MAX_BLOCKS: usize = 100_000;
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    Invalid(String),
    Limit,
    Unsupported(String),
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid(s) => write!(f, "invalid document: {s}"),
            Self::Limit => write!(f, "document size limit exceeded"),
            Self::Unsupported(s) => write!(f, "unsupported fidelity: {s}"),
        }
    }
}
impl std::error::Error for Error {}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Project,
    Text,
    Rtf,
    Docx,
    Odt,
    Html,
    Markdown,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TextStyle {
    pub font: String,
    #[serde(default)]
    pub source_style: Option<String>,
    pub size_half_points: u16,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strike: bool,
    pub color: Option<u32>,
    #[serde(default)]
    pub theme_color: Option<String>,
    #[serde(default)]
    pub theme_tint: Option<String>,
    #[serde(default)]
    pub theme_shade: Option<String>,
    #[serde(default)]
    pub odt_extra: std::collections::BTreeMap<String, String>,
    pub highlight: Option<u32>,
}
impl Default for TextStyle {
    fn default() -> Self {
        Self {
            font: "Arial".into(),
            source_style: None,
            size_half_points: 24,
            bold: false,
            italic: false,
            underline: false,
            strike: false,
            color: None,
            theme_color: None,
            theme_tint: None,
            theme_shade: None,
            odt_extra: Default::default(),
            highlight: None,
        }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Alignment {
    #[default]
    Left,
    Center,
    Right,
    Justify,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ParagraphStyle {
    #[serde(default)]
    pub source_style: Option<String>,
    #[serde(default)]
    pub odt_extra: std::collections::BTreeMap<String, String>,
    pub alignment: Alignment,
    pub left_twips: i32,
    pub right_twips: i32,
    pub first_line_twips: i32,
    pub before_twips: u32,
    pub after_twips: u32,
    pub line_twips: i32,
    #[serde(default)]
    pub tabs_twips: Vec<i32>,
    pub list: Option<ListMarker>,
    #[serde(default)]
    pub keep_with_next: bool,
    #[serde(default)]
    pub keep_lines: bool,
    #[serde(default)]
    pub page_break_before: bool,
    #[serde(default)]
    pub outline_level: Option<u8>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ListMarker {
    pub id: u64,
    pub level: u8,
    pub numbered: bool,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Run {
    pub text: String,
    pub style: TextStyle,
    pub link: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Paragraph {
    pub id: u64,
    pub style: ParagraphStyle,
    pub runs: Vec<Run>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Border {
    pub width_twips: u16,
    pub color: u32,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Cell {
    pub paragraphs: Vec<Paragraph>,
    pub row_span: u16,
    pub col_span: u16,
    pub border: Option<Border>,
    pub fill: Option<u32>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Table {
    pub id: u64,
    pub rows: Vec<Vec<Cell>>,
    /// Empty means the default 72-point width for every column.
    #[serde(default)]
    pub column_widths_twips: Vec<u32>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Image {
    pub id: u64,
    pub mime: String,
    pub data: Vec<u8>,
    pub width_twips: u32,
    pub height_twips: u32,
    pub alt: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Block {
    Paragraph(Paragraph),
    Table(Table),
    Image(Image),
    SectionBreak,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PageSettings {
    pub width_twips: u32,
    pub height_twips: u32,
    pub margin_left: u32,
    pub margin_right: u32,
    pub margin_top: u32,
    pub margin_bottom: u32,
    pub columns: u8,
    pub column_gap: u32,
    #[serde(default = "default_header_distance")]
    pub header_distance_twips: u32,
    #[serde(default = "default_header_distance")]
    pub footer_distance_twips: u32,
}
fn default_header_distance() -> u32 {
    720
}
impl Default for PageSettings {
    fn default() -> Self {
        Self {
            width_twips: 11906,
            height_twips: 16838,
            margin_left: 1440,
            margin_right: 1440,
            margin_top: 1440,
            margin_bottom: 1440,
            columns: 1,
            column_gap: 720,
            header_distance_twips: 720,
            footer_distance_twips: 720,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Section {
    pub start_block: usize,
    pub page: PageSettings,
    pub header: Vec<Paragraph>,
    pub footer: Vec<Paragraph>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Position {
    pub paragraph: u64,
    pub offset: usize,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Selection {
    pub anchor: Option<Position>,
    pub focus: Option<Position>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Annotation {
    pub id: u64,
    pub start: Position,
    pub end: Position,
    pub author: String,
    pub text: String,
    pub resolved: bool,
    #[serde(default)]
    pub date: Option<String>,
    #[serde(default)]
    pub initials: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Footnote {
    pub id: u64,
    pub at: Position,
    pub paragraphs: Vec<Paragraph>,
    #[serde(default)]
    pub endnote: bool,
    #[serde(default)]
    pub citation: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Revision {
    pub id: u64,
    pub start: Position,
    pub end: Position,
    pub author: String,
    pub insertion: bool,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Field {
    pub id: u64,
    pub at: Position,
    pub kind: FieldKind,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum FieldKind {
    PageNumber,
    PageCount,
    Date,
    Custom(String),
}
/// A named bookmark spans two stable text positions.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Bookmark {
    pub id: u64,
    pub name: String,
    pub start: Position,
    pub end: Position,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OdtTocCache {
    pub index_xml: String,
    pub source_xml: String,
    #[serde(default)]
    pub title_xml: Option<String>,
    #[serde(default)]
    pub title_paragraphs: Vec<u64>,
    pub automatic_styles: String,
    pub has_pages: bool,
    #[serde(default)]
    pub needs_refresh: bool,
}
/// Generated entries retain their heading identity independently of displayed text.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TocEntry {
    pub paragraph: u64,
    pub heading: u64,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TableOfContents {
    pub id: u64,
    pub entries: Vec<TocEntry>,
    #[serde(default)]
    pub min_level: u8,
    #[serde(default = "toc_default_max_level")]
    pub max_level: u8,
    /// Original Word field paragraphs, including source-provided cached pages.
    #[serde(default)]
    pub docx_cached: Option<Vec<String>>,
    #[serde(default)]
    pub odt_cached: Option<OdtTocCache>,
}
fn toc_default_max_level() -> u8 {
    8
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct State {
    pub blocks: Vec<Block>,
    pub sections: Vec<Section>,
    pub selection: Selection,
    pub comments: Vec<Annotation>,
    pub footnotes: Vec<Footnote>,
    pub revisions: Vec<Revision>,
    pub fields: Vec<Field>,
    #[serde(default)]
    pub bookmarks: Vec<Bookmark>,
    #[serde(default)]
    pub tables_of_contents: Vec<TableOfContents>,
    pub next_id: u64,
    pub text_policy: TextValue,
    /// Opaque ancillary DOCX parts retained during supported edits.
    #[serde(default)]
    pub docx_parts: std::collections::BTreeMap<String, Vec<u8>>,
    /// Original OpenDocument package parts retained across supported saves.
    #[serde(default)]
    pub odt_parts: std::collections::BTreeMap<String, Vec<u8>>,
    #[serde(default)]
    pub odt_source_page: Option<PageSettings>,
    #[serde(default)]
    pub odt_source_headers: Option<(Vec<Paragraph>, Vec<Paragraph>)>,
    #[serde(default)]
    pub odt_source_header_fields: Option<Vec<Field>>,
}
impl Default for State {
    fn default() -> Self {
        Self {
            blocks: vec![Block::Paragraph(Paragraph {
                id: 1,
                style: ParagraphStyle::default(),
                runs: vec![Run {
                    text: String::new(),
                    style: TextStyle::default(),
                    link: None,
                }],
            })],
            sections: vec![Section {
                start_block: 0,
                page: PageSettings::default(),
                header: vec![],
                footer: vec![],
            }],
            selection: Selection::default(),
            comments: vec![],
            footnotes: vec![],
            revisions: vec![],
            fields: vec![],
            bookmarks: vec![],
            tables_of_contents: vec![],
            next_id: 2,
            text_policy: TextValue::default(),
            docx_parts: Default::default(),
            odt_parts: Default::default(),
            odt_source_page: None,
            odt_source_headers: None,
            odt_source_header_fields: None,
        }
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Project {
    magic: String,
    version: u32,
    state: State,
}
pub struct RichDocument {
    history: SnapshotHistory<State>,
}
impl Default for RichDocument {
    fn default() -> Self {
        Self::new()
    }
}
impl RichDocument {
    pub fn new() -> Self {
        Self {
            history: SnapshotHistory::new(State::default(), 8),
        }
    }
    pub fn state(&self) -> &State {
        self.history.current()
    }
    pub fn transact(
        &mut self,
        operation: impl FnOnce(&mut State) -> Result<(), Error>,
    ) -> Result<bool, Error> {
        let mut next = self.state().clone();
        operation(&mut next)?;
        validate(&next)?;
        Ok(self.history.apply(next))
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
    pub fn mark_saved(&mut self) {
        self.history.mark_saved();
    }
    /// Capture an exact state for asynchronous file publication.
    pub fn snapshot(&self) -> State {
        self.history.snapshot()
    }
    /// Mark only the state that was actually published as saved.
    pub fn mark_saved_snapshot(&mut self, state: State) -> Result<(), Error> {
        validate(&state)?;
        self.history.mark_saved_snapshot(state);
        Ok(())
    }
    pub fn encode_snapshot(state: State, format: Format) -> Result<Vec<u8>, Error> {
        validate(&state)?;
        Self {
            history: SnapshotHistory::new(state, 8),
        }
        .encode(format)
    }

    /// Serialize Markdown and its immutable image sidecars for a destination.
    /// The host must publish the bundle using the same snapshot revision.
    pub fn markdown_bundle(
        &self,
        destination: &std::path::Path,
    ) -> Result<suite42_core::session::SaveBundle, Error> {
        validate(self.state())?;
        if !self.state().bookmarks.is_empty() {
            return Err(Error::Unsupported("bookmark Markdown export".into()));
        }
        if !self.state().tables_of_contents.is_empty() {
            return Err(Error::Unsupported(
                "table of contents Markdown export is under development".into(),
            ));
        }
        if !self.state().odt_parts.is_empty() || !self.state().docx_parts.is_empty() {
            return Err(Error::Unsupported(
                "conversion would discard retained Office package parts".into(),
            ));
        }
        let stem = destination
            .file_stem()
            .and_then(|n| n.to_str())
            .filter(|n| !n.is_empty())
            .ok_or_else(|| Error::Invalid("invalid destination name".into()))?;
        let assets = format!("{stem}.assets");
        let (main, sidecars) = markdown::encode_resources(self.state(), Some(&assets))?;
        if main.len() > MAX_BYTES {
            return Err(Error::Limit);
        }
        Ok(suite42_core::session::SaveBundle { main, sidecars })
    }

    pub fn decode(bytes: &[u8], format: Format) -> Result<Self, Error> {
        if bytes.len() > MAX_BYTES {
            return Err(Error::Limit);
        }
        let state = match format {
            Format::Project => {
                let p: Project =
                    serde_json::from_slice(bytes).map_err(|e| Error::Invalid(e.to_string()))?;
                if p.magic != "DOC42" || p.version != 1 {
                    return Err(Error::Invalid("unknown project version".into()));
                }
                p.state
            }
            Format::Docx => docx::decode(bytes)?,
            Format::Odt => odt::decode(bytes)?,
            Format::Html => html::decode(bytes)?,
            Format::Markdown => markdown::decode(bytes)?,
            Format::Rtf => rtf_read::decode(bytes)?,
            Format::Text => {
                let text = std::str::from_utf8(bytes).map_err(|e| Error::Invalid(e.to_string()))?;
                let mut s = State {
                    text_policy: TextValue::from_raw(text.to_owned()),
                    ..State::default()
                };
                s.blocks = text
                    .replace("\r\n", "\n")
                    .split('\n')
                    .enumerate()
                    .map(|(i, t)| {
                        Block::Paragraph(Paragraph {
                            id: i as u64 + 1,
                            style: ParagraphStyle::default(),
                            runs: vec![Run {
                                text: t.into(),
                                style: TextStyle::default(),
                                link: None,
                            }],
                        })
                    })
                    .collect();
                s.next_id = s.blocks.len() as u64 + 1;
                s
            }
        };
        validate(&state)?;
        Ok(Self {
            history: SnapshotHistory::new(state, 8),
        })
    }
    /// Resolve relative images for HTML/Markdown using a host-controlled local resource policy.
    pub fn decode_with_resources(
        bytes: &[u8],
        format: Format,
        resolver: &dyn Fn(&str) -> Result<Vec<u8>, Error>,
    ) -> Result<Self, Error> {
        if bytes.len() > MAX_BYTES {
            return Err(Error::Limit);
        }
        let state = match format {
            Format::Html => html::decode_with(bytes, Some(resolver))?,
            Format::Markdown => markdown::decode_with(bytes, Some(resolver))?,
            _ => return Self::decode(bytes, format),
        };
        validate(&state)?;
        Ok(Self {
            history: SnapshotHistory::new(state, 8),
        })
    }
    pub fn encode(&self, format: Format) -> Result<Vec<u8>, Error> {
        validate(self.state())?;
        if !self.state().bookmarks.is_empty()
            && !matches!(format, Format::Project | Format::Odt | Format::Docx)
        {
            return Err(Error::Unsupported("bookmark export for this format".into()));
        }
        if !self.state().tables_of_contents.is_empty()
            && !matches!(format, Format::Project | Format::Odt | Format::Docx)
        {
            return Err(Error::Unsupported(
                "table of contents format export is under development".into(),
            ));
        }
        if !self.state().odt_parts.is_empty() && !matches!(format, Format::Project | Format::Odt) {
            return Err(Error::Unsupported(
                "conversion would discard retained OpenDocument package parts".into(),
            ));
        }
        if !self.state().docx_parts.is_empty() && !matches!(format, Format::Project | Format::Docx)
        {
            return Err(Error::Unsupported(
                "conversion would discard retained Word package parts".into(),
            ));
        }
        let bytes = match format {
            Format::Project => serde_json::to_vec(&Project {
                magic: "DOC42".into(),
                version: 1,
                state: self.state().clone(),
            })
            .map_err(|e| Error::Invalid(e.to_string()))?,
            Format::Text => {
                if !plain(self.state()) {
                    return Err(Error::Unsupported(
                        "plain text would discard document content or formatting".into(),
                    ));
                }
                let text = self
                    .state()
                    .blocks
                    .iter()
                    .map(|b| match b {
                        Block::Paragraph(p) => {
                            p.runs.iter().map(|r| r.text.as_str()).collect::<String>()
                        }
                        _ => unreachable!(),
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                self.state()
                    .text_policy
                    .with_edited(&text)
                    .raw()
                    .as_bytes()
                    .to_vec()
            }
            Format::Rtf => rtf::rtf_export(self.state())?,
            Format::Docx => docx::encode(self.state())?,
            Format::Odt => odt::encode(self.state())?,
            Format::Html => html::encode(self.state())?,
            Format::Markdown => markdown::encode(self.state())?,
        };
        if bytes.len() > MAX_BYTES {
            return Err(Error::Limit);
        }
        Ok(bytes)
    }
}
fn plain(s: &State) -> bool {
    s.docx_parts.is_empty()
        && s.odt_parts.is_empty()
        && s.sections.len() == 1
        && s.sections[0] == State::default().sections[0]
        && s.comments.is_empty()
        && s.footnotes.is_empty()
        && s.revisions.is_empty()
        && s.fields.is_empty()
        && s.bookmarks.is_empty()
        && s.tables_of_contents.is_empty()
        && s.blocks.iter().all(|b| match b {
            Block::Paragraph(p) => {
                p.style == ParagraphStyle::default()
                    && p.runs
                        .iter()
                        .all(|r| r.style == TextStyle::default() && r.link.is_none())
            }
            _ => false,
        })
}
fn validate(s: &State) -> Result<(), Error> {
    if s.blocks.is_empty()
        || s.blocks.len() > MAX_BLOCKS
        || s.sections.is_empty()
        || s.sections[0].start_block != 0
    {
        return Err(Error::Invalid("invalid blocks or sections".into()));
    }
    let mut previous = None;
    for sec in &s.sections {
        let p = &sec.page;
        if sec.start_block >= s.blocks.len()
            || previous.is_some_and(|n| n >= sec.start_block)
            || p.columns == 0
            || p.columns > 3
            || p.margin_left.saturating_add(p.margin_right) >= p.width_twips
            || p.margin_top.saturating_add(p.margin_bottom) >= p.height_twips
        {
            return Err(Error::Invalid("invalid section geometry".into()));
        }
        previous = Some(sec.start_block);
    }
    let mut ids = std::collections::HashSet::new();
    let mut paragraphs = std::collections::HashMap::new();
    fn check_paragraph(
        p: &Paragraph,
        ids: &mut std::collections::HashSet<u64>,
        paragraphs: &mut std::collections::HashMap<u64, String>,
    ) -> Result<(), Error> {
        if p.id == 0 || !ids.insert(p.id) {
            return Err(Error::Invalid("duplicate paragraph ID".into()));
        }
        if p.style.tabs_twips.len() > 64
            || p.style.tabs_twips.iter().any(|v| *v < 0 || *v > 100_000)
            || p.style.tabs_twips.windows(2).any(|w| w[0] >= w[1])
        {
            return Err(Error::Invalid("invalid tab stops".into()));
        }
        let text: String = p.runs.iter().map(|r| r.text.as_str()).collect();
        if text.contains(['\r', '\n', '\0']) || p.runs.iter().any(|r| r.style.size_half_points == 0)
        {
            return Err(Error::Invalid("invalid paragraph run".into()));
        }
        paragraphs.insert(p.id, text);
        Ok(())
    }
    for block in &s.blocks {
        match block {
            Block::Paragraph(p) => check_paragraph(p, &mut ids, &mut paragraphs)?,
            Block::Table(t) => {
                if t.id == 0 || !ids.insert(t.id) || t.rows.is_empty() {
                    return Err(Error::Invalid("invalid table".into()));
                }
                let width = t.rows[0].len();
                if width == 0 || t.rows.iter().any(|r| r.len() != width) {
                    return Err(Error::Invalid("non-rectangular table".into()));
                }
                if !t.column_widths_twips.is_empty()
                    && (t.column_widths_twips.len() != width
                        || t.column_widths_twips
                            .iter()
                            .any(|w| *w == 0 || *w > 2_000_000)
                        || t.column_widths_twips
                            .iter()
                            .map(|w| u64::from(*w))
                            .sum::<u64>()
                            > i32::MAX as u64)
                {
                    return Err(Error::Invalid("invalid table column widths".into()));
                }
                for row in &t.rows {
                    for cell in row {
                        if cell.row_span == 0 || cell.col_span == 0 {
                            return Err(Error::Invalid("invalid cell span".into()));
                        }
                        for p in &cell.paragraphs {
                            check_paragraph(p, &mut ids, &mut paragraphs)?;
                        }
                    }
                }
            }
            Block::Image(i) => {
                if i.id == 0 || !ids.insert(i.id) || i.width_twips == 0 || i.height_twips == 0 {
                    return Err(Error::Invalid("invalid image".into()));
                }
            }
            Block::SectionBreak => {}
        }
    }
    for sec in &s.sections {
        for p in sec.header.iter().chain(&sec.footer) {
            check_paragraph(p, &mut ids, &mut paragraphs)?;
        }
    }
    for note in &s.footnotes {
        for p in &note.paragraphs {
            check_paragraph(p, &mut ids, &mut paragraphs)?;
        }
    }
    for id in s
        .comments
        .iter()
        .map(|x| x.id)
        .chain(s.footnotes.iter().map(|x| x.id))
        .chain(s.revisions.iter().map(|x| x.id))
        .chain(s.fields.iter().map(|x| x.id))
        .chain(s.bookmarks.iter().map(|x| x.id))
    {
        if id == 0 || !ids.insert(id) {
            return Err(Error::Invalid("duplicate annotation or field ID".into()));
        }
    }
    let valid_position = |p: &Position| {
        paragraphs
            .get(&p.paragraph)
            .is_some_and(|t| p.offset <= t.len() && t.is_char_boundary(p.offset))
    };
    if s.selection
        .anchor
        .iter()
        .chain(s.selection.focus.iter())
        .any(|p| !valid_position(p))
        || s.comments
            .iter()
            .any(|x| !valid_position(&x.start) || !valid_position(&x.end))
        || s.revisions
            .iter()
            .any(|x| !valid_position(&x.start) || !valid_position(&x.end))
        || s.footnotes.iter().any(|x| !valid_position(&x.at))
        || s.fields.iter().any(|x| !valid_position(&x.at))
        || s.bookmarks
            .iter()
            .any(|x| !valid_position(&x.start) || !valid_position(&x.end))
    {
        return Err(Error::Invalid("invalid text position".into()));
    }
    let mut bookmark_names = std::collections::HashSet::new();
    for bookmark in &s.bookmarks {
        if bookmark.name.is_empty()
            || bookmark.name.len() > 255
            || bookmark.name.chars().any(char::is_control)
            || !bookmark_names.insert(&bookmark.name)
        {
            return Err(Error::Invalid("invalid or duplicate bookmark name".into()));
        }
    }
    let body_paragraphs: std::collections::HashSet<u64> = s
        .blocks
        .iter()
        .filter_map(|b| match b {
            Block::Paragraph(p) => Some(p.id),
            _ => None,
        })
        .collect();
    let mut toc_entries = std::collections::HashSet::new();
    for toc in &s.tables_of_contents {
        if toc
            .docx_cached
            .as_ref()
            .is_some_and(|source| source.len() != toc.entries.len())
        {
            return Err(Error::Invalid("invalid cached TOC".into()));
        }
        if toc.id == 0
            || !ids.insert(toc.id)
            || toc.entries.is_empty()
            || toc.min_level > toc.max_level
            || toc.max_level > 9
        {
            return Err(Error::Invalid("invalid table of contents identity".into()));
        }
        if let Some(cache) = &toc.odt_cached {
            let mut ids = cache
                .title_paragraphs
                .iter()
                .chain(toc.entries.iter().map(|e| &e.paragraph));
            let mut previous = None;
            for id in &mut ids {
                let index = s
                    .blocks
                    .iter()
                    .position(|b| matches!(b, Block::Paragraph(p) if p.id == *id))
                    .ok_or_else(|| Error::Invalid("ODT index title paragraph".into()))?;
                if previous.is_some_and(|p| index != p + 1) {
                    return Err(Error::Invalid("non-contiguous ODT index".into()));
                }
                previous = Some(index);
            }
            if cache.title_xml.is_some() != !cache.title_paragraphs.is_empty() {
                return Err(Error::Invalid("ODT index title".into()));
            }
        }
        let positions = toc
            .entries
            .iter()
            .map(|e| {
                s.blocks
                    .iter()
                    .position(|b| matches!(b, Block::Paragraph(p) if p.id == e.paragraph))
            })
            .collect::<Option<Vec<_>>>();
        if positions
            .as_ref()
            .is_none_or(|v| v.windows(2).any(|w| w[1] != w[0] + 1))
        {
            return Err(Error::Invalid("non-contiguous table of contents".into()));
        }
        for e in &toc.entries {
            if e.paragraph == e.heading
                || !body_paragraphs.contains(&e.paragraph)
                || !body_paragraphs.contains(&e.heading)
                || !toc_entries.insert(e.paragraph)
            {
                return Err(Error::Invalid("invalid table of contents entry".into()));
            }
        }
    }
    if ids.iter().any(|id| *id >= s.next_id) {
        return Err(Error::Invalid("invalid next ID".into()));
    }
    if serde_json::to_vec(s)
        .map_err(|e| Error::Invalid(e.to_string()))?
        .len()
        > MAX_BYTES
    {
        return Err(Error::Limit);
    }
    Ok(())
}

impl RichDocument {
    /// Replace a UTF-8 range within one paragraph as a single history entry.
    pub fn replace_text(
        &mut self,
        paragraph: u64,
        range: std::ops::Range<usize>,
        text: &str,
        style: TextStyle,
        selection: Selection,
    ) -> Result<bool, Error> {
        self.transact(|s| {
            if cached_toc_paragraph(s, paragraph) {
                return Err(Error::Unsupported(
                    "update the imported TOC before editing its cached entries".into(),
                ));
            }
            if s.revisions
                .iter()
                .any(|x| x.start.paragraph == paragraph || x.end.paragraph == paragraph)
                || s.fields.iter().any(|x| x.at.paragraph == paragraph)
            {
                return Err(Error::Unsupported(
                    "editing revision or field anchors".into(),
                ));
            }
            let p = paragraph_mut(s, paragraph)
                .ok_or_else(|| Error::Invalid("paragraph not found".into()))?;
            let length: usize = p.runs.iter().map(|r| r.text.len()).sum();
            if range.start > range.end || range.end > length {
                return Err(Error::Invalid("range outside paragraph".into()));
            }
            let all: String = p.runs.iter().map(|r| r.text.as_str()).collect();
            if !all.is_char_boundary(range.start) || !all.is_char_boundary(range.end) {
                return Err(Error::Invalid("range splits UTF-8".into()));
            }
            let mut out = Vec::new();
            let mut pos = 0;
            for run in &p.runs {
                let end = pos + run.text.len();
                if pos < range.start {
                    let n = range.start.min(end) - pos;
                    if n > 0 {
                        let mut r = run.clone();
                        r.text = run.text[..n].into();
                        out.push(r);
                    }
                }
                pos = end;
            }
            if !text.is_empty() {
                out.push(Run {
                    text: text.into(),
                    style,
                    link: None,
                });
            }
            pos = 0;
            for run in &p.runs {
                let end = pos + run.text.len();
                if end > range.end {
                    let n = range.end.saturating_sub(pos);
                    let mut r = run.clone();
                    r.text = run.text[n..].into();
                    if !r.text.is_empty() {
                        out.push(r);
                    }
                }
                pos = end;
            }
            p.runs = out;
            annotations::remap_edit(s, paragraph, range, text.len());
            s.selection = selection;
            Ok(())
        })
    }
    pub fn set_paragraph_style(
        &mut self,
        paragraph: u64,
        style: ParagraphStyle,
        selection: Selection,
    ) -> Result<bool, Error> {
        self.transact(|s| {
            if cached_toc_paragraph(s, paragraph) {
                return Err(Error::Unsupported(
                    "update the imported TOC before editing its cached entries".into(),
                ));
            }
            paragraph_mut(s, paragraph)
                .ok_or_else(|| Error::Invalid("paragraph not found".into()))?
                .style = style;
            s.selection = selection;
            Ok(())
        })
    }
}
fn cached_toc_paragraph(s: &State, id: u64) -> bool {
    s.tables_of_contents.iter().any(|toc| {
        toc.odt_cached
            .as_ref()
            .is_some_and(|c| c.title_paragraphs.contains(&id))
            || ((toc.docx_cached.is_some()
                || toc.odt_cached.as_ref().is_some_and(|c| !c.needs_refresh))
                && toc.entries.iter().any(|e| e.paragraph == id))
    })
}
fn paragraph_mut(s: &mut State, id: u64) -> Option<&mut Paragraph> {
    for block in &mut s.blocks {
        match block {
            Block::Paragraph(p) if p.id == id => return Some(p),
            Block::Table(t) => {
                for row in &mut t.rows {
                    for cell in row {
                        for p in &mut cell.paragraphs {
                            if p.id == id {
                                return Some(p);
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }
    None
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn project_history_and_text_policy() {
        let mut d = RichDocument::decode(b"one\r\ntwo", Format::Text).unwrap();
        assert_eq!(d.encode(Format::Text).unwrap(), b"one\r\ntwo");
        let style = TextStyle {
            bold: true,
            ..TextStyle::default()
        };
        d.replace_text(1, 0..3, "ONE", style, Selection::default())
            .unwrap();
        assert!(d.encode(Format::Text).is_err());
        let bytes = d.encode(Format::Project).unwrap();
        let copy = RichDocument::decode(&bytes, Format::Project).unwrap();
        assert_eq!(copy.state(), d.state());
        assert!(d.undo());
        assert_eq!(d.encode(Format::Text).unwrap(), b"one\r\ntwo");
        assert!(d.redo());
        assert_eq!(d.state(), copy.state());
    }
    #[test]
    fn rtf_exports_styled_unicode_and_rejects_loss() {
        let mut d = RichDocument::decode("é 😀".as_bytes(), Format::Text).unwrap();
        d.replace_text(
            1,
            0..2,
            "é",
            TextStyle {
                bold: true,
                ..TextStyle::default()
            },
            Selection::default(),
        )
        .unwrap();
        let rtf = String::from_utf8(d.encode(Format::Rtf).unwrap()).unwrap();
        assert!(rtf.starts_with("{\\rtf1"));
        assert!(rtf.contains("\\b1"));
        assert!(rtf.contains("\\u233?"));
        d.transact(|s| {
            if let Block::Paragraph(p) = &mut s.blocks[0] {
                p.style.list = Some(ListMarker {
                    id: 1,
                    level: 0,
                    numbered: true,
                });
            }
            Ok(())
        })
        .unwrap();
        let listed = RichDocument::decode(&d.encode(Format::Rtf).unwrap(), Format::Rtf).unwrap();
        assert!(matches!(&listed.state().blocks[0],Block::Paragraph(p) if p.style.list.is_some()));
        d.transact(|s| {
            if let Block::Paragraph(p) = &mut s.blocks[0] {
                p.style.source_style = Some("Heading".into());
            }
            Ok(())
        })
        .unwrap();
        assert!(matches!(d.encode(Format::Rtf), Err(Error::Unsupported(_))));
    }
    #[test]
    fn rich_projection_preserves_mixed_named_styles_after_edit() {
        let mut doc = RichDocument::new();
        doc.transact(|s| {
            let Block::Paragraph(p) = &mut s.blocks[0] else {
                panic!()
            };
            p.runs = vec![
                Run {
                    text: "one".into(),
                    style: TextStyle {
                        source_style: Some("A".into()),
                        ..TextStyle::default()
                    },
                    link: None,
                },
                Run {
                    text: "two".into(),
                    style: TextStyle {
                        source_style: Some("B".into()),
                        ..TextStyle::default()
                    },
                    link: None,
                },
            ];
            Ok(())
        })
        .unwrap();
        let rtf = String::from_utf8(doc.rich_edit_projection().unwrap())
            .unwrap()
            .replace("one", "ones");
        doc.apply_rich_edit_projection(rtf.as_bytes(), (4, 4))
            .unwrap();
        let Block::Paragraph(p) = &doc.state().blocks[0] else {
            panic!()
        };
        assert_eq!(
            p.runs.iter().map(|r| r.text.as_str()).collect::<String>(),
            "onestwo"
        );
        assert!(p
            .runs
            .iter()
            .any(|r| r.text.contains("two") && r.style.source_style.as_deref() == Some("B")));
        assert_eq!(p.runs[0].style.source_style.as_deref(), Some("A"));
    }
    #[test]
    fn replace_preserves_runs_and_undo() {
        let mut d = RichDocument::decode(b"abc abc", Format::Text).unwrap();
        d.apply_text_style(
            1,
            0..3,
            TextStyle {
                bold: true,
                ..TextStyle::default()
            },
            Selection::default(),
        )
        .unwrap();
        assert_eq!(d.replace_all_text("abc", "X").unwrap(), 2);
        let Block::Paragraph(p) = &d.state().blocks[0] else {
            panic!()
        };
        assert_eq!(
            p.runs.iter().map(|r| r.text.as_str()).collect::<String>(),
            "X X"
        );
        assert!(p.runs[0].style.bold);
        assert!(!p.runs.last().unwrap().style.bold);
        assert!(d.undo());
        assert_eq!(d.editor_text().unwrap(), "abc abc");
    }
    #[test]
    fn utf16_positions_and_saved_snapshot() {
        let mut d = RichDocument::decode("😀a\r\nb".as_bytes(), Format::Text).unwrap();
        let p = d.position_from_utf16(2).unwrap();
        assert_eq!(p.offset, 4);
        assert_eq!(d.position_to_utf16(&p).unwrap(), 2);
        assert!(d.position_from_utf16(1).is_err());
        assert!(d.position_from_utf16(4).is_err());
        let saved = d.snapshot();
        d.replace_text(1, 0..4, "x", TextStyle::default(), Selection::default())
            .unwrap();
        d.mark_saved_snapshot(saved).unwrap();
        assert!(d.is_dirty());
        assert!(d.undo());
        assert!(!d.is_dirty());
    }
    #[test]
    fn failed_transaction_preserves_history() {
        let mut d = RichDocument::new();
        assert!(d
            .transact(|s| {
                s.sections[0].page.margin_left = u32::MAX;
                Ok(())
            })
            .is_err());
        assert!(!d.can_undo());
    }
}

impl RichDocument {
    /// Translate an edit-control offset in UTF-16, with CRLF paragraph separators.
    pub fn position_from_utf16(&self, offset: usize) -> Result<Position, Error> {
        let mut cursor = 0usize;
        let blocks = &self.state().blocks;
        for (index, block) in blocks.iter().enumerate() {
            let Block::Paragraph(p) = block else {
                return Err(Error::Unsupported(
                    "linear selection over non-paragraph blocks".into(),
                ));
            };
            let text: String = p.runs.iter().map(|r| r.text.as_str()).collect();
            for (byte, c) in text.char_indices() {
                if cursor == offset {
                    return Ok(Position {
                        paragraph: p.id,
                        offset: byte,
                    });
                }
                let next = cursor + c.len_utf16();
                if offset < next {
                    return Err(Error::Invalid(
                        "selection splits UTF-16 surrogate pair".into(),
                    ));
                }
                cursor = next;
            }
            if cursor == offset {
                return Ok(Position {
                    paragraph: p.id,
                    offset: text.len(),
                });
            }
            if index + 1 < blocks.len() {
                if offset == cursor + 1 {
                    return Err(Error::Invalid("selection splits CRLF".into()));
                }
                cursor += 2;
            }
        }
        Err(Error::Invalid("selection outside document".into()))
    }
    pub fn position_to_utf16(&self, position: &Position) -> Result<usize, Error> {
        let mut cursor = 0usize;
        for block in &self.state().blocks {
            let Block::Paragraph(p) = block else {
                return Err(Error::Unsupported(
                    "linear selection over non-paragraph blocks".into(),
                ));
            };
            let text: String = p.runs.iter().map(|r| r.text.as_str()).collect();
            if p.id == position.paragraph {
                if position.offset > text.len() || !text.is_char_boundary(position.offset) {
                    return Err(Error::Invalid("invalid position".into()));
                }
                return Ok(cursor + text[..position.offset].encode_utf16().count());
            }
            cursor += text.encode_utf16().count() + 2;
        }
        Err(Error::Invalid("paragraph not found".into()))
    }
}

impl RichDocument {
    /// Apply a character style without changing text positions or attached anchors.
    pub fn apply_text_style(
        &mut self,
        paragraph: u64,
        range: std::ops::Range<usize>,
        style: TextStyle,
        selection: Selection,
    ) -> Result<bool, Error> {
        self.transact(|s| {
            if cached_toc_paragraph(s, paragraph) {
                return Err(Error::Unsupported(
                    "update the imported TOC before editing its cached entries".into(),
                ));
            }
            let p = paragraph_mut(s, paragraph)
                .ok_or_else(|| Error::Invalid("paragraph not found".into()))?;
            let text: String = p.runs.iter().map(|r| r.text.as_str()).collect();
            if range.start > range.end
                || range.end > text.len()
                || !text.is_char_boundary(range.start)
                || !text.is_char_boundary(range.end)
            {
                return Err(Error::Invalid("invalid formatting range".into()));
            }
            if range.is_empty() {
                return Ok(());
            }
            let mut out = Vec::new();
            let mut pos = 0;
            for run in &p.runs {
                let end = pos + run.text.len();
                let left = range.start.saturating_sub(pos).min(run.text.len());
                let right = range.end.saturating_sub(pos).min(run.text.len());
                if left > 0 {
                    let mut part = run.clone();
                    part.text = run.text[..left].into();
                    out.push(part);
                }
                if right > left {
                    let mut part = run.clone();
                    part.text = run.text[left..right].into();
                    part.style = style.clone();
                    out.push(part);
                }
                if right < run.text.len() {
                    let mut part = run.clone();
                    part.text = run.text[right..].into();
                    out.push(part);
                }
                pos = end;
            }
            p.runs = out;
            s.selection = selection;
            Ok(())
        })
    }
}

impl RichDocument {
    /// Render a plain-text projection. Non-text blocks cannot be represented by EDIT.
    pub fn editor_text(&self) -> Result<String, Error> {
        let mut lines = Vec::new();
        for b in &self.state().blocks {
            match b {
                Block::Paragraph(p) => {
                    lines.push(p.runs.iter().map(|r| r.text.as_str()).collect::<String>())
                }
                _ => {
                    return Err(Error::Unsupported(
                        "text projection of tables or images".into(),
                    ))
                }
            }
        }
        let canonical = lines.join("\n");
        Ok(self
            .state()
            .text_policy
            .with_edited(&canonical)
            .raw()
            .to_owned())
    }
    /// The plain native editor may only mutate a document it can represent without loss.
    pub fn can_edit_plain(&self) -> bool {
        plain(self.state())
    }
    pub fn set_plain_editor_text(&mut self, text: &str) -> Result<bool, Error> {
        if !self.can_edit_plain() {
            return Err(Error::Unsupported(
                "rich document requires a rich editor".into(),
            ));
        }
        let text = text.replace("\r\n", "\n");
        if text.len() > 4 * 1024 * 1024 {
            return Err(Error::Limit);
        }
        if text.contains('\r') || text.contains('\0') {
            return Err(Error::Invalid("unsupported text character".into()));
        }
        self.transact(|s| {
            let mut blocks = Vec::new();
            for (i, line) in text.split('\n').enumerate() {
                let id = match s.blocks.get(i) {
                    Some(Block::Paragraph(p)) => p.id,
                    _ => {
                        let id = s.next_id;
                        s.next_id = s.next_id.checked_add(1).ok_or(Error::Limit)?;
                        id
                    }
                };
                blocks.push(Block::Paragraph(Paragraph {
                    id,
                    style: ParagraphStyle::default(),
                    runs: vec![Run {
                        text: line.into(),
                        style: TextStyle::default(),
                        link: None,
                    }],
                }));
            }
            s.blocks = blocks;
            s.selection = Selection::default();
            Ok(())
        })
    }
}

impl RichDocument {
    fn header_field_ids(&self) -> std::collections::HashSet<u64> {
        self.state()
            .sections
            .iter()
            .flat_map(|s| s.header.iter().chain(&s.footer))
            .map(|p| p.id)
            .collect()
    }
    /// Render an RTF view without exposing opaque package metadata to RichEdit.
    pub fn rich_edit_projection(&self) -> Result<Vec<u8>, Error> {
        let mut view = self.snapshot();
        view.docx_parts.clear();
        view.odt_parts.clear();
        view.odt_source_page = None;
        view.odt_source_headers = None;
        view.odt_source_header_fields = None;
        view.comments.clear();
        view.footnotes.clear();
        view.bookmarks.clear();
        let header_ids = self.header_field_ids();
        view.fields
            .retain(|f| !header_ids.contains(&f.at.paragraph));
        if view.sections.len() == 1 {
            view.sections[0].header.clear();
            view.sections[0].footer.clear();
        }
        for block in &mut view.blocks {
            if let Block::Image(image) = block {
                image.alt.clear();
            }
            if let Block::Table(table) = block {
                for row in &mut table.rows {
                    for cell in row {
                        for p in &mut cell.paragraphs {
                            p.style.source_style = None;
                            p.style.odt_extra.clear();
                            for r in &mut p.runs {
                                r.style.source_style = None;
                                r.style.theme_color = None;
                                r.style.theme_tint = None;
                                r.style.theme_shade = None;
                                r.style.odt_extra.clear();
                                r.style.odt_extra.clear();
                            }
                        }
                    }
                }
            }
            if let Block::Paragraph(p) = block {
                p.style.source_style = None;
                p.style.odt_extra.clear();
                for r in &mut p.runs {
                    r.style.source_style = None;
                    r.style.theme_color = None;
                    r.style.theme_tint = None;
                    r.style.theme_shade = None;
                    r.style.odt_extra.clear();
                }
            }
        }
        rtf::rtf_export(&view)
    }
    /// Apply an entire RichEdit projection as one transaction, retaining non-view metadata.
    pub fn apply_rich_edit_projection(
        &mut self,
        bytes: &[u8],
        selection: (usize, usize),
    ) -> Result<bool, Error> {
        let mut parsed = rtf_read::decode_projection(bytes)?;
        let old = self.state();
        if old.sections.len() != 1
            || !old.revisions.is_empty()
            || old
                .fields
                .iter()
                .any(|f| !self.header_field_ids().contains(&f.at.paragraph))
            || old.blocks.iter().any(|b| match b {
                Block::Paragraph(_) => false,
                Block::Image(_) => false,
                Block::Table(t) => table_edit::table_layout(t).is_err(),
                _ => true,
            })
        {
            return Err(Error::Unsupported(
                "rich editor cannot safely edit this document structure".into(),
            ));
        }
        let links = |s: &State| -> std::collections::HashSet<String> {
            s.blocks
                .iter()
                .flat_map(|b| match b {
                    Block::Paragraph(p) => vec![p],
                    Block::Table(t) => t
                        .rows
                        .iter()
                        .flat_map(|r| r.iter().flat_map(|c| c.paragraphs.iter()))
                        .collect(),
                    _ => vec![],
                })
                .flat_map(|p| p.runs.iter().filter_map(|r| r.link.clone()))
                .collect()
        };
        if !links(old).is_subset(&links(&parsed)) {
            return Err(Error::Unsupported(
                "RichEdit removed a hyperlink target".into(),
            ));
        }
        let mut matching: Vec<Option<usize>> = (0..parsed.blocks.len()).map(Some).collect();
        if !old.comments.is_empty()
            || !old.footnotes.is_empty()
            || !old.tables_of_contents.is_empty()
            || !old.bookmarks.is_empty()
        {
            matching.fill(None);
            let mut a = 0usize;
            let mut b = 0usize;
            loop {
                let old_object = (a..old.blocks.len())
                    .find(|i| !matches!(old.blocks[*i], Block::Paragraph(_)))
                    .unwrap_or(old.blocks.len());
                let new_object = (b..parsed.blocks.len())
                    .find(|i| !matches!(parsed.blocks[*i], Block::Paragraph(_)))
                    .unwrap_or(parsed.blocks.len());
                for offset in 0..(old_object - a).min(new_object - b) {
                    matching[b + offset] = Some(a + offset);
                }
                if old_object == old.blocks.len() && new_object == parsed.blocks.len() {
                    break;
                }
                if old_object == old.blocks.len() || new_object == parsed.blocks.len() {
                    return Err(Error::Unsupported(
                        "structural edits around annotated objects".into(),
                    ));
                }
                let compatible = matches!(
                    (&old.blocks[old_object], &parsed.blocks[new_object]),
                    (Block::Table(_), Block::Table(_)) | (Block::Image(_), Block::Image(_))
                );
                if !compatible {
                    return Err(Error::Unsupported(
                        "structural edits around annotated objects".into(),
                    ));
                }
                matching[new_object] = Some(old_object);
                a = old_object + 1;
                b = new_object + 1;
            }
        }
        let mut next_id = old.next_id;
        let mut used_images = std::collections::HashSet::new();
        for (i, block) in parsed.blocks.iter_mut().enumerate() {
            match block {
                Block::Paragraph(p) => {
                    if let Some(Block::Paragraph(previous)) =
                        matching[i].and_then(|index| old.blocks.get(index))
                    {
                        p.style.source_style = previous.style.source_style.clone();
                        p.style.odt_extra = previous.style.odt_extra.clone();
                        restore_run_metadata(&previous.runs, &mut p.runs)?;
                        p.id = previous.id;
                    } else {
                        p.id = next_id;
                        next_id = next_id.checked_add(1).ok_or(Error::Limit)?;
                    }
                }
                Block::Table(table) => {
                    let Some(Block::Table(previous)) =
                        matching[i].and_then(|index| old.blocks.get(index))
                    else {
                        return Err(Error::Unsupported(
                            "RichEdit inserted or moved a table".into(),
                        ));
                    };
                    if table.rows.len() != previous.rows.len()
                        || table
                            .rows
                            .iter()
                            .zip(&previous.rows)
                            .any(|(a, b)| a.len() != b.len())
                    {
                        return Err(Error::Unsupported("RichEdit changed table shape".into()));
                    }
                    if table_edit::table_layout(table)? != table_edit::table_layout(previous)? {
                        return Err(Error::Unsupported("RichEdit changed merged cells".into()));
                    }
                    if table_edit::column_widths(table) != table_edit::column_widths(previous) {
                        return Err(Error::Unsupported(
                            "RichEdit changed table column widths".into(),
                        ));
                    }
                    table.column_widths_twips = previous.column_widths_twips.clone();
                    table.id = previous.id;
                    for (row, oldrow) in table.rows.iter_mut().zip(&previous.rows) {
                        for (cell, oldcell) in row.iter_mut().zip(oldrow) {
                            if cell.fill != oldcell.fill || cell.border != oldcell.border {
                                return Err(Error::Unsupported(
                                    "RichEdit changed table cell formatting".into(),
                                ));
                            }
                            for (j, p) in cell.paragraphs.iter_mut().enumerate() {
                                if let Some(oldp) = oldcell.paragraphs.get(j) {
                                    p.id = oldp.id;
                                    p.style.source_style = oldp.style.source_style.clone();
                                    p.style.odt_extra = oldp.style.odt_extra.clone();
                                    restore_run_metadata(&oldp.runs, &mut p.runs)?;
                                } else {
                                    p.id = next_id;
                                    next_id = next_id.checked_add(1).ok_or(Error::Limit)?;
                                }
                            }
                        }
                    }
                }
                Block::Image(image) => {
                    let previous = old.blocks.iter().find_map(|b| match b {
                        Block::Image(old)
                            if !used_images.contains(&old.id)
                                && old.mime == image.mime
                                && old.data == image.data =>
                        {
                            Some(old)
                        }
                        _ => None,
                    });
                    if let Some(previous) = previous {
                        image.id = previous.id;
                        image.alt = previous.alt.clone();
                        used_images.insert(previous.id);
                    } else {
                        image.id = next_id;
                        next_id = next_id.checked_add(1).ok_or(Error::Limit)?;
                    }
                }
                _ => return Err(Error::Unsupported("RichEdit output block".into())),
            }
        }
        if old
            .blocks
            .iter()
            .filter_map(|b| {
                if let Block::Image(i) = b {
                    Some(i.id)
                } else {
                    None
                }
            })
            .any(|id| !used_images.contains(&id))
        {
            return Err(Error::Unsupported(
                "RichEdit removed or transformed a picture".into(),
            ));
        }
        parsed.next_id = next_id;
        parsed.sections = old.sections.clone();
        parsed.fields = old.fields.clone();
        parsed.text_policy = old.text_policy.clone();
        parsed.docx_parts = old.docx_parts.clone();
        parsed.odt_parts = old.odt_parts.clone();
        parsed.odt_source_page = old.odt_source_page.clone();
        parsed.odt_source_headers = old.odt_source_headers.clone();
        parsed.odt_source_header_fields = old.odt_source_header_fields.clone();
        let mut anchor_source = old.clone();
        for bookmark in &old.bookmarks {
            anchor_source.comments.push(Annotation {
                id: 0,
                start: bookmark.start.clone(),
                end: bookmark.end.clone(),
                author: String::new(),
                text: String::new(),
                resolved: false,
                date: None,
                initials: None,
            });
        }
        for toc in &old.tables_of_contents {
            if let Some(cache) = &toc.odt_cached {
                for paragraph in &cache.title_paragraphs {
                    let at = Position {
                        paragraph: *paragraph,
                        offset: 0,
                    };
                    anchor_source.comments.push(Annotation {
                        id: 0,
                        start: at.clone(),
                        end: at,
                        author: String::new(),
                        text: String::new(),
                        resolved: false,
                        date: None,
                        initials: None,
                    });
                }
            }
            for entry in &toc.entries {
                for paragraph in [entry.paragraph, entry.heading] {
                    let at = Position {
                        paragraph,
                        offset: 0,
                    };
                    anchor_source.comments.push(Annotation {
                        id: 0,
                        start: at.clone(),
                        end: at,
                        author: String::new(),
                        text: String::new(),
                        resolved: false,
                        date: None,
                        initials: None,
                    });
                }
            }
        }
        annotations::remap_projection(&anchor_source, &mut parsed)?;
        let mapped = parsed.comments.split_off(old.comments.len());
        let mut mapped = mapped.into_iter();
        parsed.bookmarks = old.bookmarks.clone();
        for bookmark in &mut parsed.bookmarks {
            let mapped = mapped
                .next()
                .ok_or_else(|| Error::Invalid("bookmark anchor".into()))?;
            bookmark.start = mapped.start;
            bookmark.end = mapped.end;
        }
        parsed.tables_of_contents = old.tables_of_contents.clone();
        for toc in &mut parsed.tables_of_contents {
            if let Some(cache) = &mut toc.odt_cached {
                for paragraph in &mut cache.title_paragraphs {
                    *paragraph = mapped
                        .next()
                        .ok_or_else(|| Error::Invalid("ODT index title anchor".into()))?
                        .start
                        .paragraph;
                }
            }
            for entry in &mut toc.entries {
                entry.paragraph = mapped
                    .next()
                    .ok_or_else(|| Error::Invalid("TOC anchor".into()))?
                    .start
                    .paragraph;
                entry.heading = mapped
                    .next()
                    .ok_or_else(|| Error::Invalid("TOC heading anchor".into()))?
                    .start
                    .paragraph;
            }
        }
        // An unchanged heading moved as a block retains its identity by its unique text.
        // If a replacement removes its heading style entirely, reject the ambiguous edit.
        for (before_toc, after_toc) in old
            .tables_of_contents
            .iter()
            .zip(&mut parsed.tables_of_contents)
        {
            for (before_entry, after_entry) in before_toc.entries.iter().zip(&mut after_toc.entries)
            {
                let original = old
                    .blocks
                    .iter()
                    .find_map(|b| match b {
                        Block::Paragraph(p) if p.id == before_entry.heading => {
                            Some(p.runs.iter().map(|r| r.text.as_str()).collect::<String>())
                        }
                        _ => None,
                    })
                    .ok_or_else(|| Error::Invalid("TOC heading".into()))?;
                let exact = parsed
                    .blocks
                    .iter()
                    .filter_map(|b| match b {
                        Block::Paragraph(p)
                            if p.style.outline_level.is_some()
                                && p.runs.iter().map(|r| r.text.as_str()).collect::<String>()
                                    == original =>
                        {
                            Some(p.id)
                        }
                        _ => None,
                    })
                    .collect::<Vec<_>>();
                if exact.len() == 1 {
                    after_entry.heading = exact[0];
                }
                if !parsed.blocks.iter().any(|b| matches!(b, Block::Paragraph(p) if p.id == after_entry.heading && p.style.outline_level.is_some())) {
                    return Err(Error::Unsupported("editing removed a TOC heading; update or remove the table of contents first".into()));
                }
            }
        }
        for (before_toc, after_toc) in old
            .tables_of_contents
            .iter()
            .zip(&parsed.tables_of_contents)
        {
            if let (Some(a), Some(b)) = (&before_toc.odt_cached, &after_toc.odt_cached) {
                for (&before, &after) in a.title_paragraphs.iter().zip(&b.title_paragraphs) {
                    let find = |state: &State, id| {
                        state.blocks.iter().find_map(|block| match block {
                            Block::Paragraph(p) if p.id == id => {
                                Some((p.style.clone(), p.runs.clone()))
                            }
                            _ => None,
                        })
                    };
                    if find(old, before) != find(&parsed, after) {
                        return Err(Error::Unsupported(
                            "update the imported TOC before editing its cached title".into(),
                        ));
                    }
                }
            }
            if before_toc.docx_cached.is_none()
                && before_toc
                    .odt_cached
                    .as_ref()
                    .is_none_or(|c| c.needs_refresh)
            {
                continue;
            }
            for (before_entry, after_entry) in before_toc.entries.iter().zip(&after_toc.entries) {
                fn find(state: &State, id: u64) -> Option<&Paragraph> {
                    state.blocks.iter().find_map(|b| match b {
                        Block::Paragraph(p) if p.id == id => Some(p),
                        _ => None,
                    })
                }
                let a = find(old, before_entry.paragraph)
                    .ok_or_else(|| Error::Invalid("cached TOC entry".into()))?;
                let b = find(&parsed, after_entry.paragraph)
                    .ok_or_else(|| Error::Invalid("cached TOC entry".into()))?;
                fn same_contents(a: &Paragraph, b: &Paragraph) -> bool {
                    a.runs
                        .iter()
                        .flat_map(|r| r.text.chars().map(move |c| (c, &r.style)))
                        .eq(b
                            .runs
                            .iter()
                            .flat_map(|r| r.text.chars().map(move |c| (c, &r.style))))
                }
                if a.style != b.style || !same_contents(a, b) {
                    return Err(Error::Unsupported(
                        "update the imported TOC before editing its cached entries".into(),
                    ));
                }
            }
        }
        let toc_links = parsed
            .tables_of_contents
            .iter()
            .flat_map(|t| t.entries.iter())
            .map(|e| (e.paragraph, e.heading))
            .collect::<Vec<_>>();
        for (paragraph, heading) in toc_links {
            let p = paragraph_mut(&mut parsed, paragraph)
                .ok_or_else(|| Error::Invalid("TOC paragraph".into()))?;
            for run in &mut p.runs {
                run.link = Some(format!("#DOC42_H{heading}"));
            }
        }
        validate(&parsed)?;
        let temp = Self {
            history: SnapshotHistory::new(parsed.clone(), 0),
        };
        parsed.selection = Selection {
            anchor: temp.position_from_utf16(selection.0).ok(),
            focus: temp.position_from_utf16(selection.1).ok(),
        };
        fn normalized(mut s: State) -> State {
            s.selection = Selection::default();
            fn paragraph(p: &mut Paragraph) {
                p.runs.retain(|r| {
                    !r.text.is_empty() || r.style != TextStyle::default() || r.link.is_some()
                });
            }
            for block in &mut s.blocks {
                match block {
                    Block::Paragraph(p) => paragraph(p),
                    Block::Table(t) => {
                        for row in &mut t.rows {
                            for cell in row {
                                for p in &mut cell.paragraphs {
                                    paragraph(p)
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
            s
        }
        if normalized(parsed.clone()) == normalized(old.clone()) {
            return Ok(false);
        }
        self.transact(|s| {
            *s = parsed;
            Ok(())
        })
    }
}

impl RichDocument {
    /// Replace literal occurrences across paragraphs as one transaction, retaining run formatting.
    pub fn replace_all_text(&mut self, query: &str, replacement: &str) -> Result<usize, Error> {
        if query.is_empty() {
            return Ok(0);
        }
        if query.contains(['\r', '\n']) || replacement.contains(['\r', '\n']) {
            if !self.can_edit_plain() {
                return Err(Error::Unsupported(
                    "replacement across rich paragraphs".into(),
                ));
            }
            let text = self.editor_text()?.replace("\r\n", "\n");
            let query = query.replace("\r\n", "\n");
            let replacement = replacement.replace("\r\n", "\n");
            let count = text.matches(&query).count();
            if count > 0 {
                self.set_plain_editor_text(&text.replace(&query, &replacement))?;
            }
            return Ok(count);
        }
        let mut count = 0;
        self.transact(|s| {
            if !s.comments.is_empty()
                || !s.footnotes.is_empty()
                || !s.revisions.is_empty()
                || !s.fields.is_empty()
            {
                return Err(Error::Unsupported(
                    "replacement requires annotation remapping".into(),
                ));
            }
            for block in &mut s.blocks {
                let Block::Paragraph(p) = block else {
                    return Err(Error::Unsupported("replacement in non-text blocks".into()));
                };
                let text: String = p.runs.iter().map(|r| r.text.as_str()).collect();
                let matches: Vec<usize> = text.match_indices(query).map(|(i, _)| i).collect();
                if matches.is_empty() {
                    continue;
                }
                count += matches.len();
                let mut output = Vec::new();
                let mut cursor = 0;
                for start in matches {
                    append_run_slice(&p.runs, cursor..start, &mut output);
                    if !replacement.is_empty() {
                        let mut pos = 0;
                        let template = p
                            .runs
                            .iter()
                            .find(|r| {
                                let end = pos + r.text.len();
                                let hit = start >= pos && start < end;
                                pos = end;
                                hit
                            })
                            .ok_or_else(|| Error::Invalid("replace position".into()))?;
                        let mut run = template.clone();
                        run.text = replacement.into();
                        output.push(run);
                    }
                    cursor = start + query.len();
                }
                append_run_slice(&p.runs, cursor..text.len(), &mut output);
                p.runs = output;
            }
            if count > 0 {
                s.selection = Selection::default();
            }
            Ok(())
        })?;
        Ok(count)
    }
}
fn append_run_slice(runs: &[Run], range: std::ops::Range<usize>, out: &mut Vec<Run>) {
    let mut start = 0;
    for r in runs {
        let end = start + r.text.len();
        let a = range.start.max(start);
        let b = range.end.min(end);
        if a < b {
            let mut part = r.clone();
            part.text = r.text[a - start..b - start].into();
            out.push(part);
        }
        start = end;
    }
}

fn restore_run_metadata(old: &[Run], new: &mut Vec<Run>) -> Result<(), Error> {
    let old_text: String = old.iter().map(|r| r.text.as_str()).collect();
    let new_text: String = new.iter().map(|r| r.text.as_str()).collect();
    let mut prefix = 0usize;
    for (a, b) in old_text.chars().zip(new_text.chars()) {
        if a != b {
            break;
        }
        prefix += a.len_utf8();
    }
    let mut suffix = 0usize;
    for (a, b) in old_text[prefix..]
        .chars()
        .rev()
        .zip(new_text[prefix..].chars().rev())
    {
        if a != b {
            break;
        }
        suffix += a.len_utf8();
    }
    let old_suffix = old_text.len() - suffix;
    let new_suffix = new_text.len() - suffix;
    let mut spans = Vec::new();
    let mut pos = 0;
    for r in old {
        let end = pos + r.text.len();
        if end > pos {
            spans.push((pos, end, &r.style));
        }
        pos = end;
    }
    let at = |index: usize| -> Option<&TextStyle> {
        let i = spans.partition_point(|(_, end, _)| *end <= index);
        spans
            .get(i)
            .and_then(|(start, end, s)| (*start <= index && index < *end).then_some(*s))
    };
    let visual = |s: &TextStyle| {
        let mut s = s.clone();
        s.source_style = None;
        s.theme_color = None;
        s.theme_tint = None;
        s.theme_shade = None;
        s.odt_extra.clear();
        s
    };
    let mut output: Vec<Run> = Vec::new();
    let mut offset = 0usize;
    for run in std::mem::take(new) {
        let target_visual = visual(&run.style);
        let fallback = at(prefix)
            .or_else(|| prefix.checked_sub(1).and_then(at))
            .filter(|s| visual(s) == target_visual)
            .or_else(|| {
                spans
                    .iter()
                    .map(|x| x.2)
                    .find(|s| visual(s) == target_visual)
            });
        for c in run.text.chars() {
            let old_pos = if offset < prefix {
                Some(offset)
            } else if offset >= new_suffix {
                Some(old_suffix + offset - new_suffix)
            } else {
                None
            };
            let source = old_pos.and_then(at).or(fallback);
            let mut style = run.style.clone();
            if let Some(source) = source {
                style.source_style = source.source_style.clone();
                style.odt_extra = source.odt_extra.clone();
                if style.color == source.color {
                    style.theme_color = source.theme_color.clone();
                    style.theme_tint = source.theme_tint.clone();
                    style.theme_shade = source.theme_shade.clone();
                }
            }
            if let Some(last) = output
                .last_mut()
                .filter(|last| last.style == style && last.link == run.link)
            {
                last.text.push(c);
            } else {
                output.push(Run {
                    text: c.to_string(),
                    style,
                    link: run.link.clone(),
                });
            }
            offset += c.len_utf8();
        }
    }
    *new = output;
    Ok(())
}
