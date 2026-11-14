# Supported HTML, CSS, and TypeScript

The Rendrix compiler turns a documented subset of HTML, CSS, and TypeScript into a `.rdx` bundle. One rendering engine reads it: **Rendrix**, the Rendrix engine.

> **Rendrix is in early beta.** Feature coverage is limited — state-driven re-render, hover/active/focus states, scrolling, images, text editing, accessibility, and animations are still on the [Rendrix roadmap](RENDERER_RENDRIX.md). It is a long way from stable. If you relied on the removed WebView renderer, consult the pre-rewrite implementation at branch `backup/webview-era` (tag `webview-era-checkpoint`, commit `0fc860b`).

## Renderer selection

Set the project renderer in `rendrix.config.json`:

```json
{
  "renderer": "rendrix"
}
```

`renderer` accepts `rendrix` (the default; omitting the key selects it too) or `native`, which selects the deprecated legacy host and prints a deprecation warning until the Rendrix M1 gate removes it. `webview` is a hard error: the WebView renderer was removed in the rewrite. `rendrix dev` and `rendrix build` select the matching host automatically.

### Removed: WebView renderer

The WebView renderer (Wry on WebView2, WKWebView, and WebKitGTK) was deleted with the rewrite. Behavior that only that renderer had is preserved here as history; the full pre-rewrite text of this document is on branch `backup/webview-era`:

- Multi-page bundles via `webviewPages` (project-relative paths), an app-local custom URL served through a `rendrix` custom protocol with 404 on missing assets ([`webview_main.rs`](../native/src/webview_main.rs)), and a 256 MiB default `maxBundleSize`.
- The injected page bridge `window.rendrix.invoke` and `window.rendrix.on` ([`webview_bridge.rs`](../native/src/webview_bridge.rs)); window operations executed on the UI thread, other host requests ran on one worker thread, and IPC send and invoke were dispatched in the page and required IPC permission.
- Backend state surviving a `dev` reload in `sessionStorage`, or across app restarts in `localStorage` with `"persist": true`.
- A generated page content security policy: build nonces for script elements, `unsafe-eval` for the backend bootstrap, inline styles allowed, `connect-src` limited to the page origin and the configured HTTPS network hosts under browser CORS rules. See [SAFETY.md](SAFETY.md).

Use `rendrix import electron <dir> [--out DIR] [--build] [--force] [--fix]` to import an Electron project, or pass the project directory to `rendrix run`, `rendrix build` or a target shortcut, which converts it into a new temporary directory first. `dev`, `compile` and `preview` expect a Rendrix project. The source project is not modified; the importer writes `RENDRIX_IMPORT_REPORT.md` and prints its path. Review the report: each Electron API the app uses is checked against the [coverage ledger](../packages/cli/src/electron-compat/coverage.mjs), and partial or unsupported APIs are listed. `--build` runs the source project's build script, and a conversion with no built renderer runs it as well ([`import-electron.mjs`](../packages/cli/src/import-electron.mjs)).

The Electron `BrowserWindow` shim provides `show()`, `hide()`, `isVisible()`, `setFullScreen()`/`isFullScreen()`/`setFullScreenable()`, `setBounds()`/`setPosition()`/`setSize()`/`getBounds()`/`getSize()`/`getPosition()`, `isMaximized()`/`isMinimized()`, and `focus()`/`blur()` for easy migration. `ready-to-show` is emitted after creation and after each `loadFile()`/`loadURL()`; `show()`/`hide()` emit `show`/`hide` and `setFullScreen()` emits `enter-full-screen`/`leave-full-screen` on additional (non-primary) windows, which also route via `window.show`/`window.hide`/`window.setFullScreen` and track `isVisible()`/`isFullScreen()` from the last requested state. `setBounds()`/`setPosition()`/`setSize()` track bounds locally, emit `will-move`/`will-resize` stubs before `move`/`resize`, and route via `window.setBounds`/`window.setPosition`/`window.setSize` on additional windows (the primary window is tracked locally only, without moving native chrome). `focus()`/`blur()` update the local focused window and route via `window.focus`/`window.blur` on additional windows; `isMaximized()`/`isMinimized()` report local state updated by `minimize()`/`maximize()`/`restore()` and host `focus`/`blur`/`move`/`resize` events, and `getFocusedWindow()` returns the locally focused window. The primary window's `show()`/`hide()` resolve without a host call and `setFullScreen()` throws `BrowserWindow fullscreen not supported on this renderer`. See the coverage ledger for the full `BrowserWindow` note, including the stub nature of `will-move`/`will-resize` (no geometry is carried) and the remaining parent/modal, preload, and IPC limits.

