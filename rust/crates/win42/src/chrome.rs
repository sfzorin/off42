//! OFF42 window chrome shared by the Windows apps: palette, app accents, interface fonts,
//! anti-aliased shapes and the thin-stroke icon set.
//!
//! The look follows the web redesign: flat chrome on a neutral background, one rounded toolbar strip,
//! a title row with the file name, its format and a primary Save pill, a grey canvas with white pages,
//! and 1 px separators instead of sunken borders. Every interactive element stays a native Win32 control
//! (menu bar, buttons, list boxes, edit controls); this module only paints them.
#![cfg_attr(not(windows), allow(dead_code))]

pub(crate) type Rgb = [u8; 3];

/// Chrome colours for one appearance. Document content (pages, cells, slides) keeps its own colours.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Palette {
    /// Window, title row, menu bar, side panels and status line.
    pub background: Rgb,
    /// The rounded toolbar strip.
    pub strip: Rgb,
    pub hover: Rgb,
    pub pressed: Rgb,
    /// 1 px lines between panels.
    pub separator: Rgb,
    /// Group dividers inside the toolbar strip and the format badge outline.
    pub divider: Rgb,
    pub text: Rgb,
    pub secondary: Rgb,
    pub icon: Rgb,
    /// Area around pages and slides.
    pub canvas: Rgb,
    /// Soft page shadow.
    pub shadow: Rgb,
    pub snackbar: Rgb,
    pub snackbar_text: Rgb,
}

pub(crate) const LIGHT: Palette = Palette {
    background: [0xf8, 0xf9, 0xfb],
    strip: [0xed, 0xf1, 0xf7],
    hover: [0xdd, 0xe2, 0xea],
    pressed: [0xcf, 0xd6, 0xe1],
    separator: [0xe1, 0xe3, 0xe6],
    divider: [0xc7, 0xcc, 0xd4],
    text: [0x1f, 0x1f, 0x1f],
    secondary: [0x5b, 0x5f, 0x66],
    icon: [0x44, 0x47, 0x46],
    canvas: [0xf8, 0xf9, 0xfb],
    shadow: [0x3c, 0x40, 0x43],
    snackbar: [0x30, 0x33, 0x3a],
    snackbar_text: [0xf1, 0xf3, 0xf4],
};

pub(crate) const DARK: Palette = Palette {
    background: [0x1f, 0x20, 0x23],
    strip: [0x2c, 0x2f, 0x35],
    hover: [0x3b, 0x3f, 0x47],
    pressed: [0x47, 0x4c, 0x55],
    separator: [0x35, 0x38, 0x3e],
    divider: [0x4c, 0x50, 0x58],
    text: [0xe8, 0xea, 0xed],
    secondary: [0x9a, 0xa0, 0xa6],
    icon: [0xc4, 0xc7, 0xc5],
    canvas: [0x17, 0x18, 0x1b],
    shadow: [0x00, 0x00, 0x00],
    snackbar: [0xe8, 0xea, 0xed],
    snackbar_text: [0x1f, 0x1f, 0x1f],
};

/// Desk colour around Rich Edit pages. DOC42's editor is document content and stays paper-like in both
/// appearances, so this does not follow dark mode.
pub(crate) const DESK: Rgb = [0xee, 0xf0, 0xf3];
pub(crate) const DESK_EDGE: Rgb = [0xd5, 0xd9, 0xdf];

/// Per-app accent, the same family as the web apps and the macOS icons.
pub(crate) fn accent(app: &str) -> Rgb {
    match app {
        "DOC42" => [0x2f, 0x6f, 0xde],
        "XLS42" => [0x1e, 0x8e, 0x4f],
        "PPT42" => [0xe0, 0x89, 0x1b],
        "PDF42" => [0xd2, 0x3f, 0x31],
        "IMG42" => [0x7b, 0x4f, 0xc4],
        "SCN42" => [0x0b, 0x8a, 0x8a],
        _ => [0x2f, 0x6f, 0xde],
    }
}

/// `a` moved towards `b` by `t` (0 keeps `a`, 1 gives `b`).
pub(crate) fn mix(a: Rgb, b: Rgb, t: f64) -> Rgb {
    let t = t.clamp(0.0, 1.0);
    let channel = |x: u8, y: u8| (f64::from(x) * (1.0 - t) + f64::from(y) * t).round() as u8;
    [channel(a[0], b[0]), channel(a[1], b[1]), channel(a[2], b[2])]
}

/// Tinted fill for the primary pill, checked toolbar buttons, the selected sheet tab and list selection.
pub(crate) fn accent_fill(accent: Rgb, dark: bool) -> Rgb {
    if dark {
        mix(accent, DARK.background, 0.62)
    } else {
        mix(accent, [255; 3], 0.74)
    }
}

/// Text and icons drawn on [`accent_fill`].
pub(crate) fn accent_ink(accent: Rgb, dark: bool) -> Rgb {
    if dark {
        mix(accent, [255; 3], 0.72)
    } else {
        mix(accent, [0; 3], 0.58)
    }
}

/// WCAG relative luminance (used by the palette tests).
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn luminance(c: Rgb) -> f64 {
    let linear = |v: u8| {
        let v = f64::from(v) / 255.0;
        if v <= 0.03928 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * linear(c[0]) + 0.7152 * linear(c[1]) + 0.0722 * linear(c[2])
}

/// WCAG contrast ratio between two colours.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn contrast(a: Rgb, b: Rgb) -> f64 {
    let (x, y) = (luminance(a), luminance(b));
    (x.max(y) + 0.05) / (x.min(y) + 0.05)
}

/// The icon set, drawn as thin strokes on a 24-unit grid like the web app's inline SVG icons.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Icon {
    Undo,
    Redo,
    Print,
    Save,
    Bold,
    Italic,
    Underline,
    AlignLeft,
    AlignCenter,
    AlignRight,
    Justify,
    Table,
    Chart,
    Copy,
    Paste,
    Plus,
    More,
    TextSelect,
    TextBox,
    Shape,
    Play,
    ZoomIn,
    ZoomOut,
    Fit,
    RotateLeft,
    RotateRight,
    Rectangle,
    Highlight,
    Scan,
    Crop,
    Previous,
    Next,
    Slide,
}

