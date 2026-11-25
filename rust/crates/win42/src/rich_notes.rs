//! Print-only note layout. Body pagination is monotone when reserving note space.
use crate::{PrintGeometry, PrintText, RichPrint, SceneFont, TextAlign};
use std::{
    mem::zeroed,
    ptr::{null, null_mut},
};
use windows_sys::Win32::{Foundation::*, Graphics::Gdi::*, UI::WindowsAndMessaging::*};
const EM_FINDTEXTEXW: u32 = WM_USER + 124;
const EM_FORMATRANGE: u32 = WM_USER + 57;
const EM_GETTEXTLENGTHEX: u32 = WM_USER + 95;
#[repr(C)]
struct CharRange {
    min: i32,
    max: i32,
}
#[repr(C)]
struct FindTextEx {
    range: CharRange,
    text: *const u16,
    found: CharRange,
}
#[repr(C, packed(4))]
struct FormatRange {
    hdc: HDC,
    target: HDC,
    rect: RECT,
    page: RECT,
    range: CharRange,
}
#[repr(C)]
struct TextLength {
    flags: u32,
    codepage: u32,
}
#[derive(Clone)]
struct Glyph {
    text: Vec<u16>,
    width: i32,
    font: SceneFont,
    color: [u8; 3],
}
#[derive(Clone)]
struct Line {
    glyphs: Vec<Glyph>,
    width: i32,
    height: i32,
    align: TextAlign,
}
struct NoteLayout {
    lines: Vec<Line>,
    height: i32,
}
pub struct Page {
    pub start: i32,
    pub end: i32,
    reserve: i32,
    footnotes: Vec<usize>,
    endnotes: Vec<usize>,
}
pub struct Layout {
    pub pages: Vec<Page>,
    notes: Vec<NoteLayout>,
}

