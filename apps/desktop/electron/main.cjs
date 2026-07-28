const { app, BrowserWindow, ipcMain, dialog } = require('electron');
const path = require('path');
const fs = require('fs');

function loadNative() {
  try {
    return require('ferrisync-native');
  } catch (err) {
    // Packaged / alternate path
    const candidates = [
      path.join(__dirname, '..', 'node_modules', 'ferrisync-native'),
      path.join(process.resourcesPath || '', 'app.asar.unpacked', 'node_modules', 'ferrisync-native'),
    ];
    for (const c of candidates) {
      try {
        return require(c);
      } catch (_) {
        /* continue */
      }
    }
    throw err;
  }
}

let native;
try {
  native = loadNative();
} catch (e) {
  console.error('Failed to load ferrisync-native:', e);
}

function tasksPath() {
  return path.join(app.getPath('userData'), 'tasks.json');
}

function readTasks() {
  try {
    const p = tasksPath();
    if (!fs.existsSync(p)) return [];
    return JSON.parse(fs.readFileSync(p, 'utf8'));
  } catch {
    return [];
  }
}

function writeTasks(tasks) {
  const p = tasksPath();
  fs.mkdirSync(path.dirname(p), { recursive: true });
  fs.writeFileSync(p, JSON.stringify(tasks, null, 2), 'utf8');
}

function createWindow() {
  const win = new BrowserWindow({
    width: 1280,
    height: 800,
    minWidth: 960,
    minHeight: 600,
    backgroundColor: '#e8eaed',
    webPreferences: {
      preload: path.join(__dirname, 'preload.cjs'),
      contextIsolation: true,
      nodeIntegration: false,
      sandbox: false,
    },
    title: 'Ferrisync',
  });

  if (!app.isPackaged) {
    win.loadURL('http://localhost:5173');
  } else {
    win.loadFile(path.join(__dirname, '..', 'dist', 'index.html'));
  }
}

app.whenReady().then(() => {
  ipcMain.handle('dialog:pickFolder', async () => {
    const result = await dialog.showOpenDialog({
      properties: ['openDirectory'],
    });
    if (result.canceled || !result.filePaths[0]) return null;
    return result.filePaths[0];
  });

  ipcMain.handle('native:compare', async (_e, source, destination) => {
    if (!native) throw new Error('Native module not loaded');
    return native.compare(source, destination);
  });

  ipcMain.handle('native:sync', async (_e, source, destination) => {
    if (!native) throw new Error('Native module not loaded');
    return native.sync(source, destination);
  });

  ipcMain.handle('native:cleanup', async (_e, source, destination, forceEnabled, dryRun) => {
    if (!native) throw new Error('Native module not loaded');
    return native.cleanup(source, destination, forceEnabled, dryRun);
  });

  ipcMain.handle('native:appDataPath', async () => {
    if (!native) return app.getPath('userData');
    return native.appDataPath();
  });

  ipcMain.handle('tasks:list', async () => readTasks());
  ipcMain.handle('tasks:save', async (_e, tasks) => {
    writeTasks(tasks);
    return true;
  });

  createWindow();
});

app.on('window-all-closed', () => {
  if (process.platform !== 'darwin') app.quit();
});