The Electron `session` shim provides `session.defaultSession`, `session.fromPartition(partition)` and `fromPath(path)`. Cookie control is available as `defaultSession.cookies.get/set/remove` (with `getCookies`/`setCookies`/`clearCookies` aliases). Calls resolve via `host('session.cookies')` when the host implements it; otherwise cookies are held in an in-memory `Map` per partition, and `persist:` partitions also mirror non-session cookies to a JSON file under `userData` so they survive restarts. `clearStorageData`/`clearData` filter storage types and fall back to cookies plus current-page `localStorage`; permission request/check handlers are stored and invocable through `requestPermission`/`checkPermission`; `setUserAgent` applies to the host or the current page; `webRequest` records listeners without interception; and `downloadURL`/`will-download` plus `createInterruptedDownload` provide `DownloadItem` lifecycle stubs. The importer maps `new BrowserWindow({ webPreferences: { partition } })`, `session.defaultSession`, `session.getSession`, or any `cookies`/`getCookies`/`setCookies`/`clearCookies` usage to `"persist": true` so page state (via `localStorage`) and cookies survive restarts. Proxy, cache, extension and network-emulation controls remain host-dependent stubs. See the coverage ledger and [`PROTOCOL_SESSION.md`](PROTOCOL_SESSION.md) for the full notes.

The Electron `protocol` shim handles in-page custom schemes (`app`, `file`, and allowlisted custom schemes). `protocol.handle(scheme, handler)` and `protocol.registerFileProtocol(scheme, handler?)` record a handler, and `BrowserWindow.loadURL('app://...')` / `loadURL('file://...')` rewrite the URL onto the host's packaged `rendrix://localhost/` space, so `app://localhost/index.html` loads the packaged document (`webviewPages`, the multi-page list, was a webview-era feature and is historical). Handlers may return paths, `{data, mimeType}` buffers, response-like objects, or `{url, method, headers, uploadData}` remote proxies that run through the permission-checked HTTPS transport; binary and streaming bodies (ReadableStream/async iterables) are normalized. `protocol.registerSchemesAsPrivileged` validates and records standard/secure/CORS/fetch/CSP/service-worker/stream/code-cache flags, with `secure` and `corsEnabled` applied during dispatch; browser-level CORS, service-worker and CSP behaviour was the removed WebView renderer's, and those flags are historical for the in-tree engines. `protocol.unhandle`, `isProtocolHandled`/`isProtocolRegistered`/`isProtocolIntercepted`, and `unregisterProtocol` manage the registry. Handlers that compute paths dynamically still run through `dispatchRequest`; the importer allowlists literal schemes into `protocolSchemes` in `rendrix.config.json` and warns when a scheme is dynamic. Remote schemes (`http`, `https`, `ws`, `wss`) cannot be used with `handle`/`register*Protocol`: HTTPS uses the permission-checked `permissions.network` transport, and `registerHttpProtocol` for a remote scheme throws. `intercept*Protocol` does accept remote schemes for explicit `dispatchRequest` calls, but the removed WebView renderer's page network was not intercepted. See [`PROTOCOL_SESSION.md`](PROTOCOL_SESSION.md) for examples and the remaining limits.

It supports `data-bind`, `{{state.path}}` text interpolation, `data-onclick`, `data-oninput`, forms with `data-submit`, `data-fetch`, file selection/read/save, and clipboard bindings. The removed WebView renderer accepted a smaller binding subset (no `data-repeat`, `data-virtual`, compile-time components, native window controls, or per-element CSS warnings) and kept backend state in `sessionStorage` (or `localStorage` with `"persist": true`). That behavior is historical.

