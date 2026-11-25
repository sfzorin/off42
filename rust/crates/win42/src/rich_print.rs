//! Shared RichEdit pagination and header/footer device rendering.
use crate::{PrintGeometry, PrintParagraph, PrintText, RichPrint, SceneFont, TextAlign};
use std::mem::zeroed;
use windows_sys::Win32::{
    Foundation::*,
    Graphics::Gdi::*,
    UI::WindowsAndMessaging::{SendMessageW, WM_USER},
};
const EM_FORMATRANGE: u32 = WM_USER + 57;
const EM_GETTEXTLENGTHEX: u32 = WM_USER + 95;
#[repr(C)]
struct CharRange {
    min: i32,
    max: i32,
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
fn range(dc: HDC, g: PrintGeometry, offset: (i32, i32), start: i32) -> FormatRange {
    FormatRange {
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
            bottom: g.height_twips - g.margin_bottom_twips - offset.1,
        },
        range: CharRange {
            min: start,
            max: -1,
        },
    }
}
pub unsafe fn release(edit: HWND) {
    SendMessageW(edit, EM_FORMATRANGE, 0, 0);
}
pub unsafe fn paginate(
    edit: HWND,
    dc: HDC,
    g: PrintGeometry,
    offset: (i32, i32),
) -> Result<Vec<(i32, i32)>, String> {
    if !g.valid() {
        return Err("Invalid page geometry".into());
    }
    let text = TextLength {
        flags: 2 | 8,
        codepage: 1200,
    };
    let len = SendMessageW(edit, EM_GETTEXTLENGTHEX, &text as *const _ as usize, 0);
    if !(0..=i32::MAX as isize).contains(&len) {
        return Err("Cannot determine rich text length".into());
    }
    let mut pages = Vec::new();
    let mut start = 0;
    loop {
        if pages.len() >= 1024 {
            release(edit);
            return Err("Document exceeds page limit".into());
        }
        let mut fr = range(dc, g, offset, start);
        let next = SendMessageW(edit, EM_FORMATRANGE, 0, &mut fr as *mut _ as isize) as i32;
        if next <= start && len > 0 {
            release(edit);
            return Err("Text could not fit on the page".into());
        }
        pages.push((start, next));
        if next >= len as i32 || len == 0 {
            break;
        }
        start = next;
    }
    release(edit);
    Ok(pages)
}
pub unsafe fn body(edit: HWND, dc: HDC, g: PrintGeometry, offset: (i32, i32), start: i32) -> i32 {
    let mut fr = range(dc, g, offset, start);
    SendMessageW(edit, EM_FORMATRANGE, 1, &mut fr as *mut _ as isize) as i32
}
fn text(t: &PrintText, page: usize, count: usize) -> String {
    match t {
        PrintText::Literal(s) => s.clone(),
        PrintText::PageNumber => (page + 1).to_string(),
        PrintText::PageCount => count.to_string(),
    }
}
pub(super) unsafe fn font(f: &SceneFont, dpi: i32) -> Result<HFONT, String> {
    if !f.points.is_finite() || !(1.0..=1000.0).contains(&f.points) {
        return Err("Invalid header/footer font size".into());
    }
    let mut lf: LOGFONTW = zeroed();
    lf.lfHeight = -((f.points * dpi as f64 / 72.0).round().max(1.0) as i32);
    lf.lfWeight = if f.bold { 700 } else { 400 };
    lf.lfItalic = u8::from(f.italic);
    lf.lfUnderline = u8::from(f.underline);
    for (i, c) in f
        .family
        .encode_utf16()
        .take(lf.lfFaceName.len() - 1)
        .enumerate()
    {
        lf.lfFaceName[i] = c;
    }
    let font = CreateFontIndirectW(&lf);
    if font.is_null() {
        Err("Cannot create header/footer font".into())
    } else {
        Ok(font)
    }
}
unsafe fn line_height(p: &PrintParagraph, dpi: i32) -> i32 {
    let points = p
        .fragments
        .iter()
        .map(|f| f.font.points)
        .fold(10.0f64, f64::max);
    ((points * dpi as f64 / 72.0) * 1.2).ceil().max(1.0) as i32
}
unsafe fn draw_lines(
    dc: HDC,
    lines: &[PrintParagraph],
    left: i32,
    width: i32,
    mut y: i32,
    dpi: i32,
    page: usize,
    count: usize,
) -> Result<(), String> {
    for line in lines {
        let mut segments = Vec::new();
        let mut total = 0i32;
        for fragment in &line.fragments {
            let value = text(&fragment.text, page, count);
            let data: Vec<u16> = value.encode_utf16().collect();
            // GDI text calls may read an empty Vec's dangling pointer.
            if data.is_empty() {
                continue;
            }
            let font = font(&fragment.font, dpi)?;
            let old = SelectObject(dc, font as _);
            let mut extent: SIZE = zeroed();
            let ok = GetTextExtentPoint32W(
                dc,
                data.as_ptr(),
                data.len().min(i32::MAX as usize) as i32,
                &mut extent,
            );
            SelectObject(dc, old);
            DeleteObject(font as _);
            if ok == 0 {
                return Err("Cannot measure header/footer text".into());
            }
            total = total
                .checked_add(extent.cx)
                .ok_or("Header/footer width overflow")?;
            segments.push((fragment, data, extent.cx));
        }
        if total > width {
            return Err("Header/footer text exceeds the page width".into());
        }
        let mut x = left
            + match line.align {
                TextAlign::Left => 0,
                TextAlign::Center => (width - total) / 2,
                TextAlign::Right => width - total,
            };
        for (fragment, data, advance) in segments {
            let font = font(&fragment.font, dpi)?;
            let old = SelectObject(dc, font as _);
            SetBkMode(dc, TRANSPARENT as i32);
            let c = fragment.color;
            SetTextColor(
                dc,
                u32::from(c[0]) | (u32::from(c[1]) << 8) | (u32::from(c[2]) << 16),
            );
            let ok = TextOutW(
                dc,
                x,
                y,
                data.as_ptr(),
                data.len().min(i32::MAX as usize) as i32,
            );
            SelectObject(dc, old);
            DeleteObject(font as _);
            if ok == 0 {
                return Err("Cannot draw header/footer text".into());
            }
            x += advance;
        }
        y += line_height(line, dpi);
    }
    Ok(())
}
pub unsafe fn decorations(
    dc: HDC,
    job: &RichPrint,
    offset_px: (i32, i32),
    page: usize,
    count: usize,
) -> Result<(), String> {
    let dx = GetDeviceCaps(dc, LOGPIXELSX as i32);
    let dy = GetDeviceCaps(dc, LOGPIXELSY as i32);
    if dx <= 0 || dy <= 0 {
        return Err("Invalid output DPI".into());
    }
    let px = |v: i32, dpi: i32| ((i64::from(v) * i64::from(dpi)) / 1440) as i32;
    let g = job.geometry;
    let left = px(g.margin_left_twips, dx) - offset_px.0;
    let width = px(
        g.width_twips - g.margin_left_twips - g.margin_right_twips,
        dx,
    );
    let header_y = px(job.header_distance_twips, dy) - offset_px.1;
    let footer_bottom = px(g.height_twips - job.footer_distance_twips, dy) - offset_px.1;
    let hh: i32 = job.header.iter().map(|p| line_height(p, dy)).sum();
    let fh: i32 = job.footer.iter().map(|p| line_height(p, dy)).sum();
    let body_top = px(g.margin_top_twips, dy) - offset_px.1;
    let body_bottom = px(g.height_twips - g.margin_bottom_twips, dy) - offset_px.1;
    if (!job.header.is_empty() && (header_y < -offset_px.1 || header_y + hh > body_top))
        || (!job.footer.is_empty()
            && (footer_bottom - fh < body_bottom
                || footer_bottom > px(g.height_twips, dy) - offset_px.1))
    {
        return Err("Headers or footers do not fit in the page margins".into());
    }
    let saved = SaveDC(dc);
    let result = (|| {
        draw_lines(dc, &job.header, left, width, header_y, dy, page, count)?;
        draw_lines(
            dc,
            &job.footer,
            left,
            width,
            footer_bottom - fh,
            dy,
            page,
            count,
        )
    })();
    if saved != 0 {
        RestoreDC(dc, saved);
    }
    result
}
