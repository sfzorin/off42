//! Native Print Layout desk over Rich Edit; the page and sheet geometry lives in `suite42_app::rich_pages`.
pub use suite42_app::rich_pages::*;

#[cfg(windows)]
pub use native::*;

#[cfg(windows)]
mod native {
    use super::{PageLayout, Sheets};
    use crate::RichPrint;
    use std::{
        cell::{Cell, RefCell},
        mem::{size_of, zeroed},
        ptr::{null, null_mut},
        time::{Duration, Instant},
    };
    use windows_sys::Win32::{
        Foundation::*,
        Graphics::Gdi::*,
        System::LibraryLoader::GetModuleHandleW,
        UI::{
            Controls::SetScrollInfo,
            HiDpi::GetDpiForWindow,
            Input::KeyboardAndMouse::{GetCapture, SetFocus},
            Shell::DefSubclassProc,
            WindowsAndMessaging::*,
        },
    };

    const EM_SETSEL: u32 = 0x00B1;
    const EM_GETRECT: u32 = 0x00B2;
    const EM_SETRECT: u32 = 0x00B3;
    const EM_LINESCROLL: u32 = 0x00B6;
    const EM_SCROLLCARET: u32 = 0x00B7;
    const EM_REPLACESEL: u32 = 0x00C2;
    const EM_UNDO: u32 = 0x00C7;
    const EM_POSFROMCHAR: u32 = 0x00D6;
    const EM_EXSETSEL: u32 = WM_USER + 55;
    const EM_STREAMIN: u32 = WM_USER + 73;
    const EM_REDO: u32 = WM_USER + 84;
    const EM_GETTEXTLENGTHEX: u32 = WM_USER + 95;
    const EM_SHOWSCROLLBAR: u32 = WM_USER + 96;
    const EM_SETTEXTEX: u32 = WM_USER + 97;
    const EM_GETSCROLLPOS: u32 = WM_USER + 221;
    const EM_SETSCROLLPOS: u32 = WM_USER + 222;
    const SETTLE: Duration = Duration::from_millis(250);
    const DESK: u32 = 0x00E4_E1DE;
    const EDGE: u32 = 0x00B4_B0AC;
    const SHADOW: u32 = 0x00A8_A4A0;
    const MAX_CACHED: usize = 8;

    #[derive(Default)]
    struct Canvas {
        layout: Option<PageLayout>,
        /// Unscrolled page left edge and width in client pixels.
        frame: Option<(i32, i32)>,
        dirty_since: Option<Instant>,
    }

    /// One sheet rendered at the display's logical DPI.
    struct Bitmap {
        index: usize,
        width: i32,
        height: i32,
        pixels: Vec<u8>,
    }

    /// Print Layout state: the desk window and the live editor's place on it.
    struct View {
        requested: bool,
        edit: HWND,
        desk: HWND,
        /// Editor area assigned by the host layout, in parent client coordinates.
        area: Option<RECT>,
        /// The editor currently sits over a sheet (borderless, clipped, no scroll bars).
        overlaid: bool,
        scroll: i32,
        /// Zero-based sheet that holds the live editor.
        page: usize,
        /// Live body page starts drawn as sheets.
        starts: Vec<i32>,
        job: Option<RichPrint>,
        /// Editor scroll position, hidden rows above the sheet body, clip bottom.
        pinned: Option<(POINT, i32, i32)>,
        cache: Vec<Bitmap>,
        last: (usize, i32),
        reformat: bool,
    }

    impl Default for View {
        fn default() -> Self {
            Self {
                requested: false,
                edit: null_mut(),
                desk: null_mut(),
                area: None,
                overlaid: false,
                scroll: 0,
                page: 0,
                starts: Vec::new(),
                job: None,
                pinned: None,
                cache: Vec::new(),
                last: (usize::MAX, -1),
                reformat: false,
            }
        }
    }

    thread_local! {
        static CANVAS: RefCell<Canvas> = RefCell::new(Canvas::default());
        static VIEW: RefCell<View> = RefCell::new(View::default());
        /// Set while this module drives the editor, so its own messages are not followed.
        static BUSY: Cell<bool> = const { Cell::new(false) };
    }

    struct Busy(bool);
    impl Busy {
        fn enter() -> Self {
            Busy(BUSY.with(|busy| busy.replace(true)))
        }
    }
    impl Drop for Busy {
        fn drop(&mut self) {
            BUSY.with(|busy| busy.set(self.0));
        }
    }

    /// The document or page geometry changed; re-measure once edits settle.
    pub fn invalidate() {
        CANVAS.with(|canvas| {
            let mut canvas = canvas.borrow_mut();
            canvas.dirty_since.get_or_insert_with(Instant::now);
        });
    }

    pub fn clear() {
        CANVAS.with(|canvas| *canvas.borrow_mut() = Canvas::default());
        VIEW.with(|view| {
            let mut view = view.borrow_mut();
            let requested = view.requested;
            *view = View { requested, ..View::default() };
        });
    }

