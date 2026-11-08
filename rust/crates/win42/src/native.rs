use crate::{wide, App};
use std::{
    cell::{Cell, RefCell},
    ffi::OsString,
    mem::{size_of, zeroed},
    os::windows::ffi::{OsStrExt, OsStringExt},
    path::{Path, PathBuf},
    ptr::{null, null_mut},
};
use windows_sys::Win32::{
    Foundation::*,
    Graphics::Gdi::*,
    System::{
        Com::{CoInitializeEx, CoTaskMemFree, CoUninitialize, COINIT_APARTMENTTHREADED},
        LibraryLoader::{GetModuleHandleW, LoadLibraryW},
        Memory::{GlobalLock,GlobalUnlock,GlobalSize},
    },
    UI::{
        Controls::{Dialogs::*, *},
        HiDpi::*,
        Input::KeyboardAndMouse::{
            EnableWindow, IsWindowEnabled, GetFocus, GetKeyState, ReleaseCapture, SetCapture, SetFocus, VK_CONTROL,
            VK_SHIFT,
        },
        Shell::{DragAcceptFiles, DragFinish, DragQueryFileW, ShellExecuteW, DefSubclassProc, SetWindowSubclass, RemoveWindowSubclass},
        WindowsAndMessaging::*,
    },
};
const OPEN: u16 = 1;
const SAVE: u16 = 2;
// Host-only IDs stay outside the applications’ 10+ command range.
const SAVE_AS: u16 = 65507;
const EXIT: u16 = 3;
const UNDO: u16 = 4;
const REDO: u16 = 5;
const ABOUT: u16 = 6;
const FIND: u16 = 7;
const REPLACE: u16 = 8;
const NEW: u16 = 9;
const LIST: usize = 100;
const EDIT: usize = 101;
const TABLE: usize = 102;
const TOOLBAR_MORE: u16 = 65520;
const CANCEL_OPEN: u16 = 65519;
const CREATE_LABEL_SHEET: u16 = crate::CREATE_LABEL_SHEET_COMMAND;
const CREATE_BARCODE: u16 = 65517;
const IMPORT_MARK: u16 = 65516;
const INSERT_MARK: u16 = 65515;
const EXPORT_TECHNICAL: u16 = 65514;
const CONFIGURE_CALIBRATION: u16 = 65513;
const TOGGLE_CALIBRATION: u16 = 65512;
const CALIBRATION_SHEET: u16 = 65511;
const TOGGLE_PROOFING: u16 = 65510;
const INSERT_TECHNICAL: u16 = 65509;
const EDIT_TECHNICAL: u16 = 65508;
const NEW_WINDOW: u16 = 65506;
const RECOVER_DOCUMENT:u16=65505;
const CHECK_SPELLING:u16=65504;
const TOGGLE_SIDEBAR:u16=65503;
const TOGGLE_TOOLBAR:u16=65502;
const TOGGLE_STATUS:u16=65501;
const TOGGLE_RULERS:u16=65500;
const LANGUAGE_FIRST: u16 = 64000;
const RECENT_FIRST: u16 = 62000;
const CLEAR_RECENTS: u16 = RECENT_FIRST + crate::recents::LIMIT as u16;
struct AsyncOpen {
    path: PathBuf,
    phase: AsyncOpenPhase,
}
enum AsyncOpenPhase {
    Loading,
    Checking {
        expected: suite42_core::session::FileIdentity,
        result: std::sync::mpsc::Receiver<std::io::Result<suite42_core::session::FileIdentity>>,
    },
}
struct State {
    app: Box<dyn App>,
    calibrate_print: bool,
    panels:crate::window_layout::Panels,
    deferred_commands: Vec<(u16, u16)>,
    close_confirmed: bool,
    recovery:Option<crate::recovery::Journal>,
    recovery_last:std::time::Instant,
    recovery_unsaved:bool,
    recovered_copy:Option<crate::recovery::Claim>,
    recovery_error:Option<String>,
    path: Option<PathBuf>,
    session: suite42_core::session::DocumentSession,
    async_open: Option<AsyncOpen>,
    hwnd: HWND,
    list: HWND,
    table: HWND,
    edit: HWND,
    canvas: HWND,
    status: HWND,
    menu: HMENU,
    recent_menu: HMENU,
    recent_paths: Vec<PathBuf>,
    grid_cache: Option<Vec<Vec<String>>>,
    sparse_grid:Option<crate::SparseTable>,
    toolbar: Vec<(u16, HWND)>,
    toolbar_more: HWND,
    ruler_horizontal: HWND,
    ruler_vertical: HWND,
    ruler_format: Option<(crate::PrintGeometry, u16, i32, i32, i32)>,
    fullscreen: Option<(isize, WINDOWPLACEMENT)>,
}
impl State {
    fn dirty(&self) -> bool {
        self.app.dirty() || self.recovery_unsaved
    }
    fn status_text(&self) -> String {
        if self.async_open.is_some() {
            format!("{} · Opening… (Esc to cancel)", self.app.status())
        } else if let Some(error) = &self.recovery_error {
            format!("{} · {}: {error}",self.app.status(),crate::localization::text("Recovery"))
        } else if self.recovery_unsaved {
            format!(
                "{} · {}",
                self.app.status(),
                crate::localization::text("Recovered copy; save to keep your changes")
            )
        } else {
            self.app.status()
        }
    }
    fn clear_recovery(&mut self) {
        self.recovery_unsaved = false;
        if let Some(journal) = self.recovery.as_mut() {
            if let Err(error) = journal.clear() {
                self.recovery_error = Some(error);
            }
        }
        if let Some(copy) = self.recovered_copy.take() {
            if let Err(error) = copy.discard() {
                self.recovery_error = Some(error);
            }
        }
    }
}
unsafe fn poll_recovery() {
    let previous = STATE.with(|slot| {
        slot.borrow()
            .as_ref()
            .and_then(|state| state.recovery_error.clone())
    });
    STATE.with(|slot| {
        let mut slot = slot.borrow_mut();
        let Some(state) = slot.as_mut() else {
            return;
        };
        if !state.app.supports_recovery() {
            return;
        }
        if let Some(journal) = state.recovery.as_mut() {
            while let Some(result) = journal.poll() {
                state.recovery_error = result.err();
            }
        }
        if !state.dirty() {
            if let Some(journal) = state.recovery.as_mut() {
                if let Err(error) = journal.clear() {
                    state.recovery_error = Some(error);
                }
            }
            return;
        }
        let delay = std::env::var("OFF42_RECOVERY_INTERVAL_MS")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(30000)
            .clamp(1000, 30000);
        if state.recovery_last.elapsed() < std::time::Duration::from_millis(delay)
            || state.async_open.is_some()
        {
            return;
        }
        state.recovery_last = std::time::Instant::now();
        let result = (|| -> Result<(), String> {
            let Some(snapshot) = state.app.recovery_snapshot()? else {
                return Ok(());
            };
            if state.recovery.is_none() {
                let root =
                    crate::recovery::directory().ok_or("Local recovery storage is unavailable")?;
                state.recovery = Some(crate::recovery::Journal::start(&root, state.app.name())?);
            }
            let origin = state.path.clone().or_else(|| {
                state
                    .recovered_copy
                    .as_ref()
                    .and_then(|copy| copy.origin().map(Path::to_path_buf))
            });
            state.recovery.as_mut().unwrap().record(snapshot, origin)?;
            Ok(())
        })();
        if let Err(error) = result {
            state.recovery_error = Some(error);
        }
    });
    if STATE.with(|slot| {
        slot.borrow()
            .as_ref()
            .and_then(|state| state.recovery_error.clone())
    }) != previous
    {
        update_caption();
    }
}
unsafe fn recover_document(hwnd: HWND) -> Result<(), String> {
    let (app, supported) = STATE.with(|slot| {
        let slot = slot.borrow();
        let state = slot.as_ref().unwrap();
        (state.app.name(), state.app.supports_recovery())
    });
    if !supported {
        return Ok(());
    }
    let root = crate::recovery::directory().ok_or("Local recovery storage is unavailable")?;
    let copies = crate::recovery::copies(&root, app);
    if copies.is_empty() {
        msg(
            hwnd,
            &crate::localization::text("No recovery copies are available."),
            MB_OK | MB_ICONINFORMATION,
        );
        return Ok(());
    }
    let choices = copies
        .iter()
        .enumerate()
        .map(|(index, copy)| format!("{}. {}", index + 1, copy.title()))
        .collect::<Vec<_>>();
    let request = crate::InputRequest {
        title: "Recover Unsaved Document",
        fields: vec![
            ("Recovery copy", choices[0].clone()),
            ("Action", "Recover".into()),
        ],
    };
    let Some(values) = crate::prompt::show_choices(
        hwnd,
        &request,
        false,
        false,
        &[
            (0, choices.clone()),
            (1, vec!["Recover".into(), "Delete Recovery Copy".into()]),
        ],
    ) else {
        return Ok(());
    };
    let index = choices
        .iter()
        .position(|v| v == &values[0])
        .ok_or("Choose a recovery copy")?;
    if values[1] == "Delete Recovery Copy" {
        copies[index].discard()?;
        return Ok(());
    }
    if values[1] != "Recover" {
        return Err("Choose a recovery action".into());
    }
    let claim=copies[index].acquire()?;
    if !confirm(hwnd) {
        return Ok(());
    }
    let allowed = STATE.with(|slot| {
        let slot = slot.borrow();
        let app = &slot.as_ref().unwrap().app;
        app.recovery_extensions().iter().any(|extension|extension.eq_ignore_ascii_case(claim.extension()))||app.open_formats()
            .iter()
            .chain(app.save_formats().iter())
            .any(|format| {
                format
                    .extensions
                    .iter()
                    .any(|extension| extension.eq_ignore_ascii_case(claim.extension()))
            })
    });
    if !allowed {
        return Err("This recovery format is unavailable in this application".into());
    }
    let path = claim.materialize()?;
    cancel_pending_open();
    STATE.with(|slot| -> Result<(), String> {
        let mut slot = slot.borrow_mut();
        let state = slot.as_mut().unwrap();
        state.app.open(&path)?;
        state.clear_recovery();
        state.path = None;
        state.session = Default::default();
        state.recovery_unsaved = true;
        state.recovered_copy = Some(claim);
        state.recovery_error = None;
        state.recovery_last = std::time::Instant::now() - std::time::Duration::from_secs(30);
        Ok(())
    })?;
    refresh();
    Ok(())
}

thread_local! { static STATE:RefCell<Option<State>>=const { RefCell::new(None) }; static REFRESHING:Cell<bool>=const {Cell::new(false)}; static DRAGGING:Cell<bool>=const {Cell::new(false)}; static POLL_MS:Cell<u32>=const {Cell::new(100)}; }

unsafe fn msg(hwnd: HWND, s: &str, flags: MESSAGEBOX_STYLE) -> MESSAGEBOX_RESULT {
    MessageBoxW(hwnd, wide(s).as_ptr(), wide("off42").as_ptr(), flags)
}
unsafe fn pick_folder(hwnd: HWND, title: &str) -> Option<String> {
    use windows::{
        core::PCWSTR,
        Win32::{
            Foundation::HWND as WinHwnd,
            System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER},
            UI::Shell::{
                FileOpenDialog, IFileOpenDialog, FOS_FORCEFILESYSTEM, FOS_NOCHANGEDIR,
                FOS_PATHMUSTEXIST, FOS_PICKFOLDERS, SIGDN_FILESYSPATH,
            },
        },
    };
    let initialized = CoInitializeEx(null(), COINIT_APARTMENTTHREADED as u32);
    if initialized < 0 {
        return None;
    }
    let result = (|| -> windows::core::Result<String> {
        let picker: IFileOpenDialog =
            CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER)?;
        let flags = picker.GetOptions()?
            | FOS_PICKFOLDERS
            | FOS_FORCEFILESYSTEM
            | FOS_PATHMUSTEXIST
            | FOS_NOCHANGEDIR;
        picker.SetOptions(flags)?;
        let title = wide(&crate::localization::text(title));
        picker.SetTitle(PCWSTR(title.as_ptr()))?;
        picker.Show(Some(WinHwnd(hwnd)))?;
        let item = picker.GetResult()?;
        let path = item.GetDisplayName(SIGDN_FILESYSPATH)?;
        let result = path.to_string();
        CoTaskMemFree(path.0.cast());
        Ok(result?)
    })()
    .ok();
    CoUninitialize();
    result
}
unsafe fn pick_multiple_files(hwnd:HWND,action:crate::FileAction)->Result<Option<Vec<PathBuf>>,String>{
    use windows::{core::PCWSTR,Win32::{Foundation::HWND as WinHwnd,System::Com::{CoCreateInstance,CLSCTX_INPROC_SERVER},UI::Shell::{Common::COMDLG_FILTERSPEC,FileOpenDialog,IFileOpenDialog,FOS_ALLOWMULTISELECT,FOS_FORCEFILESYSTEM,FOS_PATHMUSTEXIST,FOS_FILEMUSTEXIST,FOS_NOCHANGEDIR,FOS_DONTADDTORECENT,SIGDN_FILESYSPATH}}};
    if CoInitializeEx(null(),COINIT_APARTMENTTHREADED as u32)<0{return Err("Could not initialize the Windows file picker".into());}
    let result=(||->Result<Option<Vec<PathBuf>>,String>{
        let result=(||->windows::core::Result<Option<Vec<PathBuf>>>{
            let picker:IFileOpenDialog=CoCreateInstance(&FileOpenDialog,None,CLSCTX_INPROC_SERVER)?;
            picker.SetOptions(picker.GetOptions()?|FOS_ALLOWMULTISELECT|FOS_FORCEFILESYSTEM|FOS_PATHMUSTEXIST|FOS_FILEMUSTEXIST|FOS_NOCHANGEDIR|FOS_DONTADDTORECENT)?;
            let(title,format,pattern)=(wide(&crate::localization::text(action.title)),wide(&crate::localization::text(action.format)),wide(action.pattern));
            picker.SetTitle(PCWSTR(title.as_ptr()))?;picker.SetFileTypes(&[COMDLG_FILTERSPEC{pszName:PCWSTR(format.as_ptr()),pszSpec:PCWSTR(pattern.as_ptr())}])?;
            match picker.Show(Some(WinHwnd(hwnd))){Err(e)if e.code().0 as u32==0x800704c7=>return Ok(None),result=>result?,}
            let items=picker.GetResults()?;let mut paths=Vec::new();let count=items.GetCount()?;
            if count>128{return Err(windows::core::Error::new(windows::core::HRESULT(0x80070057u32 as i32),"Choose at most 128 files"));}
            for index in 0..count{let path=items.GetItemAt(index)?.GetDisplayName(SIGDN_FILESYSPATH)?;paths.push(PathBuf::from(OsString::from_wide(path.as_wide())));CoTaskMemFree(path.0.cast());}Ok(Some(paths))
        })();result.map_err(|e|format!("Could not select input files: {e}"))
    })();CoUninitialize();result
}

