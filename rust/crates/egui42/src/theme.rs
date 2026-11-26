//! Colours, fonts and widget style shared by every off42 window on Linux.
//!
//! The values follow the suite's web design: flat chrome on a neutral background, one rounded
//! toolbar strip, white paper with a soft shadow, thin separators and one accent per app.
use eframe::egui::{
    self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Margin, Shadow,
    Stroke, TextStyle, Theme, Vec2,
};
use std::sync::Arc;

pub const SEMIBOLD: &str = "semibold";

/// Identity of one app: name shown on the home screen and its accent colours.
#[derive(Clone, Copy, Debug)]
pub struct AppInfo {
    pub exe: &'static str,
    pub title: &'static str,
    pub accent: Color32,
    /// Light tint used for the primary button and the selected item in the app drawer.
    pub pill: Color32,
    pub pill_text: Color32,
    pub logo: crate::icons::Logo,
}

pub const APPS: [AppInfo; 6] = [
    AppInfo {
        exe: "DOC42",
        title: "Documents",
        accent: Color32::from_rgb(0x2f, 0x6f, 0xde),
        pill: Color32::from_rgb(0xc9, 0xdc, 0xfb),
        pill_text: Color32::from_rgb(0x0b, 0x2f, 0x6b),
        logo: crate::icons::Logo::Document,
    },
    AppInfo {
        exe: "XLS42",
        title: "Spreadsheets",
        accent: Color32::from_rgb(0x1e, 0x8e, 0x4f),
        pill: Color32::from_rgb(0xc4, 0xeb, 0xd3),
        pill_text: Color32::from_rgb(0x0b, 0x3d, 0x20),
        logo: crate::icons::Logo::Sheet,
    },
    AppInfo {
        exe: "PPT42",
        title: "Presentations",
        accent: Color32::from_rgb(0xe0, 0x89, 0x1b),
        pill: Color32::from_rgb(0xfb, 0xdc, 0xb3),
        pill_text: Color32::from_rgb(0x4a, 0x2a, 0x00),
        logo: crate::icons::Logo::Slides,
    },
    AppInfo {
        exe: "PDF42",
        title: "PDF",
        accent: Color32::from_rgb(0xd2, 0x3f, 0x31),
        pill: Color32::from_rgb(0xf8, 0xd3, 0xcf),
        pill_text: Color32::from_rgb(0x5c, 0x13, 0x0c),
        logo: crate::icons::Logo::Pdf,
    },
    AppInfo {
        exe: "IMG42",
        title: "Images",
        accent: Color32::from_rgb(0x7b, 0x4f, 0xc4),
        pill: Color32::from_rgb(0xe3, 0xd7, 0xf5),
        pill_text: Color32::from_rgb(0x2e, 0x17, 0x58),
        logo: crate::icons::Logo::Image,
    },
    AppInfo {
        exe: "SCN42",
        title: "Scans",
        accent: Color32::from_rgb(0x0b, 0x8a, 0x8a),
        pill: Color32::from_rgb(0xc2, 0xe8, 0xe8),
        pill_text: Color32::from_rgb(0x05, 0x3c, 0x3c),
        logo: crate::icons::Logo::Scan,
    },
];

pub fn app_info(name: &str) -> AppInfo {
    APPS.iter()
        .copied()
        .find(|a| a.exe.eq_ignore_ascii_case(name))
        .unwrap_or(APPS[0])
}

/// Semantic colours for one theme. Paper (pages, slides, pictures) stays white in both.
#[derive(Clone, Copy, Debug)]
pub struct Palette {
    pub dark: bool,
    pub bg: Color32,
    pub surface: Color32,
    pub toolbar: Color32,
    pub hover: Color32,
    pub pressed: Color32,
    pub menu_hover: Color32,
    pub text: Color32,
    pub muted: Color32,
    pub icon: Color32,
    pub line: Color32,
    pub separator: Color32,
    pub select: Color32,
    pub select_soft: Color32,
    pub toggled: Color32,
    pub toggled_text: Color32,
    pub snack: Color32,
    pub snack_text: Color32,
    pub snack_action: Color32,
    pub grid_head: Color32,
    pub grid_line: Color32,
    pub grid_cell: Color32,
    pub paper: Color32,
    pub paper_text: Color32,
    pub error: Color32,
}

