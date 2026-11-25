//! A modal, scrollable screen rendering of RichEdit's actual formatted pages.
use crate::{wide, RichPrint};
use std::{
    mem::{size_of, zeroed},
    ptr::{null, null_mut},
};
use windows_sys::Win32::{
    Foundation::*,
    Graphics::Gdi::*,
    System::LibraryLoader::GetModuleHandleW,
    UI::{
        Controls::SetScrollInfo,
        Input::KeyboardAndMouse::{
            EnableWindow, SetActiveWindow, SetFocus, VK_DOWN, VK_END, VK_HOME, VK_NEXT, VK_PRIOR,
            VK_UP,
        },
        WindowsAndMessaging::*,
    },
};
const MAX_PIXELS: usize = 32_000_000;
struct Page {
    width: i32,
    height: i32,
    pixels: Vec<u8>,
}
struct Preview {
    pages: Vec<Page>,
    scroll: i32,
    zoom: f64,
}
unsafe fn render(edit: HWND, job: &RichPrint) -> Result<Vec<Page>, String> {
    let geometry = job.geometry;
    if !geometry.valid() {
        return Err("Invalid page geometry".into());
    }
    let screen = GetDC(null_mut());
    if screen.is_null() {
        return Err("Cannot access display device".into());
    }
    let dc = CreateCompatibleDC(screen);
    if dc.is_null() {
        ReleaseDC(null_mut(), screen);
        return Err("Cannot create preview device".into());
    }
    let dpi_x = GetDeviceCaps(screen, LOGPIXELSX as i32);
    let dpi_y = GetDeviceCaps(screen, LOGPIXELSY as i32);
    if dpi_x <= 0 || dpi_y <= 0 {
        DeleteDC(dc);
        ReleaseDC(null_mut(), screen);
        return Err("Invalid display DPI".into());
    }
    let w = (i64::from(geometry.width_twips) * i64::from(dpi_x) / 1440) as i32;
    let h = (i64::from(geometry.height_twips) * i64::from(dpi_y) / 1440) as i32;
    if w <= 0
        || h <= 0
        || w > 8192
        || h > 8192
        || (w as usize).saturating_mul(h as usize) > MAX_PIXELS
    {
        DeleteDC(dc);
        ReleaseDC(null_mut(), screen);
        return Err("Preview page exceeds display limits".into());
    }
    let (print_edit, temporary) = match crate::rich_notes::temporary_edit(edit, job) {
        Ok(v) => v,
        Err(e) => {
            DeleteDC(dc);
            ReleaseDC(null_mut(), screen);
            return Err(e);
        }
    };
    let result = (|| {
        let note_layout = if job.notes.is_empty() {
            None
        } else {
            Some(crate::rich_notes::paginate(print_edit, dc, job, (0, 0))?)
        };
        let ranges = if let Some(layout) = &note_layout {
            layout.pages.iter().map(|p| (p.start, p.end)).collect()
        } else {
            crate::rich_print::paginate(print_edit, dc, geometry, (0, 0))?
        };
        if ranges.len() > 128
            || ranges
                .len()
                .saturating_mul(w as usize)
                .saturating_mul(h as usize)
                > MAX_PIXELS
        {
            return Err("Document exceeds preview memory limit".into());
        }
        let mut pages = Vec::new();
        for (index, (start, end)) in ranges.iter().copied().enumerate() {
            let mut bmi: BITMAPINFO = zeroed();
            bmi.bmiHeader.biSize = size_of::<BITMAPINFOHEADER>() as u32;
            bmi.bmiHeader.biWidth = w;
            bmi.bmiHeader.biHeight = -h;
            bmi.bmiHeader.biPlanes = 1;
            bmi.bmiHeader.biBitCount = 32;
            bmi.bmiHeader.biCompression = BI_RGB;
            let mut bits = null_mut();
            let bitmap = CreateDIBSection(dc, &bmi, DIB_RGB_COLORS, &mut bits, null_mut(), 0);
            if bitmap.is_null() || bits.is_null() {
                if !bitmap.is_null() {
                    DeleteObject(bitmap as _);
                }
                return Err("Cannot allocate preview page".into());
            }
            let previous = SelectObject(dc, bitmap as _);
            PatBlt(dc, 0, 0, w, h, WHITENESS);
            let drawn = if let Some(layout) = &note_layout {
                crate::rich_notes::draw_page(print_edit, dc, job, layout, index, (0, 0), (0, 0))
            } else {
                let next = crate::rich_print::body(print_edit, dc, geometry, (0, 0), start);
                if next != end {
                    Err("Preview pagination changed while rendering".into())
                } else {
                    crate::rich_print::decorations(dc, job, (0, 0), index, ranges.len())
                }
            };
            GdiFlush();
            let pixels = if drawn.is_ok() {
                std::slice::from_raw_parts(bits as *const u8, (w as usize) * (h as usize) * 4)
                    .to_vec()
            } else {
                Vec::new()
            };
            SelectObject(dc, previous);
            DeleteObject(bitmap as _);
            drawn?;
            pages.push(Page {
                width: w,
                height: h,
                pixels,
            });
        }
        Ok(pages)
    })();
    crate::rich_print::release(print_edit);
    if temporary {
        DestroyWindow(print_edit);
    }
    DeleteDC(dc);
    ReleaseDC(null_mut(), screen);
    result
}
unsafe fn update_scroll(hwnd: HWND, state: &mut Preview) {
    let mut rc: RECT = zeroed();
    GetClientRect(hwnd, &mut rc);
    let total = state
        .pages
        .iter()
        .fold(24i64, |n, p| {
            n + ((p.height as f64 * state.zoom).round() as i64) + 24
        })
        .min(i32::MAX as i64) as i32;
    let page = rc.bottom.max(1);
    state.scroll = state.scroll.clamp(0, (total - page).max(0));
    let info = SCROLLINFO {
        cbSize: size_of::<SCROLLINFO>() as u32,
        fMask: SIF_RANGE | SIF_PAGE | SIF_POS,
        nMin: 0,
        nMax: total.saturating_sub(1),
        nPage: page as u32,
        nPos: state.scroll,
        nTrackPos: 0,
    };
    SetScrollInfo(hwnd, SB_VERT, &info, 1);
}
unsafe extern "system" fn proc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    if msg == WM_NCCREATE {
        let cs = &*(l as *const CREATESTRUCTW);
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, cs.lpCreateParams as isize);
        return 1;
    }
    let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut Preview;
    if ptr.is_null() {
        return DefWindowProcW(hwnd, msg, w, l);
    }
    let s = &mut *ptr;
    match msg {
        WM_SIZE => {
            update_scroll(hwnd, s);
            InvalidateRect(hwnd, null(), 1);
            0
        }
        WM_VSCROLL => {
            let mut info: SCROLLINFO = zeroed();
            info.cbSize = size_of::<SCROLLINFO>() as u32;
            info.fMask = SIF_TRACKPOS;
            GetScrollInfo(hwnd, SB_VERT, &mut info);
            let action = (w & 0xffff) as i32;
            s.scroll = match action {
                SB_LINEUP => s.scroll - 40,
                SB_LINEDOWN => s.scroll + 40,
                SB_PAGEUP => s.scroll - 500,
                SB_PAGEDOWN => s.scroll + 500,
                SB_THUMBPOSITION | SB_THUMBTRACK => info.nTrackPos,
                SB_TOP => 0,
                SB_BOTTOM => i32::MAX,
                _ => s.scroll,
            };
            update_scroll(hwnd, s);
            InvalidateRect(hwnd, null(), 1);
            0
        }
        WM_MOUSEWHEEL => {
            let delta = ((w >> 16) as u16 as i16) as i32;
            s.scroll = s.scroll.saturating_sub(delta * 3);
            update_scroll(hwnd, s);
            InvalidateRect(hwnd, null(), 1);
            0
        }
        WM_KEYDOWN => {
            let code = w as u16;
            if code == b'+' as u16 || code == 0xbb {
                s.zoom = (s.zoom * 1.2).min(2.0)
            } else if code == b'-' as u16 || code == 0xbd {
                s.zoom = (s.zoom / 1.2).max(0.25)
            } else if code == VK_UP {
                s.scroll -= 40
            } else if code == VK_DOWN {
                s.scroll += 40
            } else if code == VK_PRIOR {
                s.scroll -= 500
            } else if code == VK_NEXT {
                s.scroll += 500
            } else if code == VK_HOME {
                s.scroll = 0
            } else if code == VK_END {
                s.scroll = i32::MAX
            } else {
                return DefWindowProcW(hwnd, msg, w, l);
            }
            update_scroll(hwnd, s);
            InvalidateRect(hwnd, null(), 1);
            0
        }
        WM_PAINT => {
            let mut ps: PAINTSTRUCT = zeroed();
            let dc = BeginPaint(hwnd, &mut ps);
            let mut rc: RECT = zeroed();
            GetClientRect(hwnd, &mut rc);
            FillRect(dc, &rc, GetSysColorBrush(COLOR_APPWORKSPACE));
            SetStretchBltMode(dc, HALFTONE);
            let mut y = 24 - s.scroll;
            for page in &s.pages {
                let width = (page.width as f64 * s.zoom).round() as i32;
                let height = (page.height as f64 * s.zoom).round() as i32;
                let x = ((rc.right - width) / 2).max(8);
                if y + height >= 0 && y < rc.bottom {
                    let mut bmi: BITMAPINFO = zeroed();
                    bmi.bmiHeader.biSize = size_of::<BITMAPINFOHEADER>() as u32;
                    bmi.bmiHeader.biWidth = page.width;
                    bmi.bmiHeader.biHeight = -page.height;
                    bmi.bmiHeader.biPlanes = 1;
                    bmi.bmiHeader.biBitCount = 32;
                    bmi.bmiHeader.biCompression = BI_RGB;
                    StretchDIBits(
                        dc,
                        x,
                        y,
                        width,
                        height,
                        0,
                        0,
                        page.width,
                        page.height,
                        page.pixels.as_ptr() as _,
                        &bmi,
                        DIB_RGB_COLORS,
                        SRCCOPY,
                    );
                }
                y = y.saturating_add(height + 24);
            }
            EndPaint(hwnd, &ps);
            0
        }
        WM_CLOSE => {
            DestroyWindow(hwnd);
            0
        }
        WM_NCDESTROY => {
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
            DefWindowProcW(hwnd, msg, w, l)
        }
        _ => DefWindowProcW(hwnd, msg, w, l),
    }
}
pub unsafe fn open(owner: HWND, edit: HWND, job: RichPrint) -> Result<(), String> {
    let pages = render(edit, &job)?;
    let instance = GetModuleHandleW(null());
    let class = wide("Suite42RichPrintPreview");
    let wc = WNDCLASSW {
        lpfnWndProc: Some(proc),
        hInstance: instance,
        lpszClassName: class.as_ptr(),
        hCursor: LoadCursorW(null_mut(), IDC_ARROW),
        ..zeroed()
    };
    RegisterClassW(&wc);
    let mut state = Box::new(Preview {
        pages,
        scroll: 0,
        zoom: 1.0,
    });
    let state_ptr = &mut *state as *mut Preview;
    let hwnd = CreateWindowExW(
        0,
        class.as_ptr(),
        wide("Print layout preview · +/− zoom").as_ptr(),
        WS_OVERLAPPEDWINDOW | WS_VSCROLL | WS_VISIBLE,
        CW_USEDEFAULT,
        CW_USEDEFAULT,
        900,
        700,
        owner,
        null_mut(),
        instance,
        state_ptr as _,
    );
    if hwnd.is_null() {
        return Err("Cannot open print preview".into());
    }
    EnableWindow(owner, 0);
    ShowWindow(hwnd, SW_SHOW);
    SetFocus(hwnd);
    let mut msg: MSG = zeroed();
    while IsWindow(hwnd) != 0 {
        let status = GetMessageW(&mut msg, null_mut(), 0, 0);
        if status <= 0 {
            if status == 0 {
                PostQuitMessage(msg.wParam as i32);
            }
            break;
        }
        TranslateMessage(&msg);
        DispatchMessageW(&msg);
    }
    if IsWindow(hwnd) != 0 {
        DestroyWindow(hwnd);
    }
    EnableWindow(owner, 1);
    SetActiveWindow(owner);
    Ok(())
}
