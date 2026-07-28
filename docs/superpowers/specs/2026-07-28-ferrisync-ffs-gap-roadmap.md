# Ferrisync ↔ FreeFileSync Gap Spec (Roadmap)

**Date:** 2026-07-28  
**Status:** Draft for future implementation  
**References:**
- FreeFileSync product / manual: https://freefilesync.org/
- FreeFileSync community: https://freefilesync.org/forum/
- Current Ferrisync Electron GUI: `apps/desktop/`
- Prior design: `docs/superpowers/specs/2026-07-28-ferrisync-electron-gui-design.md`

## Purpose

Capture differences between Ferrisync (as of Electron MVP) and FreeFileSync so later work can close **UI/UX gaps that matter** without accidentally copying FreeFileSync’s full sync model (especially destination deletes).

This is a **prioritized roadmap spec**, not a commitment to feature parity.

## Product positioning (do not dilute)

| | FreeFileSync | Ferrisync (intended) |
|--|--------------|----------------------|
| Job | General folder compare + sync / backup suite | One-way staging → NAS with integrity proof + safe source cleanup |
| Sync variants | Two way, Mirror, Update, Custom | **One-way Update-like only** (copy new/updated; never delete dest) |
| Integrity | Optional binary compare (often re-read) | **BLAKE3 streamed during copy** + SQLite verification state |
| Cleanup | Versioning on target overwrite/delete | **Retention on source** after verified (dry-run / quarantine) |

**Hard constraint:** Ferrisync must not gain silent Mirror/Two-way destination deletion unless product owners explicitly approve a later, separately gated feature with strong confirmations.

---

## Baseline: what Ferrisync already has

- Electron UI: sidebar tasks, dual Source/Dest panes, Compare → Synchronize, log + basic stats.
- Typography: Inter + JetBrains Mono.
- Core: one-way sync, fail-safe temp+rename, BLAKE3 verify, SQLite state, retention/quarantine APIs.
- Packaging: Windows NSIS installer path (`npm run dist`).

---

## Gap analysis

### A. Layout & interaction (visual parity with FreeFileSync)

| Gap | FreeFileSync | Ferrisync today | Priority |
|-----|--------------|-----------------|----------|
| A1 Filter | Real include/exclude | Button stub | P0 |
| A2 Per-row action affordance | Direction/category icons per file | Color only | P0 |
| A3 Sync progress | Live progress / cancel | Stats after completion | P0 |
| A4 Config sidebar richness | Profiles, last sync icons, save/open | New/Delete + text | P1 |
| A5 Overview panel | Folder / items / size tree-ish | Flat item+size counts | P1 |
| A6 Statistics by operation | + / update / delete counts | Scanned / to copy / copied / bytes | P1 |
| A7 Multi folder-pair in one job | Multiple pairs | One pair per task | P2 |
| A8 Drag-drop paths, explorer integration | Supported | Browse dialog only | P2 |

### B. Sync semantics (mostly out of scope / deferred)

| Gap | FreeFileSync | Ferrisync | Decision |
|-----|--------------|-----------|----------|
| B1 Mirror (delete on dest) | Yes | No | **Out of scope** unless explicit new product decision |
| B2 Two-way | Yes | No | **Out of scope** for NAS staging product |
| B3 Update-with-database “changes” | Yes (`sync.ffs_db`) | Partial (SQLite, not FFS change model) | Keep Ferrisync model; document as Update-like |
| B4 Custom per-category rules | Yes | No | P2 optional “exclude row from sync” only |
| B5 Versioning on dest overwrite | Yes | No | P2; optional later; do not confuse with source quarantine |

### C. Connectivity & platform

| Gap | FreeFileSync | Ferrisync | Priority |
|-----|--------------|-----------|----------|
| C1 FTP/SFTP/Google Drive/MTP | Yes | Local/SMB paths only | P3 |
| C2 macOS / Linux installers | Yes | Windows first | P2 |
| C3 Batch / RealTimeSync / scheduler | Yes | Manual GUI + CLI | P2 |
| C4 Locked files (VSS), NTFS ADS | Yes | Best-effort normal IO | P3 |

### D. Ferrisync advantages to preserve

| Advantage | Requirement for future UI work |
|-----------|--------------------------------|
| D1 Streamed BLAKE3 verify | Always show verify status in Compare/Sync results |
| D2 Queryable state DB | Status panel / per-file “verified at” |
| D3 Source retention + quarantine | Dedicated Cleanup UI with dry-run default |
| D4 Safe defaults | Never enable dest-delete or live retention without explicit confirm |

---

## Recommended implementation phases

### Phase 1 — UX closer to FreeFileSync without changing sync model (P0)

**Goal:** User who knows FreeFileSync understands Ferrisync in under a minute.

1. **Real filters (A1)**  
   - Include/exclude glob patterns (e.g. `*.tmp`, `**/Thumbs.db`).  
   - Apply on Compare and Sync.  
   - Persist per task.

2. **Per-row compare categories (A2)**  
   - Columns: relative path, size, category (`new` / `updated` / `unchanged` / missing-on-dest).  
   - Optional checkbox “include in sync” (default on for new/updated).

3. **Sync progress + cancel (A3)**  
   - Native/core emits progress events (files done, bytes, current path).  
   - UI progress bar + Cancel that stops worker pool cooperatively.

4. **Verify visibility (D1/D2)**  
   - After Sync, show verified count / error count.  
   - Optional “Show only problems” filter.

**Exit criteria:** Compare → filter → selective Sync → live progress works on a folder with ≥1k files.

### Phase 2 — Sidebar & stats polish (P1)

1. Task rename, duplicate, last status icon (ok / warn / error).  
2. Overview: items, bytes to copy, verified, errors.  
3. Statistics strip closer to FFS (+ create / update / skip / error).  
4. Cleanup (dry-run) button in UI wired to existing retention APIs.

**Exit criteria:** User can manage multiple NAS jobs and run dry-run cleanup from GUI.

### Phase 3 — Ops & packaging (P2)

1. CLI/GUI shared task export/import.  
2. Optional Windows Task Scheduler helper (document + export `.xml` / schtasks).  
3. macOS/Linux builds if demanded.  
4. Optional dest versioning folder (explicit opt-in, separate from source quarantine).

### Phase 4 — Explicitly deferred (P3 / product gate)

- Mirror / Two-way / dest deletes.  
- Cloud protocols (SFTP, Google Drive, MTP).  
- Full FreeFileSync binary-compare UX parity.  
- RealTimeSync equivalent.

Any Phase 4 item requires a new design approval with safety model updates.

---

## Non-goals for this roadmap

- Pixel-perfect FreeFileSync clone (icons, donation edition features, business licensing).  
- Replacing BLAKE3 verify with CRC-only or post-hoc binary compare as the primary integrity path.  
- Enabling destination deletion by default.

---

## Suggested tracking labels

- `ux-ffs-parity` — Phase 1–2 UI gaps  
- `integrity` — verify/status visibility  
- `retention-ui` — cleanup in Electron  
- `sync-model` — blocked without product gate (Mirror/Two-way)

## Success metric (product)

A FreeFileSync user can:

1. Pick source/dest like FFS,  
2. Compare and visually understand what will copy,  
3. Sync with progress,  
4. Trust that dest is never silently deleted,  
5. Optionally dry-run source cleanup after verified retention —

without needing the CLI for the happy path.
