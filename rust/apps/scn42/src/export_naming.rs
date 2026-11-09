//! Local filename templates. OCR text is expanded only in the export picker.
use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    io::{ErrorKind, Read},
    path::{Path, PathBuf},
};
use suite42_core::session::{publish_bundle, FileIdentity, SaveBundle};

const MAX_SETTINGS: u64 = 4096;
const MAX_TEMPLATE: usize = 512;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Settings {
    pub(super) template: String,
    next: u64,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            template: format!("{} YYYY-MM-DD HH-mm", win42::localization::text("Scan")),
            next: 1,
        }
    }
}

impl Settings {
    pub(super) fn suggest(&self, page: Option<&scn42_core::Page>) -> String {
        let title = if self.template.contains("{title}") {
            page.map(|page| first_line(&page.words)).unwrap_or_default()
        } else {
            String::new()
        };
        expand(&self.template, Local::now(), self.next, &title)
    }
}

fn first_line(words: &[scn42_core::OcrWord]) -> String {
    let Some(first) = words.first() else {
        return String::new();
    };
    words
        .iter()
        .take(32)
        .take_while(|word| {
            let top = word.y.max(first.y);
            let bottom = word
                .y
                .saturating_add(word.height)
                .min(first.y.saturating_add(first.height));
            bottom.saturating_sub(top) >= word.height.min(first.height).div_ceil(2)
        })
        .map(|word| word.text.as_str())
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(160)
        .collect()
}

fn expand(template: &str, date: DateTime<Local>, next: u64, title: &str) -> String {
    let number = format!("{next:03}");
    let name = template
        .replace("YYYY", &date.format("%Y").to_string())
        .replace("MM", &date.format("%m").to_string())
        .replace("DD", &date.format("%d").to_string())
        .replace("HH", &date.format("%H").to_string())
        .replace("mm", &date.format("%M").to_string())
        .replace("ss", &date.format("%S").to_string())
        .replace("{n}", &number)
        .replace("###", &number)
        .replace("{title}", title);
    let name: String = name
        .chars()
        .map(|ch| {
            if ch.is_control() || matches!(ch, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*')
            {
                '-'
            } else {
                ch
            }
        })
        .collect();
    let name = name.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut units = 0;
    let name: String = name
        .chars()
        .take_while(|ch| {
            units += ch.len_utf16();
            units <= 160
        })
        .collect();
    let name = name.trim_matches(|ch: char| ch.is_whitespace() || matches!(ch, '-' | '_' | '.'));
    if name.is_empty() {
        return win42::localization::text("Scan");
    }
    let stem = name
        .split('.')
        .next()
        .unwrap_or_default()
        .trim_end()
        .to_ascii_uppercase();
    if matches!(
        stem.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
    ) || stem
        .strip_prefix("COM")
        .or_else(|| stem.strip_prefix("LPT"))
        .is_some_and(|digit| {
            matches!(
                digit,
                "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
            )
        })
    {
        format!("Scan {name}")
    } else {
        name.into()
    }
}

fn location() -> Option<PathBuf> {
    Some(
        PathBuf::from(std::env::var_os("LOCALAPPDATA")?)
            .join("off42")
            .join("Scan Export Naming.json"),
    )
}

fn valid_template(template: &str) -> bool {
    !template.trim().is_empty()
        && template.len() <= MAX_TEMPLATE
        && !template.chars().any(char::is_control)
}

fn read(path: &Path) -> Result<(Settings, Option<FileIdentity>), String> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == ErrorKind::NotFound => {
            return Ok((Settings::default(), None))
        }
        Err(error) => return Err(error.to_string()),
    };
    let modified = file.metadata().ok().and_then(|meta| meta.modified().ok());
    let mut bytes = Vec::new();
    file.take(MAX_SETTINGS + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() as u64 > MAX_SETTINGS {
        return Err("Scan export settings exceed the size limit".into());
    }
    let settings: Settings = serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
    if settings.next == 0 || !valid_template(&settings.template) {
        return Err("Invalid scan export naming settings".into());
    }
    Ok((settings, Some(FileIdentity::from_bytes(&bytes, modified))))
}

pub(super) fn load() -> Settings {
    location()
        .and_then(|path| read(&path).ok())
        .map(|(settings, _)| settings)
        .unwrap_or_default()
}

fn edit(path: &Path, change: impl FnOnce(&mut Settings)) -> Result<(), String> {
    fs::create_dir_all(path.parent().ok_or("Invalid scan settings path")?)
        .map_err(|error| error.to_string())?;
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(path.with_extension("lock"))
        .map_err(|error| error.to_string())?;
    lock.lock().map_err(|error| error.to_string())?;
    let (mut settings, expected) = read(path)?;
    change(&mut settings);
    let main = serde_json::to_vec(&settings).map_err(|error| error.to_string())?;
    publish_bundle(
        path,
        &SaveBundle {
            main,
            sidecars: Vec::new(),
        },
        expected.as_ref(),
    )
    .map(|_| ())
    .map_err(|error| error.to_string())
}

pub(super) fn set_template(template: &str) -> Result<(), String> {
    let template = template.trim();
    if !valid_template(template) {
        return Err("Enter a filename template of up to 512 bytes".into());
    }
    let path = location().ok_or("Scan export settings directory unavailable")?;
    edit(&path, |settings| settings.template = template.into())
}

pub(super) fn advance() -> Result<(), String> {
    let path = location().ok_or("Scan export settings directory unavailable")?;
    edit(&path, |settings| {
        settings.next = settings.next.saturating_add(1)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn token_expansion_and_windows_filename_safety() {
        let date = Local
            .with_ymd_and_hms(2026, 3, 5, 14, 7, 8)
            .single()
            .unwrap();
        assert_eq!(
            expand("YYYY-MM-DD HH-mm-ss {n} ### {title}", date, 7, "A:/B? YYYY"),
            "2026-03-05 14-07-08 007 007 A--B- YYYY"
        );
        assert_eq!(expand("COM¹.txt", date, 1, ""), "Scan COM¹.txt");
        assert_eq!(expand("NUL.tar", date, 1, ""), "Scan NUL.tar");
        assert_eq!(
            expand("{title}", date, 1, &"𐀀".repeat(120))
                .encode_utf16()
                .count(),
            160
        );
    }

    #[test]
    fn settings_updates_preserve_template_and_counter_across_process_handles() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("naming.json");
        edit(&path, |settings| settings.template = "Scan {n}".into()).unwrap();
        std::thread::scope(|scope| {
            for _ in 0..4 {
                scope.spawn(|| edit(&path, |settings| settings.next += 1).unwrap());
            }
        });
        let settings = read(&path).unwrap().0;
        assert_eq!(settings.next, 5);
        assert_eq!(settings.template, "Scan {n}");
    }
}
