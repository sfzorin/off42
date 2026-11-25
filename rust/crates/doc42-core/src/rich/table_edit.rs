//! Transactional table structure edits. Covered cells retain empty placeholders,
//! so the grid remains rectangular and its anchor owns all original paragraphs.
use super::*;

impl RichDocument {
    /// Insert or delete a physical row/column, expanding or shrinking any
    /// crossed merge. Deleting its anchor promotes that anchor into the next
    /// covered slot so its text and style remain part of the surviving merge.
    pub fn edit_table_structure(
        &mut self,
        table_id: u64,
        row_axis: bool,
        insert: bool,
        index: usize,
    ) -> Result<bool, Error> {
        self.transact(|state| {
            let table = state
                .blocks
                .iter_mut()
                .find_map(|b| match b {
                    Block::Table(t) if t.id == table_id => Some(t),
                    _ => None,
                })
                .ok_or_else(|| Error::Invalid("table not found".into()))?;
            let layout = table_layout(table)?;
            let height = table.rows.len();
            let width = table.rows[0].len();
            let size = if row_axis { height } else { width };
            if (insert && (index > size || size >= 256))
                || (!insert && (index >= size || size <= 1))
            {
                return Err(Error::Invalid(
                    "table position is outside the supported range".into(),
                ));
            }
            let mut anchors = Vec::new();
            for (row, owners) in layout.iter().enumerate() {
                for (col, owner) in owners.iter().enumerate() {
                    if *owner == (row, col) {
                        let cell = &table.rows[row][col];
                        anchors.push((row, col, cell.row_span as usize, cell.col_span as usize));
                    }
                }
            }
            if insert {
                for &(row, col, h, w) in &anchors {
                    if row_axis && row < index && index < row + h {
                        table.rows[row][col].row_span = table.rows[row][col]
                            .row_span
                            .checked_add(1)
                            .ok_or(Error::Limit)?;
                    }
                    if !row_axis && col < index && index < col + w {
                        table.rows[row][col].col_span = table.rows[row][col]
                            .col_span
                            .checked_add(1)
                            .ok_or(Error::Limit)?;
                    }
                }
                let mut blank = || -> Result<Cell, Error> {
                    let id = state.next_id;
                    state.next_id = state.next_id.checked_add(1).ok_or(Error::Limit)?;
                    Ok(Cell {
                        paragraphs: vec![Paragraph {
                            id,
                            style: ParagraphStyle::default(),
                            runs: vec![Run {
                                text: String::new(),
                                style: TextStyle::default(),
                                link: None,
                            }],
                        }],
                        row_span: 1,
                        col_span: 1,
                        border: None,
                        fill: None,
                    })
                };
                if row_axis {
                    let mut row = Vec::with_capacity(width);
                    for _ in 0..width {
                        row.push(blank()?);
                    }
                    table.rows.insert(index, row);
                } else {
                    for row in &mut table.rows {
                        row.insert(index, blank()?);
                    }
                    if !table.column_widths_twips.is_empty() {
                        table.column_widths_twips.insert(index, 1440);
                    }
                }
            } else {
                for &(row, col, h, w) in &anchors {
                    if row_axis && row <= index && index < row + h && h > 1 {
                        if row == index {
                            let (above, below) = table.rows.split_at_mut(row + 1);
                            std::mem::swap(&mut above[row][col], &mut below[0][col]);
                            table.rows[row + 1][col].row_span -= 1;
                        } else {
                            table.rows[row][col].row_span -= 1;
                        }
                    }
                    if !row_axis && col <= index && index < col + w && w > 1 {
                        if col == index {
                            table.rows[row].swap(col, col + 1);
                            table.rows[row][col + 1].col_span -= 1;
                        } else {
                            table.rows[row][col].col_span -= 1;
                        }
                    }
                }
                if row_axis {
                    table.rows.remove(index);
                } else {
                    for row in &mut table.rows {
                        row.remove(index);
                    }
                    if !table.column_widths_twips.is_empty() {
                        table.column_widths_twips.remove(index);
                    }
                }
                state.selection = Selection::default();
            }
            table_layout(table)?;
            Ok(())
        })
    }
    pub fn merge_table_cells(
        &mut self,
        table_id: u64,
        row: usize,
        first: usize,
        last: usize,
    ) -> Result<bool, Error> {
        self.merge_table_rectangle(table_id, row, first, row, last)
    }
    pub fn merge_table_rectangle(
        &mut self,
        table_id: u64,
        top: usize,
        left: usize,
        bottom: usize,
        right: usize,
    ) -> Result<bool, Error> {
        self.transact(|state| {
            if top > bottom || left > right || (top == bottom && left == right) {
                return Err(Error::Invalid("select at least two cells".into()));
            }
            let table = state
                .blocks
                .iter_mut()
                .find_map(|block| match block {
                    Block::Table(t) if t.id == table_id => Some(t),
                    _ => None,
                })
                .ok_or_else(|| Error::Invalid("table not found".into()))?;
            let layout = table_layout(table)?;
            if bottom >= table.rows.len()
                || right >= table.rows[0].len()
                || bottom - top + 1 > 256
                || right - left + 1 > 256
            {
                return Err(Error::Invalid("cell range outside table".into()));
            }
            for row in top..=bottom {
                for col in left..=right {
                    let cell = &table.rows[row][col];
                    if layout[row][col] != (row, col) || cell.row_span != 1 || cell.col_span != 1 {
                        return Err(Error::Unsupported("merge intersects a merged cell".into()));
                    }
                    if cell.paragraphs.is_empty() {
                        return Err(Error::Invalid("empty cell structure".into()));
                    }
                }
            }
            // The merged cell adopts the first cell's cell-level style. Original
            // paragraphs retain their formatting; Undo restores all source styles.
            let mut moved = Vec::new();
            for row in top..=bottom {
                for col in left..=right {
                    if row == top && col == left {
                        continue;
                    }
                    let cell = &mut table.rows[row][col];
                    moved.append(&mut cell.paragraphs);
                    cell.fill = None;
                    cell.border = None;
                    let id = state.next_id;
                    state.next_id = state.next_id.checked_add(1).ok_or(Error::Limit)?;
                    cell.paragraphs = vec![Paragraph {
                        id,
                        style: ParagraphStyle::default(),
                        runs: vec![Run {
                            text: String::new(),
                            style: TextStyle::default(),
                            link: None,
                        }],
                    }];
                }
            }
            let anchor = &mut table.rows[top][left];
            anchor.paragraphs.extend(moved);
            anchor.row_span = (bottom - top + 1) as u16;
            anchor.col_span = (right - left + 1) as u16;
            Ok(())
        })
    }
    pub fn table_cell_anchor(
        &self,
        table_id: u64,
        row: usize,
        col: usize,
    ) -> Result<(usize, usize), Error> {
        let table = self
            .state()
            .blocks
            .iter()
            .find_map(|b| match b {
                Block::Table(t) if t.id == table_id => Some(t),
                _ => None,
            })
            .ok_or_else(|| Error::Invalid("table not found".into()))?;
        table_layout(table)?
            .get(row)
            .and_then(|r| r.get(col))
            .copied()
            .ok_or_else(|| Error::Invalid("cell not found".into()))
    }
    pub fn split_table_cell(
        &mut self,
        table_id: u64,
        row: usize,
        col: usize,
    ) -> Result<bool, Error> {
        self.transact(|state| {
            let table = state
                .blocks
                .iter_mut()
                .find_map(|block| match block {
                    Block::Table(t) if t.id == table_id => Some(t),
                    _ => None,
                })
                .ok_or_else(|| Error::Invalid("table not found".into()))?;
            let layout = table_layout(table)?;
            let &(top, left) = layout
                .get(row)
                .and_then(|r| r.get(col))
                .ok_or_else(|| Error::Invalid("cell not found".into()))?;
            let cell = &table.rows[top][left];
            if cell.row_span == 1 && cell.col_span == 1 {
                return Err(Error::Invalid("cell is not merged".into()));
            }
            let (height, width, fill, border) = (
                cell.row_span as usize,
                cell.col_span as usize,
                cell.fill,
                cell.border.clone(),
            );
            for row in top..top + height {
                for col in left..left + width {
                    let cell = &mut table.rows[row][col];
                    cell.row_span = 1;
                    cell.col_span = 1;
                    cell.fill = fill;
                    cell.border = border.clone();
                }
            }
            Ok(())
        })
    }
}