pub const LIGHT: Palette = Palette {
    dark: false,
    bg: Color32::from_rgb(0xf8, 0xf9, 0xfb),
    surface: Color32::WHITE,
    toolbar: Color32::from_rgb(0xed, 0xf1, 0xf7),
    hover: Color32::from_rgb(0xdd, 0xe2, 0xea),
    pressed: Color32::from_rgb(0xd0, 0xd7, 0xe2),
    menu_hover: Color32::from_rgb(0xf0, 0xf2, 0xf5),
    text: Color32::from_rgb(0x1f, 0x1f, 0x1f),
    muted: Color32::from_rgb(0x5b, 0x5f, 0x66),
    icon: Color32::from_rgb(0x44, 0x47, 0x46),
    line: Color32::from_rgb(0xe1, 0xe3, 0xe6),
    separator: Color32::from_rgb(0xc7, 0xcc, 0xd4),
    select: Color32::from_rgb(0x1a, 0x73, 0xe8),
    select_soft: Color32::from_rgb(0xd3, 0xe3, 0xfd),
    toggled: Color32::from_rgb(0xd3, 0xe2, 0xfd),
    toggled_text: Color32::from_rgb(0x0b, 0x2f, 0x6b),
    snack: Color32::from_rgb(0x30, 0x33, 0x3a),
    snack_text: Color32::from_rgb(0xf1, 0xf3, 0xf4),
    snack_action: Color32::from_rgb(0xa8, 0xc7, 0xfa),
    grid_head: Color32::from_rgb(0xf8, 0xf9, 0xfa),
    grid_line: Color32::from_rgb(0xe1, 0xe3, 0xe6),
    grid_cell: Color32::WHITE,
    paper: Color32::WHITE,
    paper_text: Color32::from_rgb(0x1f, 0x1f, 0x1f),
    error: Color32::from_rgb(0xb3, 0x26, 0x1e),
};

pub const DARK: Palette = Palette {
    dark: true,
    bg: Color32::from_rgb(0x1b, 0x1c, 0x1f),
    surface: Color32::from_rgb(0x27, 0x29, 0x2d),
    toolbar: Color32::from_rgb(0x2b, 0x2f, 0x36),
    hover: Color32::from_rgb(0x3a, 0x3f, 0x48),
    pressed: Color32::from_rgb(0x45, 0x4b, 0x55),
    menu_hover: Color32::from_rgb(0x34, 0x38, 0x3f),
    text: Color32::from_rgb(0xe3, 0xe3, 0xe3),
    muted: Color32::from_rgb(0xa8, 0xad, 0xb4),
    icon: Color32::from_rgb(0xc4, 0xc7, 0xc5),
    line: Color32::from_rgb(0x3c, 0x40, 0x43),
    separator: Color32::from_rgb(0x5f, 0x63, 0x68),
    select: Color32::from_rgb(0x8a, 0xb4, 0xf8),
    select_soft: Color32::from_rgb(0x39, 0x44, 0x57),
    toggled: Color32::from_rgb(0x3b, 0x4a, 0x63),
    toggled_text: Color32::from_rgb(0xd3, 0xe3, 0xfd),
    snack: Color32::from_rgb(0xe3, 0xe3, 0xe3),
    snack_text: Color32::from_rgb(0x1f, 0x1f, 0x1f),
    snack_action: Color32::from_rgb(0x0b, 0x57, 0xd0),
    grid_head: Color32::from_rgb(0x23, 0x25, 0x29),
    grid_line: Color32::from_rgb(0x3c, 0x40, 0x43),
    grid_cell: Color32::from_rgb(0x1f, 0x20, 0x23),
    paper: Color32::WHITE,
    paper_text: Color32::from_rgb(0x1f, 0x1f, 0x1f),
    error: Color32::from_rgb(0xf2, 0xb8, 0xb5),
};

pub fn palette(ctx: &egui::Context) -> Palette {
    if ctx.global_style().visuals.dark_mode {
        DARK
    } else {
        LIGHT
    }
}

