# Ferrisync

Rust CLI/library for one-way folder sync to a NAS (or any destination), with **streamed BLAKE3 verification** during copy and an optional **retention cleanup** for verified source files.

## Crates

| Crate | Role |
|-------|------|
| `ferrisync-core` | Library: scan, queue/workers, copy+hash, SQLite state, retention |
| `ferrisync` | Thin CLI front-end |

## GUI (easy testing)

Native desktop console built with egui, calling `ferrisync-core` directly:

```bash
cargo run -p ferrisync-gui --release
```

1. Browse / reload a `ferrisync.toml` (or start from `config.example.toml`).
2. Pick a folder pair.
3. Use **Sync**, **Status**, **Cleanup (dry-run)** — live cleanup requires two confirmations.

## Build

```bash
cargo build --release
cargo test
```

Binary: `target/release/ferrisync` (or `ferrisync.exe` on Windows).

## Quick start

1. Copy `config.example.toml` to `ferrisync.toml` and set your folder pairs.
2. Run a sync:

```bash
ferrisync sync --config ferrisync.toml
```

3. Check a file’s status:

```bash
ferrisync status --config ferrisync.toml --pair survey-raw --path nested/file.bin
```

4. **Dry-run** retention (default). Review the audit log before enabling deletes:

```bash
ferrisync cleanup --config ferrisync.toml
```

## Subcommands

| Command | Purpose |
|---------|---------|
| `sync` | Compare (size+mtime), copy new/updated files, stream BLAKE3 verify, update state |
| `status` | Query the SQLite state store for a pair/path |
| `cleanup` | Evaluate retention candidates (respects `dry_run` / `enabled`) |
| `purge-quarantine` | Permanently remove quarantined files past `purge_grace_days` |

## Sync model

- **One-way only**: source → destination. Destination files are never deleted by Ferrisync.
- **Fail-safe copy**: write to `.ferrisync-tmp-<uuid>-<name>` in the destination directory, hash while copying, atomic rename only if digests match.
- **Concurrency**: bounded worker pool per run (`defaults.concurrency` or per-pair override).
- **State**: one global SQLite DB (default under the OS app-data directory for Ferrisync), keyed by `(pair_id, relative_path)`.

## Safety model for auto-deletion

Source cleanup is **off by default** and engineered to be hard to trigger accidentally.

A source file is quarantined or deleted only when **all** of the following are true:

1. `retention.enabled = true` in config (default: `false`).
2. `retention.dry_run = false` (default: `true` — dry-run only logs).
3. State status is `verified` (BLAKE3 matched at copy time).
4. `now - verified_at >= retention_days` (default 4 days, UTC unix timestamps).
5. Current source `(size, mtime)` still match the values stored at verification.
6. A **fresh** BLAKE3 of the source still matches the stored `dest_hash`.

Deletion modes:

- `quarantine` (recommended): move under `quarantine_dir`, preserving relative path. Then `purge-quarantine` removes files after `purge_grace_days`.
- `permanent`: immediate delete — must be set explicitly.

Every skip/act decision can be written to `audit_log` as JSON lines.

**Recommended rollout:** leave `enabled=false` → enable with `dry_run=true` and review audit output → set `mode=quarantine` with a real `quarantine_dir` → only then set `dry_run=false`.

## Config reference

See `config.example.toml`. Important defaults:

```toml
[defaults.retention]
enabled = false
dry_run = true
retention_days = 4
mode = "quarantine"
purge_grace_days = 14
```

## Roadmap (out of MVP)

- Windows service / scheduled daemon
- FreeFileSync-like GUI consuming `ferrisync-core`
