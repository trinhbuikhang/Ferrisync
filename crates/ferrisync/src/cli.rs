//! CLI argument definitions and dispatch.

use std::path::PathBuf;

use clap::{Parser, Subcommand};
use ferrisync_core::config::Config;
use ferrisync_core::logging::AuditLog;
use ferrisync_core::retention::{cleanup_pair, purge_quarantine, RetentionOptions};
use ferrisync_core::state_store::StateStore;
use ferrisync_core::sync_engine::{sync_pair, SyncOptions};
use tracing::{info, Level};
use tracing_subscriber::EnvFilter;

#[derive(Debug, Parser)]
#[command(name = "ferrisync", version, about = "NAS file sync with BLAKE3 verification and retention cleanup")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    /// Compare, copy, and verify folder pairs
    Sync {
        #[arg(short, long, default_value = "ferrisync.toml")]
        config: PathBuf,
        /// Limit to a single pair id
        #[arg(long)]
        pair: Option<String>,
    },
    /// Query sync/verification status from the state store
    Status {
        #[arg(short, long, default_value = "ferrisync.toml")]
        config: PathBuf,
        #[arg(long)]
        pair: Option<String>,
        #[arg(long)]
        path: Option<String>,
    },
    /// Run retention policy (respects dry_run)
    Cleanup {
        #[arg(short, long, default_value = "ferrisync.toml")]
        config: PathBuf,
        #[arg(long)]
        pair: Option<String>,
    },
    /// Permanently remove quarantined files past purge grace period
    PurgeQuarantine {
        #[arg(short, long, default_value = "ferrisync.toml")]
        config: PathBuf,
        #[arg(long)]
        pair: Option<String>,
    },
}

pub fn run(cli: Cli) -> anyhow::Result<()> {
    match cli.command {
        Commands::Sync { config, pair } => cmd_sync(config, pair),
        Commands::Status { config, pair, path } => cmd_status(config, pair, path),
        Commands::Cleanup { config, pair } => cmd_cleanup(config, pair),
        Commands::PurgeQuarantine { config, pair } => cmd_purge(config, pair),
    }
}

fn init_logging(level: &str) {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(level));
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .try_init();
}

fn load(config_path: &PathBuf) -> anyhow::Result<(Config, StateStore)> {
    let config = Config::load(config_path)?;
    init_logging(&config.log_level);
    let store = StateStore::open(config.state_db_path())?;
    Ok((config, store))
}

fn selected_pairs<'a>(
    config: &'a Config,
    pair: &Option<String>,
) -> anyhow::Result<Vec<&'a ferrisync_core::config::PairConfig>> {
    if let Some(id) = pair {
        Ok(vec![config.pair(id)?])
    } else {
        Ok(config.pairs.iter().collect())
    }
}

fn cmd_sync(config_path: PathBuf, pair: Option<String>) -> anyhow::Result<()> {
    let (config, store) = load(&config_path)?;
    for p in selected_pairs(&config, &pair)? {
        let opts = SyncOptions::from_config(&config, p);
        info!(pair = %p.id, "starting sync");
        let stats = sync_pair(p, &store, &opts)?;
        info!(
            pair = %p.id,
            scanned = stats.scanned,
            copied = stats.copied,
            skipped = stats.skipped,
            errors = stats.errors,
            bytes = stats.bytes_copied,
            "sync finished"
        );
    }
    Ok(())
}

fn cmd_status(
    config_path: PathBuf,
    pair: Option<String>,
    path: Option<String>,
) -> anyhow::Result<()> {
    let (config, store) = load(&config_path)?;
    for p in selected_pairs(&config, &pair)? {
        if let Some(rel) = &path {
            match store.get_file(&p.id, rel)? {
                Some(rec) => {
                    println!(
                        "pair={} path={} status={} size={} mtime={} source_hash={} dest_hash={} synced_at={:?} verified_at={:?} deleted_at={:?}",
                        rec.pair_id,
                        rec.path,
                        rec.status.as_str(),
                        rec.size,
                        rec.mtime,
                        rec.source_hash.as_deref().unwrap_or("-"),
                        rec.dest_hash.as_deref().unwrap_or("-"),
                        rec.synced_at,
                        rec.verified_at,
                        rec.deleted_at,
                    );
                }
                None => {
                    println!("pair={} path={} status=unknown (not in state store)", p.id, rel);
                }
            }
        } else {
            println!(
                "pair={} source={} destination={}",
                p.id,
                p.source.display(),
                p.destination.display()
            );
            println!("  (pass --path REL to query a specific file)");
        }
    }
    let _ = Level::INFO;
    Ok(())
}

fn open_audit(config: &Config) -> anyhow::Result<Option<AuditLog>> {
    match &config.audit_log {
        Some(path) => Ok(Some(AuditLog::open(path)?)),
        None => Ok(None),
    }
}

fn cmd_cleanup(config_path: PathBuf, pair: Option<String>) -> anyhow::Result<()> {
    let (config, store) = load(&config_path)?;
    let audit = open_audit(&config)?;
    for p in selected_pairs(&config, &pair)? {
        let retention = config.effective_retention(p);
        let opts = RetentionOptions::from_config(retention);
        info!(
            pair = %p.id,
            enabled = opts.config.enabled,
            dry_run = opts.config.dry_run,
            "starting cleanup"
        );
        let stats = cleanup_pair(p, &store, &opts, audit.as_ref())?;
        info!(
            pair = %p.id,
            candidates = stats.candidates,
            acted = stats.deleted_or_quarantined,
            skipped = stats.skipped,
            dry_run_would_act = stats.dry_run_would_act,
            "cleanup finished"
        );
    }
    Ok(())
}

fn cmd_purge(config_path: PathBuf, pair: Option<String>) -> anyhow::Result<()> {
    let (config, store) = load(&config_path)?;
    let audit = open_audit(&config)?;
    for p in selected_pairs(&config, &pair)? {
        let retention = config.effective_retention(p);
        let opts = RetentionOptions::from_config(retention);
        let stats = purge_quarantine(p, &store, &opts, audit.as_ref())?;
        info!(
            pair = %p.id,
            purged = stats.purged,
            skipped = stats.skipped,
            dry_run_would_act = stats.dry_run_would_act,
            "purge finished"
        );
    }
    Ok(())
}
