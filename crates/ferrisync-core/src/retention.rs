//! Retention cleanup: quarantine or permanent delete after verified retention window.

use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use tracing::{info, warn};

use crate::config::{PairConfig, RetentionConfig, RetentionMode};
use crate::error::{Error, Result};
use crate::hasher::{hash_file, hex_encode};
use crate::logging::AuditLog;
use crate::scanner::{join_rel, mtime_unix};
use crate::state_store::{FileRecord, FileStatus, StateStore};

#[derive(Debug, Clone)]
pub struct RetentionOptions {
    pub config: RetentionConfig,
    pub now_unix: i64,
}

impl RetentionOptions {
    pub fn from_config(config: RetentionConfig) -> Self {
        Self {
            config,
            now_unix: now_unix(),
        }
    }
}

#[derive(Debug, Default, Clone)]
pub struct CleanupStats {
    pub candidates: u64,
    pub deleted_or_quarantined: u64,
    pub skipped: u64,
    pub dry_run_would_act: u64,
}

#[derive(Debug, Default, Clone)]
pub struct PurgeStats {
    pub purged: u64,
    pub skipped: u64,
    pub dry_run_would_act: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RetentionDecision {
    Qualify,
    Skip { reason: String },
}

/// Pure evaluation of retention age against verified_at (UTC unix seconds).
pub fn qualifies_by_age(verified_at: i64, now_unix: i64, retention_days: u32) -> bool {
    let retention_secs = i64::from(retention_days).saturating_mul(86_400);
    now_unix.saturating_sub(verified_at) >= retention_secs
}

/// Evaluate whether a verified record is a deletion candidate given current source metadata.
pub fn evaluate_candidate(
    record: &FileRecord,
    source_size: u64,
    source_mtime: i64,
    source_hash_hex: &str,
    now_unix: i64,
    retention_days: u32,
) -> RetentionDecision {
    if record.status != FileStatus::Verified {
        return RetentionDecision::Skip {
            reason: format!("status is {:?}, not verified", record.status),
        };
    }
    let Some(verified_at) = record.verified_at else {
        return RetentionDecision::Skip {
            reason: "verified_at is missing".into(),
        };
    };
    if !qualifies_by_age(verified_at, now_unix, retention_days) {
        return RetentionDecision::Skip {
            reason: format!(
                "retention window not elapsed (verified_at={verified_at}, now={now_unix}, days={retention_days})"
            ),
        };
    }
    if record.size as u64 != source_size || record.mtime != source_mtime {
        return RetentionDecision::Skip {
            reason: "source size/mtime changed since verification; requires resync".into(),
        };
    }
    let Some(dest_hash) = record.dest_hash.as_deref() else {
        return RetentionDecision::Skip {
            reason: "dest_hash missing".into(),
        };
    };
    if source_hash_hex != dest_hash {
        return RetentionDecision::Skip {
            reason: "source re-hash does not match stored dest_hash".into(),
        };
    }
    RetentionDecision::Qualify
}

/// Run retention cleanup for one pair.
pub fn cleanup_pair(
    pair: &PairConfig,
    store: &StateStore,
    opts: &RetentionOptions,
    audit: Option<&AuditLog>,
) -> Result<CleanupStats> {
    let mut stats = CleanupStats::default();

    if !opts.config.enabled {
        info!(pair = %pair.id, "retention disabled; nothing to do");
        return Ok(stats);
    }

    let retention_secs = i64::from(opts.config.retention_days).saturating_mul(86_400);
    let cutoff = opts.now_unix.saturating_sub(retention_secs);
    let candidates = store.list_verified_for_retention(&pair.id, cutoff)?;
    stats.candidates = candidates.len() as u64;

    for record in candidates {
        let source_path = join_rel(&pair.source, &record.path);
        if !source_path.exists() {
            stats.skipped += 1;
            write_audit(
                audit,
                "skip",
                &pair.id,
                &record.path,
                "source file already absent",
                opts.config.dry_run,
            )?;
            continue;
        }

        let meta = fs::metadata(&source_path).map_err(|e| Error::io(&source_path, e))?;
        let source_mtime = mtime_unix(&meta)?;
        let source_size = meta.len();
        let source_hash = hex_encode(&hash_file(&source_path)?);

        match evaluate_candidate(
            &record,
            source_size,
            source_mtime,
            &source_hash,
            opts.now_unix,
            opts.config.retention_days,
        ) {
            RetentionDecision::Skip { reason } => {
                stats.skipped += 1;
                write_audit(
                    audit,
                    "skip",
                    &pair.id,
                    &record.path,
                    &reason,
                    opts.config.dry_run,
                )?;
                store.log_event(
                    Some(&pair.id),
                    Some(&record.path),
                    "retention_skip",
                    Some(&reason),
                    opts.now_unix,
                )?;
            }
            RetentionDecision::Qualify => {
                if opts.config.dry_run {
                    stats.dry_run_would_act += 1;
                    write_audit(
                        audit,
                        "would_delete",
                        &pair.id,
                        &record.path,
                        "passes all retention checks",
                        true,
                    )?;
                    continue;
                }
                apply_deletion(pair, store, &record, &source_path, &opts.config, opts.now_unix)?;
                stats.deleted_or_quarantined += 1;
                write_audit(
                    audit,
                    match opts.config.mode {
                        RetentionMode::Quarantine => "quarantine",
                        RetentionMode::Permanent => "permanent_delete",
                    },
                    &pair.id,
                    &record.path,
                    "passes all retention checks",
                    false,
                )?;
            }
        }
    }

    Ok(stats)
}

fn apply_deletion(
    pair: &PairConfig,
    store: &StateStore,
    record: &FileRecord,
    source_path: &Path,
    config: &RetentionConfig,
    now: i64,
) -> Result<()> {
    match config.mode {
        RetentionMode::Quarantine => {
            let qdir = config.quarantine_dir.as_ref().ok_or_else(|| {
                Error::Config("quarantine_dir required for quarantine mode".into())
            })?;
            let dest = join_rel(qdir, &record.path);
            if let Some(parent) = dest.parent() {
                fs::create_dir_all(parent).map_err(|e| Error::io(parent, e))?;
            }
            if dest.exists() {
                fs::remove_file(&dest).map_err(|e| Error::io(&dest, e))?;
            }
            fs::rename(source_path, &dest).map_err(|e| Error::io(source_path, e))?;
            let mut updated = record.clone();
            updated.status = FileStatus::Quarantined;
            updated.deleted_at = Some(now);
            store.upsert_file(&updated)?;
            store.log_event(
                Some(&pair.id),
                Some(&record.path),
                "quarantined",
                Some(&dest.to_string_lossy()),
                now,
            )?;
        }
        RetentionMode::Permanent => {
            fs::remove_file(source_path).map_err(|e| Error::io(source_path, e))?;
            let mut updated = record.clone();
            updated.status = FileStatus::Deleted;
            updated.deleted_at = Some(now);
            store.upsert_file(&updated)?;
            store.log_event(
                Some(&pair.id),
                Some(&record.path),
                "deleted",
                None,
                now,
            )?;
        }
    }
    Ok(())
}

/// Permanently remove quarantined files older than purge_grace_days.
pub fn purge_quarantine(
    pair: &PairConfig,
    store: &StateStore,
    opts: &RetentionOptions,
    audit: Option<&AuditLog>,
) -> Result<PurgeStats> {
    let mut stats = PurgeStats::default();
    let grace_secs = i64::from(opts.config.purge_grace_days).saturating_mul(86_400);
    let cutoff = opts.now_unix.saturating_sub(grace_secs);
    let rows = store.list_quarantined(&pair.id, cutoff)?;

    let Some(qdir) = opts.config.quarantine_dir.as_ref() else {
        warn!(pair = %pair.id, "no quarantine_dir configured; purge skipped");
        return Ok(stats);
    };

    for record in rows {
        let path = join_rel(qdir, &record.path);
        if opts.config.dry_run {
            stats.dry_run_would_act += 1;
            write_audit(
                audit,
                "would_purge",
                &pair.id,
                &record.path,
                "past purge grace period",
                true,
            )?;
            continue;
        }
        if path.exists() {
            fs::remove_file(&path).map_err(|e| Error::io(&path, e))?;
        }
        let mut updated = record.clone();
        updated.status = FileStatus::Deleted;
        updated.deleted_at = Some(opts.now_unix);
        store.upsert_file(&updated)?;
        stats.purged += 1;
        write_audit(
            audit,
            "purge",
            &pair.id,
            &record.path,
            "past purge grace period",
            false,
        )?;
    }
    Ok(stats)
}

fn write_audit(
    audit: Option<&AuditLog>,
    action: &str,
    pair_id: &str,
    path: &str,
    reason: &str,
    dry_run: bool,
) -> Result<()> {
    if let Some(log) = audit {
        log.write_event(action, pair_id, path, reason, dry_run)?;
    }
    Ok(())
}

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use assert_fs::prelude::*;
    use assert_fs::TempDir;
    use crate::config::{CompareMode, PairConfig};
    use crate::sync_engine::{sync_pair, SyncOptions};

