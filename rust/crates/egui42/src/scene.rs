//! Paints the portable `CanvasScene` (slides, slide thumbnails) with egui shapes.
//!
//! Scene units are points; `Transform` rotates geometry and text, `ClippedOffset` clips and
//! moves, `OpacityLayer` fades. Concave vector fills are rasterised with tiny-skia because egui
//! only fills convex polygons.
use eframe::egui::{
    self, epaint::CubicBezierShape, Align2, Color32, FontFamily, FontId, Pos2, Rect, Shape,
    Stroke, TextureHandle, Vec2,
};
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use suite42_app::{CanvasScene, PathSegment, Point, SceneFont, SceneItem, TextAlign};

#[derive(Default)]
pub struct SceneTextures {
    textures: HashMap<u64, (TextureHandle, u64)>,
    frame: u64,
}

impl SceneTextures {
    /// Drop textures that were not painted during the last few frames.
    pub fn end_frame(&mut self) {
        self.frame += 1;
        let frame = self.frame;
        self.textures.retain(|_, (_, used)| frame.saturating_sub(*used) < 120);
    }
    fn texture(&mut self, ctx: &egui::Context, key: u64, make: impl FnOnce() -> Option<egui::ColorImage>) -> Option<egui::TextureId> {
        if let Some((t, used)) = self.textures.get_mut(&key) {
            *used = self.frame;
            return Some(t.id());
        }
        let image = make()?;
        let t = ctx.load_texture(format!("scene-{key:x}"), image, egui::TextureOptions::LINEAR);
        let id = t.id();
        self.textures.insert(key, (t, self.frame));
        Some(id)
    }
}

/// Map from scene points to screen points.
#[derive(Clone, Copy)]
struct Map {
    origin: Pos2,
    scale: f32,
    offset: Vec2,
    rotation: Option<(Pos2, f32)>,
    opacity: f32,
}

impl Map {
    fn pt(&self, x: f64, y: f64) -> Pos2 {
        let mut p = Pos2::new(x as f32, y as f32) + self.offset;
        if let Some((c, angle)) = self.rotation {
            let (s, co) = angle.sin_cos();
            let d = p - c;
            p = c + Vec2::new(d.x * co - d.y * s, d.x * s + d.y * co);
        }
        self.origin + p.to_vec2() * self.scale
    }
    fn color(&self, c: [u8; 3]) -> Color32 {
        Color32::from_rgba_unmultiplied(c[0], c[1], c[2], (self.opacity * 255.0).round() as u8)
    }
    fn angle(&self) -> f32 {
        self.rotation.map_or(0.0, |(_, a)| a)
    }
}

/// Bounds of the scene's first full-size rectangle (the slide or page), else 720×405 points.
pub fn scene_size(scene: &CanvasScene) -> Vec2 {
    scene
        .items
        .iter()
        .find_map(|item| match item {
            SceneItem::Rect { x, y, width, height, .. } if *x == 0.0 && *y == 0.0 && *width > 50.0 => {
                Some(Vec2::new(*width as f32, *height as f32))
            }
            _ => None,
        })
        .unwrap_or(Vec2::new(720.0, 405.0))
}

pub fn paint(ui: &egui::Ui, textures: &mut SceneTextures, scene: &CanvasScene, rect: Rect) {
    let size = scene_size(scene);
    let scale = (rect.width() / size.x).min(rect.height() / size.y);
    let painter = ui.painter_at(rect);
    let map = Map { origin: rect.min, scale, offset: Vec2::ZERO, rotation: None, opacity: 1.0 };
    for item in &scene.items {
        item_shapes(ui, &painter, textures, item, map);
    }
}

fn font(f: &SceneFont, scale: f32) -> FontId {
    let size = (f.points as f32 * scale).max(1.0);
    if f.bold {
        crate::theme::semibold(size)
    } else {
        FontId::new(size, FontFamily::Proportional)
    }
}

