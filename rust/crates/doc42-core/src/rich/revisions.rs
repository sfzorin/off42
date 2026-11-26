//! Tracked changes: revision ranges over body text, Accept/Reject, editor markup and
//! recording of typed edits while Track Changes is on.
//!
//! Deleted text stays in the document inside a deletion revision until it is accepted.
//! The Rich Edit projection shows revisions with reserved marker colours; those
//! markers are display-only and are removed again when an edited projection returns.
use super::annotations::{
    changed_range, global_position, linear_body, local_position, remap_offset, LinearBody,
};
use super::*;
use std::ops::Range;

/// Insertion that gained a display underline.
pub(super) const INSERT_MARK: u32 = 0x1A5F_B4;
/// Insertion whose own formatting is already underlined.
pub(super) const INSERT_KEEP: u32 = 0x1A5F_B5;
/// Deleted text (shown struck through).
pub(super) const DELETE_MARK: u32 = 0xC0_1C28;

fn text_of(p: &Paragraph) -> String {
    p.runs.iter().map(|r| r.text.as_str()).collect()
}

fn paragraph<'a>(s: &'a State, id: u64) -> Option<&'a Paragraph> {
    s.blocks.iter().find_map(|block| match block {
        Block::Paragraph(p) if p.id == id => Some(p),
        Block::Table(t) => t
            .rows
            .iter()
            .flatten()
            .flat_map(|c| &c.paragraphs)
            .find(|p| p.id == id),
        _ => None,
    })
}

/// Per-paragraph byte ranges covered by a revision, in document order.
pub(super) fn segments(
    s: &State,
    start: &Position,
    end: &Position,
) -> Result<Vec<(u64, Range<usize>)>, Error> {
    if start.paragraph == end.paragraph {
        if start.offset > end.offset {
            return Err(Error::Invalid("reversed revision range".into()));
        }
        return Ok(vec![(start.paragraph, start.offset..end.offset)]);
    }
    let mut out = Vec::new();
    let mut inside = false;
    for block in &s.blocks {
        match block {
            Block::Paragraph(p) => {
                let len = p.runs.iter().map(|r| r.text.len()).sum();
                if p.id == start.paragraph {
                    inside = true;
                    out.push((p.id, start.offset..len));
                } else if p.id == end.paragraph {
                    if !inside {
                        return Err(Error::Invalid("reversed revision range".into()));
                    }
                    out.push((p.id, 0..end.offset));
                    return Ok(out);
                } else if inside {
                    out.push((p.id, 0..len));
                }
            }
            _ if inside => {
                return Err(Error::Unsupported(
                    "a tracked change cannot span a table or picture".into(),
                ))
            }
            _ => {}
        }
    }
    Err(Error::Invalid("revision range outside body".into()))
}

/// Split runs at `range` and transform the covered pieces.
fn split_runs(p: &mut Paragraph, range: Range<usize>, mut f: impl FnMut(&mut Run)) {
    let mut out = Vec::with_capacity(p.runs.len() + 2);
    let mut pos = 0;
    for run in std::mem::take(&mut p.runs) {
        let end = pos + run.text.len();
        let a = range.start.clamp(pos, end);
        let b = range.end.clamp(pos, end);
        if run.text.is_empty() {
            out.push(run);
            continue;
        }
        let piece = |from: usize, to: usize| Run {
            text: run.text[from - pos..to - pos].into(),
            style: run.style.clone(),
            link: run.link.clone(),
        };
        if a > pos {
            out.push(piece(pos, a));
        }
        if b > a {
            let mut covered = piece(a, b);
            f(&mut covered);
            out.push(covered);
        }
        if end > b {
            out.push(piece(b, end));
        }
        pos = end;
    }
    p.runs = out;
}

fn slice_runs(p: &Paragraph, range: Range<usize>) -> Vec<Run> {
    let mut copy = p.clone();
    let mut kept = Vec::new();
    split_runs(&mut copy, range, |run| kept.push(run.clone()));
    kept
}