    fn verified_record(verified_at: i64, size: i64, mtime: i64, hash: &str) -> FileRecord {
        FileRecord {
            pair_id: "p".into(),
            path: "f.bin".into(),
            size,
            mtime,
            source_hash: Some(hash.into()),
            dest_hash: Some(hash.into()),
            synced_at: Some(verified_at),
            verified_at: Some(verified_at),
            deleted_at: None,
            status: FileStatus::Verified,
        }
    }

    #[test]
    fn age_boundary_exactly_at_retention_qualifies() {
        let retention_days = 4u32;
        let verified_at = 1_000_000i64;
        let now = verified_at + i64::from(retention_days) * 86_400;
        assert!(qualifies_by_age(verified_at, now, retention_days));
    }

    #[test]
    fn age_one_second_before_boundary_does_not_qualify() {
        let retention_days = 4u32;
        let verified_at = 1_000_000i64;
        let now = verified_at + i64::from(retention_days) * 86_400 - 1;
        assert!(!qualifies_by_age(verified_at, now, retention_days));
    }

    #[test]
    fn dst_transition_dates_use_utc_seconds() {
        // 2026-03-08 02:00 US DST spring-forward region — we only use UTC unix.
        let verified_at = 1_773_360_000i64; // fixed UTC instant
        let now = verified_at + 4 * 86_400;
        assert!(qualifies_by_age(verified_at, now, 4));
        assert!(!qualifies_by_age(verified_at, now - 1, 4));
    }

