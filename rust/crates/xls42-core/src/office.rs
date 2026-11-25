//! XLSX export and conservative re-opening of unchanged XLS42-produced packages.
use super::*;
use rust_xlsxwriter::{Color, Format, FormatBorder, FormatUnderline, Formula, Workbook as Xlsx};
use sha2::{Digest, Sha256};
use std::io::{Cursor, Read, Write};
const EXTRA: &str = "xls42/project.json";
const LIMIT: usize = 64 * 1024 * 1024;
fn format(s: &CellStyle) -> Format {
    let mut f = Format::new();
    if s.bold {
        f = f.set_bold()
    }
    if s.italic {
        f = f.set_italic()
    }
    if s.underline {
        f = f.set_underline(FormatUnderline::Single)
    }
    if !s.font.is_empty() {
        f = f.set_font_name(&s.font)
    }
    if s.font_size > 0 {
        f = f.set_font_size(s.font_size)
    }
    if let Some(c) = s.foreground {
        f = f.set_font_color(Color::RGB(c & 0xffffff))
    }
    if let Some(c) = s.background {
        f = f.set_background_color(Color::RGB(c & 0xffffff))
    }
    if !s.number_format.is_empty() {
        f = f.set_num_format(&s.number_format)
    }
    if s.borders & 1 != 0 {
        f = f.set_border_top(FormatBorder::Thin)
    }
    if s.borders & 2 != 0 {
        f = f.set_border_right(FormatBorder::Thin)
    }
    if s.borders & 4 != 0 {
        f = f.set_border_bottom(FormatBorder::Thin)
    }
    if s.borders & 8 != 0 {
        f = f.set_border_left(FormatBorder::Thin)
    }
    f
}
fn digest(archive: &mut zip::ZipArchive<Cursor<&[u8]>>) -> Result<[u8; 32], Error> {
    let mut hash = Sha256::new();
    let mut total = 0usize;
    let mut names = std::collections::HashSet::new();
    for i in 0..archive.len() {
        let mut file = archive
            .by_index(i)
            .map_err(|_| Error::Invalid("invalid Office archive"))?;
        let name = file.name().to_string();
        if !names.insert(name.clone()) {
            return Err(Error::Invalid("duplicate Office part"));
        }
        if name == EXTRA {
            continue;
        }
        total = total
            .checked_add(file.size() as usize)
            .ok_or(Error::Limit)?;
        if total > LIMIT {
            return Err(Error::Limit);
        }
        hash.update((name.len() as u64).to_le_bytes());
        hash.update(name.as_bytes());
        hash.update(file.size().to_le_bytes());
        let mut buf = [0; 8192];
        loop {
            let n = file.read(&mut buf)?;
            if n == 0 {
                break;
            }
            hash.update(&buf[..n]);
        }
    }
    Ok(hash.finalize().into())
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Embedded {
    format: String,
    digest: [u8; 32],
    project: Vec<u8>,
}
fn embed(bytes: Vec<u8>, format: &str, project: Vec<u8>) -> Result<Vec<u8>, Error> {
    if bytes.len() > LIMIT {
        return Err(Error::Limit);
    }
    let sum = digest(
        &mut zip::ZipArchive::new(Cursor::new(bytes.as_slice()))
            .map_err(|_| Error::Invalid("invalid Office archive"))?,
    )?;
    let payload = serde_json::to_vec(&Embedded {
        format: format.into(),
        digest: sum,
        project,
    })
    .map_err(|_| Error::Invalid("cannot encode project"))?;
    let mut writer = zip::ZipWriter::new_append(Cursor::new(bytes))
        .map_err(|_| Error::Invalid("invalid Office archive"))?;
    writer
        .start_file(EXTRA, zip::write::SimpleFileOptions::default())
        .map_err(|_| Error::Invalid("cannot write Office metadata"))?;
    writer.write_all(&payload)?;
    let bytes = writer
        .finish()
        .map_err(|_| Error::Invalid("cannot finish Office archive"))?
        .into_inner();
    if bytes.len() > LIMIT {
        return Err(Error::Limit);
    }
    Ok(bytes)
}
impl Workbook {
    pub fn xlsx_bytes(&self) -> Result<Vec<u8>, Error> {
        self.require_editable()?;
        if self.external_source.is_some() {
            return self.patch_external("xlsx");
        }
        let state = self.history.current();
        if state
            .sheets
            .iter()
            .flat_map(|s| s.cells.values())
            .any(|c| matches!(c.text, CellContent::Date(_) | CellContent::Error(_)))
        {
            return Err(Error::Invalid(
                "XLSX export of date and error cells is unsupported",
            ));
        }
        if state.styles.iter().any(|s| s.borders & !15 != 0) {
            return Err(Error::Invalid("unsupported border style"));
        }
        let formats: Vec<_> = state.styles.iter().map(format).collect();
        let mut book = Xlsx::new();
        let recalc_time = chrono::Local::now().naive_local();
        for sheet in &state.sheets {
            let ws = book.add_worksheet();
            ws.set_name(&sheet.name)
                .map_err(|_| Error::Invalid("invalid XLSX sheet name"))?;
            if sheet.view.frozen_rows != 0 || sheet.view.frozen_columns != 0 {
                ws.set_freeze_panes(
                    sheet.view.frozen_rows as u32,
                    sheet.view.frozen_columns as u16,
                )
                .map_err(|_| Error::Invalid("Invalid frozen panes"))?;
            }
            for row in &sheet.view.hidden_rows {
                ws.set_row_hidden(*row as u32)
                    .map_err(|_| Error::Invalid("Cannot hide row"))?;
            }
            for col in &sheet.view.hidden_columns {
                ws.set_column_hidden(*col as u16)
                    .map_err(|_| Error::Invalid("Cannot hide column"))?;
            }

            if sheet.protected {
                ws.protect();
            }
            for (&r, &h) in &sheet.row_heights {
                ws.set_row_height_pixels(r as u32, h as u32)
                    .map_err(|_| Error::Invalid("invalid row height"))?;
            }
            for (&c, &w) in &sheet.col_widths {
                ws.set_column_width_pixels(c as u16, w as u32)
                    .map_err(|_| Error::Invalid("invalid column width"))?;
            }
            for m in &sheet.merges {
                let f = &formats[sheet.cells.get(&(m.top, m.left)).map_or(0, |c| c.style)];
                ws.merge_range(
                    m.top as u32,
                    m.left as u16,
                    m.bottom as u32,
                    m.right as u16,
                    "",
                    f,
                )
                .map_err(|_| Error::Invalid("cannot export merge"))?;
            }
            let mut eval_state = state.clone();
            eval_state.active = sheet.id;
            let eval = Workbook {
                history: SnapshotHistory::new(eval_state, 0),
                snapshots: BTreeMap::new(),
                next_revision: 1,
                external_source: None,
                external_baseline: None,
                external_format: None,
                external_cache: BTreeMap::new(),
            };
            let grid = formula::display_grid_at(&eval, recalc_time);
            for (&(r, c), cell) in &sheet.cells {
                let (r, c) = (r as u32, c as u16);
                let f = &formats[cell.style];
                match &cell.text {
                    CellContent::Text(t) => ws.write_string_with_format(r, c, t.raw(), f),
                    CellContent::Number(t) => ws.write_number_with_format(
                        r,
                        c,
                        t.raw()
                            .parse::<f64>()
                            .map_err(|_| Error::Invalid("invalid number"))?,
                        f,
                    ),
                    CellContent::Boolean(t) => {
                        ws.write_boolean_with_format(r, c, t.raw().eq_ignore_ascii_case("true"), f)
                    }
                    CellContent::Formula(t) => {
                        let value = &grid[r as usize][c as usize];
                        let value = if value.starts_with('#') {
                            "#N/A"
                        } else {
                            value.as_str()
                        };
                        ws.write_formula_with_format(
                            r,
                            c,
                            Formula::new(t.raw()).set_result(value),
                            f,
                        )
                    }
                    _ => unreachable!(),
                }
                .map_err(|_| Error::Invalid("cannot encode XLSX cell"))?;
            }
            for table in &sheet.tables {
                use rust_xlsxwriter::{Table, TableColumn, TableFunction};
                let columns: Vec<_> = (table.range.left..=table.range.right)
                    .map(|column| {
                        let header = sheet
                            .cells
                            .get(&(table.range.top, column))
                            .map(|c| c.text.raw())
                            .unwrap_or("");
                        let column = TableColumn::new().set_header(header);
                        if table.total_row {
                            column.set_total_function(TableFunction::Sum)
                        } else {
                            column
                        }
                    })
                    .collect();
                let table_format = Table::new()
                    .set_name(&table.name)
                    .set_columns(&columns)
                    .set_total_row(table.total_row)
                    .set_autofilter(sheet.filter.is_none());
                ws.add_table(
                    table.range.top as u32,
                    table.range.left as u16,
                    table.range.bottom as u32,
                    table.range.right as u16,
                    &table_format,
                )
                .map_err(|_| Error::Invalid("Cannot encode spreadsheet table"))?;
            }
            if let Some(filter) = &sheet.filter {
                use rust_xlsxwriter::{FilterCondition, FilterCriteria};
                let r = filter.range;
                ws.autofilter(r.top as u32, r.left as u16, r.bottom as u32, r.right as u16)
                    .map_err(|_| Error::Invalid("cannot encode XLSX filter"))?;
                let escaped = filter
                    .contains
                    .replace('~', "~~")
                    .replace('*', "~*")
                    .replace('?', "~?");
                let condition = FilterCondition::new()
                    .add_custom_filter(FilterCriteria::Contains, escaped.as_str());
                ws.filter_column(filter.column as u16, &condition)
                    .map_err(|_| Error::Invalid("cannot encode XLSX filter"))?;
                let grid = eval.display_grid_at(recalc_time);
                let needle = filter.contains.to_lowercase();
                for row in r.top + 1..=r.bottom {
                    if !grid[row][filter.column].to_lowercase().contains(&needle) {
                        ws.set_row_hidden(row as u32)
                            .map_err(|_| Error::Invalid("cannot hide filtered row"))?;
                    }
                }
            }
            let settings = &sheet.print;
            {
                let (w, h) = (settings.paper_width_twips, settings.paper_height_twips);
                let (short, long, landscape) = if w > h { (h, w, true) } else { (w, h, false) };
                let paper = if (short - 11906).abs() <= 20 && (long - 16838).abs() <= 20 {
                    9
                } else if (short - 12240).abs() <= 20 && (long - 15840).abs() <= 20 {
                    1
                } else {
                    return Err(Error::Invalid(
                        "XLSX export supports A4 and Letter paper sizes",
                    ));
                };
                ws.set_paper_size(paper);
                if landscape {
                    ws.set_landscape();
                } else {
                    ws.set_portrait();
                }
                ws.set_margins(
                    settings.margin_left_twips as f64 / 1440.0,
                    settings.margin_right_twips as f64 / 1440.0,
                    settings.margin_top_twips as f64 / 1440.0,
                    settings.margin_bottom_twips as f64 / 1440.0,
                    0.3,
                    0.3,
                );
                if let Some(r) = settings.area {
                    ws.set_print_area(r.top as u32, r.left as u16, r.bottom as u32, r.right as u16)
                        .map_err(|_| Error::Invalid("cannot encode print area"))?;
                }
                if settings.repeat_header_rows > 0 {
                    let first = settings.area.map_or(0, |r| r.top);
                    let last = first + settings.repeat_header_rows - 1;
                    if last >= sheet.rows {
                        return Err(Error::Invalid("print headers exceed sheet"));
                    }
                    ws.set_repeat_rows(first as u32, last as u32)
                        .map_err(|_| Error::Invalid("cannot encode print headers"))?;
                }
                if settings.fit_width {
                    ws.set_print_fit_to_pages(1, 0);
                }
                if !settings.row_breaks.is_empty() {
                    let breaks: Vec<_> = settings.row_breaks.iter().map(|r| *r as u32).collect();
                    ws.set_page_breaks(&breaks)
                        .map_err(|_| Error::Invalid("cannot encode page breaks"))?;
                }
            }
            for chart in &sheet.charts {
                use rust_xlsxwriter::{Chart as XChart, ChartType};
                let kind = match chart.kind {
                    ChartKind::Bar => ChartType::Column,
                    ChartKind::Line => ChartType::Line,
                    ChartKind::Pie => ChartType::Pie,
                    ChartKind::Scatter => ChartType::Scatter,
                };
                let mut xchart = XChart::new(kind);
                xchart.title().set_name(&chart.title);
                for series in &chart.series {
                    let source = state
                        .sheets
                        .iter()
                        .find(|s| s.id == series.sheet)
                        .ok_or(Error::Invalid("chart source sheet missing"))?;
                    let values = series.values;
                    let xseries = xchart.add_series();
                    xseries.set_name(&series.name).set_values((
                        source.name.as_str(),
                        values.top as u32,
                        values.left as u16,
                        values.bottom as u32,
                        values.right as u16,
                    ));
                    if let Some(cat) = series.categories {
                        xseries.set_categories((
                            source.name.as_str(),
                            cat.top as u32,
                            cat.left as u16,
                            cat.bottom as u32,
                            cat.right as u16,
                        ));
                    }
                }
                ws.insert_chart(chart.anchor_row as u32, chart.anchor_col as u16, &xchart)
                    .map_err(|_| Error::Invalid("cannot encode XLSX chart"))?;
            }
            for rule in &sheet.validations {
                use rust_xlsxwriter::{DataValidation as XValidation, DataValidationRule};
                let validation = match &rule.kind {
                    ValidationKind::Number { minimum, maximum } => XValidation::new()
                        .allow_decimal_number(DataValidationRule::Between(*minimum, *maximum)),
                    ValidationKind::List { values } => {
                        if values.iter().any(|s| s.contains(',')) {
                            return Err(Error::Invalid(
                                "XLSX validation list values cannot contain commas",
                            ));
                        }
                        XValidation::new().allow_list_strings(values).map_err(|_| {
                            Error::Invalid("XLSX validation list exceeds Excel limits")
                        })?
                    }
                }
                .ignore_blank(rule.allow_blank);
                let r = rule.range;
                ws.add_data_validation(
                    r.top as u32,
                    r.left as u16,
                    r.bottom as u32,
                    r.right as u16,
                    &validation,
                )
                .map_err(|_| Error::Invalid("cannot encode XLSX validation"))?;
            }
            for rule in &sheet.conditional_formats {
                use rust_xlsxwriter::{
                    ConditionalFormatCell, ConditionalFormatCellRule, ConditionalFormatText,
                    ConditionalFormatTextRule,
                };
                let mut fmt = Format::new();
                if rule.bold {
                    fmt = fmt.set_bold()
                }
                if let Some(c) = rule.foreground {
                    fmt = fmt.set_font_color(Color::RGB(c & 0xffffff))
                }
                if let Some(c) = rule.background {
                    fmt = fmt.set_background_color(Color::RGB(c & 0xffffff))
                }
                let r = rule.range;
                match &rule.condition {
                    Condition::TextContains(text) => {
                        let cf = ConditionalFormatText::new()
                            .set_rule(ConditionalFormatTextRule::Contains(text.clone()))
                            .set_format(fmt);
                        ws.add_conditional_format(
                            r.top as u32,
                            r.left as u16,
                            r.bottom as u32,
                            r.right as u16,
                            &cf,
                        )
                        .map_err(|_| Error::Invalid("cannot encode XLSX conditional format"))?;
                    }
                    condition => {
                        let pred = match condition {
                            Condition::GreaterThan(v) => ConditionalFormatCellRule::GreaterThan(*v),
                            Condition::LessThan(v) => ConditionalFormatCellRule::LessThan(*v),
                            Condition::EqualTo(v) => ConditionalFormatCellRule::EqualTo(*v),
                            _ => unreachable!(),
                        };
                        let cf = ConditionalFormatCell::new().set_rule(pred).set_format(fmt);
                        ws.add_conditional_format(
                            r.top as u32,
                            r.left as u16,
                            r.bottom as u32,
                            r.right as u16,
                            &cf,
                        )
                        .map_err(|_| Error::Invalid("cannot encode XLSX conditional format"))?;
                    }
                }
            }
        }
        let bytes = book
            .save_to_buffer()
            .map_err(|_| Error::Invalid("cannot encode XLSX"))?;
        let bytes = if state.date_1904 {
            set_generated_xlsx_1904(&bytes)?
        } else {
            bytes
        };
        embed(bytes, "xlsx", self.project_bytes()?)
    }
    pub fn open_office(path: &Path, kind: &str) -> Result<Self, Error> {
        let mut bytes = Vec::new();
        File::open(path)?
            .take((LIMIT + 1) as u64)
            .read_to_end(&mut bytes)?;
        Self::from_office_bytes(&bytes, kind)
    }
    pub fn from_office_bytes(bytes: &[u8], kind: &str) -> Result<Self, Error> {
        if bytes.len() > LIMIT {
            return Err(Error::Limit);
        }
        if kind != "xls" {
            let mut archive = zip::ZipArchive::new(Cursor::new(bytes))
                .map_err(|_| Error::Invalid("invalid Office archive"))?;
            let sum = digest(&mut archive)?;
            let mut payload = Vec::new();
            if let Ok(part) = archive.by_name(EXTRA) {
                part.take((LIMIT + 1) as u64).read_to_end(&mut payload)?;
                if payload.len() <= LIMIT {
                    if let Ok(embedded) = serde_json::from_slice::<Embedded>(&payload) {
                        if embedded.format == kind && embedded.digest == sum {
                            return Self::from_project_bytes(&embedded.project);
                        }
                    }
                }
            };
        }
        Self::from_external_office(bytes, kind)
    }
}
fn set_generated_xlsx_1904(bytes: &[u8]) -> Result<Vec<u8>, Error> {
    let mut archive =
        zip::ZipArchive::new(Cursor::new(bytes)).map_err(|_| Error::Invalid("Invalid XLSX"))?;
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for index in 0..archive.len() {
        let mut file = archive
            .by_index(index)
            .map_err(|_| Error::Invalid("Invalid XLSX"))?;
        if file.name() == "xl/workbook.xml" {
            let mut text = String::new();
            file.read_to_string(&mut text)?;
            let d = xml(&text)?;
            let node = d
                .descendants()
                .find(|n| n.has_tag_name("workbookPr"))
                .ok_or(Error::Invalid("Missing workbook properties"))?;
            let range = node.range();
            let part =
                crate::xlsx_styles::attrs(&text[range.clone()], &[("date1904", "1".into())])?;
            text.replace_range(range, &part);
            writer
                .start_file("xl/workbook.xml", zip::write::SimpleFileOptions::default())
                .map_err(|_| Error::Invalid("Cannot write XLSX"))?;
            writer.write_all(text.as_bytes())?;
        } else {
            writer
                .raw_copy_file(file)
                .map_err(|_| Error::Invalid("Cannot preserve XLSX part"))?;
        }
    }
    Ok(writer
        .finish()
        .map_err(|_| Error::Invalid("Cannot write XLSX"))?
        .into_inner())
}
fn add_ods_database_ranges(bytes: &[u8], state: &State) -> Result<Vec<u8>, Error> {
    let mut archive =
        zip::ZipArchive::new(Cursor::new(bytes)).map_err(|_| Error::Invalid("Invalid ODS"))?;
    let content =
        zip_text(&mut archive, "content.xml")?.ok_or(Error::Invalid("Missing ODS content"))?;
    let d = xml(&content)?;
    let spreadsheet = d
        .descendants()
        .find(|n| n.has_tag_name("spreadsheet"))
        .ok_or(Error::Invalid("Missing ODS spreadsheet"))?;
    let prefix = spreadsheet
        .children()
        .find(|n| n.has_tag_name("table"))
        .map(|n| prefix(&content[n.range()]))
        .unwrap_or("table:");
    let mut items = String::new();
    for sheet in &state.sheets {
        for table in &sheet.tables {
            let sheet_name = sheet.name.replace('\'', "''");
            let r = table.range;
            let target = format!(
                "'{}'.$${}${}:'{}'.$${}${}",
                sheet_name,
                col_name(r.left),
                r.top + 1,
                sheet_name,
                col_name(r.right),
                r.bottom + 1
            )
            .replace("$$", "$");
            items.push_str(&format!("<{prefix}database-range {prefix}name=\"{}\" {prefix}target-range-address=\"{}\" {prefix}contains-header=\"true\" {prefix}display-filter-buttons=\"true\"/>", escape_xml(&table.name), escape_xml(&target)));
        }
    }
    let insert = spreadsheet
        .children()
        .filter(|n| n.has_tag_name("table"))
        .map(|n| n.range().end)
        .max()
        .ok_or(Error::Invalid("Missing ODS sheets"))?;
    let mut content = content.clone();
    content.insert_str(
        insert,
        &format!("<{prefix}database-ranges>{items}</{prefix}database-ranges>"),
    );
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for index in 0..archive.len() {
        let file = archive
            .by_index(index)
            .map_err(|_| Error::Invalid("Invalid ODS"))?;
        let name = file.name().to_owned();
        if name == EXTRA {
            continue;
        }
        if name == "content.xml" {
            writer
                .start_file(
                    name,
                    zip::write::SimpleFileOptions::default().compression_method(file.compression()),
                )
                .map_err(|_| Error::Invalid("Cannot write ODS"))?;
            writer.write_all(content.as_bytes())?;
        } else {
            writer
                .raw_copy_file(file)
                .map_err(|_| Error::Invalid("Cannot preserve ODS"))?;
        }
    }
    Ok(writer
        .finish()
        .map_err(|_| Error::Invalid("Cannot write ODS"))?
        .into_inner())
}
fn ods_formula_to_a1(input: &str) -> Option<String> {
    let input = input
        .strip_prefix("=of:=")
        .or_else(|| input.strip_prefix("of:="))
        .or_else(|| input.strip_prefix('='))?;
    let mut out = String::from("=");
    let mut i = 0;
    let b = input.as_bytes();
    let mut in_string = false;
    while i < b.len() {
        if b[i] == b'"' {
            out.push('"');
            i += 1;
            if in_string && b.get(i) == Some(&b'"') {
                out.push('"');
                i += 1;
            } else {
                in_string = !in_string;
            }
            continue;
        }
        if !in_string && b[i] == b'[' {
            let end = input[i + 1..].find(']')? + i + 1;
            let range = &input[i + 1..end];
            let mut parts = range.split(':');
            for (index, part) in parts.by_ref().enumerate() {
                if index > 1 {
                    return None;
                }
                if index == 1 {
                    out.push(':');
                }
                let dot = part.rfind('.')?;
                let sheet = &part[..dot];
                let cell = &part[dot + 1..];
                if parse_a1_endpoint(cell, 0).is_none_or(|v| v.0 != cell.len()) {
                    return None;
                }
                if !sheet.is_empty() && sheet != "$" {
                    out.push_str(sheet.trim_start_matches('$'));
                    out.push('!');
                }
                out.push_str(cell);
            }
            i = end + 1;
            continue;
        }
        if !in_string && b[i] == b';' {
            out.push(',');
            i += 1;
            continue;
        }
        let ch = input[i..].chars().next()?;
        out.push(ch);
        i += ch.len_utf8();
    }
    Some(out)
}
fn ods_formula(input: &str) -> Result<String, Error> {
    let b = input.as_bytes();
    let mut out = String::from("of:");
    let mut i = 0;
    let mut quoted = false;
    while i < b.len() {
        if b[i] == b'"' {
            quoted = !quoted;
            out.push('"');
            i += 1;
            continue;
        }
        if quoted || !b[i].is_ascii() {
            let c = input[i..].chars().next().unwrap();
            out.push(c);
            i += c.len_utf8();
            continue;
        }
        if b[i] == b'\'' || b[i].is_ascii_alphabetic() || b[i] == b'_' {
            let start = i;
            let mut j = i;
            let mut name = String::new();
            if b[j] == b'\'' {
                j += 1;
                while j < b.len() {
                    let ch = input[j..].chars().next().unwrap();
                    j += ch.len_utf8();
                    if ch == '\'' {
                        if b.get(j) == Some(&b'\'') {
                            name.push('\'');
                            j += 1;
                        } else {
                            break;
                        }
                    } else {
                        name.push(ch);
                    }
                }
            } else {
                while j < b.len() && (b[j].is_ascii_alphanumeric() || matches!(b[j], b'_' | b'.')) {
                    j += 1;
                }
                name.push_str(&input[start..j]);
            }
            if b.get(j) == Some(&b'!') {
                let ref_start = j + 1;
                let (mut end, _, _, _, _) = parse_a1_endpoint(input, ref_start)
                    .ok_or(Error::Invalid("Unsupported ODS sheet reference"))?;
                out.push_str("['");
                out.push_str(&name.replace('\'', "''"));
                out.push_str("'.");
                out.push_str(&input[ref_start..end]);
                if b.get(end) == Some(&b':') {
                    let next = end + 1;
                    let (finish, _, _, _, _) = parse_a1_endpoint(input, next)
                        .ok_or(Error::Invalid("Unsupported ODS sheet range"))?;
                    out.push_str(":.");
                    out.push_str(&input[next..finish]);
                    end = finish;
                }
                out.push(']');
                i = end;
                continue;
            }
        }
        if b[i] == b'!' || b[i] == b'[' || b[i] == b']' {
            return Err(Error::Invalid(
                "ODS export of qualified or structured references is unsupported",
            ));
        }
        let start = i;
        let mut j = i;
        if j < b.len() && b[j] == b'$' {
            j += 1
        }
        let cs = j;
        while j < b.len() && b[j].is_ascii_alphabetic() {
            j += 1
        }
        let ce = j;
        if j < b.len() && b[j] == b'$' {
            j += 1
        }
        let rs = j;
        while j < b.len() && b[j].is_ascii_digit() {
            j += 1
        }
        if ce > cs
            && ce - cs <= 3
            && j > rs
            && (start == 0 || !b[start - 1].is_ascii_alphanumeric())
            && (j == b.len() || (!b[j].is_ascii_alphanumeric() && b[j] != b'('))
        {
            out.push_str("[.");
            out.push_str(&input[start..j]);
            if j < b.len() && b[j] == b':' {
                let mut k = j + 1;
                if k < b.len() && b[k] == b'$' {
                    k += 1
                }
                let ks = k;
                while k < b.len() && b[k].is_ascii_alphabetic() {
                    k += 1
                }
                let ke = k;
                if k < b.len() && b[k] == b'$' {
                    k += 1
                }
                let kr = k;
                while k < b.len() && b[k].is_ascii_digit() {
                    k += 1
                }
                if ke > ks && k > kr {
                    out.push_str(":.");
                    out.push_str(&input[j + 1..k]);
                    j = k;
                }
            }
            out.push(']');
            i = j;
            continue;
        }
        out.push(if b[i] == b',' { ';' } else { b[i] as char });
        i += 1
    }
    if quoted {
        return Err(Error::Invalid("invalid formula"));
    }
    Ok(out)
}
impl Workbook {
    pub fn ods_bytes(&self) -> Result<Vec<u8>, Error> {
        self.require_editable()?;
        if self.external_source.is_some() {
            return self.patch_external("ods");
        }
        use spreadsheet_ods::{
            color::Rgb,
            style::units::{Border, LineStyle},
            CellStyle as OdsStyle, Length, Sheet, WorkBook,
        };
        let state = self.history.current();
        let recalc_time = chrono::Local::now().naive_local();
        if state.date_1904 {
            return Err(Error::Invalid(
                "Native export of the 1904 date system is not yet supported",
            ));
        }
        if state.sheets.iter().any(|s| !s.tables.is_empty()) {
            let mut converted_state = state.clone();
            let mut cells = 0usize;
            let mut style_cache = BTreeMap::new();
            for sheet in &mut converted_state.sheets {
                for table in &sheet.tables {
                    let range = table.range;
                    cells = cells.saturating_add(
                        (range.bottom - range.top + 1).saturating_mul(range.right - range.left + 1),
                    );
                    if cells > MAX_ROWS * MAX_COLS {
                        return Err(Error::Limit);
                    }
                    for row in range.top..=range.bottom {
                        for col in range.left..=range.right {
                            let role = if row == range.top {
                                1
                            } else if table.total_row && row == range.bottom {
                                2
                            } else if (row - range.top) % 2 == 0 {
                                3
                            } else {
                                0
                            };
                            if role == 0 {
                                continue;
                            }
                            let cell = sheet.cells.entry((row, col)).or_insert_with(|| Cell {
                                text: CellContent::default(),
                                style: 0,
                            });
                            let key = (cell.style, role);
                            cell.style = if let Some(id) = style_cache.get(&key) {
                                *id
                            } else {
                                if style_cache.len() >= 1024 {
                                    return Err(Error::Limit);
                                }
                                let mut style = converted_state.styles[cell.style].clone();
                                if role == 1 {
                                    style.bold = true;
                                    style.foreground.get_or_insert(0xffffff);
                                    style.background.get_or_insert(0x35618b);
                                } else if role == 2 {
                                    style.bold = true;
                                    style.background.get_or_insert(0xdbe8f4);
                                } else {
                                    style.background.get_or_insert(0xeaf2f8);
                                }
                                let id = if let Some(id) =
                                    converted_state.styles.iter().position(|v| *v == style)
                                {
                                    id
                                } else {
                                    if converted_state.styles.len() >= 100_000 {
                                        return Err(Error::Limit);
                                    }
                                    converted_state.styles.push(style);
                                    converted_state.styles.len() - 1
                                };
                                style_cache.insert(key, id);
                                id
                            };
                        }
                    }
                }
            }
            let mut converted = Workbook {
                history: SnapshotHistory::new(converted_state, 0),
                snapshots: BTreeMap::new(),
                next_revision: 1,
                external_source: None,
                external_baseline: None,
                external_format: None,
                external_cache: BTreeMap::new(),
            };
            let tables: Vec<_> = state
                .sheets
                .iter()
                .flat_map(|sheet| sheet.tables.iter().map(move |table| (sheet.id, table.id)))
                .collect();
            for (sheet, table) in tables {
                converted.select_sheet(sheet)?;
                converted.remove_table(table)?;
            }
            let bytes = converted.ods_bytes()?;
            let bytes = add_ods_database_ranges(&bytes, state)?;
            return embed(bytes, "ods", self.project_bytes()?);
        }
        if state
            .sheets
            .iter()
            .any(|s| s.filter.is_some() || s.print != PrintSettings::default())
        {
            return Err(Error::Invalid(
                "ODS export of filter or print settings is not yet supported",
            ));
        }
        if state
            .sheets
            .iter()
            .any(|s| !s.charts.is_empty() || !s.pivots.is_empty())
        {
            return Err(Error::Invalid(
                "ODS export of charts or pivot configurations is not yet supported",
            ));
        }
        if state
            .sheets
            .iter()
            .any(|s| !s.validations.is_empty() || !s.conditional_formats.is_empty())
        {
            return Err(Error::Invalid(
                "ODS export of validation and conditional formatting is not yet supported",
            ));
        }
        if state.sheets.iter().any(|s| {
            s.protected
                || s.cells
                    .values()
                    .any(|c| matches!(c.text, CellContent::Date(_) | CellContent::Error(_)))
        }) || state
            .styles
            .iter()
            .any(|s| s.borders & !15 != 0 || !s.number_format.is_empty())
        {
            return Err(Error::Invalid("ODS export of protection, dates, errors, unknown borders or number formats is unsupported"));
        }
        let mut book = WorkBook::default();
        let mut styles = Vec::new();
        for (i, s) in state.styles.iter().enumerate() {
            let mut st = OdsStyle::new_empty();
            st.set_name(format!("xls42style{i}"));
            if s.bold {
                st.set_font_bold()
            }
            if s.italic {
                st.set_font_italic()
            }
            if s.underline {
                st.set_text_underline_style(LineStyle::Solid)
            }
            if s.borders & 1 != 0 {
                st.set_border_top(Length::Pt(1.0), Border::Solid, Rgb::new(0, 0, 0))
            }
            if s.borders & 2 != 0 {
                st.set_border_right(Length::Pt(1.0), Border::Solid, Rgb::new(0, 0, 0))
            }
            if s.borders & 4 != 0 {
                st.set_border_bottom(Length::Pt(1.0), Border::Solid, Rgb::new(0, 0, 0))
            }
            if s.borders & 8 != 0 {
                st.set_border_left(Length::Pt(1.0), Border::Solid, Rgb::new(0, 0, 0))
            }
            if !s.font.is_empty() {
                st.set_font_name(&s.font)
            }
            if s.font_size > 0 {
                st.set_font_size(Length::Pt(s.font_size as f64).into())
            }
            if let Some(c) = s.foreground {
                st.set_color(Rgb::new((c >> 16) as u8, (c >> 8) as u8, c as u8))
            }
            if let Some(c) = s.background {
                st.set_background_color(Rgb::new((c >> 16) as u8, (c >> 8) as u8, c as u8))
            }
            styles.push(book.add_cellstyle(st));
        }
        for sh in &state.sheets {
            let mut sheet = Sheet::new(&sh.name);
            if sh.view.frozen_rows > 0 {
                sheet.split_row_header((sh.view.frozen_rows - 1) as u32);
            }
            if sh.view.frozen_columns > 0 {
                sheet.split_col_header((sh.view.frozen_columns - 1) as u32);
            }
            for row in &sh.view.hidden_rows {
                sheet.set_row_visible(*row as u32, spreadsheet_ods::sheet::Visibility::Collapsed);
            }
            for col in &sh.view.hidden_columns {
                sheet.set_col_visible(*col as u32, spreadsheet_ods::sheet::Visibility::Collapsed);
            }
            for (&r, &h) in &sh.row_heights {
                sheet.set_row_height(r as u32, Length::Pt(h as f64 * 0.75))
            }
            for (&c, &w) in &sh.col_widths {
                sheet.set_col_width(c as u32, Length::Pt(w as f64 * 0.75))
            }
            for m in &sh.merges {
                sheet.set_row_span(m.top as u32, m.left as u32, (m.bottom - m.top + 1) as u32);
                sheet.set_col_span(m.top as u32, m.left as u32, (m.right - m.left + 1) as u32)
            }
            let mut eval_state = state.clone();
            eval_state.active = sh.id;
            let eval = Workbook {
                history: SnapshotHistory::new(eval_state, 0),
                snapshots: BTreeMap::new(),
                next_revision: 1,
                external_source: None,
                external_baseline: None,
                external_format: None,
                external_cache: BTreeMap::new(),
            };
            let grid = formula::display_grid_at(&eval, recalc_time);
            for (&(r, c), cell) in &sh.cells {
                let (r, c) = (r as u32, c as u32);
                match &cell.text {
                    CellContent::Text(t) => sheet.set_value(r, c, t.raw()),
                    CellContent::Number(t) => sheet.set_value(
                        r,
                        c,
                        t.raw()
                            .parse::<f64>()
                            .map_err(|_| Error::Invalid("invalid number"))?,
                    ),
                    CellContent::Boolean(t) => {
                        sheet.set_value(r, c, t.raw().eq_ignore_ascii_case("true"))
                    }
                    CellContent::Formula(t) => {
                        if let Ok(n) = grid[r as usize][c as usize].parse::<f64>() {
                            sheet.set_value(r, c, n)
                        }
                        sheet.set_formula(r, c, ods_formula(t.raw())?)
                    }
                    _ => unreachable!(),
                }
                sheet.set_cellstyle(r, c, &styles[cell.style]);
            }
            book.push_sheet(sheet);
        }
        let bytes = spreadsheet_ods::write_ods_buf(&mut book, Vec::new())
            .map_err(|_| Error::Invalid("cannot encode ODS"))?;
        embed(bytes, "ods", self.project_bytes()?)
    }
}

impl Workbook {
    pub fn save_office(&mut self, path: &Path, format: &str) -> Result<(), Error> {
        let bytes = match format {
            "xlsx" => self.xlsx_bytes()?,
            "ods" => self.ods_bytes()?,
            _ => return Err(Error::Invalid("unknown Office format")),
        };
        write_new(path, &bytes)?;
        self.history.mark_saved();
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn office_packages_and_safe_reopen() {
        let mut b = Workbook::new(4, 4).unwrap();
        b.set_content(0, 0, CellContent::Text(TextValue::from_raw("123".into())))
            .unwrap();
        b.set_cell(1, 0, "3".into()).unwrap();
        b.set_cell(1, 1, "=SUM(A2:A2)".into()).unwrap();
        b.set_style(
            Range {
                top: 0,
                left: 0,
                bottom: 0,
                right: 0,
            },
            CellStyle {
                bold: true,
                ..Default::default()
            },
        )
        .unwrap();
        b.merge(Range {
            top: 2,
            left: 0,
            bottom: 2,
            right: 1,
        })
        .unwrap();
        b.add_sheet("Second").unwrap();
        for format in ["xlsx", "ods"] {
            let bytes = if format == "xlsx" {
                b.xlsx_bytes().unwrap()
            } else {
                b.ods_bytes().unwrap()
            };
            let mut archive = zip::ZipArchive::new(Cursor::new(bytes.as_slice())).unwrap();
            let main = if format == "xlsx" {
                "xl/workbook.xml"
            } else {
                "content.xml"
            };
            assert!(archive.by_name(main).is_ok());
            let restored = Workbook::from_office_bytes(&bytes, format).unwrap();
            assert_eq!(restored.sheets().len(), 2);
            assert_eq!(restored.active_sheet(), b.active_sheet());
            if format == "ods" {
                let parsed = spreadsheet_ods::read_ods_buf(&bytes).unwrap();
                assert_eq!(parsed.num_sheets(), 2);
            }
        }
    }
}

impl Workbook {
    fn from_external_office(bytes: &[u8], kind: &str) -> Result<Self, Error> {
        use calamine::{Data, Reader};
        let mut source = calamine::open_workbook_auto_from_rs(Cursor::new(bytes))
            .map_err(|_| Error::Invalid("unsupported or invalid spreadsheet"))?;
        let names = source.sheet_names().to_vec();
        if names.is_empty() || names.len() > MAX_SHEETS {
            return Err(Error::Limit);
        }
        let mut sheets = Vec::new();
        let mut cache = BTreeMap::new();
        let mut total = 0usize;
        for (index, name) in names.into_iter().enumerate() {
            let values = source
                .worksheet_range(&name)
                .map_err(|_| Error::Invalid("cannot read sheet values"))?;
            let formulas = source
                .worksheet_formula(&name)
                .map_err(|_| Error::Invalid("cannot read sheet formulas"))?;
            let end = values.end().unwrap_or((0, 0));
            let fend = formulas.end().unwrap_or((0, 0));
            let rows = (end.0.max(fend.0) as usize + 1).max(20);
            let cols = (end.1.max(fend.1) as usize + 1).max(8);
            if rows > MAX_ROWS || cols > MAX_COLS {
                return Err(Error::Limit);
            }
            let id = index as u64 + 1;
            let mut sheet = Sheet::new(id, name, rows, cols);
            let start = values.start().unwrap_or((0, 0));
            for (r, c, data) in values.used_cells() {
                let (r, c) = (r + start.0 as usize, c + start.1 as usize);
                let val = match data {
                    Data::Empty => continue,
                    Data::Int(v) => CellContent::Number(TextValue::from_raw(v.to_string())),
                    Data::Float(v) if v.is_finite() => {
                        CellContent::Number(TextValue::from_raw(v.to_string()))
                    }
                    Data::Float(_) => return Err(Error::Invalid("invalid numeric value")),
                    Data::String(v) => CellContent::Text(TextValue::from_raw(v.clone())),
                    Data::Bool(v) => CellContent::Boolean(TextValue::from_raw(v.to_string())),
                    Data::DateTime(v) => CellContent::Date(TextValue::from_raw(
                        v.as_datetime()
                            .map(|d| d.format("%Y-%m-%dT%H:%M:%S%.f").to_string())
                            .unwrap_or_else(|| v.as_f64().to_string()),
                    )),
                    Data::DateTimeIso(v) | Data::DurationIso(v) => {
                        CellContent::Date(TextValue::from_raw(v.clone()))
                    }
                    Data::Error(v) => CellContent::Error(TextValue::from_raw(v.to_string())),
                };
                total = total.checked_add(val.raw().len()).ok_or(Error::Limit)?;
                if total > MAX_TEXT {
                    return Err(Error::Limit);
                }
                sheet.cells.insert(
                    (r, c),
                    Cell {
                        text: val,
                        style: 0,
                    },
                );
            }
            let start = formulas.start().unwrap_or((0, 0));
            for (r, c, formula) in formulas.used_cells() {
                let (r, c) = (r + start.0 as usize, c + start.1 as usize);
                if let Some(cached) = sheet.cells.get(&(r, c)) {
                    cache.insert((id, r, c), cached.text.raw().to_owned());
                }
                let text = if kind == "ods" {
                    ods_formula_to_a1(formula).unwrap_or_else(|| {
                        if formula.starts_with('=') {
                            formula.clone()
                        } else {
                            format!("={formula}")
                        }
                    })
                } else if formula.starts_with('=') {
                    formula.clone()
                } else {
                    format!("={formula}")
                };
                total = total.checked_add(text.len()).ok_or(Error::Limit)?;
                if total > MAX_TEXT {
                    return Err(Error::Limit);
                }
                sheet.cells.insert(
                    (r, c),
                    Cell {
                        text: CellContent::Formula(TextValue::from_raw(text)),
                        style: 0,
                    },
                );
            }
            sheets.push(sheet);
        }
        let mut state = State {
            next_id: sheets.len() as u64 + 1,
            sheets,
            active: 1,
            styles: vec![CellStyle::default()],
            date_1904: false,
            selection: Range {
                top: 0,
                left: 0,
                bottom: 0,
                right: 0,
            },
        };
        if kind == "ods" {
            import_ods_layout(bytes, &mut state)?;
        } else if kind == "xlsx" {
            let mut archive = zip::ZipArchive::new(Cursor::new(bytes))
                .map_err(|_| Error::Invalid("invalid XLSX"))?;
            if let Some(workbook) = zip_text(&mut archive, "xl/workbook.xml")? {
                state.date_1904 = xml(&workbook)?
                    .descendants()
                    .find(|n| n.has_tag_name("workbookPr"))
                    .and_then(|n| n.attribute("date1904"))
                    .is_some_and(|v| matches!(v, "1" | "true"));
            }
            import_xlsx_layout(bytes, &mut state)?;
        }
        Ok(Self {
            history: SnapshotHistory::new(state.clone(), 8),
            snapshots: BTreeMap::new(),
            next_revision: 1,
            external_source: Some(bytes.to_vec()),
            external_baseline: Some(state),
            external_format: Some(kind.to_owned()),
            external_cache: cache,
        })
    }
}

fn import_ods_layout(bytes: &[u8], state: &mut State) -> Result<(), Error> {
    use spreadsheet_ods::Length;
    let book = spreadsheet_ods::read_ods_buf(bytes)
        .map_err(|_| Error::Invalid("cannot parse ODS layout"))?;
    let mut archive =
        zip::ZipArchive::new(Cursor::new(bytes)).map_err(|_| Error::Invalid("invalid ODS"))?;
    let xml_content =
        zip_text(&mut archive, "content.xml")?.ok_or(Error::Invalid("missing ODS content"))?;
    let document = xml(&xml_content)?;
    for node in document.descendants().filter(|n| n.has_tag_name("table")) {
        let name = node
            .attributes()
            .find(|a| a.name() == "name")
            .map(|a| a.value());
        let protected = node
            .attributes()
            .find(|a| a.name() == "protected")
            .is_some_and(|a| matches!(a.value(), "true" | "1"));
        if let Some(sheet) = state
            .sheets
            .iter_mut()
            .find(|s| Some(s.name.as_str()) == name)
        {
            sheet.protected = protected;
        }
    }
    if book.num_sheets() != state.sheets.len() {
        return Err(Error::Invalid("inconsistent ODS sheets"));
    }
    for i in 0..state.sheets.len() {
        let source = book.sheet(i);
        let sheet = &mut state.sheets[i];
        if matches!(
            source.config().vert_split_mode,
            spreadsheet_ods::sheet::SplitMode::Heading
        ) {
            let n = source.config().vert_split_pos as usize;
            if n < MAX_ROWS {
                sheet.rows = sheet.rows.max(n + 1);
                sheet.view.frozen_rows = n;
            }
        }
        if matches!(
            source.config().hor_split_mode,
            spreadsheet_ods::sheet::SplitMode::Heading
        ) {
            let n = source.config().hor_split_pos as usize;
            if n < MAX_COLS {
                sheet.cols = sheet.cols.max(n + 1);
                sheet.view.frozen_columns = n;
            }
        }
        for row in 0..sheet.rows {
            if source.row_visible(row as u32) != spreadsheet_ods::sheet::Visibility::Visible {
                sheet.view.hidden_rows.insert(row);
            }
        }
        for col in 0..sheet.cols {
            if source.col_visible(col as u32) != spreadsheet_ods::sheet::Visibility::Visible {
                sheet.view.hidden_columns.insert(col);
            }
        }
        for r in 0..sheet.rows {
            if let Length::Pt(v) = source.row_height(r as u32) {
                if v > 0.0 && v < 49151.0 {
                    sheet.row_heights.insert(r, (v / 0.75).round() as u16);
                }
            }
        }
        for c in 0..sheet.cols {
            if let Length::Pt(v) = source.col_width(c as u32) {
                if v > 0.0 && v < 49151.0 {
                    sheet.col_widths.insert(c, (v / 0.75).round() as u16);
                }
            }
        }
        for r in 0..sheet.rows {
            for c in 0..sheet.cols {
                let rs = source.row_span(r as u32, c as u32) as usize;
                let cs = source.col_span(r as u32, c as u32) as usize;
                if rs > 1 || cs > 1 {
                    let m = Range {
                        top: r,
                        left: c,
                        bottom: r + rs - 1,
                        right: c + cs - 1,
                    };
                    if m.valid(sheet) && !sheet.merges.iter().any(|x| x.overlaps(m)) {
                        sheet.merges.push(m)
                    }
                }
                if let Some(reference) = source.cellstyle(r as u32, c as u32) {
                    if let Some(style) = book.cellstyle(reference.as_ref()) {
                        let mut out = CellStyle::default();
                        for (key, value) in style.textstyle().iter() {
                            match key.as_ref() {
                                "fo:font-weight" => out.bold = value == "bold",
                                "fo:font-style" => out.italic = value == "italic",
                                "style:font-name" => out.font = value.into(),
                                "fo:font-size" => {
                                    out.font_size = value
                                        .strip_suffix("pt")
                                        .and_then(|v| v.parse::<f64>().ok())
                                        .unwrap_or(0.0)
                                        .clamp(0.0, u16::MAX as f64)
                                        as u16
                                }
                                "fo:color" => out.foreground = parse_rgb(value),
                                "style:text-underline-style" => out.underline = value != "none",
                                _ => {}
                            }
                        }
                        for (key, value) in style.cellstyle().iter() {
                            match key.as_ref() {
                                "fo:background-color" => out.background = parse_rgb(value),
                                "fo:border-top" => {
                                    if value != "none" {
                                        out.borders |= 1
                                    }
                                }
                                "fo:border-right" => {
                                    if value != "none" {
                                        out.borders |= 2
                                    }
                                }
                                "fo:border-bottom" => {
                                    if value != "none" {
                                        out.borders |= 4
                                    }
                                }
                                "fo:border-left" => {
                                    if value != "none" {
                                        out.borders |= 8
                                    }
                                }
                                "fo:border" => {
                                    if value != "none" {
                                        out.borders = 15
                                    }
                                }
                                _ => {}
                            }
                        }
                        let id = match state.styles.iter().position(|s| *s == out) {
                            Some(id) => id,
                            None => {
                                state.styles.push(out);
                                state.styles.len() - 1
                            }
                        };
                        if id != 0 {
                            sheet
                                .cells
                                .entry((r, c))
                                .or_insert_with(|| Cell {
                                    text: CellContent::default(),
                                    style: 0,
                                })
                                .style = id
                        }
                    }
                }
            }
        }
    }
    Ok(())
}
fn parse_rgb(s: &str) -> Option<u32> {
    let s = s.strip_prefix('#')?;
    if s.len() != 6 {
        return None;
    }
    u32::from_str_radix(s, 16).ok()
}
#[cfg(test)]
mod external_regression {
    use super::*;
    #[test]
    fn external_books_edit_values_preserving_other_parts() {
        let mut x = rust_xlsxwriter::Workbook::new();
        let w = x.add_worksheet();
        w.set_name("External").unwrap();
        w.write_number_with_format(
            1,
            1,
            42,
            &rust_xlsxwriter::Format::new()
                .set_bold()
                .set_background_color(rust_xlsxwriter::Color::RGB(0xff0000)),
        )
        .unwrap();
        w.merge_range(2, 0, 2, 1, "header", &rust_xlsxwriter::Format::new())
            .unwrap();
        w.write_formula(
            1,
            2,
            rust_xlsxwriter::Formula::new("=B2*2").set_result("84"),
        )
        .unwrap();
        let bytes = x.save_to_buffer().unwrap();
        let mut writer = zip::ZipWriter::new_append(Cursor::new(bytes)).unwrap();
        writer
            .start_file(
                "customXml/opaque.txt",
                zip::write::SimpleFileOptions::default(),
            )
            .unwrap();
        writer.write_all(b"retain me").unwrap();
        let bytes = writer.finish().unwrap().into_inner();
        let mut b = Workbook::from_office_bytes(&bytes, "xlsx").unwrap();
        assert!(!b.is_read_only());
        assert_eq!(b.cell(1, 1), Some("42"));
        assert!(b.style(1, 1).unwrap().bold);
        assert_eq!(b.style(1, 1).unwrap().background, Some(0xff0000));
        assert_eq!(
            b.merges(),
            [Range {
                top: 2,
                left: 0,
                bottom: 2,
                right: 1
            }]
        );
        assert_eq!(b.cell(1, 2), Some("=B2*2"));
        assert_eq!(b.display_grid()[1][2], "84");
        b.set_cell(0, 0, "new & <text>".into()).unwrap();
        b.set_cell(1, 1, "99".into()).unwrap();
        b.set_row_height(1, 48).unwrap();
        b.set_row_height(8, 40).unwrap();
        b.set_column_width(1, 140).unwrap();
        b.set_column_width(3, 105).unwrap();
        let mut style = b.style(1, 1).unwrap().clone();
        style.italic = true;
        style.background = Some(0x2277aa);
        style.number_format = "0.00".into();
        style.borders = 15;
        b.set_style(
            Range {
                top: 1,
                left: 1,
                bottom: 1,
                right: 1,
            },
            style,
        )
        .unwrap();
        let mut formula_style = b.style(1, 2).unwrap().clone();
        formula_style.underline = true;
        b.set_style(
            Range {
                top: 1,
                left: 2,
                bottom: 1,
                right: 2,
            },
            formula_style,
        )
        .unwrap();
        b.merge(Range {
            top: 4,
            left: 0,
            bottom: 4,
            right: 1,
        })
        .unwrap();
        let saved = b.xlsx_bytes().unwrap();
        let mut z = zip::ZipArchive::new(Cursor::new(saved.as_slice())).unwrap();
        let mut opaque = String::new();
        z.by_name("customXml/opaque.txt")
            .unwrap()
            .read_to_string(&mut opaque)
            .unwrap();
        assert_eq!(opaque, "retain me");
        let reopened = Workbook::from_office_bytes(&saved, "xlsx").unwrap();
        assert_eq!(reopened.cell(0, 0), Some("new & <text>"));
        assert_eq!(reopened.cell(1, 1), Some("99"));
        assert_eq!(reopened.row_height(1), Some(48));
        assert_eq!(reopened.row_height(8), Some(40));
        assert_eq!(reopened.column_width(1), Some(140));
        assert_eq!(reopened.column_width(3), Some(105));
        assert!(reopened.style(1, 1).unwrap().bold);
        assert!(reopened.style(1, 1).unwrap().italic);
        assert_eq!(reopened.style(1, 1).unwrap().background, Some(0x2277aa));
        assert_eq!(reopened.style(1, 1).unwrap().borders, 15);
        assert_eq!(reopened.style(1, 1).unwrap().number_format, "0.00");
        assert_eq!(reopened.cell(1, 2), Some("=B2*2"));
        assert!(reopened.style(1, 2).unwrap().underline);
        assert!(reopened.merges().contains(&Range {
            top: 4,
            left: 0,
            bottom: 4,
            right: 1
        }));
        let original_sheet = b.active_sheet();
        let added = b.add_sheet("Added").unwrap();
        assert_ne!(added, original_sheet);
        assert!(b.add_sheet("Added").is_err());
        assert_eq!(b.active_sheet(), added);
        b.set_cell(0, 0, "new sheet value".into()).unwrap();
        assert!(b.undo());
        assert_eq!(b.cell(0, 0), Some(""));
        assert!(b.redo());
        assert_eq!(b.cell(0, 0), Some("new sheet value"));
        let saved_with_sheet = b.xlsx_bytes().unwrap();
        let mut archive = zip::ZipArchive::new(Cursor::new(saved_with_sheet.as_slice())).unwrap();
        let mut retained = String::new();
        archive
            .by_name("customXml/opaque.txt")
            .unwrap()
            .read_to_string(&mut retained)
            .unwrap();
        assert_eq!(retained, "retain me");
        let mut reopened = Workbook::from_office_bytes(&saved_with_sheet, "xlsx").unwrap();
        assert_eq!(reopened.sheets().len(), 2);
        let new_id = reopened.sheets().iter().find(|s| s.1 == "Added").unwrap().0;
        reopened.select_sheet(new_id).unwrap();
        assert_eq!(reopened.cell(0, 0), Some("new sheet value"));
        reopened.select_sheet(reopened.sheets()[0].0).unwrap();
        assert_eq!(reopened.cell(0, 0), Some("new & <text>"));
        assert_eq!(reopened.cell(1, 1), Some("99"));
        assert!(reopened.style(1, 1).unwrap().italic);
        assert_eq!(reopened.cell(1, 2), Some("=B2*2"));
        assert!(b.project_bytes().is_err());
        assert!(b.is_dirty());
    }
    #[test]
    fn external_ods_preserves_visible_style_and_merges() {
        use spreadsheet_ods::{CellStyle as OdsStyle, Sheet, WorkBook};
        let mut o = WorkBook::default();
        let mut style = OdsStyle::new_empty();
        style.set_name("mybold");
        style.set_font_bold();
        let style = o.add_cellstyle(style);
        let mut sh = Sheet::new("Sheet1");
        sh.set_value(0, 0, "hello");
        sh.set_cellstyle(0, 0, &style);
        sh.set_row_span(0, 0, 2);
        sh.set_col_span(0, 0, 2);
        o.push_sheet(sh);
        let bytes = spreadsheet_ods::write_ods_buf(&mut o, Vec::new()).unwrap();
        let mut writer = zip::ZipWriter::new_append(Cursor::new(bytes)).unwrap();
        writer
            .start_file(
                "Pictures/opaque.bin",
                zip::write::SimpleFileOptions::default(),
            )
            .unwrap();
        writer.write_all(b"unparsed payload").unwrap();
        let bytes = writer.finish().unwrap().into_inner();
        let mut b = Workbook::from_office_bytes(&bytes, "ods").unwrap();
        assert!(!b.is_read_only());
        assert_eq!(b.cell(0, 0), Some("hello"));
        assert!(b.style(0, 0).unwrap().bold);
        b.set_cell(0, 0, "changed".into()).unwrap();
        let mut style = b.style(0, 0).unwrap().clone();
        style.italic = true;
        style.background = Some(0x35aadc);
        style.borders = 15;
        b.set_style(
            Range {
                top: 0,
                left: 0,
                bottom: 0,
                right: 0,
            },
            style,
        )
        .unwrap();
        b.merge(Range {
            top: 4,
            left: 0,
            bottom: 4,
            right: 1,
        })
        .unwrap();
        let saved = b.ods_bytes().unwrap();
        let mut z = zip::ZipArchive::new(Cursor::new(saved.as_slice())).unwrap();
        let mut data = String::new();
        z.by_name("Pictures/opaque.bin")
            .unwrap()
            .read_to_string(&mut data)
            .unwrap();
        assert_eq!(data, "unparsed payload");
        let reopened = Workbook::from_office_bytes(&saved, "ods").unwrap();
        assert_eq!(reopened.cell(0, 0), Some("changed"));
        assert!(reopened.style(0, 0).unwrap().bold);
        assert!(reopened.style(0, 0).unwrap().italic);
        assert_eq!(reopened.style(0, 0).unwrap().background, Some(0x35aadc));
        assert_eq!(reopened.style(0, 0).unwrap().borders, 15);
        assert!(reopened.merges().contains(&Range {
            top: 4,
            left: 0,
            bottom: 4,
            right: 1
        }));
        assert_eq!(
            b.merges(),
            [
                Range {
                    top: 0,
                    left: 0,
                    bottom: 1,
                    right: 1
                },
                Range {
                    top: 4,
                    left: 0,
                    bottom: 4,
                    right: 1
                }
            ]
        );
    }
}
fn zip_text(
    archive: &mut zip::ZipArchive<Cursor<&[u8]>>,
    path: &str,
) -> Result<Option<String>, Error> {
    let mut file = match archive.by_name(path) {
        Ok(f) => f,
        Err(_) => return Ok(None),
    };
    if file.size() > LIMIT as u64 {
        return Err(Error::Limit);
    }
    let mut text = String::new();
    file.read_to_string(&mut text)?;
    Ok(Some(text))
}
pub(super) fn xml<'a>(text: &'a str) -> Result<roxmltree::Document<'a>, Error> {
    roxmltree::Document::parse(text).map_err(|_| Error::Invalid("invalid XLSX XML"))
}
fn child<'a, 'b>(node: roxmltree::Node<'a, 'b>, name: &str) -> Option<roxmltree::Node<'a, 'b>> {
    node.children().find(|n| n.has_tag_name(name))
}
fn attrs<'a, 'b>(node: roxmltree::Node<'a, 'b>, name: &str) -> Option<&'a str> {
    node.attribute(name)
}
fn xlsx_color(n: Option<roxmltree::Node<'_, '_>>) -> Option<u32> {
    let rgb = n?.attribute("rgb")?;
    let rgb = if rgb.len() == 8 { &rgb[2..] } else { rgb };
    parse_rgb(&format!("#{rgb}"))
}
fn a1(s: &str) -> Option<(usize, usize)> {
    let s = s.trim_matches('$');
    let idx = s.find(|ch: char| ch.is_ascii_digit())?;
    let mut col = 0usize;
    for c in s[..idx].bytes() {
        if !c.is_ascii_alphabetic() {
            return None;
        }
        col = col
            .checked_mul(26)?
            .checked_add((c.to_ascii_uppercase() - b'A' + 1) as usize)?
    }
    let row = s[idx..].replace('$', "").parse::<usize>().ok()?;
    Some((row.checked_sub(1)?, col.checked_sub(1)?))
}
fn import_xlsx_layout(bytes: &[u8], state: &mut State) -> Result<(), Error> {
    let mut archive =
        zip::ZipArchive::new(Cursor::new(bytes)).map_err(|_| Error::Invalid("invalid XLSX"))?;
    let (Some(workbook), Some(rels)) = (
        zip_text(&mut archive, "xl/workbook.xml")?,
        zip_text(&mut archive, "xl/_rels/workbook.xml.rels")?,
    ) else {
        return Ok(());
    };
    let workbook = xml(&workbook)?;
    let rels = xml(&rels)?;
    let mut targets = BTreeMap::new();
    for rel in rels
        .descendants()
        .filter(|n| n.has_tag_name("Relationship"))
    {
        if let (Some(id), Some(t)) = (attrs(rel, "Id"), attrs(rel, "Target")) {
            let path = if t.starts_with('/') {
                t.trim_start_matches('/').to_string()
            } else {
                format!("xl/{t}")
            };
            if !path.split('/').any(|s| s == "..") {
                targets.insert(id.to_owned(), path);
            }
        }
    }
    let mut formats = vec![CellStyle::default()];
    if let Some(styles) = zip_text(&mut archive, "xl/styles.xml")? {
        let d = xml(&styles)?;
        let root = d.root_element();
        let fonts: Vec<_> = child(root, "fonts")
            .map(|n| n.children().filter(|x| x.has_tag_name("font")).collect())
            .unwrap_or_default();
        let fills: Vec<_> = child(root, "fills")
            .map(|n| n.children().filter(|x| x.has_tag_name("fill")).collect())
            .unwrap_or_default();
        let borders: Vec<_> = child(root, "borders")
            .map(|n| n.children().filter(|x| x.has_tag_name("border")).collect())
            .unwrap_or_default();
        let mut numberformats = BTreeMap::new();
        if let Some(n) = child(root, "numFmts") {
            for fmt in n.children().filter(|n| n.has_tag_name("numFmt")) {
                if let (Some(id), Some(code)) = (
                    fmt.attribute("numFmtId")
                        .and_then(|s| s.parse::<usize>().ok()),
                    fmt.attribute("formatCode"),
                ) {
                    numberformats.insert(id, code.to_owned());
                }
            }
        }
        if let Some(xfs) = child(root, "cellXfs") {
            formats.clear();
            for xf in xfs.children().filter(|n| n.has_tag_name("xf")) {
                let mut style = CellStyle::default();
                let index = |name| {
                    xf.attribute(name)
                        .and_then(|x| x.parse::<usize>().ok())
                        .unwrap_or(0)
                };
                if let Some(font) = fonts.get(index("fontId")) {
                    style.bold = child(*font, "b").is_some_and(|n| n.attribute("val") != Some("0"));
                    style.italic =
                        child(*font, "i").is_some_and(|n| n.attribute("val") != Some("0"));
                    style.underline = child(*font, "u").is_some();
                    style.font = child(*font, "name")
                        .and_then(|n| n.attribute("val"))
                        .unwrap_or("")
                        .into();
                    style.font_size = child(*font, "sz")
                        .and_then(|n| n.attribute("val"))
                        .and_then(|x| x.parse::<f64>().ok())
                        .unwrap_or(0.0)
                        .clamp(0.0, u16::MAX as f64) as u16;
                    style.foreground = xlsx_color(child(*font, "color"));
                }
                if let Some(fill) = fills.get(index("fillId")) {
                    style.background =
                        xlsx_color(child(*fill, "patternFill").and_then(|n| child(n, "fgColor")))
                }
                if let Some(border) = borders.get(index("borderId")) {
                    for (side, bit) in [("top", 1), ("right", 2), ("bottom", 4), ("left", 8)] {
                        if child(*border, side)
                            .and_then(|n| n.attribute("style"))
                            .is_some_and(|s| s != "none")
                        {
                            style.borders |= bit
                        }
                    }
                }
                let n = index("numFmtId");
                style.number_format = numberformats.get(&n).cloned().unwrap_or_else(|| {
                    match n {
                        14 => "mm-dd-yy",
                        15 => "d-mmm-yy",
                        16 => "d-mmm",
                        17 => "mmm-yy",
                        18 => "h:mm AM/PM",
                        19 => "h:mm:ss AM/PM",
                        20 => "h:mm",
                        21 => "h:mm:ss",
                        22 => "m/d/yy h:mm",
                        _ => "",
                    }
                    .into()
                });
                formats.push(style)
            }
        }
    }
    let sheet_nodes: Vec<_> = workbook
        .descendants()
        .filter(|n| n.has_tag_name("sheet"))
        .collect();
    for node in sheet_nodes {
        let Some(name) = node.attribute("name") else {
            continue;
        };
        let Some(sheet) = state.sheets.iter_mut().find(|s| s.name == name) else {
            continue;
        };
        let Some(id) = node
            .attributes()
            .find(|a| a.name() == "id")
            .map(|a| a.value())
        else {
            continue;
        };
        let Some(path) = targets.get(id) else {
            continue;
        };
        let Some(text) = zip_text(&mut archive, path)? else {
            continue;
        };
        let d = xml(&text)?;
        if let Some(pane) = d.descendants().find(|n| {
            n.has_tag_name("pane") && matches!(n.attribute("state"), Some("frozen" | "frozenSplit"))
        }) {
            let rows = pane
                .attribute("ySplit")
                .and_then(|v| v.parse::<usize>().ok())
                .unwrap_or(0);
            let cols = pane
                .attribute("xSplit")
                .and_then(|v| v.parse::<usize>().ok())
                .unwrap_or(0);
            if rows < MAX_ROWS && cols < MAX_COLS {
                sheet.rows = sheet.rows.max(rows + 1);
                sheet.cols = sheet.cols.max(cols + 1);
                sheet.view.frozen_rows = rows;
                sheet.view.frozen_columns = cols;
            }
        }
        for row in d.descendants().filter(|n| {
            n.has_tag_name("row") && matches!(n.attribute("hidden"), Some("1" | "true"))
        }) {
            if let Some(index) = row
                .attribute("r")
                .and_then(|v| v.parse::<usize>().ok())
                .and_then(|v| v.checked_sub(1))
            {
                if index < sheet.rows {
                    sheet.view.hidden_rows.insert(index);
                }
            }
        }
        for col in d.descendants().filter(|n| {
            n.has_tag_name("col") && matches!(n.attribute("hidden"), Some("1" | "true"))
        }) {
            if let (Some(min), Some(max)) = (
                col.attribute("min").and_then(|v| v.parse::<usize>().ok()),
                col.attribute("max").and_then(|v| v.parse::<usize>().ok()),
            ) {
                if min > 0 && min <= max {
                    for index in min - 1..max.min(sheet.cols) {
                        sheet.view.hidden_columns.insert(index);
                    }
                }
            }
        }
        sheet.protected = d
            .root_element()
            .children()
            .find(|n| n.has_tag_name("sheetProtection"))
            .is_some_and(|n| !matches!(n.attribute("sheet"), Some("0" | "false")));
        for row in d.descendants().filter(|n| n.has_tag_name("row")) {
            if let (Some(r), Some(h)) = (
                row.attribute("r")
                    .and_then(|x| x.parse::<usize>().ok())
                    .and_then(|x| x.checked_sub(1)),
                row.attribute("ht").and_then(|x| x.parse::<f64>().ok()),
            ) {
                if r < sheet.rows && h > 0.0 && h < 49151.0 {
                    sheet.row_heights.insert(r, (h / 0.75).round() as u16);
                }
            }
        }
        for col in d.descendants().filter(|n| n.has_tag_name("col")) {
            if let (Some(min), Some(max), Some(width)) = (
                col.attribute("min").and_then(|x| x.parse::<usize>().ok()),
                col.attribute("max").and_then(|x| x.parse::<usize>().ok()),
                col.attribute("width").and_then(|x| x.parse::<f64>().ok()),
            ) {
                if min > 0 && min <= max && max <= MAX_COLS && width > 0.0 && width < 9000.0 {
                    for c in min - 1..max.min(sheet.cols) {
                        sheet.col_widths.insert(c, (width * 7.0).round() as u16);
                    }
                }
            }
        }
        for node in d.descendants().filter(|n| n.has_tag_name("c")) {
            let Some((r, c)) = node.attribute("r").and_then(a1) else {
                continue;
            };
            if r >= sheet.rows || c >= sheet.cols {
                continue;
            }
            if node
                .children()
                .any(|n| n.has_tag_name("f") && n.attribute("t") == Some("shared"))
            {
                sheet.shared_formula_cells.push((r, c));
            }
            if let Some(formula) = node.children().find(|n| {
                n.has_tag_name("f") && matches!(n.attribute("t"), Some("array" | "dataTable"))
            }) {
                let range = formula
                    .attribute("ref")
                    .and_then(|reference| {
                        reference
                            .split_once(':')
                            .and_then(|(first, last)| Some((a1(first)?, a1(last)?)))
                            .map(|((top, left), (bottom, right))| Range {
                                top,
                                left,
                                bottom,
                                right,
                            })
                            .or_else(|| {
                                a1(reference).map(|(row, col)| Range {
                                    top: row,
                                    left: col,
                                    bottom: row,
                                    right: col,
                                })
                            })
                    })
                    .unwrap_or(Range {
                        top: r,
                        left: c,
                        bottom: r,
                        right: c,
                    });
                sheet.protected_formula_ranges.push(range);
            }
            let index = node
                .attribute("s")
                .and_then(|x| x.parse::<usize>().ok())
                .unwrap_or(0);
            if let Some(style) = formats.get(index) {
                let id = match state.styles.iter().position(|x| x == style) {
                    Some(id) => id,
                    None => {
                        state.styles.push(style.clone());
                        state.styles.len() - 1
                    }
                };
                if id != 0 {
                    sheet
                        .cells
                        .entry((r, c))
                        .or_insert_with(|| Cell {
                            text: CellContent::default(),
                            style: 0,
                        })
                        .style = id;
                }
            }
        }
        for node in d.descendants().filter(|n| n.has_tag_name("mergeCell")) {
            let Some((a, b)) = node.attribute("ref").and_then(|v| v.split_once(':')) else {
                continue;
            };
            if let (Some((top, left)), Some((bottom, right))) = (a1(a), a1(b)) {
                let m = Range {
                    top,
                    left,
                    bottom,
                    right,
                };
                if m.valid(sheet) && !sheet.merges.iter().any(|v| v.overlaps(m)) {
                    sheet.merges.push(m)
                }
            }
        }
        let (directory, filename) = path
            .rsplit_once('/')
            .ok_or(Error::Invalid("invalid sheet path"))?;
        let relpath = format!("{directory}/_rels/{filename}.rels");
        if let Some(reltext) = zip_text(&mut archive, &relpath)? {
            let relations = xml(&reltext)?;
            for relation in relations.descendants().filter(|n| {
                n.has_tag_name("Relationship")
                    && n.attribute("Type").is_some_and(|t| t.ends_with("/table"))
            }) {
                let Some(target) = relation.attribute("Target") else {
                    continue;
                };
                let joined = if target.starts_with('/') {
                    target.trim_start_matches('/').to_owned()
                } else {
                    format!("{directory}/{target}")
                };
                let mut components = Vec::new();
                for part in joined.split('/') {
                    match part {
                        "" | "." => {}
                        ".." => {
                            if components.pop().is_none() {
                                break;
                            }
                        }
                        part => components.push(part),
                    }
                }
                let path = components.join("/");
                if !path.starts_with("xl/tables/") {
                    continue;
                }
                let Some(table_text) = zip_text(&mut archive, &path)? else {
                    continue;
                };
                let table_doc = xml(&table_text)?;
                let root = table_doc.root_element();
                let Some(name) = root.attribute("name") else {
                    continue;
                };
                let Some((a, b)) = root
                    .attribute("ref")
                    .and_then(|s| s.split_once(':'))
                    .and_then(|(a, b)| Some((a1(a)?, a1(b)?)))
                else {
                    continue;
                };
                let range = Range {
                    top: a.0,
                    left: a.1,
                    bottom: b.0,
                    right: b.1,
                };
                if range.bottom >= MAX_ROWS || range.right >= MAX_COLS {
                    continue;
                }
                sheet.rows = sheet.rows.max(range.bottom + 1);
                sheet.cols = sheet.cols.max(range.right + 1);
                sheet.opaque_table_headers.push(Range {
                    top: range.top,
                    left: range.left,
                    bottom: range.top,
                    right: range.right,
                });
                let id = state.next_id;
                state.next_id = id.checked_add(1).ok_or(Error::Limit)?;
                sheet.tables.push(SpreadsheetTable {
                    id,
                    name: name.into(),
                    range,
                    total_row: root.attribute("totalsRowCount").is_some_and(|v| v != "0"),
                });
            }
        }
    }
    if tables::validate_tables(state).is_err() {
        for sheet in &mut state.sheets {
            sheet.tables.clear();
        }
    } else {
        for sheet in &mut state.sheets {
            sheet.opaque_table_headers.clear();
        }
    }
    Ok(())
}
fn changed_cells(
    old: &State,
    new: &State,
    allow_styles: bool,
    allow_geometry: bool,
    renames: &[SheetRename],
) -> Result<Vec<Vec<((usize, usize), Option<Cell>)>>, Error> {
    if (!allow_geometry && old.sheets.len() != new.sheets.len())
        || !new.styles.starts_with(&old.styles)
    {
        return Err(Error::Invalid(
            "This Office edit cannot preserve unsupported workbook content",
        ));
    }
    let mut all = Vec::new();
    for a in &old.sheets {
        let Some(b) = new.sheets.iter().find(|s| s.id == a.id) else {
            if allow_geometry {
                all.push(Vec::new());
                continue;
            } else {
                return Err(Error::Invalid(
                    "External sheet insertion or removal is not yet supported",
                ));
            }
        };
        if (!allow_geometry && a.name != b.name)
            || (!allow_geometry && (a.rows != b.rows || a.cols != b.cols))
            || (!allow_geometry && (a.row_heights != b.row_heights || a.col_widths != b.col_widths))
            || a.protected != b.protected
            || a.filter != b.filter
            || a.print != b.print
        {
            return Err(Error::Invalid(
                "Structural Office edits cannot yet preserve references",
            ));
        }
        let mut keys = std::collections::BTreeSet::new();
        keys.extend(a.cells.keys().copied());
        keys.extend(b.cells.keys().copied());
        let mut changes = Vec::new();
        for key in keys {
            let before = a.cells.get(&key);
            let after = b.cells.get(&key);
            if before == after
                || before.zip(after).is_some_and(|(a, b)| {
                    a.style == b.style && equivalent_content(&a.text, &b.text, renames)
                })
            {
                continue;
            }
            if !allow_styles && before.map_or(0, |c| c.style) != after.map_or(0, |c| c.style) {
                return Err(Error::Invalid(
                    "Office formatting edits cannot yet be preserved",
                ));
            }
            changes.push((key, after.cloned()));
        }
        all.push(changes);
    }
    Ok(all)
}
impl Workbook {
    pub(super) fn validate_external_rename(&self, state: &State) -> Result<(), Error> {
        let bytes = self
            .external_source
            .as_ref()
            .ok_or(Error::Invalid("missing source"))?;
        let old = self
            .external_baseline
            .as_ref()
            .ok_or(Error::Invalid("missing source state"))?;
        if self.external_format.as_deref() == Some("ods") {
            return patch_ods_structure(bytes, old, state).map(|_| ());
        }
        let renames = sheet_renames(old, state)?;
        let changes = changed_cells(old, state, true, true, &renames)?;
        patch_xlsx(bytes, old, state, &changes, &renames).map(|_| ())
    }
    fn patch_external(&self, kind: &str) -> Result<Vec<u8>, Error> {
        if self.external_format.as_deref() != Some(kind) {
            return Err(Error::Invalid(
                "Conversion would discard unsupported Office content; save in the original format",
            ));
        }
        let bytes = self
            .external_source
            .as_ref()
            .ok_or(Error::Invalid("missing source"))?;
        let old = self
            .external_baseline
            .as_ref()
            .ok_or(Error::Invalid("missing source state"))?;
        if kind == "ods"
            && (self
                .history
                .current()
                .sheets
                .iter()
                .any(|sheet| sheet.source_structure)
                || old.sheets.len() != self.history.current().sheets.len()
                || old
                    .sheets
                    .iter()
                    .zip(&self.history.current().sheets)
                    .any(|(a, b)| a.id != b.id || a.name != b.name))
        {
            return patch_ods_structure(bytes, old, self.history.current());
        }
        let renames = sheet_renames(old, self.history.current())?;
        let changes = changed_cells(
            old,
            self.history.current(),
            kind == "ods" || kind == "xlsx",
            kind == "xlsx",
            &renames,
        )?;
        if changes.iter().all(Vec::is_empty)
            && renames.is_empty()
            && old.sheets.len() == self.history.current().sheets.len()
            && old.sheets.iter().enumerate().all(|(i, a)| {
                self.history.current().sheets.get(i).is_some_and(|b| {
                    a.id == b.id
                        && a.rows == b.rows
                        && a.cols == b.cols
                        && !b.source_structure
                        && a.merges == b.merges
                        && a.view == b.view
                        && a.row_heights == b.row_heights
                        && a.col_widths == b.col_widths
                })
            })
        {
            return Ok(bytes.clone());
        }
        match kind {
            "xlsx" => patch_xlsx(bytes, old, self.history.current(), &changes, &renames),
            "ods" => patch_ods(bytes, old, self.history.current(), &changes),
            _ => Err(Error::Invalid("This Office format cannot yet be edited")),
        }
    }
}
fn patch_ods_structure(bytes: &[u8], old: &State, current: &State) -> Result<Vec<u8>, Error> {
    let mut archive =
        zip::ZipArchive::new(Cursor::new(bytes)).map_err(|_| Error::Invalid("Invalid ODS"))?;
    let content =
        zip_text(&mut archive, "content.xml")?.ok_or(Error::Invalid("Missing ODS content"))?;
    let d = xml(&content)?;
    let spreadsheet = d
        .descendants()
        .find(|n| n.has_tag_name("spreadsheet"))
        .ok_or(Error::Invalid("Missing ODS spreadsheet"))?;
    if current.sheets.iter().any(|sheet| sheet.source_structure) {
        if d.descendants().any(|n| {
            n.is_element()
                && (matches!(
                    n.tag_name().name(),
                    "named-expressions"
                        | "database-ranges"
                        | "data-validations"
                        | "conditional-formats"
                        | "annotation"
                        | "table-source"
                ) || n.tag_name().namespace().is_some_and(|ns| {
                    ns.contains(":drawing:") || ns.contains(":chart:") || ns.contains(":form:")
                }))
        }) {
            return Err(Error::Invalid(
                "ODS contains references or objects that cannot yet be shifted safely",
            ));
        }
        if old
            .sheets
            .iter()
            .any(|sheet| !sheet.row_heights.is_empty() || !sheet.col_widths.is_empty())
        {
            return Err(Error::Invalid(
                "ODS structural editing of styles, merges, or geometry is not yet supported",
            ));
        }
        if d.descendants()
            .filter(|n| {
                n.is_element()
                    && matches!(
                        n.tag_name().name(),
                        "table-row" | "table-column" | "table-cell"
                    )
            })
            .any(|n| {
                n.attributes().any(|a| {
                    matches!(a.name(), "visibility")
                        || a.name() == "style-name" && !n.has_tag_name("table-cell")
                })
            })
        {
            return Err(Error::Invalid(
                "ODS structural editing of styled or hidden cells is not yet supported",
            ));
        }
    }
    let tables: Vec<_> = spreadsheet
        .children()
        .filter(|n| n.has_tag_name("table"))
        .collect();
    if tables.len() != old.sheets.len() {
        return Err(Error::Invalid("Unsupported ODS sheet structure"));
    }
    let removed_or_renamed: Vec<_> = old
        .sheets
        .iter()
        .filter(|source| current.sheets.iter().all(|s| s.id != source.id))
        .collect();
    let renames = sheet_renames(old, current)?;
    if current
        .sheets
        .iter()
        .filter(|s| !old.sheets.iter().any(|o| o.id == s.id))
        .any(|s| renames.iter().any(|r| r.old.eq_ignore_ascii_case(&s.name)))
    {
        return Err(Error::Invalid(
            "Save and reopen before reusing a renamed ODS sheet name",
        ));
    }
    if !renames.is_empty() || !removed_or_renamed.is_empty() {
        let paths: Vec<_> = archive
            .file_names()
            .filter(|name| name.ends_with(".xml") && *name != "content.xml")
            .map(str::to_owned)
            .collect();
        for path in paths {
            if let Some(text) = zip_text(&mut archive, &path)? {
                if renames.iter().any(|r| ods_has_sheet_ref(&text, &r.old))
                    || removed_or_renamed
                        .iter()
                        .any(|s| ods_has_sheet_ref(&text, &s.name))
                {
                    return Err(Error::Invalid(
                        "ODS package metadata refers to a renamed or removed sheet",
                    ));
                }
            }
        }
    }
    for node in d.descendants().filter(|n| n.is_element()) {
        let owner = node
            .ancestors()
            .find(|n| n.parent() == Some(spreadsheet) && n.has_tag_name("table"));
        let owner_index = owner.and_then(|n| tables.iter().position(|t| *t == n));
        for attr in node.attributes() {
            if !matches!(
                attr.name(),
                "formula"
                    | "cell-range-address"
                    | "base-cell-address"
                    | "target-range-address"
                    | "print-ranges"
            ) {
                continue;
            }
            for source in &removed_or_renamed {
                let source_index = old.sheets.iter().position(|s| s.id == source.id).unwrap();
                if current.sheets.iter().all(|s| s.id != source.id)
                    && owner_index == Some(source_index)
                {
                    continue;
                }
                if ods_has_sheet_ref(attr.value(), &source.name) {
                    return Err(Error::Invalid(
                        "Referenced ODS sheets cannot yet be renamed or deleted safely",
                    ));
                }
            }
        }
    }
    let mut replacements = Vec::new();
    let table_prefix = tables
        .first()
        .map(|node| prefix(&content[node.range()]))
        .unwrap_or("table:");
    for sheet in &current.sheets {
        let source_id = if old.sheets.iter().any(|s| s.id == sheet.id) {
            Some(sheet.id)
        } else {
            sheet.copied_from
        };
        let value = if let Some(id) = source_id {
            let index = old
                .sheets
                .iter()
                .position(|s| s.id == id)
                .ok_or(Error::Invalid("Missing ODS copy source"))?;
            let node = tables[index];
            if sheet.copied_from.is_some()
                && node.descendants().any(|n| {
                    n.is_element()
                        && (n.tag_name().namespace().is_some_and(|ns| {
                            ns.contains(":drawing:")
                                || ns.contains(":chart:")
                                || ns.contains(":form:")
                        }) || matches!(n.tag_name().name(), "annotation" | "table-source"))
                })
            {
                return Err(Error::Invalid(
                    "Duplicating ODS sheets with linked objects is not yet supported",
                ));
            }
            let attr = node
                .attributes()
                .find(|a| a.name() == "name")
                .ok_or(Error::Invalid("Missing ODS sheet name"))?;
            let mut text = content[node.range()].to_owned();
            let start = attr.range_value().start - node.range().start;
            let end = attr.range_value().end - node.range().start;
            text.replace_range(start..end, &escape_xml(&sheet.name));
            text
        } else {
            format!(
                "<{table_prefix}table {table_prefix}name=\"{}\"><{table_prefix}table-row><{table_prefix}table-cell/></{table_prefix}table-row></{table_prefix}table>",
                escape_xml(&sheet.name)
            )
        };
        replacements.push(value);
    }
    if tables.is_empty() {
        return Err(Error::Invalid("ODS has no sheets"));
    }
    for pair in tables.windows(2) {
        if spreadsheet.children().any(|n| {
            n.is_element()
                && !n.has_tag_name("table")
                && n.range().start > pair[0].range().end
                && n.range().end < pair[1].range().start
        }) {
            return Err(Error::Invalid(
                "Interleaved ODS sheet metadata cannot yet be reordered safely",
            ));
        }
    }
    let mut xml_edits = Vec::new();
    for (index, node) in tables.iter().enumerate() {
        xml_edits.push((
            node.range().start,
            node.range().end,
            if index == 0 {
                replacements.join("")
            } else {
                String::new()
            },
        ));
    }
    let transformed = apply_edits(content, xml_edits);
    let transformed = rewrite_ods_structure_refs(&transformed, old, current, &renames)?;
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for index in 0..archive.len() {
        let file = archive
            .by_index(index)
            .map_err(|_| Error::Invalid("Invalid ODS entry"))?;
        let name = file.name().to_owned();
        if name == EXTRA {
            continue;
        }
        if name == "content.xml" {
            writer
                .start_file(
                    name,
                    zip::write::SimpleFileOptions::default().compression_method(file.compression()),
                )
                .map_err(|_| Error::Invalid("Cannot write ODS"))?;
            writer.write_all(transformed.as_bytes())?;
        } else {
            writer
                .raw_copy_file(file)
                .map_err(|_| Error::Invalid("Cannot preserve ODS part"))?;
        }
    }
    let temporary = writer
        .finish()
        .map_err(|_| Error::Invalid("Cannot write ODS"))?
        .into_inner();
    let imported = Workbook::from_office_bytes(&temporary, "ods")?;
    let mut baseline = imported.history.current().clone();
    let mut desired = current.clone();
    if baseline.sheets.len() != desired.sheets.len() {
        return Err(Error::Invalid("Cannot reconstruct ODS sheets"));
    }
    let mut styles = baseline.styles.clone();
    let mut mapping = Vec::new();
    for style in &desired.styles {
        let index = if let Some(index) = styles.iter().position(|s| s == style) {
            index
        } else {
            styles.push(style.clone());
            styles.len() - 1
        };
        mapping.push(index);
    }
    for (before, after) in baseline.sheets.iter_mut().zip(&mut desired.sheets) {
        before.id = after.id;
        before.rows = after.rows;
        before.cols = after.cols;
        for cell in after.cells.values_mut() {
            cell.style = *mapping.get(cell.style).ok_or(Error::Bounds)?;
        }
    }
    desired.styles = styles;
    let changes = changed_cells(&baseline, &desired, true, false, &[])?;
    patch_ods(&temporary, &baseline, &desired, &changes)
}
fn rewrite_ods_structure_refs(
    text: &str,
    old: &State,
    current: &State,
    renames: &[SheetRename],
) -> Result<String, Error> {
    let d = xml(text)?;
    let spreadsheet = d
        .descendants()
        .find(|n| n.has_tag_name("spreadsheet"))
        .ok_or(Error::Invalid("Missing spreadsheet"))?;
    let tables: Vec<_> = spreadsheet
        .children()
        .filter(|n| n.has_tag_name("table"))
        .collect();
    let mut edits = Vec::new();
    for node in d.descendants().filter(|n| n.is_element()) {
        let owner = node
            .ancestors()
            .find(|n| n.parent() == Some(spreadsheet) && n.has_tag_name("table"));
        let sheet = owner
            .and_then(|n| tables.iter().position(|t| *t == n))
            .and_then(|i| current.sheets.get(i));
        for attr in node.attributes().filter(|a| {
            matches!(
                a.name(),
                "formula"
                    | "cell-range-address"
                    | "base-cell-address"
                    | "target-range-address"
                    | "print-ranges"
            )
        }) {
            let mut value = attr.value().to_owned();
            if let Some(copy) = sheet.and_then(|s| {
                s.copied_from.and_then(|id| {
                    old.sheets
                        .iter()
                        .find(|o| o.id == id)
                        .map(|source| (s, source))
                })
            }) {
                value = rewrite_ods_ref(&value, &copy.1.name, &copy.0.name);
            }
            for rename in renames {
                value = rewrite_ods_ref(&value, &rename.old, &rename.temporary);
            }
            for rename in renames {
                value = rewrite_ods_ref(&value, &rename.temporary, &rename.new);
            }
            if value != attr.value() {
                edits.push((
                    attr.range_value().start,
                    attr.range_value().end,
                    escape_xml(&value),
                ));
            }
        }
    }
    Ok(apply_edits(text.to_owned(), edits))
}
fn rewrite_ods_ref(input: &str, old: &str, new: &str) -> String {
    let quoted = format!("'{}'.", old.replace('\'', "''"));
    let plain = format!("{}.", old);
    let replacement = format!("'{}'.", new.replace('\'', "''"));
    let bytes = input.as_bytes();
    let mut i = 0;
    let mut out = String::new();
    let mut in_string = false;
    while i < bytes.len() {
        if bytes[i] == b'"' {
            out.push('"');
            i += 1;
            if in_string && bytes.get(i) == Some(&b'"') {
                out.push('"');
                i += 1;
            } else {
                in_string = !in_string;
            }
            continue;
        }
        if !in_string
            && (i == 0
                || !bytes[i - 1].is_ascii_alphanumeric()
                    && !matches!(bytes[i - 1], b'_' | b'.' | b'\''))
        {
            let token = [&quoted, &plain].into_iter().find(|token| {
                input
                    .get(i..i + token.len())
                    .is_some_and(|part| part.eq_ignore_ascii_case(token))
            });
            if let Some(token) = token {
                out.push_str(&replacement);
                i += token.len();
                continue;
            }
        }
        let ch = input[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}
fn ods_has_sheet_ref(value: &str, name: &str) -> bool {
    let plain = format!("{}.", name).to_lowercase();
    let quoted = format!("'{}'.", name.replace('\'', "''")).to_lowercase();
    let value = value.to_lowercase();
    value.contains(&plain) || value.contains(&quoted)
}
fn patch_ods_settings(text: &str, old: &State, current: &State) -> Result<String, Error> {
    let d = xml(text)?;
    let mut edits = Vec::new();
    let tables = d
        .descendants()
        .find(|n| {
            n.has_tag_name("config-item-map-named")
                && n.attributes()
                    .any(|a| a.name() == "name" && a.value() == "Tables")
        })
        .ok_or(Error::Invalid("ODS sheet view settings are missing"))?;
    for (before, after) in old.sheets.iter().zip(&current.sheets) {
        if before.view.frozen_rows == after.view.frozen_rows
            && before.view.frozen_columns == after.view.frozen_columns
        {
            continue;
        }
        let entry = tables
            .children()
            .find(|n| {
                n.has_tag_name("config-item-map-entry")
                    && n.attributes()
                        .any(|a| a.name() == "name" && a.value() == before.name)
            })
            .ok_or(Error::Invalid("ODS sheet view entry is missing"))?;
        if entry.descendants().any(|n| {
            n.has_tag_name("config-item")
                && n.attributes().any(|a| {
                    a.name() == "name"
                        && matches!(a.value(), "HorizontalSplitMode" | "VerticalSplitMode")
                })
                && n.text() == Some("1")
        }) {
            return Err(Error::Invalid(
                "Movable ODS splits cannot yet be changed safely",
            ));
        }
        for (key, value) in [
            (
                "HorizontalSplitMode",
                if after.view.frozen_columns > 0 { 2 } else { 0 },
            ),
            (
                "VerticalSplitMode",
                if after.view.frozen_rows > 0 { 2 } else { 0 },
            ),
            ("HorizontalSplitPosition", after.view.frozen_columns),
            ("VerticalSplitPosition", after.view.frozen_rows),
            ("PositionRight", after.view.frozen_columns),
            ("PositionBottom", after.view.frozen_rows),
        ] {
            let item = entry
                .children()
                .find(|n| {
                    n.has_tag_name("config-item")
                        && n.attributes()
                            .any(|a| a.name() == "name" && a.value() == key)
                })
                .ok_or(Error::Invalid("ODS split setting is missing"))?;
            let child = item
                .children()
                .find(|n| n.is_text())
                .ok_or(Error::Invalid("Invalid ODS split setting"))?;
            edits.push((child.range().start, child.range().end, value.to_string()));
        }
    }
    Ok(apply_edits(text.to_owned(), edits))
}
fn shift_ods_cell(
    mut row: usize,
    mut col: usize,
    edits: &[StructuralEdit],
) -> Option<(usize, usize)> {
    for edit in edits {
        let value = if edit.rows { &mut row } else { &mut col };
        if edit.delete && *value >= edit.at && *value < edit.at + edit.count {
            return None;
        }
        if edit.delete && *value >= edit.at + edit.count {
            *value -= edit.count;
        } else if !edit.delete && *value >= edit.at {
            *value = value.checked_add(edit.count)?;
        }
    }
    Some((row, col))
}
fn patch_ods(
    bytes: &[u8],
    old: &State,
    current: &State,
    changes: &[Vec<((usize, usize), Option<Cell>)>],
) -> Result<Vec<u8>, Error> {
    use spreadsheet_ods::Value;
    let mut book = spreadsheet_ods::read_ods_buf(bytes)
        .map_err(|_| Error::Invalid("cannot read original ODS"))?;
    let view_changed = old
        .sheets
        .iter()
        .zip(&current.sheets)
        .any(|(before, after)| before.view != after.view);
    for (i, (before, after)) in old.sheets.iter().zip(&current.sheets).enumerate() {
        for row in before
            .view
            .hidden_rows
            .symmetric_difference(&after.view.hidden_rows)
        {
            if after.view.hidden_rows.contains(row)
                && book.sheet(i).used_grid_size().0 <= *row as u32
            {
                book.sheet_mut(i)
                    .set_value(*row as u32, 0, spreadsheet_ods::Value::Empty);
            }
            book.sheet_mut(i).set_row_visible(
                *row as u32,
                if after.view.hidden_rows.contains(row) {
                    spreadsheet_ods::sheet::Visibility::Collapsed
                } else {
                    spreadsheet_ods::sheet::Visibility::Visible
                },
            );
        }
        for col in before
            .view
            .hidden_columns
            .symmetric_difference(&after.view.hidden_columns)
        {
            if after.view.hidden_columns.contains(col)
                && book.sheet(i).used_grid_size().1 <= *col as u32
            {
                book.sheet_mut(i)
                    .set_value(0, *col as u32, spreadsheet_ods::Value::Empty);
            }
            book.sheet_mut(i).set_col_visible(
                *col as u32,
                if after.view.hidden_columns.contains(col) {
                    spreadsheet_ods::sheet::Visibility::Collapsed
                } else {
                    spreadsheet_ods::sheet::Visibility::Visible
                },
            );
        }
    }
    let mut style_seq = 0usize;
    let mut shifted_style_ids: Vec<BTreeMap<(usize, usize), usize>> =
        vec![BTreeMap::new(); old.sheets.len()];
    let mut style_scan = 0usize;
    for (i, (before, after)) in old.sheets.iter().zip(&current.sheets).enumerate() {
        if !after.source_structure {
            continue;
        }
        style_scan = style_scan.saturating_add(before.rows.saturating_mul(before.cols));
        if style_scan > 1_000_000 {
            return Err(Error::Limit);
        }
        let mut styles = Vec::new();
        for row in 0..before.rows {
            for col in 0..before.cols {
                if let Some(reference) = book.sheet(i).cellstyle(row as u32, col as u32).cloned() {
                    let id = before.cells.get(&(row, col)).map_or(0, |c| c.style);
                    styles.push((
                        (row, col),
                        shift_ods_cell(row, col, &after.source_edits),
                        reference,
                        id,
                    ));
                }
            }
        }
        for ((row, col), _, _, _) in &styles {
            book.sheet_mut(i).clear_cellstyle(*row as u32, *col as u32);
        }
        for (_, destination, reference, id) in styles {
            if let Some((row, col)) =
                destination.filter(|(r, c)| *r < after.rows && *c < after.cols)
            {
                book.sheet_mut(i)
                    .set_cellstyle(row as u32, col as u32, &reference);
                shifted_style_ids[i].insert((row, col), id);
            }
        }
    }
    for (i, (before, after)) in old.sheets.iter().zip(&current.sheets).enumerate() {
        for m in &before.merges {
            if !after.merges.contains(m) {
                let sheet = book.sheet_mut(i);
                sheet.set_row_span(m.top as u32, m.left as u32, 1);
                sheet.set_col_span(m.top as u32, m.left as u32, 1);
            }
        }
        for m in &after.merges {
            if !before.merges.contains(m) {
                let sheet = book.sheet_mut(i);
                sheet.set_row_span(m.top as u32, m.left as u32, (m.bottom - m.top + 1) as u32);
                sheet.set_col_span(m.top as u32, m.left as u32, (m.right - m.left + 1) as u32);
            }
        }
    }
    for (i, cells) in changes.iter().enumerate() {
        for &((row, col), ref value) in cells {
            let (r, c) = (row as u32, col as u32);
            let before = old.sheets[i].cells.get(&(row, col));
            let after = value.as_ref();
            let old_id = if current.sheets[i].source_structure {
                shifted_style_ids[i].get(&(row, col)).copied().unwrap_or(0)
            } else {
                before.map_or(0, |v| v.style)
            };
            let new_id = after.map_or(0, |v| v.style);
            let replacement_style = if old_id != new_id {
                let prior = &old.styles[old_id];
                let next = &current.styles[new_id];
                if prior.number_format != next.number_format {
                    return Err(Error::Invalid(
                        "ODS number-format editing is not yet supported",
                    ));
                }
                let source_ref = book.sheet(i).cellstyle(r, c).cloned();
                let mut style = source_ref
                    .as_ref()
                    .and_then(|s| book.cellstyle(s.as_ref()))
                    .cloned()
                    .unwrap_or_else(spreadsheet_ods::CellStyle::new_empty);
                loop {
                    style_seq += 1;
                    let name = format!("XLS42ExternalStyle{style_seq}");
                    if book.cellstyle(&name).is_none() {
                        style.set_name(name);
                        break;
                    }
                }
                style.set_origin(spreadsheet_ods::style::StyleOrigin::Content);
                style.set_styleuse(spreadsheet_ods::style::StyleUse::Automatic);
                update_ods_style(&mut style, prior, next);
                Some(book.add_cellstyle(style))
            } else {
                None
            };
            let sheet = book.sheet_mut(i);
            if let Some(ref style) = replacement_style {
                sheet.set_cellstyle(r, c, style)
            }
            let value_changed = before.map(|v| &v.text) != after.map(|v| &v.text);
            if !value_changed {
                continue;
            }
            sheet.clear_formula(r, c);
            match after.map(|v| &v.text) {
                None => sheet.set_value(r, c, Value::Empty),
                Some(CellContent::Text(v)) => sheet.set_value(r, c, v.raw()),
                Some(CellContent::Number(v)) => sheet.set_value(
                    r,
                    c,
                    v.raw()
                        .parse::<f64>()
                        .map_err(|_| Error::Invalid("invalid number"))?,
                ),
                Some(CellContent::Boolean(v)) => {
                    sheet.set_value(r, c, v.raw().eq_ignore_ascii_case("true"))
                }
                Some(CellContent::Formula(v)) => {
                    sheet.set_value(r, c, Value::Empty);
                    sheet.set_formula(r, c, ods_formula(v.raw())?)
                }
                _ => {
                    return Err(Error::Invalid(
                        "editing dates and errors in ODS is unsupported",
                    ))
                }
            }
        }
    }
    let generated = spreadsheet_ods::write_ods_buf(&mut book, Vec::new())
        .map_err(|_| Error::Invalid("cannot write ODS without loss"))?;
    verify_ods_untouched_styles(bytes, &generated, changes, old, current)?;
    let mut original =
        zip::ZipArchive::new(Cursor::new(bytes)).map_err(|_| Error::Invalid("invalid ODS"))?;
    let mut generated_zip = zip::ZipArchive::new(Cursor::new(generated.as_slice()))
        .map_err(|_| Error::Invalid("invalid ODS output"))?;
    let before =
        zip_text(&mut original, "content.xml")?.ok_or(Error::Invalid("missing ODS content"))?;
    verify_ods_repeated_styles(&before)?;
    let after = zip_text(&mut generated_zip, "content.xml")?
        .ok_or(Error::Invalid("missing generated ODS content"))?;
    let after = preserve_ods_auto_styles(&before, &after)?;
    verify_ods_opaque_content(&before, &after, view_changed)?;
    if view_changed {
        let parsed = spreadsheet_ods::read_ods_buf(&generated)
            .map_err(|_| Error::Invalid("Invalid ODS view output"))?;
        let prior = spreadsheet_ods::read_ods_buf(bytes)
            .map_err(|_| Error::Invalid("Invalid ODS source"))?;
        for (i, sheet) in current.sheets.iter().enumerate() {
            for row in 0..sheet.rows {
                let hidden = parsed.sheet(i).row_visible(row as u32)
                    != spreadsheet_ods::sheet::Visibility::Visible;
                if old.sheets[i].view.hidden_rows.contains(&row)
                    == sheet.view.hidden_rows.contains(&row)
                    && prior.sheet(i).row_visible(row as u32)
                        != parsed.sheet(i).row_visible(row as u32)
                {
                    return Err(Error::Invalid(
                        "Unchanged ODS row visibility would be altered",
                    ));
                }
                if hidden != sheet.view.hidden_rows.contains(&row) {
                    return Err(Error::Invalid("ODS row visibility could not be preserved"));
                }
            }
            for col in 0..sheet.cols {
                let hidden = parsed.sheet(i).col_visible(col as u32)
                    != spreadsheet_ods::sheet::Visibility::Visible;
                if old.sheets[i].view.hidden_columns.contains(&col)
                    == sheet.view.hidden_columns.contains(&col)
                    && prior.sheet(i).col_visible(col as u32)
                        != parsed.sheet(i).col_visible(col as u32)
                {
                    return Err(Error::Invalid(
                        "Unchanged ODS column visibility would be altered",
                    ));
                }
                if hidden != sheet.view.hidden_columns.contains(&col) {
                    return Err(Error::Invalid(
                        "ODS column visibility could not be preserved",
                    ));
                }
            }
        }
    }
    let freeze_changed = old.sheets.iter().zip(&current.sheets).any(|(a, b)| {
        a.view.frozen_rows != b.view.frozen_rows || a.view.frozen_columns != b.view.frozen_columns
    });
    let settings = if freeze_changed {
        let settings = zip_text(&mut original, "settings.xml")?
            .ok_or(Error::Invalid("ODS view settings are missing"))?;
        Some(patch_ods_settings(&settings, old, current)?)
    } else {
        None
    };

    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for i in 0..original.len() {
        let file = original
            .by_index(i)
            .map_err(|_| Error::Invalid("invalid ODS entry"))?;
        let name = file.name().to_owned();
        if name == EXTRA {
            continue;
        }
        if name == "settings.xml" && settings.is_some() {
            let opts =
                zip::write::SimpleFileOptions::default().compression_method(file.compression());
            writer
                .start_file(name, opts)
                .map_err(|_| Error::Invalid("cannot write ODS settings"))?;
            writer.write_all(settings.as_ref().unwrap().as_bytes())?;
        } else if name == "content.xml" {
            let opts =
                zip::write::SimpleFileOptions::default().compression_method(file.compression());
            writer
                .start_file(name, opts)
                .map_err(|_| Error::Invalid("cannot write ODS content"))?;
            writer.write_all(after.as_bytes())?;
        } else {
            writer
                .raw_copy_file(file)
                .map_err(|_| Error::Invalid("cannot preserve ODS entry"))?;
        }
    }
    let out = writer
        .finish()
        .map_err(|_| Error::Invalid("cannot finish ODS"))?
        .into_inner();
    if out.len() > LIMIT {
        return Err(Error::Limit);
    }
    Ok(out)
}
pub(super) fn escape_xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}
pub(super) fn col_name(mut c: usize) -> String {
    c += 1;
    let mut s = Vec::new();
    while c > 0 {
        c -= 1;
        s.push((b'A' + (c % 26) as u8) as char);
        c /= 26
    }
    s.iter().rev().collect()
}
fn cell_xml(
    prefix: &str,
    r: usize,
    c: usize,
    cell: Option<&Cell>,
    style: Option<&str>,
) -> Result<String, Error> {
    let pos = format!("{}{}", col_name(c), r + 1);
    let mut out = format!("<{prefix}c r=\"{pos}\"");
    if let Some(s) = style {
        out.push_str(&format!(" s=\"{}\"", escape_xml(s)))
    }
    let Some(cell) = cell else {
        out.push_str("/>");
        return Ok(out);
    };
    let raw = cell.text.raw();
    if raw
        .chars()
        .any(|c| !matches!(c, '\t' | '\n' | '\r') && (c as u32) < 32)
    {
        return Err(Error::Invalid("cell contains XML control characters"));
    }
    match &cell.text {
        CellContent::Text(_) => {
            out.push_str(&format!(" t=\"inlineStr\"><{prefix}is><{prefix}t xml:space=\"preserve\">{}</{prefix}t></{prefix}is></{prefix}c>",escape_xml(raw)));
        }
        CellContent::Number(_) => out.push_str(&format!(
            "><{prefix}v>{}</{prefix}v></{prefix}c>",
            escape_xml(raw)
        )),
        CellContent::Boolean(_) => out.push_str(&format!(
            " t=\"b\"><{prefix}v>{}</{prefix}v></{prefix}c>",
            if raw.eq_ignore_ascii_case("true") {
                "1"
            } else {
                "0"
            }
        )),
        CellContent::Formula(_) => out.push_str(&format!(
            "><{prefix}f>{}</{prefix}f></{prefix}c>",
            escape_xml(
                raw.strip_prefix('=')
                    .ok_or(Error::Invalid("invalid formula"))?
            )
        )),
        _ => {
            return Err(Error::Invalid(
                "editing dates or errors in XLSX is unsupported",
            ))
        }
    }
    Ok(out)
}
pub(super) fn prefix(raw: &str) -> &str {
    let Some(start) = raw.find('<') else {
        return "";
    };
    let name = raw[start + 1..]
        .split(|c: char| c.is_whitespace() || c == '>' || c == '/')
        .next()
        .unwrap_or("");
    if let Some(i) = name.find(':') {
        &name[..=i]
    } else {
        ""
    }
}
pub(super) fn apply_edits(mut text: String, mut edits: Vec<(usize, usize, String)>) -> String {
    edits.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| b.1.cmp(&a.1)));
    for (a, b, value) in edits {
        text.replace_range(a..b, &value)
    }
    text
}
fn patch_row(
    text: &str,
    row: roxmltree::Node<'_, '_>,
    changes: &[((usize, usize), Option<Cell>)],
    old: &Sheet,
    styles: &BTreeMap<(usize, usize), usize>,
    renames: &[SheetRename],
) -> Result<String, Error> {
    let rr = row.range();
    let original = &text[rr.clone()];
    let p = prefix(original);
    let mut edits = Vec::new();
    let mut insertions: BTreeMap<usize, Vec<String>> = BTreeMap::new();
    let existing: Vec<_> = row.children().filter(|n| n.has_tag_name("c")).collect();
    let closing = original.rfind("</");
    let self_closing = original.trim_end().ends_with("/>");
    for ((r, c), value) in changes {
        if let Some(node) = existing
            .iter()
            .copied()
            .find(|n| n.attribute("r").and_then(a1) == Some((*r, *c)))
        {
            let desired_style = styles.get(&(*r, *c)).map(usize::to_string);
            let value_changed = match (old.cells.get(&(*r, *c)), value.as_ref()) {
                (Some(a), Some(b)) => !equivalent_content(&a.text, &b.text, renames),
                (None, None) => false,
                _ => true,
            };
            if !value_changed {
                if let Some(style) = desired_style {
                    let range = node.range();
                    let cell = crate::xlsx_styles::attrs(&text[range.clone()], &[("s", style)])?;
                    edits.push((range.start - rr.start, range.end - rr.start, cell));
                }
                continue;
            }
            if node
                .attributes()
                .any(|a| !matches!(a.name(), "r" | "s" | "t"))
                || node
                    .children()
                    .any(|n| n.is_element() && !matches!(n.tag_name().name(), "v" | "f" | "is"))
                || node
                    .children()
                    .filter(|n| n.has_tag_name("f"))
                    .any(|n| n.attributes().len() > 0)
                || node
                    .children()
                    .filter(|n| n.has_tag_name("is"))
                    .any(|is| is.descendants().any(|n| n.has_tag_name("r")))
            {
                return Err(Error::Invalid(
                    "This cell has rich or shared metadata that cannot yet be edited safely",
                ));
            }
            let range = node.range();
            edits.push((
                range.start - rr.start,
                range.end - rr.start,
                cell_xml(
                    p,
                    *r,
                    *c,
                    value.as_ref(),
                    desired_style.as_deref().or(node.attribute("s")),
                )?,
            ));
        } else if value.is_some() {
            let position = existing
                .iter()
                .find_map(|n| {
                    let (_, col) = n.attribute("r").and_then(a1)?;
                    if col > *c {
                        Some(n.range().start - rr.start)
                    } else {
                        None
                    }
                })
                .or(closing)
                .unwrap_or(original.len());
            insertions.entry(position).or_default().push(cell_xml(
                p,
                *r,
                *c,
                value.as_ref(),
                styles.get(&(*r, *c)).map(usize::to_string).as_deref(),
            )?);
        }
    }
    if self_closing && !insertions.is_empty() {
        let at = original.rfind("/>").ok_or(Error::Invalid("invalid row"))?;
        let values = insertions.into_values().flatten().collect::<String>();
        return Ok(format!("{}>{values}</{p}row>", &original[..at]));
    }
    for (pos, values) in insertions {
        edits.push((pos, pos, values.join("")))
    }
    Ok(apply_edits(original.to_owned(), edits))
}
fn patch_sheet(
    text: &str,
    changes: &[((usize, usize), Option<Cell>)],
    old: &Sheet,
    styles: &BTreeMap<(usize, usize), usize>,
    renames: &[SheetRename],
) -> Result<String, Error> {
    if changes.is_empty() {
        return Ok(text.to_owned());
    }
    let d = xml(text)?;
    let data = d
        .descendants()
        .find(|n| n.has_tag_name("sheetData"))
        .ok_or(Error::Invalid("missing XLSX sheet data"))?;
    let dr = data.range();
    let original = &text[dr.clone()];
    let p = prefix(original);
    let rows: Vec<_> = data.children().filter(|n| n.has_tag_name("row")).collect();
    let mut grouped: BTreeMap<usize, Vec<_>> = BTreeMap::new();
    for v in changes {
        grouped.entry(v.0 .0).or_default().push(v.clone())
    }
    let mut edits = Vec::new();
    let mut inserts: BTreeMap<usize, Vec<String>> = BTreeMap::new();
    for (row, items) in grouped {
        if let Some(node) = rows
            .iter()
            .copied()
            .find(|n| n.attribute("r").and_then(|x| x.parse::<usize>().ok()) == Some(row + 1))
        {
            let rr = node.range();
            edits.push((
                rr.start - dr.start,
                rr.end - dr.start,
                patch_row(text, node, &items, old, styles, renames)?,
            ));
        } else {
            let mut cells = String::new();
            for ((r, c), cell) in items {
                if cell.is_some() {
                    cells.push_str(&cell_xml(
                        p,
                        r,
                        c,
                        cell.as_ref(),
                        styles.get(&(r, c)).map(usize::to_string).as_deref(),
                    )?);
                }
            }
            if cells.is_empty() {
                continue;
            }
            let element = format!("<{p}row r=\"{}\">{cells}</{p}row>", row + 1);
            let at = rows
                .iter()
                .find_map(|n| {
                    let nr = n.attribute("r")?.parse::<usize>().ok()?;
                    if nr > row + 1 {
                        Some(n.range().start - dr.start)
                    } else {
                        None
                    }
                })
                .or_else(|| original.rfind("</"))
                .unwrap_or(original.len());
            inserts.entry(at).or_default().push(element);
        }
    }
    let inner = if original.trim_end().ends_with("/>") && !inserts.is_empty() {
        let at = original.rfind("/>").unwrap();
        format!(
            "{}>{}</{p}sheetData>",
            &original[..at],
            inserts.into_values().flatten().collect::<String>()
        )
    } else {
        for (pos, values) in inserts {
            edits.push((pos, pos, values.join("")))
        }
        apply_edits(original.to_owned(), edits)
    };
    let mut out = text.to_owned();
    out.replace_range(dr, &inner);
    Ok(out)
}
fn patch_calc(text: &str) -> Result<String, Error> {
    let doc = xml(text)?;
    let root = doc.root_element();
    let p = prefix(&text[root.range().clone()]);
    let mut result = text.to_owned();
    if let Some(node) = root.children().find(|n| n.has_tag_name("calcPr")) {
        let range = node.range();
        let original = &text[range.clone()];
        let opening = original
            .find('>')
            .ok_or(Error::Invalid("invalid calculation settings"))?;
        let insert = if original.as_bytes().get(opening.wrapping_sub(1)) == Some(&b'/') {
            opening - 1
        } else {
            opening
        };
        let mut edits = Vec::new();
        for attr in node.attributes() {
            if matches!(attr.name(), "calcMode" | "fullCalcOnLoad" | "forceFullCalc") {
                let a = attr.range();
                edits.push((a.start - range.start, a.end - range.start, String::new()));
            }
        }
        edits.push((
            insert,
            insert,
            " calcMode=\"auto\" fullCalcOnLoad=\"1\" forceFullCalc=\"1\"".into(),
        ));
        let out = apply_edits(original.to_owned(), edits);
        result.replace_range(range, &out)
    } else {
        let end = root.range().end;
        let start = text[..end]
            .rfind("</")
            .ok_or(Error::Invalid("invalid workbook XML"))?;
        result.insert_str(
            start,
            &format!("<{p}calcPr calcMode=\"auto\" fullCalcOnLoad=\"1\" forceFullCalc=\"1\"/>"),
        )
    }
    Ok(result)
}
fn update_dimension(
    text: &str,
    changes: &[((usize, usize), Option<Cell>)],
) -> Result<String, Error> {
    let doc = xml(text)?;
    let Some(node) = doc.descendants().find(|n| n.has_tag_name("dimension")) else {
        return Ok(text.to_owned());
    };
    let Some(value) = node.attribute("ref") else {
        return Ok(text.to_owned());
    };
    let (a, b) = value.split_once(':').unwrap_or((value, value));
    let Some((mut top, mut left)) = a1(a) else {
        return Ok(text.to_owned());
    };
    let Some((mut bottom, mut right)) = a1(b) else {
        return Ok(text.to_owned());
    };
    for ((r, c), v) in changes {
        if v.is_some() {
            top = top.min(*r);
            left = left.min(*c);
            bottom = bottom.max(*r);
            right = right.max(*c)
        }
    }
    let Some(attr) = node.attributes().find(|a| a.name() == "ref") else {
        return Ok(text.to_owned());
    };
    let replacement = format!(
        "ref=\"{}{}:{}{}\"",
        col_name(left),
        top + 1,
        col_name(right),
        bottom + 1
    );
    let mut out = text.to_owned();
    out.replace_range(attr.range(), &replacement);
    Ok(out)
}
fn patch_xlsx_defined_names(text: &str, old: &State, current: &State) -> Result<String, Error> {
    let d = xml(text)?;
    let mut edits = Vec::new();
    for node in d.descendants().filter(|n| n.has_tag_name("definedName")) {
        let scope = node
            .attribute("localSheetId")
            .and_then(|v| v.parse::<usize>().ok())
            .and_then(|index| current.sheets.get(index))
            .map(|sheet| sheet.id);
        for child in node.children().filter(|n| n.is_text()) {
            let original = child.text().unwrap_or("");
            if current.sheets.iter().any(|s| !s.source_edits.is_empty())
                && has_3d_sheet_reference(original)
            {
                return Err(Error::Invalid(
                    "Three-dimensional named references cannot yet be shifted safely",
                ));
            }
            let mut value = original.to_owned();
            for sheet in &current.sheets {
                let Some(source) = old.sheets.iter().find(|s| s.id == sheet.id) else {
                    continue;
                };
                for edit in &sheet.source_edits {
                    if scope == Some(sheet.id)
                        && parse_axis_range(&value, 0).is_some_and(|v| v.0 == value.len())
                    {
                        value =
                            adjust_axis_range(&value, edit.rows, edit.at, edit.count, edit.delete)
                                .unwrap_or(value);
                        continue;
                    }
                    value = adjust_formula_refs(
                        &value,
                        &source.name,
                        scope == Some(sheet.id),
                        edit.rows,
                        edit.at,
                        edit.count,
                        edit.delete,
                    );
                }
            }
            if let Some(copy) = scope.and_then(|id| {
                current.sheets.iter().find(|s| {
                    s.id == id
                        && s.copied_from.is_some()
                        && !old.sheets.iter().any(|old| old.id == id)
                })
            }) {
                for edit in &copy.source_edits {
                    value = adjust_formula_refs(
                        &value,
                        "\u{1}no-sheet",
                        true,
                        edit.rows,
                        edit.at,
                        edit.count,
                        edit.delete,
                    );
                }
            }
            if value != original {
                edits.push((child.range().start, child.range().end, escape_xml(&value)));
            }
        }
    }
    Ok(apply_edits(text.to_owned(), edits))
}
fn guard_simple_xlsx_structure(
    text: &str,
    _old: &Sheet,
    _current: &Sheet,
    _workbook: &roxmltree::Document<'_>,
    archive: &mut zip::ZipArchive<Cursor<&[u8]>>,
) -> Result<(), Error> {
    if archive.file_names().any(|name| {
        name.starts_with("xl/")
            && !(matches!(
                name,
                "xl/workbook.xml"
                    | "xl/_rels/workbook.xml.rels"
                    | "xl/styles.xml"
                    | "xl/sharedStrings.xml"
            ) || name.starts_with("xl/theme/")
                || name.starts_with("xl/tables/") && name.ends_with(".xml")
                || name.starts_with("xl/worksheets/")
                    && !name.contains("/_rels/")
                    && name.ends_with(".xml")
                || name.starts_with("xl/worksheets/_rels/") && name.ends_with(".rels"))
    }) {
        return Err(Error::Invalid(
            "This workbook has geometry or references that cannot yet be shifted safely",
        ));
    }
    let relationships: Vec<_> = archive
        .file_names()
        .filter(|name| name.starts_with("xl/worksheets/_rels/") && name.ends_with(".rels"))
        .map(str::to_owned)
        .collect();
    for path in relationships {
        let content =
            zip_text(archive, &path)?.ok_or(Error::Invalid("missing worksheet relationships"))?;
        let d = xml(&content)?;
        if d.descendants()
            .filter(|n| n.has_tag_name("Relationship"))
            .any(|n| {
                let kind = n.attribute("Type").unwrap_or("");
                !(kind.ends_with("/hyperlink") && n.attribute("TargetMode") == Some("External")
                    || kind.ends_with("/table") && n.attribute("TargetMode").is_none())
            })
        {
            return Err(Error::Invalid(
                "Linked worksheet objects cannot yet be shifted safely",
            ));
        }
    }
    let other_sheets: Vec<_> = archive
        .file_names()
        .filter(|name| {
            name.starts_with("xl/worksheets/")
                && !name.contains("/_rels/")
                && name.ends_with(".xml")
        })
        .map(str::to_owned)
        .collect();
    for path in other_sheets {
        let other = zip_text(archive, &path)?.ok_or(Error::Invalid("missing worksheet"))?;
        let other_doc = xml(&other)?;
        if other_doc
            .root_element()
            .children()
            .filter(|node| node.is_element())
            .any(|node| {
                !matches!(
                    node.tag_name().name(),
                    "dimension"
                        | "sheetViews"
                        | "sheetFormatPr"
                        | "cols"
                        | "sheetData"
                        | "mergeCells"
                        | "hyperlinks"
                        | "autoFilter"
                        | "conditionalFormatting"
                        | "dataValidations"
                        | "tableParts"
                        | "pageMargins"
                )
            })
        {
            return Err(Error::Invalid(
                "Worksheet metadata may contain references that cannot yet be shifted safely",
            ));
        }
    }
    let d = xml(text)?;
    let root = d.root_element();
    if d.descendants()
        .any(|n| n.has_tag_name("f") && matches!(n.attribute("t"), Some("array" | "dataTable")))
    {
        return Err(Error::Invalid(
            "Shared or array formulas cannot yet be shifted safely",
        ));
    }
    if d.descendants().any(|n| {
        n.has_tag_name("pane") && !matches!(n.attribute("state"), Some("frozen" | "frozenSplit"))
    }) || root.children().filter(|n| n.is_element()).any(|n| {
        !matches!(
            n.tag_name().name(),
            "dimension"
                | "sheetViews"
                | "sheetFormatPr"
                | "cols"
                | "sheetData"
                | "mergeCells"
                | "hyperlinks"
                | "autoFilter"
                | "conditionalFormatting"
                | "dataValidations"
                | "tableParts"
                | "pageMargins"
        )
    }) || d.descendants().filter(|n| n.has_tag_name("row")).any(|n| {
        n.attributes()
            .any(|a| !matches!(a.name(), "r" | "spans" | "ht" | "customHeight"))
    }) || d.descendants().filter(|n| n.has_tag_name("col")).any(|n| {
        n.attributes()
            .any(|a| !matches!(a.name(), "min" | "max" | "width" | "customWidth"))
    }) {
        return Err(Error::Invalid(
            "This worksheet contains metadata that cannot yet be shifted safely",
        ));
    }
    Ok(())
}
fn patch_xlsx_tables(
    archive: &mut zip::ZipArchive<Cursor<&[u8]>>,
    parts: &BTreeMap<SheetId, String>,
    old: &State,
    current: &State,
    replacements: &mut BTreeMap<String, String>,
) -> Result<(), Error> {
    let mut mapped = std::collections::BTreeSet::new();
    for source in &old.sheets {
        let Some(sheet) = current.sheets.iter().find(|s| s.id == source.id) else {
            continue;
        };
        let path = parts.get(&source.id).ok_or(Error::Bounds)?;
        let (directory, filename) = path
            .rsplit_once('/')
            .ok_or(Error::Invalid("Invalid worksheet path"))?;
        let relpath = format!("{directory}/_rels/{filename}.rels");
        let Some(reltext) = zip_text(archive, &relpath)? else {
            continue;
        };
        let relations = xml(&reltext)?;
        for relationship in relations.descendants().filter(|n| {
            n.has_tag_name("Relationship")
                && n.attribute("Type").is_some_and(|t| t.ends_with("/table"))
        }) {
            if sheet.source_edits.iter().any(|edit| !edit.rows) {
                return Err(Error::Invalid("Column changes in workbooks containing structured tables are not yet supported"));
            }
            let target = relationship
                .attribute("Target")
                .ok_or(Error::Invalid("Invalid table relationship"))?;
            let mut components = Vec::new();
            let joined = if target.starts_with('/') {
                target.trim_start_matches('/').to_owned()
            } else {
                format!("{directory}/{target}")
            };
            for component in joined.split('/') {
                match component {
                    "" | "." => {}
                    ".." => {
                        components
                            .pop()
                            .ok_or(Error::Invalid("Invalid table target"))?;
                    }
                    value => components.push(value),
                }
            }
            let table_path = components.join("/");
            if !table_path.starts_with("xl/tables/") || !mapped.insert(table_path.clone()) {
                return Err(Error::Invalid("Invalid or shared table target"));
            }
            let table = zip_text(archive, &table_path)?.ok_or(Error::Invalid("Missing table"))?;
            let d = xml(&table)?;
            let root = d.root_element();
            let source_table = root.attribute("name").and_then(|name| {
                source
                    .tables
                    .iter()
                    .find(|table| table.name.eq_ignore_ascii_case(name))
            });
            let target_table =
                source_table.and_then(|table| sheet.tables.iter().find(|next| next.id == table.id));
            let mut header_changes = Vec::new();
            if let (Some(before), Some(after)) = (source_table, target_table) {
                for offset in 0..=before.range.right - before.range.left {
                    let previous = source
                        .cells
                        .get(&(before.range.top, before.range.left + offset))
                        .map(|c| c.text.raw())
                        .unwrap_or("");
                    let next = sheet
                        .cells
                        .get(&(after.range.top, after.range.left + offset))
                        .map(|c| c.text.raw())
                        .unwrap_or("");
                    if previous != next {
                        header_changes.push((previous.to_owned(), next.to_owned()));
                    }
                }
            }
            let table_ref = root
                .attribute("ref")
                .ok_or(Error::Invalid("Invalid table range"))?;
            let (start, end) = table_ref
                .split_once(':')
                .and_then(|(a, b)| Some((a1(a)?, a1(b)?)))
                .ok_or(Error::Invalid("Invalid table range"))?;
            let mut bounds = Range {
                top: start.0,
                left: start.1,
                bottom: end.0,
                right: end.1,
            };
            let headers = root
                .attribute("headerRowCount")
                .and_then(|v| v.parse::<usize>().ok())
                .unwrap_or(1);
            let totals = root
                .attribute("totalsRowCount")
                .and_then(|v| v.parse::<usize>().ok())
                .unwrap_or(0);
            for edit in &sheet.source_edits {
                if edit.delete {
                    let last = edit.at + edit.count - 1;
                    let header_last = bounds.top.saturating_add(headers.saturating_sub(1));
                    let total_start = bounds.bottom.saturating_add(1).saturating_sub(totals);
                    if headers > 0 && edit.at <= header_last && last >= bounds.top
                        || totals > 0 && edit.at <= bounds.bottom && last >= total_start
                    {
                        return Err(Error::Invalid(
                            "Deleting table headers or totals is not yet supported",
                        ));
                    }
                }
                bounds = shift_range(bounds, edit.rows, edit.at, edit.count, edit.delete).ok_or(
                    Error::Invalid("Deleting an entire structured table is not yet supported"),
                )?;
            }
            if d.descendants().any(|n| n.has_tag_name("extLst")) {
                return Err(Error::Invalid(
                    "Extended tables cannot yet be shifted safely",
                ));
            }
            let mut changes = Vec::new();
            for column in d.descendants().filter(|n| n.has_tag_name("tableColumn")) {
                if let Some(attr) = column.attributes().find(|a| a.name() == "name") {
                    if let Some((_, next)) = header_changes
                        .iter()
                        .find(|(old, _)| old.eq_ignore_ascii_case(attr.value()))
                    {
                        changes.push((
                            attr.range_value().start,
                            attr.range_value().end,
                            escape_xml(next),
                        ));
                    }
                }
            }
            for node in d.descendants().filter(|n| {
                n.has_tag_name("table")
                    || n.has_tag_name("autoFilter")
                    || n.has_tag_name("sortState")
                    || n.has_tag_name("sortCondition")
            }) {
                if let Some(attr) = node.attributes().find(|a| a.name() == "ref") {
                    let mut value = attr.value().to_owned();
                    for edit in &sheet.source_edits {
                        value = adjust_refs(&value, edit.rows, edit.at, edit.count, edit.delete);
                    }
                    if value.contains("#REF!") {
                        return Err(Error::Invalid(
                            "Deleting an entire structured table is not yet supported",
                        ));
                    }
                    if value != attr.value() {
                        changes.push((
                            attr.range_value().start,
                            attr.range_value().end,
                            escape_xml(&value),
                        ));
                    }
                }
            }
            for node in d.descendants().filter(|n| {
                n.has_tag_name("calculatedColumnFormula") || n.has_tag_name("totalsRowFormula")
            }) {
                for child in node.children().filter(|n| n.is_text()) {
                    let original = child.text().unwrap_or("");
                    let mut value = original.to_owned();
                    for changed in &current.sheets {
                        let Some(target) = old.sheets.iter().find(|s| s.id == changed.id) else {
                            continue;
                        };
                        for edit in &changed.source_edits {
                            value = adjust_formula_refs(
                                &value,
                                &target.name,
                                sheet.id == changed.id,
                                edit.rows,
                                edit.at,
                                edit.count,
                                edit.delete,
                            );
                        }
                    }
                    for (previous, next) in &header_changes {
                        if let Some(table) = target_table {
                            value = crate::tables::replace_formula_token(
                                &value,
                                &format!("{}[{}]", table.name, previous),
                                &format!("{}[{}]", table.name, next),
                            );
                        }
                    }
                    if value != original {
                        changes.push((child.range().start, child.range().end, escape_xml(&value)));
                    }
                }
            }
            let modified = apply_edits(table.clone(), changes);
            if modified != table {
                replacements.insert(table_path, modified);
            }
        }
    }
    let unmapped = archive.file_names().any(|name| {
        name.starts_with("xl/tables/") && name.ends_with(".xml") && !mapped.contains(name)
    });
    if unmapped {
        return Err(Error::Invalid(
            "Unlinked table parts cannot yet be shifted safely",
        ));
    }
    Ok(())
}
fn patch_xlsx_filter_refs(text: &str, edits: &[StructuralEdit]) -> Result<String, Error> {
    if edits.is_empty() {
        return Ok(text.to_owned());
    }
    let d = xml(text)?;
    let mut updates = Vec::new();
    for filter in d.descendants().filter(|n| n.has_tag_name("autoFilter")) {
        if filter.descendants().any(|n| n.has_tag_name("extLst")) {
            return Err(Error::Invalid(
                "Extended filters cannot yet be shifted safely",
            ));
        }
        let reference = filter
            .attribute("ref")
            .ok_or(Error::Invalid("Invalid filter range"))?;
        let left = reference
            .split(':')
            .next()
            .and_then(a1)
            .map(|v| v.1)
            .ok_or(Error::Invalid("Invalid filter range"))?;
        let mut new_left = left;
        let mut value = reference.to_owned();
        for edit in edits {
            value = adjust_refs(&value, edit.rows, edit.at, edit.count, edit.delete);
        }
        if value.contains("#REF!") {
            updates.push((filter.range().start, filter.range().end, String::new()));
            continue;
        }
        new_left = value
            .split(':')
            .next()
            .and_then(a1)
            .map(|v| v.1)
            .unwrap_or(new_left);
        if let Some(attr) = filter.attributes().find(|a| a.name() == "ref") {
            if value != reference {
                updates.push((
                    attr.range_value().start,
                    attr.range_value().end,
                    escape_xml(&value),
                ));
            }
        }
        for column in filter.children().filter(|n| n.has_tag_name("filterColumn")) {
            let attr = column
                .attributes()
                .find(|a| a.name() == "colId")
                .ok_or(Error::Invalid("Invalid filter column"))?;
            let mut coordinate = left
                .checked_add(
                    attr.value()
                        .parse::<usize>()
                        .map_err(|_| Error::Invalid("Invalid filter column"))?,
                )
                .ok_or(Error::Limit)?;
            let mut removed = false;
            for edit in edits.iter().filter(|edit| !edit.rows) {
                if edit.delete && coordinate >= edit.at && coordinate < edit.at + edit.count {
                    removed = true;
                    break;
                }
                if edit.delete && coordinate >= edit.at + edit.count {
                    coordinate -= edit.count;
                } else if !edit.delete && coordinate >= edit.at {
                    coordinate += edit.count;
                }
            }
            if removed {
                updates.push((column.range().start, column.range().end, String::new()));
            } else {
                let id = coordinate
                    .checked_sub(new_left)
                    .ok_or(Error::Invalid("Invalid shifted filter column"))?
                    .to_string();
                if id != attr.value() {
                    updates.push((attr.range_value().start, attr.range_value().end, id));
                }
            }
        }
        for node in filter
            .descendants()
            .filter(|n| n.has_tag_name("sortState") || n.has_tag_name("sortCondition"))
        {
            if let Some(attr) = node.attributes().find(|a| a.name() == "ref") {
                let mut value = attr.value().to_owned();
                for edit in edits {
                    value = adjust_refs(&value, edit.rows, edit.at, edit.count, edit.delete);
                }
                if value.contains("#REF!") {
                    return Err(Error::Invalid(
                        "Removing a sorted filter range is not yet supported",
                    ));
                }
                if value != attr.value() {
                    updates.push((
                        attr.range_value().start,
                        attr.range_value().end,
                        escape_xml(&value),
                    ));
                }
            }
        }
    }
    Ok(apply_edits(text.to_owned(), updates))
}
fn patch_xlsx_rules(
    text: &str,
    sheet: &Sheet,
    old: &State,
    current: &State,
) -> Result<String, Error> {
    let d = xml(text)?;
    let mut updates = Vec::new();
    let mut removed = Vec::new();
    for node in d
        .descendants()
        .filter(|n| n.has_tag_name("dataValidation") || n.has_tag_name("conditionalFormatting"))
    {
        if node
            .descendants()
            .any(|n| n.has_tag_name("cfvo") && n.attribute("type") == Some("formula"))
        {
            return Err(Error::Invalid(
                "Formula-based conditional thresholds cannot yet be shifted safely",
            ));
        }
        if node.descendants().any(|n| n.has_tag_name("extLst")) {
            return Err(Error::Invalid(
                "Extended validation rules cannot yet be shifted safely",
            ));
        }
        if let Some(attr) = node.attributes().find(|a| a.name() == "sqref") {
            let mut parts = Vec::new();
            for reference in attr.value().split_whitespace() {
                let mut value = reference.to_owned();
                for edit in &sheet.source_edits {
                    value = adjust_refs(&value, edit.rows, edit.at, edit.count, edit.delete);
                }
                if !value.contains("#REF!") {
                    parts.push(value);
                }
            }
            if parts.is_empty() {
                removed.push(node.range());
                updates.push((node.range().start, node.range().end, String::new()));
                continue;
            }
            let value = parts.join(" ");
            if value != attr.value() {
                updates.push((
                    attr.range_value().start,
                    attr.range_value().end,
                    escape_xml(&value),
                ));
            }
        }
    }
    for node in d.descendants().filter(|n| {
        n.has_tag_name("formula") || n.has_tag_name("formula1") || n.has_tag_name("formula2")
    }) {
        if !node
            .ancestors()
            .any(|n| n.has_tag_name("dataValidation") || n.has_tag_name("conditionalFormatting"))
            || removed.iter().any(|r| r.contains(&node.range().start))
        {
            continue;
        }
        for child in node.children().filter(|n| n.is_text()) {
            let original = child.text().unwrap_or("");
            let mut value = original.to_owned();
            for changed in &current.sheets {
                let Some(source) = old.sheets.iter().find(|s| s.id == changed.id) else {
                    continue;
                };
                for edit in &changed.source_edits {
                    value = adjust_formula_refs(
                        &value,
                        &source.name,
                        sheet.id == changed.id,
                        edit.rows,
                        edit.at,
                        edit.count,
                        edit.delete,
                    );
                }
            }
            if value != original {
                updates.push((child.range().start, child.range().end, escape_xml(&value)));
            }
        }
    }
    let mut result = apply_edits(text.to_owned(), updates);
    let parsed = xml(&result)?;
    let containers: Vec<_> = parsed
        .descendants()
        .filter(|n| n.has_tag_name("dataValidations"))
        .map(|n| {
            (
                n.range(),
                n.children()
                    .filter(|c| c.has_tag_name("dataValidation"))
                    .count(),
            )
        })
        .collect();
    for (range, count) in containers.into_iter().rev() {
        let value = if count == 0 {
            String::new()
        } else {
            crate::xlsx_styles::attrs(&result[range.clone()], &[("count", count.to_string())])?
        };
        result.replace_range(range, &value);
    }
    Ok(result)
}
fn patch_xlsx_hyperlinks(
    text: &str,
    sheet: &Sheet,
    old: &State,
    current: &State,
) -> Result<String, Error> {
    let d = xml(text)?;
    let mut updates = Vec::new();
    for node in d.descendants().filter(|n| n.has_tag_name("hyperlink")) {
        if node
            .attributes()
            .any(|a| !matches!(a.name(), "ref" | "location" | "display" | "tooltip" | "id"))
        {
            return Err(Error::Invalid(
                "Extended hyperlink metadata cannot yet be shifted safely",
            ));
        }
        let Some(reference) = node.attribute("ref") else {
            return Err(Error::Invalid("Invalid hyperlink"));
        };
        let mut shifted = reference.to_owned();
        for edit in &sheet.source_edits {
            shifted = adjust_refs(&shifted, edit.rows, edit.at, edit.count, edit.delete);
        }
        if shifted.contains("#REF!") {
            updates.push((node.range().start, node.range().end, String::new()));
            continue;
        }
        if let Some(attr) = node.attributes().find(|a| a.name() == "ref") {
            if shifted != reference {
                updates.push((
                    attr.range_value().start,
                    attr.range_value().end,
                    escape_xml(&shifted),
                ));
            }
        }
        if let Some(attr) = node.attributes().find(|a| a.name() == "location") {
            let mut value = attr.value().to_owned();
            for changed in &current.sheets {
                let Some(source) = old.sheets.iter().find(|s| s.id == changed.id) else {
                    continue;
                };
                for edit in &changed.source_edits {
                    value = adjust_formula_refs(
                        &value,
                        &source.name,
                        sheet.id == changed.id,
                        edit.rows,
                        edit.at,
                        edit.count,
                        edit.delete,
                    );
                }
            }
            if value != attr.value() {
                updates.push((
                    attr.range_value().start,
                    attr.range_value().end,
                    escape_xml(&value),
                ));
            }
        }
    }
    Ok(apply_edits(text.to_owned(), updates))
}
fn patch_xlsx_view_refs(text: &str, edits: &[StructuralEdit]) -> Result<String, Error> {
    let d = xml(text)?;
    let mut updates = Vec::new();
    for node in d
        .descendants()
        .filter(|n| n.has_tag_name("selection") || n.has_tag_name("pane"))
    {
        for attr in node.attributes() {
            let name = attr.name();
            let mut value = attr.value().to_owned();
            if matches!(name, "activeCell" | "sqref" | "topLeftCell") {
                for edit in edits {
                    value = adjust_refs(&value, edit.rows, edit.at, edit.count, edit.delete);
                }
                if value.contains("#REF!") {
                    value = "A1".into();
                }
            } else if node.has_tag_name("pane") && matches!(name, "xSplit" | "ySplit") {
                let mut split = value
                    .parse::<usize>()
                    .map_err(|_| Error::Invalid("Unsupported frozen pane"))?;
                for edit in edits.iter().filter(|edit| edit.rows == (name == "ySplit")) {
                    if edit.delete {
                        split = split.saturating_sub(edit.count.min(split.saturating_sub(edit.at)));
                    } else if edit.at < split {
                        split = split.checked_add(edit.count).ok_or(Error::Limit)?;
                    }
                }
                if split == 0 && attr.value() != "0" {
                    return Err(Error::Invalid(
                        "Removing an entire frozen pane is not yet supported",
                    ));
                }
                value = split.to_string();
            } else {
                continue;
            }
            if value != attr.value() {
                updates.push((
                    attr.range_value().start,
                    attr.range_value().end,
                    escape_xml(&value),
                ));
            }
        }
    }
    Ok(apply_edits(text.to_owned(), updates))
}
pub(super) fn translate_shared_formula(input: &str, dr: isize, dc: isize) -> String {
    let bytes = input.as_bytes();
    let mut text = String::new();
    let mut qualifiers = Vec::new();
    let mut i = 0;
    let mut in_string = false;
    while i < bytes.len() {
        if bytes[i] == b'"' {
            text.push('"');
            i += 1;
            if in_string && bytes.get(i) == Some(&b'"') {
                text.push('"');
                i += 1;
            } else {
                in_string = !in_string;
            }
            continue;
        }
        if in_string {
            let ch = input[i..].chars().next().unwrap();
            text.push(ch);
            i += ch.len_utf8();
            continue;
        }
        let start = i;
        if bytes[i] == b'[' {
            let mut depth = 1usize;
            i += 1;
            while i < bytes.len() && depth > 0 {
                if bytes[i] == b'[' {
                    depth += 1;
                } else if bytes[i] == b']' {
                    depth -= 1;
                }
                i += 1;
            }
            let marker = format!("\u{1}{}\u{2}", qualifiers.len());
            qualifiers.push(input[start..i].to_owned());
            text.push_str(&marker);
            continue;
        }
        if bytes[i] == b'\'' {
            i += 1;
            while i < bytes.len() {
                let ch = input[i..].chars().next().unwrap();
                i += ch.len_utf8();
                if ch == '\'' {
                    if bytes.get(i) == Some(&b'\'') {
                        i += 1;
                    } else {
                        break;
                    }
                }
            }
        } else if bytes[i].is_ascii_alphabetic() || bytes[i] == b'_' {
            i += 1;
            while i < bytes.len()
                && (bytes[i].is_ascii_alphanumeric() || matches!(bytes[i], b'_' | b'.'))
            {
                i += 1;
            }
        } else {
            let ch = input[i..].chars().next().unwrap();
            text.push(ch);
            i += ch.len_utf8();
            continue;
        }
        if bytes.get(i) == Some(&b'!') && parse_a1_endpoint(input, i + 1).is_some() {
            i += 1;
            let marker = format!("\u{1}{}\u{2}", qualifiers.len());
            qualifiers.push(input[start..i].to_owned());
            text.push_str(&marker);
        } else {
            text.push_str(&input[start..i]);
        }
    }
    let mut result = translate_refs(&text, dr, dc);
    for (index, qualifier) in qualifiers.into_iter().enumerate() {
        result = result.replace(&format!("\u{1}{index}\u{2}"), &qualifier);
    }
    result
}
fn deshare_xlsx(
    text: &str,
    sheet: &Sheet,
    changes: &[((usize, usize), Option<Cell>)],
    structural: bool,
) -> Result<String, Error> {
    let d = xml(text)?;
    let mut groups: BTreeMap<String, Vec<((usize, usize), roxmltree::Node<'_, '_>)>> =
        BTreeMap::new();
    for cell in d.descendants().filter(|n| n.has_tag_name("c")) {
        let Some(coord) = cell.attribute("r").and_then(a1) else {
            continue;
        };
        for formula in cell
            .children()
            .filter(|n| n.has_tag_name("f") && n.attribute("t") == Some("shared"))
        {
            let id = formula
                .attribute("si")
                .map(str::to_owned)
                .unwrap_or_else(|| format!("missing:{}:{}", coord.0, coord.1));
            groups.entry(id).or_default().push((coord, formula));
        }
    }
    let mut edits = Vec::new();
    for members in groups.values() {
        if !structural
            && !members.iter().any(|(coord, _)| {
                changes.iter().any(|(changed, next)| {
                    changed == coord
                        && sheet.cells.get(coord).map(|c| &c.text) != next.as_ref().map(|c| &c.text)
                })
            })
        {
            continue;
        }
        if members.iter().any(|(_, formula)| {
            formula.attribute("si").is_none()
                || formula
                    .attributes()
                    .any(|a| !matches!(a.name(), "t" | "si" | "ref"))
        }) {
            return Err(Error::Invalid(
                "Extended or invalid shared formula is unsupported",
            ));
        }
        let anchors: Vec<_> = members
            .iter()
            .filter(|(_, f)| f.text().is_some_and(|text| !text.is_empty()))
            .collect();
        if anchors.len() != 1 {
            return Err(Error::Invalid("Invalid shared formula group"));
        }
        let (anchor_coord, anchor) = anchors[0];
        let source = format!("={}", anchor.text().unwrap_or(""));
        if source.contains('[') {
            return Err(Error::Invalid(
                "Shared formulas with structured or external references are not yet supported",
            ));
        }
        for (coord, formula) in members {
            let expected = translate_shared_formula(
                &source,
                coord.0 as isize - anchor_coord.0 as isize,
                coord.1 as isize - anchor_coord.1 as isize,
            );
            let imported = sheet
                .cells
                .get(coord)
                .and_then(|cell| match &cell.text {
                    CellContent::Formula(value) => Some(value.raw()),
                    _ => None,
                })
                .ok_or(Error::Invalid("Shared formula member cannot be resolved"))?;
            if imported != expected {
                return Err(Error::Invalid(
                    "Shared formula translation cannot be verified",
                ));
            }
            let p = prefix(&text[formula.range()]);
            let value = expected.strip_prefix('=').unwrap_or(&expected);
            edits.push((
                formula.range().start,
                formula.range().end,
                format!("<{p}f>{}</{p}f>", escape_xml(value)),
            ));
        }
    }
    Ok(apply_edits(text.to_owned(), edits))
}
fn patch_xlsx_sheet_contents(
    text: &str,
    sheet: &Sheet,
    current_sheet: &Sheet,
    old: &State,
    current: &State,
    changes: &[((usize, usize), Option<Cell>)],
    renames: &[SheetRename],
    style_doc: &mut Option<String>,
    style_cache: &mut BTreeMap<(usize, usize, usize), usize>,
) -> Result<String, Error> {
    let text = deshare_xlsx(text, sheet, changes, current_sheet.source_structure)?;
    let mut stylemap = BTreeMap::new();
    let sheet_doc = xml(&text)?;
    for (coord, after) in changes {
        let previous = sheet.cells.get(coord).map_or(0, |c| c.style);
        let next = after.as_ref().map_or(0, |c| c.style);
        if previous == next {
            continue;
        }
        if sheet_doc.descendants().any(|n| {
            n.has_tag_name("col") && n.attribute("style").is_some()
                || n.has_tag_name("row") && n.attribute("s").is_some()
        }) {
            return Err(Error::Invalid(
                "XLSX row or column styles cannot yet be safely edited",
            ));
        }
        let old_xf = sheet_doc
            .descendants()
            .find(|n| n.has_tag_name("c") && n.attribute("r").and_then(a1) == Some(*coord))
            .and_then(|n| n.attribute("s"))
            .and_then(|s| s.parse::<usize>().ok())
            .unwrap_or(0);
        let key = (old_xf, previous, next);
        let id = if let Some(id) = style_cache.get(&key) {
            *id
        } else {
            if style_cache.len() >= 1024 {
                return Err(Error::Limit);
            }
            let doc = style_doc
                .as_ref()
                .ok_or(Error::Invalid("XLSX has no style definitions"))?;
            let (new_doc, id) = crate::xlsx_styles::clone_style(
                doc,
                old_xf,
                &old.styles[previous],
                &current.styles[next],
            )?;
            *style_doc = Some(new_doc);
            style_cache.insert(key, id);
            id
        };
        stylemap.insert(*coord, id);
    }
    let patched = patch_sheet(&text, changes, sheet, &stylemap, renames)?;
    let patched = patch_xlsx_merges(&patched, &sheet.merges, &current_sheet.merges)?;
    let patched = if current_sheet.source_structure {
        patch_xlsx_view_refs(&patched, &current_sheet.source_edits)?
    } else {
        patched
    };
    let patched = if current_sheet.source_structure {
        let d = xml(&patched)?;
        let mut edits = Vec::new();
        if let Some(cols) = d.root_element().children().find(|n| n.has_tag_name("cols")) {
            edits.push((cols.range().start, cols.range().end, String::new()));
        }
        for row in d.descendants().filter(|n| n.has_tag_name("row")) {
            for attr in row
                .attributes()
                .filter(|a| matches!(a.name(), "ht" | "customHeight"))
            {
                edits.push((attr.range().start, attr.range().end, String::new()));
            }
        }
        let stripped = apply_edits(patched.clone(), edits);
        let mut empty_geometry = sheet.clone();
        empty_geometry.row_heights.clear();
        empty_geometry.col_widths.clear();
        patch_xlsx_geometry(&stripped, &empty_geometry, current_sheet)?
    } else {
        patch_xlsx_geometry(&patched, sheet, current_sheet)?
    };
    let patched = patch_xlsx_view_state(&patched, &sheet.view, &current_sheet.view)?;
    update_dimension(&patched, changes)
}
fn patch_xlsx(
    bytes: &[u8],
    old: &State,
    current: &State,
    changes: &[Vec<((usize, usize), Option<Cell>)>],
    renames: &[SheetRename],
) -> Result<Vec<u8>, Error> {
    if current
        .sheets
        .iter()
        .filter(|sheet| !old.sheets.iter().any(|source| source.id == sheet.id))
        .any(|sheet| {
            renames
                .iter()
                .any(|rename| rename.old.eq_ignore_ascii_case(&sheet.name))
        })
    {
        return Err(Error::Invalid(
            "Save and reopen before reusing a renamed source sheet name",
        ));
    }
    let mut archive =
        zip::ZipArchive::new(Cursor::new(bytes)).map_err(|_| Error::Invalid("invalid XLSX"))?;
    let workbook = zip_text(&mut archive, "xl/workbook.xml")?
        .ok_or(Error::Invalid("unsupported XLSX workbook location"))?;
    let relationships = zip_text(&mut archive, "xl/_rels/workbook.xml.rels")?
        .ok_or(Error::Invalid("missing workbook relationships"))?;
    let doc = xml(&workbook)?;
    let reordered = old
        .sheets
        .iter()
        .map(|s| s.id)
        .ne(current.sheets.iter().map(|s| s.id));
    if !renames.is_empty() || reordered {
        if doc.descendants().any(|n| {
            n.has_tag_name("workbookProtection")
                && !matches!(n.attribute("lockStructure"), Some("0" | "false"))
        }) {
            return Err(Error::Protected);
        }
        for name in archive.file_names() {
            if [
                "xl/pivotCache/",
                "xl/pivotTables/",
                "xl/externalLinks/",
                "xl/queryTables/",
                "xl/slicerCaches/",
                "xl/model/",
                "xl/vbaProject.bin",
                "xl/connections.xml",
            ]
            .iter()
            .any(|p| name.starts_with(p))
            {
                return Err(Error::Invalid(
                    "Workbook has linked or pivot structures that cannot yet be safely renamed",
                ));
            }
        }
    }
    let rel = xml(&relationships)?;
    let mut targets = BTreeMap::new();
    for n in rel.descendants().filter(|n| n.has_tag_name("Relationship")) {
        if let (Some(id), Some(target)) = (n.attribute("Id"), n.attribute("Target")) {
            let path = if target.starts_with('/') {
                target.trim_start_matches('/').to_owned()
            } else {
                format!("xl/{target}")
            };
            if !path.split('/').any(|part| part == "..") {
                targets.insert(id.to_string(), path);
            }
        }
    }
    let mut source_parts = BTreeMap::new();
    let mut sheet_relations = BTreeMap::new();
    for sheet in &old.sheets {
        let node = doc
            .descendants()
            .find(|n| n.has_tag_name("sheet") && n.attribute("name") == Some(sheet.name.as_str()))
            .ok_or(Error::Invalid("missing XLSX sheet"))?;
        let rid = node
            .attributes()
            .find(|a| a.name() == "id" && a.namespace().is_some())
            .map(|a| a.value())
            .ok_or(Error::Invalid("missing sheet relationship"))?;
        source_parts.insert(
            sheet.id,
            targets
                .get(rid)
                .ok_or(Error::Invalid("missing sheet part"))?
                .clone(),
        );
        sheet_relations.insert(sheet.id, rid.to_owned());
    }
    let removed: Vec<_> = old
        .sheets
        .iter()
        .filter(|s| !current.sheets.iter().any(|v| v.id == s.id))
        .collect();
    let deleted_paths: std::collections::BTreeSet<_> = removed
        .iter()
        .filter_map(|s| source_parts.get(&s.id).cloned())
        .collect();
    if !removed.is_empty() {
        if archive.by_name("xl/calcChain.xml").is_ok() {
            return Err(Error::Invalid(
                "Deleting sheets with an existing calculation chain is not yet supported",
            ));
        }
        let paths: Vec<String> = archive
            .file_names()
            .filter(|n| n.starts_with("xl/") && n.ends_with(".xml"))
            .map(str::to_owned)
            .collect();
        for path in paths {
            if deleted_paths.contains(&path) {
                continue;
            }
            let text = zip_text(&mut archive, &path)?.ok_or(Error::Invalid("missing XML part"))?;
            guard_deleted_sheet_refs(&text, &path, old, current, &deleted_paths)?;
        }
    }
    let mut replacements = BTreeMap::new();
    let mut style_doc = zip_text(&mut archive, "xl/styles.xml")?;
    let mut style_cache: BTreeMap<(usize, usize, usize), usize> = BTreeMap::new();
    for (i, sheet) in old.sheets.iter().enumerate() {
        let Some(current_sheet) = current.sheets.iter().find(|s| s.id == sheet.id) else {
            continue;
        };
        if changes[i].is_empty()
            && sheet.rows == current_sheet.rows
            && sheet.cols == current_sheet.cols
            && !current_sheet.source_structure
            && sheet.merges == current_sheet.merges
            && sheet.view == current_sheet.view
            && sheet.row_heights == current_sheet.row_heights
            && sheet.col_widths == current_sheet.col_widths
        {
            continue;
        }
        let node = doc
            .descendants()
            .find(|n| n.has_tag_name("sheet") && n.attribute("name") == Some(&sheet.name))
            .ok_or(Error::Invalid("missing XLSX sheet"))?;
        let id = node
            .attributes()
            .find(|a| a.name() == "id")
            .map(|a| a.value())
            .ok_or(Error::Invalid("missing XLSX sheet relationship"))?;
        let path = targets
            .get(id)
            .ok_or(Error::Invalid("unknown XLSX sheet relationship"))?;
        let text =
            zip_text(&mut archive, path)?.ok_or(Error::Invalid("missing XLSX sheet data"))?;
        if current_sheet.source_structure {
            guard_simple_xlsx_structure(&text, sheet, current_sheet, &doc, &mut archive)?;
        }
        let patched = patch_xlsx_sheet_contents(
            &text,
            sheet,
            current_sheet,
            old,
            current,
            &changes[i],
            renames,
            &mut style_doc,
            &mut style_cache,
        )?;
        replacements.insert(path.to_owned(), patched);
    }
    let table_headers_changed = old.sheets.iter().any(|source| {
        current
            .sheets
            .iter()
            .find(|s| s.id == source.id)
            .is_some_and(|sheet| {
                source.tables.iter().any(|table| {
                    sheet
                        .tables
                        .iter()
                        .find(|t| t.id == table.id)
                        .is_some_and(|next| {
                            (0..=table.range.right - table.range.left).any(|offset| {
                                source
                                    .cells
                                    .get(&(table.range.top, table.range.left + offset))
                                    .map(|c| c.text.raw())
                                    != sheet
                                        .cells
                                        .get(&(next.range.top, next.range.left + offset))
                                        .map(|c| c.text.raw())
                            })
                        })
                })
            })
    });
    if current.sheets.iter().any(|sheet| sheet.source_structure) || table_headers_changed {
        patch_xlsx_tables(&mut archive, &source_parts, old, current, &mut replacements)?;
        for source in &old.sheets {
            let Some(sheet) = current.sheets.iter().find(|sheet| sheet.id == source.id) else {
                continue;
            };
            let path = source_parts.get(&sheet.id).ok_or(Error::Bounds)?;
            let prior = replacements.remove(path);
            let text = match &prior {
                Some(value) => value.clone(),
                None => zip_text(&mut archive, path)?.ok_or(Error::Invalid("missing worksheet"))?,
            };
            let patched = patch_xlsx_hyperlinks(&text, sheet, old, current)?;
            let patched = patch_xlsx_rules(&patched, sheet, old, current)?;
            let patched = patch_xlsx_filter_refs(&patched, &sheet.source_edits)?;
            if prior.is_some() || patched != text {
                replacements.insert(path.clone(), patched);
            }
        }
    }
    let mut new_entries: BTreeMap<SheetId, (String, u32)> = BTreeMap::new();
    let mut additions: Vec<(SheetId, String, Vec<u8>)> = Vec::new();
    let mut used_paths: std::collections::BTreeSet<String> =
        archive.file_names().map(str::to_owned).collect();
    let mut used_rels: std::collections::BTreeSet<String> = rel
        .descendants()
        .filter(|n| n.has_tag_name("Relationship"))
        .filter_map(|n| n.attribute("Id").map(str::to_owned))
        .collect();
    let mut next_sheet_id = doc
        .descendants()
        .filter(|n| n.has_tag_name("sheet"))
        .filter_map(|n| n.attribute("sheetId").and_then(|v| v.parse::<u32>().ok()))
        .max()
        .unwrap_or(0);
    for sheet in current
        .sheets
        .iter()
        .filter(|s| !old.sheets.iter().any(|v| v.id == s.id))
    {
        let mut serial = sheet.id;
        let path = loop {
            let name = format!("xl/worksheets/off42_sheet_{serial}.xml");
            if used_paths.insert(name.clone()) {
                break name;
            }
            serial = serial.checked_add(1).ok_or(Error::Limit)?
        };
        let mut serial = sheet.id;
        let rid = loop {
            let id = format!("rIdOff42{serial}");
            if used_rels.insert(id.clone()) {
                break id;
            }
            serial = serial.checked_add(1).ok_or(Error::Limit)?
        };
        next_sheet_id = next_sheet_id.checked_add(1).ok_or(Error::Limit)?;
        let content = if let Some(source_id) = sheet.copied_from {
            let baseline = old
                .sheets
                .iter()
                .find(|v| v.id == source_id)
                .ok_or(Error::Invalid("Duplicate source is unavailable"))?;
            let src = source_parts.get(&source_id).ok_or(Error::Bounds)?;
            let (directory, filename) = src
                .rsplit_once('/')
                .ok_or(Error::Invalid("invalid worksheet path"))?;
            let relpath = format!("{directory}/_rels/{filename}.rels");
            if let Some(xml) = zip_text(&mut archive, &relpath)? {
                if self::xml(&xml)?
                    .descendants()
                    .any(|n| n.has_tag_name("Relationship"))
                {
                    return Err(Error::Invalid("Duplicating external sheets with linked drawings or objects is not yet supported"));
                }
            }
            let text =
                zip_text(&mut archive, src)?.ok_or(Error::Invalid("missing duplicate source"))?;
            let text = rewrite_xlsx_rename_xml(
                &text,
                src,
                &[SheetRename {
                    old: baseline.name.clone(),
                    new: sheet.name.clone(),
                    temporary: format!("\u{1}{}", sheet.id),
                }],
            )?;
            if sheet.source_structure {
                guard_simple_xlsx_structure(&text, baseline, sheet, &doc, &mut archive)?;
            }
            let mut keys = std::collections::BTreeSet::new();
            keys.extend(baseline.cells.keys().copied());
            keys.extend(sheet.cells.keys().copied());
            let changes: Vec<_> = keys
                .into_iter()
                .filter_map(|key| {
                    let before = baseline.cells.get(&key);
                    let after = sheet.cells.get(&key);
                    (before != after).then(|| (key, after.cloned()))
                })
                .collect();
            patch_xlsx_sheet_contents(
                &text,
                baseline,
                sheet,
                old,
                current,
                &changes,
                renames,
                &mut style_doc,
                &mut style_cache,
            )?
        } else {
            xlsx_new_sheet_xml(sheet, old, current, &mut style_doc, &mut style_cache)?
        };
        new_entries.insert(sheet.id, (rid, next_sheet_id));
        additions.push((sheet.id, path, content.into_bytes()));
    }
    if !style_cache.is_empty() {
        replacements.insert(
            "xl/styles.xml".into(),
            style_doc.ok_or(Error::Invalid("missing XLSX styles"))?,
        );
    }
    if archive.by_name("xl/calcChain.xml").is_ok() {
        for (i, group) in changes.iter().enumerate() {
            for (coord, new) in group {
                let old_formula = old.sheets[i]
                    .cells
                    .get(coord)
                    .is_some_and(|c| matches!(c.text, CellContent::Formula(_)));
                let new_formula = new
                    .as_ref()
                    .is_some_and(|c| matches!(c.text, CellContent::Formula(_)));
                let value_changed = match (old.sheets[i].cells.get(coord), new.as_ref()) {
                    (Some(a), Some(b)) => !equivalent_content(&a.text, &b.text, renames),
                    (None, None) => false,
                    _ => true,
                };
                if value_changed && (old_formula || new_formula) {
                    return Err(Error::Invalid(
                        "Editing formulas with an existing calculation chain is not yet supported",
                    ));
                }
            }
        }
    }
    let workbook_order = if reordered {
        rebuild_xlsx_workbook(&workbook, old, current, &new_entries)?
    } else {
        workbook.clone()
    };
    if reordered {
        let reltext = rebuild_xlsx_relationships(
            &relationships,
            &sheet_relations,
            &removed,
            &new_entries,
            &additions,
        )?;
        replacements.insert("xl/_rels/workbook.xml.rels".into(), reltext);
        if !additions.is_empty() {
            let types = zip_text(&mut archive, "[Content_Types].xml")?
                .ok_or(Error::Invalid("missing content types"))?;
            replacements.insert(
                "[Content_Types].xml".into(),
                append_sheet_content_types(&types, &additions)?,
            );
        }
    }
    let workbook_order = patch_xlsx_defined_names(&workbook_order, old, current)?;
    replacements.insert("xl/workbook.xml".into(), patch_calc(&workbook_order)?);
    if !renames.is_empty() {
        let paths: Vec<String> = archive
            .file_names()
            .filter(|n| n.starts_with("xl/") && n.ends_with(".xml"))
            .map(str::to_owned)
            .collect();
        for path in paths {
            let prior = replacements.remove(&path);
            let base = match &prior {
                Some(v) => v.clone(),
                None => zip_text(&mut archive, &path)?.ok_or(Error::Invalid("missing XML part"))?,
            };
            let modified = rewrite_xlsx_rename_xml(&base, &path, renames)?;
            if prior.is_some() || modified != base {
                replacements.insert(path, modified);
            }
        }
    }
    if !renames.is_empty() {
        for (_, _, bytes) in &mut additions {
            let text =
                std::str::from_utf8(bytes).map_err(|_| Error::Invalid("invalid sheet XML"))?;
            *bytes = rewrite_xlsx_rename_xml(text, "xl/worksheets/new.xml", renames)?.into_bytes();
        }
    }
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for i in 0..archive.len() {
        let entry = archive
            .by_index(i)
            .map_err(|_| Error::Invalid("invalid XLSX entry"))?;
        let name = entry.name().to_owned();
        if name == EXTRA {
            continue;
        }
        if let Some(value) = replacements.remove(&name) {
            let opts =
                zip::write::SimpleFileOptions::default().compression_method(entry.compression());
            writer
                .start_file(name, opts)
                .map_err(|_| Error::Invalid("cannot write XLSX part"))?;
            writer.write_all(value.as_bytes())?;
        } else {
            writer
                .raw_copy_file(entry)
                .map_err(|_| Error::Invalid("cannot preserve XLSX part"))?;
        }
    }
    if !replacements.is_empty() {
        return Err(Error::Invalid("missing XLSX part"));
    }
    for (_, path, bytes) in additions {
        writer
            .start_file(
                path,
                zip::write::SimpleFileOptions::default()
                    .compression_method(zip::CompressionMethod::Deflated),
            )
            .map_err(|_| Error::Invalid("cannot add worksheet part"))?;
        writer.write_all(&bytes)?;
    }
    let out = writer
        .finish()
        .map_err(|_| Error::Invalid("cannot finish XLSX"))?
        .into_inner();
    if out.len() > LIMIT {
        return Err(Error::Limit);
    }
    Ok(out)
}

