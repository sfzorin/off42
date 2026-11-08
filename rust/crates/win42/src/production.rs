use crate::{InputRequest, LabelSource};
use label42::{Format, Label, Stock, MAX_LABELS};

pub(crate) fn sheet_request(source: &LabelSource) -> Result<InputRequest,String> {
    if let LabelSource::Unavailable(error)=source {return Err(error.clone());}
    if let LabelSource::PdfArtwork{region,..}=source {
        return Ok(InputRequest{title:"Create Artwork Label Sheet",fields:vec![
            ("Optional region: left, bottom, width, height (PDF points)",region.map(|r|format!("{}, {}, {}, {}",r[0],r[1],r[2]-r[0],r[3]-r[1])).unwrap_or_default()),
            ("Stock name or width, height, gaps (mm)","70, 37, 0, 0".into()),
            ("Copies, first slot (top left = 1)","1, 1".into())]});
    }
    let mut fields = match source {
        LabelSource::Text(text) => vec![
            ("Label text", text.clone()),
            ("Barcode value (optional)", String::new()),
        ],
        LabelSource::PdfArtwork{..}|LabelSource::Unavailable(_)=>unreachable!(),
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
            "Stock name or width, height, gaps (mm)",
            "70, 37, 0, 0".into(),
        ),
        ("Copies per label, first slot (top left = 1)", "1, 1".into()),
    ]);
    Ok(InputRequest {
        title: "Create Label Sheet",
        fields,
    })
}

pub(crate) fn barcode_choices() -> Vec<(usize, Vec<String>)> {
    vec![(
        1,
        ["QR", "Data Matrix", "Code 128", "Aztec"]
            .into_iter()
            .map(str::to_owned)
            .collect(),
    )]
}

pub(crate) fn sheet_choices() -> Result<Vec<(usize, Vec<String>)>, String> {
    Ok(vec![
        (
            2,
            ["QR", "Data Matrix", "Code 128", "Aztec"]
                .into_iter()
                .map(str::to_owned)
                .collect(),
        ),
        (
            3,
            Stock::catalog()?
                .iter()
                .map(|stock| stock.name.clone())
                .collect(),
        ),
    ])
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

fn stock_from_text(size:&str)->Result<Stock,String> {
    let stock = if let Some(stock) = Stock::catalog()?.iter().find(|stock| {
        stock.id.eq_ignore_ascii_case(size.trim()) || stock.name.eq_ignore_ascii_case(size.trim())
    }) {
        stock.clone()
    } else {
        let [width, height, horizontal_gap, vertical_gap] = numbers(size, "label size")?;
        Stock::custom(width, height, horizontal_gap, vertical_gap)?
    };
    Ok(stock)
}
#[cfg(windows)]
pub(crate) fn artwork_choices()->Result<Vec<(usize,Vec<String>)>,String>{
    Ok(vec![(1,Stock::catalog()?.iter().map(|stock|stock.name.clone()).collect())])
}

pub(crate) fn sheet_output(source: &LabelSource, fields: &[String]) -> Result<Vec<u8>, String> {
    if let LabelSource::Unavailable(error)=source {return Err(error.clone());}
    if let LabelSource::PdfArtwork{bytes,region}=source {
        let [area,size,print]=fields else{return Err("Invalid artwork label settings".into());};
        let selected=if area.trim().is_empty(){*region}else{let [x,y,w,h]=numbers::<f64,4>(area,"PDF artwork region")?;if ![x,y,w,h].iter().all(|v|v.is_finite())||w<=0.||h<=0.{return Err("Choose a positive PDF artwork region".into());}Some([x,y,x+w,y+h])};
        let stock=stock_from_text(size)?;let[copies,first_slot]=numbers(print,"copies and first slot")?;
        return label42::sheet_pdf_artwork_stock(&stock,first_slot,copies,bytes,selected);
    }
    let [first, second, format, size, print] = fields else {
        return Err("Invalid label settings.".into());
    };
    let stock=stock_from_text(size)?;
    let [copies, first_slot] = numbers(print, "copies and first slot")?;
    let labels = match source {
        LabelSource::PdfArtwork{..}|LabelSource::Unavailable(_)=>unreachable!(),
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
    label42::sheet_pdf_stock(&stock, first_slot, format, &labels)
}

pub(crate) fn technical_request() -> InputRequest {
    InputRequest {
        title: "Symbol or Code",
        fields: vec![
            ("Symbol or barcode format", "QR".into()),
            ("Barcode value", String::new()),
            ("Width (mm)", "35".into()),
            ("Caption (yes/no)", "yes".into()),
        ],
    }
}
pub(crate) fn edit_technical_request(element: &label42::TechnicalElement) -> InputRequest {
    let kind = element
        .symbol_id
        .as_ref()
        .and_then(|id| label42::symbols().iter().find(|symbol| symbol.id == *id))
        .map(|symbol| symbol.title)
        .unwrap_or(match element.code_type {
            Format::Qr => "QR",
            Format::DataMatrix => "Data Matrix",
            Format::Code128 => "Code 128",
            Format::Aztec => "Aztec",
        });
    let mut request = technical_request();
    request.fields[0].1 = kind.into();
    request.fields[1].1 = element.value.clone();
    request.fields[2].1 = format!("{:.3}", element.width_mm);
    request.fields[3].1 = if element.caption { "yes" } else { "no" }.into();
    request
}

pub(crate) fn technical_choices() -> Vec<(usize, Vec<String>)> {
    let mut types = vec![
        "QR".into(),
        "Data Matrix".into(),
        "Code 128".into(),
        "Aztec".into(),
    ];
    types.extend(
        label42::symbols()
            .iter()
            .map(|symbol| symbol.title.to_owned()),
    );
    vec![(0, types), (3, vec!["yes".into(), "no".into()])]
}

pub(crate) fn technical_element(fields: &[String]) -> Result<label42::TechnicalElement, String> {
    let [kind, value, width, caption] = fields else {
        return Err("Invalid technical element settings.".into());
    };
    let symbol = label42::symbols().iter().find(|symbol| {
        symbol.id.eq_ignore_ascii_case(kind.trim())
            || symbol.title.eq_ignore_ascii_case(kind.trim())
    });
    let element = label42::TechnicalElement {
        symbol_id: symbol.map(|symbol| symbol.id.to_owned()),
        code_type: if symbol.is_some() {
            Format::Qr
        } else {
            Format::parse(kind)?
        },
        value: value.clone(),
        width_mm: width
            .trim()
            .parse()
            .map_err(|_| "Enter a width in millimetres.")?,
        caption: match caption.trim().to_ascii_lowercase().as_str() {
            "yes" => true,
            "no" => false,
            _ => return Err("Choose yes or no for the caption.".into()),
        },
    };
    element.validate()?;
    Ok(element)
}
