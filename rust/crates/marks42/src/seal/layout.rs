//! Stamp layout families (Kit42 `SealGenerator` private drawing): every stamp becomes a list of filled outlines and
//! strokes in CoreGraphics (y-up) layout units, with the canvas size. Rendering and pressing are in `mod.rs`.

use super::catalog::{
    Centre, Edge, Frame, Grid, Layout, Rect, Ring, Round, Row, SealTemplate, Shape, Word, MONO, SANS,
};
use super::text::{self, clusters, fit, font, Glyph};
use super::SealSpec;
use std::f64::consts::PI;
use tiny_skia::{LineCap, LineJoin, Path, PathBuilder, Stroke, Transform};

/// An affine transform with CoreGraphics semantics: `translated`, `rotated` and `scaled` apply the new operation
/// to points before the existing transform (CGAffineTransform.translatedBy etc.).
#[derive(Clone, Copy, Debug)]
pub(crate) struct Affine {
    a: f64,
    b: f64,
    c: f64,
    d: f64,
    tx: f64,
    ty: f64,
}

impl Affine {
    pub(crate) const IDENTITY: Affine = Affine { a: 1.0, b: 0.0, c: 0.0, d: 1.0, tx: 0.0, ty: 0.0 };

    pub(crate) fn translation(x: f64, y: f64) -> Affine {
        Affine { tx: x, ty: y, ..Self::IDENTITY }
    }

    /// Maps y-up coordinates onto a top-down bitmap of the given height.
    pub(crate) fn flip(height: f64) -> Affine {
        Affine { d: -1.0, ty: height, ..Self::IDENTITY }
    }

    pub(crate) fn concat(self, m: Affine) -> Affine {
        Affine {
            a: self.a * m.a + self.c * m.b,
            b: self.b * m.a + self.d * m.b,
            c: self.a * m.c + self.c * m.d,
            d: self.b * m.c + self.d * m.d,
            tx: self.a * m.tx + self.c * m.ty + self.tx,
            ty: self.b * m.tx + self.d * m.ty + self.ty,
        }
    }

    pub(crate) fn translated(self, x: f64, y: f64) -> Affine {
        self.concat(Self::translation(x, y))
    }

    pub(crate) fn rotated(self, angle: f64) -> Affine {
        let (s, c) = angle.sin_cos();
        self.concat(Affine { a: c, b: s, c: -s, d: c, tx: 0.0, ty: 0.0 })
    }

    pub(crate) fn scaled(self, sx: f64, sy: f64) -> Affine {
        self.concat(Affine { a: sx, d: sy, ..Self::IDENTITY })
    }

    pub(crate) fn ts(self) -> Transform {
        Transform::from_row(self.a as f32, self.b as f32, self.c as f32, self.d as f32, self.tx as f32, self.ty as f32)
    }
}

/// One part of a stamp drawing.
pub(crate) enum Piece {
    Fill(Path),
    Stroke(Path, Stroke),
}

/// The stamp's outlines (CGMutablePath): pieces are filled as a union.
#[derive(Default)]
pub(crate) struct Drawing {
    pub(crate) pieces: Vec<Piece>,
}

fn round_stroke(width: f64) -> Stroke {
    Stroke { width: width as f32, line_cap: LineCap::Round, line_join: LineJoin::Round, miter_limit: 10.0, dash: None }
}

impl Drawing {
    fn fill(&mut self, path: Option<Path>) {
        if let Some(p) = path {
            self.pieces.push(Piece::Fill(p));
        }
    }

    /// `path.copy(strokingWithWidth:lineCap: .round, lineJoin: .round, miterLimit: 10)` added to the drawing.
    fn stroke(&mut self, path: Option<Path>, width: f64) {
        if let Some(p) = path {
            self.pieces.push(Piece::Stroke(p, round_stroke(width)));
        }
    }

    fn rect(&mut self, x: f64, y: f64, w: f64, h: f64) {
        if let Some(r) = tiny_skia::Rect::from_xywh(x as f32, y as f32, w as f32, h as f32) {
            self.pieces.push(Piece::Fill(PathBuilder::from_rect(r)));
        }
    }

    fn glyph(&mut self, g: &Glyph, t: Affine) {
        if let Some(p) = &g.path {
            self.fill(p.clone().transform(t.ts()));
        }
    }
}