    pub fn layout() -> Option<PageLayout> {
        CANVAS.with(|canvas| canvas.borrow().layout.clone())
    }

    /// Record the page frame; `reformatted` reports a zoom or page-format change.
    pub fn set_frame(edit: HWND, left: i32, width: i32, reformatted: bool) {
        let changed = CANVAS.with(|canvas| {
            let mut canvas = canvas.borrow_mut();
            let frame = Some((left, width));
            let changed = canvas.frame != frame;
            canvas.frame = frame;
            changed
        });
        if changed || reformatted {
            if requested() {
                unsafe { place(edit) };
            } else {
                unsafe { InvalidateRect(edit, null(), 0) };
            }
        }
    }

    /// True when a measurement is due; the caller then supplies the current job.
    pub fn due() -> bool {
        CANVAS.with(|canvas| {
            canvas
                .borrow()
                .dirty_since
                .is_some_and(|since| since.elapsed() >= SETTLE)
        })
    }

    /// Whether Print Layout was requested (the View menu check mark).
    pub fn requested() -> bool {
        VIEW.with(|view| view.borrow().requested)
    }

    /// Whether the editor currently sits on a sheet of the Print Layout desk.
    pub fn overlaid() -> bool {
        VIEW.with(|view| view.try_borrow().is_ok_and(|view| view.overlaid))
    }

    /// Turn Print Layout on or off; the host lays the window out again afterwards.
    pub unsafe fn set_requested(edit: HWND, on: bool) {
        VIEW.with(|view| {
            let mut view = view.borrow_mut();
            view.requested = on;
            view.edit = edit;
            view.scroll = 0;
        });
        invalidate();
        if !on {
            place(edit);
        }
    }

    /// The host needs to restore its normal editor format rectangle.
    pub fn take_reformat() -> bool {
        VIEW.with(|view| std::mem::take(&mut view.borrow_mut().reformat))
    }

    /// Client size the page frame is centred in while the desk is shown.
    pub unsafe fn view_size() -> Option<(i32, i32)> {
        let desk = VIEW.with(|view| {
            let view = view.borrow();
            (view.overlaid && !view.desk.is_null()).then_some(view.desk)
        })?;
        let mut rect: RECT = zeroed();
        GetClientRect(desk, &mut rect);
        Some((rect.right, rect.bottom))
    }

    /// Vertical ruler origin: top of the sheet holding the editor, in desk pixels.
    pub fn vertical_origin() -> Option<i32> {
        let view = VIEW.with(|view| {
            let view = view.borrow();
            view.overlaid.then(|| (view.page, view.scroll))
        })?;
        let sheets = sheets()?;
        Some(sheets.top(view.0) - view.1)
    }

    /// Place the editor in `area`, or over the current sheet while Print Layout is shown.
    /// Returns false when the host should position the editor normally.
    pub unsafe fn arrange(edit: HWND, area: RECT) -> bool {
        let requested = VIEW.with(|view| {
            let mut view = view.borrow_mut();
            view.area = Some(area);
            view.edit = edit;
            view.requested
        });
        if !requested {
            return false;
        }
        place(edit)
    }

    /// Measure the live editor (page breaks) and the print job (page count).
    pub unsafe fn measure(edit: HWND, job: Option<&RichPrint>) -> bool {
        let layout = {
            let _busy = Busy::enter();
            job.and_then(|job| layout_for(edit, job).ok())
        };
        let changed = CANVAS.with(|canvas| {
            let mut canvas = canvas.borrow_mut();
            canvas.dirty_since = None;
            let changed = canvas.layout != layout;
            canvas.layout = layout.clone();
            changed
        });
        let caret = crate::rich_edit::selection(edit).1;
        let requested = VIEW.with(|view| {
            let mut view = view.borrow_mut();
            view.job = job.cloned();
            view.starts = layout.map(|layout| layout.starts).unwrap_or_default();
            view.cache.clear();
            view.edit = edit;
            if view.requested {
                view.page = super::page_index(&view.starts, caret);
            }
            view.requested
        });
        if requested {
            place(edit);
        } else if changed {
            InvalidateRect(edit, null(), 0);
        }
        changed
    }

    unsafe fn layout_for(edit: HWND, job: &RichPrint) -> Result<PageLayout, String> {
        let dc = layout_dc()?;
        let result = (|| {
            let live = crate::rich_print::paginate(edit, dc, job.geometry, (0, 0))?;
            let starts: Vec<i32> = live.iter().map(|(start, _)| *start).collect();
            let count = if job.body_rtf.is_some() || !job.notes.is_empty() {
                let (print_edit, temporary) = crate::rich_notes::temporary_edit(edit, job)?;
                let pages = crate::rich_notes::paginate(print_edit, dc, job, (0, 0))
                    .map(|layout| layout.pages.len());
                crate::rich_print::release(print_edit);
                if temporary {
                    DestroyWindow(print_edit);
                }
                pages?
            } else {
                starts.len()
            };
            Ok(PageLayout {
                starts,
                count: count.max(1),
            })
        })();
        DeleteDC(dc);
        result
    }

