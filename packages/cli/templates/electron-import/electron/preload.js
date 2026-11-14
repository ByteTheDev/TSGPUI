const { contextBridge, ipcRenderer } = require('electron');

contextBridge.exposeInMainWorld('api', {
  saveNote: (text) => ipcRenderer.invoke('note:save', text),
  loadNote: () => ipcRenderer.invoke('note:load')
});