/// Map every grid position to its owning merge anchor.
pub(super) fn table_layout(table: &Table) -> Result<Vec<Vec<(usize, usize)>>, Error> {
    let height = table.rows.len();
    let width = table.rows.first().map_or(0, Vec::len);
    if height == 0 || width == 0 || table.rows.iter().any(|r| r.len() != width) {
        return Err(Error::Invalid("non-rectangular table".into()));
    }
    let unset = (usize::MAX, usize::MAX);
    let mut result = vec![vec![unset; width]; height];
    for row in 0..height {
        for col in 0..width {
            let cell = &table.rows[row][col];
            if result[row][col] != unset {
                if cell.row_span != 1
                    || cell.col_span != 1
                    || cell.fill.is_some()
                    || cell.border.is_some()
                    || cell.paragraphs.len() != 1
                    || cell.paragraphs[0].style != ParagraphStyle::default()
                    || cell.paragraphs[0].runs.iter().any(|r| {
                        !r.text.is_empty() || r.link.is_some() || r.style != TextStyle::default()
                    })
                {
                    return Err(Error::Unsupported("nonempty covered table cell".into()));
                }
                continue;
            }
            let h = cell.row_span as usize;
            let w = cell.col_span as usize;
            if h == 0 || w == 0 || h > 256 || w > 256 || row + h > height || col + w > width {
                return Err(Error::Invalid("invalid merged cell span".into()));
            }
            for cells in result.iter_mut().skip(row).take(h) {
                for owner in cells.iter_mut().skip(col).take(w) {
                    if *owner != unset {
                        return Err(Error::Invalid("overlapping merged cells".into()));
                    }
                    *owner = (row, col);
                }
            }
        }
    }
    Ok(result)
}