/// Icon for a toolbar command, chosen by its English source title; `None` keeps a text button.
pub(crate) fn icon_for(title: &str) -> Option<Icon> {
    let title = title.replace('&', "");
    let title = title.trim_end_matches(['…', '.']).trim();
    Some(match title {
        "Undo" => Icon::Undo,
        "Redo" => Icon::Redo,
        "Print" => Icon::Print,
        "Bold" | "Toggle bold" | "Bold Selection" => Icon::Bold,
        "Italic" | "Italic Selection" => Icon::Italic,
        "Underline" | "Underline Selection" => Icon::Underline,
        "Align left" | "Align Text Left" => Icon::AlignLeft,
        "Center" | "Align Text Center" => Icon::AlignCenter,
        "Align right" | "Align Text Right" => Icon::AlignRight,
        "Justify" => Icon::Justify,
        "Insert table" | "Insert Table" => Icon::Table,
        "Insert chart" | "Insert Chart" => Icon::Chart,
        "Copy cells" | "Copy Selected Text" | "Copy Objects" => Icon::Copy,
        "Paste cells" | "Paste Objects" => Icon::Paste,
        "Add sheet" => Icon::Plus,
        "More" => Icon::More,
        "View and Select Text" | "Select Text" => Icon::TextSelect,
        "Insert Text Box" => Icon::TextBox,
        "Insert Shape" => Icon::Shape,
        "Start Slide Show" => Icon::Play,
        "Zoom In" => Icon::ZoomIn,
        "Zoom Out" => Icon::ZoomOut,
        "Fit to Window" => Icon::Fit,
        "Rotate Left" => Icon::RotateLeft,
        "Rotate Right" => Icon::RotateRight,
        "Draw Rectangle" => Icon::Rectangle,
        "Highlight Selected Text" => Icon::Highlight,
        "From Scanner" => Icon::Scan,
        "Crop Page" => Icon::Crop,
        "Previous image in folder" => Icon::Previous,
        "Next image in folder" => Icon::Next,
        "Add Slide" => Icon::Slide,
        _ => return None,
    })
}

/// One stroke or fill of an icon in 24-unit coordinates.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Mark {
    Line(Vec<(f64, f64)>),
    Closed(Vec<(f64, f64)>),
    Dot(f64, f64, f64),
    Ring(f64, f64, f64),
}

fn arc(cx: f64, cy: f64, r: f64, from: f64, to: f64) -> Vec<(f64, f64)> {
    let steps = ((to - from).abs() / 12.0).ceil().max(2.0) as usize;
    (0..=steps)
        .map(|i| {
            let a = (from + (to - from) * i as f64 / steps as f64).to_radians();
            (cx + r * a.cos(), cy + r * a.sin())
        })
        .collect()
}

fn rect(x: f64, y: f64, w: f64, h: f64) -> Mark {
    Mark::Closed(vec![(x, y), (x + w, y), (x + w, y + h), (x, y + h)])
}

fn chain(parts: &[Vec<(f64, f64)>]) -> Vec<(f64, f64)> {
    parts.iter().flatten().copied().collect()
}

