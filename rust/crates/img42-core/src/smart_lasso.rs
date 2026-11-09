use super::ImageDocument;

const MAX_SAMPLES: usize = 2048;
type Point = (f64, f64);
type Color = [f32; 3];

pub(super) fn refine(
    image: &ImageDocument,
    trace: &[(u32, u32)],
    radius: f64,
) -> Option<Vec<(u32, u32)>> {
    if !(3..=4096).contains(&trace.len()) || !(1.0..=256.0).contains(&radius) {
        return None;
    }
    let points: Vec<Point> = trace.iter().map(|&(x, y)| (x.into(), y.into())).collect();
    let step = (radius / 5.0).clamp(1.5, 3.0);
    let (center, spacing) = resample(&points, step)?;
    let count = center.len();
    let area: f64 = pairs(&center).map(|(a, b)| a.0 * b.1 - a.1 * b.0).sum();
    if area.abs() < 2.0 {
        return None;
    }
    let span = (radius / spacing).round().max(2.0) as usize;
    let span = span.min((count - 1) / 4);
    let normals: Vec<Point> = (0..count)
        .map(|i| {
            let a = center[(i + count - span) % count];
            let b = center[(i + span) % count];
            let tangent = (b.0 - a.0, b.1 - a.1);
            let norm = tangent.0.hypot(tangent.1).max(0.0001) * area.signum();
            (tangent.1 / norm, -tangent.0 / norm)
        })
        .collect();
    let choices = ((2.0 * radius).round() as usize + 1).clamp(9, 61);
    let delta = 2.0 * radius / (choices - 1) as f64;
    let offset = |j: i32| -radius + f64::from(j) * delta;
    let mut edge = Vec::with_capacity(count * choices);
    let mut region = Vec::with_capacity(count * choices);
    for (&point, &normal) in center.iter().zip(&normals) {
        let profile: Vec<Color> = (-1..=choices as i32)
            .map(|j| sample(image, shift(point, normal, offset(j))))
            .collect();
        let inner = mean(&profile[1..4]);
        let outer = mean(&profile[choices - 2..choices + 1]);
        let contrast = (distance(inner, outer) / 0.12).min(1.0);
        let values = &profile[1..choices + 1];
        let total_inner: f32 = values.iter().map(|&v| distance(v, inner)).sum();
        let mut remaining_outer: f32 = values.iter().map(|&v| distance(v, outer)).sum();
        let worst = total_inner.max(remaining_outer);
        let mut accumulated_inner = 0.0;
        for j in 0..choices {
            edge.push(distance(profile[j], profile[j + 2]));
            let penalty = accumulated_inner + remaining_outer;
            let gain = ((worst - penalty) / worst.max(f32::EPSILON)).max(0.0);
            region.push(gain * contrast);
            accumulated_inner += distance(values[j], inner);
            remaining_outer -= distance(values[j], outer);
        }
    }
    let mut magnitudes = edge.clone();
    let percentile = magnitudes.len() * 95 / 100;
    let (_, scale, _) = magnitudes.select_nth_unstable_by(percentile, f32::total_cmp);
    let scale = scale.max(0.04);
    let costs: Vec<f32> = edge
        .iter()
        .zip(region)
        .enumerate()
        .map(|(i, (edge, fit))| {
            let pull = (offset((i % choices) as i32).abs() / radius) as f32;
            1.4 - (edge / scale).min(1.2) - 0.6 * fit + 0.04 * pull
        })
        .collect();
    let open = path(&costs, count, choices, 0, None);
    let seam = count / 2;
    let closed = path(&costs, count, choices, seam, Some(open[seam]));
    let snapped: Vec<Point> = center
        .iter()
        .zip(normals)
        .zip(closed)
        .map(|((&point, normal), j)| shift(point, normal, offset(j as i32)))
        .collect();
    let mut outline: Vec<_> = (0..count)
        .map(|i| {
            let a = snapped[(i + count - 1) % count];
            let b = snapped[i];
            let c = snapped[(i + 1) % count];
            let x = ((a.0 + 2.0 * b.0 + c.0) * 0.25).round();
            let y = ((a.1 + 2.0 * b.1 + c.1) * 0.25).round();
            (
                x.clamp(0.0, f64::from(image.width() - 1)) as u32,
                y.clamp(0.0, f64::from(image.height() - 1)) as u32,
            )
        })
        .collect();
    outline.dedup();
    (outline.len() >= 3).then_some(outline)
}

