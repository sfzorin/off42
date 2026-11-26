//! Shared UI language and catalogs. Document strings are never routed through this module.
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    ffi::OsString,
    io::Read,
    path::{Path, PathBuf},
    sync::{LazyLock, OnceLock},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Language {
    pub code: &'static str,
    pub name: &'static str,
    pub rtl: bool,
}
pub const LANGUAGES: [Language; 20] = [
    Language {
        code: "en",
        name: "English",
        rtl: false,
    },
    Language {
        code: "zh-Hans",
        name: "简体中文",
        rtl: false,
    },
    Language {
        code: "zh-Hant",
        name: "繁體中文（台灣）",
        rtl: false,
    },
    Language {
        code: "es",
        name: "Español",
        rtl: false,
    },
    Language {
        code: "ar",
        name: "العربية",
        rtl: true,
    },
    Language {
        code: "fr",
        name: "Français",
        rtl: false,
    },
    Language {
        code: "pt",
        name: "Português",
        rtl: false,
    },
    Language {
        code: "id",
        name: "Bahasa Indonesia",
        rtl: false,
    },
    Language {
        code: "ru",
        name: "Русский",
        rtl: false,
    },
    Language {
        code: "de",
        name: "Deutsch",
        rtl: false,
    },
    Language {
        code: "ja",
        name: "日本語",
        rtl: false,
    },
    Language {
        code: "vi",
        name: "Tiếng Việt",
        rtl: false,
    },
    Language {
        code: "tr",
        name: "Türkçe",
        rtl: false,
    },
    Language {
        code: "fil",
        name: "Filipino",
        rtl: false,
    },
    Language {
        code: "ko",
        name: "한국어",
        rtl: false,
    },
    Language {
        code: "th",
        name: "ไทย",
        rtl: false,
    },
    Language {
        code: "it",
        name: "Italiano",
        rtl: false,
    },
    Language {
        code: "pl",
        name: "Polski",
        rtl: false,
    },
    Language {
        code: "ms",
        name: "Bahasa Melayu",
        rtl: false,
    },
    Language {
        code: "hu",
        name: "Magyar",
        rtl: false,
    },
];
pub fn matching(identifier: &str) -> Option<Language> {
    let identifier = identifier.replace('_', "-").to_ascii_lowercase();
    let parts = identifier.split('-').collect::<Vec<_>>();
    let base = *parts.first()?;
    let code = if base == "zh" {
        if parts.contains(&"hant") {
            "zh-Hant"
        } else if parts.contains(&"hans") {
            "zh-Hans"
        } else if parts.iter().any(|part| matches!(*part, "tw" | "hk" | "mo")) {
            "zh-Hant"
        } else {
            "zh-Hans"
        }
    } else if base == "tl" {
        "fil"
    } else {
        base
    };
    LANGUAGES
        .iter()
        .copied()
        .find(|language| language.code.eq_ignore_ascii_case(code))
}
#[derive(Debug)]
pub struct LaunchArguments {
    pub language: Option<Language>,
    pub files: Vec<PathBuf>,
}
pub fn launch_arguments(
    arguments: impl IntoIterator<Item = OsString>,
) -> Result<LaunchArguments, String> {
    let mut arguments = arguments.into_iter();
    let mut output = LaunchArguments {
        language: None,
        files: Vec::new(),
    };
    while let Some(argument) = arguments.next() {
        if argument == "--" {
            output.files.extend(arguments.map(PathBuf::from));
            break;
        }
        let inline = argument
            .to_str()
            .and_then(|value| value.strip_prefix("--language="));
        if argument == "--language" || inline.is_some() {
            let value = match inline {
                Some(value) => OsString::from(value),
                None => arguments.next().ok_or("--language needs a language code")?,
            };
            output.language = Some(
                value
                    .to_str()
                    .and_then(matching)
                    .ok_or("Choose a supported UI language code")?,
            );
        } else {
            output.files.push(argument.into());
        }
    }
    Ok(output)
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Settings {
    version: u8,
    language: String,
}
fn settings_path() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA").map(|path| {
        PathBuf::from(path)
            .join("off42")
            .join("Preferences")
            .join("language.json")
    })
}
fn read_settings() -> Option<Language> {
    read_settings_at(&settings_path()?)
}
fn read_settings_at(path: &Path) -> Option<Language> {
    let file = std::fs::File::open(path).ok()?;
    let mut data = Vec::new();
    file.take(2049).read_to_end(&mut data).ok()?;
    if data.len() > 2048 {
        return None;
    }
    let settings: Settings = serde_json::from_slice(&data).ok()?;
    (settings.version == 1)
        .then(|| matching(&settings.language))
        .flatten()
}
pub fn current() -> Language {
    static CURRENT: OnceLock<Language> = OnceLock::new();
    *CURRENT.get_or_init(|| {
        launch_arguments(std::env::args_os().skip(1))
            .ok()
            .and_then(|args| args.language)
            .or_else(read_settings)
            .unwrap_or(LANGUAGES[0])
    })
}
pub fn save(language: Language) -> Result<(), String> {
    let destination =
        settings_path().ok_or("Windows local preferences directory is unavailable")?;
    save_at(&destination, language)
}
fn save_at(destination: &Path, language: Language) -> Result<(), String> {
    std::fs::create_dir_all(
        destination
            .parent()
            .ok_or("Invalid language preferences path")?,
    )
    .map_err(|error| error.to_string())?;
    let expected = if destination.exists() {
        Some(
            suite42_core::session::FileIdentity::read(&destination)
                .map_err(|error| error.to_string())?,
        )
    } else {
        None
    };
    let main = serde_json::to_vec(&Settings {
        version: 1,
        language: language.code.into(),
    })
    .map_err(|error| error.to_string())?;
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
fn normalized(value: &str) -> String {
    value
        .replace("&&", "\u{1}")
        .replace('&', "")
        .replace('\u{1}', "&")
        .replace("...", "…")
        .trim()
        .to_lowercase()
}
struct Catalog {
    values: BTreeMap<String, String>,
    normalized: BTreeMap<String, String>,
}
static CATALOGS: LazyLock<[OnceLock<Catalog>; 19]> =
    LazyLock::new(|| std::array::from_fn(|_| OnceLock::new()));
fn catalog(language: Language) -> Option<&'static Catalog> {
    let index = LANGUAGES
        .iter()
        .position(|candidate| *candidate == language)?
        .checked_sub(1)?;
    Some(CATALOGS[index].get_or_init(|| {
        let mut values: BTreeMap<String, String> =
            serde_json::from_str(catalog_data(language.code))
                .expect("bundled UI catalog must be valid JSON");
        static WINDOWS: LazyLock<BTreeMap<String, BTreeMap<String, String>>> =
            LazyLock::new(|| {
                serde_json::from_str(include_str!("../resources/windows-translations.json"))
                    .expect("bundled Windows UI catalog must be valid JSON")
            });
        if let Some(additions) = WINDOWS.get(language.code) {
            values.extend(additions.clone());
        }
        let normalized = values
            .keys()
            .map(|key| (normalized(key), key.clone()))
            .collect();
        Catalog { values, normalized }
    }))
}
fn catalog_data(code: &str) -> &'static str {
    match code {
        "zh-Hans" => include_str!("../resources/translations/zh-Hans.json"),
        "zh-Hant" => include_str!("../resources/translations/zh-Hant.json"),
        "es" => include_str!("../resources/translations/es.json"),
        "ar" => include_str!("../resources/translations/ar.json"),
        "fr" => include_str!("../resources/translations/fr.json"),
        "pt" => include_str!("../resources/translations/pt.json"),
        "id" => include_str!("../resources/translations/id.json"),
        "ru" => include_str!("../resources/translations/ru.json"),
        "de" => include_str!("../resources/translations/de.json"),
        "ja" => include_str!("../resources/translations/ja.json"),
        "vi" => include_str!("../resources/translations/vi.json"),
        "tr" => include_str!("../resources/translations/tr.json"),
        "fil" => include_str!("../resources/translations/fil.json"),
        "ko" => include_str!("../resources/translations/ko.json"),
        "th" => include_str!("../resources/translations/th.json"),
        "it" => include_str!("../resources/translations/it.json"),
        "pl" => include_str!("../resources/translations/pl.json"),
        "ms" => include_str!("../resources/translations/ms.json"),
        "hu" => include_str!("../resources/translations/hu.json"),
        _ => "{}",
    }
}
pub fn translate_in(language: Language, source: &str) -> String {
    let Some(catalog) = catalog(language) else {
        return source.into();
    };
    if let Some(value) = catalog.values.get(source).filter(|value| !value.is_empty()) {
        return value.clone();
    }
    let key = normalized(source);
    let alias = match key.trim_end_matches(['…', ':']).trim() {
        "about" => "about %@",
        "exit" => "quit %@",
        "create label sheet" => "create labels",
        "create symbol or code" => "symbol or code",
        "insert picture" => "image from file",
        "speaker notes" => "notes",
        "edit speaker notes" => "edit notes",
        "export slide images" => "export images…",
        "scan history" => "history",
        "clear scan history" => "clear history",
        "sheets" => "sheet",
        "toggle bold" => "bold",
        _ => key.as_str(),
    };
    let key = catalog
        .normalized
        .get(alias)
        .or_else(|| catalog.normalized.get(&key))
        .or_else(|| {
            catalog
                .normalized
                .get(alias.trim_end_matches(['…', ':']).trim())
        })
        .or_else(|| catalog.normalized.get(&format!("{alias}…")));
    key.and_then(|key| catalog.values.get(key))
        .filter(|value| !value.is_empty())
        .map(|value| {
            if alias == "about %@" || alias == "quit %@" {
                value.replace("%@", "").trim().into()
            } else {
                value.clone()
            }
        })
        .unwrap_or_else(|| source.into())
}
pub fn text(source: &str) -> String {
    translate_in(current(), source)
}
pub fn menu(source: &str) -> String {
    if current().code == "en" {
        return source.into();
    }
    let (label, accelerator) = source.split_once('\t').unwrap_or((source, ""));
    let translated = text(label);
    if translated == label {
        return source.into();
    }
    let mut output = translated.replace('&', "&&");
    if !accelerator.is_empty() {
        output.push('\t');
        output.push_str(accelerator);
    }
    output
}
pub fn choice_value(originals: &[String], value: &str) -> String {
    let mut matches = originals.iter().filter(|original| text(original) == value);
    match (matches.next(), matches.next()) {
        (Some(original), None) => original.clone(),
        _ => value.into(),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn windows_specific_labels_cover_every_language_without_changing_payloads() {
        for language in LANGUAGES.into_iter().skip(1) {
            for key in [
                "Symbol or Code",
                "Symbol or barcode format",
                "Barcode value",
                "Width (mm)",
                "Caption (yes/no)",
                "Insert Symbol or Code…",
                "Edit Symbol or Code…",
                "Create Symbol or Code…",
                "No Recent Documents",
                "Check Spelling as You Type",
                "Edit Speaker Notes…",
                "Speaker Notes",
                "Export Slide Images…",
                "Scan History",
                "Clear Scan History…",
            ] {
                assert_ne!(translate_in(language, key), key, "{}: {key}", language.code);
            }
            assert_eq!(translate_in(language, "PCB-0007"), "PCB-0007");
        }
    }
    #[test]
    fn language_order_tags_catalogs_and_literal_data_are_preserved() {
        assert_eq!(LANGUAGES.len(), 20);
        assert_eq!(matching("zh-TW").unwrap().code, "zh-Hant");
        assert_eq!(matching("zh-Hans-TW").unwrap().code, "zh-Hans");
        assert_eq!(matching("zh_Hant_CN").unwrap().code, "zh-Hant");
        assert_eq!(matching("tl-PH").unwrap().code, "fil");
        assert_eq!(matching("ru-RU").unwrap().code, "ru");
        assert!(matching("unknown").is_none());
        for language in LANGUAGES.into_iter().skip(1) {
            let catalog = catalog(language).unwrap();
            assert!(catalog.values.len() > 2000);
            assert_eq!(translate_in(language, "File"), catalog.values["File"]);
            assert_eq!(translate_in(language, "PCB-0007"), "PCB-0007");
        }
        assert_eq!(
            translate_in(matching("ru").unwrap(), "&Open..."),
            "Открыть…"
        );
        assert_eq!(translate_in(matching("tr").unwrap(), "&Save"), "Kaydet");
    }
    #[test]
    fn settings_round_trip_without_changing_document_or_process_data() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("preferences.json");
        assert!(read_settings_at(&path).is_none());
        save_at(&path, matching("ru").unwrap()).unwrap();
        assert_eq!(read_settings_at(&path).unwrap().code, "ru");
        save_at(&path, matching("zh-Hant").unwrap()).unwrap();
        assert_eq!(read_settings_at(&path).unwrap().code, "zh-Hant");
        std::fs::write(&path, br#"{"version":99,"language":"ru"}"#).unwrap();
        assert!(read_settings_at(&path).is_none());
        assert_eq!(
            choice_value(&["yes".into(), "no".into()], "PCB-0007"),
            "PCB-0007"
        );
    }
    #[test]
    fn language_options_do_not_become_files_and_delimiter_keeps_literal_paths() {
        let args = ["--language", "zh-TW", "--", "--language", "book.xlsx"].map(OsString::from);
        let parsed = launch_arguments(args).unwrap();
        assert_eq!(parsed.language.unwrap().code, "zh-Hant");
        assert_eq!(
            parsed.files,
            [PathBuf::from("--language"), PathBuf::from("book.xlsx")]
        );
        assert!(launch_arguments([OsString::from("--language")]).is_err());
        assert!(launch_arguments([OsString::from("--language=invalid")]).is_err());
    }
}
