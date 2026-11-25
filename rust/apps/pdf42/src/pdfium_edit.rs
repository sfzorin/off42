//! Disposable PDFium sessions for editing existing page text objects.
use std::{
    ffi::{c_char, c_void, CString},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
};
type H = *mut c_void;
#[link(name = "kernel32")]
extern "system" {
    fn LoadLibraryExW(name: *const u16, file: H, flags: u32) -> H;
    fn GetProcAddress(module: H, name: *const c_char) -> *mut c_void;
    fn FreeLibrary(module: H) -> i32;
}
#[repr(C)]
struct FileWrite {
    version: i32,
    write: unsafe extern "system" fn(*mut FileWrite, *const c_void, u32) -> i32,
    data: Vec<u8>,
    failed: bool,
}
unsafe extern "system" fn write_block(this: *mut FileWrite, data: *const c_void, len: u32) -> i32 {
    let state = &mut *this;
    if len == 0 {
        return 1;
    }
    let Some(size) = state.data.len().checked_add(len as usize) else {
        state.failed = true;
        return 0;
    };
    if size > 256 * 1024 * 1024 || data.is_null() {
        state.failed = true;
        return 0;
    }
    state
        .data
        .extend_from_slice(std::slice::from_raw_parts(data.cast::<u8>(), len as usize));
    1
}
#[repr(C)]
#[derive(Default)]
struct Matrix {
    a: f32,
    b: f32,
    c: f32,
    d: f32,
    e: f32,
    f: f32,
}
struct Api {
    lib: H,
    destroy: unsafe extern "system" fn(),
    close_doc: unsafe extern "system" fn(H),
    close_page: unsafe extern "system" fn(H),
    close_text: unsafe extern "system" fn(H),
    load: unsafe extern "system" fn(*const c_void, usize, *const c_char) -> H,
    page: unsafe extern "system" fn(H, i32) -> H,
    text: unsafe extern "system" fn(H) -> H,
    char_at: unsafe extern "system" fn(H, f64, f64, f64, f64) -> i32,
    text_object: unsafe extern "system" fn(H, i32) -> H,
    count: unsafe extern "system" fn(H) -> i32,
    object: unsafe extern "system" fn(H, i32) -> H,
    object_type: unsafe extern "system" fn(H) -> i32,
    bounds: unsafe extern "system" fn(H, *mut f32, *mut f32, *mut f32, *mut f32) -> i32,
    get_text: unsafe extern "system" fn(H, H, *mut u16, u32) -> u32,
    set_text: unsafe extern "system" fn(H, *const u16) -> i32,
    get_matrix: unsafe extern "system" fn(H, *mut Matrix) -> i32,
    set_matrix: unsafe extern "system" fn(H, *const Matrix) -> i32,
    font_size: unsafe extern "system" fn(H, *mut f32) -> i32,
    get_fill: unsafe extern "system" fn(H, *mut u32, *mut u32, *mut u32, *mut u32) -> i32,
    set_fill: unsafe extern "system" fn(H, u32, u32, u32, u32) -> i32,
    text_mode: unsafe extern "system" fn(H) -> i32,
    has_transparency: unsafe extern "system" fn(H) -> i32,
    marks: unsafe extern "system" fn(H) -> i32,
    clip: unsafe extern "system" fn(H) -> H,
    clip_count: unsafe extern "system" fn(H) -> i32,
    load_font: unsafe extern "system" fn(H, *const u8, u32, i32, i32) -> H,
    close_font: unsafe extern "system" fn(H),
    create_text: unsafe extern "system" fn(H, H, f32) -> H,
    insert_at: unsafe extern "system" fn(H, H, usize) -> i32,
    remove: unsafe extern "system" fn(H, H) -> i32,
    destroy_obj: unsafe extern "system" fn(H),
    generate: unsafe extern "system" fn(H) -> i32,
    save: unsafe extern "system" fn(H, *mut FileWrite, u32) -> i32,
}
impl Drop for Api {
    fn drop(&mut self) {
        unsafe {
            (self.destroy)();
            FreeLibrary(self.lib);
        }
    }
}
impl Api {
    unsafe fn open() -> Result<Self, String> {
        let name: Vec<u16> = "pdfium.dll\0".encode_utf16().collect();
        let lib = LoadLibraryExW(name.as_ptr(), std::ptr::null_mut(), 0x200 | 0x800);
        if lib.is_null() {
            return Err("PDF text editing requires the bundled x64 pdfium.dll".into());
        }
        macro_rules! sym {
            ($n:literal,$t:ty) => {{
                let n = CString::new($n).unwrap();
                let p = GetProcAddress(lib, n.as_ptr());
                if p.is_null() {
                    FreeLibrary(lib);
                    return Err(format!("pdfium.dll is missing {}", $n));
                }
                std::mem::transmute::<*mut c_void, $t>(p)
            }};
        }
        let init = sym!("FPDF_InitLibrary", unsafe extern "system" fn());
        let api = Self {
            lib,
            destroy: sym!("FPDF_DestroyLibrary", unsafe extern "system" fn()),
            close_doc: sym!("FPDF_CloseDocument", unsafe extern "system" fn(H)),
            close_page: sym!("FPDF_ClosePage", unsafe extern "system" fn(H)),
            close_text: sym!("FPDFText_ClosePage", unsafe extern "system" fn(H)),
            load: sym!(
                "FPDF_LoadMemDocument64",
                unsafe extern "system" fn(*const c_void, usize, *const c_char) -> H
            ),
            page: sym!("FPDF_LoadPage", unsafe extern "system" fn(H, i32) -> H),
            text: sym!("FPDFText_LoadPage", unsafe extern "system" fn(H) -> H),
            char_at: sym!(
                "FPDFText_GetCharIndexAtPos",
                unsafe extern "system" fn(H, f64, f64, f64, f64) -> i32
            ),
            text_object: sym!(
                "FPDFText_GetTextObject",
                unsafe extern "system" fn(H, i32) -> H
            ),
            count: sym!("FPDFPage_CountObjects", unsafe extern "system" fn(H) -> i32),
            object: sym!("FPDFPage_GetObject", unsafe extern "system" fn(H, i32) -> H),
            object_type: sym!("FPDFPageObj_GetType", unsafe extern "system" fn(H) -> i32),
            bounds: sym!(
                "FPDFPageObj_GetBounds",
                unsafe extern "system" fn(H, *mut f32, *mut f32, *mut f32, *mut f32) -> i32
            ),
            get_text: sym!(
                "FPDFTextObj_GetText",
                unsafe extern "system" fn(H, H, *mut u16, u32) -> u32
            ),
            set_text: sym!(
                "FPDFText_SetText",
                unsafe extern "system" fn(H, *const u16) -> i32
            ),
            get_matrix: sym!(
                "FPDFPageObj_GetMatrix",
                unsafe extern "system" fn(H, *mut Matrix) -> i32
            ),
            set_matrix: sym!(
                "FPDFPageObj_SetMatrix",
                unsafe extern "system" fn(H, *const Matrix) -> i32
            ),
            font_size: sym!(
                "FPDFTextObj_GetFontSize",
                unsafe extern "system" fn(H, *mut f32) -> i32
            ),
            get_fill: sym!(
                "FPDFPageObj_GetFillColor",
                unsafe extern "system" fn(H, *mut u32, *mut u32, *mut u32, *mut u32) -> i32
            ),
            set_fill: sym!(
                "FPDFPageObj_SetFillColor",
                unsafe extern "system" fn(H, u32, u32, u32, u32) -> i32
            ),
            text_mode: sym!(
                "FPDFTextObj_GetTextRenderMode",
                unsafe extern "system" fn(H) -> i32
            ),
            has_transparency: sym!(
                "FPDFPageObj_HasTransparency",
                unsafe extern "system" fn(H) -> i32
            ),
            marks: sym!(
                "FPDFPageObj_CountMarks",
                unsafe extern "system" fn(H) -> i32
            ),
            clip: sym!("FPDFPageObj_GetClipPath", unsafe extern "system" fn(H) -> H),
            clip_count: sym!(
                "FPDFClipPath_CountPaths",
                unsafe extern "system" fn(H) -> i32
            ),
            load_font: sym!(
                "FPDFText_LoadFont",
                unsafe extern "system" fn(H, *const u8, u32, i32, i32) -> H
            ),
            close_font: sym!("FPDFFont_Close", unsafe extern "system" fn(H)),
            create_text: sym!(
                "FPDFPageObj_CreateTextObj",
                unsafe extern "system" fn(H, H, f32) -> H
            ),
            insert_at: sym!(
                "FPDFPage_InsertObjectAtIndex",
                unsafe extern "system" fn(H, H, usize) -> i32
            ),
            remove: sym!(
                "FPDFPage_RemoveObject",
                unsafe extern "system" fn(H, H) -> i32
            ),
            destroy_obj: sym!("FPDFPageObj_Destroy", unsafe extern "system" fn(H)),
            generate: sym!(
                "FPDFPage_GenerateContent",
                unsafe extern "system" fn(H) -> i32
            ),
            save: sym!(
                "FPDF_SaveAsCopy",
                unsafe extern "system" fn(H, *mut FileWrite, u32) -> i32
            ),
        };
        init();
        Ok(api)
    }
}
struct Session<'a> {
    api: &'a Api,
    doc: H,
    page: H,
    text: H,
}
impl Drop for Session<'_> {
    fn drop(&mut self) {
        unsafe {
            if !self.text.is_null() {
                (self.api.close_text)(self.text)
            }
            if !self.page.is_null() {
                (self.api.close_page)(self.page)
            }
            if !self.doc.is_null() {
                (self.api.close_doc)(self.doc)
            }
        }
    }
}
impl<'a> Session<'a> {
    unsafe fn open(api: &'a Api, bytes: &[u8], page_index: usize) -> Result<Self, String> {
        let doc = (api.load)(bytes.as_ptr().cast(), bytes.len(), std::ptr::null());
        if doc.is_null() {
            return Err("PDFium could not open PDF snapshot".into());
        }
        let mut s = Self {
            api,
            doc,
            page: std::ptr::null_mut(),
            text: std::ptr::null_mut(),
        };
        s.page = (api.page)(doc, page_index as i32);
        if s.page.is_null() {
            return Err("PDFium could not open the page".into());
        }
        s.text = (api.text)(s.page);
        if s.text.is_null() {
            return Err("PDFium could not read page text".into());
        }
        Ok(s)
    }
    unsafe fn object_text(&self, obj: H) -> Result<String, String> {
        let len = (self.api.get_text)(obj, self.text, std::ptr::null_mut(), 0);
        if len < 2 || len > 1_000_000 || len % 2 != 0 {
            return Err("Text object cannot be decoded safely".into());
        }
        let mut buf = vec![0u16; len as usize / 2];
        if (self.api.get_text)(obj, self.text, buf.as_mut_ptr(), len) != len {
            return Err("Could not read text object".into());
        }
        if buf.last() == Some(&0) {
            buf.pop();
        }
        String::from_utf16(&buf).map_err(|_| "Invalid text object Unicode".into())
    }
}
#[derive(Debug)]
struct ObjectEvidence {
    kind: i32,
    bounds: Option<[f32; 4]>,
    text: Option<String>,
}
unsafe fn evidence(s: &Session<'_>, index: i32) -> Result<ObjectEvidence, String> {
    let object = (s.api.object)(s.page, index);
    if object.is_null() {
        return Err("PDF page object is unavailable".into());
    }
    let kind = (s.api.object_type)(object);
    let (mut l, mut b, mut r, mut t) = (0f32, 0f32, 0f32, 0f32);
    let bounds = if (s.api.bounds)(object, &mut l, &mut b, &mut r, &mut t) != 0 {
        Some([l, b, r, t])
    } else {
        None
    };
    let text = if kind == 1 {
        Some(s.object_text(object)?)
    } else {
        None
    };
    Ok(ObjectEvidence { kind, bounds, text })
}
fn same_bounds(a: Option<[f32; 4]>, b: Option<[f32; 4]>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => a.iter().zip(b).all(|(a, b)| (*a - b).abs() < 0.01),
        _ => false,
    }
}
#[derive(Clone)]
pub struct Selected {
    pub page: usize,
    pub index: i32,
    pub text: String,
}
pub fn inspect(bytes: &[u8], page: usize, point: [f64; 2]) -> Result<Selected, String> {
    let _guard = crate::search::PDFIUM_LOCK
        .lock()
        .map_err(|_| "PDFium lock failed")?;
    unsafe {
        let api = Api::open()?;
        let s = Session::open(&api, bytes, page)?;
        let char_index = (api.char_at)(s.text, point[0], point[1], 3., 3.);
        if char_index < 0 {
            return Err("No text object at this point".into());
        }
        let hit = (api.text_object)(s.text, char_index);
        if hit.is_null() {
            return Err("Text object unavailable".into());
        }
        let count = (api.count)(s.page);
        let index = (0..count)
            .find(|i| (api.object)(s.page, *i) == hit)
            .ok_or("Text is inside a form or unsupported nested object")?;
        let text = s.object_text(hit)?;
        Ok(Selected { page, index, text })
    }
}
pub fn edit_async(
    bytes: Vec<u8>,
    selected: Selected,
    replacement: String,
    fallback: bool,
    cancel: Arc<AtomicBool>,
) -> mpsc::Receiver<Result<Vec<u8>, String>> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let result = (|| {
            if replacement.contains('\0') {
                return Err("Text cannot contain NUL".into());
            }
            let _guard = crate::search::PDFIUM_LOCK
                .lock()
                .map_err(|_| "PDFium lock failed")?;
            if cancel.load(Ordering::Relaxed) {
                return Err("Text edit cancelled".into());
            }
            unsafe {
                let api = Api::open()?;
                let mut s = Session::open(&api, &bytes, selected.page)?;
                let count = (api.count)(s.page);
                if !(0..=100_000).contains(&count) {
                    return Err("Page object count exceeds editing limit".into());
                }
                let before = (0..count)
                    .map(|i| evidence(&s, i))
                    .collect::<Result<Vec<_>, _>>()?;
                let obj = (api.object)(s.page, selected.index);
                if obj.is_null() || s.object_text(obj)? != selected.text {
                    return Err("Text object changed before editing".into());
                }
                struct FontGuard<'a> {
                    api: &'a Api,
                    font: H,
                }
                impl Drop for FontGuard<'_> {
                    fn drop(&mut self) {
                        if !self.font.is_null() {
                            unsafe { (self.api.close_font)(self.font) }
                        }
                    }
                }
                let mut font_guard = FontGuard {
                    api: &api,
                    font: std::ptr::null_mut(),
                };
                if fallback && !replacement.is_empty() {
                    suite42_pdf_text::PdfFont::validate(&replacement)?;
                    let mut matrix = Matrix::default();
                    let mut size = 0f32;
                    let (mut r, mut g, mut b, mut a) = (0, 0, 0, 0);
                    let marks = (api.marks)(obj);
                    if marks != 0 {
                        return Err(format!(
                            "Font replacement cannot preserve marked text (mark count {marks})"
                        ));
                    }
                    let clip = (api.clip)(obj);
                    if clip.is_null() {
                        return Err(
                            "Font replacement cannot inspect the text clipping state".into()
                        );
                    }
                    let clip_paths = (api.clip_count)(clip);
                    // PDFium 8125 returns -1 for a valid page object's clip handle
                    // when its underlying clip path has no reference. Zero can
                    // mean a text-only clip, so it must not be treated as empty.
                    if clip_paths != -1 {
                        return Err(format!("Font replacement cannot preserve clipped text (path count {clip_paths})"));
                    }
                    let mode = (api.text_mode)(obj);
                    if mode != 0 {
                        return Err(format!(
                            "Font replacement requires fill text (render mode {mode})"
                        ));
                    }
                    let transparent = (api.has_transparency)(obj);
                    if transparent != 0 {
                        return Err(format!(
                            "Font replacement cannot preserve transparency (state {transparent})"
                        ));
                    }
                    if (api.get_matrix)(obj, &mut matrix) == 0 {
                        return Err("Font replacement cannot inspect the text matrix".into());
                    }
                    if (api.font_size)(obj, &mut size) == 0 {
                        return Err("Font replacement cannot inspect the text font size".into());
                    }
                    if (api.get_fill)(obj, &mut r, &mut g, &mut b, &mut a) == 0 {
                        return Err("Font replacement cannot inspect the text fill color".into());
                    }
                    if a != 255 {
                        return Err(format!("Font replacement requires opaque text (alpha {a})"));
                    }
                    let data =
                        include_bytes!("../../../crates/doc42-layout-probe/fixtures/NotoSans.ttf");
                    font_guard.font =
                        (api.load_font)(s.doc, data.as_ptr(), data.len() as u32, 2, 1);
                    if font_guard.font.is_null() {
                        return Err("PDFium could not load the bundled Noto font".into());
                    }
                    let replacement_object = (api.create_text)(s.doc, font_guard.font, size);
                    if replacement_object.is_null() {
                        return Err("PDFium could not create replacement text".into());
                    }
                    let wide: Vec<u16> = replacement.encode_utf16().chain(Some(0)).collect();
                    if (api.set_text)(replacement_object, wide.as_ptr()) == 0
                        || (api.set_matrix)(replacement_object, &matrix) == 0
                        || (api.set_fill)(replacement_object, r, g, b, a) == 0
                    {
                        (api.destroy_obj)(replacement_object);
                        return Err("Could not prepare replacement text object".into());
                    }
                    (api.close_text)(s.text);
                    s.text = std::ptr::null_mut();
                    if (api.remove)(s.page, obj) == 0 {
                        (api.destroy_obj)(replacement_object);
                        return Err("Could not remove original text object".into());
                    }
                    (api.destroy_obj)(obj);
                    if (api.insert_at)(s.page, replacement_object, selected.index as usize) == 0 {
                        (api.destroy_obj)(replacement_object);
                        return Err("Could not insert replacement text object".into());
                    }
                } else if replacement.is_empty() {
                    (api.close_text)(s.text);
                    s.text = std::ptr::null_mut();
                    if (api.remove)(s.page, obj) == 0 {
                        return Err("PDFium could not remove the text object".into());
                    }
                    (api.destroy_obj)(obj);
                } else {
                    let wide: Vec<u16> = replacement.encode_utf16().chain(Some(0)).collect();
                    if (api.set_text)(obj, wide.as_ptr()) == 0 {
                        return Err("PDFium could not apply text in the existing font".into());
                    }
                }
                if cancel.load(Ordering::Relaxed) {
                    return Err("Text edit cancelled".into());
                }
                if !s.text.is_null() {
                    (api.close_text)(s.text);
                    s.text = std::ptr::null_mut();
                }
                if (api.generate)(s.page) == 0 {
                    return Err("PDFium could not regenerate page content".into());
                }
                let mut writer = FileWrite {
                    version: 1,
                    write: write_block,
                    data: Vec::new(),
                    failed: false,
                };
                if (api.save)(s.doc, &mut writer, 2) == 0 || writer.failed {
                    return Err("PDFium could not save the edited PDF".into());
                }
                drop(font_guard);
                drop(s);
                if cancel.load(Ordering::Relaxed) {
                    return Err("Text edit cancelled".into());
                }
                let check = Session::open(&api, &writer.data, selected.page)?;
                let expected_count = count - if replacement.is_empty() { 1 } else { 0 };
                if (api.count)(check.page) != expected_count {
                    return Err("Edited PDF failed page object verification".into());
                }
                for (i, original) in before.iter().enumerate() {
                    if i == selected.index as usize {
                        continue;
                    }
                    let next = i as i32
                        - if replacement.is_empty() && i > selected.index as usize {
                            1
                        } else {
                            0
                        };
                    let current = evidence(&check, next)?;
                    if original.kind != current.kind
                        || original.text != current.text
                        || !same_bounds(original.bounds, current.bounds)
                    {
                        return Err(
                            "Edited PDF changed another page object; edit was not applied".into(),
                        );
                    }
                }
                if !replacement.is_empty() {
                    let obj = (api.object)(check.page, selected.index);
                    if obj.is_null() || check.object_text(obj)? != replacement {
                        return Err("Existing font cannot represent the requested text; edit was not applied".into());
                    }
                }
                Ok(writer.data)
            }
        })();
        let _ = tx.send(result);
    });
    rx
}
pub fn inspect_async(
    bytes: Vec<u8>,
    page: usize,
    point: [f64; 2],
    cancel: Arc<AtomicBool>,
) -> mpsc::Receiver<Result<Selected, String>> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let result = if cancel.load(Ordering::Relaxed) {
            Err("Text selection cancelled".into())
        } else {
            inspect(&bytes, page, point)
        };
        let _ = tx.send(result);
    });
    rx
}

