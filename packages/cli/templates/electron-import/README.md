# Electron import demo

An ordinary Electron app: a main process, a preload script with `contextBridge`, IPC (`ipcMain.handle`
and `ipcRenderer.invoke`), `app.getPath('userData')` and file access. It has no Rendrix code in it.

Import and run it with Rendrix:

```sh
rendrix import electron . --out ../electron-import-demo-rendrix --build
rendrix run ../electron-import-demo-rendrix
```

Or run it in place; Rendrix converts it into a temporary project first:

```sh
rendrix run .
```

Read `RENDRIX_IMPORT_REPORT.md` in the output folder. It lists every Electron API the app uses and whether
Rendrix implements it fully, partly or not at all. To run the original under Electron, `npm install` then
`npm start`.