    #[test]
    fn mtime_change_skips_deletion() {
        let rec = verified_record(100, 10, 50, "abc");
        let decision = evaluate_candidate(&rec, 10, 99, "abc", 100 + 5 * 86_400, 4);
        assert!(matches!(decision, RetentionDecision::Skip { .. }));
    }

    #[test]
    fn hash_mismatch_skips_deletion() {
        let rec = verified_record(100, 10, 50, "abc");
        let decision = evaluate_candidate(&rec, 10, 50, "zzz", 100 + 5 * 86_400, 4);
        assert!(matches!(decision, RetentionDecision::Skip { .. }));
    }

    #[test]
    fn dry_run_does_not_touch_filesystem() {
        let tmp = TempDir::new().unwrap();
        let src = tmp.child("src");
        let dst = tmp.child("dst");
        let q = tmp.child("q");
        src.create_dir_all().unwrap();
        dst.create_dir_all().unwrap();
        q.create_dir_all().unwrap();
        src.child("old.bin").write_str("data").unwrap();

        let store = StateStore::open_in_memory().unwrap();
        let pair = PairConfig {
            id: "p1".into(),
            source: src.path().to_path_buf(),
            destination: dst.path().to_path_buf(),
            concurrency: Some(2),
            retention: None,
        };
        sync_pair(
            &pair,
            &store,
            &SyncOptions {
                concurrency: 2,
                retry_count: 0,
                retry_backoff_ms: 1,
                heartbeat_secs: 60,
                compare_mode: CompareMode::SizeMtime,
            },
        )
        .unwrap();

        let mut rec = store.get_file("p1", "old.bin").unwrap().unwrap();
        rec.verified_at = Some(now_unix() - 5 * 86_400);
        store.upsert_file(&rec).unwrap();

        let audit_path = tmp.child("audit.jsonl");
        let audit = AuditLog::open(audit_path.path()).unwrap();
        let opts = RetentionOptions {
            config: RetentionConfig {
                enabled: true,
                dry_run: true,
                retention_days: 4,
                mode: RetentionMode::Quarantine,
                quarantine_dir: Some(q.path().to_path_buf()),
                purge_grace_days: 14,
            },
            now_unix: now_unix(),
        };
        let stats = cleanup_pair(&pair, &store, &opts, Some(&audit)).unwrap();
        assert_eq!(stats.dry_run_would_act, 1);
        assert!(src.child("old.bin").path().exists());
        assert!(!q.child("old.bin").path().exists());
    }

