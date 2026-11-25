//! Remember layout and column mappings without retaining document text or code values.
use crate::{InputRequest, LabelSource};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    io::Read,
    path::{Path, PathBuf},
};

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Profile {
    version: u8,
    format: String,
    stock: String,
    copies_and_start: String,
    header_signature: Option<String>,
    mapping: Option<[String; 2]>,
}
fn signature(source: &LabelSource) -> Option<String> {
    let LabelSource::Rows(rows) = source else {
        return None;
    };
    let (_, headers) = rows.first()?;
    let mut hash = Sha256::new();
    hash.update((headers.len() as u64).to_le_bytes());
    for header in headers {
        hash.update((header.len() as u64).to_le_bytes());
        hash.update(header.as_bytes());
    }
    Some(
        hash.finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect(),
    )
}
fn path(directory: &Path, app: &str, source: &LabelSource) -> Result<PathBuf, String> {
    if app.is_empty()
        || app.len() > 32
        || !app
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err("Invalid label profile name".into());
    }
    Ok(directory.join(format!(
        "{app}-{}.json",
        if matches!(source, LabelSource::Rows(_)) {
            "rows"
        } else if matches!(source,LabelSource::PdfArtwork{..}|LabelSource::PictureArtwork{..}) {"artwork"} else {
            "text"
        }
    )))
}
fn valid(profile: &Profile) -> bool {
    profile.version == 1
        && [&profile.format, &profile.stock, &profile.copies_and_start]
            .iter()
            .all(|value| !value.is_empty() && value.len() <= 512)
        && profile
            .header_signature
            .as_ref()
            .is_none_or(|value| value.len() == 64 && value.bytes().all(|c| c.is_ascii_hexdigit()))
        && profile
            .mapping
            .as_ref()
            .is_none_or(|values| values.iter().all(|value| value.len() <= 512))
}
pub(crate) fn restore(
    directory: &Path,
    app: &str,
    source: &LabelSource,
    request: &mut InputRequest,
) -> Result<(), String> {
    let file = match std::fs::File::open(path(directory, app, source)?) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.to_string()),
    };
    let mut data = Vec::new();
    file.take(8193)
        .read_to_end(&mut data)
        .map_err(|error| error.to_string())?;
    if data.len() > 8192 {
        return Err("Label profile is too large".into());
    }
    let profile: Profile = serde_json::from_slice(&data).map_err(|error| error.to_string())?;
    if matches!(source,LabelSource::PdfArtwork{..}|LabelSource::PictureArtwork{..}) {
        if !valid(&profile)||request.fields.len()!=3{return Err("Invalid artwork label preferences".into());}
        request.fields[1].1=profile.stock;request.fields[2].1=profile.copies_and_start;return Ok(());
    }
    if !valid(&profile) || request.fields.len() != 5 {
        return Err("Invalid label preferences".into());
    }
    request.fields[2].1 = profile.format;
    request.fields[3].1 = profile.stock;
    request.fields[4].1 = profile.copies_and_start;
    if profile.header_signature.is_some() && profile.header_signature == signature(source) {
        if let Some([first, second]) = profile.mapping {
            request.fields[0].1 = first;
            request.fields[1].1 = second;
        }
    }
    Ok(())
}
pub(crate) fn save(
    directory: &Path,
    app: &str,
    source: &LabelSource,
    fields: &[String],
) -> Result<(), String> {
    let artwork;
    let fields=if matches!(source,LabelSource::PdfArtwork{..}|LabelSource::PictureArtwork{..}) {
        let [_,stock,copies]=fields else{return Err("Invalid artwork label preferences".into());};
        artwork=[String::new(),String::new(),"PDF artwork".into(),stock.clone(),copies.clone()];&artwork[..]
    }else{fields};
    let [first, second, format, stock, copies] = fields else {
        return Err("Invalid label preferences".into());
    };
    let profile = Profile {
        version: 1,
        format: format.clone(),
        stock: stock.clone(),
        copies_and_start: copies.clone(),
        header_signature: signature(source),
        mapping: matches!(source, LabelSource::Rows(_)).then(|| [first.clone(), second.clone()]),
    };
    if !valid(&profile) {
        return Err("Invalid label preferences".into());
    }
    let destination = path(directory, app, source)?;
    std::fs::create_dir_all(directory).map_err(|error| error.to_string())?;
    let expected = if destination.exists() {
        Some(
            suite42_core::session::FileIdentity::read(&destination)
                .map_err(|error| error.to_string())?,
        )
    } else {
        None
    };
    let main = serde_json::to_vec(&profile).map_err(|error| error.to_string())?;
    suite42_core::session::publish_bundle(
        &destination,
        &suite42_core::session::SaveBundle {
            main,
            sidecars: Vec::new(),
        },
        expected.as_ref(),
    )
    .map(|_| ())
    .map_err(|error| error.to_string())
}
pub(crate) fn directory() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA")
        .map(|path| PathBuf::from(path).join("off42").join("Label Preferences"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preserves_layout_and_matching_mappings_but_never_text_payloads() {
        let dir = tempfile::tempdir().unwrap();
        let source = LabelSource::Text("private selected text".into());
        let fields =
            ["private name", "private code", "QR", "70, 37, 0, 0", "2, 3"].map(String::from);
        save(dir.path(), "DOC42", &source, &fields).unwrap();
        let raw = std::fs::read_to_string(path(dir.path(), "DOC42", &source).unwrap()).unwrap();
        assert!(!raw.contains("private"));
        let mut request = crate::production::sheet_request(&source).unwrap();
        restore(dir.path(), "DOC42", &source, &mut request).unwrap();
        assert_ne!(request.fields[0].1, "private name");
        assert_ne!(request.fields[1].1, "private code");
        assert_eq!(request.fields[4].1, "2, 3");
        let rows = LabelSource::Rows(vec![
            (1, vec!["Name".into(), "Code".into()]),
            (2, vec!["private row".into(), "private row code".into()]),
        ]);
        let fields = ["2", "1, 0, 1", "Aztec", "70, 37, 0, 0", "1, 4"].map(String::from);
        save(dir.path(), "XLS42", &rows, &fields).unwrap();
        let mut request = crate::production::sheet_request(&rows).unwrap();
        restore(dir.path(), "XLS42", &rows, &mut request).unwrap();
        assert_eq!(request.fields[0].1, "2");
        assert_eq!(request.fields[1].1, "1, 0, 1");
        let different = LabelSource::Rows(vec![(1, vec!["Code".into(), "Name".into()])]);
        let mut request = crate::production::sheet_request(&different).unwrap();
        let first = request.fields[0].1.clone();
        let second = request.fields[1].1.clone();
        restore(dir.path(), "XLS42", &different, &mut request).unwrap();
        assert_eq!(
            (request.fields[0].1.clone(), request.fields[1].1.clone()),
            (first, second)
        );
        assert_eq!(request.fields[2].1, "Aztec");
        let raw = std::fs::read_to_string(path(dir.path(), "XLS42", &rows).unwrap()).unwrap();
        assert!(!raw.contains("private"));
        assert!(!raw.contains("Name"));
    }
}

#[cfg(test)]
mod artwork_tests {
    use super::*;
    #[test]
    fn remembers_artwork_stock_without_pdf_bytes_or_private_region(){
        let directory=tempfile::tempdir().unwrap();let source=LabelSource::PdfArtwork{bytes:b"PRIVATE PDF DATA".to_vec(),region:Some([10.,20.,30.,40.])};
        let fields=["PRIVATE REGION","70, 37, 0, 0","3, 2"].map(str::to_string);
        save(directory.path(),"PDF42",&source,&fields).unwrap();
        let data=std::fs::read_to_string(path(directory.path(),"PDF42",&source).unwrap()).unwrap();assert!(!data.contains("PRIVATE"));assert!(!data.contains("10,20"));
        let mut request=crate::production::sheet_request(&source).unwrap();let original=request.fields[0].1.clone();restore(directory.path(),"PDF42",&source,&mut request).unwrap();assert_eq!(request.fields[0].1,original);assert_eq!(request.fields[2].1,"3, 2");
    }
}
