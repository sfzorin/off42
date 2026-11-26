//! Page canvas for paginated rich editors: page breaks, desk margins and the
//! current page. Pagination reuses the print paginator, so on-screen page numbers
//! and counts match Print Preview and printed output for the same geometry.

/// Measured layout of the live editor, in editor character positions.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PageLayout {
    /// First character of every page; the first entry is always 0.
    pub starts: Vec<i32>,
    /// Printed page count (notes can add pages beyond the live body breaks).
    pub count: usize,
}

impl PageLayout {
    /// One-based page holding `caret`, clamped to the printed count.
    pub fn page_of(&self, caret: usize) -> usize {
        let caret = i32::try_from(caret).unwrap_or(i32::MAX);
        let index = self.starts.partition_point(|start| *start <= caret).max(1);
        index.min(self.count.max(1))
    }
}

/// Status-bar fragment for the caret's page, or None before the first measurement.
pub fn status(layout: Option<&PageLayout>, caret: usize) -> Option<String> {
    let layout = layout.filter(|layout| layout.count > 0)?;
    Some(format!(
        "{} {} / {}",
        crate::localization::text("Page"),
        layout.page_of(caret),
        layout.count
    ))
}

#[cfg(windows)]
pub use native::*;

#[cfg(windows)]
mod native {
    use super::PageLayout;
    use crate::RichPrint;
    use std::{
        cell::RefCell,
        mem::zeroed,
        ptr::null_mut,
        time::{Duration, Instant},
    };
    use windows_sys::Win32::{
        Foundation::*,
        Graphics::Gdi::*,
        UI::{Shell::DefSubclassProc, WindowsAndMessaging::*},
    };

    const EM_POSFROMCHAR: u32 = 0x00D6;
    const EM_GETSCROLLPOS: u32 = WM_USER + 221;
    const SETTLE: Duration = Duration::from_millis(250);

    #[derive(Default)]
    struct Canvas {
        layout: Option<PageLayout>,
        /// Unscrolled page left edge and width in client pixels.
        frame: Option<(i32, i32)>,
        dirty_since: Option<Instant>,
    }

    thread_local! {
        static CANVAS: RefCell<Canvas> = RefCell::new(Canvas::default());
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
    }

    pub fn layout() -> Option<PageLayout> {
        CANVAS.with(|canvas| canvas.borrow().layout.clone())
    }

    pub fn set_frame(edit: HWND, left: i32, width: i32) {
        let changed = CANVAS.with(|canvas| {
            let mut canvas = canvas.borrow_mut();
            let frame = Some((left, width));
            let changed = canvas.frame != frame;
            canvas.frame = frame;
            changed
        });
        if changed {
            unsafe { InvalidateRect(edit, std::ptr::null(), 0) };
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

    /// Measure the live editor (page breaks) and the print job (page count).
    pub unsafe fn measure(edit: HWND, job: Option<&RichPrint>) -> bool {
        let layout = job.and_then(|job| layout_for(edit, job).ok());
        let changed = CANVAS.with(|canvas| {
            let mut canvas = canvas.borrow_mut();
            canvas.dirty_since = None;
            let changed = canvas.layout != layout;
            canvas.layout = layout;
            changed
        });
        if changed {
            InvalidateRect(edit, std::ptr::null(), 0);
        }
        changed
    }

    unsafe fn layout_for(edit: HWND, job: &RichPrint) -> Result<PageLayout, String> {
        let screen = GetDC(null_mut());
        if screen.is_null() {
            return Err("Cannot access display device".into());
        }
        let dc = CreateCompatibleDC(screen);
        ReleaseDC(null_mut(), screen);
        if dc.is_null() {
            return Err("Cannot create layout device".into());
        }
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

    /// Subclass procedure drawing the desk and page breaks over the editor.
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
        let result = DefSubclassProc(hwnd, message, w, l);
        if message == WM_PAINT || message == WM_VSCROLL || message == WM_MOUSEWHEEL {
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
        let desk = CreateSolidBrush(0x00E4_E1DE);
        let edge = CreateSolidBrush(0x00B4_B0AC);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn caret_maps_to_its_page_and_clamps_to_the_printed_count() {
        let layout = PageLayout { starts: vec![0, 120, 300], count: 4 };
        assert_eq!(layout.page_of(0), 1);
        assert_eq!(layout.page_of(119), 1);
        assert_eq!(layout.page_of(120), 2);
        assert_eq!(layout.page_of(5000), 3);
        let notes = PageLayout { starts: vec![0, 10, 20], count: 2 };
        assert_eq!(notes.page_of(25), 2);
        assert_eq!(status(None, 0), None);
        assert!(status(Some(&layout), 130).unwrap().ends_with(" 2 / 4"));
    }
}