unsafe fn dialog(
    hwnd: HWND,
    save: bool,
    ext: &str,
    format: &str,
    pattern: &str,
) -> Option<PathBuf> {
    file_dialog(hwnd, save, ext, format, pattern, None)
}
unsafe fn file_dialog(
    hwnd: HWND,
    save: bool,
    ext: &str,
    format: &str,
    pattern: &str,
    private_title: Option<&str>,
) -> Option<PathBuf> {
    let mut buf = vec![0u16; 32768];
    let filter: Vec<u16> = format!("{format}\0{pattern}\0\0").encode_utf16().collect();
    let extw = wide(ext);
    let mut f: OPENFILENAMEW = zeroed();
    f.lStructSize = size_of::<OPENFILENAMEW>() as u32;
    f.hwndOwner = hwnd;
    f.lpstrFilter = filter.as_ptr();
    f.lpstrFile = buf.as_mut_ptr();
    f.nMaxFile = buf.len() as u32;
    f.lpstrDefExt = extw.as_ptr();
    f.Flags = OFN_EXPLORER
        | OFN_NOCHANGEDIR
        | OFN_PATHMUSTEXIST
        | if save { 0 } else { OFN_FILEMUSTEXIST };
    let title;
    if let Some(name) = private_title {
        title = wide(&crate::localization::text(name));
        f.lpstrTitle = title.as_ptr();
        f.Flags |= OFN_DONTADDTORECENT;
    }
    if (if save {
        GetSaveFileNameW(&mut f)
    } else {
        GetOpenFileNameW(&mut f)
    }) == 0
    {
        return None;
    }
    let n = buf.iter().position(|&x| x == 0).unwrap_or(buf.len());
    Some(PathBuf::from(OsString::from_wide(&buf[..n])))
}
unsafe fn format_dialog(
    hwnd: HWND,
    formats: &[crate::FileFormat],
    save: bool,
) -> Option<(PathBuf, usize)> {
    let mut buf = vec![0u16; 32768];
    let mut filter = Vec::new();
    for format in formats {
        let pattern = format
            .extensions
            .iter()
            .map(|e| format!("*.{e}"))
            .collect::<Vec<_>>()
            .join(";");
        filter.extend(crate::localization::text(format.title).encode_utf16());
        filter.push(0);
        filter.extend(pattern.encode_utf16());
        filter.push(0);
    }
    filter.push(0);
    let mut f: OPENFILENAMEW = zeroed();
    f.lStructSize = size_of::<OPENFILENAMEW>() as u32;
    f.hwndOwner = hwnd;
    f.lpstrFilter = filter.as_ptr();
    f.nFilterIndex = 1;
    f.lpstrFile = buf.as_mut_ptr();
    f.nMaxFile = buf.len() as u32;
    f.Flags = OFN_EXPLORER
        | OFN_NOCHANGEDIR
        | OFN_PATHMUSTEXIST
        | if save { 0 } else { OFN_FILEMUSTEXIST };
    if (if save {
        GetSaveFileNameW(&mut f)
    } else {
        GetOpenFileNameW(&mut f)
    }) == 0
    {
        return None;
    }
    let index = (f.nFilterIndex as usize)
        .saturating_sub(1)
        .min(formats.len() - 1);
    let n = buf.iter().position(|&x| x == 0).unwrap_or(buf.len());
    let mut path = PathBuf::from(OsString::from_wide(&buf[..n]));
    if save && path.extension().is_none() {
        if let Some(ext) = formats[index].extensions.first() {
            path.set_extension(ext);
        }
    }
    Some((path, index))
}
unsafe fn save(hwnd: HWND) -> bool {
    save_mode(hwnd, false)
}
unsafe fn save_mode(hwnd: HWND, save_as: bool) -> bool {
    let opt_in = STATE.with(|s| !s.borrow().as_ref().unwrap().app.save_formats().is_empty());
    if opt_in {
        return save_snapshot(hwnd, save_as);
    }
    legacy_save(hwnd)
}
unsafe fn save_snapshot(hwnd: HWND, save_as: bool) -> bool {
    use suite42_core::session::{publish_bundle, FileIdentity};
    let (mut formats, existing, recovery_extension) = STATE.with(|s| {
        let s = s.borrow();
        let s = s.as_ref().unwrap();
        (s.app.save_formats(), s.session.location().cloned(),s.recovered_copy.as_ref().map(|copy|copy.extension().to_owned()))
    });
    if existing.is_none(){if let Some(extension)=recovery_extension{
        if let Some(index)=formats.iter().position(|format|format.extensions.iter().any(|value|value.eq_ignore_ascii_case(&extension))){formats.swap(0,index);}
    }}
    if formats.is_empty() {
        return false;
    }
    let can_save_in_place = STATE.with(|s| s.borrow().as_ref().unwrap().app.can_save_in_place());
    let selected = if !save_as && can_save_in_place {
        existing
            .as_ref()
            .and_then(|l| formats.iter().position(|f| f.id == l.format))
    } else {
        None
    };
    let (path, index) = if let (Some(location), Some(index)) = (&existing, selected) {
        (location.path.clone(), index)
    } else {
        match format_dialog(hwnd, &formats, true) {
            Some(v) => v,
            None => return false,
        }
    };
    let format = &formats[index];
    if crate::recovery::directory().is_some_and(|root|crate::recovery::owns_destination(&root,&path)){
        msg(hwnd,&crate::localization::text("Choose a save destination outside the automatic recovery folder."),MB_OK|MB_ICONWARNING);return false;
    }
    if !path
        .extension()
        .and_then(|v| v.to_str())
        .is_some_and(|e| format.extensions.iter().any(|v| e.eq_ignore_ascii_case(v)))
    {
        msg(
            hwnd,
            "The file extension does not match the selected format.",
            MB_OK | MB_ICONWARNING,
        );
        return false;
    }
    let is_current = existing.as_ref().is_some_and(|l| l.path == path);
    let expected = if is_current {
        existing.as_ref().map(|l| l.identity.clone())
    } else {
        None
    };
    let expected = if path.exists() {
        let actual = match FileIdentity::read(&path) {
            Ok(i) => i,
            Err(e) => {
                msg(hwnd, &e.to_string(), MB_OK | MB_ICONERROR);
                return false;
            }
        };
        if expected.as_ref() != Some(&actual) {
            if msg(
                hwnd,
                "This file exists or has changed since it was opened. Replace it?",
                MB_YESNO | MB_ICONWARNING,
            ) != IDYES
            {
                return false;
            }
            Some(actual)
        } else {
            expected
        }
    } else if expected.is_some() {
        if msg(
            hwnd,
            "The original file was removed. Recreate it?",
            MB_YESNO | MB_ICONWARNING,
        ) != IDYES
        {
            return false;
        }
        None
    } else {
        None
    };
    let snapshot = STATE.with(|s| {
        s.borrow_mut()
            .as_mut()
            .unwrap()
            .app
            .save_bundle_to(format.id, &path, is_current)
    });
    let (revision, bundle) = match snapshot {
        Ok(Some(v)) => v,
        Ok(None) => {
            msg(
                hwnd,
                "This document cannot be saved in that format.",
                MB_OK | MB_ICONERROR,
            );
            return false;
        }
        Err(e) => {
            msg(hwnd, &e, MB_OK | MB_ICONERROR);
            return false;
        }
    };
    if let Err(e) = publish_bundle(&path, &bundle, expected.as_ref()) {
        msg(hwnd, &e.to_string(), MB_OK | MB_ICONERROR);
        return false;
    }
    let result = STATE.with(|s| {
        let mut s = s.borrow_mut();
        let s = s.as_mut().unwrap();
        let result = s
            .session
            .published(path.clone(), format.id.into(), revision);
        if result.is_ok() {
            s.app.saved_snapshot(revision);
            s.clear_recovery();
            s.path = Some(path);
        }
        result
    });
    if let Err(e) = result {
        msg(hwnd, &e.to_string(), MB_OK | MB_ICONERROR);
        return false;
    }
    remember_current_document();
    true
}
unsafe fn legacy_save(hwnd: HWND) -> bool {
    let (ext, fmt) = STATE.with(|s| {
        let s = s.borrow();
        let a = &s.as_ref().unwrap().app;
        (a.extension(), a.format_name())
    });
    loop {
        let Some(p) = dialog(hwnd, true, ext, fmt, &format!("*.{ext}")) else {
            return false;
        };
        if p.extension()
            .and_then(|x| x.to_str())
            .is_none_or(|x| !x.eq_ignore_ascii_case(ext))
        {
            msg(
                hwnd,
                &format!("Choose a .{ext} file name."),
                MB_OK | MB_ICONWARNING,
            );
            continue;
        }
        if p.exists() {
            msg(
                hwnd,
                "Choose a new file name; existing files cannot be overwritten.",
                MB_OK | MB_ICONWARNING,
            );
            continue;
        }
        let r = STATE.with(|s| {
            let mut s = s.borrow_mut();
            let s = s.as_mut().unwrap();
            let r = s.app.save(&p);
            if r.is_ok() {
                s.clear_recovery();
                s.path = Some(p)
            }
            r
        });
        match r {
            Ok(()) => {
                remember_current_document();
                return true;
            }
            Err(e) => {
                msg(hwnd, &e, MB_OK | MB_ICONERROR);
                return false;
            }
        }
    }
}
unsafe fn confirm(hwnd: HWND) -> bool {
    if !STATE.with(|s| s.borrow().as_ref().unwrap().dirty()) {
        return true;
    }
    match msg(
        hwnd,
        "Save changes before continuing?",
        MB_YESNOCANCEL | MB_ICONWARNING,
    ) {
        IDYES => save(hwnd),
        IDNO => true,
        _ => false,
    }
}
unsafe fn refresh() {
    let (
        hwnd,
        list,
        table,
        edit,
        canvas,
        status,
        menu,
        title,
        items,
        text,
        info,
        undo,
        redo,
        visible,
        readonly,
        image,
        grid,
        sparse,
    ) = STATE.with(|s| {
        let s = s.borrow();
        let s = s.as_ref().unwrap();
        (
            s.hwnd,
            s.list,
            s.table,
            s.edit,
            s.canvas,
            s.status,
            s.menu,
            format!(
                "{}{} - {}",
                s.path
                    .as_ref()
                    .and_then(|p| p.file_name())
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| crate::localization::text("Untitled")),
                if s.dirty() { " *" } else { "" },
                s.app.name()
            ),
            s.app
                .grid_sheets()
                .map(|v| v.0)
                .unwrap_or_else(|| s.app.items()),
            s.app.text(),
            s.status_text(),
            s.app.can_undo(),
            s.app.can_redo(),
            s.app.list_visible(),
            s.app.text_readonly(),
            s.app.image().is_some() || s.app.canvas_scene().is_some(),
            if s.app.uses_custom_grid(){None}else{s.app.table()},
            if s.app.uses_custom_grid(){s.app.sparse_table().or_else(||s.app.table().and_then(crate::SparseTable::from_dense))}else{None},
        )
    });
    let grid_visible=grid.is_some()||sparse.is_some();
    STATE.with(|s|s.borrow_mut().as_mut().unwrap().sparse_grid=sparse);
    REFRESHING.with(|x| x.set(true));
    SetWindowTextW(hwnd, wide(&title).as_ptr());
    SetWindowTextW(status, wide(&info).as_ptr());
    let rich = STATE.with(|s| s.borrow().as_ref().unwrap().app.rich_text());
    let focus_rich = rich.is_some() && IsWindowVisible(edit) == 0;
    if let Some(rtf) = rich {
        let selection = if focus_rich {
            STATE
                .with(|s| s.borrow().as_ref().unwrap().app.rich_selection_to_restore())
                .unwrap_or_else(|| crate::rich_edit::selection(edit))
        } else {
            crate::rich_edit::selection(edit)
        };
        let _ = crate::rich_edit::load(edit, &rtf);
        crate::rich_edit::set_selection(edit, selection);
    } else {
        SetWindowTextW(edit, wide(&text).as_ptr());
    }
    SendMessageW(edit, EM_SETREADONLY, readonly as usize, 0);
    SendMessageW(list, LB_RESETCONTENT, 0, 0);
    for item in items {
        SendMessageW(list, LB_ADDSTRING, 0, wide(&item).as_ptr() as isize);
    }
    let (selected, actions, can_save) = STATE.with(|s| {
        let s = s.borrow();
        let a = &s.as_ref().unwrap().app;
        (
            a.grid_sheets().map(|v| v.1).unwrap_or_else(|| a.selected()),
            a.actions()
                .iter()
                .chain(a.view_actions().iter())
                .map(|&(id, _)| (id, a.command_state(id)))
                .chain(
                    a.file_actions()
                        .iter()
                        .map(|action| (action.id, a.command_state(action.id))),
                )
                .chain(
                    a.command_specs()
                        .into_iter()
                        .map(|c| (c.id, a.command_state(c.id))),
                )
                .collect::<Vec<_>>(),
            a.can_save(),
        )
    });
    let file_enabled = STATE.with(|s| {
        let s = s.borrow();
        let a = &s.as_ref().unwrap().app;
        a.file_actions()
            .iter()
            .map(|f| (f.id, a.enabled(f.id)))
            .collect::<Vec<_>>()
    });
    for (id, enabled) in file_enabled {
        EnableMenuItem(
            menu,
            id as u32,
            MF_BYCOMMAND | if enabled { MF_ENABLED } else { MF_GRAYED },
        );
    }
    let can_insert_mark = STATE.with(|s| {
        let s = s.borrow();
        let app = &s.as_ref().unwrap().app;
        app.mark_file_action().map_or_else(
            || app.mark_canvas_size().is_some(),
            |id| app.command_state(id).enabled,
        )
    });
    EnableMenuItem(
        menu,
        INSERT_MARK as u32,
        MF_BYCOMMAND
            | if can_insert_mark {
                MF_ENABLED
            } else {
                MF_GRAYED
            },
    );
    let (insert_technical, edit_technical) = STATE.with(|state| {
        let state = state.borrow();
        let app = &state.as_ref().unwrap().app;
        (
            app.can_insert_technical_element(),
            !app.technical_targets().is_empty(),
        )
    });
    for (id, enabled) in [
        (INSERT_TECHNICAL, insert_technical),
        (EDIT_TECHNICAL, insert_technical && edit_technical),
    ] {
        EnableMenuItem(
            menu,
            id as u32,
            MF_BYCOMMAND | if enabled { MF_ENABLED } else { MF_GRAYED },
        );
    }
    let calibration_enabled = STATE.with(|state| state.borrow().as_ref().unwrap().calibrate_print);
    CheckMenuItem(
        menu,
        TOGGLE_CALIBRATION as u32,
        MF_BYCOMMAND
            | if calibration_enabled {
                MF_CHECKED
            } else {
                MF_UNCHECKED
            },
    );
    let proofing = STATE.with(|state| {
        let state = state.borrow();
        let state = state.as_ref().unwrap();
        state.app.uses_rich_edit() && crate::rich_edit::proofing_enabled(state.edit)
    });
    CheckMenuItem(
        menu,
        TOGGLE_PROOFING as u32,
        MF_BYCOMMAND | if proofing { MF_CHECKED } else { MF_UNCHECKED },
    );
    let spelling_available=STATE.with(|slot|{
        let slot=slot.borrow();let state=slot.as_ref().unwrap();state.app.uses_rich_edit()&&!state.app.text_readonly()&&IsWindowVisible(state.edit)!=0&&state.async_open.is_none()
    });
    EnableMenuItem(menu,CHECK_SPELLING as u32,MF_BYCOMMAND|if spelling_available{MF_ENABLED}else{MF_GRAYED});
    let opening = STATE.with(|s| s.borrow().as_ref().is_some_and(|s| s.async_open.is_some()));
    EnableMenuItem(
        menu,
        CANCEL_OPEN as u32,
        MF_BYCOMMAND | if opening { MF_ENABLED } else { MF_GRAYED },
    );
    EnableMenuItem(
        menu,
        SAVE as u32,
        MF_BYCOMMAND | if can_save { MF_ENABLED } else { MF_GRAYED },
    );
    SendMessageW(list, LB_SETCURSEL, selected, 0);
    for (id, state) in actions {
        let state=effective_command_state(id,state);
        update_toolbar_command(id,state);
        EnableMenuItem(
            menu,
            id as u32,
            MF_BYCOMMAND | if state.enabled { MF_ENABLED } else { MF_GRAYED },
        );
        CheckMenuItem(
            menu,
            id as u32,
            MF_BYCOMMAND
                | if state.checked {
                    MF_CHECKED
                } else {
                    MF_UNCHECKED
                },
        );
    }
    update_table(table, grid.as_deref());
    ShowWindow(table, if grid_visible { SW_SHOW } else { SW_HIDE });
    ShowWindow(
        list,
        if visible
            && (!grid_visible
                || STATE.with(|s| s.borrow().as_ref().unwrap().app.grid_sheets().is_some()))
        {
            SW_SHOW
        } else {
            SW_HIDE
        },
    );
    let inline = STATE.with(|s| {
        s.borrow()
            .as_ref()
            .unwrap()
            .app
            .inline_rich_rect()
            .is_some()
    });
    ShowWindow(edit, if image && !inline { SW_HIDE } else { SW_SHOW });
    if focus_rich && (!image || inline) {
        SetFocus(edit);
    }
    ShowWindow(canvas, if image { SW_SHOW } else { SW_HIDE });
    InvalidateRect(canvas, null(), 1);
    EnableMenuItem(
        menu,
        UNDO as u32,
        MF_BYCOMMAND | if undo { MF_ENABLED } else { MF_GRAYED },
    );
    EnableMenuItem(
        menu,
        REDO as u32,
        MF_BYCOMMAND | if redo { MF_ENABLED } else { MF_GRAYED },
    );
    REFRESHING.with(|x| x.set(false));
    sync_presentation(hwnd);
    sync_panel_menu();
    DrawMenuBar(hwnd);
    layout(hwnd);
    flush_deferred_commands();
}
unsafe fn sync_presentation(hwnd: HWND) {
    let (presenting, active) = STATE.with(|s| {
        let s = s.borrow();
        let s = s.as_ref().unwrap();
        (s.app.presentation_size().is_some(), s.fullscreen.is_some())
    });
    if presenting && !active {
        let mut placement: WINDOWPLACEMENT = zeroed();
        placement.length = size_of::<WINDOWPLACEMENT>() as u32;
        if GetWindowPlacement(hwnd, &mut placement) == 0 {
            return;
        }
        let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
        let mut info: MONITORINFO = zeroed();
        info.cbSize = size_of::<MONITORINFO>() as u32;
        if GetMonitorInfoW(monitor, &mut info) == 0 {
            return;
        }
        let style = GetWindowLongPtrW(hwnd, GWL_STYLE);
        STATE.with(|s| s.borrow_mut().as_mut().unwrap().fullscreen = Some((style, placement)));
        SetMenu(hwnd, null_mut());
        SetWindowLongPtrW(hwnd, GWL_STYLE, (WS_POPUP | WS_VISIBLE) as isize);
        SetWindowPos(
            hwnd,
            HWND_TOP,
            info.rcMonitor.left,
            info.rcMonitor.top,
            info.rcMonitor.right - info.rcMonitor.left,
            info.rcMonitor.bottom - info.rcMonitor.top,
            SWP_FRAMECHANGED | SWP_SHOWWINDOW,
        );
    } else if !presenting && active {
        let (style, placement, menu) = STATE.with(|s| {
            let mut s = s.borrow_mut();
            let s = s.as_mut().unwrap();
            let (style, placement) = s.fullscreen.take().unwrap();
            (style, placement, s.menu)
        });
        SetWindowLongPtrW(hwnd, GWL_STYLE, style);
        SetMenu(hwnd, menu);
        SetWindowPlacement(hwnd, &placement);
        SetWindowPos(
            hwnd,
            null_mut(),
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_FRAMECHANGED,
        );
    }
}
unsafe fn scene_for_canvas(
    hwnd: HWND,
    mut scene: crate::CanvasScene,
    size: Option<(f64, f64)>,
) -> crate::CanvasScene {
    if let Some((width, height)) = size {
        if width > 0.0 && height > 0.0 {
            let mut r: RECT = zeroed();
            GetClientRect(hwnd, &mut r);
            let zoom = (r.right as f64 / width).min(r.bottom as f64 / height);
            if zoom.is_finite() && zoom > 0.0 {
                scene.viewport = crate::Viewport {
                    zoom,
                    pan: crate::Point {
                        x: (r.right as f64 - width * zoom) / 2.0,
                        y: (r.bottom as f64 - height * zoom) / 2.0,
                    },
                };
            }
        }
    }
    scene
}
unsafe fn layout(hwnd: HWND) {
    let (
        list,
        table,
        edit,
        canvas,
        status,
        visible,
        grid,
        toolbar,
        toolbar_more,
        ruler_horizontal,
        ruler_vertical,
        has_ruler,
        panels,
    ) = STATE.with(|s| {
        let s = s.borrow();
        let s = s.as_ref().unwrap();
        (
            s.list,
            s.table,
            s.edit,
            s.canvas,
            s.status,
            s.app.list_visible()&&s.panels.sidebar,
            s.grid_cache.is_some()||s.sparse_grid.is_some(),
            s.toolbar.clone(),
            s.toolbar_more,
            s.ruler_horizontal,
            s.ruler_vertical,
            s.panels.rulers && s.app.rich_ruler().is_some()
                && s.app.canvas_scene().is_none()
                && s.app.image().is_none(),
            s.panels,
        )
    });
    let mut r: RECT = zeroed();
    GetClientRect(hwnd, &mut r);
    let presenting = STATE.with(|s| {
        s.borrow()
            .as_ref()
            .unwrap()
            .app
            .presentation_size()
            .is_some()
    });
    if presenting {
        for control in [list, table, edit, status] {
            ShowWindow(control, SW_HIDE);
        }
        for (_, button) in &toolbar {
            ShowWindow(*button, SW_HIDE);
        }
        ShowWindow(toolbar_more, SW_HIDE);
        if !ruler_horizontal.is_null() {
            ShowWindow(ruler_horizontal, SW_HIDE);
            ShowWindow(ruler_vertical, SW_HIDE);
        }
        ShowWindow(canvas, SW_SHOW);
        MoveWindow(canvas, 0, 0, r.right.max(0), r.bottom.max(0), 1);
        SetFocus(canvas);
        return;
    }
    ShowWindow(status, if panels.status{SW_SHOW}else{SW_HIDE});
    let dpi = GetDpiForWindow(hwnd) as i32;
    let sh = if panels.status{24 * dpi / 96}else{0};
    let row_height = (32 * dpi / 96).max(1);
    let button_width = (105 * dpi / 96).max(1);
    let columns = (r.right.max(1) / button_width).max(1) as usize;
    let max_rows =
        ((r.bottom - sh - (80 * dpi / 96)).max(row_height) / row_height).clamp(1, 3) as usize;
    let capacity = columns.saturating_mul(max_rows);
    let toolbar_len=if panels.toolbar{toolbar.len()}else{0};
    let overflow = toolbar_len > capacity;
    let visible_count = if overflow {
        capacity.saturating_sub(1)
    } else {
        toolbar_len
    };
    let occupied = visible_count + usize::from(overflow);
    let rows = if occupied == 0 {
        0
    } else {
        occupied.div_ceil(columns)
    };
    let bar_height = rows as i32 * row_height;
    for (index, (_, button)) in toolbar.iter().enumerate() {
        if index < visible_count {
            let row = index / columns;
            let col = index % columns;
            MoveWindow(
                *button,
                col as i32 * button_width,
                row as i32 * row_height,
                button_width.min((r.right - col as i32 * button_width).max(1)),
                row_height,
                1,
            );
            ShowWindow(*button, SW_SHOW);
        } else {
            ShowWindow(*button, SW_HIDE);
        }
    }
    if overflow {
        let index = visible_count;
        MoveWindow(
            toolbar_more,
            (index % columns) as i32 * button_width,
            (index / columns) as i32 * row_height,
            button_width.min((r.right - (index % columns) as i32 * button_width).max(1)),
            row_height,
            1,
        );
        ShowWindow(toolbar_more, SW_SHOW);
    } else {
        ShowWindow(toolbar_more, SW_HIDE);
    }
    if grid {
        ShowWindow(list,if visible&&STATE.with(|s|s.borrow().as_ref().unwrap().app.grid_sheets().is_some()){SW_SHOW}else{SW_HIDE});
        if !ruler_horizontal.is_null() {
            ShowWindow(ruler_horizontal, SW_HIDE);
            ShowWindow(ruler_vertical, SW_HIDE);
        }
        let height = (r.bottom - sh - bar_height).max(0);
        let top = height * 3 / 4;
        let sheet_width =
            if visible && STATE.with(|s| s.borrow().as_ref().unwrap().app.grid_sheets().is_some()) {
                (180 * dpi / 96).min(r.right / 3)
            } else {
                0
            };
        MoveWindow(list, 0, bar_height, sheet_width, height, 1);
        MoveWindow(
            table,
            sheet_width,
            bar_height,
            r.right - sheet_width,
            top,
            1,
        );
        MoveWindow(
            edit,
            sheet_width,
            bar_height + top,
            r.right - sheet_width,
            height - top,
            1,
        );
        MoveWindow(status, 0, r.bottom - sh, r.right, sh, 1);
        return;
    }
    ShowWindow(list,if visible{SW_SHOW}else{SW_HIDE});
    let left = if visible {
        (240 * dpi / 96).min(r.right / 2)
    } else {
        0
    };
    MoveWindow(
        list,
        0,
        bar_height,
        left,
        (r.bottom - sh - bar_height).max(0),
        1,
    );
    let rw = if has_ruler {
        (26 * dpi / 96).max(16)
    } else {
        0
    };
    let rh = if has_ruler {
        (28 * dpi / 96).max(16)
    } else {
        0
    };
    if !ruler_horizontal.is_null() {
        ShowWindow(ruler_horizontal, if has_ruler { SW_SHOW } else { SW_HIDE });
        ShowWindow(ruler_vertical, if has_ruler { SW_SHOW } else { SW_HIDE });
        if has_ruler {
            MoveWindow(
                ruler_horizontal,
                left + rw,
                bar_height,
                (r.right - left - rw).max(0),
                rh,
                1,
            );
            MoveWindow(
                ruler_vertical,
                left,
                bar_height + rh,
                rw,
                (r.bottom - sh - bar_height - rh).max(0),
                1,
            );
        }
    }
    MoveWindow(
        edit,
        left + rw,
        bar_height + rh,
        (r.right - left - rw).max(0),
        (r.bottom - sh - bar_height - rh).max(0),
        1,
    );
    MoveWindow(
        canvas,
        left,
        bar_height,
        (r.right - left).max(0),
        (r.bottom - sh - bar_height).max(0),
        1,
    );
    let inline = STATE.with(|s| {
        let s = s.borrow();
        let a = &s.as_ref().unwrap().app;
        (
            a.inline_rich_rect(),
            a.canvas_scene(),
            a.presentation_size(),
        )
    });
    if let (Some(rect), Some(scene), size) = inline {
        let view = scene_for_canvas(canvas, scene, size).viewport;
        let a = view.document_to_view(crate::Point {
            x: rect.x,
            y: rect.y,
        });
        let b = view.document_to_view(crate::Point {
            x: rect.x + rect.width,
            y: rect.y + rect.height,
        });
        MoveWindow(
            edit,
            left + a.x.round() as i32,
            bar_height + a.y.round() as i32,
            (b.x - a.x).round().max(1.0) as i32,
            (b.y - a.y).round().max(1.0) as i32,
            1,
        );
        SetWindowPos(
            edit,
            HWND_TOP,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW,
        );
    }
    MoveWindow(status, 0, (r.bottom - sh).max(0), r.right, sh, 1);
    if has_ruler {
        sync_rulers(hwnd);
    }
}
/// Synchronize the document page rectangle with the edit control and both rulers.
unsafe fn sync_rulers(_hwnd: HWND) {
    let (edit, horizontal, vertical) = STATE.with(|s| {
        let s = s.borrow();
        let s = s.as_ref().unwrap();
        (s.edit, s.ruler_horizontal, s.ruler_vertical)
    });
    if edit.is_null() || horizontal.is_null() {
        return;
    }
    let selection = crate::rich_edit::selection(edit);
    let ruler = STATE.with(|s| {
        let mut s = s.borrow_mut();
        let app = &mut s.as_mut().unwrap().app;
        app.set_rich_selection(selection);
        app.rich_ruler()
    });
    let Some(data) = ruler else { return };
    if !data.page.valid() || data.zoom_percent == 0 {
        return;
    }
    let mut rect: RECT = zeroed();
    GetClientRect(edit, &mut rect);
    let dpi = (GetDpiForWindow(edit) as i32).max(1);
    let px = |v: i32| {
        ((i64::from(v) * i64::from(dpi) * i64::from(data.zoom_percent)) / 144_000)
            .clamp(0, i64::from(i32::MAX)) as i32
    };
    let page_width = px(data.page.width_twips);
    let page_left = ((rect.right - page_width) / 2).max(0);
    let key = (data.page, data.zoom_percent, rect.right, rect.bottom, dpi);
    let changed = STATE.with(|s| {
        let mut s = s.borrow_mut();
        let s = s.as_mut().unwrap();
        if s.ruler_format == Some(key) {
            false
        } else {
            s.ruler_format = Some(key);
            true
        }
    });
    if changed {
        let mut a = u32::from(data.zoom_percent);
        let mut b = 100u32;
        let (mut x, mut y) = (a, b);
        while y != 0 {
            let t = x % y;
            x = y;
            y = t
        }
        a /= x;
        b /= x;
        SendMessageW(edit, WM_USER + 225, a as usize, b as isize);
        let left = page_left.saturating_add(px(data.page.margin_left_twips));
        let right = page_left
            .saturating_add(page_width)
            .saturating_sub(px(data.page.margin_right_twips))
            .max(left.saturating_add(1));
        let top = px(data.page.margin_top_twips);
        let bottom = rect
            .bottom
            .saturating_sub(px(data.page.margin_bottom_twips))
            .max(top.saturating_add(1));
        let format = RECT {
            left,
            top,
            right,
            bottom,
        };
        SendMessageW(edit, EM_SETRECT, 0, &format as *const _ as isize);
    }
    let mut scroll: POINT = zeroed();
    SendMessageW(edit, WM_USER + 221, 0, &mut scroll as *mut _ as isize);
    crate::rich_ruler::update(horizontal, data.clone(), page_left - scroll.x);
    crate::rich_ruler::update(vertical, data, -scroll.y);
}
unsafe fn cancel_pending_open() {
    let canceled = STATE.with(|s| {
        if let Some(state) = s.borrow_mut().as_mut() {
            if state.async_open.take().is_some() {
                state.app.cancel_async_open();
                return true;
            }
        }
        false
    });
    if canceled {
        update_caption();
    }
}
unsafe fn update_recent_menu() {
    let (hwnd, menu, app) = STATE.with(|state| {
        let state = state.borrow();
        let state = state.as_ref().unwrap();
        (state.hwnd, state.recent_menu, state.app.name())
    });
    let paths = crate::recents::load(app);
    while GetMenuItemCount(menu) > 0 {
        DeleteMenu(menu, 0, MF_BYPOSITION);
    }
    if paths.is_empty() {
        AppendMenuW(
            menu,
            MF_STRING | MF_GRAYED,
            0,
            wide(&crate::localization::text("No Recent Documents")).as_ptr(),
        );
    }
    for (index, path) in paths.iter().enumerate() {
        // Path text is user data: escape menu mnemonics, never translate it.
        let title = format!(
            "{}. {}",
            index + 1,
            path.to_string_lossy().replace('&', "&&")
        );
        AppendMenuW(
            menu,
            MF_STRING,
            usize::from(RECENT_FIRST) + index,
            wide(&title).as_ptr(),
        );
    }
    AppendMenuW(menu, MF_SEPARATOR, 0, null());
    item(menu, CLEAR_RECENTS, "Clear Menu");
    STATE.with(|state| state.borrow_mut().as_mut().unwrap().recent_paths = paths);
    DrawMenuBar(hwnd);
}
unsafe fn remember_current_document() {
    let document = STATE.with(|state| {
        let state = state.borrow();
        let state = state.as_ref()?;
        Some((state.app.name(), state.path.clone()?))
    });
    if let Some((app, path)) = document {
        if crate::recents::remember(app, &path).is_ok() {
            update_recent_menu();
        }
    }
}

fn spawn_document_window(path: Option<&Path>) -> Result<(), String> {
    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    let mut process = std::process::Command::new(executable);
    process
        .arg("--language")
        .arg(crate::localization::current().code)
        .arg("--");
    if let Some(path) = path {
        process.arg(path);
    }
    process
        .spawn()
        .map(|_| ())
        .map_err(|error| error.to_string())
}
unsafe fn restore_window_layout(hwnd: HWND) {
    let app = STATE.with(|state| state.borrow().as_ref().unwrap().app.name());
    if let Some(layout) = crate::window_layout::load(app) {
        let [left, top, right, bottom] = layout.normal;
        let placement = WINDOWPLACEMENT {
            length: size_of::<WINDOWPLACEMENT>() as u32,
            showCmd: if layout.maximized {
                SW_SHOWMAXIMIZED as u32
            } else {
                SW_SHOWNORMAL as u32
            },
            ptMinPosition: POINT { x: -1, y: -1 },
            ptMaxPosition: POINT { x: -1, y: -1 },
            rcNormalPosition: RECT {
                left,
                top,
                right,
                bottom,
            },
            ..zeroed()
        };
        // Keep WINDOWPLACEMENT coordinates paired with its API. Windows adjusts
        // entirely offscreen positions after monitor/resolution changes.
        if SetWindowPlacement(hwnd, &placement) != 0 {
            return;
        }
    }
    ShowWindow(hwnd, SW_SHOW);
}
unsafe fn remember_window_layout(hwnd: HWND) {
    let (app, fullscreen) = STATE.with(|state| {
        let state = state.borrow();
        let state = state.as_ref().unwrap();
        (
            state.app.name(),
            state.fullscreen.as_ref().map(|(_, placement)| *placement),
        )
    });
    let mut placement = fullscreen.unwrap_or(WINDOWPLACEMENT {
        length: size_of::<WINDOWPLACEMENT>() as u32,
        ..zeroed()
    });
    if fullscreen.is_none() && GetWindowPlacement(hwnd, &mut placement) == 0 {
        return;
    }
    let r = placement.rcNormalPosition;
    let maximized = placement.showCmd == SW_SHOWMAXIMIZED as u32
        || (placement.showCmd == SW_SHOWMINIMIZED as u32
            && placement.flags & WPF_RESTORETOMAXIMIZED != 0);
    if let Some(mut layout) =
        crate::window_layout::Layout::new([r.left, r.top, r.right, r.bottom], maximized)
    {
        layout.panels=STATE.with(|state|state.borrow().as_ref().unwrap().panels);
        // A preference write failure must not prevent closing a saved document.
        let _ = crate::window_layout::save(app, &layout);
    }
}
unsafe fn open_paths(hwnd: HWND, paths: Vec<PathBuf>) {
    let first_add = paths.first().is_some_and(|path| {
        STATE.with(|state| state.borrow().as_ref().unwrap().app.opens_as_import(path))
    });
    if !first_add && !confirm(hwnd) {
        return;
    }
    for (index, path) in paths.into_iter().enumerate() {
        let add = STATE.with(|state| state.borrow().as_ref().unwrap().app.opens_as_import(&path));
        let result = if index == 0 || add {
            open_document_path(path)
        } else {
            spawn_document_window(Some(&path))
        };
        if let Err(error) = result {
            msg(hwnd, &error, MB_OK | MB_ICONERROR);
        }
    }
    refresh();
}

