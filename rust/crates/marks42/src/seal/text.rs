//! Text for stamps (Kit42 `InkDraw.glyphs`, `width`, `fit`, `covers` and `CTFontGetCapHeight`).
//!
//! macOS asks CoreText for a font by PostScript name and falls back per character to fonts that have the missing
//! characters. Here fonts come from the system font database (fontdb): the macOS name first, then close Windows
//! and Linux equivalents (Helvetica Neue → Arial / Segoe UI, Condensed Black → Impact, Songti → SimSun, Hiragino
//! Mincho → Yu Mincho / MS Mincho, Apple Myungjo → Batang, …), and finally the Noto Sans font bundled with the
//! crate, so every host draws Latin, Cyrillic and Greek. Text is shaped with rustybuzz (kerning, ligatures,
//! Arabic joining) and laid out with a small bidi pass so Hebrew and Arabic read right to left with numbers
//! left to right, as CoreText does.
//!
//! Diagnostics: `MARKS42_NO_SYSTEM_FONTS=1` uses only the bundled font; `MARKS42_WINDOWS_FONTS=1` ignores the
//! macOS-only faces, so a Mac previews the substitutes a Windows PC would use (where it has them installed).

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use tiny_skia::{Path, PathBuilder};

static BUNDLED: &[u8] = include_bytes!("../../../doc42-layout-probe/fixtures/NotoSans.ttf");

/// A parsed font face kept for the life of the process.
pub(crate) struct Face {
    face: rustybuzz::Face<'static>,
    upem: f64,
    cap_height: f64,
    /// Apple fonts with an AAT `kerx` table that the shaper skips but CoreText kerns with the legacy `kern` table
    /// (Helvetica Neue Bold and Condensed): pair kerning is applied from `kern` after shaping.
    legacy_kern: bool,
    /// PostScript name (diagnostics).
    #[allow(dead_code)]
    pub(crate) name: String,
}

impl Face {
    fn load(data: &'static [u8], index: u32, name: String) -> Option<&'static Face> {
        let face = rustybuzz::Face::from_slice(data, index)?;
        let upem = face.units_per_em() as f64;
        // CoreText measures fonts without an OS/2 cap height from the glyphs: halfway between the flat top of "H"
        // and the overshoot of "O" (Helvetica Neue Condensed: (714 + 728) / 2 = 721).
        let top = |c: char| face.glyph_index(c).and_then(|g| face.glyph_bounding_box(g)).map(|b| b.y_max as f64);
        let cap_height = match face.capital_height() {
            Some(c) if c > 0 => c as f64,
            _ => match (top('H'), top('O')) {
                (Some(h), Some(o)) => (h + o) / 2.0,
                (Some(h), None) => h,
                _ => face.ascender() as f64 * 0.7,
            },
        };
        let tables = face.tables();
        let legacy_kern = tables.kerx.is_some() && tables.gpos.is_none() && tables.kern.is_some();
        Some(Box::leak(Box::new(Face { face, upem, cap_height, legacy_kern, name })))
    }

    /// Horizontal pair kerning from the legacy `kern` table, in font units.
    fn kern(&self, left: u16, right: u16) -> i32 {
        let Some(kern) = self.face.tables().kern else { return 0 };
        kern.subtables
            .into_iter()
            .filter(|s| s.horizontal && !s.variable && !s.has_cross_stream)
            .filter_map(|s| s.glyphs_kerning(ttf_parser::GlyphId(left), ttf_parser::GlyphId(right)))
            .map(i32::from)
            .sum()
    }

    fn has(&self, c: char) -> bool {
        self.face.glyph_index(c).is_some_and(|g| g.0 != 0)
    }

    fn same(&self, other: &Face) -> bool {
        std::ptr::eq(self, other)
    }
}

struct Store {
    db: fontdb::Database,
    bundled: &'static Face,
    by_name: Mutex<HashMap<String, Option<&'static Face>>>,
    resolved: Mutex<HashMap<String, &'static Face>>,
    fallback: Mutex<HashMap<char, Option<&'static Face>>>,
    /// Diagnostics (`MARKS42_WINDOWS_FONTS`): ignore macOS-only faces, to preview the Windows substitutes on a Mac.
    windows_only: bool,
}

/// Faces that ship with macOS only.
const MAC_ONLY: &[&str] = &[
    "HelveticaNeue",
    "Helvetica",
    "STSong",
    "STHeiti",
    "Hiragino",
    "HiraMin",
    "HiraKaku",
    "AppleMyungjo",
    "AppleSDGothicNeo",
    "AppleGothic",
    "GeezaPro",
    "ArialHebrew",
    "PingFang",
    "AppleSymbols",
    "Times-",
    "Courier-",
    "Songti",
];

fn store() -> &'static Store {
    static STORE: OnceLock<Store> = OnceLock::new();
    STORE.get_or_init(|| {
        let mut db = fontdb::Database::new();
        if std::env::var_os("MARKS42_NO_SYSTEM_FONTS").is_none() {
            db.load_system_fonts();
        }
        let bundled = Face::load(BUNDLED, 0, "NotoSans (bundled)".into()).expect("bundled Noto Sans parses");
        Store {
            db,
            bundled,
            by_name: Mutex::new(HashMap::new()),
            resolved: Mutex::new(HashMap::new()),
            fallback: Mutex::new(HashMap::new()),
            windows_only: std::env::var_os("MARKS42_WINDOWS_FONTS").is_some(),
        }
    })
}

