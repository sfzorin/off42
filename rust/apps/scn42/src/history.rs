//! Per-user history of exported scans. Entries contain paths and file metadata only.
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
};
use suite42_core::session::{publish_bundle, FileIdentity, SaveBundle};

const LIMIT: usize = 200;
const MAX_BYTES: u64 = 2 * 1024 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Entry {
    pub path: PathBuf,
    pub pages: usize,
    pub bytes: u64,
    saved_at: i64,
}

impl Entry {
    fn valid(&self) -> bool {
        self.path.is_absolute()
            && self.path.as_os_str().len() <= 4096
            && !self.path.to_string_lossy().chars().any(char::is_control)
            && (1..=10_000).contains(&self.pages)
            && self.bytes > 0
    }

    pub(super) fn label(&self, index: usize) -> String {
        let date = chrono::DateTime::from_timestamp(self.saved_at, 0)
            .map(|date| {
                date.with_timezone(&chrono::Local)
                    .format("%Y-%m-%d %H:%M")
                    .to_string()
            })
            .unwrap_or_default();
        let name = self.path.file_name().unwrap_or_default().to_string_lossy();
        let pages = if self.pages == 1 {
            win42::localization::text("1 page")
        } else {
            win42::localization::text("%d pages").replace("%d", &self.pages.to_string())
        };
        format!(
            "{}. {name} · {date} · {pages} · {:.1} KiB · {}",
            index + 1,
            self.bytes as f64 / 1024.0,
            self.path
                .parent()
                .unwrap_or_else(|| Path::new(""))
                .display(),
        )
    }
}

pub(super) fn location() -> Option<PathBuf> {
    Some(
        PathBuf::from(std::env::var_os("LOCALAPPDATA")?)
            .join("off42")
            .join("Scan History.json"),
    )
}

fn read(path: &Path) -> Result<(Vec<Entry>, Option<FileIdentity>), String> {
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok((Vec::new(), None))
        }
        Err(error) => return Err(error.to_string()),
    };
    let meta = file.metadata().map_err(|error| error.to_string())?;
    let mut bytes = Vec::new();
    file.take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err("Scan history exceeds its size limit".into());
    }
    let entries: Vec<Entry> = serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
    if entries.len() > LIMIT || !entries.iter().all(Entry::valid) {
        return Err("Invalid scan history".into());
    }
    Ok((
        entries,
        Some(FileIdentity::from_bytes(&bytes, meta.modified().ok())),
    ))
}

pub(super) fn load() -> Vec<Entry> {
    location()
        .and_then(|path| read(&path).ok())
        .map_or_else(Vec::new, |(entries, _)| entries)
}

fn same_path(a: &Path, b: &Path) -> bool {
    if cfg!(windows) {
        a.to_string_lossy()
            .eq_ignore_ascii_case(&b.to_string_lossy())
    } else {
        a == b
    }
}

fn edit(path: &Path, change: impl Fn(&mut Vec<Entry>)) -> Result<(), String> {
    fs::create_dir_all(path.parent().ok_or("Invalid history path")?)
        .map_err(|error| error.to_string())?;
    let lock = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(path.with_extension("lock"))
        .map_err(|error| error.to_string())?;
    lock.lock().map_err(|error| error.to_string())?;
    let (mut entries, expected) = read(path)?;
    change(&mut entries);
    let main = serde_json::to_vec(&entries).map_err(|error| error.to_string())?;
    if main.len() as u64 > MAX_BYTES {
        return Err("Scan history exceeds its size limit".into());
    }
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

pub(super) fn record(paths: &[(&Path, usize)]) -> Result<(), String> {
    let history = location().ok_or("Scan history directory unavailable")?;
    record_at(&history, paths)
}

fn record_at(history: &Path, paths: &[(&Path, usize)]) -> Result<(), String> {
    let saved_at = chrono::Utc::now().timestamp();
    let new: Vec<_> = paths
        .iter()
        .map(|(path, pages)| {
            let path = std::path::absolute(path)
                .and_then(|path| suite42_core::win32_path(&path))
                .map_err(|error| error.to_string())?;
            let meta = fs::metadata(&path).map_err(|error| error.to_string())?;
            let entry = Entry {
                path,
                pages: *pages,
                bytes: meta.len(),
                saved_at,
            };
            if !meta.is_file() || !entry.valid() {
                return Err("Invalid scan history entry".into());
            }
            Ok(entry)
        })
        .collect::<Result<_, String>>()?;
    edit(history, |entries| {
        for entry in new.iter().rev() {
            entries.retain(|old| !same_path(&old.path, &entry.path));
            entries.insert(0, entry.clone());
        }
        entries.truncate(LIMIT);
    })
}

pub(super) fn remove(path: &Path) -> Result<(), String> {
    let history = location().ok_or("Scan history directory unavailable")?;
    edit(&history, |entries| {
        entries.retain(|entry| !same_path(&entry.path, path))
    })
}

pub(super) fn clear() -> Result<(), String> {
    let history = location().ok_or("Scan history directory unavailable")?;
    edit(&history, Vec::clear)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_metadata_and_replaces_duplicates_without_touching_exports() {
        let dir = tempfile::tempdir().unwrap();
        let history = dir.path().join("history.json");
        let a = dir.path().join("a.pdf");
        let b = dir.path().join("b.png");
        fs::write(&a, b"private scan bytes").unwrap();
        fs::write(&b, b"other pixels").unwrap();
        record_at(&history, &[(&a, 3), (&b, 1)]).unwrap();
        record_at(&history, &[(&b, 2)]).unwrap();
        let (entries, _) = read(&history).unwrap();
        let a_key = suite42_core::win32_path(&a).unwrap();
        let b_key = suite42_core::win32_path(&b).unwrap();
        assert_eq!(
            entries
                .iter()
                .map(|entry| (entry.path.as_path(), entry.pages))
                .collect::<Vec<_>>(),
            [(b_key.as_path(), 2), (a_key.as_path(), 3)]
        );
        assert!(!fs::read_to_string(&history)
            .unwrap()
            .contains("private scan bytes"));
        edit(&history, Vec::clear).unwrap();
        assert!(read(&history).unwrap().0.is_empty());
        assert_eq!(fs::read(a).unwrap(), b"private scan bytes");
    }

    #[test]
    fn concurrent_exports_retain_both_history_entries() {
        let dir = tempfile::tempdir().unwrap();
        let history = dir.path().join("history.json");
        let first = dir.path().join("first.pdf");
        let second = dir.path().join("second.pdf");
        fs::write(&first, b"first").unwrap();
        fs::write(&second, b"second").unwrap();
        let ready = std::sync::Barrier::new(2);
        std::thread::scope(|scope| {
            let a = scope.spawn(|| {
                ready.wait();
                record_at(&history, &[(&first, 1)])
            });
            let b = scope.spawn(|| {
                ready.wait();
                record_at(&history, &[(&second, 1)])
            });
            a.join().unwrap().unwrap();
            b.join().unwrap().unwrap();
        });
        let (entries, _) = read(&history).unwrap();
        let mut names = entries
            .iter()
            .map(|entry| entry.path.file_name().unwrap())
            .collect::<Vec<_>>();
        names.sort();
        assert_eq!(names, ["first.pdf", "second.pdf"]);
    }
}