unsafe fn open_document_path(path: PathBuf) -> Result<(), String> {
    cancel_pending_open();
    if STATE.with(|state| state.borrow().as_ref().unwrap().app.opens_as_import(&path)) {
        return STATE.with(|state| state.borrow_mut().as_mut().unwrap().app.import_open(&path));
    }
    let async_started = STATE.with(|s| {
        let mut s = s.borrow_mut();
        let s = s.as_mut().unwrap();
        if s.app.open_is_add() {
            Ok(false)
        } else {
            s.app.begin_async_open(&path)
        }
    })?;
    if async_started {
        STATE.with(|s| {
            s.borrow_mut().as_mut().unwrap().async_open = Some(AsyncOpen {
                path,
                phase: AsyncOpenPhase::Loading,
            })
        });
        return Ok(());
    }
    let result = STATE.with(|s| {
        let mut s = s.borrow_mut();
        let s = s.as_mut().unwrap();
        let result = s.app.open(&path);
        if result.is_ok() && !s.app.open_is_add() {
            s.clear_recovery();
            s.session = Default::default();
            let formats = s.app.save_formats();
            let extension = path.extension().and_then(|v| v.to_str());
            if let Some(format) = formats.iter().find(|f| {
                extension.is_some_and(|e| f.extensions.iter().any(|v| e.eq_ignore_ascii_case(v)))
            }) {
                if let Ok(Some((revision, _))) = s.app.save_snapshot(format.id) {
                    if let Ok(session) = suite42_core::session::DocumentSession::opened(
                        path.clone(),
                        format.id.into(),
                        revision,
                    ) {
                        s.session = session;
                    }
                }
            }
            s.path = Some(path);
        }
        result
    });
    if result.is_ok() {
        remember_current_document();
    }
    result
}
unsafe fn poll_pending_open(hwnd: HWND) {
    use std::sync::mpsc::{self, TryRecvError};
    let pending = STATE.with(|s| s.borrow_mut().as_mut().and_then(|s| s.async_open.take()));
    let Some(mut pending) = pending else {
        return;
    };
    let result = match pending.phase {
        AsyncOpenPhase::Loading => {
            trace_command("open_loading", OPEN, 0);
            let ready = STATE.with(|s| {
                s.borrow_mut()
                    .as_mut()
                    .unwrap()
                    .app
                    .take_async_open_result()
            });
            match ready {
                None => {
                    let request = STATE.with(|s| {
                        s.borrow_mut()
                            .as_mut()
                            .unwrap()
                            .app
                            .take_async_open_password_request()
                    });
                    if let Some(request) = request {
                        if let Some(mut values) =
                            crate::prompt::show_options(hwnd, &request, false, true)
                        {
                            let result = STATE.with(|s| {
                                s.borrow_mut()
                                    .as_mut()
                                    .unwrap()
                                    .app
                                    .submit_async_open_password(values.first().map(String::as_str))
                            });
                            for value in &mut values {
                                value.as_bytes_mut().fill(0);
                            }
                            match result {
                                Ok(()) => {
                                    pending.phase = AsyncOpenPhase::Loading;
                                    Ok(Some(pending))
                                }
                                Err(error) => Err(error),
                            }
                        } else {
                            STATE
                                .with(|s| {
                                    s.borrow_mut()
                                        .as_mut()
                                        .unwrap()
                                        .app
                                        .submit_async_open_password(None)
                                })
                                .map(|_| None)
                        }
                    } else {
                        pending.phase = AsyncOpenPhase::Loading;
                        Ok(Some(pending))
                    }
                }
                Some(Err(e)) => Err(e),
                Some(Ok(expected)) => {
                    trace_command("open_decoded", OPEN, 0);
                    let path = pending.path.clone();
                    let (sender, receiver) = mpsc::channel();
                    match std::thread::Builder::new()
                        .name("suite42-open-verify".into())
                        .spawn(move || {
                            let _ = sender.send(suite42_core::session::FileIdentity::read(&path));
                        }) {
                        Ok(_) => {
                            pending.phase = AsyncOpenPhase::Checking {
                                expected,
                                result: receiver,
                            };
                            Ok(Some(pending))
                        }
                        Err(e) => Err(e.to_string()),
                    }
                }
            }
        }
        AsyncOpenPhase::Checking { expected, result } => match result.try_recv() {
            Err(TryRecvError::Empty) => {
                pending.phase = AsyncOpenPhase::Checking { expected, result };
                Ok(Some(pending))
            }
            Err(TryRecvError::Disconnected) => Err("Could not verify the opened file".into()),
            Ok(Err(e)) => Err(e.to_string()),
            Ok(Ok(actual)) => {
                trace_command("open_verified", OPEN, 0);
                if actual != expected
                    || !expected
                        .metadata_matches(&pending.path)
                        .map_err(|e| e.to_string())
                        .unwrap_or(false)
                {
                    Err("The source file changed while it was opening. Open it again.".into())
                } else {
                    STATE.with(|s| {
                        let mut s = s.borrow_mut();
                        let s = s.as_mut().unwrap();
                        let format = pending
                            .path
                            .extension()
                            .and_then(|x| x.to_str())
                            .unwrap_or("");
                        let all = s.app.open_formats().into_iter().chain(s.app.save_formats());
                        let format = all
                            .into_iter()
                            .find(|f| f.extensions.iter().any(|e| e.eq_ignore_ascii_case(format)))
                            .map(|f| f.id.to_string())
                            .unwrap_or_else(|| format.to_ascii_lowercase());
                        trace_command("open_commit_begin", OPEN, 0);
                        let revision = s.app.commit_async_open()?;
                        trace_command("open_commit_end", OPEN, 0);
                        s.clear_recovery();
                        s.session = suite42_core::session::DocumentSession::opened_with_identity(
                            pending.path.clone(),
                            format,
                            revision,
                            expected,
                        );
                        s.path = Some(pending.path);
                        Ok(None)
                    })
                }
            }
        },
    };
    match result {
        Ok(Some(pending)) => {
            STATE.with(|s| s.borrow_mut().as_mut().unwrap().async_open = Some(pending))
        }
        Ok(None) => {
            remember_current_document();
            refresh();
        }
        Err(e) => {
            STATE.with(|s| s.borrow_mut().as_mut().unwrap().app.cancel_async_open());
            msg(hwnd, &e, MB_OK | MB_ICONERROR);
            refresh();
        }
    }
}
unsafe fn apply_requested_rich_selection() {
    let (edit, selection) = STATE.with(|s| {
        let mut s = s.borrow_mut();
        let s = s.as_mut().unwrap();
        (s.edit, s.app.take_requested_rich_selection())
    });
    if let Some(selection) = selection {
        if !edit.is_null() {
            crate::rich_edit::set_selection(edit, selection);
            SetFocus(edit);
        }
    }
}
unsafe fn production_command(hwnd: HWND, id: u16) -> Result<(), String> {
    use crate::{production, LabelSource};
    let is_sheet = id == CREATE_LABEL_SHEET;
    let source_override = if is_sheet {
        STATE.with(|state| state.borrow_mut().as_mut().unwrap().app.take_label_source_override())?
    } else { None };
    let source = if let Some(source) = source_override { source } else if is_sheet {
        let mut sources =
            STATE.with(|state| state.borrow().as_ref().unwrap().app.label_sources())?;
        if sources.is_empty() {
            return Err("There are no label sources in this document".into());
        }
        let rich_selection=STATE.with(|state|{
            let state=state.borrow();let state=state.as_ref().unwrap();
            let width=state.app.label_rich_selection_width()?;
            if state.edit.is_null(){return None;}
            let range=crate::rich_edit::selection(state.edit);
            (range.0<range.1).then_some((state.edit,range,width))
        });
        // Native offsets include table delimiters and object characters that are absent
        // from the model's plain-text projection. Use the actual selected text here.
        if let Some((edit,_,_))=rich_selection {
            let bytes=crate::rich_edit::extract_selection(edit,false)?;
            if bytes.len()%2!=0||bytes.len()>2*1024*1024{return Err("Selected label text exceeds one million characters".into());}
            let units=bytes.chunks_exact(2).map(|b|u16::from_le_bytes([b[0],b[1]])).collect::<Vec<_>>();
            let selected=String::from_utf16(&units).map_err(|_|"Select complete text characters")?.replace('\u{fffc}',"").replace("\r\n","\n").replace('\r',"\n");
            for(_,source)in &mut sources {if let LabelSource::Text(text)=source{*text=selected.clone();}}
        }
        let index = if sources.len() == 1&&rich_selection.is_none() {
            0
        } else {
            let mut choices = sources
                .iter()
                .map(|(title, _)| title.clone())
                .collect::<Vec<_>>();
            if rich_selection.is_some(){choices.push("Selected Content Artwork".into());}
            let request = crate::InputRequest {
                title: "Label Source",
                fields: vec![("Source", choices[0].clone())],
            };
            let Some(values) =
                crate::prompt::show_choices(hwnd, &request, false, false, &[(0, choices.clone())])
            else {
                return Ok(());
            };
            choices
                .iter()
                .position(|choice| choice == &values[0])
                .ok_or("Choose a label source")?
        };
        if index==sources.len(){
            let(edit,range,width)=rich_selection.ok_or("Select document content first")?;
            if crate::rich_edit::selection(edit)!=range{return Err("The selection changed; select the label content again".into());}
            crate::rich_artwork::capture(edit,width)?
        }else{sources.remove(index).1}
    } else {
        LabelSource::Text(String::new())
    };
    let mut request = if is_sheet {
        production::sheet_request(&source)?
    } else if id == EXPORT_TECHNICAL {
        production::technical_request()
    } else {
        production::barcode_request(String::new())
    };
    let app_name = STATE.with(|state| state.borrow().as_ref().unwrap().app.name());
    let preferences = crate::label_preferences::directory();
    if is_sheet {
        if let Some(directory) = &preferences {
            let _ = crate::label_preferences::restore(directory, app_name, &source, &mut request);
        }
    }
    let multiline = is_sheet && matches!(&source, LabelSource::Text(_));
    let choices = if matches!(&source,LabelSource::PdfArtwork{..}|LabelSource::PictureArtwork{..}) {
        production::artwork_choices()?
    } else if is_sheet {
        production::sheet_choices()?
    } else if id == EXPORT_TECHNICAL {
        production::technical_choices()
    } else {
        production::barcode_choices()
    };
    let Some(fields) = crate::prompt::show_choices(hwnd, &request, multiline, false, &choices)
    else {
        return Ok(());
    };
    if id == EXPORT_TECHNICAL {
        let element = production::technical_element(&fields)?;
        let formats = [
            crate::FileFormat {
                id: "png",
                title: "PNG symbol or code",
                extensions: &["png"],
            },
            crate::FileFormat {
                id: "svg",
                title: "SVG vector symbol or code",
                extensions: &["svg"],
            },
            crate::FileFormat {
                id: "pdf",
                title: "PDF vector symbol or code",
                extensions: &["pdf"],
            },
        ];
        if let Some((path, index)) = format_dialog(hwnd, &formats, true) {
            let extension = formats[index].extensions[0];
            if !path
                .extension()
                .is_some_and(|value| value.eq_ignore_ascii_case(extension))
            {
                return Err(format!("Choose a file name ending in .{extension}."));
            }
            let output = match extension {
                "svg" => element.svg()?.into_bytes(),
                "pdf" => element.pdf()?,
                _ => element.png()?,
            };
            suite42_core::write_new(&path, &output)
                .map_err(|error| format!("Could not save the technical object: {error}"))?;
        }
        return Ok(());
    }
    if is_sheet {
        use crate::pdf_preview::{Action,Update};
        let mut fields=fields;
        let mut output:std::sync::Arc<[u8]>=production::sheet_output(&source,&fields)?.into();
        crate::pdf_preview::show(hwnd,output.clone(),|action,owner|{
            match action {
                Action::Save=>{
                    if let Some(path)=dialog(owner,true,"pdf","PDF label sheet","*.pdf") {
                        if !path.extension().is_some_and(|extension|extension.eq_ignore_ascii_case("pdf")){return Err("Choose a file name ending in .pdf".into());}
                        suite42_core::write_new(&path,&output).map_err(|e|e.to_string())?;
                        if let Some(directory)=&preferences{let _=crate::label_preferences::save(directory,app_name,&source,&fields);}
                        return Ok(Update::Message(crate::localization::text("Saved %@").replacen("%@",&path.file_name().unwrap_or_default().to_string_lossy(),1)));
                    }
                },
                Action::Print=>print_job(owner,&crate::PrintJob::ActualSizePdf(output.clone()))?,
                Action::Settings=>{
                    let mut request=production::sheet_request(&source)?;
                    for ((_,value),current)in request.fields.iter_mut().zip(&fields){*value=current.clone();}
                    if let Some(values)=crate::prompt::show_choices(owner,&request,multiline,false,&choices){
                        let replacement:std::sync::Arc<[u8]>=production::sheet_output(&source,&values)?.into();
                        fields=values;output=replacement.clone();return Ok(Update::Replace(replacement));
                    }
                }
            }
            Ok(Update::Keep)
        })?;
        return Ok(());
    }
    let output=production::barcode_output(&fields)?;
    if let Some(path)=dialog(hwnd,true,"png","PNG barcode","*.png") {
        if !path.extension().is_some_and(|extension|extension.eq_ignore_ascii_case("png")){return Err("Choose a file name ending in .png".into());}
        suite42_core::write_new(&path,&output).map_err(|e|format!("Could not save the production file: {e}"))?;
    }
    Ok(())
}
unsafe fn technical_command(hwnd: HWND, edit: bool) -> Result<(), String> {
    let enabled = STATE.with(|state| {
        state
            .borrow()
            .as_ref()
            .unwrap()
            .app
            .can_insert_technical_element()
    });
    if !enabled {
        return Ok(());
    }
    let (target, request) = if edit {
        let (targets, selected) = STATE.with(|state| {
            let state = state.borrow();
            let app = &state.as_ref().unwrap().app;
            (app.technical_targets(), app.selected_technical_target())
        });
        if targets.is_empty() {
            return Err("The current document has no editable symbols or codes.".into());
        }
        let selected = selected
            .and_then(|id| targets.iter().position(|target| target.id == id))
            .unwrap_or(0);
        let selected = if targets.len() == 1 {
            0
        } else {
            let labels = targets
                .iter()
                .map(|target| format!("{} — {}", target.id, target.title))
                .collect::<Vec<_>>();
            let request = crate::InputRequest {
                title: "Choose Symbol or Code",
                fields: vec![("Object", labels[selected].clone())],
            };
            let Some(values) =
                crate::prompt::show_choices(hwnd, &request, false, false, &[(0, labels.clone())])
            else {
                return Ok(());
            };
            labels
                .iter()
                .position(|label| label == &values[0])
                .ok_or("Choose an existing technical object")?
        };
        let target = &targets[selected];
        (
            Some(target.id),
            crate::production::edit_technical_request(&target.element),
        )
    } else {
        (None, crate::production::technical_request())
    };
    let Some(values) = crate::prompt::show_choices(
        hwnd,
        &request,
        false,
        false,
        &crate::production::technical_choices(),
    ) else {
        return Ok(());
    };
    let raster = crate::TechnicalRaster::render(crate::production::technical_element(&values)?)?;
    STATE.with(|state| {
        state
            .borrow_mut()
            .as_mut()
            .unwrap()
            .app
            .apply_technical_element(target, raster)
    })?;
    refresh();
    Ok(())
}

unsafe fn pick_library_image(hwnd: HWND, directory: &Path) -> Result<Option<PathBuf>, String> {
    use windows::{
        core::{w, PCWSTR},
        Win32::{
            Foundation::HWND as WinHwnd,
            System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER},
            UI::Shell::{
                Common::COMDLG_FILTERSPEC, FileOpenDialog, IFileOpenDialog, IShellItem,
                SHCreateItemFromParsingName, FOS_DONTADDTORECENT, FOS_FORCEFILESYSTEM,
                FOS_NOCHANGEDIR, SIGDN_FILESYSPATH,
            },
        },
    };
    if CoInitializeEx(null(), COINIT_APARTMENTTHREADED as u32) < 0 {
        return Err("Could not open the Windows image picker.".into());
    }
    let result = (|| -> windows::core::Result<Option<PathBuf>> {
        let picker: IFileOpenDialog =
            CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER)?;
        picker.SetOptions(
            picker.GetOptions()? | FOS_FORCEFILESYSTEM | FOS_NOCHANGEDIR | FOS_DONTADDTORECENT,
        )?;
        picker.SetTitle(w!("Insert Signature or Stamp from Library"))?;
        picker.SetFileTypes(&[COMDLG_FILTERSPEC {
            pszName: w!("Library images"),
            pszSpec: w!("*.png"),
        }])?;
        let folder = directory
            .as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect::<Vec<_>>();
        let item: IShellItem = SHCreateItemFromParsingName(PCWSTR(folder.as_ptr()), None)?;
        picker.SetFolder(&item)?;
        match picker.Show(Some(WinHwnd(hwnd))) {
            Err(error) if error.code().0 as u32 == 0x800704c7 => return Ok(None),
            other => other?,
        }
        let path = picker.GetResult()?.GetDisplayName(SIGDN_FILESYSPATH)?;
        let path_buf = PathBuf::from(OsString::from_wide(path.as_wide()));
        CoTaskMemFree(path.0.cast());
        Ok(Some(path_buf))
    })();
    CoUninitialize();
    result.map_err(|error| format!("Could not open the signature and stamp library: {error}"))
}

unsafe fn mark_command(hwnd: HWND, id: u16) -> Result<(), String> {
    let (supported, target, canvas) = STATE.with(|state| {
        let state = state.borrow();
        let app = &state.as_ref().unwrap().app;
        let target = app
            .mark_file_action()
            .filter(|&action| app.command_state(action).enabled);
        (app.supports_marks(), target, app.mark_canvas_size())
    });
    if !supported || (id != IMPORT_MARK && target.is_none() && canvas.is_none()) {
        return Ok(());
    }
    let directory = crate::marks::library_directory()?;
    if id == IMPORT_MARK {
        if let Some(source) = file_dialog(
            hwnd,
            false,
            "",
            "PNG or JPEG image",
            "*.png;*.jpg;*.jpeg",
            Some("Import Your Signature or Stamp"),
        ) {
            let request = crate::InputRequest {
                title: "Import Signature or Stamp",
                fields: vec![("Remove white paper background (yes/no)", "no".into())],
            };
            let Some(values) = crate::prompt::show_choices(
                hwnd,
                &request,
                false,
                false,
                &[(0, vec!["no".into(), "yes".into()])],
            ) else {
                return Ok(());
            };
            let cleanup = match values[0].trim().to_ascii_lowercase().as_str() {
                "yes" => true,
                "no" => false,
                _ => return Err("Choose yes or no for background cleanup.".into()),
            };
            crate::marks::import_image_with_cleanup(&source, &directory, cleanup)?;
            msg(
                hwnd,
                "Saved to the shared signature and stamp library.",
                MB_OK | MB_ICONINFORMATION,
            );
        }
    } else if let Some(path) = pick_library_image(hwnd, &directory)? {
        let path = crate::marks::library_image(&path, &directory)?;
        if let Some(target) = target {
            STATE.with(|state| {
                state
                    .borrow_mut()
                    .as_mut()
                    .unwrap()
                    .app
                    .file_action(target, &path)
            })?;
        } else if let Some((canvas_width, canvas_height)) = canvas {
            let mark = crate::marks::read_image(&path)?;
            let width = mark.width().min((canvas_width / 3).max(1));
            let height = ((f64::from(width) * f64::from(mark.height()) / f64::from(mark.width()))
                .round() as u32)
                .clamp(1, canvas_height);
            let request = crate::InputRequest {
                title: "Place Signature or Stamp",
                fields: vec![
                    ("Left (pixels)", ((canvas_width - width) / 2).to_string()),
                    ("Top (pixels)", ((canvas_height - height) / 2).to_string()),
                    ("Width (pixels)", width.to_string()),
                    ("Height (pixels)", height.to_string()),
                ],
            };
            let Some(fields) = crate::prompt::show(hwnd, &request) else {
                return Ok(());
            };
            let values = fields
                .iter()
                .map(|value| {
                    value
                        .trim()
                        .parse::<u32>()
                        .map_err(|_| "Enter whole pixel coordinates and dimensions.".to_string())
                })
                .collect::<Result<Vec<_>, _>>()?;
            STATE.with(|state| {
                let mut state = state.borrow_mut();
                let app = &mut state.as_mut().unwrap().app;
                if app.mark_canvas_size() != canvas {
                    return Err("The target page changed while choosing the mark.".into());
                }
                app.place_mark(
                    mark.width(),
                    mark.height(),
                    mark.as_raw(),
                    [values[0], values[1], values[2], values[3]],
                )
            })?;
        }
    }
    Ok(())
}

unsafe fn language_command(hwnd: HWND, index: usize) -> Result<(), String> {
    let language = *crate::localization::LANGUAGES
        .get(index)
        .ok_or("Unknown UI language")?;
    if language == crate::localization::current() || !confirm(hwnd) {
        return Ok(());
    }
    EnableWindow(hwnd, 0);
    let result = (|| -> Result<(), String> {
        let executable = std::env::current_exe().map_err(|error| error.to_string())?;
        let path = STATE.with(|state| {
            let state = state.borrow();
            let state = state.as_ref().unwrap();
            state
                .session
                .location()
                .map(|location| location.path.clone())
                .or_else(|| state.path.clone())
        });
        crate::localization::save(language)?;
        let mut process = std::process::Command::new(executable);
        process.arg("--language").arg(language.code).arg("--");
        if let Some(path) = path {
            process.arg(path);
        }
        process
            .spawn()
            .map_err(|error| format!("Could not restart this app: {error}"))?;
        STATE.with(|state| state.borrow_mut().as_mut().unwrap().close_confirmed = true);
        PostMessageW(hwnd, WM_CLOSE, 0, 0);
        Ok(())
    })();
    if result.is_err() {
        EnableWindow(hwnd, 1);
    }
    result
}
unsafe fn flush_deferred_commands() {
    if REFRESHING.with(|refreshing| refreshing.get()) {
        return;
    }
    let pending = STATE.with(|state| {
        let mut state = state.try_borrow_mut().ok()?;
        let state = state.as_mut()?;
        Some((state.hwnd, std::mem::take(&mut state.deferred_commands)))
    });
    if let Some((hwnd, commands)) = pending {
        for (id, notify) in commands {
            PostMessageW(
                hwnd,
                WM_COMMAND,
                usize::from(id) | (usize::from(notify) << 16),
                0,
            );
        }
    }
}
fn trace_command(stage: &str, id: u16, notify: u16) {
    use std::io::Write;
    static PATH: std::sync::OnceLock<Option<PathBuf>> = std::sync::OnceLock::new();
    if let Some(path) =
        PATH.get_or_init(|| std::env::var_os("OFF42_UI_TRACE_FILE").map(PathBuf::from))
    {
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
        {
            let _ = writeln!(
                file,
                "{} {stage} command={id} notify={notify} refreshing={}",
                std::process::id(),
                REFRESHING.with(|refreshing| refreshing.get())
            );
        }
    }
}
thread_local! {
    static PLAIN_EDIT_DEPTH:Cell<usize>=const{Cell::new(0)};
    static PLAIN_EDIT_CHANGED:Cell<bool>=const{Cell::new(false)};
}
// Standard multiline EDIT emits deletion and insertion EN_CHANGE notifications for
// a single replacement/paste. Deliver the final value once after the native operation.
unsafe extern "system" fn plain_edit_proc(
    h: HWND,
    m: u32,
    w: WPARAM,
    l: LPARAM,
    id: usize,
    _data: usize,
) -> LRESULT {
    if m == WM_NCDESTROY {
        RemoveWindowSubclass(h, Some(plain_edit_proc), id);
    }
    let mutation = matches!(
        m,
        WM_SETTEXT | WM_PASTE | WM_CUT | WM_CLEAR | WM_CHAR | WM_KEYDOWN | WM_UNDO | EM_REPLACESEL
    );
    if mutation {
        PLAIN_EDIT_DEPTH.with(|depth| depth.set(depth.get() + 1));
    }
    let result = DefSubclassProc(h, m, w, l);
    if mutation {
        let outer = PLAIN_EDIT_DEPTH.with(|depth| {
            depth.set(depth.get() - 1);
            depth.get() == 0
        });
        if outer && PLAIN_EDIT_CHANGED.with(|changed| changed.replace(false)) {
            command(GetParent(h), EDIT as u16, EN_CHANGE as u16);
            flush_deferred_commands();
        }
    }
    result
}

