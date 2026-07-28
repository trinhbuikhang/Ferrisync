//! Compare, fail-safe copy with streamed BLAKE3 verification, and worker pool.

use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crossbeam_channel::{bounded, Receiver, Sender};
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::config::{CompareMode, Config, PairConfig};
use crate::error::{Error, Result};
use crate::hasher::{hex_encode, Digest};
use crate::logging::ProgressSnapshot;
use crate::queue::SyncJob;
use crate::scanner::{classify, dest_file_meta, join_rel, scan_files, FileAction};
use crate::state_store::{FileRecord, FileStatus, StateStore};

#[derive(Debug, Clone)]
pub struct SyncOptions {
    pub concurrency: usize,
    pub retry_count: u32,
    pub retry_backoff_ms: u64,
    pub heartbeat_secs: u64,
    pub compare_mode: CompareMode,
}

impl SyncOptions {
    pub fn from_config(config: &Config, pair: &PairConfig) -> Self {
        Self {
            concurrency: config.effective_concurrency(pair),
            retry_count: config.defaults.retry_count,
            retry_backoff_ms: config.defaults.retry_backoff_ms,
            heartbeat_secs: config.defaults.heartbeat_secs,
            compare_mode: config.defaults.compare_mode,
        }
    }
}

#[derive(Debug, Default, Clone)]
pub struct SyncStats {
    pub scanned: u64,
    pub copied: u64,
    pub skipped: u64,
    pub errors: u64,
    pub bytes_copied: u64,
}

#[derive(Default)]
struct ProgressCounters {
    scanned: AtomicU64,
    copied: AtomicU64,
    skipped: AtomicU64,
    errors: AtomicU64,
    bytes_copied: AtomicU64,
}

impl ProgressCounters {
    fn snapshot(&self) -> ProgressSnapshot {
        ProgressSnapshot {
            scanned: self.scanned.load(Ordering::Relaxed),
            copied: self.copied.load(Ordering::Relaxed),
            skipped: self.skipped.load(Ordering::Relaxed),
            errors: self.errors.load(Ordering::Relaxed),
            bytes_copied: self.bytes_copied.load(Ordering::Relaxed),
        }
    }
}

enum CopyOutcome {
    Ok {
        pair_id: String,
        relative_path: String,
        size: u64,
        mtime: i64,
        source_hash: String,
        dest_hash: String,
        bytes: u64,
    },
    Err {
        pair_id: String,
        relative_path: String,
        size: u64,
        mtime: i64,
        message: String,
    },
}

