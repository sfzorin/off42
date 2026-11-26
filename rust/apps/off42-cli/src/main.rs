//! `off42`: one command line for Windows, Linux and macOS over the shared document cores.
//! Everything here reads bytes, calls a core crate and writes bytes; no window system is involved.
use doc42_core::rich::{Format as DocFormat, RichDocument};
use std::path::{Path, PathBuf};
use xls42_core::Workbook;

type Result<T> = std::result::Result<T, String>;

/// Print a line, ignoring a closed pipe (for example `off42 marks list | head`).
macro_rules! out {
    ($($arg:tt)*) => {{
        use std::io::Write;
        let _ = writeln!(std::io::stdout(), $($arg)*);
    }};
}

const USAGE: &str = "off42 — documents, spreadsheets, presentations, PDF, images, labels and marks

Usage:
  off42 convert IN OUT              convert within a family, or to .pdf (plain-text layout)
  off42 text IN                     print text (documents, sheets, slides, PDF)
  off42 info IN                     summary: kind, pages/sheets/slides, size
  off42 pdf merge OUT IN...         merge PDF files
  off42 pdf extract IN PAGES OUT    copy pages, e.g. \"1,3-5\" (reverse: \"5-3\")
  off42 barcode FORMAT VALUE OUT.png [--width MM]   FORMAT: qr, datamatrix, code128, aztec
  off42 labels OUT.pdf [--stock ID | --size WxH] [--gap X,Y] [--code FORMAT] [--copies N] [--first SLOT]
               (TEXT... | --from FILE)   one label per line; with --code, \"text<TAB>code\"
  off42 labels stocks               list built-in label sheets
  off42 marks list                  list product and conformity marks
  off42 marks render ID OUT.png [--width MM]
  off42 signature NAME OUT [--count N] [--form F] [--pen P] [--ink I] [--seed S]
  off42 stamp list
  off42 stamp TEMPLATE OUT [--ink I] [--set KEY=VALUE]... [--count N] [--seed S]
  off42 formats | --help | --version

Families: documents doc42 docx rtf odt html md txt; sheets xls42 xlsx xls ods csv (xls and xls42 read-only);
slides pptx odp json; images png jpg tif gif webp bmp; pdf. OUT for signature/stamp is a file
name prefix: OUT-1.png, OUT-2.png...
";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(()) => {}
        Err(error) => {
            eprintln!("off42: {error}");
            std::process::exit(1);
        }
    }
}

