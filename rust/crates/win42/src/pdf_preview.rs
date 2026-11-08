//! Modal label preview: immutable PDF data, one bounded page bitmap, explicit save/print/settings.
use crate::wide;
use std::{
    cell::RefCell,
    mem::{size_of, zeroed},
    ptr::{null, null_mut},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
};
use windows_sys::Win32::{
    Foundation::*,
    Graphics::Gdi::*,
    System::LibraryLoader::GetModuleHandleW,
    UI::{
        HiDpi::GetDpiForWindow,
        Input::KeyboardAndMouse::{
            EnableWindow, GetKeyState, ReleaseCapture, SetCapture, SetFocus, VK_CONTROL,
        },
        WindowsAndMessaging::*,
    },
};
#[derive(Clone, Copy)]
pub(super) enum Action {
    Save,
    Print,
    Settings,
}
pub(super) enum Update {
    Keep,
    Message(String),
    Replace(Arc<[u8]>),
}
struct Worker {
    cancel: Arc<AtomicBool>,
    rx: mpsc::Receiver<Result<(usize, (u32, u32, Vec<u8>)), String>>,
}
impl Drop for Worker {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}
struct State {
    bytes: Arc<[u8]>,
    buttons: Vec<(u16, HWND)>,
    status: HWND,
    frame: Option<(u32, u32, Vec<u8>)>,
    worker: Option<Worker>,
    pages: usize,
    page: usize,
    zoom: f64,
    pan: (f64, f64),
    drag: Option<(i32, i32, f64, f64)>,
    area: RECT,
    pending: Option<Action>,
    done: bool,
    message: String,
}
thread_local! {static PREVIEW:RefCell<Option<State>>=const{RefCell::new(None)};}
fn begin_render() {
    PREVIEW.with(|slot| {
        let mut slot = slot.borrow_mut();
        let Some(s) = slot.as_mut() else {
            return;
        };
        s.worker = None;
        s.frame = None;
        s.message = crate::localization::text("Preparing preview…");
        let bytes = s.bytes.clone();
        let page = s.page;
        let cancel = Arc::new(AtomicBool::new(false));
        let token = cancel.clone();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let result = (|| {
                let doc = crate::pdfium::Document::open(bytes, &token)?;
                if token.load(Ordering::Relaxed) {
                    return Err("Preview cancelled".into());
                }
                let pages = doc.page_count();
                let (width, height, mut pixels) = doc.render(page, 2000, 2000)?;
                for pixel in pixels.chunks_exact_mut(4) {
                    pixel.swap(0, 2);
                }
                let frame = (width, height, pixels);
                if token.load(Ordering::Relaxed) {
                    return Err("Preview cancelled".into());
                }
                Ok((pages, frame))
            })();
            let _ = tx.send(result);
        });
        s.worker = Some(Worker { cancel, rx });
    });
}
unsafe fn update_status() {
    PREVIEW.with(|slot| {
        let slot = slot.borrow();
        let Some(s) = slot.as_ref() else {
            return;
        };
        let page = crate::localization::text("Page %d of %d")
            .replacen("%d", &(s.page + 1).to_string(), 1)
            .replacen("%d", &s.pages.to_string(), 1);
        let text = if s.message.is_empty() {
            format!(
                "{page} · {}% · {}",
                (s.zoom * 100.).round() as u32,
                crate::localization::text("Print at actual size")
            )
        } else {
            s.message.clone()
        };
        SetWindowTextW(s.status, wide(&text).as_ptr());
        for &(id, button) in &s.buttons {
            let enabled = match id {
                3 => s.pages > 0,
                5 => s.page > 0,
                6 => s.pages > s.page + 1,
                7 => s.zoom > 1.,
                8 => s.zoom < 4.,
                _ => true,
            };
            EnableWindow(button, enabled as i32);
        }
    });
}
fn clamp_pan(state: &mut State) {
    let Some((width, height, _)) = &state.frame else {
        state.pan = (0., 0.);
        return;
    };
    let w = f64::from((state.area.right - state.area.left).max(1));
    let h = f64::from((state.area.bottom - state.area.top).max(1));
    let scale = (w / f64::from(*width)).min(h / f64::from(*height)) * state.zoom;
    let x = (f64::from(*width) * scale - w).max(0.) / 2.;
    let y = (f64::from(*height) * scale - h).max(0.) / 2.;
    state.pan = (state.pan.0.clamp(-x, x), state.pan.1.clamp(-y, y));
}
unsafe fn button_width(button: HWND, minimum: i32, scale: f64) -> i32 {
    let length = GetWindowTextLengthW(button).clamp(0, 1024);
    let mut text = vec![0u16; length as usize + 1];
    GetWindowTextW(button, text.as_mut_ptr(), text.len() as i32);
    let dc = GetDC(button);
    let font = SendMessageW(button, WM_GETFONT, 0, 0) as HGDIOBJ;
    let previous = SelectObject(dc, font);
    let mut size: SIZE = zeroed();
    let _ = GetTextExtentPoint32W(dc, text.as_ptr(), length, &mut size);
    SelectObject(dc, previous);
    ReleaseDC(button, dc);
    ((minimum as f64 * scale).round() as i32).max(size.cx + (24. * scale).round() as i32)
}
unsafe fn layout(h: HWND) {
    let mut rect: RECT = zeroed();
    GetClientRect(h, &mut rect);
    let scale = GetDpiForWindow(h).max(96) as f64 / 96.;
    let unit = |n: i32| (n as f64 * scale).round() as i32;
    PREVIEW.with(|slot| {
        let mut slot = slot.borrow_mut();
        let Some(s) = slot.as_mut() else {
            return;
        };
        let pad = unit(10);
        let gap = unit(8);
        let bh = unit(28);
        let w = rect.right.max(1);
        let height = rect.bottom.max(1);
        // Wrap buttons instead of allowing controls outside a small or high-DPI work area.
        let mut x = pad;
        let mut y = pad;
        for id in [5, 6, 7, 8, 9] {
            if let Some(&(_, button)) = s.buttons.iter().find(|(n, _)| *n == id) {
                let bw = button_width(
                    button,
                    if matches!(id, 7 | 8) {
                        38
                    } else if id == 9 {
                        86
                    } else {
                        124
                    },
                    scale,
                );
                if x + bw > w - pad && x > pad {
                    x = pad;
                    y += bh + gap;
                }
                MoveWindow(button, x, y, bw.min((w - 2 * pad).max(1)), bh, 1);
                x += bw + gap;
            }
        }
        let top = y + bh + gap;
        MoveWindow(s.status, pad, top, (w - 2 * pad).max(1), unit(28), 1);
        let action_widths = [(1, 132), (3, 100), (4, 144), (2, 100)].map(|(id, width)| {
            let button = s.buttons.iter().find(|(n, _)| *n == id).unwrap().1;
            (id, button_width(button, width, scale))
        });
        let mut action_rows = 1;
        let mut used = 0;
        for &(_, width) in &action_widths {
            let width = width + gap;
            if used + width > w - 2 * pad && used > 0 {
                action_rows += 1;
                used = 0;
            }
            used += width;
        }
        let bottom = height - pad - action_rows * (bh + gap) + gap;
        let mut x = pad;
        let mut y = bottom;
        for &(id, width) in &action_widths {
            if let Some(&(_, button)) = s.buttons.iter().find(|(n, _)| *n == id) {
                let bw = width;
                if x + bw > w - pad && x > pad {
                    x = pad;
                    y += bh + gap;
                }
                MoveWindow(button, x, y, bw.min((w - 2 * pad).max(1)), bh, 1);
                x += bw + gap;
            }
        }
        s.area = RECT {
            left: pad,
            top: top + unit(32),
            right: w - pad,
            bottom: (bottom - gap).max(top + unit(33)),
        };
        clamp_pan(s);
    });
    InvalidateRect(h, null(), 0);
}
fn navigate(delta: isize) -> bool {
    PREVIEW.with(|slot| {
        let mut slot = slot.borrow_mut();
        let Some(s) = slot.as_mut() else {
            return false;
        };
        let next = s.page.saturating_add_signed(delta);
        if next >= s.pages || next == s.page {
            return false;
        }
        s.page = next;
        s.pan = (0., 0.);
        true
    })
}
unsafe fn zoom(h: HWND, multiplier: f64) {
    PREVIEW.with(|slot| {
        if let Some(s) = slot.borrow_mut().as_mut() {
            s.zoom = (s.zoom * multiplier).clamp(1., 4.);
            s.pan = (0., 0.);
            s.message.clear();
        }
    });
    update_status();
    InvalidateRect(h, null(), 0);
}
unsafe fn paint(h: HWND) {
    let mut ps: PAINTSTRUCT = zeroed();
    let dc = BeginPaint(h, &mut ps);
    let mut client: RECT = zeroed();
    GetClientRect(h, &mut client);
    FillRect(dc, &client, (COLOR_BTNFACE + 1) as HBRUSH);
    PREVIEW.with(|slot| {
        let slot = slot.borrow();
        let Some(s) = slot.as_ref() else {
            return;
        };
        let Some((width, height, pixels)) = &s.frame else {
            return;
        };
        let area = s.area;
        let aw = (area.right - area.left).max(1);
        let ah = (area.bottom - area.top).max(1);
        let scale =
            (f64::from(aw) / f64::from(*width)).min(f64::from(ah) / f64::from(*height)) * s.zoom;
        let dw = (f64::from(*width) * scale).round().max(1.) as i32;
        let dh = (f64::from(*height) * scale).round().max(1.) as i32;
        let px = s.pan.0.clamp(
            -f64::from((dw - aw).max(0)) / 2.,
            f64::from((dw - aw).max(0)) / 2.,
        );
        let py = s.pan.1.clamp(
            -f64::from((dh - ah).max(0)) / 2.,
            f64::from((dh - ah).max(0)) / 2.,
        );
        let saved = SaveDC(dc);
        IntersectClipRect(dc, area.left, area.top, area.right, area.bottom);
        let mut info: BITMAPINFO = zeroed();
        info.bmiHeader.biSize = size_of::<BITMAPINFOHEADER>() as u32;
        info.bmiHeader.biWidth = *width as i32;
        info.bmiHeader.biHeight = -(*height as i32);
        info.bmiHeader.biPlanes = 1;
        info.bmiHeader.biBitCount = 32;
        info.bmiHeader.biCompression = BI_RGB;
        SetStretchBltMode(dc, HALFTONE);
        StretchDIBits(
            dc,
            area.left + (aw - dw) / 2 + px.round() as i32,
            area.top + (ah - dh) / 2 + py.round() as i32,
            dw,
            dh,
            0,
            0,
            *width as i32,
            *height as i32,
            pixels.as_ptr().cast(),
            &info,
            DIB_RGB_COLORS,
            SRCCOPY,
        );
        RestoreDC(dc, saved);
    });
    EndPaint(h, &ps);
}
unsafe extern "system" fn proc(h: HWND, message: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    match message {
        WM_GETMINMAXINFO => {
            let dpi = GetDpiForWindow(h).max(96) as i32;
            let monitor = MonitorFromWindow(h, MONITOR_DEFAULTTONEAREST);
            let mut info: MONITORINFO = zeroed();
            info.cbSize = size_of::<MONITORINFO>() as u32;
            let (w, height) = if !monitor.is_null() && GetMonitorInfoW(monitor, &mut info) != 0 {
                (
                    info.rcWork.right - info.rcWork.left,
                    info.rcWork.bottom - info.rcWork.top,
                )
            } else {
                (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN))
            };
            let limits = &mut *(l as *mut MINMAXINFO);
            limits.ptMinTrackSize.x = (480 * dpi / 96).min(w);
            limits.ptMinTrackSize.y = (360 * dpi / 96).min(height);
            0
        }
        WM_CLOSE => {
            PREVIEW.with(|s| {
                if let Some(s) = s.borrow_mut().as_mut() {
                    s.done = true;
                }
            });
            0
        }
        WM_SIZE => {
            layout(h);
            0
        }
        WM_PAINT => {
            paint(h);
            0
        }
        WM_DPICHANGED => {
            let rect = &*(l as *const RECT);
            SetWindowPos(
                h,
                null_mut(),
                rect.left,
                rect.top,
                rect.right - rect.left,
                rect.bottom - rect.top,
                SWP_NOZORDER | SWP_NOACTIVATE,
            );
            layout(h);
            0
        }
        WM_TIMER => {
            let result = PREVIEW.with(|s| {
                s.borrow()
                    .as_ref()
                    .and_then(|s| s.worker.as_ref())
                    .and_then(|worker| match worker.rx.try_recv() {
                        Ok(v) => Some(v),
                        Err(mpsc::TryRecvError::Disconnected) => {
                            Some(Err("PDF preview worker stopped".into()))
                        }
                        Err(mpsc::TryRecvError::Empty) => None,
                    })
            });
            if let Some(result) = result {
                PREVIEW.with(|s| {
                    if let Some(s) = s.borrow_mut().as_mut() {
                        s.worker = None;
                        match result {
                            Ok((pages, frame)) => {
                                s.pages = pages;
                                s.frame = Some(frame);
                                if s.message == crate::localization::text("Preparing preview…") {
                                    s.message.clear();
                                }
                            }
                            Err(e) => s.message = e,
                        }
                    }
                });
                update_status();
                InvalidateRect(h, null(), 0);
            }
            0
        }
        WM_COMMAND => {
            match w as u16 {
                1 => {
                    PREVIEW.with(|s| s.borrow_mut().as_mut().unwrap().pending = Some(Action::Save))
                }
                2 => PREVIEW.with(|s| s.borrow_mut().as_mut().unwrap().done = true),
                3 => {
                    PREVIEW.with(|s| s.borrow_mut().as_mut().unwrap().pending = Some(Action::Print))
                }
                4 => PREVIEW
                    .with(|s| s.borrow_mut().as_mut().unwrap().pending = Some(Action::Settings)),
                5 | 6 => {
                    if navigate(if w as u16 == 5 { -1 } else { 1 }) {
                        begin_render();
                    }
                }
                10 | 11 => {
                    let changed = PREVIEW.with(|slot| {
                        let mut slot = slot.borrow_mut();
                        let s = slot.as_mut().unwrap();
                        let page = if w as u16 == 10 {
                            0
                        } else {
                            s.pages.saturating_sub(1)
                        };
                        if s.page == page {
                            return false;
                        }
                        s.page = page;
                        s.pan = (0., 0.);
                        true
                    });
                    if changed {
                        begin_render();
                    }
                }
                7 => zoom(h, 1. / 1.5),
                8 => zoom(h, 1.5),
                9 => {
                    PREVIEW.with(|s| {
                        let mut s = s.borrow_mut();
                        let s = s.as_mut().unwrap();
                        s.zoom = 1.;
                        s.pan = (0., 0.);
                        s.message.clear();
                    });
                }
                _ => {}
            }
            update_status();
            InvalidateRect(h, null(), 0);
            0
        }
        WM_KEYDOWN => {
            match w as u32 {
                27 => {
                    SendMessageW(h, WM_CLOSE, 0, 0);
                }
                35 | 36 => {
                    let changed = PREVIEW.with(|slot| {
                        let mut slot = slot.borrow_mut();
                        let s = slot.as_mut().unwrap();
                        let page = if w as u32 == 35 {
                            s.pages.saturating_sub(1)
                        } else {
                            0
                        };
                        if s.page == page {
                            return false;
                        }
                        s.page = page;
                        s.pan = (0., 0.);
                        true
                    });
                    if changed {
                        begin_render();
                        update_status();
                        InvalidateRect(h, null(), 0);
                    }
                }
                33 | 34 => {
                    if navigate(if w as u32 == 33 { -1 } else { 1 }) {
                        begin_render();
                        update_status();
                        InvalidateRect(h, null(), 0);
                    }
                }
                _ => {}
            }
            0
        }
        WM_MOUSEWHEEL => {
            let delta = (w >> 16) as u16 as i16;
            if GetKeyState(VK_CONTROL as i32) < 0 {
                zoom(h, if delta > 0 { 1.5 } else { 1. / 1.5 });
            } else {
                let enlarged = PREVIEW.with(|s| s.borrow().as_ref().is_some_and(|s| s.zoom > 1.));
                if enlarged {
                    PREVIEW.with(|slot| {
                        let mut slot = slot.borrow_mut();
                        let state = slot.as_mut().unwrap();
                        state.pan.1 += f64::from(delta) / 120. * 64.;
                        clamp_pan(state);
                    });
                    InvalidateRect(h, null(), 0);
                } else if navigate(if delta > 0 { -1 } else { 1 }) {
                    begin_render();
                    update_status();
                    InvalidateRect(h, null(), 0);
                }
            }
            0
        }
        WM_LBUTTONDOWN => {
            let (x, y) = (
                l as u32 as u16 as i16 as i32,
                (l as u32 >> 16) as u16 as i16 as i32,
            );
            let drag = PREVIEW.with(|s| {
                let mut s = s.borrow_mut();
                let Some(s) = s.as_mut() else {
                    return false;
                };
                if x >= s.area.left && x < s.area.right && y >= s.area.top && y < s.area.bottom {
                    s.drag = Some((x, y, s.pan.0, s.pan.1));
                    true
                } else {
                    false
                }
            });
            if drag {
                SetFocus(h);
                SetCapture(h);
            }
            0
        }
        WM_MOUSEMOVE => {
            let (x, y) = (
                l as u32 as u16 as i16 as i32,
                (l as u32 >> 16) as u16 as i16 as i32,
            );
            let changed = PREVIEW.with(|s| {
                let mut s = s.borrow_mut();
                let Some(s) = s.as_mut() else {
                    return false;
                };
                if let Some((a, b, px, py)) = s.drag {
                    s.pan = (px + f64::from(x - a), py + f64::from(y - b));
                    clamp_pan(s);
                    true
                } else {
                    false
                }
            });
            if changed {
                InvalidateRect(h, null(), 0);
            }
            0
        }
        WM_LBUTTONUP | WM_CAPTURECHANGED | WM_CANCELMODE => {
            PREVIEW.with(|s| {
                if let Some(s) = s.borrow_mut().as_mut() {
                    s.drag = None;
                }
            });
            if message == WM_LBUTTONUP {
                ReleaseCapture();
            }
            0
        }
        _ => DefWindowProcW(h, message, w, l),
    }
}
pub(super) unsafe fn show<F>(owner: HWND, bytes: Arc<[u8]>, mut action: F) -> Result<(), String>
where
    F: FnMut(Action, HWND) -> Result<Update, String>,
{
    if PREVIEW.with(|s| s.borrow().is_some()) {
        return Err("A label preview is already open".into());
    }
    let instance = GetModuleHandleW(null());
    let class = wide("Off42PdfPreviewWindow");
    let wc = WNDCLASSW {
        lpfnWndProc: Some(proc),
        hInstance: instance,
        hCursor: LoadCursorW(null_mut(), IDC_ARROW),
        hbrBackground: (COLOR_BTNFACE + 1) as HBRUSH,
        lpszClassName: class.as_ptr(),
        ..zeroed()
    };
    RegisterClassW(&wc);
    let dpi = GetDpiForWindow(owner).max(96) as i32;
    let mut work = RECT {
        left: 0,
        top: 0,
        right: GetSystemMetrics(SM_CXSCREEN),
        bottom: GetSystemMetrics(SM_CYSCREEN),
    };
    let monitor = MonitorFromWindow(owner, MONITOR_DEFAULTTONEAREST);
    let mut info: MONITORINFO = zeroed();
    info.cbSize = size_of::<MONITORINFO>() as u32;
    if !monitor.is_null() && GetMonitorInfoW(monitor, &mut info) != 0 {
        work = info.rcWork;
    }
    let width = (860 * dpi / 96).min(work.right - work.left);
    let height = (760 * dpi / 96).min(work.bottom - work.top);
    let h = CreateWindowExW(
        WS_EX_DLGMODALFRAME,
        class.as_ptr(),
        wide(&crate::localization::text("Label Sheet Preview")).as_ptr(),
        WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_THICKFRAME | WS_CLIPCHILDREN,
        work.left + (work.right - work.left - width) / 2,
        work.top + (work.bottom - work.top - height) / 2,
        width,
        height,
        owner,
        null_mut(),
        instance,
        null(),
    );
    if h.is_null() {
        return Err("Cannot create the label preview window".into());
    }
    let mut buttons = Vec::new();
    for (id, title) in [
        (1, "Save PDF…"),
        (3, "Print…"),
        (4, "Settings…"),
        (2, "Close"),
        (5, "Previous Page"),
        (6, "Next Page"),
        (7, "−"),
        (8, "+"),
        (9, "Fit Page"),
    ] {
        let button = CreateWindowExW(
            0,
            wide("BUTTON").as_ptr(),
            wide(&crate::localization::text(title)).as_ptr(),
            WS_CHILD | WS_VISIBLE | WS_TABSTOP,
            0,
            0,
            0,
            0,
            h,
            id as HMENU,
            instance,
            null(),
        );
        buttons.push((id, button));
    }
    let status = CreateWindowExW(
        0,
        wide("STATIC").as_ptr(),
        null(),
        WS_CHILD | WS_VISIBLE,
        0,
        0,
        0,
        0,
        h,
        100 as HMENU,
        instance,
        null(),
    );
    if status.is_null() || buttons.iter().any(|(_, button)| button.is_null()) {
        DestroyWindow(h);
        return Err("Cannot create label preview controls".into());
    }
    let font = GetStockObject(DEFAULT_GUI_FONT) as usize;
    for (_, button) in &buttons {
        SendMessageW(*button, WM_SETFONT, font, 1);
    }
    SendMessageW(status, WM_SETFONT, font, 1);
    PREVIEW.with(|s| {
        *s.borrow_mut() = Some(State {
            bytes,
            buttons,
            status,
            frame: None,
            worker: None,
            pages: 0,
            page: 0,
            zoom: 1.,
            pan: (0., 0.),
            drag: None,
            area: zeroed(),
            pending: None,
            done: false,
            message: String::new(),
        })
    });
    layout(h);
    begin_render();
    update_status();
    EnableWindow(owner, 0);
    ShowWindow(h, SW_SHOW);
    SetFocus(h);
    SetTimer(h, 1, 40, None);
    let shortcuts = [
        (b'S' as u16, 1),
        (b'P' as u16, 3),
        (0xBB, 8),
        (0xBD, 7),
        (b'0' as u16, 9),
        (0x24, 10),
        (0x23, 11),
    ]
    .map(|(key, cmd)| ACCEL {
        fVirt: FVIRTKEY | FCONTROL,
        key,
        cmd,
    });
    let accelerators = CreateAcceleratorTableW(shortcuts.as_ptr(), shortcuts.len() as i32);
    let mut message: MSG = zeroed();
    let mut quit = None;
    while !PREVIEW.with(|s| s.borrow().as_ref().is_none_or(|s| s.done)) {
        let result = GetMessageW(&mut message, null_mut(), 0, 0);
        if result <= 0 {
            if result == 0 {
                quit = Some(message.wParam as i32);
            }
            break;
        }
        if TranslateAcceleratorW(h, accelerators, &message) == 0
            && IsDialogMessageW(h, &message) == 0
        {
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
        let pending = PREVIEW.with(|s| s.borrow_mut().as_mut().and_then(|s| s.pending.take()));
        if let Some(command) = pending {
            match action(command, h) {
                Ok(Update::Keep) => {}
                Ok(Update::Message(message)) => {
                    PREVIEW.with(|s| s.borrow_mut().as_mut().unwrap().message = message)
                }
                Ok(Update::Replace(bytes)) => {
                    PREVIEW.with(|s| {
                        let mut s = s.borrow_mut();
                        let s = s.as_mut().unwrap();
                        s.bytes = bytes;
                        s.page = 0;
                        s.pages = 0;
                        s.zoom = 1.;
                        s.pan = (0., 0.);
                    });
                    begin_render();
                }
                Err(error) => PREVIEW.with(|s| s.borrow_mut().as_mut().unwrap().message = error),
            }
            update_status();
            InvalidateRect(h, null(), 0);
        }
    }
    DestroyAcceleratorTable(accelerators);
    KillTimer(h, 1);
    PREVIEW.with(|s| *s.borrow_mut() = None);
    EnableWindow(owner, 1);
    DestroyWindow(h);
    SetForegroundWindow(owner);
    if let Some(code) = quit {
        PostQuitMessage(code);
    }
    Ok(())
}
