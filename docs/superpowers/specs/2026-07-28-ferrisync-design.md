# Ferrisync Design Spec

**Date:** 2026-07-28  
**Status:** Approved for implementation  
**Product:** NAS file sync tool with streamed BLAKE3 verification and retention cleanup

## Goals

Build a Rust CLI (MVP) backed by a reusable library that:

1. One-way syncs files from source folders to destination folders (typically NAS/SMB).
2. Verifies integrity by streaming BLAKE3 during the copy (source and dest digests compared immediately).
3. Persists sync/verification state in a global SQLite database.
4. Optionally removes or quarantines source files only after strict verified + retention + re-verify checks.

Long-term: Windows service and FreeFileSync-like GUI will consume the same library API. Out of MVP scope.

## Decisions

| Topic | Choice |
|-------|--------|
| Sync semantics | One-way copy only — never delete destination files |
| Delivery shape | CLI first; service/GUI later |
| Crate layout | `ferrisync-core` (library) + `ferrisync` (thin CLI) |
| State store | One global SQLite DB; rows keyed by `(pair_id, relative_path)` |
| Runtime | In-process work queue + bounded workers (no external broker) |
| Hash | BLAKE3 streamed during copy |
| Retention defaults | `enabled=false`, `dry_run=true`; when enabled, prefer `quarantine` |
| Language | All code, comments, logs, identifiers in English |

## Architecture

```
ferrisync (CLI)
    │
    ▼
ferrisync-core
    ├── config          TOML load + validation
    ├── scanner         parallel directory walk → enqueue compare jobs
    ├── queue           in-process SyncJob / RetentionJob channels
    ├── hasher          BLAKE3 streaming helpers
    ├── sync_engine     compare (size+mtime) + fail-safe copy + verify
    ├── state_store     SQLite WAL, migrations, CRUD
    ├── retention       candidate rules, quarantine, purge
    └── logging         tracing + heartbeat/progress + JSONL audit
```

### Sync pipeline

1. **Scan** source (and needed dest metadata) with parallel walker (`jwalk`).
2. **Compare** each relative path against state store and dest metadata (`size + mtime` by default).
3. **Enqueue** `SyncJob` for new/updated files only.
4. **Workers** (semaphore-bounded per destination):
   - Read source while hashing.
   - Write to temp file in dest directory while hashing.
   - Compare digests; on match, atomic rename to final path; on mismatch, leave no final file (or remove temp), mark `error`/`quarantined` as appropriate.
5. **Commit** state row: hashes, `synced_at`, `verified_at`, `status`.

### Retention pipeline (separate subcommand)

Conservative, low concurrency. A source file is a deletion candidate only if ALL hold:

1. `status == verified`
2. `now - verified_at >= retention_days`
3. Current source `(size, mtime)` still match values recorded at verification
4. Fresh source BLAKE3 still matches stored `dest_hash`

Modes: `quarantine` (move under `quarantine_dir` preserving relative path) or `permanent` (explicit opt-in).  
`dry_run=true` only logs (audit JSONL) what would happen.

## Data model

Global DB path default: platform app-data dir, e.g. `%APPDATA%/Ferrisync/state.db` (overridable in config).

```sql
CREATE TABLE folder_pairs (
  id            TEXT PRIMARY KEY,
  source        TEXT NOT NULL,
  destination   TEXT NOT NULL
);

CREATE TABLE files (
  pair_id       TEXT NOT NULL,
  path          TEXT NOT NULL,  -- relative path within the pair
  size          INTEGER NOT NULL,
  mtime         INTEGER NOT NULL, -- unix seconds, source mtime at last sync
  source_hash   TEXT,
  dest_hash     TEXT,
  synced_at     INTEGER,
  verified_at   INTEGER,
  deleted_at    INTEGER,
  status        TEXT NOT NULL,  -- pending|synced|verified|quarantined|deleted|error
  PRIMARY KEY (pair_id, path),
  FOREIGN KEY (pair_id) REFERENCES folder_pairs(id)
);

CREATE TABLE events (
  id            INTEGER PRIMARY KEY AUTOINCREMENT,
  pair_id       TEXT,
  path          TEXT,
  event_type    TEXT NOT NULL,
  detail        TEXT,
  created_at    INTEGER NOT NULL
);
```

Statuses: `pending`, `synced`, `verified`, `quarantined`, `deleted`, `error`.

## Config (TOML)

```toml
state_db = "C:/Users/me/AppData/Roaming/Ferrisync/state.db"  # optional
log_level = "info"
audit_log = "./logs/audit.jsonl"

[defaults]
concurrency = 4
retry_count = 3
retry_backoff_ms = 500
compare_mode = "size_mtime"  # or "hash" for force re-hash compare later

[defaults.retention]
enabled = false
dry_run = true
retention_days = 4
mode = "quarantine"          # quarantine | permanent
quarantine_dir = ""          # required when mode=quarantine and enabled
purge_grace_days = 14

[[pairs]]
id = "survey-raw"
source = "D:/Staging/Survey"
destination = "//NAS/Survey/Archive"
# optional per-pair overrides for concurrency / retention
```

## CLI

```
ferrisync sync [--config PATH] [--pair ID]
ferrisync status [--config PATH] [--pair ID] [--path REL]
ferrisync cleanup [--config PATH] [--pair ID]
ferrisync purge-quarantine [--config PATH] [--pair ID]
```

## Safety model (auto-delete)

A source file is removed or quarantined only when:

1. It was successfully copied and BLAKE3 matched (`verified`).
2. Retention period has elapsed since `verified_at`.
3. Source metadata is unchanged since verification.
4. Source content re-hashes to the stored `dest_hash` at cleanup time.
5. Retention is explicitly `enabled=true` (default off).
6. Unless `dry_run=false`, no filesystem mutation occurs (default dry-run on).
7. Default destructive mode is quarantine, not permanent delete.

Every skip/delete decision is written to the audit log.

## Error handling

- Transient I/O: retry with configurable count/backoff.
- Hash mismatch on copy: do not promote temp to final path; mark `error`; never treat as deletion candidate.
- Mid-write failure: temp file cleaned up; final dest path never holds partial content.
- Heartbeat logs every N seconds: scanned, copied, bytes, throughput, ETA.

## Testing

Unit: hasher, compare classification, retention date math (UTC, DST boundaries), config defaults.  
Integration (`tempfile`/`assert_fs`): full sync cycle, interrupted copy / atomic rename, corruption → not verified / not deletable, retention cases, quarantine+purge, concurrency consistency.  
No real NAS required.

## Out of scope (phase 2+)

- GUI (FreeFileSync-like)
- Windows service / scheduler daemon
- Mirror/delete-on-destination
- Bidirectional sync