(Historical, WebView renderer:) the host installed `window.rendrix.invoke(op, args)` and `window.rendrix.on(channel, handler)` before page scripts, the backend used `__rendrix_host` and `__rendrix_emit`, and IPC was routed in the page when `permissions.ipc` was enabled. Window operations required `permissions.window`; filesystem roots and processes required their respective grants. Windows provided dialog and clipboard operations via Windows Forms; these operations on other platforms reject as unsupported. `notifications.show` is supported on the native renderer (desktop toast, or a notification on macOS and Linux). On the removed WebView renderer the host additionally showed an OS toast via notify-rust (WinRT toast on Windows, Notification Center on macOS, D-Bus/Notifications on Linux) when `permissions.notifications` was set, with `Notification.show()` in the Electron shim logging the failure, recording a compatibility warning, and falling back to an in-page toast; that fallback never threw, so `show()` kept its void, non-throwing contract. That path is historical.

The remaining HTML, CSS, and backend reference describes the Rendrix engine and the deprecated GPUI renderer unless a section says otherwise.

## HTML

Supported elements: `html`, `body`, `main`, `header`, `footer`, `nav`, `aside`, `section`, `article`, `div`, `h1` through `h6`, `p`, `span`, `strong`, `small`, `blockquote`, `pre`, `code`, `figure`, `figcaption`, `details`, `summary`, `table`, `caption`, `thead`, `tbody`, `tfoot`, `tr`, `th`, `td`, `button`, `label`, `input`, `textarea`, `select`, `option`, `form`, `ul`, `ol`, `li`, `img`, `menu`, `menuitem`, `hr`, `template`, `use`, and `slot`.

Semantic and table elements currently render through the native container layout; use CSS flex or grid to control their layout. The renderer does not yet provide browser table algorithms, disclosure behavior, or heading defaults.

Common attributes include `id`, `class`, `title`, `aria-label`, `role`, `tabindex`, `name`, and `value`. Controls also support `type`, `placeholder`, `checked`, `selected`, `disabled`, `required`, `min`, `max`, and `step`.

- `data-bind="stateKey"` displays backend state. Text supports `{{stateKey}}` interpolation. Dotted paths work for nested objects and repeated items.
- `data-onclick="actionName"` runs a button action. `data-oninput="actionName"` runs when an input, textarea, or select value changes. Actions receive an event object with `type` plus fields such as `value`, `checked`, `name`, and `submit`.
- `<form data-submit="save">` submits through a `button type="submit"`. Required fields and numeric `min`, `max`, and `step` constraints are checked before dispatch.
- Single-line text inputs support typing through the platform text input (including IME composition), a blinking caret, selection by mouse or Shift+arrow, Home/End, word-wise movement and deletion, Ctrl/Cmd+A/C/X/V/Z, Shift+Ctrl/Cmd+Z and Ctrl/Cmd+Y, Tab focus traversal, and password masking. Undo history is per field, bounded, and reset on app reload. Textareas use the same native text path: multi-line caret and selection, IME, clipboard, wrapping to the field width, scrolling by wheel and by caret, Enter for a new line, Up/Down by visual row, Home/End by line, Ctrl/Cmd+Home/End for the whole text, and double-click to select a word. The default height is about three lines unless the element sets `height`.
- A select opens a native dropdown popup on click. Up/Down keys also change the value, and Escape closes the popup.
- `data-repeat="items as item"` renders one node for each array element in backend state. Use `{{item.name}}` for item values. Nested repeats and state-bound inputs use the array index in their state path. Add `data-virtual="36"` (a row height in pixels, from 1 to 1000) to build only the visible rows of a scrolling list. Every row must be exactly that height, and rows fill the list's width unless they set one. Without `data-virtual`, every row is built.
- `<template data-component="Card">...</template>` and `<use component="Card" data-prop-title="Hello"/>` expand static reusable components at compile time. A component can read string props with `{{props.title}}`. Recursive components and duplicate expanded IDs are rejected.

