# Ferrisync MVP Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship a Rust workspace (`ferrisync-core` + `ferrisync` CLI) that one-way syncs folder pairs with streamed BLAKE3 verification, SQLite state, and safe retention cleanup.

**Architecture:** In-process work queue with bounded copy/hash workers; global SQLite WAL state keyed by `(pair_id, path)`; retention is a separate conservative pipeline.

**Tech Stack:** Rust 2021, clap, serde/toml, rusqlite (bundled), blake3, jwalk, rayon/crossbeam, tracing, tempfile, assert_fs, proptest (optional).

## Global Constraints

- All code, comments, logs, identifiers in English.
- One-way sync only — never delete destination files.
- Copy via temp file + atomic rename; stream BLAKE3 during copy.
- Retention defaults: `enabled=false`, `dry_run=true`; prefer quarantine.
- `cargo test` must pass with no external NAS.
- Spec: `docs/superpowers/specs/2026-07-28-ferrisync-design.md`

---

## File Structure

```
Cargo.toml                          # workspace
crates/ferrisync-core/Cargo.toml
crates/ferrisync-core/src/lib.rs
crates/ferrisync-core/src/config.rs
crates/ferrisync-core/src/hasher.rs
crates/ferrisync-core/src/scanner.rs
crates/ferrisync-core/src/state_store.rs
crates/ferrisync-core/src/sync_engine.rs
crates/ferrisync-core/src/queue.rs
crates/ferrisync-core/src/retention.rs
crates/ferrisync-core/src/logging.rs
crates/ferrisync-core/src/error.rs
crates/ferrisync/Cargo.toml
crates/ferrisync/src/main.rs
crates/ferrisync/src/cli.rs
tests/integration/                  # workspace integration tests via ferrisync-core
docs/...
README.md
config.example.toml
.gitignore
```

---

### Task 1: Workspace scaffold + config module

**Files:**
- Create: `Cargo.toml`, `.gitignore`, `crates/ferrisync-core/Cargo.toml`, `crates/ferrisync-core/src/lib.rs`, `crates/ferrisync-core/src/error.rs`, `crates/ferrisync-core/src/config.rs`, `crates/ferrisync/Cargo.toml`, `crates/ferrisync/src/main.rs`, `config.example.toml`

**Produces:**
- `Config::load(path) -> Result<Config>`
- Types: `Config`, `PairConfig`, `RetentionConfig`, `CompareMode`, `RetentionMode`
- Defaults: retention `enabled=false`, `dry_run=true`, `retention_days=4`, `mode=Quarantine`

- [ ] **Step 1:** Create workspace Cargo.toml and crate stubs; add deps: serde, toml, thiserror, anyhow.
- [ ] **Step 2:** Write config unit tests for valid TOML, invalid TOML, and default retention flags.
- [ ] **Step 3:** Implement config parsing + validation (`pairs` non-empty ids unique; quarantine_dir required when enabled && mode=quarantine && !dry_run).
- [ ] **Step 4:** `cargo test -p ferrisync-core config` passes.
- [ ] **Step 5:** Commit: `feat: scaffold workspace and TOML config`

---

### Task 2: Hasher (BLAKE3 streaming)

**Files:**
- Create: `crates/ferrisync-core/src/hasher.rs`
- Modify: `crates/ferrisync-core/src/lib.rs`

**Produces:**
- `hash_reader<R: Read>(reader) -> Result<[u8; 32]>`
- `hash_file(path) -> Result<[u8; 32]>`
- `hex_encode(digest: &[u8; 32]) -> String`

- [ ] **Step 1:** Write tests: empty file, known BLAKE3 vector, determinism.
- [ ] **Step 2:** Implement streaming hasher with `blake3` crate.
- [ ] **Step 3:** `cargo test -p ferrisync-core hasher` passes.
- [ ] **Step 4:** Commit: `feat: add BLAKE3 streaming hasher`

---

### Task 3: State store (SQLite)

**Files:**
- Create: `crates/ferrisync-core/src/state_store.rs`

**Produces:**
- `StateStore::open(path) -> Result<StateStore>`
- `upsert_file`, `get_file`, `list_verified_for_retention`, `set_status`, `log_event`
- Schema as in design spec; WAL mode; `FileStatus` enum