fn replace_runs(p: &mut Paragraph, range: Range<usize>, runs: Vec<Run>) {
    let len: usize = p.runs.iter().map(|r| r.text.len()).sum();
    let mut out = slice_runs(p, 0..range.start);
    out.extend(runs);
    out.extend(slice_runs(p, range.end..len));
    out.retain(|r| !r.text.is_empty());
    if out.is_empty() {
        let style = p.runs.first().map(|r| r.style.clone()).unwrap_or_default();
        out.push(Run {
            text: String::new(),
            style,
            link: None,
        });
    }
    p.runs = out;
}

fn each_paragraph_mut(s: &mut State, mut f: impl FnMut(&mut Paragraph)) {
    for block in &mut s.blocks {
        match block {
            Block::Paragraph(p) => f(p),
            Block::Table(t) => t
                .rows
                .iter_mut()
                .flatten()
                .flat_map(|c| &mut c.paragraphs)
                .for_each(&mut f),
            _ => {}
        }
    }
}

/// Merge neighbouring runs whose formatting and link are identical.
pub(super) fn coalesce(p: &mut Paragraph) {
    let mut out: Vec<Run> = Vec::with_capacity(p.runs.len());
    for run in std::mem::take(&mut p.runs) {
        match out.last_mut() {
            Some(last) if last.style == run.style && last.link == run.link => {
                last.text.push_str(&run.text)
            }
            _ => out.push(run),
        }
    }
    p.runs = out;
}

pub(super) fn coalesce_all(s: &mut State) {
    each_paragraph_mut(s, coalesce);
}

/// Apply display markup for every revision; the view keeps no revision list.
pub(super) fn markup(view: &mut State) -> Result<(), Error> {
    let revisions = std::mem::take(&mut view.revisions);
    for revision in &revisions {
        for (id, range) in segments(view, &revision.start, &revision.end)? {
            let p = paragraph_mut(view, id).ok_or_else(|| Error::Invalid("revision".into()))?;
            split_runs(p, range, |run| {
                let t = &mut run.style;
                if revision.insertion {
                    if t.color.is_none() {
                        if t.underline {
                            t.color = Some(INSERT_KEEP);
                        } else {
                            t.underline = true;
                            t.color = Some(INSERT_MARK);
                        }
                    }
                } else {
                    t.strike = true;
                    t.color = Some(DELETE_MARK);
                }
            });
        }
    }
    Ok(())
}

/// Remove display markers that returned from the editor, including on newly typed text.
pub(super) fn clean_markup(s: &mut State) {
    each_paragraph_mut(s, |p| {
        for run in &mut p.runs {
            let t = &mut run.style;
            match t.color {
                Some(INSERT_MARK) => {
                    t.color = None;
                    t.underline = false;
                }
                Some(INSERT_KEEP) => t.color = None,
                Some(DELETE_MARK) => {
                    t.color = None;
                    t.strike = false;
                }
                _ => {}
            }
        }
    });
}

/// Deleted text is not editable; when it returns unchanged, restore its exact runs.
pub(super) fn restore_deleted(old: &State, parsed: &mut State) -> Result<(), Error> {
    for new in parsed.revisions.clone().iter().filter(|r| !r.insertion) {
        let Some(before) = old.revisions.iter().find(|r| r.id == new.id) else {
            continue;
        };
        let a = segments(old, &before.start, &before.end)?;
        let b = segments(parsed, &new.start, &new.end)?;
        let text = |s: &State, (id, range): &(u64, Range<usize>)| {
            paragraph(s, *id).map(|p| text_of(p)[range.clone()].to_owned())
        };
        let same = a.len() == b.len()
            && a.iter().zip(&b).all(|(x, y)| text(old, x).is_some() && text(old, x) == text(parsed, y));
        if !same {
            continue;
        }
        for ((old_id, old_range), (new_id, new_range)) in a.into_iter().zip(b) {
            let runs = slice_runs(paragraph(old, old_id).unwrap(), old_range);
            let p = paragraph_mut(parsed, new_id).ok_or_else(|| Error::Invalid("revision".into()))?;
            replace_runs(p, new_range, runs);
        }
    }
    Ok(())
}