- Slots: `<slot></slot>` inside a template receives the children written between `<use>` tags. `<slot name="footer">Default</slot>` receives children marked `slot="footer"`. Text inside a slot is its fallback, used when the caller gives nothing. Content with no matching slot is an error. `<use .../>` with no children is self-closed, and `<use ...>...</use>` takes slot content.
- Attributes written on `<use>`, such as `class`, `data-bind`, `data-oninput`, `data-onclick`, `placeholder`, or `title`, go onto the component's first element. `class` names are merged. The element is checked again, so `data-onclick` on a component whose first element is not a `<button>` is an error.
- Slot content keeps the props of the place where it is written. `{{props.x}}` in content written at the top level is empty, and a nested component's own `{{props.*}}` text shows that component's props.
- Built-in library: `Sidebar`, `Toolbar` (slots `start` and `end`; `title` is the fallback text), `SegmentedControl`, `SearchField` (an `<input type="search">`, so `data-bind` and `data-oninput` go on the `<use>`), `ListRow` (`title`, with a default slot for detail text), `Table`, `TableHeader`, `TableRow`, `TableHeaderCell`, `TableCell`, `CodeEditor` (a styled textarea that accepts the usual input bindings), `EmptyState` (`title` plus a default slot), and `Sheet` (`title`, with slots for the body and `footer`). Compose table rows from header/data cells and put them inside `Table`. These are native-rendered layout components; table roles are metadata and do not create a screen-reader accessibility tree in GPUI 0.2.2. An app template with the same name replaces a library entry. Library styles are plain `.ts-*` class selectors, and their colors are `--ts-*` custom properties, so `:root` restyles them.
- `<img src="assets/photo.png" alt="...">` embeds a project-relative PNG, JPEG, GIF, WebP, BMP, or SVG. Each image is limited to 8 MiB; bundles are limited to 16 MiB.
- `<video>`, `<audio>`, `<iframe>`, and `<canvas>` are not supported. Compilation fails with `Unsupported HTML element <video>` (and likewise for the others), on every renderer, because the HTML parser accepts only the documented subset. For video or audio playback, open the media in the system browser (`shell.openExternal`-style link) or bundle a player outside Rendrix; building media decoding into the native (GPUI) renderer is out of scope for 1.0.
- `data-shortcut="mod-s"` binds a button action to a key. `mod` means Ctrl on Windows/Linux and Cmd on macOS. `ctrl`, `alt`, `shift`, and `cmd` can also be written explicitly.
- Native menus use `<menu data-app-menu="File">` for an application menu and `<menu id="edit">` for a context menu. Put `<menuitem data-onclick="actionName">` entries and `<hr>` separators inside. Menu items may contain nested menu items for submenus. Add `data-shortcut="mod-s"` to an item to bind its shortcut. Attach `data-contextmenu="edit"` to an element to show that context menu on right-click. Context menus support disabled entries and static checkmarks with `checked`; checked state does not change automatically. Native application menus cannot currently disable or check items.
- `data-ondrop="actionName"` on a rendered container or button dispatches a `drop` event with `files`, an array of `{name, path}` objects for the files the user dropped. The host reports paths only and does not read file contents. File drops are native-renderer behavior; browser preview support may differ by platform.
- `data-confirm="deleteItem"` shows a native confirmation prompt, then runs the action only when confirmed.
- `data-clipboard-copy="stateKey"` copies a state value from a button. `data-clipboard-paste="stateKey"` reads clipboard text into that state value after a button click.
- `data-file-open="pathKey"` opens the native file chooser and stores the selected path. Add `data-file-read="textKey"` to read the chosen UTF-8 text file into another state key. `data-file-save="textKey"` opens a native save dialog and writes that state's text. File reads and writes are limited to 1 MiB and require a user-selected path.
- `data-fetch="https://api.example.com/path" data-fetch-into="result"` performs a user-initiated HTTPS GET and stores its text response. Add `data-fetch-method="POST"` to send a request; the body is the state key named by `data-fetch-body`. Strings are sent as text, and non-string values are serialized as JSON when `data-fetch-content-type="application/json"` is set. `data-fetch-status-into="status"` stores the numeric HTTP status, and `data-fetch-error-into="error"` stores an empty string on success or the error message on failure. The exact host must be listed in `permissions.network` in `rendrix.config.json`. Redirects are disabled, requests and bodies time out after 15 seconds, and response bodies are limited to 1 MiB.