fn ellipse_path(cx: f64, cy: f64, a: f64, b: f64) -> Option<Path> {
    PathBuilder::from_oval(tiny_skia::Rect::from_xywh(
        (cx - a) as f32,
        (cy - b) as f32,
        (2.0 * a) as f32,
        (2.0 * b) as f32,
    )?)
}

/// CGPath(roundedRect:cornerWidth:cornerHeight:): quarter-ellipse corners drawn with cubic curves.
fn rounded_rect(x: f64, y: f64, w: f64, h: f64, radius: f64) -> Option<Path> {
    let r = radius.min(w / 2.0).min(h / 2.0).max(0.0);
    let k = r * (1.0 - 0.552_284_749_8);
    let (x0, y0, x1, y1) = (x as f32, y as f32, (x + w) as f32, (y + h) as f32);
    let (r, k) = (r as f32, k as f32);
    let mut p = PathBuilder::new();
    p.move_to(x1 - r, y0);
    p.cubic_to(x1 - k, y0, x1, y0 + k, x1, y0 + r);
    p.line_to(x1, y1 - r);
    p.cubic_to(x1, y1 - k, x1 - k, y1, x1 - r, y1);
    p.line_to(x0 + r, y1);
    p.cubic_to(x0 + k, y1, x0, y1 - k, x0, y1 - r);
    p.line_to(x0, y0 + r);
    p.cubic_to(x0, y0 + k, x0 + k, y0, x0 + r, y0);
    p.close();
    p.finish()
}

fn polygon(points: &[(f64, f64)]) -> Option<Path> {
    let mut p = PathBuilder::new();
    let (x, y) = points.first()?;
    p.move_to(*x as f32, *y as f32);
    for (x, y) in &points[1..] {
        p.line_to(*x as f32, *y as f32);
    }
    p.close();
    p.finish()
}

/// Lays out the stamp and returns its canvas size (width, height).
pub(crate) fn draw(s: &SealSpec, t: &SealTemplate, p: &mut Drawing) -> (f64, f64) {
    let text = |key: &str| s.text_for(t, key);
    match &t.layout {
        Layout::Round(r) => round(&text, r, p),
        Layout::Oval(r) => oval(&text, r, p),
        Layout::Rect(r) => rect(&text, r, p),
        Layout::Word(w) => word(&text, w, p),
        Layout::Grid(g) => grid(&text, g, p),
        Layout::Dater(f) => dater(&text, f, p),
        Layout::Table => table(&text, p),
        Layout::Triangle => triangle(&text, p),
    }
}

type Text<'a> = dyn Fn(&str) -> String + 'a;