    unsafe fn layout_dc() -> Result<HDC, String> {
        let screen = GetDC(null_mut());
        if screen.is_null() {
            return Err("Cannot access display device".into());
        }
        let dc = CreateCompatibleDC(screen);
        ReleaseDC(null_mut(), screen);
        if dc.is_null() {
            return Err("Cannot create layout device".into());
        }
        Ok(dc)
    }

    /// Re-paginate the live body now (the caret left the sheet it was on).
    unsafe fn repaginate(edit: HWND) {
        let Some(job) = VIEW.with(|view| view.borrow().job.clone()) else {
            return;
        };
        let Ok(dc) = layout_dc() else { return };
        let live = crate::rich_print::paginate(edit, dc, job.geometry, (0, 0));
        DeleteDC(dc);
        let Ok(live) = live else { return };
        let starts: Vec<i32> = live.iter().map(|(start, _)| *start).collect();
        // The status bar layout (with notes) follows at the next settled measurement.
        VIEW.with(|view| {
            let mut view = view.borrow_mut();
            view.starts = starts;
            view.cache.clear();
        });
        invalidate();
    }

    /// Sheet geometry while Print Layout can be shown.
    fn sheets() -> Option<Sheets> {
        let frame = CANVAS.with(|canvas| canvas.borrow().frame)?;
        VIEW.with(|view| {
            let view = view.borrow();
            let job = view.job.as_ref()?;
            if !view.requested || view.area.is_none() || view.starts.is_empty() {
                return None;
            }
            let g = job.geometry;
            if !g.valid() || frame.1 <= 0 {
                return None;
            }
            let height = i64::from(frame.1) * i64::from(g.height_twips) / i64::from(g.width_twips);
            let dpi = unsafe { GetDpiForWindow(view.edit) }.max(96) as i32;
            Some(Sheets {
                count: view.starts.len(),
                left: frame.0,
                width: frame.1,
                height: height.clamp(1, 1 << 24) as i32,
                gap: 16 * dpi / 96,
            })
        })
    }

    /// Twips to desk pixels at the current sheet scale.
    fn scale(sheets: &Sheets, job: &RichPrint, twips: i32) -> i32 {
        (i64::from(twips) * i64::from(sheets.width) / i64::from(job.geometry.width_twips.max(1)))
            as i32
    }

    /// Body rectangle of sheet `index` in unscrolled desk coordinates.
    fn body(sheets: &Sheets, job: &RichPrint, index: usize) -> RECT {
        let g = job.geometry;
        let top = sheets.top(index);
        RECT {
            left: sheets.left + scale(sheets, job, g.margin_left_twips),
            top: top + scale(sheets, job, g.margin_top_twips),
            right: sheets.left + sheets.width - scale(sheets, job, g.margin_right_twips),
            bottom: top + sheets.height - scale(sheets, job, g.margin_bottom_twips),
        }
    }

    unsafe fn char_y(edit: HWND, position: i32) -> i32 {
        let mut point: POINTL = zeroed();
        SendMessageW(edit, EM_POSFROMCHAR, &mut point as *mut _ as usize, position as isize);
        point.y
    }

    unsafe fn scroll_pos(edit: HWND) -> POINT {
        let mut point: POINT = zeroed();
        SendMessageW(edit, EM_GETSCROLLPOS, 0, &mut point as *mut _ as isize);
        point
    }

    unsafe fn text_length(edit: HWND) -> i32 {
        let request = [2u32 | 8, 1200];
        SendMessageW(edit, EM_GETTEXTLENGTHEX, request.as_ptr() as usize, 0).clamp(0, i32::MAX as isize)
            as i32
    }

    /// Scroll the editor so the line holding `target` is at its top; returns the
    /// rows still above it (non-zero when the editor cannot scroll further).
    unsafe fn pin_line(edit: HWND, target: i32) -> i32 {
        let mut ratio = 1.0f64;
        for _ in 0..4 {
            let y = char_y(edit, target);
            if y.abs() <= 1 {
                break;
            }
            let mut point = scroll_pos(edit);
            let step = (f64::from(y) * ratio).round() as i32;
            point.y = point.y.saturating_add(step).max(0);
            SendMessageW(edit, EM_SETSCROLLPOS, 0, &point as *const _ as isize);
            let moved = y - char_y(edit, target);
            if moved == 0 {
                break;
            }
            ratio = (ratio * f64::from(y) / f64::from(moved)).clamp(0.25, 4.0);
        }
        char_y(edit, target).max(0)
    }

