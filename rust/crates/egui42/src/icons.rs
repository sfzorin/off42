//! Thin-stroke interface icons. Each icon is a 24×24 SVG drawn in white, rasterised once with
//! resvg when the window starts, and tinted with the current text or accent colour when painted.
use eframe::egui::{self, Color32, Pos2, Rect, Shape, Stroke, TextureHandle, Vec2};
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Icon {
    Menu,
    Search,
    Globe,
    Help,
    Plus,
    Minus,
    Upload,
    Download,
    Folder,
    NewFile,
    Undo,
    Redo,
    ChevronDown,
    ChevronRight,
    Bold,
    Italic,
    Underline,
    TextColor,
    AlignLeft,
    AlignCenter,
    AlignRight,
    Justify,
    Bullets,
    Numbers,
    Table,
    Image,
    ZoomIn,
    ZoomOut,
    Close,
    Check,
    Info,
    Lock,
    More,
    Outline,
    List,
    Rotate,
    Trash,
    Copy,
    Scan,
    Text,
    Comment,
    Slide,
    Shape,
    Play,
    Sort,
    Filter,
    Function,
    Merge,
    Link,
    Page,
}

fn svg(icon: Icon) -> &'static str {
    use Icon::*;
    match icon {
        Menu => r#"<path d="M4 7h16M4 12h16M4 17h16"/>"#,
        Search => r#"<circle cx="11" cy="11" r="7"/><path d="m20 20-3.5-3.5"/>"#,
        Globe => r#"<circle cx="12" cy="12" r="9"/><path d="M3 12h18M12 3c2.5 2.8 3.7 5.8 3.7 9s-1.2 6.2-3.7 9c-2.5-2.8-3.7-5.8-3.7-9S9.5 5.8 12 3z"/>"#,
        Help => r#"<circle cx="12" cy="12" r="9"/><path d="M9.6 9.3a2.5 2.5 0 1 1 3.4 2.3c-.6.3-1 .8-1 1.5v.4"/><path d="M12 17h.01"/>"#,
        Plus => r#"<path d="M12 5v14M5 12h14"/>"#,
        Minus => r#"<path d="M5 12h14"/>"#,
        Upload => r#"<path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"/><path d="M12 16v-6M9.5 12.5 12 10l2.5 2.5"/>"#,
        Download => r#"<path d="M12 4v11M7 10l5 5 5-5M5 20h14"/>"#,
        Folder => r#"<path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"/>"#,
        NewFile => r#"<path d="M6 3h8l4 4v14H6zM12 11v6M9 14h6"/>"#,
        Undo => r#"<path d="M9 14 4 9l5-5"/><path d="M4 9h11a5 5 0 0 1 0 10h-3"/>"#,
        Redo => r#"<path d="m15 14 5-5-5-5"/><path d="M20 9H9a5 5 0 0 0 0 10h3"/>"#,
        ChevronDown => r#"<path d="m6 9 6 6 6-6"/>"#,
        ChevronRight => r#"<path d="m9 6 6 6-6 6"/>"#,
        Bold => r#"<path d="M7 5h6a3.5 3.5 0 0 1 0 7H7zM7 12h7a3.5 3.5 0 0 1 0 7H7z"/>"#,
        Italic => r#"<path d="M10 5h8M6 19h8M14 5l-4 14"/>"#,
        Underline => r#"<path d="M7 4v7a5 5 0 0 0 10 0V4M5 20h14"/>"#,
        TextColor => r#"<path d="m7 16 5-12 5 12M9 12h6M4 20h16"/>"#,
        AlignLeft => r#"<path d="M4 6h16M4 10h10M4 14h16M4 18h10"/>"#,
        AlignCenter => r#"<path d="M4 6h16M7 10h10M4 14h16M7 18h10"/>"#,
        AlignRight => r#"<path d="M4 6h16M10 10h10M4 14h16M10 18h10"/>"#,
        Justify => r#"<path d="M4 6h16M4 10h16M4 14h16M4 18h16"/>"#,
        Bullets => r#"<path d="M9 6h11M9 12h11M9 18h11"/><circle cx="4.5" cy="6" r="1" fill="white"/><circle cx="4.5" cy="12" r="1" fill="white"/><circle cx="4.5" cy="18" r="1" fill="white"/>"#,
        Numbers => r#"<path d="M10 6h10M10 12h10M10 18h10"/><path d="M4 5h1.5v4M4 9h3M4 15h3l-3 3.5h3" stroke-width="1.5"/>"#,
        Table => r#"<rect x="3" y="4" width="18" height="16" rx="2"/><path d="M3 10h18M3 15h18M9 4v16M15 4v16"/>"#,
        Image => r#"<rect x="3" y="4" width="18" height="16" rx="2"/><circle cx="9" cy="10" r="2"/><path d="m21 17-5-5-9 8"/>"#,
        ZoomIn => r#"<circle cx="11" cy="11" r="7"/><path d="m20 20-3.5-3.5M11 8v6M8 11h6"/>"#,
        ZoomOut => r#"<circle cx="11" cy="11" r="7"/><path d="m20 20-3.5-3.5M8 11h6"/>"#,
        Close => r#"<path d="M6 6l12 12M18 6 6 18"/>"#,
        Check => r#"<path d="m5 12 5 5 9-10"/>"#,
        Info => r#"<circle cx="12" cy="12" r="9"/><path d="M12 11v6M12 7.5h.01"/>"#,
        Lock => r#"<rect x="5" y="11" width="14" height="10" rx="2"/><path d="M8 11V8a4 4 0 0 1 8 0v3"/>"#,
        More => r#"<circle cx="12" cy="5" r="1.4" fill="white"/><circle cx="12" cy="12" r="1.4" fill="white"/><circle cx="12" cy="19" r="1.4" fill="white"/>"#,
        Outline => r#"<path d="M4 6h16M8 12h12M8 18h12"/>"#,
        List => r#"<path d="M8 6h12M8 12h12M8 18h12M4 6h.01M4 12h.01M4 18h.01"/>"#,
        Rotate => r#"<path d="M20 12a8 8 0 1 1-2.3-5.7"/><path d="M20 4v5h-5"/>"#,
        Trash => r#"<path d="M4 7h16M9 7V4h6v3M6 7l1 13h10l1-13"/>"#,
        Copy => r#"<rect x="8" y="8" width="12" height="12" rx="2"/><path d="M16 8V6a2 2 0 0 0-2-2H6a2 2 0 0 0-2 2v8a2 2 0 0 0 2 2h2"/>"#,
        Scan => r#"<path d="M4 8V5h3M17 5h3v3M20 16v3h-3M7 19H4v-3M4 12h16"/>"#,
        Text => r#"<path d="M5 6V4h14v2M12 4v16M9 20h6"/>"#,
        Comment => r#"<path d="M4 5h16v11H9l-5 4z"/>"#,
        Slide => r#"<rect x="3" y="5" width="18" height="12" rx="1.5"/><path d="M12 17v3M8 20h8"/>"#,
        Shape => r#"<circle cx="9" cy="9" r="5"/><rect x="11" y="11" width="9" height="9" rx="1"/>"#,
        Play => r#"<path d="M8 5v14l11-7z"/>"#,
        Sort => r#"<path d="M7 4v16M3 16l4 4 4-4M17 20V4M13 8l4-4 4 4"/>"#,
        Filter => r#"<path d="M4 5h16l-6 8v6l-4 1v-7z"/>"#,
        Function => r#"<path d="M15 4h-2a3 3 0 0 0-3 3v13M7 10h7M14 14l5 5M19 14l-5 5"/>"#,
        Merge => r#"<rect x="3" y="5" width="18" height="14" rx="1.5"/><path d="M7 12h10M9 10l-2 2 2 2M15 10l2 2-2 2"/>"#,
        Link => r#"<path d="M10 14a4 4 0 0 0 5.7 0l3-3a4 4 0 0 0-5.7-5.7l-1 1"/><path d="M14 10a4 4 0 0 0-5.7 0l-3 3a4 4 0 0 0 5.7 5.7l1-1"/>"#,
        Page => r#"<path d="M6 3h8l4 4v14H6z"/><path d="M14 3v4h4"/>"#,
    }
}

