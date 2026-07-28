//! Compare-only scan: classify source files against destination + optional state.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::config::Config;
use crate::error::Result;
use crate::scanner::{classify, dest_file_meta, scan_files, FileAction};
use crate::state_store::StateStore;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompareAction {
    New,
    Updated,
    Unchanged,
    MissingOnDest,
}

impl From<FileAction> for CompareAction {
    fn from(value: FileAction) -> Self {
        match value {
            FileAction::New => Self::New,
            FileAction::Updated => Self::Updated,
            FileAction::Unchanged => Self::Unchanged,
            FileAction::MissingOnDest => Self::MissingOnDest,
        }
    }
}

impl CompareAction {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::New => "new",
            Self::Updated => "updated",
            Self::Unchanged => "unchanged",
            Self::MissingOnDest => "missing_on_dest",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompareRow {
    pub relative_path: String,
    pub action: CompareAction,
    pub source_size: Option<u64>,
    pub dest_size: Option<u64>,
    pub source_mtime: Option<i64>,
    pub dest_mtime: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompareResult {
    pub rows: Vec<CompareRow>,
    pub scanned: u64,
    pub to_copy: u64,
    pub unchanged: u64,
}

/// Compare source tree to destination (and state DB when provided).
pub fn compare_pair(
    source: impl AsRef<Path>,
    destination: impl AsRef<Path>,
    store: Option<&StateStore>,
    pair_id: Option<&str>,
) -> Result<CompareResult> {
    let source = source.as_ref();
    let destination = destination.as_ref();
    let files = scan_files(source)?;
    let scanned = files.len() as u64;
    let mut rows = Vec::with_capacity(files.len());
    let mut to_copy = 0u64;
    let mut unchanged = 0u64;

    for meta in files {
        let dest_meta = dest_file_meta(destination, &meta.relative_path)?;
        let state = match (store, pair_id) {
            (Some(store), Some(id)) => store.get_file(id, &meta.relative_path)?,
            _ => None,
        };
        let action = CompareAction::from(classify(
            &meta,
            dest_meta.as_ref(),
            state.as_ref(),
        ));
        match action {
            CompareAction::Unchanged => unchanged += 1,
            CompareAction::New | CompareAction::Updated | CompareAction::MissingOnDest => {
                to_copy += 1
            }
        }
        rows.push(CompareRow {
            relative_path: meta.relative_path,
            action,
            source_size: Some(meta.size),
            dest_size: dest_meta.as_ref().map(|d| d.size),
            source_mtime: Some(meta.mtime),
            dest_mtime: dest_meta.as_ref().map(|d| d.mtime),
        });
    }

    rows.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));

    Ok(CompareResult {
        rows,
        scanned,
        to_copy,
        unchanged,
    })
}

/// Convenience: build session config, open store, compare.
pub fn compare_folders(source: impl AsRef<Path>, destination: impl AsRef<Path>) -> Result<CompareResult> {
    let config = Config::single_pair(source.as_ref().to_path_buf(), destination.as_ref().to_path_buf());
    let _ = config.save(crate::config::default_gui_session_path());
    let store = StateStore::open(config.state_db_path())?;
    let pair = &config.pairs[0];
    store.upsert_pair(&pair.id, &pair.source, &pair.destination)?;
    compare_pair(&pair.source, &pair.destination, Some(&store), Some(&pair.id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use assert_fs::prelude::*;
    use assert_fs::TempDir;
    use std::fs;

    #[test]
    fn compare_detects_new_and_unchanged() {
        let tmp = TempDir::new().unwrap();
        let src = tmp.child("src");
        let dst = tmp.child("dst");
        src.create_dir_all().unwrap();
        dst.create_dir_all().unwrap();
        src.child("a.txt").write_str("aaa").unwrap();
        src.child("b.txt").write_str("bbb").unwrap();
        fs::copy(src.child("a.txt").path(), dst.child("a.txt").path()).unwrap();

        let result = compare_pair(src.path(), dst.path(), None, None).unwrap();
        assert_eq!(result.scanned, 2);
        let a = result
            .rows
            .iter()
            .find(|r| r.relative_path == "a.txt")
            .unwrap();
        let b = result
            .rows
            .iter()
            .find(|r| r.relative_path == "b.txt")
            .unwrap();
        assert_ne!(a.action, CompareAction::New);
        assert_eq!(b.action, CompareAction::New);
        assert!(result.to_copy >= 1);
    }
}
