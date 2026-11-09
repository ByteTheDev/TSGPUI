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
  },
  partial: {
    app: 'lifecycle (whenReady, quit, window-all-closed), getPath for provided paths, getName/getVersion. No dock, badge, login-item, or single-instance lock',
    BrowserWindow: 'WebView host can create additional windows, route local window commands and relay IPC through the primary window. Focus, blur, move, resize, minimize, maximize and restore events are reported; a close listener can prevent a user close. ready-to-show is emitted after creation and after each loadFile/loadURL; show/hide emit show/hide events and setFullScreen emits enter-full-screen/leave-full-screen on additional windows (isVisible/isFullScreen track the last requested state, setFullScreenable records the flag locally, and the primary window show()/hide() resolves without a host call while setFullScreen() throws). will-move/will-resize are emitted as stubs before move/resize without geometry, and native move/resize carry no bounds, so getBounds() returns the last setBounds value. setBounds/setPosition/setSize/getBounds/getSize/getPosition track bounds locally, emit the will-* and move/resize events, and route via window.setBounds/window.setPosition/window.setSize on additional windows (primary tracked locally only); focus()/blur() update local focus and route via window.focus/window.blur on additional windows, isMaximized()/isMinimized() report local state updated by minimize/maximize/restore calls and host focus/move events, and getFocusedWindow() returns the locally focused window. parent and modal windows work on Windows only. Statically associated windows and local pages can have separate preloads; dynamic associations, conflicting preloads on the same page, Electron context isolation and remote URLs are unsupported. IPC depends on the primary window remaining open.',
    Menu: 'menu objects and templates can be inspected and modified, but native menus are not shown and popup is unsupported',
    MenuItem: 'menu item properties and submenus are supported as data; menu item roles and native activation are not implemented',
    net: 'request uses the host permission-checked HTTPS text transport; binary streaming, sessions, redirects, cancellation, net.fetch and isOnline are unsupported',
    nativeImage: 'PNG buffers/data URLs can be stored and inspected; image decoding, transformations, JPEG, path and bitmap loading, and multiple scale factors are unsupported',
    nativeTheme: 'themeSource and updated events use renderer media queries where available; OS theme state and events are unavailable in the main-process host',
    webContents: 'fromId and getAllWebContents return each window\'s send, on, once, removeListener and id; no direct page access, navigation or devtools',
    safeStorage: 'Windows only, using DPAPI for the current account on this machine. isEncryptionAvailable, encryptString and decryptString return promises, and encryptString resolves to a Uint8Array instead of a Buffer; Electron returns these synchronously, so callers must await them. Elsewhere isEncryptionAvailable resolves to false and encryption rejects',
    MessageChannelMain:'in-process channels only: ports are structured-cloned within the same process; transferring ports or buffers and utility-process channels are not supported',
    screen: 'display bounds, work areas (the same as bounds) and scale factors come from the host and are cached; call screen.refresh() after display changes. The native renderer reports the app window\'s scale factor for every display, since GPUI has no per-display scale. getCursorScreenPoint is unavailable and display events are not emitted',
    webFrame: 'main frame only: zoom uses CSS zoom on the document, and insertCSS and executeJavaScript act on the page; no frame tree',
    powerSaveBlocker: 'Windows only: keeps the system or display awake with SetThreadExecutionState; other platforms record an error and do nothing',
    Tray: 'accepted but no tray icon is shown',
    session: 'defaultSession and fromPartition cookies (get/set/remove, getCookies/setCookies/clearCookies) use host(\'session.cookies\') when the host provides it and otherwise an in-memory Map per partition; path-based and cache/data controls are stubs, persistence across restarts requires "persist": true',
    protocol: 'handle and registerFileProtocol serve in-page schemes (app, file and custom allowlisted schemes) from packaged assets via webviewPages; remote/http proxy protocols, buffer/stream responses and privileged-scheme semantics other than recording are partial',
    globalShortcut: 'register returns false, so shortcuts never fire',
  },
  unsupported: {
    autoUpdater: 'TSGPUI updates are shipped by its release process, not by the app',
    BaseWindow: 'the separate Electron BaseWindow API is not implemented',
    BrowserView: 'multiple views per window are not supported',
    View: 'view layout is not supported',
    WebContentsView: 'multiple views per window are not supported',
    webFrameMain: 'no access to frame internals',
    contentTracing: 'tracing is not available in the embedded engines',
    crashReporter: 'no crash upload; start a crash reporter in your own code',
    desktopCapturer: 'screen and window capture is not available',
    inAppPurchase: 'store purchases are not available',
    netLog: 'network logging is not available',
    powerMonitor: 'power events are not available',
    pushNotifications: 'push notifications are not available',
    ShareMenu: 'the system share menu is not available',
    systemPreferences: 'system preference queries are not available',
    TouchBar: 'the Touch Bar is not available',
    utilityProcess: 'utility processes are not supported; run work in the backend',
    webUtils: 'file path lookup for dropped files is not available; use data-ondrop',
  },
};

// Names that appear in `COVERAGE` and the surface, reconciled by the test.
export function statusOf(name) {
  for (const status of ['implemented', 'partial', 'unsupported']) {
    if (Object.prototype.hasOwnProperty.call(COVERAGE[status], name)) return {status, reason: COVERAGE[status][name]};
  }
  return null;
}
