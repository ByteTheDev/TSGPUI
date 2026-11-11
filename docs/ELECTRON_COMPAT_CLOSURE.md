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
- `nativeImage` - decoded PNG/JPEG/GIF/WebP pixels in the host, real `resize`/`crop`/`toPNG`/`toJPEG`/`toDataURL`, `getSize`/`getAspectRatio`, `createFromBitmap`, `createFromPath` through the host filesystem grants. See [NATIVE_IMAGE.md](NATIVE_IMAGE.md).
- `nativeTheme` - `themeSource` and `updated` via renderer media queries.
- `webContents` - `fromId`, `getAllWebContents`, send/on/once/removeListener/id; load events (`did-start-loading`/`did-finish-load`/`did-fail-load`/`did-stop-loading`), title/URL/loading state, `stop`/`reload`, CSS zoom, `findInPage` stub and silent DevTools no-ops.
- `safeStorage` - Windows DPAPI async encrypt/decrypt.
- `MessageChannelMain`, `MessagePortMain` - in-process structured-clone channels.
- `screen` - cached display bounds, work areas, scale factors, `getCursorScreenPoint` after refresh, and host-polled display add/remove/metrics events.
- `powerMonitor` - host power state (AC/battery, idle, thermal, lock) and suspend/resume/on-ac/on-battery/lock/unlock events. Windows uses native power notifications; macOS and Linux poll their platform state.
- `systemPreferences` - dark mode, accent color, system colors and reduced-motion/contrast/transparency flags from Windows registry/SystemParametersInfo, macOS defaults and Linux gsettings, with media-query and palette fallbacks.
- `webFrame` - CSS zoom, insertCSS, executeJavaScript, frame basics; main-frame `send`/`postMessage` reach the page.
- `powerSaveBlocker` - Windows `SetThreadExecutionState`, in-memory elsewhere.
- `Tray` - in-memory inspectable state and click/right-click/double-click events.
- `session` - host-backed or in-memory cookies, with persistent `persist:` partitions mirrored to a JSON file under `userData`; storage clearing, permission handlers, user agent, webRequest listener registry and download item stubs.
- `protocol` - in-page schemes from packaged assets, remote proxy responses through the permission-checked HTTPS transport, binary/streaming bodies and recorded privileged-scheme flags.
- `globalShortcut` - in-memory register/query/unregister registry.
- `ipcMain`, `ipcRenderer` - invoke/send/on/once/removeListener/removeAllListeners, plus `postMessage`; `sendSync` warns because the host bridge is async.
- `contextBridge` - `exposeInMainWorld`.
- `dialog`, `shell`, `clipboard`, `Notification` - host-backed dialogs, external URL, clipboard, notifications.

## (b) Need a GPUI/platform upgrade

These require new native host operations or platform APIs before they can behave like Electron.

- `screen.getCursorScreenPoint` live global cursor coordinates - needs a host op backed by platform cursor APIs (`GetCursorPos`/equivalent); currently cached after refresh.
- Per-display scale factors - GPUI exposes one scale for the app window, so all displays report the window scale.
- Real window chrome for `BrowserWindow` show/hide/fullscreen/focus/bounds - the WebView host applies these, but the native GPUI host tracks them locally.
- OS-level global shortcut delivery - requires registering with the OS keyboard hook.
- OS tray chrome - requires a native tray implementation.
- `nativeImage` multiple scale factors and representations.
- Synchronous IPC (`sendSync`) - requires a synchronous host bridge.
- Parent/modal windows on non-Windows platforms.
- `protocol` browser-level privileged-scheme semantics (real CORS/fetch/CSP/service-worker/code-cache behaviour) - the shim records the flags and applies secure/CORS to its own dispatch, but those browser behaviours need WebView host support.
- Multi-view layout (`BaseWindow`, `BrowserView`, `View`, `WebContentsView`) - requires a GPUI view hierarchy.
- `desktopCapturer`, `ShareMenu` - require OS capture/share APIs.
- macOS `NSWorkspace` sleep/lock notifications and Linux logind signals for exact suspend/lock delivery - current delivery is polled.

## (c) Permanently unsupported with reasons

- `autoUpdater` - TSGPUI ships updates through its release process, not in-app update checks.
- `TouchBar` - macOS Touch Bar hardware only; no cross-platform equivalent.
- `inAppPurchase` - store billing is platform/store specific and not provided by the host.
- `pushNotifications` - remote push requires platform push services; local notifications are supported via `Notification`.
- `net.fetch` - use `net.request`, which is permission-checked and host-backed.
- `contentTracing` - Chromium tracing internals are not exposed; the shim writes a Chrome tracing-event JSON timeline with TSGPUI markers.
- `netLog` - Chromium socket/DNS/QUIC internals are not exposed; the shim records `net.request` traffic in a JSON log.
- `crashReporter` upload and native minidumps - parameters and `captureCrash` dumps are written under `userData/crashes`; uploads and native crash dumps are not supported.
- `utilityProcess` messaging - fork starts a child process through the host, but there is no Node module runtime, `postMessage` port or transferable support.
- `webUtils.getPathForFile` - file paths are not exposed to web content for security.
- `app` dock badge/login-item/single-instance APIs - not provided by the current host.