fn item_shapes(ui: &egui::Ui, painter: &egui::Painter, textures: &mut SceneTextures, item: &SceneItem, map: Map) {
    match item {
        SceneItem::OpacityLayer { opacity, items, .. } => {
            let mut m = map;
            m.opacity *= f32::from(*opacity) / 255.0;
            for i in items {
                item_shapes(ui, painter, textures, i, m);
            }
        }
        SceneItem::ClippedOffset { clip, offset, items } => {
            let a = map.pt(clip.x, clip.y);
            let b = map.pt(clip.x + clip.width, clip.y + clip.height);
            let clipped = painter.with_clip_rect(Rect::from_two_pos(a, b).intersect(painter.clip_rect()));
            let mut m = map;
            m.offset += Vec2::new(offset.x as f32, offset.y as f32);
            for i in items {
                item_shapes(ui, &clipped, textures, i, m);
            }
        }
        SceneItem::Transform { center, degrees, items } => {
            let mut m = map;
            let c = Pos2::new(center.x as f32, center.y as f32) + map.offset;
            m.rotation = Some((c, map.angle() + (*degrees as f32).to_radians()));
            for i in items {
                item_shapes(ui, painter, textures, i, m);
            }
        }
        SceneItem::Rect { x, y, width, height, fill, stroke } => {
            let pts = vec![
                map.pt(*x, *y),
                map.pt(x + width, *y),
                map.pt(x + width, y + height),
                map.pt(*x, y + height),
            ];
            painter.add(Shape::convex_polygon(pts, map.color(*fill), Stroke::new(1.0, map.color(*stroke))));
        }
        SceneItem::Ellipse { x, y, width, height, fill, stroke } => {
            let pts: Vec<Pos2> = (0..48)
                .map(|i| {
                    let t = i as f64 / 48.0 * std::f64::consts::TAU;
                    map.pt(x + width / 2.0 * (1.0 + t.cos()), y + height / 2.0 * (1.0 + t.sin()))
                })
                .collect();
            let fill = fill.map_or(Color32::TRANSPARENT, |c| map.color(c));
            let stroke = stroke.map_or(Stroke::NONE, |c| Stroke::new(1.0, map.color(c)));
            painter.add(Shape::convex_polygon(pts, fill, stroke));
        }
        SceneItem::Text { x, y, text, color } => {
            let galley = painter.layout_no_wrap(text.clone(), FontId::proportional(12.0 * map.scale), map.color(*color));
            let shape = egui::epaint::TextShape::new(map.pt(*x, *y), galley, map.color(*color)).with_angle(map.angle());
            painter.add(shape);
        }
        SceneItem::TextBox { x, y, width, height, text, color, font: f, align, wrap } => {
            let width_px = (*width as f32 * map.scale).max(1.0);
            let mut job = egui::text::LayoutJob::simple(
                text.clone(),
                font(f, map.scale),
                map.color(*color),
                if *wrap { width_px } else { f32::INFINITY },
            );
            job.halign = match align {
                TextAlign::Left => egui::Align::LEFT,
                TextAlign::Center => egui::Align::Center,
                TextAlign::Right => egui::Align::RIGHT,
            };
            let galley = painter.layout_job(job);
            let anchor_x = match align {
                TextAlign::Left => *x,
                TextAlign::Center => x + width / 2.0,
                TextAlign::Right => x + width,
            };
            let clip = Rect::from_two_pos(map.pt(*x, *y), map.pt(x + width, y + height)).expand(2.0);
            let p = painter.with_clip_rect(clip.intersect(painter.clip_rect()));
            p.add(egui::epaint::TextShape::new(map.pt(anchor_x, *y), galley, map.color(*color)).with_angle(map.angle()));
        }
        SceneItem::RichText { x, y, width, paragraphs, .. } => {
            let mut job = egui::text::LayoutJob::default();
            job.wrap.max_width = (*width as f32 * map.scale).max(1.0);
            for (i, para) in paragraphs.iter().enumerate() {
                if i > 0 {
                    job.append("\n", 0.0, egui::TextFormat::default());
                }
                if para.bullet {
                    let f = para.runs.first().map(|r| font(&r.font, map.scale)).unwrap_or_default();
                    let c = para.runs.first().map_or(Color32::BLACK, |r| map.color(r.color));
                    job.append("•  ", 0.0, egui::TextFormat::simple(f, c));
                }
                for run in &para.runs {
                    let mut format = egui::TextFormat::simple(font(&run.font, map.scale), map.color(run.color));
                    format.italics = run.font.italic;
                    if run.font.underline {
                        format.underline = Stroke::new(1.0, map.color(run.color));
                    }
                    job.append(&run.text, 0.0, format);
                }
            }
            let galley = painter.layout_job(job);
            painter.add(egui::epaint::TextShape::new(map.pt(*x, *y), galley, Color32::BLACK).with_angle(map.angle()));
        }
        SceneItem::Path { points, color, closed } => {
            let mut pts: Vec<Pos2> = points.iter().map(|p| map.pt(p.x, p.y)).collect();
            if *closed && pts.len() > 2 {
                pts.push(pts[0]);
            }
            painter.add(Shape::line(pts, Stroke::new(1.2, map.color(*color))));
        }
        SceneItem::VectorPath { segments, fill, stroke } => {
            vector_path(ui, painter, textures, segments, *fill, *stroke, map);
        }
        SceneItem::Image { x, y, width, height, rgba } => {
            image(ui, painter, textures, map, (*x, *y, f64::from(*width), f64::from(*height)), (*width, *height), rgba);
        }
        SceneItem::ImageScaled { x, y, width, height, pixel_width, pixel_height, rgba } => {
            image(ui, painter, textures, map, (*x, *y, *width, *height), (*pixel_width, *pixel_height), rgba);
        }
    }
}