/// Re-anchor every annotation after one contiguous edit of `start..end` replaced by
/// `inserted` bytes, either in the linear body or inside one table-cell paragraph.
fn remap_anchors(
    s: &mut State,
    old: Option<&LinearBody>,
    local: Option<u64>,
    start: usize,
    end: usize,
    inserted: usize,
) -> Result<(), Error> {
    let new = linear_body(s);
    let map = |position: &Position, right: bool| -> Result<Position, Error> {
        if let (Some(old), Some(new)) = (old, new.as_ref()) {
            if let Some(global) = global_position(old, position) {
                return local_position(new, remap_offset(global, start, end, inserted, right));
            }
        }
        if local == Some(position.paragraph) {
            return Ok(Position {
                paragraph: position.paragraph,
                offset: remap_offset(position.offset, start, end, inserted, right),
            });
        }
        Ok(position.clone())
    };
    let removed = |position: &Position| -> bool {
        if start == end {
            return false;
        }
        let offset = match (old, local) {
            (Some(old), _) => global_position(old, position),
            (None, Some(id)) if position.paragraph == id => Some(position.offset),
            _ => None,
        };
        offset.is_some_and(|o| o >= start && o < end)
    };
    let insertion = start == end;
    for comment in &mut s.comments {
        let collapsed = comment.start == comment.end;
        comment.start = map(&comment.start, insertion || collapsed)?;
        comment.end = map(&comment.end, !insertion || collapsed)?;
    }
    for bookmark in &mut s.bookmarks {
        let collapsed = bookmark.start == bookmark.end;
        bookmark.start = map(&bookmark.start, insertion || collapsed)?;
        bookmark.end = map(&bookmark.end, !insertion || collapsed)?;
    }
    let mut revisions = std::mem::take(&mut s.revisions);
    for revision in &mut revisions {
        revision.start = map(&revision.start, true)?;
        revision.end = map(&revision.end, false)?;
    }
    s.revisions = revisions
        .into_iter()
        .filter(|r| r.start != r.end)
        .collect();
    let notes = std::mem::take(&mut s.footnotes);
    for mut note in notes {
        if removed(&note.at) {
            continue;
        }
        note.at = map(&note.at, true)?;
        s.footnotes.push(note);
    }
    let fields = std::mem::take(&mut s.fields);
    for mut field in fields {
        if removed(&field.at) {
            continue;
        }
        field.at = map(&field.at, true)?;
        s.fields.push(field);
    }
    for position in s.selection.anchor.iter_mut().chain(s.selection.focus.iter_mut()) {
        *position = map(position, true)?;
    }
    Ok(())
}

/// Remove text between two positions, joining paragraphs when the range spans breaks.
fn remove_range(s: &mut State, start: &Position, end: &Position) -> Result<(), Error> {
    let body = linear_body(s);
    let global = body
        .as_ref()
        .and_then(|b| Some((global_position(b, start)?, global_position(b, end)?)));
    if let Some((a, b)) = global {
        if a >= b {
            return Ok(());
        }
        let parts = segments(s, start, end)?;
        let ids: Vec<u64> = parts.iter().map(|(id, _)| *id).collect();
        if parts.len() > 1 {
            let gone: std::collections::HashSet<u64> = ids[1..].iter().copied().collect();
            if s.tables_of_contents
                .iter()
                .flat_map(|t| &t.entries)
                .any(|e| gone.contains(&e.paragraph) || gone.contains(&e.heading))
            {
                return Err(Error::Unsupported(
                    "remove or update the table of contents before joining its paragraphs".into(),
                ));
            }
            let last = paragraph(s, *ids.last().unwrap()).unwrap().clone();
            let tail = slice_runs(&last, end.offset..last.runs.iter().map(|r| r.text.len()).sum());
            let first = paragraph_mut(s, ids[0]).unwrap();
            let len: usize = first.runs.iter().map(|r| r.text.len()).sum();
            replace_runs(first, start.offset..len, tail);
            s.blocks
                .retain(|b| !matches!(b, Block::Paragraph(p) if gone.contains(&p.id)));
        } else {
            let p = paragraph_mut(s, start.paragraph).unwrap();
            replace_runs(p, start.offset..end.offset, Vec::new());
        }
        return remap_anchors(s, body.as_ref(), None, a, b, 0);
    }
    if start.paragraph != end.paragraph || start.offset > end.offset {
        return Err(Error::Unsupported("tracked change across table cells".into()));
    }
    let p = paragraph_mut(s, start.paragraph).ok_or_else(|| Error::Invalid("revision".into()))?;
    replace_runs(p, start.offset..end.offset, Vec::new());
    remap_anchors(s, None, Some(start.paragraph), start.offset, end.offset, 0)
}

