// Coverage ledger for `require('electron')`. Every export in the Electron surface snapshot
// (surface/electron-33.json) must be classified here; the test suite enforces that, so a new
// Electron release cannot add APIs that TS-GPU silently ignores.
//
// Status meanings:
//   implemented  the shim behaves like Electron for normal use.
//   partial      the shim runs, but a documented part of the behaviour is missing. The reason says what.
//   unsupported  the shim exists but throws a clear error when used. The reason says why.
//
// Lazy shards: electron.ts eagerly provides window show/hide/fullscreen, notifications and IPC
// (plus app/dialog/shell/clipboard/protocol/coverage). Menu/MenuItem, net, nativeImage,
// nativeTheme, screen/MessageChannelMain/MessagePortMain/webFrame/powerSaveBlocker and
// session/Session are loaded on first access from their electron-*.ts shards, so apps that
// never use them do not ship them (the sample uses none of menu/image/theme/geolocation).
export const SURFACE_FILE = 'surface/electron-33.json';

export const COVERAGE = {
  implemented: {
ipcMain: 'handle/on/once/removeHandler route through the host bridge',
    ipcRenderer: 'invoke/send/on/once/removeListener route through window.tsgpui',
    contextBridge: 'exposeInMainWorld sets a global on the page',
    dialog: 'open, save and message dialogs use the native host',
    shell: 'openExternal opens the system browser',
    clipboard: 'readText and writeText use the host clipboard',
    Notification: 'show() posts a host notification (needs the notifications grant)',
app: 'lifecycle (whenReady, quit, window-all-closed), getPath for provided paths, getName/getVersion. No dock, badge, login-item, or single-instance lock',
    BrowserWindow: 'WebView host can create additional windows, route local window commands and relay IPC through the primary window. Focus, blur, move, resize, minimize, maximize and restore events are reported; a close listener can prevent a user close. ready-to-show is emitted after creation and after each loadFile/loadURL; show/hide emit show/hide events and setFullScreen emits enter-full-screen/leave-full-screen on additional windows (isVisible/isFullScreen track the last requested state, setFullScreenable records the flag locally, and the primary window show()/hide() resolves without a host call while setFullScreen() throws). will-move/will-resize are emitted as stubs before move/resize without geometry, and native move/resize carry no bounds, so getBounds() returns the last setBounds value. setBounds/setPosition/setSize/getBounds/getSize/getPosition track bounds locally, emit the will-* and move/resize events, and route via window.setBounds/window.setPosition/window.setSize on additional windows (primary tracked locally only); focus()/blur() update local focus and route via window.focus/window.blur on additional windows, isMaximized()/isMinimized() report local state updated by minimize/maximize/restore calls and host focus/move events, and getFocusedWindow() returns the locally focused window. parent and modal windows work on Windows only. Statically associated windows and local pages can have separate preloads; dynamic associations, conflicting preloads on the same page, Electron context isolation and remote URLs are unsupported. IPC depends on the primary window remaining open.',
    Menu: 'menu objects and templates can be inspected and modified; disabled and hidden aliases are honored; native menus are shown on the native renderer, popup is unsupported',
    MenuItem: 'menu item properties and submenus are supported as data; menu item roles and native activation are not implemented',
    net: 'request uses the host permission-checked HTTPS transport; supports binary bodies, response streaming, redirect handling and cancellation; net.fetch is unsupported, isOnline reports navigator.onLine',
    nativeImage: 'PNG/JPEG/GIF/WebP images are decoded by the native host; resize, crop, toPNG, toJPEG and toDataURL work on real pixels, and getSize/getAspectRatio report decoded dimensions; createFromPath reads through filesystem grants, createFromBuffer accepts encoded bytes, and createFromBitmap accepts BGRA bitmaps. Host decoding is asynchronous: createFromPath waits for it and the other creators expose whenReady(). Multiple scale factors are unsupported (the creation scale factor is reported), and toBitmap returns raw pixels only for createFromBitmap images',
    nativeTheme: 'themeSource and updated events use renderer media queries where available; OS theme state and events are unavailable in the main-process host',
    webContents: 'fromId and getAllWebContents return each window\'s webContents. loadURL/loadFile emit did-start-loading, did-finish-load, did-fail-load and did-stop-loading; getTitle/getURL/isLoading track the last navigation, stop() cancels local loading state, reload()/reloadIgnoringCache() reload the page or last target, setZoomFactor/setZoomLevel apply CSS zoom to the page, findInPage returns a request id and emits found-in-page with zero matches, and openDevTools/closeDevTools/toggleDevTools are silent no-ops. Navigation history, devtools and real page access remain limited',
    safeStorage: 'Windows only, using DPAPI for the current account on this machine. isEncryptionAvailable, encryptString and decryptString return promises, and encryptString resolves to a Uint8Array instead of a Buffer; Electron returns these synchronously, so callers must await them. Elsewhere isEncryptionAvailable resolves to false and encryption rejects',
    MessageChannelMain:'in-process channels only: ports are structured-cloned within the same process; transferring ports and buffers is accepted, utility-process channels are not supported',
    screen: 'display bounds, work areas (the same as bounds) and scale factors come from the host and are cached; call screen.refresh() after display changes. The host polls monitor changes and emits display-added, display-removed and display-metrics-changed, which the shim re-emits with normalized Display objects. The native renderer reports the app window\'s scale factor for every display, since GPUI has no per-display scale. getCursorScreenPoint returns cached coordinates after refresh',
    webFrame: 'main frame only: zoom uses CSS zoom on the document, and insertCSS and executeJavaScript act on the page; basic frame-tree properties are exposed',
    powerSaveBlocker: 'Windows only: keeps the system or display awake with SetThreadExecutionState; other platforms record an error and do nothing',
    Tray: 'OS tray icons: Windows uses Shell_NotifyIcon on a message-only window (click, right-click, double-click, tooltip, HMENU context menus and bounds), macOS uses NSStatusItem/NSMenu on the main thread, and Linux uses a StatusNotifierItem with a dbusmenu through ksni. Menu item clicks dispatch the MenuItem click handler, checkbox and radio state toggles before the handler runs, and destroy removes the OS object. setPressedImage is honored on macOS, displayBalloon/focus are accepted without native effects, and getBounds is reported where the OS exposes it',
    session: 'defaultSession, fromPartition and fromPath expose per-partition state. Cookies use host(\'session.cookies\') when available and otherwise an in-memory Map; persist: partitions also mirror non-session cookies to a JSON file under userData and reload them on first use. clearStorageData/clearData filter storages and fall back to cookies plus the current page localStorage; setPermissionRequestHandler/setPermissionCheckHandler store callbacks reachable through requestPermission/checkPermission (the host emits no prompts); setUserAgent applies to the host or current page navigator. webRequest records listeners without intercepting requests; downloadURL/will-download and createInterruptedDownload return DownloadItem lifecycle stubs with cancel/pause/resume. Proxy, cache, extension and network emulation controls remain stubs, and path-based sessions share the default in-memory namespace',
    protocol: 'handle, register* and intercept* manage in-page schemes (app, file and custom allowlisted schemes) that BrowserWindow.loadURL rewrites onto packaged app assets. Handlers may return paths, {data,mimeType} buffers, Response-like objects or {url,method,headers,uploadData} remote proxy responses; proxy requests use the permission-checked net transport. Binary bodies, ReadableStream/async-iterable bodies and callback-style handlers are normalized, and responses expose text/arrayBuffer/stream helpers. registerSchemesAsPrivileged validates and stores standard/secure/bypassCSP/allowServiceWorkers/supportFetchAPI/corsEnabled/stream/codeCache per scheme; secure and corsEnabled affect dispatchRequest, while browser-level CORS, service-worker, CSP and code-cache behaviour is not applied. Remote http/https/ws/wss schemes cannot be used with handle/register*Protocol; registerHttpProtocol only proxies a custom scheme to an http(s) URL, while intercept*Protocol records a remote handler that dispatchRequest can call but the page network stack is not intercepted',
    globalShortcut: 'register/unregister/unregisterAll forward to the host and keep an in-memory registry for isRegistered. Windows uses RegisterHotKey, macOS a listen-only CGEventTap (the app needs Accessibility permission), and Linux XGrabKey on the X11 root window. Registration is asynchronous: a valid accelerator returns true optimistically, and a binding the OS refuses is removed from the registry with a compat warning. Wayland sessions cannot register global hotkeys',
    autoUpdater: 'checks a configured update feed, emits update-available or update-not-available, has the host download and SHA-256-verify the artifact into userData/updates, and swaps it over the running executable on quitAndInstall (relaunch on request)',
    BaseWindow: 'partial shim: window-style helpers and bounds tracking; multi-view layout unsupported',
    BrowserView: 'partial shim: view bounds tracking and stub webContents; multi-view layout unsupported',
    View: 'partial shim: bounds tracking, visibility and events; view hierarchy unsupported',
    WebContentsView: 'partial shim: constructible view with stub webContents; view hierarchy unsupported',
    webFrameMain: 'partial shim: frame lookup and executeJavaScript; the main frame can send and postMessage to the page, but there is no frame tree, isolated world or transferable support',
    contentTracing: 'partial shim: startRecording/stopRecording write a Chrome tracing-event JSON timeline with TS-GPU markers; Chromium renderer, GPU and thread internals are not captured',
    crashReporter: 'partial shim: records crash parameters and writes crash metadata and captureCrash dumps under userData/crashes; no native minidumps or upload',
    desktopCapturer: 'partial shim: getSources returns an empty list',
    inAppPurchase: 'partial shim: purchaseProduct returns false; no store support',
    netLog: 'partial shim: startLogging/stopLogging write a JSON net log and record net.request traffic; Chromium socket, DNS and QUIC internals are not captured',
    powerMonitor: 'reads the host power state (AC/battery, idle time, thermal, lock) and emits suspend, resume, on-ac, on-battery, shutdown, lock-screen and unlock-screen from native power notifications and host state transitions; idle and thermal values are best-effort',
    pushNotifications: 'partial shim: register returns false',
    ShareMenu: 'partial shim: popup/closePopup are no-ops',
    systemPreferences: 'reads dark mode, accent color, system colors and reduced-motion/inverted/contrast/transparency flags from the host (Windows registry and SystemParametersInfo, macOS defaults, Linux gsettings); renderer media queries and a built-in palette are the fallback; TouchID and media access remain partial',
    TouchBar: 'partial shim: constructor returns an object; no native Touch Bar',
    utilityProcess: 'partial shim: fork starts a child process through the host (defaults to node; grant it in permissions.processes), relays stdout/stderr/exit and exposes a UtilityProcess; no Node module runtime, message ports or transferables',
    webUtils: 'partial shim: getPathForFile returns a path or empty string',
  },
  partial: {},
  unsupported: {},
};

// Names that appear in `COVERAGE` and the surface, reconciled by the test.
export function statusOf(name) {
  for (const status of ['implemented', 'partial', 'unsupported']) {
    if (Object.prototype.hasOwnProperty.call(COVERAGE[status], name)) return {status, reason: COVERAGE[status][name]};
  }
  return null;
}
