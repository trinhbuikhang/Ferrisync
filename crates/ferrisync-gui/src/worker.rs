//! Background jobs so the UI stays responsive.

use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

use ferrisync_core::config::{default_gui_session_path, Config};
use ferrisync_core::logging::AuditLog;
use ferrisync_core::retention::{cleanup_pair, purge_quarantine, RetentionOptions};
use ferrisync_core::state_store::StateStore;
use ferrisync_core::sync_engine::{sync_pair, SyncOptions};

#[derive(Debug, Clone)]
pub struct FolderSession {
    pub source: PathBuf,
    pub destination: PathBuf,
}

#[derive(Debug, Clone)]
pub enum Job {
    Sync(FolderSession),
    Status(FolderSession),
    Cleanup {
        session: FolderSession,
        force_dry_run: bool,
        force_enabled: bool,
    },
    Purge {
        session: FolderSession,
        force_dry_run: bool,
    },
}

#[derive(Debug, Clone)]
pub enum JobEvent {
    Log(String),
    Done { ok: bool },
}

pub struct WorkerHandle {
    job_tx: Sender<Job>,
    pub event_rx: Receiver<JobEvent>,
}

impl WorkerHandle {
    pub fn spawn() -> Self {
        let (job_tx, job_rx) = mpsc::channel::<Job>();
        let (event_tx, event_rx) = mpsc::channel::<JobEvent>();

        thread::spawn(move || {
            while let Ok(job) = job_rx.recv() {
                let result = run_job(job, &event_tx);
                if let Err(e) = &result {
                    let _ = event_tx.send(JobEvent::Log(format!("error: {e:#}")));
                }
                let _ = event_tx.send(JobEvent::Done { ok: result.is_ok() });
            }
        });

        Self { job_tx, event_rx }
    }

    pub fn submit(&self, job: Job) -> Result<(), String> {
        self.job_tx
            .send(job)
            .map_err(|_| "worker thread stopped".to_string())
    }
}

fn log(tx: &Sender<JobEvent>, msg: impl Into<String>) {
    let _ = tx.send(JobEvent::Log(msg.into()));
}

fn prepare_config(session: &FolderSession) -> anyhow::Result<Config> {
    if session.source.as_os_str().is_empty() {
        anyhow::bail!("choose a source folder first");
    }
    if session.destination.as_os_str().is_empty() {
        anyhow::bail!("choose a destination folder first");
    }
    if !session.source.is_dir() {
        anyhow::bail!(
            "source folder does not exist: {}",
            session.source.display()
        );
    }

    let config = Config::single_pair(session.source.clone(), session.destination.clone());
    config.validate()?;

    // Auto-persist so the next GUI launch restores these folders.
    let session_path = default_gui_session_path();
    config.save(&session_path)?;

    Ok(config)
}

fn run_job(job: Job, tx: &Sender<JobEvent>) -> anyhow::Result<()> {
    match job {
        Job::Sync(session) => {
            let config = prepare_config(&session)?;
            log(
                tx,
                format!(
                    "session saved to {}",
                    default_gui_session_path().display()
                ),
            );
            let store = StateStore::open(config.state_db_path())?;
            let pair = &config.pairs[0];
            log(
                tx,
                format!(
                    "SYNC start pair={} src={} dst={}",
                    pair.id,
                    pair.source.display(),
                    pair.destination.display()
                ),
            );
            let opts = SyncOptions::from_config(&config, pair);
            let stats = sync_pair(pair, &store, &opts)?;
            log(
                tx,
                format!(
                    "SYNC done scanned={} copied={} skipped={} errors={} bytes={}",
                    stats.scanned, stats.copied, stats.skipped, stats.errors, stats.bytes_copied
                ),
            );
            Ok(())
        }
        Job::Status(session) => {
            let config = prepare_config(&session)?;
            let store = StateStore::open(config.state_db_path())?;
            let pair = &config.pairs[0];
            log(
                tx,
                format!(
                    "STATUS pair={} source={} destination={}",
                    pair.id,
                    pair.source.display(),
                    pair.destination.display()
                ),
            );
            log(
                tx,
                format!("state_db={}", config.state_db_path().display()),
            );
            let files = ferrisync_core::scanner::scan_files(&pair.source)?;
            let mut verified = 0u64;
            let mut missing = 0u64;
            let mut other = 0u64;
            for meta in files.iter().take(5000) {
                match store.get_file(&pair.id, &meta.relative_path)? {
                    Some(rec) if rec.status.as_str() == "verified" => verified += 1,
                    Some(_) => other += 1,
                    None => missing += 1,
                }
            }
            log(
                tx,
                format!(
                    "STATUS sample files={} verified_in_db={} other_status={} not_in_db={} (capped scan)",
                    files.len().min(5000),
                    verified,
                    other,
                    missing
                ),
            );
            Ok(())
        }
        Job::Cleanup {
            session,
            force_dry_run,
            force_enabled,
        } => {
            let config = prepare_config(&session)?;
            let store = StateStore::open(config.state_db_path())?;
            let pair = &config.pairs[0];
            let mut retention = config.effective_retention(pair);
            if force_enabled {
                retention.enabled = true;
            }
            if force_dry_run {
                retention.dry_run = true;
            }
            let audit = open_audit(&config)?;
            log(
                tx,
                format!(
                    "CLEANUP start pair={} enabled={} dry_run={} days={} mode={:?}",
                    pair.id,
                    retention.enabled,
                    retention.dry_run,
                    retention.retention_days,
                    retention.mode
                ),
            );
            if !retention.enabled {
                log(
                    tx,
                    "CLEANUP skipped: retention.enabled=false (enable Force retention in GUI)",
                );
                return Ok(());
            }
            let opts = RetentionOptions::from_config(retention);
            let stats = cleanup_pair(pair, &store, &opts, audit.as_ref())?;
            log(
                tx,
                format!(
                    "CLEANUP done candidates={} acted={} skipped={} dry_run_would_act={}",
                    stats.candidates,
                    stats.deleted_or_quarantined,
                    stats.skipped,
                    stats.dry_run_would_act
                ),
            );
            Ok(())
        }
        Job::Purge {
            session,
            force_dry_run,
        } => {
            let config = prepare_config(&session)?;
            let store = StateStore::open(config.state_db_path())?;
            let pair = &config.pairs[0];
            let mut retention = config.effective_retention(pair);
            if force_dry_run {
                retention.dry_run = true;
            }
            let audit = open_audit(&config)?;
            let opts = RetentionOptions::from_config(retention);
            let stats = purge_quarantine(pair, &store, &opts, audit.as_ref())?;
            log(
                tx,
                format!(
                    "PURGE done purged={} skipped={} dry_run_would_act={}",
                    stats.purged, stats.skipped, stats.dry_run_would_act
                ),
            );
            Ok(())
        }
    }
}

fn open_audit(config: &Config) -> anyhow::Result<Option<AuditLog>> {
    match &config.audit_log {
        Some(path) => Ok(Some(AuditLog::open(path)?)),
        None => Ok(None),
    }
}
