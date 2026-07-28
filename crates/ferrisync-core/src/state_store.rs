//! SQLite-backed persistent sync state.

use std::path::{Path, PathBuf};

use rusqlite::{params, Connection, OptionalExtension};

use crate::error::Result;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileStatus {
    Pending,
    Synced,
    Verified,
    Quarantined,
    Deleted,
    Error,
}

impl FileStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Synced => "synced",
            Self::Verified => "verified",
            Self::Quarantined => "quarantined",
            Self::Deleted => "deleted",
            Self::Error => "error",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "pending" => Some(Self::Pending),
            "synced" => Some(Self::Synced),
            "verified" => Some(Self::Verified),
            "quarantined" => Some(Self::Quarantined),
            "deleted" => Some(Self::Deleted),
            "error" => Some(Self::Error),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct FileRecord {
    pub pair_id: String,
    pub path: String,
    pub size: i64,
    pub mtime: i64,
    pub source_hash: Option<String>,
    pub dest_hash: Option<String>,
    pub synced_at: Option<i64>,
    pub verified_at: Option<i64>,
    pub deleted_at: Option<i64>,
    pub status: FileStatus,
}

pub struct StateStore {
    conn: Connection,
}

impl StateStore {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
        let conn = Connection::open(path)?;
        conn.execute_batch(
            "
            PRAGMA journal_mode=WAL;
            PRAGMA synchronous=NORMAL;
            PRAGMA foreign_keys=ON;
            ",
        )?;
        let store = Self { conn };
        store.migrate()?;
        Ok(store)
    }

    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch("PRAGMA foreign_keys=ON;")?;
        let store = Self { conn };
        store.migrate()?;
        Ok(store)
    }

    fn migrate(&self) -> Result<()> {
        self.conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS folder_pairs (
                id TEXT PRIMARY KEY,
                source TEXT NOT NULL,
                destination TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS files (
                pair_id TEXT NOT NULL,
                path TEXT NOT NULL,
                size INTEGER NOT NULL,
                mtime INTEGER NOT NULL,
                source_hash TEXT,
                dest_hash TEXT,
                synced_at INTEGER,
                verified_at INTEGER,
                deleted_at INTEGER,
                status TEXT NOT NULL,
                PRIMARY KEY (pair_id, path),
                FOREIGN KEY (pair_id) REFERENCES folder_pairs(id)
            );

            CREATE TABLE IF NOT EXISTS events (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                pair_id TEXT,
                path TEXT,
                event_type TEXT NOT NULL,
                detail TEXT,
                created_at INTEGER NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_files_status
                ON files(pair_id, status, verified_at);
            ",
        )?;
        Ok(())
    }

    pub fn upsert_pair(&self, id: &str, source: &Path, destination: &Path) -> Result<()> {
        self.conn.execute(
            "INSERT INTO folder_pairs (id, source, destination) VALUES (?1, ?2, ?3)
             ON CONFLICT(id) DO UPDATE SET source=excluded.source, destination=excluded.destination",
            params![
                id,
                source.to_string_lossy(),
                destination.to_string_lossy()
            ],
        )?;
        Ok(())
    }

    pub fn upsert_file(&self, record: &FileRecord) -> Result<()> {
        self.conn.execute(
            "INSERT INTO files (
                pair_id, path, size, mtime, source_hash, dest_hash,
                synced_at, verified_at, deleted_at, status
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
             ON CONFLICT(pair_id, path) DO UPDATE SET
                size=excluded.size,
                mtime=excluded.mtime,
                source_hash=excluded.source_hash,
                dest_hash=excluded.dest_hash,
                synced_at=excluded.synced_at,
                verified_at=excluded.verified_at,
                deleted_at=excluded.deleted_at,
                status=excluded.status",
            params![
                record.pair_id,
                record.path,
                record.size,
                record.mtime,
                record.source_hash,
                record.dest_hash,
                record.synced_at,
                record.verified_at,
                record.deleted_at,
                record.status.as_str(),
            ],
        )?;
        Ok(())
    }

    pub fn get_file(&self, pair_id: &str, path: &str) -> Result<Option<FileRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT pair_id, path, size, mtime, source_hash, dest_hash,
                    synced_at, verified_at, deleted_at, status
             FROM files WHERE pair_id = ?1 AND path = ?2",
        )?;
        let row = stmt
            .query_row(params![pair_id, path], |row| {
                Ok(FileRecord {
                    pair_id: row.get(0)?,
                    path: row.get(1)?,
                    size: row.get(2)?,
                    mtime: row.get(3)?,
                    source_hash: row.get(4)?,
                    dest_hash: row.get(5)?,
                    synced_at: row.get(6)?,
                    verified_at: row.get(7)?,
                    deleted_at: row.get(8)?,
                    status: FileStatus::parse(&row.get::<_, String>(9)?)
                        .unwrap_or(FileStatus::Error),
                })
            })
            .optional()?;
        Ok(row)
    }

    pub fn list_verified_for_retention(
        &self,
        pair_id: &str,
        verified_before: i64,
    ) -> Result<Vec<FileRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT pair_id, path, size, mtime, source_hash, dest_hash,
                    synced_at, verified_at, deleted_at, status
             FROM files
             WHERE pair_id = ?1
               AND status = 'verified'
               AND verified_at IS NOT NULL
               AND verified_at <= ?2",
        )?;
        let rows = stmt.query_map(params![pair_id, verified_before], |row| {
            Ok(FileRecord {
                pair_id: row.get(0)?,
                path: row.get(1)?,
                size: row.get(2)?,
                mtime: row.get(3)?,
                source_hash: row.get(4)?,
                dest_hash: row.get(5)?,
                synced_at: row.get(6)?,
                verified_at: row.get(7)?,
                deleted_at: row.get(8)?,
                status: FileStatus::Verified,
            })
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    pub fn list_quarantined(
        &self,
        pair_id: &str,
        deleted_before: i64,
    ) -> Result<Vec<FileRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT pair_id, path, size, mtime, source_hash, dest_hash,
                    synced_at, verified_at, deleted_at, status
             FROM files
             WHERE pair_id = ?1
               AND status = 'quarantined'
               AND deleted_at IS NOT NULL
               AND deleted_at <= ?2",
        )?;
        let rows = stmt.query_map(params![pair_id, deleted_before], |row| {
            Ok(FileRecord {
                pair_id: row.get(0)?,
                path: row.get(1)?,
                size: row.get(2)?,
                mtime: row.get(3)?,
                source_hash: row.get(4)?,
                dest_hash: row.get(5)?,
                synced_at: row.get(6)?,
                verified_at: row.get(7)?,
                deleted_at: row.get(8)?,
                status: FileStatus::Quarantined,
            })
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    pub fn log_event(
        &self,
        pair_id: Option<&str>,
        path: Option<&str>,
        event_type: &str,
        detail: Option<&str>,
        created_at: i64,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT INTO events (pair_id, path, event_type, detail, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![pair_id, path, event_type, detail, created_at],
        )?;
        Ok(())
    }

    pub fn event_count(&self) -> Result<i64> {
        let count: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM events", [], |row| row.get(0))?;
        Ok(count)
    }

    pub fn db_path_hint() -> PathBuf {
        PathBuf::from("state.db")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upsert_and_get_file() {
        let store = StateStore::open_in_memory().unwrap();
        store
            .upsert_pair("p1", Path::new("/src"), Path::new("/dst"))
            .unwrap();
        let record = FileRecord {
            pair_id: "p1".into(),
            path: "a/b.bin".into(),
            size: 10,
            mtime: 100,
            source_hash: Some("aa".into()),
            dest_hash: Some("aa".into()),
            synced_at: Some(200),
            verified_at: Some(200),
            deleted_at: None,
            status: FileStatus::Verified,
        };
        store.upsert_file(&record).unwrap();
        let got = store.get_file("p1", "a/b.bin").unwrap().unwrap();
        assert_eq!(got.size, 10);
        assert_eq!(got.status, FileStatus::Verified);
        assert_eq!(got.source_hash.as_deref(), Some("aa"));
    }

    #[test]
    fn list_verified_respects_cutoff() {
        let store = StateStore::open_in_memory().unwrap();
        store
            .upsert_pair("p1", Path::new("/src"), Path::new("/dst"))
            .unwrap();
        for (path, verified_at) in [("old.bin", 100i64), ("new.bin", 500i64)] {
            store
                .upsert_file(&FileRecord {
                    pair_id: "p1".into(),
                    path: path.into(),
                    size: 1,
                    mtime: 1,
                    source_hash: Some("h".into()),
                    dest_hash: Some("h".into()),
                    synced_at: Some(verified_at),
                    verified_at: Some(verified_at),
                    deleted_at: None,
                    status: FileStatus::Verified,
                })
                .unwrap();
        }
        let rows = store.list_verified_for_retention("p1", 200).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].path, "old.bin");
    }

    #[test]
    fn log_event_persists() {
        let store = StateStore::open_in_memory().unwrap();
        store
            .log_event(Some("p1"), Some("x"), "test", Some("detail"), 1)
            .unwrap();
        assert_eq!(store.event_count().unwrap(), 1);
    }
}