fn canonical_xml(node: roxmltree::Node<'_, '_>, out: &mut String) {
    if node.is_element() {
        out.push('<');
        out.push_str(node.tag_name().namespace().unwrap_or(""));
        out.push(':');
        out.push_str(node.tag_name().name());
        let mut attrs: Vec<_> = node
            .attributes()
            .map(|a| (a.namespace().unwrap_or(""), a.name(), a.value()))
            .collect();
        attrs.sort_unstable();
        for (ns, name, value) in attrs {
            out.push('|');
            out.push_str(ns);
            out.push(':');
            out.push_str(name);
            out.push('=');
            out.push_str(value)
        }
        out.push('>');
        for c in node.children() {
            canonical_xml(c, out)
        }
        out.push_str("</>")
    } else if node.is_text() {
        let t = node.text().unwrap_or("");
        if !t.trim().is_empty() {
            out.push_str(t)
        }
    }
}
fn verify_ods_opaque_content(
    before: &str,
    after: &str,
    allow_visibility: bool,
) -> Result<(), Error> {
    fn parts(text: &str) -> Result<Vec<String>, Error> {
        let d = xml(text)?;
        let mut all = Vec::new();
        for n in d.descendants().filter(|n| n.is_element()) {
            let name = n.tag_name().name();
            let ns = n.tag_name().namespace().unwrap_or("");
            let root = matches!(
                name,
                "annotation"
                    | "database-ranges"
                    | "named-expressions"
                    | "data-validations"
                    | "conditional-formats"
                    | "table-source"
            ) || ns.contains(":drawing:")
                || ns.contains(":chart:")
                || ns.contains(":form:");
            if root
                && !n.ancestors().skip(1).any(|a| {
                    a.is_element()
                        && (matches!(
                            a.tag_name().name(),
                            "annotation"
                                | "database-ranges"
                                | "named-expressions"
                                | "data-validations"
                                | "conditional-formats"
                                | "table-source"
                        ) || a.tag_name().namespace().is_some_and(|s| {
                            s.contains(":drawing:") || s.contains(":chart:") || s.contains(":form:")
                        }))
                })
            {
                let mut value = String::new();
                canonical_xml(n, &mut value);
                all.push(value)
            }
        }
        all.sort();
        Ok(all)
    }
    if parts(before)? != parts(after)? {
        return Err(Error::Invalid(
            "ODS contains unsupported content that could change during editing",
        ));
    }
    fn other_attributes(text: &str, allow_visibility: bool) -> Result<Vec<String>, Error> {
        let d = xml(text)?;
        let mut attrs = Vec::new();
        for n in d.descendants().filter(|n| n.is_element()) {
            if n.ancestors()
                .any(|a| a.is_element() && a.has_tag_name("automatic-styles"))
            {
                continue;
            }
            for a in n.attributes() {
                if allow_visibility
                    && a.name() == "visibility"
                    && matches!(n.tag_name().name(), "table-row" | "table-column")
                {
                    continue;
                }
                if a.name() == "style-name"
                    && (n.has_tag_name("table-cell") || n.has_tag_name("covered-table-cell"))
                {
                    continue;
                }
                if matches!(
                    a.name(),
                    "number-rows-spanned"
                        | "number-columns-spanned"
                        | "value-type"
                        | "value"
                        | "string-value"
                        | "boolean-value"
                        | "date-value"
                        | "time-value"
                        | "formula"
                        | "number-rows-repeated"
                        | "number-columns-repeated"
                ) {
                    continue;
                }
                attrs.push(format!(
                    "{}|{}|{}",
                    a.namespace().unwrap_or(""),
                    a.name(),
                    a.value()
                ));
            }
        }
        attrs.sort();
        Ok(attrs)
    }
    if other_attributes(before, allow_visibility)? != other_attributes(after, allow_visibility)? {
        return Err(Error::Invalid(
            "ODS contains cell or document metadata that cannot yet be preserved",
        ));
    }
    Ok(())
}

