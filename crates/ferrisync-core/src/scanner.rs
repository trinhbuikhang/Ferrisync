//! Parallel directory scanning and compare classification.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use jwalk::WalkDir;

use crate::error::{Error, Result};
use crate::state_store::FileRecord;

#[derive(Debug, Clone)]
pub struct FileMeta {
    pub relative_path: String,
    pub absolute_path: PathBuf,
    pub size: u64,
    pub mtime: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileAction {
    New,
    Updated,
    Unchanged,
    /// Present in state/source expectation but missing on destination — treat like New for copy.
    MissingOnDest,
}

/// Convert a filesystem modified time to unix seconds.
pub fn mtime_unix(meta: &fs::Metadata) -> Result<i64> {
    let modified = meta.modified()?;
    let duration = modified
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_err(|e| Error::Other(format!("mtime before unix epoch: {e}")))?;
    Ok(duration.as_secs() as i64)
}

/// Walk `root` and collect regular files with paths relative to `root`.
/// Uses forward slashes in relative paths for stable DB keys across platforms.
pub fn scan_files(root: impl AsRef<Path>) -> Result<Vec<FileMeta>> {
    let root = root.as_ref();
    if !root.exists() {
        return Err(Error::Other(format!(
            "source path does not exist: {}",
            root.display()
        )));
    }

    let mut files = Vec::new();
    for entry in WalkDir::new(root).parallelism(jwalk::Parallelism::RayonDefaultPool {
        busy_timeout: std::time::Duration::from_millis(100),
    }) {
        let entry = entry.map_err(|e| Error::Other(format!("walk error: {e}")))?;
        if !entry.file_type().is_file() {
            continue;
        }
        let abs = entry.path();
        let rel = abs
            .strip_prefix(root)
            .map_err(|e| Error::Other(format!("strip prefix failed: {e}")))?;
        let relative_path = normalize_rel_path(rel);
        let meta = fs::metadata(&abs).map_err(|e| Error::io(&abs, e))?;
        files.push(FileMeta {
            relative_path,
            absolute_path: abs,
            size: meta.len(),
            mtime: mtime_unix(&meta)?,
        });
    }
    Ok(files)
}

pub fn normalize_rel_path(path: &Path) -> String {
    path.components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

/// Classify whether a source file needs copying based on size+mtime and optional state.
pub fn classify(
    source: &FileMeta,
    dest_meta: Option<&FileMeta>,
    state: Option<&FileRecord>,
) -> FileAction {
    match dest_meta {
        None => {
            if state.is_some() {
                FileAction::MissingOnDest
            } else {
                FileAction::New
            }
        }
        Some(dest) => {
            let meta_matches = dest.size == source.size && dest.mtime == source.mtime;
            if let Some(rec) = state {
                let state_matches = rec.size as u64 == source.size && rec.mtime == source.mtime;
                if meta_matches
                    && state_matches
                    && matches!(
                        rec.status,
                        crate::state_store::FileStatus::Verified
                            | crate::state_store::FileStatus::Synced
                    )
                {
                    return FileAction::Unchanged;
                }
                if !meta_matches || !state_matches {
                    return FileAction::Updated;
                }
                return FileAction::Unchanged;
            }
            if meta_matches {
                FileAction::Unchanged
            } else {
                FileAction::Updated
            }
        }
    }
}

/// Read metadata for a destination file if it exists.
pub fn dest_file_meta(dest_root: &Path, relative_path: &str) -> Result<Option<FileMeta>> {
    let abs = join_rel(dest_root, relative_path);
    if !abs.exists() {
        return Ok(None);
    }
    let meta = fs::metadata(&abs).map_err(|e| Error::io(&abs, e))?;
    if !meta.is_file() {
        return Ok(None);
    }
    Ok(Some(FileMeta {
        relative_path: relative_path.to_string(),
        absolute_path: abs,
        size: meta.len(),
        mtime: mtime_unix(&meta)?,
    }))
}

pub fn join_rel(root: &Path, relative_path: &str) -> PathBuf {
    let mut out = root.to_path_buf();
    for part in relative_path.split('/') {
        if !part.is_empty() && part != "." && part != ".." {
            out.push(part);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state_store::{FileRecord, FileStatus};

    fn meta(path: &str, size: u64, mtime: i64) -> FileMeta {
        FileMeta {
            relative_path: path.into(),
            absolute_path: PathBuf::from(path),
            size,
            mtime,
        }
    }

    fn record(size: i64, mtime: i64, status: FileStatus) -> FileRecord {
        FileRecord {
            pair_id: "p".into(),
            path: "f.bin".into(),
            size,
            mtime,
            source_hash: None,
            dest_hash: None,
            synced_at: Some(1),
            verified_at: Some(1),
            deleted_at: None,
            status,
        }
    }

    #[test]
    fn classify_new_when_no_dest() {
        let src = meta("a.bin", 10, 1);
        assert_eq!(classify(&src, None, None), FileAction::New);
    }

    #[test]
    fn classify_missing_on_dest_when_state_exists() {
        let src = meta("a.bin", 10, 1);
        let state = record(10, 1, FileStatus::Verified);
        assert_eq!(
            classify(&src, None, Some(&state)),
            FileAction::MissingOnDest
        );
    }

    #[test]
    fn classify_unchanged_when_size_mtime_and_state_match() {
        let src = meta("a.bin", 10, 5);
        let dest = meta("a.bin", 10, 5);
        let state = record(10, 5, FileStatus::Verified);
        assert_eq!(
            classify(&src, Some(&dest), Some(&state)),
            FileAction::Unchanged
        );
    }

    #[test]
    fn classify_updated_when_mtime_changes() {
        let src = meta("a.bin", 10, 9);
        let dest = meta("a.bin", 10, 5);
        let state = record(10, 5, FileStatus::Verified);
        assert_eq!(
            classify(&src, Some(&dest), Some(&state)),
            FileAction::Updated
        );
    }

    #[test]
    fn scan_finds_nested_files() {
        let dir = tempfile::tempdir().unwrap();
        let nested = dir.path().join("sub");
        fs::create_dir_all(&nested).unwrap();
        fs::write(nested.join("x.txt"), b"hi").unwrap();
        let files = scan_files(dir.path()).unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].relative_path, "sub/x.txt");
    }
}