/// Geometry of an icon on the 24-unit grid (angles in degrees, y pointing down).
pub(crate) fn marks(icon: Icon) -> Vec<Mark> {
    use Mark::*;
    match icon {
        Icon::Undo => vec![
            Line(vec![(9.0, 14.0), (4.0, 9.0), (9.0, 4.0)]),
            Line(chain(&[vec![(4.0, 9.0), (15.0, 9.0)], arc(15.0, 14.0, 5.0, -90.0, 90.0), vec![(12.0, 19.0)]])),
        ],
        Icon::Redo => vec![
            Line(vec![(15.0, 14.0), (20.0, 9.0), (15.0, 4.0)]),
            Line(chain(&[vec![(20.0, 9.0), (9.0, 9.0)], arc(9.0, 14.0, 5.0, 270.0, 90.0), vec![(12.0, 19.0)]])),
        ],
        Icon::Print => vec![
            Line(vec![(7.0, 9.0), (7.0, 3.0), (17.0, 3.0), (17.0, 9.0)]),
            Closed(vec![(3.0, 9.0), (21.0, 9.0), (21.0, 17.0), (3.0, 17.0)]),
            rect(7.0, 14.0, 10.0, 7.0),
        ],
        Icon::Save => vec![
            Line(vec![(12.0, 4.0), (12.0, 15.0)]),
            Line(vec![(7.0, 10.0), (12.0, 15.0), (17.0, 10.0)]),
            Line(vec![(5.0, 20.0), (19.0, 20.0)]),
        ],
        Icon::Bold => vec![
            Line(chain(&[vec![(7.0, 12.0), (7.0, 5.0), (13.0, 5.0)], arc(13.0, 8.5, 3.5, -90.0, 90.0), vec![(7.0, 12.0)]])),
            Line(chain(&[vec![(7.0, 12.0), (14.0, 12.0)], arc(14.0, 15.5, 3.5, -90.0, 90.0), vec![(7.0, 19.0), (7.0, 12.0)]])),
        ],
        Icon::Italic => vec![
            Line(vec![(10.0, 5.0), (18.0, 5.0)]),
            Line(vec![(6.0, 19.0), (14.0, 19.0)]),
            Line(vec![(14.0, 5.0), (10.0, 19.0)]),
        ],
        Icon::Underline => vec![
            Line(chain(&[vec![(7.0, 4.0), (7.0, 11.0)], arc(12.0, 11.0, 5.0, 180.0, 0.0), vec![(17.0, 4.0)]])),
            Line(vec![(5.0, 20.0), (19.0, 20.0)]),
        ],
        Icon::AlignLeft => lines(&[(4.0, 20.0), (4.0, 14.0), (4.0, 20.0), (4.0, 14.0)]),
        Icon::AlignCenter => lines(&[(4.0, 20.0), (7.0, 17.0), (4.0, 20.0), (7.0, 17.0)]),
        Icon::AlignRight => lines(&[(4.0, 20.0), (10.0, 20.0), (4.0, 20.0), (10.0, 20.0)]),
        Icon::Justify => lines(&[(4.0, 20.0), (4.0, 20.0), (4.0, 20.0), (4.0, 20.0)]),
        Icon::Table => vec![
            rect(3.5, 5.0, 17.0, 14.0),
            Line(vec![(3.5, 10.0), (20.5, 10.0)]),
            Line(vec![(3.5, 14.5), (20.5, 14.5)]),
            Line(vec![(10.0, 5.0), (10.0, 19.0)]),
        ],
        Icon::Chart => vec![
            Line(vec![(4.0, 4.0), (4.0, 20.0), (20.0, 20.0)]),
            Line(vec![(8.5, 16.0), (8.5, 12.0)]),
            Line(vec![(12.5, 16.0), (12.5, 7.0)]),
            Line(vec![(16.5, 16.0), (16.5, 10.0)]),
        ],
        Icon::Copy => vec![
            Closed(vec![(9.0, 9.0), (20.0, 9.0), (20.0, 20.0), (9.0, 20.0)]),
            Line(vec![(15.0, 9.0), (15.0, 4.0), (4.0, 4.0), (4.0, 15.0), (9.0, 15.0)]),
        ],
        Icon::Paste => vec![
            Line(vec![(9.0, 5.0), (6.0, 5.0), (6.0, 21.0), (18.0, 21.0), (18.0, 5.0), (15.0, 5.0)]),
            rect(9.0, 3.0, 6.0, 4.0),
        ],
        Icon::Plus => vec![Line(vec![(12.0, 5.0), (12.0, 19.0)]), Line(vec![(5.0, 12.0), (19.0, 12.0)])],
        Icon::More => vec![Dot(5.5, 12.0, 1.7), Dot(12.0, 12.0, 1.7), Dot(18.5, 12.0, 1.7)],
        Icon::TextSelect => vec![
            Line(vec![(9.0, 4.0), (15.0, 4.0)]),
            Line(vec![(9.0, 20.0), (15.0, 20.0)]),
            Line(vec![(12.0, 4.0), (12.0, 20.0)]),
        ],
        Icon::TextBox => vec![
            rect(3.5, 4.0, 17.0, 16.0),
            Line(vec![(8.0, 8.5), (16.0, 8.5)]),
            Line(vec![(12.0, 8.5), (12.0, 16.0)]),
        ],
        Icon::Shape => vec![Ring(9.0, 9.0, 5.0), Closed(vec![(11.0, 11.0), (20.0, 11.0), (20.0, 20.0), (11.0, 20.0)])],
        Icon::Play => vec![Closed(vec![(8.0, 5.0), (19.0, 12.0), (8.0, 19.0)])],
        Icon::ZoomIn => vec![
            Ring(10.5, 10.5, 6.5),
            Line(vec![(15.5, 15.5), (20.0, 20.0)]),
            Line(vec![(10.5, 7.5), (10.5, 13.5)]),
            Line(vec![(7.5, 10.5), (13.5, 10.5)]),
        ],
        Icon::ZoomOut => vec![
            Ring(10.5, 10.5, 6.5),
            Line(vec![(15.5, 15.5), (20.0, 20.0)]),
            Line(vec![(7.5, 10.5), (13.5, 10.5)]),
        ],
        Icon::Fit => vec![
            Line(vec![(4.0, 9.0), (4.0, 4.0), (9.0, 4.0)]),
            Line(vec![(15.0, 4.0), (20.0, 4.0), (20.0, 9.0)]),
            Line(vec![(20.0, 15.0), (20.0, 20.0), (15.0, 20.0)]),
            Line(vec![(9.0, 20.0), (4.0, 20.0), (4.0, 15.0)]),
        ],
        Icon::RotateLeft => vec![
            Line(arc(12.0, 13.0, 7.0, 200.0, 520.0)),
            Line(vec![(4.0, 6.5), (5.4, 10.6), (9.4, 9.0)]),
        ],
        Icon::RotateRight => vec![
            Line(arc(12.0, 13.0, 7.0, 340.0, 20.0)),
            Line(vec![(20.0, 6.5), (18.6, 10.6), (14.6, 9.0)]),
        ],
        Icon::Rectangle => vec![rect(4.0, 6.0, 16.0, 12.0)],
        Icon::Highlight => vec![
            Line(vec![(14.0, 4.0), (20.0, 10.0), (11.0, 19.0), (5.0, 13.0), (14.0, 4.0)]),
            Line(vec![(5.0, 13.0), (3.0, 20.0), (8.0, 18.0)]),
        ],
        Icon::Scan => vec![
            Line(vec![(4.0, 8.0), (4.0, 4.0), (8.0, 4.0)]),
            Line(vec![(16.0, 4.0), (20.0, 4.0), (20.0, 8.0)]),
            Line(vec![(20.0, 16.0), (20.0, 20.0), (16.0, 20.0)]),
            Line(vec![(8.0, 20.0), (4.0, 20.0), (4.0, 16.0)]),
            Line(vec![(3.0, 12.0), (21.0, 12.0)]),
        ],
        Icon::Crop => vec![
            Line(vec![(7.0, 3.0), (7.0, 17.0), (21.0, 17.0)]),
            Line(vec![(3.0, 7.0), (17.0, 7.0), (17.0, 21.0)]),
        ],
        Icon::Previous => vec![Line(vec![(15.0, 5.0), (8.0, 12.0), (15.0, 19.0)])],
        Icon::Next => vec![Line(vec![(9.0, 5.0), (16.0, 12.0), (9.0, 19.0)])],
        Icon::Slide => vec![
            rect(3.0, 6.0, 15.0, 11.0),
            Line(vec![(18.0, 9.0), (21.0, 9.0), (21.0, 20.0), (6.0, 20.0), (6.0, 17.0)]),
        ],
    }
}

/// Four horizontal text lines from per-line (start, end) pairs: the alignment icons.
fn lines(spans: &[(f64, f64); 4]) -> Vec<Mark> {
    [6.0, 10.0, 14.0, 18.0]
        .iter()
        .zip(spans)
        .map(|(&y, &(a, b))| Mark::Line(vec![(a, y), (b, y)]))
        .collect()
}