fn run(args: &[String]) -> Result<()> {
    let a: Vec<&str> = args.iter().map(String::as_str).collect();
    match a.as_slice() {
        [] | ["-h" | "--help" | "help"] => {
            out!("{}", USAGE.trim_end());
            Ok(())
        }
        ["--version" | "version"] => {
            out!("off42 {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        ["formats"] => {
            for (family, exts) in FAMILIES {
                out!("{family}\t{}", exts.join(" "));
            }
            Ok(())
        }
        ["convert", input, output] => convert(Path::new(input), Path::new(output)),
        ["text", input] => {
            out!("{}", text(&mut load(Path::new(input))?)?);
            Ok(())
        }
        ["info", input] => {
            out!("{}", info(&load(Path::new(input))?)?);
            Ok(())
        }
        ["pdf", "merge", output, inputs @ ..] if !inputs.is_empty() => {
            let paths: Vec<PathBuf> = inputs.iter().map(PathBuf::from).collect();
            let bytes = pdf42_core::merge::merge_files(&paths, &Default::default(), &Default::default())?;
            write(Path::new(output), &bytes)
        }
        ["pdf", "extract", input, pages, output] => {
            let doc = pdf42_core::PdfDocument::open_bytes(&read(Path::new(input))?)?;
            let indices = page_list(pages, doc.page_count())?;
            let bytes = doc.prepare_page_extraction()?.extract(&indices)?;
            write(Path::new(output), &bytes)
        }
        ["barcode", format, value, output, rest @ ..] => {
            let width = option(rest, "--width")?.map_or(Ok(40.0), |w| number(w))?;
            let png = label42::barcode_png(label42::Format::parse(format)?, value, width)?;
            write(Path::new(output), &png)
        }
        ["labels", "stocks"] => {
            for s in label42::Stock::catalog()? {
                out!("{}\t{}\t{}x{} mm\t{}x{} per sheet", s.id, s.name, s.width, s.height, s.columns, s.rows);
            }
            Ok(())
        }
        ["labels", output, rest @ ..] => labels(Path::new(output), rest),
        ["marks", "list"] => {
            for m in label42::symbols() {
                out!("{}\t{}\t{}", m.id, m.category, m.title);
            }
            Ok(())
        }
        ["marks", "render", id, output, rest @ ..] => {
            let width = option(rest, "--width")?.map_or(Ok(30.0), |w| number(w))?;
            let element = label42::TechnicalElement {
                symbol_id: Some(id.to_string()),
                code_type: label42::Format::Qr,
                value: String::new(),
                width_mm: width,
                caption: false,
            };
            write(Path::new(output), &element.png()?)
        }
        ["signature", name, output, rest @ ..] => signature(name, output, rest),
        ["stamp", "list"] => {
            for t in marks42::seal::templates() {
                let fields: Vec<&str> = t.fields().iter().map(|f| f.key).collect();
                out!("{}\t{}\t{}\tfields: {}", t.id, t.category.title(), t.title(), fields.join(", "));
            }
            Ok(())
        }
        ["stamp", template, output, rest @ ..] => stamp(template, output, rest),
        _ => Err(format!("unrecognised command. Run `off42 --help`.\n\n{USAGE}")),
    }
}

const FAMILIES: [(&str, &[&str]); 5] = [
    ("document", &["doc42", "docx", "rtf", "odt", "html", "htm", "md", "markdown", "txt"]),
    ("sheet", &["xls42", "xlsx", "xls", "ods", "csv"]),
    ("slides", &["pptx", "odp", "json"]),
    ("image", &["png", "jpg", "jpeg", "tif", "tiff", "gif", "webp", "bmp"]),
    ("pdf", &["pdf"]),
];

fn extension(path: &Path) -> String {
    path.extension().and_then(|e| e.to_str()).unwrap_or_default().to_ascii_lowercase()
}

fn read(path: &Path) -> Result<Vec<u8>> {
    std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))
}

fn write(path: &Path, bytes: &[u8]) -> Result<()> {
    std::fs::write(path, bytes).map_err(|e| format!("{}: {e}", path.display()))?;
    eprintln!("wrote {} ({} bytes)", path.display(), bytes.len());
    Ok(())
}

fn doc_format(ext: &str) -> Option<DocFormat> {
    Some(match ext {
        "doc42" => DocFormat::Project,
        "docx" => DocFormat::Docx,
        "rtf" => DocFormat::Rtf,
        "odt" => DocFormat::Odt,
        "html" | "htm" => DocFormat::Html,
        "md" | "markdown" => DocFormat::Markdown,
        "txt" => DocFormat::Text,
        _ => return None,
    })
}

/// A file opened by the core that owns its format.
enum Loaded {
    Document(RichDocument),
    Sheet(Workbook),
    Slides(ppt42_core::Presentation, Option<ppt42_core::pptx::Source>),
    Image(img42_core::ImageDocument),
    Pdf(Vec<u8>),
}

fn load(path: &Path) -> Result<Loaded> {
    let ext = extension(path);
    if let Some(format) = doc_format(&ext) {
        return RichDocument::decode(&read(path)?, format).map(Loaded::Document).map_err(|e| e.to_string());
    }
    let sheet = |r: std::result::Result<Workbook, xls42_core::Error>| r.map(Loaded::Sheet).map_err(|e| e.to_string());
    match ext.as_str() {
        "csv" => sheet(Workbook::from_csv_bytes(&read(path)?)),
        "xlsx" | "xls" | "ods" => sheet(Workbook::from_office_bytes(&read(path)?, &ext)),
        "xls42" => sheet(Workbook::open_project(path)),
        "pptx" => {
            let (deck, source) = ppt42_core::pptx::import(&read(path)?)?;
            Ok(Loaded::Slides(deck, Some(source)))
        }
        "odp" => Ok(Loaded::Slides(ppt42_core::odp::import(&read(path)?)?, None)),
        "json" => Ok(Loaded::Slides(ppt42_core::Presentation::open(path)?, None)),
        "png" | "jpg" | "jpeg" | "tif" | "tiff" | "gif" | "webp" | "bmp" => {
            img42_core::ImageDocument::open(path).map(Loaded::Image).map_err(|e| e.to_string())
        }
        "pdf" => {
            let bytes = read(path)?;
            pdf42_core::PdfDocument::open_bytes(&bytes)?;
            Ok(Loaded::Pdf(bytes))
        }
        _ => Err(format!("unsupported input type .{ext} (see `off42 formats`)")),
    }
}

