use win42::{CanvasScene, Point, SceneFont, SceneItem, TextAlign, Viewport};
use xls42_core::{ChartData, ChartKind};
const COLORS: [[u8; 3]; 6] = [
    [54, 113, 207],
    [234, 123, 50],
    [87, 163, 107],
    [174, 99, 176],
    [220, 168, 54],
    [66, 165, 176],
];
fn label(scene: &mut CanvasScene, x: f64, y: f64, text: impl Into<String>, color: [u8; 3]) {
    scene.items.push(SceneItem::TextBox {
        x,
        y,
        width: 180.0,
        height: 18.0,
        text: text.into(),
        color,
        font: SceneFont {
            family: "Arial".into(),
            points: 10.0,
            bold: false,
            italic: false,
            underline: false,
        },
        align: TextAlign::Left,
        wrap: false,
    })
}
pub fn scene(data: &ChartData) -> CanvasScene {
    let mut scene = CanvasScene {
        viewport: Viewport::default(),
        items: vec![SceneItem::Rect {
            x: 0.0,
            y: 0.0,
            width: 700.0,
            height: 440.0,
            fill: [255; 3],
            stroke: [210; 3],
        }],
    };
    label(&mut scene, 24.0, 18.0, &data.title, [25, 25, 25]);
    if data.series.is_empty() {
        label(&mut scene, 40.0, 90.0, "No chart data", [90; 3]);
        return scene;
    }
    if data.kind == ChartKind::Pie {
        let values: Vec<_> = data.series[0]
            .1
            .iter()
            .filter_map(|(label, n)| n.filter(|n| *n > 0.0).map(|n| (label.clone(), n)))
            .collect();
        let total: f64 = values.iter().map(|(_, v)| v).sum();
        if total <= 0.0 || !total.is_finite() {
            label(
                &mut scene,
                40.0,
                90.0,
                "Pie chart needs positive values",
                [90; 3],
            );
            return scene;
        }
        let size = 260usize;
        let mut rgba = vec![0u8; size * size * 4];
        for y in 0..size {
            for x in 0..size {
                let dx = x as f64 - 129.5;
                let dy = y as f64 - 129.5;
                if dx * dx + dy * dy <= 126.0 * 126.0 {
                    let angle = (dy.atan2(dx) + std::f64::consts::FRAC_PI_2)
                        .rem_euclid(std::f64::consts::TAU);
                    let mut acc = 0.0;
                    let mut index = values.len() - 1;
                    for (i, (_, v)) in values.iter().enumerate() {
                        acc += v / total * std::f64::consts::TAU;
                        if angle <= acc {
                            index = i;
                            break;
                        }
                    }
                    let color = COLORS[index % COLORS.len()];
                    let at = (y * size + x) * 4;
                    rgba[at..at + 3].copy_from_slice(&color);
                    rgba[at + 3] = 255
                }
            }
        }
        scene.items.push(SceneItem::Image {
            x: 55.0,
            y: 65.0,
            width: size as u32,
            height: size as u32,
            rgba,
        });
        for (i, (name, n)) in values.iter().take(14).enumerate() {
            label(
                &mut scene,
                350.0,
                75.0 + i as f64 * 22.0,
                format!("{}: {}", name, n),
                COLORS[i % COLORS.len()],
            )
        }
        return scene;
    }
    let all: Vec<f64> = data
        .series
        .iter()
        .flat_map(|(_, p)| p.iter().filter_map(|(_, v)| *v))
        .collect();
    if all.is_empty() {
        label(&mut scene, 40.0, 90.0, "No numeric chart data", [90; 3]);
        return scene;
    }
    let min = all.iter().copied().fold(0.0, f64::min);
    let max = all.iter().copied().fold(0.0, f64::max);
    let span = (max - min).max(1e-12);
    let (x0, y0, w, h) = (65.0, 65.0, 520.0, 300.0);
    let fy = |v: f64| y0 + h - (v - min) / span * h;
    scene.items.push(SceneItem::Path {
        points: vec![
            Point { x: x0, y: y0 },
            Point { x: x0, y: y0 + h },
            Point {
                x: x0 + w,
                y: y0 + h,
            },
        ],
        color: [90; 3],
        closed: false,
    });
    for step in 0..=4 {
        let n = min + span * step as f64 / 4.0;
        let y = fy(n);
        label(&mut scene, 5.0, y - 5.0, format!("{n:.2}"), [80; 3]);
        scene.items.push(SceneItem::Path {
            points: vec![Point { x: x0, y }, Point { x: x0 + w, y }],
            color: [225; 3],
            closed: false,
        })
    }
    let count = data
        .series
        .iter()
        .map(|(_, p)| p.len())
        .max()
        .unwrap_or(1)
        .max(1);
    let numeric_x: Vec<_> = data
        .series
        .iter()
        .flat_map(|(_, p)| {
            p.iter()
                .filter_map(|(x, _)| x.parse::<f64>().ok().filter(|n| n.is_finite()))
        })
        .collect();
    let xmin = numeric_x.iter().copied().reduce(f64::min).unwrap_or(0.0);
    let xmax = numeric_x
        .iter()
        .copied()
        .reduce(f64::max)
        .unwrap_or(count as f64);
    let xspan = (xmax - xmin).max(1e-12);
    for (s, (name, points)) in data.series.iter().enumerate() {
        let color = COLORS[s % COLORS.len()];
        label(&mut scene, 90.0 + s as f64 * 100.0, 395.0, name, color);
        let mut path = Vec::new();
        for (i, (label_text, value)) in points.iter().enumerate() {
            let Some(value) = value else {
                if path.len() > 1 {
                    scene.items.push(SceneItem::Path {
                        points: std::mem::take(&mut path),
                        color,
                        closed: false,
                    })
                } else {
                    path.clear();
                }
                continue;
            };
            let x = if data.kind == ChartKind::Scatter {
                let Ok(x) = label_text.parse::<f64>() else {
                    continue;
                };
                if !x.is_finite() {
                    continue;
                }
                x0 + (x - xmin) / xspan * w
            } else {
                x0 + (i as f64 + 0.5) * w / count as f64
            };
            let y = fy(*value);
            if data.kind == ChartKind::Bar {
                let group = w / count as f64;
                let bw = (group / (data.series.len() as f64 + 1.0)).min(32.0);
                let bx = x - group * 0.4 + bw * s as f64;
                let baseline = fy(0.0);
                scene.items.push(SceneItem::Rect {
                    x: bx,
                    y: y.min(baseline),
                    width: bw * 0.9,
                    height: (baseline - y).abs().max(1.0),
                    fill: color,
                    stroke: color,
                });
            } else if data.kind == ChartKind::Scatter {
                scene.items.push(SceneItem::Rect {
                    x: x - 3.0,
                    y: y - 3.0,
                    width: 6.0,
                    height: 6.0,
                    fill: color,
                    stroke: color,
                });
            } else {
                path.push(Point { x, y });
                scene.items.push(SceneItem::Rect {
                    x: x - 2.0,
                    y: y - 2.0,
                    width: 4.0,
                    height: 4.0,
                    fill: color,
                    stroke: color,
                });
            }
            if s == 0 && count <= 12 {
                label(&mut scene, x - 12.0, y0 + h + 8.0, label_text, [70; 3])
            }
        }
        if data.kind == ChartKind::Line && path.len() > 1 {
            scene.items.push(SceneItem::Path {
                points: path,
                color,
                closed: false,
            })
        }
    }
    scene
}
/// Scale the chart to fit a print page in logical points.
pub fn print_scene(data: &ChartData, width: f64, height: f64) -> CanvasScene {
    let mut scene = scene(data);
    let scale = (width / 700.0).min(height / 440.0).min(1.0);
    let mut pending: Vec<&mut SceneItem> = scene.items.iter_mut().collect();
    while let Some(item) = pending.pop() {
        match item {
            SceneItem::ClippedOffset {
                clip,
                offset,
                items,
            } => {
                clip.x *= scale;
                clip.y *= scale;
                clip.width *= scale;
                clip.height *= scale;
                offset.x *= scale;
                offset.y *= scale;
                pending.extend(items.iter_mut());
            }
            SceneItem::OpacityLayer { bounds, items, .. } => {
                bounds.x *= scale;
                bounds.y *= scale;
                bounds.width *= scale;
                bounds.height *= scale;
                pending.extend(items.iter_mut());
            }
            SceneItem::Transform { center, items, .. } => {
                center.x *= scale;
                center.y *= scale;
                pending.extend(items.iter_mut());
            }
            SceneItem::Rect {
                x,
                y,
                width,
                height,
                ..
            } => {
                *x *= scale;
                *y *= scale;
                *width *= scale;
                *height *= scale
            }
            SceneItem::Ellipse {
                x,
                y,
                width,
                height,
                ..
            } => {
                *x *= scale;
                *y *= scale;
                *width *= scale;
                *height *= scale;
            }
            SceneItem::Text { x, y, .. } => {
                *x *= scale;
                *y *= scale
            }
            SceneItem::TextBox {
                x,
                y,
                width,
                height,
                font,
                ..
            } => {
                *x *= scale;
                *y *= scale;
                *width *= scale;
                *height *= scale;
                font.points *= scale
            }
            SceneItem::RichText {
                x,
                y,
                width,
                height,
                paragraphs,
            } => {
                *x *= scale;
                *y *= scale;
                *width *= scale;
                *height *= scale;
                for p in paragraphs {
                    for r in &mut p.runs {
                        r.font.points *= scale;
                    }
                }
            }
            SceneItem::Path { points, .. } => {
                for p in points {
                    p.x *= scale;
                    p.y *= scale
                }
            }
            SceneItem::Image {
                x,
                y,
                width,
                height,
                rgba,
            } => {
                *x *= scale;
                *y *= scale;
                let oldw = *width as usize;
                let oldh = *height as usize;
                let neww = ((oldw as f64 * scale).round() as usize).max(1);
                let newh = ((oldh as f64 * scale).round() as usize).max(1);
                let mut data = vec![0; neww * newh * 4];
                for r in 0..newh {
                    for c in 0..neww {
                        let sr = r * oldh / newh;
                        let sc = c * oldw / neww;
                        let from = (sr * oldw + sc) * 4;
                        let to = (r * neww + c) * 4;
                        data[to..to + 4].copy_from_slice(&rgba[from..from + 4]);
                    }
                }
                *rgba = data;
                *width = neww as u32;
                *height = newh as u32
            }
            SceneItem::ImageScaled {
                x,
                y,
                width,
                height,
                ..
            } => {
                *x *= scale;
                *y *= scale;
                *width *= scale;
                *height *= scale;
            }
        }
    }
    scene
}
