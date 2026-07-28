# Ferrisync Electron GUI Design Spec

**Date:** 2026-07-28  
**Status:** Approved  
**Approach:** B — Electron + napi-rs binding to `ferrisync-core`

## Goals

1. Desktop GUI inspired by FreeFileSync (sidebar + dual panes + Compare/Sync), visually cleaner.
2. Typography close to Cursor IDE: **Inter** (UI) + **JetBrains Mono** (paths, file lists, log).
3. Keep all sync/verify/retention logic in Rust (`ferrisync-core`).
4. Ship a Windows **NSIS `.exe` installer** for machines without a Rust toolchain.

## Non-goals (this phase)

- Full FreeFileSync feature parity (bidirectional, mirror delete, cloud icons).
- macOS/Linux installers (Windows first).
- Replacing the CLI; CLI remains supported.

## Architecture

```
apps/desktop/                 Electron main + Vite React renderer
      │ IPC (preload bridge)
      ▼
packages/ferrisync-native/    napi-rs addon (cdylib)
      │
      ▼
crates/ferrisync-core/        scan, compare, sync, state, retention
```

### Native API surface (`ferrisync-native`)

| Function | Behavior |
|----------|----------|
| `compare(source, dest)` | Scan source, classify vs dest + state; return rows for both panes |
| `sync(source, dest)` | Run `sync_pair`; return stats |
| `status(source, dest)` | Summary counts from state DB vs source sample |
| `cleanup(source, dest, opts)` | Retention dry-run / live |
| `pickFolder()` | Not in napi — use Electron `dialog` in main process |

Session persistence: auto-save `gui-session.toml` under AppData (existing `Config::single_pair` / `default_gui_session_path`).

### Compare row model

```ts
type CompareRow = {
  relativePath: string;
  action: "new" | "updated" | "unchanged" | "missing_on_dest";
  sourceSize?: number;
  destSize?: number;
  sourceMtime?: number;
  destMtime?: number;
};
```

Left pane shows source-side entries; right pane shows dest-side (empty size when missing).

## UI layout

```
┌──────────────┬─────────────────────────────────────────────┐
│ Tasks        │     [Compare]   [Filter*]   [Synchronize]   │
│ New / rename ├──────────────────────┬──────────────────────┤
│              │ Source [Browse]      │ Dest [Browse]        │
│ Overview     │ Relative path │ Size │ Relative path │ Size │
│ items/size   │ …                    │ …                    │
└──────────────┴──────────────────────┴──────────────────────┘
│ Log / Statistics (copied, skipped, errors, bytes)          │
└────────────────────────────────────────────────────────────┘
```

\*Filter: MVP stub (show all); real filters later.

### Visual direction

- Light workspace (FreeFileSync familiarity) with refined spacing.
- Accent: Compare = steel blue `#2F6FED`; Sync = green `#2F9E62`; brand mark amber `#C9892A`.
- Avoid purple gradients / cream-serif AI defaults.
- Fonts: Inter 400/600 UI; JetBrains Mono 13px lists/paths.

### Tasks sidebar

- Local JSON list in AppData: `{ id, name, source, dest, lastSyncAt?, lastStatus? }`.
- Selecting a task fills path bars; Compare/Sync operate on those paths.

## Packaging

1. `cargo build -p ferrisync-native --release` (via napi build scripts).
2. `electron-builder --win nsis` produces `Ferrisync Setup x.y.z.exe`.
3. Native `.node` bundled into `app.asar.unpacked` / `extraResources` as required by napi-rs Electron packaging.

## Testing

- Unit: core `compare_pair` classification (Rust).
- Manual: run Electron against two temp folders; Compare then Sync; installer smoke on a clean VM when available.

## Migration

- `ferrisync-gui` (egui) kept as optional/dev; README points end-users to Electron app + installer.
