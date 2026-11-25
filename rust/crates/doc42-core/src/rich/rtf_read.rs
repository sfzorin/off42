//! Strict RTF reader for plain/styled paragraphs and page settings.
use super::*;
use std::collections::HashMap;
#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Destination {
    #[default]
    Body,
    Header,
    Footer,
}
#[derive(Clone, Default)]
struct Group {
    destination: Destination,
    style: TextStyle,
    link: Option<String>,
    font_table: bool,
    font_id: Option<i32>,
    font_charset: Option<i32>,
    font_codepage: Option<i32>,
    /// Active `\fN`; `None` means the document default font.
    font_index: Option<i32>,
    color_table: bool,
    skip: bool,
    optional_destination: bool,
    uc: usize,
}
fn fail(s: &str) -> Error {
    Error::Invalid(s.into())
}
#[derive(Clone, Default)]
struct CellBorderDef {
    active: bool,
    solid: bool,
    width: Option<u16>,
    color: Option<u32>,
}
struct Reader {
    groups: Vec<Group>,
    fonts: HashMap<i32, String>,
    /// Per-font `\fcharset` and `\cpg`, used to decode `\'xx` bytes.
    font_codepages: HashMap<i32, (Option<i32>, Option<i32>)>,
    lead_byte: Option<u8>,
    font_text: Vec<u16>,
    pending: Vec<u16>,
    runs: Vec<Run>,
    blocks: Vec<Block>,
    para: ParagraphStyle,
    page: PageSettings,
    next_id: u64,
    colors: Vec<Option<u32>>,
    color: u32,
    color_set: bool,
    encoding: &'static encoding_rs::Encoding,
    fallback: usize,
    projection: bool,
    default_font: i32,
    image_pending: bool,
    lists: std::collections::BTreeMap<u64, Vec<bool>>,
    list_overrides: std::collections::BTreeMap<u64, u64>,
    list_index: Option<u64>,
    list_level: u8,
    marker_text: bool,
    in_table: bool,
    table_rows: Vec<Vec<Cell>>,
    table_widths: Vec<u32>,
    row_widths: Vec<u32>,
    cell_boundary: u32,
    table_cells: Vec<Cell>,
    table_merge_flags: Vec<u8>,
    row_vertical_flags: Vec<u8>,
    table_vertical_flags: Vec<Vec<u8>>,
    table_row_borders: Vec<Vec<[Option<Border>; 4]>>,
    next_cell_vertical: u8,
    table_fills: Vec<Option<u32>>,
    table_borders: Vec<[Option<Border>; 4]>,
    next_borders: [CellBorderDef; 4],
    border_edge: Option<usize>,
    next_cell_fill: Option<u32>,
    next_cell_merge: u8,
    cell_paragraphs: Vec<Paragraph>,
    header: Vec<Paragraph>,
    footer: Vec<Paragraph>,
    fields: Vec<Field>,
    pending_fields: Vec<(usize, FieldKind)>,
}
impl Reader {
    fn flush(&mut self) -> Result<(), Error> {
        if !self.pending.is_empty() {
            let text =
                String::from_utf16(&self.pending).map_err(|_| fail("invalid RTF Unicode"))?;
            self.pending.clear();
            self.runs.push(Run {
                text,
                style: self
                    .groups
                    .last()
                    .ok_or_else(|| fail("unbalanced RTF"))?
                    .style
                    .clone(),
                link: self.groups.last().and_then(|g| g.link.clone()),
            });
        }
        Ok(())
    }
    fn emit(&mut self, u: u16) -> Result<(), Error> {
        let g = self
            .groups
            .last()
            .ok_or_else(|| fail("RTF outside group"))?;
        if g.skip {
            return Ok(());
        }
        if g.color_table {
            if u == b';' as u16 {
                self.colors.push(if self.color_set {
                    Some(self.color)
                } else {
                    None
                });
                self.color = 0;
                self.color_set = false;
            } else if !(u as u8).is_ascii_whitespace() {
                return Err(fail("invalid color table"));
            }
            return Ok(());
        }
        if g.font_table {
            if u == b';' as u16 {
                let id = g.font_id.ok_or_else(|| fail("font without id"))?;
                let name =
                    String::from_utf16(&self.font_text).map_err(|_| fail("invalid font name"))?;
                self.font_text.clear();
                self.fonts.insert(id, name);
                let g = self.groups.last_mut().unwrap();
                let codepage = (g.font_charset.take(), g.font_codepage.take());
                self.font_codepages.insert(id, codepage);
            } else {
                self.font_text.push(u);
            }
        } else {
            self.pending.push(u);
        }
        Ok(())
    }
    fn field(&mut self, kind: FieldKind) -> Result<(), Error> {
        let prefix: usize = self.runs.iter().map(|r| r.text.len()).sum();
        let pending =
            String::from_utf16(&self.pending).map_err(|_| fail("invalid field position"))?;
        self.pending_fields.push((prefix + pending.len(), kind));
        Ok(())
    }
    fn flush_table(&mut self) -> Result<(), Error> {
        if self.table_rows.is_empty() {
            return Ok(());
        }
        let id = self.next_id;
        self.next_id = self.next_id.checked_add(1).ok_or(Error::Limit)?;
        let widths = std::mem::take(&mut self.table_widths);
        let mut table = Table {
            column_widths_twips: if widths.iter().all(|w| *w == 1440) {
                Vec::new()
            } else {
                widths
            },
            id,
            rows: std::mem::take(&mut self.table_rows),
        };
        let vertical = std::mem::take(&mut self.table_vertical_flags);
        let borders = std::mem::take(&mut self.table_row_borders);
        let height = table.rows.len();
        let width = table.rows[0].len();
        if vertical.len() != height
            || borders.len() != height
            || vertical.iter().any(|r| r.len() != width)
            || borders.iter().any(|r| r.len() != width)
        {
            return Err(fail("RTF table descriptor count"));
        }
        let horizontal = table
            .rows
            .iter()
            .map(|row| super::table_edit::horizontal_spans(row))
            .collect::<Result<Vec<_>, _>>()?;
        let mut consumed = vec![vec![false; width]; height];
        for row in 0..height {
            for col in 0..width {
                let span = horizontal[row][col] as usize;
                if span == 0 {
                    continue;
                }
                let flag = vertical[row][col];
                if vertical[row][col..col + span].iter().any(|f| *f != flag) {
                    return Err(Error::Unsupported("inconsistent RTF vertical merge".into()));
                }
                if flag == 2 {
                    if !consumed[row][col] {
                        return Err(fail("vertical merge continuation without anchor"));
                    }
                    continue;
                }
                if flag == 1 {
                    let mut bottom = row + 1;
                    while bottom < height
                        && horizontal[bottom][col] as usize == span
                        && vertical[bottom][col..col + span].iter().all(|f| *f == 2)
                    {
                        let fill = table.rows[row][col].fill;
                        let cell = &mut table.rows[bottom][col];
                        if cell.fill.is_some() && cell.fill != fill
                            || cell.paragraphs.len() != 1
                            || cell.paragraphs[0].style != ParagraphStyle::default()
                            || cell.paragraphs[0].runs.iter().any(|r| {
                                !r.text.is_empty()
                                    || r.link.is_some()
                                    || r.style != TextStyle::default()
                            })
                        {
                            return Err(Error::Unsupported(
                                "nonempty or differently styled vertical merge continuation".into(),
                            ));
                        }
                        cell.fill = None;
                        cell.col_span = 1;
                        consumed[bottom][col] = true;
                        bottom += 1;
                    }
                    table.rows[row][col].row_span =
                        u16::try_from(bottom - row).map_err(|_| Error::Limit)?;
                }
            }
        }
        let layout = super::table_edit::table_layout(&table)?;
        for row in 0..height {
            for col in 0..width {
                if layout[row][col] != (row, col) {
                    continue;
                }
                let h = table.rows[row][col].row_span as usize;
                let w = table.rows[row][col].col_span as usize;
                let all = borders[row..row + h]
                    .iter()
                    .flat_map(|r| r[col..col + w].iter())
                    .flat_map(|edges| edges.iter())
                    .filter_map(|b| b.as_ref())
                    .collect::<Vec<_>>();
                if let Some(first) = all.first() {
                    if all.iter().any(|b| *b != *first)
                        || (col..col + w).any(|c| {
                            borders[row][c][0].is_none() || borders[row + h - 1][c][2].is_none()
                        })
                        || (row..row + h).any(|r| {
                            borders[r][col][1].is_none() || borders[r][col + w - 1][3].is_none()
                        })
                    {
                        return Err(Error::Unsupported("nonuniform RTF cell borders".into()));
                    }
                    table.rows[row][col].border = Some((*first).clone());
                }
            }
        }
        self.blocks.push(Block::Table(table));
        Ok(())
    }
    fn paragraph(&mut self) -> Result<(), Error> {
        self.flush()?;
        if self.image_pending {
            self.image_pending = false;
            if !self.runs.is_empty() {
                return Err(Error::Unsupported("picture mixed with text".into()));
            }
            return Ok(());
        }
        if self.marker_text && self.para.list.is_none() {
            return Err(Error::Unsupported("RTF legacy list marker".into()));
        }
        self.marker_text = false;
        let id = self.next_id;
        self.next_id += 1;
        let paragraph = Paragraph {
            id,
            style: self.para.clone(),
            runs: std::mem::take(&mut self.runs),
        };
        for (offset, kind) in std::mem::take(&mut self.pending_fields) {
            let field_id = self.next_id;
            self.next_id += 1;
            self.fields.push(Field {
                id: field_id,
                at: Position {
                    paragraph: id,
                    offset,
                },
                kind,
            });
        }
        match self
            .groups
            .last()
            .map(|g| g.destination)
            .unwrap_or_default()
        {
            Destination::Header => self.header.push(paragraph),
            Destination::Footer => self.footer.push(paragraph),
            Destination::Body => {
                if self.in_table {
                    self.cell_paragraphs.push(paragraph)
                } else {
                    self.flush_table()?;
                    self.blocks.push(Block::Paragraph(paragraph))
                }
            }
        };
        Ok(())
    }
    fn update_list(&mut self) -> Result<(), Error> {
        self.para.list = if let Some(ls) = self.list_index {
            let id = *self
                .list_overrides
                .get(&ls)
                .ok_or_else(|| fail("unknown list override"))?;
            let numbered = *self
                .lists
                .get(&id)
                .and_then(|v| v.get(self.list_level as usize))
                .ok_or_else(|| fail("unknown list level"))?;
            Some(ListMarker {
                id,
                level: self.list_level,
                numbered,
            })
        } else {
            None
        };
        Ok(())
    }
    fn control(&mut self, word: &str, arg: Option<i32>) -> Result<(), Error> {
        if self.groups.last().ok_or_else(|| fail("group"))?.skip {
            return Ok(());
        }
        if self.groups.last().unwrap().optional_destination {
            if matches!(word, "generator" | "info" | "xmlnstbl" | "fldinst") {
                let g = self.groups.last_mut().unwrap();
                g.skip = true;
                g.optional_destination = false;
                return Ok(());
            }
            return Err(Error::Unsupported(format!(
                "RTF optional destination {word}"
            )));
        }
        if matches!(word, "generator" | "info") {
            self.groups.last_mut().unwrap().skip = true;
            return Ok(());
        }
        if word == "colortbl" {
            self.groups.last_mut().unwrap().color_table = true;
            return Ok(());
        }
        if self.groups.last().unwrap().color_table {
            let n = u32::try_from(arg.ok_or_else(|| fail("color"))?).map_err(|_| fail("color"))?;
            if n > 255 {
                return Err(fail("color"));
            }
            match word {
                "red" => self.color = (self.color & 0x00ffff) | (n << 16),
                "green" => self.color = (self.color & 0xff00ff) | (n << 8),
                "blue" => self.color = (self.color & 0xffff00) | n,
                _ => return Err(Error::Unsupported(format!("RTF color control {word}"))),
            }
            self.color_set = true;
            return Ok(());
        }
        if word == "fonttbl" {
            self.groups
                .last_mut()
                .ok_or_else(|| fail("group"))?
                .font_table = true;
            return Ok(());
        }
        if self.groups.last().ok_or_else(|| fail("group"))?.font_table {
            if word == "f" {
                self.groups.last_mut().unwrap().font_id = arg;
            } else if word == "fcharset" {
                self.groups.last_mut().unwrap().font_charset = arg;
            } else if word == "cpg" {
                self.groups.last_mut().unwrap().font_codepage = arg;
            } else if !matches!(
                word,
                "fnil"
                    | "froman"
                    | "fswiss"
                    | "fmodern"
                    | "fscript"
                    | "fdecor"
                    | "ftech"
                    | "fbidi"
                    | "fcharset"
                    | "fprq"
                    | "cpg"
            ) {
                return Err(Error::Unsupported(format!("RTF font control {word}")));
            }
            return Ok(());
        }
        match word {
            "header" => self.groups.last_mut().unwrap().destination = Destination::Header,
            "footer" => self.groups.last_mut().unwrap().destination = Destination::Footer,
            "chpgn" => self.field(FieldKind::PageNumber)?,
            "rtf" if arg == Some(1) => {}
            "ansi" => {}
            "field" | "fldrslt" | "flddirty" => {}
            "uc" => {
                let n = usize::try_from(arg.ok_or_else(|| fail("uc"))?).map_err(|_| fail("uc"))?;
                if n > 16 {
                    return Err(fail("uc"));
                }
                self.groups.last_mut().unwrap().uc = n;
            }
            "ansicpg" => {
                let n = arg.ok_or_else(|| fail("codepage"))?;
                let name = format!("windows-{n}");
                self.encoding = encoding_rs::Encoding::for_label(name.as_bytes())
                    .ok_or_else(|| Error::Unsupported(format!("RTF codepage {n}")))?;
            }
            "deff" => self.default_font = arg.ok_or_else(|| fail("default font"))?,
            "deflang" | "viewkind" | "viewscale" | "nouicompat" | "fet" => {}
            "lang" | "langfe" if self.projection => {}
            "cf" | "highlight" => {
                self.flush()?;
                let n = usize::try_from(arg.ok_or_else(|| fail("color index"))?)
                    .map_err(|_| fail("color index"))?;
                let c = *self.colors.get(n).ok_or_else(|| fail("unknown color"))?;
                let t = &mut self.groups.last_mut().unwrap().style;
                if word == "cf" {
                    t.color = c
                } else {
                    t.highlight = c
                }
            }
            "plain" => {
                self.flush()?;
                let mut style = TextStyle::default();
                if let Some(font) = self.fonts.get(&self.default_font) {
                    style.font = font.clone();
                }
                let g = self.groups.last_mut().unwrap();
                g.style = style;
                g.font_index = None;
            }
            "f" => {
                self.flush()?;
                let n = arg.ok_or_else(|| fail("font index"))?;
                let font = self.fonts.get(&n).ok_or_else(|| fail("unknown font"))?.clone();
                let g = self.groups.last_mut().unwrap();
                g.style.font = font;
                g.font_index = Some(n);
            }
            "fs" => {
                self.flush()?;
                self.groups.last_mut().unwrap().style.size_half_points =
                    u16::try_from(arg.ok_or_else(|| fail("font size"))?)
                        .map_err(|_| fail("font size"))?;
            }
            "b" | "i" | "ul" | "strike" => {
                self.flush()?;
                let on = arg.unwrap_or(1) != 0;
                let t = &mut self.groups.last_mut().unwrap().style;
                match word {
                    "b" => t.bold = on,
                    "i" => t.italic = on,
                    "ul" => t.underline = on,
                    _ => t.strike = on,
                }
            }
            "trowd" => {
                if self.in_table {
                    return Err(fail("nested table"));
                }
                self.in_table = true;
                self.table_cells.clear();
                self.row_widths.clear();
                self.cell_boundary = 0;
                self.table_merge_flags.clear();
                self.row_vertical_flags.clear();
                self.next_cell_vertical = 0;
                self.table_fills.clear();
                self.table_borders.clear();
                self.next_borders = Default::default();
                self.border_edge = None;
                self.next_cell_fill = None;
                self.next_cell_merge = 0;
                self.cell_paragraphs.clear();
            }
            "clbrdrt" | "clbrdrl" | "clbrdrb" | "clbrdrr" if self.in_table => {
                let index = match word {
                    "clbrdrt" => 0,
                    "clbrdrl" => 1,
                    "clbrdrb" => 2,
                    _ => 3,
                };
                if self.next_borders[index].active {
                    return Err(fail("duplicate cell border"));
                }
                self.next_borders[index].active = true;
                self.border_edge = Some(index);
            }
            "brdrs" if self.in_table && self.border_edge.is_some() => {
                self.next_borders[self.border_edge.unwrap()].solid = true
            }
            "brdrw" if self.in_table && self.border_edge.is_some() => {
                self.next_borders[self.border_edge.unwrap()].width = Some(
                    u16::try_from(arg.ok_or_else(|| fail("border width"))?)
                        .map_err(|_| fail("border width"))?,
                );
            }
            "brdrcf" if self.in_table && self.border_edge.is_some() => {
                let index = usize::try_from(arg.ok_or_else(|| fail("border color"))?)
                    .map_err(|_| fail("border color"))?;
                self.next_borders[self.border_edge.unwrap()].color = Some(
                    self.colors
                        .get(index)
                        .and_then(|v| *v)
                        .ok_or_else(|| fail("border color"))?,
                );
            }
            "clcbpat" if self.in_table => {
                let index = usize::try_from(arg.ok_or_else(|| fail("cell color index"))?)
                    .map_err(|_| fail("cell color index"))?;
                self.next_cell_fill = *self
                    .colors
                    .get(index)
                    .ok_or_else(|| fail("unknown cell color"))?;
            }
            "clshdng" if self.in_table && arg.unwrap_or(0) == 0 => {}
            "clvmgf" if self.in_table && self.next_cell_vertical == 0 => {
                self.next_cell_vertical = 1
            }
            "clvmrg" if self.in_table && self.next_cell_vertical == 0 => {
                self.next_cell_vertical = 2
            }
            "clmgf" if self.in_table && self.next_cell_merge == 0 => self.next_cell_merge = 1,
            "clmrg" if self.in_table && self.next_cell_merge == 0 => self.next_cell_merge = 2,
            "cellx" if self.in_table => {
                let boundary = u32::try_from(arg.ok_or_else(|| fail("cell boundary"))?)
                    .map_err(|_| fail("cell boundary"))?;
                let width = boundary
                    .checked_sub(self.cell_boundary)
                    .filter(|w| *w > 0 && *w <= 2_000_000)
                    .ok_or_else(|| fail("cell width"))?;
                self.row_widths.push(width);
                self.cell_boundary = boundary;
                let mut border: [Option<Border>; 4] = Default::default();
                for (i, b) in self.next_borders.iter().enumerate() {
                    if !b.active {
                        continue;
                    }
                    if !b.solid {
                        return Err(Error::Unsupported("RTF cell border style".into()));
                    }
                    let width_twips = b
                        .width
                        .filter(|v| *v > 0)
                        .ok_or_else(|| fail("border width"))?;
                    border[i] = Some(Border {
                        width_twips,
                        color: b.color.ok_or_else(|| fail("border color"))?,
                    });
                }
                self.table_borders.push(border);
                self.next_borders = Default::default();
                self.border_edge = None;
                self.table_merge_flags.push(self.next_cell_merge);
                self.row_vertical_flags.push(self.next_cell_vertical);
                self.next_cell_vertical = 0;
                self.table_fills.push(self.next_cell_fill.take());
                self.next_cell_merge = 0;
            }
            "intbl" if self.in_table => {}
            "cell" if self.in_table => {
                self.flush()?;
                if !self.runs.is_empty() || self.cell_paragraphs.is_empty() {
                    self.paragraph()?;
                }
                self.table_cells.push(Cell {
                    paragraphs: std::mem::take(&mut self.cell_paragraphs),
                    row_span: 1,
                    col_span: 1,
                    border: None,
                    fill: None,
                });
            }
            "row" if self.in_table => {
                if !self.runs.is_empty()
                    || !self.cell_paragraphs.is_empty()
                    || self.table_cells.is_empty()
                {
                    return Err(fail("incomplete table row"));
                }
                if self.table_widths.is_empty() {
                    self.table_widths = self.row_widths.clone();
                } else if self.table_widths != self.row_widths {
                    return Err(Error::Unsupported(
                        "RTF table rows with different column widths".into(),
                    ));
                }
                if self.table_merge_flags.len() != self.table_cells.len() {
                    return Err(fail("table cell definitions"));
                }
                if self.table_fills.len() != self.table_cells.len() {
                    return Err(fail("table cell colors"));
                }
                if self.table_borders.len() != self.table_cells.len() {
                    return Err(fail("table cell borders"));
                }
                for (cell, fill) in self.table_cells.iter_mut().zip(&self.table_fills) {
                    cell.fill = *fill;
                }
                let mut anchor: Option<usize> = None;
                for (i, flag) in self.table_merge_flags.iter().copied().enumerate() {
                    match flag {
                        0 => anchor = None,
                        1 => anchor = Some(i),
                        2 => {
                            let start =
                                anchor.ok_or_else(|| fail("merge continuation without anchor"))?;
                            let anchor_fill = self.table_cells[start].fill;
                            let covered = &mut self.table_cells[i];
                            if covered.fill.is_some() && covered.fill != anchor_fill {
                                return Err(Error::Unsupported(
                                    "different backgrounds within RTF merged cell".into(),
                                ));
                            }
                            covered.fill = None;
                            self.table_cells[start].col_span = self.table_cells[start]
                                .col_span
                                .checked_add(1)
                                .ok_or(Error::Limit)?;
                        }
                        _ => unreachable!(),
                    }
                }
                super::table_edit::horizontal_spans(&self.table_cells)?;
                if self.row_vertical_flags.len() != self.table_cells.len() {
                    return Err(fail("table vertical cell definitions"));
                }
                self.table_vertical_flags
                    .push(std::mem::take(&mut self.row_vertical_flags));
                self.table_row_borders
                    .push(std::mem::take(&mut self.table_borders));
                self.table_rows.push(std::mem::take(&mut self.table_cells));
                self.in_table = false;
            }
            "tab" => self.emit(9)?,
            "par" => self.paragraph()?,
            "pard" => {
                self.para = ParagraphStyle::default();
                self.list_index = None;
                self.list_level = 0;
            }
            "ls" => {
                let n = u64::try_from(arg.ok_or_else(|| fail("list index"))?)
                    .map_err(|_| fail("list index"))?;
                self.list_index = if n == 0 { None } else { Some(n) };
                self.update_list()?;
            }
            "ilvl" => {
                self.list_level = u8::try_from(arg.ok_or_else(|| fail("list level"))?)
                    .map_err(|_| fail("list level"))?;
                self.update_list()?;
            }
            "keepn" => self.para.keep_with_next = arg.unwrap_or(1) != 0,
            "keep" => self.para.keep_lines = arg.unwrap_or(1) != 0,
            "pagebb" => self.para.page_break_before = arg.unwrap_or(1) != 0,
            "outlinelevel" => {
                self.para.outline_level = Some(
                    u8::try_from(arg.ok_or_else(|| fail("outline level"))?)
                        .map_err(|_| fail("outline level"))?,
                )
            }
            "ql" => self.para.alignment = Alignment::Left,
            "qc" => self.para.alignment = Alignment::Center,
            "qr" => self.para.alignment = Alignment::Right,
            "qj" => self.para.alignment = Alignment::Justify,
            "tx" => {
                let n = arg.ok_or_else(|| fail("tab stop"))?;
                if n < 0 {
                    return Err(fail("tab stop"));
                }
                self.para.tabs_twips.push(n);
            }
            "li" => self.para.left_twips = arg.ok_or_else(|| fail("indent"))?,
            "ri" => self.para.right_twips = arg.ok_or_else(|| fail("indent"))?,
            "fi" => self.para.first_line_twips = arg.ok_or_else(|| fail("indent"))?,
            "sb" => {
                self.para.before_twips = u32::try_from(arg.ok_or_else(|| fail("spacing"))?)
                    .map_err(|_| fail("spacing"))?
            }
            "sa" => {
                self.para.after_twips = u32::try_from(arg.ok_or_else(|| fail("spacing"))?)
                    .map_err(|_| fail("spacing"))?
            }
            "sl" => self.para.line_twips = arg.ok_or_else(|| fail("spacing"))?,
            "paperw" | "paperh" | "margl" | "margr" | "margt" | "margb" | "headery" | "footery" => {
                let n = u32::try_from(arg.ok_or_else(|| fail("page geometry"))?)
                    .map_err(|_| fail("page geometry"))?;
                match word {
                    "paperw" => self.page.width_twips = n,
                    "paperh" => self.page.height_twips = n,
                    "margl" => self.page.margin_left = n,
                    "margr" => self.page.margin_right = n,
                    "margt" => self.page.margin_top = n,
                    "margb" => self.page.margin_bottom = n,
                    "headery" => self.page.header_distance_twips = n,
                    _ => self.page.footer_distance_twips = n,
                }
            }
            _ => return Err(Error::Unsupported(format!("RTF control {word}"))),
        }
        Ok(())
    }
}
fn codepage_encoding(codepage: i32) -> Option<&'static encoding_rs::Encoding> {
    match codepage {
        932 => Some(encoding_rs::SHIFT_JIS),
        936 => Some(encoding_rs::GBK),
        949 => Some(encoding_rs::EUC_KR),
        950 => Some(encoding_rs::BIG5),
        874 | 1250..=1258 => {
            encoding_rs::Encoding::for_label(format!("windows-{codepage}").as_bytes())
        }
        _ => None,
    }
}
impl Reader {
    /// Encoding for `\'xx` bytes under the active font. RichEdit tags typed text with
    /// the charset of the keyboard layout in use, e.g. `\fcharset204` for Russian.
    fn byte_encoding(&self) -> Result<&'static encoding_rs::Encoding, Error> {
        let font = self
            .groups
            .last()
            .and_then(|g| g.font_index)
            .unwrap_or(self.default_font);
        let codepage = match self.font_codepages.get(&font) {
            Some(&(_, Some(codepage))) => codepage,
            Some(&(Some(charset), None)) => match charset {
                0 | 1 => return Ok(self.encoding),
                128 => 932,
                129 => 949,
                134 => 936,
                136 => 950,
                161 => 1253,
                162 => 1254,
                163 => 1258,
                177 => 1255,
                178 => 1256,
                186 => 1257,
                204 => 1251,
                222 => 874,
                238 => 1250,
                other => return Err(Error::Unsupported(format!("RTF font charset {other}"))),
            },
            _ => return Ok(self.encoding),
        };
        codepage_encoding(codepage)
            .ok_or_else(|| Error::Unsupported(format!("RTF font codepage {codepage}")))
    }
    fn decode_byte(&mut self, byte: u8) -> Result<(), Error> {
        let encoding = self.byte_encoding()?;
        let data = if encoding.is_single_byte() {
            vec![byte]
        } else if let Some(lead) = self.lead_byte.take() {
            vec![lead, byte]
        } else if byte >= 0x81 {
            self.lead_byte = Some(byte);
            return Ok(());
        } else {
            vec![byte]
        };
        let (decoded, bad) = encoding.decode_without_bom_handling(&data);
        if bad {
            return Err(fail("invalid codepage byte"));
        }
        for u in decoded.encode_utf16() {
            self.emit(u)?
        }
        Ok(())
    }
}
pub(super) fn decode(bytes: &[u8]) -> Result<State, Error> {
    decode_inner(bytes, false)
}
pub(super) fn decode_projection(bytes: &[u8]) -> Result<State, Error> {
    decode_inner(bytes, true)
}
fn decode_inner(bytes: &[u8], projection: bool) -> Result<State, Error> {
    if !bytes.starts_with(b"{\\rtf1") {
        return Err(fail("missing RTF header"));
    }
    let mut r = Reader {
        groups: vec![],
        fonts: HashMap::new(),
        font_codepages: HashMap::new(),
        lead_byte: None,
        font_text: vec![],
        pending: vec![],
        runs: vec![],
        blocks: vec![],
        para: ParagraphStyle::default(),
        page: PageSettings::default(),
        next_id: 1,
        colors: vec![],
        color: 0,
        color_set: false,
        encoding: encoding_rs::WINDOWS_1252,
        fallback: 0,
        projection,
        default_font: 0,
        image_pending: false,
        lists: Default::default(),
        list_overrides: Default::default(),
        list_index: None,
        list_level: 0,
        marker_text: false,
        in_table: false,
        table_rows: vec![],
        table_widths: vec![],
        row_widths: vec![],
        cell_boundary: 0,
        table_cells: vec![],
        table_merge_flags: vec![],
        row_vertical_flags: vec![],
        table_vertical_flags: vec![],
        table_row_borders: vec![],
        next_cell_vertical: 0,
        table_fills: vec![],
        table_borders: vec![],
        next_borders: Default::default(),
        border_edge: None,
        next_cell_fill: None,
        next_cell_merge: 0,
        cell_paragraphs: vec![],
        header: vec![],
        footer: vec![],
        fields: vec![],
        pending_fields: vec![],
    };
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        i += 1;
        match b {
            b'{' => {
                if bytes[i..].starts_with(b"\\*\\listtable") {
                    let start = i - 1;
                    let end = super::rtf_lists::group_end(bytes, start)?;
                    r.lists = super::rtf_lists::definitions(&bytes[start..end])?;
                    i = end;
                    continue;
                }
                if bytes[i..].starts_with(b"\\*\\listoverridetable") {
                    let start = i - 1;
                    let end = super::rtf_lists::group_end(bytes, start)?;
                    r.list_overrides = super::rtf_lists::overrides(&bytes[start..end])?;
                    i = end;
                    continue;
                }
                if bytes[i..].starts_with(b"\\pntext") {
                    let start = i - 1;
                    let end = super::rtf_lists::group_end(bytes, start)?;
                    r.marker_text = true;
                    i = end;
                    continue;
                }
                if bytes[i..].starts_with(b"\\pict") {
                    r.flush()?;
                    if !r.runs.is_empty() || r.image_pending {
                        return Err(Error::Unsupported("picture mixed with text".into()));
                    }
                    let start = i - 1;
                    let end = bytes[i..]
                        .iter()
                        .position(|b| *b == b'}')
                        .map(|n| i + n + 1)
                        .ok_or_else(|| fail("unterminated picture"))?;
                    let image = super::rtf_picture::parse(&bytes[start..end], r.next_id)?;
                    r.next_id += 1;
                    r.blocks.push(Block::Image(image));
                    r.image_pending = true;
                    i = end;
                    continue;
                }
                if !r.groups.is_empty() {
                    r.flush()?;
                }
                let mut g = r.groups.last().cloned().unwrap_or_else(|| Group {
                    uc: 1,
                    ..Group::default()
                });
                if bytes[i..].starts_with(b"\\field") {
                    let start = i - 1;
                    let end = super::rtf_lists::group_end(bytes, start)?;
                    match super::rtf_field::parse(&bytes[start..end])? {
                        super::rtf_field::Parsed::Hyperlink(url) => g.link = Some(url),
                        super::rtf_field::Parsed::PageNumber => {
                            r.field(FieldKind::PageNumber)?;
                            i = end;
                            continue;
                        }
                        super::rtf_field::Parsed::PageCount => {
                            r.field(FieldKind::PageCount)?;
                            i = end;
                            continue;
                        }
                    }
                }
                r.groups.push(g)
            }
            b'}' => {
                r.flush()?;
                let ended = r.groups.pop().ok_or_else(|| fail("unbalanced RTF"))?;
                if ended.font_table && r.groups.last().is_some_and(|g| !g.font_table) {
                    if let Some(font) = r.fonts.get(&r.default_font) {
                        r.groups.last_mut().unwrap().style.font = font.clone();
                    }
                }
                if r.groups.is_empty() {
                    // RichEdit's EM_STREAMOUT (and files it writes) end with a NUL.
                    if bytes[i..].iter().any(|b| !b.is_ascii_whitespace() && *b != 0) {
                        return Err(fail("trailing RTF data"));
                    }
                    break;
                }
            }
            b'\\' => {
                if i >= bytes.len() {
                    return Err(fail("truncated RTF escape"));
                }
                let c = bytes[i];
                i += 1;
                if c == b'*' {
                    r.groups
                        .last_mut()
                        .ok_or_else(|| fail("group"))?
                        .optional_destination = true;
                    continue;
                }
                if c == b'\'' {
                    if i + 2 > bytes.len() {
                        return Err(fail("hex escape"));
                    }
                    let hex =
                        std::str::from_utf8(&bytes[i..i + 2]).map_err(|_| fail("hex escape"))?;
                    let v = u8::from_str_radix(hex, 16).map_err(|_| fail("hex escape"))?;
                    i += 2;
                    if r.fallback > 0 {
                        r.fallback -= 1;
                        continue;
                    }
                    r.decode_byte(v)?;
                    continue;
                }
                if matches!(c, b'\\' | b'{' | b'}') {
                    if r.fallback > 0 {
                        r.fallback -= 1;
                    } else {
                        r.emit(c as u16)?;
                    }
                    continue;
                }
                if !c.is_ascii_alphabetic() {
                    return Err(Error::Unsupported("RTF control symbol".into()));
                }
                let start = i - 1;
                while i < bytes.len() && bytes[i].is_ascii_alphabetic() {
                    i += 1
                }
                let word = std::str::from_utf8(&bytes[start..i]).map_err(|_| fail("control"))?;
                let begin = i;
                if i < bytes.len() && bytes[i] == b'-' {
                    i += 1
                }
                while i < bytes.len() && bytes[i].is_ascii_digit() {
                    i += 1
                }
                let arg = if i > begin {
                    Some(
                        std::str::from_utf8(&bytes[begin..i])
                            .map_err(|_| fail("number"))?
                            .parse::<i32>()
                            .map_err(|_| fail("number"))?,
                    )
                } else {
                    None
                };
                if i < bytes.len() && bytes[i] == b' ' {
                    i += 1
                }
                if word == "u" {
                    let n = arg.ok_or_else(|| fail("Unicode control"))?;
                    if !(-32768..=32767).contains(&n) {
                        return Err(fail("Unicode value"));
                    }
                    r.emit(n as i16 as u16)?;
                    r.fallback = r.groups.last().ok_or_else(|| fail("group"))?.uc;
                } else {
                    r.control(word, arg)?;
                }
            }
            b'\r' | b'\n' => {}
            b => {
                if r.fallback > 0 {
                    r.fallback -= 1;
                    continue;
                }
                if b.is_ascii() && r.lead_byte.is_none() {
                    r.emit(b as u16)?
                } else {
                    r.decode_byte(b)?
                }
            }
        }
    }
    if r.in_table {
        return Err(fail("unterminated table"));
    }
    r.flush_table()?;
    if !r.groups.is_empty() {
        return Err(fail("unbalanced RTF"));
    }
    if !r.pending.is_empty() || !r.runs.is_empty() {
        return Err(Error::Unsupported("RTF unterminated paragraph".into()));
    }
    if r.blocks.is_empty() {
        return Err(fail("no paragraphs"));
    }
    Ok(State {
        blocks: r.blocks,
        sections: vec![Section {
            start_block: 0,
            page: r.page,
            header: r.header,
            footer: r.footer,
        }],
        next_id: r.next_id,
        fields: r.fields,
        ..State::default()
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn font_charsets_decode_bytes_like_richedit_keyboard_layouts() {
        // RichEdit output after typing with Russian, English and Chinese layouts.
        let bytes = br"{\rtf1\ansi\ansicpg1252\deff0\nouicompat{\fonttbl{\f0\fnil\fcharset204 Arial;}{\f1\fnil Arial;}{\f2\fnil\fcharset134 SimSun;}}
{\*\generator Riched20 10.0.26100}\viewkind4\uc1
\pard\f0\fs24\lang1049 Hello \'cf\'f0\'e8\'e2\'e5\'f2\f1\lang1033  Caf\'e9 \f2\'d6\'d0\'ce\'c4\par
}";
        let mut bytes = bytes.to_vec();
        bytes.push(0);
        let state = decode_projection(&bytes).unwrap();
        let Block::Paragraph(p) = &state.blocks[0] else {
            panic!()
        };
        let text: String = p.runs.iter().map(|r| r.text.as_str()).collect();
        assert_eq!(text, "Hello \u{41f}\u{440}\u{438}\u{432}\u{435}\u{442} Caf\u{e9} \u{4e2d}\u{6587}");
        let symbol = br"{\rtf1\ansi{\fonttbl{\f0\fnil\fcharset2 Symbol;}}\f0 abc\par}";
        assert!(decode(symbol).is_ok());
        let symbol_byte = br"{\rtf1\ansi{\fonttbl{\f0\fnil\fcharset2 Symbol;}}\f0 \'b7\par}";
        assert!(matches!(decode(symbol_byte), Err(Error::Unsupported(_))));
    }
    #[test]
    fn richedit_trailing_nul_is_accepted_but_other_trailing_data_is_not() {
        let state = decode(b"{\\rtf1\\ansi Hello\\par}\r\n\0").unwrap();
        let Block::Paragraph(p) = &state.blocks[0] else {
            panic!()
        };
        assert_eq!(p.runs[0].text, "Hello");
        assert!(decode(b"{\\rtf1\\ansi Hello\\par}\r\nx").is_err());
    }
    #[test]
    fn independently_authored_horizontal_merge_controls_are_read() {
        let bytes = br"{\rtf1\ansi\trowd\clmgf\cellx1000\clmrg\cellx2000\pard\intbl First\par\cell\pard\intbl\par\cell\row}";
        let state = decode(bytes).unwrap();
        let Block::Table(t) = &state.blocks[0] else {
            panic!()
        };
        assert_eq!(t.rows[0][0].col_span, 2);
        assert_eq!(t.rows[0][0].paragraphs[0].runs[0].text, "First");
    }
    #[test]
    fn table_rows_and_cells_round_trip() {
        let mut s = State::default();
        let p = Paragraph {
            id: 3,
            style: ParagraphStyle::default(),
            runs: vec![Run {
                text: "cell".into(),
                style: TextStyle::default(),
                link: None,
            }],
        };
        s.blocks.push(Block::Table(Table {
            column_widths_twips: Vec::new(),
            id: 2,
            rows: vec![vec![Cell {
                paragraphs: vec![p],
                row_span: 1,
                col_span: 1,
                border: None,
                fill: None,
            }]],
        }));
        s.next_id = 4;
        let bytes = super::super::rtf::rtf_export(&s).unwrap();
        let reopened = decode(&bytes).unwrap();
        let Block::Table(t) = &reopened.blocks[1] else {
            panic!()
        };
        assert_eq!(t.rows[0][0].paragraphs[0].runs[0].text, "cell");
    }
    #[test]
    fn list_table_and_override_round_trip() {
        let mut s = State::default();
        let Block::Paragraph(p) = &mut s.blocks[0] else {
            panic!()
        };
        p.runs[0].text = "item".into();
        p.style.list = Some(ListMarker {
            id: 7,
            level: 1,
            numbered: true,
        });
        let bytes = super::super::rtf::rtf_export(&s).unwrap();
        let reopened = decode(&bytes).unwrap();
        let Block::Paragraph(p) = &reopened.blocks[0] else {
            panic!()
        };
        assert_eq!(
            p.style.list,
            Some(ListMarker {
                id: 7,
                level: 1,
                numbered: true
            })
        );
    }
    #[test]
    fn picture_destination_round_trip() {
        let mut s = State::default();
        s.blocks.push(Block::Image(Image {
            id: 2,
            mime: "image/png".into(),
            data: vec![137, 80, 78, 71, 13, 10, 26, 10, 0, 0],
            width_twips: 1440,
            height_twips: 720,
            alt: String::new(),
        }));
        s.next_id = 3;
        let bytes = super::super::rtf::rtf_export(&s).unwrap();
        let reopened = decode(&bytes).unwrap();
        let Block::Image(i) = &reopened.blocks[1] else {
            panic!()
        };
        assert_eq!((i.width_twips, i.height_twips), (1440, 720));
        assert_eq!(i.data, vec![137, 80, 78, 71, 13, 10, 26, 10, 0, 0]);
    }
    #[test]
    fn windows_rtf_header_colors_and_codepage() {
        let bytes=br"{\rtf1\ansi\ansicpg1252\deff0\deflang1033{\fonttbl{\f0\fnil\fcharset0 Arial;}}{\colortbl;\red51\green102\blue153;}{\*\generator Test;}\viewkind4\uc1\pard\cf1\f0\fs24 Caf\'e9 \u1046?\par}";
        let s = decode(bytes).unwrap();
        let Block::Paragraph(p) = &s.blocks[0] else {
            panic!()
        };
        assert_eq!(
            p.runs.iter().map(|r| r.text.as_str()).collect::<String>(),
            "Café Ж"
        );
        assert_eq!(p.runs.last().unwrap().style.color, Some(0x336699));
    }
    #[test]
    fn read_own_styled_unicode_export() {
        let mut s = State::default();
        if let Block::Paragraph(p) = &mut s.blocks[0] {
            p.runs[0].text = "é 😀".into();
            p.runs[0].style.bold = true;
            p.style.alignment = Alignment::Center;
        }
        let bytes = super::super::rtf::rtf_export(&s).unwrap();
        let reopened = decode(&bytes).unwrap();
        let (Block::Paragraph(a), Block::Paragraph(b)) = (&s.blocks[0], &reopened.blocks[0]) else {
            panic!()
        };
        assert_eq!(a.runs, b.runs);
        assert_eq!(a.style, b.style);
        assert_eq!(s.sections[0].page, reopened.sections[0].page);
    }
}
