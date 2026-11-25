use std::ptr;
use windows_sys::Win32::{
    Foundation::{HWND, LPARAM},
    UI::WindowsAndMessaging::{SendMessageW, WM_USER},
};
const EM_STREAMIN: u32 = WM_USER + 73;
const EM_STREAMOUT: u32 = WM_USER + 74;
const EM_EXGETSEL: u32 = WM_USER + 52;
const EM_EXSETSEL: u32 = WM_USER + 55;
const EM_SETUNDOLIMIT: u32 = WM_USER + 82;
const EM_GETEVENTMASK: u32 = WM_USER + 59;
const EM_SETEVENTMASK: u32 = WM_USER + 69;
const ENM_CHANGE: usize = 1;
const SF_RTF: usize = 2;
#[repr(C)]
struct CharRange {
    min: i32,
    max: i32,
}
// richedit.h declares EDITSTREAM under pack(4): the callback is at offset 12 on x64.
#[repr(C, packed(4))]
struct EditStream {
    cookie: usize,
    error: u32,
    callback: Option<unsafe extern "system" fn(usize, *mut u8, i32, *mut i32) -> u32>,
}
struct Reader<'a> {
    bytes: &'a [u8],
    offset: usize,
}
unsafe extern "system" fn read(cookie: usize, buffer: *mut u8, len: i32, done: *mut i32) -> u32 {
    let r = &mut *(cookie as *mut Reader<'_>);
    let n = (len.max(0) as usize).min(r.bytes.len() - r.offset);
    ptr::copy_nonoverlapping(r.bytes.as_ptr().add(r.offset), buffer, n);
    r.offset += n;
    *done = n as i32;
    0
}
unsafe extern "system" fn write(cookie: usize, buffer: *mut u8, len: i32, done: *mut i32) -> u32 {
    if len < 0 {
        return 1;
    }
    let output = &mut *(cookie as *mut Vec<u8>);
    if output.len().saturating_add(len as usize) > 64 * 1024 * 1024 {
        return 1;
    }
    output.extend_from_slice(std::slice::from_raw_parts(buffer, len as usize));
    *done = len;
    0
}
pub unsafe fn disable_undo(hwnd: HWND) {
    SendMessageW(hwnd, EM_SETUNDOLIMIT, 0, 0);
}
/// Unlike EDIT, RichEdit sends EN_CHANGE only when ENM_CHANGE is in its event mask.
pub unsafe fn notify_changes(hwnd: HWND) {
    let mask = SendMessageW(hwnd, EM_GETEVENTMASK, 0, 0) as usize;
    SendMessageW(hwnd, EM_SETEVENTMASK, 0, (mask | ENM_CHANGE) as LPARAM);
}
pub unsafe fn selection(hwnd: HWND) -> (usize, usize) {
    let mut range = CharRange { min: 0, max: 0 };
    SendMessageW(hwnd, EM_EXGETSEL, 0, &mut range as *mut _ as LPARAM);
    (range.min.max(0) as usize, range.max.max(0) as usize)
}
pub unsafe fn set_selection(hwnd: HWND, selection: (usize, usize)) {
    let range = CharRange {
        min: selection.0.min(i32::MAX as usize) as i32,
        max: selection.1.min(i32::MAX as usize) as i32,
    };
    SendMessageW(hwnd, EM_EXSETSEL, 0, &range as *const _ as LPARAM);
}
pub unsafe fn load(hwnd: HWND, bytes: &[u8]) -> Result<(), String> {
    let mut reader = Reader { bytes, offset: 0 };
    let mut stream = EditStream {
        cookie: &mut reader as *mut _ as usize,
        error: 0,
        callback: Some(read),
    };
    SendMessageW(hwnd, EM_STREAMIN, SF_RTF, &mut stream as *mut _ as LPARAM);
    let error = stream.error;
    if error != 0 {
        Err(format!("RichEdit stream-in failed: {error}"))
    } else {
        Ok(())
    }
}
pub unsafe fn extract(hwnd: HWND) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    let mut stream = EditStream {
        cookie: &mut bytes as *mut Vec<u8> as usize,
        error: 0,
        callback: Some(write),
    };
    SendMessageW(hwnd, EM_STREAMOUT, SF_RTF, &mut stream as *mut _ as LPARAM);
    let error = stream.error;
    if error != 0 {
        Err(format!("RichEdit stream-out failed: {error}"))
    } else {
        Ok(bytes)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn edit_stream_matches_richedit_h_layout() {
        assert_eq!(std::mem::offset_of!(EditStream, error), size_of::<usize>());
        assert_eq!(
            std::mem::offset_of!(EditStream, callback),
            size_of::<usize>() + 4
        );
        assert_eq!(size_of::<EditStream>(), 2 * size_of::<usize>() + 4);
    }
}