unsafe fn update_toolbar_command(id:u16,state:crate::CommandState){
    let button=STATE.with(|slot|{
        let slot=slot.borrow();let slot=slot.as_ref()?;
        slot.toolbar.iter().find(|(key,_)|*key==id).map(|(_,button)|(*button,slot.app.toolbar_command_toggle(id)))
    });
    if let Some((button,toggle))=button{
        EnableWindow(button,state.enabled as i32);
        if toggle{SendMessageW(button,BM_SETCHECK,if state.checked{BST_CHECKED}else{BST_UNCHECKED}as usize,0);}
    }
}

unsafe fn effective_command_state(id: u16, fallback: crate::CommandState) -> crate::CommandState {
    let native = STATE.with(|slot| {
        let state = slot.borrow();
        let state = state.as_ref()?;
        Some((
            state.edit,
            state.app.rich_format_command(id)?,
            !state.app.text_readonly(),
        ))
    });
    match native {
        Some((edit, command, editable)) if !edit.is_null() => {
            crate::rich_format::state(edit, command, editable && IsWindowVisible(edit) != 0)
        }
        _ => fallback,
    }
}

unsafe fn native_rich_format(
    hwnd: HWND,
    edit: HWND,
    command: crate::RichFormatCommand,
) -> Result<(), String> {
    let Some(format) = crate::rich_format::prepare(hwnd, edit, command)? else {
        SetFocus(edit);
        update_caption();
        return Ok(());
    };
    let selection = crate::rich_edit::selection(edit);
    let before = crate::rich_edit::extract(edit)?;
    let previous = REFRESHING.with(|flag| flag.replace(true));
    let result = (|| {
        crate::rich_format::apply(edit, &format)?;
        if format.affects_document(selection) {
            let bytes = crate::rich_edit::extract(edit)?;
            STATE.with(|slot| {
                slot.borrow_mut()
                    .as_mut()
                    .unwrap()
                    .app
                    .edit_rich_text(&bytes, selection)
            })?;
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = crate::rich_edit::load(edit, &before);
        crate::rich_edit::set_selection(edit, selection);
    }
    REFRESHING.with(|flag| flag.set(previous));
    flush_deferred_commands();
    SetFocus(edit);
    update_caption();
    result
}

unsafe fn native_spelling_replace(edit:HWND,range:(usize,usize),value:&str,expected:&[u16])->Result<(),String>{
    if crate::proofing::raw_text(edit)?!=expected{return Err("The document changed; check spelling again".into())}
    let before=crate::rich_edit::extract(edit)?;let selection=crate::rich_edit::selection(edit);
    let previous=REFRESHING.with(|flag|flag.replace(true));let mut prepared=false;
    let result=(||{
        let projection=STATE.with(|slot|slot.borrow_mut().as_mut().unwrap().app.prepare_rich_word(expected,range,value))?;prepared=true;
        crate::rich_edit::load(edit,&projection)?;
        let inserted:Vec<_>=value.encode_utf16().collect();let mut after=expected.to_vec();after.splice(range.0..range.1,inserted.iter().copied());
        if crate::proofing::raw_text(edit)?!=after{return Err("The spelling replacement could not be preserved".into())}
        let caret=range.0+inserted.len();crate::rich_edit::set_selection(edit,(caret,caret));
        STATE.with(|slot|{let mut slot=slot.borrow_mut();let app=&mut slot.as_mut().unwrap().app;app.finish_rich_word(true)?;app.set_rich_selection((caret,caret));Ok::<(),String>(())})?;prepared=false;
        Ok(())
    })();
    if result.is_err(){
        if prepared{let _=STATE.with(|slot|slot.borrow_mut().as_mut().unwrap().app.finish_rich_word(false));}
        let _=crate::rich_edit::load(edit,&before);crate::rich_edit::set_selection(edit,selection);
    }
    REFRESHING.with(|flag|flag.set(previous));flush_deferred_commands();update_caption();result
}

unsafe fn command(hwnd: HWND, id: u16, notify: u16) {
    trace_command("received", id, notify);
    if REFRESHING.with(|refreshing| refreshing.get()) {
        // RichEdit/TSF can pump posted messages during a model refresh. Keep
        // actual menu and toolbar clicks; ignore generated control notifications.
        if id != EDIT as u16 && id != LIST as u16 && notify == 0 {
            STATE.with(|state| {
                if let Some(state) = state.borrow_mut().as_mut() {
                    if state.deferred_commands.len() < 256 {
                        state.deferred_commands.push((id, notify));
                    }
                }
            });
        }
        return;
    }
    if id==EDIT as u16 && notify==EN_CHANGE as u16 && PLAIN_EDIT_DEPTH.with(|depth|depth.get()>0) {
        PLAIN_EDIT_CHANGED.with(|changed|changed.set(true));
        return;
    }
    // Control notifications are not app commands. In particular, RichEdit
    // emits EN_UPDATE on focus/layout changes; treating ID 101 as an action
    // reloads the editor, which emits another EN_UPDATE and starves modal UI.
    if (id == EDIT as u16 && notify != EN_CHANGE as u16)
        || (id == LIST as u16 && notify != LBN_SELCHANGE as u16)
    {
        return;
    }
    if id == CLEAR_RECENTS {
        let app = STATE.with(|state| state.borrow().as_ref().unwrap().app.name());
        if let Err(error) = crate::recents::clear(app) {
            msg(hwnd, &error, MB_OK | MB_ICONERROR);
        }
        update_recent_menu();
        return;
    }
    if (RECENT_FIRST..CLEAR_RECENTS).contains(&id) {
        let path = STATE.with(|state| {
            state
                .borrow()
                .as_ref()
                .unwrap()
                .recent_paths
                .get(usize::from(id - RECENT_FIRST))
                .cloned()
        });
        if let Some(path) = path {
            open_paths(hwnd, vec![path]);
        }
        return;
    }
    if (LANGUAGE_FIRST..LANGUAGE_FIRST + crate::localization::LANGUAGES.len() as u16).contains(&id)
    {
        if let Err(error) = language_command(hwnd, usize::from(id - LANGUAGE_FIRST)) {
            msg(hwnd, &error, MB_OK | MB_ICONERROR);
        }
        return;
    }
    if id == TOOLBAR_MORE {
        let (button, items) = STATE.with(|s| {
            let s = s.borrow();
            let s = s.as_ref().unwrap();
            let specs = s.app.command_specs();
            let items = s
                .toolbar
                .iter()
                .filter(|(_, h)| IsWindowVisible(*h) == 0)
                .filter_map(|(id, _)| {
                    specs
                        .iter()
                        .find(|spec| spec.id == *id)
                        .map(|spec| (*id, spec.title, s.app.command_state(*id)))
                })
                .collect::<Vec<_>>();
            (s.toolbar_more, items)
        });
        if !items.is_empty() {
            let menu = CreatePopupMenu();
            for (id, title, state) in items {
                let state=effective_command_state(id,state);
                let flags = MF_STRING
                    | if state.enabled { MF_ENABLED } else { MF_GRAYED }
                    | if state.checked {
                        MF_CHECKED
                    } else {
                        MF_UNCHECKED
                    };
                AppendMenuW(
                    menu,
                    flags,
                    id as usize,
                    wide(&crate::localization::menu(title)).as_ptr(),
                );
            }
            let mut r: RECT = zeroed();
            GetWindowRect(button, &mut r);
            let selected = TrackPopupMenu(
                menu,
                TPM_RETURNCMD | TPM_NONOTIFY,
                r.left,
                r.bottom,
                0,
                hwnd,
                null(),
            );
            DestroyMenu(menu);
            if selected != 0 {
                command(hwnd, selected as u16, 0);
            }
        }
        return;
    }
    if matches!(id,TOGGLE_SIDEBAR|TOGGLE_TOOLBAR|TOGGLE_STATUS|TOGGLE_RULERS){
        STATE.with(|slot|{
            let mut slot=slot.borrow_mut();let s=slot.as_mut().unwrap();if s.fullscreen.is_some(){return}
            match id{
                TOGGLE_SIDEBAR if s.app.list_visible()=>s.panels.sidebar=!s.panels.sidebar,
                TOGGLE_TOOLBAR=>s.panels.toolbar=!s.panels.toolbar,
                TOGGLE_STATUS=>s.panels.status=!s.panels.status,
                TOGGLE_RULERS if s.app.rich_ruler().is_some()=>s.panels.rulers=!s.panels.rulers,
                _=>{}
            }
        });
        let previous_focus=GetFocus();
        layout(hwnd);sync_panel_menu();remember_window_layout(hwnd);
        if previous_focus.is_null()||IsWindowVisible(previous_focus)==0{
            let target=STATE.with(|slot|{let slot=slot.borrow();let s=slot.as_ref().unwrap();if IsWindowVisible(s.table)!=0{s.table}else if IsWindowVisible(s.edit)!=0{s.edit}else{s.canvas}});SetFocus(target);
        }
        DrawMenuBar(hwnd);return;
    }
    if id==CHECK_SPELLING{
        let edit=STATE.with(|slot|{let slot=slot.borrow();let state=slot.as_ref().unwrap();(state.app.uses_rich_edit()&&!state.app.text_readonly()&&IsWindowVisible(state.edit)!=0&&state.async_open.is_none()).then_some(state.edit)});
        if let Some(edit)=edit{if let Err(error)=crate::proofing::show(hwnd,edit,|range,value,expected|native_spelling_replace(edit,range,value,expected)){msg(hwnd,&crate::localization::text(&error),MB_OK|MB_ICONERROR);}}
        update_caption();return;
    }
    if id == TOGGLE_PROOFING {
        STATE.with(|state| {
            let state = state.borrow();
            let state = state.as_ref().unwrap();
            if state.app.uses_rich_edit() {
                crate::rich_edit::set_proofing(
                    state.edit,
                    !crate::rich_edit::proofing_enabled(state.edit),
                );
            }
        });
        refresh();
        return;
    }
    if matches!(
        id,
        CONFIGURE_CALIBRATION | TOGGLE_CALIBRATION | CALIBRATION_SHEET
    ) {
        if let Err(error) = calibration_command(hwnd, id) {
            msg(hwnd, &error, MB_OK | MB_ICONERROR);
        }
        refresh();
        return;
    }
    if id == CANCEL_OPEN {
        cancel_pending_open();
        refresh();
        return;
    }
    if id==RECOVER_DOCUMENT{
        if let Err(error)=recover_document(hwnd){msg(hwnd,&error,MB_OK|MB_ICONERROR);}return;
    }
    if matches!(id, NEW | NEW_WINDOW) {
        if id == NEW && !STATE.with(|s| s.borrow().as_ref().unwrap().app.supports_new()) {
            return;
        }
        if let Err(error) = spawn_document_window(None) {
            msg(hwnd, &error, MB_OK | MB_ICONERROR);
        }
        return;
    }
    if id != LIST as u16 && (id != EDIT as u16 || notify == EN_CHANGE as u16) {
        cancel_pending_open();
    }
    if id == EDIT as u16 && notify == EN_CHANGE as u16 {
        let h = STATE.with(|s| s.borrow().as_ref().unwrap().edit);
        let rich = STATE.with(|s| s.borrow().as_ref().unwrap().app.uses_rich_edit());
        if rich {
            let result = crate::rich_edit::extract(h).and_then(|bytes| {
                let selection = crate::rich_edit::selection(h);
                STATE.with(|s| {
                    s.borrow_mut()
                        .as_mut()
                        .unwrap()
                        .app
                        .edit_rich_text(&bytes, selection)
                })
            });
            if let Err(e) = result {
                msg(hwnd, &e, MB_OK | MB_ICONERROR);
                refresh();
            } else {
                update_caption();
            }
            return;
        }
        let n = GetWindowTextLengthW(h) as usize;
        let mut b = vec![0u16; n + 1];
        GetWindowTextW(h, b.as_mut_ptr(), b.len() as i32);
        let result = match String::from_utf16(&b[..n]) {
            Ok(text) => STATE.with(|s| s.borrow_mut().as_mut().unwrap().app.edit(text)),
            Err(_) => Err("Invalid UTF-16 input cannot be saved.".into()),
        };
        if let Err(e) = result {
            msg(hwnd, &e, MB_OK | MB_ICONERROR);
            refresh()
        } else {
            let (table, grid, sparse, cell, list, items, selected) = STATE.with(|s| {
                let s = s.borrow();
                let s = s.as_ref().unwrap();
                (
                    s.table,
                    if s.app.uses_custom_grid(){None}else{s.app.table()},
                    if s.app.uses_custom_grid(){s.app.sparse_table().or_else(||s.app.table().and_then(crate::SparseTable::from_dense))}else{None},
                    s.app.selected_cell(),
                    s.list,
                    s.app.items(),
                    s.app.selected(),
                )
            });
            let _ = cell;
            STATE.with(|s|s.borrow_mut().as_mut().unwrap().sparse_grid=sparse);
            update_table(table, grid.as_deref());
            if !items.is_empty() {
                REFRESHING.with(|x| x.set(true));
                SendMessageW(list, LB_RESETCONTENT, 0, 0);
                for item in items {
                    SendMessageW(list, LB_ADDSTRING, 0, wide(&item).as_ptr() as isize);
                }
                SendMessageW(list, LB_SETCURSEL, selected, 0);
                REFRESHING.with(|x| x.set(false));
                flush_deferred_commands();
            }
            update_caption()
        }
        return;
    }
    if id == LIST as u16 && notify == LBN_SELCHANGE as u16 {
        let l = STATE.with(|s| s.borrow().as_ref().unwrap().list);
        let i = SendMessageW(l, LB_GETCURSEL, 0, 0);
        if i >= 0 {
            STATE.with(|s| {
                let mut s = s.borrow_mut();
                let a = &mut s.as_mut().unwrap().app;
                if a.grid_sheets().is_some() {
                    a.select_grid_sheet(i as usize);
                } else {
                    a.select(i as usize);
                }
            });
            refresh();
            SendMessageW(l, LB_SETCURSEL, i as usize, 0);
        }
        return;
    }
    match id {
        OPEN => {
            let (e, f, pattern) = STATE.with(|s| {
                let s = s.borrow();
                let a = &s.as_ref().unwrap().app;
                (a.extension(), a.format_name(), a.open_filter())
            });
            let formats = STATE.with(|s| s.borrow().as_ref().unwrap().app.open_formats());
            let selected = if formats.is_empty() {
                dialog(hwnd, false, e, f, &pattern)
            } else {
                format_dialog(hwnd, &formats, false).map(|v| v.0)
            };
            if let Some(path) = selected {
                open_paths(hwnd, vec![path]);
            }
        }
        SAVE => {
            save(hwnd);
        }
        SAVE_AS => {
            save_mode(hwnd, true);
        }
        EXIT => {
            SendMessageW(hwnd, WM_CLOSE, 0, 0);
            return;
        }
        FIND | REPLACE => {
            if !STATE.with(|s| s.borrow().as_ref().unwrap().app.supports_find_replace()) {
                return;
            }
            let req = if id == FIND {
                crate::InputRequest {
                    title: "Find next",
                    fields: vec![("Find", String::new())],
                }
            } else {
                crate::InputRequest {
                    title: "Replace all",
                    fields: vec![("Find", String::new()), ("Replace with", String::new())],
                }
            };
            if let Some(values) = crate::prompt::show(hwnd, &req) {
                if id == FIND {
                    let edit = STATE.with(|s| s.borrow().as_ref().unwrap().edit);
                    let rich = STATE.with(|s| s.borrow().as_ref().unwrap().app.uses_rich_edit());
                    let result = if rich {
                        crate::rich_edit::find_next(
                            edit,
                            &values[0],
                            crate::rich_edit::selection(edit).1,
                        )
                    } else {
                        let mut end: u32 = 0;
                        SendMessageW(edit, EM_GETSEL, 0, &mut end as *mut _ as isize);
                        STATE.with(|s| {
                            s.borrow()
                                .as_ref()
                                .unwrap()
                                .app
                                .find_next(&values[0], end as usize)
                        })
                    };
                    match result {
                        Ok(Some((start, end))) => {
                            SetFocus(edit);
                            if rich {
                                crate::rich_edit::set_selection(edit, (start, end));
                            } else {
                                SendMessageW(edit, EM_SETSEL, start, end as isize);
                            }
                            SendMessageW(edit, EM_SCROLLCARET, 0, 0);
                        }
                        Ok(None) => {
                            msg(hwnd, "Text not found.", MB_OK);
                        }
                        Err(e) => {
                            msg(hwnd, &e, MB_OK | MB_ICONERROR);
                        }
                    }
                    return;
                }
                let result = STATE.with(|s| {
                    s.borrow_mut()
                        .as_mut()
                        .unwrap()
                        .app
                        .replace_all(&values[0], &values[1])
                });
                match result {
                    Ok(n) => {
                        refresh();
                        msg(hwnd, &format!("Replaced {n} occurrence(s)."), MB_OK);
                    }
                    Err(e) => {
                        msg(hwnd, &e, MB_OK | MB_ICONERROR);
                    }
                }
            }
            return;
        }
        ABOUT => {
            trace_command("about_begin", id, notify);
            let text = STATE.with(|s| {
                format!(
                    "{}\n\n{}",
                    s.borrow().as_ref().unwrap().app.name(),
                    s.borrow().as_ref().unwrap().app.status()
                )
            });
            msg(hwnd, &text, MB_OK | MB_ICONINFORMATION);
            trace_command("about_end", id, notify);
        }
        _ => {
            let (edit, rich) = STATE.with(|s| {
                let s = s.borrow();
                let s = s.as_ref().unwrap();
                (s.edit, s.app.uses_rich_edit())
            });
            if rich {
                let selection = crate::rich_edit::selection(edit);
                STATE.with(|s| {
                    s.borrow_mut()
                        .as_mut()
                        .unwrap()
                        .app
                        .set_rich_selection(selection)
                });
            }
            if matches!(
                id,
                CREATE_LABEL_SHEET
                    | CREATE_BARCODE
                    | IMPORT_MARK
                    | INSERT_MARK
                    | EXPORT_TECHNICAL
                    | INSERT_TECHNICAL
                    | EDIT_TECHNICAL
            ) {
                let result = match id {
                    CREATE_LABEL_SHEET | CREATE_BARCODE | EXPORT_TECHNICAL => {
                        production_command(hwnd, id)
                    }
                    INSERT_TECHNICAL | EDIT_TECHNICAL => {
                        technical_command(hwnd, id == EDIT_TECHNICAL)
                    }
                    _ => mark_command(hwnd, id),
                };
                if let Err(error) = result {
                    msg(hwnd, &error, MB_OK | MB_ICONERROR);
                }
                if id == INSERT_MARK {
                    refresh();
                }
                return;
            }
            let (file_action, request, enabled) = STATE.with(|s| {
                let s = s.borrow();
                let a = &s.as_ref().unwrap().app;
                (
                    a.file_actions().iter().find(|a| a.id == id).copied(),
                    a.input_request(id),
                    a.command_state(id).enabled,
                )
            });
            if !effective_command_state(id,crate::CommandState{enabled,checked:false}).enabled {
                return;
            }
            let native_format=STATE.with(|slot|slot.borrow().as_ref().unwrap().app.rich_format_command(id));
            if let Some(format)=native_format{
                if let Err(error)=native_rich_format(hwnd,edit,format){msg(hwnd,&error,MB_OK|MB_ICONERROR);}
                return;
            }
            if let Some(a) = file_action {
                let settings =
                    STATE.with(|s| s.borrow().as_ref().unwrap().app.file_input_request(id));
                let settings = if let Some(request) = settings {
                    let choices = STATE.with(|s| s.borrow().as_ref().unwrap().app.input_choices(id));
                    match crate::prompt::show_choices(hwnd, &request, false, false, &choices) {
                        Some(values) => Some(values),
                        None => return,
                    }
                } else {
                    None
                };
                let a = if let Some(values) = &settings {
                    match STATE.with(|s| {
                        s.borrow().as_ref().unwrap().app.configured_file_action(id, values)
                    }) {
                        Ok(action) => action,
                        Err(error) => {
                            msg(hwnd, &error, MB_OK | MB_ICONWARNING);
                            return;
                        }
                    }
                } else {
                    a
                };
                if let Some(p) = dialog(hwnd, a.save, a.extension, a.format, a.pattern) {
                    let problem = if !a.save {
                        None
                    } else if p
                        .extension()
                        .and_then(|x| x.to_str())
                        .is_none_or(|x| !x.eq_ignore_ascii_case(a.extension))
                    {
                        Some(format!("Choose a file name ending in .{}.", a.extension))
                    } else if p.exists() {
                        Some("A file with this name already exists. Exports never replace files; choose a new name.".into())
                    } else {
                        None
                    };
                    if let Some(problem) = problem {
                        msg(hwnd, &problem, MB_OK | MB_ICONWARNING);
                        return;
                    }
                    let r = STATE.with(|s| {
                        let mut state = s.borrow_mut();
                        let app = &mut state.as_mut().unwrap().app;
                        if let Some(values) = &settings {
                            app.file_action_with_input(id, &p, values)
                        } else {
                            app.file_action(id, &p)
                        }
                    });
                    if let Err(e) = r {
                        msg(hwnd, &e, MB_OK | MB_ICONERROR);
                    }
                }
                refresh();
                return;
            }
            let clipboard_request =
                STATE.with(|s| s.borrow().as_ref().unwrap().app.clipboard_request(id));
            if let Some(request) = clipboard_request {
                let result = match request {
                    crate::ClipboardRequest::WriteText(text) => {
                        crate::clipboard::write_text(hwnd, &text)
                    }
                    crate::ClipboardRequest::WriteData { format, bytes } => {
                        crate::clipboard::write_data(hwnd, &format, &bytes)
                    }
                    crate::ClipboardRequest::WriteDataAndText {
                        format,
                        bytes,
                        text,
                    } => crate::clipboard::write_data_and_text(hwnd, &format, &bytes, &text),
                    crate::ClipboardRequest::ReadDataOrText(format) => {
                        match crate::clipboard::read_data_or_text(hwnd, &format) {
                            Ok(crate::clipboard::ClipboardValue::Data(bytes)) => STATE.with(|s| {
                                s.borrow_mut()
                                    .as_mut()
                                    .unwrap()
                                    .app
                                    .clipboard_data(id, bytes)
                            }),
                            Ok(crate::clipboard::ClipboardValue::Text(text)) => STATE.with(|s| {
                                s.borrow_mut()
                                    .as_mut()
                                    .unwrap()
                                    .app
                                    .clipboard_text(id, text)
                            }),
                            Err(error) => Err(error),
                        }
                    }
                    crate::ClipboardRequest::ReadData(format) => {
                        crate::clipboard::read_data(hwnd, &format).and_then(|bytes| {
                            STATE.with(|s| {
                                s.borrow_mut()
                                    .as_mut()
                                    .unwrap()
                                    .app
                                    .clipboard_data(id, bytes)
                            })
                        })
                    }
                    crate::ClipboardRequest::ReadText => crate::clipboard::read_text(hwnd)
                        .and_then(|text| {
                            STATE.with(|s| {
                                s.borrow_mut()
                                    .as_mut()
                                    .unwrap()
                                    .app
                                    .clipboard_text(id, text)
                            })
                        }),
                };
                if let Err(e) = result {
                    msg(hwnd, &e, MB_OK | MB_ICONERROR);
                }
                refresh();
                return;
            }
            let preview = STATE.with(|s| s.borrow().as_ref().unwrap().app.rich_preview(id));
            if let Some(geometry) = preview {
                let edit = STATE.with(|s| s.borrow().as_ref().unwrap().edit);
                if let Err(e) = crate::rich_preview::open(hwnd, edit, geometry) {
                    msg(hwnd, &e, MB_OK | MB_ICONERROR);
                }
                return;
            }
            let job = STATE.with(|s| s.borrow().as_ref().unwrap().app.print_job(id));
            if let Some(job) = job {
                if let Err(e) = print_job(hwnd, &job) {
                    msg(hwnd, &e, MB_OK | MB_ICONERROR);
                }
                return;
            }
            let dialog_request =
                STATE.with(|s| s.borrow().as_ref().unwrap().app.dialog_request(id));
            if let Some(request) = dialog_request {
                let result = match request {
                    crate::DialogRequest::Confirmation{title,text}=>Some(crate::DialogResult::Confirmed(
                        MessageBoxW(hwnd,wide(&text).as_ptr(),wide(&crate::localization::text(title)).as_ptr(),MB_YESNO|MB_ICONQUESTION)==IDYES
                    )),
                    crate::DialogRequest::BatchFolder{input,title}=>{
                        match pick_multiple_files(hwnd,input){
                            Ok(Some(inputs)) if !inputs.is_empty()=>pick_folder(hwnd,title).map(|output|crate::DialogResult::Files{inputs,output:output.into()}),
                            Ok(_)=>None,Err(error)=>{msg(hwnd,&error,MB_OK|MB_ICONERROR);None}
                        }
                    },
                    crate::DialogRequest::CombineFiles{input,output}=>{
                        match pick_multiple_files(hwnd,input){
                            Ok(Some(inputs))if !inputs.is_empty()=>{
                                if let Some(path)=file_dialog(hwnd,true,output.extension,output.format,output.pattern,Some(output.title)){
                                    if path.exists(){msg(hwnd,"This file already exists; choose a new output file",MB_OK|MB_ICONWARNING);None}
                                    else if path.extension().and_then(|e|e.to_str()).is_none_or(|e|!e.eq_ignore_ascii_case(output.extension)){msg(hwnd,&format!("Choose a file name ending in .{}",output.extension),MB_OK|MB_ICONWARNING);None}
                                    else{Some(crate::DialogResult::Files{inputs,output:path})}
                                }else{None}
                            },Ok(_)=>None,Err(error)=>{msg(hwnd,&error,MB_OK|MB_ICONERROR);None}
                        }
                    },
                    crate::DialogRequest::Report(report)=>{crate::report::show(hwnd,id,report);None},
                    crate::DialogRequest::Information { title, text } => {
                        MessageBoxW(hwnd,wide(&text).as_ptr(),wide(&crate::localization::text(title)).as_ptr(),MB_OK|MB_ICONINFORMATION);
                        None
                    }
                    crate::DialogRequest::Color(c) => {
                        crate::dialogs::color(hwnd, c).map(crate::DialogResult::Color)
                    }
                    crate::DialogRequest::Font => {
                        crate::dialogs::font(hwnd).map(crate::DialogResult::Font)
                    }
                    crate::DialogRequest::PageSetup(margins) => {
                        crate::dialogs::page_setup(hwnd, margins)
                            .map(crate::DialogResult::PageSetup)
                    }
                };
                if let Some(result) = result {
                    if let Err(e) = STATE.with(|s| {
                        s.borrow_mut()
                            .as_mut()
                            .unwrap()
                            .app
                            .dialog_result(id, result)
                    }) {
                        msg(hwnd, &e, MB_OK | MB_ICONERROR);
                    }
                }
                refresh();
                return;
            }
            if let Some(mut req) = request {
                let folders =
                    STATE.with(|s| s.borrow().as_ref().unwrap().app.input_folder_fields(id));
                for (index, title) in folders {
                    let Some(path) = pick_folder(hwnd, title) else {
                        return;
                    };
                    let Some(field) = req.fields.get_mut(index) else {
                        return;
                    };
                    field.1 = path;
                }
                let (multiline, password) = STATE.with(|s| {
                    let s = s.borrow();
                    let a = &s.as_ref().unwrap().app;
                    (a.input_multiline_fields(id), a.password_input(id))
                });
                let choices = STATE.with(|s| s.borrow().as_ref().unwrap().app.input_choices(id));
                if let Some(mut values) =
                    crate::prompt::show_choices_for_fields(hwnd, &req, &multiline, password, &choices)
                {
                    let r = STATE.with(|s| {
                        s.borrow_mut()
                            .as_mut()
                            .unwrap()
                            .app
                            .input_action(id, &values)
                    });
                    if password {
                        for value in &mut values {
                            value.as_bytes_mut().fill(0);
                        }
                    }
                    if let Err(e) = r {
                        msg(hwnd, &e, MB_OK | MB_ICONERROR);
                    } else if let Some(path) =
                        STATE.with(|s| s.borrow_mut().as_mut().unwrap().app.take_pending_open())
                    {
                        if confirm(hwnd) {
                            if let Err(e) = open_document_path(path) {
                                msg(hwnd, &e, MB_OK | MB_ICONERROR);
                            }
                        } else {
                            STATE
                                .with(|s| s.borrow_mut().as_mut().unwrap().app.cancel_async_open());
                        }
                    }
                }
                refresh();
                apply_requested_rich_selection();
                return;
            }
            let (edit, rich) = STATE.with(|s| {
                let s = s.borrow();
                let s = s.as_ref().unwrap();
                (s.edit, s.app.uses_rich_edit())
            });
            if rich {
                let selection = crate::rich_edit::selection(edit);
                STATE.with(|s| {
                    s.borrow_mut()
                        .as_mut()
                        .unwrap()
                        .app
                        .set_rich_selection(selection)
                });
            }
            let r = STATE.with(|s| s.borrow_mut().as_mut().unwrap().app.action(id));
            if let Err(e) = r {
                msg(hwnd, &e, MB_OK | MB_ICONERROR);
            } else {
                let url = STATE.with(|s| s.borrow_mut().as_mut().unwrap().app.take_external_url());
                if let Some(url) = url {
                    let valid = url.len() <= 4096
                        && !url.chars().any(|c| c.is_control() || c.is_whitespace())
                        && (url.starts_with("https://")
                            || url.starts_with("http://")
                            || url.starts_with("mailto:"));
                    if !valid {
                        msg(
                            hwnd,
                            "This link has an unsupported or invalid address.",
                            MB_OK | MB_ICONERROR,
                        );
                    } else {
                        let target = wide(&url);
                        if ShellExecuteW(
                            hwnd,
                            null(),
                            target.as_ptr(),
                            null(),
                            null(),
                            SW_SHOWNORMAL,
                        ) as isize
                            <= 32
                        {
                            msg(hwnd, "Could not open this link.", MB_OK | MB_ICONERROR);
                        }
                    }
                }
                let pending =
                    STATE.with(|s| s.borrow_mut().as_mut().unwrap().app.take_pending_open());
                if let Some(path) = pending {
                    if confirm(hwnd) {
                        if let Err(e) = open_document_path(path) {
                            msg(hwnd, &e, MB_OK | MB_ICONERROR);
                        }
                    }
                }
            }
        }
    }
    refresh();
    apply_requested_rich_selection();
    if id == UNDO || id == REDO {
        let (edit, selection) = STATE.with(|s| {
            let s = s.borrow();
            let s = s.as_ref().unwrap();
            (s.edit, s.app.rich_selection_to_restore())
        });
        if let Some(selection) = selection {
            crate::rich_edit::set_selection(edit, selection);
        }
    }
}
unsafe fn sync_panel_menu(){
    let (menu,items)=STATE.with(|slot|{let slot=slot.borrow();let s=slot.as_ref().unwrap();(s.menu,[(TOGGLE_SIDEBAR,s.app.list_visible(),s.panels.sidebar),(TOGGLE_TOOLBAR,!s.toolbar.is_empty(),s.panels.toolbar),(TOGGLE_STATUS,true,s.panels.status),(TOGGLE_RULERS,s.app.rich_ruler().is_some(),s.panels.rulers)])});
    for (id,available,shown) in items{
        EnableMenuItem(menu,id as u32,MF_BYCOMMAND|if available{MF_ENABLED}else{MF_GRAYED});
        CheckMenuItem(menu,id as u32,MF_BYCOMMAND|if available&&shown{MF_CHECKED}else{MF_UNCHECKED});
    }
}
unsafe fn update_caption() {
    let (h, status, menu, title, info, u, r, actions) = STATE.with(|s| {
        let s = s.borrow();
        let s = s.as_ref().unwrap();
        (
            s.hwnd,
            s.status,
            s.menu,
            format!(
                "{}{} - {}",
                s.path
                    .as_ref()
                    .and_then(|p| p.file_name())
                    .map(|x| x.to_string_lossy().into_owned())
                    .unwrap_or_else(|| crate::localization::text("Untitled")),
                if s.dirty() { " *" } else { "" },
                s.app.name()
            ),
            s.status_text(),
            s.app.can_undo(),
            s.app.can_redo(),
            s.app
                .actions()
                .iter()
                .chain(s.app.view_actions().iter())
                .map(|&(id, _)| (id, s.app.command_state(id)))
                .chain(
                    s.app
                        .file_actions()
                        .iter()
                        .map(|action| (action.id, s.app.command_state(action.id))),
                )
                .chain(
                    s.app
                        .command_specs()
                        .into_iter()
                        .map(|c| (c.id, s.app.command_state(c.id))),
                )
                .collect::<Vec<_>>(),
        )
    });
    for (id, state) in actions {
        let state=effective_command_state(id,state);
        EnableMenuItem(
            menu,
            id as u32,
            MF_BYCOMMAND | if state.enabled { MF_ENABLED } else { MF_GRAYED },
        );
        CheckMenuItem(
            menu,
            id as u32,
            MF_BYCOMMAND
                | if state.checked {
                    MF_CHECKED
                } else {
                    MF_UNCHECKED
                },
        );
        update_toolbar_command(id,state);
    }
    SetWindowTextW(h, wide(&title).as_ptr());
    SetWindowTextW(status, wide(&info).as_ptr());
    EnableMenuItem(
        menu,
        UNDO as u32,
        MF_BYCOMMAND | if u { MF_ENABLED } else { MF_GRAYED },
    );
    EnableMenuItem(
        menu,
        REDO as u32,
        MF_BYCOMMAND | if r { MF_ENABLED } else { MF_GRAYED },
    );
    let opening = STATE.with(|s| s.borrow().as_ref().is_some_and(|s| s.async_open.is_some()));
    EnableMenuItem(
        menu,
        CANCEL_OPEN as u32,
        MF_BYCOMMAND | if opening { MF_ENABLED } else { MF_GRAYED },
    );
    sync_panel_menu();
    DrawMenuBar(h);
}
unsafe fn select_grid_cell(row: usize, col: Option<usize>) {
    let next = STATE.with(|s| {
        crate::grid_selection_event(
            s.borrow().as_ref().unwrap().app.selected_cell(),
            Some(row),
            col,
            REFRESHING.with(|x| x.get()),
        )
    });
    if let Some((row, col)) = next {
        STATE.with(|s| s.borrow_mut().as_mut().unwrap().app.select_cell(row, col));
        let (edit, text) = STATE.with(|s| {
            let s = s.borrow();
            let s = s.as_ref().unwrap();
            (s.edit, s.app.text())
        });
        let previous = REFRESHING.with(|x| x.replace(true));
        SetWindowTextW(edit, wide(&text).as_ptr());
        REFRESHING.with(|x| x.set(previous));
        flush_deferred_commands();
        update_caption();
    }
}
unsafe extern "system" fn proc(h: HWND, m: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    match m {
        WM_TIMER if w == 42 => {
            poll_pending_open(h);
            if STATE.with(|s| s.borrow().as_ref().is_some_and(|s| s.async_open.is_some())) {
                update_caption();
            }
            let changed = STATE.with(|s| {
                s.borrow_mut()
                    .as_mut()
                    .is_some_and(|s| s.app.poll_background())
            });
            poll_recovery();
            if changed {
                refresh();
            }
            let interval = STATE.with(|s| {
                s.borrow()
                    .as_ref()
                    .map_or(100, |s| s.app.background_poll_ms().clamp(16, 1000))
            });
            POLL_MS.with(|previous| {
                if previous.get() != interval {
                    SetTimer(h, 42, interval, None);
                    previous.set(interval);
                }
            });
            if !crate::report::is_open() && IsWindowEnabled(h)!=0{
                let report=STATE.with(|s|s.borrow_mut().as_mut().and_then(|s|s.app.take_pending_report()));
                if let Some((id,report))=report{crate::report::show(h,id,report);}
            }
            if !crate::report::is_open() && IsWindowEnabled(h)!=0{
                let path=STATE.with(|s|s.borrow_mut().as_mut().and_then(|s|s.app.take_pending_new_window()));
                if let Some(path)=path{if let Err(error)=spawn_document_window(Some(&path)){msg(h,&error,MB_OK|MB_ICONERROR);}}
            }
            if !crate::report::is_open() && IsWindowEnabled(h)!=0 {
                let command=STATE.with(|s|s.borrow_mut().as_mut().and_then(|s|s.app.take_pending_command()));
                if let Some(id)=command.filter(|id|crate::application_command_id(*id)||*id==CREATE_LABEL_SHEET) { PostMessageW(h,WM_COMMAND,id as usize,0); }
            }
            sync_rulers(h);
            let pending = STATE.with(|s| {
                s.borrow_mut()
                    .as_mut()
                    .and_then(|s| (!crate::report::is_open()&&IsWindowEnabled(h)!=0).then(||s.app.take_pending_print_job()).flatten())
            });
            if let Some(result) = pending {
                match result {
                    Ok(job) => {
                        if let Err(e) = print_job(h, &job) {
                            msg(h, &e, MB_OK | MB_ICONERROR);
                        }
                    }
                    Err(e) => {
                        msg(h, &e, MB_OK | MB_ICONERROR);
                    }
                }
                update_caption();
            }
            0
        }
        WM_COMMAND => {
            let id = (w & 0xffff) as usize;
            if id != EDIT && !REFRESHING.with(|r| r.get()) {
                let table = STATE.with(|s| {
                    s.borrow()
                        .as_ref()
                        .and_then(|s| s.app.uses_custom_grid().then_some(s.table))
                });
                if let Some(table) = table.filter(|h| !h.is_null()) {
                    if !crate::grid::finish_pending(table) {
                        return 0;
                    }
                }
            }
            command(h, (w & 0xffff) as u16, (w >> 16) as u16);
            flush_deferred_commands();
            0
        }
        WM_NOTIFY => {
            if !REFRESHING.with(|x| x.get()) && !l.eq(&0) {
                let header = &*(l as *const NMHDR);
                if header.idFrom==EDIT&&header.code==0x702 {
                    let selection=crate::rich_edit::selection(header.hwndFrom);
                    STATE.with(|slot|slot.borrow_mut().as_mut().unwrap().app.set_rich_selection(selection));
                    update_caption();
                    return 0;
                }
                if header.idFrom == TABLE {
                    if header.code == NM_CUSTOMDRAW {
                        let draw = &mut *(l as *mut NMLVCUSTOMDRAW);
                        if draw.nmcd.dwDrawStage == CDDS_PREPAINT {
                            return CDRF_NOTIFYITEMDRAW as isize;
                        }
                        if draw.nmcd.dwDrawStage == CDDS_ITEMPREPAINT {
                            return CDRF_NOTIFYSUBITEMDRAW as isize;
                        }
                        if draw.nmcd.dwDrawStage == (CDDS_ITEMPREPAINT | CDDS_SUBITEM) {
                            let row = draw.nmcd.dwItemSpec;
                            let col = draw.iSubItem.max(0) as usize;
                            let (style, selection) = STATE.with(|s| {
                                let s = s.borrow();
                                let a = &s.as_ref().unwrap().app;
                                (a.grid_cell_style(row, col), a.grid_selection())
                            });
                            let rgb =
                                |c: [u8; 3]| c[0] as u32 | (c[1] as u32) << 8 | (c[2] as u32) << 16;
                            if let Some(style) = style {
                                if style.bold
                                    || style.italic
                                    || style.merged.is_some()
                                    || style.borders != 0
                                {
                                    if let Some(merged) = style.merged {
                                        if row != merged.top || col != merged.left {
                                            return CDRF_SKIPDEFAULT as isize;
                                        }
                                    }
                                    let mut rect = RECT {
                                        left: LVIR_BOUNDS as i32,
                                        top: col as i32,
                                        right: 0,
                                        bottom: 0,
                                    };
                                    if SendMessageW(
                                        header.hwndFrom,
                                        LVM_GETSUBITEMRECT,
                                        row,
                                        &mut rect as *mut _ as isize,
                                    ) != 0
                                    {
                                        if let Some(merged) = style.merged {
                                            let mut last = RECT {
                                                left: LVIR_BOUNDS as i32,
                                                top: merged.right as i32,
                                                right: 0,
                                                bottom: 0,
                                            };
                                            if SendMessageW(
                                                header.hwndFrom,
                                                LVM_GETSUBITEMRECT,
                                                merged.bottom,
                                                &mut last as *mut _ as isize,
                                            ) != 0
                                            {
                                                rect.right = last.right;
                                                rect.bottom = last.bottom;
                                            }
                                        }
                                        let selected = selection.is_some_and(|r| {
                                            row >= r.top
                                                && row <= r.bottom
                                                && col >= r.left
                                                && col <= r.right
                                        });
                                        let bg = if selected {
                                            [190, 215, 245]
                                        } else {
                                            style.background.unwrap_or([255, 255, 255])
                                        };
                                        let brush = CreateSolidBrush(rgb(bg));
                                        FillRect(draw.nmcd.hdc, &rect, brush);
                                        DeleteObject(brush as _);
                                        if style.borders != 0 {
                                            let pen = CreatePen(PS_SOLID, 1, rgb([0, 0, 0]));
                                            let old = SelectObject(draw.nmcd.hdc, pen as _);
                                            let line = |x1, y1, x2, y2| {
                                                MoveToEx(draw.nmcd.hdc, x1, y1, null_mut());
                                                LineTo(draw.nmcd.hdc, x2, y2);
                                            };
                                            if style.borders & 1 != 0 {
                                                line(rect.left, rect.top, rect.right, rect.top);
                                            }
                                            if style.borders & 2 != 0 {
                                                line(
                                                    rect.right - 1,
                                                    rect.top,
                                                    rect.right - 1,
                                                    rect.bottom,
                                                );
                                            }
                                            if style.borders & 4 != 0 {
                                                line(
                                                    rect.left,
                                                    rect.bottom - 1,
                                                    rect.right,
                                                    rect.bottom - 1,
                                                );
                                            }
                                            if style.borders & 8 != 0 {
                                                line(rect.left, rect.top, rect.left, rect.bottom);
                                            }
                                            SelectObject(draw.nmcd.hdc, old);
                                            DeleteObject(pen as _);
                                        }
                                        let text = STATE.with(|s| {
                                            s.borrow()
                                                .as_ref()
                                                .unwrap()
                                                .grid_cache
                                                .as_ref()
                                                .and_then(|g| g.get(row))
                                                .and_then(|r| r.get(col))
                                                .cloned()
                                                .unwrap_or_default()
                                        });
                                        let mut lf: LOGFONTW = zeroed();
                                        GetObjectW(
                                            GetStockObject(DEFAULT_GUI_FONT),
                                            size_of::<LOGFONTW>() as i32,
                                            &mut lf as *mut _ as _,
                                        );
                                        if style.bold {
                                            lf.lfWeight = 700;
                                        }
                                        if style.italic {
                                            lf.lfItalic = 1;
                                        }
                                        let font = CreateFontIndirectW(&lf);
                                        let old = SelectObject(draw.nmcd.hdc, font as _);
                                        SetBkMode(draw.nmcd.hdc, TRANSPARENT as i32);
                                        SetTextColor(
                                            draw.nmcd.hdc,
                                            rgb(style.foreground.unwrap_or([0, 0, 0])),
                                        );
                                        let text: Vec<u16> = text.encode_utf16().collect();
                                        if !text.is_empty() {
                                            ExtTextOutW(
                                                draw.nmcd.hdc,
                                                rect.left + 3,
                                                rect.top + 2,
                                                ETO_CLIPPED,
                                                &rect,
                                                text.as_ptr(),
                                                text.len() as u32,
                                                null(),
                                            );
                                        }
                                        SelectObject(draw.nmcd.hdc, old);
                                        DeleteObject(font as _);
                                        return CDRF_SKIPDEFAULT as isize;
                                    }
                                }
                                if let Some(c) = style.foreground {
                                    draw.clrText = rgb(c);
                                }
                                if let Some(c) = style.background {
                                    draw.clrTextBk = rgb(c);
                                }
                            }
                            if selection.is_some_and(|r| {
                                row >= r.top && row <= r.bottom && col >= r.left && col <= r.right
                            }) {
                                draw.clrTextBk = rgb([190, 215, 245]);
                            }
                            return CDRF_DODEFAULT as isize;
                        }
                    }
                    if header.code == NM_CLICK {
                        let n = &*(l as *const NMITEMACTIVATE);
                        if n.iItem >= 0 && n.iSubItem >= 0 {
                            let row = n.iItem as usize;
                            let col = n.iSubItem as usize;
                            let anchor =
                                STATE.with(|s| s.borrow().as_ref().unwrap().app.selected_cell());
                            if GetKeyState(VK_SHIFT as i32) < 0 {
                                if let Some((ar, ac)) = anchor {
                                    STATE.with(|s| {
                                        s.borrow_mut().as_mut().unwrap().app.select_grid_range(
                                            crate::GridRange {
                                                top: ar.min(row),
                                                left: ac.min(col),
                                                bottom: ar.max(row),
                                                right: ac.max(col),
                                            },
                                        )
                                    });
                                }
                            } else {
                                select_grid_cell(row, Some(col));
                                STATE.with(|s| {
                                    s.borrow_mut().as_mut().unwrap().app.select_grid_range(
                                        crate::GridRange {
                                            top: row,
                                            left: col,
                                            bottom: row,
                                            right: col,
                                        },
                                    )
                                });
                            }
                            InvalidateRect(header.hwndFrom, null(), 0);
                        }
                    } else if header.code == LVN_ITEMCHANGED {
                        if GetKeyState(VK_SHIFT as i32) < 0 {
                            return 0;
                        }
                        let n = &*(l as *const NMLISTVIEW);
                        if n.iItem >= 0
                            && n.uChanged & LVIF_STATE != 0
                            && n.uNewState & LVIS_SELECTED != 0
                            && n.uOldState & LVIS_SELECTED == 0
                        {
                            select_grid_cell(n.iItem as usize, None);
                        }
                    }
                }
            }
            0
        }
        crate::rich_ruler::BEGIN => {
            let edit = STATE.with(|s| s.borrow().as_ref().unwrap().edit);
            let selection = crate::rich_edit::selection(edit);
            STATE.with(|s| {
                s.borrow_mut()
                    .as_mut()
                    .unwrap()
                    .app
                    .set_rich_selection(selection)
            });
            0
        }
        crate::rich_ruler::APPLY => {
            if l == 0 {
                return 0;
            }
            let command = &*(l as *const crate::rich_ruler::Command);
            let action = STATE.with(|s| {
                let s = s.borrow();
                let ruler = s.as_ref().unwrap().app.rich_ruler()?;
                let page = ruler.page;
                let paragraph = ruler.paragraph;
                let value = command.position;
                Some(match command.kind {
                    0 => crate::RichRulerAction::LeftMargin(value),
                    1 => crate::RichRulerAction::RightMargin(page.width_twips - value),
                    2 => crate::RichRulerAction::TopMargin(value),
                    3 => crate::RichRulerAction::BottomMargin(page.height_twips - value),
                    4 => crate::RichRulerAction::LeftIndent(value - page.margin_left_twips),
                    5 => crate::RichRulerAction::RightIndent(
                        page.width_twips - page.margin_right_twips - value,
                    ),
                    6 => crate::RichRulerAction::FirstIndent(
                        value - page.margin_left_twips - paragraph?.left_twips,
                    ),
                    7 => crate::RichRulerAction::AddTab(value - page.margin_left_twips),
                    8 => crate::RichRulerAction::MoveTab {
                        from: command.from,
                        to: value - page.margin_left_twips,
                    },
                    9 => crate::RichRulerAction::RemoveTab(command.from),
                    _ => return None,
                })
            });
            if let Some(action) = action {
                cancel_pending_open();
                let result = STATE.with(|s| {
                    s.borrow_mut()
                        .as_mut()
                        .unwrap()
                        .app
                        .rich_ruler_action(action)
                });
                if let Err(e) = result {
                    msg(h, &e, MB_OK | MB_ICONERROR);
                }
                refresh();
            }
            0
        }
        WM_SIZE => {
            if STATE.with(|s| s.borrow().as_ref().is_some_and(|s| !s.edit.is_null())) {
                layout(h)
            }
            0
        }
        WM_DROPFILES => {
            let drop = w as windows_sys::Win32::UI::Shell::HDROP;
            let count = DragQueryFileW(drop, u32::MAX, null_mut(), 0);
            let mut paths = Vec::new();
            for index in 0..count {
                let length = DragQueryFileW(drop, index, null_mut(), 0);
                let mut buffer = vec![0; length as usize + 1];
                let copied = DragQueryFileW(drop, index, buffer.as_mut_ptr(), buffer.len() as u32);
                if copied == length && copied != 0 {
                    paths.push(PathBuf::from(OsString::from_wide(
                        &buffer[..copied as usize],
                    )));
                }
            }
            DragFinish(drop);
            if !paths.is_empty() {
                open_paths(h, paths);
            }
            0
        }
        WM_DPICHANGED => {
            let r = &*(l as *const RECT);
            SetWindowPos(
                h,
                null_mut(),
                r.left,
                r.top,
                r.right - r.left,
                r.bottom - r.top,
                SWP_NOZORDER | SWP_NOACTIVATE,
            );
            0
        }
        WM_CLOSE => {
            let table = STATE.with(|s| {
                s.borrow()
                    .as_ref()
                    .and_then(|s| s.app.uses_custom_grid().then_some(s.table))
            });
            if let Some(table) = table.filter(|h| !h.is_null()) {
                if !crate::grid::finish_pending(table) {
                    return 0;
                }
            }
            let confirmed = STATE.with(|state| {
                state
                    .borrow()
                    .as_ref()
                    .is_some_and(|state| state.close_confirmed)
            });
            if confirmed || confirm(h) {
                cancel_pending_open();
                remember_window_layout(h);
                STATE.with(|slot|{if let Some(state)=slot.borrow_mut().as_mut(){state.clear_recovery();}});
                DestroyWindow(h);
            }
            0
        }
        WM_DESTROY => {
            KillTimer(h, 42);
            PostQuitMessage(0);
            0
        }
        _ => DefWindowProcW(h, m, w, l),
    }
}
unsafe fn item(m: HMENU, id: u16, t: &str) {
    AppendMenuW(
        m,
        MF_STRING,
        id as usize,
        wide(&crate::localization::menu(t)).as_ptr(),
    );
}
struct ComApartment;
impl Drop for ComApartment {
    fn drop(&mut self) {
        unsafe {
            CoUninitialize();
        }
    }
}

pub fn run(app: Box<dyn App>) {
    unsafe {
        let invalid=app.actions().iter().map(|a|a.0).chain(app.file_actions().iter().map(|a|a.id)).chain(app.view_actions().iter().map(|a|a.0)).find(|id|!crate::application_command_id(*id));
        if let Some(id)=invalid{msg(null_mut(),&format!("Application command {id} conflicts with a native host identifier"),MB_OK|MB_ICONERROR);return;}
        if CoInitializeEx(null(), COINIT_APARTMENTTHREADED as u32) < 0 {
            msg(
                null_mut(),
                "Could not initialize Windows text and document services.",
                MB_OK | MB_ICONERROR,
            );
            return;
        }
        let _apartment = ComApartment;
        SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let instance = GetModuleHandleW(null());
        let class = wide("Suite42Native");
        let wc = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(proc),
            hInstance: instance,
            hCursor: LoadCursorW(null_mut(), IDC_ARROW),
            hbrBackground: (COLOR_WINDOW + 1) as HBRUSH,
            lpszClassName: class.as_ptr(),
            ..zeroed()
        };
        if RegisterClassW(&wc) == 0 {
            return;
        }
        let canvas_class = wide("Suite42Canvas");
        let cc = WNDCLASSW {
            style: CS_DBLCLKS,
            lpfnWndProc: Some(canvas_proc),
            hInstance: instance,
            hbrBackground: (COLOR_WINDOW + 1) as HBRUSH,
            lpszClassName: canvas_class.as_ptr(),
            ..zeroed()
        };
        RegisterClassW(&cc);
        let bar = CreateMenu();
        let file = CreatePopupMenu();
        let print_command=app.default_print_command();
        if app.supports_new() {
            item(file, NEW, "&New\tCtrl+N");
        }
        item(file, NEW_WINDOW, "New &Window\tCtrl+Shift+N");
        item(file, OPEN, "&Open...\tCtrl+O");
        let recent_menu = CreatePopupMenu();
        AppendMenuW(
            file,
            MF_POPUP,
            recent_menu as usize,
            wide(&crate::localization::menu("Open Recent")).as_ptr(),
        );
        item(file, CANCEL_OPEN, "Cancel opening\tEsc");
        item(file, SAVE, "&Save\tCtrl+S");
        item(file, SAVE_AS, "Save &As...\tCtrl+Shift+S");
        if app.supports_recovery(){item(file,RECOVER_DOCUMENT,"Recover Unsaved Document…");}
        if let Some(id)=print_command{item(file,id,"&Print…\tCtrl+P");}
        for a in app.file_actions() {
            item(file, a.id, a.title);
        }
        let printing = CreatePopupMenu();
        item(
            printing,
            TOGGLE_CALIBRATION,
            "Use Saved Printer Calibration",
        );
        item(
            printing,
            CONFIGURE_CALIBRATION,
            "Configure Printer Calibration…",
        );
        item(
            printing,
            CALIBRATION_SHEET,
            "Export 100 mm Calibration Sheet…",
        );
        AppendMenuW(
            file,
            MF_POPUP,
            printing as usize,
            wide(&crate::localization::menu("Printer Calibration")).as_ptr(),
        );
        item(file, EXIT, "E&xit");
        AppendMenuW(
            bar,
            MF_POPUP,
            file as usize,
            wide(&crate::localization::menu("&File")).as_ptr(),
        );
        let editmenu = CreatePopupMenu();
        item(editmenu, UNDO, "&Undo\tCtrl+Z");
        item(editmenu, REDO, "&Redo\tCtrl+Y");
        if app.supports_find_replace() {
            item(editmenu, FIND, "&Find...\tCtrl+F");
            item(editmenu, REPLACE, "&Replace All...\tCtrl+H");
        }
        if app.uses_rich_edit() {
            item(editmenu, TOGGLE_PROOFING, "Check Spelling as You Type");
            item(editmenu,CHECK_SPELLING,"Check Spelling…");
        }
        for &(id, t) in app.actions() {
            if Some(id)!=print_command{item(editmenu, id, t)}
        }
        AppendMenuW(
            bar,
            MF_POPUP,
            editmenu as usize,
            wide(&crate::localization::menu("&Edit")).as_ptr(),
        );
        let view = CreatePopupMenu();
        for (id,title) in [(TOGGLE_SIDEBAR,"Show Sidebar"),(TOGGLE_TOOLBAR,"Show Toolbar"),(TOGGLE_STATUS,"Show Status Bar"),(TOGGLE_RULERS,"Show Rulers")]{item(view,id,title);}
        AppendMenuW(view,MF_SEPARATOR,0,null());
        for &(id, title) in app.view_actions() {
            item(view, id, title);
        }
        let languages = CreatePopupMenu();
        for (index, language) in crate::localization::LANGUAGES.iter().enumerate() {
            AppendMenuW(
                languages,
                MF_STRING
                    | if *language == crate::localization::current() {
                        MF_CHECKED
                    } else {
                        MF_UNCHECKED
                    },
                usize::from(LANGUAGE_FIRST) + index,
                wide(language.name).as_ptr(),
            );
        }
        AppendMenuW(
            view,
            MF_POPUP,
            languages as usize,
            wide(&crate::localization::menu("Language")).as_ptr(),
        );
        AppendMenuW(
            bar,
            MF_POPUP,
            view as usize,
            wide(&crate::localization::menu("&View")).as_ptr(),
        );
        let specs = app.command_specs();
        let mut groups: Vec<(&str, HMENU)> = Vec::new();
        for spec in &specs {
            if Some(spec.id)==print_command{continue;}
            let menu = if let Some((_, menu)) = groups.iter().find(|(name, _)| *name == spec.group)
            {
                *menu
            } else {
                let menu = CreatePopupMenu();
                groups.push((spec.group, menu));
                menu
            };
            let title = match spec.accelerator {
                Some(key) => format!("{}\t{}", spec.title, key),
                None => spec.title.to_string(),
            };
            item(menu, spec.id, &title);
        }
        if app.supports_production() || app.supports_technical_elements() {
            let menu =
                if let Some((_, menu)) = groups.iter().find(|(name, _)| *name == "Production") {
                    *menu
                } else {
                    let menu = CreatePopupMenu();
                    groups.push(("Production", menu));
                    menu
                };
            if app.supports_production() {
                item(menu, CREATE_BARCODE, "Create Barcode…");
            }
            item(menu, EXPORT_TECHNICAL, "Create Symbol or Code…");
            if app.supports_production() {
                item(menu, CREATE_LABEL_SHEET, "Create Label Sheet…");
            }
            if app.supports_technical_elements() {
                item(menu, INSERT_TECHNICAL, "Insert Symbol or Code…");
                item(menu, EDIT_TECHNICAL, "Edit Symbol or Code…");
            }
        }
        for (name, menu) in groups {
            AppendMenuW(
                bar,
                MF_POPUP,
                menu as usize,
                wide(&crate::localization::menu(name)).as_ptr(),
            );
        }
        if app.supports_marks() {
            let menu = CreatePopupMenu();
            item(menu, IMPORT_MARK, "Import to Library…");
            item(menu, INSERT_MARK, "Insert from Library…");
            AppendMenuW(
                bar,
                MF_POPUP,
                menu as usize,
                wide(&crate::localization::menu("Signatures && Stamps")).as_ptr(),
            );
        }
        let help = CreatePopupMenu();
        item(help, ABOUT, "&About");
        AppendMenuW(
            bar,
            MF_POPUP,
            help as usize,
            wide(&crate::localization::menu("&Help")).as_ptr(),
        );
        STATE.with(|s| {
            *s.borrow_mut() = Some(State {
                panels:crate::window_layout::load(app.name()).map(|layout|layout.panels).unwrap_or_default(),
                app,
                calibrate_print: false,
                deferred_commands: Vec::new(),
                close_confirmed: false,
                recovery:None,
                recovery_last:std::time::Instant::now(),
                recovery_unsaved:false,
                recovered_copy:None,
                recovery_error:None,
                path: None,
                session: Default::default(),
                async_open: None,
                hwnd: null_mut(),
                list: null_mut(),
                table: null_mut(),
                edit: null_mut(),
                canvas: null_mut(),
                status: null_mut(),
                menu: bar,
                recent_menu,
                recent_paths: Vec::new(),
                grid_cache: None,
                sparse_grid:None,
                toolbar: Vec::new(),
                toolbar_more: null_mut(),
                ruler_horizontal: null_mut(),
                ruler_vertical: null_mut(),
                ruler_format: None,
                fullscreen: None,
            })
        });
        let h = CreateWindowExW(
            0,
            class.as_ptr(),
            wide("off42").as_ptr(),
            WS_OVERLAPPEDWINDOW,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            1000,
            700,
            null_mut(),
            bar,
            instance,
            null(),
        );
        if h.is_null() {
            return;
        }
        let mut toolbar = Vec::new();
        for spec in &specs {
            if !STATE.with(|s|s.borrow().as_ref().unwrap().app.toolbar_command_visible(spec.id)) {continue;}
            let toggle=STATE.with(|s|s.borrow().as_ref().unwrap().app.toolbar_command_toggle(spec.id));
            let button = CreateWindowExW(
                0,
                wide("BUTTON").as_ptr(),
                wide(&crate::localization::text(spec.title)).as_ptr(),
                WS_CHILD | WS_VISIBLE | WS_TABSTOP | if toggle{(BS_CHECKBOX|BS_PUSHLIKE)as u32}else{0},
                0,
                0,
                0,
                0,
                h,
                spec.id as HMENU,
                instance,
                null(),
            );
            toolbar.push((spec.id, button));
        }
        let toolbar_more = CreateWindowExW(
            0,
            wide("BUTTON").as_ptr(),
            wide("More…").as_ptr(),
            WS_CHILD | WS_TABSTOP,
            0,
            0,
            0,
            0,
            h,
            TOOLBAR_MORE as HMENU,
            instance,
            null(),
        );
        let icc = INITCOMMONCONTROLSEX {
            dwSize: size_of::<INITCOMMONCONTROLSEX>() as u32,
            dwICC: ICC_LISTVIEW_CLASSES,
        };
        InitCommonControlsEx(&icc);
        let custom_grid = STATE.with(|s| s.borrow().as_ref().unwrap().app.uses_custom_grid());
        let table = if custom_grid {
            crate::grid::create(h, TABLE)
        } else {
            CreateWindowExW(
                WS_EX_CLIENTEDGE,
                wide("SysListView32").as_ptr(),
                null(),
                WS_CHILD | WS_TABSTOP | WS_VISIBLE | LVS_REPORT | LVS_SHOWSELALWAYS | LVS_SINGLESEL,
                0,
                0,
                0,
                0,
                h,
                TABLE as HMENU,
                instance,
                null(),
            )
        };
        let list = CreateWindowExW(
            WS_EX_CLIENTEDGE,
            wide("LISTBOX").as_ptr(),
            null(),
            WS_CHILD | WS_VISIBLE | WS_VSCROLL | WS_TABSTOP | LBS_NOTIFY as u32,
            0,
            0,
            0,
            0,
            h,
            LIST as HMENU,
            instance,
            null(),
        );
        let rich = STATE.with(|s| s.borrow().as_ref().unwrap().app.uses_rich_edit());
        if rich {
            LoadLibraryW(wide("Msftedit.dll").as_ptr());
        }
        let edit = CreateWindowExW(
            WS_EX_CLIENTEDGE,
            wide(if rich { "RICHEDIT50W" } else { "EDIT" }).as_ptr(),
            null(),
            WS_CHILD
                | WS_VISIBLE
                | WS_TABSTOP
                | WS_VSCROLL
                | ES_MULTILINE as u32
                | ES_AUTOVSCROLL as u32
                | ES_WANTRETURN as u32,
            0,
            0,
            0,
            0,
            h,
            EDIT as HMENU,
            instance,
            null(),
        );
        if !rich && SetWindowSubclass(edit,Some(plain_edit_proc),42,0)==0 {
            msg(h,"Could not initialize the document editor",MB_OK|MB_ICONERROR);
            DestroyWindow(h);
            return;
        }
        let canvas = CreateWindowExW(
            0,
            canvas_class.as_ptr(),
            null(),
            WS_CHILD,
            0,
            0,
            0,
            0,
            h,
            null_mut(),
            instance,
            null(),
        );
        SendMessageW(edit, EM_SETLIMITTEXT, 16777216, 0);
        if rich {
            crate::rich_edit::disable_undo(edit);
            crate::rich_edit::notify_changes(edit);
            crate::rich_edit::set_proofing(edit, true);
        }
        let status = CreateWindowExW(
            0,
            wide("STATIC").as_ptr(),
            null(),
            WS_CHILD | WS_VISIBLE,
            0,
            0,
            0,
            0,
            h,
            null_mut(),
            instance,
            null(),
        );
        let font = GetStockObject(DEFAULT_GUI_FONT) as usize;
        // Buttons otherwise draw in the bitmap System font, which lacks "…".
        let buttons = toolbar.iter().map(|&(_, b)| b).chain([toolbar_more]);
        for control in [list, table, edit, status].into_iter().chain(buttons) {
            SendMessageW(control, WM_SETFONT, font, 1);
        }
        let ruler_horizontal = if rich {
            crate::rich_ruler::create(h, false)
        } else {
            null_mut()
        };
        let ruler_vertical = if rich {
            crate::rich_ruler::create(h, true)
        } else {
            null_mut()
        };
        STATE.with(|s| {
            let mut s = s.borrow_mut();
            let s = s.as_mut().unwrap();
            s.hwnd = h;
            s.app.set_native_window(h as usize);
            s.list = list;
            s.table = table;
            s.edit = edit;
            s.canvas = canvas;
            s.status = status;
            s.toolbar = toolbar;
            s.toolbar_more = toolbar_more;
            s.ruler_horizontal = ruler_horizontal;
            s.ruler_vertical = ruler_vertical;
        });
        let mut acc = vec![
            ACCEL {
                fVirt: FVIRTKEY | FCONTROL,
                key: b'N' as u16,
                cmd: NEW,
            },
            ACCEL {
                fVirt: FVIRTKEY | FCONTROL | FSHIFT,
                key: b'N' as u16,
                cmd: NEW_WINDOW,
            },
            ACCEL {
                fVirt: FVIRTKEY | FCONTROL,
                key: b'F' as u16,
                cmd: FIND,
            },
            ACCEL {
                fVirt: FVIRTKEY | FCONTROL,
                key: b'H' as u16,
                cmd: REPLACE,
            },
            ACCEL {
                fVirt: FVIRTKEY | FCONTROL,
                key: b'O' as u16,
                cmd: OPEN,
            },
            ACCEL {
                fVirt: FVIRTKEY | FCONTROL,
                key: b'S' as u16,
                cmd: SAVE,
            },
            ACCEL {
                fVirt: FVIRTKEY | FCONTROL | FSHIFT,
                key: b'S' as u16,
                cmd: SAVE_AS,
            },
            ACCEL {
                fVirt: FVIRTKEY | FCONTROL,
                key: b'Z' as u16,
                cmd: UNDO,
            },
            ACCEL {
                fVirt: FVIRTKEY | FCONTROL,
                key: b'Y' as u16,
                cmd: REDO,
            },
        ];
        if let Some(cmd)=print_command{acc.push(ACCEL{fVirt:FVIRTKEY|FCONTROL,key:b'P' as u16,cmd});}
        for spec in &specs {
            if Some(spec.id)==print_command{continue;}
            if let Some(key) = spec.accelerator {
                let parts: Vec<_> = key.split('+').collect();
                if let Some(last) = parts.last() {
                    if last.len() == 1 {
                        let mut flags = FVIRTKEY;
                        if parts.iter().any(|p| p.eq_ignore_ascii_case("ctrl")) {
                            flags |= FCONTROL;
                        }
                        if parts.iter().any(|p| p.eq_ignore_ascii_case("shift")) {
                            flags |= FSHIFT;
                        }
                        if parts.iter().any(|p| p.eq_ignore_ascii_case("alt")) {
                            flags |= FALT;
                        }
                        acc.push(ACCEL {
                            fVirt: flags,
                            key: last.as_bytes()[0].to_ascii_uppercase() as u16,
                            cmd: spec.id,
                        });
                    }
                }
            }
        }
        let a = CreateAcceleratorTableW(acc.as_ptr(), acc.len() as i32);
        update_recent_menu();
        refresh();
        SetTimer(h, 42, 100, None);
        restore_window_layout(h);
        DragAcceptFiles(h, 1);
        match crate::localization::launch_arguments(std::env::args_os().skip(1)) {
            Ok(arguments) if !arguments.files.is_empty() => open_paths(h, arguments.files),
            Ok(_) => {
                let should_recover=STATE.with(|slot|{let slot=slot.borrow();let state=slot.as_ref().unwrap();state.app.supports_recovery()&&crate::recovery::directory().is_some_and(|root|!crate::recovery::copies(&root,state.app.name()).is_empty())});
                if should_recover{PostMessageW(h,WM_COMMAND,RECOVER_DOCUMENT as usize,0);}
            }
            Err(error) => {
                msg(h, &error, MB_OK | MB_ICONERROR);
            }
        }
        let mut m: MSG = zeroed();
        while GetMessageW(&mut m, null_mut(), 0, 0) > 0 {
            if m.message == WM_KEYDOWN
                && m.wParam == windows_sys::Win32::UI::Input::KeyboardAndMouse::VK_ESCAPE as usize
                && STATE.with(|s| {
                    s.borrow()
                        .as_ref()
                        .is_some_and(|s| s.app.inline_rich_rect().is_some() && GetFocus() == s.edit)
                })
            {
                let result = STATE.with(|s| {
                    s.borrow_mut()
                        .as_mut()
                        .unwrap()
                        .app
                        .finish_inline_rich(false)
                });
                if let Err(e) = result {
                    msg(h, &e, MB_OK | MB_ICONERROR);
                }
                refresh();
                continue;
            }
            if m.message == WM_KEYDOWN
                && m.wParam == windows_sys::Win32::UI::Input::KeyboardAndMouse::VK_ESCAPE as usize
                && STATE.with(|s| s.borrow().as_ref().is_some_and(|s| s.async_open.is_some()))
            {
                cancel_pending_open();
                refresh();
                continue;
            }
            let accelerator = TranslateAcceleratorW(h, a, &m);
            let dialog = if accelerator == 0 {
                IsDialogMessageW(h, &m)
            } else {
                0
            };
            if m.message == WM_COMMAND {
                trace_command(
                    if accelerator != 0 {
                        "accelerator_processed"
                    } else if dialog != 0 {
                        "dialog_processed"
                    } else {
                        "dispatch"
                    },
                    m.wParam as u16,
                    (m.wParam >> 16) as u16,
                );
            }
            if accelerator == 0 && dialog == 0 {
                TranslateMessage(&m);
                DispatchMessageW(&m);
            }
        }
        DestroyAcceleratorTable(a);
        // Flush normal-close cleanup before the process releases its recovery lock.
        STATE.with(|slot|{slot.borrow_mut().take();});
    }
}

unsafe extern "system" fn canvas_proc(h: HWND, m: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    // Current tools update pointer state only during a captured gesture. Idle
    // movement must not repaint or reset the caption: synthesized mouse moves
    // can otherwise keep the queue busy ahead of low-priority background timers.
    if m == WM_MOUSEMOVE && !DRAGGING.with(|dragging| dragging.get()) {
        return 0;
    }
    if m == WM_CAPTURECHANGED || m == WM_CANCELMODE {
        if DRAGGING.with(|d| d.replace(false)) {
            let _ = STATE.with(|s| {
                s.borrow_mut()
                    .as_mut()
                    .unwrap()
                    .app
                    .canvas_event(crate::CanvasEvent::Cancel)
            });
        }
        update_caption();
        InvalidateRect(h, null(), 0);
        return 0;
    }
    if m == WM_KEYDOWN {
        cancel_pending_open();
        let was_presenting = STATE.with(|s| {
            s.borrow()
                .as_ref()
                .unwrap()
                .app
                .presentation_size()
                .is_some()
        });
        let result = STATE.with(|s| {
            s.borrow_mut()
                .as_mut()
                .unwrap()
                .app
                .canvas_event(crate::CanvasEvent::Key {
                    code: w as u32,
                    modifiers: 0,
                })
        });
        if let Err(e) = result {
            msg(h, &e, MB_OK | MB_ICONERROR);
        }
        if w as u32==27&&DRAGGING.with(|dragging|dragging.get()){ReleaseCapture();}
        let pending=STATE.with(|s|s.borrow_mut().as_mut().unwrap().app.take_pending_open());
        if let Some(path)=pending {
            let owner=GetAncestor(h,GA_ROOT);
            if confirm(owner) {if let Err(e)=open_document_path(path){msg(owner,&e,MB_OK|MB_ICONERROR);}}
            refresh();return 0;
        }
        if was_presenting {
            refresh();
        } else {
            update_caption();
        }
        InvalidateRect(h, null(), 0);
        return 0;
    }
    if matches!(
        m,
        WM_LBUTTONDOWN | WM_LBUTTONDBLCLK | WM_MOUSEMOVE | WM_LBUTTONUP
    ) {
        if m == WM_LBUTTONDOWN {
            if STATE.with(|s| {
                s.borrow()
                    .as_ref()
                    .unwrap()
                    .app
                    .inline_rich_rect()
                    .is_some()
            }) {
                let result = STATE.with(|s| {
                    s.borrow_mut()
                        .as_mut()
                        .unwrap()
                        .app
                        .finish_inline_rich(true)
                });
                if let Err(e) = result {
                    msg(h, &e, MB_OK | MB_ICONERROR);
                }
                refresh();
            }
            cancel_pending_open();
            DRAGGING.with(|d| d.set(true));
            SetCapture(h);
            SetFocus(h);
        }
        let (image, zoom, scene, presentation) = STATE.with(|s| {
            let s = s.borrow();
            let a = &s.as_ref().unwrap().app;
            (
                a.image(),
                a.image_zoom(),
                a.canvas_scene(),
                a.presentation_size(),
            )
        });
        if let Some(scene) = scene {
            let scene = scene_for_canvas(h, scene, presentation);
            let p = crate::Point {
                x: (l as u32 & 0xffff) as u16 as i16 as f64,
                y: ((l as u32 >> 16) & 0xffff) as u16 as i16 as f64,
            };
            if let Some(point) = scene.viewport.view_to_document(p) {
                let event = match m {
                    WM_LBUTTONDBLCLK => crate::CanvasEvent::DoubleClick(point),
                    WM_LBUTTONDOWN
                        if STATE.with(|s| {
                            s.borrow().as_ref().unwrap().app.canvas_pointer_modifiers()
                        }) && (GetKeyState(VK_CONTROL as i32) < 0
                            || GetKeyState(VK_SHIFT as i32) < 0) =>
                    {
                        crate::CanvasEvent::PointerDownModified {
                            point,
                            modifiers: (if GetKeyState(VK_CONTROL as i32) < 0 {
                                1
                            } else {
                                0
                            }) | (if GetKeyState(VK_SHIFT as i32) < 0 {
                                2
                            } else {
                                0
                            }),
                        }
                    }
                    WM_LBUTTONDOWN => crate::CanvasEvent::PointerDown(point),
                    WM_LBUTTONUP => crate::CanvasEvent::PointerUp(point),
                    _ => crate::CanvasEvent::PointerMove(point),
                };
                let result =
                    STATE.with(|s| s.borrow_mut().as_mut().unwrap().app.canvas_event(event));
                if let Err(e) = result {
                    msg(h, &e, MB_OK | MB_ICONERROR);
                }
                if m == WM_LBUTTONDBLCLK
                    && STATE.with(|s| {
                        s.borrow()
                            .as_ref()
                            .unwrap()
                            .app
                            .inline_rich_rect()
                            .is_some()
                    })
                {
                    refresh();
                } else {
                    update_caption();
                }
                InvalidateRect(h, null(), 0);
            }
        }
        if let Some((width, height, _)) = image {
            if width > 0 && height > 0 {
                let mut rect: RECT = zeroed();
                GetClientRect(h, &mut rect);
                let scale = zoom.unwrap_or_else(|| {
                    (rect.right as f64 / width as f64).min(rect.bottom as f64 / height as f64)
                });
                if scale > 0.0 {
                    let point = crate::Point {
                        x: ((l as u32 & 0xffff) as u16 as i16 as f64
                            - (rect.right as f64 - width as f64 * scale) / 2.0)
                            / scale,
                        y: (((l as u32 >> 16) & 0xffff) as u16 as i16 as f64
                            - (rect.bottom as f64 - height as f64 * scale) / 2.0)
                            / scale,
                    };
                    let event = match m {
                        WM_LBUTTONDBLCLK => crate::CanvasEvent::DoubleClick(point),
                        WM_LBUTTONDOWN => crate::CanvasEvent::PointerDown(point),
                        WM_LBUTTONUP => crate::CanvasEvent::PointerUp(point),
                        _ => crate::CanvasEvent::PointerMove(point),
                    };
                    let result =
                        STATE.with(|s| s.borrow_mut().as_mut().unwrap().app.canvas_event(event));
                    if let Err(e) = result {
                        msg(h, &e, MB_OK | MB_ICONERROR);
                    }
                    update_caption();
                    InvalidateRect(h, null(), 0);
                }
            }
        }
        if m == WM_LBUTTONUP {
            DRAGGING.with(|d| d.set(false));
            ReleaseCapture();
            let command=STATE.with(|slot|slot.borrow_mut().as_mut().and_then(|state|state.app.take_pending_command()));
            if let Some(id)=command.filter(|id|crate::application_command_id(*id)||*id==CREATE_LABEL_SHEET){PostMessageW(GetAncestor(h,GA_ROOT),WM_COMMAND,id as usize,0);}
        }
        return 0;
    }
    if m != WM_PAINT {
        return DefWindowProcW(h, m, w, l);
    }
    let mut ps: PAINTSTRUCT = zeroed();
    let dc = BeginPaint(h, &mut ps);
    let mut rect: RECT = zeroed();
    GetClientRect(h, &mut rect);
    FillRect(dc, &rect, (COLOR_WINDOW + 1) as HBRUSH);
    let (image, zoom, scene, presentation) = STATE.with(|s| {
        let s = s.borrow();
        let s = s.as_ref().unwrap();
        (
            s.app.image(),
            s.app.image_zoom(),
            s.app.canvas_scene(),
            s.app.presentation_size(),
        )
    });
    if let Some(scene) = scene {
        draw_scene(dc, &scene_for_canvas(h, scene, presentation));
    }
    if let Some((width, height, pixels)) = image {
        if width > 0 && height > 0 {
            let scale = zoom.unwrap_or_else(|| {
                (rect.right as f64 / width as f64).min(rect.bottom as f64 / height as f64)
            });
            let dw = (width as f64 * scale).max(1.0) as i32;
            let dh = (height as f64 * scale).max(1.0) as i32;
            let mut bgra = Vec::with_capacity(pixels.len());
            for (i, p) in pixels.as_chunks::<4>().0.iter().enumerate() {
                let a = p[3] as u16;
                let bg = if (i % (width as usize) / 8 + i / (width as usize) / 8).is_multiple_of(2)
                {
                    255u16
                } else {
                    204u16
                };
                for c in [p[2], p[1], p[0]] {
                    bgra.push(((c as u16 * a + bg * (255 - a) + 127) / 255) as u8)
                }
                bgra.push(255)
            }
            let mut bmi: BITMAPINFO = zeroed();
            bmi.bmiHeader.biSize = size_of::<BITMAPINFOHEADER>() as u32;
            bmi.bmiHeader.biWidth = width as i32;
            bmi.bmiHeader.biHeight = -(height as i32);
            bmi.bmiHeader.biPlanes = 1;
            bmi.bmiHeader.biBitCount = 32;
            bmi.bmiHeader.biCompression = BI_RGB;
            SetStretchBltMode(dc, HALFTONE);
            StretchDIBits(
                dc,
                (rect.right - dw) / 2,
                (rect.bottom - dh) / 2,
                dw,
                dh,
                0,
                0,
                width as i32,
                height as i32,
                bgra.as_ptr().cast(),
                &bmi,
                DIB_RGB_COLORS,
                SRCCOPY,
            );
        }
    }
    EndPaint(h, &ps);
    0
}

unsafe fn update_table(table: HWND, rows: Option<&[Vec<String>]>) {
    let custom = STATE.with(|s| {
        s.borrow()
            .as_ref()
            .is_some_and(|s| s.app.uses_custom_grid())
    });
    if custom {
        STATE.with(|s| {
            if let Some(s) = s.borrow_mut().as_mut() {
                s.grid_cache = rows.map(|v| v.to_vec())
            }
        });
        if rows.is_some()||STATE.with(|s|s.borrow().as_ref().unwrap().sparse_grid.is_some()) {
            crate::grid::refresh(table)
        }
        return;
    }
    let Some(rows) = rows else {
        STATE.with(|s| s.borrow_mut().as_mut().unwrap().grid_cache = None);
        return;
    };
    let (transition, selected) = STATE.with(|s| {
        let mut s = s.borrow_mut();
        let s = s.as_mut().unwrap();
        let selected = s.app.selected_cell();
        let transition = crate::grid_transition(s.grid_cache.as_deref(), rows, selected);
        s.grid_cache = Some(rows.to_vec());
        (transition, selected)
    });
    let was_refreshing = REFRESHING.with(|x| x.replace(true));
    match transition {
        crate::GridTransition::Rebuild { selection } => {
            populate_table(table, rows);
            if let Some((row, col)) = selection {
                if Some((row, col)) != selected {
                    STATE.with(|s| s.borrow_mut().as_mut().unwrap().app.select_cell(row, col));
                }
            }
        }
        crate::GridTransition::Update { cells } => {
            for (row, col) in cells {
                let mut text = wide(&rows[row][col]);
                let item = LVITEMW {
                    mask: LVIF_TEXT,
                    iItem: row as i32,
                    iSubItem: col as i32,
                    pszText: text.as_mut_ptr(),
                    ..zeroed()
                };
                SendMessageW(table, LVM_SETITEMTEXTW, row, &item as *const _ as isize);
            }
        }
    }
    let selected = STATE.with(|s| s.borrow().as_ref().unwrap().app.selected_cell());
    let actual = SendMessageW(table, LVM_GETNEXTITEM, usize::MAX, LVNI_SELECTED as isize);
    if let Some((row, _)) = selected {
        if actual != row as isize {
            let clear = LVITEMW {
                stateMask: LVIS_SELECTED | LVIS_FOCUSED,
                state: 0,
                ..zeroed()
            };
            SendMessageW(
                table,
                LVM_SETITEMSTATE,
                usize::MAX,
                &clear as *const _ as isize,
            );
            let item = LVITEMW {
                stateMask: LVIS_SELECTED | LVIS_FOCUSED,
                state: LVIS_SELECTED | LVIS_FOCUSED,
                ..zeroed()
            };
            SendMessageW(table, LVM_SETITEMSTATE, row, &item as *const _ as isize);
            SendMessageW(table, LVM_ENSUREVISIBLE, row, 0);
        }
    }
    REFRESHING.with(|x| x.set(was_refreshing));
    flush_deferred_commands();
}

unsafe fn populate_table(table: HWND, rows: &[Vec<String>]) {
    SendMessageW(table, LVM_DELETEALLITEMS, 0, 0);
    while SendMessageW(table, LVM_DELETECOLUMN, 0, 0) != 0 {}
    let cols = rows.first().map_or(0, Vec::len);
    for col in 0..cols {
        let mut title = wide(&format!("Column {}", col + 1));
        let column = LVCOLUMNW {
            mask: LVCF_TEXT | LVCF_WIDTH | LVCF_SUBITEM,
            cx: 140,
            pszText: title.as_mut_ptr(),
            iSubItem: col as i32,
            ..zeroed()
        };
        SendMessageW(table, LVM_INSERTCOLUMNW, col, &column as *const _ as isize);
    }
    for (row, cells) in rows.iter().enumerate() {
        for (col, text) in cells.iter().enumerate() {
            let mut text = wide(text);
            let item = LVITEMW {
                mask: LVIF_TEXT,
                iItem: row as i32,
                iSubItem: col as i32,
                pszText: text.as_mut_ptr(),
                ..zeroed()
            };
            SendMessageW(
                table,
                if col == 0 {
                    LVM_INSERTITEMW
                } else {
                    LVM_SETITEMTEXTW
                },
                if col == 0 { 0 } else { row },
                &item as *const _ as isize,
            );
        }
    }
}

unsafe fn draw_scene(dc: HDC, scene: &crate::CanvasScene) {
    draw_scene_items(dc, scene.viewport, &scene.items);
}

unsafe fn draw_scene_items(dc: HDC, viewport: crate::Viewport, items: &[crate::SceneItem]) {
    use crate::{Point, SceneItem};
    let cv = |p: Point| viewport.document_to_view(p);
    let rgb = |c: [u8; 3]| c[0] as u32 | (c[1] as u32) << 8 | (c[2] as u32) << 16;
    for item in items {
        match item {
            SceneItem::OpacityLayer {
                bounds,
                opacity,
                items,
            } => {
                crate::scene_transform::opacity_layer(
                    dc,
                    viewport,
                    *bounds,
                    *opacity,
                    |memory, view| draw_scene_items(memory, view, items),
                );
            }
            SceneItem::ClippedOffset {
                clip,
                offset,
                items,
            } => {
                if [clip.x, clip.y, clip.width, clip.height, offset.x, offset.y]
                    .iter()
                    .all(|v| v.is_finite())
                    && clip.width > 0.0
                    && clip.height > 0.0
                {
                    let a = cv(Point {
                        x: clip.x,
                        y: clip.y,
                    });
                    let b = cv(Point {
                        x: clip.x + clip.width,
                        y: clip.y + clip.height,
                    });
                    let saved = SaveDC(dc);
                    if saved != 0 {
                        IntersectClipRect(dc, a.x as i32, a.y as i32, b.x as i32, b.y as i32);
                        let shifted = crate::Viewport {
                            zoom: viewport.zoom,
                            pan: Point {
                                x: viewport.pan.x + offset.x * viewport.zoom,
                                y: viewport.pan.y + offset.y * viewport.zoom,
                            },
                        };
                        draw_scene_items(dc, shifted, items);
                        RestoreDC(dc, saved);
                    }
                }
            }
            SceneItem::Transform {
                center,
                degrees,
                items,
            } => {
                crate::scene_transform::with_rotation(dc, viewport, *center, *degrees, || {
                    draw_scene_items(dc, viewport, items)
                });
            }
            SceneItem::Rect {
                x,
                y,
                width,
                height,
                fill,
                stroke,
            } => {
                let a = cv(Point { x: *x, y: *y });
                let b = cv(Point {
                    x: x + width,
                    y: y + height,
                });
                let brush = CreateSolidBrush(rgb(*fill));
                let pen = CreatePen(PS_SOLID, 1, rgb(*stroke));
                let oldb = SelectObject(dc, brush as _);
                let oldp = SelectObject(dc, pen as _);
                Rectangle(dc, a.x as i32, a.y as i32, b.x as i32, b.y as i32);
                SelectObject(dc, oldb);
                SelectObject(dc, oldp);
                DeleteObject(brush as _);
                DeleteObject(pen as _);
            }
            SceneItem::Ellipse {
                x,
                y,
                width,
                height,
                fill,
                stroke,
            } => {
                let a = cv(Point { x: *x, y: *y });
                let b = cv(Point {
                    x: x + width,
                    y: y + height,
                });
                let brush = fill
                    .map(|c| CreateSolidBrush(rgb(c)))
                    .unwrap_or_else(|| GetStockObject(NULL_BRUSH) as _);
                let pen = stroke
                    .map(|c| CreatePen(PS_SOLID, 1, rgb(c)))
                    .unwrap_or_else(|| GetStockObject(NULL_PEN) as _);
                let oldb = SelectObject(dc, brush as _);
                let oldp = SelectObject(dc, pen as _);
                Ellipse(dc, a.x as i32, a.y as i32, b.x as i32, b.y as i32);
                SelectObject(dc, oldb);
                SelectObject(dc, oldp);
                if fill.is_some() {
                    DeleteObject(brush as _);
                }
                if stroke.is_some() {
                    DeleteObject(pen as _);
                }
            }
            SceneItem::Text { x, y, text, color } => {
                if text.is_empty() {
                    continue;
                }
                let p = cv(Point { x: *x, y: *y });
                let t: Vec<u16> = text.encode_utf16().collect();
                SetBkMode(dc, TRANSPARENT as i32);
                SetTextColor(dc, rgb(*color));
                TextOutW(dc, p.x as i32, p.y as i32, t.as_ptr(), t.len() as i32);
            }
            SceneItem::TextBox {
                x,
                y,
                width,
                height,
                text,
                color,
                font,
                align,
                wrap,
            } => {
                let a = cv(Point { x: *x, y: *y });
                let b = cv(Point {
                    x: x + width,
                    y: y + height,
                });
                // DrawTextW reads the first character even for a zero count, and
                // an empty Vec's pointer is dangling.
                if text.is_empty()
                    || b.x <= a.x
                    || b.y <= a.y
                    || !font.points.is_finite()
                    || font.points <= 0.0
                {
                    continue;
                }
                let family = wide(&font.family);
                let size = (font.points * viewport.zoom).round().clamp(1.0, 32767.0) as i32;
                let handle = CreateFontW(
                    -size,
                    0,
                    0,
                    0,
                    if font.bold { 700 } else { 400 },
                    font.italic as u32,
                    font.underline as u32,
                    0,
                    DEFAULT_CHARSET as u32,
                    OUT_DEFAULT_PRECIS as u32,
                    CLIP_DEFAULT_PRECIS as u32,
                    DEFAULT_QUALITY as u32,
                    DEFAULT_PITCH as u32,
                    family.as_ptr(),
                );
                if handle.is_null() {
                    continue;
                }
                let old = SelectObject(dc, handle as _);
                let saved = SaveDC(dc);
                SetBkMode(dc, TRANSPARENT as i32);
                SetTextColor(dc, rgb(*color));
                let mut rect = RECT {
                    left: a.x as i32,
                    top: a.y as i32,
                    right: b.x as i32,
                    bottom: b.y as i32,
                };
                let t: Vec<u16> = text.encode_utf16().collect();
                let flags = DT_NOPREFIX
                    | match align {
                        crate::TextAlign::Left => DT_LEFT,
                        crate::TextAlign::Center => DT_CENTER,
                        crate::TextAlign::Right => DT_RIGHT,
                    }
                    | if *wrap { DT_WORDBREAK } else { DT_SINGLELINE };
                DrawTextW(
                    dc,
                    t.as_ptr(),
                    t.len().min(i32::MAX as usize) as i32,
                    &mut rect,
                    flags,
                );
                if saved != 0 {
                    RestoreDC(dc, saved);
                }
                SelectObject(dc, old);
                DeleteObject(handle as _);
            }
            SceneItem::RichText {
                x,
                y,
                width,
                height,
                paragraphs,
            } => {
                let a = cv(Point { x: *x, y: *y });
                let b = cv(Point {
                    x: x + width,
                    y: y + height,
                });
                draw_rich_scene(dc, a, b, viewport.zoom, paragraphs);
            }
            SceneItem::VectorPath {
                segments,
                fill,
                stroke,
            } => {
                if segments.is_empty() || (fill.is_none() && stroke.is_none()) {
                    continue;
                }
                let saved = SaveDC(dc);
                if saved == 0 {
                    continue;
                }
                let brush = fill.map(|c| CreateSolidBrush(rgb(c)));
                let pen = stroke.map(|c| CreatePen(PS_SOLID, 1, rgb(c)));
                SelectObject(dc, brush.unwrap_or_else(|| GetStockObject(NULL_BRUSH)) as _);
                SelectObject(dc, pen.unwrap_or_else(|| GetStockObject(NULL_PEN)) as _);
                SetPolyFillMode(dc, ALTERNATE);
                if BeginPath(dc) != 0 {
                    for segment in segments {
                        match segment {
                            crate::PathSegment::Move(p) => {
                                let p = cv(*p);
                                MoveToEx(dc, p.x as i32, p.y as i32, null_mut());
                            }
                            crate::PathSegment::Line(p) => {
                                let p = cv(*p);
                                LineTo(dc, p.x as i32, p.y as i32);
                            }
                            crate::PathSegment::Cubic(a, b, c) => {
                                let points = [*a, *b, *c].map(|p| {
                                    let p = cv(p);
                                    POINT {
                                        x: p.x as i32,
                                        y: p.y as i32,
                                    }
                                });
                                PolyBezierTo(dc, points.as_ptr(), 3);
                            }
                            crate::PathSegment::Close => {
                                CloseFigure(dc);
                            }
                        }
                    }
                    if EndPath(dc) != 0 {
                        if fill.is_some() && stroke.is_some() {
                            StrokeAndFillPath(dc);
                        } else if fill.is_some() {
                            FillPath(dc);
                        } else {
                            StrokePath(dc);
                        }
                    } else {
                        AbortPath(dc);
                    }
                }
                RestoreDC(dc, saved);
                if let Some(brush) = brush {
                    DeleteObject(brush as _);
                }
                if let Some(pen) = pen {
                    DeleteObject(pen as _);
                }
            }
            SceneItem::Path {
                points,
                color,
                closed,
            } => {
                if let Some(first) = points.first() {
                    let pen = CreatePen(PS_SOLID, 1, rgb(*color));
                    let old = SelectObject(dc, pen as _);
                    let a = cv(*first);
                    MoveToEx(dc, a.x as i32, a.y as i32, null_mut());
                    for point in &points[1..] {
                        let p = cv(*point);
                        LineTo(dc, p.x as i32, p.y as i32);
                    }
                    if *closed {
                        LineTo(dc, a.x as i32, a.y as i32);
                    }
                    SelectObject(dc, old);
                    DeleteObject(pen as _);
                }
            }
            SceneItem::Image {
                x,
                y,
                width,
                height,
                rgba,
            } => {
                let p = cv(Point { x: *x, y: *y });
                draw_rgba(
                    dc,
                    p.x,
                    p.y,
                    *width as f64 * viewport.zoom,
                    *height as f64 * viewport.zoom,
                    *width,
                    *height,
                    rgba,
                );
            }
            SceneItem::ImageScaled {
                x,
                y,
                width,
                height,
                pixel_width,
                pixel_height,
                rgba,
            } => {
                let p = cv(Point { x: *x, y: *y });
                draw_rgba(
                    dc,
                    p.x,
                    p.y,
                    *width * viewport.zoom,
                    *height * viewport.zoom,
                    *pixel_width,
                    *pixel_height,
                    rgba,
                );
            }
        }
    }
}

unsafe fn draw_rich_scene(
    dc: HDC,
    a: crate::Point,
    b: crate::Point,
    zoom: f64,
    paragraphs: &[crate::SceneParagraph],
) {
    if b.x <= a.x || b.y <= a.y || !zoom.is_finite() || zoom <= 0.0 {
        return;
    }
    struct Segment {
        text: Vec<u16>,
        font: HFONT,
        color: [u8; 3],
        width: i32,
        height: i32,
    }
    let saved = SaveDC(dc);
    if saved == 0 {
        return;
    }
    IntersectClipRect(dc, a.x as i32, a.y as i32, b.x as i32, b.y as i32);
    SetBkMode(dc, TRANSPARENT as i32);
    let max_width = (b.x - a.x).floor().max(1.0) as i32;
    let mut y = a.y as i32;
    let mut fonts = Vec::new();
    for paragraph in paragraphs {
        let mut lines: Vec<Vec<Segment>> = vec![Vec::new()];
        let mut width = 0i32;
        let mut runs = paragraph.runs.clone();
        if paragraph.bullet {
            let style = runs.first().cloned().unwrap_or(crate::SceneRun {
                text: String::new(),
                font: crate::SceneFont {
                    family: "Arial".into(),
                    points: 12.0,
                    bold: false,
                    italic: false,
                    underline: false,
                },
                color: [0, 0, 0],
            });
            runs.insert(
                0,
                crate::SceneRun {
                    text: "• ".into(),
                    ..style
                },
            );
        }
        for run in &runs {
            if !run.font.points.is_finite() || run.font.points <= 0.0 {
                continue;
            }
            let family = wide(&run.font.family);
            let size = (run.font.points * zoom).round().clamp(1.0, 32767.0) as i32;
            let font = CreateFontW(
                -size,
                0,
                0,
                0,
                if run.font.bold { 700 } else { 400 },
                run.font.italic as u32,
                run.font.underline as u32,
                0,
                DEFAULT_CHARSET as u32,
                OUT_DEFAULT_PRECIS as u32,
                CLIP_DEFAULT_PRECIS as u32,
                DEFAULT_QUALITY as u32,
                DEFAULT_PITCH as u32,
                family.as_ptr(),
            );
            if font.is_null() {
                continue;
            }
            fonts.push(font);
            let old = SelectObject(dc, font as _);
            let mut metrics: TEXTMETRICW = zeroed();
            GetTextMetricsW(dc, &mut metrics);
            let height = metrics.tmHeight.max(1);
            for token in run.text.split_inclusive(char::is_whitespace) {
                let text = token.encode_utf16().collect::<Vec<_>>();
                if text.is_empty() {
                    continue;
                }
                let mut size: SIZE = zeroed();
                if GetTextExtentPoint32W(
                    dc,
                    text.as_ptr(),
                    text.len().min(i32::MAX as usize) as i32,
                    &mut size,
                ) == 0
                {
                    continue;
                }
                let token_width = size.cx.max(0);
                if width > 0 && width.saturating_add(token_width) > max_width {
                    lines.push(Vec::new());
                    width = 0;
                }
                lines.last_mut().unwrap().push(Segment {
                    text,
                    font,
                    color: run.color,
                    width: token_width,
                    height,
                });
                width = width.saturating_add(token_width);
            }
            SelectObject(dc, old);
        }
        for line in lines {
            let line_width = line.iter().map(|s| s.width).sum::<i32>();
            let line_height = line
                .iter()
                .map(|s| s.height)
                .max()
                .unwrap_or((12.0 * zoom).round().max(1.0) as i32);
            let mut x = a.x as i32
                + match paragraph.align {
                    crate::TextAlign::Left => 0,
                    crate::TextAlign::Center => (max_width - line_width).max(0) / 2,
                    crate::TextAlign::Right => (max_width - line_width).max(0),
                };
            for segment in line {
                let old = SelectObject(dc, segment.font as _);
                SetTextColor(
                    dc,
                    segment.color[0] as u32
                        | ((segment.color[1] as u32) << 8)
                        | ((segment.color[2] as u32) << 16),
                );
                TextOutW(
                    dc,
                    x,
                    y,
                    segment.text.as_ptr(),
                    segment.text.len().min(i32::MAX as usize) as i32,
                );
                SelectObject(dc, old);
                x = x.saturating_add(segment.width);
            }
            y = y.saturating_add(line_height);
            if y >= b.y as i32 {
                break;
            }
        }
        if y >= b.y as i32 {
            break;
        }
    }
    RestoreDC(dc, saved);
    for font in fonts {
        DeleteObject(font as _);
    }
}

/// Draw an RGBA raster over the existing destination using premultiplied alpha.
pub(super) unsafe fn draw_rgba(
    dc: HDC,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    pixel_width: u32,
    pixel_height: u32,
    rgba: &[u8],
) {
    let expected = (pixel_width as usize)
        .checked_mul(pixel_height as usize)
        .and_then(|n| n.checked_mul(4));
    if expected != Some(rgba.len())
        || pixel_width == 0
        || pixel_height == 0
        || pixel_width > i32::MAX as u32
        || pixel_height > i32::MAX as u32
        || !x.is_finite()
        || !y.is_finite()
        || !width.is_finite()
        || !height.is_finite()
        || width <= 0.0
        || height <= 0.0
        || width > i32::MAX as f64
        || height > i32::MAX as f64
    {
        return;
    }
    let mut pixels = Vec::with_capacity(rgba.len());
    for p in rgba.chunks_exact(4) {
        let a = p[3] as u16;
        for v in [p[2], p[1], p[0]] {
            pixels.push(((v as u16 * a + 127) / 255) as u8);
        }
        pixels.push(p[3]);
    }
    let mut bmi: BITMAPINFO = zeroed();
    bmi.bmiHeader.biSize = size_of::<BITMAPINFOHEADER>() as u32;
    bmi.bmiHeader.biWidth = pixel_width as i32;
    bmi.bmiHeader.biHeight = -(pixel_height as i32);
    bmi.bmiHeader.biPlanes = 1;
    bmi.bmiHeader.biBitCount = 32;
    bmi.bmiHeader.biCompression = BI_RGB;
    let memory = CreateCompatibleDC(dc);
    if memory.is_null() {
        return;
    }
    let mut bits = null_mut();
    let bitmap = CreateDIBSection(dc, &bmi, DIB_RGB_COLORS, &mut bits, null_mut(), 0);
    if bitmap.is_null() || bits.is_null() {
        if !bitmap.is_null() {
            DeleteObject(bitmap as _);
        }
        DeleteDC(memory);
        return;
    }
    std::ptr::copy_nonoverlapping(pixels.as_ptr(), bits.cast::<u8>(), pixels.len());
    let previous = SelectObject(memory, bitmap as _);
    AlphaBlend(
        dc,
        x as i32,
        y as i32,
        width.round().max(1.0) as i32,
        height.round().max(1.0) as i32,
        memory,
        0,
        0,
        pixel_width as i32,
        pixel_height as i32,
        BLENDFUNCTION {
            BlendOp: AC_SRC_OVER as u8,
            BlendFlags: 0,
            SourceConstantAlpha: 255,
            AlphaFormat: AC_SRC_ALPHA as u8,
        },
    );
    SelectObject(memory, previous);
    DeleteObject(bitmap as _);
    DeleteDC(memory);
}

unsafe fn selected_printer_name(handle: HGLOBAL) -> Result<String, String> {
    use windows_sys::Win32::System::Memory::{GlobalLock, GlobalSize, GlobalUnlock};
    if handle.is_null() {
        return Err("Printer identity is unavailable.".into());
    }
    let size = GlobalSize(handle);
    let data = GlobalLock(handle);
    if data.is_null() {
        return Err("Could not read printer identity.".into());
    }
    let result = (|| {
        if size < size_of::<DEVNAMES>() {
            return Err("Invalid printer identity.".into());
        }
        let names = &*(data as *const DEVNAMES);
        let start = usize::from(names.wDeviceOffset);
        let units = std::slice::from_raw_parts(data as *const u16, size / 2);
        let rest = units.get(start..).ok_or("Invalid printer name offset.")?;
        let end = rest
            .iter()
            .position(|&value| value == 0)
            .ok_or("Invalid printer name.")?;
        String::from_utf16(&rest[..end]).map_err(|error| error.to_string())
    })();
    GlobalUnlock(handle);
    result
}

unsafe fn printer_profile_key(name: &str, dc: HDC) -> Result<String, String> {
    let (dx, dy) = (
        GetDeviceCaps(dc, LOGPIXELSX as i32),
        GetDeviceCaps(dc, LOGPIXELSY as i32),
    );
    let (w, h) = (
        GetDeviceCaps(dc, PHYSICALWIDTH as i32),
        GetDeviceCaps(dc, PHYSICALHEIGHT as i32),
    );
    if dx <= 0 || dy <= 0 || w <= 0 || h <= 0 {
        return Err("Printer reported invalid paper dimensions.".into());
    }
    let width = (f64::from(w) * 25400.0 / f64::from(dx)).round() as i64;
    let height = (f64::from(h) * 25400.0 / f64::from(dy)).round() as i64;
    Ok(format!("{name}/{width}x{height}um"))
}

unsafe fn calibration_command(hwnd: HWND, id: u16) -> Result<(), String> {
    if id == TOGGLE_CALIBRATION {
        STATE.with(|state| {
            let mut state = state.borrow_mut();
            let state = state.as_mut().unwrap();
            state.calibrate_print = !state.calibrate_print;
        });
        return Ok(());
    }
    if id == CALIBRATION_SHEET {
        let bytes = label42::calibration_pdf()?;
        if let Some(path) = dialog(hwnd, true, "pdf", "PDF calibration sheet", "*.pdf") {
            if !path
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("pdf"))
            {
                return Err("Choose a file name ending in .pdf.".into());
            }
            suite42_core::write_new(&path, &bytes).map_err(|error| error.to_string())?;
        }
        return Ok(());
    }
    let mut chooser: PRINTDLGW = zeroed();
    chooser.lStructSize = size_of::<PRINTDLGW>() as u32;
    chooser.hwndOwner = hwnd;
    chooser.Flags = PD_RETURNDC | PD_NOSELECTION | PD_NOPAGENUMS;
    let accepted = PrintDlgW(&mut chooser) != 0;
    let name = selected_printer_name(chooser.hDevNames);
    let key = name
        .as_ref()
        .map_err(Clone::clone)
        .and_then(|name| printer_profile_key(name, chooser.hDC));
    if !chooser.hDevMode.is_null() {
        GlobalFree(chooser.hDevMode);
    }
    if !chooser.hDevNames.is_null() {
        GlobalFree(chooser.hDevNames);
    }
    if !chooser.hDC.is_null() {
        DeleteDC(chooser.hDC);
    }
    if !accepted {
        return Ok(());
    }
    let (name, key) = (name?, key?);
    let root = crate::print_calibration::directory()?;
    let value = crate::print_calibration::load(&root, &key)?.unwrap_or_default();
    let request = crate::InputRequest {
        title: "Printer Calibration",
        fields: vec![
            (
                "Measured horizontal 100 mm line (mm)",
                format!("{:.5}", 100.0 / value.scale_x),
            ),
            (
                "Measured vertical 100 mm line (mm)",
                format!("{:.5}", 100.0 / value.scale_y),
            ),
            (
                "Horizontal offset (mm, right is positive)",
                value.offset_x.to_string(),
            ),
            (
                "Vertical offset (mm, down is positive)",
                value.offset_y.to_string(),
            ),
        ],
    };
    let Some(fields) = crate::prompt::show(hwnd, &request) else {
        return Ok(());
    };
    let values = fields
        .iter()
        .map(|value| {
            value
                .trim()
                .parse::<f64>()
                .map_err(|_| "Enter measurements in millimetres.".to_string())
        })
        .collect::<Result<Vec<_>, _>>()?;
    let correction = crate::print_calibration::PrintCorrection::measured(
        values[0], values[1], values[2], values[3],
    )?;
    crate::print_calibration::save(&root, &key, correction)?;
    msg(hwnd, &format!("Calibration saved for {name}. Turn on Use Saved Printer Calibration to apply it when printing."), MB_OK | MB_ICONINFORMATION);
    Ok(())
}