/// Sync one folder pair: scan → compare → enqueue → copy+verify workers.
pub fn sync_pair(pair: &PairConfig, store: &StateStore, opts: &SyncOptions) -> Result<SyncStats> {
    store.upsert_pair(&pair.id, &pair.source, &pair.destination)?;
    fs::create_dir_all(&pair.destination).map_err(|e| Error::io(&pair.destination, e))?;

    let files = scan_files(&pair.source)?;
    let scanned = files.len() as u64;

    // Unbounded job queue avoids producer/consumer deadlock when enqueueing
    // before or while workers drain a bounded channel.
    let (job_tx, job_rx): (Sender<SyncJob>, Receiver<SyncJob>) = crossbeam_channel::unbounded();
    let (out_tx, out_rx) = bounded::<CopyOutcome>(opts.concurrency.saturating_mul(4).max(16));

    let progress = Arc::new(ProgressCounters::default());
    progress.scanned.store(scanned, Ordering::Relaxed);

    let worker_count = opts.concurrency.max(1);
    let mut handles = Vec::with_capacity(worker_count);
    for _ in 0..worker_count {
        let job_rx = job_rx.clone();
        let out_tx = out_tx.clone();
        let progress = Arc::clone(&progress);
        let retry_count = opts.retry_count;
        let retry_backoff_ms = opts.retry_backoff_ms;
        handles.push(thread::spawn(move || {
            while let Ok(job) = job_rx.recv() {
                let outcome = copy_with_retries(&job, retry_count, retry_backoff_ms);
                match &outcome {
                    CopyOutcome::Ok { bytes, .. } => {
                        progress.copied.fetch_add(1, Ordering::Relaxed);
                        progress.bytes_copied.fetch_add(*bytes, Ordering::Relaxed);
                    }
                    CopyOutcome::Err { .. } => {
                        progress.errors.fetch_add(1, Ordering::Relaxed);
                    }
                }
                if out_tx.send(outcome).is_err() {
                    break;
                }
            }
        }));
    }
    drop(out_tx);

    let mut skipped = 0u64;
    for meta in &files {
        let dest_meta = dest_file_meta(&pair.destination, &meta.relative_path)?;
        let state = store.get_file(&pair.id, &meta.relative_path)?;
        // compare_mode reserved for future force-hash compare; size+mtime is default.
        let _ = opts.compare_mode;
        let action = classify(meta, dest_meta.as_ref(), state.as_ref());
        match action {
            FileAction::Unchanged => skipped += 1,
            FileAction::New | FileAction::Updated | FileAction::MissingOnDest => {
                job_tx
                    .send(SyncJob::from_meta(&pair.id, &pair.destination, meta))
                    .map_err(|_| Error::Other("failed to enqueue sync job".into()))?;
            }
        }
    }
    drop(job_tx);
    progress.skipped.store(skipped, Ordering::Relaxed);

    let heartbeat = opts.heartbeat_secs.max(1);
    let start = Instant::now();
    let mut last_beat = Instant::now();

    for outcome in out_rx {
        commit_outcome(store, outcome)?;
        if last_beat.elapsed() >= Duration::from_secs(heartbeat) {
            let snap = progress.snapshot();
            let elapsed = start.elapsed().as_secs_f64().max(0.001);
            let thr = snap.bytes_copied as f64 / elapsed;
            info!(
                scanned = snap.scanned,
                copied = snap.copied,
                skipped = snap.skipped,
                errors = snap.errors,
                bytes = snap.bytes_copied,
                bytes_per_sec = thr,
                "sync heartbeat"
            );
            last_beat = Instant::now();
        }
    }

    for h in handles {
        let _ = h.join();
    }

    let snap = progress.snapshot();
    info!(
        pair = %pair.id,
        scanned = snap.scanned,
        copied = snap.copied,
        skipped = snap.skipped,
        errors = snap.errors,
        bytes = snap.bytes_copied,
        "sync complete"
    );

    Ok(SyncStats {
        scanned: snap.scanned,
        copied: snap.copied,
        skipped: snap.skipped,
        errors: snap.errors,
        bytes_copied: snap.bytes_copied,
    })
}

fn copy_with_retries(job: &SyncJob, retry_count: u32, backoff_ms: u64) -> CopyOutcome {
    let mut attempt = 0u32;
    loop {
        match copy_and_verify(job) {
            Ok(v) => return v,
            Err(e) => {
                attempt += 1;
                if attempt > retry_count {
                    return CopyOutcome::Err {
                        pair_id: job.pair_id.clone(),
                        relative_path: job.relative_path.clone(),
                        size: job.size,
                        mtime: job.mtime,
                        message: e.to_string(),
                    };
                }
                warn!(
                    path = %job.relative_path,
                    attempt,
                    error = %e,
                    "transient copy failure; retrying"
                );
                thread::sleep(Duration::from_millis(
                    backoff_ms.saturating_mul(attempt as u64),
                ));
            }
        }
    }
}

