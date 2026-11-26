//! Create Signature… / Create Stamp…: generated variants are written to a scratch folder for choosing in the native
//! picker (with Explorer thumbnails), and the chosen one is kept in the shared signature and stamp library, from which
//! every app inserts it. The drawing itself is `marks42`, the port of the macOS generators.
//!
//! Learn Signature from Samples…: one to five pictures of the user's own signature are traced (as on macOS), and the
//! variants are new copies in that hand — half made from the samples' ink, half redrawn by the pen model from the
//! traced strokes, blending compatible samples.

use marks42::{seal, signature, InkColor, SeededRandom};
use std::{
    fs,
    path::{Path, PathBuf},
};

/// Variants offered per generation (the macOS window shows three; the picker shows a grid, so offer six).
pub const VARIANTS: usize = 6;
/// Library pictures are drawn at twice the macOS on-screen height for sharp printing.
pub const SIGNATURE_PIXEL_HEIGHT: f64 = 260.0;

pub fn form_titles() -> Vec<String> {
    signature::SignatureForm::ALL.iter().map(|f| f.title().to_owned()).collect()
}
pub fn pen_titles() -> Vec<String> {
    signature::Pen::ALL.iter().map(|p| p.title().to_owned()).collect()
}
pub fn ink_titles() -> Vec<String> {
    InkColor::ALL.iter().map(|i| i.title().to_owned()).collect()
}

fn pick<T: Copy>(all: &[T], title: impl Fn(T) -> &'static str, value: &str, what: &str) -> Result<T, String> {
    all.iter().copied().find(|item| title(*item) == value.trim()).ok_or_else(|| format!("Choose a {what}."))
}

/// The signature settings from the Create Signature fields: name, form, pen, ink.
pub struct SignatureRequest {
    pub name: String,
    pub form: signature::SignatureForm,
    pub pen: signature::Pen,
    pub ink: InkColor,
}

impl SignatureRequest {
    pub fn parse(fields: &[String]) -> Result<Self, String> {
        let [name, form, pen, ink] = fields else {
            return Err("Invalid signature settings.".into());
        };
        let name = name.split_whitespace().collect::<Vec<_>>().join(" ");
        if name.is_empty() || name.chars().count() > 80 {
            return Err("Enter a name of up to 80 characters.".into());
        }
        Ok(Self {
            name,
            form: pick(&signature::SignatureForm::ALL, |f| f.title(), form, "signature form")?,
            pen: pick(&signature::Pen::ALL, |p| p.title(), pen, "pen")?,
            ink: pick(&InkColor::ALL, |i| i.title(), ink, "ink colour")?,
        })
    }

    /// New styles for the name and form, each drawn as a fresh impression (as New Variants on macOS).
    pub fn variants(&self, count: usize, rng: &mut SeededRandom) -> Result<Vec<image::RgbaImage>, String> {
        signature::design_seeds_with(count, self.form, rng)
            .into_iter()
            .map(|design| {
                signature::render(&self.name, design, rng.next_u64(), self.ink.pen(), self.pen, SIGNATURE_PIXEL_HEIGHT)
                    .ok_or_else(|| "This name could not be written as a signature.".to_owned())
            })
            .collect()
    }
}

/// Most pictures for Learn Signature from Samples… (the macOS hint: three to five samples teach the hand).
pub const MAXIMUM_LEARN_SAMPLES: usize = 5;

/// One signature per picture: each is traced into a sample, keeping the picture for copies from its ink.
pub fn learn_samples(pictures: &[(String, image::RgbaImage)]) -> Result<Vec<signature::SignatureSample>, String> {
    use crate::localization::text;
    if pictures.is_empty() || pictures.len() > MAXIMUM_LEARN_SAMPLES {
        return Err(text("Choose one to five pictures of your signature."));
    }
    pictures
        .iter()
        .map(|(name, image)| {
            signature::SignatureSample::from_picture(image).ok_or_else(|| {
                format!("{}: {name}\n{}", text("No signature found in the picture"), text("Use dark ink on light paper, one signature per picture."))
            })
        })
        .collect()
}

/// Pen and ink for the learned copies.
pub struct LearnRequest {
    pub pen: signature::Pen,
    pub ink: InkColor,
}

impl LearnRequest {
    pub fn parse(fields: &[String]) -> Result<Self, String> {
        let [pen, ink] = fields else {
            return Err("Invalid signature settings.".into());
        };
        Ok(Self { pen: pick(&signature::Pen::ALL, |p| p.title(), pen, "pen")?, ink: pick(&InkColor::ALL, |i| i.title(), ink, "ink colour")? })
    }

