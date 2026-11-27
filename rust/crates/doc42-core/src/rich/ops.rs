//! State-level editing operations. Each one changes a [`State`] in place, so a host can run several of them inside
//! one [`RichDocument::transact`] and get one Undo step (the stage-2 command layer in `edit42` does that).
//! Offsets are UTF-8 byte offsets within a paragraph's text; positions follow the same rules as the rest of the core.
//! Text edits refuse paragraphs anchored by revisions, fields or a cached table of contents, like
//! [`RichDocument::replace_text`]; comments, bookmarks and notes move with the text around them.
use super::*;

/// The paragraph's text (all runs).
pub fn text(p: &Paragraph) -> String {
    p.runs.iter().map(|r| r.text.as_str()).collect()
}

/// A paragraph in the body or in a table cell.
pub fn paragraph(s: &State, id: u64) -> Option<&Paragraph> {
    for block in &s.blocks {
        match block {
            Block::Paragraph(p) if p.id == id => return Some(p),
            Block::Table(t) => {
                if let Some(p) = t.rows.iter().flatten().flat_map(|c| &c.paragraphs).find(|p| p.id == id) {
                    return Some(p);
                }
            }
            _ => {}
        }
    }
    None
}

pub fn paragraph_mut(s: &mut State, id: u64) -> Option<&mut Paragraph> {
    super::paragraph_mut(s, id)
}

/// The block index of a body paragraph (not one inside a table).
pub fn body_index(s: &State, id: u64) -> Option<usize> {
    s.blocks.iter().position(|b| matches!(b, Block::Paragraph(p) if p.id == id))
}

/// A new paragraph or list id.
pub fn allocate_id(s: &mut State) -> Result<u64, Error> {
    let id = s.next_id;
    s.next_id = s.next_id.checked_add(1).ok_or(Error::Limit)?;
    Ok(id)
}

/// Whether the text of a paragraph may be edited (no revision or field anchors, no cached TOC entry).
pub fn check_text_editable(s: &State, id: u64) -> Result<(), Error> {
    if cached_toc_paragraph(s, id) {
        return Err(Error::Unsupported(
            "update the imported TOC before editing its cached entries".into(),
        ));
    }
    if s.revisions.iter().any(|x| x.start.paragraph == id || x.end.paragraph == id)
        || s.fields.iter().any(|x| x.at.paragraph == id)
    {
        return Err(Error::Unsupported("editing revision or field anchors".into()));
    }
    Ok(())
}

fn checked_range(text: &str, range: &std::ops::Range<usize>) -> Result<(), Error> {
    if range.start > range.end
        || range.end > text.len()
        || !text.is_char_boundary(range.start)
        || !text.is_char_boundary(range.end)
    {
        return Err(Error::Invalid("range outside paragraph or inside a character".into()));
    }
    Ok(())
}

/// Joins neighbouring runs with the same style and link, drops empty runs and keeps one empty run (with the style
/// of the first run) in an empty paragraph, so typing there keeps that style.
pub fn normalize_runs(p: &mut Paragraph) {
    let fallback = p.runs.first().map(|r| r.style.clone()).unwrap_or_default();
    let mut out: Vec<Run> = Vec::with_capacity(p.runs.len());
    for run in std::mem::take(&mut p.runs) {
        if run.text.is_empty() {
            continue;
        }
        match out.last_mut() {
            Some(last) if last.style == run.style && last.link == run.link => last.text.push_str(&run.text),
            _ => out.push(run),
        }
    }
    if out.is_empty() {
        out.push(Run { text: String::new(), style: fallback, link: None });
    }
    p.runs = out;
}

/// The style of the text just before `offset` (or of the first run at the start): what typed text inherits.
pub fn style_at(p: &Paragraph, offset: usize) -> TextStyle {
    let mut pos = 0;
    let mut style = p.runs.first().map(|r| r.style.clone()).unwrap_or_default();
    for run in &p.runs {
        if run.text.is_empty() {
            continue;
        }
        if pos < offset {
            style = run.style.clone();
        }
        pos += run.text.len();
    }
    style
}