    /// Show the desk and place the editor over the current sheet, or restore the
    /// normal editor when Print Layout is off or the document cannot be paginated.
    unsafe fn place(edit: HWND) -> bool {
        let _busy = Busy::enter();
        let sheets = sheets();
        let (area, job, starts, page, scroll, overlaid) = VIEW.with(|view| {
            let view = view.borrow();
            (view.area, view.job.clone(), view.starts.clone(), view.page, view.scroll, view.overlaid)
        });
        let (Some(sheets), Some(area), Some(job)) = (sheets, area, job) else {
            restore(edit, overlaid, area);
            return false;
        };
        let desk = desk(edit);
        if desk.is_null() {
            restore(edit, overlaid, Some(area));
            return false;
        }
        let shown = IsWindowVisible(edit) != 0;
        SetWindowPos(
            desk,
            edit,
            area.left,
            area.top,
            (area.right - area.left).max(0),
            (area.bottom - area.top).max(0),
            SWP_NOACTIVATE | if shown { SWP_SHOWWINDOW } else { SWP_HIDEWINDOW },
        );
        if !overlaid {
            let style = GetWindowLongPtrW(edit, GWL_EXSTYLE);
            SetWindowLongPtrW(edit, GWL_EXSTYLE, style & !(WS_EX_CLIENTEDGE as isize));
            let style = GetWindowLongPtrW(edit, GWL_STYLE);
            SetWindowLongPtrW(edit, GWL_STYLE, style & !((WS_VSCROLL | WS_HSCROLL) as isize));
            SendMessageW(edit, EM_SHOWSCROLLBAR, SB_VERT as usize, 0);
            SendMessageW(edit, EM_SHOWSCROLLBAR, SB_HORZ as usize, 0);
            SetWindowPos(
                edit,
                null_mut(),
                0,
                0,
                0,
                0,
                SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
            );
        }
        let mut client: RECT = zeroed();
        GetClientRect(desk, &mut client);
        let scroll = super::clamp_scroll(scroll, client.bottom, sheets.total());
        update_scroll(desk, scroll, client.bottom, sheets.total());
        let page = page.min(sheets.count - 1);
        let body = body(&sheets, &job, page);
        let width = (body.right - body.left).max(1);
        let height = (body.bottom - body.top).max(1);
        let x = area.left + body.left;
        let top = area.top + body.top - scroll;
        let previous = VIEW.with(|view| view.borrow().pinned.map(|(_, hidden, _)| hidden));
        let mut hidden = previous.unwrap_or(0);
        for _ in 0..3 {
            SetWindowPos(edit, null_mut(), x, top - hidden, width, height, SWP_NOZORDER | SWP_NOACTIVATE);
            let format = RECT { left: 0, top: 0, right: width, bottom: height };
            let mut current: RECT = zeroed();
            SendMessageW(edit, EM_GETRECT, 0, &mut current as *mut _ as isize);
            if (current.left, current.top, current.right, current.bottom) != (0, 0, width, height) {
                SendMessageW(edit, EM_SETRECT, 0, &format as *const _ as isize);
            }
            let measured = pin_line(edit, starts[page]).min(height - 1);
            if measured == hidden {
                break;
            }
            hidden = measured;
        }
        let bottom = match starts.get(page + 1) {
            Some(next) => char_y(edit, *next).clamp(hidden, height),
            None => height,
        };
        let edit_top = top - hidden;
        let visible = RECT {
            left: (area.left - x).max(0),
            top: (area.top - edit_top).max(hidden),
            right: (area.left + client.right - x).min(width),
            bottom: (area.top + client.bottom - edit_top).min(bottom),
        };
        let region = if visible.right > visible.left && visible.bottom > visible.top {
            CreateRectRgn(visible.left, visible.top, visible.right, visible.bottom)
        } else {
            CreateRectRgn(0, 0, 0, 0)
        };
        SetWindowRgn(edit, region, 1);
        let pinned = (scroll_pos(edit), hidden, bottom);
        let count = sheets.count;
        VIEW.with(|view| {
            let mut view = view.borrow_mut();
            view.overlaid = true;
            view.scroll = scroll;
            view.page = page;
            view.pinned = Some(pinned);
        });
        let caption = crate::wide(&super::caption(page, count));
        let mut current = [0u16; 128];
        let length = GetWindowTextW(desk, current.as_mut_ptr(), current.len() as i32).max(0) as usize;
        if current[..length] != caption[..caption.len() - 1] {
            SetWindowTextW(desk, caption.as_ptr());
        }
        InvalidateRect(desk, null(), 0);
        true
    }