    /// New copies of the learned signature: alternately from the samples' ink (what macOS shows for pictures) and
    /// redrawn with the chosen pen from the traced strokes.
    pub fn variants(&self, samples: &[signature::SignatureSample], count: usize, rng: &mut SeededRandom) -> Result<Vec<image::RgbaImage>, String> {
        let from_ink = signature::LearnedSignature::new(samples.to_vec());
        let redrawn = signature::LearnedSignature::new(samples.iter().map(|s| s.traced_only()).collect());
        (0..count)
            .map(|i| {
                let learned = if i % 2 == 0 { &from_ink } else { &redrawn };
                learned
                    .render(rng.next_u64(), self.ink.pen(), self.pen, SIGNATURE_PIXEL_HEIGHT)
                    .ok_or_else(|| "This signature could not be copied from the samples.".to_owned())
            })
            .collect()
    }
}

/// A file-name-safe stem: no path separators, reserved or control characters, at most 60 characters.
pub fn safe_stem(text: &str) -> String {
    let cleaned: String = text
        .chars()
        .map(|c| if c.is_control() || r#"<>:"/\|?*"#.contains(c) { ' ' } else { c })
        .collect();
    let stem = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    let stem: String = stem.chars().take(60).collect();
    let stem = stem.trim_end_matches(['.', ' ']).to_owned();
    if stem.is_empty() { "Mark".into() } else { stem }
}

/// Replaces the previous variants in `folder` with the new ones, named `<stem> 1.png`, `<stem> 2.png`…
pub fn write_variants(folder: &Path, stem: &str, images: &[image::RgbaImage]) -> Result<Vec<PathBuf>, String> {
    fs::create_dir_all(folder).map_err(|e| format!("Could not prepare the variants: {e}"))?;
    for entry in fs::read_dir(folder).map_err(|e| e.to_string())?.flatten() {
        if entry.path().extension().is_some_and(|x| x.eq_ignore_ascii_case("png")) {
            let _ = fs::remove_file(entry.path());
        }
    }
    images
        .iter()
        .enumerate()
        .map(|(i, image)| {
            let path = folder.join(format!("{} {}.png", safe_stem(stem), i + 1));
            fs::write(&path, marks42::png(image)?).map_err(|e| format!("Could not save a variant: {e}"))?;
            Ok(path)
        })
        .collect()
}

/// Copies the chosen variant into the shared library under a new name (never replacing an existing picture).
pub fn keep_in_library(chosen: &Path, library: &Path, stem: &str) -> Result<PathBuf, String> {
    let bytes = fs::read(chosen).map_err(|e| format!("Could not read the chosen picture: {e}"))?;
    if !bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Err("Choose one of the generated pictures.".into());
    }
    fs::create_dir_all(library).map_err(|e| format!("Could not open the library: {e}"))?;
    let stem = safe_stem(stem);
    for n in 1..10_000 {
        let name = if n == 1 { format!("{stem}.png") } else { format!("{stem} ({n}).png") };
        let target = library.join(name);
        match fs::OpenOptions::new().write(true).create_new(true).open(&target) {
            Ok(mut file) => {
                use std::io::Write;
                file.write_all(&bytes).map_err(|e| format!("Could not save to the library: {e}"))?;
                return Ok(target);
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(format!("Could not save to the library: {e}")),
        }
    }
    Err("The library already has too many pictures with this name.".into())
}

/// Stamp styles as offered in the chooser: "<category> — <title>" in the interface language (titles exist in
/// English, Russian and Turkish; categories follow the shared catalogue).
pub fn stamp_labels(language: &str) -> Vec<String> {
    seal::templates().iter().map(|t| stamp_label(t, language)).collect()
}

fn stamp_label(t: &seal::SealTemplate, language: &str) -> String {
    let title = match language {
        "ru" => t.titles[1],
        "tr" => t.titles[2],
        _ => t.titles[0],
    };
    format!("{} — {title}", crate::localization::text(t.category.title()))
}

/// The template chosen in the style field.
pub fn stamp_template(label: &str, language: &str) -> Result<&'static seal::SealTemplate, String> {
    seal::templates()
        .iter()
        .find(|t| stamp_label(t, language) == label.trim() || t.id == label.trim())
        .ok_or_else(|| "Choose a stamp style.".into())
}

/// "Stamp's own ink" keeps the template's colour; any other entry is an ink title.
pub const OWN_INK: &str = "Default";

pub fn stamp_ink_titles() -> Vec<String> {
    std::iter::once(OWN_INK.to_owned()).chain(ink_titles()).collect()
}

