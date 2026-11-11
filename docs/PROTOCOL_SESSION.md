# Protocol, session, and webContents shims

This document describes the deepened Electron compatibility shims in
[`packages/cli/src/electron-compat/runtime`](../packages/cli/src/electron-compat/runtime):

- [`protocol.ts`](../packages/cli/src/electron-compat/runtime/protocol.ts)
- [`electron-session.ts`](../packages/cli/src/electron-compat/runtime/electron-session.ts)
- the `webContents` object in [`electron.ts`](../packages/cli/src/electron-compat/runtime/electron.ts)

They are a compatibility layer for apps converted with `tsgpui import`, not a
Chromium replacement. The WebView host still owns navigation, cookies, network
access and rendering; the shims keep the Electron-shaped state and route what the
host supports.

## protocol

### Registry

| API | Behaviour |
| --- | --- |
| `handle(scheme, handler)` | Registers a handler for an in-page scheme. |
| `unhandle(scheme)` | Removes a handler. |
| `registerFileProtocol(scheme, handler?)` | With no handler, rewrites the request URL onto the packaged `tsgpui://localhost/` space. |
| `registerBufferProtocol`, `registerStringProtocol`, `registerStreamProtocol`, `registerHttpProtocol` | Register a handler with the same dispatch path. |
| `interceptFileProtocol`, `interceptBufferProtocol`, `interceptStringProtocol`, `interceptStreamProtocol`, `interceptHttpProtocol` | Register a handler and mark the scheme intercepted. |
| `isProtocolHandled`, `isProtocolRegistered`, `isProtocolIntercepted` | Query the corresponding registry state. |
| `unregisterProtocol`, `uninterceptProtocol` | Remove the handler and intercepted flag. Privileged-scheme records stay. |
| `registerSchemesAsPrivileged(entries)` | Validates and records per-scheme privileges. |
| `dispatchRequest(url)` | Internal entry point: runs the handler and normalizes the response. |
| `rewriteCustomUrl(url)` | Maps an `app:`/`file:`/custom-scheme URL onto the packaged asset space. |

Privileged-scheme records are separate from handlers:
`isProtocolRegistered` reports handlers/intercepts, while
`getSchemePrivileges(scheme)` and `getPrivilegedSchemes()` report privilege
records.

Scheme names are lowercased and validated. `tsgpui` is host-owned. Registering
`http`, `https`, `ws` or `wss` with `handle` or `register*Protocol` throws:
remote traffic must use the permission-checked HTTPS transport
(`permissions.network`). `registerHttpProtocol` is therefore for a **custom**
scheme whose handler returns a remote target, not for registering `https` itself.

The `intercept*Protocol` methods are the exception: Electron uses them to wrap
existing schemes, including `http`/`https`. The shim stores those remote handlers
and `dispatchRequest` can invoke them, but the system WebView still performs
page-network requests itself, so a remote interception handler affects explicit
`dispatchRequest` calls only. Treat this as registry support, not transparent
page-network interception.

`BrowserWindow.loadURL('app://...')` and `loadURL('file://...')` are rewritten to
`tsgpui://localhost/...`, so packaged pages listed in `html` / `webviewPages`
load. The importer allowlists literal schemes in `protocolSchemes`; dynamic
scheme names need manual review.

### Handler styles and responses

A handler can use either Electron form:

```js
protocol.handle('app', (request) => ({ data: 'hello', mimeType: 'text/plain' }));

protocol.handle('app', (request, callback) => {
  callback({ data: 'hello', mimeType: 'text/plain' });
});
```

Callback handlers are asynchronous: the shim waits for `callback(...)`. An async
handler that returns a value also works.

The `request` object contains `url`, `scheme`, `host`, `path`, `method`,
`referrer` and `isSecure`, plus the recorded `privileges` object.

A response can be:

- a string or path (`app://localhost/index.html`), rewritten onto packaged assets;
- `{data, mimeType}` for buffer protocols;
- `{status, headers, body}`, where `body` is a string, `Uint8Array`,
  `ArrayBuffer`, typed-array view, Blob-like object with `arrayBuffer()`, a
  `ReadableStream`, or an async iterable/generator;
- `{url, method, headers, uploadData}` (or `body`/`data`) for a remote proxy.

Normalized responses expose `statusCode`, `status`, `headers`, `mimeType`,
`body`, and the helpers `text()`, `arrayBuffer()` and `stream()`. Binary bodies
are kept as bytes; streams are collected so the result can be read repeatedly.
String chunks from generators/streams are UTF-8 encoded.

### Remote proxy protocols

Returning a remote target proxies the request through the host's
permission-checked transport:

```js
protocol.registerHttpProtocol('api', (request) => ({
  url: 'https://api.example.com' + request.path,
  method: 'GET',
  headers: { accept: 'application/json' },
  uploadData: [{ bytes: new Uint8Array([1, 2, 3]) }],
}));
```

The target must be `http:` or `https:`. The host only permits granted HTTPS
hosts and caps request/response bodies at 1 MiB. `uploadData` entries can be
strings or byte arrays; entries that name files or blob UUIDs throw instead of
silently reading local files. The proxy returns the host response as a
normalized response with binary-safe body helpers.

