const { contextBridge, ipcRenderer } = require('electron');

contextBridge.exposeInMainWorld('ferrisync', {
  pickFolder: () => ipcRenderer.invoke('dialog:pickFolder'),
  compare: (source, destination) =>
    ipcRenderer.invoke('native:compare', source, destination),
  sync: (source, destination) =>
    ipcRenderer.invoke('native:sync', source, destination),
  cleanup: (source, destination, forceEnabled, dryRun) =>
    ipcRenderer.invoke('native:cleanup', source, destination, forceEnabled, dryRun),
  appDataPath: () => ipcRenderer.invoke('native:appDataPath'),
  listTasks: () => ipcRenderer.invoke('tasks:list'),
  saveTasks: (tasks) => ipcRenderer.invoke('tasks:save', tasks),
});