fn preserve_ods_auto_styles(before: &str, after: &str) -> Result<String, Error> {
    let a = xml(before)?;
    let b = xml(after)?;
    let left = a.descendants().find(|n| n.has_tag_name("automatic-styles"));
    let right = b.descendants().find(|n| n.has_tag_name("automatic-styles"));
    match (left, right) {
        (Some(l), Some(r)) => {
            let name = |n: roxmltree::Node<'_, '_>| {
                n.attributes()
                    .find(|a| a.name() == "name")
                    .map(|a| a.value().to_owned())
            };
            let names: std::collections::BTreeSet<_> = l
                .children()
                .filter(|n| n.is_element())
                .filter_map(name)
                .collect();
            let mut appended = String::new();
            for n in r.children().filter(|n| n.is_element()) {
                let Some(name) = name(n) else { continue };
                if !names.contains(&name) {
                    if !name.starts_with("XLS42ExternalStyle") {
                        return Err(Error::Invalid(
                            "ODS rewrite introduced unsupported automatic styles",
                        ));
                    }
                    appended.push_str(&after[n.range()]);
                }
            }
            let original = &before[l.range()];
            let mut merged = original.to_owned();
            if !appended.is_empty() {
                if original.trim_end().ends_with("/>") {
                    let i = original.rfind("/>").unwrap();
                    let p = prefix(original);
                    merged = format!("{}>{}</{p}automatic-styles>", &original[..i], appended);
                } else {
                    let i = original
                        .rfind("</")
                        .ok_or(Error::Invalid("invalid automatic styles"))?;
                    merged.insert_str(i, &appended)
                }
            }
            let mut result = after.to_owned();
            result.replace_range(r.range(), &merged);
            xml(&result)?;
            Ok(result)
        }
        (None, None) => Ok(after.to_owned()),
        _ => Err(Error::Invalid("ODS automatic styles cannot be preserved")),
    }
}