fn sheet_text(book: &mut Workbook) -> Result<String> {
    let sheets = book.sheets();
    let mut out = String::new();
    for (index, (id, name)) in sheets.iter().enumerate() {
        book.select_sheet(*id).map_err(|e| e.to_string())?;
        if sheets.len() > 1 {
            if index > 0 {
                out.push('\n');
            }
            out.push_str(&format!("== {name}\n"));
        }
        let cells = book.display_cells();
        let rows = cells.keys().map(|(r, _)| r + 1).max().unwrap_or(0);
        let cols = cells.keys().map(|(_, c)| c + 1).max().unwrap_or(0);
        for r in 0..rows {
            let line: Vec<&str> = (0..cols).map(|c| cells.get(&(r, c)).map_or("", String::as_str)).collect();
            out.push_str(line.join("\t").trim_end());
            out.push('\n');
        }
    }
    Ok(out)
}

fn text(loaded: &mut Loaded) -> Result<String> {
    Ok(match loaded {
        Loaded::Document(doc) => doc.editor_text().map_err(|e| e.to_string())?,
        Loaded::Sheet(book) => sheet_text(book)?,
        Loaded::Slides(deck, _) => deck
            .slides()
            .iter()
            .enumerate()
            .map(|(i, s)| format!("--- Slide {}: {}\n{}\n", i + 1, s.title, s.body))
            .collect(),
        Loaded::Image(_) => return Err("images have no text (OCR is not part of the CLI)".into()),
        Loaded::Pdf(bytes) => {
            let doc = lopdf::Document::load_mem(bytes).map_err(|e| e.to_string())?;
            let pages: Vec<u32> = doc.get_pages().keys().copied().collect();
            pages
                .iter()
                .map(|p| doc.extract_text(&[*p]).unwrap_or_default())
                .collect::<Vec<_>>()
                .join("\n\u{c}\n")
        }
    })
}

fn info(loaded: &Loaded) -> Result<String> {
    Ok(match loaded {
        Loaded::Document(doc) => {
            let text = doc.editor_text().map_err(|e| e.to_string())?;
            format!("document\nparagraphs: {}\ncharacters: {}", text.lines().count(), text.chars().count())
        }
        Loaded::Sheet(book) => {
            let names: Vec<String> = book.sheets().into_iter().map(|(_, n)| n).collect();
            format!("sheet\nsheets: {} ({})", names.len(), names.join(", "))
        }
        Loaded::Slides(deck, _) => format!("slides\nslides: {}", deck.slides().len()),
        Loaded::Image(image) => format!("image\nsize: {}x{}", image.width(), image.height()),
        Loaded::Pdf(bytes) => {
            let doc = pdf42_core::PdfDocument::open_bytes(bytes)?;
            format!("pdf\npages: {}\nbytes: {}", doc.page_count(), bytes.len())
        }
    })
}

