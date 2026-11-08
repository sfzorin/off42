use crate::{wide, InputRequest};
use std::{
    cell::RefCell,
    mem::zeroed,
    ptr::{null, null_mut},
};
use windows_sys::Win32::{
    Foundation::*,
    Graphics::Gdi::*,
    System::{
        LibraryLoader::GetModuleHandleW,
        SystemServices::{SS_NOPREFIX, SS_RIGHT},
    },
    UI::{Controls::*, HiDpi::*, Input::KeyboardAndMouse::*, WindowsAndMessaging::*},
};
struct Prompt {
    fields: Vec<HWND>,
    choices: Vec<(usize, Vec<String>)>,
    result: Option<Vec<String>>,
    done: bool,
}
thread_local! { static PROMPT: RefCell<Option<Prompt>> = const { RefCell::new(None) }; }
unsafe extern "system" fn proc(h: HWND, m: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    match m {
        WM_COMMAND if (w & 0xffff) as i32 == IDOK => {
            let (fields, choices) = PROMPT.with(|p| {
                let p = p.borrow();
                let p = p.as_ref().unwrap();
                (p.fields.clone(), p.choices.clone())
            });
            let mut values = Vec::new();
            for (index, field) in fields.into_iter().enumerate() {
                let n = GetWindowTextLengthW(field) as usize;
                let mut buf = vec![0u16; n + 1];
                GetWindowTextW(field, buf.as_mut_ptr(), (n + 1) as i32);
                let decoded = String::from_utf16(&buf[..n]);
                buf.fill(0);
                match decoded {
                    Ok(s) => {
                        let value = if let Some((_, options)) =
                            choices.iter().find(|(field, _)| *field == index)
                        {
                            let selected = SendMessageW(field, CB_GETCURSEL, 0, 0);
                            options
                                .get(selected as usize)
                                .filter(|option| crate::localization::text(option) == s)
                                .cloned()
                                .unwrap_or_else(|| crate::localization::choice_value(options, &s))
                        } else {
                            s
                        };
                        values.push(value);
                    }
                    Err(_) => {
                        MessageBoxW(
                            h,
                            wide(&crate::localization::text("Invalid text")).as_ptr(),
                            wide("off42").as_ptr(),
                            MB_OK,
                        );
                        return 0;
                    }
                }
            }
            PROMPT.with(|p| {
                let mut p = p.borrow_mut();
                let p = p.as_mut().unwrap();
                p.result = Some(values);
                p.done = true;
            });
            0
        }
        WM_COMMAND if (w & 0xffff) as i32 == IDCANCEL => {
            PROMPT.with(|p| p.borrow_mut().as_mut().unwrap().done = true);
            0
        }
        WM_CLOSE => {
            PROMPT.with(|p| p.borrow_mut().as_mut().unwrap().done = true);
            0
        }
        _ => DefWindowProcW(h, m, w, l),
    }
}
pub unsafe fn show(owner: HWND, req: &InputRequest) -> Option<Vec<String>> {
    show_options(owner, req, false, false)
}
pub unsafe fn show_options(
    owner: HWND,
    req: &InputRequest,
    multiline: bool,
    password: bool,
) -> Option<Vec<String>> {
    show_choices(owner, req, multiline, password, &[])
}
pub unsafe fn show_choices(
    owner: HWND,
    req: &InputRequest,
    multiline: bool,
    password: bool,
    choices: &[(usize, Vec<String>)],
) -> Option<Vec<String>> {
    show_choices_for_fields(owner,req,if multiline { &[0] } else { &[] },password,choices)
}
pub unsafe fn show_choices_for_fields(
    owner:HWND, req:&InputRequest, multiline:&[usize], password:bool, choices:&[(usize,Vec<String>)]
) -> Option<Vec<String>> {
    if PROMPT.with(|p| p.borrow().is_some()) {
        return None;
    }
    let previous_focus = GetFocus();
    let instance = GetModuleHandleW(null());
    let class = wide("Suite42InputDialog");
    let wc = WNDCLASSW {
        lpfnWndProc: Some(proc),
        hInstance: instance,
        hCursor: LoadCursorW(null_mut(), IDC_ARROW),
        hbrBackground: (COLOR_WINDOW + 1) as HBRUSH,
        lpszClassName: class.as_ptr(),
        ..zeroed()
    };
    RegisterClassW(&wc);
    let dpi = GetDpiForWindow(owner).max(96) as i32;
    let scale = |x: i32| x * dpi / 96;
    let mut work = RECT {
        left: 0,
        top: 0,
        right: GetSystemMetrics(SM_CXSCREEN),
        bottom: GetSystemMetrics(SM_CYSCREEN),
    };
    let mut info: MONITORINFO = zeroed();
    info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
    let monitor = MonitorFromWindow(owner, MONITOR_DEFAULTTONEAREST);
    if !monitor.is_null() && GetMonitorInfoW(monitor, &mut info) != 0 {
        work = info.rcWork;
    }
    let font = GetStockObject(DEFAULT_GUI_FONT) as usize;
    let dc = GetDC(owner);
    let previous_font = SelectObject(dc, font as HGDIOBJ);
    // Long forms use columns instead of extending the final fields/buttons offscreen.
    let mut columns = 1usize;
    let (positions, button_y, width, height, field_width) = loop {
        let width = scale(448 * columns as i32 + 12).min(work.right - work.left);
        let field_width = ((width - scale(36)) / columns as i32 - scale(12)).max(scale(120));
        let per_column = req.fields.len().div_ceil(columns).max(1);
        let mut positions = Vec::new();
        let mut column_y = vec![scale(12); columns];
        for (index, (label, _)) in req.fields.iter().enumerate() {
            let column = (index / per_column).min(columns - 1);
            let mut rect = RECT {
                left: 0,
                top: 0,
                right: field_width,
                bottom: 0,
            };
            let label = wide(&crate::localization::text(label));
            DrawTextW(
                dc,
                label.as_ptr(),
                -1,
                &mut rect,
                DT_CALCRECT | DT_WORDBREAK | DT_NOPREFIX,
            );
            let label_height = (rect.bottom - rect.top).max(scale(20));
            let extra = if multiline.contains(&index) {
                scale(100)
            } else {
                0
            };
            positions.push((
                scale(12) + column as i32 * (field_width + scale(12)),
                column_y[column],
                label_height,
                extra,
            ));
            column_y[column] += label_height + scale(38) + extra;
        }
        let button_y = column_y.into_iter().max().unwrap_or(scale(12)) + scale(6);
        let height = button_y + scale(68);
        if height <= work.bottom - work.top
            || columns >= 3
            || scale(300 * (columns + 1) as i32) > work.right - work.left
        {
            break (positions, button_y, width, height, field_width);
        }
        columns += 1;
    };
    SelectObject(dc, previous_font);
    ReleaseDC(owner, dc);
    let mut owner_rect: RECT = zeroed();
    GetWindowRect(owner, &mut owner_rect);
    let left = (owner_rect.left + (owner_rect.right - owner_rect.left - width) / 2)
        .clamp(work.left, (work.right - width).max(work.left));
    let top = (owner_rect.top + (owner_rect.bottom - owner_rect.top - height) / 2)
        .clamp(work.top, (work.bottom - height).max(work.top));
    let h = CreateWindowExW(
        WS_EX_DLGMODALFRAME
            | if crate::localization::current().rtl {
                WS_EX_RTLREADING
            } else {
                0
            },
        class.as_ptr(),
        wide(&crate::localization::text(req.title)).as_ptr(),
        WS_CAPTION | WS_SYSMENU | WS_POPUP,
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
        return None;
    }
    let mut fields = Vec::new();
    for (i, (label, value)) in req.fields.iter().enumerate() {
        let (x, y, label_height, extra) = positions[i];
        let t = CreateWindowExW(
            if crate::localization::current().rtl {
                WS_EX_RTLREADING
            } else {
                0
            },
            wide("STATIC").as_ptr(),
            wide(&crate::localization::text(label)).as_ptr(),
            WS_CHILD
                | WS_VISIBLE
                | SS_NOPREFIX
                | if crate::localization::current().rtl {
                    SS_RIGHT
                } else {
                    0
                },
            x,
            y,
            field_width,
            label_height,
            h,
            null_mut(),
            instance,
            null(),
        );
        let options = choices
            .iter()
            .find(|(field, _)| *field == i)
            .map(|(_, values)| values);
        let combo = options.is_some();
        let e = CreateWindowExW(
            WS_EX_CLIENTEDGE,
            wide(if combo { "COMBOBOX" } else { "EDIT" }).as_ptr(),
            wide(value).as_ptr(),
            WS_CHILD
                | WS_VISIBLE
                | WS_TABSTOP
                | if combo {
                    WS_VSCROLL | CBS_DROPDOWN as u32 | CBS_AUTOHSCROLL as u32
                } else if multiline.contains(&i) {
                    WS_VSCROLL | ES_MULTILINE as u32 | ES_AUTOVSCROLL as u32 | ES_WANTRETURN as u32
                } else {
                    ES_AUTOHSCROLL as u32
                        | if password && i == 0 {
                            ES_PASSWORD as u32
                        } else {
                            0
                        }
                },
            x,
            y + label_height + scale(1),
            field_width,
            if combo { scale(220) } else { scale(24) + extra },
            h,
            (100 + i) as HMENU,
            instance,
            null(),
        );
        if let Some(options) = options {
            for option in options {
                SendMessageW(
                    e,
                    CB_ADDSTRING,
                    0,
                    wide(&crate::localization::text(option)).as_ptr() as LPARAM,
                );
            }
            if let Some(selected) = options.iter().position(|option| option == value) {
                SendMessageW(e, CB_SETCURSEL, selected, 0);
            } else {
                SetWindowTextW(e, wide(value).as_ptr());
            }
        }
        SendMessageW(t, WM_SETFONT, font, 1);
        SendMessageW(e, WM_SETFONT, font, 1);
        SendMessageW(
            e,
            if combo { CB_LIMITTEXT } else { EM_SETLIMITTEXT },
            if multiline.contains(&i) { 65536 } else { 4096 },
            0,
        );
        fields.push(e);
    }
    let y = button_y;
    for (id, title, x, style) in [
        (IDOK, "OK", width - scale(200), BS_DEFPUSHBUTTON),
        (IDCANCEL, "Cancel", width - scale(110), BS_PUSHBUTTON),
    ] {
        let b = CreateWindowExW(
            0,
            wide("BUTTON").as_ptr(),
            wide(&crate::localization::text(title)).as_ptr(),
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | style as u32,
            x,
            y,
            scale(80),
            scale(26),
            h,
            id as HMENU,
            instance,
            null(),
        );
        SendMessageW(b, WM_SETFONT, font, 1);
    }
    PROMPT.with(|p| {
        *p.borrow_mut() = Some(Prompt {
            fields: fields.clone(),
            choices: choices.to_vec(),
            result: None,
            done: false,
        })
    });
    EnableWindow(owner, 0);
    ShowWindow(h, SW_SHOW);
    if let Some(&field) = fields.first() {
        SetFocus(field);
        SendMessageW(field, EM_SETSEL, 0, -1);
    }
    let mut m: MSG = zeroed();
    while !PROMPT.with(|p| p.borrow().as_ref().unwrap().done) {
        let result = GetMessageW(&mut m, null_mut(), 0, 0);
        if result <= 0 {
            if result == 0 {
                PostQuitMessage(m.wParam as i32);
            }
            break;
        }
        if !multiline.is_empty()
            && m.message == WM_KEYDOWN
            && m.wParam == VK_RETURN as usize
            && multiline.iter().any(|i|fields.get(*i).is_some_and(|f|*f==GetFocus()))
            && GetKeyState(VK_CONTROL as i32) >= 0
        {
            TranslateMessage(&m);
            DispatchMessageW(&m);
            continue;
        }
        if m.message == WM_KEYDOWN
            && (m.wParam == VK_RETURN as usize || m.wParam == VK_ESCAPE as usize)
        {
            SendMessageW(
                h,
                WM_COMMAND,
                if m.wParam == VK_RETURN as usize {
                    IDOK as usize
                } else {
                    IDCANCEL as usize
                },
                0,
            );
            continue;
        }
        if IsDialogMessageW(h, &m) == 0 {
            TranslateMessage(&m);
            DispatchMessageW(&m);
        }
    }
    EnableWindow(owner, 1);
    DestroyWindow(h);
    SetActiveWindow(owner);
    if !previous_focus.is_null()
        && IsWindow(previous_focus) != 0
        && (previous_focus == owner || IsChild(owner, previous_focus) != 0)
    {
        SetFocus(previous_focus);
    }
    PROMPT.with(|p| p.borrow_mut().take().and_then(|p| p.result))
}