/// Collapse per-row horizontal groups with vertical continuation markers into
/// a rectangular anchor while preserving only representable, empty covered cells.
pub(super) fn resolve_vertical(table: &mut Table, vertical: &[Vec<u8>]) -> Result<(), Error> {
    let height = table.rows.len();
    let width = table.rows.first().map_or(0, Vec::len);
    if vertical.len() != height || vertical.iter().any(|r| r.len() != width) {
        return Err(Error::Invalid("vertical table descriptor count".into()));
    }
    let horizontal = table
        .rows
        .iter()
        .map(|row| horizontal_spans(row))
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
                return Err(Error::Unsupported("inconsistent vertical merge".into()));
            }
            if flag == 2 {
                if !consumed[row][col] {
                    return Err(Error::Invalid(
                        "vertical continuation without anchor".into(),
                    ));
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
                    let border = table.rows[row][col].border.clone();
                    let cell = &mut table.rows[bottom][col];
                    if cell.fill.is_some() && cell.fill != fill
                        || cell.border.is_some() && cell.border != border
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
                    cell.border = None;
                    cell.col_span = 1;
                    consumed[bottom][col] = true;
                    bottom += 1;
                }
                table.rows[row][col].row_span =
                    u16::try_from(bottom - row).map_err(|_| Error::Limit)?;
            } else if flag != 0 {
                return Err(Error::Invalid("invalid vertical merge marker".into()));
            }
        }
    }
    table_layout(table)?;
    Ok(())
}

/// Reconstruct a uniform border from the physical perimeter of each cell span.
pub(super) fn apply_perimeter_borders(
    table: &mut Table,
    borders: &[Vec<[Option<Border>; 4]>],
) -> Result<(), Error> {
    let layout = table_layout(table)?;
    let height = table.rows.len();
    let width = table.rows[0].len();
    if borders.len() != height || borders.iter().any(|r| r.len() != width) {
        return Err(Error::Invalid("table border descriptor count".into()));
    }
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
                    return Err(Error::Unsupported("nonuniform table borders".into()));
                }
                table.rows[row][col].border = Some((*first).clone());
            }
        }
    }
    Ok(())
}

pub(super) fn column_widths(table: &Table) -> Vec<u32> {
    if table.column_widths_twips.is_empty() {
        vec![1440; table.rows.first().map_or(0, Vec::len)]
    } else {
        table.column_widths_twips.clone()
    }
}