    /// Return the editor to the normal continuous view.
    unsafe fn restore(edit: HWND, overlaid: bool, area: Option<RECT>) {
        let desk = VIEW.with(|view| {
            let mut view = view.borrow_mut();
            view.overlaid = false;
            view.pinned = None;
            view.cache.clear();
            if overlaid {
                view.reformat = true;
            }
            view.desk
        });
        if !desk.is_null() {
            ShowWindow(desk, SW_HIDE);
        }
        if !overlaid {
            return;
        }
        SetWindowRgn(edit, null_mut(), 1);
        let style = GetWindowLongPtrW(edit, GWL_EXSTYLE);
        SetWindowLongPtrW(edit, GWL_EXSTYLE, style | WS_EX_CLIENTEDGE as isize);
        let style = GetWindowLongPtrW(edit, GWL_STYLE);
        SetWindowLongPtrW(edit, GWL_STYLE, style | WS_VSCROLL as isize);
        SendMessageW(edit, EM_SHOWSCROLLBAR, SB_VERT as usize, 1);
        SetWindowPos(
            edit,
            null_mut(),
            0,
            0,
            0,
            0,
            SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
        );
        if let Some(area) = area {
            MoveWindow(
                edit,
                area.left,
                area.top,
                (area.right - area.left).max(0),
                (area.bottom - area.top).max(0),
                1,
            );
        }
        SendMessageW(edit, EM_SCROLLCARET, 0, 0);
        InvalidateRect(edit, null(), 1);
    }

    unsafe fn update_scroll(desk: HWND, scroll: i32, view: i32, total: i32) {
        let info = SCROLLINFO {
            cbSize: size_of::<SCROLLINFO>() as u32,
            fMask: SIF_RANGE | SIF_PAGE | SIF_POS | SIF_DISABLENOSCROLL,
            nMin: 0,
            nMax: total.saturating_sub(1).max(0),
            nPage: view.max(0) as u32,
            nPos: scroll,
            nTrackPos: 0,
        };
        SetScrollInfo(desk, SB_VERT, &info, 1);
    }

    unsafe fn desk(edit: HWND) -> HWND {
        let existing = VIEW.with(|view| view.borrow().desk);
        if !existing.is_null() && IsWindow(existing) != 0 {
            return existing;
        }
        let class = crate::wide("Off42PageView");
        let instance = GetModuleHandleW(null());
        let mut info: WNDCLASSW = zeroed();
        if GetClassInfoW(instance, class.as_ptr(), &mut info) == 0 {
            info.style = CS_DBLCLKS;
            info.lpfnWndProc = Some(desk_proc);
            info.hInstance = instance;
            info.hCursor = LoadCursorW(null_mut(), IDC_ARROW);
            info.lpszClassName = class.as_ptr();
            RegisterClassW(&info);
        }
        let desk = CreateWindowExW(
            0,
            class.as_ptr(),
            null(),
            WS_CHILD | WS_VSCROLL | WS_CLIPSIBLINGS,
            0,
            0,
            0,
            0,
            GetParent(edit),
            null_mut(),
            instance,
            null(),
        );
        VIEW.with(|view| view.borrow_mut().desk = desk);
        desk
    }

    /// Scroll the desk by `delta` pixels (or to `absolute`) and move the editor with it.
    unsafe fn scroll_desk(desk: HWND, target: impl FnOnce(i32, i32) -> i32) {
        let Some(sheets) = sheets() else { return };
        let mut client: RECT = zeroed();
        GetClientRect(desk, &mut client);
        let (edit, scroll) = VIEW.with(|view| {
            let view = view.borrow();
            (view.edit, view.scroll)
        });
        let next = super::clamp_scroll(target(scroll, client.bottom), client.bottom, sheets.total());
        if next != scroll {
            VIEW.with(|view| view.borrow_mut().scroll = next);
            place(edit);
        }
    }

    unsafe extern "system" fn desk_proc(hwnd: HWND, message: u32, w: WPARAM, l: LPARAM) -> LRESULT {
        match message {
            WM_ERASEBKGND => 1,
            WM_PAINT => {
                paint(hwnd);
                0
            }
            WM_VSCROLL => {
                let mut info: SCROLLINFO = zeroed();
                info.cbSize = size_of::<SCROLLINFO>() as u32;
                info.fMask = SIF_TRACKPOS;
                GetScrollInfo(hwnd, SB_VERT, &mut info);
                let line = 40 * GetDpiForWindow(hwnd).max(96) as i32 / 96;
                let action = (w & 0xffff) as i32;
                scroll_desk(hwnd, |scroll, view| match action {
                    SB_LINEUP => scroll - line,
                    SB_LINEDOWN => scroll + line,
                    SB_PAGEUP => scroll - view.max(line),
                    SB_PAGEDOWN => scroll + view.max(line),
                    SB_THUMBPOSITION | SB_THUMBTRACK => info.nTrackPos,
                    SB_TOP => 0,
                    SB_BOTTOM => i32::MAX,
                    _ => scroll,
                });
                0
            }
            WM_MOUSEWHEEL => {
                let delta = i32::from((w >> 16) as u16 as i16);
                let dpi = GetDpiForWindow(hwnd).max(96) as i32;
                scroll_desk(hwnd, |scroll, _| scroll.saturating_sub(delta * dpi / 96 / 2));
                0
            }
            WM_SETFOCUS => {
                let edit = VIEW.with(|view| view.borrow().edit);
                if !edit.is_null() {
                    SetFocus(edit);
                }
                0
            }
            WM_LBUTTONDOWN | WM_LBUTTONDBLCLK => {
                click(hwnd, message, w, l);
                0
            }
            _ => DefWindowProcW(hwnd, message, w, l),
        }
    }

