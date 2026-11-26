use crate::InputRequest;
use label42::{Format, Label, Stock, MAX_LABELS};
/// Content offered to the label sheet generator.
pub enum LabelSource {
    Text(String),
    /// Keep other source choices usable if an individual artwork capture fails.
    Unavailable(String),
    /// Independent single-page PDF; optional region is in its unrotated PDF coordinates.
    PdfArtwork { bytes:Vec<u8>, region:Option<[f64;4]> },
    /// Encoded PNG/JPEG; decode only the source actually selected for label generation.
    PictureArtwork { bytes:Vec<u8>, width_points:f64, height_points:f64 },
    /// Original one-based row number and rendered values from the selected columns.
    Rows(Vec<(usize, Vec<String>)>),
}

pub fn sheet_request(source: &LabelSource) -> Result<InputRequest,String> {
    if let LabelSource::Unavailable(error)=source {return Err(error.clone());}
    if let LabelSource::PictureArtwork{..}=source {return sheet_request(&LabelSource::PdfArtwork{bytes:Vec::new(),region:None});}
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
        LabelSource::PdfArtwork{..}|LabelSource::PictureArtwork{..}|LabelSource::Unavailable(_)=>unreachable!(),
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

pub fn barcode_choices() -> Vec<(usize, Vec<String>)> {
    vec![(
        1,
        ["QR", "Data Matrix", "Code 128", "Aztec"]
            .into_iter()
            .map(str::to_owned)
            .collect(),
    )]
}

pub fn sheet_choices() -> Result<Vec<(usize, Vec<String>)>, String> {
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

pub fn barcode_request(value: String) -> InputRequest {
    InputRequest {
        title: "Create Barcode",
        fields: vec![
            ("Value", value),
            ("Format (QR, Data Matrix, Code 128, Aztec)", "QR".into()),
            ("Width (mm)", "35".into()),
        ],
    }
}

pub fn barcode_output(fields: &[String]) -> Result<Vec<u8>, String> {
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
pub fn artwork_choices()->Result<Vec<(usize,Vec<String>)>,String>{
    Ok(vec![(1,Stock::catalog()?.iter().map(|stock|stock.name.clone()).collect())])
}

pub fn sheet_output(source: &LabelSource, fields: &[String]) -> Result<Vec<u8>, String> {
    if let LabelSource::Unavailable(error)=source {return Err(error.clone());}
    if let LabelSource::PictureArtwork{bytes,width_points,height_points}=source {
        if bytes.len()>16*1024*1024{return Err("Picture artwork exceeds 16 MB".into());}
        let reader=image::ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format().map_err(|e|e.to_string())?;
        if !matches!(reader.format(),Some(image::ImageFormat::Png|image::ImageFormat::Jpeg)){return Err("Picture labels support PNG or JPEG".into());}
        let (width,height)=reader.into_dimensions().map_err(|e|e.to_string())?;
        if width==0||height==0||u64::from(width)*u64::from(height)>16_000_000{return Err("Picture artwork exceeds 16 million pixels".into());}
        let mut reader=image::ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format().map_err(|e|e.to_string())?;
        let mut limits=image::Limits::default();limits.max_alloc=Some(128*1024*1024);reader.limits(limits);
        let pixels=reader.decode().map_err(|e|e.to_string())?.to_rgba8();
        let pdf=label42::bitmap_artwork_pdf(width,height,pixels.as_raw(),(*width_points,*height_points))?;
        return sheet_output(&LabelSource::PdfArtwork{bytes:pdf,region:None},fields);
    }

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
        LabelSource::PdfArtwork{..}|LabelSource::PictureArtwork{..}|LabelSource::Unavailable(_)=>unreachable!(),
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

pub fn technical_request() -> InputRequest {
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
pub fn edit_technical_request(element: &label42::TechnicalElement) -> InputRequest {
    let kind = element
        .symbol_id
        .as_ref()
        .and_then(|id| label42::symbols().iter().find(|symbol| symbol.id == *id))
        .map(mark_label)
        .unwrap_or_else(|| match element.code_type {
            Format::Qr => "QR",
            Format::DataMatrix => "Data Matrix",
            Format::Code128 => "Code 128",
            Format::Aztec => "Aztec",
        }.into());
    let mut request = technical_request();
    request.fields[0].1 = kind.into();
    request.fields[1].1 = element.value.clone();
    request.fields[2].1 = format!("{:.3}", element.width_mm);
    request.fields[3].1 = if element.caption { "yes" } else { "no" }.into();
    request
}

fn language() -> &'static str {
    crate::localization::current().code
}

/// A mark as shown in the format list: its name in the interface language and its region, e.g. "CE — EU".
pub fn mark_label(symbol: &label42::TechnicalSymbol) -> String {
    symbol.name_in(language())
}

/// The mark chosen in the format field (by displayed name, English name or file id).
pub fn find_mark(kind: &str) -> Option<&'static label42::TechnicalSymbol> {
    let kind = kind.trim();
    label42::symbols().iter().find(|symbol| {
        symbol.id.eq_ignore_ascii_case(kind) || symbol.title.eq_ignore_ascii_case(kind) || mark_label(symbol) == kind
    })
}

/// What the mark is for and its conditions of use, shown before inserting it (as under the macOS preview).
/// Informs only: every mark can be inserted. `None` for barcodes.
pub fn mark_notice(element: &label42::TechnicalElement) -> Option<String> {
    let symbol = element.symbol_id.as_deref().and_then(|id| label42::symbols().iter().find(|s| s.id == id))?;
    let t = crate::localization::text;
    let mut conditions = Vec::new();
    if symbol.unverified {
        conditions.push(t("Unverified artwork: compare it with the official mark before use."));
    }
    if symbol.legal == "trademark" {
        conditions.push(t("Trademark or certification mark: use only with the owner's authorization for this product."));
    }
    if let Some(warning) = symbol.warning_in(language()) {
        conditions.push(warning);
    }
    if symbol.needs_number {
        conditions.push(t("Add the required registration or licence number next to the mark."));
    }
    if conditions.is_empty() {
        return None;
    }
    let purpose = symbol.purpose_in(language());
    Some(format!("{}\n\n{}\n\n{}", mark_label(symbol), purpose, conditions.join("\n")).replace("\n\n\n\n", "\n\n"))
}

pub fn technical_choices() -> Vec<(usize, Vec<String>)> {
    let mut types = vec![
        "QR".into(),
        "Data Matrix".into(),
        "Code 128".into(),
        "Aztec".into(),
    ];
    // Marks follow the codes, grouped in the macOS category order, named in the interface language.
    for category in label42::CATEGORIES {
        types.extend(label42::symbols().iter().filter(|s| s.category == category).map(mark_label));
    }
    vec![(0, types), (3, vec!["yes".into(), "no".into()])]
}

pub fn technical_element(fields: &[String]) -> Result<label42::TechnicalElement, String> {
    let [kind, value, width, caption] = fields else {
        return Err("Invalid technical element settings.".into());
    };
    let symbol = find_mark(kind);
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

#[cfg(test)]
mod mark_choice_tests {
    use super::*;

    #[test]
    fn marks_are_listed_after_codes_and_resolve_by_name_title_or_id() {
        let choices = technical_choices();
        let types = &choices[0].1;
        assert_eq!(&types[..4], ["QR", "Data Matrix", "Code 128", "Aztec"]);
        assert_eq!(types.len(), 4 + label42::symbols().len());
        assert!(types.contains(&"CE — EU".to_owned()) && types.contains(&"Protective earth".to_owned()));
        for kind in ["CE — EU", "ce", "Protective earth", "protective-earth"] {
            assert!(find_mark(kind).is_some(), "{kind}");
        }
        let element = technical_element(&["CE — EU".into(), String::new(), "20".into(), "no".into()]).unwrap();
        assert_eq!(element.symbol_id.as_deref(), Some("ce"));
    }

    #[test]
    fn conformity_and_unverified_marks_show_conditions_but_plain_symbols_and_codes_do_not() {
        let mark = |id: &str| label42::TechnicalElement { symbol_id: Some(id.into()), code_type: Format::Qr, value: String::new(), width_mm: 20.0, caption: false };
        let ce = mark_notice(&mark("ce")).unwrap();
        assert!(ce.starts_with("CE — EU") && ce.contains("conformity assessment"), "{ce}");
        assert!(mark_notice(&mark("iso-9001")).unwrap().contains("Unverified artwork"));
        assert!(mark_notice(&mark("bis")).unwrap().contains("licence number"));
        let qr = label42::TechnicalElement { symbol_id: None, code_type: Format::Qr, value: "A".into(), width_mm: 20.0, caption: false };
        assert!(mark_notice(&qr).is_none());
    }
}

#[cfg(test)]
mod picture_artwork_tests {
 use super::*;
 #[test]
 fn decodes_only_the_selected_picture_and_rejects_invalid_artwork_before_output(){
  use image::ImageEncoder;
  let mut png=Vec::new();image::codecs::png::PngEncoder::new(&mut png).write_image(&[255,0,0,255,0,0,255,128],2,1,image::ExtendedColorType::Rgba8).unwrap();
  let source=LabelSource::PictureArtwork{bytes:png,width_points:120.,height_points:60.};
  assert_eq!(sheet_request(&source).unwrap().fields.len(),3);
  let fields=["","70, 37, 0, 0","2, 24"].map(str::to_string);let output=sheet_output(&source,&fields).unwrap();assert!(output.starts_with(b"%PDF-"));
  let bad=LabelSource::PictureArtwork{bytes:b"not a picture".to_vec(),width_points:120.,height_points:60.};assert!(sheet_output(&bad,&fields).is_err());
  let unavailable=LabelSource::Unavailable("Capture unavailable".into());assert!(sheet_request(&unavailable).is_err());
 }
}
