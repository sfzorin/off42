use crate::{session::FileIdentity, win32_path};
use std::{
    io::{ErrorKind, Write},
    path::{Path, PathBuf},
};

/// Stage all output in the destination directory before publishing any file.
/// On a partial publication failure, only remove outputs that still match our bytes.
pub fn publish_new_files(
    paths: Vec<PathBuf>,
    label: &str,
    mut encode: impl FnMut(usize) -> Result<Vec<u8>, String>,
) -> Result<Vec<PathBuf>, String> {
    let paths: Vec<_> = paths
        .into_iter()
        .map(|path| win32_path(&path))
        .collect::<Result<_, _>>()
        .map_err(|error| error.to_string())?;
    let directory = paths
        .first()
        .and_then(|path| path.parent())
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    for path in &paths {
        if path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."))
            != directory
        {
            return Err("Batch export destinations must share a directory".into());
        }
        match std::fs::symlink_metadata(path) {
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Ok(_) => {
                return Err(format!(
                    "{label} export destination exists: {}",
                    path.display()
                ))
            }
            Err(error) => return Err(error.to_string()),
        }
    }

    let mut prepared = Vec::with_capacity(paths.len());
    for index in 0..paths.len() {
        let bytes = encode(index)?;
        let mut temp =
            tempfile::NamedTempFile::new_in(directory).map_err(|error| error.to_string())?;
        temp.write_all(&bytes)
            .and_then(|()| temp.as_file().sync_all())
            .map_err(|error| error.to_string())?;
        let modified = temp
            .as_file()
            .metadata()
            .and_then(|meta| meta.modified())
            .ok();
        prepared.push((temp, FileIdentity::from_bytes(&bytes, modified)));
    }

    let mut published: Vec<(&Path, FileIdentity)> = Vec::new();
    for (path, (temp, identity)) in paths.iter().zip(prepared) {
        if let Err(error) = temp.persist_noclobber(path) {
            let mut issues = Vec::new();
            for (path, identity) in published.into_iter().rev() {
                if !matches!(FileIdentity::read(path), Ok(current) if current == identity)
                    || !identity.metadata_matches(path).unwrap_or(false)
                    || std::fs::remove_file(path).is_err()
                {
                    issues.push(path.display().to_string());
                }
            }
            if issues.is_empty() {
                return Err(error.error.to_string());
            }
            return Err(format!(
                "{}. Could not roll back: {}",
                error.error,
                issues.join(", ")
            ));
        }
        published.push((path.as_path(), identity));
    }
    Ok(paths)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn late_collision_rolls_back_only_the_published_batch_files() {
        let directory = tempfile::tempdir().unwrap();
        let first = directory.path().join("slide-001.png");
        let second = directory.path().join("slide-002.png");
        let result = publish_new_files(
            vec![first.clone(), second.clone()],
            "Slide image",
            |index| {
                if index == 1 {
                    std::fs::write(&second, b"other process").unwrap();
                }
                Ok(b"export".to_vec())
            },
        );
        assert!(result.is_err());
        assert!(!first.exists());
        assert_eq!(std::fs::read(second).unwrap(), b"other process");
    }
}