/// A system face by PostScript name (case-insensitive), loaded once.
fn system_face(ps: &str) -> Option<&'static Face> {
    let s = store();
    if s.windows_only && MAC_ONLY.iter().any(|m| ps.starts_with(m)) {
        return None;
    }
    if let Some(found) = s.by_name.lock().unwrap().get(ps) {
        return *found;
    }
    let face =
        s.db.faces().find(|f| f.post_script_name.eq_ignore_ascii_case(ps)).and_then(|info| load_face(&s.db, info.id));
    s.by_name.lock().unwrap().insert(ps.to_string(), face);
    face
}

fn load_face(db: &fontdb::Database, id: fontdb::ID) -> Option<&'static Face> {
    let (source, index) = db.face_source(id)?;
    let name = db.face(id).map(|f| f.post_script_name.clone()).unwrap_or_default();
    let data: &'static [u8] = match source {
        fontdb::Source::Binary(arc) => {
            let leaked: &'static std::sync::Arc<dyn AsRef<[u8]> + Sync + Send> = Box::leak(Box::new(arc));
            (**leaked).as_ref()
        }
        fontdb::Source::File(path) => Box::leak(std::fs::read(path).ok()?.into_boxed_slice()),
        fontdb::Source::SharedFile(_, arc) => {
            let leaked: &'static std::sync::Arc<dyn AsRef<[u8]> + Sync + Send> = Box::leak(Box::new(arc));
            (**leaked).as_ref()
        }
    };
    Face::load(data, index, name)
}

