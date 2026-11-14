# Electron import demo

An ordinary Electron app: a main process, a preload script with `contextBridge`, IPC (`ipcMain.handle`
and `ipcRenderer.invoke`), `app.getPath('userData')` and file access. It has no TS-GPU code in it.

Import and run it with TS-GPU:

```sh
tsgpu import electron . --out ../electron-import-demo-tsgpu --build
tsgpu run ../electron-import-demo-tsgpu
```

Or run it in place; TS-GPU converts it into a temporary project first:

```sh
tsgpu run .
```

Read `TSGPU_IMPORT_REPORT.md` in the output folder. It lists every Electron API the app uses and whether
TS-GPU implements it fully, partly or not at all. To run the original under Electron, `npm install` then
`npm start`.
