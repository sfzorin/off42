use crate::FontChoice;
use std::{
    mem::{size_of, zeroed},
    ptr::null_mut,
};
use windows_sys::Win32::{Foundation::HWND, Graphics::Gdi::LOGFONTW, UI::Controls::Dialogs::*};

pub unsafe fn color(owner: HWND, initial: [u8; 3]) -> Option<[u8; 3]> {
    let mut custom = [0u32; 16];
    let mut dialog: CHOOSECOLORW = zeroed();
    dialog.lStructSize = size_of::<CHOOSECOLORW>() as u32;
    dialog.hwndOwner = owner;
    dialog.lpCustColors = custom.as_mut_ptr();
    dialog.rgbResult = initial[0] as u32 | (initial[1] as u32) << 8 | (initial[2] as u32) << 16;
    dialog.Flags = CC_RGBINIT | CC_FULLOPEN;
    if ChooseColorW(&mut dialog) == 0 {
        return None;
    }
    Some([
        dialog.rgbResult as u8,
        (dialog.rgbResult >> 8) as u8,
        (dialog.rgbResult >> 16) as u8,
    ])
}
pub unsafe fn font(owner: HWND) -> Option<FontChoice> {
    font_with_initial(owner, None)
}
pub unsafe fn font_with_initial(owner: HWND, initial: Option<&FontChoice>) -> Option<FontChoice> {
    let mut logfont: LOGFONTW = zeroed();
    let mut dialog: CHOOSEFONTW = zeroed();
    dialog.lStructSize = size_of::<CHOOSEFONTW>() as u32;
    dialog.hwndOwner = owner;
    dialog.hDC = null_mut();
    dialog.lpLogFont = &mut logfont;
    dialog.Flags = CF_SCREENFONTS | CF_EFFECTS;
    if let Some(initial) = initial {
        for (target, unit) in logfont
            .lfFaceName
            .iter_mut()
            .take(31)
            .zip(initial.family.encode_utf16())
        {
            *target = unit;
        }
        let dpi = windows_sys::Win32::UI::HiDpi::GetDpiForWindow(owner).max(96);
        logfont.lfHeight = -(initial.points * f64::from(dpi) / 72.).round() as i32;
        logfont.lfWeight = if initial.bold { 700 } else { 400 };
        logfont.lfItalic = initial.italic as u8;
        logfont.lfUnderline = initial.underline as u8;
        dialog.iPointSize = (initial.points * 10.).round() as i32;
        dialog.rgbColors = u32::from(initial.color[0])
            | u32::from(initial.color[1]) << 8
            | u32::from(initial.color[2]) << 16;
        dialog.Flags |= CF_INITTOLOGFONTSTRUCT;
    }
    if ChooseFontW(&mut dialog) == 0 {
        return None;
    }
    let end = logfont
        .lfFaceName
        .iter()
        .position(|&c| c == 0)
        .unwrap_or(logfont.lfFaceName.len());
    Some(FontChoice {
        family: String::from_utf16_lossy(&logfont.lfFaceName[..end]),
        points: dialog.iPointSize as f64 / 10.0,
        bold: logfont.lfWeight >= 700,
        italic: logfont.lfItalic != 0,
        underline: logfont.lfUnderline != 0,
        color: [
            dialog.rgbColors as u8,
            (dialog.rgbColors >> 8) as u8,
            (dialog.rgbColors >> 16) as u8,
        ],
    })
}

pub unsafe fn page_setup(owner: HWND, margins: crate::PageMargins) -> Option<crate::PageMargins> {
    use windows_sys::Win32::Foundation::GlobalFree;
    let mut dialog: PAGESETUPDLGW = zeroed();
    dialog.lStructSize = size_of::<PAGESETUPDLGW>() as u32;
    dialog.hwndOwner = owner;
    dialog.Flags = PSD_MARGINS | PSD_INHUNDREDTHSOFMILLIMETERS;
    dialog.rtMargin.left = margins.left;
    dialog.rtMargin.top = margins.top;
    dialog.rtMargin.right = margins.right;
    dialog.rtMargin.bottom = margins.bottom;
    let accepted = PageSetupDlgW(&mut dialog) != 0;
    if !dialog.hDevMode.is_null() {
        GlobalFree(dialog.hDevMode);
    }
    if !dialog.hDevNames.is_null() {
        GlobalFree(dialog.hDevNames);
    }
    if !accepted {
        return None;
    }
    Some(crate::PageMargins {
        left: dialog.rtMargin.left,
        top: dialog.rtMargin.top,
        right: dialog.rtMargin.right,
        bottom: dialog.rtMargin.bottom,
        paper_width: dialog.ptPaperSize.x,
        paper_height: dialog.ptPaperSize.y,
    })
}
