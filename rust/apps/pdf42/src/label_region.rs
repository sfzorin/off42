//! Map a readonly PDF selection through crop bounds and page rotation.
#[cfg_attr(not(windows), allow(dead_code))]
pub(super) fn page_point(
    bounds: [f64; 4],
    rotation: i64,
    size: [u32; 2],
    p: [f64; 2],
) -> Result<[f64; 2], String> {
    if size.contains(&0)
        || p.iter().chain(bounds.iter()).any(|v| !v.is_finite())
        || bounds[2] <= bounds[0]
        || bounds[3] <= bounds[1]
    {
        return Err("Invalid PDF page geometry".into());
    }
    let u = (p[0] / f64::from(size[0])).clamp(0., 1.);
    let v = (p[1] / f64::from(size[1])).clamp(0., 1.);
    let (x, y) = match rotation {
        0 => (u, 1. - v),
        90 => (v, u),
        180 => (1. - u, v),
        270 => (1. - v, 1. - u),
        _ => return Err("Invalid page rotation".into()),
    };
    Ok([
        bounds[0] + x * (bounds[2] - bounds[0]),
        bounds[1] + y * (bounds[3] - bounds[1]),
    ])
}
#[cfg_attr(not(windows), allow(dead_code))]
pub(super) fn rectangle(a: [f64; 2], b: [f64; 2]) -> Result<[f64; 4], String> {
    if a.into_iter().chain(b).any(|v| !v.is_finite()) {
        return Err("Invalid PDF label area".into());
    }
    let area = [
        a[0].min(b[0]),
        a[1].min(b[1]),
        a[0].max(b[0]),
        a[1].max(b[1]),
    ];
    if area[2] - area[0] < 1. || area[3] - area[1] < 1. {
        return Err("Drag a larger area for labels".into());
    }
    Ok(area)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cropped_rotations_map_display_corners_and_clamp_outside_drags() {
        for (rotation, a, b) in [
            (0, [20., 820.], [575., 20.]),
            (90, [20., 20.], [575., 820.]),
            (180, [575., 20.], [20., 820.]),
            (270, [575., 820.], [20., 20.]),
        ] {
            let bounds = [20., 20., 575., 820.];
            let size = [800, 555];
            assert_eq!(page_point(bounds, rotation, size, [0., 0.]).unwrap(), a);
            assert_eq!(page_point(bounds, rotation, size, [800., 555.]).unwrap(), b);
            assert_eq!(page_point(bounds, rotation, size, [-50., -50.]).unwrap(), a);
            assert_eq!(page_point(bounds, rotation, size, [900., 900.]).unwrap(), b);
            assert_eq!(rectangle(a, b).unwrap(), bounds);
        }
        assert!(page_point([0., 0., 100., 100.], 45, [100, 100], [0., 0.]).is_err());
        assert!(page_point([0., 0., 100., 100.], 0, [0, 100], [0., 0.]).is_err());
    }
    #[test]
    fn reverse_drag_and_degenerate_areas() {
        assert_eq!(
            rectangle([180., 25.], [40., 90.]).unwrap(),
            [40., 25., 180., 90.]
        );
        assert_eq!(
            rectangle([40., 90.], [180., 25.]).unwrap(),
            [40., 25., 180., 90.]
        );
        assert!(rectangle([0., 0.], [10., 0.5]).is_err());
        assert!(rectangle([0., 0.], [f64::NAN, 10.]).is_err());
        assert!(rectangle([0., 0.], [10., f64::INFINITY]).is_err());
    }
}
