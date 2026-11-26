//! Chart drawing and the Insert/Edit Chart dialog.
use ppt42_core::chart::{self, Chart, Mark};
use win42::{PathSegment, Point, SceneItem};

pub(super) const INSERT: u16 = 80;
pub(super) const EDIT: u16 = 81;
const LEGEND: [&str; 2] = ["No", "Yes"];
pub(super) const DATA_FIELD: usize = 3;

/// Scene items for a chart frame at (`x`, `y`).
pub(super) fn scene(chart: &Chart, x: f64, y: f64, width: f64, height: f64) -> Vec<SceneItem> {
    let p = |(px, py): (f64, f64)| Point { x: x + px, y: y + py };
    chart::layout(chart, width, height)
        .into_iter()
        .map(|mark| match mark {
            Mark::Rect { x: rx, y: ry, width, height, fill, stroke } => SceneItem::Rect {
                x: x + rx,
                y: y + ry,
                width,
                height,
                fill,
                stroke: stroke.unwrap_or(fill),
            },
            Mark::Polygon { points, fill, stroke } => {
                let mut segments = Vec::with_capacity(points.len() + 1);
                for (i, point) in points.into_iter().enumerate() {
                    segments.push(if i == 0 { PathSegment::Move(p(point)) } else { PathSegment::Line(p(point)) });
                }
                segments.push(PathSegment::Close);
                SceneItem::VectorPath { segments, fill: Some(fill), stroke }
            }
            Mark::Line { points, color } => SceneItem::Path { points: points.into_iter().map(p).collect(), color, closed: false },
            Mark::Text { x: tx, y: ty, width, height, text, points, bold, align } => SceneItem::TextBox {
                x: x + tx,
                y: y + ty,
                width,
                height,
                text,
                color: [64, 64, 64],
                font: win42::SceneFont { family: "Arial".into(), points, bold, italic: false, underline: false },
                align: match align {
                    -1 => win42::TextAlign::Left,
                    0 => win42::TextAlign::Center,
                    _ => win42::TextAlign::Right,
                },
                wrap: false,
            },
        })
        .collect()
}

pub(super) fn request(id: u16, chart: &Chart) -> win42::InputRequest {
    win42::InputRequest {
        title: if id == INSERT { "Insert Chart" } else { "Edit Chart" },
        fields: vec![
            ("Chart type", chart.type_label().into()),
            ("Title", chart.title.clone()),
            ("Legend", LEGEND[chart.legend as usize].into()),
            (
                "Data (tab-separated; first row series names, first column categories)",
                win42::to_native_text(&chart.grid()),
            ),
        ],
    }
}

pub(super) fn choices() -> Vec<(usize, Vec<String>)> {
    vec![
        (0, chart::TYPES.iter().map(|t| t.0.to_owned()).collect()),
        (2, LEGEND.map(str::to_owned).to_vec()),
    ]
}

/// Applies the dialog values to `base`.
pub(super) fn parse(values: &[String], base: &Chart) -> Result<Chart, String> {
    let [kind, title, legend, data] = values else {
        return Err("Invalid chart form".into());
    };
    let mut chart = base.clone();
    chart.set_type_label(kind)?;
    chart.title = title.trim().to_owned();
    chart.legend = match legend.as_str() {
        "Yes" => true,
        "No" => false,
        _ => return Err("Choose whether to show the legend".into()),
    };
    chart.set_grid(data)?;
    if !chart.valid() {
        return Err("Invalid chart data".into());
    }
    Ok(chart)
}
