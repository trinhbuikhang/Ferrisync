//! TOML configuration loading and validation.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

pub const DEFAULT_RETENTION_DAYS: u32 = 4;
pub const DEFAULT_CONCURRENCY: usize = 4;
pub const DEFAULT_RETRY_COUNT: u32 = 3;
pub const DEFAULT_RETRY_BACKOFF_MS: u64 = 500;
pub const DEFAULT_PURGE_GRACE_DAYS: u32 = 14;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum CompareMode {
    #[default]
    SizeMtime,
    Hash,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum RetentionMode {
    #[default]
    Quarantine,
    Permanent,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetentionConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_true")]
    pub dry_run: bool,
    #[serde(default = "default_retention_days")]
    pub retention_days: u32,
    #[serde(default)]
    pub mode: RetentionMode,
    #[serde(default)]
    pub quarantine_dir: Option<PathBuf>,
    #[serde(default = "default_purge_grace_days")]
    pub purge_grace_days: u32,
}

fn default_true() -> bool {
    true
}

fn default_retention_days() -> u32 {
    DEFAULT_RETENTION_DAYS
}

fn default_purge_grace_days() -> u32 {
    DEFAULT_PURGE_GRACE_DAYS
}

impl Default for RetentionConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            dry_run: true,
            retention_days: DEFAULT_RETENTION_DAYS,
            mode: RetentionMode::Quarantine,
            quarantine_dir: None,
            purge_grace_days: DEFAULT_PURGE_GRACE_DAYS,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DefaultsConfig {
    #[serde(default = "default_concurrency")]
    pub concurrency: usize,
    #[serde(default = "default_retry_count")]
    pub retry_count: u32,
    #[serde(default = "default_retry_backoff_ms")]
    pub retry_backoff_ms: u64,
    #[serde(default)]
    pub compare_mode: CompareMode,
    #[serde(default)]
    pub retention: RetentionConfig,
    #[serde(default = "default_heartbeat_secs")]
    pub heartbeat_secs: u64,
}

fn default_concurrency() -> usize {
    DEFAULT_CONCURRENCY
}

fn default_retry_count() -> u32 {
    DEFAULT_RETRY_COUNT
}

fn default_retry_backoff_ms() -> u64 {
    DEFAULT_RETRY_BACKOFF_MS
}

fn default_heartbeat_secs() -> u64 {
    10
}

impl Default for DefaultsConfig {
    fn default() -> Self {
        Self {
            concurrency: DEFAULT_CONCURRENCY,
            retry_count: DEFAULT_RETRY_COUNT,
            retry_backoff_ms: DEFAULT_RETRY_BACKOFF_MS,
            compare_mode: CompareMode::SizeMtime,
            retention: RetentionConfig::default(),
            heartbeat_secs: 10,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairConfig {
    pub id: String,
    pub source: PathBuf,
    pub destination: PathBuf,
    pub concurrency: Option<usize>,
    pub retention: Option<RetentionConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub state_db: Option<PathBuf>,
    #[serde(default = "default_log_level")]
    pub log_level: String,
    pub audit_log: Option<PathBuf>,
    #[serde(default)]
    pub defaults: DefaultsConfig,
    #[serde(default)]
    pub pairs: Vec<PairConfig>,
}

fn default_log_level() -> String {
    "info".to_string()
}

impl Config {
    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let text = fs::read_to_string(path).map_err(|e| Error::io(path, e))?;
        let config: Config = toml::from_str(&text)?;
        config.validate()?;
        Ok(config)
    }

    pub fn from_str(text: &str) -> Result<Self> {
        let config: Config = toml::from_str(text)?;
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> Result<()> {
        if self.pairs.is_empty() {
            return Err(Error::Config("at least one [[pairs]] entry is required".into()));
        }

        let mut ids = std::collections::HashSet::new();
        for pair in &self.pairs {
            if pair.id.trim().is_empty() {
                return Err(Error::Config("pair id must not be empty".into()));
            }
            if !ids.insert(pair.id.clone()) {
                return Err(Error::Config(format!("duplicate pair id: {}", pair.id)));
            }
            if pair.source.as_os_str().is_empty() {
                return Err(Error::Config(format!(
                    "pair {}: source path is empty",
                    pair.id
                )));
            }
            if pair.destination.as_os_str().is_empty() {
                return Err(Error::Config(format!(
                    "pair {}: destination path is empty",
                    pair.id
                )));
            }

            let retention = pair
                .retention
                .as_ref()
                .unwrap_or(&self.defaults.retention);
            retention.validate_for_pair(&pair.id)?;
        }

        if self.defaults.concurrency == 0 {
            return Err(Error::Config("defaults.concurrency must be >= 1".into()));
        }

        Ok(())
    }

    pub fn state_db_path(&self) -> PathBuf {
        if let Some(path) = &self.state_db {
            return path.clone();
        }
        default_state_db_path()
    }

    pub fn pair(&self, id: &str) -> Result<&PairConfig> {
        self.pairs
            .iter()
            .find(|p| p.id == id)
            .ok_or_else(|| Error::PairNotFound(id.to_string()))
    }

    pub fn effective_retention(&self, pair: &PairConfig) -> RetentionConfig {
        pair.retention
            .clone()
            .unwrap_or_else(|| self.defaults.retention.clone())
    }

    pub fn effective_concurrency(&self, pair: &PairConfig) -> usize {
        pair.concurrency.unwrap_or(self.defaults.concurrency).max(1)
    }
}

impl RetentionConfig {
    pub fn validate_for_pair(&self, pair_id: &str) -> Result<()> {
        if self.enabled
            && !self.dry_run
            && self.mode == RetentionMode::Quarantine
            && self
                .quarantine_dir
                .as_ref()
                .map(|p| p.as_os_str().is_empty())
                .unwrap_or(true)
        {
            return Err(Error::Config(format!(
                "pair {}: quarantine_dir is required when retention is enabled with mode=quarantine and dry_run=false",
                pair_id
            )));
        }
        Ok(())
    }
}

pub fn default_state_db_path() -> PathBuf {
    if let Some(dirs) = directories::ProjectDirs::from("com", "Ferrisync", "Ferrisync") {
        dirs.data_dir().join("state.db")
    } else {
        PathBuf::from("ferrisync-state.db")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retention_defaults_are_safe() {
        let r = RetentionConfig::default();
        assert!(!r.enabled);
        assert!(r.dry_run);
        assert_eq!(r.retention_days, 4);
        assert_eq!(r.mode, RetentionMode::Quarantine);
    }

    #[test]
    fn parses_valid_toml() {
        let toml = r#"
log_level = "debug"

[defaults]
concurrency = 2

[defaults.retention]
enabled = false
dry_run = true
retention_days = 4

[[pairs]]
id = "survey"
source = "D:/src"
destination = "E:/dst"
"#;
        let cfg = Config::from_str(toml).unwrap();
        assert_eq!(cfg.pairs.len(), 1);
        assert_eq!(cfg.pairs[0].id, "survey");
        assert!(!cfg.defaults.retention.enabled);
        assert!(cfg.defaults.retention.dry_run);
        assert_eq!(cfg.defaults.concurrency, 2);
    }

    #[test]
    fn rejects_empty_pairs() {
        let toml = r#"
[defaults]
concurrency = 1
"#;
        let err = Config::from_str(toml).unwrap_err();
        assert!(matches!(err, Error::Config(_)));
    }

    #[test]
    fn rejects_duplicate_pair_ids() {
        let toml = r#"
[[pairs]]
id = "a"
source = "s1"
destination = "d1"

[[pairs]]
id = "a"
source = "s2"
destination = "d2"
"#;
        let err = Config::from_str(toml).unwrap_err();
        assert!(matches!(err, Error::Config(_)));
    }

    #[test]
    fn rejects_enabled_quarantine_without_dir() {
        let toml = r#"
[[pairs]]
id = "a"
source = "s"
destination = "d"
retention = { enabled = true, dry_run = false, mode = "quarantine" }
"#;
        let err = Config::from_str(toml).unwrap_err();
        assert!(matches!(err, Error::Config(_)));
    }

    #[test]
    fn dry_run_defaults_to_true_when_omitted() {
        let toml = r#"
[[pairs]]
id = "a"
source = "s"
destination = "d"

[defaults.retention]
enabled = true
"#;
        let cfg = Config::from_str(toml).unwrap();
        assert!(cfg.defaults.retention.dry_run);
        assert!(cfg.defaults.retention.enabled);
    }
}