/// The macOS font names used by the stamp catalogue and close equivalents on other systems, in order of preference.
fn candidates(name: &str) -> &'static [&'static str] {
    match name {
        "HelveticaNeue-CondensedBold" => &[
            "ArialNarrow-Bold",
            "Arial-BoldMT",
            "SegoeUI-Bold",
            "LiberationSansNarrow-Bold",
            "LiberationSans-Bold",
            "DejaVuSansCondensed-Bold",
        ],
        "HelveticaNeue-CondensedBlack" => {
            &["Impact", "ArialNarrow-Bold", "Arial-BoldMT", "SegoeUI-Black", "LiberationSans-Bold"]
        }
        "HelveticaNeue-Bold" => {
            &["Helvetica-Bold", "Arial-BoldMT", "SegoeUI-Bold", "LiberationSans-Bold", "DejaVuSans-Bold"]
        }
        "HelveticaNeue-Medium" => {
            &["Helvetica", "ArialMT", "SegoeUI-Semibold", "SegoeUI", "LiberationSans", "DejaVuSans"]
        }
        "TimesNewRomanPS-BoldMT" => &["Times-Bold", "LiberationSerif-Bold", "DejaVuSerif-Bold", "Georgia-Bold"],
        "Courier-Bold" => &["CourierNewPS-BoldMT", "LiberationMono-Bold", "DejaVuSansMono-Bold"],
        "STSongti-SC-Black" => &[
            "STSongti-SC-Bold",
            "SimSun",
            "NSimSun",
            "STSong",
            "MicrosoftYaHei-Bold",
            "MicrosoftYaHei",
            "NotoSerifCJKsc-Black",
            "NotoSerifCJKsc-Bold",
            "NotoSansCJKsc-Bold",
        ],
        "STSongti-TC-Bold" => &[
            "STSongti-SC-Bold",
            "PMingLiU",
            "MingLiU",
            "SimSun",
            "MicrosoftJhengHeiBold",
            "MicrosoftJhengHeiRegular",
            "NotoSerifCJKtc-Bold",
            "NotoSansCJKtc-Bold",
        ],
        "HiraMinProN-W6" => &[
            "HiraMinProN-W3",
            "YuMincho-Demibold",
            "YuMincho-Regular",
            "MS-Mincho",
            "MS-PMincho",
            "YuGothic-Bold",
            "NotoSerifCJKjp-Bold",
            "NotoSansCJKjp-Bold",
        ],
        "AppleMyungjo" => {
            &["Batang", "BatangChe", "Gungsuh", "MalgunGothic", "NotoSerifCJKkr-Regular", "NotoSansCJKkr-Regular"]
        }
        "AppleSDGothicNeo-Bold" => &["MalgunGothicBold", "MalgunGothic-Bold", "MalgunGothic", "NotoSansCJKkr-Bold"],
        "GeezaPro-Bold" => &["Arial-BoldMT", "SegoeUI-Bold", "Tahoma-Bold", "NotoSansArabic-Bold", "DejaVuSans-Bold"],
        "ArialHebrew-Bold" => {
            &["Arial-BoldMT", "SegoeUI-Bold", "Tahoma-Bold", "NotoSansHebrew-Bold", "DejaVuSans-Bold"]
        }
        _ => &[],
    }
}

/// Fonts tried for characters the chosen font lacks (CoreText's cascade list, roughly).
const CASCADE: &[&str] = &[
    "HelveticaNeue",
    "Helvetica",
    "Arial-BoldMT",
    "ArialMT",
    "SegoeUI",
    "PingFangSC-Regular",
    "HiraginoSans-W3",
    "AppleSDGothicNeo-Regular",
    "GeezaPro",
    "ArialHebrew",
    "AppleSymbols",
    "MicrosoftYaHei",
    "YuGothic-Regular",
    "MalgunGothic",
    "SimSun",
    "Tahoma",
    "SegoeUISymbol",
    "ArialUnicodeMS",
];

/// The face for a font name: the macOS font, a close substitute, a generic sans or the bundled Noto Sans.
pub(crate) fn resolve(name: &str) -> &'static Face {
    let s = store();
    if let Some(face) = s.resolved.lock().unwrap().get(name) {
        return face;
    }
    let face = std::iter::once(name)
        .chain(candidates(name).iter().copied())
        .chain(["HelveticaNeue", "Helvetica", "ArialMT", "SegoeUI", "LiberationSans", "DejaVuSans"])
        .find_map(system_face)
        .unwrap_or(s.bundled);
    s.resolved.lock().unwrap().insert(name.to_string(), face);
    face
}

/// A face that has the character, for font fallback.
fn fallback(c: char) -> Option<&'static Face> {
    let s = store();
    if let Some(found) = s.fallback.lock().unwrap().get(&c) {
        return *found;
    }
    let face = CASCADE
        .iter()
        .filter_map(|n| system_face(n))
        .find(|f| f.has(c))
        .or_else(|| s.bundled.has(c).then_some(s.bundled))
        .or_else(|| {
            // Any installed face with the character (slow path, cached per character).
            let mut ids: Vec<_> = s.db.faces().map(|f| (f.post_script_name.clone(), f.id)).collect();
            ids.sort();
            let id = ids.into_iter().map(|(_, id)| id).find(|&id| {
                s.db.with_face_data(id, |data, index| {
                    ttf_parser::Face::parse(data, index).ok().and_then(|f| f.glyph_index(c)).is_some_and(|g| g.0 != 0)
                })
                .unwrap_or(false)
            })?;
            let ps = s.db.face(id)?.post_script_name.clone();
            system_face(&ps)
        });
    s.fallback.lock().unwrap().insert(c, face);
    face
}

/// A font at a size (CTFont).
#[derive(Clone, Copy)]
pub(crate) struct Font {
    pub(crate) face: &'static Face,
    pub(crate) size: f64,
}

