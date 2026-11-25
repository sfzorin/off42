//! Optional positioned text search through a locally supplied PDFium DLL.
use std::{
    ffi::{c_char, c_void, CString},
    sync::Mutex,
};
pub(crate) static PDFIUM_LOCK: Mutex<()> = Mutex::new(());
type Handle = *mut c_void;
#[link(name = "kernel32")]
extern "system" {
    fn LoadLibraryExW(name: *const u16, file: Handle, flags: u32) -> Handle;
    fn GetProcAddress(module: Handle, name: *const c_char) -> *mut c_void;
    fn FreeLibrary(module: Handle) -> i32;
    fn GetLastError() -> u32;
}
struct Library(Handle);
impl Drop for Library {
    fn drop(&mut self) {
        unsafe {
            FreeLibrary(self.0);
        }
    }
}
#[derive(Clone, Debug)]
pub struct Hit {
    pub page: usize,
    pub boxes: Vec<[f64; 4]>,
}

pub fn find(bytes: &[u8], query: &str, page_count: usize) -> Result<Vec<Hit>, String> {
    if query.is_empty() || query.contains('\0') {
        return Err("Enter search text without NUL characters".into());
    }
    let _lock = PDFIUM_LOCK.lock().map_err(|_| "PDF search lock failed")?;
    unsafe {
        let name: Vec<u16> = "pdfium.dll\0".encode_utf16().collect();
        let lib = Library(LoadLibraryExW(
            name.as_ptr(),
            std::ptr::null_mut(),
            0x200 | 0x800,
        ));
        if lib.0.is_null() {
            let code = GetLastError();
            return Err(format!("PDF text search requires a compatible x64 pdfium.dll alongside PDF42.exe (Windows error {code}; 193 indicates the wrong architecture)"));
        }
        macro_rules! symbol {
            ($name:literal, $ty:ty) => {{
                let name = CString::new($name).unwrap();
                let p = GetProcAddress(lib.0, name.as_ptr());
                if p.is_null() {
                    return Err(format!("pdfium.dll is missing {}", $name));
                }
                std::mem::transmute::<*mut c_void, $ty>(p)
            }};
        }
        let init = symbol!("FPDF_InitLibrary", unsafe extern "C" fn());
        let destroy = symbol!("FPDF_DestroyLibrary", unsafe extern "C" fn());
        let load = symbol!(
            "FPDF_LoadMemDocument64",
            unsafe extern "C" fn(*const c_void, usize, *const c_char) -> Handle
        );
        let close = symbol!("FPDF_CloseDocument", unsafe extern "C" fn(Handle));
        let load_page = symbol!("FPDF_LoadPage", unsafe extern "C" fn(Handle, i32) -> Handle);
        let close_page = symbol!("FPDF_ClosePage", unsafe extern "C" fn(Handle));
        let text_load = symbol!("FPDFText_LoadPage", unsafe extern "C" fn(Handle) -> Handle);
        let text_close = symbol!("FPDFText_ClosePage", unsafe extern "C" fn(Handle));
        let start = symbol!(
            "FPDFText_FindStart",
            unsafe extern "C" fn(Handle, *const u16, u32, i32) -> Handle
        );
        let next = symbol!("FPDFText_FindNext", unsafe extern "C" fn(Handle) -> i32);
        let result_index = symbol!(
            "FPDFText_GetSchResultIndex",
            unsafe extern "C" fn(Handle) -> i32
        );
        let result_count = symbol!("FPDFText_GetSchCount", unsafe extern "C" fn(Handle) -> i32);
        let get_box = symbol!(
            "FPDFText_GetCharBox",
            unsafe extern "C" fn(Handle, i32, *mut f64, *mut f64, *mut f64, *mut f64) -> i32
        );
        let find_close = symbol!("FPDFText_FindClose", unsafe extern "C" fn(Handle));
        init();
        let doc = load(bytes.as_ptr().cast(), bytes.len(), std::ptr::null());
        if doc.is_null() {
            destroy();
            return Err("PDFium could not open the current PDF snapshot".into());
        }
        let term: Vec<u16> = query.encode_utf16().chain(Some(0)).collect();
        let mut hits = Vec::new();
        for i in 0..page_count {
            let page = load_page(doc, i as i32);
            if page.is_null() {
                close(doc);
                destroy();
                return Err(format!("PDFium could not load page {}", i + 1));
            }
            let text = text_load(page);
            if text.is_null() {
                close_page(page);
                close(doc);
                destroy();
                return Err(format!("PDFium could not read text on page {}", i + 1));
            }
            let search = start(text, term.as_ptr(), 0, 0);
            if search.is_null() {
                text_close(text);
                close_page(page);
                close(doc);
                destroy();
                return Err("PDFium could not start text search".into());
            }
            while hits.len() < 1000 && next(search) != 0 {
                let first = result_index(search);
                let count = result_count(search);
                let mut boxes = Vec::new();
                for char_index in first..first.saturating_add(count) {
                    let (mut l, mut r, mut b, mut t) = (0., 0., 0., 0.);
                    if get_box(text, char_index, &mut l, &mut r, &mut b, &mut t) != 0
                        && r > l
                        && t > b
                    {
                        boxes.push([l, b, r, t]);
                    }
                }
                hits.push(Hit { page: i, boxes });
            }
            find_close(search);
            text_close(text);
            close_page(page);
            if hits.len() >= 1000 {
                break;
            }
        }
        close(doc);
        destroy();
        Ok(hits)
    }
}