fn round(s: &Text, o: &Round, p: &mut Drawing) -> (f64, f64) {
    let c = (500.0, 500.0);
    let mut r = 470.0;
    match o.ring {
        Ring::Solid => {
            ring(p, c, r, 14.0);
            r -= 24.0;
            if o.double {
                ring(p, c, r, 4.0);
                r -= 12.0;
            }
        }
        Ring::Thick => {
            ring(p, c, r, 24.0);
            r -= 34.0;
            if o.double {
                ring(p, c, r, 4.0);
                r -= 12.0;
            }
        }
        Ring::Thin => {
            ring(p, c, r, 8.0);
            r -= 18.0;
            if o.double {
                ring(p, c, r, 3.0);
                r -= 10.0;
            }
        }
        Ring::Dotted => {
            ring(p, c, r, 9.0);
            dots(p, c, r - 24.0, 72, 5.0);
            r -= 40.0;
            if o.double {
                ring(p, c, r, 3.0);
                r -= 10.0;
            }
        }
        Ring::Rope => {
            ring(p, c, r, 6.0);
            rope(p, c, r - 20.0, 96);
            ring(p, c, r - 40.0, 6.0);
            r -= 52.0;
        }
        Ring::Serrated => {
            serrated(p, c, r + 18.0, r - 4.0, 120);
            ring(p, c, r - 22.0, 5.0);
            r -= 34.0;
        }
        Ring::Stars => {
            ring(p, c, r, 10.0);
            star_ring(p, c, r - 28.0, 40, 10.0);
            ring(p, c, r - 50.0, 4.0);
            r -= 60.0;
        }
    }
    let band = if o.small { 112.0 } else { 72.0 };
    let base = r - band - 4.0;
    let (edge, bottom) = (s("edge"), s("bottom"));
    match o.edge {
        Edge::Full => arc_text(p, &edge, o.font, c, base, band, Arc::Full, 1.0),
        Edge::TopBottom => {
            arc_text(p, &edge, o.font, c, base, band, Arc::Top(PI * 0.13, PI * 0.87), 1.0);
            arc_text(p, &bottom, o.font, c, base, band, Arc::Bottom(PI * 1.17, PI * 1.83), 1.0);
            for a in [PI, 0.0] {
                star(p, (c.0 + (base + band * 0.4) * a.cos(), c.1 + (base + band * 0.4) * a.sin()), 16.0, 5);
            }
        }
        Edge::Top => arc_text(p, &edge, o.font, c, base - 20.0, band + 40.0, Arc::Top(-PI * 0.22, PI * 1.22), 0.78),
    }
    r = base - 16.0;
    if o.double || (o.ring == Ring::Solid && o.edge != Edge::Top) {
        ring(p, c, r, 4.0);
        r -= 10.0;
    }
    let inner = s("inner");
    if o.inner && !inner.is_empty() {
        arc_text(p, &inner, o.font, c, r - 50.0, 44.0, Arc::Full, 1.0);
        r -= 66.0;
        ring(p, c, r, 3.0);
        r -= 8.0;
    }
    let centre = s("centre");
    match o.centre {
        Centre::Lines => lines(p, &centre, o.centre_font, c, (r * 1.45, r * 1.1), if o.small { 130.0 } else { 96.0 }),
        Centre::Initials => lines(p, &centre, o.centre_font, c, (r * 1.3, r * 0.9), r * 0.9),
        Centre::Star => {
            star(p, (c.0, c.1 + 10.0), r * 0.42, 5);
            if !bottom.is_empty() {
                lines(p, &bottom, o.font, (c.0, c.1 - r * 0.62), (r * 1.15, r * 0.3), 84.0);
            }
        }
        Centre::Vertical => vertical(p, &centre, o.centre_font, c, (r * 1.0, r * 1.5)),
        Centre::Grid => {}
    }
    (1000.0, 1000.0)
}

fn oval(s: &Text, o: &Round, p: &mut Drawing) -> (f64, f64) {
    let c = (600.0, 430.0);
    let (mut a, mut b) = (580.0, 410.0);
    if o.ring == Ring::Thick {
        ellipse(p, c, a, b, 22.0);
        a -= 30.0;
        b -= 30.0;
    } else {
        ellipse(p, c, a, b, 13.0);
        a -= 22.0;
        b -= 22.0;
    }
    if o.double && o.ring != Ring::Thick {
        ellipse(p, c, a, b, 4.0);
    }
    ellipse_text(p, &s("edge"), o.font, c, a - 70.0, b - 70.0, 60.0, PI * 0.88, PI * 0.12, false);
    ellipse_text(p, &s("bottom"), o.font, c, a - 70.0, b - 70.0, 60.0, PI * 1.15, PI * 1.85, true);
    ellipse(p, c, a - 106.0, b - 106.0, 4.0);
    lines(p, &s("centre"), o.centre_font, c, ((a - 130.0) * 1.5, (b - 130.0) * 1.2), 110.0);
    (1200.0, 860.0)
}

