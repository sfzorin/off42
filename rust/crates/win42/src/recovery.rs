//! Atomic local recovery copies. A held file lock separates live sessions from crashed ones.
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::{File, OpenOptions},
    io::Read,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
    time::{SystemTime, UNIX_EPOCH},
};
const MAGIC: &[u8; 8] = b"OFF42RC1";
const MAX_HEADER: usize = 131072;
pub const MAX_PAYLOAD: usize = 256 * 1024 * 1024;
static NEXT: AtomicU64 = AtomicU64::new(1);
#[derive(Debug)]
pub struct Snapshot {
    pub extension: String,
    pub bytes: Vec<u8>,
}
impl Snapshot {
    pub fn new(extension: &str, bytes: Vec<u8>) -> Self {
        Self {
            extension: extension.into(),
            bytes,
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Header {
    version: u8,
    app: String,
    extension: String,
    origin: Option<PathBuf>,
    saved_at: u64,
    length: u64,
    sha256: String,
}
fn hash(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|v| format!("{v:02x}"))
        .collect()
}
fn timestamp(seconds: u64) -> String {
    let days = (seconds / 86400) as i64;
    let z = days + 719468;
    let era = z / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let mut year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = mp + if mp < 10 { 3 } else { -9 };
    if month <= 2 {
        year += 1;
    }
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02} UTC",
        seconds / 3600 % 24,
        seconds / 60 % 60
    )
}
fn component(s: &str) -> bool {
    !s.is_empty() && s.len() <= 32 && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
}
fn valid(header: &Header, app: &str) -> bool {
    header.version == 1
        && header.saved_at <= 253402300799
        && header.app == app
        && component(app)
        && component(&header.extension)
        && header.extension.len() <= 10
        && header.length > 0
        && header.length <= MAX_PAYLOAD as u64
        && header.sha256.len() == 64
        && header.sha256.bytes().all(|b| b.is_ascii_hexdigit())
        && header.origin.as_ref().is_none_or(|p| {
            p.is_absolute() && p.as_os_str().len() <= 32767 && !p.to_string_lossy().contains('\0')
        })
}
pub fn directory() -> Option<PathBuf> {
    Some(
        PathBuf::from(std::env::var_os("LOCALAPPDATA")?)
            .join("off42")
            .join("Recovery"),
    )
}
fn lock(path: &Path, create: bool) -> Result<File, String> {
    if !create
        && std::fs::symlink_metadata(path)
            .map_err(|e| e.to_string())?
            .file_type()
            .is_symlink()
    {
        return Err("Invalid recovery lock".into());
    }
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(create)
        .open(path)
        .map_err(|e| e.to_string())?;
    file.try_lock()
        .map_err(|e| format!("Recovery copy is open in another window: {e}"))?;
    Ok(file)
}
fn header(path: &Path, app: &str) -> Result<(File, Header), String> {
    if std::fs::symlink_metadata(path)
        .map_err(|e| e.to_string())?
        .file_type()
        .is_symlink()
    {
        return Err("Invalid recovery record".into());
    }
    let mut file = File::open(path).map_err(|e| e.to_string())?;
    let mut prefix = [0u8; 12];
    file.read_exact(&mut prefix).map_err(|e| e.to_string())?;
    if &prefix[..8] != MAGIC {
        return Err("Unknown recovery record".into());
    }
    let length = u32::from_le_bytes(prefix[8..].try_into().unwrap()) as usize;
    if length == 0 || length > MAX_HEADER {
        return Err("Recovery header exceeds limits".into());
    }
    let mut bytes = vec![0; length];
    file.read_exact(&mut bytes).map_err(|e| e.to_string())?;
    let header: Header = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    if !valid(&header, app)
        || file.metadata().map_err(|e| e.to_string())?.len() != 12 + length as u64 + header.length
    {
        return Err("Invalid or incomplete recovery record".into());
    }
    Ok((file, header))
}
#[derive(Clone, Debug)]
pub struct Copy {
    directory: PathBuf,
    app: String,
    header: Header,
}
impl Copy {
    pub fn title(&self) -> String {
        format!(
            "{} · {}",
            self.header
                .origin
                .as_ref()
                .and_then(|p| p.file_name())
                .map(|v| v.to_string_lossy().into_owned())
                .unwrap_or_else(|| crate::localization::text("Untitled")),
            timestamp(self.header.saved_at)
        )
    }
    pub fn acquire(&self) -> Result<Claim, String> {
        let lock = lock(&self.directory.join("session.lock"), false)?;
        let (mut file, header) = header(&self.directory.join("document.off42r"), &self.app)?;
        let mut bytes = Vec::with_capacity(header.length as usize);
        file.by_ref()
            .take(header.length + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        if bytes.len() as u64 != header.length || hash(&bytes) != header.sha256 {
            return Err("The recovery copy is damaged".into());
        }
        Ok(Claim {
            directory: self.directory.clone(),
            _lock: Some(lock),
            header,
            bytes,
        })
    }
}
pub struct Claim {
    directory: PathBuf,
    _lock: Option<File>,
    header: Header,
    bytes: Vec<u8>,
}
impl Claim {
    pub fn extension(&self) -> &str {
        &self.header.extension
    }
    pub fn origin(&self) -> Option<&Path> {
        self.header.origin.as_deref()
    }
    pub fn materialize(&self) -> Result<PathBuf, String> {
        let path = self
            .directory
            .join(format!("Recovered.{}", self.header.extension));
        let identity = if path.exists() {
            Some(suite42_core::session::FileIdentity::read(&path).map_err(|e| e.to_string())?)
        } else {
            None
        };
        suite42_core::session::publish_bundle(
            &path,
            &suite42_core::session::SaveBundle {
                main: self.bytes.clone(),
                sidecars: vec![],
            },
            identity.as_ref(),
        )
        .map_err(|e| e.to_string())?;
        Ok(path)
    }
    pub fn discard(&self) -> Result<(), String> {
        for path in [
            self.directory.join("document.off42r"),
            self.directory
                .join(format!("Recovered.{}", self.header.extension)),
        ] {
            match std::fs::remove_file(path) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.to_string()),
            }
        }
        Ok(())
    }
}
/// A document save must not target a folder that automatic recovery cleanup owns.
pub fn owns_destination(root: &Path, path: &Path) -> bool {
    let Ok(root) = std::fs::canonicalize(root) else {
        return false;
    };
    let resolved = std::fs::canonicalize(path).or_else(|_| {
        let parent = path
            .parent()
            .ok_or_else(|| std::io::Error::other("Missing destination parent"))?;
        Ok::<_, std::io::Error>(
            std::fs::canonicalize(parent)?.join(
                path.file_name()
                    .ok_or_else(|| std::io::Error::other("Missing filename"))?,
            ),
        )
    });
    let Ok(resolved) = resolved else {
        return false;
    };
    #[cfg(windows)]
    {
        let root = root.to_string_lossy().to_lowercase();
        let path = resolved.to_string_lossy().to_lowercase();
        path == root || path.starts_with(&(root + "\\"))
    }
    #[cfg(not(windows))]
    {
        resolved.starts_with(root)
    }
}
pub fn copies(root: &Path, app: &str) -> Vec<Copy> {
    if !component(app) {
        return vec![];
    }
    let Ok(entries) = std::fs::read_dir(root.join(app)) else {
        return vec![];
    };
    let mut copies = Vec::new();
    for entry in entries.take(128).flatten() {
        if !entry.file_type().is_ok_and(|t| t.is_dir())
            || !entry.file_name().to_str().is_some_and(component)
        {
            continue;
        }
        let directory = entry.path();
        let Ok(_lock) = lock(&directory.join("session.lock"), false) else {
            continue;
        };
        if let Ok((_, header)) = header(&directory.join("document.off42r"), app) {
            copies.push(Copy {
                directory,
                app: app.into(),
                header,
            });
        }
    }
    copies.sort_by_key(|copy| std::cmp::Reverse(copy.header.saved_at));
    copies
}
struct Store {
    directory: PathBuf,
    app: String,
    _lock: Option<File>,
}
impl Store {
    fn create(root: &Path, app: &str) -> Result<Self, String> {
        if !component(app) {
            return Err("Invalid recovery app".into());
        }
        let parent = root.join(app);
        std::fs::create_dir_all(&parent).map_err(|e| e.to_string())?;
        if std::fs::read_dir(&parent)
            .map_err(|e| e.to_string())?
            .take(128)
            .count()
            >= 128
        {
            return Err(
                "Recover or discard older copies before creating another recovery session".into(),
            );
        }
        let id = format!(
            "{}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|e| e.to_string())?
                .as_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        );
        let directory = parent.join(id);
        std::fs::create_dir(&directory).map_err(|e| e.to_string())?;
        let lock = lock(&directory.join("session.lock"), true)?;
        Ok(Self {
            directory,
            app: app.into(),
            _lock: Some(lock),
        })
    }
    fn write(&self, snapshot: Snapshot, origin: Option<PathBuf>) -> Result<(), String> {
        let header = Header {
            version: 1,
            app: self.app.clone(),
            extension: snapshot.extension,
            origin,
            saved_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|e| e.to_string())?
                .as_secs(),
            length: snapshot.bytes.len() as u64,
            sha256: hash(&snapshot.bytes),
        };
        if !valid(&header, &self.app) {
            return Err("Recovery snapshot exceeds limits".into());
        }
        let mut total = 0u64;
        let parent = self
            .directory
            .parent()
            .ok_or("Invalid recovery directory")?;
        for entry in std::fs::read_dir(parent)
            .map_err(|e| e.to_string())?
            .take(129)
        {
            let entry = entry.map_err(|e| e.to_string())?;
            if entry.path() != self.directory
                && entry.file_type().map_err(|e| e.to_string())?.is_dir()
            {
                if let Ok(metadata) = std::fs::metadata(entry.path().join("document.off42r")) {
                    total = total.saturating_add(metadata.len());
                }
            }
        }
        let metadata = serde_json::to_vec(&header).map_err(|e| e.to_string())?;
        if metadata.len() > MAX_HEADER {
            return Err("Recovery header exceeds limits".into());
        }
        if total
            .saturating_add(header.length)
            .saturating_add(metadata.len() as u64 + 12)
            > 1024 * 1024 * 1024
        {
            return Err("Recovery storage is full; recover or discard older copies".into());
        }
        let mut main = Vec::with_capacity(12 + metadata.len() + snapshot.bytes.len());
        main.extend_from_slice(MAGIC);
        main.extend_from_slice(&(metadata.len() as u32).to_le_bytes());
        main.extend_from_slice(&metadata);
        main.extend_from_slice(&snapshot.bytes);
        let path = self.directory.join("document.off42r");
        let identity = if path.exists() {
            Some(suite42_core::session::FileIdentity::read(&path).map_err(|e| e.to_string())?)
        } else {
            None
        };
        suite42_core::session::publish_bundle(
            &path,
            &suite42_core::session::SaveBundle {
                main,
                sidecars: vec![],
            },
            identity.as_ref(),
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    }
    fn clear(&self) -> Result<(), String> {
        match std::fs::remove_file(self.directory.join("document.off42r")) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.to_string()),
        }
    }
}
enum Command {
    Write(Snapshot, Option<PathBuf>),
    Clear,
    Finish,
}
pub struct Journal {
    tx: mpsc::Sender<Command>,
    rx: mpsc::Receiver<(bool, Result<(), String>)>,
    thread: Option<std::thread::JoinHandle<()>>,
    last: Option<String>,
    writing: bool,
    needs_clear: bool,
}
impl Journal {
    pub fn start(root: &Path, app: &str) -> Result<Self, String> {
        let store = Store::create(root, app)?;
        let (tx, commands) = mpsc::channel();
        let (results, rx) = mpsc::channel();
        let thread = std::thread::spawn(move || {
            while let Ok(command) = commands.recv() {
                let writing = matches!(&command, Command::Write(..));
                let result = match command {
                    Command::Write(snapshot, origin) => store.write(snapshot, origin),
                    Command::Clear => store.clear(),
                    Command::Finish => {
                        let _ = store.clear();
                        break;
                    }
                };
                let _ = results.send((writing, result));
            }
        });
        Ok(Self {
            tx,
            rx,
            thread: Some(thread),
            last: None,
            writing: false,
            needs_clear: false,
        })
    }
    pub fn record(&mut self, snapshot: Snapshot, origin: Option<PathBuf>) -> Result<bool, String> {
        if snapshot.bytes.is_empty() || snapshot.bytes.len() > MAX_PAYLOAD {
            return Err("Recovery snapshot exceeds 256 MB".into());
        }
        let identity = format!(
            "{}:{origin:?}:{}",
            snapshot.extension,
            hash(&snapshot.bytes)
        );
        if self.last.as_ref() == Some(&identity) {
            return Ok(false);
        }
        if self.writing {
            return Ok(false);
        }
        self.tx
            .send(Command::Write(snapshot, origin))
            .map_err(|_| "Recovery writer stopped")?;
        self.last = Some(identity);
        self.writing = true;
        self.needs_clear = true;
        Ok(true)
    }

