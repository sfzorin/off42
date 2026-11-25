//! Native charts: data model, DrawingML chart parts with an embedded workbook,
//! ODF chart objects and a resolution-independent drawing layout.
use roxmltree::{Document, Node};
use serde::{Deserialize, Serialize};
use std::io::{Cursor, Write};
use zip::{write::SimpleFileOptions, CompressionMethod, ZipWriter};

pub const MAX_SERIES: usize = 64;
pub const MAX_CATEGORIES: usize = 1000;
const MAX_LABEL: usize = 1024;
/// Office theme accent colors used when a series has no explicit color.
pub const PALETTE: [[u8; 3]; 6] = [
    [0x44, 0x72, 0xC4],
    [0xED, 0x7D, 0x31],
    [0xA5, 0xA5, 0xA5],
    [0xFF, 0xC0, 0x00],
    [0x5B, 0x9B, 0xD5],
    [0x70, 0xAD, 0x47],
];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChartKind {
    Column,
    Bar,
    Line,
    Area,
    Pie,
    Doughnut,
    Scatter,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChartGrouping {
    #[default]
    Clustered,
    Stacked,
    Percent,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChartSeries {
    pub name: String,
    pub values: Vec<Option<f64>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<[u8; 3]>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Chart {
    pub kind: ChartKind,
    #[serde(default)]
    pub grouping: ChartGrouping,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub title: String,
    #[serde(default = "yes")]
    pub legend: bool,
    pub categories: Vec<String>,
    pub series: Vec<ChartSeries>,
}
fn yes() -> bool {
    true
}

/// Chart type labels offered by the editor, with their kind and grouping.
pub const TYPES: [(&str, ChartKind, ChartGrouping); 12] = [
    ("Column", ChartKind::Column, ChartGrouping::Clustered),
    ("Stacked column", ChartKind::Column, ChartGrouping::Stacked),
    ("100% stacked column", ChartKind::Column, ChartGrouping::Percent),
    ("Bar", ChartKind::Bar, ChartGrouping::Clustered),
    ("Stacked bar", ChartKind::Bar, ChartGrouping::Stacked),
    ("100% stacked bar", ChartKind::Bar, ChartGrouping::Percent),
    ("Line", ChartKind::Line, ChartGrouping::Clustered),
    ("Area", ChartKind::Area, ChartGrouping::Clustered),
    ("Stacked area", ChartKind::Area, ChartGrouping::Stacked),
    ("Pie", ChartKind::Pie, ChartGrouping::Clustered),
    ("Doughnut", ChartKind::Doughnut, ChartGrouping::Clustered),
    ("Scatter", ChartKind::Scatter, ChartGrouping::Clustered),
];

impl Chart {
    /// A small sample used by Insert Chart.
    pub fn sample() -> Self {
        Self {
            kind: ChartKind::Column,
            grouping: ChartGrouping::Clustered,
            title: String::new(),
            legend: true,
            categories: ["Q1", "Q2", "Q3", "Q4"].map(String::from).to_vec(),
            series: vec![
                ChartSeries { name: "Series 1".into(), values: [4.3, 2.5, 3.5, 4.5].map(Some).to_vec(), color: None },
                ChartSeries { name: "Series 2".into(), values: [2.4, 4.4, 1.8, 2.8].map(Some).to_vec(), color: None },
                ChartSeries { name: "Series 3".into(), values: [2.0, 2.0, 3.0, 5.0].map(Some).to_vec(), color: None },
            ],
        }
    }
    pub fn type_label(&self) -> &'static str {
        let grouping = match self.kind {
            ChartKind::Column | ChartKind::Bar | ChartKind::Area => self.grouping,
            _ => ChartGrouping::Clustered,
        };
        let grouping = if self.kind == ChartKind::Area && grouping == ChartGrouping::Percent {
            ChartGrouping::Stacked
        } else {
            grouping
        };
        TYPES
            .iter()
            .find(|(_, k, g)| *k == self.kind && *g == grouping)
            .map(|t| t.0)
            .unwrap_or("Column")
    }
    pub fn set_type_label(&mut self, label: &str) -> Result<(), String> {
        let (_, kind, grouping) = TYPES
            .iter()
            .find(|(name, _, _)| name.eq_ignore_ascii_case(label.trim()))
            .ok_or("Choose a chart type")?;
        self.kind = *kind;
        self.grouping = *grouping;
        Ok(())
    }
    pub fn color(&self, series: usize) -> [u8; 3] {
        self.series
            .get(series)
            .and_then(|s| s.color)
            .unwrap_or(PALETTE[series % PALETTE.len()])
    }
    pub fn valid(&self) -> bool {
        !self.series.is_empty()
            && self.series.len() <= MAX_SERIES
            && !self.categories.is_empty()
            && self.categories.len() <= MAX_CATEGORIES
            && self.title.len() <= MAX_LABEL
            && self.categories.iter().all(|c| c.len() <= MAX_LABEL)
            && self.series.iter().all(|s| {
                s.name.len() <= MAX_LABEL
                    && s.values.len() == self.categories.len()
                    && s.values.iter().flatten().all(|v| v.is_finite())
            })
    }
    /// Tab-separated grid: the first row names series, the first column names categories.
    pub fn grid(&self) -> String {
        let mut out = String::new();
        for s in &self.series {
            out.push('\t');
            out.push_str(&clean(&s.name));
        }
        for (i, c) in self.categories.iter().enumerate() {
            out.push('\n');
            out.push_str(&clean(c));
            for s in &self.series {
                out.push('\t');
                if let Some(v) = s.values[i] {
                    out.push_str(&number(v));
                }
            }
        }
        out
    }
    /// Replaces data from a grid; series colors stay with their column position.
    pub fn set_grid(&mut self, grid: &str) -> Result<(), String> {
        let rows = grid
            .split('\n')
            .map(|line| line.trim_end_matches('\r'))
            .filter(|line| !line.trim().is_empty())
            .map(|line| line.split('\t').map(str::trim).collect::<Vec<_>>())
            .collect::<Vec<_>>();
        let header = rows.first().ok_or("Enter a header row with series names")?;
        let columns = rows.iter().map(Vec::len).max().unwrap_or(0);
        if columns < 2 {
            return Err("Separate the series columns with tabs".into());
        }
        if rows.len() < 2 {
            return Err("Enter at least one category row".into());
        }
        if columns - 1 > MAX_SERIES || rows.len() - 1 > MAX_CATEGORIES {
            return Err("The chart has too many series or categories".into());
        }
        let mut series = (1..columns)
            .map(|c| ChartSeries {
                name: header.get(c).copied().filter(|v| !v.is_empty()).map(String::from).unwrap_or_else(|| format!("Series {c}")),
                values: Vec::new(),
                color: self.series.get(c - 1).and_then(|s| s.color),
            })
            .collect::<Vec<_>>();
        let mut categories = Vec::new();
        for (r, row) in rows.iter().enumerate().skip(1) {
            categories.push(row[0].to_owned());
            for (c, s) in series.iter_mut().enumerate() {
                let cell = row.get(c + 1).copied().unwrap_or("");
                s.values.push(if cell.is_empty() {
                    None
                } else {
                    Some(parse_number(cell).ok_or_else(|| format!("Row {}, column {} is not a number", r + 1, c + 2))?)
                });
            }
        }
        let mut next = self.clone();
        next.series = series;
        next.categories = categories;
        if !next.valid() {
            return Err("Invalid chart data".into());
        }
        *self = next;
        Ok(())
    }
    fn uses_axes(&self) -> bool {
        !matches!(self.kind, ChartKind::Pie | ChartKind::Doughnut)
    }
    fn x_values(&self) -> Vec<f64> {
        self.categories
            .iter()
            .enumerate()
            .map(|(i, c)| parse_number(c).unwrap_or(i as f64 + 1.0))
            .collect()
    }
}
fn clean(s: &str) -> String {
    s.replace(['\t', '\n', '\r'], " ")
}
fn parse_number(s: &str) -> Option<f64> {
    let v = s.trim().replace(',', ".").parse::<f64>().ok()?;
    v.is_finite().then_some(v)
}
pub fn number(v: f64) -> String {
    if v == v.trunc() && v.abs() < 1e15 {
        format!("{}", v as i64)
    } else {
        let s = format!("{v:.6}");
        s.trim_end_matches('0').trim_end_matches('.').to_owned()
    }
}
fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}
fn hex(c: [u8; 3]) -> String {
    format!("{:02X}{:02X}{:02X}", c[0], c[1], c[2])
}
fn parse_hex(v: &str) -> Option<[u8; 3]> {
    let v = v.trim_start_matches('#');
    if v.len() != 6 {
        return None;
    }
    Some([
        u8::from_str_radix(&v[0..2], 16).ok()?,
        u8::from_str_radix(&v[2..4], 16).ok()?,
        u8::from_str_radix(&v[4..6], 16).ok()?,
    ])
}
/// Spreadsheet column name for a zero-based index.
fn column(mut index: usize) -> String {
    let mut name = Vec::new();
    loop {
        name.push(b'A' + (index % 26) as u8);
        if index < 26 {
            break;
        }
        index = index / 26 - 1;
    }
    name.reverse();
    String::from_utf8(name).unwrap()
}

fn child<'a, 'i>(n: Node<'a, 'i>, name: &str) -> Option<Node<'a, 'i>> {
    n.children().find(|c| c.is_element() && c.tag_name().name() == name)
}
fn kids<'a, 'i>(n: Node<'a, 'i>, name: &'static str) -> impl Iterator<Item = Node<'a, 'i>> {
    n.children().filter(move |c| c.is_element() && c.tag_name().name() == name)
}
fn val<'a>(n: Node<'a, '_>) -> Option<&'a str> {
    n.attribute("val")
}
fn local_attr<'a>(n: Node<'a, '_>, name: &str) -> Option<&'a str> {
    n.attributes().find(|a| a.name() == name).map(|a| a.value())
}
fn texts(n: Node<'_, '_>, name: &str) -> String {
    n.descendants()
        .filter(|c| c.is_element() && c.tag_name().name() == name)
        .filter_map(|c| c.text())
        .collect()
}

/// Reads the cached points of a reference or literal (`c:strRef`, `c:numRef`, `c:strLit`, ...).
fn cache(n: Node<'_, '_>) -> Vec<Option<String>> {
    let Some(cache) = n
        .descendants()
        .find(|c| c.is_element() && matches!(c.tag_name().name(), "strCache" | "numCache" | "strLit" | "numLit"))
    else {
        return Vec::new();
    };
    let points = kids(cache, "pt")
        .filter_map(|p| Some((p.attribute("idx")?.parse::<usize>().ok()?, child(p, "v")?.text().unwrap_or("").to_owned())))
        .filter(|(i, _)| *i < MAX_CATEGORIES)
        .collect::<Vec<_>>();
    let count = child(cache, "ptCount")
        .and_then(val)
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(0)
        .max(points.iter().map(|p| p.0 + 1).max().unwrap_or(0))
        .min(MAX_CATEGORIES);
    let mut out = vec![None; count];
    for (i, v) in points {
        out[i] = Some(v);
    }
    out
}

/// Parses a DrawingML chart part. Returns `None` for unsupported chart types.
pub fn from_drawingml(bytes: &[u8]) -> Option<Chart> {
    let text = std::str::from_utf8(bytes).ok()?;
    let doc = Document::parse(text).ok()?;
    let chart = child(doc.root_element(), "chart")?;
    let plot = child(chart, "plotArea")?;
    let group = plot.children().find(|n| {
        n.is_element()
            && matches!(
                n.tag_name().name(),
                "barChart" | "bar3DChart" | "lineChart" | "line3DChart" | "areaChart" | "area3DChart"
                    | "pieChart" | "pie3DChart" | "doughnutChart" | "scatterChart"
            )
    })?;
    let grouping = match child(group, "grouping").and_then(val) {
        Some("stacked") => ChartGrouping::Stacked,
        Some("percentStacked") => ChartGrouping::Percent,
        _ => ChartGrouping::Clustered,
    };
    let kind = match group.tag_name().name() {
        "barChart" | "bar3DChart" => {
            if child(group, "barDir").and_then(val) == Some("bar") {
                ChartKind::Bar
            } else {
                ChartKind::Column
            }
        }
        "lineChart" | "line3DChart" => ChartKind::Line,
        "areaChart" | "area3DChart" => ChartKind::Area,
        "pieChart" | "pie3DChart" => ChartKind::Pie,
        "doughnutChart" => ChartKind::Doughnut,
        _ => ChartKind::Scatter,
    };
    let mut categories: Vec<String> = Vec::new();
    let mut series = Vec::new();
    for ser in kids(group, "ser").take(MAX_SERIES) {
        let name = child(ser, "tx")
            .map(|tx| {
                let cached = cache(tx).into_iter().flatten().collect::<Vec<_>>().join(" ");
                if cached.is_empty() {
                    child(tx, "v").and_then(|v| v.text()).map(String::from).unwrap_or_else(|| texts(tx, "t"))
                } else {
                    cached
                }
            })
            .unwrap_or_default();
        let cats = child(ser, "cat").or_else(|| child(ser, "xVal")).map(cache).unwrap_or_default();
        if categories.len() < cats.len() {
            categories = cats.into_iter().enumerate().map(|(i, c)| c.unwrap_or_else(|| (i + 1).to_string())).collect();
        }
        let values = child(ser, "val")
            .or_else(|| child(ser, "yVal"))
            .map(cache)
            .unwrap_or_default()
            .into_iter()
            .map(|v| v.and_then(|v| parse_number(&v)))
            .collect::<Vec<_>>();
        let color = child(ser, "spPr").and_then(|p| {
            let fill = if kind == ChartKind::Line || kind == ChartKind::Scatter {
                child(p, "ln").and_then(|l| child(l, "solidFill")).or_else(|| child(p, "solidFill"))
            } else {
                child(p, "solidFill")
            }?;
            child(fill, "srgbClr").and_then(val).and_then(parse_hex)
        });
        series.push(ChartSeries {
            name: if name.is_empty() { format!("Series {}", series.len() + 1) } else { name },
            values,
            color,
        });
    }
    let count = categories.len().max(series.iter().map(|s| s.values.len()).max().unwrap_or(0));
    if series.is_empty() || count == 0 {
        return None;
    }
    while categories.len() < count {
        categories.push((categories.len() + 1).to_string());
    }
    for s in &mut series {
        s.values.resize(count, None);
    }
    let title = child(chart, "title")
        .map(|t| {
            let rich = texts(t, "t");
            if rich.is_empty() {
                child(t, "tx").map(|tx| cache(tx).into_iter().flatten().collect::<String>()).unwrap_or_default()
            } else {
                rich
            }
        })
        .map(|t| if t.is_empty() && series.len() == 1 && child(chart, "autoTitleDeleted").and_then(val) != Some("1") { series[0].name.clone() } else { t })
        .unwrap_or_default();
    let chart = Chart {
        kind,
        grouping: if matches!(kind, ChartKind::Pie | ChartKind::Doughnut | ChartKind::Scatter) { ChartGrouping::Clustered } else { grouping },
        title,
        legend: child(chart, "legend").is_some(),
        categories,
        series,
    };
    chart.valid().then_some(chart)
}

/// Writes a DrawingML chart part. `workbook` is the relationship id of the embedded workbook.
pub fn to_drawingml(chart: &Chart, workbook: Option<&str>) -> String {
    let n = chart.categories.len();
    let last = n + 1;
    let ax = chart.uses_axes();
    let mut series = String::new();
    for (i, s) in chart.series.iter().enumerate() {
        let col = column(i + 1);
        let color = hex(chart.color(i));
        let fill = match chart.kind {
            ChartKind::Line | ChartKind::Scatter => format!("<c:spPr><a:ln w=\"28575\" cap=\"rnd\"><a:solidFill><a:srgbClr val=\"{color}\"/></a:solidFill><a:round/></a:ln></c:spPr>"),
            ChartKind::Pie | ChartKind::Doughnut => String::new(),
            _ => format!("<c:spPr><a:solidFill><a:srgbClr val=\"{color}\"/></a:solidFill></c:spPr>"),
        };
        let points = |text: bool| {
            let mut out = format!("<c:ptCount val=\"{n}\"/>");
            for (idx, c) in chart.categories.iter().enumerate() {
                let v = if text {
                    Some(esc(c))
                } else {
                    s.values[idx].map(number)
                };
                if let Some(v) = v {
                    out.push_str(&format!("<c:pt idx=\"{idx}\"><c:v>{v}</c:v></c:pt>"));
                }
            }
            out
        };
        let tx = format!("<c:tx><c:strRef><c:f>Sheet1!${col}$1</c:f><c:strCache><c:ptCount val=\"1\"/><c:pt idx=\"0\"><c:v>{}</c:v></c:pt></c:strCache></c:strRef></c:tx>", esc(&s.name));
        let values = format!("<c:numRef><c:f>Sheet1!${col}$2:${col}${last}</c:f><c:numCache><c:formatCode>General</c:formatCode>{}</c:numCache></c:numRef>", points(false));
        let head = format!("<c:ser><c:idx val=\"{i}\"/><c:order val=\"{i}\"/>{tx}{fill}");
        match chart.kind {
            ChartKind::Scatter => {
                let xs = chart.x_values();
                let mut xcache = format!("<c:ptCount val=\"{n}\"/>");
                for (idx, x) in xs.iter().enumerate() {
                    xcache.push_str(&format!("<c:pt idx=\"{idx}\"><c:v>{}</c:v></c:pt>", number(*x)));
                }
                series.push_str(&format!("{head}<c:marker><c:symbol val=\"circle\"/><c:size val=\"5\"/></c:marker><c:xVal><c:numRef><c:f>Sheet1!$A$2:$A${last}</c:f><c:numCache><c:formatCode>General</c:formatCode>{xcache}</c:numCache></c:numRef></c:xVal><c:yVal>{values}</c:yVal><c:smooth val=\"0\"/></c:ser>"));
            }
            _ => {
                let cat = format!("<c:cat><c:strRef><c:f>Sheet1!$A$2:$A${last}</c:f><c:strCache>{}</c:strCache></c:strRef></c:cat>", points(true));
                let mid = match chart.kind {
                    ChartKind::Column | ChartKind::Bar => "<c:invertIfNegative val=\"0\"/>",
                    ChartKind::Line => "<c:marker><c:symbol val=\"circle\"/><c:size val=\"5\"/></c:marker>",
                    _ => "",
                };
                let tail = if chart.kind == ChartKind::Line { "<c:smooth val=\"0\"/>" } else { "" };
                series.push_str(&format!("{head}{mid}{cat}<c:val>{values}</c:val>{tail}</c:ser>"));
            }
        }
    }
    let grouping = match chart.grouping {
        ChartGrouping::Clustered => "clustered",
        ChartGrouping::Stacked => "stacked",
        ChartGrouping::Percent => "percentStacked",
    };
    let standard = if chart.grouping == ChartGrouping::Clustered { "standard" } else { grouping };
    let axes = "<c:axId val=\"500000001\"/><c:axId val=\"500000002\"/>";
    let group = match chart.kind {
        ChartKind::Column | ChartKind::Bar => format!(
            "<c:barChart><c:barDir val=\"{}\"/><c:grouping val=\"{grouping}\"/><c:varyColors val=\"0\"/>{series}<c:gapWidth val=\"150\"/>{}{axes}</c:barChart>",
            if chart.kind == ChartKind::Bar { "bar" } else { "col" },
            if chart.grouping == ChartGrouping::Clustered { "" } else { "<c:overlap val=\"100\"/>" }
        ),
        ChartKind::Line => format!("<c:lineChart><c:grouping val=\"{standard}\"/><c:varyColors val=\"0\"/>{series}<c:marker val=\"1\"/>{axes}</c:lineChart>"),
        ChartKind::Area => format!("<c:areaChart><c:grouping val=\"{standard}\"/><c:varyColors val=\"0\"/>{series}{axes}</c:areaChart>"),
        ChartKind::Pie => format!("<c:pieChart><c:varyColors val=\"1\"/>{series}<c:firstSliceAng val=\"0\"/></c:pieChart>"),
        ChartKind::Doughnut => format!("<c:doughnutChart><c:varyColors val=\"1\"/>{series}<c:firstSliceAng val=\"0\"/><c:holeSize val=\"50\"/></c:doughnutChart>"),
        ChartKind::Scatter => format!("<c:scatterChart><c:scatterStyle val=\"lineMarker\"/><c:varyColors val=\"0\"/>{series}{axes}</c:scatterChart>"),
    };
    let common = "<c:majorTickMark val=\"out\"/><c:minorTickMark val=\"none\"/><c:tickLblPos val=\"nextTo\"/>";
    let (cat_pos, val_pos) = if chart.kind == ChartKind::Bar { ("l", "b") } else { ("b", "l") };
    let axes_xml = if !ax {
        String::new()
    } else if chart.kind == ChartKind::Scatter {
        format!("<c:valAx><c:axId val=\"500000001\"/><c:scaling><c:orientation val=\"minMax\"/></c:scaling><c:delete val=\"0\"/><c:axPos val=\"b\"/><c:numFmt formatCode=\"General\" sourceLinked=\"1\"/>{common}<c:crossAx val=\"500000002\"/><c:crosses val=\"autoZero\"/><c:crossBetween val=\"midCat\"/></c:valAx><c:valAx><c:axId val=\"500000002\"/><c:scaling><c:orientation val=\"minMax\"/></c:scaling><c:delete val=\"0\"/><c:axPos val=\"l\"/><c:majorGridlines/><c:numFmt formatCode=\"General\" sourceLinked=\"1\"/>{common}<c:crossAx val=\"500000001\"/><c:crosses val=\"autoZero\"/><c:crossBetween val=\"midCat\"/></c:valAx>")
    } else {
        let format = if chart.grouping == ChartGrouping::Percent { "0%" } else { "General" };
        format!("<c:catAx><c:axId val=\"500000001\"/><c:scaling><c:orientation val=\"minMax\"/></c:scaling><c:delete val=\"0\"/><c:axPos val=\"{cat_pos}\"/><c:numFmt formatCode=\"General\" sourceLinked=\"1\"/>{common}<c:crossAx val=\"500000002\"/><c:crosses val=\"autoZero\"/><c:auto val=\"1\"/><c:lblAlgn val=\"ctr\"/><c:lblOffset val=\"100\"/><c:noMultiLvlLbl val=\"0\"/></c:catAx><c:valAx><c:axId val=\"500000002\"/><c:scaling><c:orientation val=\"minMax\"/></c:scaling><c:delete val=\"0\"/><c:axPos val=\"{val_pos}\"/><c:majorGridlines/><c:numFmt formatCode=\"{format}\" sourceLinked=\"1\"/>{common}<c:crossAx val=\"500000001\"/><c:crosses val=\"autoZero\"/><c:crossBetween val=\"between\"/></c:valAx>")
    };
    let title = if chart.title.is_empty() {
        "<c:autoTitleDeleted val=\"1\"/>".to_owned()
    } else {
        format!("<c:title><c:tx><c:rich><a:bodyPr/><a:lstStyle/><a:p><a:pPr><a:defRPr sz=\"1400\" b=\"1\"/></a:pPr><a:r><a:rPr lang=\"en-US\" sz=\"1400\" b=\"1\"/><a:t>{}</a:t></a:r></a:p></c:rich></c:tx><c:overlay val=\"0\"/></c:title><c:autoTitleDeleted val=\"0\"/>", esc(&chart.title))
    };
    let legend = if chart.legend {
        "<c:legend><c:legendPos val=\"r\"/><c:overlay val=\"0\"/></c:legend>"
    } else {
        ""
    };
    let external = workbook
        .map(|id| format!("<c:externalData r:id=\"{}\"><c:autoUpdate val=\"0\"/></c:externalData>", esc(id)))
        .unwrap_or_default();
    format!("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><c:chartSpace xmlns:c=\"http://schemas.openxmlformats.org/drawingml/2006/chart\" xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\"><c:roundedCorners val=\"0\"/><c:chart>{title}<c:plotArea><c:layout/>{group}{axes_xml}</c:plotArea>{legend}<c:plotVisOnly val=\"1\"/><c:dispBlanksAs val=\"gap\"/></c:chart>{external}</c:chartSpace>")
}

/// A minimal SpreadsheetML workbook holding the chart data, so Office can edit it.
pub fn workbook(chart: &Chart) -> Result<Vec<u8>, String> {
    let main = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";
    let rel = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
    let pkg = "http://schemas.openxmlformats.org/package/2006/relationships";
    let text_cell = |r: &str, v: &str| format!("<c r=\"{r}\" t=\"inlineStr\"><is><t xml:space=\"preserve\">{}</t></is></c>", esc(v));
    let mut rows = String::from("<row r=\"1\">");
    for (i, s) in chart.series.iter().enumerate() {
        rows.push_str(&text_cell(&format!("{}1", column(i + 1)), &s.name));
    }
    rows.push_str("</row>");
    let xs = chart.x_values();
    for (r, c) in chart.categories.iter().enumerate() {
        let row = r + 2;
        rows.push_str(&format!("<row r=\"{row}\">"));
        if chart.kind == ChartKind::Scatter {
            rows.push_str(&format!("<c r=\"A{row}\"><v>{}</v></c>", number(xs[r])));
        } else {
            rows.push_str(&text_cell(&format!("A{row}"), c));
        }
        for (i, s) in chart.series.iter().enumerate() {
            if let Some(v) = s.values[r] {
                rows.push_str(&format!("<c r=\"{}{row}\"><v>{}</v></c>", column(i + 1), number(v)));
            }
        }
        rows.push_str("</row>");
    }
    let parts = [
        ("[Content_Types].xml", format!("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/><Default Extension=\"xml\" ContentType=\"application/xml\"/><Override PartName=\"/xl/workbook.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml\"/><Override PartName=\"/xl/worksheets/sheet1.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml\"/></Types>")),
        ("_rels/.rels", format!("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><Relationships xmlns=\"{pkg}\"><Relationship Id=\"rId1\" Type=\"{rel}/officeDocument\" Target=\"xl/workbook.xml\"/></Relationships>")),
        ("xl/workbook.xml", format!("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><workbook xmlns=\"{main}\" xmlns:r=\"{rel}\"><sheets><sheet name=\"Sheet1\" sheetId=\"1\" r:id=\"rId1\"/></sheets></workbook>")),
        ("xl/_rels/workbook.xml.rels", format!("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><Relationships xmlns=\"{pkg}\"><Relationship Id=\"rId1\" Type=\"{rel}/worksheet\" Target=\"worksheets/sheet1.xml\"/></Relationships>")),
        ("xl/worksheets/sheet1.xml", format!("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><worksheet xmlns=\"{main}\"><sheetData>{rows}</sheetData></worksheet>")),
    ];
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    for (name, data) in parts {
        zip.start_file(name, options).map_err(|e| e.to_string())?;
        zip.write_all(data.as_bytes()).map_err(|e| e.to_string())?;
    }
    Ok(zip.finish().map_err(|e| e.to_string())?.into_inner())
}

pub const ODF_CHART_MIME: &str = "application/vnd.oasis.opendocument.chart";

/// Parses the `content.xml` of an embedded ODF chart object.
pub fn from_odf(bytes: &[u8]) -> Option<Chart> {
    let text = std::str::from_utf8(bytes).ok()?;
    let doc = Document::parse(text).ok()?;
    let chart = doc.descendants().find(|n| n.is_element() && n.has_tag_name(("urn:oasis:names:tc:opendocument:xmlns:chart:1.0", "chart")))?;
    let class = local_attr(chart, "class")?.rsplit(':').next()?;
    let style = |name: Option<&str>, props: &str| {
        let name = name?;
        doc.descendants()
            .find(|n| n.is_element() && n.tag_name().name() == "style" && local_attr(*n, "name") == Some(name))
            .and_then(|s| child(s, props))
    };
    let plot = child(chart, "plot-area")?;
    let props = style(local_attr(plot, "style-name"), "chart-properties");
    let flag = |name: &str| props.and_then(|p| local_attr(p, name)) == Some("true");
    let grouping = if flag("percentage") {
        ChartGrouping::Percent
    } else if flag("stacked") {
        ChartGrouping::Stacked
    } else {
        ChartGrouping::Clustered
    };
    let kind = match class {
        "bar" if flag("vertical") => ChartKind::Bar,
        "bar" => ChartKind::Column,
        "line" => ChartKind::Line,
        "area" => ChartKind::Area,
        "circle" => ChartKind::Pie,
        "ring" => ChartKind::Doughnut,
        "scatter" => ChartKind::Scatter,
        _ => return None,
    };
    let table = chart.parent()?.descendants().find(|n| n.is_element() && n.tag_name().name() == "table")?;
    let cell_text = |c: Node<'_, '_>| texts(c, "p");
    let cells = |row: Node<'_, '_>| {
        let mut out = Vec::new();
        for c in row.children().filter(|c| c.is_element() && matches!(c.tag_name().name(), "table-cell" | "covered-table-cell")) {
            let repeat = local_attr(c, "number-columns-repeated").and_then(|v| v.parse::<usize>().ok()).unwrap_or(1).clamp(1, MAX_SERIES + 1);
            for _ in 0..repeat {
                let numeric = local_attr(c, "value").and_then(parse_number);
                out.push((cell_text(c), numeric));
            }
            if out.len() > MAX_SERIES + 1 {
                break;
            }
        }
        out
    };
    let rows = table
        .descendants()
        .filter(|n| n.is_element() && n.tag_name().name() == "table-row")
        .take(MAX_CATEGORIES + 1)
        .map(cells)
        .collect::<Vec<_>>();
    let header = rows.first()?;
    let width = rows.iter().map(Vec::len).max()?;
    if width < 2 || rows.len() < 2 {
        return None;
    }
    let colors = kids(plot, "series")
        .map(|s| {
            style(local_attr(s, "style-name"), "graphic-properties").and_then(|g| {
                let key = if kind == ChartKind::Line || kind == ChartKind::Scatter { "stroke-color" } else { "fill-color" };
                local_attr(g, key).or_else(|| local_attr(g, "fill-color")).and_then(parse_hex)
            })
        })
        .collect::<Vec<_>>();
    let series = (1..width)
        .map(|c| ChartSeries {
            name: header.get(c).map(|v| v.0.clone()).filter(|v| !v.is_empty()).unwrap_or_else(|| format!("Series {c}")),
            values: rows[1..].iter().map(|r| r.get(c).and_then(|v| v.1.or_else(|| parse_number(&v.0)))).collect(),
            color: colors.get(c - 1).copied().flatten(),
        })
        .collect();
    let categories = rows[1..]
        .iter()
        .enumerate()
        .map(|(i, r)| r.first().map(|v| v.0.clone()).filter(|v| !v.is_empty()).unwrap_or_else(|| (i + 1).to_string()))
        .collect();
    let chart = Chart {
        kind,
        grouping: if matches!(kind, ChartKind::Pie | ChartKind::Doughnut | ChartKind::Scatter) { ChartGrouping::Clustered } else { grouping },
        title: child(chart, "title").map(|t| texts(t, "p")).unwrap_or_default(),
        legend: child(chart, "legend").is_some(),
        categories,
        series,
    };
    chart.valid().then_some(chart)
}

/// Writes the `content.xml` of an embedded ODF chart object sized in points.
pub fn to_odf(chart: &Chart, width: f64, height: f64) -> String {
    let n = chart.categories.len();
    let last = n + 1;
    let lastcol = column(chart.series.len());
    let class = match chart.kind {
        ChartKind::Column | ChartKind::Bar => "bar",
        ChartKind::Line => "line",
        ChartKind::Area => "area",
        ChartKind::Pie => "circle",
        ChartKind::Doughnut => "ring",
        ChartKind::Scatter => "scatter",
    };
    let mut styles = format!(
        "<style:style style:name=\"pa\" style:family=\"chart\"><style:chart-properties chart:vertical=\"{}\" chart:stacked=\"{}\" chart:percentage=\"{}\"{}/></style:style>",
        chart.kind == ChartKind::Bar,
        chart.grouping == ChartGrouping::Stacked,
        chart.grouping == ChartGrouping::Percent,
        if matches!(chart.kind, ChartKind::Line | ChartKind::Scatter) { " chart:symbol-type=\"automatic\"" } else { "" }
    );
    let mut series = String::new();
    for (i, _) in chart.series.iter().enumerate() {
        let col = column(i + 1);
        let c = hex(chart.color(i));
        let graphic = if matches!(chart.kind, ChartKind::Line | ChartKind::Scatter) {
            format!("draw:stroke=\"solid\" svg:stroke-width=\"0.07cm\" svg:stroke-color=\"#{c}\" draw:fill-color=\"#{c}\"")
        } else {
            format!("draw:fill=\"solid\" draw:fill-color=\"#{c}\"")
        };
        styles.push_str(&format!("<style:style style:name=\"s{i}\" style:family=\"chart\"><style:graphic-properties {graphic}/></style:style>"));
        let points = if matches!(chart.kind, ChartKind::Pie | ChartKind::Doughnut) {
            (0..n)
                .map(|p| {
                    let name = format!("p{i}_{p}");
                    styles.push_str(&format!("<style:style style:name=\"{name}\" style:family=\"chart\"><style:graphic-properties draw:fill=\"solid\" draw:fill-color=\"#{}\"/></style:style>", hex(PALETTE[p % PALETTE.len()])));
                    format!("<chart:data-point chart:style-name=\"{name}\"/>")
                })
                .collect::<String>()
        } else {
            String::new()
        };
        let domain = if chart.kind == ChartKind::Scatter {
            format!("<chart:domain table:cell-range-address=\"local-table.$A$2:.$A${last}\"/>")
        } else {
            String::new()
        };
        series.push_str(&format!("<chart:series chart:style-name=\"s{i}\" chart:class=\"chart:{class}\" chart:values-cell-range-address=\"local-table.${col}$2:.${col}${last}\" chart:label-cell-address=\"local-table.${col}$1\">{domain}{points}</chart:series>"));
    }
    let axes = if chart.uses_axes() {
        format!("<chart:axis chart:dimension=\"x\" chart:name=\"primary-x\"><chart:categories table:cell-range-address=\"local-table.$A$2:.$A${last}\"/></chart:axis><chart:axis chart:dimension=\"y\" chart:name=\"primary-y\"><chart:grid chart:class=\"major\"/></chart:axis>")
    } else {
        format!("<chart:axis chart:dimension=\"x\" chart:name=\"primary-x\"><chart:categories table:cell-range-address=\"local-table.$A$2:.$A${last}\"/></chart:axis>")
    };
    let title = if chart.title.is_empty() {
        String::new()
    } else {
        format!("<chart:title><text:p>{}</text:p></chart:title>", esc(&chart.title))
    };
    let legend = if chart.legend { "<chart:legend chart:legend-position=\"end\" style:legend-expansion=\"high\"/>" } else { "" };
    let mut table = format!("<table:table table:name=\"local-table\"><table:table-header-columns><table:table-column/></table:table-header-columns><table:table-columns><table:table-column table:number-columns-repeated=\"{}\"/></table:table-columns><table:table-header-rows><table:table-row><table:table-cell/>", chart.series.len());
    for s in &chart.series {
        table.push_str(&format!("<table:table-cell office:value-type=\"string\"><text:p>{}</text:p></table:table-cell>", esc(&s.name)));
    }
    table.push_str("</table:table-row></table:table-header-rows><table:table-rows>");
    let xs = chart.x_values();
    for (r, c) in chart.categories.iter().enumerate() {
        table.push_str("<table:table-row>");
        if chart.kind == ChartKind::Scatter {
            let x = number(xs[r]);
            table.push_str(&format!("<table:table-cell office:value-type=\"float\" office:value=\"{x}\"><text:p>{x}</text:p></table:table-cell>"));
        } else {
            table.push_str(&format!("<table:table-cell office:value-type=\"string\"><text:p>{}</text:p></table:table-cell>", esc(c)));
        }
        for s in &chart.series {
            match s.values[r] {
                Some(v) => {
                    let v = number(v);
                    table.push_str(&format!("<table:table-cell office:value-type=\"float\" office:value=\"{v}\"><text:p>{v}</text:p></table:table-cell>"));
                }
                None => table.push_str("<table:table-cell/>"),
            }
        }
        table.push_str("</table:table-row>");
    }
    table.push_str("</table:table-rows></table:table>");
    format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?><office:document-content xmlns:office=\"urn:oasis:names:tc:opendocument:xmlns:office:1.0\" xmlns:style=\"urn:oasis:names:tc:opendocument:xmlns:style:1.0\" xmlns:text=\"urn:oasis:names:tc:opendocument:xmlns:text:1.0\" xmlns:table=\"urn:oasis:names:tc:opendocument:xmlns:table:1.0\" xmlns:draw=\"urn:oasis:names:tc:opendocument:xmlns:drawing:1.0\" xmlns:fo=\"urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0\" xmlns:svg=\"urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0\" xmlns:chart=\"urn:oasis:names:tc:opendocument:xmlns:chart:1.0\" xmlns:xlink=\"http://www.w3.org/1999/xlink\" office:version=\"1.2\"><office:automatic-styles>{styles}</office:automatic-styles><office:body><office:chart><chart:chart chart:class=\"chart:{class}\" svg:width=\"{:.2}pt\" svg:height=\"{:.2}pt\">{title}{legend}<chart:plot-area chart:style-name=\"pa\" table:cell-range-address=\"local-table.$A$1:.${lastcol}${last}\" chart:data-source-has-labels=\"both\">{axes}{series}</chart:plot-area></chart:chart>{table}</office:chart></office:body></office:document-content>", width, height)
}