fn sample_hash(bytes: &[u8], h: &mut impl Hasher) {
    bytes.len().hash(h);
    for chunk in bytes.chunks(4093) {
        chunk[0].hash(h);
        chunk[chunk.len() / 2].hash(h);
        chunk[chunk.len() - 1].hash(h);
    }
}

#[allow(clippy::too_many_arguments)]
fn image(
    ui: &egui::Ui,
    painter: &egui::Painter,
    textures: &mut SceneTextures,
    map: Map,
    (x, y, w, h): (f64, f64, f64, f64),
    (pw, ph): (u32, u32),
    rgba: &[u8],
) {
    if rgba.len() != pw as usize * ph as usize * 4 || pw == 0 || ph == 0 {
        return;
    }
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    (pw, ph).hash(&mut hasher);
    sample_hash(rgba, &mut hasher);
    let key = hasher.finish();
    let Some(id) = textures.texture(ui.ctx(), key, || {
        Some(egui::ColorImage::from_rgba_unmultiplied([pw as usize, ph as usize], rgba))
    }) else {
        return;
    };
    let tint = Color32::from_white_alpha((map.opacity * 255.0) as u8);
    if map.rotation.is_none() {
        let rect = Rect::from_two_pos(map.pt(x, y), map.pt(x + w, y + h));
        painter.image(id, rect, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), tint);
    } else {
        let mut mesh = egui::Mesh::with_texture(id);
        let corners = [(x, y, 0.0, 0.0), (x + w, y, 1.0, 0.0), (x + w, y + h, 1.0, 1.0), (x, y + h, 0.0, 1.0)];
        for (cx, cy, u, v) in corners {
            mesh.vertices.push(egui::epaint::Vertex { pos: map.pt(cx, cy), uv: Pos2::new(u, v), color: tint });
        }
        mesh.add_triangle(0, 1, 2);
        mesh.add_triangle(0, 2, 3);
        painter.add(Shape::mesh(mesh));
    }
}

/// Flatten a path into sub-polygons in screen space.
fn flatten(segments: &[PathSegment], map: Map) -> Vec<Vec<Pos2>> {
    let mut out: Vec<Vec<Pos2>> = Vec::new();
    let mut current: Vec<Pos2> = Vec::new();
    let mut last = Point::default();
    for s in segments {
        match s {
            PathSegment::Move(p) => {
                if current.len() > 1 {
                    out.push(std::mem::take(&mut current));
                }
                current.clear();
                current.push(map.pt(p.x, p.y));
                last = *p;
            }
            PathSegment::Line(p) => {
                current.push(map.pt(p.x, p.y));
                last = *p;
            }
            PathSegment::Cubic(a, b, c) => {
                let bez = CubicBezierShape::from_points_stroke(
                    [map.pt(last.x, last.y), map.pt(a.x, a.y), map.pt(b.x, b.y), map.pt(c.x, c.y)],
                    false,
                    Color32::TRANSPARENT,
                    Stroke::NONE,
                );
                current.extend(bez.flatten(Some(0.25)).into_iter().skip(1));
                last = *c;
            }
            PathSegment::Close => {
                if let Some(first) = current.first().copied() {
                    current.push(first);
                }
            }
        }
    }
    if current.len() > 1 {
        out.push(current);
    }
    out
}

