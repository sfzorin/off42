use crate::{wide, InputRequest};
use std::{
    cell::RefCell,
    mem::zeroed,
    ptr::{null, null_mut},
};
use windows_sys::Win32::{
    Foundation::*,
    Graphics::Gdi::*,
    System::LibraryLoader::GetModuleHandleW,
    UI::{Controls::*, HiDpi::*, Input::KeyboardAndMouse::*, WindowsAndMessaging::*},
};
struct Prompt {
    fields: Vec<HWND>,
    result: Option<Vec<String>>,
    done: bool,
}
thread_local! { static PROMPT: RefCell<Option<Prompt>> = const { RefCell::new(None) }; }
unsafe extern "system" fn proc(h: HWND, m: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    match m {
        WM_COMMAND if (w & 0xffff) as i32 == IDOK => {
            let fields = PROMPT.with(|p| p.borrow().as_ref().unwrap().fields.clone());
            let mut values = Vec::new();
            for field in fields {
                let n = GetWindowTextLengthW(field) as usize;
                let mut buf = vec![0u16; n + 1];
                GetWindowTextW(field, buf.as_mut_ptr(), (n + 1) as i32);
                let decoded = String::from_utf16(&buf[..n]);
                buf.fill(0);
                match decoded {
                    Ok(s) => values.push(s),
                    Err(_) => {
                        MessageBoxW(
                            h,
                            wide("Invalid text").as_ptr(),
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
    let dpi = GetDpiForWindow(owner) as i32;
    let scale = |x: i32| x * dpi / 96;
    let width = scale(400);
    let extra = if multiline && !req.fields.is_empty() {
        100
    } else {
        0
    };
    let height = scale(110 + req.fields.len() as i32 * 58 + extra);
    let h = CreateWindowExW(
        WS_EX_DLGMODALFRAME,
        class.as_ptr(),
        wide(req.title).as_ptr(),
        WS_CAPTION | WS_SYSMENU | WS_POPUP,
        CW_USEDEFAULT,
        CW_USEDEFAULT,
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
    let font = GetStockObject(DEFAULT_GUI_FONT) as usize;
    let mut fields = Vec::new();
    for (i, (label, value)) in req.fields.iter().enumerate() {
        let y = scale(12 + i as i32 * 58 + if i > 0 { extra } else { 0 });
        let t = CreateWindowExW(
            0,
            wide("STATIC").as_ptr(),
            wide(label).as_ptr(),
            WS_CHILD | WS_VISIBLE,
            scale(12),
            y,
            scale(360),
            scale(20),
            h,
            null_mut(),
            instance,
            null(),
        );
        let e = CreateWindowExW(
            WS_EX_CLIENTEDGE,
            wide("EDIT").as_ptr(),
            wide(value).as_ptr(),
            WS_CHILD
                | WS_VISIBLE
                | WS_TABSTOP
                | if multiline && i == 0 {
                    WS_VSCROLL | ES_MULTILINE as u32 | ES_AUTOVSCROLL as u32 | ES_WANTRETURN as u32
                } else {
                    ES_AUTOHSCROLL as u32
                        | if password && i == 0 {
                            ES_PASSWORD as u32
                        } else {
                            0
                        }
                },
            scale(12),
            y + scale(21),
            scale(360),
            scale(24 + if multiline && i == 0 { extra } else { 0 }),
            h,
            (100 + i) as HMENU,
            instance,
            null(),
        );
        SendMessageW(t, WM_SETFONT, font, 1);
        SendMessageW(e, WM_SETFONT, font, 1);
        SendMessageW(
            e,
            EM_SETLIMITTEXT,
            if multiline && i == 0 { 65536 } else { 4096 },
            0,
        );
        fields.push(e);
    }
    let y = scale(18 + req.fields.len() as i32 * 58 + extra);
    for (id, title, x, style) in [
        (IDOK, "OK", 200, BS_DEFPUSHBUTTON),
        (IDCANCEL, "Cancel", 290, BS_PUSHBUTTON),
    ] {
        let b = CreateWindowExW(
            0,
            wide("BUTTON").as_ptr(),
            wide(title).as_ptr(),
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | style as u32,
            scale(x),
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
        if multiline
            && m.message == WM_KEYDOWN
            && m.wParam == VK_RETURN as usize
            && fields.first().is_some_and(|f| *f == GetFocus())
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
