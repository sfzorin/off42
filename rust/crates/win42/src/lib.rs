pub mod ocr;
pub mod recovery;
pub use suite42_app::{GridAxis, GridRange, SparseTable};
#[cfg(windows)]
use suite42_app::grid_geometry;
mod system_font;
pub use system_font::program as system_font_program;
#[cfg(any(windows,test))]
mod proofing;
#[cfg(windows)]
use suite42_app::pdf_print_layout;
#[cfg(windows)]
pub mod pdfium;
#[cfg(windows)]
mod pdf_preview;
pub use suite42_app::localization;
mod theme;
pub use suite42_app::host::*;
pub use suite42_app::report::{ReportEvent, ReportRequest};
pub use suite42_app::{InputRequest, LabelSource};
pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

/// True while the Delete key is held, so editors can tell forward from backward deletion.
pub fn forward_delete_pressed() -> bool {
    #[cfg(windows)]
    unsafe {
        windows_sys::Win32::UI::Input::KeyboardAndMouse::GetKeyState(0x2E) < 0
    }
    #[cfg(not(windows))]
    false
}
#[cfg(windows)]
mod clipboard;
#[cfg(windows)]
use suite42_app::clipboard_envelope;
#[cfg(windows)]
mod dialogs;
#[cfg(windows)]
mod grid;
#[cfg(windows)]
use suite42_app::label_preferences;
#[cfg(windows)]
mod marks;
pub mod generator;
#[cfg(windows)]
use suite42_app::recents;
#[cfg(windows)]
use suite42_app::window_layout;
#[cfg(windows)]
mod native;
#[cfg(windows)]
pub use native::{render_scene_rgb, SceneTextMeasurer};
#[cfg(windows)]
mod report;
#[cfg(windows)]
use suite42_app::note_pagination;
#[cfg(windows)]
mod print_calibration;
#[cfg(windows)]
use suite42_app::production;
#[cfg(windows)]
mod prompt;
#[cfg(windows)]
mod rich_edit;
#[cfg(windows)]
mod rich_format;
#[cfg(windows)]
mod rich_notes;
#[cfg(windows)]
mod rich_preview;
#[cfg(windows)]
mod rich_artwork;
#[cfg(windows)]
mod rich_pages;
#[cfg(windows)]
mod rich_print;
#[cfg(windows)]
mod rich_ruler;
#[cfg(windows)]
mod scene_transform;
#[cfg(windows)]
pub use native::run;

/// Explorer's natural filename ordering for numbered PDF batches.
pub fn logical_file_order(a: &std::ffi::OsStr, b: &std::ffi::OsStr) -> std::cmp::Ordering {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        let a: Vec<_> = a.encode_wide().chain(Some(0)).collect();
        let b: Vec<_> = b.encode_wide().chain(Some(0)).collect();
        return unsafe { windows_sys::Win32::UI::Shell::StrCmpLogicalW(a.as_ptr(), b.as_ptr()) }
            .cmp(&0);
    }
    #[cfg(not(windows))]
    {
        suite42_app::host::natural_file_order(a, b)
    }
}