#[cfg(windows)]
pub(crate) use sys::*;

#[cfg(windows)]
mod sys {
    use super::*;
    use std::cell::RefCell;
    use std::collections::HashMap;
    use std::mem::{size_of, zeroed};
    use std::ptr::{null, null_mut};
    use windows_sys::Win32::{
        Foundation::{HWND, LPARAM, LRESULT, RECT, SIZE, WPARAM},
        Graphics::Gdi::*,
        UI::Controls::{DRAWITEMSTRUCT, ODS_DISABLED, ODS_GRAYED, ODS_HOTLIGHT, ODS_NOACCEL, ODS_SELECTED},
        UI::WindowsAndMessaging::*,
    };

    pub(crate) fn palette() -> Palette {
        if crate::theme::is_dark() {
            DARK
        } else {
            LIGHT
        }
    }

    pub(crate) fn colorref(c: Rgb) -> u32 {
        c[0] as u32 | (c[1] as u32) << 8 | (c[2] as u32) << 16
    }

    pub(crate) fn scale(dpi: i32, value: i32) -> i32 {
        value * dpi.max(1) / 96
    }

    thread_local! {
        static BRUSHES: RefCell<HashMap<u32, usize>> = RefCell::new(HashMap::new());
        static FONTS: RefCell<HashMap<(i32, i32), usize>> = RefCell::new(HashMap::new());
        static FACE: RefCell<Option<Vec<u16>>> = const { RefCell::new(None) };
    }

    /// Cached solid brush; brushes live as long as the process.
    pub(crate) unsafe fn brush(c: Rgb) -> HBRUSH {
        let key = colorref(c);
        BRUSHES.with(|cache| *cache.borrow_mut().entry(key).or_insert_with(|| CreateSolidBrush(key) as usize)) as HBRUSH
    }

    pub(crate) unsafe fn fill(dc: HDC, rect: &RECT, c: Rgb) {
        FillRect(dc, rect, brush(c));
    }

    /// "Segoe UI Variable Text" where installed (Windows 11), otherwise "Segoe UI".
    unsafe fn face() -> Vec<u16> {
        if let Some(face) = FACE.with(|f| f.borrow().clone()) {
            return face;
        }
        let preferred = crate::wide("Segoe UI Variable Text");
        let font = CreateFontW(-14, 0, 0, 0, 400, 0, 0, 0, DEFAULT_CHARSET as u32, 0, 0, CLEARTYPE_QUALITY as u32, 0, preferred.as_ptr());
        let dc = CreateCompatibleDC(null_mut());
        let mut found = false;
        if !font.is_null() && !dc.is_null() {
            let previous = SelectObject(dc, font);
            let mut name = [0u16; 64];
            GetTextFaceW(dc, name.len() as i32, name.as_mut_ptr());
            let end = name.iter().position(|&c| c == 0).unwrap_or(name.len());
            found = String::from_utf16_lossy(&name[..end]).eq_ignore_ascii_case("Segoe UI Variable Text");
            SelectObject(dc, previous);
        }
        if !dc.is_null() {
            DeleteDC(dc);
        }
        if !font.is_null() {
            DeleteObject(font);
        }
        let face = if found { preferred } else { crate::wide("Segoe UI") };
        FACE.with(|f| *f.borrow_mut() = Some(face.clone()));
        face
    }

    /// Interface font of `px` device pixels (already DPI-scaled) and the given weight; cached.
    pub(crate) unsafe fn font(px: i32, weight: i32) -> HFONT {
        let key = (px.max(6), weight);
        if let Some(font) = FONTS.with(|cache| cache.borrow().get(&key).copied()) {
            return font as HFONT;
        }
        let face = face();
        let font = CreateFontW(-key.0, 0, 0, 0, weight, 0, 0, 0, DEFAULT_CHARSET as u32, 0, 0, CLEARTYPE_QUALITY as u32, 0, face.as_ptr());
        let font = if font.is_null() { GetStockObject(DEFAULT_GUI_FONT) as HFONT } else { font };
        FONTS.with(|cache| cache.borrow_mut().insert(key, font as usize));
        font
    }

    /// Interface text size in logical pixels for body text, panels and the toolbar.
    pub(crate) const BODY: i32 = 14;
    pub(crate) const SMALL: i32 = 13;
    pub(crate) const TITLE: i32 = 18;

    pub(crate) unsafe fn text_size(dc: HDC, font: HFONT, text: &str) -> SIZE {
        let previous = SelectObject(dc, font);
        let units: Vec<u16> = text.encode_utf16().collect();
        let mut size: SIZE = zeroed();
        GetTextExtentPoint32W(dc, units.as_ptr(), units.len() as i32, &mut size);
        SelectObject(dc, previous);
        size
    }

    pub(crate) unsafe fn measure(hwnd: HWND, font: HFONT, text: &str) -> SIZE {
        let dc = GetDC(hwnd);
        let size = text_size(dc, font, text);
        ReleaseDC(hwnd, dc);
        size
    }

    /// Draw `text` in `rect` with the given colour, font and `DT_*` flags.
    pub(crate) unsafe fn text(dc: HDC, rect: RECT, value: &str, color: Rgb, font: HFONT, flags: u32) {
        let previous = SelectObject(dc, font);
        SetBkMode(dc, TRANSPARENT as i32);
        SetTextColor(dc, colorref(color));
        let mut units: Vec<u16> = value.encode_utf16().collect();
        let mut rect = rect;
        DrawTextW(dc, units.as_mut_ptr(), units.len() as i32, &mut rect, flags | DT_NOPREFIX);
        SelectObject(dc, previous);
    }

    /// Supersampled drawing surface: everything is drawn white on black at `k` times the size, then
    /// reduced to coverage and composited in one colour.
    pub(crate) struct Surface {
        dc: HDC,
        k: f64,
    }

