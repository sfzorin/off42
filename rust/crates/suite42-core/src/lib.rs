//! Shared bounded history and non-destructive file publication.
use std::{
    io::{self, Write},
    path::{Path, PathBuf},
};

/// Path for Win32 move/replace calls (tempfile's persist, ReplaceFileW), which
/// stop at MAX_PATH unless the path is verbatim; std's own file APIs add the
/// prefix themselves. Apply it to both the temporary's directory and the
/// destination, since the move reads both paths. Elsewhere the path is unchanged.
pub fn win32_path(path: &Path) -> io::Result<PathBuf> {
    #[cfg(windows)]
    {
        use std::{
            ffi::OsString,
            os::windows::ffi::{OsStrExt, OsStringExt},
            path::{Component, Prefix},
        };
        let absolute = std::path::absolute(path)?;
        let prefix: &str = match absolute.components().next() {
            Some(Component::Prefix(p)) => match p.kind() {
                Prefix::Disk(_) => r"\\?\",
                Prefix::UNC(..) => r"\\?\UNC",
                _ => return Ok(absolute),
            },
            _ => return Ok(absolute),
        };
        // `\\server\share` becomes `\\?\UNC\server\share`.
        let skip = if prefix.ends_with("UNC") { 1 } else { 0 };
        let wide: Vec<u16> = prefix
            .encode_utf16()
            .chain(absolute.as_os_str().encode_wide().skip(skip))
            .collect();
        Ok(OsString::from_wide(&wide).into())
    }
    #[cfg(not(windows))]
    Ok(path.to_path_buf())
}

/// Raw text and the line-ending policy retained across editor transactions.
/// Files with no newline cannot encode their former policy on disk.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TextValue {
    raw: String,
    crlf: bool,
}
impl TextValue {
    pub fn from_raw(raw: String) -> Self {
        let crlf = raw.contains("\r\n");
        Self { raw, crlf }
    }
    pub fn raw(&self) -> &str {
        &self.raw
    }
    /// Reject a policy that contradicts an unambiguous newline style.
    /// Mixed styles and lone CR are preserved for the host to handle.
    pub fn policy_consistent(&self) -> bool {
        let bytes = self.raw.as_bytes();
        let mut crlf = false;
        let mut lf = false;
        for (i, byte) in bytes.iter().enumerate() {
            if *byte == b'\n' {
                if i > 0 && bytes[i - 1] == b'\r' {
                    crlf = true;
                } else {
                    lf = true;
                }
            }
        }
        match (crlf, lf) {
            (true, false) => self.crlf,
            (false, true) => !self.crlf,
            _ => true,
        }
    }
    /// Accept LF-normalized or CRLF editor text, retaining this value's policy.
    pub fn with_edited(&self, text: &str) -> Self {
        let canonical = text.replace("\r\n", "\n");
        let raw = if self.crlf {
            canonical.replace('\n', "\r\n")
        } else {
            canonical
        };
        Self {
            raw,
            crlf: self.crlf,
        }
    }
}
impl Default for TextValue {
    fn default() -> Self {
        Self::from_raw(String::new())
    }
}

pub struct SnapshotHistory<T> {
    current: T,
    saved: T,
    undo: Vec<T>,
    redo: Vec<T>,
    limit: usize,
}
impl<T: Clone + PartialEq> SnapshotHistory<T> {
    pub fn new(initial: T, limit: usize) -> Self {
        Self {
            current: initial.clone(),
            saved: initial,
            undo: Vec::new(),
            redo: Vec::new(),
            limit,
        }
    }
    /// Replace current state without changing history or saved state; may change dirty status.
    pub fn replace_current(&mut self, value: T) {
        self.current = value;
    }
    pub fn current(&self) -> &T {
        &self.current
    }
    fn push(stack: &mut Vec<T>, value: T, limit: usize) {
        if limit > 0 {
            if stack.len() == limit {
                stack.remove(0);
            }
            stack.push(value);
        }
    }
    pub fn apply(&mut self, value: T) -> bool {
        if value == self.current {
            return false;
        }
        Self::push(&mut self.undo, self.current.clone(), self.limit);
        self.current = value;
        self.redo.clear();
        true
    }
    pub fn undo(&mut self) -> bool {
        if let Some(value) = self.undo.pop() {
            Self::push(&mut self.redo, self.current.clone(), self.limit);
            self.current = value;
            true
        } else {
            false
        }
    }
    pub fn redo(&mut self) -> bool {
        if let Some(value) = self.redo.pop() {
            Self::push(&mut self.undo, self.current.clone(), self.limit);
            self.current = value;
            true
        } else {
            false
        }
    }
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }
    pub fn discard_redo(&mut self) {
        self.redo.clear();
    }
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }
    pub fn is_dirty(&self) -> bool {
        self.current != self.saved
    }
    pub fn mark_saved(&mut self) {
        self.saved = self.current.clone();
    }
    pub fn snapshot(&self) -> T {
        self.current.clone()
    }
    pub fn mark_saved_snapshot(&mut self, snapshot: T) {
        self.saved = snapshot;
    }
}
/// Write to a new destination only, without clobbering any existing file.
/// Publication atomicity depends on the platform and filesystem.
pub fn write_new(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let mut temp = tempfile::NamedTempFile::new_in(win32_path(parent)?)?;
    temp.write_all(bytes)?;
    temp.flush()?;
    temp.as_file().sync_all()?;
    temp.persist_noclobber(win32_path(path)?)
        .map(|_| ())
        .map_err(|e| e.error)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn write_new_publishes_beyond_max_path() {
        let root = tempfile::tempdir().unwrap();
        let mut folder = root.path().to_path_buf();
        for n in 0..5 {
            folder.push(format!("{}{}", "long-path-component-".repeat(3), n));
        }
        std::fs::create_dir_all(&folder).unwrap();
        let path = folder.join("note.txt");
        assert!(path.to_string_lossy().len() > 260);
        write_new(&path, b"42").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"42");
        assert!(write_new(&path, b"43").is_err());
    }
    #[cfg(windows)]
    #[test]
    fn win32_path_is_verbatim_for_disk_and_unc() {
        assert_eq!(
            win32_path(Path::new(r"C:\a\..\b\c.txt")).unwrap(),
            PathBuf::from(r"\\?\C:\b\c.txt")
        );
        assert_eq!(
            win32_path(Path::new(r"\\server\share\d.txt")).unwrap(),
            PathBuf::from(r"\\?\UNC\server\share\d.txt")
        );
        let verbatim = PathBuf::from(r"\\?\C:\e.txt");
        assert_eq!(win32_path(&verbatim).unwrap(), verbatim);
    }
    #[test]
    fn bounded_history_and_saved_state() {
        let mut h = SnapshotHistory::new(1, 2);
        assert!(!h.apply(1));
        assert!(h.apply(2));
        h.apply(3);
        h.apply(4);
        assert!(h.undo());
        assert!(h.undo());
        assert!(!h.undo());
        assert_eq!(*h.current(), 2);
        h.mark_saved();
        assert!(!h.is_dirty());
        assert!(h.redo());
        assert!(h.is_dirty());
        assert!(h.undo());
        assert!(!h.is_dirty());
        h.apply(5);
        assert!(!h.can_redo());
    }
    #[test]
    fn write_new_preserves_existing() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("a");
        write_new(&p, b"one").unwrap();
        assert_eq!(
            write_new(&p, b"two").unwrap_err().kind(),
            io::ErrorKind::AlreadyExists
        );
        assert_eq!(std::fs::read(p).unwrap(), b"one");
    }
}

pub mod session;