    /// A click on a sheet moves the editor to that sheet and forwards the click.
    unsafe fn click(desk: HWND, message: u32, w: WPARAM, l: LPARAM) {
        let Some(sheets) = sheets() else { return };
        let x = i32::from(l as u16 as i16);
        let y = i32::from((l >> 16) as u16 as i16);
        let (edit, scroll) = VIEW.with(|view| {
            let view = view.borrow();
            (view.edit, view.scroll)
        });
        VIEW.with(|view| view.borrow_mut().page = sheets.hit(y + scroll));
        if !place(edit) {
            return;
        }
        let Some((_, hidden, bottom)) = VIEW.with(|view| view.borrow().pinned) else {
            return;
        };
        let mut point = POINT { x, y };
        ClientToScreen(desk, &mut point);
        ScreenToClient(edit, &mut point);
        let mut client: RECT = zeroed();
        GetClientRect(edit, &mut client);
        point.x = point.x.clamp(0, (client.right - 1).max(0));
        point.y = point.y.clamp(hidden, (bottom - 1).max(hidden));
        SetFocus(edit);
        let position = ((point.y as u32 & 0xffff) << 16) | (point.x as u32 & 0xffff);
        SendMessageW(edit, message, w, position as isize);
    }

    unsafe fn paint(desk: HWND) {
        let mut ps: PAINTSTRUCT = zeroed();
        let dc = BeginPaint(desk, &mut ps);
        let mut client: RECT = zeroed();
        GetClientRect(desk, &mut client);
        let width = client.right.max(1);
        let height = client.bottom.max(1);
        let memory = CreateCompatibleDC(dc);
        let bitmap = CreateCompatibleBitmap(dc, width, height);
        let previous = SelectObject(memory, bitmap as _);
        let desk_brush = CreateSolidBrush(DESK);
        let edge = CreateSolidBrush(EDGE);
        let shadow = CreateSolidBrush(SHADOW);
        let white = GetStockObject(WHITE_BRUSH) as HBRUSH;
        FillRect(memory, &client, desk_brush);
        let state = VIEW.with(|view| {
            let mut view = view.borrow_mut();
            (
                view.edit,
                view.job.clone(),
                view.starts.clone(),
                view.scroll,
                view.page,
                view.overlaid,
                std::mem::take(&mut view.cache),
            )
        });
        let (edit, job, starts, scroll, page, overlaid, mut cache) = state;
        if let (Some(sheets), Some(job), true) = (sheets(), job, overlaid) {
            let _busy = Busy::enter();
            let offset = (2 * GetDpiForWindow(desk).max(96) as i32 / 96).max(1);
            SetStretchBltMode(memory, HALFTONE);
            SetBrushOrgEx(memory, 0, 0, null_mut());
            let range = sheets.visible(scroll, scroll + height);
            cache.retain(|bitmap| range.contains(&bitmap.index));
            for index in range {
                let top = sheets.top(index) - scroll;
                let sheet = RECT {
                    left: sheets.left,
                    top,
                    right: sheets.left + sheets.width,
                    bottom: top + sheets.height,
                };
                let drop = RECT {
                    left: sheet.left + offset,
                    top: sheet.top + offset,
                    right: sheet.right + offset,
                    bottom: sheet.bottom + offset,
                };
                FillRect(memory, &drop, shadow);
                FillRect(memory, &sheet, white);
                if !cache.iter().any(|bitmap| bitmap.index == index) {
                    if let Some(rendered) = render(edit, &job, &starts, index) {
                        cache.push(rendered);
                    }
                }
                if let Some(rendered) = cache.iter().find(|bitmap| bitmap.index == index) {
                    let mut info: BITMAPINFO = zeroed();
                    info.bmiHeader.biSize = size_of::<BITMAPINFOHEADER>() as u32;
                    info.bmiHeader.biWidth = rendered.width;
                    info.bmiHeader.biHeight = -rendered.height;
                    info.bmiHeader.biPlanes = 1;
                    info.bmiHeader.biBitCount = 32;
                    info.bmiHeader.biCompression = BI_RGB;
                    StretchDIBits(
                        memory,
                        sheet.left,
                        sheet.top,
                        sheets.width,
                        sheets.height,
                        0,
                        0,
                        rendered.width,
                        rendered.height,
                        rendered.pixels.as_ptr() as _,
                        &info,
                        DIB_RGB_COLORS,
                        SRCCOPY,
                    );
                }
                if index == page {
                    // The live editor shows this body; hide the rendered copy beneath it.
                    let mut live = body(&sheets, &job, index);
                    live.top -= scroll;
                    live.bottom -= scroll;
                    FillRect(memory, &live, white);
                }
                FrameRect(memory, &sheet, edge);
            }
            cache.truncate(MAX_CACHED);
        }
        BitBlt(dc, 0, 0, width, height, memory, 0, 0, SRCCOPY);
        VIEW.with(|view| view.borrow_mut().cache = cache);
        SelectObject(memory, previous);
        DeleteObject(bitmap as _);
        DeleteDC(memory);
        DeleteObject(desk_brush as _);
        DeleteObject(edge as _);
        DeleteObject(shadow as _);
        EndPaint(desk, &ps);
    }