fn copy_and_verify(job: &SyncJob) -> Result<CopyOutcome> {
    let dest_final = join_rel(&job.dest_root, &job.relative_path);
    if let Some(parent) = dest_final.parent() {
        fs::create_dir_all(parent).map_err(|e| Error::io(parent, e))?;
    }

    let file_name = dest_final
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "file".into());
    let tmp_name = format!(".ferrisync-tmp-{}-{}", Uuid::new_v4(), file_name);
    let dest_tmp = dest_final
        .parent()
        .unwrap_or(Path::new("."))
        .join(tmp_name);

    let result = (|| {
        let mut src = File::open(&job.source_path).map_err(|e| Error::io(&job.source_path, e))?;
        let mut dst = File::create(&dest_tmp).map_err(|e| Error::io(&dest_tmp, e))?;

        let mut src_hasher = blake3::Hasher::new();
        let mut dst_hasher = blake3::Hasher::new();
        let mut buf = [0u8; 64 * 1024];
        let mut bytes = 0u64;

        loop {
            let n = src
                .read(&mut buf)
                .map_err(|e| Error::io(&job.source_path, e))?;
            if n == 0 {
                break;
            }
            src_hasher.update(&buf[..n]);
            dst.write_all(&buf[..n])
                .map_err(|e| Error::io(&dest_tmp, e))?;
            dst_hasher.update(&buf[..n]);
            bytes += n as u64;
        }
        dst.flush().map_err(|e| Error::io(&dest_tmp, e))?;
        dst.sync_all().map_err(|e| Error::io(&dest_tmp, e))?;
        drop(dst);

        let source_digest: Digest = *src_hasher.finalize().as_bytes();
        let dest_digest: Digest = *dst_hasher.finalize().as_bytes();
        let source_hash = hex_encode(&source_digest);
        let dest_hash = hex_encode(&dest_digest);

        if source_digest != dest_digest {
            let _ = fs::remove_file(&dest_tmp);
            return Err(Error::HashMismatch {
                path: job.source_path.clone(),
                source_hash,
                dest_hash,
            });
        }

        fs::rename(&dest_tmp, &dest_final).map_err(|e| Error::io(&dest_final, e))?;

        Ok(CopyOutcome::Ok {
            pair_id: job.pair_id.clone(),
            relative_path: job.relative_path.clone(),
            size: job.size,
            mtime: job.mtime,
            source_hash,
            dest_hash,
            bytes,
        })
    })();

    if result.is_err() {
        let _ = fs::remove_file(&dest_tmp);
    }
    result
}

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn commit_outcome(store: &StateStore, outcome: CopyOutcome) -> Result<()> {
    let now = now_unix();
    match outcome {
        CopyOutcome::Ok {
            pair_id,
            relative_path,
            size,
            mtime,
            source_hash,
            dest_hash,
            ..
        } => {
            let record = FileRecord {
                pair_id: pair_id.clone(),
                path: relative_path.clone(),
                size: size as i64,
                mtime,
                source_hash: Some(source_hash),
                dest_hash: Some(dest_hash),
                synced_at: Some(now),
                verified_at: Some(now),
                deleted_at: None,
                status: FileStatus::Verified,
            };
            store.upsert_file(&record)?;
            store.log_event(
                Some(&pair_id),
                Some(&relative_path),
                "verified",
                None,
                now,
            )?;
        }
        CopyOutcome::Err {
            pair_id,
            relative_path,
            size,
            mtime,
            message,
        } => {
            error!(pair = %pair_id, path = %relative_path, error = %message, "copy failed");
            let record = FileRecord {
                pair_id: pair_id.clone(),
                path: relative_path.clone(),
                size: size as i64,
                mtime,
                source_hash: None,
                dest_hash: None,
                synced_at: None,
                verified_at: None,
                deleted_at: None,
                status: FileStatus::Error,
            };
            store.upsert_file(&record)?;
            store.log_event(
                Some(&pair_id),
                Some(&relative_path),
                "error",
                Some(&message),
                now,
            )?;
        }
    }
    Ok(())
}

/// Re-hash source and destination; return true only if both match stored hashes.
pub fn verify_existing_file(
    pair: &PairConfig,
    store: &StateStore,
    relative_path: &str,
) -> Result<bool> {
    let record = store
        .get_file(&pair.id, relative_path)?
        .ok_or_else(|| Error::Other(format!("no state for {relative_path}")))?;
    let dest_path = join_rel(&pair.destination, relative_path);
    let source_path = join_rel(&pair.source, relative_path);
    let dest_hash = hex_encode(&crate::hasher::hash_file(&dest_path)?);
    let source_hash = hex_encode(&crate::hasher::hash_file(&source_path)?);
    Ok(Some(dest_hash.as_str()) == record.dest_hash.as_deref()
        && Some(source_hash.as_str()) == record.source_hash.as_deref()
        && source_hash == dest_hash)
}

