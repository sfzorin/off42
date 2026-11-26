//! System-following dark appearance for native window chrome.
//!
//! Windows 10 1809 (build 17763) and later expose dark title bars through DWM and dark common-control
//! themes through `uxtheme.dll`. Several of those entry points are exported only by ordinal, so every
//! function is resolved at run time and the window stays light whenever one is missing, the build is
//! older, high contrast is active or the user chose the light app mode. Document content (pages,
//! sheets, slides, images) is never registered here and keeps its own colours.
#![cfg_attr(not(windows), allow(dead_code))]

/// Window background for frames, panels, toolbars and status bars.
pub(crate) const BACKGROUND: [u8; 3] = [32, 32, 32];
/// Background for list and edit fields inside dark chrome.
pub(crate) const FIELD: [u8; 3] = [43, 43, 43];
/// Text drawn on dark chrome.
pub(crate) const TEXT: [u8; 3] = [240, 240, 240];

/// Earliest build with the dark-mode uxtheme ordinals and a DWM dark title bar attribute.
pub(crate) const FIRST_DARK_BUILD: u32 = 17763;

/// The DWM attribute that switches the caption to dark: 19 before build 18985, 20 afterwards.
pub(crate) fn immersive_attribute(build: u32) -> Option<u32> {
    match build {
        b if b >= 18985 => Some(20),
        b if b >= FIRST_DARK_BUILD => Some(19),
        _ => None,
    }
}

/// Whether chrome should be dark given `AppsUseLightTheme` and the high-contrast state.
/// A missing value means light, which is the Windows default.
pub(crate) fn wants_dark(apps_use_light_theme: Option<u32>, high_contrast: bool, build: u32) -> bool {
    !high_contrast && build >= FIRST_DARK_BUILD && apps_use_light_theme == Some(0)
}

/// Whether a `WM_SETTINGCHANGE` parameter names the colour-set change Windows broadcasts on a theme switch.
pub(crate) fn is_color_set_change(name: &[u16]) -> bool {
    let end = name.iter().position(|&c| c == 0).unwrap_or(name.len());
    String::from_utf16_lossy(&name[..end]) == "ImmersiveColorSet"
}

/// Control theme class used for a native control class in dark mode.
pub(crate) fn dark_theme_for(class: &str) -> &'static str {
    match class.to_ascii_lowercase().as_str() {
        "edit" | "combobox" => "DarkMode_CFD",
        _ => "DarkMode_Explorer",
    }
}

#[cfg(windows)]
pub(crate) use sys::*;

