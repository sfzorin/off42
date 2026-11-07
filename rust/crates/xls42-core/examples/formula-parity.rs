//! Report reference-case coverage. Failures remain visible; this is not an all-pass test gate.
use serde::Deserialize;
use serde_json::json;
use xls42_core::Workbook;
#[derive(Deserialize)]
struct Fixture {
    cases: Vec<Case>,
}
#[derive(Deserialize)]
struct Case {
    formula: String,
    expected: Option<String>,
}
fn main() {
    let fixture: Fixture =
        serde_json::from_str(include_str!("../fixtures/macos-function-cases.json")).unwrap();
    let mut book = Workbook::new(fixture.cases.len().max(10), 24).unwrap();
    for (col, values) in [
        (10, vec!["3", "7", "2", "9", "4", "7", "1", "8", "5", "6"]),
        (
            11,
            vec![
                "apple", "Banana", "cherry", "apple", "date", "Elder", "fig", "grape", "apple",
                "kiwi",
            ],
        ),
        (
            12,
            vec![
                "2024-01-15",
                "2024-02-29",
                "2023-12-31",
                "2024-07-04",
                "2025-03-10",
                "2020-02-29",
                "2024-11-04",
                "2019-06-15",
                "2024-12-25",
                "2026-01-01",
            ],
        ),
        (
            13,
            vec![
                "2.1", "3.9", "6.2", "7.8", "10.1", "12.2", "13.8", "16.1", "18", "20.2",
            ],
        ),
        (14, vec!["1", "2", "3", "4", "5", "6", "7", "8", "9", "10"]),
        (15, vec!["-1000", "300", "400", "400", "300", "100"]),
        (
            16,
            vec![
                "2024-01-01",
                "2024-03-01",
                "2024-10-30",
                "2025-02-15",
                "2025-04-01",
                "2025-06-30",
            ],
        ),
        (17, vec!["TRUE", "text", "", "5", "=NA()", "0"]),
        (20, vec!["1", "2", "3", "4", "5"]),
        (21, vec!["5", "3", "8", "1", "9"]),
        (22, vec!["10", "12", "20", "14", "25"]),
        (23, vec!["2024-12-25", "2025-01-01", "2024-11-28"]),
    ] {
        for (row, value) in values.into_iter().enumerate() {
            book.set_cell(row, col, value.into()).unwrap();
        }
    }
    for (row, case) in fixture.cases.iter().enumerate() {
        book.set_cell(row, 0, case.formula.clone()).unwrap();
    }
    let grid = book.calculated_grid();
    let mut passed = 0;
    let mut skipped = 0;
    let mut cases = vec![];
    for (row, case) in fixture.cases.into_iter().enumerate() {
        let actual = &grid[row][0];
        let status = match &case.expected {
            None => {
                skipped += 1;
                "locale_or_volatile"
            }
            Some(expected)
                if actual == expected
                    || actual
                        .parse::<f64>()
                        .ok()
                        .zip(expected.parse::<f64>().ok())
                        .is_some_and(|(a, b)| {
                            a.is_finite()
                                && b.is_finite()
                                && (a - b).abs() <= 1e-8 * b.abs().max(1.)
                        }) =>
            {
                passed += 1;
                "pass"
            }
            _ => "gap",
        };
        cases.push(json!({"formula":case.formula,"expected":case.expected,"actual":actual,"status":status}));
    }
    println!("{}", serde_json::to_string_pretty(&json!({"passed":passed,"skipped":skipped,"gaps":cases.len()-passed-skipped,"cases":cases})).unwrap());
}