    pub fn clear(&mut self) -> Result<(), String> {
        self.last = None;
        if !self.needs_clear {
            return Ok(());
        }
        self.tx
            .send(Command::Clear)
            .map_err(|_| "Recovery writer stopped")?;
        self.needs_clear = false;
        Ok(())
    }
    pub fn poll(&mut self) -> Option<Result<(), String>> {
        let (writing, result) = self.rx.try_recv().ok()?;
        if writing {
            self.writing = false;
        }
        if result.is_err() {
            self.last = None;
            if !writing {
                self.needs_clear = true;
            }
        }
        Some(result)
    }
}
impl Drop for Journal {
    fn drop(&mut self) {
        let _ = self.tx.send(Command::Finish);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for Store {
    fn drop(&mut self) {
        if !self.directory.join("document.off42r").exists() {
            self._lock.take();
            let _ = std::fs::remove_file(self.directory.join("session.lock"));
            let _ = std::fs::remove_dir(&self.directory);
        }
    }
}
impl Drop for Claim {
    fn drop(&mut self) {
        if !self.directory.join("document.off42r").exists() {
            self._lock.take();
            let _ = std::fs::remove_file(self.directory.join("session.lock"));
            let _ = std::fs::remove_dir(&self.directory);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn snapshot(text: &str) -> Snapshot {
        Snapshot::new("doc42", text.as_bytes().to_vec())
    }
    #[test]
    fn recovery_timestamps_are_readable_utc() {
        assert_eq!(timestamp(0), "1970-01-01 00:00 UTC");
        assert_eq!(timestamp(1700000000), "2023-11-14 22:13 UTC");
        assert_eq!(timestamp(951782400), "2000-02-29 00:00 UTC");
    }
    #[test]
    fn live_sessions_are_excluded_and_claims_are_exclusive() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("original.docx");
        std::fs::write(&source, b"ORIGINAL").unwrap();
        let store = Store::create(root.path(), "DOC42").unwrap();
        store
            .write(snapshot("UNSAVED"), Some(source.clone()))
            .unwrap();
        assert!(copies(root.path(), "DOC42").is_empty());
        drop(store);
        let copy = copies(root.path(), "DOC42").remove(0);
        let claim = copy.acquire().unwrap();
        assert!(copy.acquire().is_err());
        assert_eq!(
            std::fs::read(claim.materialize().unwrap()).unwrap(),
            b"UNSAVED"
        );
        assert_eq!(std::fs::read(&source).unwrap(), b"ORIGINAL");
        assert!(copies(root.path(), "DOC42").is_empty());
        claim.discard().unwrap();
        drop(claim);
        assert!(copies(root.path(), "DOC42").is_empty());
    }
    #[test]
    fn replacement_and_failed_capture_preserve_the_last_complete_record() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::create(root.path(), "DOC42").unwrap();
        store.write(snapshot("VERSION-1"), None).unwrap();
        store.write(snapshot("VERSION-2"), None).unwrap();
        assert!(store
            .write(Snapshot::new("../bad", b"INVALID".to_vec()), None)
            .is_err());
        drop(store);
        let claim = copies(root.path(), "DOC42").remove(0).acquire().unwrap();
        assert_eq!(claim.bytes, b"VERSION-2");
    }
    #[test]
    fn corrupt_payload_and_oversized_or_truncated_headers_are_not_restored() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::create(root.path(), "DOC42").unwrap();
        store.write(snapshot("DATA"), None).unwrap();
        let path = store.directory.join("document.off42r");
        drop(store);
        let copy = copies(root.path(), "DOC42").remove(0);
        let mut bytes = std::fs::read(&path).unwrap();
        *bytes.last_mut().unwrap() = b'!';
        std::fs::write(&path, &bytes).unwrap();
        assert!(copy.acquire().is_err());
        std::fs::write(
            &path,
            [MAGIC.as_slice(), &((MAX_HEADER + 1) as u32).to_le_bytes()].concat(),
        )
        .unwrap();
        assert!(copies(root.path(), "DOC42").is_empty());
        std::fs::write(&path, b"OFF42RC1\x10").unwrap();
        assert!(copies(root.path(), "DOC42").is_empty());
    }
    #[test]
    fn invalid_identifiers_and_other_apps_are_excluded() {
        let root = tempfile::tempdir().unwrap();
        assert!(Store::create(root.path(), "../DOC42").is_err());
        let store = Store::create(root.path(), "DOC42").unwrap();
        store.write(snapshot("DATA"), None).unwrap();
        drop(store);
        assert!(copies(root.path(), "XLS42").is_empty());
        assert_eq!(copies(root.path(), "DOC42").len(), 1);
    }
    #[test]
    fn normal_worker_shutdown_clears_backups_and_does_not_leave_empty_sessions() {
        let root = tempfile::tempdir().unwrap();
        let mut journal = Journal::start(root.path(), "DOC42").unwrap();
        assert!(journal.record(snapshot("UNSAVED"), None).unwrap());
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while journal.poll().is_none() {
            assert!(std::time::Instant::now() < deadline);
            std::thread::yield_now();
        }
        assert!(!journal.record(snapshot("UNSAVED"), None).unwrap());
        journal.clear().unwrap();
        drop(journal);
        assert_eq!(
            std::fs::read_dir(root.path().join("DOC42"))
                .unwrap()
                .count(),
            0
        );
    }
    #[test]
    fn failed_replacement_still_clears_an_older_backup_on_save_or_discard() {
        let root = tempfile::tempdir().unwrap();
        let mut journal = Journal::start(root.path(), "DOC42").unwrap();
        fn done(journal: &mut Journal) -> Result<(), String> {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
            loop {
                if let Some(result) = journal.poll() {
                    return result;
                }
                assert!(std::time::Instant::now() < deadline);
                std::thread::yield_now();
            }
        }
        assert!(journal.record(snapshot("OLDER"), None).unwrap());
        done(&mut journal).unwrap();
        assert!(journal
            .record(Snapshot::new("../bad", b"INVALID".to_vec()), None)
            .unwrap());
        assert!(done(&mut journal).is_err());
        journal.clear().unwrap();
        done(&mut journal).unwrap();
        drop(journal);
        assert_eq!(
            std::fs::read_dir(root.path().join("DOC42"))
                .unwrap()
                .count(),
            0
        );
    }
    #[test]
    fn cleanup_preserves_unrelated_files_and_save_paths_resolve_inside_the_owned_cache() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::create(root.path(), "DOC42").unwrap();
        store.write(snapshot("DATA"), None).unwrap();
        let directory = store.directory.clone();
        drop(store);
        let claim = copies(root.path(), "DOC42").remove(0).acquire().unwrap();
        let temporary = claim.materialize().unwrap();
        let unrelated = directory.join("user-file.txt");
        std::fs::write(&unrelated, b"KEEP").unwrap();
        assert!(owns_destination(root.path(), &temporary));
        assert!(owns_destination(root.path(), &directory.join("new.docx")));
        assert!(!owns_destination(
            root.path(),
            &root.path().parent().unwrap().join("outside.docx")
        ));
        claim.discard().unwrap();
        assert!(!temporary.exists());
        assert_eq!(std::fs::read(unrelated).unwrap(), b"KEEP");
    }
    #[test]
    fn quota_refusal_keeps_existing_recovery_data() {
        let root = tempfile::tempdir().unwrap();
        let older = Store::create(root.path(), "DOC42").unwrap();
        older.write(snapshot("OLDER"), None).unwrap();
        let path = older.directory.join("document.off42r");
        OpenOptions::new()
            .write(true)
            .open(&path)
            .unwrap()
            .set_len(1024 * 1024 * 1024)
            .unwrap();
        let store = Store::create(root.path(), "DOC42").unwrap();
        assert!(store
            .write(snapshot("NEW"), None)
            .unwrap_err()
            .contains("storage is full"));
        assert_eq!(std::fs::metadata(path).unwrap().len(), 1024 * 1024 * 1024);
    }
    #[cfg(unix)]
    #[test]
    fn symlinked_session_directories_and_records_are_not_followed() {
        use std::os::unix::fs::symlink;
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let store = Store::create(outside.path(), "DOC42").unwrap();
        store.write(snapshot("OUTSIDE"), None).unwrap();
        let directory = store.directory.clone();
        drop(store);
        std::fs::create_dir_all(root.path().join("DOC42")).unwrap();
        symlink(&directory, root.path().join("DOC42").join("linked")).unwrap();
        assert!(copies(root.path(), "DOC42").is_empty());
        let store = Store::create(root.path(), "DOC42").unwrap();
        symlink(
            directory.join("document.off42r"),
            store.directory.join("document.off42r"),
        )
        .unwrap();
        drop(store);
        assert!(copies(root.path(), "DOC42").is_empty());
    }
}
