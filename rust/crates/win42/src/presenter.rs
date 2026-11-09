use super::*;
use windows_sys::Win32::System::SystemServices::SS_OWNERDRAW;

const HEADER: u16 = 65480;
const NEXT_LABEL: u16 = 65481;
const PREVIEW: u16 = 65482;
const NOTES_LABEL: u16 = 65483;
const NOTES: u16 = 65484;
const PREVIOUS: u16 = 65485;
const NEXT: u16 = 65486;
const BLACK: u16 = 65487;
const END: u16 = 65488;

pub(super) struct Controls {
    canvas: HWND,
    header: HWND,
    next_label: HWND,
    preview: HWND,
    notes_label: HWND,
    notes: HWND,
    buttons: [HWND; 4],
    position: Option<usize>,
    elapsed: Option<u64>,
}

impl Controls {
    unsafe fn create(owner: HWND, canvas: HWND) -> Option<Self> {
        let instance = GetModuleHandleW(null());
        let mut created = Vec::new();
        let mut child = |class, label, style, id| {
            let hwnd = CreateWindowExW(
                0,
                wide(class).as_ptr(),
                wide(&crate::localization::text(label)).as_ptr(),
                WS_CHILD | style,
                0,
                0,
                0,
                0,
                owner,
                id as usize as HMENU,
                instance,
                null(),
            );
            if !hwnd.is_null() {
                SendMessageW(
                    hwnd,
                    WM_SETFONT,
                    GetStockObject(DEFAULT_GUI_FONT) as usize,
                    0,
                );
                created.push(hwnd);
            }
            hwnd
        };
        let controls = Self {
            canvas,
            header: child("STATIC", "", 0, HEADER),
            next_label: child("STATIC", "Next", 0, NEXT_LABEL),
            preview: child("STATIC", "", SS_OWNERDRAW, PREVIEW),
            notes_label: child("STATIC", "Speaker Notes", 0, NOTES_LABEL),
            notes: child(
                "EDIT",
                "",
                WS_TABSTOP
                    | WS_BORDER
                    | WS_VSCROLL
                    | ES_MULTILINE as u32
                    | ES_AUTOVSCROLL as u32
                    | ES_READONLY as u32,
                NOTES,
            ),
            buttons: [
                child("BUTTON", "Previous", WS_TABSTOP, PREVIOUS),
                child("BUTTON", "Next", WS_TABSTOP, NEXT),
                child("BUTTON", "Black Screen", WS_TABSTOP, BLACK),
                child("BUTTON", "End Show", WS_TABSTOP, END),
            ],
            position: None,
            elapsed: None,
        };
        if created.len() == 9 {
            SetWindowLongPtrW(
                canvas,
                GWL_STYLE,
                GetWindowLongPtrW(canvas, GWL_STYLE) | WS_TABSTOP as isize,
            );
            Some(controls)
        } else {
            for hwnd in created {
                DestroyWindow(hwnd);
            }
            None
        }
    }

    unsafe fn destroy(self) {
        SetWindowLongPtrW(
            self.canvas,
            GWL_STYLE,
            GetWindowLongPtrW(self.canvas, GWL_STYLE) & !(WS_TABSTOP as isize),
        );
        for hwnd in [
            self.header,
            self.next_label,
            self.preview,
            self.notes_label,
            self.notes,
        ]
        .into_iter()
        .chain(self.buttons)
        {
            DestroyWindow(hwnd);
        }
    }
}

pub(super) unsafe fn hide() {
    let controls = STATE.with(|slot| {
        slot.borrow_mut()
            .as_mut()
            .and_then(|state| state.presenter.take())
    });
    if let Some(controls) = controls {
        controls.destroy();
    }
}

