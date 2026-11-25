//! Opt-in variable-geometry native spreadsheet grid.
use crate::grid_geometry::{index_at, index_at_view, offsets, position};
use crate::GridRange;
use std::{
    collections::BTreeSet,
    mem::{size_of, zeroed},
    ptr::{null, null_mut},
};
use windows_sys::Win32::{
    Foundation::*,
    Graphics::Gdi::*,
    System::LibraryLoader::GetModuleHandleW,
    UI::{
        Controls::{SetScrollInfo, EM_SETSEL},
        Input::KeyboardAndMouse::{
            GetKeyState, ReleaseCapture, SetCapture, SetFocus, VK_CONTROL, VK_DELETE, VK_DOWN,
            VK_END, VK_ESCAPE, VK_F2, VK_HOME, VK_LEFT, VK_NEXT, VK_PRIOR, VK_RETURN, VK_RIGHT,
            VK_SHIFT, VK_TAB, VK_UP,
        },
        WindowsAndMessaging::*,
    },
};
pub(super) struct Snapshot {
    pub rows: Vec<Vec<String>>,
    pub widths: Vec<i32>,
    pub heights: Vec<i32>,
    pub labels: Vec<String>,
    pub selected: Option<(usize, usize)>,
    pub range: Option<GridRange>,
    pub readonly: bool,
    pub frozen: (usize, usize),
    pub clipboard: Option<(u16, u16)>,
}
const HEADER: i32 = 25;
const ROW_HEADER: i32 = 48;
const COMMIT: u32 = WM_APP + 0x481;
const CANCEL: u32 = WM_APP + 0x482;
struct GridState {
    x: i32,
    y: i32,
    drag: Option<Drag>,
    selecting: bool,
    preview_selection: Option<(GridRange, (usize, usize))>,
    anchor: Option<(usize, usize)>,
    focus_pane: Option<(bool, bool)>,
    editor: HWND,
    committing: bool,
}
#[derive(Clone, Copy)]
struct Drag {
    index: usize,
    start: i32,
    original: i32,
    last: i32,
    row: bool,
}
unsafe fn state(hwnd: HWND) -> Option<&'static mut GridState> {
    (GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut GridState).as_mut()
}
fn coordinate(l: LPARAM) -> (i32, i32) {
    (
        (l as u32 & 0xffff) as u16 as i16 as i32,
        ((l as u32 >> 16) & 0xffff) as u16 as i16 as i32,
    )
}
fn color(v: [u8; 3]) -> u32 {
    v[0] as u32 | ((v[1] as u32) << 8) | ((v[2] as u32) << 16)
}
fn selected(range: Option<GridRange>, r: usize, c: usize) -> bool {
    range.is_some_and(|v| r >= v.top && r <= v.bottom && c >= v.left && c <= v.right)
}
fn col_label(mut c: usize) -> String {
    c += 1;
    let mut s = Vec::new();
    while c > 0 {
        c -= 1;
        s.push((b'A' + (c % 26) as u8) as char);
        c /= 26
    }
    s.iter().rev().collect()
}
unsafe fn fill(dc: HDC, rect: &RECT, c: [u8; 3]) {
    let brush = CreateSolidBrush(color(c));
    FillRect(dc, rect, brush);
    DeleteObject(brush as _);
}
unsafe fn line(dc: HDC, x1: i32, y1: i32, x2: i32, y2: i32, c: [u8; 3]) {
    let pen = CreatePen(PS_SOLID, 1, color(c));
    let old = SelectObject(dc, pen as _);
    MoveToEx(dc, x1, y1, null_mut());
    LineTo(dc, x2, y2);
    SelectObject(dc, old);
    DeleteObject(pen as _);
}
unsafe fn text(
    dc: HDC,
    rect: RECT,
    value: &str,
    c: [u8; 3],
    bold: bool,
    italic: bool,
    center: bool,
) {
    let font = CreateFontW(
        -14,
        0,
        0,
        0,
        if bold { 700 } else { 400 },
        italic as u32,
        0,
        0,
        DEFAULT_CHARSET as u32,
        0,
        0,
        0,
        0,
        crate::wide("Segoe UI").as_ptr(),
    );
    let old = SelectObject(dc, font as _);
    SetBkMode(dc, TRANSPARENT as i32);
    SetTextColor(dc, color(c));
    let value = crate::wide(value);
    let mut rect = rect;
    DrawTextW(
        dc,
        value.as_ptr(),
        (value.len() - 1) as i32,
        &mut rect,
        DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS | if center { DT_CENTER } else { DT_LEFT },
    );
    SelectObject(dc, old);
    DeleteObject(font as _);
}
fn cell_fragment(
    r: usize,
    c: usize,
    merged: Option<GridRange>,
    xs: &[i32],
    ys: &[i32],
    st: &GridState,
    frozen: (usize, usize),
    client: (i32, i32),
    pane: (bool, bool),
) -> Option<crate::grid_geometry::Fragment> {
    let range = merged.unwrap_or(GridRange {
        top: r,
        left: c,
        bottom: r,
        right: c,
    });
    crate::grid_geometry::pane_fragment(
        range,
        xs,
        ys,
        frozen,
        (st.x, st.y),
        (ROW_HEADER, HEADER),
        client,
        pane,
    )
}
fn rect_from_tuple(v: (i32, i32, i32, i32)) -> RECT {
    RECT {
        left: v.0,
        top: v.1,
        right: v.2,
        bottom: v.3,
    }
}