fn convert(input: &Path, output: &Path) -> Result<()> {
    let mut loaded = load(input)?;
    let ext = extension(output);
    let title = input.file_stem().and_then(|s| s.to_str()).unwrap_or("off42");
    if ext == "pdf" && !matches!(loaded, Loaded::Pdf(_) | Loaded::Image(_)) {
        return write(output, &suite42_pdf_text::plain_text_pdf(title, &text(&mut loaded)?, 10.5)?);
    }
    let bytes = match (&loaded, ext.as_str()) {
        (Loaded::Pdf(bytes), "pdf") => bytes.clone(),
        (Loaded::Image(_), "pdf") => return Err("image to PDF is not available in the CLI yet".into()),
        (Loaded::Document(doc), _) => {
            let format = doc_format(&ext).ok_or_else(|| format!("documents cannot be saved as .{ext}"))?;
            let mut state = doc.state().clone();
            // Unknown parts of the source package (themes, custom XML...) only survive in their own format.
            let keep_docx = matches!(format, DocFormat::Project | DocFormat::Docx);
            let keep_odt = matches!(format, DocFormat::Project | DocFormat::Odt);
            if (!keep_docx && !state.docx_parts.is_empty()) || (!keep_odt && !state.odt_parts.is_empty()) {
                eprintln!("note: package parts that .{ext} cannot hold were left out");
                if !keep_docx {
                    state.docx_parts.clear();
                }
                if !keep_odt {
                    state.odt_parts.clear();
                }
            }
            RichDocument::encode_snapshot(state, format).map_err(|e| e.to_string())?
        }
        (Loaded::Sheet(book), _) => match ext.as_str() {
            "xlsx" => book.xlsx_bytes(),
            "ods" => book.ods_bytes(),
            "csv" => book.csv_bytes(),
            "xls42" => book.project_bytes(),
            _ => return Err(format!("sheets cannot be saved as .{ext}")),
        }
        .map_err(|e| e.to_string())?,
        (Loaded::Slides(deck, source), _) => match (ext.as_str(), source) {
            ("pptx", Some(source)) => ppt42_core::pptx::export(deck, source)?,
            ("pptx", None) => ppt42_core::pptx::export_new(deck)?,
            ("odp", _) => ppt42_core::odp::export(deck)?,
            ("json", _) => deck.save_snapshot()?.0,
            _ => return Err(format!("slides cannot be saved as .{ext}")),
        },
        (Loaded::Image(image), _) => {
            let format = match ext.as_str() {
                "jpg" | "jpeg" => "jpeg",
                "tif" | "tiff" => "tiff",
                other => other,
            };
            image.save_snapshot_format(format).map_err(|e| e.to_string())?.0
        }
        (Loaded::Pdf(_), _) => return Err(format!("PDF cannot be converted to .{ext}; try `off42 text`")),
    };
    write(output, &bytes)
}

/// One-based list such as "1, 3-5" or "5-3" into zero-based page indices.
fn page_list(spec: &str, count: usize) -> Result<Vec<usize>> {
    let mut out = Vec::new();
    for part in spec.split(',').map(str::trim).filter(|p| !p.is_empty()) {
        let (a, b) = part.split_once('-').unwrap_or((part, part));
        let parse = |s: &str| -> Result<usize> {
            let n: usize = s.trim().parse().map_err(|_| format!("bad page number {s:?}"))?;
            if n == 0 || n > count {
                return Err(format!("page {n} is outside 1-{count}"));
            }
            Ok(n - 1)
        };
        let (a, b) = (parse(a)?, parse(b)?);
        if a <= b {
            out.extend(a..=b);
        } else {
            out.extend((b..=a).rev());
        }
    }
    if out.is_empty() {
        return Err("choose at least one page".into());
    }
    Ok(out)
}

fn option<'a>(rest: &[&'a str], name: &str) -> Result<Option<&'a str>> {
    match rest.iter().position(|a| *a == name) {
        Some(i) => rest.get(i + 1).copied().map(Some).ok_or_else(|| format!("{name} needs a value")),
        None => Ok(None),
    }
}

fn options<'a>(rest: &[&'a str], name: &str) -> Vec<&'a str> {
    rest.windows(2).filter(|w| w[0] == name).map(|w| w[1]).collect()
}

/// Positional words that are neither options nor option values.
fn positional<'a>(rest: &[&'a str]) -> Vec<&'a str> {
    let mut out = Vec::new();
    let mut skip = false;
    for a in rest {
        if skip {
            skip = false;
        } else if a.starts_with("--") {
            skip = true;
        } else {
            out.push(*a);
        }
    }
    out
}

fn number(s: &str) -> Result<f64> {
    s.trim().parse().map_err(|_| format!("not a number: {s}"))
}

