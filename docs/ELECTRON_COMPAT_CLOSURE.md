# Electron compatibility closure

This report tracks the remaining partial or unsupported Electron APIs in
`packages/cli/src/electron-compat/coverage.mjs` and the host support available in
`native/src` and `packages/cli/src/electron-compat/runtime`.

## (a) Implementable now with available primitives

These are implemented or implementable with the host operations and JS shims that
already exist. No Rust/GPUI change is required.

- `app` - lifecycle, `whenReady`, `quit`, `window-all-closed`, `getPath`, `getName`, `getVersion`.
- `BrowserWindow` - additional windows, local bounds/focus tracking, show/hide/fullscreen events, parent/modal on Windows, loadFile/loadURL, IPC relay.
- `Menu`, `MenuItem` - inspectable data objects, templates, disabled/hidden aliases.
- `net` - permission-checked HTTPS `request` with binary bodies, streaming, redirects and cancellation.
- `nativeImage` - PNG bytes/data URLs, dimensions for PNG/JPEG/GIF/WebP, `getAspectRatio`, `resize`, `crop`, `createFromBitmap`, `createFromPath` via the host filesystem.
- `nativeTheme` - `themeSource` and `updated` via renderer media queries.
- `webContents` - `fromId`, `getAllWebContents`, send/on/once/removeListener/id.
- `safeStorage` - Windows DPAPI async encrypt/decrypt.
- `MessageChannelMain`, `MessagePortMain` - in-process structured-clone channels.
- `screen` - cached display bounds, work areas, scale factors, `getCursorScreenPoint` after refresh, display add/remove/metrics events on refresh.
- `webFrame` - CSS zoom, insertCSS, executeJavaScript, frame basics.
- `powerSaveBlocker` - Windows `SetThreadExecutionState`, in-memory elsewhere.
- `Tray` - in-memory inspectable state and click/right-click/double-click events.
- `session` - cookies through `session.cookies` or an in-memory per-partition Map.
- `protocol` - in-page schemes from packaged assets.
- `globalShortcut` - in-memory register/query/unregister registry.
- `ipcMain`, `ipcRenderer` - invoke/send/on/once/removeListener/removeAllListeners, plus `postMessage`; `sendSync` warns because the host bridge is async.
- `contextBridge` - `exposeInMainWorld`.
- `dialog`, `shell`, `clipboard`, `Notification` - host-backed dialogs, external URL, clipboard, notifications.

## (b) Need a GPUI/platform upgrade

These require new native host operations or platform APIs before they can behave like Electron.

- `screen.getCursorScreenPoint` live global cursor coordinates - needs a host op backed by platform cursor APIs (`GetCursorPos`/equivalent); currently cached after refresh.
- Live display add/remove/metrics events - needs display-change callbacks from the OS/GPUI; currently emitted only by `screen.refresh()`.
- Per-display scale factors - GPUI exposes one scale for the app window, so all displays report the window scale.
- Real window chrome for `BrowserWindow` show/hide/fullscreen/focus/bounds - the WebView host applies these, but the native GPUI host tracks them locally.
- OS-level global shortcut delivery - requires registering with the OS keyboard hook.
- OS tray chrome - requires a native tray implementation.
- OS theme state and events - requires OS theme notifications instead of renderer media queries.
- `nativeImage` real pixel resampling/encoding and multiple scale factors.
- Synchronous IPC (`sendSync`) - requires a synchronous host bridge.
- Parent/modal windows on non-Windows platforms.
- `protocol` remote/http proxy protocols and privileged-scheme semantics.
- Multi-view layout (`BaseWindow`, `BrowserView`, `View`, `WebContentsView`) - requires a GPUI view hierarchy.
- `desktopCapturer`, `ShareMenu`, `systemPreferences` - require OS capture/share/preference APIs.

## (c) Permanently unsupported with reasons

- `autoUpdater` - TSGPUI ships updates through its release process, not in-app update checks.
- `TouchBar` - macOS Touch Bar hardware only; no cross-platform equivalent.
- `inAppPurchase` - store billing is platform/store specific and not provided by the host.
- `pushNotifications` - remote push requires platform push services; local notifications are supported via `Notification`.
- `net.fetch` - use `net.request`, which is permission-checked and host-backed.
- `contentTracing`, `netLog` - tracing/logging internals are not exposed by the host.
- `crashReporter` upload - only local crash logging is available; uploads are not supported.
- `utilityProcess` - there is no Node module runtime; use `child_process`/`process.spawn`.
- `webUtils.getPathForFile` - file paths are not exposed to web content for security.
- `app` dock badge/login-item/single-instance APIs - not provided by the current host.