    /// Render sheet `index` (body text, header and footer) like Print Preview does.
    unsafe fn render(edit: HWND, job: &RichPrint, starts: &[i32], index: usize) -> Option<Bitmap> {
        let start = *starts.get(index)?;
        let geometry = job.geometry;
        let screen = GetDC(null_mut());
        if screen.is_null() {
            return None;
        }
        let dpi_x = GetDeviceCaps(screen, LOGPIXELSX as i32);
        let dpi_y = GetDeviceCaps(screen, LOGPIXELSY as i32);
        let dc = CreateCompatibleDC(screen);
        ReleaseDC(null_mut(), screen);
        if dc.is_null() {
            return None;
        }
        let width = (i64::from(geometry.width_twips) * i64::from(dpi_x) / 1440) as i32;
        let height = (i64::from(geometry.height_twips) * i64::from(dpi_y) / 1440) as i32;
        if dpi_x <= 0 || dpi_y <= 0 || !(1..=8192).contains(&width) || !(1..=8192).contains(&height) {
            DeleteDC(dc);
            return None;
        }
        let mut info: BITMAPINFO = zeroed();
        info.bmiHeader.biSize = size_of::<BITMAPINFOHEADER>() as u32;
        info.bmiHeader.biWidth = width;
        info.bmiHeader.biHeight = -height;
        info.bmiHeader.biPlanes = 1;
        info.bmiHeader.biBitCount = 32;
        info.bmiHeader.biCompression = BI_RGB;
        let mut bits = null_mut();
        let bitmap = CreateDIBSection(dc, &info, DIB_RGB_COLORS, &mut bits, null_mut(), 0);
        if bitmap.is_null() || bits.is_null() {
            if !bitmap.is_null() {
                DeleteObject(bitmap as _);
            }
            DeleteDC(dc);
            return None;
        }
        let previous = SelectObject(dc, bitmap as _);
        PatBlt(dc, 0, 0, width, height, WHITENESS);
        crate::rich_print::body(edit, dc, geometry, (0, 0), start);
        crate::rich_print::release(edit);
        // Headers that do not fit the margins are reported by printing; here the body still shows.
        let _ = crate::rich_print::decorations(dc, job, (0, 0), index, starts.len());
        GdiFlush();
        let pixels =
            std::slice::from_raw_parts(bits as *const u8, (width as usize) * (height as usize) * 4).to_vec();
        SelectObject(dc, previous);
        DeleteObject(bitmap as _);
        DeleteDC(dc);
        Some(Bitmap { index, width, height, pixels })
    }

    /// Messages after which the caret, the text or the editor scroll may have changed.
    fn moves_caret(message: u32) -> bool {
        matches!(
            message,
            WM_KEYDOWN
                | WM_CHAR
                | WM_LBUTTONUP
                | WM_LBUTTONDOWN
                | WM_LBUTTONDBLCLK
                | WM_PASTE
                | WM_CUT
                | WM_CLEAR
                | WM_UNDO
                | WM_SETTEXT
                | WM_VSCROLL
                | WM_IME_ENDCOMPOSITION
                | WM_IME_COMPOSITION
                | EM_SETSEL
                | EM_EXSETSEL
                | EM_REPLACESEL
                | EM_UNDO
                | EM_REDO
                | EM_STREAMIN
                | EM_SETTEXTEX
                | EM_SCROLLCARET
                | EM_LINESCROLL
                | EM_SETSCROLLPOS
        )
    }

    /// Keep the editor on the sheet that holds the caret and the caret on screen.
    unsafe fn follow(edit: HWND, message: u32) {
        if BUSY.with(Cell::get) || !moves_caret(message) {
            return;
        }
        let snapshot = VIEW.with(|view| {
            let view = view.borrow();
            let (scroll, hidden, bottom) = view.pinned.filter(|_| view.overlaid)?;
            Some((scroll, hidden, bottom, view.page, view.starts.clone(), view.last, view.desk))
        });
        let Some((pinned, hidden, bottom, page, starts, last, desk)) = snapshot else {
            return;
        };
        if GetCapture() == edit && message != WM_LBUTTONUP {
            return;
        }
        let _busy = Busy::enter();
        let caret = crate::rich_edit::selection(edit).1;
        let length = text_length(edit);
        let y = char_y(edit, caret.min(length as usize) as i32);
        let scroll = scroll_pos(edit);
        let moved = scroll.x != pinned.x || scroll.y != pinned.y;
        let outside = y < hidden || y >= bottom;
        if moved || outside || super::page_index(&starts, caret) != page {
            if outside || last.1 != length {
                repaginate(edit);
            }
            VIEW.with(|view| {
                let mut view = view.borrow_mut();
                view.page = super::page_index(&view.starts, caret);
            });
            place(edit);
        }
        if last != (caret, length) {
            VIEW.with(|view| view.borrow_mut().last = (caret, length));
            reveal_caret(edit, desk, caret, length);
        }
    }