fn labels(output: &Path, rest: &[&str]) -> Result<()> {
    let code = option(rest, "--code")?.map(label42::Format::parse).transpose()?;
    let copies = option(rest, "--copies")?.map_or(Ok(1), |c| c.parse().map_err(|_| "bad --copies".to_string()))?;
    let first = option(rest, "--first")?.map_or(Ok(1), |c| c.parse().map_err(|_| "bad --first".to_string()))?;
    let stock = if let Some(id) = option(rest, "--stock")? {
        label42::Stock::catalog()?
            .iter()
            .find(|s| s.id == id)
            .cloned()
            .ok_or_else(|| format!("unknown stock {id}; see `off42 labels stocks`"))?
    } else {
        let size = option(rest, "--size")?.unwrap_or("70x37");
        let (w, h) = size.split_once(['x', 'X']).ok_or("--size is WIDTHxHEIGHT in mm")?;
        let gap = option(rest, "--gap")?.unwrap_or("0,0");
        let (gx, gy) = gap.split_once(',').ok_or("--gap is X,Y in mm")?;
        label42::Stock::custom(number(w)?, number(h)?, number(gx)?, number(gy)?)?
    };
    let lines: Vec<String> = match option(rest, "--from")? {
        Some(file) => String::from_utf8(read(Path::new(file))?)
            .map_err(|_| "label file must be UTF-8".to_string())?
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(str::to_owned)
            .collect(),
        None => positional(rest).into_iter().map(str::to_owned).collect(),
    };
    if lines.is_empty() {
        return Err("give label texts or --from FILE".into());
    }
    let labels: Vec<label42::Label> = lines
        .into_iter()
        .map(|line| {
            let (text, code_value) = match (code, line.split_once('\t')) {
                (Some(_), Some((t, c))) => (t.to_owned(), Some(c.to_owned())),
                (Some(_), None) => (line.clone(), Some(line.clone())),
                (None, _) => (line.replace('\t', " "), None),
            };
            label42::Label { text, code: code_value, copies }
        })
        .collect();
    write(output, &label42::sheet_pdf_stock(&stock, first, code, &labels)?)
}

fn pick<T: Copy>(all: &[T], title: impl Fn(T) -> &'static str, value: Option<&str>, what: &str) -> Result<T> {
    let Some(value) = value else { return Ok(all[0]) };
    let wanted = value.trim().to_lowercase();
    all.iter().copied().find(|item| title(*item).to_lowercase().starts_with(&wanted)).ok_or_else(|| {
        let names: Vec<&str> = all.iter().map(|i| title(*i)).collect();
        format!("unknown {what} {value:?}; choose one of: {}", names.join(", "))
    })
}

fn rng(rest: &[&str]) -> Result<marks42::SeededRandom> {
    let seed = match option(rest, "--seed")? {
        Some(s) => s.parse().map_err(|_| "bad --seed".to_string())?,
        None => marks42::SeededRandom::fresh_seed(),
    };
    Ok(marks42::SeededRandom::new(seed))
}

fn count(rest: &[&str]) -> Result<usize> {
    let n: usize = option(rest, "--count")?.map_or(Ok(3), |c| c.parse().map_err(|_| "bad --count".to_string()))?;
    if !(1..=20).contains(&n) {
        return Err("--count is 1 to 20".into());
    }
    Ok(n)
}

fn write_variants(output: &str, images: &[image::RgbaImage]) -> Result<()> {
    for (i, image) in images.iter().enumerate() {
        write(Path::new(&format!("{output}-{}.png", i + 1)), &marks42::png(image)?)?;
    }
    Ok(())
}

fn signature(name: &str, output: &str, rest: &[&str]) -> Result<()> {
    use marks42::signature::{Pen, SignatureForm};
    let name = name.split_whitespace().collect::<Vec<_>>().join(" ");
    if name.is_empty() || name.chars().count() > 80 {
        return Err("the name must have 1 to 80 characters".into());
    }
    let form = pick(&SignatureForm::ALL, |f| f.title(), option(rest, "--form")?, "form")?;
    let pen = pick(&Pen::ALL, |p| p.title(), option(rest, "--pen")?, "pen")?;
    let ink = pick(&marks42::InkColor::ALL, |i| i.title(), option(rest, "--ink")?, "ink")?;
    let mut rng = rng(rest)?;
    let images = marks42::signature::design_seeds_with(count(rest)?, form, &mut rng)
        .into_iter()
        .map(|design| {
            marks42::signature::render(&name, design, rng.next_u64(), ink.pen(), pen, 260.0)
                .ok_or_else(|| "this name could not be written as a signature".to_string())
        })
        .collect::<Result<Vec<_>>>()?;
    write_variants(output, &images)
}

