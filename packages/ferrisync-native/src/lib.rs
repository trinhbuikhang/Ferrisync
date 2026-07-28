#![deny(clippy::all)]

use napi::bindgen_prelude::*;
use napi_derive::napi;

use ferrisync_core::{
    app_data_dir, compare_folders, default_gui_session_path, sync_folders, CleanupStats, Config,
    RetentionMode, RetentionOptions, StateStore,
};
use ferrisync_core::retention::{cleanup_pair, purge_quarantine};

#[napi(object)]
pub struct JsCompareRow {
    pub relative_path: String,
    pub action: String,
    pub source_size: Option<i64>,
    pub dest_size: Option<i64>,
    pub source_mtime: Option<i64>,
    pub dest_mtime: Option<i64>,
}

#[napi(object)]
pub struct JsCompareResult {
    pub rows: Vec<JsCompareRow>,
    pub scanned: i64,
    pub to_copy: i64,
    pub unchanged: i64,
}

#[napi(object)]
pub struct JsSyncStats {
    pub scanned: i64,
    pub copied: i64,
    pub skipped: i64,
    pub errors: i64,
    pub bytes_copied: i64,
}

#[napi(object)]
pub struct JsCleanupStats {
    pub candidates: i64,
    pub deleted_or_quarantined: i64,
    pub skipped: i64,
    pub dry_run_would_act: i64,
}

#[napi]
pub fn compare(source: String, destination: String) -> Result<JsCompareResult> {
    let result = compare_folders(source, destination).map_err(to_napi_err)?;
    Ok(JsCompareResult {
        rows: result
            .rows
            .into_iter()
            .map(|r| JsCompareRow {
                relative_path: r.relative_path,
                action: r.action.as_str().to_string(),
                source_size: r.source_size.map(|v| v as i64),
                dest_size: r.dest_size.map(|v| v as i64),
                source_mtime: r.source_mtime,
                dest_mtime: r.dest_mtime,
            })
            .collect(),
        scanned: result.scanned as i64,
        to_copy: result.to_copy as i64,
        unchanged: result.unchanged as i64,
    })
}

#[napi]
pub fn sync(source: String, destination: String) -> Result<JsSyncStats> {
    let stats = sync_folders(source, destination).map_err(to_napi_err)?;
    Ok(JsSyncStats {
        scanned: stats.scanned as i64,
        copied: stats.copied as i64,
        skipped: stats.skipped as i64,
        errors: stats.errors as i64,
        bytes_copied: stats.bytes_copied as i64,
    })
}

#[napi]
pub fn cleanup(
    source: String,
    destination: String,
    force_enabled: bool,
    dry_run: bool,
) -> Result<JsCleanupStats> {
    let config = Config::single_pair(source.into(), destination.into());
    let _ = config.save(default_gui_session_path());
    let store = StateStore::open(config.state_db_path()).map_err(to_napi_err)?;
    let pair = &config.pairs[0];
    let mut retention = config.effective_retention(pair);
    if force_enabled {
        retention.enabled = true;
    }
    retention.dry_run = dry_run;
    if retention.quarantine_dir.is_none() {
        retention.quarantine_dir = Some(app_data_dir().join("quarantine"));
        retention.mode = RetentionMode::Quarantine;
    }
    let opts = RetentionOptions::from_config(retention);
    let stats: CleanupStats = cleanup_pair(pair, &store, &opts, None).map_err(to_napi_err)?;
    Ok(JsCleanupStats {
        candidates: stats.candidates as i64,
        deleted_or_quarantined: stats.deleted_or_quarantined as i64,
        skipped: stats.skipped as i64,
        dry_run_would_act: stats.dry_run_would_act as i64,
    })
}

#[napi]
pub fn purge_quarantine_dry(source: String, destination: String) -> Result<JsCleanupStats> {
    let config = Config::single_pair(source.into(), destination.into());
    let store = StateStore::open(config.state_db_path()).map_err(to_napi_err)?;
    let pair = &config.pairs[0];
    let mut retention = config.effective_retention(pair);
    retention.dry_run = true;
    let opts = RetentionOptions::from_config(retention);
    let stats = purge_quarantine(pair, &store, &opts, None).map_err(to_napi_err)?;
    Ok(JsCleanupStats {
        candidates: 0,
        deleted_or_quarantined: stats.purged as i64,
        skipped: stats.skipped as i64,
        dry_run_would_act: stats.dry_run_would_act as i64,
    })
}

#[napi]
pub fn app_data_path() -> String {
    app_data_dir().display().to_string()
}

fn to_napi_err(err: ferrisync_core::Error) -> Error {
    Error::from_reason(err.to_string())
}