### Privileged schemes

`registerSchemesAsPrivileged` accepts entries such as:

```js
protocol.registerSchemesAsPrivileged([{
  scheme: 'app',
  privileges: {
    standard: true,
    secure: true,
    supportFetchAPI: true,
    corsEnabled: true,
    allowServiceWorkers: false,
    bypassCSP: false,
    stream: true,
    codeCache: true,
  },
}]);
```

Known flags are validated and recorded per scheme. Repeating a scheme merges the
flags. Unknown flags produce a compatibility warning and are ignored. The
`getSchemePrivileges(scheme)` and `getPrivilegedSchemes()` helpers expose the
registry for diagnostics and tests.

Only the flags the shim can apply are applied:

- `secure` sets `request.isSecure` for dispatch;
- `corsEnabled` adds `Access-Control-Allow-Origin: *` to dispatched responses
  when the handler did not set one;
- `standard`, `supportFetchAPI`, `bypassCSP`, `allowServiceWorkers`, `stream` and
  `codeCache` are recorded and reported, but browser-level CORS, fetch, CSP,
  service-worker and code-cache behaviour is controlled by the system WebView,
  not by this shim.

Call `registerSchemesAsPrivileged` during startup, before windows load pages.

## session

### Partitions

`session.defaultSession` is a persistent session (`persist:tsgpui-default`).
`session.fromPartition(partition)` caches one `Session` per partition; a
`persist:` prefix makes it persistent. `session.fromPath(path)` creates a
persistent session keyed by the path. `isPersistent()`, `storagePath` and
`getStoragePath()` are exposed.

### Cookies

`session.cookies` provides:

- `get(details)` / `getCookies(details)`
- `set(cookie)` / `setCookies(cookie)`
- `remove(url, name)` or `remove(details)`
- `clear()` / `clearCookies()`
- `flushStore()`

The shim first calls `host('session.cookies', {action, ...})`. A host that
implements the operation is authoritative. When the host rejects or is missing,
cookies live in a per-partition in-memory `Map` with URL/domain/path/name
matching.

For persistent sessions, the in-memory store is also mirrored to disk:

- one file per partition under the host-provided `userData` root, named
  `tsgpui-session-cookies-<sanitized-partition>-<hash>.json`;
- the file is loaded on the first cookie operation for that session;
- only non-session cookies (those with an `expirationDate` and without
  `session: true`) are written;
- expired cookies are purged on load and read;
- `set`, `remove` and `clear` write after mutating the store, and `flushStore`
  awaits the pending write;
- a host without filesystem access falls back to memory with one compatibility
  warning.

The shim stores the cookie registry; it does not inject `Cookie`/`Set-Cookie`
headers into page requests. That is the WebView host's job.

### Storage, cache and data clearing

`clearStorageData(options)` and `clearData(options)` validate `origin` and
`storages`. Known storage names are `appcache`, `cookies`, `filesystem`,
`indexdb`, `localstorage`, `shadercache`, `websql`, `serviceworkers` and
`cachestorage`.

When the host does not implement the operation:

- `cookies` clears the shim cookie store (and rewrites the persistent file);
- `localstorage` clears the current page's `localStorage` and `sessionStorage`;
- other storage types are reported as needing host support;
- an `origin` that does not match the current page origin skips page storage.

`clearCache`, `clearAuthCache`, `clearCodeCaches`, `clearHostResolverCache` and
the shared-dictionary methods remain host-dependent stubs.

### Permission handlers

`setPermissionRequestHandler(handler)` and `setPermissionCheckHandler(handler)`
validate a function or `null` and store it. The WebView host does not emit
permission prompts, so the shim exposes two extension methods that invoke the
stored callbacks:

```js
session.setPermissionRequestHandler((webContents, permission, callback, details) => {
  callback(permission === 'notifications');
});

const granted = await session.requestPermission({ permission: 'notifications' });

session.setPermissionCheckHandler((webContents, permission, origin, details) => permission === 'clipboard-read');
const allowed = await session.checkPermission('clipboard-read', 'https://example.com');
```

`requestPermission` accepts a callback-style handler, a promise-returning
handler, or a synchronous return value. `getPermissionRequestHandler()` and
`getPermissionCheckHandler()` expose the stored callback for diagnostics.

### User agent

`setUserAgent(userAgent, acceptLanguages?)` validates the strings and stores
them. It calls the host; if the host cannot apply the value, it patches the
current page's `navigator.userAgent` when possible and records that the change is
page-local. `getUserAgent()` returns the configured value (or the current page's
navigator value when none is set).

### webRequest

All Electron webRequest events are present:

`onBeforeRequest`, `onBeforeSendHeaders`, `onSendHeaders`, `onHeadersReceived`,
`onResponseStarted`, `onBeforeRedirect`, `onCompleted`, `onErrorOccurred`.