/// One laid-out glyph: its outline at the line origin (y up, in points at the font size), where it was placed
/// along the line, and its advance (Kit42 `LaidGlyph`).
pub(crate) struct Glyph {
    pub(crate) path: Option<Path>,
    pub(crate) x: f64,
    #[allow(dead_code)]
    pub(crate) y: f64,
    pub(crate) advance: f64,
}

/// CTFontCreateWithName.
pub(crate) fn font(name: &str, size: f64) -> Font {
    Font { face: resolve(name), size }
}

impl Font {
    pub(crate) fn with_size(self, size: f64) -> Font {
        Font { size, ..self }
    }

    /// CTFontGetCapHeight.
    pub(crate) fn cap_height(&self) -> f64 {
        self.face.cap_height * self.size / self.face.upem
    }

    /// Glyph outlines of a line of text, shaped, with font fallback, in visual order (InkDraw.glyphs).
    pub(crate) fn glyphs(&self, text: &str) -> Vec<Glyph> {
        let mut out = Vec::new();
        let mut pen = 0.0;
        for run in runs(text, self.face) {
            let scale = self.size / run.face.upem;
            for (glyph, advance, dx, dy) in shape(&run) {
                out.push(Glyph {
                    path: outline(run.face, glyph, scale),
                    x: pen + dx * scale,
                    y: dy * scale,
                    advance: advance * scale,
                });
                pen += advance * scale;
            }
        }
        out
    }

    /// Typographic width of a line (InkDraw.width).
    pub(crate) fn width(&self, text: &str) -> f64 {
        runs(text, self.face)
            .iter()
            .map(|run| shape(run).iter().map(|g| g.1).sum::<f64>() * self.size / run.face.upem)
            .sum()
    }

    /// Whether the font itself has every character, without fallback (InkDraw.covers).
    #[allow(dead_code)]
    pub(crate) fn covers(&self, text: &str) -> bool {
        text.chars().filter(|c| !c.is_whitespace() && *c != '.' && *c != '-').all(|c| self.face.has(c))
    }
}

/// Largest font size (≤ max_size, ≥ 6) at which the text fits the width (InkDraw.fit).
pub(crate) fn fit(name: &str, text: &str, width: f64, max_size: f64) -> Font {
    let f = font(name, max_size);
    let w = f.width(text);
    if w > width && w > 0.0 {
        return f.with_size((max_size * width / w).max(6.0));
    }
    f
}

/// Shapes one run: (glyph, advance, x offset, y offset) in font units, in visual order.
fn shape(run: &Run) -> Vec<(u16, f64, f64, f64)> {
    let mut buffer = rustybuzz::UnicodeBuffer::new();
    buffer.push_str(&run.text);
    buffer.guess_segment_properties();
    buffer.set_direction(if run.rtl { rustybuzz::Direction::RightToLeft } else { rustybuzz::Direction::LeftToRight });
    let shaped = rustybuzz::shape(&run.face.face, &[], buffer);
    let mut out: Vec<(u16, f64, f64, f64)> = shaped
        .glyph_infos()
        .iter()
        .zip(shaped.glyph_positions())
        .map(|(info, pos)| (info.glyph_id as u16, pos.x_advance as f64, pos.x_offset as f64, pos.y_offset as f64))
        .collect();
    if run.face.legacy_kern {
        for i in 1..out.len() {
            out[i - 1].1 += run.face.kern(out[i - 1].0, out[i].0) as f64;
        }
    }
    out
}

struct Outline {
    b: PathBuilder,
    s: f32,
}

impl ttf_parser::OutlineBuilder for Outline {
    fn move_to(&mut self, x: f32, y: f32) {
        self.b.move_to(x * self.s, y * self.s)
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.b.line_to(x * self.s, y * self.s)
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        self.b.quad_to(x1 * self.s, y1 * self.s, x * self.s, y * self.s)
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        self.b.cubic_to(x1 * self.s, y1 * self.s, x2 * self.s, y2 * self.s, x * self.s, y * self.s)
    }
    fn close(&mut self) {
        self.b.close()
    }
}