    impl Surface {
        fn p(&self, x: f64, y: f64) -> windows_sys::Win32::Foundation::POINT {
            windows_sys::Win32::Foundation::POINT { x: (x * self.k).round() as i32, y: (y * self.k).round() as i32 }
        }
        /// Filled rounded rectangle.
        pub(crate) fn round_rect(&self, x: f64, y: f64, w: f64, h: f64, r: f64) {
            unsafe {
                let old_pen = SelectObject(self.dc, GetStockObject(NULL_PEN));
                let old_brush = SelectObject(self.dc, GetStockObject(WHITE_BRUSH));
                let (a, b) = (self.p(x, y), self.p(x + w, y + h));
                let d = (2.0 * r * self.k).round() as i32;
                RoundRect(self.dc, a.x, a.y, b.x + 1, b.y + 1, d, d);
                SelectObject(self.dc, old_brush);
                SelectObject(self.dc, old_pen);
            }
        }
        /// Rounded-rectangle outline of the given stroke width, centred on the edge.
        pub(crate) fn round_outline(&self, x: f64, y: f64, w: f64, h: f64, r: f64, width: f64) {
            unsafe {
                let pen = self.pen(width);
                let old_pen = SelectObject(self.dc, pen);
                let old_brush = SelectObject(self.dc, GetStockObject(NULL_BRUSH));
                let (a, b) = (self.p(x, y), self.p(x + w, y + h));
                let d = (2.0 * r * self.k).round() as i32;
                RoundRect(self.dc, a.x, a.y, b.x, b.y, d, d);
                SelectObject(self.dc, old_brush);
                SelectObject(self.dc, old_pen);
                DeleteObject(pen);
            }
        }
        unsafe fn pen(&self, width: f64) -> HGDIOBJ {
            let brush = LOGBRUSH { lbStyle: BS_SOLID, lbColor: 0x00FF_FFFF, lbHatch: 0 };
            ExtCreatePen(
                (PS_GEOMETRIC | PS_SOLID | PS_ENDCAP_ROUND | PS_JOIN_ROUND) as u32,
                ((width * self.k).round() as u32).max(1),
                &brush,
                0,
                null(),
            ) as HGDIOBJ
        }
        /// Stroke an open or closed polyline.
        pub(crate) fn stroke(&self, points: &[(f64, f64)], width: f64, closed: bool) {
            if points.len() < 2 {
                return;
            }
            unsafe {
                let pen = self.pen(width);
                let old_pen = SelectObject(self.dc, pen);
                let old_brush = SelectObject(self.dc, GetStockObject(NULL_BRUSH));
                let pts: Vec<_> = points.iter().map(|&(x, y)| self.p(x, y)).collect();
                if closed {
                    Polygon(self.dc, pts.as_ptr(), pts.len() as i32);
                } else {
                    Polyline(self.dc, pts.as_ptr(), pts.len() as i32);
                }
                SelectObject(self.dc, old_brush);
                SelectObject(self.dc, old_pen);
                DeleteObject(pen);
            }
        }
        /// Filled polygon.
        pub(crate) fn polygon(&self, points: &[(f64, f64)]) {
            unsafe {
                let old_pen = SelectObject(self.dc, GetStockObject(NULL_PEN));
                let old_brush = SelectObject(self.dc, GetStockObject(WHITE_BRUSH));
                let pts: Vec<_> = points.iter().map(|&(x, y)| self.p(x, y)).collect();
                Polygon(self.dc, pts.as_ptr(), pts.len() as i32);
                SelectObject(self.dc, old_brush);
                SelectObject(self.dc, old_pen);
            }
        }
        pub(crate) fn circle(&self, cx: f64, cy: f64, r: f64) {
            self.round_rect(cx - r, cy - r, 2.0 * r, 2.0 * r, r);
        }
        pub(crate) fn ring(&self, cx: f64, cy: f64, r: f64, width: f64) {
            self.stroke(&arc(cx, cy, r, 0.0, 360.0), width, true);
        }
        /// Draw an icon into the square at (`x`, `y`) of `size` pixels.
        pub(crate) fn icon(&self, icon: Icon, x: f64, y: f64, size: f64) {
            let s = size / 24.0;
            let at = |p: &(f64, f64)| (x + p.0 * s, y + p.1 * s);
            let width = (1.8 * s).max(1.1);
            for mark in marks(icon) {
                match mark {
                    Mark::Line(points) => self.stroke(&points.iter().map(at).collect::<Vec<_>>(), width, false),
                    Mark::Closed(points) => self.stroke(&points.iter().map(at).collect::<Vec<_>>(), width, true),
                    Mark::Dot(cx, cy, r) => self.circle(x + cx * s, y + cy * s, r * s),
                    Mark::Ring(cx, cy, r) => self.ring(x + cx * s, y + cy * s, r * s, width),
                }
            }
        }
    }