fn rect(s: &Text, o: &Rect, p: &mut Drawing) -> (f64, f64) {
    let framed = o.frame != Frame::None;
    let pad = if framed { 80.0 } else { 40.0 };
    let width = o.width;
    let inner = width - pad * 2.0;
    // Expand multi-line fields into one row per line, then measure.
    let mut rows: Vec<(&Row, String)> = Vec::new();
    for row in &o.rows {
        if let Row::Text { key, upper, .. } = row {
            let value = s(key);
            for line in value.split('\n').filter(|l| !l.is_empty()) {
                rows.push((row, if *upper { line.to_uppercase() } else { line.to_string() }));
            }
        } else {
            rows.push((row, String::new()));
        }
    }
    let height = |row: &Row| match row {
        Row::Text { size, .. } => size * 1.32,
        Row::Space(h) => *h,
        Row::Line(_) => 26.0,
        Row::Divider => 30.0,
    };
    let total: f64 = rows.iter().map(|(r, _)| height(r)).sum();
    let h = total + pad * 2.0;
    let mut y = h - pad;
    for (row, text) in &rows {
        let rh = height(row);
        match row {
            Row::Text { font: name, size, .. } => {
                let f = fit(name, text, inner, *size);
                let w = f.width(text);
                let x = if o.left { pad } else { (width - w) / 2.0 };
                let baseline = y - rh + (rh - f.cap_height()) / 2.0;
                for g in f.glyphs(text) {
                    p.glyph(&g, Affine::translation(x + g.x, baseline));
                }
            }
            Row::Line(fraction) => {
                let lw = inner * fraction;
                let x = if o.left { pad } else { (width - lw) / 2.0 };
                p.rect(x, y - rh / 2.0 - 2.5, lw, 5.0);
            }
            Row::Divider => {
                let (dx, dw) = if framed { (40.0, 80.0) } else { (0.0, 0.0) };
                p.rect(pad - dx, y - rh / 2.0 - 3.0, inner + dw, 6.0);
            }
            Row::Space(_) => {}
        }
        y -= rh;
    }
    frame(p, (0.0, 0.0, width, h), o.frame);
    (width, h)
}

fn frame(p: &mut Drawing, b: (f64, f64, f64, f64), style: Frame) {
    let (x, y, w, h) = (b.0 + 12.0, b.1 + 12.0, b.2 - 24.0, b.3 - 24.0);
    let mut outline =
        |x: f64, y: f64, w: f64, h: f64, radius: f64, width: f64| p.stroke(rounded_rect(x, y, w, h, radius), width);
    match style {
        Frame::None => {}
        Frame::Single => outline(x, y, w, h, 6.0, 9.0),
        Frame::Rounded => outline(x, y, w, h, 34.0, 9.0),
        Frame::Thick => outline(x, y, w, h, 4.0, 18.0),
        Frame::Double => {
            outline(x, y, w, h, 22.0, 16.0);
            outline(x + 24.0, y + 24.0, w - 48.0, h - 48.0, 12.0, 5.0);
        }
    }
}

fn word(s: &Text, o: &Word, p: &mut Drawing) -> (f64, f64) {
    let size = (1300.0, 640.0);
    let outer = (30.0, 30.0, size.0 - 60.0, size.1 - 60.0);
    let radius = if o.rounded { 26.0 } else { 4.0 };
    p.stroke(rounded_rect(outer.0, outer.1, outer.2, outer.3, radius), 18.0);
    let inner = (outer.0 + 26.0, outer.1 + 26.0, outer.2 - 52.0, outer.3 - 52.0);
    if o.frame == Frame::Double {
        p.stroke(rounded_rect(inner.0, inner.1, inner.2, inner.3, if o.rounded { 14.0 } else { 2.0 }), 6.0);
    }
    let mut rows: Vec<(String, &str, f64)> = Vec::new();
    let (top, date) = (s("top"), s("date"));
    if !top.is_empty() {
        rows.push((top.to_uppercase(), SANS, 52.0));
    }
    rows.push((s("word").to_uppercase(), o.font, 200.0));
    if !date.is_empty() {
        rows.push((date, SANS, 62.0));
    }
    stack(p, &rows, (inner.0 + 40.0, inner.1 + 30.0, inner.2 - 80.0, inner.3 - 60.0));
    size
}

