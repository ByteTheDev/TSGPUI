# Changelog

## 0.4.0-beta.2

This is a beta. Interfaces may change before 0.4.0, and the platform hosts for macOS and Linux have been built by CI but not yet run by hand.

0.4.0-beta.2 supersedes 0.4.0-beta.1, which is deprecated. The beta.1 package was published with a Linux host build failure: the `xattr` dependency did not compile on Linux (fixed by vendoring `xattr` 0.2.3 under `native/patches`).

### Added

- **Electron import.** `tsgpui import electron <dir>` converts an Electron project into a TSGPUI project and writes a review report. `tsgpui run`, `build` and the target shortcuts also accept Electron project directories. The importer does not modify the source project.
- **Electron compatibility runtime.** `require('electron')`, `fs`, `path`, `os`, `child_process`, `https` and `net` are provided by shims. Every Electron 33 export is classified as implemented, partial or unsupported in the coverage ledger, and a test fails when the ledger is out of date.
- **Electron shims on the native renderer.** The native renderer runs the same shims through new `api.native` operations: `app`, `clipboard`, `dialog`, `shell`, `net`, `power` and `screen`.
- **Named filesystem roots.** `api.native.filesystem` operations accept `{ root, path }`. The root can be a permission root, `userData`, or a file the user picked in a dialog. Picked files are granted for that one file only.
- **WebView renderer.** Set `renderer: "webview"` to render with the platform browser engine (WebView2, WKWebView or WebKitGTK).
- **Prebuilt hosts and packaging.** Hosts for six targets are downloaded and verified against `hosts.json`. Windows packaging can produce a signed executable and an NSIS installer.

### Changed

- Clipboard, dialog, power-blocker, external-link and HTTPS code is shared by both hosts in `native/src/platform.rs`.
- The browser preview's `api.native` has the new operations, which reject with a message that they need the native or WebView host.
- `fs.readdir` returns names, as Electron does. `withFileTypes` returns entries with `isDirectory()` and `isFile()`. `fs.stat` reports `mtime` and `mtimeMs`.
- `userData` is created when the app starts, so the first write to it succeeds.

### Fixed

- A backend action can await a host call that was started while the backend loaded, such as `app.whenReady()`. Previously the action failed with "did not finish".

### Known limits

- The native renderer has one window and no page. `window.create`, `window.load` and `ipc.invoke` from the backend are unavailable. `webContents.send` messages are dropped with a warning, and close listeners cannot cancel a close.
- Tray, global shortcuts, `nativeImage` decoding, cursor position and display events are partial or unsupported. See the [coverage ledger](packages/cli/src/electron-compat/coverage.mjs).
- GPUI does not report a scale factor per display, so the native renderer reports the app window's scale factor for every display.
- The browser preview does not run the Electron shims.
- Clipboard, dialogs and power blockers work on Windows only.

See [docs/SUPPORTED_SYNTAX.md](docs/SUPPORTED_SYNTAX.md) for the full `api.native` reference.
