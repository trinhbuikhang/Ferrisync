//! Tracing helpers and JSONL audit logging.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::error::{Error, Result};

#[derive(Debug)]
pub struct AuditLog {
    path: PathBuf,
    file: Mutex<File>,
}

impl AuditLog {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(|e| Error::io(&path, e))?;
        Ok(Self {
            path,
            file: Mutex::new(file),
        })
    }

    pub fn write_event(
        &self,
        action: &str,
        pair_id: &str,
        path: &str,
        reason: &str,
        dry_run: bool,
    ) -> Result<()> {
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        // Minimal JSON without pulling in serde_json as a hard dep for every build —
        // escape manually for controlled fields.
        let line = format!(
            "{{\"ts\":{ts},\"action\":\"{}\",\"pair_id\":\"{}\",\"path\":\"{}\",\"reason\":\"{}\",\"dry_run\":{}}}\n",
            escape_json(action),
            escape_json(pair_id),
            escape_json(path),
            escape_json(reason),
            if dry_run { "true" } else { "false" },
        );
        let mut guard = self
            .file
            .lock()
            .map_err(|_| Error::Other("audit log mutex poisoned".into()))?;
        guard
            .write_all(line.as_bytes())
            .map_err(|e| Error::io(&self.path, e))?;
        guard.flush().map_err(|e| Error::io(&self.path, e))?;
        Ok(())
    }
}

fn escape_json(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
}

#[derive(Debug, Default, Clone)]
pub struct ProgressSnapshot {
    pub scanned: u64,
    pub copied: u64,
    pub skipped: u64,
    pub errors: u64,
    pub bytes_copied: u64,
}
