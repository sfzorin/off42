use super::*;
use pdf42_core::decorations::{DecorationKind, HeaderFooter, NumberFormat, PageNumbers, Watermark};
#[derive(Default)]
pub(super) struct Settings {
    watermark: Watermark,
    header: HeaderFooter,
    header_font:String,
    font_cache:std::collections::BTreeMap<String,suite42_pdf_text::FontProgram>,
}
impl Settings {
    pub(super) fn batch_settings(&self) -> HeaderFooter {
        let mut spec = self.header.clone();
        if spec.date.is_empty() {
            spec.date = chrono::Local::now().format("%Y-%m-%d").to_string();
        }
        spec
    }
    pub(super) fn finish_batch(&mut self, baseline: &HeaderFooter, next: u64) {
        let mut current = self.header.clone();
        if current.date.is_empty() {
            current.date = baseline.date.clone();
        }
        current.file_name = baseline.file_name.clone();
        if current == *baseline {
            self.header.bates_start = next;
        }
    }
}
const HEADER_FONTS:[&str;6]=["Noto Sans","Arial","Arial Bold","Times New Roman","Times New Roman Bold","Courier New"];
const POSITIONS: [&str; 6] = [
    "Top left",
    "Top center",
    "Top right",
    "Bottom left",
    "Bottom center",
    "Bottom right",
];
const FORMATS: [&str; 3] = ["Number", "Page n of total", "- n -"];
fn number(value: &str) -> Result<f64, String> {
    value.trim().parse().map_err(|_| "Enter a number".into())
}
fn color(value: &str) -> Result<[u8; 3], String> {
    let value = value.trim().trim_start_matches('#');
    if value.len() != 6 || !value.is_ascii() {
        return Err("Use a six-digit RGB color, for example #D21923".into());
    }
    Ok([0, 2, 4]
        .map(|i| u8::from_str_radix(&value[i..i + 2], 16))
        .into_iter()
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| "Invalid RGB color")?
        .try_into()
        .unwrap())
}
fn hex(color: [u8; 3]) -> String {
    format!("#{:02X}{:02X}{:02X}", color[0], color[1], color[2])
}
impl Pdf {
    pub(super) fn decoration_request(&self, id: u16) -> Option<win42::InputRequest> {
        let doc = self.doc.as_ref()?;
        let pages = format!("1-{}", doc.page_count());
        let settings = &self.decorations;
        Some(match id {
            90 => win42::InputRequest {
                title: "Text Watermark",
                fields: vec![
                    ("Text", settings.watermark.text.clone()),
                    ("Font size (pt)", settings.watermark.size.to_string()),
                    (
                        "Opacity (0–100%)",
                        (settings.watermark.opacity * 100.0).to_string(),
                    ),
                    (
                        "Counter-clockwise rotation (degrees)",
                        settings.watermark.degrees.to_string(),
                    ),
                    ("Color (#RRGGBB)", hex(settings.watermark.color)),
                    ("Pages", pages),
                ],
            },
            91 => win42::InputRequest {
                title: "Page Numbers",
                fields: vec![
                    ("Position", "Bottom center".into()),
                    ("Format", "Number".into()),
                    ("Starting number", "1".into()),
                    ("Skip first selected page", "No".into()),
                    ("Font size (pt)", "10".into()),
                    ("Margin from edge (pt)", "28".into()),
                    ("Pages", pages),
                ],
            },
            92 => {
                let h = &settings.header;
                let mut fields = [
                    "Header left — <<page>>, <<pages>>, <<label>>, <<date>>, <<file>>, <<bates>>",
                    "Header center",
                    "Header right",
                    "Footer left",
                    "Footer center",
                    "Footer right",
                ]
                .into_iter()
                .zip(&h.texts)
                .map(|(k, v)| (k, v.clone()))
                .collect::<Vec<_>>();
                fields.extend([
                    ("Font size (pt)", h.size.to_string()),
                    ("Top/bottom margin (pt)", h.vertical.to_string()),
                    ("Side margin (pt)", h.horizontal.to_string()),
                    ("Pages", pages),
                    (
                        "Also number other PDF files (Bates numbers continue)",
                        "No".into(),
                    ),
                    ("Font",if settings.header_font.is_empty(){"Noto Sans".into()}else{settings.header_font.clone()}),
                    ("Color (#RRGGBB)",hex(h.color)),
                ]);
                win42::InputRequest {
                    title: "Header & Footer",
                    fields,
                }
            }
            93 => win42::InputRequest {
                title: "Bates Numbering Settings",
                fields: vec![
                    ("Prefix", settings.header.bates_prefix.clone()),
                    ("Suffix", settings.header.bates_suffix.clone()),
                    ("Digits (0–12)", settings.header.bates_digits.to_string()),
                    ("Starting number", settings.header.bates_start.to_string()),
                    (
                        "Date text",
                        if settings.header.date.is_empty() {
                            chrono::Local::now().format("%Y-%m-%d").to_string()
                        } else {
                            settings.header.date.clone()
                        },
                    ),
                ],
            },
            94 => win42::InputRequest {
                title: "Image Watermark",
                fields: vec![
                    (
                        "Opacity (0–100%)",
                        (settings.watermark.opacity * 100.0).to_string(),
                    ),
                    (
                        "Counter-clockwise rotation (degrees)",
                        settings.watermark.degrees.to_string(),
                    ),
                    ("Pages", pages),
                ],
            },
            _ => return None,
        })
    }
    pub(super) fn decoration_choices(&self, id: u16) -> Vec<(usize, Vec<String>)> {
        if id == 91 {
            vec![
                (0, POSITIONS.map(str::to_owned).to_vec()),
                (1, FORMATS.map(str::to_owned).to_vec()),
                (3, vec!["No".into(), "Yes".into()]),
            ]
        } else if id == 92 {
            vec![(10, vec!["No".into(), "Yes".into()]),(11,HEADER_FONTS.iter().map(|s|(*s).into()).collect())]
        } else {
            vec![]
        }
    }
    pub(super) fn decoration_action(
        &mut self,
        id: u16,
        values: &[String],
    ) -> Option<Result<bool, String>> {
        if !(90..=93).contains(&id) {
            return None;
        }
        Some((|| {
            let doc = self.doc.as_ref().ok_or("Open a PDF first")?;
            match id {
                90 => {
                    let [text, size, opacity, degrees, color_text, pages] = values else {
                        return Err("Invalid watermark settings".into());
                    };
                    let spec = Watermark {
                        text: text.clone(),
                        image: None,
                        size: number(size)?,
                        opacity: number(opacity)? / 100.0,
                        degrees: number(degrees)?,
                        color: color(color_text)?,
                    };
                    let pages = page_export::parse_pages(pages, doc.page_count())?;
                    self.doc
                        .as_mut()
                        .unwrap()
                        .add_watermark(&pages, spec.clone())?;
                    self.decorations.watermark = spec;
                }
                91 => {
                    let [position, format, start, skip, size, margin, pages] = values else {
                        return Err("Invalid page number settings".into());
                    };
                    let slot = POSITIONS
                        .iter()
                        .position(|p| *p == position)
                        .ok_or("Choose a position")?;
                    let format = match format.as_str() {
                        "Number" => NumberFormat::Plain,
                        "Page n of total" => NumberFormat::PageOfTotal,
                        "- n -" => NumberFormat::Dashes,
                        _ => return Err("Choose a numbering format".into()),
                    };
                    let skip_first = match skip.as_str() {
                        "No" => false,
                        "Yes" => true,
                        _ => return Err("Choose Yes or No".into()),
                    };
                    let spec = PageNumbers {
                        slot,
                        format,
                        start: start
                            .trim()
                            .parse()
                            .map_err(|_| "Invalid starting number")?,
                        skip_first,
                        size: number(size)?,
                        margin: number(margin)?,
                    };
                    let pages = page_export::parse_pages(pages, doc.page_count())?;
                    self.doc.as_mut().unwrap().add_page_numbers(&pages, &spec)?;
                }
                92 => {
                    if !matches!(values.len(), 10 | 11 | 12 | 13) {
                        return Err("Invalid header/footer settings".into());
                    }
                    let batch = match values.get(10).map(String::as_str).unwrap_or("No") {
                        "No" => false,
                        "Yes" => true,
                        _ => return Err("Choose Yes or No".into()),
                    };
                    if batch && self.batch_headers.is_some() {
                        return Err("A PDF header batch is already running".into());
                    }
                    let font_title=values.get(11).map(String::as_str).unwrap_or(if self.decorations.header_font.is_empty(){"Noto Sans"}else{&self.decorations.header_font});
                    if !HEADER_FONTS.contains(&font_title){return Err("Choose a header/footer font".into())}
                    let font_title=font_title.to_owned();
                    let font=if font_title=="Noto Sans"{None}else{
                        if !self.decorations.font_cache.contains_key(&font_title){
                            let(family,bold)=match font_title.as_str(){"Arial"=>("Arial",false),"Arial Bold"=>("Arial",true),"Times New Roman"=>("Times New Roman",false),"Times New Roman Bold"=>("Times New Roman",true),"Courier New"=>("Courier New",false),_=>unreachable!()};
                            let program=suite42_pdf_text::FontProgram::new(win42::system_font_program(family,bold)?)?;self.decorations.font_cache.insert(font_title.clone(),program);
                        }
                        self.decorations.font_cache.get(&font_title).cloned()
                    };
                    let mut spec = self.decorations.header.clone();spec.font=font;
                    if let Some(value)=values.get(12){spec.color=color(value)?;}
                    spec.texts = std::array::from_fn(|i| values[i].clone());
                    spec.size = number(&values[6])?;
                    spec.vertical = number(&values[7])?;
                    spec.horizontal = number(&values[8])?;
                    spec.file_name = self.source_file_name.clone();
                    if spec.date.is_empty() {
                        spec.date = chrono::Local::now().format("%Y-%m-%d").to_string();
                    }
                    let pages = page_export::parse_pages(&values[9], doc.page_count())?;
                    let next = self
                        .doc
                        .as_mut()
                        .unwrap()
                        .add_header_footer(&pages, &spec)?;
                    spec.bates_start = next;
                    self.decorations.header = spec;
                    self.decorations.header_font=font_title;
                    if batch {
                        self.pending_command = Some(112);
                    }
                }
                93 => {
                    let [prefix, suffix, digits, start, date] = values else {
                        return Err("Invalid Bates settings".into());
                    };
                    if [prefix, suffix, date]
                        .iter()
                        .any(|s| s.len() > 1024 || s.chars().any(char::is_control))
                    {
                        return Err("Use single-line Bates and date text".into());
                    }
                    let digits: usize = digits.trim().parse().map_err(|_| "Invalid digit count")?;
                    if digits > 12 {
                        return Err("Use up to 12 digits".into());
                    }
                    let start = start
                        .trim()
                        .parse()
                        .map_err(|_| "Invalid Bates starting number")?;
                    let h = &mut self.decorations.header;
                    h.bates_prefix = prefix.clone();
                    h.bates_suffix = suffix.clone();
                    h.bates_digits = digits;
                    h.bates_start = start;
                    h.date = date.clone();
                    return Ok(false);
                }
                _ => unreachable!(),
            }
            Ok(true)
        })())
    }
    pub(super) fn validate_image_watermark(
        &self,
        values: &[String],
    ) -> Result<(Watermark, Vec<usize>), String> {
        let [opacity, degrees, pages] = values else {
            return Err("Invalid image watermark settings".into());
        };
        let mut spec = self.decorations.watermark.clone();
        spec.opacity = number(opacity)? / 100.0;
        spec.degrees = number(degrees)?;
        if !spec.opacity.is_finite()
            || !(0.0..=1.0).contains(&spec.opacity)
            || !spec.degrees.is_finite()
        {
            return Err("Invalid watermark opacity or rotation".into());
        }
        let doc = self.doc.as_ref().ok_or("Open a PDF first")?;
        let pages = page_export::parse_pages(pages, doc.page_count())?;
        Ok((spec, pages))
    }
    pub(super) fn add_image_watermark(
        &mut self,
        path: &Path,
        values: &[String],
    ) -> Result<(), String> {
        use std::io::Read;
        let (mut spec, pages) = self.validate_image_watermark(values)?;
        let mut data = Vec::new();
        std::fs::File::open(path)
            .and_then(|f| f.take(16 * 1024 * 1024 + 1).read_to_end(&mut data))
            .map_err(|e| e.to_string())?;
        if data.len() > 16 * 1024 * 1024 {
            return Err("Watermark image exceeds 16 MB".into());
        }
        let format = image::guess_format(&data).map_err(|e| e.to_string())?;
        if !matches!(format, image::ImageFormat::Png | image::ImageFormat::Jpeg) {
            return Err("Choose a PNG or JPEG watermark image".into());
        }
        let mut reader = image::ImageReader::with_format(std::io::Cursor::new(data), format);
        let mut limits = image::Limits::default();
        limits.max_image_width = Some(4096);
        limits.max_image_height = Some(4096);
        limits.max_alloc = Some(32 * 1024 * 1024);
        reader.limits(limits);
        let image = reader.decode().map_err(|e| e.to_string())?.to_rgba8();
        if u64::from(image.width()) * u64::from(image.height()) > 4_000_000 {
            return Err("Watermark image exceeds four million pixels".into());
        }
        spec.image = Some(pdf42_core::ImageStamp {
            width: image.width(),
            height: image.height(),
            rgba: image.into_raw().into(),
        });
        self.doc.as_mut().unwrap().add_watermark(&pages, spec)?;
        self.branch();
        self.request_render()
    }
    pub(super) fn remove_live_decorations(&mut self, id: u16) -> Result<(), String> {
        let kinds = if id == 95 {
            vec![DecorationKind::Watermark, DecorationKind::PageNumber]
        } else {
            vec![DecorationKind::HeaderFooter]
        };
        self.doc
            .as_mut()
            .ok_or("Open a PDF first")?
            .remove_decorations(&kinds);
        self.branch();
        self.request_render()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use win42::App;
    #[test]
    fn bates_settings_dont_edit_document_and_save_name_tracks_successful_destination() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("original.pdf");
        std::fs::write(&source, include_bytes!("../fixtures/page-export.pdf")).unwrap();
        let mut app = Pdf::default();
        app.open(&source).unwrap();
        assert_eq!(app.input_request(92).unwrap().title, "Header & Footer");
        let original = app.doc.as_ref().unwrap().save_snapshot().unwrap().0;
        app.input_action(
            93,
            &[
                "PCB-".into(),
                "-X".into(),
                "6".into(),
                "7".into(),
                "2026-11-07".into(),
            ],
        )
        .unwrap();
        assert!(!app.dirty());
        assert!(!app.can_undo());
        app.input_action(
            90,
            &[
                "DRAFT".into(),
                "36".into(),
                "25".into(),
                "45".into(),
                "#D21923".into(),
                "3-1".into(),
            ],
        )
        .unwrap();
        assert!(app.dirty());
        let before = app.doc.as_ref().unwrap().save_snapshot().unwrap().0;
        assert!(app
            .input_action(
                90,
                &[
                    "DRAFT".into(),
                    "NaN".into(),
                    "25".into(),
                    "45".into(),
                    "#D21923".into(),
                    "1".into()
                ]
            )
            .is_err());
        assert_eq!(app.doc.as_ref().unwrap().save_snapshot().unwrap().0, before);
        let target = directory.path().join("renamed.pdf");
        let (revision, bytes) = app
            .save_snapshot_to("pdf", &target, false)
            .unwrap()
            .unwrap();
        assert_eq!(app.source_file_name, "original.pdf");
        app.saved_snapshot(revision + 1);
        assert_eq!(app.source_file_name, "original.pdf");
        std::fs::write(&target, bytes).unwrap();
        app.saved_snapshot(revision);
        assert_eq!(app.source_file_name, "renamed.pdf");
        assert!(!app.dirty());
        app.action(4).unwrap();
        assert_eq!(
            app.doc.as_ref().unwrap().save_snapshot().unwrap().0,
            original
        );
        app.action(5).unwrap();
        assert_eq!(app.doc.as_ref().unwrap().save_snapshot().unwrap().0, before);
        assert_eq!(app.decorations.header.bates_start, 7);
    }
    #[test]
    fn batch_continuation_is_explicit_after_atomic_current_document_edit() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("source.pdf");
        std::fs::write(&source, include_bytes!("../fixtures/page-export.pdf")).unwrap();
        let mut app = Pdf::default();
        app.open(&source).unwrap();
        let original = app.doc.as_ref().unwrap().save_snapshot().unwrap().0;
        let values = [
            "<<bates>>",
            "",
            "",
            "",
            "",
            "",
            "10",
            "28",
            "36",
            "1-3",
            "Yes",
        ]
        .map(str::to_owned);
        app.input_action(92, &values).unwrap();
        assert_eq!(app.decorations.header.bates_start, 4);
        assert_eq!(app.take_pending_command(), Some(112));
        assert_eq!(app.take_pending_command(), None);
        assert!(matches!(
            app.dialog_request(112),
            Some(win42::DialogRequest::BatchFolder { .. })
        ));
        app.action(4).unwrap();
        assert_eq!(
            app.doc.as_ref().unwrap().save_snapshot().unwrap().0,
            original
        );
        app.action(5).unwrap();
        assert!(app.dirty());
        let baseline = app.decorations.batch_settings();
        app.decorations.finish_batch(&baseline, 10);
        assert_eq!(app.decorations.header.bates_start, 10);
        app.decorations.finish_batch(&baseline, 20);
        assert_eq!(
            app.decorations.header.bates_start, 10,
            "changed settings must win"
        );
    }
    #[test]
    fn image_settings_are_validated_before_the_file_picker_and_change_nothing() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("source.pdf");
        std::fs::write(&source, include_bytes!("../fixtures/page-export.pdf")).unwrap();
        let mut app = Pdf::default();
        app.open(&source).unwrap();
        let original = app.doc.as_ref().unwrap().save_snapshot().unwrap().0;
        for values in [
            ["101", "0", "1"],
            ["50", "inf", "1"],
            ["50", "0", "1,1"],
            ["50", "0", "0"],
        ] {
            assert!(app
                .configured_file_action(94, &values.map(str::to_owned))
                .is_err());
        }
        let values = ["50", "45", "3-1"].map(str::to_owned);
        assert_eq!(
            app.configured_file_action(94, &values).unwrap().extension,
            "png"
        );
        assert_eq!(
            app.doc.as_ref().unwrap().save_snapshot().unwrap().0,
            original
        );
        assert!(!app.dirty());
    }
}