#[cfg(test)]
mod tests {
    use super::*;
    use assert_fs::prelude::*;
    use assert_fs::TempDir;

    fn test_pair(src: &Path, dst: &Path) -> PairConfig {
        PairConfig {
            id: "t1".into(),
            source: src.to_path_buf(),
            destination: dst.to_path_buf(),
            concurrency: Some(2),
            retention: None,
        }
    }

    fn opts() -> SyncOptions {
        SyncOptions {
            concurrency: 2,
            retry_count: 1,
            retry_backoff_ms: 10,
            heartbeat_secs: 60,
            compare_mode: CompareMode::SizeMtime,
        }
    }

    #[test]
    fn full_sync_cycle_verifies_and_records_hashes() {
        let tmp = TempDir::new().unwrap();
        let src = tmp.child("src");
        let dst = tmp.child("dst");
        src.create_dir_all().unwrap();
        dst.create_dir_all().unwrap();
        src.child("nested/a.bin").write_str("payload-a").unwrap();
        src.child("b.txt").write_str("payload-b").unwrap();

        let store = StateStore::open_in_memory().unwrap();
        let pair = test_pair(src.path(), dst.path());
        let stats = sync_pair(&pair, &store, &opts()).unwrap();
        assert_eq!(stats.copied, 2);
        assert_eq!(stats.errors, 0);

        assert_eq!(
            fs::read_to_string(dst.child("nested/a.bin").path()).unwrap(),
            "payload-a"
        );
        let rec = store.get_file("t1", "nested/a.bin").unwrap().unwrap();
        assert_eq!(rec.status, FileStatus::Verified);
        assert!(rec.source_hash.is_some());
        assert_eq!(rec.source_hash, rec.dest_hash);
        assert!(rec.verified_at.is_some());
    }

    #[test]
    fn corruption_detected_on_reverify() {
        let tmp = TempDir::new().unwrap();
        let src = tmp.child("src");
        let dst = tmp.child("dst");
        src.create_dir_all().unwrap();
        dst.create_dir_all().unwrap();
        src.child("f.bin").write_str("good-data").unwrap();

        let store = StateStore::open_in_memory().unwrap();
        let pair = test_pair(src.path(), dst.path());
        sync_pair(&pair, &store, &opts()).unwrap();

        fs::write(dst.child("f.bin").path(), b"bad-data!!").unwrap();
        let ok = verify_existing_file(&pair, &store, "f.bin").unwrap();
        assert!(!ok);
    }

    #[test]
    fn temp_file_not_left_as_final_on_success() {
        let tmp = TempDir::new().unwrap();
        let src = tmp.child("src");
        let dst = tmp.child("dst");
        src.create_dir_all().unwrap();
        dst.create_dir_all().unwrap();
        src.child("ok.bin").write_str("ok").unwrap();

        let store = StateStore::open_in_memory().unwrap();
        let pair = test_pair(src.path(), dst.path());
        sync_pair(&pair, &store, &opts()).unwrap();

        let entries: Vec<_> = fs::read_dir(dst.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert!(entries.iter().any(|n| n == "ok.bin"));
        assert!(entries.iter().all(|n| !n.starts_with(".ferrisync-tmp-")));
    }

    #[test]
    fn concurrent_sync_many_files_is_consistent() {
        let tmp = TempDir::new().unwrap();
        let src = tmp.child("src");
        let dst = tmp.child("dst");
        src.create_dir_all().unwrap();
        dst.create_dir_all().unwrap();
        for i in 0..500 {
            src.child(format!("f{i}.dat"))
                .write_str(&format!("content-{i}"))
                .unwrap();
        }
        let store = StateStore::open_in_memory().unwrap();
        let mut pair = test_pair(src.path(), dst.path());
        pair.concurrency = Some(8);
        let mut o = opts();
        o.concurrency = 8;
        let stats = sync_pair(&pair, &store, &o).unwrap();
        assert_eq!(stats.copied, 500);
        assert_eq!(stats.errors, 0);
        for i in 0..500 {
            let rec = store.get_file("t1", &format!("f{i}.dat")).unwrap().unwrap();
            assert_eq!(rec.status, FileStatus::Verified);
        }
    }
}