fn grid(s: &Text, o: &Grid, p: &mut Drawing) -> (f64, f64) {
    let chars: Vec<String> =
        clusters(&s("chars")).into_iter().filter(|c| !c.chars().all(char::is_whitespace)).collect();
    let (size, area) = match o.shape {
        Shape::Square => {
            p.stroke(rounded_rect(40.0, 40.0, 920.0, 920.0, 18.0), o.frame);
            let inset = o.frame + 34.0;
            ((1000.0, 1000.0), (40.0 + inset, 40.0 + inset, 920.0 - 2.0 * inset, 920.0 - 2.0 * inset))
        }
        Shape::Round => {
            ring(p, (500.0, 500.0), 470.0, o.frame);
            ((1000.0, 1000.0), (200.0, 170.0, 600.0, 660.0))
        }
        Shape::Oval => {
            ellipse(p, (380.0, 500.0), 350.0, 480.0, o.frame);
            ((760.0, 1000.0), (170.0, 160.0, 420.0, 680.0))
        }
    };
    if chars.is_empty() {
        return size;
    }
    // CJK reads in columns from the right; Latin and other alphabets in rows from the left.
    let latin = chars.iter().all(|c| c.chars().all(|u| (u as u32) < 0x2E80));
    let n = chars.len();
    let per_column = o.per_column.unwrap_or_else(|| ((n as f64).sqrt().ceil() as usize).max(1));
    let columns = (n as f64 / per_column as f64).ceil() as usize;
    let (cell_w, cell_h) = (area.2 / columns as f64, area.3 / per_column as f64);
    let f = font(o.font, 200.0);
    let (min_x, max_x, max_y) = (area.0, area.0 + area.2, area.1 + area.3);
    for (i, ch) in chars.iter().enumerate() {
        let (col, row) = if latin { (i % columns, i / columns) } else { (i / per_column, i % per_column) };
        let x = if latin { min_x + col as f64 * cell_w } else { max_x - (col + 1) as f64 * cell_w };
        let cell = (x, max_y - (row + 1) as f64 * cell_h, cell_w, cell_h);
        let mut glyph_path = PathBuilder::new();
        for g in f.glyphs(ch) {
            if let Some(path) = &g.path {
                glyph_path.push_path(path);
            }
        }
        let Some(glyph_path) = glyph_path.finish() else { continue };
        let Some(b) = glyph_path.compute_tight_bounds() else { continue };
        let (bw, bh) = (b.width() as f64, b.height() as f64);
        if bw <= 0.0 || bh <= 0.0 {
            continue;
        }
        // Seal script fills its cell: scale each axis, but keep the proportions within 1.6×.
        let mut sx = cell.2 * 0.88 / bw;
        let mut sy = cell.3 * 0.88 / bh;
        sx = sx.min(sy * 1.6);
        sy = sy.min(sx * 1.6);
        let (bmx, bmy) = ((b.left() + b.right()) as f64 / 2.0, (b.top() + b.bottom()) as f64 / 2.0);
        let t = Affine::translation(cell.0 + cell.2 / 2.0, cell.1 + cell.3 / 2.0).scaled(sx, sy).translated(-bmx, -bmy);
        p.fill(glyph_path.transform(t.ts()));
    }
    size
}

fn dater(s: &Text, name: &str, p: &mut Drawing) -> (f64, f64) {
    let (c, r) = ((500.0, 500.0), 470.0);
    ring(p, c, r, 16.0);
    for y in [120.0, -120.0] {
        let half: f64 = (r * r - y * y).sqrt() - 6.0;
        p.rect(c.0 - half, c.1 + y - 3.0, half * 2.0, 6.0);
    }
    lines(p, &s("top"), name, (c.0, c.1 + 265.0), (560.0, 170.0), 150.0);
    lines(p, &s("date"), MONO, c, (800.0, 150.0), 130.0);
    lines(p, &s("name"), name, (c.0, c.1 - 265.0), (560.0, 170.0), 150.0);
    (1000.0, 1000.0)
}

fn table(s: &Text, p: &mut Drawing) -> (f64, f64) {
    let labels: Vec<String> = s("rows").split('\n').filter(|l| !l.is_empty()).map(String::from).collect();
    let top = s("top");
    let (row_h, width) = (130.0, 1300.0);
    let count = labels.len() + usize::from(!top.is_empty());
    let h = count.max(1) as f64 * row_h + 40.0;
    let b = (20.0, 20.0, width - 40.0, h - 40.0);
    if let Some(r) = tiny_skia::Rect::from_xywh(b.0 as f32, b.1 as f32, b.2 as f32, b.3 as f32) {
        let stroke = Stroke {
            width: 12.0,
            line_cap: LineCap::Square,
            line_join: LineJoin::Miter,
            miter_limit: 10.0,
            dash: None,
        };
        p.pieces.push(Piece::Stroke(PathBuilder::from_rect(r), stroke));
    }
    let mut y = b.1 + b.3;
    if !top.is_empty() {
        lines(p, &top.to_uppercase(), SANS, (b.0 + b.2 / 2.0, y - row_h / 2.0), (b.2 - 80.0, row_h * 0.6), 64.0);
        y -= row_h;
        p.rect(b.0, y - 3.0, b.2, 6.0);
    }
    let split = b.0 + b.2 * 0.42;
    p.rect(split - 3.0, b.1, 6.0, y - b.1);
    for (i, label) in labels.iter().enumerate() {
        let f = fit("HelveticaNeue-Medium", label, split - b.0 - 60.0, 54.0);
        for g in f.glyphs(label) {
            p.glyph(&g, Affine::translation(b.0 + 30.0 + g.x, y - row_h / 2.0 - f.cap_height() / 2.0));
        }
        y -= row_h;
        if i < labels.len() - 1 {
            p.rect(b.0, y - 3.0, b.2, 6.0);
        }
    }
    (width, h)
}

