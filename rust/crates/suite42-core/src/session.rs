//! Document location and saved revision tracking. Identity is not an atomic CAS.
use std::{
    fs, io,
    path::{Path, PathBuf},
    time::SystemTime,
};
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileIdentity {
    pub len: u64,
    pub modified: Option<SystemTime>,
    pub content_hash: u64,
}
impl FileIdentity {
    /// Build the same identity from the exact bytes decoded by a worker.
    pub fn from_bytes(bytes: &[u8], modified: Option<SystemTime>) -> Self {
        use std::hash::Hasher;
        let mut hash = std::collections::hash_map::DefaultHasher::new();
        for chunk in bytes.chunks(65536) {
            hash.write(chunk);
        }
        Self {
            len: bytes.len() as u64,
            modified,
            content_hash: hash.finish(),
        }
    }
    /// Cheap final guard after a full background identity check. Like publication,
    /// this is not a filesystem compare-and-swap against concurrent writers.
    pub fn metadata_matches(&self, path: &Path) -> io::Result<bool> {
        let metadata = fs::metadata(path)?;
        Ok(metadata.len() == self.len && metadata.modified().ok() == self.modified)
    }
    pub fn read(path: &Path) -> io::Result<Self> {
        let m = fs::metadata(path)?;
        use std::{hash::Hasher, io::Read};
        let mut file = fs::File::open(path)?;
        let mut hash = std::collections::hash_map::DefaultHasher::new();
        let mut buffer = [0u8; 65536];
        loop {
            let n = file.read(&mut buffer)?;
            if n == 0 {
                break;
            }
            hash.write(&buffer[..n]);
        }
        Ok(Self {
            len: m.len(),
            modified: m.modified().ok(),
            content_hash: hash.finish(),
        })
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DocumentLocation {
    pub path: PathBuf,
    pub format: String,
    pub identity: FileIdentity,
}
#[derive(Clone, Debug)]
pub struct SaveSnapshot {
    pub revision: u64,
    pub bytes: Vec<u8>,
}
#[derive(Clone, Debug, Default)]
pub struct DocumentSession {
    location: Option<DocumentLocation>,
    saved_revision: Option<u64>,
}
impl DocumentSession {
    pub fn opened(path: PathBuf, format: String, revision: u64) -> io::Result<Self> {
        let identity = FileIdentity::read(&path)?;
        Ok(Self::opened_with_identity(path, format, revision, identity))
    }
    pub fn opened_with_identity(
        path: PathBuf,
        format: String,
        revision: u64,
        identity: FileIdentity,
    ) -> Self {
        Self {
            location: Some(DocumentLocation {
                path,
                format,
                identity,
            }),
            saved_revision: Some(revision),
        }
    }
    pub fn location(&self) -> Option<&DocumentLocation> {
        self.location.as_ref()
    }
    pub fn saved_revision(&self) -> Option<u64> {
        self.saved_revision
    }
    pub fn externally_changed(&self) -> io::Result<bool> {
        match &self.location {
            Some(l) => match FileIdentity::read(&l.path) {
                Ok(now) => Ok(now != l.identity),
                Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(true),
                Err(e) => Err(e),
            },
            None => Ok(false),
        }
    }
    /// Record only after successful publication; later edits remain dirty.
    pub fn published(&mut self, path: PathBuf, format: String, revision: u64) -> io::Result<()> {
        let identity = FileIdentity::read(&path)?;
        self.location = Some(DocumentLocation {
            path,
            format,
            identity,
        });
        self.saved_revision = Some(revision);
        Ok(())
    }
    pub fn is_dirty(&self, revision: u64) -> bool {
        self.saved_revision != Some(revision)
    }
}

/// Write and flush a temporary sibling before publishing it. Existing files are
/// replaced only if their last observed identity matches. The check and replace
/// are separate filesystem operations and do not provide atomic compare-and-swap.
pub fn publish(
    path: &Path,
    bytes: &[u8],
    expected: Option<&FileIdentity>,
) -> io::Result<FileIdentity> {
    publish_bytes(path, bytes, expected)?;
    FileIdentity::read(path)
}
fn publish_bytes(path: &Path, bytes: &[u8], expected: Option<&FileIdentity>) -> io::Result<()> {
    use std::io::Write;
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let mut temporary = tempfile::NamedTempFile::new_in(crate::win32_path(parent)?)?;
    temporary.write_all(bytes)?;
    temporary.flush()?;
    temporary.as_file().sync_all()?;
    match expected {
        None => {
            temporary
                .persist_noclobber(crate::win32_path(path)?)
                .map_err(|e| e.error)?;
        }
        Some(expected) => {
            if FileIdentity::read(path)? != *expected {
                return Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    "Destination changed since it was opened or confirmed",
                ));
            }
            // ReplaceFileW opens the replacement with exclusive access. Close the
            // NamedTempFile handle first while retaining TempPath's cleanup guard.
            let temporary_path = temporary.into_temp_path();
            replace_existing(&temporary_path, path)?;
        }
    }
    Ok(())
}
/// A main document and immutable, sibling-relative resources. Publication is
/// ordered, not a filesystem-wide transaction: resources appear before the
/// main file, and rollback is best effort if publication fails.
#[derive(Clone, Debug)]
pub struct SaveBundle {
    pub main: Vec<u8>,
    pub sidecars: Vec<SaveSidecar>,
}
#[derive(Clone, Debug)]
pub struct SaveSidecar {
    /// Exactly `<document-stem>.assets/<name>`; absolute paths and traversal
    /// are rejected by `publish_bundle`.
    pub relative_path: PathBuf,
    pub bytes: Vec<u8>,
}

