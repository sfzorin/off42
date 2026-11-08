//! Text formatting belongs to the native projection, including table cells and
//! insertion-point attributes. The host commits selected changes through the app's
//! existing guarded projection adapter, once per user action.
use crate::{CommandState, FontChoice, RichFormatCommand};
use std::mem::{size_of, zeroed};
use windows_sys::Win32::{Foundation::HWND, UI::WindowsAndMessaging::SendMessageW};

const GET: u32 = 1024 + 58;
const SET: u32 = 1024 + 68;
const BOLD: u32 = 1;
const ITALIC: u32 = 2;
const UNDERLINE: u32 = 4;
const COLOR: u32 = 0x4000_0000;
const FACE: u32 = 0x2000_0000;
const SIZE: u32 = 0x8000_0000;

#[repr(C)]
#[derive(Clone, Copy)]
pub(super) struct CharacterFormat {
    size: u32,
    mask: u32,
    effects: u32,
    height: i32,
    offset: i32,
    color: u32,
    charset: u8,
    pitch: u8,
    face: [u16; 32],
}
#[repr(C)]
pub(super) struct ParagraphFormat {
    size: u32,
    mask: u32,
    numbering: u16,
    effects: u16,
    start: i32,
    right: i32,
    offset: i32,
    alignment: u16,
    tab_count: i16,
    tabs: [i32; 32],
    space_before: i32,
    space_after: i32,
    line_spacing: i32,
    style: i16,
    line_rule: u8,
    outline_level: u8,
    shading_weight: u16,
    shading_style: u16,
    numbering_start: u16,
    numbering_style: u16,
    numbering_tab: u16,
    border_space: u16,
    border_width: u16,
    borders: u16,
}
pub(super) enum Change {
    Character(CharacterFormat),
    Paragraph(ParagraphFormat),
}
impl Change {
    pub(super) fn affects_document(&self, selection: (usize, usize)) -> bool {
        matches!(self, Self::Paragraph(_)) || selection.0 != selection.1
    }
}
fn alignment(command: RichFormatCommand) -> Option<u16> {
    Some(match command {
        RichFormatCommand::AlignLeft => 1,
        RichFormatCommand::AlignRight => 2,
        RichFormatCommand::AlignCenter => 3,
        RichFormatCommand::Justify => 4,
        _ => return None,
    })
}

unsafe fn current(edit: HWND) -> CharacterFormat {
    let mut format: CharacterFormat = zeroed();
    format.size = size_of::<CharacterFormat>() as u32;
    SendMessageW(edit, GET, 1, &mut format as *mut _ as isize);
    format
}

fn effect(command: RichFormatCommand) -> u32 {
    match command {
        RichFormatCommand::Bold => BOLD,
        RichFormatCommand::Italic => ITALIC,
        RichFormatCommand::Underline => UNDERLINE,
        _ => 0,
    }
}

pub(super) unsafe fn state(edit: HWND, command: RichFormatCommand, editable: bool) -> CommandState {
    if let Some(value) = alignment(command) {
        let mut paragraph: ParagraphFormat = zeroed();
        paragraph.size = size_of::<ParagraphFormat>() as u32;
        SendMessageW(edit, 1024 + 61, 0, &mut paragraph as *mut _ as isize);
        return CommandState {
            enabled: editable,
            checked: paragraph.mask & 8 != 0 && paragraph.alignment == value,
        };
    }
    let format = current(edit);
    let flag = effect(command);
    CommandState {
        enabled: editable,
        checked: flag != 0 && format.mask & flag != 0 && format.effects & flag != 0,
    }
}

fn choice(format: &CharacterFormat) -> FontChoice {
    let end = format.face.iter().position(|&v| v == 0).unwrap_or(32);
    FontChoice {
        family: String::from_utf16_lossy(&format.face[..end]),
        points: if format.height > 0 {
            f64::from(format.height) / 20.
        } else {
            12.
        },
        bold: format.effects & BOLD != 0,
        italic: format.effects & ITALIC != 0,
        underline: format.effects & UNDERLINE != 0,
        color: if format.effects & COLOR != 0 {
            [0, 0, 0]
        } else {
            [
                format.color as u8,
                (format.color >> 8) as u8,
                (format.color >> 16) as u8,
            ]
        },
    }
}