fn update_ods_style(out: &mut spreadsheet_ods::CellStyle, old: &CellStyle, new: &CellStyle) {
    use spreadsheet_ods::{
        color::Rgb,
        style::units::{Border, LineStyle},
        Length,
    };
    let rgb = |c: u32| Rgb::new((c >> 16) as u8, (c >> 8) as u8, c as u8);
    if old.bold != new.bold {
        out.textstyle_mut()
            .set_attr("fo:font-weight", if new.bold { "bold" } else { "normal" })
    }
    if old.italic != new.italic {
        out.textstyle_mut().set_attr(
            "fo:font-style",
            if new.italic { "italic" } else { "normal" },
        )
    }
    if old.underline != new.underline {
        out.set_text_underline_style(if new.underline {
            LineStyle::Solid
        } else {
            LineStyle::None
        })
    }
    if old.font != new.font {
        if new.font.is_empty() {
            out.textstyle_mut().clear_attr("style:font-name");
        } else {
            out.set_font_name(&new.font)
        }
    }
    if old.font_size != new.font_size {
        if new.font_size == 0 {
            out.textstyle_mut().clear_attr("fo:font-size");
        } else {
            out.set_font_size(Length::Pt(new.font_size as f64).into())
        }
    }
    if old.foreground != new.foreground {
        if let Some(c) = new.foreground {
            out.set_color(rgb(c))
        } else {
            out.textstyle_mut().clear_attr("fo:color");
        }
    }
    if old.background != new.background {
        if let Some(c) = new.background {
            out.set_background_color(rgb(c))
        } else {
            out.cellstyle_mut().clear_attr("fo:background-color");
        }
    }
    if old.borders != new.borders {
        for key in [
            "fo:border",
            "fo:border-top",
            "fo:border-right",
            "fo:border-bottom",
            "fo:border-left",
        ] {
            out.cellstyle_mut().clear_attr(key);
        }
        let black = Rgb::new(0, 0, 0);
        if new.borders & 1 != 0 {
            out.set_border_top(Length::Pt(1.0), Border::Solid, black)
        }
        if new.borders & 2 != 0 {
            out.set_border_right(Length::Pt(1.0), Border::Solid, black)
        }
        if new.borders & 4 != 0 {
            out.set_border_bottom(Length::Pt(1.0), Border::Solid, black)
        }
        if new.borders & 8 != 0 {
            out.set_border_left(Length::Pt(1.0), Border::Solid, black)
        }
    }
}

