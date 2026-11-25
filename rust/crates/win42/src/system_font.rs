//! Read the actual selected Windows TrueType program; embedding policy belongs to the PDF font layer.
pub fn program(family: &str, bold: bool) -> Result<Vec<u8>, String> {
    if family.trim().is_empty() || family.encode_utf16().count() >= 32 || family.contains('\0') {
        return Err("Invalid font family".into());
    }
    #[cfg(not(windows))]
    {
        let _ = bold;
        Err("System font selection is available on Windows".into())
    }
    #[cfg(windows)]
    unsafe {
        use std::ptr::null_mut;
        use windows_sys::Win32::Graphics::Gdi::*;
        let dc = CreateCompatibleDC(null_mut());
        if dc.is_null() {
            return Err("Could not open a font device".into());
        }
        struct Device(HDC);
        impl Drop for Device {
            fn drop(&mut self) {
                unsafe {
                    DeleteDC(self.0);
                }
            }
        }
        let _device = Device(dc);
        let font = CreateFontW(
            -1000,
            0,
            0,
            0,
            if bold { 700 } else { 400 },
            0,
            0,
            0,
            DEFAULT_CHARSET as u32,
            OUT_TT_ONLY_PRECIS as u32,
            CLIP_DEFAULT_PRECIS as u32,
            DEFAULT_QUALITY as u32,
            DEFAULT_PITCH as u32,
            crate::wide(family).as_ptr(),
        );
        if font.is_null() {
            return Err("The selected font is unavailable".into());
        }
        struct Font(HGDIOBJ);
        impl Drop for Font {
            fn drop(&mut self) {
                unsafe {
                    DeleteObject(self.0);
                }
            }
        }
        let _font = Font(font);
        let previous = SelectObject(dc, font);
        if previous.is_null() || previous as isize == -1 {
            return Err("Could not select the font".into());
        }
        struct Restore(HDC, HGDIOBJ);
        impl Drop for Restore {
            fn drop(&mut self) {
                unsafe {
                    SelectObject(self.0, self.1);
                }
            }
        }
        let _restore = Restore(dc, previous);
        let mut actual = [0u16; 64];
        let n = GetTextFaceW(dc, actual.len() as i32, actual.as_mut_ptr());
        if n <= 0 {
            return Err("Could not identify the selected font".into());
        }
        let n = actual.iter().position(|&v| v == 0).unwrap_or(actual.len());
        if !String::from_utf16_lossy(&actual[..n]).eq_ignore_ascii_case(family) {
            return Err("The selected font is unavailable; choose an installed font".into());
        }
        let size = GetFontData(dc, 0, 0, null_mut(), 0);
        if size == GDI_ERROR as u32 || size < 12 || size > 16 * 1024 * 1024 {
            return Err("Could not read a TrueType font up to 16 MB".into());
        }
        let mut bytes = vec![0; size as usize];
        if GetFontData(dc, 0, 0, bytes.as_mut_ptr().cast(), size) != size {
            return Err("Could not read the selected font".into());
        }
        Ok(bytes)
    }
}
#[cfg(all(windows, test))]
mod tests {
    use super::*;
    #[test]
    fn selected_windows_families_and_bold_programs_are_real_and_distinct() {
        for family in ["Arial", "Times New Roman", "Courier New"] {
            let regular = program(family, false).unwrap();
            let bold = program(family, true).unwrap();
            assert!(regular.len() > 10000 && bold.len() > 10000);
            assert!(
                regular != bold,
                "Regular and bold font programs must differ for {family}"
            );
        }
        assert!(program("off42 nonexistent font", false).is_err());
    }
}