/// Replaces a range of one paragraph with text (no line breaks) in `style`.
pub fn replace_text(
    s: &mut State,
    paragraph: u64,
    range: std::ops::Range<usize>,
    text: &str,
    style: TextStyle,
) -> Result<(), Error> {
    check_text_editable(s, paragraph)?;
    if text.contains(['\r', '\n', '\0']) {
        return Err(Error::Invalid("line breaks start a new paragraph".into()));
    }
    let p = paragraph_mut(s, paragraph).ok_or_else(|| Error::Invalid("paragraph not found".into()))?;
    let all = self::text(p);
    checked_range(&all, &range)?;
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
    let keep = style.clone();
    if !text.is_empty() {
        out.push(Run { text: text.into(), style, link: None });
    }
    pos = 0;
    for run in &p.runs {
        let end = pos + run.text.len();
        if end > range.end {
            let n = range.end.saturating_sub(pos);
            let mut r = run.clone();
            r.text = run.text[n..].into();
            out.push(r);
        }
        pos = end;
    }
    if out.iter().all(|r| r.text.is_empty()) {
        out = vec![Run { text: String::new(), style: keep, link: None }];
    }
    p.runs = out;
    normalize_runs(p);
    annotations::remap_edit(s, paragraph, range, text.len());
    Ok(())
}

/// Changes the character style of a range from each run's current style (for example bold on, the rest kept).
pub fn style_text(
    s: &mut State,
    paragraph: u64,
    range: std::ops::Range<usize>,
    patch: &dyn Fn(&TextStyle) -> TextStyle,
) -> Result<(), Error> {
    if cached_toc_paragraph(s, paragraph) {
        return Err(Error::Unsupported(
            "update the imported TOC before editing its cached entries".into(),
        ));
    }
    let p = paragraph_mut(s, paragraph).ok_or_else(|| Error::Invalid("paragraph not found".into()))?;
    checked_range(&text(p), &range)?;
    if range.is_empty() {
        return Ok(());
    }
    let mut out = Vec::new();
    let mut pos = 0;
    for run in &p.runs {
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
            part.style = patch(&run.style);
            out.push(part);
        }
        if right < run.text.len() {
            let mut part = run.clone();
            part.text = run.text[right..].into();
            out.push(part);
        }
        pos += run.text.len();
    }
    p.runs = out;
    normalize_runs(p);
    Ok(())
}

/// Sets a paragraph's style (alignment, list, outline level, …).
pub fn set_paragraph_style(s: &mut State, paragraph: u64, style: ParagraphStyle) -> Result<(), Error> {
    if cached_toc_paragraph(s, paragraph) {
        return Err(Error::Unsupported(
            "update the imported TOC before editing its cached entries".into(),
        ));
    }
    paragraph_mut(s, paragraph).ok_or_else(|| Error::Invalid("paragraph not found".into()))?.style = style;
    Ok(())
}

fn shift_sections_for_insert(s: &mut State, index: usize) {
    for section in s.sections.iter_mut().skip(1) {
        if section.start_block >= index {
            section.start_block += 1;
        }
    }
}

fn shift_sections_for_removal(s: &mut State, index: usize) {
    for section in s.sections.iter_mut() {
        if section.start_block > index {
            section.start_block -= 1;
        }
    }
    // A section whose only block was removed disappears; the one after it starts there.
    let len = s.blocks.len();
    let mut kept: Vec<Section> = Vec::with_capacity(s.sections.len());
    for section in std::mem::take(&mut s.sections) {
        if section.start_block >= len {
            continue;
        }
        if kept.last().is_some_and(|last| last.start_block == section.start_block) {
            kept.pop();
        }
        kept.push(section);
    }
    if let Some(first) = kept.first_mut() {
        first.start_block = 0;
    }
    s.sections = kept;
}