fn verify_ods_untouched_styles(
    original: &[u8],
    generated: &[u8],
    changes: &[Vec<((usize, usize), Option<Cell>)>],
    old: &State,
    current: &State,
) -> Result<(), Error> {
    let a = spreadsheet_ods::read_ods_buf(original)
        .map_err(|_| Error::Invalid("invalid ODS source"))?;
    let b = spreadsheet_ods::read_ods_buf(generated)
        .map_err(|_| Error::Invalid("invalid generated ODS"))?;
    for (i, sheet) in old.sheets.iter().enumerate() {
        let changed: std::collections::BTreeSet<_> = changes[i]
            .iter()
            .filter_map(|(coord, after)| {
                let previous = sheet.cells.get(coord).map_or(0, |c| c.style);
                let next = after.as_ref().map_or(0, |c| c.style);
                (previous != next).then_some(*coord)
            })
            .collect();
        let (ar, ac) = a.sheet(i).used_grid_size();
        let (br, bc) = b.sheet(i).used_grid_size();
        let rows = sheet.rows.max(ar as usize).max(br as usize);
        let cols = sheet.cols.max(ac as usize).max(bc as usize);
        if rows > MAX_ROWS || cols > MAX_COLS {
            return Err(Error::Invalid(
                "ODS style grid exceeds safe preservation limits",
            ));
        }
        for r in 0..rows {
            for c in 0..cols {
                let merge_changed = sheet
                    .merges
                    .iter()
                    .chain(current.sheets[i].merges.iter())
                    .any(|m| {
                        m.top == r
                            && m.left == c
                            && sheet.merges.contains(m) != current.sheets[i].merges.contains(m)
                    });
                if !merge_changed
                    && (a.sheet(i).row_span(r as u32, c as u32)
                        != b.sheet(i).row_span(r as u32, c as u32)
                        || a.sheet(i).col_span(r as u32, c as u32)
                            != b.sheet(i).col_span(r as u32, c as u32))
                {
                    return Err(Error::Invalid("ODS rewriting changed an unedited merge"));
                }
                if changed.contains(&(r, c))
                    || current.sheets[i].source_structure
                        && changes[i].iter().any(|(coord, _)| *coord == (r, c))
                {
                    continue;
                }
                let before = a.sheet(i).cellstyle(r as u32, c as u32).map(|v| v.as_ref());
                let after = b.sheet(i).cellstyle(r as u32, c as u32).map(|v| v.as_ref());
                if before != after {
                    return Err(Error::Invalid(
                        "ODS rewriting changed an unedited cell style",
                    ));
                }
            }
        }
    }
    Ok(())
}

