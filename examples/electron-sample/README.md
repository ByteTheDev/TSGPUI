# Electron Sample

A small Electron notes app exercising IPC, file dialogs, HTTPS and notifications. Run `npm run build` before `npm start`.

> Rendrix BrowserWindow note: `examples/electron-sample-rendrix` uses only `new BrowserWindow({width:960,height:640,title:'Electron Sample'})` and `win.loadFile(dist/index.html)`. The Rendrix shim's first-window `loadFile` is a no-op (config `html` loads the page; extra windows would need `permissions.window:true`, already set). `show/hide/fullscreen/setBounds/move` are unsupported and must not be assumed — see `RENDRIX_IMPORT_REPORT.md` (BrowserWindow partial).
