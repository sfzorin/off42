//! Native report list with live navigation and explicit keep/clear actions.
use crate::{wide, ReportEvent, ReportRequest};
use std::{
    cell::RefCell,
    mem::zeroed,
    ptr::{null, null_mut},
};
use windows_sys::Win32::{
    Foundation::*,
    Graphics::Gdi::*,
    System::LibraryLoader::GetModuleHandleW,
    UI::{
        Controls::*,
        HiDpi::GetDpiForWindow,
        Input::KeyboardAndMouse::{EnableWindow, SetFocus},
        WindowsAndMessaging::*,
    },
};
struct State {
    owner: HWND,
    id: u16,
    table: HWND,
    summary: HWND,
    keep: HWND,
    clear: HWND,
    close: HWND,
    details: HWND,
    request: ReportRequest,
    done: bool,
    keeping: bool,
}
thread_local! {static REPORT:RefCell<Option<State>>=const{RefCell::new(None)};}
pub fn is_open() -> bool {
    REPORT.with(|s| s.borrow().is_some())
}
unsafe fn event(e: ReportEvent) -> bool {
    let (owner, id) = REPORT.with(|s| {
        let s = s.borrow();
        let s = s.as_ref().unwrap();
        (s.owner, s.id)
    });
    crate::native::report_event(owner, id, e)
}
unsafe fn layout(h: HWND) {
    let mut rect: RECT = zeroed();
    GetClientRect(h, &mut rect);
    let scale = GetDpiForWindow(h).max(96) as f64 / 96.0;
    let unit = |n: i32| (n as f64 * scale).round() as i32;
    REPORT.with(|s| {
        let s = s.borrow();
        let s = s.as_ref().unwrap();
        let (w, height) = (rect.right, rect.bottom);
        let pad = unit(12);
        let bottom = height - unit(42);
        let details_y = height - unit(120);
        MoveWindow(s.summary, pad, pad, w - 2 * pad, unit(38), 1);
        MoveWindow(
            s.table,
            pad,
            unit(54),
            w - 2 * pad,
            (details_y - unit(60)).max(unit(50)),
            1,
        );
        MoveWindow(s.details, pad, details_y, w - 2 * pad, unit(68), 1);
        MoveWindow(
            s.keep,
            pad,
            bottom,
            (w - unit(270)).max(unit(120)),
            unit(28),
            1,
        );
        MoveWindow(s.clear, w - unit(250), bottom, unit(130), unit(28), 1);
        MoveWindow(s.close, w - unit(108), bottom, unit(96), unit(28), 1);
        let available = (w - 2 * pad - unit(24)).max(unit(250));
        let columns = s.request.columns.len().max(1);
        for i in 0..columns {
            let width = if i == 0 && columns > 1 && s.request.compact_first_column {
                unit(60)
            } else {
                (available
                    - if columns > 1 && s.request.compact_first_column {
                        unit(60)
                    } else {
                        0
                    })
                    / (columns
                        - if columns > 1 && s.request.compact_first_column {
                            1
                        } else {
                            0
                        }) as i32
            };
            SendMessageW(s.table, LVM_SETCOLUMNWIDTH, i, (width.max(1)) as isize);
        }
    });
}
unsafe extern "system" fn proc(h: HWND, m: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    match m {
        WM_SIZE => {
            if is_open() {
                layout(h);
            }
            0
        }
        WM_GETMINMAXINFO => {
            let info = &mut *(l as *mut MINMAXINFO);
            let dpi = GetDpiForWindow(h).max(96) as i32;
            info.ptMinTrackSize = POINT {
                x: 480 * dpi / 96,
                y: 320 * dpi / 96,
            };
            0
        }
        WM_COMMAND if w & 0xffff == 2 => {
            if is_open() {
                REPORT.with(|s| s.borrow_mut().as_mut().unwrap().done = true);
            }
            0
        }
        WM_COMMAND if w & 0xffff == 10 => {
            let keep = REPORT.with(|s| s.borrow().as_ref().unwrap().keep);
            let value = SendMessageW(keep, BM_GETCHECK, 0, 0) == BST_CHECKED as isize;
            if event(ReportEvent::Keep(value)) {
                REPORT.with(|s| s.borrow_mut().as_mut().unwrap().keeping = value);
            } else {
                SendMessageW(
                    keep,
                    BM_SETCHECK,
                    if value { BST_UNCHECKED } else { BST_CHECKED } as usize,
                    0,
                );
            }
            0
        }
        WM_COMMAND if w & 0xffff == 11 => {
            if event(ReportEvent::Clear) {
                REPORT.with(|s| {
                    let mut s = s.borrow_mut();
                    let s = s.as_mut().unwrap();
                    s.keeping = false;
                    SendMessageW(s.keep, BM_SETCHECK, BST_UNCHECKED as usize, 0);
                    EnableWindow(s.keep, 0);
                    EnableWindow(s.clear, 0);
                });
            }
            0
        }
        WM_NOTIFY if l != 0 && is_open() => {
            let hdr = &*(l as *const NMHDR);
            if hdr.idFrom == 100 && hdr.code == NM_CUSTOMDRAW {
                let draw = &mut *(l as *mut NMLVCUSTOMDRAW);
                match draw.nmcd.dwDrawStage {
                    CDDS_PREPAINT => return CDRF_NOTIFYITEMDRAW as isize,
                    CDDS_ITEMPREPAINT => return CDRF_NOTIFYSUBITEMDRAW as isize,
                    stage if stage == CDDS_ITEMPREPAINT | CDDS_SUBITEM => {
                        if let Some(color) = REPORT.with(|s| {
                            s.borrow().as_ref().and_then(|s| {
                                s.request
                                    .column_colors
                                    .get(draw.iSubItem as usize)
                                    .copied()
                                    .flatten()
                            })
                        }) {
                            draw.clrText = color[0] as u32
                                | ((color[1] as u32) << 8)
                                | ((color[2] as u32) << 16);
                        }
                        return CDRF_DODEFAULT as isize;
                    }
                    _ => {}
                }
            }
            if hdr.idFrom == 100 && hdr.code == LVN_ITEMCHANGED {
                let changed = &*(l as *const NMLISTVIEW);
                if changed.iItem >= 0
                    && changed.uNewState & LVIS_SELECTED != 0
                    && changed.uOldState & LVIS_SELECTED == 0
                {
                    let row = changed.iItem as usize;
                    let details = REPORT.with(|s| {
                        let s = s.borrow();
                        let s = s.as_ref().unwrap();
                        let text = s
                            .request
                            .rows
                            .get(row)
                            .map(|r| {
                                r.iter()
                                    .enumerate()
                                    .map(|(i, value)| {
                                        format!(
                                            "{}: {}",
                                            crate::localization::text(
                                                s.request.columns.get(i).copied().unwrap_or("Text")
                                            ),
                                            value
                                        )
                                    })
                                    .collect::<Vec<_>>()
                                    .join("\r\n")
                            })
                            .unwrap_or_default();
                        (s.details, text)
                    });
                    SetWindowTextW(details.0, wide(&details.1).as_ptr());
                    event(ReportEvent::Select(row));
                }
            }
            0
        }
        WM_CLOSE => {
            if is_open() {
                REPORT.with(|s| s.borrow_mut().as_mut().unwrap().done = true);
            }
            0
        }
        _ => DefWindowProcW(h, m, w, l),
    }
}
pub unsafe fn show(owner: HWND, id: u16, request: ReportRequest) {
    if is_open()
        || request.columns.is_empty()
        || request.columns.len() > 8
        || request.rows.len() > 10_000
    {
        return;
    }
    let instance = GetModuleHandleW(null());
    let class = wide("Off42ReportWindow");
    let wc = WNDCLASSW {
        lpfnWndProc: Some(proc),
        hInstance: instance,
        hCursor: LoadCursorW(null_mut(), IDC_ARROW),
        hbrBackground: (COLOR_WINDOW + 1) as HBRUSH,
        lpszClassName: class.as_ptr(),
        ..zeroed()
    };
    RegisterClassW(&wc);
    let mut owner_rect: RECT = zeroed();
    GetWindowRect(owner, &mut owner_rect);
    let dpi = GetDpiForWindow(owner).max(96) as i32;
    let (mut width, mut height) = (900 * dpi / 96, 560 * dpi / 96);
    let mut work = RECT {
        left: 0,
        top: 0,
        right: GetSystemMetrics(SM_CXSCREEN),
        bottom: GetSystemMetrics(SM_CYSCREEN),
    };
    let monitor = MonitorFromWindow(owner, MONITOR_DEFAULTTONEAREST);
    let mut info: MONITORINFO = zeroed();
    info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
    if !monitor.is_null() && GetMonitorInfoW(monitor, &mut info) != 0 {
        work = info.rcWork;
    }
    width = width.min((work.right - work.left).max(1));
    height = height.min((work.bottom - work.top).max(1));
    let left = (owner_rect.left + 40).clamp(work.left, work.right - width);
    let top = (owner_rect.top + 40).clamp(work.top, work.bottom - height);
    let h = CreateWindowExW(
        WS_EX_DLGMODALFRAME,
        class.as_ptr(),
        wide(&request.title).as_ptr(),
        WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_THICKFRAME,
        left,
        top,
        width,
        height,
        owner,
        null_mut(),
        instance,
        null(),
    );
    if h.is_null() {
        return;
    }
    let child = |class: &str, title: &str, style: u32, id: usize| {
        CreateWindowExW(
            0,
            wide(class).as_ptr(),
            wide(title).as_ptr(),
            WS_CHILD | WS_VISIBLE | style,
            0,
            0,
            0,
            0,
            h,
            id as HMENU,
            instance,
            null(),
        )
    };
    let summary = child("STATIC", &request.summary, 0, 101);
    let table = child(
        "SysListView32",
        "",
        LVS_REPORT | LVS_SINGLESEL | WS_TABSTOP | WS_BORDER,
        100,
    );
    let details = child(
        "EDIT",
        "",
        ES_MULTILINE as u32 | ES_READONLY as u32 | WS_VSCROLL | WS_BORDER,
        102,
    );
    let keep = child(
        "BUTTON",
        &crate::localization::text("Keep highlights when saving"),
        BS_AUTOCHECKBOX as u32 | WS_TABSTOP,
        10,
    );
    let clear = child(
        "BUTTON",
        &crate::localization::text("Remove Highlights"),
        BS_PUSHBUTTON as u32 | WS_TABSTOP,
        11,
    );
    let close = child(
        "BUTTON",
        &crate::localization::text("Close"),
        BS_DEFPUSHBUTTON as u32 | WS_TABSTOP,
        2,
    );
    let font = GetStockObject(DEFAULT_GUI_FONT) as usize;
    for control in [summary, table, details, keep, clear, close] {
        SendMessageW(control, WM_SETFONT, font, 1);
    }
    SendMessageW(
        table,
        LVM_SETEXTENDEDLISTVIEWSTYLE,
        0,
        (LVS_EX_FULLROWSELECT | LVS_EX_DOUBLEBUFFER | LVS_EX_GRIDLINES) as isize,
    );
    for (i, title) in request.columns.iter().enumerate() {
        let mut name = wide(&crate::localization::text(title));
        let column = LVCOLUMNW {
            mask: LVCF_TEXT | LVCF_WIDTH,
            pszText: name.as_mut_ptr(),
            cx: 160,
            ..zeroed()
        };
        SendMessageW(table, LVM_INSERTCOLUMNW, i, &column as *const _ as isize);
    }
    if !request.highlight_controls {
        ShowWindow(keep, SW_HIDE);
        ShowWindow(clear, SW_HIDE);
    }
    let keeping = request.keeping;
    let allow_keep = request.allow_keep;
    REPORT.with(|s| {
        *s.borrow_mut() = Some(State {
            owner,
            id,
            table,
            summary,
            keep,
            clear,
            close,
            details,
            request,
            done: false,
            keeping,
        })
    });
    REPORT.with(|s| {
        let s = s.borrow();
        let s = s.as_ref().unwrap();
        for (i, row) in s.request.rows.iter().enumerate() {
            for (j, text) in row.iter().enumerate().take(s.request.columns.len()) {
                let mut value = wide(text);
                let item = LVITEMW {
                    mask: LVIF_TEXT,
                    iItem: i as i32,
                    iSubItem: j as i32,
                    pszText: value.as_mut_ptr(),
                    ..zeroed()
                };
                SendMessageW(
                    table,
                    if j == 0 {
                        LVM_INSERTITEMW
                    } else {
                        LVM_SETITEMW
                    },
                    0,
                    &item as *const _ as isize,
                );
            }
        }
    });
    SendMessageW(
        keep,
        BM_SETCHECK,
        if keeping { BST_CHECKED } else { BST_UNCHECKED } as usize,
        0,
    );
    EnableWindow(keep, allow_keep as i32);
    EnableWindow(clear, allow_keep as i32);
    layout(h);
    EnableWindow(owner, 0);
    ShowWindow(h, SW_SHOW);
    SetFocus(table);
    let mut message: MSG = zeroed();
    let mut quit = None;
    while !REPORT.with(|s| s.borrow().as_ref().is_none_or(|s| s.done)) {
        let result = GetMessageW(&mut message, null_mut(), 0, 0);
        if result <= 0 {
            if result == 0 {
                quit = Some(message.wParam as i32);
            }
            break;
        }
        if IsDialogMessageW(h, &message) == 0 {
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
    let keep = REPORT.with(|s| s.borrow().as_ref().is_some_and(|s| s.keeping));
    event(ReportEvent::Closed { keep });
    EnableWindow(owner, 1);
    DestroyWindow(h);
    REPORT.with(|s| *s.borrow_mut() = None);
    SetForegroundWindow(owner);
    if let Some(code) = quit {
        PostQuitMessage(code);
    }
}