fn verify_ods_repeated_styles(content: &str) -> Result<(), Error> {
    let d = xml(content)?;
    for n in d.descendants().filter(|n| n.is_element()) {
        let repeated = n
            .attributes()
            .find(|a| matches!(a.name(), "number-columns-repeated" | "number-rows-repeated"))
            .and_then(|a| a.value().parse::<usize>().ok())
            .unwrap_or(1);
        if repeated > 1
            && (n
                .attribute((
                    "urn:oasis:names:tc:opendocument:xmlns:table:1.0",
                    "style-name",
                ))
                .is_some()
                || n.descendants().any(|d| {
                    d.is_element()
                        && d.attribute((
                            "urn:oasis:names:tc:opendocument:xmlns:table:1.0",
                            "style-name",
                        ))
                        .is_some()
                }))
        {
            return Err(Error::Invalid(
                "ODS contains repeated styled cells that cannot yet be safely rewritten",
            ));
        }
    }
    Ok(())
}
fn patch_xlsx_merges(text: &str, old: &[Range], new: &[Range]) -> Result<String, Error> {
    if old == new {
        return Ok(text.to_owned());
    }
    let doc = xml(text)?;
    let root = doc.root_element();
    let element = root.children().find(|n| n.has_tag_name("mergeCells"));
    let prefix = prefix(&text[root.range()]);
    let format_ref = |r: &Range| {
        format!(
            "{}{}:{}{}",
            col_name(r.left),
            r.top + 1,
            col_name(r.right),
            r.bottom + 1
        )
    };
    let mut items = Vec::new();
    if let Some(el) = element {
        if el
            .children()
            .any(|n| n.is_element() && !n.has_tag_name("mergeCell"))
            || el.attributes().any(|a| a.name() != "count")
        {
            return Err(Error::Invalid(
                "extended XLSX merge metadata cannot yet be edited",
            ));
        }
        for node in el.children().filter(|n| n.has_tag_name("mergeCell")) {
            if node.attributes().any(|a| a.name() != "ref") {
                return Err(Error::Invalid(
                    "extended XLSX merge metadata cannot yet be edited",
                ));
            }
            let Some(reference) = node.attribute("ref") else {
                return Err(Error::Invalid("invalid XLSX merge"));
            };
            let parsed = reference
                .split_once(':')
                .and_then(|(a, b)| Some((a1(a)?, a1(b)?)))
                .map(|((top, left), (bottom, right))| Range {
                    top,
                    left,
                    bottom,
                    right,
                });
            if parsed.is_none_or(|r| !old.contains(&r) || new.contains(&r)) {
                items.push(text[node.range()].to_owned())
            }
        }
    }
    for m in new {
        if !old.contains(m) {
            items.push(format!("<{prefix}mergeCell ref=\"{}\"/>", format_ref(m)))
        }
    }
    let mut out = text.to_owned();
    let replacement = if items.is_empty() {
        String::new()
    } else {
        format!(
            "<{prefix}mergeCells count=\"{}\">{}</{prefix}mergeCells>",
            items.len(),
            items.join("")
        )
    };
    if let Some(el) = element {
        out.replace_range(el.range(), &replacement)
    } else if !replacement.is_empty() {
        let insert = root
            .children()
            .filter(|n| {
                n.is_element()
                    && matches!(
                        n.tag_name().name(),
                        "sheetData"
                            | "sheetCalcPr"
                            | "sheetProtection"
                            | "protectedRanges"
                            | "scenarios"
                            | "autoFilter"
                            | "sortState"
                            | "dataConsolidate"
                            | "customSheetViews"
                    )
            })
            .map(|n| n.range().end)
            .max()
            .ok_or(Error::Invalid("missing XLSX sheet data"))?;
        out.insert_str(insert, &replacement)
    }
    Ok(out)
}
fn patch_xlsx_view_state(text: &str, old: &SheetView, new: &SheetView) -> Result<String, Error> {
    if old == new {
        return Ok(text.to_owned());
    }
    let mut output = text.to_owned();
    if old.hidden_rows != new.hidden_rows {
        let d = xml(&output)?;
        if d.descendants().any(|n| n.has_tag_name("autoFilter")) {
            return Err(Error::Invalid(
                "Hidden rows in a filtered source sheet cannot yet be changed safely",
            ));
        }
        for &row in old.hidden_rows.symmetric_difference(&new.hidden_rows) {
            let d = xml(&output)?;
            let root = d.root_element();
            let data = root
                .children()
                .find(|n| n.has_tag_name("sheetData"))
                .ok_or(Error::Invalid("Missing sheet data"))?;
            let p = prefix(&output[data.range()]);
            let node = data.children().find(|n| {
                n.has_tag_name("row")
                    && n.attribute("r").and_then(|v| v.parse::<usize>().ok()) == Some(row + 1)
            });
            let hidden = if new.hidden_rows.contains(&row) {
                "1"
            } else {
                "0"
            };
            if let Some(node) = node {
                let range = node.range();
                let modified = crate::xlsx_styles::attrs(
                    &output[range.clone()],
                    &[("hidden", hidden.into())],
                )?;
                output.replace_range(range, &modified);
            } else {
                let part = format!("<{p}row r=\"{}\" hidden=\"{}\"/>", row + 1, hidden);
                let raw = &output[data.range()];
                if raw.trim_end().ends_with("/>") {
                    let range = data.range();
                    let at = raw.rfind("/>").unwrap();
                    let value = format!("{}>{part}</{p}sheetData>", &raw[..at]);
                    output.replace_range(range, &value);
                } else {
                    let index = data
                        .children()
                        .filter(|n| n.has_tag_name("row"))
                        .find(|n| {
                            n.attribute("r")
                                .and_then(|v| v.parse::<usize>().ok())
                                .is_some_and(|v| v > row + 1)
                        })
                        .map(|n| n.range().start)
                        .unwrap_or_else(|| data.range().start + raw.rfind("</").unwrap());
                    output.insert_str(index, &part);
                }
            }
        }
    }
    if old.frozen_rows != new.frozen_rows || old.frozen_columns != new.frozen_columns {
        let d = xml(&output)?;
        let root = d.root_element();
        let views = root
            .children()
            .find(|n| n.has_tag_name("sheetViews"))
            .ok_or(Error::Invalid("Missing sheet view"))?;
        let all: Vec<_> = views
            .children()
            .filter(|n| n.has_tag_name("sheetView"))
            .collect();
        if all.len() != 1 {
            return Err(Error::Invalid(
                "Multiple sheet views cannot yet be edited safely",
            ));
        }
        let view = all[0];
        let p = prefix(&output[view.range()]);
        let panes: Vec<_> = view.children().filter(|n| n.has_tag_name("pane")).collect();
        if panes.len() > 1
            || panes.first().is_some_and(|pane| {
                !matches!(pane.attribute("state"), Some("frozen" | "frozenSplit"))
            })
        {
            return Err(Error::Invalid("Split panes cannot yet be changed safely"));
        }
        let selections: Vec<_> = view
            .children()
            .filter(|n| n.has_tag_name("selection"))
            .collect();
        if selections.iter().any(|selection| {
            selection
                .attributes()
                .any(|a| !matches!(a.name(), "pane" | "activeCell" | "activeCellId" | "sqref"))
        }) {
            return Err(Error::Invalid(
                "Extended pane selections cannot yet be changed safely",
            ));
        }
        let old_active = panes.first().and_then(|pane| pane.attribute("activePane"));
        let selected = selections
            .iter()
            .copied()
            .find(|selection| selection.attribute("pane") == old_active)
            .or_else(|| selections.first().copied());
        let rows = new.frozen_rows;
        let cols = new.frozen_columns;
        let active = if rows > 0 && cols > 0 {
            "bottomRight"
        } else if rows > 0 {
            "bottomLeft"
        } else if cols > 0 {
            "topRight"
        } else {
            "topLeft"
        };
        let pane = if rows == 0 && cols == 0 {
            String::new()
        } else {
            format!("<{p}pane xSplit=\"{cols}\" ySplit=\"{rows}\" topLeftCell=\"{}{}\" activePane=\"{active}\" state=\"frozen\"/>",col_name(cols),rows+1)
        };
        let mut edits = Vec::new();
        if let Some(node) = panes.first() {
            edits.push((node.range().start, node.range().end, pane));
        } else if !pane.is_empty() {
            let raw = &output[view.range()];
            if raw.trim_end().ends_with("/>") {
                let at = raw.rfind("/>").unwrap();
                edits.push((
                    view.range().start,
                    view.range().end,
                    format!("{}>{pane}</{p}sheetView>", &raw[..at]),
                ));
            } else {
                let at = view.range().start
                    + raw.find('>').ok_or(Error::Invalid("Invalid sheet view"))?
                    + 1;
                edits.push((at, at, pane));
            }
        }
        for node in selections {
            let range = node.range();
            let modified = if Some(node) == selected {
                crate::xlsx_styles::attrs(&output[range.clone()], &[("pane", active.into())])?
            } else {
                String::new()
            };
            edits.push((range.start, range.end, modified));
        }
        output = apply_edits(output, edits);
    }
    Ok(output)
}
fn patch_xlsx_geometry(text: &str, old: &Sheet, new: &Sheet) -> Result<String, Error> {
    let mut output = text.to_owned();
    for (&row, &height) in &new.row_heights {
        if old.row_heights.get(&row) == Some(&height) {
            continue;
        }
        let d = xml(&output)?;
        let root = d.root_element();
        let data = root
            .children()
            .find(|n| n.has_tag_name("sheetData"))
            .ok_or(Error::Invalid("missing sheet data"))?;
        let p = prefix(&output[data.range()]);
        let existing = data.children().find(|n| {
            n.has_tag_name("row")
                && n.attribute("r").and_then(|v| v.parse::<usize>().ok()) == Some(row + 1)
        });
        let ht = (height as f64 * 0.75).to_string();
        if let Some(node) = existing {
            let range = node.range();
            let modified = crate::xlsx_styles::attrs(
                &output[range.clone()],
                &[("ht", ht), ("customHeight", "1".into())],
            )?;
            output.replace_range(range, &modified)
        } else {
            let part = format!(
                "<{p}row r=\"{}\" ht=\"{}\" customHeight=\"1\"/>",
                row + 1,
                ht
            );
            let raw = &output[data.range()];
            if raw.trim_end().ends_with("/>") {
                let range = data.range();
                let i = raw.rfind("/>").unwrap();
                let merged = format!("{}>{part}</{p}sheetData>", &raw[..i]);
                output.replace_range(range, &merged)
            } else {
                let index = data
                    .children()
                    .filter(|n| n.has_tag_name("row"))
                    .find(|n| {
                        n.attribute("r")
                            .and_then(|v| v.parse::<usize>().ok())
                            .is_some_and(|r| r > row + 1)
                    })
                    .map(|n| n.range().start)
                    .unwrap_or_else(|| data.range().start + raw.rfind("</").unwrap());
                output.insert_str(index, &part)
            }
        }
    }
    let mut changed: BTreeMap<usize, (Option<u16>, Option<bool>)> = BTreeMap::new();
    for (&col, &width) in &new.col_widths {
        if old.col_widths.get(&col) != Some(&width) {
            changed.entry(col).or_default().0 = Some(width);
        }
    }
    for &col in old
        .view
        .hidden_columns
        .symmetric_difference(&new.view.hidden_columns)
    {
        changed.entry(col).or_default().1 = Some(new.view.hidden_columns.contains(&col));
    }
    let changed: Vec<_> = changed.into_iter().collect();
    if changed.is_empty() {
        return Ok(output);
    }
    let d = xml(&output)?;
    let root = d.root_element();
    let existing = root.children().find(|n| n.has_tag_name("cols"));
    let p = prefix(&output[root.range()]);
    let mut segments: Vec<(usize, usize, String)> = Vec::new();
    let mut prior_end = 0usize;
    if let Some(cols) = existing {
        if cols
            .children()
            .any(|n| n.is_element() && !n.has_tag_name("col"))
        {
            return Err(Error::Invalid(
                "extended column metadata cannot be resized safely",
            ));
        }
        for col in cols.children().filter(|n| n.has_tag_name("col")) {
            let first = col
                .attribute("min")
                .and_then(|v| v.parse::<usize>().ok())
                .ok_or(Error::Invalid("invalid XLSX column"))?;
            let last = col
                .attribute("max")
                .and_then(|v| v.parse::<usize>().ok())
                .ok_or(Error::Invalid("invalid XLSX column"))?;
            if first == 0 || last < first || first <= prior_end || last > 16384 {
                return Err(Error::Invalid("overlapping XLSX column definitions"));
            }
            prior_end = last;
            let raw = &output[col.range()];
            let mut cursor = first;
            for &(idx, (width, hidden)) in &changed {
                let n = idx + 1;
                if n < first || n > last {
                    continue;
                }
                if cursor < n {
                    let cloned = crate::xlsx_styles::attrs(
                        raw,
                        &[("min", cursor.to_string()), ("max", (n - 1).to_string())],
                    )?;
                    segments.push((cursor, n - 1, cloned));
                }
                let mut updates = vec![("min", n.to_string()), ("max", n.to_string())];
                if let Some(width) = width {
                    let value = ((width as u32 * 256) / 7) as f64 / 256.0;
                    updates.push(("width", value.to_string()));
                    updates.push(("customWidth", "1".into()));
                }
                if let Some(hidden) = hidden {
                    updates.push(("hidden", if hidden { "1" } else { "0" }.into()));
                }
                let cloned = crate::xlsx_styles::attrs(raw, &updates)?;
                segments.push((n, n, cloned));
                cursor = n + 1;
            }
            if cursor <= last {
                let cloned = crate::xlsx_styles::attrs(
                    raw,
                    &[("min", cursor.to_string()), ("max", last.to_string())],
                )?;
                segments.push((cursor, last, cloned));
            }
        }
    }
    for &(idx, (width, hidden)) in &changed {
        let n = idx + 1;
        if !segments.iter().any(|(a, b, _)| n >= *a && n <= *b) {
            let mut attrs = format!("min=\"{n}\" max=\"{n}\"");
            if let Some(width) = width {
                let value = ((width as u32 * 256) / 7) as f64 / 256.0;
                attrs.push_str(&format!(" width=\"{value}\" customWidth=\"1\""));
            }
            if let Some(hidden) = hidden {
                attrs.push_str(if hidden {
                    " hidden=\"1\""
                } else {
                    " hidden=\"0\""
                });
            }
            segments.push((n, n, format!("<{p}col {attrs}/>")));
        }
    }
    segments.sort_by_key(|v| v.0);
    let content = segments.into_iter().map(|v| v.2).collect::<String>();
    if let Some(cols) = existing {
        let range = cols.range();
        let raw = &output[range.clone()];
        let node = if raw.trim_end().ends_with("/>") {
            let at = raw.rfind("/>").unwrap();
            format!("{}>{content}</{p}cols>", &raw[..at])
        } else {
            let first = raw
                .find('>')
                .ok_or(Error::Invalid("invalid XLSX columns"))?;
            let last = raw
                .rfind("</")
                .ok_or(Error::Invalid("invalid XLSX columns"))?;
            format!("{}{}{}", &raw[..first + 1], content, &raw[last..])
        };
        output.replace_range(range, &node)
    } else {
        let index = root
            .children()
            .find(|n| n.has_tag_name("sheetData"))
            .ok_or(Error::Invalid("missing sheet data"))?
            .range()
            .start;
        output.insert_str(index, &format!("<{p}cols>{content}</{p}cols>"))
    }
    Ok(output)
}
#[cfg(test)]
mod external_protection_regression {
    use super::*;
    #[test]
    fn protected_external_sheet_blocks_editing() {
        let mut x = rust_xlsxwriter::Workbook::new();
        let sheet = x.add_worksheet();
        sheet.write_string(0, 0, "locked").unwrap();
        sheet.protect();
        let bytes = x.save_to_buffer().unwrap();
        let mut book = Workbook::from_office_bytes(&bytes, "xlsx").unwrap();
        assert!(book.is_protected());
        assert!(matches!(
            book.set_cell(0, 0, "changed".into()),
            Err(Error::Protected)
        ));
        assert!(matches!(book.set_row_height(0, 32), Err(Error::Protected)));
        assert_eq!(book.cell(0, 0), Some("locked"));
    }
}
#[derive(Clone)]
struct SheetRename {
    old: String,
    new: String,
    temporary: String,
}
fn sheet_renames(old: &State, new: &State) -> Result<Vec<SheetRename>, Error> {
    let mut out = Vec::new();
    for a in &old.sheets {
        let Some(b) = new.sheets.iter().find(|s| s.id == a.id) else {
            continue;
        };
        if a.name != b.name {
            out.push(SheetRename {
                old: a.name.clone(),
                new: b.name.clone(),
                temporary: format!("\u{1}{}", a.id),
            })
        }
    }
    Ok(out)
}

