import { useCallback, useEffect, useMemo, useState } from 'react';
import type { TaskItem } from './vite-env';

type Row = {
  relativePath: string;
  action: string;
  sourceSize?: number | null;
  destSize?: number | null;
};

type LogLine = { kind: 'info' | 'ok' | 'err' | 'warn'; text: string };

function formatBytes(n?: number | null): string {
  if (n == null) return '—';
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  return `${(n / (1024 * 1024)).toFixed(2)} MB`;
}

function normalizeRows(raw: unknown): Row[] {
  if (!raw || typeof raw !== 'object') return [];
  const result = raw as {
    rows?: Array<Record<string, unknown>>;
  };
  return (result.rows ?? []).map((r) => ({
    relativePath: String(r.relativePath ?? r.relative_path ?? ''),
    action: String(r.action ?? ''),
    sourceSize: (r.sourceSize ?? r.source_size) as number | null | undefined,
    destSize: (r.destSize ?? r.dest_size) as number | null | undefined,
  }));
}

export default function App() {
  const [tasks, setTasks] = useState<TaskItem[]>([]);
  const [activeId, setActiveId] = useState<string | null>(null);
  const [source, setSource] = useState('');
  const [dest, setDest] = useState('');
  const [rows, setRows] = useState<Row[]>([]);
  const [busy, setBusy] = useState(false);
  const [logs, setLogs] = useState<LogLine[]>([
    { kind: 'info', text: 'Choose source and destination folders, then Compare.' },
  ]);
  const [stats, setStats] = useState({
    scanned: 0,
    toCopy: 0,
    unchanged: 0,
    copied: 0,
    errors: 0,
    bytes: 0,
  });

  const api = window.ferrisync;

  const pushLog = useCallback((kind: LogLine['kind'], text: string) => {
    setLogs((prev) => [...prev.slice(-400), { kind, text }]);
  }, []);

  const persistTasks = useCallback(
    async (next: TaskItem[]) => {
      setTasks(next);
      if (api?.saveTasks) await api.saveTasks(next);
    },
    [api],
  );

  useEffect(() => {
    (async () => {
      if (!api?.listTasks) return;
      const list = await api.listTasks();
      setTasks(list);
      if (list[0]) {
        setActiveId(list[0].id);
        setSource(list[0].source);
        setDest(list[0].dest);
      }
    })().catch((e) => pushLog('err', String(e)));
  }, [api, pushLog]);

  const activeTask = useMemo(
    () => tasks.find((t) => t.id === activeId) ?? null,
    [tasks, activeId],
  );

  const overview = useMemo(() => {
    const items = rows.length;
    const size = rows.reduce((acc, r) => acc + (r.sourceSize ?? 0), 0);
    return { items, size };
  }, [rows]);

  async function browse(side: 'source' | 'dest') {
    if (!api?.pickFolder) return;
    const folder = await api.pickFolder();
    if (!folder) return;
    if (side === 'source') setSource(folder);
    else setDest(folder);
  }

  async function onNewTask() {
    const id = crypto.randomUUID();
    const task: TaskItem = {
      id,
      name: `Task ${tasks.length + 1}`,
      source: '',
      dest: '',
    };
    await persistTasks([task, ...tasks]);
    setActiveId(id);
    setSource('');
    setDest('');
    setRows([]);
  }

  async function selectTask(task: TaskItem) {
    setActiveId(task.id);
    setSource(task.source);
    setDest(task.dest);
    setRows([]);
  }

  async function rememberPaths() {
    if (!activeId) {
      const id = crypto.randomUUID();
      const task: TaskItem = {
        id,
        name: source.split(/[/\\]/).filter(Boolean).pop() || 'Task',
        source,
        dest,
      };
      await persistTasks([task, ...tasks]);
      setActiveId(id);
      return task;
    }
    const next = tasks.map((t) =>
      t.id === activeId ? { ...t, source, dest } : t,
    );
    await persistTasks(next);
    return next.find((t) => t.id === activeId)!;
  }

  async function onCompare() {
    if (!api?.compare) {
      pushLog('err', 'Ferrisync API unavailable (open via Electron).');
      return;
    }
    if (!source || !dest) {
      pushLog('warn', 'Pick both source and destination folders.');
      return;
    }
    setBusy(true);
    pushLog('info', `COMPARE ${source} → ${dest}`);
    try {
      await rememberPaths();
      const result = await api.compare(source, dest);
      const normalized = normalizeRows(result);
      setRows(normalized);
      const scanned = Number(result.scanned ?? normalized.length);
      const toCopy = Number(
        result.toCopy ??
          normalized.filter((r) => r.action !== 'unchanged').length,
      );
      const unchanged = Number(result.unchanged ?? scanned - toCopy);
      setStats((s) => ({ ...s, scanned, toCopy, unchanged }));
      pushLog(
        'ok',
        `COMPARE done scanned=${scanned} to_copy=${toCopy} unchanged=${unchanged}`,
      );
    } catch (e) {
      pushLog('err', String(e));
    } finally {
      setBusy(false);
    }
  }

  async function onSync() {
    if (!api?.sync) {
      pushLog('err', 'Ferrisync API unavailable (open via Electron).');
      return;
    }
    if (!source || !dest) {
      pushLog('warn', 'Pick both source and destination folders.');
      return;
    }
    setBusy(true);
    pushLog('info', `SYNC ${source} → ${dest}`);
    try {
      const task = await rememberPaths();
      const result = await api.sync(source, dest);
      const copied = Number(result.copied ?? 0);
      const skipped = Number(result.skipped ?? 0);
      const errors = Number(result.errors ?? 0);
      const bytes = Number(result.bytesCopied ?? 0);
      setStats((s) => ({
        ...s,
        scanned: Number(result.scanned ?? s.scanned),
        copied,
        errors,
        bytes,
      }));
      pushLog(
        errors ? 'warn' : 'ok',
        `SYNC done copied=${copied} skipped=${skipped} errors=${errors} bytes=${bytes}`,
      );
      const next = tasks.map((t) =>
        t.id === task.id
          ? {
              ...t,
              source,
              dest,
              lastSyncAt: new Date().toISOString(),
              lastStatus: errors ? 'error' : 'ok',
            }
          : t,
      );
      await persistTasks(next);
      // Refresh compare view after sync
      const cmp = await api.compare(source, dest);
      setRows(normalizeRows(cmp));
    } catch (e) {
      pushLog('err', String(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="app">
      <aside className="sidebar">
        <div className="brand">
          <h1>FERRISYNC</h1>
          <p>one-way sync · BLAKE3 verify</p>
        </div>
        <div className="sidebar-toolbar">
          <button type="button" onClick={onNewTask}>
            New
          </button>
          <button
            type="button"
            onClick={() =>
              activeTask &&
              persistTasks(tasks.filter((t) => t.id !== activeTask.id)).then(() => {
                setActiveId(null);
                setSource('');
                setDest('');
                setRows([]);
              })
            }
          >
            Delete
          </button>
        </div>
        <div className="task-list">
          {tasks.length === 0 && <div className="empty">No saved tasks yet.</div>}
          {tasks.map((task) => (
            <button
              key={task.id}
              type="button"
              className={`task-item ${task.id === activeId ? 'active' : ''}`}
              onClick={() => selectTask(task)}
            >
              <div className="task-name">{task.name}</div>
              <div className="task-meta">
                {task.lastSyncAt
                  ? `Last sync ${new Date(task.lastSyncAt).toLocaleString()}`
                  : 'Never synced'}
              </div>
            </button>
          ))}
        </div>
        <div className="overview">
          <h3>Overview</h3>
          <table>
            <tbody>
              <tr>
                <td>Items</td>
                <td>{overview.items}</td>
              </tr>
              <tr>
                <td>Size</td>
                <td>{formatBytes(overview.size)}</td>
              </tr>
              <tr>
                <td>To copy</td>
                <td>{stats.toCopy}</td>
              </tr>
            </tbody>
          </table>
        </div>
      </aside>

      <main className="main">
        <div className="action-bar">
          <button
            type="button"
            className="btn btn-compare"
            disabled={busy}
            onClick={onCompare}
          >
            Compare
          </button>
          <button type="button" className="btn btn-filter" disabled title="Coming soon">
            Filter
          </button>
          <button type="button" className="btn btn-sync" disabled={busy} onClick={onSync}>
            Synchronize
          </button>
        </div>

        <div className="panes">
          <section className="pane">
            <div className="pane-header">
              <div className="pane-label">Source</div>
              <div className="path-row">
                <input
                  value={source}
                  onChange={(e) => setSource(e.target.value)}
                  placeholder="Folder to copy from"
                />
                <button type="button" onClick={() => browse('source')}>
                  Browse…
                </button>
              </div>
            </div>
            <div className="table-wrap">
              <table className="files">
                <thead>
                  <tr>
                    <th style={{ width: '75%' }}>Relative path</th>
                    <th>Size</th>
                  </tr>
                </thead>
                <tbody>
                  {rows.length === 0 && (
                    <tr>
                      <td colSpan={2} className="empty">
                        Run Compare to list source files.
                      </td>
                    </tr>
                  )}
                  {rows.map((r) => (
                    <tr key={`s-${r.relativePath}`} className={`action-${r.action}`}>
                      <td title={r.relativePath}>{r.relativePath}</td>
                      <td>{formatBytes(r.sourceSize)}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          </section>

          <section className="pane">
            <div className="pane-header">
              <div className="pane-label">Destination</div>
              <div className="path-row">
                <input
                  value={dest}
                  onChange={(e) => setDest(e.target.value)}
                  placeholder="Folder to copy to"
                />
                <button type="button" onClick={() => browse('dest')}>
                  Browse…
                </button>
              </div>
            </div>
            <div className="table-wrap">
              <table className="files">
                <thead>
                  <tr>
                    <th style={{ width: '75%' }}>Relative path</th>
                    <th>Size</th>
                  </tr>
                </thead>
                <tbody>
                  {rows.length === 0 && (
                    <tr>
                      <td colSpan={2} className="empty">
                        Destination preview appears after Compare.
                      </td>
                    </tr>
                  )}
                  {rows.map((r) => (
                    <tr key={`d-${r.relativePath}`} className={`action-${r.action}`}>
                      <td title={r.relativePath}>
                        {r.action === 'new' ? '' : r.relativePath}
                      </td>
                      <td>{r.action === 'new' ? '—' : formatBytes(r.destSize)}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          </section>
        </div>
      </main>

      <footer className="footer">
        <div className="log">
          {logs.map((l, i) => (
            <div key={`${i}-${l.text.slice(0, 12)}`} className={l.kind}>
              {l.text}
            </div>
          ))}
        </div>
        <div className="stats">
          <h3>Statistics</h3>
          <div className="stat-grid">
            <div className="stat">
              <div className="label">Scanned</div>
              <div className="value">{stats.scanned}</div>
            </div>
            <div className="stat">
              <div className="label">To copy</div>
              <div className="value">{stats.toCopy}</div>
            </div>
            <div className="stat">
              <div className="label">Copied</div>
              <div className="value">{stats.copied}</div>
            </div>
            <div className="stat">
              <div className="label">Bytes</div>
              <div className="value">{formatBytes(stats.bytes)}</div>
            </div>
          </div>
        </div>
      </footer>
    </div>
  );
}