unsafe fn apply_rich_print_correction(
    dc: HDC,
    correction: crate::print_calibration::PrintCorrection,
    physical: (i32, i32),
    dpi: (i32, i32),
) -> Result<i32, String> {
    let saved = SaveDC(dc);
    if saved == 0 {
        return Err("Could not save print device state.".into());
    }
    if correction == crate::print_calibration::PrintCorrection::default() {
        return Ok(saved);
    }
    let (x, y) = correction.device_origin((0, 0), physical, dpi);
    let transform = XFORM {
        eM11: correction.scale_x as f32,
        eM12: 0.0,
        eM21: 0.0,
        eM22: correction.scale_y as f32,
        eDx: x as f32,
        eDy: y as f32,
    };
    if SetGraphicsMode(dc, GM_ADVANCED) == 0 || SetWorldTransform(dc, &transform) == 0 {
        RestoreDC(dc, saved);
        return Err("The printer does not support calibrated text output.".into());
    }
    Ok(saved)
}

/// Print explicit page coordinates against the selected printer's physical geometry.
pub(super) unsafe fn print_job(hwnd: HWND, job: &crate::PrintJob) -> Result<(), String> {
    use windows_sys::Win32::Storage::Xps::{
        AbortDoc, EndDoc, EndPage, StartDocW, StartPage, DOCINFOW,
    };
    let geometries: Vec<crate::PrintGeometry> = match job {
        crate::PrintJob::ActualSizePdf(bytes) | crate::PrintJob::FitPdf(bytes) => {
            let source = crate::pdfium::Document::open(
                bytes.clone(),
                &std::sync::atomic::AtomicBool::new(false),
            )?;
            (0..source.page_count())
                .map(|index| {
                    source.dimensions(index).map(|(w, h)| crate::PrintGeometry {
                        width_twips: (w * 20.).round() as i32,
                        height_twips: (h * 20.).round() as i32,
                        margin_left_twips: 0,
                        margin_right_twips: 0,
                        margin_top_twips: 0,
                        margin_bottom_twips: 0,
                    })
                })
                .collect::<Result<Vec<_>, _>>()?
        }
        crate::PrintJob::Scenes(pages) | crate::PrintJob::ActualSizeScenes(pages) => {
            pages.iter().map(|p| p.geometry).collect()
        }
        crate::PrintJob::RichText(g) => vec![g.geometry],
    };
    if geometries.is_empty() || geometries.iter().any(|g| !g.valid()) {
        return Err("Invalid or empty print page geometry".into());
    }
    let mut dialog: PRINTDLGW = zeroed();
    dialog.lStructSize = size_of::<PRINTDLGW>() as u32;
    dialog.hwndOwner = hwnd;
    let pdf_job = matches!(
        job,
        crate::PrintJob::ActualSizePdf(_) | crate::PrintJob::FitPdf(_)
    );
    if pdf_job {
        dialog.Flags = PD_RETURNDEFAULT;
        let _ = PrintDlgW(&mut dialog);
        // Label PDFs are A4. Change this job's default paper without changing system printer preferences.
        if matches!(job, crate::PrintJob::ActualSizePdf(_))
            && geometries.iter().all(|g| {
                (g.width_twips - 11906).abs() <= 20 && (g.height_twips - 16838).abs() <= 20
            })
            && !dialog.hDevMode.is_null()
            && GlobalSize(dialog.hDevMode) >= size_of::<DEVMODEW>()
        {
            let mode = GlobalLock(dialog.hDevMode).cast::<DEVMODEW>();
            if !mode.is_null() {
                if (*mode).dmSize as usize >= size_of::<DEVMODEW>() {
                    (*mode).dmFields = ((*mode).dmFields & !(DM_PAPERLENGTH | DM_PAPERWIDTH))
                        | DM_PAPERSIZE
                        | DM_ORIENTATION;
                    (*mode).Anonymous1.Anonymous1.dmPaperSize = DMPAPER_A4 as i16;
                    (*mode).Anonymous1.Anonymous1.dmOrientation = DMORIENT_PORTRAIT as i16;
                }
                GlobalUnlock(dialog.hDevMode);
            }
        }
        dialog.nMinPage = 1;
        dialog.nMaxPage = geometries.len() as u16;
        dialog.nFromPage = 1;
        dialog.nToPage = dialog.nMaxPage;
    }
    dialog.Flags = PD_RETURNDC
        | PD_NOSELECTION
        | PD_USEDEVMODECOPIESANDCOLLATE
        | if pdf_job { 0 } else { PD_NOPAGENUMS };
    dialog.nCopies = 1;
    let accepted = PrintDlgW(&mut dialog) != 0;
    let printer_name = selected_printer_name(dialog.hDevNames);
    if !dialog.hDevMode.is_null() {
        GlobalFree(dialog.hDevMode);
    }
    if !dialog.hDevNames.is_null() {
        GlobalFree(dialog.hDevNames);
    }
    if !accepted {
        let error = CommDlgExtendedError();
        if error != 0 {
            return Err(format!("Print dialog failed (Windows code {error})"));
        }
        return Ok(());
    }
    let dc = dialog.hDC;
    if dc.is_null() {
        return Err("Printer did not return a device context".into());
    }
    let caps = |v| GetDeviceCaps(dc, v as i32);
    let (dpi_x, dpi_y) = (caps(LOGPIXELSX), caps(LOGPIXELSY));
    let (width, height) = (caps(PHYSICALWIDTH), caps(PHYSICALHEIGHT));
    let (offset_x, offset_y) = (caps(PHYSICALOFFSETX), caps(PHYSICALOFFSETY));
    let (print_w, print_h) = (caps(HORZRES), caps(VERTRES));
    if dpi_x <= 0
        || dpi_y <= 0
        || width <= 0
        || height <= 0
        || offset_x < 0
        || offset_y < 0
        || print_w <= 0
        || print_h <= 0
    {
        DeleteDC(dc);
        return Err("Printer reported invalid dimensions".into());
    }
    let correction = if STATE.with(|state| state.borrow().as_ref().unwrap().calibrate_print) {
        let result = printer_name.and_then(|name| {
            let key = printer_profile_key(&name, dc)?;
            crate::print_calibration::load(&crate::print_calibration::directory()?, &key)?
                .ok_or_else(|| "No calibration is saved for this printer and paper. Configure it before enabling calibration.".into())
        });
        match result {
            Ok(correction) => correction,
            Err(error) => {
                DeleteDC(dc);
                return Err(error);
            }
        }
    } else {
        crate::print_calibration::PrintCorrection::default()
    };
    let twips = |pixels: i32, dpi: i32| (i64::from(pixels) * 1440 / i64::from(dpi)) as i32;
    let paper = crate::PrintGeometry {
        width_twips: twips(width, dpi_x),
        height_twips: twips(height, dpi_y),
        margin_left_twips: twips(offset_x, dpi_x),
        margin_top_twips: twips(offset_y, dpi_y),
        margin_right_twips: twips((width - offset_x - print_w).max(0), dpi_x),
        margin_bottom_twips: twips((height - offset_y - print_h).max(0), dpi_y),
    };
    // A page that does not match the selected paper is reflowed onto it (rich text) or
    // shrunk to fit and centred (scenes, below) rather than refused: the Windows print
    // dialog offers no margins, and the default paper often differs from the document's.
    let reflowed;
    let job = match job {
        crate::PrintJob::RichText(rich) if !rich.geometry.fits_paper(paper) => {
            let mut rich = rich.clone();
            rich.geometry = rich.geometry.reflowed_onto(paper);
            if !rich.geometry.valid() {
                DeleteDC(dc);
                return Err("The selected paper is too small for this document's margins".into());
            }
            reflowed = crate::PrintJob::RichText(rich);
            &reflowed
        }
        _ => job,
    };
    // Keep embedded fonts and PDF resources alive until EndDoc has finished spooling.
    let pdf_document =
        if let crate::PrintJob::ActualSizePdf(bytes) | crate::PrintJob::FitPdf(bytes) = job {
            match crate::pdfium::Document::open(
                bytes.clone(),
                &std::sync::atomic::AtomicBool::new(false),
            ) {
                Ok(document) => Some(document),
                Err(error) => {
                    DeleteDC(dc);
                    return Err(error);
                }
            }
        } else {
            None
        };
    let title = wide("off42 document");
    let file_output = wide("FILE:");
    let info = DOCINFOW {
        cbSize: size_of::<DOCINFOW>() as i32,
        lpszDocName: title.as_ptr(),
        lpszOutput: if dialog.Flags & PD_PRINTTOFILE != 0 {
            file_output.as_ptr()
        } else {
            null()
        },
        ..zeroed()
    };
    if StartDocW(dc, &info) <= 0 {
        DeleteDC(dc);
        return Err("Could not start print job".into());
    }
    let result: Result<(), String> = (|| match job {
        crate::PrintJob::ActualSizePdf(_) | crate::PrintJob::FitPdf(_) => {
            let source = pdf_document
                .as_ref()
                .ok_or("PDF print document is unavailable")?;
            let (first, last) = if dialog.Flags & PD_PAGENUMS != 0 {
                (dialog.nFromPage as usize, dialog.nToPage as usize)
            } else {
                (1, source.page_count())
            };
            if first == 0 || last < first || last > source.page_count() {
                return Err("Invalid PDF print page range".into());
            }
            for index in first - 1..last {
                let (w, h) = source.dimensions(index)?;
                let destination = crate::pdf_print_layout::destination(
                    (w, h),
                    (width, height),
                    (dpi_x, dpi_y),
                    (offset_x, offset_y),
                    matches!(job, crate::PrintJob::FitPdf(_)),
                    correction,
                )?;
                if StartPage(dc) <= 0 {
                    return Err("Could not start PDF print page".into());
                }
                let saved = SaveDC(dc);
                if saved == 0 {
                    return Err("Could not save PDF print device state".into());
                }
                let painted = source.print_page(index, dc, destination);
                RestoreDC(dc, saved);
                painted?;
                if EndPage(dc) <= 0 {
                    return Err("Could not finish PDF print page".into());
                }
            }
            Ok(())
        }
        crate::PrintJob::Scenes(pages) | crate::PrintJob::ActualSizeScenes(pages) => {
            for page in pages {
                if StartPage(dc) <= 0 {
                    return Err("Could not start print page".into());
                }
                let saved = SaveDC(dc);
                if saved == 0 {
                    return Err("Could not save print device state".into());
                }
                let ox = (i64::from(page.geometry.margin_left_twips) * i64::from(dpi_x) / 1440)
                    as i32
                    - offset_x;
                let oy = (i64::from(page.geometry.margin_top_twips) * i64::from(dpi_y) / 1440)
                    as i32
                    - offset_y;
                let clip_w = (page.geometry.width_twips
                    - page.geometry.margin_left_twips
                    - page.geometry.margin_right_twips)
                    / 20;
                let clip_h = (page.geometry.height_twips
                    - page.geometry.margin_top_twips
                    - page.geometry.margin_bottom_twips)
                    / 20;
                // Points to device pixels at `scale`; 1000x extents keep the scale precise.
                let content = (
                    f64::from(clip_w) * f64::from(dpi_x) / 72.0,
                    f64::from(clip_h) * f64::from(dpi_y) / 72.0,
                );
                let (scale, ox, oy) = if matches!(job, crate::PrintJob::ActualSizeScenes(_)) {
                    (1.0, ox, oy)
                } else {
                    crate::fit_print_content(content, (ox, oy), (print_w, print_h))
                };
                let (ox, oy) =
                    correction.device_origin((ox, oy), (offset_x, offset_y), (dpi_x, dpi_y));
                SetMapMode(dc, MM_ANISOTROPIC);
                SetWindowExtEx(dc, 72_000, 72_000, null_mut());
                SetViewportExtEx(
                    dc,
                    (f64::from(dpi_x) * scale * correction.scale_x * 1000.0).round() as i32,
                    (f64::from(dpi_y) * scale * correction.scale_y * 1000.0).round() as i32,
                    null_mut(),
                );
                SetViewportOrgEx(dc, ox, oy, null_mut());
                IntersectClipRect(dc, 0, 0, clip_w, clip_h);
                let mut scene = page.scene.clone();
                scene.viewport = crate::Viewport::default();
                if GetDeviceCaps(dc, SHADEBLENDCAPS as i32) & SB_PIXEL_ALPHA as i32 == 0 {
                    crate::scene_transform::print_flattened(
                        dc,
                        clip_w,
                        clip_h,
                        ((f64::from(dpi_x.min(dpi_y)) * scale).round() as i32).max(1),
                        |memory, zoom| {
                            scene.viewport.zoom = zoom;
                            draw_scene(memory, &scene);
                        },
                    )?;
                } else {
                    draw_scene(dc, &scene);
                }
                RestoreDC(dc, saved);
                if EndPage(dc) <= 0 {
                    return Err("Could not finish print page".into());
                }
            }
            Ok(())
        }
        crate::PrintJob::RichText(job) => {
            let original = STATE.with(|s| s.borrow().as_ref().unwrap().edit);
            let (edit, temporary) = crate::rich_notes::temporary_edit(original, job)?;
            let ox = (i64::from(offset_x) * 1440 / i64::from(dpi_x)) as i32;
            let oy = (i64::from(offset_y) * 1440 / i64::from(dpi_y)) as i32;
            let result = (|| {
                if job.notes.is_empty() {
                    let pages = crate::rich_print::paginate(edit, dc, job.geometry, (ox, oy))?;
                    for (page, (start, end)) in pages.iter().copied().enumerate() {
                        if StartPage(dc) <= 0 {
                            return Err("Could not start print page".into());
                        }
                        let saved = apply_rich_print_correction(
                            dc,
                            correction,
                            (offset_x, offset_y),
                            (dpi_x, dpi_y),
                        )?;
                        if crate::rich_print::body(edit, dc, job.geometry, (ox, oy), start) != end {
                            return Err("Print pagination changed while rendering".into());
                        }
                        crate::rich_print::decorations(
                            dc,
                            job,
                            (offset_x, offset_y),
                            page,
                            pages.len(),
                        )?;
                        RestoreDC(dc, saved);
                        if EndPage(dc) <= 0 {
                            return Err("Could not finish print page".into());
                        }
                    }
                } else {
                    let layout = crate::rich_notes::paginate(edit, dc, job, (ox, oy))?;
                    for page in 0..layout.pages.len() {
                        if StartPage(dc) <= 0 {
                            return Err("Could not start print page".into());
                        }
                        let saved = apply_rich_print_correction(
                            dc,
                            correction,
                            (offset_x, offset_y),
                            (dpi_x, dpi_y),
                        )?;
                        crate::rich_notes::draw_page(
                            edit,
                            dc,
                            job,
                            &layout,
                            page,
                            (ox, oy),
                            (offset_x, offset_y),
                        )?;
                        RestoreDC(dc, saved);
                        if EndPage(dc) <= 0 {
                            return Err("Could not finish print page".into());
                        }
                    }
                }
                Ok(())
            })();
            crate::rich_print::release(edit);
            if temporary {
                DestroyWindow(edit);
            }
            result
        }
    })();
    let final_result = match result {
        Ok(()) => {
            if EndDoc(dc) > 0 {
                Ok(())
            } else {
                Err("Could not finish print job".into())
            }
        }
        Err(e) => {
            AbortDoc(dc);
            Err(e)
        }
    };
    DeleteDC(dc);
    final_result
}

