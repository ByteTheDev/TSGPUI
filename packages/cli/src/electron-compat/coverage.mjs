// Coverage ledger for `require('electron')`. Every export in the Electron surface snapshot
// (surface/electron-33.json) must be classified here; the test suite enforces that, so a new
// Electron release cannot add APIs that TSGPUI silently ignores.
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
    nativeImage: 'PNG buffers/data URLs can be stored and inspected; JPEG/GIF/WebP dimensions, getAspectRatio, resize, crop, createFromBitmap and createFromPath (via the host filesystem) are tracked as metadata; real pixel resampling/encoding and multiple scale factors are unsupported',
    nativeTheme: 'themeSource and updated events use renderer media queries where available; OS theme state and events are unavailable in the main-process host',
    webContents: 'fromId and getAllWebContents return each window\'s send, on, once, removeListener and id; navigation and devtools are no-op stubs, page access is limited',
    safeStorage: 'Windows only, using DPAPI for the current account on this machine. isEncryptionAvailable, encryptString and decryptString return promises, and encryptString resolves to a Uint8Array instead of a Buffer; Electron returns these synchronously, so callers must await them. Elsewhere isEncryptionAvailable resolves to false and encryption rejects',
    MessageChannelMain:'in-process channels only: ports are structured-cloned within the same process; transferring ports and buffers is accepted, utility-process channels are not supported',
    screen: 'display bounds, work areas (the same as bounds) and scale factors come from the host and are cached; call screen.refresh() after display changes. The native renderer reports the app window\'s scale factor for every display, since GPUI has no per-display scale. getCursorScreenPoint returns cached coordinates after refresh, and display add/remove/metrics events are emitted',
    webFrame: 'main frame only: zoom uses CSS zoom on the document, and insertCSS and executeJavaScript act on the page; basic frame-tree properties are exposed',
    powerSaveBlocker: 'Windows only: keeps the system or display awake with SetThreadExecutionState; other platforms record an error and do nothing',
    Tray: 'accepted; in-memory shim only, with click/right-click/double-click events, no OS tray chrome',
    session: 'defaultSession and fromPartition cookies (get/set/remove, getCookies/setCookies/clearCookies) use host(\'session.cookies\') when the host provides it and otherwise an in-memory Map per partition; cache/storage controls, persistence flags and storagePath are exposed; path-based sessions are unsupported',
    protocol: 'handle and registerFileProtocol serve in-page schemes (app, file and custom allowlisted schemes) from packaged assets via webviewPages; binary and streaming response bodies are normalized; remote/http proxy protocols and privileged-scheme semantics other than recording are partial',
    globalShortcut: 'register/unregister are tracked in memory with callback dispatch; no OS-level global key delivery',
    autoUpdater: 'partial shim: checks report no update and emit update-not-available; no download/install',
    BaseWindow: 'partial shim: window-style helpers and bounds tracking; multi-view layout unsupported',
    BrowserView: 'partial shim: view bounds tracking and stub webContents; multi-view layout unsupported',
    View: 'partial shim: bounds tracking, visibility and events; view hierarchy unsupported',
    WebContentsView: 'partial shim: constructible view with stub webContents; view hierarchy unsupported',
    webFrameMain: 'partial shim: frame lookup and executeJavaScript; no frame internals',
    contentTracing: 'partial shim: no-op tracing methods; no trace capture',
    crashReporter: 'partial shim: local-only crash logging; no upload',
    desktopCapturer: 'partial shim: getSources returns an empty list',
    inAppPurchase: 'partial shim: purchaseProduct returns false; no store support',
    netLog: 'partial shim: start/stop logging are no-ops',
    powerMonitor: 'partial shim: does not emit power events',
    pushNotifications: 'partial shim: register returns false',
    ShareMenu: 'partial shim: popup/closePopup are no-ops',
    systemPreferences: 'partial shim: isDarkMode returns false and accent color defaults to empty',
    TouchBar: 'partial shim: constructor returns an object; no native Touch Bar',
    utilityProcess: 'partial shim: fork returns a child-process-like stub',
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