    /// Paint anti-aliased shapes in one colour and opacity into `area` of `dc`. `draw` receives
    /// coordinates relative to the top-left of `area`.
    pub(crate) unsafe fn paint(dc: HDC, area: RECT, color: Rgb, opacity: u8, draw: impl FnOnce(&Surface)) {
        const K: i32 = 4;
        let (w, h) = (area.right - area.left, area.bottom - area.top);
        if w <= 0 || h <= 0 || w > 8192 || h > 4096 || opacity == 0 {
            return;
        }
        let mut info: BITMAPINFO = zeroed();
        info.bmiHeader.biSize = size_of::<BITMAPINFOHEADER>() as u32;
        info.bmiHeader.biWidth = w * K;
        info.bmiHeader.biHeight = -(h * K);
        info.bmiHeader.biPlanes = 1;
        info.bmiHeader.biBitCount = 32;
        info.bmiHeader.biCompression = BI_RGB;
        let big_dc = CreateCompatibleDC(dc);
        if big_dc.is_null() {
            return;
        }
        let mut big_bits = null_mut();
        let big = CreateDIBSection(big_dc, &info, DIB_RGB_COLORS, &mut big_bits, null_mut(), 0);
        if big.is_null() || big_bits.is_null() {
            DeleteDC(big_dc);
            return;
        }
        let old_big = SelectObject(big_dc, big);
        draw(&Surface { dc: big_dc, k: K as f64 });
        GdiFlush();
        let source = std::slice::from_raw_parts(big_bits as *const u32, (w * K * h * K) as usize);
        info.bmiHeader.biWidth = w;
        info.bmiHeader.biHeight = -h;
        let small_dc = CreateCompatibleDC(dc);
        let mut small_bits = null_mut();
        let small = if small_dc.is_null() {
            null_mut()
        } else {
            CreateDIBSection(small_dc, &info, DIB_RGB_COLORS, &mut small_bits, null_mut(), 0)
        };
        if !small.is_null() && !small_bits.is_null() {
            let target = std::slice::from_raw_parts_mut(small_bits as *mut u32, (w * h) as usize);
            let stride = (w * K) as usize;
            for y in 0..h as usize {
                for x in 0..w as usize {
                    let mut sum = 0u32;
                    for sy in 0..K as usize {
                        let row = (y * K as usize + sy) * stride + x * K as usize;
                        for sx in 0..K as usize {
                            sum += source[row + sx] & 0xff;
                        }
                    }
                    let coverage = sum / (K * K) as u32;
                    let a = coverage * opacity as u32 / 255;
                    target[y * w as usize + x] = (a << 24)
                        | ((color[0] as u32 * a / 255) << 16)
                        | ((color[1] as u32 * a / 255) << 8)
                        | (color[2] as u32 * a / 255);
                }
            }
            let old_small = SelectObject(small_dc, small);
            let blend = BLENDFUNCTION { BlendOp: AC_SRC_OVER as u8, BlendFlags: 0, SourceConstantAlpha: 255, AlphaFormat: AC_SRC_ALPHA as u8 };
            AlphaBlend(dc, area.left, area.top, w, h, small_dc, 0, 0, w, h, blend);
            SelectObject(small_dc, old_small);
            DeleteObject(small);
        }
        if !small_dc.is_null() {
            DeleteDC(small_dc);
        }
        SelectObject(big_dc, old_big);
        DeleteObject(big);
        DeleteDC(big_dc);
    }

    /// Soft drop shadow and white sheet for a page rectangle in client pixels.
    pub(crate) unsafe fn page(dc: HDC, page: RECT, dpi: i32, behind: Rgb) {
        let p = palette();
        // Axis-aligned layers need no anti-aliasing: a few blended rings, shifted down like a key light.
        let steps = scale(dpi, 4).max(2);
        for i in (1..=steps).rev() {
            let strength = if crate::theme::is_dark() { 0.5 } else { 0.22 } * (1.0 - f64::from(i - 1) / f64::from(steps)).powi(2);
            let ring = RECT { left: page.left - i, top: page.top - i + i / 2, right: page.right + i, bottom: page.bottom + i + i / 2 };
            fill(dc, &ring, mix(behind, p.shadow, strength));
        }
        fill(dc, &page, [255; 3]);
    }

    /// The menu bar drawn flat in the chrome colours through the undocumented but long-stable
    /// `WM_UAHDRAWMENU`/`WM_UAHDRAWMENUITEM` messages (used for dark menu bars by Notepad++ and others).
    /// Keyboard access, accelerators and accessibility stay those of the native menu.
    pub(crate) mod menubar {
        use super::*;
        pub(crate) const WM_UAHDRAWMENU: u32 = 0x0091;
        pub(crate) const WM_UAHDRAWMENUITEM: u32 = 0x0092;
        #[repr(C)]
        struct UahMenu {
            hmenu: HMENU,
            hdc: HDC,
            flags: u32,
        }
        #[repr(C)]
        struct UahMenuItem {
            position: i32,
            metrics: [u32; 8],
            popup: [u32; 5],
        }
        #[repr(C)]
        struct UahDrawMenuItem {
            dis: DRAWITEMSTRUCT,
            menu: UahMenu,
            item: UahMenuItem,
        }

        thread_local! { static MENU_FONTS: RefCell<HashMap<i32, usize>> = RefCell::new(HashMap::new()); }

        /// The menu font from the non-client metrics at `dpi`; cached for the process.
        unsafe fn menu_font(dpi: i32) -> HGDIOBJ {
            if let Some(font) = MENU_FONTS.with(|fonts| fonts.borrow().get(&dpi).copied()) {
                return font as HGDIOBJ;
            }
            let mut metrics: NONCLIENTMETRICSW = zeroed();
            metrics.cbSize = size_of::<NONCLIENTMETRICSW>() as u32;
            let font = if windows_sys::Win32::UI::HiDpi::SystemParametersInfoForDpi(
                SPI_GETNONCLIENTMETRICS,
                metrics.cbSize,
                &mut metrics as *mut _ as *mut core::ffi::c_void,
                0,
                dpi as u32,
            ) != 0
            {
                CreateFontIndirectW(&metrics.lfMenuFont)
            } else {
                null_mut()
            };
            let font = if font.is_null() { GetStockObject(DEFAULT_GUI_FONT) } else { font as HGDIOBJ };
            MENU_FONTS.with(|fonts| fonts.borrow_mut().insert(dpi, font as usize));
            font
        }

        unsafe fn bar_rect(hwnd: HWND) -> Option<RECT> {
            let mut info: MENUBARINFO = zeroed();
            info.cbSize = size_of::<MENUBARINFO>() as u32;
            if GetMenuBarInfo(hwnd, OBJID_MENU, 0, &mut info) == 0 {
                return None;
            }
            let mut window: RECT = zeroed();
            GetWindowRect(hwnd, &mut window);
            let mut bar = info.rcBar;
            OffsetRect(&mut bar, -window.left, -window.top);
            Some(bar)
        }

        /// Paint over the light line Windows draws between the menu bar and the client area.
        pub(crate) unsafe fn bottom_line(hwnd: HWND) {
            if GetMenu(hwnd).is_null() {
                return;
            }
            let mut client: RECT = zeroed();
            GetClientRect(hwnd, &mut client);
            let mut points = [
                windows_sys::Win32::Foundation::POINT { x: client.left, y: client.top },
                windows_sys::Win32::Foundation::POINT { x: client.right, y: client.bottom },
            ];
            MapWindowPoints(hwnd, null_mut(), points.as_mut_ptr(), 2);
            let mut window: RECT = zeroed();
            GetWindowRect(hwnd, &mut window);
            let line = RECT {
                left: points[0].x - window.left,
                top: points[0].y - window.top - 1,
                right: points[1].x - window.left,
                bottom: points[0].y - window.top,
            };
            let dc = GetWindowDC(hwnd);
            if !dc.is_null() {
                fill(dc, &line, palette().background);
                ReleaseDC(hwnd, dc);
            }
        }