fn triangle(s: &Text, p: &mut Drawing) -> (f64, f64) {
    p.stroke(polygon(&[(500.0, 940.0), (60.0, 160.0), (940.0, 160.0)]), 22.0);
    lines(p, &s("word"), SANS, (500.0, 300.0), (520.0, 150.0), 150.0);
    lines(p, &s("number"), SANS, (500.0, 520.0), (300.0, 200.0), 200.0);
    (1000.0, 1000.0)
}

fn dots(p: &mut Drawing, c: (f64, f64), r: f64, count: usize, size: f64) {
    for i in 0..count {
        let a = i as f64 / count as f64 * 2.0 * PI;
        p.fill(ellipse_path(c.0 + r * a.cos(), c.1 + r * a.sin(), size, size));
    }
}

/// Twisted-rope border: short slanted bars around the circle.
fn rope(p: &mut Drawing, c: (f64, f64), r: f64, count: usize) {
    for i in 0..count {
        let a = i as f64 / count as f64 * 2.0 * PI;
        let mut bar = PathBuilder::new();
        bar.move_to((c.0 + (r - 12.0) * a.cos()) as f32, (c.1 + (r - 12.0) * a.sin()) as f32);
        bar.line_to((c.0 + (r + 12.0) * (a + 0.035).cos()) as f32, (c.1 + (r + 12.0) * (a + 0.035).sin()) as f32);
        p.stroke(bar.finish(), 7.0);
    }
}

fn star_ring(p: &mut Drawing, c: (f64, f64), r: f64, count: usize, size: f64) {
    for i in 0..count {
        let a = i as f64 / count as f64 * 2.0 * PI;
        star(p, (c.0 + r * a.cos(), c.1 + r * a.sin()), size, 5);
    }
}

fn ring(p: &mut Drawing, c: (f64, f64), r: f64, width: f64) {
    p.stroke(ellipse_path(c.0, c.1, r, r), width);
}

fn ellipse(p: &mut Drawing, c: (f64, f64), a: f64, b: f64, width: f64) {
    p.stroke(ellipse_path(c.0, c.1, a, b), width);
}

/// Saw-tooth edge of a corporate seal.
fn serrated(p: &mut Drawing, c: (f64, f64), outer: f64, inner: f64, teeth: usize) {
    let points: Vec<(f64, f64)> = (0..teeth * 2)
        .map(|i| {
            let a = i as f64 / (teeth * 2) as f64 * 2.0 * PI;
            let r = if i % 2 == 0 { outer } else { inner };
            (c.0 + r * a.cos(), c.1 + r * a.sin())
        })
        .collect();
    p.stroke(polygon(&points), 5.0);
}

fn star(p: &mut Drawing, c: (f64, f64), r: f64, n: usize) {
    let points: Vec<(f64, f64)> = (0..n * 2)
        .map(|i| {
            let a = PI / 2.0 + i as f64 * PI / n as f64;
            let rr = if i % 2 == 0 { r } else { r * 0.38 };
            (c.0 + rr * a.cos(), c.1 + rr * a.sin())
        })
        .collect();
    p.fill(polygon(&points));
}

/// Where text runs along a circle.
#[derive(Clone, Copy)]
enum Arc {
    /// Spread evenly around the whole circle from about 9 o'clock (CIS seals).
    Full,
    /// Along the upper arc between two angles, tops outward, clockwise.
    Top(f64, f64),
    /// Along the lower arc, left to right, tops toward the centre.
    Bottom(f64, f64),
}

