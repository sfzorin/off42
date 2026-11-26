use super::*;
use crate::InkColor;

/// A synthetic signature picture (600 × 360, so tracing works at its own size, as on macOS): a ring (a closed loop),
/// a slanted bar crossing an underline. `v` shifts the parts a little for a second, compatible sample. Integer
/// geometry, so Swift and Rust see the same pixels.
fn inked(x: i64, y: i64, v: i64) -> bool {
    let (cx, cy) = (150 + 4 * v, 180 - 2 * v);
    let r2 = (x - cx) * (x - cx) + (y - cy) * (y - cy);
    if (70 * 70..=78 * 78).contains(&r2) {
        return true;
    }
    let (x0, y0, x1, y1) = (300 + 4 * v, 340 - 2 * v, 480 - 4 * v, 60 + 4 * v);
    let (dx, dy) = (x1 - x0, y1 - y0);
    let len2 = dx * dx + dy * dy;
    let (vx, vy) = (x - x0, y - y0);
    let (cross, dot) = (vx * dy - vy * dx, vx * dx + vy * dy);
    if (0..=len2).contains(&dot) && 4 * cross * cross <= 81 * len2 {
        return true;
    }
    (100..=520).contains(&x) && (316..=324).contains(&y)
}

fn picture(v: i64) -> RgbaImage {
    RgbaImage::from_fn(600, 360, |x, y| if inked(x as i64, y as i64, v) { image::Rgba([20, 20, 20, 255]) } else { image::Rgba([255; 4]) })
}

fn black() -> Ink {
    InkColor::Black.pen()
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6
}

/// Values printed by Kit42 (`SignatureLearning.trace`, `LearnedSignature.render(…capture:)`) for the same pictures,
/// samples and impressions: tracing, via-points, blending and every random draw follow the Swift code.
#[test]
fn matches_kit42_reference_values() {
    type Ends = [[f64; 3]; 2];
    let expect_trace = |s: &SignatureSample, counts: &[usize], pen: f64, ends: &[Ends]| {
        assert_eq!(s.strokes.iter().map(|s| s.len()).collect::<Vec<_>>(), counts);
        assert!(close(s.pen_width.unwrap(), pen), "{:?}", s.pen_width);
        for (stroke, e) in s.strokes.iter().zip(ends) {
            for (p, e) in [stroke.first().unwrap(), stroke.last().unwrap()].iter().zip(e) {
                assert!(close(p[0], e[0]) && close(p[1], e[1]) && close(p[4], e[2]), "{p:?} vs {e:?}");
            }
        }
    };
    let a = trace(&picture(0)).unwrap();
    expect_trace(
        &a,
        &[417, 489, 277],
        0.03285106382978723,
        &[
            [[103.93489361702127, 39.0, 10.0], [515.5651063829787, 39.0, 10.0]],
            [[157.76895709801187, 255.25764213463552, 4.0], [129.9348936170213, 253.0, 4.0]],
            [[308.0376411026424, 24.77111126278038, 2.828], [477.77849779933774, 295.74025742505034, 8.484]],
        ],
    );
    let b = trace(&picture(1)).unwrap();
    expect_trace(
        &b,
        &[417, 489, 273],
        0.03528060555042448,
        &[
            [[103.92771599657827, 39.0, 10.0], [515.5722840034217, 39.0, 10.0]],
            [[161.77601705254597, 257.25893645963345, 4.0], [133.92771599657829, 255.0, 4.0]],
            [[309.7816994889292, 23.873044447890834, 2.828], [473.48108732971883, 291.3449148564164, 8.484]],
        ],
    );

    let expect_copy = |learned: &LearnedSignature, seed: u64, counts: &[usize], ends: &[[f64; 3]]| {
        let (_, trajectories) = learned.redraw(seed, black(), Pen::Ballpoint, 120.0).unwrap();
        assert_eq!(trajectories.iter().map(|t| t.len()).collect::<Vec<_>>(), counts, "seed {seed}");
        for (t, e) in trajectories.iter().zip(ends) {
            let last = t.last().unwrap();
            assert!(close(last.p.x, e[0]) && close(last.p.y, e[1]) && close(last.t, e[2]), "{:?} vs {e:?}", last);
        }
    };
    let pair = LearnedSignature::new(vec![a.clone(), b.clone()]);
    expect_copy(
        &pair,
        7,
        &[54, 162, 40],
        &[[203.38731700950638, 19.900816971298596, 0.1676000000000001], [14.518144572423974, 104.35285014869231, 0.4201000000000003], [170.0990754468744, 129.68352532748105, 0.13760000000000008]],
    );
    expect_copy(
        &pair,
        8,
        &[56, 160, 55],
        &[[192.88673275400095, 13.154115122115588, 0.17260000000000011], [23.217258949884798, 103.36210453544479, 0.4176000000000003], [173.5604934249771, 122.6564052545838, 0.1551000000000001]],
    );
    expect_copy(
        &LearnedSignature::new(vec![a]),
        3,
        &[47, 163, 60],
        &[[205.42524332562306, 14.879546976558991, 0.1576000000000001], [21.906897473706394, 109.44516354631224, 0.4201000000000003], [182.20553934272016, 137.68931311443032, 0.1676000000000001]],
    );
    let timed = SignatureSample::new(vec![
        vec![vec![0.0, 0.0, 0.0], vec![0.3, 0.5, 0.1], vec![0.6, 0.1, 0.2], vec![1.0, 0.4, 0.3]],
        vec![vec![0.2, -0.2, 0.5], vec![0.9, -0.15, 0.7]],
    ]);
    expect_copy(
        &LearnedSignature::new(vec![timed]),
        11,
        &[135, 53],
        &[[178.4056742543242, 133.92972470096734, 0.37760000000000027], [173.023661194746, 16.738390325015743, 0.1601000000000001]],
    );

    // `[0, 1, 2, 3, 4].randomElement(using:)` with SeededRandom(5).
    let mut r = SeededRandom::new(5);
    assert_eq!((0..6).map(|_| swift_index(&mut r, 5)).collect::<Vec<_>>(), [3, 1, 0, 0, 1, 4]);
}