/// Primary button colours: the light tint in light mode, a deep tint in dark mode.
pub fn pill(info: &AppInfo, p: &Palette) -> (Color32, Color32) {
    if p.dark {
        (mix(info.accent, p.bg, 0.55), mix(info.accent, Color32::WHITE, 0.8))
    } else {
        (info.pill, info.pill_text)
    }
}

pub fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let l = |x: u8, y: u8| (f32::from(x) * (1.0 - t) + f32::from(y) * t).round() as u8;
    Color32::from_rgb(l(a.r(), b.r()), l(a.g(), b.g()), l(a.b(), b.b()))
}

pub fn semibold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(SEMIBOLD.into()))
}

pub fn shadow(dark: bool) -> Shadow {
    Shadow {
        offset: [0, 4],
        blur: 18,
        spread: 0,
        color: Color32::from_black_alpha(if dark { 110 } else { 40 }),
    }
}

/// The soft shadow under paper: pages, slides and pictures.
pub fn paper_shadow(dark: bool) -> Shadow {
    Shadow {
        offset: [0, 1],
        blur: 6,
        spread: 0,
        color: Color32::from_black_alpha(if dark { 120 } else { 38 }),
    }
}

/// Inter for the interface, with egui's fonts and any installed CJK/Arabic/Thai font as fallbacks.
pub fn fonts() -> FontDefinitions {
    let mut fonts = FontDefinitions::default();
    fonts.font_data.insert(
        "Inter".into(),
        Arc::new(FontData::from_static(include_bytes!("../fonts/Inter-Regular.ttf"))),
    );
    fonts.font_data.insert(
        "Inter-SemiBold".into(),
        Arc::new(FontData::from_static(include_bytes!("../fonts/Inter-SemiBold.ttf"))),
    );
    let fallbacks = system_fallbacks(&mut fonts);
    let proportional = fonts.families.entry(FontFamily::Proportional).or_default();
    proportional.insert(0, "Inter".into());
    proportional.extend(fallbacks.iter().cloned());
    let mut bold = vec!["Inter-SemiBold".to_string()];
    bold.extend(fonts.families[&FontFamily::Proportional].iter().skip(1).cloned());
    fonts.families.insert(FontFamily::Name(SEMIBOLD.into()), bold);
    let monospace = fonts.families.entry(FontFamily::Monospace).or_default();
    monospace.extend(fallbacks);
    fonts
}

/// Language names and translated menus need scripts Inter lacks; use installed Noto fonts when present.
fn system_fallbacks(fonts: &mut FontDefinitions) -> Vec<String> {
    const CANDIDATES: [&str; 10] = [
        "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/google-noto-cjk/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/truetype/droid/DroidSansFallbackFull.ttf",
        "/usr/share/fonts/truetype/noto/NotoSansArabic-Regular.ttf",
        "/usr/share/fonts/noto/NotoSansArabic-Regular.ttf",
        "/usr/share/fonts/truetype/noto/NotoSansThai-Regular.ttf",
        "/usr/share/fonts/noto/NotoSansThai-Regular.ttf",
        "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
        "/usr/share/fonts/TTF/DejaVuSans.ttf",
    ];
    let mut names = Vec::new();
    for path in CANDIDATES {
        let Ok(bytes) = std::fs::read(path) else { continue };
        let name = format!("system:{path}");
        fonts.font_data.insert(name.clone(), Arc::new(FontData::from_owned(bytes)));
        names.push(name);
    }
    names
}