const ALL: [Icon; 50] = {
    use Icon::*;
    [
        Menu, Search, Globe, Help, Plus, Minus, Upload, Download, Folder, NewFile, Undo, Redo,
        ChevronDown, ChevronRight, Bold, Italic, Underline, TextColor, AlignLeft, AlignCenter,
        AlignRight, Justify, Bullets, Numbers, Table, Image, ZoomIn, ZoomOut, Close, Check, Info,
        Lock, More, Outline, List, Rotate, Trash, Copy, Scan, Text, Comment, Slide, Shape, Play,
        Sort, Filter, Function, Merge, Link, Page,
    ]
};

/// Raster size of each icon; drawn at 18–20 points, so this stays sharp up to 2× scaling.
const PIXELS: u32 = 48;

pub fn rasterize(icon: Icon, pixels: u32) -> Option<egui::ColorImage> {
    let source = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="white" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">{}</svg>"#,
        svg(icon)
    );
    let tree = resvg::usvg::Tree::from_str(&source, &resvg::usvg::Options::default()).ok()?;
    let mut pixmap = resvg::tiny_skia::Pixmap::new(pixels, pixels)?;
    let scale = pixels as f32 / 24.0;
    resvg::render(&tree, resvg::tiny_skia::Transform::from_scale(scale, scale), &mut pixmap.as_mut());
    Some(egui::ColorImage::from_rgba_premultiplied(
        [pixels as usize, pixels as usize],
        pixmap.data(),
    ))
}