/// Insert plain text (with `\n` paragraph breaks) at a linear body offset.
fn insert_text(s: &mut State, at: usize, text: &str, style: &TextStyle) -> Result<(), Error> {
    let body = linear_body(s).ok_or_else(|| Error::Unsupported("tracked edit".into()))?;
    let position = local_position(&body, at)?;
    let pieces: Vec<&str> = text.split('\n').collect();
    let run = |text: &str| Run {
        text: text.into(),
        style: style.clone(),
        link: None,
    };
    let p = paragraph_mut(s, position.paragraph).ok_or_else(|| Error::Invalid("insert".into()))?;
    let len: usize = p.runs.iter().map(|r| r.text.len()).sum();
    let tail = slice_runs(p, position.offset..len);
    let paragraph_style = p.style.clone();
    if pieces.len() == 1 {
        replace_runs(p, position.offset..position.offset, vec![run(text)]);
    } else {
        replace_runs(p, position.offset..len, vec![run(pieces[0])]);
        let index = s
            .blocks
            .iter()
            .position(|b| matches!(b, Block::Paragraph(q) if q.id == position.paragraph))
            .ok_or_else(|| Error::Unsupported("tracked paragraph break in a table".into()))?;
        let mut added = Vec::new();
        for (i, piece) in pieces.iter().enumerate().skip(1) {
            let id = s.next_id;
            s.next_id = s.next_id.checked_add(1).ok_or(Error::Limit)?;
            let mut runs = vec![run(piece)];
            if i + 1 == pieces.len() {
                runs.extend(tail.clone());
            }
            runs.retain(|r| !r.text.is_empty());
            if runs.is_empty() {
                runs.push(run(""));
            }
            added.push(Block::Paragraph(Paragraph {
                id,
                style: paragraph_style.clone(),
                runs,
            }));
        }
        s.blocks.splice(index + 1..index + 1, added);
    }
    remap_anchors(s, Some(&body), None, at, at, text.len())
}

fn resolve(s: &mut State, id: u64, accept: bool) -> Result<(), Error> {
    let index = s
        .revisions
        .iter()
        .position(|r| r.id == id)
        .ok_or_else(|| Error::Invalid("tracked change not found".into()))?;
    let revision = s.revisions.remove(index);
    if revision.insertion != accept {
        remove_range(s, &revision.start, &revision.end)?;
    }
    Ok(())
}

/// The document with every change accepted, for formats without revision markup.
pub(super) fn accepted(s: &State) -> Result<State, Error> {
    let mut copy = s.clone();
    while let Some(id) = copy.revisions.first().map(|r| r.id) {
        resolve(&mut copy, id, true)?;
    }
    copy.track_changes = false;
    Ok(copy)
}

/// ISO 8601 UTC timestamp for new revisions.
pub fn now_iso8601() -> String {
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let days = (seconds / 86_400) as i64;
    let time = seconds % 86_400;
    // Civil-from-days (Howard Hinnant), proleptic Gregorian calendar.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        time / 3600,
        time / 60 % 60,
        time % 60
    )
}

fn style_at(s: &State, body: &LinearBody, at: usize) -> TextStyle {
    let Ok(position) = local_position(body, at) else {
        return TextStyle::default();
    };
    let Some(p) = paragraph(s, position.paragraph) else {
        return TextStyle::default();
    };
    let mut pos = 0;
    let mut last = None;
    for run in &p.runs {
        let end = pos + run.text.len();
        if position.offset < end || (position.offset == end && last.is_none()) {
            return run.style.clone();
        }
        last = Some(&run.style);
        pos = end;
    }
    last.cloned().unwrap_or_default()
}

fn span(body: &LinearBody, r: &Revision) -> Option<(usize, usize)> {
    Some((global_position(body, &r.start)?, global_position(body, &r.end)?))
}