/// Publish resources without replacing existing files, then publish the main
/// snapshot using `publish`'s conflict check. Identical resources may be reused.
/// If a later step fails, only resources created by this call and still matching
/// their published identity are removed. Concurrent filesystem changes can make
/// rollback incomplete; errors from cleanup are reported with the original error.
pub fn publish_bundle(
    path: &Path,
    bundle: &SaveBundle,
    expected: Option<&FileIdentity>,
) -> io::Result<FileIdentity> {
    use std::path::Component;
    let stem = path
        .file_stem()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "Document name has no stem"))?;
    let mut assets_name = stem.to_os_string();
    assets_name.push(".assets");
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let assets = parent.join(&assets_name);
    let mut seen = std::collections::HashSet::new();
    for sidecar in &bundle.sidecars {
        let mut components = sidecar.relative_path.components();
        if !matches!(components.next(), Some(Component::Normal(n)) if n == assets_name)
            || !matches!(components.next(), Some(Component::Normal(name)) if name.to_str().is_some_and(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.')) && !s.ends_with('.')))
            || components.next().is_some()
            || !seen.insert(sidecar.relative_path.clone())
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Invalid or duplicate sidecar path",
            ));
        }
    }
    let mut created_dir = false;
    let mut created: Vec<(PathBuf, FileIdentity)> = Vec::new();
    let mut main_published = false;
    let result = (|| {
        if !bundle.sidecars.is_empty() {
            match fs::create_dir(&assets) {
                Ok(()) => created_dir = true,
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
                    let meta = fs::symlink_metadata(&assets)?;
                    if !meta.is_dir() || meta.file_type().is_symlink() {
                        return Err(io::Error::new(
                            io::ErrorKind::AlreadyExists,
                            "Sidecar directory is not a real directory",
                        ));
                    }
                }
                Err(e) => return Err(e),
            }
        }
        for sidecar in &bundle.sidecars {
            let target = parent.join(&sidecar.relative_path);
            match publish_bytes(&target, &sidecar.bytes, None) {
                Ok(()) => {
                    let identity = FileIdentity::read(&target).map_err(|e| io::Error::new(e.kind(), format!("Sidecar was published but its identity could not be read; it was retained: {e}")))?;
                    created.push((target, identity));
                }
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
                    let meta = fs::symlink_metadata(&target)?;
                    if !meta.is_file()
                        || meta.file_type().is_symlink()
                        || meta.len() != sidecar.bytes.len() as u64
                        || fs::read(&target)? != sidecar.bytes
                    {
                        return Err(io::Error::new(
                            io::ErrorKind::AlreadyExists,
                            "Existing sidecar differs from the snapshot",
                        ));
                    }
                }
                Err(e) => return Err(e),
            }
        }
        publish_bytes(path, &bundle.main, expected)?;
        main_published = true;
        FileIdentity::read(path)
    })();
    match result {
        Ok(identity) => Ok(identity),
        Err(original) if main_published => Err(io::Error::new(original.kind(), format!("Main file was published but its identity could not be read; sidecars were retained: {original}"))),
        Err(original) => {
            let mut failures = Vec::new();
            for (file, identity) in created.into_iter().rev() {
                match FileIdentity::read(&file) {
                    Ok(current) if current == identity => {
                        if let Err(e) = fs::remove_file(&file) {
                            failures.push(e.to_string());
                        }
                    }
                    Ok(_) => failures.push(format!(
                        "resource changed during rollback: {}",
                        file.display()
                    )),
                    Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                    Err(e) => failures.push(e.to_string()),
                }
            }
            if created_dir {
                if let Err(e) = fs::remove_dir(&assets) {
                    if e.kind() != io::ErrorKind::NotFound {
                        failures.push(e.to_string());
                    }
                }
            }
            if failures.is_empty() {
                Err(original)
            } else {
                Err(io::Error::new(
                    original.kind(),
                    format!("{original}; rollback incomplete: {}", failures.join("; ")),
                ))
            }
        }
    }
}