Listeners and filters are validated and stored. `filter.urls` accepts wildcards
(`*`, `<all_urls>`). The shim records the listeners but the WebView host does not
intercept requests, so listeners do not cancel, redirect or rewrite headers. A
single compatibility warning is emitted per event type when first used. The
non-enumerable `webRequest._emit(event, details)` helper invokes matching
listeners for tests and diagnostics.

### Downloads

`downloadURL(url, options?)` creates a `DownloadItem` and emits
`will-download` with `(event, item, webContents)`:

```js
session.on('will-download', (event, item) => {
  if (item.getFilename() === 'blocked.zip') {
    event.preventDefault();
    return;
  }
  item.setSavePath('/tmp/file.zip');
});

session.downloadURL('https://example.com/file.zip');
```

`DownloadItem` exposes `getURL`, `getFilename`, `getSavePath`, `setSavePath`,
`getState`, `getReceivedBytes`, `getTotalBytes`, `getMimeType`, `isPaused`,
`canResume`, `isDone`, `pause`, `resume` and `cancel`, and emits `updated` and
`done`. States are `progressing`, `completed`, `cancelled` and `interrupted`.

`setDownloadPath(path)` supplies the default save directory. If the host has no
`session.downloadURL` operation, the item is marked `interrupted` and a warning
is recorded; the shim never invents downloaded bytes. If the host reports
`savePath`, `receivedBytes` or `totalBytes`, the item is updated. Cancelling via
`event.preventDefault()` or `item.cancel()` prevents the host call.

`createInterruptedDownload(options)` records an interrupted item and
`getDownloadItems()` returns the session's items (a TSGPUI extension).
Actual byte transfer, resumption and OS download UI remain host work.

### Remaining session stubs

Proxy settings, extension loading, network emulation, preconnects, host
resolution, `session.fetch`, `getBlobData`, `setSSLConfig`, certificate
verification, display-media/device/Bluetooth permission handlers and the spell
checker are tracked or host-dependent stubs. They validate obvious input and
warn when they cannot take effect.

## webContents

Each `BrowserWindow` exposes a `webContents` object. `webContents.fromId`,
`getAllWebContents`, `getFocusedWebContents` and `fromWebContents` work over the
window registry.

### Loading and events

Primary-window `BrowserWindow.loadFile` and additional-window `loadFile` /
`loadURL` emit:

- `did-start-loading` before the host load begins;
- `did-finish-load` after success;
- `did-fail-load` with `(event, errorCode, errorDescription, validatedURL, isMainFrame)` on failure or an unsupported primary navigation;
- `did-stop-loading` after success, failure, or `stop()`.

`loadURL` on the primary window remains unsupported and rejects with
`webContents.loadURL is not supported for the first window`.

`stop()` clears the local loading state, emits `did-stop-loading`, and invalidates
the pending load so a late host success cannot emit `did-finish-load`.

`reload()` reloads the primary page via `location.reload()` when a page is
available, or re-issues the last `loadFile`/`loadURL` for additional windows.
`reloadIgnoringCache()` follows the same path. With no known target they record a
warning and do nothing.

### Page state

- `getTitle()` returns the primary page's `document.title` when available,
  otherwise the tracked window/tab title. `BrowserWindow.setTitle` updates the
  tracked title.
- `getURL()` returns the last requested/loaded target, falling back to the
  current document location.
- `isLoading()` / `isLoadingMainFrame()` report the local load state.
- `openDevTools`, `closeDevTools` and `toggleDevTools` are silent no-ops;
  `isDevToolsOpened()` and `isDevToolsFocused()` return `false`.
- `setZoomFactor(factor)` requires a positive finite number, stores the factor
  and applies CSS `zoom` to the page's document element. `setZoomLevel(level)`
  converts through `1.2 ** level`. The corresponding getters return the stored
  values.
- `findInPage(text, options?)` returns an incrementing request id and emits
  `found-in-page` with `{requestId, activeMatchOrdinal: 0, matches: 0,
  selectionArea: {}, finalUpdate: true}`. It is a stub: there is no text search
  implementation. `stopFindInPage(action?)` validates the action
  (`clearSelection`, `keepSelection`, `activateSelection`).
- `executeJavaScript`, `insertCSS`, `print`, `printToPDF`, `savePage`,
  `capturePage`, `setWindowOpenHandler` and audio/frame-rate controls keep their
  existing behaviour.

Navigation history (`goBack`, `goForward`, `canGoToOffset`, `goToIndex`,
`goToOffset`, `clearHistory`) is still stubbed, and real DevTools, paint events
and media state are unavailable. In the native GPUI renderer there is no page, so
title/URL/JavaScript operations are empty or local-only.

## Tests and coverage

The focused coverage lives in
[`packages/cli/test/electron-runtime.test.mjs`](../packages/cli/test/electron-runtime.test.mjs)
and the ledger in
[`packages/cli/src/electron-compat/coverage.mjs`](../packages/cli/src/electron-compat/coverage.mjs).

```sh
bun test
# or
node --test packages/cli/test/*.test.mjs
```

The ledger notes for `protocol`, `session` and `webContents` list the supported
behaviour and the remaining host limits. Regenerate
`runtime/electron-coverage.ts` if an unsupported API entry changes.