#[cfg(test)]
mod tests {
    use super::*;
    use lopdf::{dictionary, Document, Object, Stream};
    use std::time::Duration;
    fn fixture() -> Vec<u8> {
        fixture_with(
            b"BT /F1 18 Tf 20 40 Td (Hello) Tj ET BT /F1 18 Tf 20 100 Td (Neighbor) Tj ET",
            false,
        )
    }
    fn fixture_with(content: &[u8], transparent: bool) -> Vec<u8> {
        let mut d = Document::with_version("1.7");
        let root = d.new_object_id();
        let font=d.add_object(dictionary!{"Type"=>"Font","Subtype"=>"Type1","BaseFont"=>"Helvetica","Encoding"=>"WinAnsiEncoding"});
        let stream = d.add_object(Stream::new(dictionary! {}, content.to_vec()));
        let mut resources = dictionary! {"Font"=>dictionary!{"F1"=>font}};
        if transparent {
            resources.set(
                "ExtGState",
                dictionary! {"GS1"=>dictionary!{"Type"=>"ExtGState","ca"=>0.5}},
            );
        }
        let page=d.add_object(dictionary!{"Type"=>"Page","Parent"=>root,"MediaBox"=>vec![0.into(),0.into(),200.into(),200.into()],"Resources"=>resources,"Contents"=>stream});
        d.objects.insert(
            root,
            Object::Dictionary(
                dictionary! {"Type"=>"Pages","Kids"=>vec![Object::Reference(page)],"Count"=>1},
            ),
        );
        let catalog = d.add_object(dictionary! {"Type"=>"Catalog","Pages"=>root});
        d.trailer.set("Root", catalog);
        let mut bytes = Vec::new();
        d.save_to(&mut bytes).unwrap();
        bytes
    }
    fn select_index(bytes: &[u8], index: i32) -> Selected {
        let _guard = crate::search::PDFIUM_LOCK.lock().unwrap();
        unsafe {
            let api = Api::open().unwrap();
            let session = Session::open(&api, bytes, 0).unwrap();
            let object = (api.object)(session.page, index);
            assert!(!object.is_null());
            Selected {
                page: 0,
                index,
                text: session.object_text(object).unwrap(),
            }
        }
    }
    #[test]
    fn font_replacement_refuses_marked_clipped_and_transparent_objects() {
        let cases: [(&[u8], bool, i32, &str); 5] = [
            (
                b"/Span BMC BT /F1 18 Tf 20 40 Td (Hello) Tj ET EMC",
                false,
                0,
                "marked text",
            ),
            (
                b"q 0 0 30 45 re W n BT /F1 18 Tf 20 40 Td (Hello) Tj ET Q",
                false,
                0,
                "clipped text (path count 1)",
            ),
            (
                b"BT /F1 18 Tf 20 40 Td 7 Tr (H) Tj ET BT /F1 18 Tf 20 40 Td 0 Tr (Hello) Tj ET",
                false,
                1,
                "clipped text (path count 0)",
            ),
            (
                b"/GS1 gs BT /F1 18 Tf 20 40 Td (Hello) Tj ET",
                true,
                0,
                "transparency",
            ),
            (
                b"BT /F1 18 Tf 20 40 Td 1 Tr (Hello) Tj ET",
                false,
                0,
                "render mode 1",
            ),
        ];
        for (content, transparent, index, expected) in cases {
            let source = fixture_with(content, transparent);
            let selected = select_index(&source, index);
            let original_text = selected.text.clone();
            let error = edit_async(
                source.clone(),
                selected,
                "Привет".into(),
                true,
                Arc::new(AtomicBool::new(false)),
            )
            .recv_timeout(Duration::from_secs(30))
            .unwrap()
            .unwrap_err();
            assert!(
                error.contains(expected),
                "expected {expected:?}, got {error:?}"
            );
            assert_eq!(select_index(&source, index).text, original_text);
        }
    }
    #[test]
    fn pdfium_replaces_and_deletes_real_text_objects() {
        let source = fixture();
        let selected = inspect(&source, 0, [25., 45.]).unwrap();
        assert_eq!(selected.text, "Hello");
        let edited = edit_async(
            source.clone(),
            selected,
            "World".into(),
            false,
            Arc::new(AtomicBool::new(false)),
        )
        .recv_timeout(Duration::from_secs(30))
        .unwrap()
        .unwrap();
        assert_ne!(edited, source);
        assert_eq!(inspect(&source, 0, [25., 45.]).unwrap().text, "Hello");
        assert_eq!(inspect(&edited, 0, [25., 105.]).unwrap().text, "Neighbor");
        let changed = inspect(&edited, 0, [25., 45.]).unwrap();
        assert_eq!(changed.text, "World");
        let deleted = edit_async(
            edited,
            changed,
            String::new(),
            false,
            Arc::new(AtomicBool::new(false)),
        )
        .recv_timeout(Duration::from_secs(30))
        .unwrap()
        .unwrap();
        assert!(inspect(&deleted, 0, [25., 45.]).is_err());
        assert_eq!(inspect(&deleted, 0, [25., 105.]).unwrap().text, "Neighbor");
        let _ = pdf42_core::PdfDocument::from_edited_snapshot(&deleted, 0).unwrap();
        let selected = inspect(&source, 0, [25., 45.]).unwrap();
        let cyrillic = edit_async(
            source,
            selected,
            "Привет".into(),
            true,
            Arc::new(AtomicBool::new(false)),
        )
        .recv_timeout(Duration::from_secs(30))
        .unwrap()
        .unwrap();
        assert_eq!(inspect(&cyrillic, 0, [25., 45.]).unwrap().text, "Привет");
    }
}