fn convex(points: &[Pos2]) -> bool {
    let n = points.len();
    if n < 3 {
        return false;
    }
    let mut sign = 0.0f32;
    for i in 0..n {
        let a = points[i];
        let b = points[(i + 1) % n];
        let c = points[(i + 2) % n];
        let cross = (b - a).x * (c - b).y - (b - a).y * (c - b).x;
        if cross.abs() < 1e-3 {
            continue;
        }
        if sign == 0.0 {
            sign = cross.signum();
        } else if cross.signum() != sign {
            return false;
        }
    }
    true
}

fn vector_path(
    ui: &egui::Ui,
    painter: &egui::Painter,
    textures: &mut SceneTextures,
    segments: &[PathSegment],
    fill: Option<[u8; 3]>,
    stroke: Option<[u8; 3]>,
    map: Map,
) {
    let polys = flatten(segments, map);
    if let Some(fill) = fill {
        let color = map.color(fill);
        if polys.len() == 1 && convex(&polys[0]) {
            painter.add(Shape::convex_polygon(polys[0].clone(), color, Stroke::NONE));
        } else {
            raster_fill(ui, painter, textures, &polys, color);
        }
    }
    if let Some(stroke) = stroke {
        for poly in polys {
            painter.add(Shape::line(poly, Stroke::new(1.2, map.color(stroke))));
        }
    }
}

/// Even-odd fill of arbitrary polygons through a small tiny-skia raster.
fn raster_fill(ui: &egui::Ui, painter: &egui::Painter, textures: &mut SceneTextures, polys: &[Vec<Pos2>], color: Color32) {
    let mut bounds = Rect::NOTHING;
    for p in polys.iter().flatten() {
        bounds.extend_with(*p);
    }
    if !bounds.is_positive() {
        return;
    }
    let ppp = ui.ctx().pixels_per_point();
    let (w, h) = ((bounds.width() * ppp).ceil() as u32 + 2, (bounds.height() * ppp).ceil() as u32 + 2);
    if w > 4096 || h > 4096 {
        return;
    }
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    for p in polys.iter().flatten() {
        ((p.x - bounds.min.x).to_bits(), (p.y - bounds.min.y).to_bits()).hash(&mut hasher);
    }
    color.to_array().hash(&mut hasher);
    (w, h).hash(&mut hasher);
    let key = hasher.finish();
    let Some(id) = textures.texture(ui.ctx(), key, || {
        use resvg::tiny_skia::{FillRule, Paint, PathBuilder, Pixmap, Transform};
        let mut pixmap = Pixmap::new(w, h)?;
        let mut pb = PathBuilder::new();
        for poly in polys {
            let mut it = poly.iter();
            let first = it.next()?;
            pb.move_to((first.x - bounds.min.x) * ppp + 1.0, (first.y - bounds.min.y) * ppp + 1.0);
            for p in it {
                pb.line_to((p.x - bounds.min.x) * ppp + 1.0, (p.y - bounds.min.y) * ppp + 1.0);
            }
            pb.close();
        }
        let path = pb.finish()?;
        let mut paint = Paint::default();
        let [r, g, b, a] = color.to_srgba_unmultiplied();
        paint.set_color_rgba8(r, g, b, a);
        paint.anti_alias = true;
        pixmap.fill_path(&path, &paint, FillRule::EvenOdd, Transform::identity(), None);
        Some(egui::ColorImage::from_rgba_premultiplied([w as usize, h as usize], pixmap.data()))
    }) else {
        return;
    };
    let rect = Rect::from_min_size(bounds.min - Vec2::splat(1.0 / ppp), Vec2::new(w as f32, h as f32) / ppp);
    painter.image(id, rect, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
}

/// Centre `text` in `rect` (used for placeholder pages).
pub fn centered_text(painter: &egui::Painter, rect: Rect, text: &str, color: Color32) {
    painter.text(rect.center(), Align2::CENTER_CENTER, text, FontId::proportional(13.0), color);
}