Example network permission:

```json
{
  "permissions": {
    "network": ["api.example.com"]
  }
}
```

The permission schema also reserves explicit grants for the native bridge:

```json
{
  "permissions": {
    "filesystem": { "read": ["./data"], "write": ["./data"] },
    "processes": ["git"],
    "notifications": true,
    "ipc": true,
    "window": true
  }
}
```

These grants are serialized into the app bundle. Filesystem entries are directory roots relative to the app's folder (the executable's folder, or the `.rdx` file's folder for `--bundle` runs). Process entries are executable names. Notifications, IPC, and native window APIs are opt-in booleans. Existing picker-based file operations, window title-bar controls, and network permissions continue to use their current host paths.

`api.native` is the typed host API, and every call returns a promise. Host work runs on background threads, so a slow call does not freeze the window, and the action resumes when the result arrives. While an action waits, other actions and state writes queue behind it, so each one starts from committed state and rolls back cleanly.

- `filesystem` (`readText`, `writeText`, `readBytes`, `writeBytes`, `list`, `stat`, `mkdir`, `remove`, `rename`): paths are relative to a granted root. `..`, absolute paths and symlinks that lead outside the root are refused. Reads need a `read` root, and changes need a `write` root. Writable roots are created at startup. `''` or `.` names the root itself, for `list` and `stat` only. Each call moves at most 1 MiB. Pass `{ root, path }` to address a root by name: a `permissions.filesystem` entry, `userData`, or a file the user picked with `dialog.openFile` (read) or `dialog.saveFile` (write), which covers that one file. `rename` takes two locations in the same root.
- `process.spawn(command, args)`: runs only listed executables, without a shell, in the app's folder, with a 10 second limit and 1 MiB of output per stream. On Windows, `npm` resolves to `npm.cmd` through `PATHEXT`. Batch files take only the arguments Rust can quote safely.
- `window`: `setTitle`, `close`, `minimize`, `maximize` and `restore`, each needing `permissions.window`. The window changes on the UI thread.
- `notifications.show({ title, body })`: needs `permissions.notifications`. It shows a desktop toast, or a notification on macOS and Linux. On the removed WebView renderer the host showed an OS toast via notify-rust (WinRT on Windows, Notification Center on macOS, D-Bus on Linux); if the OS toast was unavailable, the call errored and `Notification.show()` in the Electron shim logged the failure, recorded a compatibility warning, and showed an in-page toast. That path is historical.
- `ipc`: needs `permissions.ipc`. `ipc.send(channel, payload)` reaches the app's own `ipc.on(channel, listener)` listeners and every connected peer. Peers are other local processes. The host listens on `127.0.0.1` on a free port and requires a random token. A child process started by `process.spawn` gets `RENDRIX_IPC_ADDR` and `RENDRIX_IPC_TOKEN`. The peer connects, sends `{"token":"<token>"}` as its first line, then sends and receives newline-delimited `{"channel":"name","payload":...}` JSON. Messages from peers reach the app's listeners, not other peers. Each backend load gets a new port, so reloading the app disconnects peers.
- `app.info()` returns the app's name, version, platform, folders (`home`, `userData`, `documents`, `downloads`, `temp`, `appFolder`) and the roots it may use. `app.quit()` ends the app.
- `clipboard.readText()` and `clipboard.writeText(text)` use the system clipboard (Windows only).
- `dialog.openFile({ multiple, filters })` returns `{ canceled, paths }`, `dialog.saveFile({ defaultPath, filters })` returns `{ canceled, path }`, and `dialog.message({ type, title, message, buttons })` returns `{ response }`. The dialogs run on a worker thread and use the native system dialogs (Windows only). Like the removed WebView renderer, they need no grant. Picked files are granted as described under `filesystem`.
- `shell.openExternal(url)` opens an `https` or `mailto` link in the default handler.
- `net.request({ url, method, headers, body })` returns `{ status, headers, body }`. It needs the URL's host listed in `permissions.network` in `rendrix.config.json` (exact host names only), redirects are not followed, and each body is limited to 1 MiB.
- `power.setBlockers(types)` keeps the system or display awake while the array of `prevent-app-suspension` or `prevent-display-sleep` is set (Windows only). Pass an empty array to clear it.
- `screen.displays()` returns each display's `x`, `y`, `width`, `height`, `scaleFactor` and `primary`. GPUI does not report a scale factor per display, so every display reports the app window's scale factor.