/// Text along a circle. Letters stand on `radius` with their tops outward and run clockwise; a full-circle
/// text is spread evenly around, so the bottom half reads upside down as on real seals.
#[allow(clippy::too_many_arguments)]
fn arc_text(p: &mut Drawing, text: &str, name: &str, c: (f64, f64), radius: f64, band: f64, arc: Arc, condensed: f64) {
    if text.is_empty() {
        return;
    }
    let chars = clusters(text);
    let n = chars.len() as f64;
    let f = font(name, band * 0.78);
    let widths: Vec<f64> = chars.iter().map(|ch| f.width(ch) * condensed).collect();
    let total: f64 = widths.iter().sum();
    let (full, bottom, span) = match arc {
        Arc::Full => (true, false, (0.0, 0.0)),
        Arc::Top(a, b) => (false, false, (a, b)),
        Arc::Bottom(a, b) => (false, true, (a, b)),
    };
    let available = if full { 2.0 * PI * radius } else { (span.1 - span.0).abs() * radius };
    // Shrink when it does not fit, otherwise add letter spacing to fill the arc.
    let squeeze = (available * 0.97 / total.max(1.0)).min(1.0);
    let gap = if full {
        (available - total * squeeze) / n
    } else {
        ((available - total * squeeze) / (n - 1.0).max(1.0)).max(0.0)
    };
    let letter_gap = if full { gap } else { gap.min(band * 0.35) };
    let used = total * squeeze + letter_gap * if full { n } else { n - 1.0 };
    // Clockwise from the start angle (CIS: from the left, so the text begins at about 9 o'clock).
    let mut angle = if full {
        PI * 0.98
    } else if bottom {
        (span.0 + span.1) / 2.0 - used / radius / 2.0
    } else {
        (span.0 + span.1) / 2.0 + used / radius / 2.0
    };
    for (i, ch) in chars.iter().enumerate() {
        let w = widths[i] * squeeze;
        let mid = if bottom { angle + w / 2.0 / radius } else { angle - w / 2.0 / radius };
        for g in f.glyphs(ch) {
            let t = if bottom {
                let r = radius + band * 0.62;
                Affine::translation(c.0 + r * mid.cos(), c.1 + r * mid.sin()).rotated(mid + PI / 2.0)
            } else {
                Affine::translation(c.0 + radius * mid.cos(), c.1 + radius * mid.sin()).rotated(mid - PI / 2.0)
            };
            p.glyph(&g, t.scaled(squeeze * condensed, 1.0).translated(-g.advance / 2.0, 0.0));
        }
        angle += if bottom { (w + letter_gap) / radius } else { -(w + letter_gap) / radius };
    }
}

/// Text along an ellipse arc between parameter angles (top: tops outward, clockwise; bottom: upright).
#[allow(clippy::too_many_arguments)]
fn ellipse_text(
    p: &mut Drawing,
    text: &str,
    name: &str,
    c: (f64, f64),
    a: f64,
    b: f64,
    band: f64,
    from: f64,
    to: f64,
    bottom: bool,
) {
    if text.is_empty() {
        return;
    }
    let f = font(name, band * 0.8);
    let chars = clusters(text);
    let widths: Vec<f64> = chars.iter().map(|ch| f.width(ch)).collect();
    // Arc length table along the parameter.
    let steps = 600usize;
    let point = |t: f64| (c.0 + a * t.cos(), c.1 + b * t.sin());
    let mut lengths = vec![0.0];
    for i in 1..=steps {
        let t0 = from + (to - from) * (i - 1) as f64 / steps as f64;
        let t1 = from + (to - from) * i as f64 / steps as f64;
        let (p0, p1) = (point(t0), point(t1));
        lengths.push(lengths[i - 1] + (p1.0 - p0.0).hypot(p1.1 - p0.1));
    }
    let arc = *lengths.last().unwrap();
    let total: f64 = widths.iter().sum();
    let squeeze = (arc * 0.96 / total.max(1.0)).min(1.0);
    let n = chars.len() as f64;
    let gap = (band * 0.3).min(((arc - total * squeeze) / (n - 1.0).max(1.0)).max(0.0));
    let used = total * squeeze + gap * (n - 1.0);
    let mut s = (arc - used) / 2.0;
    let param = |length: f64| {
        let i = lengths.iter().position(|&l| l >= length).unwrap_or(steps);
        from + (to - from) * i as f64 / steps as f64
    };
    for (i, ch) in chars.iter().enumerate() {
        let w = widths[i] * squeeze;
        let t = param(s + w / 2.0);
        let pt = point(t);
        // Outward normal of the ellipse.
        let normal = (a * t.sin()).atan2(b * t.cos());
        for g in f.glyphs(ch) {
            let m = if bottom {
                let off = band * 0.62;
                Affine::translation(pt.0 + off * normal.cos(), pt.1 + off * normal.sin()).rotated(normal + PI / 2.0)
            } else {
                Affine::translation(pt.0, pt.1).rotated(normal - PI / 2.0)
            };
            p.glyph(&g, m.scaled(squeeze, 1.0).translated(-g.advance / 2.0, 0.0));
        }
        s += w + gap;
    }
}