fn stamp(template: &str, output: &str, rest: &[&str]) -> Result<()> {
    let template = marks42::seal::templates()
        .iter()
        .find(|t| t.id == template)
        .ok_or_else(|| format!("unknown stamp {template}; see `off42 stamp list`"))?;
    let mut spec = marks42::seal::SealSpec::example(template);
    if let Some(ink) = option(rest, "--ink")? {
        spec.ink = pick(&marks42::InkColor::ALL, |i| i.title(), Some(ink), "ink")?.stamp_pad();
    }
    for pair in options(rest, "--set") {
        let (key, value) = pair.split_once('=').ok_or("--set is KEY=VALUE")?;
        if !template.fields().iter().any(|f| f.key == key) {
            return Err(format!("stamp {} has no field {key}", template.id));
        }
        spec.values.insert(key.to_owned(), value.replace("\\n", "\n"));
    }
    let mut rng = rng(rest)?;
    let images = (0..count(rest)?)
        .map(|_| marks42::seal::render(&spec, rng.next_u64(), 2.0).ok_or_else(|| "this stamp could not be drawn".to_string()))
        .collect::<Result<Vec<_>>>()?;
    write_variants(output, &images)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run_ok(args: &[&str]) {
        run(&args.iter().map(|s| s.to_string()).collect::<Vec<_>>()).unwrap_or_else(|e| panic!("{args:?}: {e}"));
    }

    #[test]
    fn converts_documents_sheets_and_pdf() {
        let dir = tempfile::tempdir().unwrap();
        let p = |n: &str| dir.path().join(n).to_string_lossy().into_owned();
        std::fs::write(p("a.md"), "# Title\n\nHello *off42*.\n").unwrap();
        run_ok(&["convert", &p("a.md"), &p("a.docx")]);
        run_ok(&["convert", &p("a.docx"), &p("a.odt")]);
        run_ok(&["convert", &p("a.odt"), &p("a.pdf")]);
        assert!(text(&mut load(Path::new(&p("a.pdf"))).unwrap()).unwrap().contains("Hello off42"));
        std::fs::write(p("s.csv"), "name,value\nx,=6*7\n").unwrap();
        run_ok(&["convert", &p("s.csv"), &p("s.xlsx")]);
        run_ok(&["convert", &p("s.xlsx"), &p("s.ods")]);
        assert!(text(&mut load(Path::new(&p("s.ods"))).unwrap()).unwrap().contains("42"));
        run_ok(&["pdf", "merge", &p("m.pdf"), &p("a.pdf"), &p("a.pdf")]);
        run_ok(&["pdf", "extract", &p("m.pdf"), "2", &p("e.pdf")]);
        assert!(info(&load(Path::new(&p("e.pdf"))).unwrap()).unwrap().contains("pages: 1"));
    }

    #[test]
    fn makes_codes_labels_marks_signatures_and_stamps() {
        let dir = tempfile::tempdir().unwrap();
        let p = |n: &str| dir.path().join(n).to_string_lossy().into_owned();
        run_ok(&["barcode", "qr", "https://example.com", &p("q.png")]);
        run_ok(&["labels", &p("l.pdf"), "--size", "70x50", "--code", "qr", "A-1", "A-2"]);
        let mark = label42::symbols()[0].id;
        run_ok(&["marks", "render", mark, &p("m.png")]);
        run_ok(&["signature", "Ann Lee", &p("sig"), "--count", "2", "--seed", "1"]);
        let stamp = marks42::seal::templates()[0].id;
        run_ok(&["stamp", stamp, &p("st"), "--count", "1", "--seed", "2"]);
        for f in ["q.png", "l.pdf", "m.png", "sig-1.png", "sig-2.png", "st-1.png"] {
            assert!(std::fs::metadata(p(f)).unwrap().len() > 100, "{f}");
        }
    }

    #[test]
    fn converts_slides() {
        let dir = tempfile::tempdir().unwrap();
        let p = |n: &str| dir.path().join(n).to_string_lossy().into_owned();
        std::fs::write(p("d.json"), ppt42_core::Presentation::new().save_snapshot().unwrap().0).unwrap();
        run_ok(&["convert", &p("d.json"), &p("d.pptx")]);
        run_ok(&["convert", &p("d.pptx"), &p("d.odp")]);
        run_ok(&["convert", &p("d.odp"), &p("d.pdf")]);
        assert!(info(&load(Path::new(&p("d.pptx"))).unwrap()).unwrap().contains("slides: 1"));
    }

    #[test]
    fn parses_page_lists() {
        assert_eq!(page_list("1, 3-4", 5).unwrap(), vec![0, 2, 3]);
        assert_eq!(page_list("3-1", 3).unwrap(), vec![2, 1, 0]);
        assert!(page_list("6", 5).is_err());
    }
}
