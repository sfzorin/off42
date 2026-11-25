//! Constants exported from XLS42/FunctionsMath.swift; unit names are case sensitive.
use serde::Deserialize;
use std::sync::LazyLock;
#[derive(Deserialize)]
struct Catalog {
    units: Vec<(String, String, f64, bool)>,
    prefixes: Vec<(String, f64)>,
}
static CATALOG: LazyLock<Catalog> = LazyLock::new(|| {
    let mut catalog: Catalog =
        serde_json::from_str(include_str!("../../resources/unit-conversions.json"))
            .expect("valid bundled units");
    catalog
        .prefixes
        .sort_by(|a, b| b.0.len().cmp(&a.0.len()).then_with(|| a.0.cmp(&b.0)));
    catalog
});
fn resolve(unit: &str) -> Option<(&'static str, f64)> {
    let find = |name: &str| CATALOG.units.iter().find(|entry| entry.0 == name);
    if let Some((_, kind, factor, _)) = find(unit) {
        return Some((kind, *factor));
    }
    for (prefix, factor) in &CATALOG.prefixes {
        if let Some((_, kind, base, true)) = unit.strip_prefix(prefix).and_then(find) {
            if prefix.ends_with('i') && kind != "information" {
                continue;
            }
            let factor = if kind == "area" && unit.ends_with('2') {
                factor.powi(2)
            } else if kind == "volume" && unit.ends_with('3') {
                factor.powi(3)
            } else {
                *factor
            };
            return Some((kind, base * factor));
        }
    }
    None
}
fn temperature(unit: &str) -> Option<(f64, f64)> {
    match unit {
        "C" | "cel" => Some((1., 273.15)),
        "F" | "fah" => Some((5. / 9., 273.15 - 32. * 5. / 9.)),
        "K" | "kel" => Some((1., 0.)),
        "Rank" => Some((5. / 9., 0.)),
        "Reau" => Some((1.25, 273.15)),
        _ => CATALOG
            .prefixes
            .iter()
            .find(|(prefix, _)| {
                !prefix.ends_with('i')
                    && unit
                        .strip_prefix(prefix.as_str())
                        .is_some_and(|rest| matches!(rest, "K" | "kel"))
            })
            .map(|(_, factor)| (*factor, 0.)),
    }
}
pub(super) fn convert(value: f64, from: &str, to: &str) -> Option<f64> {
    if let (Some((factor_a, offset_a)), Some((factor_b, offset_b))) =
        (temperature(from), temperature(to))
    {
        return Some((value * factor_a + offset_a - offset_b) / factor_b);
    }
    let (kind_a, factor_a) = resolve(from)?;
    let (kind_b, factor_b) = resolve(to)?;
    (kind_a == kind_b).then(|| value * factor_a / factor_b)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shared_units_and_prefixes_preserve_dimensions_case_and_offsets() {
        assert_eq!(CATALOG.units.len(), 112);
        assert_eq!(CATALOG.prefixes.len(), 27);
        for (unit, _, _, _) in &CATALOG.units {
            assert_eq!(convert(1., unit, unit), Some(1.));
        }
        for (from, to, input, expected) in [
            ("in", "mm", 1., 25.4),
            ("flb", "J", 1., 1.3558179483314),
            ("m2", "cm2", 1., 10000.),
            ("cm3", "l", 1., 0.001),
            ("Mibyte", "bit", 1., 8388608.),
            ("C", "F", 100., 212.),
            ("mK", "kK", 1., 1e-6),
            ("mW", "W", 1000., 1.),
        ] {
            let actual = convert(input, from, to).unwrap();
            assert!(
                (actual - expected).abs() < 1e-10 * expected.abs().max(1.),
                "{from} -> {to}: {actual}"
            );
        }
        assert!(convert(1., "m", "g").is_none());
        assert!(convert(1., "MiW", "W").is_none());
        assert!(convert(1., "MM", "m").is_none());
        assert!(convert(1., "C", "m").is_none());
    }
}
