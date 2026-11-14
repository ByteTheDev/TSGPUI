// A small, ordinary Electron main process. Rendrix imports it unchanged:
//   rendrix import electron . --out ../electron-import-demo-rendrix
const { app, BrowserWindow, ipcMain } = require('electron');
const fs = require('fs');
const path = require('path');

function createWindow() {
  const win = new BrowserWindow({
    width: 760,
    height: 560,
    title: 'Electron Import Demo',
    webPreferences: { preload: path.join(__dirname, 'preload.js'), contextIsolation: true }
  });
  win.loadFile(path.join(__dirname, '../renderer/index.html'));
}

const noteFile = () => path.join(app.getPath('userData'), 'note.txt');

ipcMain.handle('note:save', async (_event, text) => {
  await fs.promises.mkdir(path.dirname(noteFile()), { recursive: true });
  await fs.promises.writeFile(noteFile(), String(text), 'utf8');
  return 'Saved';
});

ipcMain.handle('note:load', async () => {
  try {
    return await fs.promises.readFile(noteFile(), 'utf8');
  } catch {
    return '';
  }
});

app.whenReady().then(createWindow);
app.on('window-all-closed', () => {
  if (process.platform !== 'darwin') app.quit();
});