pub unsafe fn temporary_edit(existing: HWND, job: &RichPrint) -> Result<(HWND, bool), String> {
    let Some(rtf) = &job.body_rtf else {
        return Ok((existing, false));
    };
    let parent = GetParent(existing);
    let edit = CreateWindowExW(
        0,
        crate::wide("RICHEDIT50W").as_ptr(),
        null(),
        WS_CHILD | ES_MULTILINE as u32,
        0,
        0,
        1,
        1,
        parent,
        null_mut(),
        null_mut(),
        null(),
    );
    if edit.is_null() {
        return Err("Cannot create print layout editor".into());
    }
    if let Err(e) = crate::rich_edit::load(edit, rtf) {
        DestroyWindow(edit);
        return Err(e);
    }
    Ok((edit, true))
}
unsafe fn formatted_end(
    edit: HWND,
    dc: HDC,
    g: PrintGeometry,
    offset: (i32, i32),
    start: i32,
    limit: i32,
    reserve: i32,
    draw: bool,
) -> i32 {
    let mut fr = FormatRange {
        hdc: dc,
        target: dc,
        page: RECT {
            left: 0,
            top: 0,
            right: g.width_twips,
            bottom: g.height_twips,
        },
        rect: RECT {
            left: g.margin_left_twips - offset.0,
            top: g.margin_top_twips - offset.1,
            right: g.width_twips - g.margin_right_twips - offset.0,
            bottom: g.height_twips - g.margin_bottom_twips - offset.1 - reserve,
        },
        range: CharRange {
            min: start,
            max: limit,
        },
    };
    SendMessageW(
        edit,
        EM_FORMATRANGE,
        usize::from(draw),
        &mut fr as *mut _ as isize,
    ) as i32
}
unsafe fn make_note(
    dc: HDC,
    note: &crate::RichPrintNote,
    width: i32,
    dpi: i32,
) -> Result<NoteLayout, String> {
    if note.paragraphs.is_empty() {
        return Err("A note has no text".into());
    }
    let mut lines = Vec::new();
    let mut total_chars = 0usize;
    for (paragraph_index, p) in note.paragraphs.iter().enumerate() {
        let mut current = Line {
            glyphs: Vec::new(),
            width: 0,
            height: 0,
            align: p.align,
        };
        let mut fragments = p.fragments.clone();
        if paragraph_index == 0 {
            let first = fragments.first().cloned().unwrap_or(crate::PrintFragment {
                text: PrintText::Literal(String::new()),
                font: SceneFont {
                    family: "Arial".into(),
                    points: 10.0,
                    bold: false,
                    italic: false,
                    underline: false,
                },
                color: [0, 0, 0],
            });
            fragments.insert(
                0,
                crate::PrintFragment {
                    text: PrintText::Literal(format!("{}. ", note.label)),
                    ..first
                },
            );
        }
        for fragment in fragments {
            let PrintText::Literal(text) = fragment.text else {
                return Err("Dynamic fields in notes are not supported for printing".into());
            };
            let f = crate::rich_print::font(&fragment.font, dpi)?;
            let previous = SelectObject(dc, f as _);
            let height = ((fragment.font.points * dpi as f64 / 72.0) * 1.2)
                .ceil()
                .max(1.0) as i32;
            for ch in text.chars() {
                total_chars += 1;
                if total_chars > 100_000 {
                    SelectObject(dc, previous);
                    DeleteObject(f as _);
                    return Err("Note text exceeds print limit".into());
                }
                if ch == '\r' {
                    continue;
                }
                if ch == '\n' {
                    if current.height == 0 {
                        current.height = height;
                    }
                    lines.push(current);
                    current = Line {
                        glyphs: Vec::new(),
                        width: 0,
                        height: 0,
                        align: p.align,
                    };
                    continue;
                }
                if ch.is_control() && ch != '\t' {
                    SelectObject(dc, previous);
                    DeleteObject(f as _);
                    return Err("Unsupported control in note text".into());
                }
                let mut buf = [0u16; 2];
                let data = if ch == '\t' {
                    vec![b' ' as u16; 4]
                } else {
                    ch.encode_utf16(&mut buf).to_vec()
                };
                let mut extent: SIZE = zeroed();
                if GetTextExtentPoint32W(dc, data.as_ptr(), data.len() as i32, &mut extent) == 0 {
                    SelectObject(dc, previous);
                    DeleteObject(f as _);
                    return Err("Cannot measure note text".into());
                }
                if extent.cx > width {
                    SelectObject(dc, previous);
                    DeleteObject(f as _);
                    return Err("A note character is wider than the page".into());
                }
                if current.width + extent.cx > width && !current.glyphs.is_empty() {
                    lines.push(current);
                    current = Line {
                        glyphs: Vec::new(),
                        width: 0,
                        height: 0,
                        align: p.align,
                    };
                }
                current.width += extent.cx;
                current.height = current.height.max(height);
                current.glyphs.push(Glyph {
                    text: data,
                    width: extent.cx,
                    font: fragment.font.clone(),
                    color: fragment.color,
                });
            }
            SelectObject(dc, previous);
            DeleteObject(f as _);
        }
        if current.height == 0 {
            current.height = (dpi / 6).max(1);
        }
        lines.push(current);
    }
    let height = lines
        .iter()
        .try_fold(0i32, |a, l| a.checked_add(l.height))
        .ok_or("Note height overflow")?;
    Ok(NoteLayout { lines, height })
}
pub unsafe fn paginate(
    edit: HWND,
    dc: HDC,
    job: &RichPrint,
    offset: (i32, i32),
) -> Result<Layout, String> {
    let g = job.geometry;
    if !g.valid() {
        return Err("Invalid page geometry".into());
    }
    let dx = GetDeviceCaps(dc, LOGPIXELSX as i32);
    let dy = GetDeviceCaps(dc, LOGPIXELSY as i32);
    if dx <= 0 || dy <= 0 {
        return Err("Invalid output DPI".into());
    }
    let width = (((g.width_twips - g.margin_left_twips - g.margin_right_twips) as i64 * dx as i64)
        / 1440) as i32;
    let body_height = (((g.height_twips - g.margin_top_twips - g.margin_bottom_twips) as i64
        * dy as i64)
        / 1440) as i32;
    let mut notes = Vec::new();
    for note in &job.notes {
        let layout = make_note(dc, note, width, dy)?;
        if layout.height + dy / 5 >= body_height {
            return Err("A note is too long to fit on one page".into());
        }
        notes.push(layout);
    }
    let text = TextLength {
        flags: 2 | 8,
        codepage: 1200,
    };
    let len = SendMessageW(edit, EM_GETTEXTLENGTHEX, &text as *const _ as usize, 0);
    if !(0..=i32::MAX as isize).contains(&len) {
        return Err("Cannot determine rich text length".into());
    }
    let len = len as i32;
    let mut anchors = Vec::with_capacity(job.notes.len());
    for note in &job.notes {
        let marker = crate::wide(&note.marker);
        let mut find = FindTextEx {
            range: CharRange { min: 0, max: -1 },
            text: marker.as_ptr(),
            found: CharRange { min: -1, max: -1 },
        };
        let first = SendMessageW(edit, EM_FINDTEXTEXW, 1 | 4, &mut find as *mut _ as isize);
        if first < 0
            || find.found.min < 0
            || find.found.max <= find.found.min
            || find.found.max > len
        {
            return Err("Note marker missing from print layout".into());
        }
        let anchor = find.found.min;
        find.range.min = find.found.max;
        if SendMessageW(edit, EM_FINDTEXTEXW, 1 | 4, &mut find as *mut _ as isize) >= 0 {
            return Err("Note marker is ambiguous in print layout".into());
        }
        anchors.push(anchor as usize);
    }
    let mut pages = Vec::new();
    let mut start = 0;
    loop {
        if pages.len() >= 1024 {
            return Err("Document exceeds page limit".into());
        }
        let initial_end = formatted_end(edit, dc, g, offset, start, len, 0, false);
        if initial_end <= start && len > 0 {
            return Err("Text could not fit on the page".into());
        }
        let to_twips = |px: i32| ((px as i64 * 1440 + dy as i64 - 1) / dy as i64) as i32;
        let descriptors = job
            .notes
            .iter()
            .enumerate()
            .filter(|(_, note)| !note.endnote)
            .map(|(i, _)| (anchors[i], to_twips(notes[i].height), i))
            .collect::<Vec<_>>();
        let (end, reserve, included) = crate::note_pagination::refine_page(
            start,
            initial_end,
            &descriptors,
            to_twips((dy / 8).max(2)),
            to_twips((dy / 12).max(1)),
            g.height_twips - g.margin_top_twips - g.margin_bottom_twips,
            |cap, reserve| {
                Ok(formatted_end(
                    edit, dc, g, offset, start, cap, reserve, false,
                ))
            },
        )?;
        pages.push(Page {
            start,
            end,
            reserve,
            footnotes: included,
            endnotes: Vec::new(),
        });
        if end >= len || len == 0 {
            break;
        }
        start = end;
    }
    let mut selected = Vec::new();
    let mut used = (dy / 3).max(1);
    for (i, _) in job.notes.iter().enumerate().filter(|(_, n)| n.endnote) {
        let needed = notes[i].height + (dy / 12).max(1);
        if used + needed > body_height && !selected.is_empty() {
            pages.push(Page {
                start: len,
                end: len,
                reserve: 0,
                footnotes: Vec::new(),
                endnotes: std::mem::take(&mut selected),
            });
            used = (dy / 3).max(1);
        }
        if used + needed > body_height {
            return Err("An endnote is too long to fit on one page".into());
        }
        selected.push(i);
        used += needed;
    }
    if !selected.is_empty() {
        pages.push(Page {
            start: len,
            end: len,
            reserve: 0,
            footnotes: Vec::new(),
            endnotes: selected,
        });
    }
    if pages.len() > 1024 {
        return Err("Document exceeds page limit".into());
    }
    crate::rich_print::release(edit);
    Ok(Layout { pages, notes })
}
pub unsafe fn draw_page(
    edit: HWND,
    dc: HDC,
    job: &RichPrint,
    layout: &Layout,
    index: usize,
    offset_twips: (i32, i32),
    offset_px: (i32, i32),
) -> Result<(), String> {
    let page = &layout.pages[index];
    let g = job.geometry;
    if page.endnotes.is_empty() {
        let end = formatted_end(
            edit,
            dc,
            g,
            offset_twips,
            page.start,
            page.end,
            page.reserve,
            true,
        );
        if end != page.end {
            return Err("Print pagination changed while rendering".into());
        }
    }
    crate::rich_print::decorations(dc, job, offset_px, index, layout.pages.len())?;
    let dx = GetDeviceCaps(dc, LOGPIXELSX as i32);
    let dy = GetDeviceCaps(dc, LOGPIXELSY as i32);
    let px = |v: i32, d: i32| ((v as i64 * d as i64) / 1440) as i32;
    let left = px(g.margin_left_twips, dx) - offset_px.0;
    let width = px(
        g.width_twips - g.margin_left_twips - g.margin_right_twips,
        dx,
    );
    let mut y = if page.endnotes.is_empty() {
        px(g.height_twips - g.margin_bottom_twips - page.reserve, dy) - offset_px.1
            + (dy / 8).max(2)
    } else {
        px(g.margin_top_twips, dy) - offset_px.1 + (dy / 3).max(1)
    };
    if !page.endnotes.is_empty() {
        let label = crate::wide("Endnotes");
        TextOutW(
            dc,
            left,
            px(g.margin_top_twips, dy) - offset_px.1,
            label.as_ptr(),
            8,
        );
    }
    let entries = if page.endnotes.is_empty() {
        &page.footnotes
    } else {
        &page.endnotes
    };
    for index in entries {
        for line in &layout.notes[*index].lines {
            let mut x = left
                + match line.align {
                    TextAlign::Left => 0,
                    TextAlign::Center => (width - line.width) / 2,
                    TextAlign::Right => width - line.width,
                };
            for glyph in &line.glyphs {
                let font = crate::rich_print::font(&glyph.font, dy)?;
                let old = SelectObject(dc, font as _);
                SetBkMode(dc, TRANSPARENT as i32);
                SetTextColor(
                    dc,
                    u32::from(glyph.color[0])
                        | (u32::from(glyph.color[1]) << 8)
                        | (u32::from(glyph.color[2]) << 16),
                );
                let ok = TextOutW(dc, x, y, glyph.text.as_ptr(), glyph.text.len() as i32);
                SelectObject(dc, old);
                DeleteObject(font as _);
                if ok == 0 {
                    return Err("Cannot draw note text".into());
                }
                x += glyph.width;
            }
            y += line.height;
        }
        y += (dy / 12).max(1);
    }
    Ok(())
}