#[derive(Clone, Debug)]
pub struct Selection {
    pub page: usize,
    pub text: String,
    pub boxes: Vec<[f64; 4]>,
}
/// Select glyphs whose PDFium character boxes intersect a dragged page rectangle.
pub fn select(bytes: &[u8], page_index: usize, rect: [f64; 4]) -> Result<Selection, String> {
    let _lock = PDFIUM_LOCK.lock().map_err(|_| "PDF search lock failed")?;
    unsafe {
        let name: Vec<u16> = "pdfium.dll\0".encode_utf16().collect();
        let lib = Library(LoadLibraryExW(
            name.as_ptr(),
            std::ptr::null_mut(),
            0x200 | 0x800,
        ));
        if lib.0.is_null() {
            let code = GetLastError();
            return Err(format!(
                "Text selection requires a compatible x64 pdfium.dll (Windows error {code})"
            ));
        }
        macro_rules! symbol {
            ($name:literal, $ty:ty) => {{
                let name = CString::new($name).unwrap();
                let p = GetProcAddress(lib.0, name.as_ptr());
                if p.is_null() {
                    return Err(format!("pdfium.dll is missing {}", $name));
                }
                std::mem::transmute::<*mut c_void, $ty>(p)
            }};
        }
        let init = symbol!("FPDF_InitLibrary", unsafe extern "C" fn());
        let destroy = symbol!("FPDF_DestroyLibrary", unsafe extern "C" fn());
        let load = symbol!(
            "FPDF_LoadMemDocument64",
            unsafe extern "C" fn(*const c_void, usize, *const c_char) -> Handle
        );
        let close = symbol!("FPDF_CloseDocument", unsafe extern "C" fn(Handle));
        let load_page = symbol!("FPDF_LoadPage", unsafe extern "C" fn(Handle, i32) -> Handle);
        let close_page = symbol!("FPDF_ClosePage", unsafe extern "C" fn(Handle));
        let text_load = symbol!("FPDFText_LoadPage", unsafe extern "C" fn(Handle) -> Handle);
        let text_close = symbol!("FPDFText_ClosePage", unsafe extern "C" fn(Handle));
        let count = symbol!("FPDFText_CountChars", unsafe extern "C" fn(Handle) -> i32);
        let unicode = symbol!(
            "FPDFText_GetUnicode",
            unsafe extern "C" fn(Handle, i32) -> u32
        );
        let get_box = symbol!(
            "FPDFText_GetCharBox",
            unsafe extern "C" fn(Handle, i32, *mut f64, *mut f64, *mut f64, *mut f64) -> i32
        );
        init();
        let doc = load(bytes.as_ptr().cast(), bytes.len(), std::ptr::null());
        if doc.is_null() {
            destroy();
            return Err("PDFium could not open the current PDF snapshot".into());
        }
        let page = load_page(doc, page_index as i32);
        if page.is_null() {
            close(doc);
            destroy();
            return Err("PDFium could not load page".into());
        }
        let text_page = text_load(page);
        if text_page.is_null() {
            close_page(page);
            close(doc);
            destroy();
            return Err("PDFium could not read page text".into());
        }
        let n = count(text_page);
        if !(0..=1_000_000).contains(&n) {
            text_close(text_page);
            close_page(page);
            close(doc);
            destroy();
            return Err("PDF page text exceeds selection limit".into());
        }
        let (l, b, r, t) = (
            rect[0].min(rect[2]),
            rect[1].min(rect[3]),
            rect[0].max(rect[2]),
            rect[1].max(rect[3]),
        );
        let mut chosen = Vec::new();
        for i in 0..n {
            let (mut cl, mut cr, mut cb, mut ct) = (0., 0., 0., 0.);
            if get_box(text_page, i, &mut cl, &mut cr, &mut cb, &mut ct) != 0
                && cr > cl
                && ct > cb
                && cr > l
                && cl < r
                && ct > b
                && cb < t
            {
                chosen.push((i, [cl, cb, cr, ct]));
            }
        }
        let mut text = String::new();
        let mut boxes = Vec::new();
        for (position, (i, quad)) in chosen.iter().enumerate() {
            if let Some((previous, _)) = position.checked_sub(1).and_then(|p| chosen.get(p)) {
                for gap in previous + 1..*i {
                    if let Some(c) = char::from_u32(unicode(text_page, gap)) {
                        if c.is_whitespace() {
                            text.push(c);
                        }
                    }
                }
            }
            if let Some(c) = char::from_u32(unicode(text_page, *i)) {
                text.push(c);
            }
            boxes.push(*quad);
        }
        text_close(text_page);
        close_page(page);
        close(doc);
        destroy();
        if boxes.is_empty() {
            return Err("No positioned text intersects the selection".into());
        }
        Ok(Selection {
            page: page_index,
            text,
            boxes,
        })
    }
}
