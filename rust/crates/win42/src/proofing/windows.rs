use super::*;
use crate::{wide, InputRequest};
use ::windows::{
    core::{PCWSTR, PWSTR},
    Win32::{
        Globalization::{
            ISpellChecker, ISpellCheckerFactory, SpellCheckerFactory, CORRECTIVE_ACTION_DELETE,
            CORRECTIVE_ACTION_GET_SUGGESTIONS, CORRECTIVE_ACTION_REPLACE,
        },
        System::Com::{CoCreateInstance, IEnumString, CLSCTX_INPROC_SERVER},
    },
};
use std::{
    cell::RefCell,
    ptr::{null, null_mut},
};
use windows_sys::Win32::{
    Foundation::HWND,
    System::Com::CoTaskMemFree,
    UI::{
        Input::KeyboardAndMouse::SetFocus,
        WindowsAndMessaging::{MessageBoxW, SendMessageW, MB_ICONINFORMATION, MB_OK, WM_USER},
    },
};
thread_local! {static LANGUAGE:RefCell<Option<String>>=const{RefCell::new(None)};}
unsafe fn take_string(value: PWSTR) -> Result<String, String> {
    if value.is_null() {
        return Err("Windows returned an empty spelling string".into());
    }
    let text = value.to_string().map_err(|e| e.to_string());
    CoTaskMemFree(value.0.cast());
    text
}
unsafe fn strings(items: IEnumString, limit: usize) -> Result<Vec<String>, String> {
    let mut result = Vec::new();
    for _ in 0..limit {
        let mut value = [PWSTR::null()];
        let mut fetched = 0;
        let hr = items.Next(&mut value, Some(&mut fetched));
        if hr.is_err() {
            if !value[0].is_null() {
                CoTaskMemFree(value[0].0.cast());
            }
            return Err(hr.to_string());
        }
        if fetched == 0 {
            break;
        }
        let text = take_string(value[0])?;
        if text.len() <= 1024 && !text.contains('\0') {
            result.push(text)
        }
    }
    Ok(result)
}
struct Checker {
    checker: ISpellChecker,
}
struct Issue {
    start: usize,
    end: usize,
    word: String,
    suggestions: Vec<String>,
}
impl Checker {
    unsafe fn factory() -> Result<ISpellCheckerFactory, String> {
        CoCreateInstance(&SpellCheckerFactory, None, CLSCTX_INPROC_SERVER)
            .map_err(|e| e.to_string())
    }
    unsafe fn languages(factory: &ISpellCheckerFactory) -> Result<Vec<String>, String> {
        let mut languages = strings(
            factory.SupportedLanguages().map_err(|e| e.to_string())?,
            256,
        )?;
        languages.retain(|language| crate::localization::matching(language).is_some());
        languages.sort();
        languages.dedup();
        Ok(languages)
    }
    unsafe fn new(factory: &ISpellCheckerFactory, tag: &str) -> Result<Self, String> {
        let tag = wide(tag);
        Ok(Self {
            checker: factory
                .CreateSpellChecker(PCWSTR(tag.as_ptr()))
                .map_err(|e| e.to_string())?,
        })
    }
    unsafe fn next(
        &self,
        text: &[u16],
        mut cursor: usize,
        end: usize,
        work: &mut usize,
    ) -> Result<Option<Issue>, String> {
        while cursor < end {
            let stop = chunk_end(text, cursor, end);
            if stop <= cursor {
                return Err("Spelling check reached its text limit".into());
            }
            *work = work
                .checked_add(stop - cursor)
                .ok_or("Spelling check reached its text limit")?;
            if *work > 8_000_000 {
                return Err("Spelling check reached its text limit".into());
            }
            let mut input = check_text(&text[cursor..stop]);
            input.push(0);
            let errors = self
                .checker
                .ComprehensiveCheck(PCWSTR(input.as_ptr()))
                .map_err(|e| e.to_string())?;
            for _ in 0..4096 {
                let mut error = None;
                let hr = errors.Next(&mut error);
                hr.ok().map_err(|e| e.to_string())?;
                let Some(error) = error else { break };
                let offset = error.StartIndex().map_err(|e| e.to_string())? as usize;
                let length = error.Length().map_err(|e| e.to_string())? as usize;
                if offset.checked_add(length).is_none_or(|v| v > stop - cursor) {
                    return Err("Invalid spelling range from Windows".into());
                }
                let action = error.CorrectiveAction().map_err(|e| e.to_string())?;
                if !matches!(
                    action,
                    CORRECTIVE_ACTION_GET_SUGGESTIONS
                        | CORRECTIVE_ACTION_REPLACE
                        | CORRECTIVE_ACTION_DELETE
                ) {
                    continue;
                }
                let start = cursor + offset;
                let word = checked_word(text, start, length)?;
                let suggestions = if action == CORRECTIVE_ACTION_DELETE {
                    vec![String::new()]
                } else if action == CORRECTIVE_ACTION_REPLACE {
                    vec![take_string(
                        error.Replacement().map_err(|e| e.to_string())?,
                    )?]
                } else {
                    let word = wide(&word);
                    strings(
                        self.checker
                            .Suggest(PCWSTR(word.as_ptr()))
                            .map_err(|e| e.to_string())?,
                        12,
                    )?
                };
                let suggestions = suggestions
                    .into_iter()
                    .filter(|s| {
                        s.encode_utf16().count() <= 1024 && !s.chars().any(char::is_control)
                    })
                    .collect();
                return Ok(Some(Issue {
                    start,
                    end: start + length,
                    word,
                    suggestions,
                }));
            }
            cursor = stop;
        }
        Ok(None)
    }
    unsafe fn ignore(&self, word: &str) -> Result<(), String> {
        let word = wide(word);
        self.checker
            .Ignore(PCWSTR(word.as_ptr()))
            .map_err(|e| e.to_string())
    }
}
#[repr(C)]
struct TextLength {
    flags: u32,
    codepage: u32,
}
#[repr(C)]
struct GetText {
    bytes: u32,
    flags: u32,
    codepage: u32,
    default_char: *const u8,
    used_default: *mut i32,
}
pub(crate) unsafe fn raw_text(edit: HWND) -> Result<Vec<u16>, String> {
    let length = TextLength {
        flags: 2 | 8,
        codepage: 1200,
    };
    let n = SendMessageW(edit, WM_USER + 95, &length as *const _ as usize, 0);
    if !(0..=1_000_000).contains(&n) {
        return Err("Select up to 1,000,000 characters for spelling".into());
    }
    let mut out = vec![0; n as usize + 1];
    let request = GetText {
        bytes: (out.len() * 2) as u32,
        flags: 4,
        codepage: 1200,
        default_char: null(),
        used_default: null_mut(),
    };
    let copied = SendMessageW(
        edit,
        WM_USER + 94,
        &request as *const _ as usize,
        out.as_mut_ptr() as isize,
    );
    if copied < 0 || copied as usize > n as usize {
        return Err("Could not read the spelling text".into());
    }
    out.truncate(copied as usize);
    String::from_utf16(&out).map_err(|_| "Invalid spelling text")?;
    Ok(out)
}
/// The owner commits through its existing guarded Rich Edit adapter; this module owns no document state.
pub(crate) unsafe fn show<F>(owner: HWND, edit: HWND, mut replace: F) -> Result<(), String>
where
    F: FnMut((usize, usize), &str, &[u16]) -> Result<(), String>,
{
    let result = (|| {
        let factory = Checker::factory()?;
        let languages = Checker::languages(&factory)?;
        if languages.is_empty() {
            return Err("No supported spelling languages are installed in Windows".into());
        }
        let preferred = LANGUAGE
            .with(|s| s.borrow().clone())
            .filter(|s| languages.contains(s))
            .or_else(|| {
                let current = crate::localization::current();
                languages
                    .iter()
                    .find(|s| {
                        crate::localization::matching(s).is_some_and(|v| v.code == current.code)
                    })
                    .cloned()
            })
            .unwrap_or_else(|| languages[0].clone());
        let request = InputRequest {
            title: "Spelling",
            fields: vec![("Language", preferred)],
        };
        let Some(fields) =
            crate::prompt::show_choices(owner, &request, false, false, &[(0, languages.clone())])
        else {
            return Ok(());
        };
        let tag = fields
            .first()
            .filter(|tag| languages.contains(tag))
            .ok_or("Choose an installed spelling language")?;
        LANGUAGE.with(|s| *s.borrow_mut() = Some(tag.clone()));
        let checker = Checker::new(&factory, tag)?;
        let mut expected = raw_text(edit)?;
        let selection = crate::rich_edit::selection(edit);
        let mut cursor = if selection.0 != selection.1 {
            selection.0
        } else {
            0
        };
        let mut end = if selection.0 != selection.1 {
            selection.1.min(expected.len())
        } else {
            expected.len()
        };
        let mut work = 0;
        for _ in 0..1000 {
            let Some(issue) = checker.next(&expected, cursor, end, &mut work)? else {
                MessageBoxW(
                    owner,
                    wide(&crate::localization::text("Spelling check complete.")).as_ptr(),
                    wide(&crate::localization::text("Spelling")).as_ptr(),
                    MB_OK | MB_ICONINFORMATION,
                );
                return Ok(());
            };
            crate::rich_edit::set_selection(edit, (issue.start, issue.end));
            SendMessageW(edit, 0xB7, 0, 0);
            let replacement = issue
                .suggestions
                .first()
                .cloned()
                .unwrap_or_else(|| issue.word.clone());
            let request = InputRequest {
                title: "Spelling",
                fields: vec![
                    ("Text", issue.word.clone()),
                    ("Replace with:", replacement),
                    ("Action", "Replace".into()),
                ],
            };
            let choices = vec![
                (1, issue.suggestions),
                (2, vec!["Replace".into(), "Skip".into(), "Skip All".into()]),
            ];
            let Some(fields) =
                crate::prompt::show_readonly_choices(owner, &request, &choices, &[0])
            else {
                return Ok(());
            };
            if raw_text(edit)? != expected {
                return Err("The document changed; check spelling again".into());
            }
            match fields.get(2).map(String::as_str) {
                Some("Skip") => cursor = issue.end,
                Some("Skip All") => {
                    checker.ignore(&issue.word)?;
                    cursor = issue.end
                }
                Some("Replace") => {
                    let value = fields.get(1).ok_or("Enter replacement text")?;
                    if value.encode_utf16().count() > 1024
                        || value.chars().any(|c| {
                            c.is_control() || matches!(c, '\u{fffc}' | '\u{fdd0}'..='\u{fdef}')
                        })
                    {
                        return Err("Invalid spelling replacement".into());
                    }
                    replace((issue.start, issue.end), value, &expected)?;
                    let mut after = expected.clone();
                    let insert: Vec<_> = value.encode_utf16().collect();
                    after.splice(issue.start..issue.end, insert.iter().copied());
                    if raw_text(edit)? != after {
                        return Err("The spelling replacement could not be preserved".into());
                    }
                    end = end - (issue.end - issue.start) + insert.len();
                    cursor = issue.start + insert.len();
                    expected = after;
                }
                _ => return Err("Choose Replace, Skip, or Skip All".into()),
            }
        }
        Err("Spelling check reached its text limit".into())
    })();
    SetFocus(edit);
    result
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rich_edit_raw_text_uses_actual_table_and_picture_character_positions() {
        unsafe {
            use windows_sys::Win32::{
                System::LibraryLoader::LoadLibraryW, UI::WindowsAndMessaging::*,
            };
            LoadLibraryW(wide("Msftedit.dll").as_ptr());
            let edit = CreateWindowExW(
                0,
                wide("RICHEDIT50W").as_ptr(),
                null(),
                WS_POPUP | ES_MULTILINE as u32,
                0,
                0,
                400,
                200,
                null_mut(),
                null_mut(),
                null_mut(),
                null(),
            );
            assert!(!edit.is_null());
            crate::rich_edit::load(
                edit,
                include_bytes!("../../../doc42-core/fixtures/native-rich-edit-picture.rtf"),
            )
            .unwrap();
            let text = raw_text(edit).unwrap();
            for word in ["Cell", "Other", "After", "Tail"] {
                let range = crate::rich_edit::find_next(edit, word, 0).unwrap().unwrap();
                assert_eq!(
                    checked_word(&text, range.0, range.1 - range.0).unwrap(),
                    word
                );
            }
            assert!(text.contains(&0xFFFC));
            DestroyWindow(edit);
        }
    }
    #[test]
    fn prepared_core_correction_matches_actual_image_table_projection_without_importing_native_fonts(){unsafe{
        use windows_sys::Win32::{System::LibraryLoader::LoadLibraryW,UI::WindowsAndMessaging::*};
        use doc42_core::rich::{RichDocument,Format,Block};
        let mut doc=RichDocument::decode(include_bytes!("../../../doc42-core/fixtures/native-rich-edit-picture.doc42"),Format::Project).unwrap();
        doc.transact(|s|{let Block::Paragraph(p)=&mut s.blocks[0]else{panic!()};p.runs[0].text="😀 speling world".into();p.runs[0].style.bold=true;let Block::Table(t)=&mut s.blocks[1]else{panic!()};t.rows[0][0].paragraphs[0].runs[0].text="hello speling".into();Ok(())}).unwrap();
        LoadLibraryW(wide("Msftedit.dll").as_ptr());let edit=CreateWindowExW(0,wide("RICHEDIT50W").as_ptr(),null(),WS_POPUP|ES_MULTILINE as u32,0,0,400,200,null_mut(),null_mut(),null_mut(),null());assert!(!edit.is_null());crate::rich_edit::load(edit,&doc.rich_edit_projection().unwrap()).unwrap();
        for _ in 0..2{
            let text=raw_text(edit).unwrap();let range=crate::rich_edit::find_next(edit,"speling",0).unwrap().unwrap();
            let proposal=doc.prepare_word_edit(&text,range,"speling","spelling").unwrap_or_else(|e|panic!("{e}: {:?}",String::from_utf16(&text)));
            crate::rich_edit::load(edit,proposal.projection()).unwrap();let mut expected=text;expected.splice(range.0..range.1,"spelling".encode_utf16());assert_eq!(raw_text(edit).unwrap(),expected);doc.commit_word_edit(proposal).unwrap();
        }
        let Block::Paragraph(p)=&doc.state().blocks[0]else{panic!()};assert_eq!(p.runs.len(),1);assert_eq!(p.runs[0].style.font,"Arial");assert!(p.runs[0].style.bold);assert_eq!(p.runs[0].text,"😀 spelling world");DestroyWindow(edit);
    }}
    #[test]
    fn installed_windows_dictionary_returns_real_errors_and_suggestions() {
        unsafe {
            use windows_sys::Win32::System::Com::{
                CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED,
            };
            assert!(CoInitializeEx(null(), COINIT_APARTMENTTHREADED as u32) >= 0);
            let result = (|| -> Result<(), String> {
                let factory = Checker::factory()?;
                let languages = Checker::languages(&factory)?;
                assert!(
                    languages.iter().any(|s| s == "en-US"),
                    "Installed dictionaries: {languages:?}"
                );
                let checker = Checker::new(&factory, "en-US")?;
                let text: Vec<_> = "😀 helo world".encode_utf16().collect();
                let issue = checker
                    .next(&text, 0, text.len(), &mut 0)?
                    .ok_or("Misspelling was not detected")?;
                assert_eq!(issue.word, "helo");
                assert_eq!(issue.start, 3);
                assert!(
                    issue
                        .suggestions
                        .iter()
                        .any(|s| s.eq_ignore_ascii_case("hello")),
                    "{:?}",
                    issue.suggestions
                );
                checker.ignore("helo")?;
                assert!(checker.next(&text, 0, text.len(), &mut 0)?.is_none());
                Ok(())
            })();
            CoUninitialize();
            result.unwrap();
        }
    }
}