#[derive(Default)]
pub struct Icons {
    textures: HashMap<Icon, TextureHandle>,
}

impl Icons {
    pub fn load(&mut self, ctx: &egui::Context) {
        if !self.textures.is_empty() {
            return;
        }
        for icon in ALL {
            if let Some(image) = rasterize(icon, PIXELS) {
                let texture = ctx.load_texture(format!("icon-{icon:?}"), image, egui::TextureOptions::LINEAR);
                self.textures.insert(icon, texture);
            }
        }
    }
    pub fn image(&self, icon: Icon, size: f32) -> egui::Image<'static> {
        match self.textures.get(&icon) {
            Some(t) => egui::Image::new((t.id(), Vec2::splat(size))),
            None => egui::Image::new((egui::TextureId::default(), Vec2::splat(size))),
        }
    }
    pub fn paint(&self, painter: &egui::Painter, rect: Rect, icon: Icon, color: Color32) {
        if let Some(t) = self.textures.get(&icon) {
            painter.image(
                t.id(),
                rect,
                Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                color,
            );
        }
    }
}

/// App marks: a sheet with a folded corner in the app's accent, carrying a small white glyph.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Logo {
    Document,
    Sheet,
    Slides,
    Pdf,
    Image,
    Scan,
}

pub fn paint_logo(painter: &egui::Painter, rect: Rect, logo: Logo, accent: Color32) {
    // 28×32 design grid, as in the web mark.
    let s = (rect.width() / 28.0).min(rect.height() / 32.0);
    let o = rect.center() - Vec2::new(14.0 * s, 16.0 * s);
    let p = |x: f32, y: f32| o + Vec2::new(x * s, y * s);
    let fold = crate::theme::mix(accent, Color32::WHITE, 0.55);
    painter.add(Shape::convex_polygon(
        vec![p(3.0, 2.0), p(18.0, 2.0), p(25.0, 9.0), p(25.0, 30.0), p(3.0, 30.0)],
        accent,
        Stroke::NONE,
    ));
    painter.add(Shape::convex_polygon(vec![p(18.0, 2.0), p(25.0, 9.0), p(18.0, 9.0)], fold, Stroke::NONE));
    let white = Stroke::new(2.0 * s, Color32::WHITE);
    match logo {
        Logo::Document | Logo::Pdf => {
            painter.line_segment([p(8.0, 15.0), p(20.0, 15.0)], white);
            painter.line_segment([p(8.0, 19.0), p(20.0, 19.0)], white);
            painter.line_segment([p(8.0, 23.0), p(16.0, 23.0)], white);
        }
        Logo::Sheet => {
            let thin = Stroke::new(1.6 * s, Color32::WHITE);
            painter.rect_stroke(Rect::from_min_max(p(7.5, 13.5), p(20.5, 25.5)), 0.0, thin, egui::StrokeKind::Middle);
            painter.line_segment([p(7.5, 17.5), p(20.5, 17.5)], thin);
            painter.line_segment([p(7.5, 21.5), p(20.5, 21.5)], thin);
            painter.line_segment([p(12.5, 13.5), p(12.5, 25.5)], thin);
        }
        Logo::Slides => {
            painter.rect_filled(Rect::from_min_max(p(7.5, 14.0), p(20.5, 23.0)), 1.0 * s, Color32::WHITE);
            painter.rect_filled(Rect::from_min_max(p(9.5, 16.0), p(15.0, 18.0)), 0.0, accent);
        }
        Logo::Image => {
            painter.add(Shape::convex_polygon(
                vec![p(7.0, 25.0), p(12.0, 17.0), p(16.0, 23.0)],
                Color32::WHITE,
                Stroke::NONE,
            ));
            painter.add(Shape::convex_polygon(
                vec![p(13.0, 25.0), p(17.0, 19.0), p(21.0, 25.0)],
                Color32::WHITE,
                Stroke::NONE,
            ));
            painter.circle_filled(p(18.5, 14.5), 1.8 * s, Color32::WHITE);
        }
        Logo::Scan => {
            painter.line_segment([p(8.0, 15.0), p(20.0, 15.0)], white);
            painter.line_segment([p(6.0, 20.0), p(22.0, 20.0)], Stroke::new(2.4 * s, Color32::WHITE));
            painter.line_segment([p(8.0, 25.0), p(16.0, 25.0)], white);
        }
    }
    if logo == Logo::Pdf {
        // The PDF mark keeps the text lines; a darker band marks it apart from documents.
        painter.rect_filled(Rect::from_min_max(p(3.0, 26.0), p(25.0, 30.0)), 0.0, crate::theme::mix(accent, Color32::BLACK, 0.25));
    }
}

