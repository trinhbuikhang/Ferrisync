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

### Task 1: Core `compare_pair` API

**Files:** `crates/ferrisync-core/src/sync_engine.rs` (or `compare.rs`)

- [ ] Add `CompareRow` + `compare_pair(source, dest, store?) -> Vec<CompareRow>`
- [ ] Unit test: new/updated/unchanged classification
- [ ] Export from `lib.rs`

### Task 2: napi-rs package

**Files:** `packages/ferrisync-native/`

- [ ] Scaffold napi package linking `ferrisync-core`
- [ ] Expose `compare`, `sync`, `status`, `cleanup`
- [ ] `npm run build` produces `.node`

### Task 3: Electron app shell + FreeFileSync layout

**Files:** `apps/desktop/`

- [ ] Vite + React + Electron main/preload
- [ ] Layout: sidebar, dual panes, Compare/Sync, stats
- [ ] Fonts Inter + JetBrains Mono
- [ ] Folder pick via Electron dialog

### Task 4: Wire UI to native + tasks persistence

- [ ] IPC wrappers call napi from main or renderer (prefer main for native)
- [ ] Save/load tasks JSON in AppData

### Task 5: Installer + README

- [ ] electron-builder NSIS config
- [ ] Document `npm run dist` / run instructions
- [ ] Commit and push