unsafe fn update_scroll(hwnd: HWND) {
    let Some((old_x, old_y)) = state(hwnd).map(|s| (s.x, s.y)) else {
        return;
    };
    let Some(snap) = super::native::grid_snapshot() else {
        return;
    };
    let xs = offsets(&snap.widths);
    let ys = offsets(&snap.heights);
    let mut rc = RECT::default();
    GetClientRect(hwnd, &mut rc);
    let w = (rc.right - ROW_HEADER).max(1);
    let h = (rc.bottom - HEADER).max(1);
    let x = old_x.clamp(0, (xs.last().copied().unwrap_or(0) - w).max(0));
    let y = old_y.clamp(0, (ys.last().copied().unwrap_or(0) - h).max(0));
    if let Some(st) = state(hwnd) {
        st.x = x;
        st.y = y;
    }
    for (bar, total, page, pos) in [
        (SB_HORZ, *xs.last().unwrap_or(&0), w, x),
        (SB_VERT, *ys.last().unwrap_or(&0), h, y),
    ] {
        let info = SCROLLINFO {
            cbSize: size_of::<SCROLLINFO>() as u32,
            fMask: SIF_RANGE | SIF_PAGE | SIF_POS,
            nMin: 0,
            nMax: total.saturating_sub(1),
            nPage: page as u32,
            nPos: pos,
            ..zeroed()
        };
        SetScrollInfo(hwnd, bar, &info, 1);
    }
}
unsafe fn paint(hwnd: HWND) {
    let Some(snap) = super::native::grid_snapshot() else {
        let mut ps = PAINTSTRUCT::default();
        BeginPaint(hwnd, &mut ps);
        EndPaint(hwnd, &ps);
        return;
    };
    let Some(st) = state(hwnd) else {
        let mut ps = PAINTSTRUCT::default();
        BeginPaint(hwnd, &mut ps);
        EndPaint(hwnd, &ps);
        return;
    };
    let xs = offsets(&snap.widths);
    let ys = offsets(&snap.heights);
    let mut ps = PAINTSTRUCT::default();
    let dc = BeginPaint(hwnd, &mut ps);
    let mut rc = RECT::default();
    GetClientRect(hwnd, &mut rc);
    fill(dc, &rc, [255, 255, 255]);
    let save = SaveDC(dc);
    IntersectClipRect(dc, ROW_HEADER, HEADER, rc.right, rc.bottom);
    let mut drawn = BTreeSet::new();
    for r in 0..snap.rows.len() {
        if snap.heights[r] <= 0 {
            continue;
        }
        let top = position(&ys, r, snap.frozen.0, st.y, HEADER);
        let bottom = top + snap.heights[r];
        if bottom <= HEADER || top >= rc.bottom {
            continue;
        }
        for c in 0..snap.widths.len() {
            if snap.widths[c] <= 0 {
                continue;
            }
            let left = position(&xs, c, snap.frozen.1, st.x, ROW_HEADER);
            let right = left + snap.widths[c];
            if right <= ROW_HEADER || left >= rc.right {
                continue;
            }
            let pane_kind = (r < snap.frozen.0, c < snap.frozen.1);
            let style = super::native::grid_style(r, c).unwrap_or_default();
            let (ar, ac) = style.merged.map_or((r, c), |m| (m.top, m.left));
            let style = super::native::grid_style(ar, ac).unwrap_or_default();
            let Some(fragment) = cell_fragment(
                ar,
                ac,
                style.merged,
                &xs,
                &ys,
                st,
                snap.frozen,
                (rc.right, rc.bottom),
                pane_kind,
            ) else {
                continue;
            };
            if !drawn.insert((ar, ac, pane_kind.0, pane_kind.1)) {
                continue;
            }
            let pane = SaveDC(dc);
            IntersectClipRect(
                dc,
                fragment.clip.0,
                fragment.clip.1,
                fragment.clip.2,
                fragment.clip.3,
            );
            let rect = rect_from_tuple(fragment.rect);
            let bg = if selected(st.preview_selection.map(|v| v.0).or(snap.range), r, c) {
                [210, 228, 252]
            } else {
                style.background.unwrap_or([255; 3])
            };
            fill(dc, &rect, bg);
            let fg = style.foreground.unwrap_or([0; 3]);
            let value = snap
                .rows
                .get(ar)
                .and_then(|row| row.get(ac))
                .map_or("", String::as_str);
            let inner = RECT {
                left: rect.left + 4,
                top: rect.top,
                right: rect.right - 3,
                bottom: rect.bottom,
            };
            text(dc, inner, value, fg, style.bold, style.italic, false);
            line(
                dc,
                rect.left,
                rect.bottom - 1,
                rect.right,
                rect.bottom - 1,
                [218; 3],
            );
            line(
                dc,
                rect.right - 1,
                rect.top,
                rect.right - 1,
                rect.bottom,
                [218; 3],
            );
            for (bit, a, b) in [
                (1, (rect.left, rect.top), (rect.right, rect.top)),
                (2, (rect.right - 1, rect.top), (rect.right - 1, rect.bottom)),
                (
                    4,
                    (rect.left, rect.bottom - 1),
                    (rect.right, rect.bottom - 1),
                ),
                (8, (rect.left, rect.top), (rect.left, rect.bottom)),
            ] {
                if style.borders & bit != 0 {
                    line(dc, a.0, a.1, b.0, b.1, [0; 3])
                }
            }
            if st.preview_selection.map(|v| v.1).or(snap.selected) == Some((ar, ac)) {
                line(
                    dc,
                    rect.left,
                    rect.top,
                    rect.right,
                    rect.top,
                    [35, 100, 210],
                );
                line(
                    dc,
                    rect.left,
                    rect.bottom - 1,
                    rect.right,
                    rect.bottom - 1,
                    [35, 100, 210],
                );
                line(
                    dc,
                    rect.left,
                    rect.top,
                    rect.left,
                    rect.bottom,
                    [35, 100, 210],
                );
                line(
                    dc,
                    rect.right - 1,
                    rect.top,
                    rect.right - 1,
                    rect.bottom,
                    [35, 100, 210],
                );
            }
            RestoreDC(dc, pane);
        }
    }
    RestoreDC(dc, save);
    let header = RECT {
        left: 0,
        top: 0,
        right: rc.right,
        bottom: HEADER,
    };
    fill(dc, &header, [239, 242, 247]);
    let left = RECT {
        left: 0,
        top: HEADER,
        right: ROW_HEADER,
        bottom: rc.bottom,
    };
    fill(dc, &left, [239, 242, 247]);
    for c in 0..snap.widths.len() {
        if snap.widths[c] <= 0 {
            continue;
        }
        let a = position(&xs, c, snap.frozen.1, st.x, ROW_HEADER);
        let b = a + snap.widths[c];
        let pane = SaveDC(dc);
        let split = ROW_HEADER + xs[snap.frozen.1.min(snap.widths.len())];
        IntersectClipRect(
            dc,
            if c < snap.frozen.1 { ROW_HEADER } else { split },
            0,
            if c < snap.frozen.1 {
                split.min(rc.right)
            } else {
                rc.right
            },
            HEADER,
        );
        if b <= ROW_HEADER || a >= rc.right {
            RestoreDC(dc, pane);
            continue;
        }
        text(
            dc,
            RECT {
                left: a.max(ROW_HEADER),
                top: 0,
                right: b,
                bottom: HEADER,
            },
            &col_label(c),
            [45; 3],
            true,
            false,
            true,
        );
        line(dc, b - 1, 0, b - 1, HEADER, [195; 3]);
        RestoreDC(dc, pane);
    }
    for r in 0..snap.heights.len() {
        if snap.heights[r] <= 0 {
            continue;
        }
        let a = position(&ys, r, snap.frozen.0, st.y, HEADER);
        let b = a + snap.heights[r];
        let pane = SaveDC(dc);
        let split = HEADER + ys[snap.frozen.0.min(snap.heights.len())];
        IntersectClipRect(
            dc,
            0,
            if r < snap.frozen.0 { HEADER } else { split },
            ROW_HEADER,
            if r < snap.frozen.0 {
                split.min(rc.bottom)
            } else {
                rc.bottom
            },
        );
        if b <= HEADER || a >= rc.bottom {
            RestoreDC(dc, pane);
            continue;
        }
        text(
            dc,
            RECT {
                left: 0,
                top: a.max(HEADER),
                right: ROW_HEADER - 4,
                bottom: b,
            },
            snap.labels.get(r).map_or("", String::as_str),
            [45; 3],
            false,
            false,
            true,
        );
        line(dc, 0, b - 1, ROW_HEADER, b - 1, [195; 3]);
        RestoreDC(dc, pane);
    }
    line(dc, ROW_HEADER - 1, 0, ROW_HEADER - 1, rc.bottom, [175; 3]);
    line(dc, 0, HEADER - 1, rc.right, HEADER - 1, [175; 3]);
    if let Some(d) = st.drag {
        if d.row {
            line(dc, 0, d.last, rc.right, d.last, [35, 100, 210])
        } else {
            line(dc, d.last, 0, d.last, rc.bottom, [35, 100, 210])
        }
    }
    EndPaint(hwnd, &ps);
}
unsafe fn hit(hwnd: HWND, x: i32, y: i32) -> Option<((usize, usize), (bool, bool))> {
    if x < ROW_HEADER || y < HEADER {
        return None;
    }
    let st = state(hwnd)?;
    let snap = super::native::grid_snapshot()?;
    let xs = offsets(&snap.widths);
    let ys = offsets(&snap.heights);
    let c = index_at_view(&xs, x, snap.frozen.1, st.x, ROW_HEADER)?;
    let r = index_at_view(&ys, y, snap.frozen.0, st.y, HEADER)?;
    let anchor = super::native::grid_style(r, c)
        .and_then(|s| s.merged)
        .map_or((r, c), |m| (m.top, m.left));
    Some((anchor, (r < snap.frozen.0, c < snap.frozen.1)))
}
unsafe fn finish_editor(hwnd: HWND, commit: bool) -> bool {
    let Some(st) = state(hwnd) else { return true };
    if st.editor.is_null() || st.committing {
        return true;
    }
    let edit = st.editor;
    if commit {
        let len = GetWindowTextLengthW(edit).max(0) as usize;
        let mut buf = vec![0u16; len + 1];
        let count = GetWindowTextW(edit, buf.as_mut_ptr(), buf.len() as i32).max(0) as usize;
        let value = String::from_utf16_lossy(&buf[..count]);
        st.committing = true;
        let ok = super::native::grid_commit(value);
        let Some(st) = state(hwnd) else { return ok };
        st.committing = false;
        if !ok {
            SetFocus(edit);
            return false;
        }
    }
    let Some(st) = state(hwnd) else { return true };
    st.editor = null_mut();
    DestroyWindow(edit);
    InvalidateRect(hwnd, null(), 0);
    true
}
unsafe extern "system" fn edit_proc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    let old = GetWindowLongPtrW(hwnd, GWLP_USERDATA);
    if msg == WM_KEYDOWN {
        if w as u16 == VK_ESCAPE {
            SendMessageW(GetParent(hwnd), CANCEL, 0, 0);
            return 0;
        }
        if w as u16 == VK_TAB {
            let parent = GetParent(hwnd);
            SendMessageW(parent, COMMIT, 0, 0);
            if IsWindow(hwnd) == 0 {
                SendMessageW(parent, WM_KEYDOWN, VK_TAB as usize, 0);
            }
            return 0;
        }
        if w as u16 == VK_RETURN && GetKeyState(VK_SHIFT as i32) >= 0 {
            SendMessageW(GetParent(hwnd), COMMIT, 0, 0);
            return 0;
        }
    }
    let proc: WNDPROC = std::mem::transmute(old);
    CallWindowProcW(proc, hwnd, msg, w, l)
}
unsafe fn begin_editor(hwnd: HWND, replace: bool) {
    let Some(snap) = super::native::grid_snapshot() else {
        return;
    };
    if snap.readonly {
        return;
    }
    let Some((r, c)) = snap.selected else { return };
    let Some(st) = state(hwnd) else { return };
    if !st.editor.is_null() {
        SetFocus(st.editor);
        return;
    }
    let xs = offsets(&snap.widths);
    let ys = offsets(&snap.heights);
    let style = super::native::grid_style(r, c).unwrap_or_default();
    let mut client = RECT::default();
    GetClientRect(hwnd, &mut client);
    let preferred = st
        .focus_pane
        .unwrap_or((r < snap.frozen.0, c < snap.frozen.1));
    let fragment = cell_fragment(
        r,
        c,
        style.merged,
        &xs,
        &ys,
        st,
        snap.frozen,
        (client.right, client.bottom),
        preferred,
    )
    .or_else(|| {
        [(true, true), (true, false), (false, true), (false, false)]
            .into_iter()
            .find_map(|pane| {
                cell_fragment(
                    r,
                    c,
                    style.merged,
                    &xs,
                    &ys,
                    st,
                    snap.frozen,
                    (client.right, client.bottom),
                    pane,
                )
            })
    });
    let Some(fragment) = fragment else {
        return;
    };
    let rect = rect_from_tuple(fragment.visible);
    if rect.right <= rect.left || rect.bottom <= rect.top {
        return;
    }
    let value = if replace {
        String::new()
    } else {
        super::native::grid_editor_text()
    };
    let instance = GetModuleHandleW(null());
    let edit = CreateWindowExW(
        WS_EX_CLIENTEDGE,
        crate::wide("EDIT").as_ptr(),
        crate::wide(&value).as_ptr(),
        WS_CHILD
            | WS_VISIBLE
            | WS_TABSTOP
            | ES_MULTILINE as u32
            | ES_AUTOVSCROLL as u32
            | ES_WANTRETURN as u32,
        rect.left,
        rect.top,
        rect.right - rect.left,
        rect.bottom - rect.top,
        hwnd,
        null_mut(),
        instance,
        null(),
    );
    if edit.is_null() {
        return;
    }
    let old = SetWindowLongPtrW(edit, GWLP_WNDPROC, edit_proc as *const () as usize as isize);
    if old == 0 {
        DestroyWindow(edit);
        return;
    }
    SetWindowLongPtrW(edit, GWLP_USERDATA, old);
    st.editor = edit;
    SetFocus(edit);
    if !replace {
        SendMessageW(edit, EM_SETSEL, 0, -1);
    }
}
unsafe fn position_editor(hwnd: HWND) {
    let Some(edit) = state(hwnd).map(|s| s.editor).filter(|h| !h.is_null()) else {
        return;
    };
    let Some(snap) = super::native::grid_snapshot() else {
        return;
    };
    let Some((r, c)) = snap.selected else { return };
    let Some(st) = state(hwnd) else { return };
    let xs = offsets(&snap.widths);
    let ys = offsets(&snap.heights);
    let style = super::native::grid_style(r, c).unwrap_or_default();
    let mut client = RECT::default();
    GetClientRect(hwnd, &mut client);
    let preferred = st
        .focus_pane
        .unwrap_or((r < snap.frozen.0, c < snap.frozen.1));
    let fragment = cell_fragment(
        r,
        c,
        style.merged,
        &xs,
        &ys,
        st,
        snap.frozen,
        (client.right, client.bottom),
        preferred,
    )
    .or_else(|| {
        [(true, true), (true, false), (false, true), (false, false)]
            .into_iter()
            .find_map(|pane| {
                cell_fragment(
                    r,
                    c,
                    style.merged,
                    &xs,
                    &ys,
                    st,
                    snap.frozen,
                    (client.right, client.bottom),
                    pane,
                )
            })
    });
    let Some(fragment) = fragment else {
        return;
    };
    let rect = rect_from_tuple(fragment.visible);
    if rect.right > rect.left && rect.bottom > rect.top {
        MoveWindow(
            edit,
            rect.left,
            rect.top,
            rect.right - rect.left,
            rect.bottom - rect.top,
            1,
        );
    }
}
pub(super) unsafe fn finish_pending(hwnd: HWND) -> bool {
    finish_editor(hwnd, true)
}
unsafe fn ensure_visible(hwnd: HWND, row: usize, col: usize) {
    let Some(snap) = super::native::grid_snapshot() else {
        return;
    };
    let Some(st) = state(hwnd) else { return };
    let xs = offsets(&snap.widths);
    let ys = offsets(&snap.heights);
    if row + 1 >= ys.len() || col + 1 >= xs.len() {
        return;
    }
    let mut rc = RECT::default();
    GetClientRect(hwnd, &mut rc);
    let w = (rc.right - ROW_HEADER).max(1);
    let h = (rc.bottom - HEADER).max(1);
    if col >= snap.frozen.1 && xs[col] < st.x + xs[snap.frozen.1.min(snap.widths.len())] {
        st.x = xs[col] - xs[snap.frozen.1.min(snap.widths.len())]
    } else if col >= snap.frozen.1 && xs[col + 1] > st.x + w {
        st.x = xs[col + 1] - w
    }
    if row >= snap.frozen.0 && ys[row] < st.y + ys[snap.frozen.0.min(snap.heights.len())] {
        st.y = ys[row] - ys[snap.frozen.0.min(snap.heights.len())]
    } else if row >= snap.frozen.0 && ys[row + 1] > st.y + h {
        st.y = ys[row + 1] - h
    }
    update_scroll(hwnd);
    InvalidateRect(hwnd, null(), 0);
}
unsafe fn scroll(hwnd: HWND, vertical: bool, code: i32) {
    if !finish_editor(hwnd, true) {
        return;
    }
    let Some(st) = state(hwnd) else { return };
    let bar = if vertical { SB_VERT } else { SB_HORZ };
    let mut info = SCROLLINFO {
        cbSize: size_of::<SCROLLINFO>() as u32,
        fMask: SIF_ALL,
        ..zeroed()
    };
    GetScrollInfo(hwnd, bar, &mut info);
    let pos = if vertical { &mut st.y } else { &mut st.x };
    *pos = match code {
        SB_LINEUP => pos.saturating_sub(20),
        SB_LINEDOWN => pos.saturating_add(20),
        SB_PAGEUP => pos.saturating_sub(info.nPage as i32),
        SB_PAGEDOWN => pos.saturating_add(info.nPage as i32),
        SB_THUMBTRACK | SB_THUMBPOSITION => info.nTrackPos,
        _ => *pos,
    };
    update_scroll(hwnd);
    InvalidateRect(hwnd, null(), 0);
}
unsafe fn move_selection(hwnd: HWND, key: u16, shift: bool) {
    let Some(snap) = super::native::grid_snapshot() else {
        return;
    };
    let Some((r, c)) = snap.selected else { return };
    if snap.rows.is_empty()
        || snap.widths.is_empty()
        || r >= snap.rows.len()
        || c >= snap.widths.len()
    {
        return;
    }
    let current = super::native::grid_style(r, c)
        .and_then(|s| s.merged)
        .unwrap_or(GridRange {
            top: r,
            left: c,
            bottom: r,
            right: c,
        });
    let target = match key {
        VK_LEFT => (r, current.left.saturating_sub(1)),
        VK_RIGHT => (r, (current.right + 1).min(snap.widths.len() - 1)),
        VK_UP => (current.top.saturating_sub(1), c),
        VK_DOWN => ((current.bottom + 1).min(snap.rows.len() - 1), c),
        VK_HOME => (
            r,
            snap.widths.iter().position(|width| *width > 0).unwrap_or(c),
        ),
        VK_END => (
            r,
            snap.widths
                .iter()
                .rposition(|width| *width > 0)
                .unwrap_or(c),
        ),
        VK_PRIOR | VK_NEXT => {
            let ys = offsets(&snap.heights);
            let mut rc = RECT::default();
            GetClientRect(hwnd, &mut rc);
            let page = (rc.bottom - HEADER).max(1);
            let target = if key == VK_PRIOR {
                ys[r].saturating_sub(page)
            } else {
                ys[r].saturating_add(page).min(*ys.last().unwrap_or(&1) - 1)
            };
            (index_at(&ys, target).unwrap_or(r), c)
        }
        VK_TAB if shift => {
            if c > 0 {
                (r, c - 1)
            } else {
                (r.saturating_sub(1), snap.widths.len() - 1)
            }
        }
        VK_TAB => {
            if current.right + 1 < snap.widths.len() {
                (r, current.right + 1)
            } else {
                ((r + 1).min(snap.rows.len() - 1), 0)
            }
        }
        _ => (r, c),
    };
    let mut target = target;
    if key == VK_TAB {
        let width = snap.widths.len();
        let total = snap.rows.len().saturating_mul(width);
        let mut index = target.0 * width + target.1;
        for _ in 0..total {
            if snap.heights[index / width] > 0 && snap.widths[index % width] > 0 {
                break;
            }
            if shift {
                if index == 0 {
                    return;
                }
                index -= 1;
            } else {
                index += 1;
                if index >= total {
                    return;
                }
            }
        }
        target = (index / width, index % width);
    } else {
        let backwards = matches!(key, VK_LEFT | VK_UP | VK_HOME | VK_PRIOR);
        while snap.widths[target.1] <= 0 {
            if backwards {
                if target.1 == 0 {
                    return;
                }
                target.1 -= 1;
            } else {
                target.1 += 1;
                if target.1 >= snap.widths.len() {
                    return;
                }
            }
        }
        while snap.heights[target.0] <= 0 {
            if backwards {
                if target.0 == 0 {
                    return;
                }
                target.0 -= 1;
            } else {
                target.0 += 1;
                if target.0 >= snap.heights.len() {
                    return;
                }
            }
        }
    }
    let target = super::native::grid_style(target.0, target.1)
        .and_then(|s| s.merged)
        .map_or(target, |m| (m.top, m.left));
    if let Some(st) = state(hwnd) {
        st.focus_pane = None;
    }
    if shift && key != VK_TAB {
        let anchor = state(hwnd).and_then(|s| s.anchor).unwrap_or((r, c));
        if let Some(st) = state(hwnd) {
            st.anchor = Some(anchor)
        }
        super::native::grid_select_range(
            GridRange {
                top: anchor.0.min(target.0),
                left: anchor.1.min(target.1),
                bottom: anchor.0.max(target.0),
                right: anchor.1.max(target.1),
            },
            target,
        );
    } else {
        if let Some(st) = state(hwnd) {
            st.anchor = Some(target);
            st.focus_pane = None;
        }
        super::native::grid_select(target.0, target.1);
    }
    let target = super::native::grid_snapshot()
        .and_then(|s| s.selected)
        .unwrap_or(target);
    ensure_visible(hwnd, target.0, target.1)
}
unsafe extern "system" fn proc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    match msg {
        WM_NCCREATE => {
            let st = Box::new(GridState {
                x: 0,
                y: 0,
                drag: None,
                selecting: false,
                preview_selection: None,
                anchor: None,
                focus_pane: None,
                editor: null_mut(),
                committing: false,
            });
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, Box::into_raw(st) as isize);
            1
        }
        WM_NCDESTROY => {
            let ptr = SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0) as *mut GridState;
            if !ptr.is_null() {
                drop(Box::from_raw(ptr));
            }
            DefWindowProcW(hwnd, msg, w, l)
        }
        WM_ERASEBKGND => 1,
        WM_PAINT => {
            paint(hwnd);
            0
        }
        WM_SIZE => {
            update_scroll(hwnd);
            position_editor(hwnd);
            0
        }
        WM_VSCROLL => {
            scroll(hwnd, true, (w & 0xffff) as i32);
            0
        }
        WM_HSCROLL => {
            scroll(hwnd, false, (w & 0xffff) as i32);
            0
        }
        WM_MOUSEWHEEL => {
            let delta = ((w >> 16) & 0xffff) as u16 as i16;
            let count = (delta.unsigned_abs() as usize / 120).max(1).min(12);
            for _ in 0..count {
                scroll(hwnd, true, if delta > 0 { SB_LINEUP } else { SB_LINEDOWN })
            }
            0
        }
        WM_LBUTTONDOWN => {
            if !finish_editor(hwnd, true) {
                return 0;
            }
            let (x, y) = coordinate(l);
            let Some(snap) = super::native::grid_snapshot() else {
                return 0;
            };
            let Some((sx, sy, old_anchor)) = state(hwnd).map(|s| (s.x, s.y, s.anchor)) else {
                return 0;
            };
            let xs = offsets(&snap.widths);
            let ys = offsets(&snap.heights);
            let boundary = if y < HEADER && x >= ROW_HEADER {
                xs.iter()
                    .enumerate()
                    .skip(1)
                    .find(|(i, _)| {
                        let split = ROW_HEADER + xs[snap.frozen.1.min(snap.widths.len())];
                        let belongs = if *i - 1 < snap.frozen.1 {
                            x <= split
                        } else {
                            x >= split
                        };
                        belongs
                            && snap.widths[*i - 1] > 0
                            && (position(&xs, *i - 1, snap.frozen.1, sx, ROW_HEADER)
                                + snap.widths[*i - 1]
                                - x)
                                .abs()
                                <= 4
                    })
                    .map(|(i, _)| Drag {
                        index: i - 1,
                        start: x,
                        last: x,
                        original: snap.widths[i - 1],
                        row: false,
                    })
            } else if x < ROW_HEADER && y >= HEADER {
                ys.iter()
                    .enumerate()
                    .skip(1)
                    .find(|(i, _)| {
                        let split = HEADER + ys[snap.frozen.0.min(snap.heights.len())];
                        let belongs = if *i - 1 < snap.frozen.0 {
                            y <= split
                        } else {
                            y >= split
                        };
                        belongs
                            && snap.heights[*i - 1] > 0
                            && (position(&ys, *i - 1, snap.frozen.0, sy, HEADER)
                                + snap.heights[*i - 1]
                                - y)
                                .abs()
                                <= 4
                    })
                    .map(|(i, _)| Drag {
                        index: i - 1,
                        start: y,
                        last: y,
                        original: snap.heights[i - 1],
                        row: true,
                    })
            } else {
                None
            };
            if let Some(d) = boundary {
                if let Some(st) = state(hwnd) {
                    st.drag = Some(d)
                }
                SetCapture(hwnd);
                return 0;
            }
            if !snap.rows.is_empty() && !snap.widths.is_empty() {
                let current = snap.selected.unwrap_or((0, 0));
                let shift = GetKeyState(VK_SHIFT as i32) < 0;
                let prior = snap.range.unwrap_or(GridRange {
                    top: current.0,
                    left: current.1,
                    bottom: current.0,
                    right: current.1,
                });
                let header = if x < ROW_HEADER && y < HEADER {
                    Some((
                        GridRange {
                            top: 0,
                            left: 0,
                            bottom: snap.rows.len() - 1,
                            right: snap.widths.len() - 1,
                        },
                        current,
                    ))
                } else if y < HEADER && x >= ROW_HEADER {
                    index_at_view(&xs, x, snap.frozen.1, sx, ROW_HEADER).map(|c| {
                        (
                            GridRange {
                                top: 0,
                                left: if shift { prior.left.min(c) } else { c },
                                bottom: snap.rows.len() - 1,
                                right: if shift { prior.right.max(c) } else { c },
                            },
                            (current.0.min(snap.rows.len() - 1), c),
                        )
                    })
                } else if x < ROW_HEADER && y >= HEADER {
                    index_at_view(&ys, y, snap.frozen.0, sy, HEADER).map(|r| {
                        (
                            GridRange {
                                top: if shift { prior.top.min(r) } else { r },
                                left: 0,
                                bottom: if shift { prior.bottom.max(r) } else { r },
                                right: snap.widths.len() - 1,
                            },
                            (r, current.1.min(snap.widths.len() - 1)),
                        )
                    })
                } else {
                    None
                };
                if let Some((range, focus)) = header {
                    super::native::grid_select_range(range, focus);
                    if let Some(st) = state(hwnd) {
                        st.anchor = Some(focus);
                        st.focus_pane = None;
                    }
                    SetFocus(hwnd);
                    InvalidateRect(hwnd, null(), 0);
                    return 0;
                }
            }
            if let Some((target, pane)) = hit(hwnd, x, y) {
                let shift = GetKeyState(VK_SHIFT as i32) < 0;
                let anchor = if shift {
                    old_anchor.or(snap.selected).unwrap_or(target)
                } else {
                    target
                };
                if let Some(st) = state(hwnd) {
                    st.anchor = Some(anchor);
                    st.focus_pane = Some(pane);
                    st.selecting = true
                }
                if shift {
                    if let Some(st) = state(hwnd) {
                        st.preview_selection = Some((
                            GridRange {
                                top: anchor.0.min(target.0),
                                left: anchor.1.min(target.1),
                                bottom: anchor.0.max(target.0),
                                right: anchor.1.max(target.1),
                            },
                            target,
                        ))
                    }
                } else {
                    if let Some(st) = state(hwnd) {
                        st.preview_selection = None
                    }
                    super::native::grid_select(target.0, target.1);
                    if let Some(focus) = super::native::grid_snapshot().and_then(|s| s.selected) {
                        if let Some(st) = state(hwnd) {
                            st.anchor = Some(focus)
                        }
                    }
                }
                SetCapture(hwnd);
                SetFocus(hwnd);
                InvalidateRect(hwnd, null(), 0);
            }
            0
        }
        WM_MOUSEMOVE => {
            let (x, y) = coordinate(l);
            let (drag, selecting, anchor) = state(hwnd)
                .map(|s| (s.drag, s.selecting, s.anchor))
                .unwrap_or((None, false, None));
            if let Some(mut d) = drag {
                d.last = if d.row { y } else { x };
                if let Some(st) = state(hwnd) {
                    st.drag = Some(d)
                }
                InvalidateRect(hwnd, null(), 0);
            } else if selecting {
                let mut client = RECT::default();
                GetClientRect(hwnd, &mut client);
                if y < HEADER {
                    scroll(hwnd, true, SB_LINEUP)
                } else if y >= client.bottom {
                    scroll(hwnd, true, SB_LINEDOWN)
                }
                if x < ROW_HEADER {
                    scroll(hwnd, false, SB_LINEUP)
                } else if x >= client.right {
                    scroll(hwnd, false, SB_LINEDOWN)
                }
                let px = x.clamp(ROW_HEADER, client.right.max(ROW_HEADER + 1) - 1);
                let py = y.clamp(HEADER, client.bottom.max(HEADER + 1) - 1);
                if let (Some(anchor), Some((target, pane))) = (anchor, hit(hwnd, px, py)) {
                    if let Some(st) = state(hwnd) {
                        st.focus_pane = Some(pane);
                        st.preview_selection = Some((
                            GridRange {
                                top: anchor.0.min(target.0),
                                left: anchor.1.min(target.1),
                                bottom: anchor.0.max(target.0),
                                right: anchor.1.max(target.1),
                            },
                            target,
                        ));
                    }
                    InvalidateRect(hwnd, null(), 0);
                }
            }
            0
        }
        WM_LBUTTONUP => {
            let (x, y) = coordinate(l);
            let (drag, selecting, selection) = if let Some(st) = state(hwnd) {
                let d = st.drag.take();
                let active = st.selecting;
                st.selecting = false;
                (d, active, st.preview_selection.take())
            } else {
                (None, false, None)
            };
            if drag.is_some() || selecting {
                ReleaseCapture();
            }
            if let Some((range, focus)) = selection {
                super::native::grid_select_range(range, focus);
                InvalidateRect(hwnd, null(), 0);
            }
            if let Some(d) = drag {
                let distance = if d.row { y - d.start } else { x - d.start };
                let max = if d.row { 65535 } else { 1785 };
                let size = (d.original + distance).clamp(1, max) as u16;
                super::native::grid_resize(d.row, d.index, size);
                update_scroll(hwnd);
                InvalidateRect(hwnd, null(), 0);
            }
            0
        }
        WM_CAPTURECHANGED => {
            if let Some(st) = state(hwnd) {
                st.drag = None;
                st.selecting = false;
                st.preview_selection = None
            }
            InvalidateRect(hwnd, null(), 0);
            0
        }
        WM_LBUTTONDBLCLK => {
            let (x, y) = coordinate(l);
            if let Some(((r, c), pane)) = hit(hwnd, x, y) {
                if let Some(st) = state(hwnd) {
                    st.focus_pane = Some(pane);
                }
                super::native::grid_select(r, c);
                begin_editor(hwnd, false)
            }
            0
        }
        WM_COMMAND => {
            if ((w >> 16) & 0xffff) as u16 == EN_KILLFOCUS as u16 {
                finish_editor(hwnd, true);
            }
            0
        }
        COMMIT => {
            if finish_editor(hwnd, true) {
                SetFocus(hwnd);
            }
            0
        }
        CANCEL => {
            finish_editor(hwnd, false);
            SetFocus(hwnd);
            0
        }
        WM_GETDLGCODE => DLGC_WANTARROWS as isize | DLGC_WANTCHARS as isize | DLGC_WANTTAB as isize,
        WM_KEYDOWN => {
            let key = w as u16;
            let control = GetKeyState(VK_CONTROL as i32) < 0;
            let shift = GetKeyState(VK_SHIFT as i32) < 0;
            if control {
                if key == 65 {
                    if let Some(snap) = super::native::grid_snapshot() {
                        if !snap.rows.is_empty() && !snap.widths.is_empty() {
                            let focus = snap.selected.unwrap_or((0, 0));
                            super::native::grid_select_range(
                                GridRange {
                                    top: 0,
                                    left: 0,
                                    bottom: snap.rows.len() - 1,
                                    right: snap.widths.len() - 1,
                                },
                                focus,
                            );
                            InvalidateRect(hwnd, null(), 0);
                        }
                        return 0;
                    }
                }
                if let Some(snap) = super::native::grid_snapshot() {
                    let id = match key {
                        67 => snap.clipboard.map(|v| v.0),
                        86 => snap.clipboard.map(|v| v.1),
                        90 => Some(4),
                        89 => Some(5),
                        _ => None,
                    };
                    if let Some(id) = id {
                        SendMessageW(GetParent(hwnd), WM_COMMAND, id as usize, 0);
                        return 0;
                    }
                }
            }
            match key {
                VK_LEFT | VK_RIGHT | VK_UP | VK_DOWN | VK_TAB | VK_HOME | VK_END | VK_PRIOR
                | VK_NEXT => {
                    move_selection(hwnd, key, shift);
                    0
                }
                VK_RETURN | VK_F2 => {
                    begin_editor(hwnd, false);
                    0
                }
                VK_DELETE => {
                    super::native::grid_commit(String::new());
                    0
                }
                _ => DefWindowProcW(hwnd, msg, w, l),
            }
        }
        WM_CHAR => {
            if w >= 32 && GetKeyState(VK_CONTROL as i32) >= 0 {
                begin_editor(hwnd, true);
                if let Some(st) = state(hwnd) {
                    if !st.editor.is_null() {
                        SendMessageW(st.editor, WM_CHAR, w, l);
                    }
                }
                0
            } else {
                DefWindowProcW(hwnd, msg, w, l)
            }
        }
        _ => DefWindowProcW(hwnd, msg, w, l),
    }
}
pub(super) unsafe fn create(parent: HWND, id: usize) -> HWND {
    let instance = GetModuleHandleW(null());
    let class = crate::wide("Off42VariableGrid");
    let wc = WNDCLASSW {
        style: CS_HREDRAW | CS_VREDRAW | CS_DBLCLKS,
        lpfnWndProc: Some(proc),
        hInstance: instance,
        hCursor: LoadCursorW(null_mut(), IDC_ARROW),
        hbrBackground: (COLOR_WINDOW + 1) as HBRUSH,
        lpszClassName: class.as_ptr(),
        ..zeroed()
    };
    RegisterClassW(&wc);
    CreateWindowExW(
        WS_EX_CLIENTEDGE,
        class.as_ptr(),
        null(),
        WS_CHILD | WS_VISIBLE | WS_TABSTOP | WS_HSCROLL | WS_VSCROLL,
        0,
        0,
        0,
        0,
        parent,
        id as HMENU,
        instance,
        null(),
    )
}
pub(super) unsafe fn refresh(hwnd: HWND) {
    update_scroll(hwnd);
    position_editor(hwnd);
    InvalidateRect(hwnd, null(), 0);
}