#[cfg(not(windows))]
fn replace_existing(source: &Path, destination: &Path) -> io::Result<()> {
    fs::rename(source, destination)
}
#[cfg(windows)]
fn replace_existing(source: &Path, destination: &Path) -> io::Result<()> {
    use std::{os::windows::ffi::OsStrExt, ptr::null};
    use windows_sys::Win32::Storage::FileSystem::{
        ReplaceFileW, SetFileAttributesW, FILE_ATTRIBUTE_NORMAL,
    };
    let (source, destination) = (crate::win32_path(source)?, crate::win32_path(destination)?);
    let src: Vec<u16> = source.as_os_str().encode_wide().chain(Some(0)).collect();
    let dst: Vec<u16> = destination
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect();
    // NamedTempFile creates Windows files with FILE_ATTRIBUTE_TEMPORARY;
    // normalize it before publication, as tempfile's own persist does.
    if unsafe { SetFileAttributesW(src.as_ptr(), FILE_ATTRIBUTE_NORMAL) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let ok = unsafe { ReplaceFileW(dst.as_ptr(), src.as_ptr(), null(), 0, null(), null()) };
    if ok == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn loaded_bytes_identity_matches_file_and_attaches_without_reread() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("source.pdf");
        let bytes = vec![42u8; 70_000];
        fs::write(&path, &bytes).unwrap();
        let modified = fs::metadata(&path).unwrap().modified().ok();
        let captured = FileIdentity::from_bytes(&bytes, modified);
        assert_eq!(captured, FileIdentity::read(&path).unwrap());
        assert!(captured.metadata_matches(&path).unwrap());
        let session =
            DocumentSession::opened_with_identity(path.clone(), "pdf".into(), 7, captured.clone());
        assert_eq!(session.location().unwrap().identity, captured);
        fs::write(&path, b"changed").unwrap();
        assert!(!captured.metadata_matches(&path).unwrap());
        assert!(session.externally_changed().unwrap());
    }
    #[test]
    fn bundle_reuses_identical_assets_and_rolls_back_owned_files_on_conflict() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("example.md");
        let asset = PathBuf::from("example.assets/abc.png");
        let bundle = SaveBundle {
            main: b"first".to_vec(),
            sidecars: vec![SaveSidecar {
                relative_path: asset.clone(),
                bytes: b"image".to_vec(),
            }],
        };
        let first = publish_bundle(&path, &bundle, None).unwrap();
        let reused = SaveBundle {
            main: b"second".to_vec(),
            sidecars: bundle.sidecars.clone(),
        };
        let second = publish_bundle(&path, &reused, Some(&first)).unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"second");
        assert_eq!(fs::read(dir.path().join(&asset)).unwrap(), b"image");
        let collision = SaveBundle {
            main: b"third".to_vec(),
            sidecars: vec![SaveSidecar {
                relative_path: asset.clone(),
                bytes: b"different".to_vec(),
            }],
        };
        assert!(publish_bundle(&path, &collision, Some(&second)).is_err());
        assert_eq!(fs::read(dir.path().join(&asset)).unwrap(), b"image");
        let new_asset = PathBuf::from("example.assets/new.png");
        let conflict = SaveBundle {
            main: b"third".to_vec(),
            sidecars: vec![SaveSidecar {
                relative_path: new_asset.clone(),
                bytes: b"new".to_vec(),
            }],
        };
        assert!(publish_bundle(&path, &conflict, Some(&first)).is_err());
        assert!(!dir.path().join(new_asset).exists());
        assert_eq!(fs::read(&path).unwrap(), b"second");
        assert_eq!(fs::read(dir.path().join(asset)).unwrap(), b"image");
        let traversal = SaveBundle {
            main: vec![],
            sidecars: vec![SaveSidecar {
                relative_path: "../other".into(),
                bytes: vec![],
            }],
        };
        assert!(publish_bundle(&path, &traversal, None).is_err());
    }
    #[test]
    fn publication_rejects_conflicts_and_tracks_snapshot_revision() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("document");
        let first = publish(&path, b"original", None).unwrap();
        let mut session = DocumentSession::opened(path.clone(), "test".into(), 1).unwrap();
        assert!(!session.is_dirty(1));
        assert!(publish(&path, b"no clobber", None).is_err());
        fs::write(&path, b"external modification").unwrap();
        assert!(session.externally_changed().unwrap());
        assert!(publish(&path, b"edited", Some(&first)).is_err());
        assert_eq!(fs::read(&path).unwrap(), b"external modification");
        let confirmed = FileIdentity::read(&path).unwrap();
        publish(&path, b"edited", Some(&confirmed)).unwrap();
        session.published(path.clone(), "test".into(), 2).unwrap();
        assert!(!session.is_dirty(2));
        assert!(session.is_dirty(3));
        assert_eq!(fs::read(path).unwrap(), b"edited");
    }
}