/// Validate and return whether each position is a merge anchor or continuation.
pub(super) fn horizontal_spans(row: &[Cell]) -> Result<Vec<u16>, Error> {
    let mut result = vec![0; row.len()];
    let mut i = 0;
    while i < row.len() {
        let cell = &row[i];
        let span = cell.col_span as usize;
        if cell.row_span != 1 || span == 0 || span > 256 || i + span > row.len() {
            return Err(Error::Unsupported("invalid or vertical merged cell".into()));
        }
        result[i] = cell.col_span;
        for covered in &row[i + 1..i + span] {
            if covered.row_span != 1
                || covered.col_span != 1
                || covered.border.is_some()
                || covered.fill.is_some()
                || covered.paragraphs.len() != 1
                || covered.paragraphs[0].style != ParagraphStyle::default()
                || covered.paragraphs[0].runs.iter().any(|r| {
                    !r.text.is_empty() || r.link.is_some() || r.style != TextStyle::default()
                })
            {
                return Err(Error::Unsupported("nonempty covered table cell".into()));
            }
        }
        i += span;
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rectangular_merge_preserves_contents_in_html_and_markdown() {
        let mut doc = RichDocument::new();
        doc.transact(|s| {
            let id = s.next_id;
            s.next_id += 1;
            let mut rows = Vec::new();
            for row in 0..2 {
                let mut cells = Vec::new();
                for col in 0..2 {
                    let pid = s.next_id;
                    s.next_id += 1;
                    cells.push(Cell {
                        paragraphs: vec![Paragraph {
                            id: pid,
                            style: ParagraphStyle::default(),
                            runs: vec![Run {
                                text: format!("{row},{col}"),
                                style: TextStyle::default(),
                                link: None,
                            }],
                        }],
                        row_span: 1,
                        col_span: 1,
                        border: None,
                        fill: None,
                    });
                }
                rows.push(cells);
            }
            rows[0][0].fill = Some(0x12abef);
            rows[0][0].border = Some(Border {
                width_twips: 20,
                color: 0x334455,
            });
            rows[1][1].fill = Some(0xaabbcc);
            s.blocks.push(Block::Table(Table {
                id,
                rows,
                column_widths_twips: vec![1000, 2000],
            }));
            Ok(())
        })
        .unwrap();
        let original = doc.snapshot();
        let Block::Table(t) = &doc.state().blocks[1] else {
            panic!()
        };
        let id = t.id;
        doc.merge_table_rectangle(id, 0, 0, 1, 1).unwrap();
        let projection = doc.rich_edit_projection().unwrap();
        doc.apply_rich_edit_projection(&projection, (0, 0)).unwrap();
        for format in [
            Format::Project,
            Format::Rtf,
            Format::Docx,
            Format::Odt,
            Format::Html,
            Format::Markdown,
        ] {
            let bytes = doc.encode(format).unwrap();
            let reopened = RichDocument::decode(&bytes, format).unwrap();
            let Block::Table(t) = &reopened.state().blocks[1] else {
                panic!()
            };
            assert_eq!(
                (t.rows[0][0].row_span, t.rows[0][0].col_span),
                (2, 2),
                "{format:?}"
            );
            assert_eq!(t.rows[0][0].fill, Some(0x12abef), "{format:?}");
            assert_eq!(
                t.rows[0][0].border,
                Some(Border {
                    width_twips: 20,
                    color: 0x334455
                }),
                "{format:?}"
            );
            assert_eq!(
                t.rows[0][0]
                    .paragraphs
                    .iter()
                    .map(|p| p.runs[0].text.as_str())
                    .collect::<Vec<_>>(),
                ["0,0", "0,1", "1,0", "1,1"]
            );
        }
        assert!(doc.undo());
        assert_eq!(doc.state(), &original);
    }
    #[test]
    fn cell_background_survives_projection_and_office_formats() {
        let mut doc = RichDocument::new();
        doc.transact(|s| {
            let id = s.next_id;
            s.next_id += 1;
            let pid = s.next_id;
            s.next_id += 1;
            s.blocks.push(Block::Table(Table {
                column_widths_twips: Vec::new(),
                id,
                rows: vec![vec![Cell {
                    paragraphs: vec![Paragraph {
                        id: pid,
                        style: ParagraphStyle::default(),
                        runs: vec![Run {
                            text: "text".into(),
                            style: TextStyle::default(),
                            link: None,
                        }],
                    }],
                    row_span: 1,
                    col_span: 1,
                    border: Some(Border {
                        width_twips: 20,
                        color: 0x334455,
                    }),
                    fill: Some(0x12abef),
                }]],
            }));
            Ok(())
        })
        .unwrap();
        let rtf = doc.rich_edit_projection().unwrap();
        doc.apply_rich_edit_projection(&rtf, (0, 0)).unwrap();
        for format in [Format::Project, Format::Rtf, Format::Docx, Format::Odt] {
            let data = doc.encode(format).unwrap();
            let copy = RichDocument::decode(&data, format).unwrap();
            let Block::Table(t) = &copy.state().blocks[1] else {
                panic!()
            };
            assert_eq!(t.rows[0][0].fill, Some(0x12abef), "{format:?}");
            assert_eq!(
                t.rows[0][0].border,
                Some(Border {
                    width_twips: 20,
                    color: 0x334455
                }),
                "{format:?}"
            );
        }
    }
    #[test]
    fn merge_split_keeps_all_paragraphs_and_undo() {
        let mut doc = RichDocument::new();
        doc.transact(|s| {
            let mut cells = Vec::new();
            for text in ["one", "two", "three"] {
                let id = s.next_id;
                s.next_id += 1;
                cells.push(Cell {
                    paragraphs: vec![Paragraph {
                        id,
                        style: ParagraphStyle::default(),
                        runs: vec![Run {
                            text: text.into(),
                            style: TextStyle::default(),
                            link: None,
                        }],
                    }],
                    row_span: 1,
                    col_span: 1,
                    border: None,
                    fill: None,
                });
            }
            cells[0].fill = Some(0x112233);
            cells[1].fill = Some(0xaabbcc);
            cells[1].border = Some(Border {
                width_twips: 20,
                color: 0x223344,
            });
            let id = s.next_id;
            s.next_id += 1;
            s.blocks.push(Block::Table(Table {
                column_widths_twips: Vec::new(),
                id,
                rows: vec![cells],
            }));
            Ok(())
        })
        .unwrap();
        let original = doc.snapshot();
        let Block::Table(t) = &doc.state().blocks[1] else {
            panic!()
        };
        let id = t.id;
        doc.merge_table_cells(id, 0, 0, 2).unwrap();
        let merged = doc.snapshot();
        let projection = doc.rich_edit_projection().unwrap();
        doc.apply_rich_edit_projection(&projection, (0, 0)).unwrap();
        let Block::Table(projected) = &doc.state().blocks[1] else {
            panic!()
        };
        assert_eq!(horizontal_spans(&projected.rows[0]).unwrap(), vec![3, 0, 0]);
        let encoded = doc.encode(Format::Rtf).unwrap();
        let reopened = RichDocument::decode(&encoded, Format::Rtf).unwrap();
        let Block::Table(rt) = &reopened.state().blocks[1] else {
            panic!()
        };
        assert_eq!(horizontal_spans(&rt.rows[0]).unwrap(), vec![3, 0, 0]);
        assert_eq!(
            rt.rows[0][0]
                .paragraphs
                .iter()
                .map(|p| p.runs[0].text.as_str())
                .collect::<Vec<_>>(),
            ["one", "two", "three"]
        );
        let odt = doc.encode(Format::Odt).unwrap();
        let odt_doc = RichDocument::decode(&odt, Format::Odt).unwrap();
        let Block::Table(odt_table) = &odt_doc.state().blocks[1] else {
            panic!()
        };
        assert_eq!(horizontal_spans(&odt_table.rows[0]).unwrap(), vec![3, 0, 0]);
        assert_eq!(
            odt_table.rows[0][0]
                .paragraphs
                .iter()
                .map(|p| p.runs[0].text.as_str())
                .collect::<Vec<_>>(),
            ["one", "two", "three"]
        );
        let word = doc.encode(Format::Docx).unwrap();
        let word_doc = RichDocument::decode(&word, Format::Docx).unwrap();
        let Block::Table(word_table) = &word_doc.state().blocks[1] else {
            panic!()
        };
        assert_eq!(
            horizontal_spans(&word_table.rows[0]).unwrap(),
            vec![3, 0, 0]
        );
        assert_eq!(
            word_table.rows[0][0]
                .paragraphs
                .iter()
                .map(|p| p.runs[0].text.as_str())
                .collect::<Vec<_>>(),
            ["one", "two", "three"]
        );
        let Block::Table(t) = &doc.state().blocks[1] else {
            panic!()
        };
        assert_eq!(horizontal_spans(&t.rows[0]).unwrap(), vec![3, 0, 0]);
        assert_eq!(t.rows[0][0].fill, Some(0x112233));
        let contents: Vec<_> = t.rows[0][0]
            .paragraphs
            .iter()
            .map(|p| p.runs[0].text.as_str())
            .collect();
        assert_eq!(contents, ["one", "two", "three"]);
        doc.split_table_cell(id, 0, 0).unwrap();
        assert!(doc.undo());
        assert_eq!(doc.state(), &merged);
        assert!(doc.undo());
        assert_eq!(doc.state(), &original);
    }
}