fn rewrite_renames(s: &str, renames: &[SheetRename]) -> String {
    let mut value = s.to_owned();
    for r in renames {
        value = rewrite_sheet_refs(&value, &r.old, &r.temporary)
    }
    for r in renames {
        value = rewrite_sheet_refs(&value, &r.temporary, &r.new)
    }
    value
}
fn equivalent_content(a: &CellContent, b: &CellContent, renames: &[SheetRename]) -> bool {
    if a == b {
        return true;
    }
    matches!((a,b),(CellContent::Formula(x),CellContent::Formula(y)) if rewrite_renames(x.raw(),renames)==y.raw())
}
fn rewrite_xlsx_rename_xml(
    text: &str,
    path: &str,
    renames: &[SheetRename],
) -> Result<String, Error> {
    let d = xml(text)?;
    let mut edits = Vec::new();
    for node in d.descendants().filter(|n| n.is_element()) {
        let name = node.tag_name().name();
        if path == "xl/workbook.xml" && name == "sheet" {
            if let Some(attr) = node.attributes().find(|a| a.name() == "name") {
                if let Some(change) = renames.iter().find(|r| r.old == attr.value()) {
                    edits.push((
                        attr.range_value().start,
                        attr.range_value().end,
                        escape_xml(&change.new),
                    ))
                }
            }
        }
        for attr in node.attributes() {
            let value = rewrite_renames(attr.value(), renames);
            if value != attr.value() {
                if matches!(
                    attr.name(),
                    "formula" | "formula1" | "formula2" | "location" | "refersTo" | "sqref"
                ) {
                    let r = attr.range_value();
                    edits.push((r.start, r.end, escape_xml(&value)))
                } else {
                    return Err(Error::Invalid(
                        "Workbook has an unsupported sheet reference in XML metadata",
                    ));
                }
            }
        }
        for child in node.children().filter(|n| n.is_text()) {
            let value = child.text().unwrap_or("");
            let changed = rewrite_renames(value, renames);
            if changed == value {
                continue;
            }
            if matches!(
                name,
                "f" | "formula"
                    | "formula1"
                    | "formula2"
                    | "definedName"
                    | "calculatedColumnFormula"
                    | "totalsRowFormula"
                    | "formulaExpression"
            ) {
                let range = child.range();
                let raw = &text[range.clone()];
                if raw.starts_with('<') {
                    return Err(Error::Invalid(
                        "Sheet reference uses unsupported XML text encoding",
                    ));
                }
                edits.push((range.start, range.end, escape_xml(&changed)));
            } else if !matches!(name, "t" | "v" | "text") {
                return Err(Error::Invalid(
                    "Workbook has an unsupported sheet reference in an XML part",
                ));
            }
        }
    }
    Ok(apply_edits(text.to_owned(), edits))
}
#[cfg(test)]
mod external_rename_regression {
    use super::*;
    #[test]
    fn external_sheet_rename_updates_formulas_names_and_chart_references() {
        let mut source = rust_xlsxwriter::Workbook::new();
        source
            .add_worksheet()
            .set_name("Main")
            .unwrap()
            .write_formula(0, 0, "='Data Sheet'!$A$1+1")
            .unwrap();
        source
            .add_worksheet()
            .set_name("Data Sheet")
            .unwrap()
            .write_number(0, 0, 5)
            .unwrap();
        source
            .define_name("SourceValue", "='Data Sheet'!$A$1")
            .unwrap();
        let mut chart = rust_xlsxwriter::Chart::new(rust_xlsxwriter::ChartType::Column);
        chart.add_series().set_values(("Data Sheet", 0, 0, 0, 0));
        source
            .worksheet_from_name("Main")
            .unwrap()
            .insert_chart(3, 0, &chart)
            .unwrap();
        let bytes = source.save_to_buffer().unwrap();
        let mut book = Workbook::from_office_bytes(&bytes, "xlsx").unwrap();
        let sheet = book
            .sheets()
            .into_iter()
            .find(|(_, name)| name == "Data Sheet")
            .unwrap()
            .0;
        book.rename_sheet(sheet, "O'Brien").unwrap();
        assert_eq!(book.cell(0, 0), Some("='O''Brien'!$A$1+1"));
        let saved = book.xlsx_bytes().unwrap();
        let mut z = zip::ZipArchive::new(Cursor::new(saved.as_slice())).unwrap();
        for path in [
            "xl/workbook.xml",
            "xl/charts/chart1.xml",
            "xl/worksheets/sheet1.xml",
        ] {
            let mut xml = String::new();
            z.by_name(path).unwrap().read_to_string(&mut xml).unwrap();
            assert!(
                xml.contains("O''Brien")
                    || xml.contains("O&apos;&apos;Brien")
                    || xml.contains("O&amp;apos;"),
                "{path}: {xml}"
            );
            assert!(!xml.contains("Data Sheet!"));
        }
        let reopened = Workbook::from_office_bytes(&saved, "xlsx").unwrap();
        assert!(reopened.sheets().iter().any(|(_, n)| n == "O'Brien"));
        assert_eq!(reopened.cell(0, 0), Some("='O''Brien'!$A$1+1"));
    }
}
fn rebuild_xlsx_workbook(
    text: &str,
    old: &State,
    current: &State,
    new_entries: &BTreeMap<SheetId, (String, u32)>,
) -> Result<String, Error> {
    let doc = xml(text)?;
    let root = doc.root_element();
    let sheets = root
        .children()
        .find(|n| n.has_tag_name("sheets"))
        .ok_or(Error::Invalid("missing workbook sheets"))?;
    if sheets.children().any(|n| {
        n.is_comment()
            || n.is_element() && !n.has_tag_name("sheet")
            || n.is_text() && !n.text().unwrap_or("").trim().is_empty()
    }) {
        return Err(Error::Invalid(
            "extended workbook sheet metadata cannot yet be edited",
        ));
    }
    let nodes: Vec<_> = sheets
        .children()
        .filter(|n| n.has_tag_name("sheet"))
        .collect();
    if nodes.len() != old.sheets.len() {
        return Err(Error::Invalid("unsupported workbook sheet definitions"));
    }
    let mut source = BTreeMap::new();
    for sheet in &old.sheets {
        let node = nodes
            .iter()
            .copied()
            .find(|n| n.attribute("name") == Some(sheet.name.as_str()))
            .ok_or(Error::Invalid("missing workbook sheet"))?;
        source.insert(sheet.id, &text[node.range()]);
    }
    let firstnode = *nodes.first().ok_or(Error::Bounds)?;
    let rattr = firstnode
        .attributes()
        .find(|a| a.name() == "id" && a.namespace().is_some())
        .ok_or(Error::Invalid("missing sheet relationship namespace"))?;
    let rq = &text[rattr.range_qname()];
    let p = prefix(&text[firstnode.range()]);
    let mut parts = String::new();
    for sheet in &current.sheets {
        if let Some(raw) = source.get(&sheet.id) {
            parts.push_str(raw)
        } else {
            let (rel, id) = new_entries.get(&sheet.id).ok_or(Error::Bounds)?;
            parts.push_str(&format!(
                "<{p}sheet name=\"{}\" sheetId=\"{id}\" {rq}=\"{}\"/>",
                escape_xml(&sheet.name),
                escape_xml(rel)
            ));
        }
    }
    let first = nodes.first().unwrap().range().start;
    let last = nodes.last().unwrap().range().end;
    let mut edits = vec![(first, last, parts)];
    let mut duplicate_names = String::new();
    for n in doc.descendants().filter(|n| n.is_element()) {
        if n.has_tag_name("definedName") {
            if let Some(attr) = n.attributes().find(|a| a.name() == "localSheetId") {
                let index = attr
                    .value()
                    .parse::<usize>()
                    .map_err(|_| Error::Invalid("invalid sheet index"))?;
                let old_sheet = old
                    .sheets
                    .get(index)
                    .ok_or(Error::Invalid("invalid sheet index"))?;
                for copy in current
                    .sheets
                    .iter()
                    .filter(|s| s.copied_from == Some(old_sheet.id))
                {
                    let new_index = current
                        .sheets
                        .iter()
                        .position(|s| s.id == copy.id)
                        .ok_or(Error::Bounds)?;
                    let raw = crate::xlsx_styles::attrs(
                        &text[n.range()],
                        &[("localSheetId", new_index.to_string())],
                    )?;
                    let renames = [SheetRename {
                        old: old_sheet.name.clone(),
                        new: copy.name.clone(),
                        temporary: format!("\u{1}{}", copy.id),
                    }];
                    duplicate_names.push_str(&rewrite_xlsx_rename_xml(
                        &raw,
                        "xl/workbook.xml",
                        &renames,
                    )?);
                }
                if let Some(next) = current.sheets.iter().position(|s| s.id == old_sheet.id) {
                    let range = attr.range_value();
                    edits.push((range.start, range.end, next.to_string()))
                } else {
                    let range = n.range();
                    edits.push((range.start, range.end, String::new()))
                }
            }
        } else if n.has_tag_name("workbookView") {
            for attr in n
                .attributes()
                .filter(|a| matches!(a.name(), "activeTab" | "firstSheet"))
            {
                let index = attr
                    .value()
                    .parse::<usize>()
                    .map_err(|_| Error::Invalid("invalid sheet index"))?;
                let id = old
                    .sheets
                    .get(index)
                    .ok_or(Error::Invalid("invalid sheet index"))?
                    .id;
                let next = current
                    .sheets
                    .iter()
                    .position(|s| s.id == id)
                    .unwrap_or_else(|| {
                        current
                            .sheets
                            .iter()
                            .position(|s| s.id == current.active)
                            .unwrap_or(0)
                    });
                let range = attr.range_value();
                edits.push((range.start, range.end, next.to_string()));
            }
        }
    }
    if !duplicate_names.is_empty() {
        let defs = root
            .children()
            .find(|n| n.has_tag_name("definedNames"))
            .ok_or(Error::Invalid("missing defined-name container"))?;
        let at = defs.range().start
            + text[defs.range()]
                .rfind("</")
                .ok_or(Error::Invalid("invalid defined names"))?;
        edits.push((at, at, duplicate_names));
    }
    Ok(apply_edits(text.to_owned(), edits))
}

#[cfg(test)]
mod external_reorder_regression {
    use super::*;
    #[test]
    fn external_reorder_preserves_sheet_identity_local_names_and_undo() {
        let mut source = rust_xlsxwriter::Workbook::new();
        source
            .add_worksheet()
            .set_name("First")
            .unwrap()
            .write_string(0, 0, "alpha")
            .unwrap();
        source
            .add_worksheet()
            .set_name("Second")
            .unwrap()
            .write_string(0, 0, "beta")
            .unwrap();
        source
            .define_name("Second!LocalValue", "=Second!$A$1")
            .unwrap();
        let bytes = source.save_to_buffer().unwrap();
        let mut book = Workbook::from_office_bytes(&bytes, "xlsx").unwrap();
        let second = book.sheets()[1].0;
        book.reorder_sheet(second, 0).unwrap();
        assert_eq!(book.sheets()[0].1, "Second");
        let saved = book.xlsx_bytes().unwrap();
        let mut archive = zip::ZipArchive::new(Cursor::new(saved.as_slice())).unwrap();
        let mut xml = String::new();
        archive
            .by_name("xl/workbook.xml")
            .unwrap()
            .read_to_string(&mut xml)
            .unwrap();
        let doc = super::xml(&xml).unwrap();
        let names: Vec<_> = doc
            .descendants()
            .filter(|n| n.has_tag_name("sheet"))
            .filter_map(|n| n.attribute("name"))
            .collect();
        assert_eq!(names, ["Second", "First"]);
        let local = doc
            .descendants()
            .find(|n| n.has_tag_name("definedName") && n.attribute("name") == Some("LocalValue"))
            .unwrap();
        assert_eq!(local.attribute("localSheetId"), Some("0"));
        let reopened = Workbook::from_office_bytes(&saved, "xlsx").unwrap();
        assert_eq!(reopened.sheets()[0].1, "Second");
        assert_eq!(reopened.cell(0, 0), Some("beta"));
        book.undo();
        assert_eq!(book.sheets()[0].1, "First");
        book.redo();
        assert_eq!(book.sheets()[0].1, "Second");
    }
}
fn append_xml_children(
    text: &str,
    node: roxmltree::Node<'_, '_>,
    children: &str,
) -> Result<String, Error> {
    if children.is_empty() {
        return Ok(text.to_owned());
    }
    let range = node.range();
    let raw = &text[range.clone()];
    let p = prefix(raw);
    let local = node.tag_name().name();
    let changed = if raw.trim_end().ends_with("/>") {
        let i = raw.rfind("/>").unwrap();
        format!("{}>{children}</{p}{local}>", &raw[..i])
    } else {
        let i = raw
            .rfind("</")
            .ok_or(Error::Invalid("invalid XML container"))?;
        let mut v = raw.to_owned();
        v.insert_str(i, children);
        v
    };
    let mut output = text.to_owned();
    output.replace_range(range, &changed);
    Ok(output)
}
fn xlsx_new_sheet_xml(
    sheet: &Sheet,
    old: &State,
    current: &State,
    styles: &mut Option<String>,
    style_cache: &mut BTreeMap<(usize, usize, usize), usize>,
) -> Result<String, Error> {
    if sheet.protected
        || !sheet.validations.is_empty()
        || !sheet.conditional_formats.is_empty()
        || !sheet.charts.is_empty()
        || !sheet.pivots.is_empty()
        || sheet.filter.is_some()
        || sheet.print != PrintSettings::default()
    {
        return Err(Error::Invalid(
            "New external sheet contains unsupported metadata",
        ));
    }
    let mut styleids = BTreeMap::new();
    for (&key, cell) in &sheet.cells {
        if cell.style == 0 {
            continue;
        }
        let cache_key = (0, 0, cell.style);
        let id = if let Some(id) = style_cache.get(&cache_key) {
            *id
        } else {
            if style_cache.len() >= 1024 {
                return Err(Error::Limit);
            }
            let doc = styles
                .as_ref()
                .ok_or(Error::Invalid("XLSX has no style definitions"))?;
            let (next, id) = crate::xlsx_styles::clone_style(
                doc,
                0,
                &old.styles[0],
                current.styles.get(cell.style).ok_or(Error::Bounds)?,
            )?;
            *styles = Some(next);
            style_cache.insert(cache_key, id);
            id
        };
        styleids.insert(key, id);
    }
    let mut out=String::from("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><worksheet xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\"><sheetFormatPr defaultRowHeight=\"15\"/>");
    if !sheet.col_widths.is_empty() {
        out.push_str("<cols>");
        for (&col, &width) in &sheet.col_widths {
            let n = col + 1;
            let width = ((width as u32 * 256) / 7) as f64 / 256.0;
            out.push_str(&format!(
                "<col min=\"{n}\" max=\"{n}\" width=\"{width}\" customWidth=\"1\"/>"
            ));
        }
        out.push_str("</cols>")
    }
    out.push_str("<sheetData>");
    let mut rows = std::collections::BTreeSet::new();
    rows.extend(sheet.cells.keys().map(|(r, _)| *r));
    rows.extend(sheet.row_heights.keys().copied());
    for row in rows {
        out.push_str(&format!("<row r=\"{}\"", row + 1));
        if let Some(h) = sheet.row_heights.get(&row) {
            out.push_str(&format!(" ht=\"{}\" customHeight=\"1\"", *h as f64 * 0.75))
        }
        out.push('>');
        for (&(r, c), cell) in sheet.cells.range((row, 0)..=(row, usize::MAX)) {
            let style = styleids.get(&(r, c)).map(usize::to_string);
            out.push_str(&cell_xml("", r, c, Some(cell), style.as_deref())?);
        }
        out.push_str("</row>")
    }
    out.push_str("</sheetData>");
    if !sheet.merges.is_empty() {
        out.push_str(&format!("<mergeCells count=\"{}\">", sheet.merges.len()));
        for m in &sheet.merges {
            out.push_str(&format!(
                "<mergeCell ref=\"{}{}:{}{}\"/>",
                col_name(m.left),
                m.top + 1,
                col_name(m.right),
                m.bottom + 1
            ));
        }
        out.push_str("</mergeCells>")
    }
    out.push_str("</worksheet>");
    Ok(out)
}
fn guard_deleted_sheet_refs(
    text: &str,
    path: &str,
    old: &State,
    current: &State,
    deleted_paths: &std::collections::BTreeSet<String>,
) -> Result<(), Error> {
    if deleted_paths.contains(path) {
        return Ok(());
    }
    let removed: Vec<_> = old
        .sheets
        .iter()
        .filter(|s| !current.sheets.iter().any(|n| n.id == s.id))
        .collect();
    if removed.is_empty() {
        return Ok(());
    }
    let doc = xml(text)?;
    for node in doc.descendants().filter(|n| n.is_element()) {
        let scoped_removed = node
            .ancestors()
            .find(|n| n.has_tag_name("definedName"))
            .and_then(|n| n.attribute("localSheetId"))
            .and_then(|s| s.parse::<usize>().ok())
            .and_then(|i| old.sheets.get(i))
            .is_some_and(|s| removed.iter().any(|r| r.id == s.id));
        if scoped_removed {
            continue;
        }
        let name = node.tag_name().name();
        for attr in node.attributes() {
            if removed.iter().any(|s| {
                rewrite_sheet_refs(attr.value(), &s.name, "XLS42RemovedGuard") != attr.value()
            }) {
                return Err(Error::Invalid(
                    "A workbook object references the sheet being deleted",
                ));
            }
        }
        for child in node.children().filter(|n| n.is_text()) {
            let value = child.text().unwrap_or("");
            if !matches!(name, "t" | "v" | "text")
                && removed
                    .iter()
                    .any(|s| rewrite_sheet_refs(value, &s.name, "XLS42RemovedGuard") != value)
            {
                return Err(Error::Invalid(
                    "A workbook formula or object references the sheet being deleted",
                ));
            }
        }
    }
    Ok(())
}
fn rebuild_xlsx_relationships(
    text: &str,
    sheet_relations: &BTreeMap<SheetId, String>,
    removed: &[&Sheet],
    new_entries: &BTreeMap<SheetId, (String, u32)>,
    additions: &[(SheetId, String, Vec<u8>)],
) -> Result<String, Error> {
    let doc = xml(text)?;
    let root = doc.root_element();
    let removed_ids: std::collections::BTreeSet<_> = removed
        .iter()
        .filter_map(|s| sheet_relations.get(&s.id))
        .collect();
    let mut edits = Vec::new();
    for node in root.children().filter(|n| n.has_tag_name("Relationship")) {
        if node
            .attribute("Id")
            .is_some_and(|id| removed_ids.iter().any(|r| r.as_str() == id))
        {
            let r = node.range();
            edits.push((r.start, r.end, String::new()));
        }
    }
    let mut out = apply_edits(text.to_owned(), edits);
    let d = xml(&out)?;
    let root = d.root_element();
    let p = prefix(&out[root.range()]);
    let mut added = String::new();
    for (sheet_id, path, _) in additions {
        let (id, _) = new_entries
            .get(sheet_id)
            .ok_or_else(|| Error::Invalid("Missing worksheet relationship"))?;
        let target = path
            .strip_prefix("xl/")
            .ok_or(Error::Invalid("invalid worksheet path"))?;
        added.push_str(&format!("<{p}Relationship Id=\"{}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet\" Target=\"{}\"/>",escape_xml(id),escape_xml(target)));
    }
    out = append_xml_children(&out, root, &added)?;
    Ok(out)
}
fn append_sheet_content_types(
    text: &str,
    additions: &[(SheetId, String, Vec<u8>)],
) -> Result<String, Error> {
    let d = xml(text)?;
    let root = d.root_element();
    let p = prefix(&text[root.range()]);
    let mut added = String::new();
    for (_, path, _) in additions {
        added.push_str(&format!("<{p}Override PartName=\"/{}\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml\"/>",escape_xml(path)));
    }
    append_xml_children(text, root, &added)
}