fn outline(face: &Face, glyph: u16, scale: f64) -> Option<Path> {
    let mut o = Outline { b: PathBuilder::new(), s: scale as f32 };
    face.face.outline_glyph(ttf_parser::GlyphId(glyph), &mut o)?;
    o.b.finish()
}

// MARK: - Runs: font fallback and bidirectional order

struct Run {
    text: String,
    face: &'static Face,
    rtl: bool,
}

#[derive(Clone, Copy, PartialEq)]
enum Class {
    L,
    R,
    En,
    N,
}

fn is_mark(c: char) -> bool {
    matches!(c as u32,
        0x0300..=0x036F | 0x0483..=0x0489 | 0x0591..=0x05BD | 0x05BF | 0x05C1..=0x05C2 | 0x05C4..=0x05C5 | 0x05C7
        | 0x0610..=0x061A | 0x064B..=0x065F | 0x0670 | 0x06D6..=0x06DC | 0x06DF..=0x06E4 | 0x06E7..=0x06E8
        | 0x06EA..=0x06ED | 0x1AB0..=0x1AFF | 0x1DC0..=0x1DFF | 0x200C..=0x200D | 0x20D0..=0x20FF | 0x3099..=0x309A
        | 0xFE00..=0xFE0F | 0xFE20..=0xFE2F)
}

fn is_rtl(c: char) -> bool {
    matches!(c as u32, 0x0590..=0x08FF | 0xFB1D..=0xFDFF | 0xFE70..=0xFEFF)
}

fn class(c: char) -> Class {
    if is_rtl(c) && !c.is_numeric() {
        Class::R
    } else if c.is_numeric() {
        Class::En
    } else if c.is_alphabetic() {
        Class::L
    } else {
        Class::N
    }
}

/// User-perceived characters: a base character with its combining marks (Swift `Character`, near enough).
pub(crate) fn clusters(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut joiner = false;
    for c in text.chars() {
        match out.last_mut() {
            Some(last) if is_mark(c) || joiner => last.push(c),
            _ => out.push(c.to_string()),
        }
        joiner = c == '\u{200D}';
    }
    out
}

