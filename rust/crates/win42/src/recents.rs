//! Local recent document paths. No document content or cloud synchronization.
use serde::{Deserialize, Serialize};
use std::{
    io::Read,
    path::{Path, PathBuf},
};
pub(crate) const LIMIT: usize = 10;
const MAX_BYTES: u64 = 512 * 1024;
#[derive(Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Recent {
    version: u8,
    paths: Vec<PathBuf>,
}
fn location(app: &str) -> Option<PathBuf> {
    if app.is_empty() || app.len() > 32 || !app.bytes().all(|b| b.is_ascii_alphanumeric()) {
        return None;
    }
    Some(
        PathBuf::from(std::env::var_os("LOCALAPPDATA")?)
            .join("off42")
            .join("Recent Documents")
            .join(format!("{app}.json")),
    )
}
fn valid(path: &Path) -> bool {
    path.is_absolute()
        && path.as_os_str().len() <= 32767
        && !path.to_string_lossy().contains(['\0', '\r', '\n', '\t'])
}
fn read_at(location: &Path) -> Vec<PathBuf> {
    let data = (|| -> Option<Recent> {
        let file = std::fs::File::open(location).ok()?;
        let mut data = Vec::new();
        file.take(MAX_BYTES + 1).read_to_end(&mut data).ok()?;
        if data.len() as u64 > MAX_BYTES {
            return None;
        }
        serde_json::from_slice(&data).ok()
    })();
    let Some(data) = data.filter(|d| d.version == 1 && d.paths.len() <= LIMIT) else {
        return vec![];
    };
    if !data.paths.iter().all(|p| valid(p)) {
        return vec![];
    }
    data.paths
}
pub(crate) fn load(app: &str) -> Vec<PathBuf> {
    location(app).map(|p| read_at(&p)).unwrap_or_default()
}
fn write_at(location: &Path, paths: Vec<PathBuf>) -> Result<(), String> {
    std::fs::create_dir_all(location.parent().ok_or("Invalid recent documents path")?)
        .map_err(|e| e.to_string())?;
    let identity = if location.exists() {
        Some(suite42_core::session::FileIdentity::read(location).map_err(|e| e.to_string())?)
    } else {
        None
    };
    let main = serde_json::to_vec(&Recent { version: 1, paths }).map_err(|e| e.to_string())?;
    suite42_core::session::publish_bundle(
        location,
        &suite42_core::session::SaveBundle {
            main,
            sidecars: vec![],
        },
        identity.as_ref(),
    )
    .map(|_| ())
    .map_err(|e| e.to_string())
}
fn remember_at(location: &Path, path: &Path) -> Result<(), String> {
    let path = if path.is_absolute() {
        path.to_owned()
    } else {
        std::env::current_dir()
            .map_err(|e| e.to_string())?
            .join(path)
    };
    if !valid(&path) {
        return Err("Invalid recent document path".into());
    }
    let mut paths = read_at(location);
    paths.retain(|previous| {
        #[cfg(windows)]
        {
            !previous
                .to_string_lossy()
                .eq_ignore_ascii_case(&path.to_string_lossy())
        }
        #[cfg(not(windows))]
        {
            previous != &path
        }
    });
    paths.insert(0, path);
    paths.truncate(LIMIT);
    write_at(location, paths)
}
pub(crate) fn remember(app: &str, path: &Path) -> Result<(), String> {
    remember_at(
        &location(app).ok_or("Recent documents directory unavailable")?,
        path,
    )
}
pub(crate) fn clear(app: &str) -> Result<(), String> {
    write_at(
        &location(app).ok_or("Recent documents directory unavailable")?,
        vec![],
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn paths_round_trip_deduplicate_and_keep_only_the_latest_ten() {
        let directory = tempfile::tempdir().unwrap();
        let preferences = directory.path().join("prefs/recent.json");
        for index in 0..12 {
            remember_at(
                &preferences,
                &directory.path().join(format!("плата & {index}.docx")),
            )
            .unwrap();
        }
        let paths = read_at(&preferences);
        assert_eq!(paths.len(), 10);
        assert!(paths[0].ends_with("плата & 11.docx"));
        let last = paths.last().unwrap().clone();
        remember_at(&preferences, &last).unwrap();
        let reordered = read_at(&preferences);
        assert_eq!(reordered[0], last);
        assert_eq!(reordered.len(), 10);
        write_at(&preferences, vec![]).unwrap();
        assert!(read_at(&preferences).is_empty());
    }
    #[test]
    fn malformed_oversized_and_relative_preferences_are_ignored() {
        let directory = tempfile::tempdir().unwrap();
        let preferences = directory.path().join("recent.json");
        for bytes in [
            b"broken".to_vec(),
            br#"{"version":2,"paths":[]}"#.to_vec(),
            br#"{"version":1,"paths":["relative.docx"]}"#.to_vec(),
            vec![b' '; MAX_BYTES as usize + 1],
        ] {
            std::fs::write(&preferences, bytes).unwrap();
            assert!(read_at(&preferences).is_empty());
        }
        assert!(location("../other").is_none());
    }
}