fn pairs(points: &[Point]) -> impl Iterator<Item = (Point, Point)> + '_ {
    points
        .iter()
        .copied()
        .zip(points.iter().copied().cycle().skip(1))
}

fn resample(points: &[Point], step: f64) -> Option<(Vec<Point>, f64)> {
    let perimeter: f64 = pairs(points)
        .map(|(a, b)| (b.0 - a.0).hypot(b.1 - a.1))
        .sum();
    let count = ((perimeter / step).ceil() as usize).min(MAX_SAMPLES);
    if count < 8 {
        return None;
    }
    let spacing = perimeter / count as f64;
    let mut result = Vec::with_capacity(count);
    let mut traveled = 0.0;
    for (a, b) in pairs(points) {
        let length = (b.0 - a.0).hypot(b.1 - a.1);
        while result.len() < count && (result.len() as f64 * spacing) < traveled + length {
            let t = (result.len() as f64 * spacing - traveled) / length;
            result.push((a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t));
        }
        traveled += length;
    }
    (result.len() >= 8).then_some((result, spacing))
}

fn shift(point: Point, normal: Point, offset: f64) -> Point {
    (point.0 + normal.0 * offset, point.1 + normal.1 * offset)
}

fn sample(image: &ImageDocument, point: Point) -> Color {
    let x = (point.0 - 0.5).clamp(0.0, f64::from(image.width() - 1));
    let y = (point.1 - 0.5).clamp(0.0, f64::from(image.height() - 1));
    let x0 = x as u32;
    let y0 = y as u32;
    let x1 = (x0 + 1).min(image.width() - 1);
    let y1 = (y0 + 1).min(image.height() - 1);
    let dx = x.fract() as f32;
    let dy = y.fract() as f32;
    let mut color = [0.0; 3];
    for (x, y, weight) in [
        (x0, y0, (1.0 - dx) * (1.0 - dy)),
        (x1, y0, dx * (1.0 - dy)),
        (x0, y1, (1.0 - dx) * dy),
        (x1, y1, dx * dy),
    ] {
        let index = (y as usize * image.width() as usize + x as usize) * 4;
        let px = &image.pixels()[index..index + 4];
        let alpha = f32::from(px[3]) / 255.0;
        for (value, channel) in color.iter_mut().zip(px) {
            *value += weight * (1.0 - alpha + alpha * f32::from(*channel) / 255.0);
        }
    }
    color
}

fn distance(a: Color, b: Color) -> f32 {
    a.into_iter()
        .zip(b)
        .map(|(a, b)| (a - b).powi(2))
        .sum::<f32>()
        .sqrt()
}

fn mean(values: &[Color]) -> Color {
    std::array::from_fn(|channel| {
        values.iter().map(|color| color[channel]).sum::<f32>() / values.len() as f32
    })
}

fn path(
    costs: &[f32],
    count: usize,
    choices: usize,
    start: usize,
    anchor: Option<usize>,
) -> Vec<usize> {
    const STEP: usize = 2;
    const SMOOTHNESS: f32 = 0.12;
    let mut previous = costs[start * choices..(start + 1) * choices].to_vec();
    if let Some(anchor) = anchor {
        for (j, cost) in previous.iter_mut().enumerate() {
            if j != anchor {
                *cost = f32::INFINITY;
            }
        }
    }
    let mut next = vec![0.0; choices];
    let mut back = vec![0; count * choices];
    for step in 1..count {
        let row = ((start + step) % count) * choices;
        for j in 0..choices {
            let neighbors = j.saturating_sub(STEP)..=(j + STEP).min(choices - 1);
            let (from, best) = neighbors
                .map(|i| (i, previous[i] + SMOOTHNESS * i.abs_diff(j) as f32))
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .expect("path has at least one neighbor");
            next[j] = costs[row + j] + best;
            back[step * choices + j] = from as u8;
        }
        std::mem::swap(&mut previous, &mut next);
    }
    let mut column = (0..choices)
        .filter(|&j| anchor.is_none_or(|a| a.abs_diff(j) <= STEP))
        .min_by(|&a, &b| {
            let cost =
                |j: usize| previous[j] + anchor.map_or(0.0, |a| SMOOTHNESS * a.abs_diff(j) as f32);
            cost(a).total_cmp(&cost(b))
        })
        .expect("path has at least one endpoint");
    let mut result = vec![0; count];
    for step in (0..count).rev() {
        result[(start + step) % count] = column;
        column = usize::from(back[step * choices + column]);
    }
    result
}