fn add_revision(
    s: &mut State,
    body: &LinearBody,
    start: usize,
    end: usize,
    insertion: bool,
    author: &str,
    date: &Option<String>,
) -> Result<(), Error> {
    if start >= end {
        return Ok(());
    }
    for existing in &mut s.revisions {
        if existing.insertion != insertion || existing.author != author {
            continue;
        }
        let Some((a, b)) = span(body, existing) else { continue };
        if a <= start && end <= b {
            return Ok(());
        }
        if b == start || a == end || (a <= end && start <= b) {
            let (a, b) = (a.min(start), b.max(end));
            existing.start = local_position(body, a)?;
            existing.end = local_position(body, b)?;
            existing.date = date.clone();
            return Ok(());
        }
    }
    let id = s.next_id;
    s.next_id = s.next_id.checked_add(1).ok_or(Error::Limit)?;
    s.revisions.push(Revision {
        id,
        start: local_position(body, start)?,
        end: local_position(body, end)?,
        author: author.into(),
        insertion,
        date: date.clone(),
    });
    Ok(())
}

/// How the editor removed text, so the caret ends on the expected side of it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Removal {
    Backward,
    Forward,
}

/// Record one contiguous editor change as tracked insertion/deletion.
/// `parsed` is the edited state with anchors already remapped; Ok(None) keeps it.
pub(super) fn record(
    old: &State,
    parsed: &State,
    author: &str,
    date: Option<String>,
    removal: Removal,
) -> Result<Option<State>, Error> {
    let unsupported = || {
        Error::Unsupported(
            "Track Changes records text edits in body paragraphs; turn it off to edit tables, pictures or sections".into(),
        )
    };
    let (Some(from), Some(to)) = (linear_body(old), linear_body(parsed)) else {
        return Err(unsupported());
    };
    let (start, end, inserted) = changed_range(&from.text, &to.text);
    if start == end && inserted == 0 {
        return Ok(None);
    }
    let typed = &to.text[start..start + inserted];
    if typed.contains('\u{fffc}') || from.text[start..end].contains('\u{fffc}') {
        return Err(unsupported());
    }
    let spans: Vec<(bool, usize, usize)> = old
        .revisions
        .iter()
        .filter_map(|r| span(&from, r).map(|(a, b)| (r.insertion, a, b)))
        .collect();
    if end > start {
        if spans.iter().any(|&(ins, a, b)| ins && a <= start && end <= b) {
            // Removing one's own pending insertion is a real edit.
            return Ok(None);
        }
        let mut s = old.clone();
        let style = style_at(parsed, &to, start);
        if inserted > 0 {
            insert_text(&mut s, end, typed, &style)?;
        }
        let body = linear_body(&s).ok_or_else(unsupported)?;
        let deleted_already = spans.iter().any(|&(ins, a, b)| !ins && a <= start && end <= b);
        if !deleted_already {
            add_revision(&mut s, &body, start, end, false, author, &date)?;
        }
        add_revision(&mut s, &body, end, end + inserted, true, author, &date)?;
        let caret = if inserted > 0 || removal == Removal::Forward {
            end + inserted
        } else {
            start
        };
        let at = local_position(&body, caret)?;
        s.selection = Selection {
            anchor: Some(at.clone()),
            focus: Some(at),
        };
        return Ok(Some(s));
    }
    if spans.iter().any(|&(ins, a, b)| !ins && a < start && start < b) {
        return Err(Error::Unsupported(
            "deleted text cannot be edited; accept or reject the change first".into(),
        ));
    }
    let mut s = parsed.clone();
    add_revision(&mut s, &to, start, start + inserted, true, author, &date)?;
    Ok(Some(s))
}

