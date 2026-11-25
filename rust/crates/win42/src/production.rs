use crate::{InputRequest, LabelSource};
use label42::{Format, Label, Layout, MAX_LABELS};

pub(crate) fn sheet_request(source: &LabelSource) -> InputRequest {
    let mut fields = match source {
        LabelSource::Text(text) => vec![
            ("Label text", text.clone()),
            ("Barcode value (optional)", String::new()),
        ],
        LabelSource::Rows(rows) => {
            let columns = rows.first().map_or(0, |(_, values)| values.len());
            let defaults = (1..=columns.min(3))
                .map(|n| n.to_string())
                .collect::<Vec<_>>()
                .join(",");
            vec![
                ("Text columns (1 = first selected column)", defaults),
                (
                    "Code column, copies column, skip header (0 or 1)",
                    "0, 0, 0".into(),
                ),
            ]
        }
    };
    fields.extend([
        (
            "Barcode format (QR, Data Matrix, Code 128, Aztec)",
            "QR".into(),
        ),
        (
            "Width, height, horizontal gap, vertical gap (mm)",
            "70, 37, 0, 0".into(),
        ),
        ("Copies per label, first slot (top left = 1)", "1, 1".into()),
    ]);
    InputRequest {
        title: "Create Label Sheet",
        fields,
    }
}

pub(crate) fn barcode_request(value: String) -> InputRequest {
    InputRequest {
        title: "Create Barcode",
        fields: vec![
            ("Value", value),
            ("Format (QR, Data Matrix, Code 128, Aztec)", "QR".into()),
            ("Width (mm)", "35".into()),
        ],
    }
}

pub(crate) fn barcode_output(fields: &[String]) -> Result<Vec<u8>, String> {
    let [value, format, width] = fields else {
        return Err("Invalid barcode settings.".into());
    };
    let width = width
        .trim()
        .parse()
        .map_err(|_| "Enter a width in millimetres.")?;
    label42::barcode_png(Format::parse(format)?, value, width)
}

fn numbers<T: std::str::FromStr, const N: usize>(
    value: &str,
    label: &str,
) -> Result<[T; N], String> {
    let entries: Vec<_> = value
        .split(',')
        .map(|part| part.trim().parse())
        .collect::<Result<_, _>>()
        .map_err(|_| format!("Enter {N} comma-separated values for {label}."))?;
    entries
        .try_into()
        .map_err(|_| format!("Enter {N} comma-separated values for {label}."))
}

pub(crate) fn sheet_output(source: LabelSource, fields: &[String]) -> Result<Vec<u8>, String> {
    let [first, second, format, size, print] = fields else {
        return Err("Invalid label settings.".into());
    };
    let [width, height, horizontal_gap, vertical_gap] = numbers(size, "label size")?;
    let [copies, first_slot] = numbers(print, "copies and first slot")?;
    let layout = Layout {
        width,
        height,
        horizontal_gap,
        vertical_gap,
        first_slot,
    };
    let labels = match source {
        LabelSource::Text(_) => vec![Label {
            text: first.trim().into(),
            code: (!second.is_empty()).then(|| second.clone()),
            copies,
        }],
        LabelSource::Rows(rows) => {
            let text_columns: Vec<usize> = if first.trim().is_empty() {
                vec![]
            } else {
                first
                    .split(',')
                    .map(|n| n.trim().parse())
                    .collect::<Result<_, _>>()
                    .map_err(|_| "Enter comma-separated text column numbers.")?
            };
            let [code_col, copies_col, header] =
                numbers::<usize, 3>(second, "code, copies and header")?;
            if header > 1 || text_columns.len() > 6 || (text_columns.is_empty() && code_col == 0) {
                return Err("Select up to six text columns, a code column, or both. Use 0 or 1 for the header.".into());
            }
            rows.into_iter()
                .skip(header)
                .map(|(row, values)| {
                    let field = |col: usize| {
                        col.checked_sub(1)
                            .and_then(|i| values.get(i))
                            .ok_or_else(|| {
                                format!("Column {col} is outside the selection in row {row}.")
                            })
                    };
                    let text = text_columns
                        .iter()
                        .map(|&col| field(col).map(String::as_str))
                        .collect::<Result<Vec<_>, _>>()?
                        .join("\n");
                    let code = (code_col != 0)
                        .then(|| field(code_col).cloned())
                        .transpose()?;
                    let copies = if copies_col == 0 {
                        copies
                    } else {
                        field(copies_col)?
                            .trim()
                            .parse()
                            .ok()
                            .filter(|n| (1..=MAX_LABELS).contains(n))
                            .ok_or_else(|| {
                                format!("Row {row} needs a whole copy count between 1 and 5,000.")
                            })?
                    };
                    Ok(Label { text, code, copies })
                })
                .collect::<Result<Vec<_>, String>>()?
        }
    };
    let format = labels
        .iter()
        .any(|label| label.code.is_some())
        .then(|| Format::parse(format))
        .transpose()?;
    label42::sheet_pdf(layout, format, &labels)
}