/// Splits a line into runs of one font and one direction, in visual order.
fn runs(text: &str, primary: &'static Face) -> Vec<Run> {
    let clusters = clusters(text);
    if clusters.is_empty() {
        return Vec::new();
    }
    let classes: Vec<Class> = clusters.iter().map(|c| class(c.chars().next().unwrap())).collect();
    let rtl_paragraph = classes.iter().find(|c| matches!(c, Class::L | Class::R)) == Some(&Class::R);
    let base = if rtl_paragraph { 1u8 } else { 0 };
    // Resolve numbers and neutrals (UAX #9, simplified: W7, N1, N2, I1, I2).
    let n = classes.len();
    let mut levels = vec![base; n];
    let mut prev_strong = if rtl_paragraph { Class::R } else { Class::L };
    let mut resolved = classes.clone();
    for i in 0..n {
        match classes[i] {
            Class::L | Class::R => prev_strong = classes[i],
            Class::En if prev_strong == Class::L => resolved[i] = Class::L,
            _ => {}
        }
    }
    let direction = |c: Class| match c {
        Class::L => Some(false),
        Class::R | Class::En => Some(true),
        Class::N => None,
    };
    let mut i = 0;
    while i < n {
        if resolved[i] != Class::N {
            i += 1;
            continue;
        }
        let start = i;
        while i < n && resolved[i] == Class::N {
            i += 1;
        }
        let before = if start == 0 { Some(rtl_paragraph) } else { direction(resolved[start - 1]) };
        let after = if i == n { Some(rtl_paragraph) } else { direction(resolved[i]) };
        let dir = if before == after { before.unwrap_or(rtl_paragraph) } else { rtl_paragraph };
        for r in &mut resolved[start..i] {
            *r = if dir { Class::R } else { Class::L };
        }
    }
    for (k, c) in resolved.iter().enumerate() {
        levels[k] = match (c, rtl_paragraph) {
            (Class::L, false) => 0,
            (Class::R, false) => 1,
            (Class::En, false) => 2,
            (Class::L, true) => 2,
            (Class::En, true) => 2,
            (_, true) => 1,
            (Class::N, false) => 0,
        };
    }
    // Font per cluster: the primary face when it has every character, otherwise a fallback.
    let faces: Vec<&'static Face> = clusters
        .iter()
        .map(|cl| {
            let c = cl.chars().next().unwrap();
            if primary.has(c) || c.is_whitespace() || c.is_control() {
                primary
            } else {
                fallback(c).unwrap_or(primary)
            }
        })
        .collect();
    let mut runs: Vec<(u8, Run)> = Vec::new();
    for k in 0..n {
        let rtl = levels[k] % 2 == 1;
        match runs.last_mut() {
            Some((lv, run)) if *lv == levels[k] && run.face.same(faces[k]) => run.text.push_str(&clusters[k]),
            _ => runs.push((levels[k], Run { text: clusters[k].clone(), face: faces[k], rtl })),
        }
    }
    // L2: from the highest level down to the lowest odd level, reverse every sequence at that level or higher.
    let max = runs.iter().map(|r| r.0).max().unwrap_or(0);
    let mut level = max;
    while level >= 1 {
        let mut k = 0;
        while k < runs.len() {
            if runs[k].0 >= level {
                let start = k;
                while k < runs.len() && runs[k].0 >= level {
                    k += 1;
                }
                runs[start..k].reverse();
            } else {
                k += 1;
            }
        }
        level -= 1;
    }
    runs.into_iter().map(|(_, r)| r).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_font_draws_latin_and_cyrillic() {
        let f = Font { face: store().bundled, size: 100.0 };
        assert!(f.covers("ООО «РОМАШКА» Hello"));
        let w = f.width("Hello");
        assert!(w > 150.0 && w < 350.0, "{w}");
        let g = f.glyphs("Hé");
        assert_eq!(g.len(), 2);
        assert!(g[1].x > 40.0);
        assert!(f.cap_height() > 60.0 && f.cap_height() < 80.0);
    }

    #[test]
    fn hebrew_with_number_reads_right_to_left_and_number_left_to_right() {
        let face = resolve("ArialHebrew-Bold");
        let r = runs("ח.פ. 512345678", face);
        // The number comes first (leftmost), the Hebrew after it.
        assert!(r[0].text.starts_with('5') && !r[0].rtl, "{:?}", r.iter().map(|r| r.text.clone()).collect::<Vec<_>>());
        assert!(r.last().unwrap().rtl);
    }

    #[test]
    fn fit_shrinks_to_width_but_not_below_six_points() {
        let f = fit("HelveticaNeue-Bold", "A long company name for a stamp", 300.0, 90.0);
        assert!(f.size < 90.0 && (f.width("A long company name for a stamp") - 300.0).abs() < 2.0);
        assert_eq!(fit("HelveticaNeue-Bold", &"W".repeat(400), 100.0, 90.0).size, 6.0);
        assert_eq!(fit("HelveticaNeue-Bold", "OK", 300.0, 90.0).size, 90.0);
    }
}
#[cfg(test)]
mod coretext_metrics {
    /// Widths and cap heights printed by CoreText on macOS (CTLineGetTypographicBounds, CTFontGetCapHeight at
    /// 100 pt). Checked only where the Helvetica Neue faces are installed: they pin the legacy `kern` pair
    /// kerning and the cap height of fonts without an OS/2 value.
    #[test]
    fn helvetica_neue_matches_coretext() {
        if super::resolve("HelveticaNeue-Bold").name != "HelveticaNeue-Bold" {
            return;
        }
        for (n, s, width, cap) in [
            ("HelveticaNeue-Bold", "EJEMPLO S.A. DE C.V.", 1070.8, 71.4),
            ("HelveticaNeue-Bold", "Example Company, Inc.", 1116.3, 71.4),
            ("HelveticaNeue-Bold", "AV", 126.9, 71.4),
            ("HelveticaNeue-Medium", "Av. Reforma 1, Col. Juárez, CDMX", 1569.2, 71.4),
            ("HelveticaNeue-CondensedBlack", "PAID", 189.0, 72.1),
            ("HelveticaNeue-CondensedBlack", "ONAYLANDI", 459.5, 72.1),
        ] {
            let f = super::font(n, 100.0);
            assert!((f.width(s) - width).abs() < 0.05, "{n} {s}: {}", f.width(s));
            assert!((f.cap_height() - cap).abs() < 0.05, "{n}: {}", f.cap_height());
        }
    }
}
