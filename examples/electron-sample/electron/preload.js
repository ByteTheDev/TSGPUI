const { contextBridge, ipcRenderer } = require('electron');
contextBridge.exposeInMainWorld('api', { saveNote: text => ipcRenderer.invoke('notes:save', text), loadNote: () => ipcRenderer.invoke('notes:load'), openFile: () => ipcRenderer.invoke('file:open'), fetchStatus: () => ipcRenderer.invoke('net:fetch'), notify: body => ipcRenderer.send('notify', body) });
