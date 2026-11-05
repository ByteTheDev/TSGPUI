// Coverage ledger for `require('electron')`. Every export in the Electron surface snapshot
// (surface/electron-33.json) must be classified here; the test suite enforces that, so a new
// Electron release cannot add APIs that TSGPUI silently ignores.
//
// Status meanings:
//   implemented  the shim behaves like Electron for normal use.
//   partial      the shim runs, but a documented part of the behaviour is missing. The reason says what.
//   unsupported  the shim exists but throws a clear error when used. The reason says why.
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
    BrowserWindow: 'WebView host can create additional windows, route local window commands and relay IPC through the primary window. Focus, blur, move, resize, minimize, maximize and restore events are reported; a close listener can prevent a user close. parent and modal windows work on Windows only. Statically associated windows and local pages can have separate preloads; dynamic associations, conflicting preloads on the same page, Electron context isolation, remote URLs, show/hide/ready-to-show/fullscreen events and will-move/will-resize are unsupported. IPC depends on the primary window remaining open.',
    Menu: 'menu objects and templates can be inspected and modified, but native menus are not shown and popup is unsupported',
    MenuItem: 'menu item properties and submenus are supported as data; menu item roles and native activation are not implemented',
    net: 'request uses the host permission-checked HTTPS text transport; binary streaming, sessions, redirects, cancellation, net.fetch and isOnline are unsupported',
    nativeImage: 'PNG buffers/data URLs can be stored and inspected; image decoding, transformations, JPEG, path and bitmap loading, and multiple scale factors are unsupported',
    nativeTheme: 'themeSource and updated events use renderer media queries where available; OS theme state and events are unavailable in the main-process host',
    webContents: 'fromId and getAllWebContents return each window\'s send, on, once, removeListener and id; no direct page access, navigation or devtools',
    MessageChannelMain: 'in-process channels only: ports are structured-cloned within the same process; transferring ports or buffers and utility-process channels are not supported',
    screen: 'display bounds, work areas (the same as bounds) and scale factors come from the host and are cached; call screen.refresh() after display changes. The native renderer reports the app window\'s scale factor for every display, since GPUI has no per-display scale. getCursorScreenPoint is unavailable and display events are not emitted',
    webFrame: 'main frame only: zoom uses CSS zoom on the document, and insertCSS and executeJavaScript act on the page; no frame tree',
    powerSaveBlocker: 'Windows only: keeps the system or display awake with SetThreadExecutionState; other platforms record an error and do nothing',
    Tray: 'accepted but no tray icon is shown',
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
    protocol: 'custom protocols are not registered; assets load from the package',
    pushNotifications: 'push notifications are not available',
    safeStorage: 'OS credential encryption is not available',
    session: 'session and cookie control is not available',
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
