//! Signature and stamp dialogs; generation and the library live in `suite42_app::generator`.
pub use suite42_app::generator::*;
#[cfg(windows)]
use marks42::SeededRandom;

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