pub(super) unsafe fn prepare(
    owner: HWND,
    edit: HWND,
    command: RichFormatCommand,
) -> Result<Option<Change>, String> {
    if let Some(value) = alignment(command) {
        let mut paragraph: ParagraphFormat = zeroed();
        paragraph.size = size_of::<ParagraphFormat>() as u32;
        paragraph.mask = 8;
        paragraph.alignment = value;
        return Ok(Some(Change::Paragraph(paragraph)));
    }
    let old = current(edit);
    let mut next: CharacterFormat = zeroed();
    next.size = size_of::<CharacterFormat>() as u32;
    let flag = effect(command);
    if flag != 0 {
        next.mask = flag;
        next.effects = if old.mask & flag != 0 && old.effects & flag != 0 {
            0
        } else {
            flag
        };
    } else {
        let initial = choice(&old);
        let color = match command {
            RichFormatCommand::Font => {
                let Some(font) = crate::dialogs::font_with_initial(owner, Some(&initial)) else {
                    return Ok(None);
                };
                if font.family.is_empty()
                    || font.family.contains('\0')
                    || font.family.encode_utf16().count() >= 32
                    || !font.points.is_finite()
                    || !(0.5..=16383.5).contains(&font.points)
                {
                    return Err("Invalid font selection".into());
                }
                next.mask = FACE | SIZE | BOLD | ITALIC | UNDERLINE;
                next.height = (font.points * 20.).round() as i32;
                for (target, unit) in next.face.iter_mut().zip(font.family.encode_utf16()) {
                    *target = unit;
                }
                next.effects = u32::from(font.bold) * BOLD
                    | u32::from(font.italic) * ITALIC
                    | u32::from(font.underline) * UNDERLINE;
                font.color
            }
            RichFormatCommand::Color => {
                let Some(color) = crate::dialogs::color(owner, initial.color) else {
                    return Ok(None);
                };
                color
            }
            _ => unreachable!(),
        };
        next.mask |= COLOR;
        next.color = u32::from(color[0]) | u32::from(color[1]) << 8 | u32::from(color[2]) << 16;
    }
    Ok(Some(Change::Character(next)))
}

pub(super) unsafe fn apply(edit: HWND, format: &Change) -> Result<(), String> {
    let result = match format {
        Change::Character(format) => SendMessageW(edit, SET, 1, format as *const _ as isize),
        Change::Paragraph(format) => SendMessageW(edit, 1024 + 71, 0, format as *const _ as isize),
    };
    if result == 0 {
        Err("Windows could not apply the text format".into())
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows_sys::Win32::{System::LibraryLoader::LoadLibraryW, UI::WindowsAndMessaging::*};
    #[test]
    fn native_insertion_format_does_not_restyle_existing_text_and_mixed_selection_toggles_on() {
        unsafe {
            assert_eq!(size_of::<CharacterFormat>(), 92);
            LoadLibraryW(crate::wide("Msftedit.dll").as_ptr());
            let edit = CreateWindowExW(
                0,
                crate::wide("RICHEDIT50W").as_ptr(),
                std::ptr::null(),
                WS_POPUP | ES_MULTILINE as u32,
                0,
                0,
                400,
                200,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null(),
            );
            assert!(!edit.is_null());
            crate::rich_edit::load(edit, br"{\rtf1\ansi Plain \b Bold\b0\par}").unwrap();
            crate::rich_edit::set_selection(edit, (0, 0));
            let before = crate::rich_edit::extract(edit).unwrap();
            let format = prepare(std::ptr::null_mut(), edit, RichFormatCommand::Italic)
                .unwrap()
                .unwrap();
            apply(edit, &format).unwrap();
            assert!(state(edit, RichFormatCommand::Italic, true).checked);
            assert_eq!(crate::rich_edit::extract(edit).unwrap(), before);
            SendMessageW(edit, 0xC2, 0, crate::wide("X").as_ptr() as isize); // EM_REPLACESEL
            crate::rich_edit::set_selection(edit, (0, 1));
            assert!(state(edit, RichFormatCommand::Italic, true).checked);
            crate::rich_edit::set_selection(edit, (1, 6));
            assert!(!state(edit, RichFormatCommand::Italic, true).checked);
            crate::rich_edit::set_selection(edit, (1, 11));
            assert!(!state(edit, RichFormatCommand::Bold, true).checked);
            let next = prepare(std::ptr::null_mut(), edit, RichFormatCommand::Bold)
                .unwrap()
                .unwrap();
            apply(edit, &next).unwrap();
            assert!(state(edit, RichFormatCommand::Bold, true).checked);
            assert!(!state(edit, RichFormatCommand::Bold, false).enabled);
            assert_eq!(size_of::<ParagraphFormat>(), 188);
            crate::rich_edit::load(edit, br"{\rtf1\ansi First\par Second\par}").unwrap();
            crate::rich_edit::set_selection(edit, (6, 6));
            let before = crate::rich_edit::extract(edit).unwrap();
            let right = prepare(std::ptr::null_mut(), edit, RichFormatCommand::AlignRight)
                .unwrap()
                .unwrap();
            assert!(right.affects_document((6, 6)));
            apply(edit, &right).unwrap();
            assert_ne!(crate::rich_edit::extract(edit).unwrap(), before);
            assert!(state(edit, RichFormatCommand::AlignRight, true).checked);
            crate::rich_edit::set_selection(edit, (0, 5));
            assert!(state(edit, RichFormatCommand::AlignLeft, true).checked);
            crate::rich_edit::set_selection(edit, (0, 12));
            assert!(!state(edit, RichFormatCommand::AlignRight, true).checked);
            let center = prepare(std::ptr::null_mut(), edit, RichFormatCommand::AlignCenter)
                .unwrap()
                .unwrap();
            apply(edit, &center).unwrap();
            assert!(state(edit, RichFormatCommand::AlignCenter, true).checked);
            let justify = prepare(std::ptr::null_mut(), edit, RichFormatCommand::Justify)
                .unwrap()
                .unwrap();
            apply(edit, &justify).unwrap();
            assert!(state(edit, RichFormatCommand::Justify, true).checked);
            DestroyWindow(edit);
        }
    }
}
