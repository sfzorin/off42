//! Serialized disposable PDFium documents for preview and vector printing.
//! The input buffer and DLL remain alive until every document/page handle is closed.
use std::{
    ffi::{c_char, c_void, CString},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex, MutexGuard,
    },
};
use windows_sys::Win32::{
    Foundation::{FreeLibrary, HMODULE},
    Graphics::Gdi::HDC,
    System::LibraryLoader::{
        GetProcAddress, LoadLibraryExW, LOAD_LIBRARY_SEARCH_APPLICATION_DIR,
        LOAD_LIBRARY_SEARCH_SYSTEM32,
    },
};
type H = *mut c_void;
pub static LOCK: Mutex<()> = Mutex::new(());
struct Library(HMODULE);
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
    load: unsafe extern "system" fn(*const c_void, usize, *const c_char) -> H,
    close: unsafe extern "system" fn(H),
    count: unsafe extern "system" fn(H) -> i32,
    page: unsafe extern "system" fn(H, i32) -> H,
    close_page: unsafe extern "system" fn(H),
    width: unsafe extern "system" fn(H) -> f64,
    height: unsafe extern "system" fn(H) -> f64,
    bitmap: unsafe extern "system" fn(i32, i32, i32) -> H,
    destroy_bitmap: unsafe extern "system" fn(H),
    fill: unsafe extern "system" fn(H, i32, i32, i32, i32, u32),
    render: unsafe extern "system" fn(H, H, i32, i32, i32, i32, i32, i32),
    buffer: unsafe extern "system" fn(H) -> *mut c_void,
    stride: unsafe extern "system" fn(H) -> i32,
    print: unsafe extern "system" fn(HDC, H, i32, i32, i32, i32, i32, i32),
    flatten: unsafe extern "system" fn(H, i32) -> i32,
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
        let name = crate::wide("pdfium.dll");
        let library = Library(unsafe {
            LoadLibraryExW(
                name.as_ptr(),
                std::ptr::null_mut(),
                LOAD_LIBRARY_SEARCH_APPLICATION_DIR | LOAD_LIBRARY_SEARCH_SYSTEM32,
            )
        });
        if library.0.is_null() {
            return Err("Keep the bundled pdfium.dll beside the OFF42 executables".into());
        }
        macro_rules! sym {
            ($name:literal,$ty:ty) => {{
                let name = CString::new($name).unwrap();
                let pointer = unsafe { GetProcAddress(library.0, name.as_ptr().cast()) }
                    .ok_or(concat!("PDFium is missing ", $name))?;
                unsafe { std::mem::transmute::<unsafe extern "system" fn() -> isize, $ty>(pointer) }
            }};
        }
        let init = sym!("FPDF_InitLibrary", unsafe extern "system" fn());
        let api = Self {
            destroy: sym!("FPDF_DestroyLibrary", unsafe extern "system" fn()),
            load: sym!(
                "FPDF_LoadMemDocument64",
                unsafe extern "system" fn(*const c_void, usize, *const c_char) -> H
            ),
            close: sym!("FPDF_CloseDocument", unsafe extern "system" fn(H)),
            count: sym!("FPDF_GetPageCount", unsafe extern "system" fn(H) -> i32),
            page: sym!("FPDF_LoadPage", unsafe extern "system" fn(H, i32) -> H),
            close_page: sym!("FPDF_ClosePage", unsafe extern "system" fn(H)),
            width: sym!("FPDF_GetPageWidth", unsafe extern "system" fn(H) -> f64),
            height: sym!("FPDF_GetPageHeight", unsafe extern "system" fn(H) -> f64),
            bitmap: sym!(
                "FPDFBitmap_Create",
                unsafe extern "system" fn(i32, i32, i32) -> H
            ),
            destroy_bitmap: sym!("FPDFBitmap_Destroy", unsafe extern "system" fn(H)),
            fill: sym!(
                "FPDFBitmap_FillRect",
                unsafe extern "system" fn(H, i32, i32, i32, i32, u32)
            ),
            render: sym!(
                "FPDF_RenderPageBitmap",
                unsafe extern "system" fn(H, H, i32, i32, i32, i32, i32, i32)
            ),
            buffer: sym!(
                "FPDFBitmap_GetBuffer",
                unsafe extern "system" fn(H) -> *mut c_void
            ),
            stride: sym!("FPDFBitmap_GetStride", unsafe extern "system" fn(H) -> i32),
            print: sym!(
                "FPDF_RenderPage",
                unsafe extern "system" fn(HDC, H, i32, i32, i32, i32, i32, i32)
            ),
            flatten: sym!("FPDFPage_Flatten", unsafe extern "system" fn(H, i32) -> i32),
            _library: library,
        };
        unsafe {
            init();
        }
        Ok(api)
    }
}
pub struct Document {
    api: Api,
    handle: H,
    _lease: MutexGuard<'static, ()>,
    _bytes: Arc<[u8]>,
}
impl Drop for Document {
    fn drop(&mut self) {
        unsafe {
            (self.api.close)(self.handle);
        }
    }
}
struct Page<'a> {
    doc: &'a Document,
    handle: H,
}
impl Drop for Page<'_> {
    fn drop(&mut self) {
        unsafe {
            (self.doc.api.close_page)(self.handle);
        }
    }
}
impl Document {
    pub fn open(bytes: Arc<[u8]>, cancel: &AtomicBool) -> Result<Self, String> {
        if bytes.is_empty() || bytes.len() > 256 * 1024 * 1024 {
            return Err("PDF preview/print source exceeds 256 MB".into());
        }
        let start = std::time::Instant::now();
        let lease = loop {
            if cancel.load(Ordering::Relaxed) {
                return Err("PDF operation cancelled".into());
            }
            match LOCK.try_lock() {
                Ok(lease) => break lease,
                Err(std::sync::TryLockError::Poisoned(_)) => {
                    return Err("PDF engine is unavailable".into())
                }
                Err(std::sync::TryLockError::WouldBlock) => {}
            }
            if start.elapsed() > std::time::Duration::from_secs(60) {
                return Err("PDF engine is busy; retry the operation".into());
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        };
        let api = unsafe { Api::open()? };
        let handle = unsafe { (api.load)(bytes.as_ptr().cast(), bytes.len(), std::ptr::null()) };
        if handle.is_null() {
            return Err("Cannot open the PDF preview/print snapshot".into());
        }
        let doc = Self {
            api,
            handle,
            _lease: lease,
            _bytes: bytes,
        };
        let count = doc.page_count();
        if count == 0 || count > 1000 {
            return Err("PDF preview/print is limited to 1,000 pages".into());
        }
        Ok(doc)
    }
    pub fn page_count(&self) -> usize {
        unsafe { (self.api.count)(self.handle).max(0) as usize }
    }
    fn page(&self, index: usize) -> Result<Page<'_>, String> {
        if index >= self.page_count() {
            return Err("PDF page is out of range".into());
        }
        let handle = unsafe { (self.api.page)(self.handle, index as i32) };
        if handle.is_null() {
            return Err("Cannot load the PDF page".into());
        }
        Ok(Page { doc: self, handle })
    }
    pub fn dimensions(&self, index: usize) -> Result<(f64, f64), String> {
        let page = self.page(index)?;
        let (w, h) = unsafe {
            (
                (self.api.width)(page.handle),
                (self.api.height)(page.handle),
            )
        };
        if ![w, h]
            .iter()
            .all(|v| v.is_finite() && *v > 0. && *v <= 100_000.)
        {
            return Err("Invalid PDF page dimensions".into());
        }
        Ok((w, h))
    }
    pub fn render(
        &self,
        index: usize,
        max_width: u32,
        max_height: u32,
    ) -> Result<(u32, u32, Vec<u8>), String> {
        let (w, h) = self.dimensions(index)?;
        if max_width == 0 || max_height == 0 {
            return Err("Invalid preview size".into());
        }
        let scale = (f64::from(max_width) / w).min(f64::from(max_height) / h);
        let width = (w * scale).round().max(1.) as u32;
        let height = (h * scale).round().max(1.) as u32;
        if u64::from(width) * u64::from(height) > 4_000_000 {
            return Err("PDF preview exceeds four million pixels".into());
        }
        let page = self.page(index)?;
        let api = &self.api;
        let bitmap = unsafe { (api.bitmap)(width as i32, height as i32, 1) };
        if bitmap.is_null() {
            return Err("Cannot allocate the PDF preview".into());
        }
        struct Bitmap<'a>(&'a Api, H);
        impl Drop for Bitmap<'_> {
            fn drop(&mut self) {
                unsafe {
                    (self.0.destroy_bitmap)(self.1);
                }
            }
        }
        let bitmap = Bitmap(api, bitmap);
        unsafe {
            (api.fill)(bitmap.1, 0, 0, width as i32, height as i32, 0xFFFFFFFF);
            (api.render)(
                bitmap.1,
                page.handle,
                0,
                0,
                width as i32,
                height as i32,
                0,
                1,
            );
        }
        let stride = unsafe { (api.stride)(bitmap.1) };
        let buffer = unsafe { (api.buffer)(bitmap.1) };
        if stride < width as i32 * 4
            || stride as u64 * u64::from(height) > 20 * 1024 * 1024
            || buffer.is_null()
        {
            return Err("Invalid PDF preview buffer".into());
        }
        let data = unsafe {
            std::slice::from_raw_parts(buffer.cast::<u8>(), stride as usize * height as usize)
        };
        let mut rgba = Vec::with_capacity(width as usize * height as usize * 4);
        for row in data.chunks_exact(stride as usize) {
            for p in row[..width as usize * 4].chunks_exact(4) {
                rgba.extend_from_slice(&[p[2], p[1], p[0], 255]);
            }
        }
        Ok((width, height, rgba))
    }
    /// Printer coordinates are device pixels at physical size, already adjusted for hard margins/calibration.
    pub unsafe fn print_page(&self, index: usize, dc: HDC, rect: [i32; 4]) -> Result<(), String> {
        if dc.is_null() || rect[2] <= 0 || rect[3] <= 0 {
            return Err("Invalid PDF print destination".into());
        }
        let page = self.page(index)?;
        if !matches!(unsafe { (self.api.flatten)(page.handle, 1) }, 1 | 2) {
            return Err("Cannot flatten the printable PDF appearances".into());
        }
        unsafe {
            (self.api.print)(
                dc,
                page.handle,
                rect[0],
                rect[1],
                rect[2],
                rect[3],
                0,
                0x801,
            );
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    pub(super) fn fixture() -> Arc<[u8]> {
        let body = b"q 1 0 0 rg 20 30 40 40 re f Q\n";
        let objects = vec![
            b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
            b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Contents 4 0 R >>".to_vec(),
            [
                format!("<< /Length {} >>\nstream\n", body.len()).into_bytes(),
                body.to_vec(),
                b"endstream".to_vec(),
            ]
            .concat(),
        ];
        let mut data = b"%PDF-1.4\n".to_vec();
        let mut offsets = vec![0];
        for (i, object) in objects.iter().enumerate() {
            offsets.push(data.len());
            data.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
            data.extend_from_slice(object);
            data.extend_from_slice(b"\nendobj\n");
        }
        let xref = data.len();
        data.extend_from_slice(
            format!("xref\n0 {}\n0000000000 65535 f \n", offsets.len()).as_bytes(),
        );
        for offset in offsets.iter().skip(1) {
            data.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
        }
        data.extend_from_slice(
            format!(
                "trailer\n<< /Root 1 0 R /Size {} >>\nstartxref\n{xref}\n%%EOF\n",
                offsets.len()
            )
            .as_bytes(),
        );
        data.into()
    }
    #[test]
    fn bounded_preview_renders_expected_pixels_and_releases_the_shared_engine() {
        let bytes = fixture();
        let cancel = AtomicBool::new(false);
        let doc = Document::open(bytes.clone(), &cancel).unwrap();
        assert_eq!(doc.page_count(), 1);
        assert_eq!(doc.dimensions(0).unwrap(), (200., 200.));
        let (w, h, pixels) = doc.render(0, 400, 500).unwrap();
        assert_eq!((w, h), (400, 400));
        assert_eq!(
            &pixels[((300 * w + 100) * 4) as usize..((300 * w + 100) * 4 + 4) as usize],
            &[255, 0, 0, 255]
        );
        assert!(doc.render(0, 5000, 5000).is_err());
        assert!(doc.render(1, 400, 400).is_err());
        assert!(Document::open(bytes.clone(), &AtomicBool::new(true))
            .err()
            .unwrap()
            .contains("cancelled"));
        drop(doc);
        assert!(Document::open(bytes, &cancel).is_ok());
    }
}

#[cfg(test)]
mod print_tests {
    use super::*;
    use windows_sys::Win32::{Foundation::*, Graphics::Gdi::*};
    #[derive(Default)]
    struct Records {
        paths: usize,
        bitmaps: usize,
    }
    unsafe extern "system" fn inspect(
        _: HDC,
        _: *const HANDLETABLE,
        record: *const ENHMETARECORD,
        _: i32,
        data: LPARAM,
    ) -> i32 {
        let counts = &mut *(data as *mut Records);
        match (*record).iType {
            EMR_FILLPATH | EMR_STROKEANDFILLPATH | EMR_RECTANGLE | EMR_POLYGON | EMR_POLYGON16 => {
                counts.paths += 1
            }
            EMR_STRETCHDIBITS | EMR_SETDIBITSTODEVICE | EMR_ALPHABLEND => counts.bitmaps += 1,
            _ => {}
        }
        1
    }
    #[test]
    fn opaque_pdf_paths_print_as_vectors_in_a_native_metafile() {
        let bytes = super::tests::fixture();
        let doc = Document::open(bytes, &AtomicBool::new(false)).unwrap();
        unsafe {
            let frame = RECT {
                left: 0,
                top: 0,
                right: 7056,
                bottom: 7056,
            };
            let description = crate::wide("OFF42\0PDF vector print probe\0");
            let dc = CreateEnhMetaFileW(
                std::ptr::null_mut(),
                std::ptr::null(),
                &frame,
                description.as_ptr(),
            );
            assert!(!dc.is_null());
            struct MetaDC(HDC);
            impl Drop for MetaDC {
                fn drop(&mut self) {
                    if !self.0.is_null() {
                        unsafe {
                            let file = CloseEnhMetaFile(self.0);
                            if !file.is_null() {
                                DeleteEnhMetaFile(file);
                            }
                        }
                    }
                }
            }
            let mut owned = MetaDC(dc);
            let dpi_x = GetDeviceCaps(dc, LOGPIXELSX as i32);
            let dpi_y = GetDeviceCaps(dc, LOGPIXELSY as i32);
            assert!(dpi_x > 0 && dpi_y > 0);
            doc.print_page(
                0,
                dc,
                [
                    0,
                    0,
                    (200. * f64::from(dpi_x) / 72.).round() as i32,
                    (200. * f64::from(dpi_y) / 72.).round() as i32,
                ],
            )
            .unwrap();
            let file = CloseEnhMetaFile(dc);
            owned.0 = std::ptr::null_mut();
            assert!(!file.is_null());
            struct File(HENHMETAFILE);
            impl Drop for File {
                fn drop(&mut self) {
                    unsafe {
                        DeleteEnhMetaFile(self.0);
                    }
                }
            }
            let file = File(file);
            let mut header: ENHMETAHEADER = std::mem::zeroed();
            assert_eq!(
                GetEnhMetaFileHeader(
                    file.0,
                    std::mem::size_of::<ENHMETAHEADER>() as u32,
                    &mut header
                ),
                std::mem::size_of::<ENHMETAHEADER>() as u32
            );
            assert_eq!(
                (header.rclFrame.right, header.rclFrame.bottom),
                (7056, 7056)
            );
            let mut records = Records::default();
            assert_ne!(
                EnumEnhMetaFile(
                    std::ptr::null_mut(),
                    file.0,
                    Some(inspect),
                    (&mut records as *mut Records).cast(),
                    &frame
                ),
                0
            );
            assert!(
                records.paths > 0,
                "No native vector paths in PDF print output"
            );
            assert_eq!(
                records.bitmaps, 0,
                "Opaque PDF paths were rasterized before reaching the print device"
            );
            if let Some(output) = std::env::var_os("OFF42_AUDIT_OUTPUT") {
                let output = std::path::PathBuf::from(output);
                std::fs::create_dir_all(&output).unwrap();
                let size = GetEnhMetaFileBits(file.0, 0, std::ptr::null_mut());
                assert!(size > 0 && size < 8 * 1024 * 1024);
                let mut data = vec![0u8; size as usize];
                assert_eq!(GetEnhMetaFileBits(file.0, size, data.as_mut_ptr()), size);
                std::fs::write(output.join("pdf-vector-print.emf"), data).unwrap();
            }
        }
    }
}
