//! Error types for ferrisync-core.

use std::path::PathBuf;

use thiserror::Error;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Error)]
pub enum Error {
    #[error("IO error at {path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },

    #[error("IO error: {0}")]
    IoSimple(#[from] std::io::Error),

    #[error("config error: {0}")]
    Config(String),

    #[error("TOML parse error: {0}")]
    Toml(#[from] toml::de::Error),

    #[error("database error: {0}")]
    Db(#[from] rusqlite::Error),

    #[error("hash mismatch for {path}: source={source_hash} dest={dest_hash}")]
    HashMismatch {
        path: PathBuf,
        source_hash: String,
        dest_hash: String,
    },

    #[error("pair not found: {0}")]
    PairNotFound(String),

    #[error("{0}")]
    Other(String),
}

impl Error {
    pub fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }
}
