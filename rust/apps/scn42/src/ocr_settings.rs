//! Per-user OCR configuration. No credentials or document data are stored here.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Settings {
    pub executable: String,
    pub language: String,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            executable: "tesseract".into(),
            language: "eng".into(),
        }
    }
}
fn path() -> Option<std::path::PathBuf> {
    std::env::var_os("APPDATA").map(|p| {
        std::path::PathBuf::from(p)
            .join("SCN42")
            .join("ocr-settings.txt")
    })
}
pub fn load() -> Settings {
    path().and_then(|p| read(&p).ok()).unwrap_or_default()
}
fn read(path: &std::path::Path) -> Result<Settings, String> {
    let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let (exe, lang) = text.split_once('\n').ok_or("Invalid OCR settings")?;
    let language = lang.trim_end_matches(['\r', '\n']);
    if exe.is_empty()
        || exe.len() > 4096
        || language.is_empty()
        || language.len() > 256
        || !language
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '+' || c == '-')
    {
        return Err("Invalid OCR settings".into());
    }
    Ok(Settings {
        executable: exe.to_owned(),
        language: language.to_owned(),
    })
}
pub fn save(settings: &Settings) -> Result<(), String> {
    let Some(path) = path() else {
        return Ok(());
    };
    if settings.executable.is_empty()
        || settings.executable.contains(['\n', '\r'])
        || settings.executable.len() > 4096
        || settings.language.is_empty()
        || settings.language.len() > 256
        || !settings
            .language
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '+' || c == '-')
    {
        return Err("Invalid OCR settings".into());
    }
    std::fs::create_dir_all(path.parent().ok_or("Invalid settings path")?)
        .map_err(|e| e.to_string())?;
    let mut file =
        tempfile::NamedTempFile::new_in(path.parent().unwrap()).map_err(|e| e.to_string())?;
    use std::io::Write;
    write!(file, "{}\n{}\n", settings.executable, settings.language).map_err(|e| e.to_string())?;
    file.persist(path).map_err(|e| e.to_string())?;
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_malformed_configuration() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("ocr.txt");
        std::fs::write(&p, "C:\\Tools\\tesseract.exe\neng+rus\n").unwrap();
        assert_eq!(read(&p).unwrap().language, "eng+rus");
        std::fs::write(&p, "exe\neng\nextra\n").unwrap();
        assert!(read(&p).is_err());
    }
}
