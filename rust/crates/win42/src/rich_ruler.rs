//! Native ruler controls; the document remains authoritative in the host app.
use crate::{wide, RichRuler};
use std::{
    mem::zeroed,
    ptr::{null, null_mut},
};
use windows_sys::Win32::{
    Foundation::*,
    Graphics::Gdi::*,
    System::LibraryLoader::GetModuleHandleW,
    UI::{
        HiDpi::GetDpiForWindow,
        Input::KeyboardAndMouse::{GetKeyState, ReleaseCapture, SetCapture, VK_MENU},
        WindowsAndMessaging::*,
    },
};
pub const BEGIN: u32 = WM_APP + 241;
pub const APPLY: u32 = WM_APP + 242;
#[repr(C)]
pub struct Command {
    pub kind: u32,
    pub position: i32,
    pub from: i32,
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Axis {
    Horizontal,
    Vertical,
}
#[derive(Clone)]
struct View {
    data: RichRuler,
    origin: i32,
    dpi: i32,
}
struct State {
    axis: Axis,
    view: Option<View>,
    drag: Option<(u32, i32, i32, i32)>,
}
fn pixel(twips: i32, dpi: i32, zoom: u16) -> i32 {
    ((i64::from(twips) * i64::from(dpi) * i64::from(zoom)) / 144_000)
        .clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
}
fn position(pixel: i32, dpi: i32, zoom: u16) -> i32 {
    ((i64::from(pixel) * 144_000) / (i64::from(dpi.max(1)) * i64::from(zoom.max(1))))
        .clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
}
unsafe fn line(dc: HDC, x1: i32, y1: i32, x2: i32, y2: i32) {
    MoveToEx(dc, x1, y1, null_mut());
    LineTo(dc, x2, y2);
}
unsafe fn paint(hwnd: HWND, s: &State) {
    let mut ps: PAINTSTRUCT = zeroed();
    let dc = BeginPaint(hwnd, &mut ps);
    let mut rc: RECT = zeroed();
    GetClientRect(hwnd, &mut rc);
    FillRect(dc, &rc, GetSysColorBrush(COLOR_BTNFACE));
    if let Some(v) = &s.view {
        let g = v.data.page;
        let zoom = v.data.zoom_percent;
        let scale = |n| v.origin + pixel(n, v.dpi, zoom);
        let extent = match s.axis {
            Axis::Horizontal => g.width_twips,
            Axis::Vertical => g.height_twips,
        };
        let pen = CreatePen(PS_SOLID, 1, 0x808080);
        let old = SelectObject(dc, pen as _);
        let mut unit = 0;
        while unit <= extent {
            let x = scale(unit);
            let major = unit % 1440 == 0;
            match s.axis {
                Axis::Horizontal => {
                    if x >= 0 && x < rc.right {
                        line(dc, x, 0, x, if major { 12 } else { 6 });
                        if major {
                            let label = format!("{}", unit / 1440);
                            let w: Vec<u16> = label.encode_utf16().collect();
                            TextOutW(dc, x + 2, 0, w.as_ptr(), w.len() as i32);
                        }
                    }
                }
                Axis::Vertical => {
                    if x >= 0 && x < rc.bottom {
                        line(dc, rc.right - if major { 12 } else { 6 }, x, rc.right, x);
                    }
                }
            }
            unit += 360;
        }
        let mark = |n: i32, color: u32| {
            let p = scale(n);
            let pen = CreatePen(PS_SOLID, 2, color);
            let previous = SelectObject(dc, pen as _);
            match s.axis {
                Axis::Horizontal => line(dc, p, 0, p, rc.bottom),
                Axis::Vertical => line(dc, 0, p, rc.right, p),
            };
            SelectObject(dc, previous);
            DeleteObject(pen as _);
        };
        match s.axis {
            Axis::Horizontal => {
                mark(g.margin_left_twips, 0x606060);
                mark(g.width_twips - g.margin_right_twips, 0x606060);
                if let Some(p) = &v.data.paragraph {
                    let y = rc.bottom - 1;
                    let l = scale(g.margin_left_twips + p.left_twips);
                    let f = scale(g.margin_left_twips + p.left_twips + p.first_line_twips);
                    let r = scale(g.width_twips - g.margin_right_twips - p.right_twips);
                    let blue = CreatePen(PS_SOLID, 2, 0xB04020);
                    let prev = SelectObject(dc, blue as _);
                    line(dc, l - 4, y - 3, l + 4, y - 3);
                    line(dc, f - 4, y - 11, f + 4, y - 11);
                    line(dc, r - 4, y - 3, r + 4, y - 3);
                    for tab in &p.tabs_twips {
                        let x = scale(g.margin_left_twips + *tab);
                        line(dc, x, y - 8, x, y);
                        line(dc, x, y, x + 5, y);
                    }
                    SelectObject(dc, prev);
                    DeleteObject(blue as _);
                }
            }
            Axis::Vertical => {
                mark(g.margin_top_twips, 0x606060);
                mark(g.height_twips - g.margin_bottom_twips, 0x606060);
            }
        }
        if let Some((_, _, _, current)) = s.drag {
            let pen = CreatePen(PS_DOT, 1, 0x0000ff);
            let previous = SelectObject(dc, pen as _);
            match s.axis {
                Axis::Horizontal => line(dc, current, 0, current, rc.bottom),
                Axis::Vertical => line(dc, 0, current, rc.right, current),
            }
            SelectObject(dc, previous);
            DeleteObject(pen as _);
        }
        SelectObject(dc, old);
        DeleteObject(pen as _);
    }
    EndPaint(hwnd, &ps);
}
unsafe extern "system" fn proc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    if msg == WM_NCCREATE {
        let cs = &*(l as *const CREATESTRUCTW);
        let axis = if cs.lpCreateParams as usize == 0 {
            Axis::Horizontal
        } else {
            Axis::Vertical
        };
        SetWindowLongPtrW(
            hwnd,
            GWLP_USERDATA,
            Box::into_raw(Box::new(State {
                axis,
                view: None,
                drag: None,
            })) as isize,
        );
        return 1;
    }
    let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut State;
    if ptr.is_null() {
        return DefWindowProcW(hwnd, msg, w, l);
    }
    let s = &mut *ptr;
    match msg {
        WM_PAINT => {
            paint(hwnd, s);
            0
        }
        WM_LBUTTONDOWN => {
            SendMessageW(GetParent(hwnd), BEGIN, 0, 0);
            let Some(v) = s.view.as_ref() else { return 0 };
            let x = (l as u32 as u16 as i16) as i32;
            let y = ((l as u32 >> 16) as u16 as i16) as i32;
            let at = if s.axis == Axis::Horizontal { x } else { y };
            let cross = if s.axis == Axis::Horizontal { y } else { x };
            let scale = |n| v.origin + pixel(n, v.dpi, v.data.zoom_percent);
            let threshold = (6 * v.dpi / 96).max(4);
            let near = |n: i32| (at - scale(n)).abs() <= threshold;
            let page = v.data.page;
            let mut hit = None;
            if s.axis == Axis::Horizontal {
                let mut rc: RECT = zeroed();
                GetClientRect(hwnd, &mut rc);
                if cross >= rc.bottom / 2 {
                    if let Some(p) = &v.data.paragraph {
                        if near(page.margin_left_twips + p.left_twips + p.first_line_twips)
                            && cross < rc.bottom - 6
                        {
                            hit = Some((6, 0))
                        } else if near(page.margin_left_twips + p.left_twips) {
                            hit = Some((4, 0))
                        } else if near(page.width_twips - page.margin_right_twips - p.right_twips) {
                            hit = Some((5, 0))
                        } else if let Some(tab) = p
                            .tabs_twips
                            .iter()
                            .find(|t| near(page.margin_left_twips + **t))
                        {
                            hit = Some((8, *tab))
                        }
                    }
                } else if near(page.margin_left_twips) {
                    hit = Some((0, 0))
                } else if near(page.width_twips - page.margin_right_twips) {
                    hit = Some((1, 0))
                } else if v.data.paragraph.is_some() {
                    hit = Some((7, 0));
                }
            } else if near(page.margin_top_twips) {
                hit = Some((2, 0))
            } else if near(page.height_twips - page.margin_bottom_twips) {
                hit = Some((3, 0))
            }
            if let Some((kind, from)) = hit {
                s.drag = Some((kind, from, at, at));
                SetCapture(hwnd);
                InvalidateRect(hwnd, null(), 0);
            }
            0
        }
        WM_MOUSEMOVE => {
            if let Some((kind, from, start, _)) = s.drag {
                let at = if s.axis == Axis::Horizontal {
                    (l as u32 as u16 as i16) as i32
                } else {
                    ((l as u32 >> 16) as u16 as i16) as i32
                };
                s.drag = Some((kind, from, start, at));
                InvalidateRect(hwnd, null(), 0);
            }
            0
        }
        WM_LBUTTONUP => {
            if let Some((kind, from, start, at)) = s.drag.take() {
                let view = s.view.clone();
                let axis = s.axis;
                ReleaseCapture();
                InvalidateRect(hwnd, null(), 0);
                if kind == 7 || (at - start).abs() >= 3 {
                    if let Some(v) = &view {
                        let mut twips = position(at - v.origin, v.dpi, v.data.zoom_percent);
                        if GetKeyState(VK_MENU as i32) >= 0 {
                            twips = (twips as f64 / 142.0).round() as i32 * 142;
                        }
                        let cross = if axis == Axis::Horizontal {
                            ((l as u32 >> 16) as u16 as i16) as i32
                        } else {
                            (l as u32 as u16 as i16) as i32
                        };
                        let mut rc: RECT = zeroed();
                        GetClientRect(hwnd, &mut rc);
                        let cross_limit = if axis == Axis::Horizontal {
                            rc.bottom
                        } else {
                            rc.right
                        };
                        let kind = if kind == 8 && (cross < -12 || cross > cross_limit + 12) {
                            9
                        } else {
                            kind
                        };
                        let command = Command {
                            kind,
                            position: twips,
                            from,
                        };
                        SendMessageW(GetParent(hwnd), APPLY, 0, &command as *const _ as isize);
                    }
                }
            }
            0
        }
        WM_CAPTURECHANGED => {
            s.drag = None;
            InvalidateRect(hwnd, null(), 0);
            0
        }
        WM_NCDESTROY => {
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
            drop(Box::from_raw(ptr));
            DefWindowProcW(hwnd, msg, w, l)
        }
        _ => DefWindowProcW(hwnd, msg, w, l),
    }
}
pub unsafe fn create(parent: HWND, vertical: bool) -> HWND {
    let instance = GetModuleHandleW(null());
    let class = wide("Suite42RichRuler");
    let wc = WNDCLASSW {
        lpfnWndProc: Some(proc),
        hInstance: instance,
        lpszClassName: class.as_ptr(),
        hCursor: LoadCursorW(null_mut(), IDC_ARROW),
        ..zeroed()
    };
    RegisterClassW(&wc);
    CreateWindowExW(
        0,
        class.as_ptr(),
        null(),
        WS_CHILD | WS_VISIBLE,
        0,
        0,
        0,
        0,
        parent,
        null_mut(),
        instance,
        usize::from(vertical) as _,
    )
}
pub unsafe fn update(hwnd: HWND, data: RichRuler, origin: i32) {
    let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut State;
    if ptr.is_null() {
        return;
    }
    let dpi = GetDpiForWindow(hwnd) as i32;
    let next = View { data, origin, dpi };
    let state = &mut *ptr;
    let changed = state.view.as_ref().is_none_or(|old| {
        old.data != next.data || old.origin != next.origin || old.dpi != next.dpi
    });
    if changed {
        state.view = Some(next);
        InvalidateRect(hwnd, null(), 1);
    }
}