/// Device-independent drawing primitives in coordinates relative to the chart frame.
#[derive(Clone, Debug, PartialEq)]
pub enum Mark {
    Rect { x: f64, y: f64, width: f64, height: f64, fill: [u8; 3], stroke: Option<[u8; 3]> },
    Polygon { points: Vec<(f64, f64)>, fill: [u8; 3], stroke: Option<[u8; 3]> },
    Line { points: Vec<(f64, f64)>, color: [u8; 3] },
    /// `align`: -1 left, 0 center, 1 right.
    Text { x: f64, y: f64, width: f64, height: f64, text: String, points: f64, bold: bool, align: i8 },
}

fn nice_step(range: f64) -> f64 {
    if range <= 0.0 || !range.is_finite() {
        return 1.0;
    }
    let raw = range / 5.0;
    let magnitude = 10f64.powf(raw.log10().floor());
    let normalized = raw / magnitude;
    let nice = if normalized <= 1.0 {
        1.0
    } else if normalized <= 2.0 {
        2.0
    } else if normalized <= 2.5 {
        2.5
    } else if normalized <= 5.0 {
        5.0
    } else {
        10.0
    };
    nice * magnitude
}
fn nice_range(min: f64, max: f64) -> (f64, f64, f64) {
    let (min, max) = if min == max { (min.min(0.0), max.max(min + 1.0)) } else { (min, max) };
    let step = nice_step(max - min);
    let lo = (min / step).floor() * step;
    let hi = (max / step).ceil() * step;
    (lo, if hi <= lo { lo + step } else { hi }, step)
}