pub(super) unsafe fn layout(owner: HWND, canvas: HWND, client: RECT) -> bool {
    let active = STATE.with(|slot| {
        slot.borrow()
            .as_ref()
            .is_some_and(|state| state.app.presenter_view().is_some())
    });
    if !active {
        hide();
        return false;
    }
    let mut created = false;
    if STATE.with(|slot| slot.borrow().as_ref().unwrap().presenter.is_none()) {
        let Some(controls) = Controls::create(owner, canvas) else {
            return false;
        };
        STATE.with(|slot| slot.borrow_mut().as_mut().unwrap().presenter = Some(controls));
        created = true;
    }
    let dpi = GetDpiForWindow(owner).max(96) as i32;
    let scaled = |px| px * dpi / 96;
    let gap = scaled(12);
    let line = scaled(24);
    let button_height = scaled(32);
    let left_width = (client.right * 3 / 5 - 2 * gap).max(1);
    let right_x = left_width + 2 * gap;
    let right_width = (client.right - right_x - gap).max(1);
    let top = gap + line;
    let bottom = (client.bottom - button_height - 2 * gap).max(top + 1);
    let preview_height = (right_width * 9 / 16).min((bottom - top) / 2).max(1);
    let notes_y = top + line + preview_height + line;
    let place = |hwnd, x, y, width, height| {
        SetWindowPos(
            hwnd,
            null_mut(),
            x,
            y,
            width,
            height,
            SWP_NOZORDER | SWP_NOACTIVATE | SWP_SHOWWINDOW,
        );
    };
    let (header, next_label, preview, notes_label, notes, buttons) = STATE.with(|slot| {
        let slot = slot.borrow();
        let controls = slot.as_ref().unwrap().presenter.as_ref().unwrap();
        (
            controls.header,
            controls.next_label,
            controls.preview,
            controls.notes_label,
            controls.notes,
            controls.buttons,
        )
    });
    place(header, gap, gap, (client.right - 2 * gap).max(1), line);
    place(canvas, gap, top, left_width, bottom - top);
    place(next_label, right_x, top, right_width, line);
    place(preview, right_x, top + line, right_width, preview_height);
    place(notes_label, right_x, notes_y - line, right_width, line);
    place(
        notes,
        right_x,
        notes_y,
        right_width,
        (bottom - notes_y).max(1),
    );
    let step = ((client.right - 2 * gap) / 4).clamp(1, scaled(130));
    for (index, hwnd) in buttons.into_iter().enumerate() {
        place(
            hwnd,
            gap + index as i32 * step,
            bottom + gap,
            (step - scaled(4)).max(1),
            button_height,
        );
    }
    sync();
    if created {
        SetFocus(canvas);
    }
    true
}

pub(super) unsafe fn sync() {
    let (header, slide) = STATE
        .with(|slot| {
            let mut slot = slot.borrow_mut();
            let state = slot.as_mut()?;
            let controls = state.presenter.as_mut()?;
            let view = state.app.presenter_view()?;
            let changed = controls.position != Some(view.position);
            let header = if changed || controls.elapsed != Some(view.elapsed_seconds) {
                let time = view.elapsed_seconds;
                let minutes = time / 60;
                let position = crate::localization::text("Slide %d of %d")
                    .replacen("%d", &view.position.to_string(), 1)
                    .replacen("%d", &view.total.to_string(), 1);
                controls.elapsed = Some(time);
                Some((
                    controls.header,
                    format!("{minutes:02}:{:02}   {position}", time % 60),
                ))
            } else {
                None
            };
            let slide = changed.then(|| {
                controls.position = Some(view.position);
                (
                    controls.notes,
                    controls.next_label,
                    controls.preview,
                    view.next.is_some(),
                    if view.notes.is_empty() {
                        crate::localization::text("No notes")
                    } else {
                        crate::to_native_text(view.notes)
                    },
                )
            });
            Some((header, slide))
        })
        .unwrap_or_default();
    if let Some((hwnd, caption)) = header {
        SetWindowTextW(hwnd, wide(&caption).as_ptr());
    }
    if let Some((notes, next, preview, has_next, value)) = slide {
        SetWindowTextW(notes, wide(&value).as_ptr());
        SendMessageW(notes, EM_SETSEL, 0, 0);
        SendMessageW(notes, EM_SCROLLCARET, 0, 0);
        let title = crate::localization::text(if has_next { "Next" } else { "End of show" });
        SetWindowTextW(next, wide(&title).as_ptr());
        InvalidateRect(preview, null(), 0);
    }
}

pub(super) unsafe fn draw(item: &DRAWITEMSTRUCT) -> bool {
    if item.CtlID != u32::from(PREVIEW) {
        return false;
    }
    let scene = STATE.with(|slot| {
        let slot = slot.borrow();
        let app = &slot.as_ref()?.app;
        let next = app.presenter_view()?.next?;
        Some((app.presenter_preview(next)?, app.presentation_size()))
    });
    FillRect(
        item.hDC,
        &item.rcItem,
        GetStockObject(BLACK_BRUSH) as HBRUSH,
    );
    if let Some((scene, size)) = scene {
        let state = SaveDC(item.hDC);
        if state != 0 {
            draw_scene(item.hDC, &scene_for_canvas(item.hwndItem, scene, size));
            RestoreDC(item.hDC, state);
        }
    }
    true
}

pub(super) unsafe fn command(id: u16, notification: u16) -> bool {
    if !(HEADER..=END).contains(&id) {
        return false;
    }
    let key = match (id, notification) {
        (PREVIOUS, 0) => 37,
        (NEXT, 0) => 39,
        (BLACK, 0) => 66,
        (END, 0) => 81,
        _ => return true,
    };
    let canvas = STATE.with(|slot| {
        let slot = slot.borrow();
        let state = slot.as_ref()?;
        state.presenter.as_ref().map(|_| state.canvas)
    });
    if let Some(canvas) = canvas {
        SetFocus(canvas);
        PostMessageW(canvas, WM_KEYDOWN, key, 0);
    }
    true
}