The native renderer has one window and no page. `window.create`, `window.load` and `ipc.invoke` from the backend are not available; the removed WebView renderer was the multi-window path, and multi-window is on the Rendrix roadmap (M3). Messages sent with `webContents.send` are dropped, and close listeners cannot cancel a close. Each of these records a compatibility warning instead of failing.

In the browser preview (`rendrix preview`), file and process calls run through the preview server with the same grants and path rules. The page's token and the `Host` header are checked, so other websites cannot make these calls. `window.setTitle` and `window.close` work in the page. `minimize`, `maximize` and `restore` need the native host. IPC works only within the page, with no peers. Batch files do not run in the preview, because Node cannot start them without a shell.

ARIA and role attributes are accepted as metadata, and `aria-label` is shown as a visible fallback for unlabeled buttons and checkboxes. GPUI 0.2.2 does not expose an accessibility tree API, so these attributes are not yet announced as native screen-reader semantics. The host keeps the parsed metadata in `native/src/accessibility.rs` (with a TODO module for the future GPUI mapping). Historical: the removed WebView renderer used the platform browser's accessibility behavior. Native screen-reader support is planned for the Rendrix engine via accesskit (Rendrix M3).

Tags must be properly nested and explicitly closed, except void tags such as `input` and `img`, and self-closed `<use .../>`. The document must have one root element.

## Window

`rendrix.config.json` accepts `"window": { "width": 900, "height": 600, "titlebar": "system", "backdrop": "opaque" }`. `titlebar` is `system` (the OS title bar) or `unified` (the app draws its own). `backdrop` is `opaque` or `blur`.

With `"titlebar": "unified"`, mark the bar's drag area with `data-window-drag`. That element must not contain buttons, inputs, or other interactive elements, because Windows treats its whole area as the title bar, and clicks inside it would never reach the control. Put the window buttons beside the drag area, with `data-window-control="close"`, `"minimize"`, or `"maximize"`. A unified bar without a drag area warns that the window cannot be moved. Window buttons under the system title bar warn that they have no effect.

`"backdrop": "blur"` asks the window for a blurred backdrop. Windows applies it only on some systems, and the root background must be translucent for it to show, for example `body { background: #f5f5f799; }`.

## Persistence and the development overlay

Set `"persist": true` in `rendrix.config.json` to keep backend state across app restarts. The host writes state when it changes, at most as JSON of 256 KiB, to the per-user configuration directory (`%APPDATA%\rendrix\<name>\state.json` on Windows, `~/Library/Application Support/rendrix/<name>/state.json` on macOS, and `$XDG_CONFIG_HOME/rendrix/<name>/state.json` or `~/.config/rendrix/<name>/state.json` on Linux). On startup, values the user changed are merged into the current defaults the same way a hot reload merges them; new state keys appear with their defaults, and keys the app no longer declares are dropped. Invalid or oversized files are ignored.

