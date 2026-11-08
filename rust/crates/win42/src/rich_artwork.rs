//! Readonly RichEdit selection capture. Layout stays native; one bounded raster enters the PDF.
use std::{
    mem::{size_of, zeroed},
    ptr::{null, null_mut},
};
use windows_sys::Win32::{Foundation::*, Graphics::Gdi::*, UI::WindowsAndMessaging::*};
const EM_FORMATRANGE: u32 = WM_USER + 57;
#[repr(C)]
struct CharRange {
    min: i32,
    max: i32,
}
#[repr(C, packed(4))]
struct FormatRange {
    dc: HDC,
    target: HDC,
    rect: RECT,
    page: RECT,
    range: CharRange,
}
struct Editor(HWND);
impl Drop for Editor {
    fn drop(&mut self) {
        unsafe {
            crate::rich_print::release(self.0);
            DestroyWindow(self.0);
        }
    }
}
struct Metafile(HENHMETAFILE);
impl Drop for Metafile {
    fn drop(&mut self) {
        unsafe {
            DeleteEnhMetaFile(self.0);
        }
    }
}
struct MetaDC(HDC);
impl Drop for MetaDC {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                let f = CloseEnhMetaFile(self.0);
                if !f.is_null() {
                    DeleteEnhMetaFile(f);
                }
            }
        }
    }
}
struct Surface {
    dc: HDC,
    bitmap: HBITMAP,
    previous: HGDIOBJ,
    bits: *mut std::ffi::c_void,
}
impl Drop for Surface {
    fn drop(&mut self) {
        unsafe {
            SelectObject(self.dc, self.previous);
            DeleteObject(self.bitmap);
            DeleteDC(self.dc);
        }
    }
}
impl Surface {
    unsafe fn new(reference: HDC, width: i32, height: i32) -> Result<Self, String> {
        let dc = CreateCompatibleDC(reference);
        if dc.is_null() {
            return Err("Cannot create label artwork device".into());
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
                DeleteObject(bitmap);
            }
            DeleteDC(dc);
            return Err("Cannot allocate label artwork".into());
        }
        let previous = SelectObject(dc, bitmap);
        if previous.is_null() || previous as isize == -1 {
            DeleteObject(bitmap);
            DeleteDC(dc);
            return Err("Cannot select label artwork surface".into());
        }
        let result = Self {
            dc,
            bitmap,
            previous,
            bits,
        };
        if PatBlt(dc, 0, 0, width, height, WHITENESS) == 0 {
            return Err("Cannot clear label artwork surface".into());
        }
        Ok(result)
    }
}
pub(super) unsafe fn capture(
    existing: HWND,
    width_twips: u32,
) -> Result<crate::LabelSource, String> {
    let range = crate::rich_edit::selection(existing);
    if range.0 >= range.1 {
        return Err("Select document content first".into());
    }
    let rtf = crate::rich_edit::extract_selection(existing, true)?;
    if crate::rich_edit::selection(existing) != range {
        return Err("The document selection changed".into());
    }
    render(GetParent(existing), &rtf, width_twips)
}
unsafe fn render(parent: HWND, rtf: &[u8], width_twips: u32) -> Result<crate::LabelSource, String> {
    if rtf.is_empty() || rtf.len() > 32 * 1024 * 1024 || !(720..=288000).contains(&width_twips) {
        return Err("Selected label content exceeds layout limits".into());
    }
    let edit = Editor(CreateWindowExW(
        0,
        crate::wide("RICHEDIT50W").as_ptr(),
        null(),
        WS_CHILD | ES_MULTILINE as u32,
        0,
        0,
        1,
        1,
        parent,
        null_mut(),
        null_mut(),
        null(),
    ));
    if edit.0.is_null() {
        return Err("Cannot create the label selection layout".into());
    }
    crate::rich_edit::load(edit.0, rtf)?;
    #[repr(C)]
    struct Length {
        flags: u32,
        codepage: u32,
    }
    let request = Length {
        flags: 2 | 8,
        codepage: 1200,
    };
    let length = SendMessageW(edit.0, WM_USER + 95, &request as *const _ as usize, 0);
    if !(1..=1_000_000).contains(&length) {
        return Err("Select up to one million content characters".into());
    }
    let screen = GetDC(parent);
    if screen.is_null() {
        return Err("Cannot access the label layout device".into());
    }
    struct Screen(HWND, HDC);
    impl Drop for Screen {
        fn drop(&mut self) {
            unsafe {
                ReleaseDC(self.0, self.1);
            }
        }
    }
    let screen = Screen(parent, screen);
    let (dx, dy) = (
        GetDeviceCaps(screen.1, LOGPIXELSX as i32),
        GetDeviceCaps(screen.1, LOGPIXELSY as i32),
    );
    if dx <= 0 || dy <= 0 {
        return Err("Invalid label layout resolution".into());
    }
    let height_twips = 288000;
    let frame = RECT {
        left: 0,
        top: 0,
        right: (u64::from(width_twips) * 2540 / 1440) as i32,
        bottom: (i64::from(height_twips) * 2540 / 1440) as i32,
    };
    let mut recording = MetaDC(CreateEnhMetaFileW(screen.1, null(), &frame, null()));
    if recording.0.is_null() {
        return Err("Cannot capture the selected content layout".into());
    }
    let page = RECT {
        left: 0,
        top: 0,
        right: width_twips as i32,
        bottom: height_twips,
    };
    let mut format = FormatRange {
        dc: recording.0,
        target: screen.1,
        rect: page,
        page,
        range: CharRange { min: 0, max: -1 },
    };
    let next = SendMessageW(edit.0, EM_FORMATRANGE, 1, &mut format as *mut _ as isize);
    crate::rich_print::release(edit.0);
    if next < length {
        return Err("The selected content is too large for a label".into());
    }
    GdiFlush();
    let file = Metafile(CloseEnhMetaFile(recording.0));
    recording.0 = null_mut();
    if file.0.is_null() {
        return Err("Cannot finish selected content capture".into());
    }
    let mut header: ENHMETAHEADER = zeroed();
    if GetEnhMetaFileHeader(file.0, size_of::<ENHMETAHEADER>() as u32, &mut header) == 0 {
        return Err("Cannot measure the selected content".into());
    }
    if header.nBytes > 64 * 1024 * 1024 {
        return Err("Selected artwork exceeds 64 MB".into());
    }
    let logical_width = (f64::from(width_twips) * f64::from(dx) / 1440.).ceil();
    let logical_height = f64::from(height_twips) * f64::from(dy) / 1440.;
    let ink_width = ((i64::from(header.rclBounds.right) + 2) as f64).clamp(1., logical_width);
    let ink_height = ((i64::from(header.rclBounds.bottom) + 2) as f64).clamp(1., logical_height);
    if header.rclBounds.right <= 0 || header.rclBounds.bottom <= 0 {
        return Err("The selected content has no visible artwork".into());
    }
    let scale = (300. / f64::from(dx))
        .min(300. / f64::from(dy))
        .min((16_000_000. / (ink_width * ink_height)).sqrt())
        .min(8192. / ink_width)
        .min(8192. / ink_height);
    let width = (ink_width * scale).floor().max(1.) as i32;
    let height = (ink_height * scale).floor().max(1.) as i32;
    if i64::from(width) * i64::from(height) > 16_000_000 {
        return Err("Selected label artwork exceeds sixteen million pixels".into());
    }
    let surface = Surface::new(screen.1, width, height)?;
    let target = RECT {
        left: 0,
        top: 0,
        right: (logical_width * scale).round() as i32,
        bottom: (logical_height * scale).round() as i32,
    };
    if PlayEnhMetaFile(surface.dc, file.0, &target) == 0 {
        return Err("Cannot render the selected content".into());
    }
    GdiFlush();
    let pixels = std::slice::from_raw_parts(
        surface.bits.cast::<u8>(),
        width as usize * height as usize * 4,
    );
    let mut rgba = Vec::with_capacity(pixels.len());
    for pixel in pixels.chunks_exact(4) {
        rgba.extend_from_slice(&[pixel[2], pixel[1], pixel[0], 255]);
    }
    let bytes = label42::bitmap_artwork_pdf(
        width as u32,
        height as u32,
        &rgba,
        (
            f64::from(width) / scale / f64::from(dx) * 72.,
            f64::from(height) / scale / f64::from(dy) * 72.,
        ),
    )?;
    Ok(crate::LabelSource::PdfArtwork {
        bytes,
        region: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::ImageEncoder;
    use std::sync::{atomic::AtomicBool, Arc};
    use windows_sys::Win32::System::{Com::*, LibraryLoader::*};
    #[test]
    fn selected_mixed_content_is_tight_colored_and_does_not_touch_the_live_editor() {
        unsafe {
            let initialized = CoInitializeEx(null(), COINIT_APARTMENTTHREADED as u32) >= 0;
            struct Com(bool);
            impl Drop for Com {
                fn drop(&mut self) {
                    if self.0 {
                        unsafe {
                            CoUninitialize();
                        }
                    }
                }
            }
            let _com = Com(initialized);
            let library = LoadLibraryExW(
                crate::wide("msftedit.dll").as_ptr(),
                null_mut(),
                LOAD_LIBRARY_SEARCH_SYSTEM32,
            );
            assert!(!library.is_null());
            struct Library(HMODULE);
            impl Drop for Library {
                fn drop(&mut self) {
                    unsafe {
                        FreeLibrary(self.0);
                    }
                }
            }
            let _library = Library(library);
            let parent = CreateWindowExW(
                0,
                crate::wide("STATIC").as_ptr(),
                null(),
                WS_POPUP,
                0,
                0,
                800,
                600,
                null_mut(),
                null_mut(),
                null_mut(),
                null(),
            );
            assert!(!parent.is_null());
            struct Parent(HWND);
            impl Drop for Parent {
                fn drop(&mut self) {
                    unsafe {
                        DestroyWindow(self.0);
                    }
                }
            }
            let _parent = Parent(parent);
            let existing = Editor(CreateWindowExW(
                0,
                crate::wide("RICHEDIT50W").as_ptr(),
                null(),
                WS_CHILD | ES_MULTILINE as u32,
                0,
                0,
                800,
                600,
                parent,
                null_mut(),
                null_mut(),
                null(),
            ));
            assert!(!existing.0.is_null());
            let pixels = (0..100 * 50)
                .flat_map(|i| {
                    if i % 100 < 50 {
                        [255, 0, 0, 255]
                    } else {
                        [0, 0, 255, 255]
                    }
                })
                .collect::<Vec<_>>();
            let mut png = Vec::new();
            image::codecs::png::PngEncoder::new(&mut png)
                .write_image(&pixels, 100, 50, image::ExtendedColorType::Rgba8)
                .unwrap();
            let hex = png.iter().map(|v| format!("{v:02x}")).collect::<String>();
            let rtf=format!("{{\\rtf1\\ansi PRIVATE-BEFORE\\par\\b\\fs40 SELECTED \\b0{{\\pict\\pngblip\\picw100\\pich50\\picwgoal2400\\pichgoal1200 {hex}}}\\par PRIVATE-AFTER\\par}}");
            crate::rich_edit::load(existing.0, rtf.as_bytes()).unwrap();
            let start = crate::rich_edit::find_next(existing.0, "SELECTED", 0)
                .unwrap()
                .unwrap()
                .0;
            let end = crate::rich_edit::find_next(existing.0, "PRIVATE-AFTER", 0)
                .unwrap()
                .unwrap()
                .0;
            crate::rich_edit::set_selection(existing.0, (start, end));
            let before = crate::rich_edit::extract(existing.0).unwrap();
            let undo = SendMessageW(existing.0, 0xC6, 0, 0);
            let selected = crate::rich_edit::extract_selection(existing.0, true).unwrap();
            assert!(!selected.windows(7).any(|v| v == b"PRIVATE"));
            let crate::LabelSource::PdfArtwork { bytes, .. } = capture(existing.0, 9360).unwrap()
            else {
                panic!("Expected standalone PDF artwork")
            };
            assert_eq!(crate::rich_edit::extract(existing.0).unwrap(), before);
            assert_eq!(crate::rich_edit::selection(existing.0), (start, end));
            assert_eq!(SendMessageW(existing.0, 0xC6, 0, 0), undo);
            let doc =
                crate::pdfium::Document::open(Arc::from(bytes.clone()), &AtomicBool::new(false))
                    .unwrap();
            let (w, h) = doc.dimensions(0).unwrap();
            assert!(
                w > 100. && w < 468. && h > 30. && h < 250.,
                "Unexpected tight selection dimensions {w}x{h}"
            );
            let (_, _, rgba) = doc.render(0, 1000, 1000).unwrap();
            let red = rgba
                .chunks_exact(4)
                .filter(|p| {
                    i16::from(p[0]) > i16::from(p[1]) + 40 && i16::from(p[0]) > i16::from(p[2]) + 40
                })
                .count();
            let blue = rgba
                .chunks_exact(4)
                .filter(|p| {
                    i16::from(p[2]) > i16::from(p[0]) + 40 && i16::from(p[2]) > i16::from(p[1]) + 40
                })
                .count();
            assert!(
                red > 500 && blue > 500,
                "Selected picture lost color: {red}, {blue}"
            );
            if let Some(output) = std::env::var_os("OFF42_AUDIT_OUTPUT") {
                let output = std::path::PathBuf::from(output);
                std::fs::create_dir_all(&output).unwrap();
                std::fs::write(output.join("doc-selected-mixed-artwork.pdf"), bytes).unwrap();
            }
        }
    }
}