fn style_for(p: &Palette, base: &egui::Style) -> egui::Style {
    let mut style = base.clone();
    style.text_styles = [
        (TextStyle::Small, FontId::proportional(12.0)),
        (TextStyle::Body, FontId::proportional(14.0)),
        (TextStyle::Button, FontId::proportional(14.0)),
        (TextStyle::Heading, semibold(18.0)),
        (TextStyle::Monospace, FontId::monospace(13.0)),
    ]
    .into();
    let s = &mut style.spacing;
    s.item_spacing = Vec2::new(8.0, 6.0);
    s.button_padding = Vec2::new(8.0, 4.0);
    s.interact_size = Vec2::new(28.0, 28.0);
    s.menu_margin = Margin::symmetric(0, 6);
    s.window_margin = Margin::same(20);
    s.combo_height = 320.0;
    s.menu_width = 220.0;
    s.icon_width = 14.0;
    s.icon_width_inner = 8.0;
    style.animation_time = 0.1;
    let v = &mut style.visuals;
    v.dark_mode = p.dark;
    v.override_text_color = None;
    v.panel_fill = p.bg;
    v.window_fill = p.surface;
    v.window_stroke = Stroke::new(1.0, if p.dark { p.line } else { Color32::TRANSPARENT });
    v.window_corner_radius = CornerRadius::same(12);
    v.menu_corner_radius = CornerRadius::same(8);
    v.window_shadow = shadow(p.dark);
    v.popup_shadow = shadow(p.dark);
    v.extreme_bg_color = p.surface;
    v.text_edit_bg_color = Some(p.surface);
    v.faint_bg_color = p.grid_head;
    v.code_bg_color = p.toolbar;
    v.hyperlink_color = p.select;
    v.error_fg_color = p.error;
    v.selection.bg_fill = if p.dark {
        Color32::from_rgb(0x2f, 0x4a, 0x7a)
    } else {
        Color32::from_rgb(0xc9, 0xdc, 0xfb)
    };
    v.selection.stroke = Stroke::new(1.0, p.select);
    v.text_cursor.stroke = Stroke::new(1.5, p.text);
    v.striped = false;
    v.indent_has_left_vline = false;
    let radius = CornerRadius::same(6);
    let w = &mut v.widgets;
    w.noninteractive.bg_fill = p.surface;
    w.noninteractive.weak_bg_fill = p.surface;
    w.noninteractive.bg_stroke = Stroke::new(1.0, p.line);
    w.noninteractive.fg_stroke = Stroke::new(1.0, p.text);
    w.noninteractive.corner_radius = radius;
    w.inactive.bg_fill = p.toolbar;
    w.inactive.weak_bg_fill = p.toolbar;
    w.inactive.bg_stroke = Stroke::new(1.0, p.separator);
    w.inactive.fg_stroke = Stroke::new(1.0, p.text);
    w.inactive.corner_radius = radius;
    w.inactive.expansion = 0.0;
    w.hovered.bg_fill = p.hover;
    w.hovered.weak_bg_fill = p.hover;
    w.hovered.bg_stroke = Stroke::new(1.0, p.separator);
    w.hovered.fg_stroke = Stroke::new(1.0, p.text);
    w.hovered.corner_radius = radius;
    w.hovered.expansion = 0.0;
    w.active.bg_fill = p.pressed;
    w.active.weak_bg_fill = p.pressed;
    w.active.bg_stroke = Stroke::new(1.0, p.select);
    w.active.fg_stroke = Stroke::new(1.0, p.text);
    w.active.corner_radius = radius;
    w.active.expansion = 0.0;
    w.open.bg_fill = p.hover;
    w.open.weak_bg_fill = p.hover;
    w.open.bg_stroke = Stroke::NONE;
    w.open.fg_stroke = Stroke::new(1.0, p.text);
    w.open.corner_radius = radius;
    style
}

pub fn install(ctx: &egui::Context) {
    ctx.set_fonts(fonts());
    let light = style_for(&LIGHT, &ctx.style_of(Theme::Light));
    let dark = style_for(&DARK, &ctx.style_of(Theme::Dark));
    ctx.set_style_of(Theme::Light, light);
    ctx.set_style_of(Theme::Dark, dark);
}

/// Make widgets inside `ui` flat: no fill or outline until hovered (toolbars, title bars, lists).
pub fn flat(ui: &mut egui::Ui) {
    let w = &mut ui.visuals_mut().widgets;
    w.inactive.weak_bg_fill = Color32::TRANSPARENT;
    w.inactive.bg_fill = Color32::TRANSPARENT;
    w.inactive.bg_stroke = Stroke::NONE;
    w.hovered.bg_stroke = Stroke::NONE;
    w.active.bg_stroke = Stroke::NONE;
    w.open.bg_stroke = Stroke::NONE;
}