impl RichDocument {
    pub fn accept_revision(&mut self, id: u64) -> Result<bool, Error> {
        self.transact(|s| resolve(s, id, true))
    }
    pub fn reject_revision(&mut self, id: u64) -> Result<bool, Error> {
        self.transact(|s| resolve(s, id, false))
    }
    pub fn resolve_all_revisions(&mut self, accept: bool) -> Result<bool, Error> {
        self.transact(|s| {
            while let Some(id) = s.revisions.first().map(|r| r.id) {
                resolve(s, id, accept)?;
            }
            Ok(())
        })
    }
    pub fn set_track_changes(&mut self, on: bool) -> Result<bool, Error> {
        self.transact(|s| {
            s.track_changes = on;
            Ok(())
        })
    }
    /// Rich Edit character offset (one unit per paragraph break and picture) of a body
    /// position; None after a table, whose native layout is not linear.
    pub fn native_offset(&self, position: &Position) -> Option<usize> {
        let mut cursor = 0usize;
        for block in &self.state().blocks {
            match block {
                Block::Paragraph(p) => {
                    let text = text_of(p);
                    if p.id == position.paragraph {
                        return Some(cursor + text.get(..position.offset)?.encode_utf16().count());
                    }
                    cursor += text.encode_utf16().count() + 1;
                }
                Block::Image(_) => cursor += 2,
                _ => return None,
            }
        }
        None
    }
    /// Revisions in document order with native ranges, for navigation and review.
    pub fn revision_ranges(&self) -> Vec<(u64, bool, Option<(usize, usize)>)> {
        let s = self.state();
        let body = linear_body(s);
        let mut items: Vec<_> = s
            .revisions
            .iter()
            .map(|r| {
                let order = body.as_ref().and_then(|b| span(b, r)).map_or(usize::MAX, |x| x.0);
                let native = self
                    .native_offset(&r.start)
                    .zip(self.native_offset(&r.end));
                (order, r.id, r.insertion, native)
            })
            .collect();
        items.sort();
        items.into_iter().map(|(_, id, ins, native)| (id, ins, native)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(paragraphs: &[&str]) -> RichDocument {
        let mut d = RichDocument::new();
        d.transact(|s| {
            s.blocks.clear();
            for (i, text) in paragraphs.iter().enumerate() {
                s.blocks.push(Block::Paragraph(Paragraph {
                    id: 10 + i as u64,
                    style: ParagraphStyle::default(),
                    runs: vec![Run {
                        text: (*text).into(),
                        style: TextStyle::default(),
                        link: None,
                    }],
                }));
            }
            s.next_id = 100;
            Ok(())
        })
        .unwrap();
        d
    }
    fn texts(d: &RichDocument) -> Vec<String> {
        d.state()
            .blocks
            .iter()
            .filter_map(|b| match b {
                Block::Paragraph(p) => Some(text_of(p)),
                _ => None,
            })
            .collect()
    }
    fn at(paragraph: u64, offset: usize) -> Position {
        Position { paragraph, offset }
    }

    #[test]
    fn accept_and_reject_insertions_and_deletions_with_undo() {
        let mut d = doc(&["Hello brave world", "Second"]);
        d.transact(|s| {
            s.revisions.push(Revision {
                id: 1,
                start: at(10, 6),
                end: at(10, 12),
                author: "Ada".into(),
                insertion: false,
                date: None,
            });
            s.revisions.push(Revision {
                id: 2,
                start: at(10, 17),
                end: at(11, 0),
                author: "Ada".into(),
                insertion: false,
                date: None,
            });
            s.comments.push(Annotation {
                id: 3,
                start: at(11, 0),
                end: at(11, 6),
                author: "Bob".into(),
                text: "note".into(),
                resolved: false,
                date: None,
                initials: None,
            });
            Ok(())
        })
        .unwrap();
        let before = d.snapshot();
        d.accept_revision(1).unwrap();
        assert_eq!(texts(&d), ["Hello world", "Second"]);
        assert_eq!(d.state().revisions[0].start, at(10, 11));
        d.accept_revision(2).unwrap();
        assert_eq!(texts(&d), ["Hello worldSecond"]);
        assert_eq!(d.state().comments[0].start, at(10, 11));
        assert_eq!(d.state().comments[0].end, at(10, 17));
        d.undo();
        d.undo();
        assert_eq!(d.snapshot(), before);
        d.resolve_all_revisions(false).unwrap();
        assert_eq!(texts(&d), ["Hello brave world", "Second"]);
        assert!(d.state().revisions.is_empty());
    }

    #[test]
    fn markup_round_trips_through_the_editor_projection() {
        let mut d = doc(&["Keep new gone"]);
        d.transact(|s| {
            s.revisions.push(Revision {
                id: 1,
                start: at(10, 5),
                end: at(10, 8),
                author: "Ada".into(),
                insertion: true,
                date: None,
            });
            s.revisions.push(Revision {
                id: 2,
                start: at(10, 9),
                end: at(10, 13),
                author: "Ada".into(),
                insertion: false,
                date: None,
            });
            Ok(())
        })
        .unwrap();
        let projection = d.rich_edit_projection().unwrap();
        let text = String::from_utf8_lossy(&projection);
        assert!(text.contains("\\strike1"));
        let before = d.snapshot();
        let changed = d.apply_rich_edit_projection(&projection, (0, 0)).unwrap();
        assert_eq!(d.snapshot().blocks, before.blocks);
        assert!(!changed);
        assert_eq!(d.snapshot().blocks, before.blocks);
        assert_eq!(d.snapshot().revisions, before.revisions);
    }

    #[test]
    fn richedit_ulnone_after_an_insertion_mark_round_trips() {
        // RichEdit writes the display underline of a tracked insertion back as
        // \ul ... \ulnone; the projection must decode without changing the model.
        let mut d = doc(&["Keep new text"]);
        d.transact(|s| {
            s.revisions.push(Revision {
                id: 1,
                start: at(10, 5),
                end: at(10, 9),
                author: "Ada".into(),
                insertion: true,
                date: None,
            });
            Ok(())
        })
        .unwrap();
        let projection = String::from_utf8(d.rich_edit_projection().unwrap()).unwrap();
        let richedit = projection.replace("\\ul0", "\\ulnone");
        assert!(richedit.contains("\\ulnone"), "{projection}");
        let before = d.snapshot();
        let changed = d.apply_rich_edit_projection(richedit.as_bytes(), (0, 0)).unwrap();
        assert!(!changed);
        assert_eq!(d.snapshot().blocks, before.blocks);
        assert_eq!(d.snapshot().revisions, before.revisions);
    }

    #[test]
    fn typing_with_tracking_records_insertions_and_deletions() {
        let mut d = doc(&["One two"]);
        d.set_track_changes(true).unwrap();
        let edit = |d: &mut RichDocument, text: &str| {
            let mut copy = d.snapshot();
            copy.revisions.clear();
            if let Block::Paragraph(p) = &mut copy.blocks[0] {
                p.runs = vec![Run {
                    text: text.into(),
                    style: TextStyle::default(),
                    link: None,
                }];
            }
            let rtf = rtf::rtf_export(&copy).unwrap();
            d.apply_rich_edit_projection_tracked(&rtf, (0, 0), "Ada", Removal::Backward)
        };
        edit(&mut d, "One big two").unwrap();
        let s = d.state();
        assert_eq!(s.revisions.len(), 1);
        assert!(s.revisions[0].insertion);
        assert_eq!((s.revisions[0].start.offset, s.revisions[0].end.offset), (4, 8));
        // Backspace the "t" of "two" (projection still shows the insertion text).
        edit(&mut d, "One big wo").unwrap();
        assert_eq!(texts(&d), ["One big two"]);
        let deletion = d.state().revisions.iter().find(|r| !r.insertion).unwrap();
        assert_eq!((deletion.start.offset, deletion.end.offset), (8, 9));
        assert_eq!(d.state().selection.focus, Some(at(10, 8)));
        d.resolve_all_revisions(true).unwrap();
        assert_eq!(texts(&d), ["One big wo"]);
    }

    #[test]
    fn accepted_snapshot_exports_to_plain_formats() {
        let mut d = doc(&["A", "B"]);
        d.transact(|s| {
            s.revisions.push(Revision {
                id: 1,
                start: at(10, 1),
                end: at(11, 0),
                author: "Ada".into(),
                insertion: false,
                date: None,
            });
            Ok(())
        })
        .unwrap();
        let text = String::from_utf8(d.encode(Format::Text).unwrap()).unwrap();
        assert_eq!(text.trim_end(), "AB");
        assert_eq!(d.state().revisions.len(), 1);
    }
}
