# Ferrisync Electron GUI Implementation Plan

> **For agentic workers:** Execute task-by-task. Checkboxes track progress.

**Goal:** Electron + React FreeFileSync-like GUI bound to `ferrisync-core` via napi-rs, with Windows NSIS installer.

**Architecture:** `apps/desktop` (Electron/Vite/React) → `packages/ferrisync-native` (napi-rs) → `ferrisync-core`.

**Tech Stack:** napi-rs, Electron 34+, Vite, React 19, electron-builder, Inter + JetBrains Mono.

## Global Constraints

- English identifiers/logs in Rust/native; UI labels may be English.
- One-way sync only; safe retention defaults unchanged.
- Spec: `docs/superpowers/specs/2026-07-28-ferrisync-electron-gui-design.md`

---

## Completion audit (2026-07-28)

| Task | Status | Evidence |
|------|--------|----------|
| 1. `compare_pair` API | **Done** | `crates/ferrisync-core/src/compare.rs`, exported in `lib.rs` |
| 2. napi-rs package | **Done (minor gap)** | `packages/ferrisync-native` exposes `compare` / `sync` / `cleanup` / `purgeQuarantineDry` / `appDataPath`. Dedicated `status` API not exposed (GUI uses compare stats instead). |
| 3. Electron shell + layout | **Done** | `apps/desktop` Vite+React+Electron, dual panes, Compare/Sync, Inter+JetBrains Mono, folder dialog |
| 4. IPC + tasks persistence | **Done** | `electron/main.cjs` + `preload.cjs`; `tasks.json` under Electron userData |
| 5. Installer + README | **Done** | `npm run dist` → `Ferrisync-Setup-0.1.0.exe`; README documents flow; `ensure-electron.cjs` for blocked postinstall |

**Plan verdict: essentially complete.** Remaining follow-ups live in `docs/superpowers/specs/2026-07-28-ferrisync-ffs-gap-roadmap.md` (Filter, progress, etc.), not in this MVP plan.

---

### Task 1: Core `compare_pair` API

**Files:** `crates/ferrisync-core/src/compare.rs`

- [x] Add `CompareRow` + `compare_pair(source, dest, store?) -> CompareResult`
- [x] Unit test: new/updated/unchanged classification (+ debug cases below)
- [x] Export from `lib.rs`

### Task 2: napi-rs package

**Files:** `packages/ferrisync-native/`

- [x] Scaffold napi package linking `ferrisync-core`
- [x] Expose `compare`, `sync`, `cleanup` (and `appDataPath`); `status` deferred — use compare summary
- [x] `npm run build` produces `.node`

### Task 3: Electron app shell + FreeFileSync layout

**Files:** `apps/desktop/`

- [x] Vite + React + Electron main/preload
- [x] Layout: sidebar, dual panes, Compare/Sync, stats
- [x] Fonts Inter + JetBrains Mono
- [x] Folder pick via Electron dialog

### Task 4: Wire UI to native + tasks persistence

- [x] IPC wrappers call napi from main
- [x] Save/load tasks JSON in AppData

### Task 5: Installer + README

- [x] electron-builder NSIS config
- [x] Document `npm run dist` / run instructions (+ ensure-electron)
- [x] Commit and push

---

## Debug test matrix

Run from repo root unless noted.

### Rust (core) — always

```bash
cargo test -p ferrisync-core
cargo test -p ferrisync-core compare -- --nocapture
cargo test -p ferrisync-core sync_folders -- --nocapture
```

| Case ID | What it catches | Command / test name |
|---------|-----------------|---------------------|
| R1 | New vs existing on dest | `compare::tests::compare_detects_new_and_unchanged` |
| R2 | Size change → Updated | `compare::tests::compare_marks_size_change_as_updated` |
| R3 | Nested relative paths | `compare::tests::compare_nested_relative_paths` |
| R4 | Missing source dir error | `compare::tests::compare_errors_when_source_missing` |
| R5 | Empty source folder | `compare::tests::compare_empty_source` |
| R6 | Spaces / unicode filenames | `compare::tests::compare_handles_special_filenames` |
| R7 | State says synced but dest missing | `compare::tests::compare_missing_on_dest_when_state_exists` |
| R8 | sync then compare → mostly unchanged | `sync_engine::tests::sync_folders_then_compare_is_stable` |
| R9 | Dest never left with `.ferrisync-tmp-*` | `sync_engine::tests::temp_file_not_left_as_final_on_success` |
| R10 | Corruption after sync fails re-verify | `sync_engine::tests::corruption_detected_on_reverify` |

### Native binding (Node) — after `npm run build` in `packages/ferrisync-native`

```bash
cd packages/ferrisync-native && npm run build && node scripts/smoke.cjs
cd apps/desktop && node scripts/smoke-native.cjs
```

| Case ID | What it catches |
|---------|-----------------|
| N1 | `.node` fails to load / wrong ABI |
| N2 | `compare` throws on bad source path |
| N3 | `sync` copies bytes and `compare` afterward has `toCopy==0` for identical trees |
| N4 | `appDataPath` returns non-empty path |
| N5 | Desktop `native-binding` copy out of date vs `packages/ferrisync-native` |

### Electron runtime

```bash
cd apps/desktop
node scripts/ensure-electron.cjs
npm run dev
```

| Case ID | Manual / scripted check |
|---------|-------------------------|
| E1 | `ensure-electron` prints path to `electron.exe` (fixes allowScripts miss) |
| E2 | Window opens; Browse source/dest works |
| E3 | Compare fills both panes; new files empty on right |
| E4 | Synchronize then Compare → fewer “to copy” |
| E5 | Log shows `error:` if native missing |
| E6 | Packaged: `release/win-unpacked/Ferrisync.exe` loads native (`ELECTRON_RUN_AS_NODE=1 … -e "require('ferrisync-native')"`) |

### Installer

```bash
cd apps/desktop && npm run dist
```

| Case ID | Check |
|---------|-------|
| I1 | `release/Ferrisync-Setup-0.1.0.exe` exists |
| I2 | Install on clean machine; Compare/Sync without Rust toolchain |