The development bundle includes compile diagnostics. Press `F12` or `Ctrl+Shift+I` in the native renderer to open the development panel. The **Problems** tab lists build warnings and any failed hot-reload error, while the last successful build stays runnable. The **Inspector** tab turns the pointer into a selector: click any element to see its tag, id, classes, attributes, computed style, and state styles. Clicking controls selects them instead of running them. `Escape` closes the popup, then the panel. `rendrix build` does not embed development diagnostics, but the panel and inspector remain available.

## CSS

Selectors: element, `.class`, `#id`, `*`, compounds such as `button.primary`, comma lists, descendant (`.card h1`) and child (`.card > p`) combinators. Specificity, `!important`, and source order are respected.

State pseudo-classes: `:hover`, `:active`, `:focus`, and `:disabled`, on the last compound of a selector (`button.nav:hover`). They can change only `background`, `color`, `border-color`, `border-width`, `border-style`, `box-shadow`, `opacity`, and `cursor`. They do not work inside `@media`.

CSS variables: declare `--name: value` (in `:root` or any rule) and use `var(--name)` or `var(--name, fallback)`. Variables resolve at compile time, and a later declaration of the same name wins. The component library's colors are `--ts-*` variables, so `:root { --ts-accent: #ff375f; }` restyles every library component.

Units: `px`, `rem`, and `em` (both 16px), `%` for sizes, insets, and padding and margin, and `vw`, `vh`, `vmin`, and `vmax` relative to the window. `auto` works for `width`, `height`, `margin`, `flex-basis`, and insets.

In the native renderer, declarations GPUI cannot render are skipped with a warning that names the file, line, and property, and the rest of the rule still applies. Set `"strictCss": true` in `rendrix.config.json` to turn those warnings into build errors. (Historical: the removed WebView renderer sent the stylesheet to the platform browser engine instead.)

| Property | Values |
|---|---|
| `display` | `flex`, `block`, `grid` |
| `flex-direction` | `row`, `column` |
| `flex`, `flex-grow`, `flex-shrink`, `flex-basis` | numbers, and lengths or `%` for the basis; `flex: 1`, `flex: none`, and `flex: auto` |
| `align-items` | `center`, `flex-start`, `flex-end`, `stretch` |
| `justify-content` | `center`, `flex-start`, `flex-end`, `space-between`, `space-around` |
| `gap`, `row-gap`, `column-gap`, `border-radius`, `font-size` | nonnegative lengths |
| `padding`, `padding-top`, `padding-right`, `padding-bottom`, `padding-left` | nonnegative lengths or `%`, one to four values |
| `margin`, `margin-top`, `margin-right`, `margin-bottom`, `margin-left` | lengths, `%`, or `auto`, one to four values, may be negative |
| `width`, `height`, `min-width`, `min-height`, `max-width`, `max-height` | lengths or `%`; `auto` and `none` where they apply |
| `position` | `relative`, `absolute` (`static` is treated as `relative`) |
| `z-index` | an integer or `auto`, used as paint order for absolutely positioned siblings; it does not create a stacking context |
| `top`, `right`, `bottom`, `left`, `inset` | lengths or `%` |
| `grid-template-columns`, `grid-template-rows` | up to 12 space-separated `fr`, `px`, or `auto` tracks |
| `overflow` | `visible`, `hidden`, `scroll`, `auto` |
| `opacity` | `0` through `1` |
| `background`, `background-color` | a color, or `linear-gradient(<deg>, <color>, <color>)` (exactly two stops; `to top/right/bottom/left` also work) |
| `color` | `#RGB`, `#RGBA`, `#RRGGBB`, `#RRGGBBAA`, `rgb()`, `rgba()`, `transparent`, `white`, `black` |
| `border`, `border-width`, `border-style`, `border-color` | solid borders with a width and a color; `none` removes the border. `dashed`, `dotted`, and the other styles are not supported |
| `border-top`, `border-right`, `border-bottom`, `border-left` and their `-width` and `-style` properties | solid per-edge borders. Per-edge colors are not supported because GPUI paints one border color per element |
| `box-shadow` | comma-separated `x y [blur] [spread] color` entries. `inset` is not supported |
| `text-align` | `left`, `center`, `right` (`start` and `end` map to left and right) |
| `line-height` | a length, or a unitless multiplier of `font-size` |
| `font-style` | `normal`, `italic` (`oblique` maps to italic) |
| `white-space` | `normal`, `nowrap` |
| `text-overflow` | `clip`, `ellipsis` (GPUI draws the ellipsis when the text is clipped) |
| `cursor` | `default`, `pointer`, `text`, `not-allowed`, `grab`, `move`, `crosshair` |
| `font-weight` | `normal`, `bold`, `400`, `500`, `600`, `700` |
| `font-family` | one family name (the first in a list; quotes are optional) |