/// Centred lines inside a box, each fitted to the width; the block is centred vertically.
fn lines(p: &mut Drawing, text: &str, name: &str, c: (f64, f64), b: (f64, f64), max_size: f64) {
    let rows: Vec<&str> = text.split('\n').filter(|r| !r.is_empty()).collect();
    if rows.is_empty() {
        return;
    }
    let line_size = max_size.min(b.1 / rows.len() as f64 / 1.15);
    let fonts: Vec<text::Font> = rows.iter().map(|r| fit(name, r, b.0, line_size)).collect();
    let heights: Vec<f64> = fonts.iter().map(|f| f.cap_height()).collect();
    let spacing = line_size * 0.42;
    let total = heights.iter().sum::<f64>() + spacing * (rows.len() - 1) as f64;
    let mut y = c.1 + total / 2.0;
    for (i, row) in rows.iter().enumerate() {
        y -= heights[i];
        let w = fonts[i].width(row);
        for g in fonts[i].glyphs(row) {
            p.glyph(&g, Affine::translation(c.0 - w / 2.0 + g.x, y));
        }
        y -= spacing;
    }
}

/// Rows of (text, font, size) stacked and centred in the rect (x, y, width, height), shrunk to fit its height.
fn stack(p: &mut Drawing, rows: &[(String, &str, f64)], r: (f64, f64, f64, f64)) {
    if rows.is_empty() {
        return;
    }
    let fonts: Vec<text::Font> = rows.iter().map(|(t, f, s)| fit(f, t, r.2, *s)).collect();
    let heights: Vec<f64> = fonts.iter().map(|f| f.cap_height()).collect();
    let gaps: Vec<f64> = fonts.iter().map(|f| f.size * 0.5).collect();
    let mut total = heights.iter().sum::<f64>() + gaps[..gaps.len() - 1].iter().sum::<f64>();
    let mut scale = 1.0;
    if total > r.3 {
        scale = r.3 / total;
        total = r.3;
    }
    let mut y = r.1 + r.3 / 2.0 + total / 2.0;
    for (i, row) in rows.iter().enumerate() {
        let f = if scale < 1.0 { fonts[i].with_size(fonts[i].size * scale) } else { fonts[i] };
        y -= heights[i] * scale;
        let w = f.width(&row.0);
        let x = r.0 + r.2 / 2.0 - w / 2.0;
        for g in f.glyphs(&row.0) {
            p.glyph(&g, Affine::translation(x + g.x, y));
        }
        y -= gaps[i] * scale;
    }
}

/// Hanko: characters in vertical columns, right to left.
fn vertical(p: &mut Drawing, text: &str, name: &str, c: (f64, f64), b: (f64, f64)) {
    let chars: Vec<String> = clusters(text).into_iter().filter(|c| !c.chars().all(char::is_whitespace)).collect();
    if chars.is_empty() {
        return;
    }
    let n = chars.len();
    let per_column = if n <= 4 { n } else { (n as f64 / 2.0).ceil() as usize };
    let columns = (n as f64 / per_column as f64).ceil() as usize;
    let cell = (b.1 / per_column as f64).min(b.0 / columns as f64);
    let f = font(name, cell * 0.92);
    for (i, ch) in chars.iter().enumerate() {
        let (col, row) = (i / per_column, i % per_column);
        let x = c.0 + (columns as f64 / 2.0 - col as f64 - 0.5) * cell;
        let y = c.1 + (per_column as f64 / 2.0 - row as f64 - 1.0) * cell + cell * 0.14;
        for g in f.glyphs(ch) {
            p.glyph(&g, Affine::translation(x - g.advance / 2.0, y));
        }
    }
}
