//! Ferrisync core library: sync, verify, and retention for folder pairs.

pub mod config;
pub mod error;
pub mod hasher;
pub mod logging;
pub mod queue;
pub mod retention;
pub mod scanner;
pub mod state_store;
pub mod sync_engine;

pub use config::{
    CompareMode, Config, PairConfig, RetentionConfig, RetentionMode, DEFAULT_RETENTION_DAYS,
};
pub use error::{Error, Result};
pub use state_store::{FileRecord, FileStatus, StateStore};
pub use retention::{CleanupStats, PurgeStats, RetentionOptions};
pub use sync_engine::{sync_pair, SyncOptions, SyncStats};