#[test]
fn traces_a_synthetic_stroke_into_one_centre_line() {
    // One thick diagonal stroke, small enough to be enlarged for tracing.
    let image = RgbaImage::from_fn(200, 90, |x, y| {
        let (x, y) = (x as f64, y as f64);
        let on = (20.0..=180.0).contains(&x) && (y - (75.0 - (x - 20.0) * 0.375)).abs() <= 3.0;
        image::Rgba(if on { [10, 10, 60, 255] } else { [250, 248, 240, 255] })
    });
    let s = SignatureSample::from_picture(&image).expect("traced");
    assert_eq!(s.strokes.len(), 1, "one stroke");
    assert!(s.picture.as_ref().is_some_and(|p| p.dimensions() == (200, 90)));
    let stroke = &s.strokes[0];
    let (first, last) = (stroke.first().unwrap(), stroke.last().unwrap());
    // Written left to right and rising (y up): the work scale is 4 (360 / 90).
    assert!(first[0] < last[0] && first[1] < last[1], "{first:?} {last:?}");
    assert!((first[0] - 80.0).abs() < 16.0 && (last[0] - 720.0).abs() < 16.0, "ends reach the ink: {first:?} {last:?}");
    let rise = (last[1] - first[1]) / (last[0] - first[0]);
    assert!((rise - 0.375).abs() < 0.03, "slope {rise}");
    // A 7 px line at 4×: about 28 px wide, as a share of the 240 px tall trace.
    let pen = s.pen_width.unwrap();
    assert!((0.08..0.16).contains(&pen), "pen {pen}");
    // Via-points of a straight line: a few, all on the line (normalized: x/h, y/h with h the stroke's height).
    let n = normalized_sample(&s).unwrap();
    let points = via(&n.strokes[0], 0.012).points;
    assert!((2..=6).contains(&points.len()), "{points:?}");
    let (p0, p1) = (points[0], points[points.len() - 1]);
    for p in &points {
        let off = ((p.x - p0.x) * (p1.y - p0.y) - (p.y - p0.y) * (p1.x - p0.x)).abs() / (p1.x - p0.x).hypot(p1.y - p0.y);
        assert!(off < 0.05, "{p:?} off the line by {off}");
    }
    // Blank paper or a speck is not a signature.
    assert!(trace(&RgbaImage::from_pixel(120, 60, image::Rgba([255; 4]))).is_none());
    assert!(SignatureSample::from_picture(&RgbaImage::new(0, 0)).is_none());
}

#[test]
fn copies_are_deterministic_and_vary_between_impressions() {
    let samples: Vec<SignatureSample> = (0..3).map(|v| SignatureSample::from_picture(&picture(v)).unwrap()).collect();
    let learned = LearnedSignature::new(samples.clone());
    // From the pictures' ink.
    let a = learned.render(1, black(), Pen::Gel, 130.0).unwrap();
    assert_eq!(a, learned.render(1, black(), Pen::Gel, 130.0).unwrap());
    assert_ne!(a, learned.render(2, black(), Pen::Gel, 130.0).unwrap());
    assert!(a.pixels().filter(|p| p.0[3] > 128).count() > 2_000);
    // Redrawn from the traced strokes, blending compatible samples.
    let traced = LearnedSignature::new(samples.iter().map(SignatureSample::traced_only).collect());
    let c = traced.render(5, black(), Pen::Fountain, 130.0).unwrap();
    assert_eq!(c, traced.render(5, black(), Pen::Fountain, 130.0).unwrap());
    assert_ne!(c, traced.render(6, black(), Pen::Fountain, 130.0).unwrap());
    assert!(c.height() > 100 && c.pixels().filter(|p| p.0[3] > 128).count() > 1_000);
}

#[test]
fn blending_mixes_two_similar_samples_only() {
    let n = |v| normalized_sample(&trace(&picture(v)).unwrap()).unwrap().strokes;
    let (a, b) = (n(0), n(1));
    assert!(similar(&a, &b));
    let mut short = b.clone();
    let third = short[0].len() / 3;
    short[0].truncate(third);
    assert!(!similar(&a, &short));
    let r = resample(&a[0], 50);
    assert_eq!(r.len(), 50);
    assert!(close(r[0].p.x, a[0][0].p.x) && close(r[49].p.y, a[0].last().unwrap().p.y));
}

#[test]
fn invalid_samples_are_rejected() {
    let ok = SignatureSample::new(vec![vec![vec![0.0, 0.0], vec![1.0, 1.0]]]);
    assert!(normalized_sample(&ok).is_some());
    for bad in [
        SignatureSample::new(vec![]),
        SignatureSample::new(vec![vec![vec![0.0]]]),
        SignatureSample::new(vec![vec![vec![0.0, f64::NAN], vec![1.0, 1.0]]]),
        SignatureSample { pen_width: Some(-1.0), ..ok.clone() },
    ] {
        assert!(normalized_sample(&bad).is_none(), "{bad:?}");
    }
    assert!(LearnedSignature::new(vec![]).render(1, black(), Pen::Ballpoint, 130.0).is_none());
    assert!(LearnedSignature::new(vec![ok.clone(); MAXIMUM_SAMPLES + 1]).render(1, black(), Pen::Ballpoint, 130.0).is_none());
    assert!(LearnedSignature::new(vec![ok]).render(1, black(), Pen::Ballpoint, f64::NAN).is_none());
}