/// A stamp spec from the chosen template, ink title and field values (in the template's field order).
pub fn stamp_spec(template: &seal::SealTemplate, ink: &str, values: &[String]) -> Result<seal::SealSpec, String> {
    if values.len() != template.fields().len() {
        return Err("Invalid stamp texts.".into());
    }
    let mut spec = seal::SealSpec::example(template);
    if ink.trim() != OWN_INK {
        spec.ink = pick(&InkColor::ALL, |i| i.title(), ink, "ink colour")?.stamp_pad();
    }
    for (field, value) in template.fields().iter().zip(values) {
        if value.chars().count() > 400 {
            return Err("Stamp texts are limited to 400 characters per field.".into());
        }
        spec.values.insert(field.key.to_owned(), value.trim().to_owned());
    }
    Ok(spec)
}

/// Fresh impressions of the stamp (each pressing differs, as on macOS), drawn at twice the base size.
pub fn stamp_impressions(spec: &seal::SealSpec, count: usize, rng: &mut SeededRandom) -> Result<Vec<image::RgbaImage>, String> {
    (0..count)
        .map(|_| seal::render(spec, rng.next_u64(), 2.0).ok_or_else(|| "This stamp could not be drawn.".to_owned()))
        .collect()
}

/// Create Stamp…: style and ink, then the style's texts, then impressions to choose from. `None` when cancelled.
#[cfg(windows)]
pub unsafe fn stamp_variants(owner: windows_sys::Win32::Foundation::HWND) -> Result<Option<(String, Vec<image::RgbaImage>)>, String> {
    let language = crate::localization::current().code;
    let labels = stamp_labels(language);
    let request = crate::InputRequest {
        title: "Create Stamp",
        fields: vec![("Stamp style:", labels[0].clone()), ("Ink:", OWN_INK.into())],
    };
    let Some(choice) = crate::prompt::show_choices(owner, &request, false, false, &[(0, labels), (1, stamp_ink_titles())]) else {
        return Ok(None);
    };
    let template = stamp_template(&choice[0], language)?;
    let fields = template.fields();
    let request = crate::InputRequest {
        title: "Create Stamp",
        fields: fields.iter().map(|f| (f.label, f.example.to_owned())).collect(),
    };
    let multiline: Vec<usize> = fields.iter().enumerate().filter(|(_, f)| f.multiline).map(|(i, _)| i).collect();
    let values = if fields.is_empty() {
        Vec::new()
    } else {
        let Some(values) = crate::prompt::show_choices_for_fields(owner, &request, &multiline, false, &[]) else {
            return Ok(None);
        };
        values
    };
    let spec = stamp_spec(template, &choice[1], &values)?;
    let images = stamp_impressions(&spec, VARIANTS, &mut SeededRandom::new(SeededRandom::fresh_seed()))?;
    let name = values.iter().map(|v| v.trim()).find(|v| !v.is_empty() && v.chars().count() <= 40).unwrap_or(template.titles[0]);
    Ok(Some((name.to_owned(), images)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("off42-generator-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn parses_settings_and_rejects_invalid_ones() {
        let ok = |f: [&str; 4]| SignatureRequest::parse(&f.map(String::from));
        let request = ok(["  Иван   Петров ", "Full name", "Gel pen", "Violet ink"]).unwrap();
        assert_eq!(request.name, "Иван Петров");
        assert_eq!((request.pen, request.ink), (signature::Pen::Gel, InkColor::Violet));
        assert!(ok(["", "Any form", "Ballpoint pen", "Blue ink"]).is_err());
        assert!(ok(["Ann", "Sideways", "Ballpoint pen", "Blue ink"]).is_err());
        assert!(ok(["Ann", "Any form", "Quill", "Blue ink"]).is_err());
        assert_eq!(form_titles().len(), signature::SignatureForm::ALL.len());
        assert!(ink_titles().contains(&"Blue ink".to_owned()) && pen_titles().contains(&"Ballpoint pen".to_owned()));
    }

    #[test]
    fn generates_distinct_variants_and_keeps_the_chosen_one_without_overwriting() {
        let request = SignatureRequest::parse(&["Sergey Zorin", "Any form", "Ballpoint pen", "Blue ink"].map(String::from)).unwrap();
        let images = request.variants(3, &mut SeededRandom::new(7)).unwrap();
        assert_eq!(images.len(), 3);
        assert!(images.iter().all(|i| i.height() > 60 && i.pixels().any(|p| p.0[3] > 200)));
        assert_ne!(images[0].as_raw(), images[1].as_raw());

        let (folder, library) = (temp("variants"), temp("library"));
        fs::create_dir_all(&folder).unwrap();
        fs::write(folder.join("old.png"), b"stale").unwrap();
        let paths = write_variants(&folder, "Sergey Zorin", &images).unwrap();
        assert_eq!(paths.len(), 3);
        assert!(!folder.join("old.png").exists() && paths[0].ends_with("Sergey Zorin 1.png"));
        let first = keep_in_library(&paths[1], &library, "Signature - Sergey Zorin").unwrap();
        let second = keep_in_library(&paths[2], &library, "Signature - Sergey Zorin").unwrap();
        assert!(first.ends_with("Signature - Sergey Zorin.png") && second.ends_with("Signature - Sergey Zorin (2).png"));
        assert_eq!(fs::read(&first).unwrap(), fs::read(&paths[1]).unwrap());
        assert!(keep_in_library(&folder.join("missing.png"), &library, "x").is_err());
        let _ = (fs::remove_dir_all(folder), fs::remove_dir_all(library));
    }

    #[test]
    fn stamp_styles_resolve_and_draw_fresh_impressions_with_edited_texts() {
        let labels = stamp_labels("en");
        assert_eq!(labels.len(), seal::templates().len());
        let template = stamp_template(&labels[0], "en").unwrap();
        assert_eq!(template.id, seal::templates()[0].id);
        assert!(stamp_template(&stamp_labels("ru")[0], "ru").is_ok());
        assert!(stamp_template("No such stamp", "en").is_err());
        let values: Vec<String> = template.fields().iter().map(|f| f.example.to_owned()).collect();
        let own = stamp_spec(template, OWN_INK, &values).unwrap();
        assert_eq!(own.ink, template.ink.stamp_pad());
        let mut edited = values.clone();
        edited[0] = "ООО «Северный ветер»".into();
        let red = stamp_spec(template, "Red ink", &edited).unwrap();
        assert_eq!(red.ink, InkColor::Red.stamp_pad());
        assert!(stamp_spec(template, "Blue ink", &values[1..]).is_err());
        let images = stamp_impressions(&red, 2, &mut SeededRandom::new(3)).unwrap();
        assert!(images.iter().all(|i| i.width() > 100 && i.pixels().filter(|p| p.0[3] > 100).count() > 1000));
        assert_ne!(images[0].as_raw(), images[1].as_raw(), "each pressing differs");
    }

    #[test]
    fn learns_from_pictures_and_draws_distinct_copies() {
        // A thick stroke with a loop, like a scanned signature on off-white paper.
        let picture = |shift: f64| {
            image::RgbaImage::from_fn(240, 100, |x, y| {
                let (x, y) = (x as f64, y as f64);
                let wave = (y - (55.0 + 22.0 * ((x - shift) / 18.0).sin())).abs() <= 2.5 && (30.0..=210.0).contains(&x);
                let ring = ((x - 60.0).hypot(y - 40.0) - 18.0).abs() <= 2.5;
                image::Rgba(if wave || ring { [25, 30, 90, 255] } else { [246, 244, 236, 255] })
            })
        };
        let pictures = vec![("one.png".to_owned(), picture(0.0)), ("two.png".to_owned(), picture(2.0))];
        let samples = learn_samples(&pictures).unwrap();
        assert_eq!(samples.len(), 2);
        let request = LearnRequest::parse(&["Gel pen", "Violet ink"].map(String::from)).unwrap();
        let images = request.variants(&samples, VARIANTS, &mut SeededRandom::new(11)).unwrap();
        assert_eq!(images.len(), VARIANTS);
        assert!(images.iter().all(|i| i.height() > 60 && i.pixels().filter(|p| p.0[3] > 200).count() > 300));
        for (i, a) in images.iter().enumerate() {
            assert!(images[i + 1..].iter().all(|b| a != b), "copies differ");
        }
        // Violet ink: blue dominates green in the inked pixels.
        let violet = images[1].pixels().filter(|p| p.0[3] > 200 && p.0[2] > p.0[1]).count();
        assert!(violet > images[1].pixels().filter(|p| p.0[3] > 200).count() / 2);

        let blank = ("blank.png".to_owned(), image::RgbaImage::from_pixel(120, 60, image::Rgba([255; 4])));
        let error = learn_samples(&[pictures[0].clone(), blank]).unwrap_err();
        assert!(error.contains("blank.png"), "{error}");
        assert!(learn_samples(&[]).is_err());
        assert!(learn_samples(&vec![pictures[0].clone(); MAXIMUM_LEARN_SAMPLES + 1]).is_err());
        assert!(LearnRequest::parse(&["Quill", "Blue ink"].map(String::from)).is_err());
    }

    #[test]
    fn file_names_are_safe() {
        assert_eq!(safe_stem(r#"A<b>:c/"d\|e?*"#), "A b c d e");
        assert_eq!(safe_stem("   "), "Mark");
        assert_eq!(safe_stem("name..."), "name");
        assert_eq!(safe_stem(&"x".repeat(100)).chars().count(), 60);
    }
}