#[cfg(windows)]
mod sys {
    use super::*;
    use std::cell::RefCell;
    use std::ffi::c_void;
    use std::mem::{size_of, transmute};
    use std::ptr::{null, null_mut};
    use windows_sys::Win32::{
        Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM},
        Graphics::Gdi::{
            CreateSolidBrush, FillRect, RedrawWindow, SetBkColor, SetTextColor, HBRUSH, HDC,
            RDW_ALLCHILDREN, RDW_ERASE, RDW_FRAME, RDW_INVALIDATE,
        },
        System::LibraryLoader::{GetModuleHandleW, GetProcAddress, LoadLibraryExW, LOAD_LIBRARY_SEARCH_SYSTEM32},
        UI::WindowsAndMessaging::{
            EnumChildWindows, GetClassNameW, GetClientRect, IsWindow, SetWindowPos, SystemParametersInfoW,
            SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, WM_CTLCOLORBTN,
            WM_CTLCOLORDLG, WM_CTLCOLOREDIT, WM_CTLCOLORLISTBOX, WM_CTLCOLORSTATIC, WM_ERASEBKGND,
            WM_SETTINGCHANGE,
        },
    };

    type AllowForWindow = unsafe extern "system" fn(HWND, bool) -> u8;
    type SetAppMode = unsafe extern "system" fn(i32) -> i32;
    type Flush = unsafe extern "system" fn();
    type SetTheme = unsafe extern "system" fn(HWND, *const u16, *const u16) -> i32;
    type DwmSet = unsafe extern "system" fn(HWND, u32, *const c_void, u32) -> i32;
    type RegGet = unsafe extern "system" fn(isize, *const u16, *const u16, u32, *mut u32, *mut c_void, *mut u32) -> i32;
    type RtlGetVersion = unsafe extern "system" fn(*mut VersionInfo) -> i32;

    #[repr(C)]
    struct VersionInfo {
        size: u32,
        major: u32,
        minor: u32,
        build: u32,
        platform: u32,
        service_pack: [u16; 128],
    }
    #[repr(C)]
    struct HighContrast {
        size: u32,
        flags: u32,
        scheme: *mut u16,
    }

    #[derive(Clone, Copy)]
    struct Api {
        build: u32,
        attribute: u32,
        allow_for_window: AllowForWindow,
        set_app_mode: SetAppMode,
        flush_menus: Flush,
        refresh_policy: Flush,
        set_theme: SetTheme,
        dwm_set: DwmSet,
        reg_get: RegGet,
    }

    #[derive(Default)]
    struct Theme {
        loaded: bool,
        api: Option<Api>,
        dark: bool,
        /// Set once any window has been switched to dark, so a light start never touches control themes.
        touched: bool,
        background: usize,
        field: usize,
        frames: Vec<usize>,
        dialogs: Vec<usize>,
        chrome: Vec<usize>,
    }

    thread_local! { static THEME: RefCell<Theme> = RefCell::new(Theme::default()); }

    fn wide(value: &str) -> Vec<u16> {
        value.encode_utf16().chain([0]).collect()
    }
    fn colour(value: [u8; 3]) -> u32 {
        value[0] as u32 | (value[1] as u32) << 8 | (value[2] as u32) << 16
    }

    unsafe fn load() -> Option<Api> {
        let ntdll = GetModuleHandleW(wide("ntdll.dll").as_ptr());
        let version: RtlGetVersion = transmute(GetProcAddress(ntdll, c"RtlGetVersion".as_ptr() as _)?);
        let mut info: VersionInfo = std::mem::zeroed();
        info.size = size_of::<VersionInfo>() as u32;
        if version(&mut info) != 0 || info.major < 10 {
            return None;
        }
        let attribute = immersive_attribute(info.build)?;
        let library = |name: &str| LoadLibraryExW(wide(name).as_ptr(), null_mut(), LOAD_LIBRARY_SEARCH_SYSTEM32);
        let uxtheme = library("uxtheme.dll");
        let dwmapi = library("dwmapi.dll");
        let advapi = library("advapi32.dll");
        if uxtheme.is_null() || dwmapi.is_null() || advapi.is_null() {
            return None;
        }
        let ordinal = |n: usize| GetProcAddress(uxtheme, n as *const u8);
        Some(Api {
            build: info.build,
            attribute,
            refresh_policy: transmute(ordinal(104)?),
            allow_for_window: transmute(ordinal(133)?),
            set_app_mode: transmute(ordinal(135)?),
            flush_menus: transmute(ordinal(136)?),
            set_theme: transmute(GetProcAddress(uxtheme, c"SetWindowTheme".as_ptr() as _)?),
            dwm_set: transmute(GetProcAddress(dwmapi, c"DwmSetWindowAttribute".as_ptr() as _)?),
            reg_get: transmute(GetProcAddress(advapi, c"RegGetValueW".as_ptr() as _)?),
        })
    }

    unsafe fn high_contrast() -> bool {
        let mut value = HighContrast { size: size_of::<HighContrast>() as u32, flags: 0, scheme: null_mut() };
        // SPI_GETHIGHCONTRAST; HCF_HIGHCONTRASTON = 1.
        SystemParametersInfoW(0x42, value.size, &mut value as *mut _ as *mut c_void, 0) != 0 && value.flags & 1 != 0
    }

    unsafe fn system_prefers_dark(api: &Api) -> bool {
        let key = wide(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize");
        let name = wide("AppsUseLightTheme");
        let (mut value, mut size) = (0u32, 4u32);
        // HKEY_CURRENT_USER, RRF_RT_REG_DWORD.
        let read = (api.reg_get)(
            0x8000_0001u32 as i32 as isize,
            key.as_ptr(),
            name.as_ptr(),
            0x10,
            null_mut(),
            &mut value as *mut u32 as *mut c_void,
            &mut size,
        ) == 0;
        wants_dark(read.then_some(value), high_contrast(), api.build)
    }

    /// Runs `f` on the thread's theme state. Win32 calls that can re-enter a window procedure happen
    /// only after `with` returns, so the state is never borrowed twice.
    fn with<R>(f: impl FnOnce(&mut Theme) -> R) -> R {
        let fresh = THEME.with(|theme| !std::mem::replace(&mut theme.borrow_mut().loaded, true));
        if fresh {
            if let Some(api) = unsafe { load() } {
                unsafe {
                    // PreferredAppMode::AllowDark (AllowDarkModeForApp(true) on 1809): popup menus follow the system.
                    (api.set_app_mode)(1);
                    (api.flush_menus)();
                }
                let dark = unsafe { system_prefers_dark(&api) };
                let background = unsafe { CreateSolidBrush(colour(BACKGROUND)) } as usize;
                let field = unsafe { CreateSolidBrush(colour(FIELD)) } as usize;
                THEME.with(|theme| {
                    let mut theme = theme.borrow_mut();
                    theme.api = Some(api);
                    theme.dark = dark;
                    theme.background = background;
                    theme.field = field;
                });
            }
        }
        THEME.with(|theme| f(&mut theme.borrow_mut()))
    }

    /// The loaded API when chrome is dark, recording that a window has been switched.
    fn dark_api(t: &mut Theme) -> Option<Api> {
        let api = t.api.filter(|_| t.dark)?;
        t.touched = true;
        Some(api)
    }

    /// Loads the APIs and reads the system preference; call before creating the first window.
    pub(crate) fn init() {
        with(|_| ());
    }

    unsafe fn apply_frame(api: &Api, h: HWND, dark: bool, redraw: bool) {
        (api.allow_for_window)(h, dark);
        let value: i32 = dark as i32;
        (api.dwm_set)(h, api.attribute, &value as *const i32 as *const c_void, 4);
        if redraw {
            SetWindowPos(h, null_mut(), 0, 0, 0, 0, SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE);
        }
    }

    unsafe fn apply_control(api: &Api, h: HWND, dark: bool) {
        (api.allow_for_window)(h, dark);
        if dark {
            let mut class = [0u16; 64];
            let length = GetClassNameW(h, class.as_mut_ptr(), class.len() as i32).max(0) as usize;
            let theme = wide(dark_theme_for(&String::from_utf16_lossy(&class[..length])));
            (api.set_theme)(h, theme.as_ptr(), null());
        } else {
            (api.set_theme)(h, null(), null());
        }
    }

    /// Registers a top-level window: dark caption, and a dark erased background when its procedure calls [`handle`].
    pub(crate) unsafe fn frame(h: HWND) {
        if h.is_null() {
            return;
        }
        let api = with(|t| {
            if t.frames.contains(&(h as usize)) {
                return None;
            }
            t.frames.push(h as usize);
            dark_api(t)
        });
        if let Some(api) = api {
            apply_frame(&api, h, true, false);
        }
    }

    /// Registers child controls that belong to chrome (panels, toolbars, status bars), never document content.
    pub(crate) unsafe fn chrome(controls: &[HWND]) {
        let (api, added) = with(|t| {
            let added: Vec<HWND> =
                controls.iter().copied().filter(|&h| !h.is_null() && !t.chrome.contains(&(h as usize))).collect();
            t.chrome.extend(added.iter().map(|&h| h as usize));
            (if added.is_empty() { None } else { dark_api(t) }, added)
        });
        if let Some(api) = api {
            for h in added {
                apply_control(&api, h, true);
            }
        }
    }

    /// Registers a dialog whose every child is chrome (input prompts).
    pub(crate) unsafe fn dialog(h: HWND) {
        frame(h);
        with(|t| {
            if !t.dialogs.contains(&(h as usize)) {
                t.dialogs.push(h as usize)
            }
        });
        let mut children: Vec<HWND> = Vec::new();
        unsafe extern "system" fn collect(child: HWND, list: LPARAM) -> i32 {
            (*(list as *mut Vec<HWND>)).push(child);
            1
        }
        EnumChildWindows(h, Some(collect), &mut children as *mut _ as LPARAM);
        chrome(&children);
    }

    unsafe fn refresh() {
        let Some(api) = with(|t| t.api) else { return };
        let dark = system_prefers_dark(&api);
        let work = with(|t| {
            t.frames.retain(|&h| IsWindow(h as HWND) != 0);
            t.dialogs.retain(|&h| IsWindow(h as HWND) != 0);
            t.chrome.retain(|&h| IsWindow(h as HWND) != 0);
            let changed = dark != t.dark && (dark || t.touched);
            t.dark = dark;
            t.touched |= changed;
            changed.then(|| (t.frames.clone(), t.chrome.clone()))
        });
        let Some((frames, chrome)) = work else { return };
        (api.refresh_policy)();
        (api.flush_menus)();
        for h in chrome {
            apply_control(&api, h as HWND, dark);
        }
        for &h in &frames {
            apply_frame(&api, h as HWND, dark, true);
        }
        for h in frames {
            RedrawWindow(h as HWND, null(), null_mut(), RDW_INVALIDATE | RDW_ERASE | RDW_FRAME | RDW_ALLCHILDREN);
        }
    }

    /// Handles colour and settings messages for a registered window; `None` lets the caller continue.
    pub(crate) unsafe fn handle(h: HWND, m: u32, w: WPARAM, l: LPARAM) -> Option<LRESULT> {
        match m {
            WM_CTLCOLORSTATIC | WM_CTLCOLORBTN | WM_CTLCOLORLISTBOX | WM_CTLCOLOREDIT | WM_CTLCOLORDLG => {
                let field = matches!(m, WM_CTLCOLORLISTBOX | WM_CTLCOLOREDIT);
                let brush = with(|t| {
                    let ours = t.dialogs.contains(&(h as usize)) || t.chrome.contains(&(l as usize));
                    (t.dark && ours).then_some(if field { t.field } else { t.background })
                })?;
                let dc = w as HDC;
                SetTextColor(dc, colour(TEXT));
                SetBkColor(dc, colour(if field { FIELD } else { BACKGROUND }));
                Some(brush as LRESULT)
            }
            WM_ERASEBKGND => {
                let brush = with(|t| (t.dark && t.frames.contains(&(h as usize))).then_some(t.background))?;
                let mut rect: RECT = std::mem::zeroed();
                GetClientRect(h, &mut rect);
                FillRect(w as HDC, &rect, brush as HBRUSH);
                Some(1)
            }
            WM_SETTINGCHANGE => {
                // SPI_SETHIGHCONTRAST, or the "ImmersiveColorSet" broadcast sent when the app mode changes.
                let named = l != 0 && {
                    let text = l as *const u16;
                    let length = (0..64).position(|i| *text.add(i) == 0).unwrap_or(64);
                    is_color_set_change(std::slice::from_raw_parts(text, length))
                };
                if named || w == 0x43 {
                    refresh();
                }
                None
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dark_only_when_system_light_value_is_zero() {
        assert!(wants_dark(Some(0), false, 22631));
        assert!(!wants_dark(Some(1), false, 22631));
        assert!(!wants_dark(None, false, 22631), "missing value stays light");
        assert!(!wants_dark(Some(0), true, 22631), "high contrast keeps system colours");
        assert!(!wants_dark(Some(0), false, 17134), "pre-1809 builds stay light");
        assert!(wants_dark(Some(0), false, 17763));
    }

    #[test]
    fn dwm_attribute_follows_build() {
        assert_eq!(immersive_attribute(17134), None);
        assert_eq!(immersive_attribute(17763), Some(19));
        assert_eq!(immersive_attribute(18362), Some(19));
        assert_eq!(immersive_attribute(18985), Some(20));
        assert_eq!(immersive_attribute(20348), Some(20));
        assert_eq!(immersive_attribute(26100), Some(20));
    }

    #[test]
    fn recognises_colour_set_broadcast() {
        let name: Vec<u16> = "ImmersiveColorSet\0junk".encode_utf16().collect();
        assert!(is_color_set_change(&name));
        let other: Vec<u16> = "WindowsThemeElement".encode_utf16().collect();
        assert!(!is_color_set_change(&other));
        assert!(!is_color_set_change(&[]));
    }

    #[test]
    fn chrome_is_darker_than_content_paper() {
        let luminance = |c: [u8; 3]| c.iter().map(|&v| v as u32).sum::<u32>() / 3;
        assert!(luminance(BACKGROUND) < 64 && luminance(FIELD) < 64 && luminance(TEXT) > 200);
        assert_eq!(dark_theme_for("Edit"), "DarkMode_CFD");
        assert_eq!(dark_theme_for("ComboBox"), "DarkMode_CFD");
        assert_eq!(dark_theme_for("ListBox"), "DarkMode_Explorer");
        assert_eq!(dark_theme_for("Button"), "DarkMode_Explorer");
    }
}