/// Moves every anchor in `from` paragraph at `offset >= at` (starts) or `> at` (ends) to `to`, shifted by `delta`.
fn move_anchors(s: &mut State, from: u64, at: usize, to: u64, shift: impl Fn(usize) -> usize, keep_equal: bool) {
    let mv = |p: &mut Position, start: bool| {
        if p.paragraph == from && (p.offset > at || (p.offset == at && start && !keep_equal)) {
            p.paragraph = to;
            p.offset = shift(p.offset);
        }
    };
    for c in &mut s.comments {
        mv(&mut c.start, true);
        mv(&mut c.end, false);
    }
    for b in &mut s.bookmarks {
        mv(&mut b.start, true);
        mv(&mut b.end, false);
    }
    for n in &mut s.footnotes {
        mv(&mut n.at, true);
    }
}

/// Splits a body paragraph at `at`: the text after it moves into a new paragraph with the same style, inserted
/// after it. Returns the new paragraph's id.
pub fn split_paragraph(s: &mut State, paragraph: u64, at: usize) -> Result<u64, Error> {
    check_text_editable(s, paragraph)?;
    let index = body_index(s, paragraph).ok_or_else(|| Error::Invalid("body paragraph not found".into()))?;
    let new_id = allocate_id(s)?;
    let Block::Paragraph(p) = &mut s.blocks[index] else { unreachable!() };
    let all = text(p);
    checked_range(&all, &(at..at))?;
    let length = all.len();
    let tail_style = style_at(p, at);
    let head_style = p.runs.first().map(|r| r.style.clone()).unwrap_or_default();
    let mut head = Vec::new();
    let mut tail = Vec::new();
    let mut pos = 0;
    for run in &p.runs {
        let cut = at.saturating_sub(pos).min(run.text.len());
        if cut > 0 {
            let mut r = run.clone();
            r.text = run.text[..cut].into();
            head.push(r);
        }
        if cut < run.text.len() {
            let mut r = run.clone();
            r.text = run.text[cut..].into();
            tail.push(r);
        }
        pos += run.text.len();
    }
    if head.is_empty() {
        head.push(Run { text: String::new(), style: head_style, link: None });
    }
    if tail.is_empty() {
        tail.push(Run { text: String::new(), style: tail_style, link: None });
    }
    let mut next = Paragraph { id: new_id, style: p.style.clone(), runs: tail };
    p.runs = head;
    normalize_runs(p);
    normalize_runs(&mut next);
    s.blocks.insert(index + 1, Block::Paragraph(next));
    shift_sections_for_insert(s, index + 1);
    move_anchors(s, paragraph, at, new_id, |o| o - at, at >= length);
    Ok(new_id)
}

/// Inserts a new body paragraph at block `index`; returns its id.
pub fn insert_paragraph(s: &mut State, index: usize, style: ParagraphStyle, runs: Vec<Run>) -> Result<u64, Error> {
    if index > s.blocks.len() {
        return Err(Error::Invalid("insertion point outside the document".into()));
    }
    let id = allocate_id(s)?;
    let mut p = Paragraph { id, style, runs };
    if p.runs.iter().any(|r| r.text.contains(['\r', '\n', '\0'])) {
        return Err(Error::Invalid("line breaks start a new paragraph".into()));
    }
    normalize_runs(&mut p);
    s.blocks.insert(index, Block::Paragraph(p));
    shift_sections_for_insert(s, index);
    Ok(id)
}

fn block_paragraph_ids(block: &Block) -> Vec<u64> {
    match block {
        Block::Paragraph(p) => vec![p.id],
        Block::Table(t) => t.rows.iter().flatten().flat_map(|c| c.paragraphs.iter().map(|p| p.id)).collect(),
        _ => vec![],
    }
}