    #[test]
    fn quarantine_moves_file_and_purge_respects_grace() {
        let tmp = TempDir::new().unwrap();
        let src = tmp.child("src");
        let dst = tmp.child("dst");
        let q = tmp.child("q");
        src.create_dir_all().unwrap();
        dst.create_dir_all().unwrap();
        q.create_dir_all().unwrap();
        src.child("nested/old.bin").write_str("data").unwrap();

        let store = StateStore::open_in_memory().unwrap();
        let pair = PairConfig {
            id: "p1".into(),
            source: src.path().to_path_buf(),
            destination: dst.path().to_path_buf(),
            concurrency: Some(2),
            retention: None,
        };
        sync_pair(
            &pair,
            &store,
            &SyncOptions {
                concurrency: 2,
                retry_count: 0,
                retry_backoff_ms: 1,
                heartbeat_secs: 60,
                compare_mode: CompareMode::SizeMtime,
            },
        )
        .unwrap();

        let mut rec = store.get_file("p1", "nested/old.bin").unwrap().unwrap();
        rec.verified_at = Some(now_unix() - 5 * 86_400);
        store.upsert_file(&rec).unwrap();

        let opts = RetentionOptions {
            config: RetentionConfig {
                enabled: true,
                dry_run: false,
                retention_days: 4,
                mode: RetentionMode::Quarantine,
                quarantine_dir: Some(q.path().to_path_buf()),
                purge_grace_days: 14,
            },
            now_unix: now_unix(),
        };
        let stats = cleanup_pair(&pair, &store, &opts, None).unwrap();
        assert_eq!(stats.deleted_or_quarantined, 1);
        assert!(!src.child("nested/old.bin").path().exists());
        assert!(q.child("nested/old.bin").path().exists());

        let rec = store.get_file("p1", "nested/old.bin").unwrap().unwrap();
        assert_eq!(rec.status, FileStatus::Quarantined);

        // Freshly quarantined — purge grace not elapsed.
        let purge_stats = purge_quarantine(&pair, &store, &opts, None).unwrap();
        assert_eq!(purge_stats.purged, 0);
        assert!(q.child("nested/old.bin").path().exists());

        // Backdate deleted_at and purge.
        let mut rec = store.get_file("p1", "nested/old.bin").unwrap().unwrap();
        rec.deleted_at = Some(now_unix() - 20 * 86_400);
        store.upsert_file(&rec).unwrap();
        let purge_stats = purge_quarantine(&pair, &store, &opts, None).unwrap();
        assert_eq!(purge_stats.purged, 1);
        assert!(!q.child("nested/old.bin").path().exists());
    }

    #[test]
    fn recent_verified_file_not_touched() {
        let tmp = TempDir::new().unwrap();
        let src = tmp.child("src");
        let dst = tmp.child("dst");
        let q = tmp.child("q");
        src.create_dir_all().unwrap();
        dst.create_dir_all().unwrap();
        q.create_dir_all().unwrap();
        src.child("new.bin").write_str("data").unwrap();

        let store = StateStore::open_in_memory().unwrap();
        let pair = PairConfig {
            id: "p1".into(),
            source: src.path().to_path_buf(),
            destination: dst.path().to_path_buf(),
            concurrency: Some(2),
            retention: None,
        };
        sync_pair(
            &pair,
            &store,
            &SyncOptions {
                concurrency: 2,
                retry_count: 0,
                retry_backoff_ms: 1,
                heartbeat_secs: 60,
                compare_mode: CompareMode::SizeMtime,
            },
        )
        .unwrap();

        // verified_at is "now" from sync — 2 days ago simulation by setting now = verified + 2d
        let rec = store.get_file("p1", "new.bin").unwrap().unwrap();
        let verified_at = rec.verified_at.unwrap();
        let opts = RetentionOptions {
            config: RetentionConfig {
                enabled: true,
                dry_run: false,
                retention_days: 4,
                mode: RetentionMode::Quarantine,
                quarantine_dir: Some(q.path().to_path_buf()),
                purge_grace_days: 14,
            },
            now_unix: verified_at + 2 * 86_400,
        };
        let stats = cleanup_pair(&pair, &store, &opts, None).unwrap();
        assert_eq!(stats.deleted_or_quarantined, 0);
        assert!(src.child("new.bin").path().exists());
    }
}
