//! Editable DrawingML preset geometry. The bundled definitions match the macOS catalog.
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, f64::consts::PI, sync::OnceLock};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Preset {
    pub name: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub adjustments: BTreeMap<String, i32>,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}
#[derive(Clone, Debug, PartialEq)]
pub enum Segment {
    Move(Point),
    Line(Point),
    Cubic(Point, Point, Point),
    Close,
}
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum Fill {
    #[default]
    Normal,
    None,
    Darken,
    DarkenLess,
    Lighten,
    LightenLess,
}
impl Fill {
    pub fn color(self, color: [u8; 3]) -> Option<[u8; 3]> {
        let (target, alpha) = match self {
            Self::None => return None,
            Self::Normal => return Some(color),
            Self::Darken => (0.0, 0.4),
            Self::DarkenLess => (0.0, 0.2),
            Self::Lighten => (255.0, 0.4),
            Self::LightenLess => (255.0, 0.2),
        };
        Some(color.map(|v| (f64::from(v) * (1.0 - alpha) + target * alpha).round() as u8))
    }
}
#[derive(Clone, Debug, PartialEq)]
pub struct Subpath {
    pub segments: Vec<Segment>,
    pub fill: Fill,
    pub stroke: bool,
}
#[derive(Clone, Debug)]
enum Expr {
    Number(f64),
    Equation(usize),
    Adjustment(usize),
    Width,
    Height,
    Pi,
    Neg(Box<Expr>),
    Binary(u8, Box<Expr>, Box<Expr>),
    Function(String, Vec<Expr>),
}
struct Parser<'a> {
    bytes: &'a [u8],
    i: usize,
}
impl Parser<'_> {
    fn parse(s: &str) -> Result<Expr, String> {
        let compact = s.replace(' ', "");
        let mut p = Parser {
            bytes: compact.as_bytes(),
            i: 0,
        };
        let e = p.additive()?;
        if p.i != p.bytes.len() {
            return Err(format!("Unexpected preset expression: {s}"));
        }
        Ok(e)
    }
    fn peek(&self) -> u8 {
        self.bytes.get(self.i).copied().unwrap_or(0)
    }
    fn additive(&mut self) -> Result<Expr, String> {
        let mut e = self.term()?;
        while matches!(self.peek(), b'+' | b'-') {
            let op = self.peek();
            self.i += 1;
            e = Expr::Binary(op, Box::new(e), Box::new(self.term()?));
        }
        Ok(e)
    }
    fn term(&mut self) -> Result<Expr, String> {
        let mut e = self.unary()?;
        while matches!(self.peek(), b'*' | b'/') {
            let op = self.peek();
            self.i += 1;
            e = Expr::Binary(op, Box::new(e), Box::new(self.unary()?));
        }
        Ok(e)
    }
    fn unary(&mut self) -> Result<Expr, String> {
        match self.peek() {
            b'-' => {
                self.i += 1;
                Ok(Expr::Neg(Box::new(self.unary()?)))
            }
            b'+' => {
                self.i += 1;
                self.unary()
            }
            _ => self.primary(),
        }
    }
    fn primary(&mut self) -> Result<Expr, String> {
        match self.peek() {
            b'(' => {
                self.i += 1;
                let e = self.additive()?;
                self.expect(b')')?;
                Ok(e)
            }
            b'?' | b'$' => {
                let eq = self.peek() == b'?';
                self.i += 1;
                if eq && self.peek() == b'f' {
                    self.i += 1;
                }
                let start = self.i;
                while self.peek().is_ascii_digit() {
                    self.i += 1;
                }
                let i = std::str::from_utf8(&self.bytes[start..self.i])
                    .unwrap()
                    .parse()
                    .map_err(|_| "Invalid preset reference")?;
                Ok(if eq {
                    Expr::Equation(i)
                } else {
                    Expr::Adjustment(i)
                })
            }
            b'0'..=b'9' | b'.' => {
                let start = self.i;
                while self.peek().is_ascii_digit() || self.peek() == b'.' {
                    self.i += 1;
                }
                if matches!(self.peek(), b'e' | b'E') {
                    self.i += 1;
                    if matches!(self.peek(), b'+' | b'-') {
                        self.i += 1;
                    }
                    while self.peek().is_ascii_digit() {
                        self.i += 1;
                    }
                }
                Ok(Expr::Number(
                    std::str::from_utf8(&self.bytes[start..self.i])
                        .unwrap()
                        .parse()
                        .map_err(|_| "Invalid preset number")?,
                ))
            }
            b'a'..=b'z' | b'A'..=b'Z' => {
                let start = self.i;
                while self.peek().is_ascii_alphanumeric() {
                    self.i += 1;
                }
                let name = std::str::from_utf8(&self.bytes[start..self.i]).unwrap();
                if self.peek() == b'(' {
                    self.i += 1;
                    let mut args = Vec::new();
                    if self.peek() != b')' {
                        loop {
                            args.push(self.additive()?);
                            if self.peek() != b',' {
                                break;
                            }
                            self.i += 1;
                        }
                    }
                    self.expect(b')')?;
                    Ok(Expr::Function(name.into(), args))
                } else {
                    Ok(match name {
                        "logwidth" | "width" | "right" => Expr::Width,
                        "logheight" | "height" | "bottom" => Expr::Height,
                        "pi" => Expr::Pi,
                        _ => return Err(format!("Unknown preset symbol: {name}")),
                    })
                }
            }
            _ => Err("Invalid preset expression".into()),
        }
    }
    fn expect(&mut self, c: u8) -> Result<(), String> {
        if self.peek() != c {
            return Err("Invalid preset punctuation".into());
        }
        self.i += 1;
        Ok(())
    }
}
#[derive(Clone, Debug)]
struct Command {
    op: u8,
    values: Vec<Expr>,
}
#[derive(Clone, Debug)]
struct Handle {
    x: Expr,
    y: Expr,
    polar: bool,
    reference_x: Option<usize>,
    reference_y: Option<usize>,
}
#[derive(Clone, Debug)]
pub struct Definition {
    pub name: String,
    pub adjustments: Vec<(String, i32)>,
    equations: Vec<Expr>,
    commands: Vec<Command>,
    frames: Vec<Expr>,
    views: Vec<[f64; 2]>,
    handles: Vec<Handle>,
}
impl Definition {
    fn parse(line: &str) -> Result<Self, String> {
        let f = line.split('|').collect::<Vec<_>>();
        if f.len() != 7 {
            return Err("Invalid preset field count".into());
        }
        let adjustments = f[1]
            .split(',')
            .filter(|s| !s.is_empty())
            .map(|s| {
                let (name, value) = s.split_once('=').ok_or("Invalid preset adjustment")?;
                Ok((
                    name.into(),
                    value
                        .parse()
                        .map_err(|_| "Invalid preset adjustment value")?,
                ))
            })
            .collect::<Result<Vec<_>, String>>()?;
        let equations = f[2]
            .split(';')
            .filter(|s| !s.is_empty())
            .map(Parser::parse)
            .collect::<Result<Vec<_>, _>>()?;
        let mut tokens = f[3].split_whitespace();
        let mut commands = Vec::new();
        while let Some(t) = tokens.next() {
            let op = *t.as_bytes().first().ok_or("Missing preset command")?;
            let count = match op {
                b'M' | b'L' => 2,
                b'C' => 6,
                b'Q' | b'A' => 4,
                b'Z' | b'N' | b'F' | b'S' | b'D' | b'd' | b'H' | b'h' => 0,
                _ => return Err(format!("Unknown preset command {t}")),
            };
            let mut values = Vec::new();
            while values.len() < count {
                for s in tokens
                    .next()
                    .ok_or("Missing preset coordinates")?
                    .split(',')
                {
                    values.push(Parser::parse(s)?);
                }
            }
            if values.len() != count {
                return Err("Invalid preset coordinates".into());
            }
            commands.push(Command { op, values });
        }
        let frames = f[4]
            .split(';')
            .next()
            .unwrap_or("")
            .split(',')
            .filter(|s| !s.is_empty())
            .map(Parser::parse)
            .collect::<Result<Vec<_>, _>>()?;
        let views = f[5]
            .split(';')
            .filter(|s| !s.is_empty())
            .map(|s| {
                let (a, b) = s.split_once(',').ok_or("Invalid preset view")?;
                Ok([
                    a.parse().map_err(|_| "Invalid view width")?,
                    b.parse().map_err(|_| "Invalid view height")?,
                ])
            })
            .collect::<Result<Vec<_>, String>>()?;
        let handles = f[6]
            .split_whitespace()
            .map(|h| {
                let (position, refs) = h.split_once('@').ok_or("Invalid preset handle")?;
                let (x, y) = position.split_once(',').ok_or("Invalid handle position")?;
                let polar = refs.starts_with('p');
                let refs = refs
                    .trim_start_matches('p')
                    .split('[')
                    .next()
                    .unwrap_or("")
                    .split(',')
                    .collect::<Vec<_>>();
                let reference = |i| refs.get(i).and_then(|s: &&str| s.parse().ok());
                Ok(Handle {
                    x: Parser::parse(x)?,
                    y: Parser::parse(y)?,
                    polar,
                    reference_x: reference(0),
                    reference_y: reference(1),
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        Ok(Self {
            name: f[0].into(),
            adjustments,
            equations,
            commands,
            frames,
            views,
            handles,
        })
    }
    pub fn adjustment_values(&self, preset: &Preset) -> Vec<f64> {
        self.adjustments
            .iter()
            .map(|(name, value)| {
                f64::from(
                    *preset
                        .adjustments
                        .get(name)
                        .or_else(|| {
                            (self.adjustments.len() == 1)
                                .then(|| {
                                    preset
                                        .adjustments
                                        .get("adj")
                                        .or_else(|| preset.adjustments.get("adj1"))
                                })
                                .flatten()
                        })
                        .unwrap_or(value),
                )
            })
            .collect()
    }
    fn evaluator(&self, preset: &Preset, width: f64, height: f64) -> Eval<'_> {
        Eval {
            def: self,
            width,
            height,
            adjustments: self.adjustment_values(preset),
            cache: vec![None; self.equations.len()],
            busy: vec![false; self.equations.len()],
        }
    }
    pub fn render(&self, preset: &Preset, width: f64, height: f64) -> Vec<Subpath> {
        if !width.is_finite() || !height.is_finite() || width <= 0.0 || height <= 0.0 {
            return Vec::new();
        }
        let mut e = self.evaluator(preset, width, height);
        let mut paths = Vec::new();
        let mut path = Subpath {
            segments: Vec::new(),
            fill: Fill::Normal,
            stroke: true,
        };
        let mut sub = 0;
        let mut current = Point { x: 0.0, y: 0.0 };
        let mut start = current;
        for cmd in &self.commands {
            let scale = self
                .views
                .get(sub)
                .filter(|v| v[0] > 0.0 && v[1] > 0.0)
                .map(|v| [width / v[0], height / v[1]])
                .unwrap_or([1.0, 1.0]);
            let v = cmd.values.iter().map(|p| e.value(p)).collect::<Vec<_>>();
            let pt = |k: usize| Point {
                x: v[k * 2] * scale[0],
                y: v[k * 2 + 1] * scale[1],
            };
            match cmd.op {
                b'M' => {
                    current = pt(0);
                    start = current;
                    path.segments.push(Segment::Move(current));
                }
                b'L' => {
                    current = pt(0);
                    if path.segments.is_empty() {
                        start = current;
                        path.segments.push(Segment::Move(current));
                    } else {
                        path.segments.push(Segment::Line(current));
                    }
                }
                b'C' if !path.segments.is_empty() => {
                    current = pt(2);
                    path.segments.push(Segment::Cubic(pt(0), pt(1), current));
                }
                b'Q' if !path.segments.is_empty() => {
                    let q = pt(0);
                    let end = pt(1);
                    let a = Point {
                        x: current.x + (q.x - current.x) * 2.0 / 3.0,
                        y: current.y + (q.y - current.y) * 2.0 / 3.0,
                    };
                    let b = Point {
                        x: end.x + (q.x - end.x) * 2.0 / 3.0,
                        y: end.y + (q.y - end.y) * 2.0 / 3.0,
                    };
                    path.segments.push(Segment::Cubic(a, b, end));
                    current = end;
                }
                b'A' => {
                    if path.segments.is_empty() {
                        path.segments.push(Segment::Move(current));
                        start = current;
                    }
                    arc(
                        &mut path.segments,
                        &mut current,
                        v[0] * scale[0],
                        v[1] * scale[1],
                        v[2],
                        v[3],
                    );
                }
                b'Z' => {
                    if !path.segments.is_empty() {
                        path.segments.push(Segment::Close);
                        current = start;
                    }
                }
                b'N' => {
                    if !path.segments.is_empty() {
                        paths.push(path);
                    }
                    path = Subpath {
                        segments: Vec::new(),
                        fill: Fill::Normal,
                        stroke: true,
                    };
                    sub += 1;
                    current = Point { x: 0.0, y: 0.0 };
                    start = current;
                }
                b'F' => path.fill = Fill::None,
                b'S' => path.stroke = false,
                b'D' => path.fill = Fill::Darken,
                b'd' => path.fill = Fill::DarkenLess,
                b'H' => path.fill = Fill::Lighten,
                b'h' => path.fill = Fill::LightenLess,
                _ => {}
            }
        }
        if !path.segments.is_empty() {
            paths.push(path);
        }
        paths
    }
    pub fn handle_points(&self, preset: &Preset, width: f64, height: f64) -> Vec<Point> {
        let mut e = self.evaluator(preset, width, height);
        let scale = self
            .views
            .first()
            .filter(|v| v[0] > 0.0 && v[1] > 0.0)
            .map(|v| [width / v[0], height / v[1]])
            .unwrap_or([1.0, 1.0]);
        self.handles
            .iter()
            .map(|h| Point {
                x: e.value(&h.x) * scale[0],
                y: e.value(&h.y) * scale[1],
            })
            .collect()
    }
    pub fn drag_handle(
        &self,
        preset: &Preset,
        index: usize,
        target: Point,
        width: f64,
        height: f64,
    ) -> Preset {
        let Some(handle) = self.handles.get(index) else {
            return preset.clone();
        };
        if !target.x.is_finite() || !target.y.is_finite() {
            return preset.clone();
        }
        let mut values = self.adjustment_values(preset);
        let refs = [handle.reference_x, handle.reference_y]
            .into_iter()
            .flatten()
            .filter(|i| *i < values.len())
            .collect::<Vec<_>>();
        let candidate = |values: &[f64]| Preset {
            name: preset.name.clone(),
            adjustments: self
                .adjustments
                .iter()
                .zip(values)
                .map(|((name, _), v)| (name.clone(), v.round() as i32))
                .collect(),
        };
        let distance = |values: &[f64]| {
            let p = self.handle_points(&candidate(values), width, height)[index];
            (p.x - target.x).hypot(p.y - target.y)
        };
        for _ in 0..2 {
            for &r in &refs {
                let angle = handle.polar && Some(r) == handle.reference_x;
                let (mut lo, mut hi) = if angle {
                    (-21600000.0, 21600000.0)
                } else {
                    (-1000000.0, 1000000.0)
                };
                for _ in 0..4 {
                    let mut best = values[r];
                    let mut best_distance = f64::INFINITY;
                    for sample in 0..=60 {
                        let mut test = values.clone();
                        test[r] = lo + (hi - lo) * sample as f64 / 60.0;
                        let d = distance(&test);
                        if d < best_distance - 1e-9 {
                            best_distance = d;
                            best = test[r];
                        }
                    }
                    let span = (hi - lo) / 60.0;
                    values[r] = best;
                    lo = best - span;
                    hi = best + span;
                }
            }
        }
        candidate(&values)
    }
    pub fn text_rect(&self, preset: &Preset, width: f64, height: f64) -> [f64; 4] {
        if self.frames.len() != 4 {
            return [0.0, 0.0, width, height];
        }
        let mut e = self.evaluator(preset, width, height);
        let v = self.frames.iter().map(|p| e.value(p)).collect::<Vec<_>>();
        [v[0], v[1], (v[2] - v[0]).max(1.0), (v[3] - v[1]).max(1.0)]
    }
}
struct Eval<'a> {
    def: &'a Definition,
    width: f64,
    height: f64,
    adjustments: Vec<f64>,
    cache: Vec<Option<f64>>,
    busy: Vec<bool>,
}
impl Eval<'_> {
    fn equation(&mut self, i: usize) -> f64 {
        if i >= self.cache.len() || self.busy[i] {
            return 0.0;
        }
        if let Some(v) = self.cache[i] {
            return v;
        }
        self.busy[i] = true;
        let v = self.value(&self.def.equations[i]);
        self.busy[i] = false;
        let v = if v.is_finite() { v } else { 0.0 };
        self.cache[i] = Some(v);
        v
    }
    fn value(&mut self, expr: &Expr) -> f64 {
        match expr {
            Expr::Number(v) => *v,
            Expr::Equation(i) => self.equation(*i),
            Expr::Adjustment(i) => self.adjustments.get(*i).copied().unwrap_or(0.0),
            Expr::Width => self.width,
            Expr::Height => self.height,
            Expr::Pi => PI,
            Expr::Neg(e) => -self.value(e),
            Expr::Binary(op, a, b) => {
                let a = self.value(a);
                let b = self.value(b);
                match op {
                    b'+' => a + b,
                    b'-' => a - b,
                    b'*' => a * b,
                    _ => {
                        if b == 0.0 {
                            0.0
                        } else {
                            a / b
                        }
                    }
                }
            }
            Expr::Function(name, args) => {
                let v = args.iter().map(|a| self.value(a)).collect::<Vec<_>>();
                let a = |i| v.get(i).copied().unwrap_or(0.0);
                match name.as_str() {
                    "if" => {
                        if a(0) > 0.0 {
                            a(1)
                        } else {
                            a(2)
                        }
                    }
                    "min" => a(0).min(a(1)),
                    "max" => a(0).max(a(1)),
                    "abs" => a(0).abs(),
                    "sqrt" => a(0).max(0.0).sqrt(),
                    "sin" => a(0).sin(),
                    "cos" => a(0).cos(),
                    "tan" => a(0).tan(),
                    "atan" => a(0).atan(),
                    "atan2" => a(0).atan2(a(1)),
                    _ => 0.0,
                }
            }
        }
    }
}
fn arc(segments: &mut Vec<Segment>, current: &mut Point, rx: f64, ry: f64, start: f64, sweep: f64) {
    if (rx.abs() < 0.0001 && ry.abs() < 0.0001) || sweep == 0.0 {
        return;
    }
    let param = |a: f64| (rx * a.to_radians().sin()).atan2(ry * a.to_radians().cos());
    let mut angle = param(start);
    let mut delta = param(start + sweep) - angle;
    if sweep.abs() >= 360.0 {
        delta = if sweep > 0.0 { 2.0 * PI } else { -2.0 * PI };
    } else if sweep > 0.0 {
        while delta <= 0.0 {
            delta += 2.0 * PI;
        }
    } else {
        while delta >= 0.0 {
            delta -= 2.0 * PI;
        }
    }
    let center = Point {
        x: current.x - rx * angle.cos(),
        y: current.y - ry * angle.sin(),
    };
    let count = (delta.abs() / (PI / 2.0)).ceil().max(1.0) as usize;
    let step = delta / count as f64;
    for _ in 0..count {
        let end = angle + step;
        let k = 4.0 / 3.0 * (step / 4.0).tan();
        let a = Point {
            x: center.x + rx * (angle.cos() - k * angle.sin()),
            y: center.y + ry * (angle.sin() + k * angle.cos()),
        };
        let b = Point {
            x: center.x + rx * (end.cos() + k * end.sin()),
            y: center.y + ry * (end.sin() - k * end.cos()),
        };
        *current = Point {
            x: center.x + rx * end.cos(),
            y: center.y + ry * end.sin(),
        };
        segments.push(Segment::Cubic(a, b, *current));
        angle = end;
    }
}
pub fn catalog() -> &'static [Definition] {
    static CATALOG: OnceLock<Vec<Definition>> = OnceLock::new();
    CATALOG.get_or_init(|| {
        include_str!("presets.txt")
            .lines()
            .map(|s| Definition::parse(s).expect("Bundled preset definitions must parse"))
            .collect()
    })
}
pub fn definition(name: &str) -> Option<&'static Definition> {
    catalog().iter().find(|d| d.name == name)
}
impl Preset {
    pub fn new(name: &str) -> Result<Self, String> {
        definition(name).ok_or("Unknown preset shape")?;
        Ok(Self {
            name: name.into(),
            adjustments: BTreeMap::new(),
        })
    }
    pub fn valid(&self) -> bool {
        definition(&self.name).is_some_and(|d| {
            self.adjustments.len() <= d.adjustments.len() + 1
                && self.adjustments.keys().all(|k| {
                    d.adjustments.iter().any(|(n, _)| n == k)
                        || (d.adjustments.len() == 1 && matches!(k.as_str(), "adj" | "adj1"))
                })
        })
    }
    pub fn paths(&self, width: f64, height: f64) -> Vec<Subpath> {
        definition(&self.name)
            .map(|d| d.render(self, width, height))
            .unwrap_or_default()
    }
    pub fn handles(&self, width: f64, height: f64) -> Vec<Point> {
        definition(&self.name)
            .map(|d| d.handle_points(self, width, height))
            .unwrap_or_default()
    }
    pub fn drag_handle(&self, index: usize, target: Point, width: f64, height: f64) -> Self {
        definition(&self.name)
            .map(|d| d.drag_handle(self, index, target, width, height))
            .unwrap_or_else(|| self.clone())
    }
    pub fn text_rect(&self, width: f64, height: f64) -> [f64; 4] {
        definition(&self.name)
            .map(|d| d.text_rect(self, width, height))
            .unwrap_or([0.0, 0.0, width, height])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn complete_catalog_evaluates_at_nonsquare_sizes() {
        assert_eq!(catalog().len(), 187);
        for d in catalog() {
            for (w, h) in [(180.0, 90.0), (53.0, 247.0), (720.0, 405.0)] {
                let preset = Preset::new(&d.name).unwrap();
                let paths = preset.paths(w, h);
                assert!(!paths.is_empty(), "{}", d.name);
                for p in paths {
                    for s in p.segments {
                        let points = match s {
                            Segment::Move(p) | Segment::Line(p) => vec![p],
                            Segment::Cubic(a, b, c) => vec![a, b, c],
                            Segment::Close => Vec::new(),
                        };
                        assert!(
                            points.iter().all(|p| p.x.is_finite() && p.y.is_finite()),
                            "{}",
                            d.name
                        );
                    }
                }
                assert!(
                    preset.text_rect(w, h).iter().all(|v| v.is_finite()),
                    "{}",
                    d.name
                );
            }
        }
    }
    #[test]
    fn ellipse_uses_clockwise_beziers_and_rectangle_uses_actual_size() {
        let rect = Preset::new("rect").unwrap().paths(180.0, 90.0);
        assert_eq!(
            rect[0].segments[1],
            Segment::Line(Point { x: 180.0, y: 0.0 })
        );
        let ellipse = Preset::new("ellipse").unwrap().paths(180.0, 90.0);
        assert_eq!(
            ellipse[0]
                .segments
                .iter()
                .filter(|s| matches!(s, Segment::Cubic(..)))
                .count(),
            4
        );
        let mut p = Preset::new("roundRect").unwrap();
        let a = p.paths(180.0, 90.0);
        p.adjustments.insert("adj".into(), 40000);
        assert_ne!(a, p.paths(180.0, 90.0));
        assert_eq!(p.paths(180.0, 90.0), {
            p.adjustments.clear();
            p.adjustments.insert("adj1".into(), 40000);
            p.paths(180.0, 90.0)
        });
    }
    #[test]
    fn invalid_expressions_and_adjustments_are_rejected() {
        for s in ["?", "min(1,)", "noSuchSymbol", "1;2", "$-1"] {
            assert!(Parser::parse(s).is_err());
        }
        let mut p = Preset::new("rect").unwrap();
        p.adjustments.insert("unknown".into(), 5);
        assert!(!p.valid());
    }
}

#[cfg(test)]
mod reference_tests {
    use super::*;
    use serde_json::Value;
    fn close(a: f64, b: f64, context: &str) {
        assert!(
            (a - b).abs() <= 1e-7 * (1.0 + a.abs().max(b.abs())),
            "{context}: {a} != {b}"
        );
    }
    fn point(v: &[Value], offset: usize) -> Point {
        Point {
            x: v[offset].as_f64().unwrap(),
            y: v[offset + 1].as_f64().unwrap(),
        }
    }
    fn reference_segments(commands: &[Value]) -> Vec<Segment> {
        let mut current = Point { x: 0.0, y: 0.0 };
        let mut start = current;
        let mut out = Vec::new();
        for command in commands {
            let c = command.as_array().unwrap();
            out.push(match c[0].as_str().unwrap() {
                "M" => {
                    current = point(c, 1);
                    start = current;
                    Segment::Move(current)
                }
                "L" => {
                    current = point(c, 1);
                    Segment::Line(current)
                }
                "C" => {
                    current = point(c, 5);
                    Segment::Cubic(point(c, 1), point(c, 3), current)
                }
                "Q" => {
                    let q = point(c, 1);
                    let end = point(c, 3);
                    let a = Point {
                        x: current.x + (q.x - current.x) * 2.0 / 3.0,
                        y: current.y + (q.y - current.y) * 2.0 / 3.0,
                    };
                    let b = Point {
                        x: end.x + (q.x - end.x) * 2.0 / 3.0,
                        y: end.y + (q.y - end.y) * 2.0 / 3.0,
                    };
                    current = end;
                    Segment::Cubic(a, b, end)
                }
                "Z" => {
                    current = start;
                    Segment::Close
                }
                _ => panic!("Unknown reference command"),
            });
        }
        out
    }
    #[test]
    fn matches_actual_mac_engine_paths_frames_and_adjustment_handles() {
        let fixture: Value =
            serde_json::from_str(include_str!("../../fixtures/macos-shape-cases.json")).unwrap();
        let cases = fixture["cases"].as_array().unwrap();
        assert!(cases.len() > 600);
        for case in cases {
            let name = case["name"].as_str().unwrap();
            let w = case["width"].as_f64().unwrap();
            let h = case["height"].as_f64().unwrap();
            let mut preset = Preset::new(name).unwrap();
            preset.adjustments = serde_json::from_value(case["adjust"].clone()).unwrap();
            assert!(preset.valid(), "{name}");
            let paths = preset.paths(w, h);
            let expected = case["paths"].as_array().unwrap();
            assert_eq!(paths.len(), expected.len(), "{name}");
            for (path, expected) in paths.iter().zip(expected) {
                let fill = match path.fill {
                    Fill::Normal => "norm",
                    Fill::None => "none",
                    Fill::Darken => "darken",
                    Fill::DarkenLess => "darkenLess",
                    Fill::Lighten => "lighten",
                    Fill::LightenLess => "lightenLess",
                };
                assert_eq!(fill, expected["fill"].as_str().unwrap(), "{name}");
                assert_eq!(path.stroke, expected["stroke"].as_bool().unwrap(), "{name}");
                let expected = reference_segments(expected["commands"].as_array().unwrap());
                assert_eq!(path.segments.len(), expected.len(), "{name}");
                let points = |s: &Segment| match *s {
                    Segment::Move(p) => ('M', vec![p]),
                    Segment::Line(p) => ('L', vec![p]),
                    Segment::Cubic(a, b, c) => ('C', vec![a, b, c]),
                    Segment::Close => ('Z', Vec::new()),
                };
                for (a, b) in path.segments.iter().zip(&expected) {
                    let (op, a) = points(a);
                    let (expected_op, b) = points(b);
                    assert_eq!(op, expected_op, "{name}");
                    for (a, b) in a.iter().zip(b) {
                        close(a.x, b.x, name);
                        close(a.y, b.y, name);
                    }
                }
            }
            for (a, b) in preset
                .text_rect(w, h)
                .iter()
                .zip(case["text_rect"].as_array().unwrap())
            {
                close(*a, b.as_f64().unwrap(), name);
            }
            let handles = preset.handles(w, h);
            let expected = case["handles"].as_array().unwrap();
            assert_eq!(handles.len(), expected.len(), "{name}");
            for (a, b) in handles.iter().zip(expected) {
                close(a.x, b[0].as_f64().unwrap(), name);
                close(a.y, b[1].as_f64().unwrap(), name);
            }
        }
    }
    #[test]
    fn dragging_handle_changes_geometry_without_moving_the_box() {
        let p = Preset::new("roundRect").unwrap();
        let handles = p.handles(180.0, 90.0);
        assert_eq!(handles.len(), 1);
        let target = Point { x: 30.0, y: 0.0 };
        let changed = p.drag_handle(0, target, 180.0, 90.0);
        assert_ne!(changed, p);
        let point = changed.handles(180.0, 90.0)[0];
        assert!((point.x - target.x).hypot(point.y - target.y) < 0.1);
        assert!(changed.valid());
        assert_ne!(p.paths(180.0, 90.0), changed.paths(180.0, 90.0));
        assert_eq!(
            changed.drag_handle(usize::MAX, target, 180.0, 90.0),
            changed
        );
    }
}
