//! Bounded extraction in PDFium reading order. Layout reconstruction is intentionally not implied.
use super::TextBuilder;
use doc42_core::rich::{RichDocument, TextStyle};
use std::{
    ffi::{c_char, c_void},
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
};
type Handle = *mut c_void;
#[link(name = "kernel32")]
extern "system" {
    fn LoadLibraryExW(name: *const u16, file: Handle, flags: u32) -> Handle;
    fn GetProcAddress(module: Handle, name: *const c_char) -> *mut c_void;
    fn FreeLibrary(module: Handle) -> i32;
}
struct Library(Handle);
impl Drop for Library {
    fn drop(&mut self) {
        unsafe {
            FreeLibrary(self.0);
        }
    }
}
struct Api {
    _library: Library,
    destroy: unsafe extern "system" fn(),
    load: unsafe extern "system" fn(*const c_void, usize, *const c_char) -> Handle,
    close: unsafe extern "system" fn(Handle),
    page: unsafe extern "system" fn(Handle, i32) -> Handle,
    close_page: unsafe extern "system" fn(Handle),
    text: unsafe extern "system" fn(Handle) -> Handle,
    close_text: unsafe extern "system" fn(Handle),
    count: unsafe extern "system" fn(Handle) -> i32,
    unicode: unsafe extern "system" fn(Handle, i32) -> u32,
    box_rect: unsafe extern "system" fn(Handle, i32, *mut f64, *mut f64, *mut f64, *mut f64) -> i32,
    size: unsafe extern "system" fn(Handle, i32) -> f64,
    font: unsafe extern "system" fn(Handle, i32, *mut c_void, u32, *mut i32) -> u32,
    weight: unsafe extern "system" fn(Handle, i32) -> i32,
    fill: unsafe extern "system" fn(Handle, i32, *mut u32, *mut u32, *mut u32, *mut u32) -> i32,
}
impl Drop for Api {
    fn drop(&mut self) {
        unsafe {
            (self.destroy)();
        }
    }
}
impl Api {
    unsafe fn open() -> Result<Self, String> {
        let path: Vec<u16> = "pdfium.dll\0".encode_utf16().collect();
        let library = Library(LoadLibraryExW(
            path.as_ptr(),
            std::ptr::null_mut(),
            0x200 | 0x800,
        ));
        if library.0.is_null() {
            return Err("PDF text export requires the bundled x64 pdfium.dll".into());
        }
        macro_rules! symbol {
            ($name:literal, $ty:ty) => {{
                let address = GetProcAddress(library.0, concat!($name, "\0").as_ptr().cast());
                if address.is_null() {
                    return Err(concat!("PDFium is missing ", $name).into());
                }
                std::mem::transmute::<*mut c_void, $ty>(address)
            }};
        }
        let init = symbol!("FPDF_InitLibrary", unsafe extern "system" fn());
        let api = Self {
            destroy: symbol!("FPDF_DestroyLibrary", unsafe extern "system" fn()),
            load: symbol!(
                "FPDF_LoadMemDocument64",
                unsafe extern "system" fn(*const c_void, usize, *const c_char) -> Handle
            ),
            close: symbol!("FPDF_CloseDocument", unsafe extern "system" fn(Handle)),
            page: symbol!(
                "FPDF_LoadPage",
                unsafe extern "system" fn(Handle, i32) -> Handle
            ),
            close_page: symbol!("FPDF_ClosePage", unsafe extern "system" fn(Handle)),
            text: symbol!(
                "FPDFText_LoadPage",
                unsafe extern "system" fn(Handle) -> Handle
            ),
            close_text: symbol!("FPDFText_ClosePage", unsafe extern "system" fn(Handle)),
            count: symbol!(
                "FPDFText_CountChars",
                unsafe extern "system" fn(Handle) -> i32
            ),
            unicode: symbol!(
                "FPDFText_GetUnicode",
                unsafe extern "system" fn(Handle, i32) -> u32
            ),
            box_rect: symbol!(
                "FPDFText_GetCharBox",
                unsafe extern "system" fn(
                    Handle,
                    i32,
                    *mut f64,
                    *mut f64,
                    *mut f64,
                    *mut f64,
                ) -> i32
            ),
            size: symbol!(
                "FPDFText_GetFontSize",
                unsafe extern "system" fn(Handle, i32) -> f64
            ),
            font: symbol!(
                "FPDFText_GetFontInfo",
                unsafe extern "system" fn(Handle, i32, *mut c_void, u32, *mut i32) -> u32
            ),
            weight: symbol!(
                "FPDFText_GetFontWeight",
                unsafe extern "system" fn(Handle, i32) -> i32
            ),
            fill: symbol!(
                "FPDFText_GetFillColor",
                unsafe extern "system" fn(
                    Handle,
                    i32,
                    *mut u32,
                    *mut u32,
                    *mut u32,
                    *mut u32,
                ) -> i32
            ),
            _library: library,
        };
        init();
        Ok(api)
    }
    unsafe fn style(&self, page: Handle, index: i32) -> TextStyle {
        let mut style = TextStyle::default();
        let size = (self.size)(page, index);
        if size.is_finite() && size > 0. {
            style.size_half_points = (size * 2.).round().clamp(1., 65535.) as u16;
        }
        let mut name = [0u8; 256];
        let mut flags = 0;
        let length = (self.font)(
            page,
            index,
            name.as_mut_ptr().cast(),
            name.len() as u32,
            &mut flags,
        );
        if (2..=256).contains(&length) {
            if let Ok(font) = std::str::from_utf8(&name[..length as usize - 1]) {
                let font = if font.as_bytes().get(6) == Some(&b'+')
                    && font.as_bytes()[..6].iter().all(u8::is_ascii_uppercase)
                {
                    &font[7..]
                } else {
                    font
                };
                // PDF font names are untrusted input; retain a plain family identifier for CSS/RTF.
                let safe: String = font
                    .chars()
                    .filter(|c| c.is_alphanumeric() || matches!(c, ' ' | '-' | '_'))
                    .take(128)
                    .collect();
                if !safe.is_empty() {
                    style.font = safe;
                }
            }
        }
        style.bold = (self.weight)(page, index) >= 600 || flags & (1 << 18) != 0;
        style.italic = flags & (1 << 6) != 0;
        let (mut r, mut g, mut b, mut a) = (0, 0, 0, 0);
        if (self.fill)(page, index, &mut r, &mut g, &mut b, &mut a) != 0
            && [r, g, b, a].iter().all(|v| *v <= 255)
        {
            // Rich text has no alpha; composite against paper white.
            let channel = |v| (v * a + 255 * (255 - a) + 127) / 255;
            style.color = Some((channel(r) << 16) | (channel(g) << 8) | channel(b));
        }
        style
    }
}
struct Document<'a> {
    api: &'a Api,
    handle: Handle,
}
impl Drop for Document<'_> {
    fn drop(&mut self) {
        unsafe {
            (self.api.close)(self.handle);
        }
    }
}
struct Page<'a> {
    api: &'a Api,
    handle: Handle,
    text: Handle,
}
impl Drop for Page<'_> {
    fn drop(&mut self) {
        unsafe {
            if !self.text.is_null() {
                (self.api.close_text)(self.text);
            }
            (self.api.close_page)(self.handle);
        }
    }
}
pub fn extract(
    bytes: &[u8],
    pages: &[usize],
    progress: &AtomicUsize,
    cancel: &AtomicBool,
) -> Result<RichDocument, String> {
    let check = || {
        if cancel.load(Ordering::Relaxed) {
            Err("PDF text export cancelled".to_string())
        } else {
            Ok(())
        }
    };
    check()?;
    let _lock = crate::search::PDFIUM_LOCK
        .lock()
        .map_err(|_| "PDFium lock failed")?;
    unsafe {
        let api = Api::open()?;
        let handle = (api.load)(bytes.as_ptr().cast(), bytes.len(), std::ptr::null());
        if handle.is_null() {
            return Err("PDFium could not open the current PDF snapshot".into());
        }
        let document = Document { api: &api, handle };
        let mut builder = TextBuilder::new();
        for (position, &index) in pages.iter().enumerate() {
            check()?;
            if index > i32::MAX as usize {
                return Err("Invalid PDF page index".into());
            }
            if position > 0 {
                builder.page_break()?;
            }
            let handle = (api.page)(document.handle, index as i32);
            if handle.is_null() {
                return Err(format!("PDFium could not open page {}", index + 1));
            }
            let mut page = Page {
                api: &api,
                handle,
                text: std::ptr::null_mut(),
            };
            page.text = (api.text)(handle);
            if page.text.is_null() {
                return Err(format!("PDFium could not read text on page {}", index + 1));
            }
            let count = (api.count)(page.text);
            if !(0..=1_000_000).contains(&count) {
                return Err("PDF page text exceeds extraction limits".into());
            }
            let mut i = 0;
            while i < count {
                if i % 1024 == 0 {
                    check()?;
                }
                let code = (api.unicode)(page.text, i);
                let style = api.style(page.text, i);
                let code = if (0xD800..=0xDBFF).contains(&code) && i + 1 < count {
                    let low = (api.unicode)(page.text, i + 1);
                    if (0xDC00..=0xDFFF).contains(&low) {
                        i += 1;
                        0x10000 + ((code - 0xD800) << 10) + low - 0xDC00
                    } else {
                        code
                    }
                } else {
                    code
                };
                builder.push(char::from_u32(code).unwrap_or('\u{fffd}'), style)?;
                i += 1;
            }
            progress.store(position + 1, Ordering::Relaxed);
        }
        check()?;
        builder.finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pdfium_extracts_current_edited_redacted_snapshot_in_requested_order() {
        let source = include_bytes!("../../fixtures/page-export.pdf");
        let progress = AtomicUsize::new(0);
        let cancel = AtomicBool::new(false);
        let snapshot = super::super::tests::redacted_snapshot();
        let extracted = extract(&snapshot, &[1, 0], &progress, &cancel).unwrap();
        let text = String::from_utf8(
            super::super::encode(&extracted, super::super::Output::Text).unwrap(),
        )
        .unwrap();
        assert!(text.starts_with("PAGE-THREE"), "{text:?}");
        assert!(!text.contains("PAGE-ONE") && !text.contains("PAGE-TWO"));
        assert!(
            text.ends_with('\u{c}'),
            "The rasterized page must remain blank: {text:?}"
        );
        assert_eq!(progress.load(Ordering::Relaxed), 2);
        cancel.store(true, Ordering::Relaxed);
        assert!(extract(source, &[0], &progress, &cancel).is_err());
    }
}

/// Same decoding as text export, retaining source-coordinate boxes and UTF-8 word positions.
pub fn positioned(
    bytes: &[u8],
    page_count: usize,
    progress: &AtomicUsize,
    cancel: &AtomicBool,
) -> Result<Vec<crate::comparison::PageText>, String> {
    let check = || {
        if cancel.load(Ordering::Relaxed) {
            Err("PDF comparison cancelled".to_string())
        } else {
            Ok(())
        }
    };
    check()?;
    let _lock = crate::search::PDFIUM_LOCK
        .lock()
        .map_err(|_| "PDFium lock failed")?;
    unsafe {
        let api = Api::open()?;
        let handle = (api.load)(bytes.as_ptr().cast(), bytes.len(), std::ptr::null());
        if handle.is_null() {
            return Err("PDFium could not open a comparison snapshot".into());
        }
        let doc = Document { api: &api, handle };
        let mut result = Vec::new();
        let mut total = 0;
        let mut glyph_count = 0;
        for index in 0..page_count {
            check()?;
            let handle = (api.page)(doc.handle, index as i32);
            if handle.is_null() {
                return Err(format!("Cannot read comparison page {}", index + 1));
            }
            let mut page = Page {
                api: &api,
                handle,
                text: std::ptr::null_mut(),
            };
            page.text = (api.text)(handle);
            if page.text.is_null() {
                return Err(format!("Cannot read comparison text on page {}", index + 1));
            }
            let count = (api.count)(page.text);
            if !(0..=1_000_000).contains(&count) {
                return Err("Comparison page exceeds its text limit".into());
            }
            glyph_count += count as usize;
            if glyph_count > 1_000_000 {
                return Err("Comparison document exceeds its character limit".into());
            }
            let mut text = crate::comparison::PageText::default();
            let mut i = 0;
            let char_box = |index: i32| -> Option<[f64; 4]> {
                let (mut l, mut r, mut b, mut t) = (0.0, 0.0, 0.0, 0.0);
                if (api.box_rect)(page.text, index, &mut l, &mut r, &mut b, &mut t) != 0
                    && [l, b, r, t].iter().all(|v| v.is_finite())
                    && r > l
                    && t > b
                {
                    Some([l, b, r, t])
                } else {
                    None
                }
            };
            while i < count {
                if i % 1024 == 0 {
                    check()?;
                }
                let mut code = (api.unicode)(page.text, i);
                let mut bounds = char_box(i);
                if (0xD800..=0xDBFF).contains(&code) && i + 1 < count {
                    let low = (api.unicode)(page.text, i + 1);
                    if (0xDC00..=0xDFFF).contains(&low) {
                        i += 1;
                        code = 0x10000 + ((code - 0xD800) << 10) + low - 0xDC00;
                        if let Some(b) = char_box(i) {
                            bounds = Some(bounds.map_or(b, |a| {
                                [
                                    a[0].min(b[0]),
                                    a[1].min(b[1]),
                                    a[2].max(b[2]),
                                    a[3].max(b[3]),
                                ]
                            }));
                        }
                    }
                }
                let start = text.text.len();
                let ch = char::from_u32(code)
                    .filter(|c| *c != '\0')
                    .unwrap_or('\u{fffd}');
                text.text.push(ch);
                let end = text.text.len();
                total += end - start;
                if total > 4 * 1024 * 1024 {
                    return Err("Comparison document exceeds 4 MB of text".into());
                }
                if let Some(b) = bounds {
                    text.glyphs.push((start..end, b));
                }
                i += 1;
            }
            result.push(text);
            progress.fetch_add(1, Ordering::Relaxed);
        }
        Ok(result)
    }
}
