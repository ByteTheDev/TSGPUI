const { app, BrowserWindow, ipcMain, dialog, Notification } = require('electron');
const path = require('path');
const fs = require('fs');
const https = require('https');
function createWindow() {
  const win = new BrowserWindow({ width: 960, height: 640, title: 'Electron Sample', webPreferences: { preload: path.join(__dirname, 'preload.js'), contextIsolation: true } });
  win.loadFile(path.join(__dirname, '../dist/index.html'));
}
ipcMain.handle('notes:save', async (_event, text) => { await fs.promises.writeFile(path.join(app.getPath('userData'), 'note.json'), JSON.stringify({ text })); return true; });
ipcMain.handle('notes:load', async () => JSON.parse(await fs.promises.readFile(path.join(app.getPath('userData'), 'note.json'), 'utf8')).text);
ipcMain.handle('file:open', async () => { const result = await dialog.showOpenDialog({ properties: ['openFile'] }); return result.canceled ? null : fs.promises.readFile(result.filePaths[0], 'utf8'); });
ipcMain.handle('net:fetch', () => new Promise((resolve, reject) => { https.get({ host: 'api.example.com', path: '/status' }, response => { let data = ''; response.on('data', chunk => data += chunk); response.on('end', () => resolve(data)); }).on('error', reject); }));
ipcMain.on('notify', (_event, body) => new Notification({ title: 'Electron Sample', body }).show());
app.whenReady().then(createWindow);
app.on('window-all-closed', () => { if (process.platform !== 'darwin') app.quit(); });
