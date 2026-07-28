# Ferrisync

One-way folder sync to a NAS (or any destination) with **streamed BLAKE3 verification** and optional **retention cleanup**.

## Desktop app (recommended)

FreeFileSync-inspired Electron UI bound to Rust via N-API (`ferrisync-native`).

### Dev

```bash
# 1) Build native addon (once / after Rust changes)
cd packages/ferrisync-native
npm install
npm run build
cd ../..

# 2) Run Electron UI
cd apps/desktop
npm install
# If npm warns about allow-scripts for electron, either approve them or just run:
#   node scripts/ensure-electron.cjs
npm run dev
```

`npm run dev` / `postinstall` runs `ensure-electron` so the Electron binary is downloaded even when npm blocks package postinstall scripts.

Flow: **Browse** source + destination → **Compare** → **Synchronize**.

Fonts: Inter (UI) + JetBrains Mono (paths/lists), similar to Cursor IDE.

### Windows installer (.exe)

```bash
cd packages/ferrisync-native && npm run build && cd ../..
cd apps/desktop
npm install
npm run dist
```

Output: `apps/desktop/release/Ferrisync-Setup-0.1.0.exe` (NSIS).

## Crates / packages

| Path | Role |
|------|------|
| `crates/ferrisync-core` | Sync, compare, hash, SQLite, retention |
| `crates/ferrisync` | CLI |
| `crates/ferrisync-gui` | Optional egui prototype |
| `packages/ferrisync-native` | napi-rs bridge |
| `apps/desktop` | Electron + React GUI |

## CLI

```bash
cargo build --release
cargo test
```

```bash
ferrisync sync --config ferrisync.toml
ferrisync status --config ferrisync.toml --pair survey-raw --path nested/file.bin
ferrisync cleanup --config ferrisync.toml
```

See `config.example.toml` for advanced CLI config. The GUI creates AppData session/state automatically.

## Sync model

- **One-way only** — never deletes destination files.
- **Fail-safe copy** — temp file + BLAKE3 during copy + atomic rename.
- **State** — SQLite under Ferrisync AppData, keyed by `(pair_id, path)`.

## Safety model for auto-deletion

Off by default (`enabled=false`, `dry_run=true`). A source file is removed/quarantined only when verified, past retention, metadata unchanged, and re-hash matches. Prefer quarantine mode.

## Docs

- `docs/superpowers/specs/2026-07-28-ferrisync-design.md`
- `docs/superpowers/specs/2026-07-28-ferrisync-electron-gui-design.md`