pub(super) fn grid_snapshot() -> Option<crate::grid::Snapshot> {
    STATE.with(|s| {
        let s = s.borrow();
        let s = s.as_ref()?;
        let rows=s.sparse_grid.clone().or_else(||s.grid_cache.clone().and_then(crate::SparseTable::from_dense))?;
        let cols=rows.columns();
        let (widths,heights)=s.app.grid_axes().unwrap_or_else(||(
            crate::GridAxis::new(cols,0,(0..cols).map(|c|(c,s.app.grid_column_width(c))).collect()).unwrap(),
            crate::GridAxis::new(rows.len(),0,(0..rows.len()).map(|r|(r,s.app.grid_row_height(r))).collect()).unwrap()
        ));
        Some(crate::grid::Snapshot {
            rows,
            widths,
            heights,
            selected: s.app.selected_cell(),
            range: s.app.grid_selection(),
            frozen: s.app.grid_frozen_panes(),
            readonly: s.app.text_readonly(),
            clipboard: s.app.grid_clipboard_commands(),
            pictures: s.app.grid_pictures(),
        })
    })
}
pub(super) fn grid_row_label(row:usize)->String {
    STATE.with(|s|s.borrow().as_ref().map_or_else(String::new,|s|s.app.grid_row_label(row)))
}
pub(super) fn grid_extend_view(row:usize,col:usize)->bool {
    STATE.with(|state| {
        let mut state=state.borrow_mut();let Some(state)=state.as_mut()else{return false};
        if !state.app.extend_grid_view(row,col){return false}
        state.sparse_grid=state.app.sparse_table();true
    })
}
pub(super) fn grid_style(row: usize, col: usize) -> Option<crate::GridCellStyle> {
    STATE.with(|s| s.borrow().as_ref()?.app.grid_cell_style(row, col))
}
pub(super) fn grid_editor_text() -> String {
    STATE.with(|s| {
        s.borrow()
            .as_ref()
            .map_or_else(String::new, |s| s.app.text())
    })
}
pub(super) unsafe fn grid_select(row: usize, col: usize) {
    grid_select_range(
        crate::GridRange {
            top: row,
            left: col,
            bottom: row,
            right: col,
        },
        (row, col),
    )
}
pub(super) unsafe fn grid_select_range(range: crate::GridRange, focus: (usize, usize)) {
    STATE.with(|s| {
        if let Some(s) = s.borrow_mut().as_mut() {
            s.app.select_grid_picture(None);
            s.app.select_grid_range_with_focus(range, focus)
        }
    });
    refresh();
}
pub(super) unsafe fn grid_select_picture(id: Option<u64>) {
    STATE.with(|state| {
        if let Some(state) = state.borrow_mut().as_mut() {
            state.app.select_grid_picture(id);
        }
    });
    refresh();
}
pub(super) unsafe fn grid_move_picture(id: u64, dx: f64, dy: f64) {
    let (hwnd, result) = STATE.with(|state| {
        let mut state = state.borrow_mut();
        let state = state.as_mut().unwrap();
        (state.hwnd, state.app.move_grid_picture(id, dx, dy))
    });
    if let Err(error) = result {
        msg(hwnd, &error, MB_OK | MB_ICONERROR);
    }
    refresh();
}
pub(super) unsafe fn grid_picture_command(delete: bool) -> bool {
    let target = STATE.with(|state| {
        let state = state.borrow();
        let state = state.as_ref()?;
        if !state
            .app
            .grid_pictures()
            .iter()
            .any(|picture| picture.selected)
        {
            return None;
        }
        let commands = state.app.grid_picture_commands()?;
        Some((state.hwnd, if delete { commands.1 } else { commands.0 }))
    });
    if let Some((hwnd, id)) = target {
        command(hwnd, id, 0);
        return true;
    }
    false
}
pub(super) unsafe fn grid_resize(row: bool, index: usize, pixels: u16) {
    cancel_pending_open();
    let (window, result) = STATE.with(|s| {
        let mut s = s.borrow_mut();
        let s = s.as_mut().unwrap();
        let result = if row {
            s.app.resize_grid_row(index, pixels)
        } else {
            s.app.resize_grid_column(index, pixels)
        };
        (s.hwnd, result)
    });
    if let Err(e) = result {
        msg(window, &e, MB_OK | MB_ICONERROR);
    }
    refresh();
}
pub(super) unsafe fn grid_commit(value: String) -> bool {
    cancel_pending_open();
    let (window, result) = STATE.with(|s| {
        let mut s = s.borrow_mut();
        let s = s.as_mut().unwrap();
        (s.hwnd, s.app.edit(value))
    });
    match result {
        Ok(()) => {
            refresh();
            true
        }
        Err(e) => {
            msg(window, &e, MB_OK | MB_ICONERROR);
            false
        }
    }
}

pub(super) unsafe fn report_event(owner: HWND, id: u16, event: crate::ReportEvent) -> bool {
    let result = STATE.with(|s| {
        s.borrow_mut()
            .as_mut()
            .ok_or_else(|| "Document is unavailable".to_string())
            .and_then(|s| s.app.report_event(id, event))
    });
    match result {
        Ok(()) => {
            refresh();
            true
        }
        Err(error) => {
            msg(owner, &error, MB_OK | MB_ICONERROR);
            false
        }
    }
}