- [ ] **Step 1:** Write tests using temp DB: migrate, upsert, get, status transition, event log.
- [ ] **Step 2:** Implement schema + CRUD with `rusqlite` (feature `bundled`).
- [ ] **Step 3:** `cargo test -p ferrisync-core state_store` passes.
- [ ] **Step 4:** Commit: `feat: add SQLite state store`

---

### Task 4: Scanner + compare classification

**Files:**
- Create: `crates/ferrisync-core/src/scanner.rs`
- Modify: sync types in `sync_engine.rs` stub if needed

**Produces:**
- `scan_pair(source) -> impl Iterator/Vec of FileMeta { relative_path, size, mtime }`
- `classify(source_meta, dest_meta_opt, state_opt) -> FileAction { New, Updated, Unchanged, MissingOnDest }`

- [ ] **Step 1:** Unit tests for classify matrix (new/updated/unchanged).
- [ ] **Step 2:** Implement `jwalk`-based scanner emitting relative paths.
- [ ] **Step 3:** Tests pass.
- [ ] **Step 4:** Commit: `feat: add scanner and compare classification`

---

### Task 5: Sync engine (fail-safe copy + streamed verify + queue)

**Files:**
- Create: `crates/ferrisync-core/src/sync_engine.rs`, `crates/ferrisync-core/src/queue.rs`, `crates/ferrisync-core/src/logging.rs`

**Produces:**
- `sync_pair(pair, store, opts) -> Result<SyncStats>`
- Fail-safe copy: write `dest/.ferrisync-tmp-<uuid>-<filename>`, hash both sides during copy, rename on match
- Bounded concurrency via semaphore
- Heartbeat via `tracing` every N seconds

- [ ] **Step 1:** Integration test: temp source → sync → dest bytes match + state `verified` + hashes set.
- [ ] **Step 2:** Implement copy+hash+rename + worker pool.
- [ ] **Step 3:** Integration test: corrupt dest after sync → re-verify path marks error / not verified; not a retention candidate.
- [ ] **Step 4:** Integration test: temp file mid-write never appears as final dest path.
- [ ] **Step 5:** Concurrency test with ~2000 small files.
- [ ] **Step 6:** Commit: `feat: sync engine with streamed BLAKE3 verify`

---

### Task 6: Retention + quarantine + purge

**Files:**
- Create: `crates/ferrisync-core/src/retention.rs`

**Produces:**
- `cleanup_pair(...) -> Result<CleanupStats>`
- `purge_quarantine(...) -> Result<PurgeStats>`
- JSONL audit writer
- Exact boundary tests for retention_days (at boundary qualifies; one second before does not)

- [ ] **Step 1:** Unit tests for candidate evaluation (age, mtime change, hash mismatch, dry_run).
- [ ] **Step 2:** Implement evaluation + quarantine move + permanent delete + audit log.
- [ ] **Step 3:** Integration: verified 5d ago → quarantined; 2d ago → untouched; mtime changed → skip+audit; dry_run → no FS change.
- [ ] **Step 4:** Purge only after `purge_grace_days`.
- [ ] **Step 5:** Commit: `feat: retention cleanup with quarantine and audit`

---

### Task 7: CLI + README

**Files:**
- Create: `crates/ferrisync/src/cli.rs`, `README.md`
- Modify: `crates/ferrisync/src/main.rs`

**Produces:**
- Subcommands: `sync`, `status`, `cleanup`, `purge-quarantine`
- README: config format, subcommands, safety model for auto-deletion

- [ ] **Step 1:** Implement clap CLI wiring to core APIs.
- [ ] **Step 2:** Write README safety section explicitly.
- [ ] **Step 3:** `cargo test` full suite green.
- [ ] **Step 4:** Commit: `feat: CLI and README`

---

## Spec coverage checklist

- [x] Sync fail-safe + streamed BLAKE3 → Task 5
- [x] SQLite state → Task 3
- [x] Retention rules + dry_run defaults → Task 6 + Task 1
- [x] Quarantine + purge → Task 6
- [x] Tests §3 → Tasks 2–6
- [x] README safety → Task 7
- [x] Queue/workers → Task 5
- [x] Library + CLI → Tasks 1, 7