/// Removes block `index`. Comments and bookmarks anchored in it move to `to` (or are removed when `to` is None and
/// they are entirely inside); notes anchored in it are deleted with it.
pub fn remove_block(s: &mut State, index: usize, to: Option<Position>) -> Result<(), Error> {
    if index >= s.blocks.len() {
        return Err(Error::Invalid("block outside the document".into()));
    }
    if s.blocks.len() == 1 {
        return Err(Error::Invalid("a document keeps at least one paragraph".into()));
    }
    let ids = block_paragraph_ids(&s.blocks[index]);
    for id in &ids {
        check_text_editable(s, *id)?;
    }
    let inside = |p: &Position| ids.contains(&p.paragraph);
    s.footnotes.retain(|n| !inside(&n.at));
    match &to {
        Some(to) => {
            for c in &mut s.comments {
                for p in [&mut c.start, &mut c.end] {
                    if ids.contains(&p.paragraph) {
                        *p = to.clone();
                    }
                }
            }
            for b in &mut s.bookmarks {
                for p in [&mut b.start, &mut b.end] {
                    if ids.contains(&p.paragraph) {
                        *p = to.clone();
                    }
                }
            }
        }
        None => {
            s.comments.retain(|c| !(inside(&c.start) && inside(&c.end)));
            s.bookmarks.retain(|b| !(inside(&b.start) && inside(&b.end)));
            if s.comments.iter().any(|c| inside(&c.start) || inside(&c.end))
                || s.bookmarks.iter().any(|b| inside(&b.start) || inside(&b.end))
            {
                return Err(Error::Unsupported("a comment or bookmark continues outside the deleted paragraph".into()));
            }
        }
    }
    s.blocks.remove(index);
    shift_sections_for_removal(s, index);
    Ok(())
}

/// Deletes from `from` to `to` across body paragraphs (and anything between them, tables included); the rest of
/// the last paragraph joins the first.
pub fn delete_range(s: &mut State, from: &Position, to: &Position) -> Result<(), Error> {
    if from.paragraph == to.paragraph {
        let style = paragraph(s, from.paragraph).map(|p| style_at(p, from.offset)).unwrap_or_default();
        return replace_text(s, from.paragraph, from.offset..to.offset, "", style);
    }
    let first = body_index(s, from.paragraph).ok_or_else(|| Error::Invalid("body paragraph not found".into()))?;
    let last = body_index(s, to.paragraph).ok_or_else(|| Error::Invalid("body paragraph not found".into()))?;
    if first > last {
        return Err(Error::Invalid("the range ends before it starts".into()));
    }
    check_text_editable(s, from.paragraph)?;
    check_text_editable(s, to.paragraph)?;
    // Cut the end of the first paragraph and the start of the last one.
    let first_text = text(paragraph(s, from.paragraph).unwrap());
    let style = style_at(paragraph(s, from.paragraph).unwrap(), from.offset);
    replace_text(s, from.paragraph, from.offset..first_text.len(), "", style.clone())?;
    let last_style = paragraph(s, to.paragraph).map(|p| style_at(p, to.offset)).unwrap_or_default();
    replace_text(s, to.paragraph, 0..to.offset, "", last_style)?;
    // Blocks in between go; their anchors move to the join point.
    let join = Position { paragraph: from.paragraph, offset: from.offset };
    for _ in first + 1..last {
        remove_block(s, first + 1, Some(join.clone()))?;
    }
    // The rest of the last paragraph joins the first.
    let Block::Paragraph(tail) = s.blocks[first + 1].clone() else { unreachable!() };
    move_anchors(s, tail.id, 0, from.paragraph, |o| o + from.offset, false);
    for c in &mut s.comments {
        for p in [&mut c.start, &mut c.end] {
            if p.paragraph == tail.id {
                *p = join.clone();
            }
        }
    }
    for b in &mut s.bookmarks {
        for p in [&mut b.start, &mut b.end] {
            if p.paragraph == tail.id {
                *p = join.clone();
            }
        }
    }
    for n in &mut s.footnotes {
        if n.at.paragraph == tail.id {
            n.at = join.clone();
        }
    }
    let Block::Paragraph(head) = &mut s.blocks[first] else { unreachable!() };
    head.runs.extend(tail.runs);
    normalize_runs(head);
    s.blocks.remove(first + 1);
    shift_sections_for_removal(s, first + 1);
    Ok(())
}