    /// Scroll the desk so the caret line is visible.
    unsafe fn reveal_caret(edit: HWND, desk: HWND, caret: usize, length: i32) {
        let (Some(sheets), Some(job), Some((_, hidden, _)), page, scroll) = VIEW.with(|view| {
            let view = view.borrow();
            (sheets(), view.job.clone(), view.pinned, view.page, view.scroll)
        }) else {
            return;
        };
        let mut client: RECT = zeroed();
        GetClientRect(desk, &mut client);
        let line = 24 * GetDpiForWindow(edit).max(96) as i32 / 96;
        let y = body(&sheets, &job, page).top + char_y(edit, caret.min(length as usize) as i32) - hidden;
        let next = super::reveal(scroll, client.bottom, y - line / 2, y + line + line / 2);
        if next != scroll {
            VIEW.with(|view| view.borrow_mut().scroll = next);
            place(edit);
        }
    }

    /// Subclass procedure drawing the desk and page breaks over the editor and
    /// keeping the editor on its sheet in Print Layout.
    pub unsafe extern "system" fn paint_proc(
        hwnd: HWND,
        message: u32,
        w: WPARAM,
        l: LPARAM,
        id: usize,
        _data: usize,
    ) -> LRESULT {
        if message == WM_NCDESTROY {
            windows_sys::Win32::UI::Shell::RemoveWindowSubclass(hwnd, Some(paint_proc), id);
            clear();
            return DefSubclassProc(hwnd, message, w, l);
        }
        if message == WM_MOUSEWHEEL && overlaid() {
            let desk = VIEW.with(|view| view.borrow().desk);
            return SendMessageW(desk, message, w, l);
        }
        let result = DefSubclassProc(hwnd, message, w, l);
        if overlaid() {
            follow(hwnd, message);
        } else if message == WM_PAINT || message == WM_VSCROLL || message == WM_MOUSEWHEEL {
            draw(hwnd);
        }
        result
    }

    unsafe fn draw(edit: HWND) {
        let Some((frame, layout)) = CANVAS.with(|canvas| {
            let canvas = canvas.borrow();
            Some((canvas.frame?, canvas.layout.clone()?))
        }) else {
            return;
        };
        let mut client: RECT = zeroed();
        GetClientRect(edit, &mut client);
        let mut scroll: POINT = zeroed();
        SendMessageW(edit, EM_GETSCROLLPOS, 0, &mut scroll as *mut _ as isize);
        let left = frame.0 - scroll.x;
        let right = left.saturating_add(frame.1);
        let dc = GetDC(edit);
        if dc.is_null() {
            return;
        }
        let desk = CreateSolidBrush(DESK);
        let edge = CreateSolidBrush(EDGE);
        if left > 0 {
            FillRect(dc, &RECT { left: 0, top: 0, right: left - 1, bottom: client.bottom }, desk);
            FillRect(dc, &RECT { left: left - 1, top: 0, right: left, bottom: client.bottom }, edge);
        }
        if right < client.right {
            FillRect(dc, &RECT { left: right, top: 0, right: right + 1, bottom: client.bottom }, edge);
            FillRect(dc, &RECT { left: right + 1, top: 0, right: client.right, bottom: client.bottom }, desk);
        }
        let pen = CreatePen(PS_DOT, 1, 0x0090_8C88);
        let old_pen = SelectObject(dc, pen as _);
        SetBkMode(dc, TRANSPARENT as i32);
        SetTextColor(dc, 0x0070_6C68);
        let font = GetStockObject(DEFAULT_GUI_FONT);
        let old_font = SelectObject(dc, font);
        for (index, start) in layout.starts.iter().enumerate().skip(1) {
            let mut point: POINTL = zeroed();
            SendMessageW(edit, EM_POSFROMCHAR, &mut point as *mut _ as usize, *start as isize);
            let y = point.y - 1;
            if y < 0 || y > client.bottom {
                continue;
            }
            MoveToEx(dc, left, y, null_mut());
            LineTo(dc, right, y);
            let label: Vec<u16> = format!("{} {}", crate::localization::text("Page"), index + 1)
                .encode_utf16()
                .collect();
            let mut size: SIZE = zeroed();
            GetTextExtentPoint32W(dc, label.as_ptr(), label.len() as i32, &mut size);
            let x = (right - size.cx - 4).max(left);
            TextOutW(dc, x, y + 2, label.as_ptr(), label.len() as i32);
        }
        SelectObject(dc, old_font);
        SelectObject(dc, old_pen);
        DeleteObject(pen as _);
        DeleteObject(desk as _);
        DeleteObject(edge as _);
        ReleaseDC(edit, dc);
    }
}