        /// Handles the menu-bar drawing messages; `None` for every other message.
        pub(crate) unsafe fn handle(hwnd: HWND, message: u32, _w: WPARAM, l: LPARAM) -> Option<LRESULT> {
            match message {
                WM_UAHDRAWMENU if l != 0 => {
                    let menu = &*(l as *const UahMenu);
                    let rect = bar_rect(hwnd)?;
                    fill(menu.hdc, &rect, palette().background);
                    Some(0)
                }
                WM_UAHDRAWMENUITEM if l != 0 => {
                    let item = &*(l as *const UahDrawMenuItem);
                    let p = palette();
                    let mut buffer = [0u16; 256];
                    let mut info: MENUITEMINFOW = zeroed();
                    info.cbSize = size_of::<MENUITEMINFOW>() as u32;
                    info.fMask = MIIM_STRING;
                    info.dwTypeData = buffer.as_mut_ptr();
                    info.cch = (buffer.len() - 1) as u32;
                    GetMenuItemInfoW(item.menu.hmenu, item.item.position as u32, 1, &mut info);
                    let length = (info.cch as usize).min(buffer.len() - 1);
                    let state = item.dis.itemState;
                    let hot = state & (ODS_HOTLIGHT | ODS_SELECTED) != 0;
                    let disabled = state & (ODS_GRAYED | ODS_DISABLED) != 0;
                    let rect = item.dis.rcItem;
                    fill(item.menu.hdc, &rect, p.background);
                    if hot {
                        let dpi = super::dpi(hwnd);
                        let inset = scale(dpi, 2);
                        let area = RECT { left: rect.left, top: rect.top + inset, right: rect.right, bottom: rect.bottom - inset };
                        let radius = f64::from(scale(dpi, 4));
                        let (w, h) = (f64::from(area.right - area.left), f64::from(area.bottom - area.top));
                        paint(item.menu.hdc, area, if state & ODS_SELECTED != 0 { p.pressed } else { p.hover }, 255, |s| s.round_rect(0.0, 0.0, w, h, radius));
                    }
                    let mut rect = rect;
                    // The system sized the items with the non-client menu font; draw with the same font.
                    let previous_font = SelectObject(item.menu.hdc, menu_font(super::dpi(hwnd)));
                    SetBkMode(item.menu.hdc, TRANSPARENT as i32);
                    SetTextColor(item.menu.hdc, colorref(if disabled { p.secondary } else { p.text }));
                    let mut flags = DT_CENTER | DT_SINGLELINE | DT_VCENTER;
                    if state & ODS_NOACCEL != 0 {
                        flags |= DT_HIDEPREFIX;
                    }
                    DrawTextW(item.menu.hdc, buffer.as_mut_ptr(), length as i32, &mut rect, flags);
                    SelectObject(item.menu.hdc, previous_font);
                    Some(0)
                }
                _ => None,
            }
        }
    }

    pub(crate) unsafe fn dpi(hwnd: HWND) -> i32 {
        (windows_sys::Win32::UI::HiDpi::GetDpiForWindow(hwnd) as i32).max(96)
    }

    /// Transient bottom-left message, the native counterpart of the web app's snackbar. It is an owned,
    /// non-activating popup so it floats over document controls without taking focus.
    pub(crate) mod snackbar {
        use super::*;
        const CLASS: &str = "Off42Snackbar";
        const HIDE_TIMER: usize = 1;
        thread_local! { static WINDOW: std::cell::Cell<usize> = const { std::cell::Cell::new(0) }; }

        unsafe extern "system" fn proc(h: HWND, m: u32, w: WPARAM, l: LPARAM) -> LRESULT {
            match m {
                WM_TIMER if w == HIDE_TIMER => {
                    KillTimer(h, HIDE_TIMER);
                    ShowWindow(h, SW_HIDE);
                    0
                }
                WM_LBUTTONUP => {
                    ShowWindow(h, SW_HIDE);
                    0
                }
                WM_MOUSEACTIVATE => MA_NOACTIVATE as LRESULT,
                WM_ERASEBKGND => 1,
                WM_PAINT => {
                    let mut ps: PAINTSTRUCT = zeroed();
                    let dc = BeginPaint(h, &mut ps);
                    let mut rect: RECT = zeroed();
                    GetClientRect(h, &mut rect);
                    let p = palette();
                    let dpi = dpi(h);
                    fill(dc, &rect, p.snackbar);
                    let mut buffer = [0u16; 512];
                    let length = GetWindowTextW(h, buffer.as_mut_ptr(), buffer.len() as i32).max(0) as usize;
                    let label = String::from_utf16_lossy(&buffer[..length]);
                    let padding = scale(dpi, 16);
                    text(dc, RECT { left: padding, top: 0, right: rect.right - padding, bottom: rect.bottom }, &label, p.snackbar_text, font(scale(dpi, BODY), 400), DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS);
                    EndPaint(h, &ps);
                    0
                }
                _ => DefWindowProcW(h, m, w, l),
            }
        }

        unsafe fn window(owner: HWND) -> HWND {
            let existing = WINDOW.with(|w| w.get()) as HWND;
            if !existing.is_null() && IsWindow(existing) != 0 {
                return existing;
            }
            let instance = windows_sys::Win32::System::LibraryLoader::GetModuleHandleW(null());
            let class = crate::wide(CLASS);
            let wc = WNDCLASSW {
                lpfnWndProc: Some(proc),
                hInstance: instance,
                hCursor: LoadCursorW(null_mut(), IDC_ARROW),
                lpszClassName: class.as_ptr(),
                ..zeroed()
            };
            RegisterClassW(&wc);
            let h = CreateWindowExW(
                WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
                class.as_ptr(),
                null(),
                WS_POPUP,
                0,
                0,
                0,
                0,
                owner,
                null_mut(),
                instance,
                null(),
            );
            if !h.is_null() {
                round_corners(h);
            }
            WINDOW.with(|w| w.set(h as usize));
            h
        }