#[cfg(test)]
mod external_sheet_structure_regression {
    use super::*;
    #[test]
    fn ods_sheet_rename_rewrites_cross_sheet_formula() {
        let mut source = spreadsheet_ods::WorkBook::default();
        let mut first = spreadsheet_ods::Sheet::new("First");
        first.set_value(0, 0, 5);
        source.push_sheet(first);
        let mut second = spreadsheet_ods::Sheet::new("Second");
        second.set_formula(0, 0, "of:=[First.A1]*2");
        source.push_sheet(second);
        let bytes = spreadsheet_ods::write_ods_buf(&mut source, Vec::new()).unwrap();
        let mut book = Workbook::from_office_bytes(&bytes, "ods").unwrap();
        let first = book.sheets()[0].0;
        book.rename_sheet(first, "New Name").unwrap();
        let saved = book.ods_bytes().unwrap();
        let mut archive = zip::ZipArchive::new(Cursor::new(saved.as_slice())).unwrap();
        let content = zip_text(&mut archive, "content.xml").unwrap().unwrap();
        assert!(content.contains("New Name"));
        let mut reopened = Workbook::from_office_bytes(&saved, "ods").unwrap();
        assert_eq!(reopened.sheets()[0].1, "New Name");
        reopened.select_sheet(reopened.sheets()[1].0).unwrap();
        assert_eq!(reopened.cell(0, 0), Some("='New Name'!A1*2"));
    }
    #[test]
    fn ods_named_ranges_follow_sheet_rename() {
        let mut book = Workbook::new(20, 8).unwrap();
        let old = book.history.current().clone();
        let id = book.active_sheet();
        book.rename_sheet(id, "Renamed").unwrap();
        let current = book.history.current();
        let renames = sheet_renames(&old, current).unwrap();
        let input = "<office:spreadsheet xmlns:office=\"urn:oasis:names:tc:opendocument:xmlns:office:1.0\" xmlns:table=\"urn:oasis:names:tc:opendocument:xmlns:table:1.0\"><table:table table:name=\"Renamed\"/><table:named-expressions><table:named-range table:name=\"R\" table:cell-range-address=\"Sheet1.$A$1:Sheet1.$B$2\"/></table:named-expressions></office:spreadsheet>";
        let output = rewrite_ods_structure_refs(input, &old, current, &renames).unwrap();
        let d = xml(&output).unwrap();
        assert_eq!(
            d.descendants()
                .find(|n| n.has_tag_name("named-range"))
                .unwrap()
                .attributes()
                .find(|a| a.name() == "cell-range-address")
                .unwrap()
                .value(),
            "'Renamed'.$A$1:'Renamed'.$B$2"
        );
    }
    #[test]
    fn ods_structure_moves_merged_ranges() {
        let mut source = spreadsheet_ods::WorkBook::default();
        let mut sheet = spreadsheet_ods::Sheet::new("First");
        sheet.set_value(1, 1, "merged");
        sheet.set_row_span(1, 1, 2);
        sheet.set_col_span(1, 1, 2);
        source.push_sheet(sheet);
        let bytes = spreadsheet_ods::write_ods_buf(&mut source, Vec::new()).unwrap();
        let mut book = Workbook::from_office_bytes(&bytes, "ods").unwrap();
        book.insert_rows(0, 1).unwrap();
        let saved = book.ods_bytes().unwrap();
        let reopened = Workbook::from_office_bytes(&saved, "ods").unwrap();
        assert_eq!(reopened.cell(2, 1), Some("merged"));
        assert!(reopened.merges().contains(&Range {
            top: 2,
            left: 1,
            bottom: 3,
            right: 2
        }));
        let source = spreadsheet_ods::read_ods_buf(&saved).unwrap();
        assert_eq!(source.sheet(0).row_span(2, 1), 2);
        assert_eq!(source.sheet(0).col_span(2, 1), 2);
    }
    #[test]
    fn ods_structure_moves_explicit_cell_style_without_losing_properties() {
        let mut source = spreadsheet_ods::WorkBook::default();
        let mut style = spreadsheet_ods::CellStyle::new_empty();
        style.set_name("custom-bold");
        style.set_font_bold();
        let style = source.add_cellstyle(style);
        let mut sheet = spreadsheet_ods::Sheet::new("First");
        sheet.set_value(1, 1, "styled");
        sheet.set_cellstyle(1, 1, &style);
        source.push_sheet(sheet);
        let bytes = spreadsheet_ods::write_ods_buf(&mut source, Vec::new()).unwrap();
        let mut book = Workbook::from_office_bytes(&bytes, "ods").unwrap();
        assert!(book.style(1, 1).unwrap().bold);
        book.insert_rows(0, 1).unwrap();
        book.insert_columns(0, 1).unwrap();
        let saved = book.ods_bytes().unwrap();
        let reopened = Workbook::from_office_bytes(&saved, "ods").unwrap();
        assert_eq!(reopened.cell(2, 2), Some("styled"));
        assert!(reopened.style(2, 2).unwrap().bold);
        let external = spreadsheet_ods::read_ods_buf(&saved).unwrap();
        assert_eq!(
            external.sheet(0).cellstyle(2, 2).map(|s| s.as_ref()),
            Some("custom-bold")
        );
    }
    #[test]
    fn ods_plain_structure_updates_values_and_cross_sheet_references() {
        let mut source = spreadsheet_ods::WorkBook::default();
        let mut first = spreadsheet_ods::Sheet::new("First");
        first.set_value(1, 0, 7);
        source.push_sheet(first);
        let mut second = spreadsheet_ods::Sheet::new("Other");
        second.set_formula(0, 0, "of:=[First.A2]*2");
        source.push_sheet(second);
        let bytes = spreadsheet_ods::write_ods_buf(&mut source, Vec::new()).unwrap();
        let mut book = Workbook::from_office_bytes(&bytes, "ods").unwrap();
        book.insert_rows(0, 1).unwrap();
        book.insert_columns(0, 1).unwrap();
        let saved = book.ods_bytes().unwrap();
        let mut reopened = Workbook::from_office_bytes(&saved, "ods").unwrap();
        assert_eq!(reopened.cell(2, 1), Some("7"));
        reopened.select_sheet(reopened.sheets()[1].0).unwrap();
        assert_eq!(reopened.cell(0, 0), Some("='First'!B3*2"));
    }
    #[test]
    fn ods_duplicate_retargets_own_qualified_formula() {
        let mut source = spreadsheet_ods::WorkBook::default();
        let mut first = spreadsheet_ods::Sheet::new("First");
        first.set_formula(0, 0, "of:=[First.B1]+[Other.B1]");
        source.push_sheet(first);
        source.push_sheet(spreadsheet_ods::Sheet::new("Other"));
        let bytes = spreadsheet_ods::write_ods_buf(&mut source, Vec::new()).unwrap();
        let mut book = Workbook::from_office_bytes(&bytes, "ods").unwrap();
        let first = book.sheets()[0].0;
        book.duplicate_sheet(first, "Copy").unwrap();
        let saved = book.ods_bytes().unwrap();
        let mut reopened = Workbook::from_office_bytes(&saved, "ods").unwrap();
        let copy = reopened.sheets().iter().find(|s| s.1 == "Copy").unwrap().0;
        reopened.select_sheet(copy).unwrap();
        assert_eq!(reopened.cell(0, 0), Some("='Copy'!B1+Other!B1"));
    }
    #[test]
    fn ods_sheet_add_duplicate_reorder_and_edit_before_save() {
        let mut source = spreadsheet_ods::WorkBook::default();
        let mut first = spreadsheet_ods::Sheet::new("First");
        first.set_value(0, 0, "alpha");
        source.push_sheet(first);
        let mut second = spreadsheet_ods::Sheet::new("Second");
        second.set_value(0, 0, "beta");
        source.push_sheet(second);
        let bytes = spreadsheet_ods::write_ods_buf(&mut source, Vec::new()).unwrap();
        let mut book = Workbook::from_office_bytes(&bytes, "ods").unwrap();
        let first = book.sheets()[0].0;
        let second = book.sheets()[1].0;
        let copy = book.duplicate_sheet(first, "Copy").unwrap();
        book.set_cell(0, 0, "edited".into()).unwrap();
        let added = book.add_sheet("New").unwrap();
        book.set_cell(0, 0, "new".into()).unwrap();
        book.reorder_sheet(added, 0).unwrap();
        book.delete_sheet(second).unwrap();
        let saved = book.ods_bytes().unwrap();
        let mut reopened = Workbook::from_office_bytes(&saved, "ods").unwrap();
        assert_eq!(
            reopened
                .sheets()
                .iter()
                .map(|s| s.1.as_str())
                .collect::<Vec<_>>(),
            ["New", "First", "Copy"]
        );
        for (name, value) in [("New", "new"), ("First", "alpha"), ("Copy", "edited")] {
            let id = reopened.sheets().iter().find(|s| s.1 == name).unwrap().0;
            reopened.select_sheet(id).unwrap();
            assert_eq!(reopened.cell(0, 0), Some(value));
        }
        assert_ne!(copy, added);
    }
    #[test]
    fn shared_formula_translation_preserves_qualified_and_mixed_references() {
        assert_eq!(
            translate_shared_formula("='Data Sheet'!$A1+B$2+\"A1\"", 1, 1),
            "='Data Sheet'!$A2+C$2+\"A1\""
        );
    }
    #[test]
    fn independently_authored_shared_formula_is_materialized_and_array_guarded() {
        let mut source = rust_xlsxwriter::Workbook::new();
        let sheet = source.add_worksheet();
        sheet.write_number(0, 0, 1).unwrap();
        sheet.write_number(1, 0, 2).unwrap();
        sheet.write_formula(0, 1, "=A1*2").unwrap();
        sheet.write_formula(1, 1, "=A2*2").unwrap();
        sheet.write_formula(0, 2, "=A1*3").unwrap();
        sheet.write_number(1, 2, 6).unwrap();
        let bytes = source.save_to_buffer().unwrap();
        let mut input = zip::ZipArchive::new(Cursor::new(bytes.as_slice())).unwrap();
        let mut output = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for index in 0..input.len() {
            let mut file = input.by_index(index).unwrap();
            if file.name() == "xl/worksheets/sheet1.xml" {
                let mut text = String::new();
                file.read_to_string(&mut text).unwrap();
                let doc = xml(&text).unwrap();
                let mut edits = Vec::new();
                for (address, replacement) in [
                    ("B1", "<f t=\"shared\" si=\"0\" ref=\"B1:B2\">A1*2</f>"),
                    ("B2", "<f t=\"shared\" si=\"0\"/>"),
                    ("C1", "<f t=\"array\" ref=\"C1:C2\">A1:A2*3</f>"),
                ] {
                    let cell = doc
                        .descendants()
                        .find(|n| n.has_tag_name("c") && n.attribute("r") == Some(address))
                        .unwrap();
                    let formula = cell.children().find(|n| n.has_tag_name("f")).unwrap();
                    edits.push((
                        formula.range().start,
                        formula.range().end,
                        replacement.to_owned(),
                    ));
                }
                let text = apply_edits(text, edits);
                output
                    .start_file(
                        "xl/worksheets/sheet1.xml",
                        zip::write::SimpleFileOptions::default(),
                    )
                    .unwrap();
                output.write_all(text.as_bytes()).unwrap();
            } else {
                output.raw_copy_file(file).unwrap();
            }
        }
        let bytes = output.finish().unwrap().into_inner();
        let mut plain_input = zip::ZipArchive::new(Cursor::new(bytes.as_slice())).unwrap();
        let mut plain_output = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for index in 0..plain_input.len() {
            let mut entry = plain_input.by_index(index).unwrap();
            if entry.name() == "xl/worksheets/sheet1.xml" {
                let mut xml = String::new();
                entry.read_to_string(&mut xml).unwrap();
                let xml = xml.replace("<f t=\"array\" ref=\"C1:C2\">A1:A2*3</f>", "<f>A1*3</f>");
                plain_output
                    .start_file(
                        "xl/worksheets/sheet1.xml",
                        zip::write::SimpleFileOptions::default(),
                    )
                    .unwrap();
                plain_output.write_all(xml.as_bytes()).unwrap();
            } else {
                plain_output.raw_copy_file(entry).unwrap();
            }
        }
        let plain_bytes = plain_output.finish().unwrap().into_inner();
        let mut structural = Workbook::from_office_bytes(&plain_bytes, "xlsx").unwrap();
        structural.insert_rows(0, 1).unwrap();
        let saved_structural = structural.xlsx_bytes().unwrap();
        let reopened = Workbook::from_office_bytes(&saved_structural, "xlsx").unwrap();
        assert_eq!(reopened.cell(1, 1), Some("=A2*2"));
        assert_eq!(reopened.cell(2, 1), Some("=A3*2"));
        let mut archive = zip::ZipArchive::new(Cursor::new(saved_structural.as_slice())).unwrap();
        assert!(!zip_text(&mut archive, "xl/worksheets/sheet1.xml")
            .unwrap()
            .unwrap()
            .contains("t=\"shared\""));
        let mut book = Workbook::from_office_bytes(&bytes, "xlsx").unwrap();
        assert_eq!(book.cell(1, 1), Some("=A2*2"));
        book.set_cell(0, 1, "9".into()).unwrap();
        assert_eq!(book.cell(1, 1), Some("=A2*2"));
        assert!(book.set_cell(1, 2, "9".into()).is_err());
        assert!(book.insert_rows(0, 1).is_err());
        book.set_cell(0, 3, "safe".into()).unwrap();
        let saved = book.xlsx_bytes().unwrap();
        let mut archive = zip::ZipArchive::new(Cursor::new(saved.as_slice())).unwrap();
        let xml = zip_text(&mut archive, "xl/worksheets/sheet1.xml")
            .unwrap()
            .unwrap();
        assert!(!xml.contains("t=\"shared\""));
        assert!(xml.contains("<f>A2*2</f>"));
        assert!(xml.contains("t=\"array\" ref=\"C1:C2\""));
        assert_eq!(
            Workbook::from_office_bytes(&saved, "xlsx")
                .unwrap()
                .cell(0, 3),
            Some("safe")
        );
    }
    #[test]
    fn external_xlsx_table_header_edit_updates_metadata_and_formulas() {
        use rust_xlsxwriter::{Table, TableColumn};
        let mut source = rust_xlsxwriter::Workbook::new();
        let sheet = source.add_worksheet();
        sheet.write_number(1, 0, 10).unwrap();
        sheet.write_formula(4, 0, "=SUM(Sales[Amount])").unwrap();
        sheet
            .add_table(
                0,
                0,
                2,
                0,
                &Table::new()
                    .set_name("Sales")
                    .set_columns(&[TableColumn::new().set_header("Amount")]),
            )
            .unwrap();
        let bytes = source.save_to_buffer().unwrap();
        let mut book = Workbook::from_office_bytes(&bytes, "xlsx").unwrap();
        assert_eq!(book.tables().len(), 1);
        assert_eq!(book.display_grid()[4][0], "10");
        book.set_cell(0, 0, "Revenue".into()).unwrap();
        assert_eq!(book.cell(4, 0), Some("=SUM(Sales[Revenue])"));
        let bytes = book.xlsx_bytes().unwrap();
        let mut archive = zip::ZipArchive::new(Cursor::new(bytes.as_slice())).unwrap();
        let table = zip_text(&mut archive, "xl/tables/table1.xml")
            .unwrap()
            .unwrap();
        let d = xml(&table).unwrap();
        assert_eq!(
            d.descendants()
                .find(|n| n.has_tag_name("tableColumn"))
                .unwrap()
                .attribute("name"),
            Some("Revenue")
        );
        let reopened = Workbook::from_office_bytes(&bytes, "xlsx").unwrap();
        assert_eq!(reopened.display_grid()[4][0], "10");
    }
    #[test]
    fn structured_table_rows_resize_table_and_preserve_relationships() {
        let mut source = rust_xlsxwriter::Workbook::new();
        let sheet = source.add_worksheet();
        sheet.write_number(1, 0, 4).unwrap();
        sheet.write_number(2, 0, 5).unwrap();
        sheet
            .add_table(0, 0, 2, 1, &rust_xlsxwriter::Table::new())
            .unwrap();
        let bytes = source.save_to_buffer().unwrap();
        let mut book = Workbook::from_office_bytes(&bytes, "xlsx").unwrap();
        book.insert_rows(2, 1).unwrap();
        let saved = book.xlsx_bytes().unwrap();
        let mut archive = zip::ZipArchive::new(Cursor::new(saved.as_slice())).unwrap();
        let table = zip_text(&mut archive, "xl/tables/table1.xml")
            .unwrap()
            .unwrap();
        let d = xml(&table).unwrap();
        assert_eq!(d.root_element().attribute("ref"), Some("A1:B4"));
        assert_eq!(
            d.descendants()
                .find(|n| n.has_tag_name("autoFilter"))
                .unwrap()
                .attribute("ref"),
            Some("A1:B4")
        );
        assert!(book.delete_rows(0, 1).is_err());
        assert!(book.insert_columns(0, 1).is_err());
    }
    #[test]
    fn external_ods_view_changes_preserve_cells_and_opaque_parts() {
        let mut source = spreadsheet_ods::WorkBook::default();
        let mut sheet = spreadsheet_ods::Sheet::new("Sheet1");
        sheet.set_value(0, 0, "keep");
        source.push_sheet(sheet);
        let bytes = spreadsheet_ods::write_ods_buf(&mut source, Vec::new()).unwrap();
        let mut writer = zip::ZipWriter::new_append(Cursor::new(bytes)).unwrap();
        writer
            .start_file(
                "Pictures/opaque.bin",
                zip::write::SimpleFileOptions::default(),
            )
            .unwrap();
        writer.write_all(b"opaque").unwrap();
        let bytes = writer.finish().unwrap().into_inner();
        let mut book = Workbook::from_office_bytes(&bytes, "ods").unwrap();
        let mut view = book.sheet_view().clone();
        view.frozen_rows = 2;
        view.frozen_columns = 1;
        view.hidden_rows.insert(3);
        view.hidden_columns.insert(2);
        book.set_sheet_view(view.clone()).unwrap();
        let saved = book.ods_bytes().unwrap();
        let reopened = Workbook::from_office_bytes(&saved, "ods").unwrap();
        assert_eq!(reopened.sheet_view(), &view);
        assert_eq!(reopened.cell(0, 0), Some("keep"));
        let mut archive = zip::ZipArchive::new(Cursor::new(saved.as_slice())).unwrap();
        let mut opaque = String::new();
        archive
            .by_name("Pictures/opaque.bin")
            .unwrap()
            .read_to_string(&mut opaque)
            .unwrap();
        assert_eq!(opaque, "opaque");
        let mut book = reopened;
        book.set_sheet_view(SheetView::default()).unwrap();
        let saved = book.ods_bytes().unwrap();
        assert_eq!(
            Workbook::from_office_bytes(&saved, "ods")
                .unwrap()
                .sheet_view(),
            &SheetView::default()
        );
    }
    #[test]
    fn external_xlsx_view_changes_preserve_cells_and_opaque_parts() {
        let mut source = rust_xlsxwriter::Workbook::new();
        let sheet = source.add_worksheet();
        sheet.write_string(0, 0, "keep").unwrap();
        sheet.set_column_width(1, 20).unwrap();
        let bytes = source.save_to_buffer().unwrap();
        let mut writer = zip::ZipWriter::new_append(Cursor::new(bytes)).unwrap();
        writer
            .start_file(
                "customXml/opaque.txt",
                zip::write::SimpleFileOptions::default(),
            )
            .unwrap();
        writer.write_all(b"opaque").unwrap();
        let bytes = writer.finish().unwrap().into_inner();
        let mut book = Workbook::from_office_bytes(&bytes, "xlsx").unwrap();
        let mut view = book.sheet_view().clone();
        view.frozen_rows = 1;
        view.frozen_columns = 1;
        view.hidden_rows.insert(2);
        view.hidden_columns.insert(1);
        book.set_sheet_view(view.clone()).unwrap();
        assert!(book.undo());
        assert!(book.redo());
        let saved = book.xlsx_bytes().unwrap();
        let reopened = Workbook::from_office_bytes(&saved, "xlsx").unwrap();
        assert_eq!(reopened.sheet_view(), &view);
        assert_eq!(reopened.cell(0, 0), Some("keep"));
        let mut archive = zip::ZipArchive::new(Cursor::new(saved.as_slice())).unwrap();
        let mut opaque = String::new();
        archive
            .by_name("customXml/opaque.txt")
            .unwrap()
            .read_to_string(&mut opaque)
            .unwrap();
        assert_eq!(opaque, "opaque");
        let mut book = reopened;
        let mut clear = book.sheet_view().clone();
        clear.frozen_rows = 0;
        clear.frozen_columns = 0;
        clear.hidden_rows.clear();
        clear.hidden_columns.clear();
        book.set_sheet_view(clear.clone()).unwrap();
        let saved = book.xlsx_bytes().unwrap();
        assert_eq!(
            Workbook::from_office_bytes(&saved, "xlsx")
                .unwrap()
                .sheet_view(),
            &clear
        );
    }
    #[test]
    fn native_xlsx_1904_writes_epoch_and_formula_cache() {
        let mut book = Workbook::new(2, 2).unwrap();
        let mut state = book.history.current().clone();
        state.date_1904 = true;
        book.history.apply(state);
        book.set_cell(0, 0, "=DATE(1904,1,2)".into()).unwrap();
        let bytes = book.xlsx_bytes().unwrap();
        let mut archive = zip::ZipArchive::new(Cursor::new(bytes.as_slice())).unwrap();
        let workbook = zip_text(&mut archive, "xl/workbook.xml").unwrap().unwrap();
        assert_eq!(
            xml(&workbook)
                .unwrap()
                .descendants()
                .find(|n| n.has_tag_name("workbookPr"))
                .unwrap()
                .attribute("date1904"),
            Some("1")
        );
        let sheet = zip_text(&mut archive, "xl/worksheets/sheet1.xml")
            .unwrap()
            .unwrap();
        assert_eq!(
            xml(&sheet)
                .unwrap()
                .descendants()
                .find(|n| n.has_tag_name("v"))
                .unwrap()
                .text(),
            Some("1")
        );
        assert!(
            Workbook::from_office_bytes(&bytes, "xlsx")
                .unwrap()
                .history
                .current()
                .date_1904
        );
    }
    #[test]
    fn external_xlsx_1904_dates_survive_recalculation_and_save() {
        let mut source = rust_xlsxwriter::Workbook::new();
        source
            .add_worksheet()
            .write_formula(0, 0, "=DATE(1904,1,1)")
            .unwrap();
        let bytes = source.save_to_buffer().unwrap();
        let mut input = zip::ZipArchive::new(Cursor::new(bytes.as_slice())).unwrap();
        let mut output = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for index in 0..input.len() {
            let mut file = input.by_index(index).unwrap();
            if file.name() == "xl/workbook.xml" {
                let mut xml = String::new();
                file.read_to_string(&mut xml).unwrap();
                let xml = xml.replace("<workbookPr", "<workbookPr date1904=\"1\"");
                output
                    .start_file("xl/workbook.xml", zip::write::SimpleFileOptions::default())
                    .unwrap();
                output.write_all(xml.as_bytes()).unwrap();
            } else {
                output.raw_copy_file(file).unwrap();
            }
        }
        let bytes = output.finish().unwrap().into_inner();
        let mut book = Workbook::from_office_bytes(&bytes, "xlsx").unwrap();
        assert!(book.history.current().date_1904);
        assert_eq!(book.display_grid()[0][0], "0");
        book.set_cell(1, 0, "=YEAR(0)".into()).unwrap();
        let saved = book.xlsx_bytes().unwrap();
        let reopened = Workbook::from_office_bytes(&saved, "xlsx").unwrap();
        assert_eq!(reopened.display_grid()[1][0], "1904");
    }
    #[test]
    fn structural_edits_shift_filter_columns_and_sort_ranges() {
        let input = "<worksheet><autoFilter ref=\"B1:D9\"><filterColumn colId=\"1\"/><sortState ref=\"B1:D9\"><sortCondition ref=\"C2:C9\"/></sortState></autoFilter></worksheet>";
        let edits = [StructuralEdit {
            rows: false,
            at: 2,
            count: 1,
            delete: false,
        }];
        let output = patch_xlsx_filter_refs(input, &edits).unwrap();
        let d = xml(&output).unwrap();
        assert_eq!(
            d.descendants()
                .find(|n| n.has_tag_name("autoFilter"))
                .unwrap()
                .attribute("ref"),
            Some("B1:E9")
        );
        assert_eq!(
            d.descendants()
                .find(|n| n.has_tag_name("filterColumn"))
                .unwrap()
                .attribute("colId"),
            Some("2")
        );
        assert_eq!(
            d.descendants()
                .find(|n| n.has_tag_name("sortCondition"))
                .unwrap()
                .attribute("ref"),
            Some("D2:D9")
        );
    }
    #[test]
    fn structural_edits_shift_validation_and_conditional_ranges() {
        let mut book = Workbook::new(20, 8).unwrap();
        let old = book.history.current().clone();
        book.insert_rows(0, 1).unwrap();
        let mut current = book.history.current().clone();
        current.sheets[0].source_edits.push(StructuralEdit {
            rows: true,
            at: 0,
            count: 1,
            delete: false,
        });
        let sheet = &current.sheets[0];
        let input = "<worksheet><conditionalFormatting sqref=\"A1:A3\"><cfRule><formula>A1&gt;1</formula></cfRule></conditionalFormatting><dataValidations count=\"1\"><dataValidation sqref=\"B1:B3\"><formula1>A1</formula1></dataValidation></dataValidations></worksheet>";
        let output = patch_xlsx_rules(input, sheet, &old, &current).unwrap();
        let d = xml(&output).unwrap();
        assert_eq!(
            d.descendants()
                .find(|n| n.has_tag_name("conditionalFormatting"))
                .unwrap()
                .attribute("sqref"),
            Some("A2:A4")
        );
        assert_eq!(
            d.descendants()
                .find(|n| n.has_tag_name("dataValidation"))
                .unwrap()
                .attribute("sqref"),
            Some("B2:B4")
        );
        assert_eq!(
            d.descendants()
                .find(|n| n.has_tag_name("formula"))
                .unwrap()
                .text(),
            Some("A2>1")
        );
    }
    #[test]
    fn structural_edits_move_hyperlinks_and_frozen_panes() {
        let mut source = rust_xlsxwriter::Workbook::new();
        let sheet = source.add_worksheet();
        sheet.set_freeze_panes(2, 1).unwrap();
        sheet.write_url(2, 1, "https://example.com").unwrap();
        let bytes = source.save_to_buffer().unwrap();
        let mut book = Workbook::from_office_bytes(&bytes, "xlsx").unwrap();
        book.insert_rows(0, 1).unwrap();
        let saved = book.xlsx_bytes().unwrap();
        let mut archive = zip::ZipArchive::new(Cursor::new(saved.as_slice())).unwrap();
        let sheet = zip_text(&mut archive, "xl/worksheets/sheet1.xml")
            .unwrap()
            .unwrap();
        let doc = xml(&sheet).unwrap();
        let link = doc
            .descendants()
            .find(|n| n.has_tag_name("hyperlink"))
            .unwrap();
        assert_eq!(link.attribute("ref"), Some("B4"));
        let pane = doc.descendants().find(|n| n.has_tag_name("pane")).unwrap();
        assert_eq!(pane.attribute("ySplit"), Some("3"));
        assert_eq!(pane.attribute("topLeftCell"), Some("B4"));
    }
    #[test]
    fn structural_edits_update_scoped_and_global_defined_names() {
        let mut source = rust_xlsxwriter::Workbook::new();
        source
            .add_worksheet()
            .set_name("Data")
            .unwrap()
            .write_number(2, 0, 7)
            .unwrap();
        source.define_name("GlobalData", "=Data!$A$1:$A$4").unwrap();
        source
            .define_name("Data!LocalData", "=Data!$A$2:$A$4")
            .unwrap();
        let bytes = source.save_to_buffer().unwrap();
        let mut book = Workbook::from_office_bytes(&bytes, "xlsx").unwrap();
        book.delete_rows(0, 1).unwrap();
        let saved = book.xlsx_bytes().unwrap();
        let mut archive = zip::ZipArchive::new(Cursor::new(saved.as_slice())).unwrap();
        let workbook = zip_text(&mut archive, "xl/workbook.xml").unwrap().unwrap();
        let doc = xml(&workbook).unwrap();
        let names: Vec<_> = doc
            .descendants()
            .filter(|n| n.has_tag_name("definedName"))
            .filter_map(|n| n.text())
            .collect();
        assert_eq!(names, ["Data!$A$1:$A$3", "Data!$A$1:$A$3"]);
    }
    #[test]
    fn structural_edits_shift_and_shrink_merges() {
        let mut source = rust_xlsxwriter::Workbook::new();
        let format = rust_xlsxwriter::Format::new();
        source
            .add_worksheet()
            .merge_range(1, 1, 3, 2, "label", &format)
            .unwrap();
        let bytes = source.save_to_buffer().unwrap();
        let mut book = Workbook::from_office_bytes(&bytes, "xlsx").unwrap();
        book.insert_rows(0, 1).unwrap();
        book.delete_rows(3, 1).unwrap();
        let saved = book.xlsx_bytes().unwrap();
        let reopened = Workbook::from_office_bytes(&saved, "xlsx").unwrap();
        assert_eq!(reopened.cell(2, 1), Some("label"));
        assert_eq!(
            reopened.merges(),
            &[Range {
                top: 2,
                left: 1,
                bottom: 3,
                right: 2
            }]
        );
    }
    #[test]
    fn structural_edits_shift_explicit_row_and_column_geometry() {
        let mut source = rust_xlsxwriter::Workbook::new();
        let sheet = source.add_worksheet();
        sheet.write_number(1, 1, 42).unwrap();
        sheet.set_row_height(1, 30).unwrap();
        sheet.set_column_width(1, 25).unwrap();
        let bytes = source.save_to_buffer().unwrap();
        let mut book = Workbook::from_office_bytes(&bytes, "xlsx").unwrap();
        let row_height = book.row_height(1);
        let col_width = book.column_width(1);
        assert!(row_height.is_some() && col_width.is_some());
        book.insert_rows(0, 1).unwrap();
        book.insert_columns(0, 1).unwrap();
        let saved = book.xlsx_bytes().unwrap();
        let reopened = Workbook::from_office_bytes(&saved, "xlsx").unwrap();
        assert_eq!(reopened.cell(2, 2), Some("42"));
        assert_eq!(reopened.row_height(2), row_height);
        assert_eq!(reopened.column_width(2), col_width);
        assert!(reopened.row_height(1).is_none());
        assert!(reopened.column_width(1).is_none());
    }
    #[test]
    fn structural_edits_update_qualified_references_on_other_sheets() {
        let mut source = rust_xlsxwriter::Workbook::new();
        source
            .add_worksheet()
            .set_name("My Data")
            .unwrap()
            .write_number(1, 0, 7)
            .unwrap();
        source
            .add_worksheet()
            .set_name("Other")
            .unwrap()
            .write_formula(0, 0, "=SUM('My Data'!$A$1:A3)")
            .unwrap();
        let bytes = source.save_to_buffer().unwrap();
        let mut book = Workbook::from_office_bytes(&bytes, "xlsx").unwrap();
        book.delete_rows(0, 1).unwrap();
        let saved = book.xlsx_bytes().unwrap();
        let mut reopened = Workbook::from_office_bytes(&saved, "xlsx").unwrap();
        assert_eq!(reopened.cell(0, 0), Some("7"));
        let other = reopened.sheets()[1].0;
        reopened.select_sheet(other).unwrap();
        assert_eq!(reopened.cell(0, 0), Some("=SUM('My Data'!$A$1:A2)"));
    }
    #[test]
    fn duplicate_retargets_own_qualified_formulas_only() {
        let mut source = rust_xlsxwriter::Workbook::new();
        source
            .add_worksheet()
            .set_name("First")
            .unwrap()
            .write_formula(0, 0, "=First!B1+Other!B1")
            .unwrap();
        source
            .add_worksheet()
            .set_name("Other")
            .unwrap()
            .write_number(0, 1, 3)
            .unwrap();
        let bytes = source.save_to_buffer().unwrap();
        let mut book = Workbook::from_office_bytes(&bytes, "xlsx").unwrap();
        let first = book.sheets()[0].0;
        let copy = book.duplicate_sheet(first, "Copy").unwrap();
        book.select_sheet(copy).unwrap();
        assert_eq!(book.cell(0, 0), Some("='Copy'!B1+Other!B1"));
        let saved = book.xlsx_bytes().unwrap();
        let mut reopened = Workbook::from_office_bytes(&saved, "xlsx").unwrap();
        let copy = reopened.sheets().iter().find(|s| s.1 == "Copy").unwrap().0;
        reopened.select_sheet(copy).unwrap();
        assert_eq!(reopened.cell(0, 0), Some("='Copy'!B1+Other!B1"));
    }
    #[test]
    fn empty_added_external_sheet_is_written() {
        let mut source = rust_xlsxwriter::Workbook::new();
        source.add_worksheet().set_name("First").unwrap();
        let bytes = source.save_to_buffer().unwrap();
        let mut book = Workbook::from_office_bytes(&bytes, "xlsx").unwrap();
        book.add_sheet("Empty").unwrap();
        let saved = book.xlsx_bytes().unwrap();
        let reopened = Workbook::from_office_bytes(&saved, "xlsx").unwrap();
        assert_eq!(reopened.sheets().len(), 2);
        assert_eq!(reopened.sheets()[1].1, "Empty");
    }
    #[test]
    fn simple_external_structure_preserves_values_and_adjusts_formulas() {
        let mut source = rust_xlsxwriter::Workbook::new();
        let sheet = source.add_worksheet();
        sheet.write_number(0, 0, 5).unwrap();
        sheet.write_formula(1, 0, "=A1*2").unwrap();
        let bytes = source.save_to_buffer().unwrap();
        let mut book = Workbook::from_office_bytes(&bytes, "xlsx").unwrap();
        book.insert_rows(0, 1).unwrap();
        book.insert_columns(0, 1).unwrap();
        let saved = book.xlsx_bytes().unwrap();
        let reopened = Workbook::from_office_bytes(&saved, "xlsx").unwrap();
        assert_eq!(reopened.cell(1, 1), Some("5"));
        assert_eq!(reopened.cell(2, 1), Some("=B2*2"));
        book.undo();
        assert_eq!(book.cell(1, 0), Some("5"));
        book.redo();
        assert_eq!(book.cell(1, 1), Some("5"));
    }
    #[test]
    fn deleting_a_referenced_sheet_is_rejected_without_mutation() {
        let mut source = rust_xlsxwriter::Workbook::new();
        source
            .add_worksheet()
            .set_name("Referenced")
            .unwrap()
            .write_number(0, 0, 5)
            .unwrap();
        source
            .add_worksheet()
            .set_name("Other")
            .unwrap()
            .write_formula(0, 0, "=Referenced!A1")
            .unwrap();
        let bytes = source.save_to_buffer().unwrap();
        let mut book = Workbook::from_office_bytes(&bytes, "xlsx").unwrap();
        let referenced = book.sheets()[0].0;
        assert!(book.delete_sheet(referenced).is_err());
        assert_eq!(book.sheets().len(), 2);
        assert!(!book.is_dirty());
    }
    #[test]
    fn add_duplicate_delete_and_reorder_external_sheets() {
        let mut source = rust_xlsxwriter::Workbook::new();
        source
            .add_worksheet()
            .set_name("First")
            .unwrap()
            .write_string(0, 0, "alpha")
            .unwrap();
        source
            .add_worksheet()
            .set_name("Second")
            .unwrap()
            .write_string(0, 0, "beta")
            .unwrap();
        let bytes = source.save_to_buffer().unwrap();
        let mut book = Workbook::from_office_bytes(&bytes, "xlsx").unwrap();
        let first = book.sheets()[0].0;
        let second = book.sheets()[1].0;
        let duplicate = book.duplicate_sheet(first, "Copy").unwrap();
        book.select_sheet(duplicate).unwrap();
        book.set_cell(0, 0, "edited copy".into()).unwrap();
        let second_copy = book.duplicate_sheet(duplicate, "Copy 2").unwrap();
        book.select_sheet(second_copy).unwrap();
        book.set_cell(0, 0, "second copy".into()).unwrap();
        let a = book.add_sheet("Added A").unwrap();
        book.select_sheet(a).unwrap();
        book.set_cell(0, 0, "new a".into()).unwrap();
        let b = book.add_sheet("Added B").unwrap();
        book.select_sheet(b).unwrap();
        book.set_cell(0, 0, "new b".into()).unwrap();
        book.reorder_sheet(b, 0).unwrap();
        book.delete_sheet(second).unwrap();
        let saved = book.xlsx_bytes().unwrap();
        let mut reopened = Workbook::from_office_bytes(&saved, "xlsx").unwrap();
        assert_eq!(
            reopened
                .sheets()
                .iter()
                .map(|s| s.1.as_str())
                .collect::<Vec<_>>(),
            ["Added B", "First", "Copy", "Copy 2", "Added A"]
        );
        for (name, value) in [
            ("Added B", "new b"),
            ("First", "alpha"),
            ("Copy", "edited copy"),
            ("Copy 2", "second copy"),
            ("Added A", "new a"),
        ] {
            let id = reopened.sheets().iter().find(|s| s.1 == name).unwrap().0;
            reopened.select_sheet(id).unwrap();
            assert_eq!(reopened.cell(0, 0), Some(value));
        }
        book.undo();
        assert!(book.sheets().iter().any(|s| s.0 == second));
        book.redo();
        assert!(!book.sheets().iter().any(|s| s.0 == second));
    }
}