const GRID: [u8; 3] = [217, 217, 217];

/// Lays the chart out in a `width` x `height` frame.
pub fn layout(chart: &Chart, width: f64, height: f64) -> Vec<Mark> {
    let mut marks = vec![Mark::Rect { x: 0.0, y: 0.0, width, height, fill: [255, 255, 255], stroke: Some(GRID) }];
    if !chart.valid() || width < 20.0 || height < 20.0 {
        return marks;
    }
    let pad = 6.0;
    let mut top = pad;
    if !chart.title.is_empty() {
        marks.push(Mark::Text { x: pad, y: pad, width: width - 2.0 * pad, height: 20.0, text: chart.title.clone(), points: 13.0, bold: true, align: 0 });
        top += 22.0;
    }
    let pie = !chart.uses_axes();
    let entries: Vec<(String, [u8; 3])> = if pie {
        chart.categories.iter().enumerate().map(|(i, c)| (c.clone(), PALETTE[i % PALETTE.len()])).collect()
    } else {
        chart.series.iter().enumerate().map(|(i, s)| (s.name.clone(), chart.color(i))).collect()
    };
    let mut right = width - pad;
    if chart.legend && width > 120.0 {
        let legend_w = (width * 0.24).clamp(50.0, 140.0);
        right -= legend_w;
        let row = 14.0;
        let total = row * entries.len() as f64;
        let mut y = top + ((height - top - pad) - total).max(0.0) / 2.0;
        for (name, color) in &entries {
            if y + row > height - pad {
                break;
            }
            marks.push(Mark::Rect { x: right + 6.0, y: y + 3.0, width: 8.0, height: 8.0, fill: *color, stroke: None });
            marks.push(Mark::Text { x: right + 18.0, y, width: legend_w - 20.0, height: row, text: name.clone(), points: 8.0, bold: false, align: -1 });
            y += row;
        }
    }
    let bottom = height - pad;
    if pie {
        let first = &chart.series[0];
        let total: f64 = first.values.iter().flatten().filter(|v| **v > 0.0).sum();
        let cx = (pad + right) / 2.0;
        let cy = (top + bottom) / 2.0;
        let r = ((right - pad).min(bottom - top) / 2.0 - 4.0).max(4.0);
        let inner = if chart.kind == ChartKind::Doughnut { r * 0.5 } else { 0.0 };
        if total <= 0.0 {
            return marks;
        }
        let mut angle = -std::f64::consts::FRAC_PI_2;
        for (i, v) in first.values.iter().enumerate() {
            let Some(v) = v.filter(|v| *v > 0.0) else { continue };
            let sweep = v / total * std::f64::consts::TAU;
            let steps = ((sweep / 0.05).ceil() as usize).max(2);
            let mut points = Vec::new();
            for k in 0..=steps {
                let a = angle + sweep * k as f64 / steps as f64;
                points.push((cx + r * a.cos(), cy + r * a.sin()));
            }
            if inner > 0.0 {
                for k in (0..=steps).rev() {
                    let a = angle + sweep * k as f64 / steps as f64;
                    points.push((cx + inner * a.cos(), cy + inner * a.sin()));
                }
            } else {
                points.push((cx, cy));
            }
            marks.push(Mark::Polygon { points, fill: PALETTE[i % PALETTE.len()], stroke: Some([255, 255, 255]) });
            angle += sweep;
        }
        return marks;
    }
    // Axis ranges.
    let n = chart.categories.len();
    let horizontal = chart.kind == ChartKind::Bar;
    let stacked = chart.grouping != ChartGrouping::Clustered && matches!(chart.kind, ChartKind::Column | ChartKind::Bar | ChartKind::Area | ChartKind::Line);
    let percent = chart.grouping == ChartGrouping::Percent && stacked;
    let totals = (0..n)
        .map(|c| chart.series.iter().filter_map(|s| s.values[c]).map(f64::abs).sum::<f64>())
        .collect::<Vec<_>>();
    let value = |s: usize, c: usize| -> Option<f64> {
        let v = chart.series[s].values[c]?;
        Some(if percent { if totals[c] == 0.0 { 0.0 } else { v / totals[c] * 100.0 } } else { v })
    };
    let (mut lo, mut hi) = (0f64, 0f64);
    for c in 0..n {
        if stacked {
            let (mut pos, mut neg) = (0.0, 0.0);
            for s in 0..chart.series.len() {
                if let Some(v) = value(s, c) {
                    if v >= 0.0 { pos += v } else { neg += v }
                }
            }
            hi = hi.max(pos);
            lo = lo.min(neg);
        } else {
            for s in 0..chart.series.len() {
                if let Some(v) = value(s, c) {
                    hi = hi.max(v);
                    lo = lo.min(v);
                }
            }
        }
    }
    if percent {
        hi = hi.min(100.0).max(if hi > 0.0 { 100.0 } else { 0.0 });
    }
    let (lo, hi, step) = nice_range(lo, hi);
    let xs = chart.x_values();
    let (xlo, xhi, xstep) = if chart.kind == ChartKind::Scatter {
        let min = xs.iter().copied().fold(f64::INFINITY, f64::min);
        let max = xs.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        nice_range(min.min(0.0), max)
    } else {
        (0.0, 1.0, 1.0)
    };
    let label = |v: f64| if percent { format!("{}%", number(v)) } else { number((v / step).round() * step) };
    let value_labels = {
        let mut out = Vec::new();
        let mut v = lo;
        while v <= hi + step * 1e-6 && out.len() < 40 {
            out.push(v);
            v += step;
        }
        out
    };
    let label_w = if horizontal { 0.0 } else { value_labels.iter().map(|v| label(*v).chars().count()).max().unwrap_or(1) as f64 * 5.0 + 6.0 };
    let cat_w = if horizontal { chart.categories.iter().map(|c| c.chars().count()).max().unwrap_or(1).min(16) as f64 * 5.0 + 6.0 } else { 0.0 };
    let left = pad + label_w.max(cat_w);
    let plot_bottom = bottom - 14.0;
    let (pw, ph) = (right - left - 4.0, plot_bottom - top);
    if pw < 10.0 || ph < 10.0 {
        return marks;
    }
    let plot_right = left + pw;
    // Gridlines and value labels.
    let vpos = |v: f64| -> f64 {
        let t = (v - lo) / (hi - lo);
        if horizontal { left + t * pw } else { plot_bottom - t * ph }
    };
    for v in &value_labels {
        let p = vpos(*v);
        if horizontal {
            marks.push(Mark::Line { points: vec![(p, top), (p, plot_bottom)], color: GRID });
            marks.push(Mark::Text { x: p - 20.0, y: plot_bottom + 1.0, width: 40.0, height: 12.0, text: label(*v), points: 7.5, bold: false, align: 0 });
        } else {
            marks.push(Mark::Line { points: vec![(left, p), (plot_right, p)], color: GRID });
            marks.push(Mark::Text { x: pad, y: p - 6.0, width: left - pad - 3.0, height: 12.0, text: label(*v), points: 7.5, bold: false, align: 1 });
        }
    }
    // Category axis and labels.
    let band = if horizontal { ph / n as f64 } else { pw / n as f64 };
    let xpos = |x: f64| left + (x - xlo) / (xhi - xlo) * pw;
    if chart.kind == ChartKind::Scatter {
        let mut x = xlo;
        while x <= xhi + xstep * 1e-6 {
            let p = xpos(x);
            marks.push(Mark::Text { x: p - 20.0, y: plot_bottom + 1.0, width: 40.0, height: 12.0, text: number((x / xstep).round() * xstep), points: 7.5, bold: false, align: 0 });
            x += xstep;
        }
    } else {
        let every = ((n as f64 * 30.0) / if horizontal { ph } else { pw }).ceil().max(1.0) as usize;
        for (i, c) in chart.categories.iter().enumerate().filter(|(i, _)| i % every == 0) {
            if horizontal {
                let y = top + band * (n - 1 - i) as f64;
                marks.push(Mark::Text { x: pad, y: y + band / 2.0 - 6.0, width: left - pad - 3.0, height: 12.0, text: c.clone(), points: 7.5, bold: false, align: 1 });
            } else {
                let x = left + band * i as f64;
                marks.push(Mark::Text { x: x - band * (every as f64 - 1.0) / 2.0, y: plot_bottom + 1.0, width: band * every as f64, height: 12.0, text: c.clone(), points: 7.5, bold: false, align: 0 });
            }
        }
    }
    let zero = vpos(0f64.clamp(lo, hi));
    if horizontal {
        marks.push(Mark::Line { points: vec![(zero, top), (zero, plot_bottom)], color: [191, 191, 191] });
    } else {
        marks.push(Mark::Line { points: vec![(left, zero), (plot_right, zero)], color: [191, 191, 191] });
    }
    let center = |c: usize| if horizontal { top + band * (n - 1 - c) as f64 + band / 2.0 } else { left + band * c as f64 + band / 2.0 };
    match chart.kind {
        ChartKind::Column | ChartKind::Bar => {
            let count = chart.series.len();
            let group = band * 0.66;
            let thick = if stacked { group } else { group / count as f64 };
            for c in 0..n {
                let (mut pos, mut neg) = (0.0, 0.0);
                for s in 0..count {
                    let Some(v) = value(s, c) else { continue };
                    let (a, b) = if stacked {
                        let base = if v >= 0.0 { pos } else { neg };
                        if v >= 0.0 { pos += v } else { neg += v }
                        (base, base + v)
                    } else {
                        (0.0, v)
                    };
                    let (p0, p1) = (vpos(a.clamp(lo, hi)), vpos(b.clamp(lo, hi)));
                    let offset = if stacked { -group / 2.0 } else { -group / 2.0 + thick * s as f64 };
                    let mid = center(c) + offset;
                    let fill = chart.color(s);
                    if horizontal {
                        marks.push(Mark::Rect { x: p0.min(p1), y: mid, width: (p1 - p0).abs().max(0.5), height: thick, fill, stroke: None });
                    } else {
                        marks.push(Mark::Rect { x: mid, y: p0.min(p1), width: thick, height: (p1 - p0).abs().max(0.5), fill, stroke: None });
                    }
                }
            }
        }
        ChartKind::Line | ChartKind::Area => {
            let mut base = vec![0.0; n];
            for s in 0..chart.series.len() {
                let color = chart.color(s);
                let mut top_line = Vec::new();
                let mut bottom_line = Vec::new();
                for c in 0..n {
                    let v = value(s, c);
                    if v.is_none() && chart.kind == ChartKind::Line && !stacked {
                        if top_line.len() > 1 {
                            marks.push(Mark::Line { points: std::mem::take(&mut top_line), color });
                        }
                        top_line.clear();
                        continue;
                    }
                    let v = v.unwrap_or(0.0);
                    let (a, b) = if stacked { (base[c], base[c] + v) } else { (0.0, v) };
                    if stacked {
                        base[c] += v;
                    }
                    let x = center(c);
                    top_line.push((x, vpos(b.clamp(lo, hi))));
                    bottom_line.push((x, vpos(a.clamp(lo, hi))));
                }
                if chart.kind == ChartKind::Area {
                    let mut points = top_line.clone();
                    points.extend(bottom_line.iter().rev());
                    if points.len() >= 3 {
                        marks.push(Mark::Polygon { points, fill: color, stroke: None });
                    }
                } else {
                    for (x, y) in &top_line {
                        marks.push(Mark::Rect { x: x - 2.5, y: y - 2.5, width: 5.0, height: 5.0, fill: color, stroke: None });
                    }
                    if top_line.len() > 1 {
                        marks.push(Mark::Line { points: top_line, color });
                    }
                }
            }
        }
        ChartKind::Scatter => {
            for s in 0..chart.series.len() {
                let color = chart.color(s);
                let points = (0..n)
                    .filter_map(|c| Some((xpos(xs[c]), vpos(value(s, c)?.clamp(lo, hi)))))
                    .collect::<Vec<_>>();
                for (x, y) in &points {
                    marks.push(Mark::Rect { x: x - 2.5, y: y - 2.5, width: 5.0, height: 5.0, fill: color, stroke: None });
                }
                if points.len() > 1 {
                    marks.push(Mark::Line { points, color });
                }
            }
        }
        ChartKind::Pie | ChartKind::Doughnut => {}
    }
    marks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grid_roundtrip_and_errors() {
        let mut chart = Chart::sample();
        chart.series[1].color = Some([1, 2, 3]);
        let grid = chart.grid();
        let mut copy = chart.clone();
        copy.set_grid(&grid.replace('\n', "\r\n")).unwrap();
        assert_eq!(copy, chart);
        copy.set_grid("\tA\tB\nx\t1\t\ny\t2,5\t3").unwrap();
        assert_eq!(copy.series.len(), 2);
        assert_eq!(copy.series[1].values, vec![None, Some(3.0)]);
        assert_eq!(copy.series[0].values, vec![Some(1.0), Some(2.5)]);
        assert_eq!(copy.series[1].color, Some([1, 2, 3]));
        assert!(copy.set_grid("\tA\nx\tbad").unwrap_err().contains("not a number"));
        assert!(copy.set_grid("only").is_err());
    }

    #[test]
    fn drawingml_and_odf_roundtrip_every_type() {
        for (label, _, _) in TYPES {
            let mut chart = Chart::sample();
            chart.title = "Sales & <Costs>".into();
            chart.set_type_label(label).unwrap();
            if chart.kind == ChartKind::Scatter {
                chart.categories = vec!["1".into(), "2".into(), "4".into(), "8".into()];
            }
            chart.series[0].values[2] = None;
            let mut expected = chart.clone();
            for (i, s) in expected.series.iter_mut().enumerate() {
                s.color = Some(PALETTE[i]);
            }
            let ooxml = to_drawingml(&chart, Some("rId1"));
            roxmltree::Document::parse(&ooxml).unwrap();
            let parsed = from_drawingml(ooxml.as_bytes()).unwrap();
            assert_eq!(parsed.type_label(), label);
            if matches!(chart.kind, ChartKind::Pie | ChartKind::Doughnut) {
                let mut uncolored = expected.clone();
                uncolored.series.iter_mut().for_each(|s| s.color = None);
                assert_eq!(parsed, uncolored, "{label}");
            } else {
                assert_eq!(parsed, expected, "{label}");
            }
            let odf = to_odf(&chart, 300.0, 200.0);
            let parsed = from_odf(odf.as_bytes()).unwrap();
            assert_eq!(parsed.type_label(), label);
            assert_eq!(parsed.categories, chart.categories);
            assert_eq!(parsed.series.iter().map(|s| s.values.clone()).collect::<Vec<_>>(), chart.series.iter().map(|s| s.values.clone()).collect::<Vec<_>>());
            assert_eq!(parsed.title, chart.title);
            assert!(!layout(&chart, 360.0, 220.0).is_empty());
        }
    }

    #[test]
    fn workbook_is_a_readable_package() {
        let bytes = workbook(&Chart::sample()).unwrap();
        let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
        let mut sheet = String::new();
        std::io::Read::read_to_string(&mut zip.by_name("xl/worksheets/sheet1.xml").unwrap(), &mut sheet).unwrap();
        assert!(sheet.contains("<c r=\"B2\"><v>4.3</v></c>"));
        assert!(sheet.contains("Series 3"));
    }

    #[test]
    fn layout_places_columns_proportionally_and_stacks_to_full_height() {
        let mut chart = Chart::sample();
        chart.legend = false;
        chart.series.truncate(1);
        chart.series[0].values = vec![Some(1.0), Some(2.0), Some(4.0), Some(0.0)];
        let bars = layout(&chart, 400.0, 200.0)
            .into_iter()
            .filter_map(|m| match m {
                Mark::Rect { height, fill, .. } if fill == PALETTE[0] => Some(height),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(bars.len(), 4);
        assert!((bars[1] / bars[0] - 2.0).abs() < 1e-6 && (bars[2] / bars[0] - 4.0).abs() < 1e-6);
        chart.series.push(chart.series[0].clone());
        chart.set_type_label("100% stacked column").unwrap();
        let heights = layout(&chart, 400.0, 200.0)
            .into_iter()
            .filter_map(|m| match m {
                Mark::Rect { x, height, fill, .. } if fill != [255, 255, 255] && height > 1.0 => Some((x.round() as i64, height)),
                _ => None,
            })
            .collect::<Vec<_>>();
        let first = heights.iter().filter(|(x, _)| *x == heights[0].0).map(|v| v.1).sum::<f64>();
        let third = heights.iter().filter(|(x, _)| *x == heights[4].0).map(|v| v.1).sum::<f64>();
        assert!((first - third).abs() < 1e-6);
    }

    #[test]
    fn office_chart_with_scheme_colors_and_literal_cache_reads() {
        let xml = r#"<c:chartSpace xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><c:chart><c:title><c:tx><c:rich><a:p><a:r><a:t>Rev</a:t></a:r><a:r><a:t>enue</a:t></a:r></a:p></c:rich></c:tx></c:title><c:plotArea><c:bar3DChart><c:barDir val="bar"/><c:grouping val="stacked"/><c:ser><c:idx val="0"/><c:tx><c:v>Lit</c:v></c:tx><c:spPr><a:solidFill><a:schemeClr val="accent2"/></a:solidFill></c:spPr><c:cat><c:strLit><c:ptCount val="3"/><c:pt idx="0"><c:v>a</c:v></c:pt><c:pt idx="2"><c:v>c</c:v></c:pt></c:strLit></c:cat><c:val><c:numLit><c:ptCount val="3"/><c:pt idx="0"><c:v>1.5</c:v></c:pt><c:pt idx="1"><c:v>x</c:v></c:pt></c:numLit></c:val></c:ser></c:bar3DChart></c:plotArea></c:chart></c:chartSpace>"#;
        let chart = from_drawingml(xml.as_bytes()).unwrap();
        assert_eq!(chart.type_label(), "Stacked bar");
        assert_eq!(chart.title, "Revenue");
        assert_eq!(chart.categories, vec!["a", "2", "c"]);
        assert_eq!(chart.series[0].name, "Lit");
        assert_eq!(chart.series[0].values, vec![Some(1.5), None, None]);
        assert_eq!(chart.series[0].color, None);
        assert!(!chart.legend);
        assert!(from_drawingml(b"<c:chartSpace xmlns:c=\"x\"><c:chart><c:plotArea><c:radarChart/></c:plotArea></c:chart></c:chartSpace>").is_none());
    }

    fn entry(bytes: &[u8], path: &str) -> Vec<u8> {
        let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
        let mut out = Vec::new();
        std::io::Read::read_to_end(&mut archive.by_name(path).unwrap(), &mut out).unwrap();
        out
    }
    fn names(bytes: &[u8]) -> Vec<String> {
        zip::ZipArchive::new(Cursor::new(bytes)).unwrap().file_names().map(String::from).collect()
    }
    fn charts(deck: &crate::Presentation) -> Vec<Chart> {
        deck.slides().iter().flat_map(|s| &s.objects).filter_map(|o| o.chart.clone()).collect()
    }

    #[test]
    fn presentation_chart_insert_edit_undo_and_office_roundtrip() {
        let mut deck = crate::Presentation::new();
        let mut chart = Chart::sample();
        chart.title = "Plan".into();
        let id = deck.insert_chart(chart.clone()).unwrap();
        let mut edited = chart.clone();
        edited.set_type_label("Stacked bar").unwrap();
        edited.set_grid("\tNorth\tSouth\nJan\t1\t2\nFeb\t3\t4").unwrap();
        deck.set_chart(id, edited.clone()).unwrap();
        assert_eq!(charts(&deck), vec![edited.clone()]);
        assert!(deck.undo());
        assert_eq!(charts(&deck), vec![chart.clone()]);
        assert!(deck.redo());
        let (project, _) = deck.save_snapshot().unwrap();
        assert!(String::from_utf8_lossy(&project).contains("\"North\""));
        let colored = |c: &Chart| {
            let mut c = c.clone();
            for i in 0..c.series.len() {
                c.series[i].color = Some(c.color(i));
            }
            c
        };
        let pptx = crate::pptx::export_new(&deck).unwrap();
        let files = names(&pptx);
        assert!(files.iter().any(|n| n.starts_with("ppt/charts/chart1_") && n.ends_with(".xml")));
        assert!(files.iter().any(|n| n.starts_with("ppt/embeddings/") && n.ends_with(".xlsx")));
        let types = String::from_utf8(entry(&pptx, "[Content_Types].xml")).unwrap();
        assert!(types.contains("drawingml.chart+xml") && types.contains("Extension=\"xlsx\""));
        let (mut opened, source) = crate::pptx::import(&pptx).unwrap();
        assert_eq!(charts(&opened), vec![colored(&edited)]);
        // Overlay: an unchanged chart keeps its parts byte for byte.
        let same = crate::pptx::export(&opened, &source).unwrap();
        let part = files.iter().find(|n| n.starts_with("ppt/charts/chart1_") && n.ends_with(".xml")).unwrap().clone();
        let book = files.iter().find(|n| n.ends_with(".xlsx")).unwrap().clone();
        assert_eq!(entry(&same, &part), entry(&pptx, &part));
        // Overlay: an edited chart rewrites its part and workbook in place.
        let object = opened.slides()[0].objects.iter().find(|o| o.chart.is_some()).unwrap().id;
        let mut again = colored(&edited);
        again.set_grid("\tNorth\tSouth\nJan\t10\t20\nFeb\t30\t40\nMar\t5\t6").unwrap();
        again.set_type_label("Line").unwrap();
        opened.set_chart(object, again.clone()).unwrap();
        let mut moved = opened.slides()[0].objects.iter().find(|o| o.id == object).unwrap().clone();
        moved.x += 20.0;
        opened.update_object(moved).unwrap();
        let updated = crate::pptx::export(&opened, &source).unwrap();
        assert_eq!(names(&updated), files);
        let xml = String::from_utf8(entry(&updated, &part)).unwrap();
        assert!(xml.contains("<c:lineChart>") && xml.contains("r:id=\"workbook\""));
        let sheet = entry(&entry(&updated, &book), "xl/worksheets/sheet1.xml");
        assert!(String::from_utf8(sheet).unwrap().contains("<c r=\"A4\" t=\"inlineStr\"><is><t xml:space=\"preserve\">Mar</t>"));
        let (reopened, source) = crate::pptx::import(&updated).unwrap();
        assert_eq!(charts(&reopened), vec![again.clone()]);
        let o = reopened.slides()[0].objects.iter().find(|o| o.chart.is_some()).unwrap();
        assert!((o.x - moved_x(&opened, object)).abs() < 0.01);
        // A chart added to an imported presentation gets its own parts.
        let mut added = reopened;
        added.insert_chart(Chart::sample()).unwrap();
        let appended = crate::pptx::export(&added, &source).unwrap();
        let types = String::from_utf8(entry(&appended, "[Content_Types].xml")).unwrap();
        assert!(types.matches("drawingml.chart+xml").count() == 2);
        assert_eq!(charts(&crate::pptx::import(&appended).unwrap().0).len(), 2);
    }
    fn moved_x(deck: &crate::Presentation, id: u64) -> f64 {
        deck.slides()[0].objects.iter().find(|o| o.id == id).unwrap().x
    }

    #[test]
    fn odp_chart_objects_roundtrip_and_overlay_edits_the_object_only() {
        let mut deck = crate::Presentation::new();
        let mut chart = Chart::sample();
        chart.set_type_label("Doughnut").unwrap();
        deck.insert_chart(chart.clone()).unwrap();
        let odp = crate::odp::export(&deck).unwrap();
        let manifest = String::from_utf8(entry(&odp, "META-INF/manifest.xml")).unwrap();
        assert!(manifest.contains("manifest:full-path=\"Object 0_0/\" manifest:media-type=\"application/vnd.oasis.opendocument.chart\""));
        assert!(names(&odp).contains(&"Object 0_0/content.xml".to_owned()));
        let (mut opened, source) = crate::odp::import_with_source(&odp).unwrap();
        let mut expected = chart.clone();
        for i in 0..expected.series.len() {
            expected.series[i].color = Some(expected.color(i));
        }
        assert_eq!(charts(&opened), vec![expected.clone()]);
        let id = opened.slides()[0].objects[0].id;
        let mut edited = expected.clone();
        edited.title = "Mix".into();
        edited.set_type_label("Column").unwrap();
        opened.set_chart(id, edited.clone()).unwrap();
        opened.insert_chart(Chart::sample()).unwrap();
        let updated = crate::odp::export_overlay(&opened, &source).unwrap();
        assert_eq!(entry(&updated, "styles.xml"), entry(&odp, "styles.xml"));
        assert!(String::from_utf8(entry(&updated, "Object 0_0/content.xml")).unwrap().contains("Mix"));
        let reopened = crate::odp::import(&updated).unwrap();
        let found = charts(&reopened);
        assert_eq!(found.len(), 2);
        assert_eq!(found[0], edited);
        assert_eq!(found[1].type_label(), "Column");
    }
}