        /// Windows 11 rounds the popup's corners; older systems keep square ones.
        unsafe fn round_corners(h: HWND) {
            use windows_sys::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryExW, LOAD_LIBRARY_SEARCH_SYSTEM32};
            type DwmSet = unsafe extern "system" fn(HWND, u32, *const core::ffi::c_void, u32) -> i32;
            let library = LoadLibraryExW(crate::wide("dwmapi.dll").as_ptr(), null_mut(), LOAD_LIBRARY_SEARCH_SYSTEM32);
            if library.is_null() {
                return;
            }
            if let Some(function) = GetProcAddress(library, c"DwmSetWindowAttribute".as_ptr() as _) {
                let set: DwmSet = std::mem::transmute(function);
                // DWMWA_WINDOW_CORNER_PREFERENCE = 33, DWMWCP_ROUNDSMALL = 3.
                let value: u32 = 3;
                set(h, 33, &value as *const u32 as *const _, 4);
            }
        }

        /// Show `message` for four seconds at the bottom left of `owner`'s client area, above `bottom` pixels.
        pub(crate) unsafe fn show(owner: HWND, message: &str, bottom: i32) {
            let h = window(owner);
            if h.is_null() {
                return;
            }
            let dpi = dpi(owner);
            let width = (measure(owner, font(scale(dpi, BODY), 400), message).cx + scale(dpi, 32)).clamp(scale(dpi, 160), scale(dpi, 560));
            let height = scale(dpi, 44);
            let mut client: RECT = zeroed();
            GetClientRect(owner, &mut client);
            let mut origin = windows_sys::Win32::Foundation::POINT { x: scale(dpi, 16), y: client.bottom - bottom - scale(dpi, 16) - height };
            ClientToScreen(owner, &mut origin);
            SetWindowTextW(h, crate::wide(message).as_ptr());
            SetWindowPos(h, HWND_TOP, origin.x, origin.y, width.min(client.right - scale(dpi, 32)).max(1), height, SWP_NOACTIVATE | SWP_SHOWWINDOW);
            InvalidateRect(h, null(), 1);
            SetTimer(h, HIDE_TIMER, 4000, None);
        }

        /// Hide the message (window moved, resized or minimized).
        pub(crate) unsafe fn hide() {
            let h = WINDOW.with(|w| w.get()) as HWND;
            if !h.is_null() && IsWindow(h) != 0 {
                ShowWindow(h, SW_HIDE);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accents_match_the_web_apps() {
        assert_eq!(accent("DOC42"), [0x2f, 0x6f, 0xde]);
        assert_eq!(accent("XLS42"), [0x1e, 0x8e, 0x4f]);
        assert_eq!(accent("PPT42"), [0xe0, 0x89, 0x1b]);
        assert_eq!(accent("PDF42"), [0xd2, 0x3f, 0x31]);
        assert_ne!(accent("IMG42"), accent("SCN42"));
    }

    #[test]
    fn accent_fills_keep_readable_labels_in_both_appearances() {
        for app in ["DOC42", "XLS42", "PPT42", "PDF42", "IMG42", "SCN42"] {
            for dark in [false, true] {
                let a = accent(app);
                let ratio = contrast(accent_fill(a, dark), accent_ink(a, dark));
                assert!(ratio >= 4.5, "{app} dark={dark} contrast {ratio:.2}");
            }
        }
    }

    #[test]
    fn chrome_text_is_readable_and_light_chrome_stays_light() {
        for p in [LIGHT, DARK] {
            assert!(contrast(p.background, p.text) >= 7.0);
            assert!(contrast(p.background, p.secondary) >= 4.5);
            assert!(contrast(p.strip, p.icon) >= 4.5);
            assert!(contrast(p.snackbar, p.snackbar_text) >= 7.0);
        }
        // The dark-mode audit samples the status line: light chrome must read as light, dark as dark.
        let mean = |c: Rgb| c.iter().map(|&v| v as u32).sum::<u32>() / 3;
        assert!(mean(LIGHT.background) > 170 && mean(DARK.background) < 80);
        assert!(mean(DESK) > 160, "the DOC42 desk is document paper in both appearances");
    }

    #[test]
    fn every_icon_stays_inside_its_grid() {
        let all = [
            Icon::Undo, Icon::Redo, Icon::Print, Icon::Save, Icon::Bold, Icon::Italic, Icon::Underline,
            Icon::AlignLeft, Icon::AlignCenter, Icon::AlignRight, Icon::Justify, Icon::Table, Icon::Chart,
            Icon::Copy, Icon::Paste, Icon::Plus, Icon::More, Icon::TextSelect, Icon::TextBox, Icon::Shape,
            Icon::Play, Icon::ZoomIn, Icon::ZoomOut, Icon::Fit, Icon::RotateLeft, Icon::RotateRight,
            Icon::Rectangle, Icon::Highlight, Icon::Scan, Icon::Crop, Icon::Previous, Icon::Next, Icon::Slide,
        ];
        for icon in all {
            let marks = marks(icon);
            assert!(!marks.is_empty(), "{icon:?}");
            for mark in marks {
                let points = match mark {
                    Mark::Line(p) | Mark::Closed(p) => p,
                    Mark::Dot(x, y, r) | Mark::Ring(x, y, r) => vec![(x - r, y - r), (x + r, y + r)],
                };
                for (x, y) in points {
                    assert!((1.0..=23.0).contains(&x) && (1.0..=23.0).contains(&y), "{icon:?} at {x},{y}");
                }
            }
        }
    }

    #[test]
    fn toolbar_titles_map_to_icons_and_unknown_titles_stay_text() {
        assert_eq!(icon_for("Bold"), Some(Icon::Bold));
        assert_eq!(icon_for("Insert table…"), Some(Icon::Table));
        assert_eq!(icon_for("&Undo"), Some(Icon::Undo));
        assert_eq!(icon_for("Rotate &Right"), Some(Icon::RotateRight));
        assert_eq!(icon_for("Font…"), None);
        assert_eq!(icon_for("Number format…"), None);
    }
}