/// Icon for a command title, so app commands get the same glyphs on every platform.
pub fn for_command(title: &str) -> Option<Icon> {
    let t = title.trim_end_matches(['…', '.']).trim().to_ascii_lowercase();
    Some(match t.as_str() {
        "bold" | "toggle bold" | "b" => Icon::Bold,
        "i" => Icon::Italic,
        "u" => Icon::Underline,
        "italic" => Icon::Italic,
        "underline" => Icon::Underline,
        "text color" | "text colour" => Icon::TextColor,
        "align left" => Icon::AlignLeft,
        "center" | "align center" | "centre" => Icon::AlignCenter,
        "align right" => Icon::AlignRight,
        "justify" => Icon::Justify,
        "bulleted list" => Icon::Bullets,
        "numbered list" => Icon::Numbers,
        "insert table" | "table" => Icon::Table,
        "insert image" | "insert picture" | "image from file" => Icon::Image,
        "zoom in" => Icon::ZoomIn,
        "zoom out" => Icon::ZoomOut,
        "add comment" => Icon::Comment,
        "navigator" => Icon::Outline,
        "new slide" | "add slide" => Icon::Slide,
        "copy selected text" | "copy" => Icon::Copy,
        "view and select text" => Icon::Text,
        "recognize image text" | "recognize text" => Icon::Scan,
        "sort range" => Icon::Sort,
        "filter" | "auto filter" => Icon::Filter,
        "merge selection" => Icon::Merge,
        "insert or edit link" => Icon::Link,
        _ if t.starts_with("rotate") => Icon::Rotate,
        "delete page" | "delete slide" | "delete pages" | "delete" => Icon::Trash,
        _ => return None,
    })
}
