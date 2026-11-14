# Changelog

## 0.5.0 — Rendrix becomes a rendering engine and application runtime (unreleased)

Direction change: Rendrix no longer targets Electron compatibility or a static HTML-subset compiler. It is
an independent engine that runs TypeScript applications with its own DOM, CSS engine, layout, GPU renderer
and runtime. See `docs/ENGINE_ASSESSMENT.md`.

### Added
- `engine/` Rust workspace: `rendrix-css`, `rendrix-dom`, `rendrix-layout`, `rendrix-gfx`, `rendrix-core`,
  `rendrix-js` (QuickJS behind a `JsEngine` trait), `rendrix-runtime` (winit window, headless driver,
  benchmark, standalone packaging).
- Web platform layer: DOM, events, forms/text editing, selectors, `fetch`, `URL`, storage, observers,
  transitions/animations, timers/`requestAnimationFrame`, images (see `docs/COMPATIBILITY.md`).
- New CLI: `rendrix init | dev | build | run | check | package | doctor` with TypeScript type-checking, TSX
  (`rendrix/jsx-runtime`), `node_modules` ESM resolution and `.rdxapp` / standalone executable output.
- `examples/app-counter` and a scripted end-to-end test (`npm run e2e`); `npm run bench`.

### Removed
- Electron compatibility layer, Electron project importer and their docs/examples.
- GPUI legacy host and the M0 static-tree renderer (`native/`), the build-time HTML/CSS compiler and the
  `{ state, actions }` backend model, old host fetching/packaging scripts, old CI workflows.
  Migration notes: `docs/ENGINE_ASSESSMENT.md` §4.

### Known limitations
Windows is the only verified platform; no accessibility tree; see the compatibility matrix.

## Unreleased — the Rendrix rewrite

**Rendrix renames from Rendrix; WebView renderer removed; Rendrix is the sole rendering engine.**
Owner-authorized decision of 2026-11-11; the full contract is
[docs/REWRITE_RENDRIX.md](docs/REWRITE_RENDRIX.md). The pre-rewrite implementation is preserved
at branch `backup/webview-era` (tag `webview-era-checkpoint`, commit `0fc860b`).

- **Developer experience.** Compile errors in HTML and CSS now print `file:line:col` (CSS no longer shows `<css input>`). `rendrix doctor` also checks the release manifest, can probe the download URL (`--online`) and checks the TypeScript/PostCSS toolchain. `rendrix init --template` adds `form`, `list-detail` and `electron-import` starters. The Electron importer resolves concatenated/templated/`url.format` `file:` page URLs, falls back to a root `index.html`, explains remote-only apps, and no longer flags `app.*` calls the runtime implements. Results on real apps: [docs/ELECTRON_IMPORT_RESULTS.md](docs/ELECTRON_IMPORT_RESULTS.md).
$2** Product Rendrix (was Rendrix); CLI package/command `rendrix`; bundle magic `RENDRI01`;
  home env var `RENDRIX_HOME`; renderer config key `"rendrix"` (the default). Entries below the
  Unreleased section describe releases as they shipped under the old name and are left as
  written.
- **WebView renderer removed.** `renderer: "webview"` is a hard error with migration guidance;
  the webview host binary, `webview-host` feature, `gtk`/`tao`/`wry` dependencies, page bridge,
  custom protocol, CDP e2e, and WebKitGTK CI dependencies are deleted. The Rendrix engine
  (`rendrix-host`, feature `rendrix`) is the only rendering engine. It is in **early beta**:
  feature coverage is limited and a long way from stable; state-driven re-render, hover/active/
  focus, scrolling, images, text editing, accessibility, and animations are on the M1–M3 ladder.
- **Deprecated legacy host.** `rendrix-legacy-host` (renderer key `"native"`) still works with a
  deprecation warning and is scheduled for removal at the Rendrix M1 gate.
- **Preserved.** The Electron-compat runtime, the updater (host-side download/verify/install),
  the QuickJS backend + permissions model, and the renderer-agnostic smoke/benchmark scripts
  all continue to work; they never depended on the WebView renderer.

## 0.4.0

First stable release of the 0.4.0 series, graduating the beta.4 work (full Electron compatibility coverage, native UI/menus, host builds). See the beta.2 notes below for the feature set.

## 0.4.0-beta.2

This is a beta. Interfaces may change before 0.4.0, and the platform hosts for macOS and Linux have been built by CI but not yet run by hand.

0.4.0-beta.2 supersedes 0.4.0-beta.1, which is deprecated. The beta.1 package was published with a Linux host build failure: the `xattr` dependency did not compile on Linux (fixed by vendoring `xattr` 0.2.3 under `native/patches`).

### Added

- **Electron import.** `rendrix import electron <dir>` converts an Electron project into a Rendrix project and writes a review report. `rendrix run`, `build` and the target shortcuts also accept Electron project directories. The importer does not modify the source project.
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
- Tray, global shortcuts, cursor position and display events are partial or unsupported. See the [coverage ledger](packages/cli/src/electron-compat/coverage.mjs).
- GPUI does not report a scale factor per display, so the native renderer reports the app window's scale factor for every display.
- The browser preview does not run the Electron shims.
- Clipboard, dialogs and power blockers work on Windows only.
- Accessibility: native `aria-*` and `role` metadata is preserved but not exposed to the OS. GPUI 0.2.2 has no accessibility API, so this needs an upgrade or bridge layer; scope before promising screen-reader support.
- Motion on the native path: CSS transitions, animations and transforms are reported as unsupported. A GPUI animation primitive would close this; otherwise use the WebView renderer for motion.
- Signing and update support: packages are produced unsigned by default; signing, notarization and in-app updates are handled outside the tool or remain roadmap work.

See [docs/SUPPORTED_SYNTAX.md](docs/SUPPORTED_SYNTAX.md) for the full `api.native` reference.
