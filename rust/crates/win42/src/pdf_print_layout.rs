//! Physical PDF placement shared by native print modes; calibration never changes document data.
use crate::print_calibration::PrintCorrection;
pub(super) fn destination(
    points: (f64, f64),
    paper: (i32, i32),
    dpi: (i32, i32),
    margins: (i32, i32),
    fit: bool,
    correction: PrintCorrection,
) -> Result<[i32; 4], String> {
    correction.validate()?;
    if ![points.0, points.1]
        .iter()
        .all(|v| v.is_finite() && *v > 0.)
        || paper.0 <= 0
        || paper.1 <= 0
        || dpi.0 <= 0
        || dpi.1 <= 0
        || margins.0 < 0
        || margins.1 < 0
        || margins.0 >= paper.0
        || margins.1 >= paper.1
    {
        return Err("Invalid PDF print geometry".into());
    }
    let content = (
        points.0 * f64::from(dpi.0) / 72.,
        points.1 * f64::from(dpi.1) / 72.,
    );
    let (factor, origin) = if fit {
        let factor = 1f64
            .min(f64::from(paper.0) / content.0)
            .min(f64::from(paper.1) / content.1);
        (
            factor,
            (
                ((f64::from(paper.0) - content.0 * factor) / 2.).round() as i32 - margins.0,
                ((f64::from(paper.1) - content.1 * factor) / 2.).round() as i32 - margins.1,
            ),
        )
    } else {
        (1., (-margins.0, -margins.1))
    };
    let origin = correction.device_origin(origin, margins, dpi);
    let (width, height) = (
        (content.0 * factor * correction.scale_x).round(),
        (content.1 * factor * correction.scale_y).round(),
    );
    if ![width, height]
        .iter()
        .all(|v| v.is_finite() && *v >= 1. && *v <= f64::from(i32::MAX))
    {
        return Err("PDF print dimensions exceed the printer limit".into());
    }
    Ok([origin.0, origin.1, width as i32, height as i32])
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn physical_fit_centers_without_upscaling_and_actual_size_keeps_the_origin() {
        let correction = PrintCorrection::default();
        assert_eq!(
            destination(
                (200., 100.),
                (1200, 1800),
                (144, 144),
                (10, 20),
                true,
                correction
            )
            .unwrap(),
            [390, 780, 400, 200]
        );
        assert_eq!(
            destination(
                (200., 100.),
                (1200, 1800),
                (144, 144),
                (10, 20),
                false,
                correction
            )
            .unwrap(),
            [-10, -20, 400, 200]
        );
        // A 10-by-5-inch source shrinks to 8.5 inches on Letter, with independent X/Y DPI.
        assert_eq!(
            destination(
                (720., 360.),
                (5100, 13200),
                (600, 1200),
                (120, 180),
                true,
                correction
            )
            .unwrap(),
            [-120, 3870, 5100, 5100]
        );
        assert_eq!(
            destination(
                (720., 360.),
                (5100, 13200),
                (600, 1200),
                (120, 180),
                false,
                correction
            )
            .unwrap(),
            [-120, -180, 6000, 6000]
        );
    }
    #[test]
    fn calibration_acts_on_physical_positions_and_both_axes_after_fitting() {
        let correction = PrintCorrection {
            offset_x: 2.54,
            offset_y: -2.54,
            scale_x: 1.02,
            scale_y: 0.98,
        };
        assert_eq!(
            destination(
                (200., 100.),
                (1200, 1800),
                (144, 144),
                (10, 20),
                true,
                correction
            )
            .unwrap(),
            [412, 750, 408, 196]
        );
        assert!(destination(
            (f64::NAN, 1.),
            (100, 100),
            (96, 96),
            (0, 0),
            true,
            PrintCorrection::default()
        )
        .is_err());
        assert!(destination(
            (100., 100.),
            (100, 100),
            (0, 96),
            (0, 0),
            true,
            PrintCorrection::default()
        )
        .is_err());
    }
}