#[cfg(all(test,windows))]
mod native_font_tests{
    use super::*;
    use win42::App;
    #[test]
    fn header_choices_embed_each_actual_windows_font_and_preserve_history(){
        let directory=tempfile::tempdir().unwrap();let source=directory.path().join("source.pdf");std::fs::write(&source,include_bytes!("../fixtures/page-export.pdf")).unwrap();
        for choice in HEADER_FONTS.into_iter().skip(1){
            let mut app=Pdf::default();app.open(&source).unwrap();let baseline=app.doc.as_ref().unwrap().save_snapshot().unwrap().0;
            let values=["FONT-42 Привет","","","","","","14","28","36","1","No",choice,"#AA0011"].map(str::to_owned);
            app.input_action(92,&values).unwrap();let program=app.decorations.header.font.as_ref().unwrap();assert_ne!(program.name(),"NotoSans");assert_eq!(app.decorations.header.color,[170,0,17]);assert_eq!(app.input_request(92).unwrap().fields[11].1,choice);
            let changed=app.doc.as_ref().unwrap().save_snapshot().unwrap().0;let pdf=lopdf::Document::load_mem(&changed).unwrap();assert!(pdf.objects.values().any(|o|o.as_dict().is_ok_and(|d|d.get(b"BaseFont").is_ok_and(|v|v.as_name().is_ok_and(|n|n==program.name().as_bytes())))));
            app.action(4).unwrap();assert_eq!(app.doc.as_ref().unwrap().save_snapshot().unwrap().0,baseline);app.action(5).unwrap();assert_eq!(app.doc.as_ref().unwrap().save_snapshot().unwrap().0,changed);
        }
    }
}
