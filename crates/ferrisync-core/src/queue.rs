//! Work queue job types for sync and retention.

use std::path::PathBuf;

use crate::scanner::FileMeta;

#[derive(Debug, Clone)]
pub struct SyncJob {
    pub pair_id: String,
    pub relative_path: String,
    pub source_path: PathBuf,
    pub dest_root: PathBuf,
    pub size: u64,
    pub mtime: i64,
}

impl SyncJob {
    pub fn from_meta(pair_id: &str, dest_root: &std::path::Path, meta: &FileMeta) -> Self {
        Self {
            pair_id: pair_id.to_string(),
            relative_path: meta.relative_path.clone(),
            source_path: meta.absolute_path.clone(),
            dest_root: dest_root.to_path_buf(),
            size: meta.size,
            mtime: meta.mtime,
        }
    }
}

#[derive(Debug, Clone)]
pub struct RetentionJob {
    pub pair_id: String,
    pub relative_path: String,
    pub source_path: PathBuf,
}