Accepted with no effect: `box-sizing`, because boxes already size the way `border-box` does. Known but not supported yet (reported as warnings): `transition`, `animation`, `transform`, `filter`, `letter-spacing`, and others. GPUI 0.2.2 has no element transform, transition, or animation primitive; the native renderer cannot emulate them without a custom animation loop and a different drawing model. Motion is therefore not available in any in-tree engine: the removed WebView renderer was the only path that supported CSS motion. The Rendrix roadmap (M3) plans rAF-driven interpolation of opacity, color and position.

Custom properties with the same name in different places are global: the last declaration in source order wins.

Custom fonts are bundled with the app. Add entries to `fonts` in `rendrix.config.json`, for example `{"family":"Inter","src":"assets/Inter-Regular.ttf"}`, then use `font-family: Inter` in CSS. TTF and OTF files up to 8 MiB each are supported. The CSS family must match the configured family name.

`@media` supports `min-width`, `max-width`, `min-height`, and `max-height` in pixels. Rules are reevaluated on window resize. Grid templates with fixed, automatic, or unequal `fr` columns are laid out as flex rows with one cell per track, so both `240px 1fr` and `1fr 2fr` keep their requested proportions. Equal-weight `fr` templates use GPUI's grid. Explicit `grid-column` and `grid-row` placement is not supported.

## Browser preview

`rendrix preview --project PATH` serves the same bundle in a browser at `http://127.0.0.1:4173/` (`--port` and `--no-open` are optional). It rebuilds on save and reloads the page, and the app's state carries over, merged as the native host merges it. `rendrix preview --out FILE` writes one self-contained HTML file with no live reload.

The preview is a reference for the shared subset, not an exact match. Its layout follows the host's defaults (column boxes, border-box sizing, no button chrome), but text metrics, flex edge cases, and grid tracks differ. A `data-virtual` list builds every row, `data-window-*` attributes do nothing, file pickers use the browser's own dialogs, and `data-fetch` depends on the browser allowing the request. The backend runs in the page with the host's API and rollback rules, and with the browser's network and storage globals hidden. The page's content security policy permits script from the page itself, which the preview needs, and network requests only to the hosts in `permissions.network`.

## Backend TypeScript

The configured backend entry exports a default `{ state, actions }` object. It can import and re-export relative project-local `.ts` modules (including `index.ts`); extensionless imports and `.js` specifiers resolving to `.ts` are supported. These modules are bundled at build time and watched during development. Type-only imports may refer to packages. Runtime package imports, dynamic imports, and `require` are unsupported. Node/Bun APIs and browser globals are not available in native mode. Run `tsc --noEmit` separately for type checking; bundling transpiles TypeScript but does not type-check it.

Actions receive typed `get`, `set`, and `update` methods plus a read-only event. `AppDefinition<State, ActionNames>` checks state and optionally the required action names. Host operations return promises and can reject when a grant is missing or an operation fails; await them and catch errors in the action. See `examples/notes` for file reads, writes, listing, and error handling. Native host operations run asynchronously; arbitrary external JavaScript APIs are not provided. (Historical: WebView mode executed the backend in the browser page under that engine's security model.)

The native backend has no general `fetch` API. Network access is limited to the declarative `data-fetch` control with an exact host allowlist. File access is available through `api.native.filesystem` inside the granted roots, and through native picker events. (Historical: WebView pages used browser APIs and browser network rules.)
