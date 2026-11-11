import { host, compatWarnings, handle, removeHandler, listen, unlisten, toBase64, fromBase64 } from './host';
import { info, infoReady } from './os';
import { EventEmitter } from './events';
import { protocol, rewriteCustomUrl } from './protocol';
export { protocol } from './protocol';
// Eager surface: window show/hide/fullscreen, notifications and IPC (plus app/dialog/shell/clipboard).
// Menu, image, theme, net, screen/system and session are opt-in shards loaded on first access
// via the synchronous lazy loader below, so apps that never use them do not ship them.
// The loader builds specifiers at runtime ('./' + file) so static copy/bundle analysis
// treats them as optional; the CommonJS loader supplies `require` in tests and bundles.
declare const require: any;
const _lazyRequire: any = typeof require !== 'undefined' ? require : undefined;
const _lazyCache = new Map<string, any>();
function _lazyLoad(file: string, key: string): any {
  if (!_lazyRequire) throw new Error(`electron.${key} requires the CommonJS runtime loader`);
  let mod = _lazyCache.get(file);
  if (!mod) {
    try { mod = _lazyRequire('./' + file); }
    catch (error: any) { throw new Error(`electron.${key} could not be loaded: ${error?.message || error}`); }
    _lazyCache.set(file, mod);
  }
  return mod[key];
}
function _lazyValue(file: string, key: string): any {
  const target: any = function () {};
  return new Proxy(target, {
    get(_t, prop) {
      const real = _lazyLoad(file, key);
      const value = Reflect.get(real, prop, real);
      return typeof value === 'function' ? value.bind(real) : value;
    },
    set(_t, prop, value) {
      const real = _lazyLoad(file, key);
      return Reflect.set(real, prop, value, real);
    },
    has(_t, prop) {
      const real = _lazyLoad(file, key);
      return prop in real;
    },
    ownKeys(_t) {
      const real = _lazyLoad(file, key);
      return Reflect.ownKeys(real);
    },
    getOwnPropertyDescriptor(_t, prop) {
      const real = _lazyLoad(file, key);
      return Reflect.getOwnPropertyDescriptor(real, prop);
    },
    apply(_t: any, thisArg: any, args: any[]) {
      const real = _lazyLoad(file, key);
      return Reflect.apply(real, thisArg, args);
    },
    construct(_t: any, args: any[]) {
      const real = _lazyLoad(file, key);
      return Reflect.construct(real, args);
    },
    getPrototypeOf(_t) {
      const real = _lazyLoad(file, key);
      try { return Object.getPrototypeOf(real); } catch { return null; }
    },
  });
}
export const Menu: any = _lazyValue('electron-menu', 'Menu');
export const MenuItem: any = _lazyValue('electron-menu', 'MenuItem');
export const net: any = _lazyValue('electron-net', 'net');
export const nativeImage: any = _lazyValue('electron-image', 'nativeImage');
export const nativeTheme: any = _lazyValue('electron-theme', 'nativeTheme');
export const screen: any = _lazyValue('electron-system', 'screen');
export const Tray: any = _lazyValue('electron-system', 'Tray');
export const globalShortcut: any = _lazyValue('electron-system', 'globalShortcut');
export const MessageChannelMain: any = _lazyValue('electron-system', 'MessageChannelMain');
export const MessagePortMain: any = _lazyValue('electron-system', 'MessagePortMain');
export const webFrame: any = _lazyValue('electron-system', 'webFrame');
export const BaseWindow: any = _lazyValue('electron-views', 'BaseWindow');
export const BrowserView: any = _lazyValue('electron-views', 'BrowserView');
export const View: any = _lazyValue('electron-views', 'View');
export const WebContentsView: any = _lazyValue('electron-webcontentsview', 'WebContentsView');
export const webFrameMain: any = _lazyValue('electron-webcontentsview', 'webFrameMain');
export const contentTracing: any = _lazyValue('electron-webcontentsview', 'contentTracing');
export const crashReporter: any = _lazyValue('electron-capture', 'crashReporter');
export const desktopCapturer: any = _lazyValue('electron-capture', 'desktopCapturer');
export const inAppPurchase: any = _lazyValue('electron-capture', 'inAppPurchase');
export const netLog: any = _lazyValue('electron-logging', 'netLog');
export const powerMonitor: any = _lazyValue('electron-logging', 'powerMonitor');
export const pushNotifications: any = _lazyValue('electron-logging', 'pushNotifications');
export const ShareMenu: any = _lazyValue('electron-platform', 'ShareMenu');
export const systemPreferences: any = _lazyValue('electron-platform', 'systemPreferences');
export const TouchBar: any = _lazyValue('electron-platform', 'TouchBar');
export const utilityProcess: any = _lazyValue('electron-utilities', 'utilityProcess');
export const webUtils: any = _lazyValue('electron-utilities', 'webUtils');
export const autoUpdater: any = _lazyValue('electron-utilities', 'autoUpdater');
export const powerSaveBlocker: any = _lazyValue('electron-system', 'powerSaveBlocker');
export const session: any = _lazyValue('electron-session', 'session');
export const Session: any = _lazyValue('electron-session', 'Session');
const g: any = globalThis;
const page = () => g.window?.tsgpui;
function warn(api: string) { compatWarnings.push(`${api} is not supported and has no effect`); }
function unsupported(api: string): never { throw new Error(`${api} is not supported by TSGPUI`); }
class App extends EventEmitter {
  isPackaged = true;
  private ready = false;
  private pathOverrides = new Map<string, string>();
  private badgeCountValue = 0;
  private loginItemSettingsValue: any = { openAtLogin: false, openAsHidden: false, wasOpenedAtLogin: false, wasOpenedAsHidden: false, restoreState: false };
  private singleInstanceLockValue = false;
  private dockVisible = false;
  private dockBadge = '';
  private dockMenu: any = null;
  private dockIcon: any = null;
  private dockBounceId = 0;
  private nameOverride: string | null = null;
  dock: any;
  commandLine: any;
  constructor() {
    super();
    this.dock = {
      show: () => { this.dockVisible = true; try { void host('app.dock.show').catch(() => {}); } catch {} },
      hide: () => { this.dockVisible = false; try { void host('app.dock.hide').catch(() => {}); } catch {} },
      isVisible: () => this.dockVisible,
      bounce: (_type?: any) => { this.dockBounceId += 1; return this.dockBounceId; },
      cancelBounce: (_id?: any) => {},
      setBadge: (text?: any) => { this.dockBadge = String(text ?? ''); try { this.badgeCountValue = Number(text) || 0; } catch {} },
      getBadge: () => this.dockBadge,
      setMenu: (menu?: any) => { this.dockMenu = menu ?? null; },
      getMenu: () => this.dockMenu,
      setIcon: (image?: any) => { this.dockIcon = image ?? null; },
    };
    this.commandLine = { hasSwitch: (_name: string) => false, getSwitchValue: (_name: string) => '' };
    infoReady.then(() => { this.ready = true; this.emit('ready'); });
  }
  whenReady() { return infoReady.then(() => undefined); }
  isReady() { return this.ready; }
  quit() { this.emit('before-quit', {}); this.emit('will-quit', {}); void host('app.quit').catch(console.error); this.emit('quit', {}, 0); }
  exit(exitCode: number = 0) { this.emit('before-quit', {}); this.emit('will-quit', {}); void host('app.quit').catch(console.error); this.emit('quit', {}, exitCode); }
  relaunch(_options?: any) { warn('app.relaunch'); }
  getVersion() { return info.version || ''; }
  getName() { return this.nameOverride ?? (info.name || ''); }
  setName(name: string) { this.nameOverride = String(name ?? ''); }
  getAppPath() { return this.pathOverrides.get('appPath') ?? info.paths?.appFolder ?? ''; }
  hide() { /* no-op */ }
  show() { /* no-op */ }
  focus(_options?: any) { /* no-op */ }
  isHidden() { return false; }
  getPath(name: string) {
    if (this.pathOverrides.has(name)) return this.pathOverrides.get(name);
    const paths = info.paths || {};
    if (!Object.prototype.hasOwnProperty.call(paths, name) || typeof paths[name] !== 'string' || !paths[name]) return unsupported(`app.getPath(${name}): path not provided by host`);
    return paths[name];
  }
  setPath(name: string, pathValue: string) { this.pathOverrides.set(name, pathValue); if (name === 'appPath') this.pathOverrides.set('appPath', pathValue); }
  // badge
  get badgeCount() { return this.badgeCountValue; }
  set badgeCount(value: number) { this.badgeCountValue = Number(value) || 0; }
  setBadgeCount(count: number) { this.badgeCountValue = Number(count) || 0; return this.badgeCountValue !== 0; }
  getBadgeCount() { return this.badgeCountValue; }
  // login items
  getLoginItemSettings(_options?: any) { return { ...this.loginItemSettingsValue }; }
  setLoginItemSettings(settings: any) { this.loginItemSettingsValue = { ...this.loginItemSettingsValue, ...(settings || {}) }; }
  // single instance
  requestSingleInstanceLock(_additionalData?: any) { if (this.singleInstanceLockValue) return false; this.singleInstanceLockValue = true; return true; }
  releaseSingleInstanceLock() { this.singleInstanceLockValue = false; }
  hasSingleInstanceLock() { return this.singleInstanceLockValue; }
  // locale
  getLocale() { return info.locale || 'en-US'; }
  getLocaleCountryCode() { return info.localeCountryCode || 'US'; }
  // dock helpers at app level
  setDockIcon? (_image: any) { warn('app.setDockIcon'); }
}
export const app = new App();
const windows = new Map<number, BrowserWindow>();
let nextWindowId = 1;
g.__tsgpui_window_closed = (id: number) => windows.get(id)?.didClose();
// The host reports native window lifecycle changes (focus, blur, resize, move, minimize, maximize, restore,
// show, hide, ready-to-show, enter-full-screen, leave-full-screen, will-move, will-resize)
// and user close requests (close-requested). Only windows with a 'close' listener receive close-requested.
g.__tsgpui_window_event = (id: number, name: string, ...args: any[]) => windows.get(id)?.dispatchNative(name, ...args);
function numberOr(value: any, fallback: number): number { return typeof value === 'number' && Number.isFinite(value) ? value : fallback; }
export class BrowserWindow extends EventEmitter {
  id: number; options: any; destroyed = false; webContents: any; parent: BrowserWindow | null = null; private created: Promise<any>; private closeWatched = false; private visible: boolean; private fullScreen: boolean; private fullScreenable: boolean;
  private bounds: {x: number; y: number; width: number; height: number};
  private normalBounds: {x: number; y: number; width: number; height: number};
  private minimized: boolean;
  private maximized: boolean;
  private focused: boolean;
  private readyToShowEmitted: boolean;
  private alwaysOnTop: boolean;
  private skipTaskbar: boolean;
  private resizable: boolean;
  private movable: boolean;
  private minimizable: boolean;
  private maximizable: boolean;
  private closable: boolean;
  private kiosk: boolean;
  private simpleFullScreen: boolean;
  private aspectRatio: number;
  private menu: any = null;
  private browserView: any = null;
  private browserViews: any[] = [];
  private opacity: number;
  private backgroundColor: any = null;
  private ignoreMouseEvents: boolean;
  private progressBar: number;
  private overlayIcon: any = null;
  private thumbarButtons: any[] = [];
  private thumbnailClip: any = null;
  private thumbnailToolTip: string;
  private representedFilename: string;
  private documentEdited: boolean;
  private flashing: boolean;
  private title: string;
  private shadow = true;
  private focusable = true;
  private enabled = true;
  constructor(options: any = {}) {
    super();
    const parent = options.parent ?? null;
    if (parent !== null && !(parent instanceof BrowserWindow)) throw new TypeError('BrowserWindow parent must be a BrowserWindow');
    if (parent && parent.destroyed) throw new Error('BrowserWindow parent is destroyed');
    this.id = nextWindowId++; this.options = options; this.parent = parent; windows.set(this.id, this);
    this.visible = options.show !== false; this.fullScreen = options.fullScreen === true; this.fullScreenable = options.fullScreenable !== false;
    this.bounds = {x: numberOr(options.x, 0), y: numberOr(options.y, 0), width: Math.max(200, Math.min(4096, numberOr(options.width, 800))), height: Math.max(200, Math.min(4096, numberOr(options.height, 600)))};
    this.normalBounds = {...this.bounds};
    this.minimized = false; this.maximized = false; this.focused = false; this.readyToShowEmitted = false;
    this.alwaysOnTop = options.alwaysOnTop === true;
    this.skipTaskbar = options.skipTaskbar === true;
    this.resizable = options.resizable !== false;
    this.movable = options.movable !== false;
    this.minimizable = options.minimizable !== false;
    this.maximizable = options.maximizable !== false;
    this.closable = options.closable !== false;
    this.kiosk = options.kiosk === true;
    this.simpleFullScreen = options.simpleFullScreen === true;
    this.aspectRatio = numberOr(options.aspectRatio, 0);
    this.opacity = numberOr(options.opacity, 1);
    this.backgroundColor = options.backgroundColor ?? null;
    this.ignoreMouseEvents = false;
    this.progressBar = -1;
    this.thumbnailToolTip = '';
    this.representedFilename = '';
    this.documentEdited = false;
    this.flashing = false;
    this.title = options.title ?? '';
    if (parent && this.id === 1) compatWarnings.push('BrowserWindow parent is ignored for the first window');
    if (options.modal && !parent) compatWarnings.push('BrowserWindow modal without a parent has no effect');
    const link = parent && this.id !== 1 ? {parentWindowId: parent.id, modal: options.modal === true} : {};
    this.created = this.id === 1 ? Promise.resolve() : host('window.create', {windowId:this.id, title:options.title, width:this.bounds.width, height:this.bounds.height, x:this.bounds.x, y:this.bounds.y, show:this.visible === true, fullScreen:this.fullScreen === true, ...link});
    this.created.then(() => { if (!this.destroyed) this.emitReadyToShow(); }).catch(() => {});
    this.created.catch(error => { console.error('Unable to create window:', error); this.didClose(); });
    const events = new EventEmitter();
    const win = this;
    const webPreferences = (options && typeof options.webPreferences === 'object' && options.webPreferences) ? options.webPreferences : {};
    let findInPageCounter = 0;
    const audioMutedState = { value: false };
    const zoomState = { factor: 1, level: 0 };
    const frameRateState = { value: 60 };
    const insertedCss = new Map<string, string>();
    const wc: any = {
      id: win.id,
      isDestroyed: () => win.destroyed,
      send: (channel: string, ...args: any[]) => {
        const payload = args.length <= 1 ? args[0] : args;
        if (win.id === 1 && g.__tsgpui_emit && g.__tsgpui_window_id !== 0) g.__tsgpui_emit(channel, payload);
        else void win.command('window.send', {channel, payload}).catch(console.error);
      },
      sendToFrame: (frameId: any, channel: string, ...args: any[]) => {
        const payload = args.length <= 1 ? args[0] : args;
        wc.send(channel, payload);
      },
      on: events.on.bind(events),
      once: events.once.bind(events),
      removeListener: events.removeListener.bind(events),
      addListener: events.on.bind(events),
      off: events.removeListener.bind(events),
      emit: events.emit.bind(events),
      removeAllListeners: events.removeAllListeners.bind(events),
      listenerCount: events.listenerCount.bind(events),
      loadURL: (url: string) => win.id === 1 ? Promise.reject(new Error('webContents.loadURL is not supported for the first window')) : win.loadURL(url),
      loadFile: (file: string) => win.id === 1 ? Promise.reject(new Error('webContents.loadFile is not supported for the first window')) : win.loadFile(file),
      // navigation stubs
      goBack() { warn('webContents.goBack'); return undefined; },
      goForward() { warn('webContents.goForward'); return undefined; },
      canGoBack() { return false; },
      canGoForward() { return false; },
      reload() { warn('webContents.reload'); return undefined; },
      reloadIgnoringCache() { warn('webContents.reloadIgnoringCache'); return undefined; },
      stop() { return undefined; },
      canGoToOffset(_offset: number) { return false; },
      goToIndex(_index: number) { warn('webContents.goToIndex'); return undefined; },
      goToOffset(_offset: number) { warn('webContents.goToOffset'); return undefined; },
      clearHistory() { return undefined; },
      // devtools stubs
      openDevTools(_options?: any) { warn('webContents.openDevTools'); return undefined; },
      closeDevTools() { warn('webContents.closeDevTools'); return undefined; },
      isDevToolsOpened() { return false; },
      isDevToolsFocused() { return false; },
      toggleDevTools() { warn('webContents.toggleDevTools'); return undefined; },
      getDevToolsTitle() { return ''; },
      setDevToolsTitle(_title: string) { return undefined; },
      // page access
      getTitle() { try { return (g.document?.title) || ''; } catch { return ''; } },
      getURL() { try { return (g.location?.href) || ''; } catch { return ''; } },
      isLoading() { return false; },
      isLoadingMainFrame() { return false; },
      // js
      executeJavaScript(code: string, userGesture?: boolean) { try { return Promise.resolve((0, g.eval)(code)); } catch (e) { return Promise.reject(e); } },
      executeJavaScriptInIsolatedWorld(worldId: number, scripts: any, userGesture?: boolean) { try { const code = Array.isArray(scripts) ? scripts.map((s:any) => s.code || s).join('\\n') : String(scripts); return Promise.resolve((0, g.eval)(code)); } catch (e) { return Promise.reject(e); } },
      // prefs/session
      getWebPreferences() { return { ...webPreferences }; },
      get session() { try { return session.defaultSession; } catch { return null; } },
      // zoom
      get zoomLevel() { return zoomState.level; },
      set zoomLevel(level: number) { wc.setZoomLevel(level); },
      setZoomLevel(level: number) { zoomState.level = Number(level) || 0; },
      getZoomLevel() { return zoomState.level; },
      get zoomFactor() { return zoomState.factor; },
      set zoomFactor(factor: number) { wc.setZoomFactor(factor); },
      setZoomFactor(factor: number) { if (typeof factor === 'number' && factor > 0) { zoomState.factor = factor; } else { warn('webContents.setZoomFactor'); } },
      getZoomFactor() { return zoomState.factor; },
      setVisualZoomLevelLimits(minimumLevel: number, maximumLevel: number) { return Promise.resolve(); },
      // find in page
      findInPage(text: string, options?: any) { findInPageCounter += 1; return findInPageCounter; },
      stopFindInPage(action?: string) { return undefined; },
      // print
      print(options?: any, callback?: Function) { if (typeof callback === 'function') { try { setTimeout(() => callback(null), 0); } catch {} } return undefined; },
      printToPDF(options?: any) { return Promise.resolve(typeof Buffer !== 'undefined' ? Buffer.alloc(0) : new Uint8Array()); },
      // save/capture
      savePage(fullPath: string, saveType?: string) { return Promise.resolve(); },
      capturePage(rect?: any) { return Promise.resolve({ isEmpty: () => true, getSize: () => ({ width: 0, height: 0 }), toPNG: () => new Uint8Array(), toJPEG: () => new Uint8Array(), toBitmap: () => new Uint8Array(), toDataURL: () => '' }); },
      // window open handler
      setWindowOpenHandler(handler: Function) { wc._windowOpenHandler = handler; },
      _windowOpenHandler: null as any,
      getWindowOpenHandler() { return wc._windowOpenHandler; },
      // preloads
      setPreloads(preloads: string[]) { wc._preloads = Array.isArray(preloads) ? [...preloads] : []; },
      getPreloads() { return [...wc._preloads]; },
      _preloads: [] as string[],
      // process ids
      getOSProcessId() { return 0; },
      getProcessId() { return 0; },
      get processId() { return 0; },
      getType() { return 'page'; },
      getOwnerBrowserWindow() { return win; },
      get owner() { return win; },
      getFrameRate() { return frameRateState.value; },
      setFrameRate(frameRate: number) { frameRateState.value = numberOr(frameRate, frameRateState.value); },
      get frameRate() { return frameRateState.value; },
      set frameRate(value: number) { wc.setFrameRate(value); },
      // audio
      get audioMuted() { return audioMutedState.value; },
      set audioMuted(muted: boolean) { wc.setAudioMuted(muted); },
      setAudioMuted(muted: boolean) { audioMutedState.value = !!muted; },
      isAudioMuted() { return audioMutedState.value; },
      // css
      insertCSS(css: string, options?: any) { const key = 'css_' + (insertedCss.size + 1); insertedCss.set(key, css); return Promise.resolve(key); },
      removeInsertedCSS(key: string) { insertedCss.delete(key); return Promise.resolve(); },
      // frames
      get mainFrame() {
        return {
          url: wc.getURL(),
          name: '',
          parentFrame: null,
          framesInSubtree: [this],
          routingId: 1,
          processId: 0,
          executeJavaScript: (code: string, userGesture?: boolean) => wc.executeJavaScript(code, userGesture),
          executeJavaScriptInIsolatedWorld: (worldId: number, scripts: any, userGesture?: boolean) => wc.executeJavaScriptInIsolatedWorld(worldId, scripts, userGesture),
          isDestroyed: () => win.destroyed,
          on: events.on.bind(events),
          once: events.once.bind(events),
          removeListener: events.removeListener.bind(events),
        };
      },
      get frames() { return [wc.mainFrame]; },
      // misc
      close() { /* no-op */ },
      focus() { /* no-op */ },
      blur() { /* no-op */ },
      isFocused() { return false; },
      sendInputEvent(event: any) { return; },
    };
    this.webContents = wc;
  }
  /** Subscribing to 'close' asks the host to report user close requests, so listeners can cancel them. */
  on(name: string | symbol, fn: Function): this {
    if (name === 'close' && !this.closeWatched && !this.destroyed) { this.closeWatched = true; void this.command('window.watchClose').catch(console.error); }
    return super.on(name, fn);
  }
  dispatchNative(name: string, ...args: any[]) {
    if (this.destroyed) return;
    // An event may carry geometry (from the native host) or a single payload object (from tests).
    const payload: any = args.length === 1 && args[0] && typeof args[0] === 'object' && !Array.isArray(args[0]) ? args[0] : (args.length >= 4 ? {x: args[0], y: args[1], width: args[2], height: args[3]} : undefined);
    const num = (value: any, fallback: number) => typeof value === 'number' && Number.isFinite(value) ? value : fallback;
    if (name === 'close-requested') { this.requestClose(); return; }
    if (name === 'ready-to-show') { this.emitReadyToShow(); return; }
    if (name === 'show') { if (!this.visible) { this.visible = true; this.emit('show'); } return; }
    if (name === 'hide') { if (this.visible) { this.visible = false; this.emit('hide'); } return; }
    if (name === 'enter-full-screen') { if (!this.fullScreen) { this.fullScreen = true; this.emit('enter-full-screen'); } return; }
    if (name === 'leave-full-screen') { if (this.fullScreen) { this.fullScreen = false; this.emit('leave-full-screen'); } return; }
    if (name === 'focus') { if (!this.focused) { this.focused = true; this.emit('focus'); } return; }
    if (name === 'blur') { if (this.focused) { this.focused = false; this.emit('blur'); } return; }
    if (name === 'minimize') { if (!this.minimized) { this.minimized = true; this.maximized = false; this.emit('minimize'); } return; }
    if (name === 'maximize') { if (!this.maximized || this.minimized) { this.maximized = true; this.minimized = false; this.emit('maximize'); } return; }
    if (name === 'unmaximize') { if (this.maximized) { this.maximized = false; this.emit('unmaximize'); } return; }
    if (name === 'restore') { if (this.minimized || this.maximized) { this.minimized = false; this.maximized = false; this.emit('restore'); } return; }
    if (name === 'move' || name === 'moved') {
      if (payload && (typeof payload.x === 'number' || typeof payload.y === 'number')) {
        if (typeof payload.x === 'number') this.bounds.x = num(payload.x, this.bounds.x);
        if (typeof payload.y === 'number') this.bounds.y = num(payload.y, this.bounds.y);
        if (typeof payload.width === 'number') this.bounds.width = Math.max(200, Math.min(4096, num(payload.width, this.bounds.width)));
        if (typeof payload.height === 'number') this.bounds.height = Math.max(200, Math.min(4096, num(payload.height, this.bounds.height)));
      }
      if (this.isNormal()) this.normalBounds = {...this.bounds};
      if (name === 'move') { this.emit('will-move'); this.emit('move'); }
      else { this.emit('moved'); }
      return;
    }
    if (name === 'resize' || name === 'resized') {
      if (payload && (typeof payload.width === 'number' || typeof payload.height === 'number' || typeof payload.x === 'number' || typeof payload.y === 'number')) {
        if (typeof payload.x === 'number') this.bounds.x = num(payload.x, this.bounds.x);
        if (typeof payload.y === 'number') this.bounds.y = num(payload.y, this.bounds.y);
        if (typeof payload.width === 'number') this.bounds.width = Math.max(200, Math.min(4096, num(payload.width, this.bounds.width)));
        if (typeof payload.height === 'number') this.bounds.height = Math.max(200, Math.min(4096, num(payload.height, this.bounds.height)));
      }
      if (this.isNormal()) this.normalBounds = {...this.bounds};
      if (name === 'resize') { this.emit('will-resize'); this.emit('resize'); }
      else { this.emit('resized'); }
      return;
    }
    this.emit(name);
  }
  private emitReadyToShow() { this.readyToShowEmitted = true; this.emit('ready-to-show'); }
  getParentWindow() { return this.parent; }
  private command(op: string, args: any = {}) { return this.created.then(() => host(op, {...args, windowId:this.id})); }
  loadFile(file: string) { this.options.loadedFile = file; if (this.id === 1) { return Promise.resolve().then(() => { if (!this.destroyed) { this.emitReadyToShow(); try { this.webContents.emit('did-finish-load'); } catch {} } }); } const normalized = file.replaceAll('\\','/'); const segments = normalized.split('/').filter(part => part && part !== '.'); if (!segments.length || segments.includes('..') || /^[a-z]:/i.test(normalized) || normalized.startsWith('//')) return Promise.reject(new Error('Additional windows require a project-relative packaged HTML path')); return this.command('window.load', {path:'/src/renderer/' + segments.map(encodeURIComponent).join('/')}).then(value => { if (!this.destroyed) { this.emitReadyToShow(); try { this.webContents.emit('did-finish-load'); } catch {} } return value; }); }
  loadURL(url: string) { this.options.loadedURL = url; if (this.id === 1) { return Promise.resolve().then(() => { if (!this.destroyed) { this.emitReadyToShow(); try { this.webContents.emit('did-finish-load'); } catch {} } }); } if (url.startsWith('tsgpui://localhost/')) return this.command('window.load', {path:url.slice('tsgpui://localhost'.length)}).then(value => { if (!this.destroyed) { this.emitReadyToShow(); try { this.webContents.emit('did-finish-load'); } catch {} } return value; }); const rewritten = rewriteCustomUrl(url); if (rewritten) return this.command('window.load', {path: rewritten.slice('tsgpui://localhost'.length)}).then(value => { if (!this.destroyed) { this.emitReadyToShow(); try { this.webContents.emit('did-finish-load'); } catch {} } return value; }); return Promise.reject(new Error('Only packaged local URLs are supported for additional windows')); }
  didClose() {
    if (this.destroyed) return;
    this.destroyed = true; windows.delete(this.id); this.emit('closed');
    // Electron quits when the last window closes unless the app handles 'window-all-closed'.
    if (!windows.size) { if (app.listenerCount('window-all-closed')) app.emit('window-all-closed'); else app.quit(); }
  }
  private requestClose() {
    let prevented = false;
    this.emit('close', {preventDefault() { prevented = true; }});
    if (!prevented) this.closeNow();
  }
  private closeNow() { if (this.destroyed) return; void this.command('window.close').catch(console.error); this.didClose(); }
  close() { if (this.destroyed) return; this.requestClose(); }
  minimize() { if (this.destroyed) return; if (!this.minimized) { this.minimized = true; this.emit('minimize'); } void this.command('window.minimize').catch(console.error); }
  maximize() { if (this.destroyed) return; if (!this.maximized || this.minimized) { this.maximized = true; this.minimized = false; this.emit('maximize'); } void this.command('window.maximize').catch(console.error); }
  restore() { if (this.destroyed) return; if (this.minimized || this.maximized) { this.minimized = false; this.maximized = false; this.emit('restore'); } void this.command('window.restore').catch(console.error); }
  show(): Promise<void> { if (this.destroyed) return Promise.resolve(); if (this.id === 1) return Promise.resolve(); if (this.visible) return this.command('window.show'); this.visible = true; this.emit('show'); return this.command('window.show'); }
  hide(): Promise<void> { if (this.destroyed) return Promise.resolve(); if (this.id === 1) return Promise.resolve(); if (!this.visible) return this.command('window.hide'); this.visible = false; this.emit('hide'); return this.command('window.hide'); }
  isVisible(): boolean { return this.visible; }
  setFullScreen(flag: boolean) { if (this.destroyed) return; if (this.id === 1) throw new Error('BrowserWindow fullscreen not supported on this renderer'); if (!g.__tsgpui_host && !g.window?.tsgpui?.invoke) throw new Error('BrowserWindow fullscreen not supported on this renderer'); const next = !!flag; if (next && !this.fullScreenable) return; if (next === this.fullScreen) return; this.fullScreen = next; this.emit(next ? 'enter-full-screen' : 'leave-full-screen'); void this.command('window.setFullScreen', {fullScreen: next}).catch(console.error); }
  isFullScreen(): boolean { return this.fullScreen; }
  isMaximized(): boolean { return this.maximized; }
  isMinimized(): boolean { return this.minimized; }
  isFocused(): boolean { return this.focused; }
  isNormal(): boolean { return !this.minimized && !this.maximized && !this.fullScreen; }
  unmaximize() { if (this.destroyed) return; if (!this.maximized) return; this.maximized = false; this.emit('unmaximize'); if (this.id !== 1) void this.command('window.restore').catch(console.error); }
  getBounds(): {x: number; y: number; width: number; height: number} { return {x: this.bounds.x, y: this.bounds.y, width: this.bounds.width, height: this.bounds.height}; }
  getSize(): {width: number; height: number} { return {width: this.bounds.width, height: this.bounds.height}; }
  getPosition(): [number, number] { return [this.bounds.x, this.bounds.y]; }
  setBounds(bounds: any, _animate?: boolean) { if (this.destroyed) return; if (!bounds || typeof bounds !== 'object') throw new TypeError('BrowserWindow.setBounds requires a bounds object'); const next = {x: numberOr((bounds as any).x, this.bounds.x), y: numberOr((bounds as any).y, this.bounds.y), width: Math.max(200, Math.min(4096, numberOr((bounds as any).width, this.bounds.width))), height: Math.max(200, Math.min(4096, numberOr((bounds as any).height, this.bounds.height)))}; const prev = this.bounds; const moved = next.x !== prev.x || next.y !== prev.y; const resized = next.width !== prev.width || next.height !== prev.height; if (!moved && !resized) return; if (moved) this.emit('will-move'); if (resized) this.emit('will-resize'); this.bounds = next; if (moved) this.emit('move'); if (resized) this.emit('resize'); if (this.isNormal()) this.normalBounds = {...this.bounds}; if (this.id !== 1) void this.command('window.setBounds', {...next}).catch(console.error); }
  setPosition(x: number, y: number, _animate?: boolean) { if (this.destroyed) return; const nextX = numberOr(x, this.bounds.x); const nextY = numberOr(y, this.bounds.y); if (nextX === this.bounds.x && nextY === this.bounds.y) return; this.emit('will-move'); this.bounds = {x: nextX, y: nextY, width: this.bounds.width, height: this.bounds.height}; this.emit('move'); if (this.isNormal()) this.normalBounds = {...this.bounds}; if (this.id !== 1) void this.command('window.setPosition', {x: nextX, y: nextY}).catch(console.error); }
  setSize(width: number, height: number, _animate?: boolean) { if (this.destroyed) return; const nextWidth = Math.max(200, Math.min(4096, numberOr(width, this.bounds.width))); const nextHeight = Math.max(200, Math.min(4096, numberOr(height, this.bounds.height))); if (nextWidth === this.bounds.width && nextHeight === this.bounds.height) return; this.emit('will-resize'); this.bounds = {x: this.bounds.x, y: this.bounds.y, width: nextWidth, height: nextHeight}; this.emit('resize'); if (this.isNormal()) this.normalBounds = {...this.bounds}; if (this.id !== 1) void this.command('window.setSize', {width: nextWidth, height: nextHeight}).catch(console.error); }
  focus() { if (this.destroyed) return; if (this.focused) return; this.focused = true; this.emit('focus'); if (this.id !== 1) void this.command('window.focus').catch(console.error); }
  blur() { if (this.destroyed) return; if (!this.focused) return; this.focused = false; this.emit('blur'); if (this.id !== 1) void this.command('window.blur').catch(console.error); }
  isDestroyed() { return this.destroyed; }
  private hostState(op: string, args: any = {}) {
    if (this.destroyed || this.id === 1) return;
    void this.command(op, args).catch(() => { compatWarnings.push(`${op} is not supported and has no effect`); });
  }
  // stateful chrome-ish properties
  getTitle() { return this.title; }
  setTitle(title: string) { if (this.destroyed) return; this.title = String(title ?? ''); void this.command('window.setTitle', {title: this.title}).catch(console.error); }
  isAlwaysOnTop() { return this.alwaysOnTop; }
  setAlwaysOnTop(flag: boolean, level?: any, relativeLevel?: any) { if (this.destroyed) return; this.alwaysOnTop = !!flag; this.hostState('window.setAlwaysOnTop', {alwaysOnTop: this.alwaysOnTop, level, relativeLevel}); }
  setSkipTaskbar(skip: boolean) { if (this.destroyed) return; this.skipTaskbar = !!skip; this.hostState('window.setSkipTaskbar', {skipTaskbar: this.skipTaskbar}); }
  isResizable() { return this.resizable; }
  setResizable(resizable: boolean) { if (this.destroyed) return; this.resizable = !!resizable; this.hostState('window.setResizable', {resizable: this.resizable}); }
  isMovable() { return this.movable; }
  setMovable(movable: boolean) { if (this.destroyed) return; this.movable = !!movable; this.hostState('window.setMovable', {movable: this.movable}); }
  isMinimizable() { return this.minimizable; }
  setMinimizable(minimizable: boolean) { if (this.destroyed) return; this.minimizable = !!minimizable; this.hostState('window.setMinimizable', {minimizable: this.minimizable}); }
  isMaximizable() { return this.maximizable; }
  setMaximizable(maximizable: boolean) { if (this.destroyed) return; this.maximizable = !!maximizable; this.hostState('window.setMaximizable', {maximizable: this.maximizable}); }
  isClosable() { return this.closable; }
  setClosable(closable: boolean) { if (this.destroyed) return; this.closable = !!closable; this.hostState('window.setClosable', {closable: this.closable}); }
  setFullScreenable(fullScreenable: boolean) { this.fullScreenable = !!fullScreenable; this.hostState('window.setFullScreenable', {fullScreenable: this.fullScreenable}); }
  isSimpleFullScreen() { return this.simpleFullScreen; }
  setSimpleFullScreen(flag: boolean) { if (this.destroyed) return; const next = !!flag; if (next === this.simpleFullScreen) return; this.simpleFullScreen = next; this.hostState('window.setSimpleFullScreen', {simpleFullScreen: this.simpleFullScreen}); }
  isKiosk() { return this.kiosk; }
  setKiosk(flag: boolean) { if (this.destroyed) return; this.kiosk = !!flag; this.hostState('window.setKiosk', {kiosk: this.kiosk}); }
  setAspectRatio(aspectRatio: number, extraSize?: any) { if (this.destroyed) return; this.aspectRatio = numberOr(aspectRatio, this.aspectRatio); this.hostState('window.setAspectRatio', {aspectRatio: this.aspectRatio, extraSize}); }
  getAspectRatio() { return this.aspectRatio; }
  // menu
  setMenu(menu: any) { if (this.destroyed) return; this.menu = menu ?? null; this.hostState('window.setMenu', {menu: this.menu}); }
  getMenu() { return this.menu; }
  removeMenu() { if (this.destroyed) return; this.menu = null; this.hostState('window.removeMenu', {}); }
  // child windows
  getChildWindows() { return [...windows.values()].filter(w => w.parent === this && !w.destroyed); }
  // browser view stubs
  getBrowserView() { return this.browserView; }
  getBrowserViews() { return [...this.browserViews]; }
  addBrowserView(view: any) { if (this.destroyed) return; if (view && !this.browserViews.includes(view)) this.browserViews.push(view); if (!this.browserView) this.browserView = view; this.hostState('window.addBrowserView', {view}); }
  removeBrowserView(view: any) { if (this.destroyed) return; this.browserViews = this.browserViews.filter(v => v !== view); if (this.browserView === view) this.browserView = this.browserViews[0] ?? null; this.hostState('window.removeBrowserView', {view}); }
  setBrowserView(view: any) { if (this.destroyed) return; this.browserView = view ?? null; this.browserViews = view ? [view] : []; this.hostState('window.setBrowserView', {view}); }
  // bounds
  getNormalBounds() { return { x: this.normalBounds.x, y: this.normalBounds.y, width: this.normalBounds.width, height: this.normalBounds.height }; }
  center() { if (this.destroyed) return; try {
    const primary = (screen as any)?.getPrimaryDisplay?.();
    const area = primary?.workAreaSize || primary?.size || { width: 1920, height: 1080 };
    const x = Math.max(0, Math.round((area.width - this.bounds.width) / 2));
    const y = Math.max(0, Math.round((area.height - this.bounds.height) / 2));
    this.setPosition(x, y);
  } catch { /* ignore */ } }
  flashFrame(flag: boolean) { if (this.destroyed) return; this.flashing = !!flag; this.hostState('window.flashFrame', {flag: this.flashing}); }
  // opacity / colors
  setOpacity(opacity: number) { if (this.destroyed) return; this.opacity = numberOr(opacity, this.opacity); this.hostState('window.setOpacity', {opacity: this.opacity}); }
  getOpacity() { return this.opacity; }
  setBackgroundColor(color: any) { if (this.destroyed) return; this.backgroundColor = color; this.hostState('window.setBackgroundColor', {backgroundColor: this.backgroundColor}); }
  getBackgroundColor() { return this.backgroundColor; }
  // mouse events
  setIgnoreMouseEvents(ignore: boolean, options?: any) { if (this.destroyed) return; this.ignoreMouseEvents = !!ignore; this.hostState('window.setIgnoreMouseEvents', {ignore: this.ignoreMouseEvents, options}); }
  // progress
  setProgressBar(progress: number, options?: any) { if (this.destroyed) return; this.progressBar = numberOr(progress, this.progressBar); this.hostState('window.setProgressBar', {progress: this.progressBar, options}); }
  // overlay / thumbnail
  setOverlayIcon(overlay: any, description: string) { if (this.destroyed) return; this.overlayIcon = overlay; this.hostState('window.setOverlayIcon', {overlay, description}); }
  setThumbarButtons(buttons: any[]) { if (this.destroyed) return; this.thumbarButtons = Array.isArray(buttons) ? [...buttons] : []; this.hostState('window.setThumbarButtons', {buttons: this.thumbarButtons}); return true; }
  setThumbnailClip(region: any) { if (this.destroyed) return; this.thumbnailClip = region; this.hostState('window.setThumbnailClip', {region}); }
  setThumbnailToolTip(toolTip: string) { if (this.destroyed) return; this.thumbnailToolTip = String(toolTip ?? ''); this.hostState('window.setThumbnailToolTip', {toolTip: this.thumbnailToolTip}); }
  // represented file / document edited
  setRepresentedFilename(filename: string) { if (this.destroyed) return; this.representedFilename = String(filename ?? ''); this.hostState('window.setRepresentedFilename', {filename: this.representedFilename}); }
  getRepresentedFilename() { return this.representedFilename; }
  setDocumentEdited(edited: boolean) { if (this.destroyed) return; this.documentEdited = !!edited; this.hostState('window.setDocumentEdited', {edited: this.documentEdited}); }
  isDocumentEdited() { return this.documentEdited; }
  // shadow/focusable/enabled
  hasShadow() { return this.shadow; }
  setHasShadow(hasShadow: boolean) { if (this.destroyed) return; this.shadow = !!hasShadow; this.hostState('window.setHasShadow', {hasShadow: this.shadow}); }
  isFocusable() { return this.focusable; }
  setFocusable(focusable: boolean) { if (this.destroyed) return; this.focusable = !!focusable; this.hostState('window.setFocusable', {focusable: this.focusable}); }
  isEnabled() { return this.enabled; }
  setEnabled(enabled: boolean) { if (this.destroyed) return; this.enabled = !!enabled; this.hostState('window.setEnabled', {enabled: this.enabled}); }
  // native handle
  getNativeWindowHandle() { try { if (typeof Buffer !== 'undefined') return Buffer.alloc(8); } catch {} return new Uint8Array(8); }
  // preview
  previewFile(path: string, displayName?: string) { warn('BrowserWindow.previewFile'); return undefined; }
  // closeable helpers already
  // bounds tracking update normalBounds on setBounds etc? patch below
  static getAllWindows() { return [...windows.values()]; }
  static getFocusedWindow() { for (const win of windows.values()) if (win.focused) return win; return [...windows.values()].find(win => !win.destroyed && win.visible && !win.minimized) || windows.values().next().value || null; }
}
// BaseWindow, BrowserView, View, WebContentsView are provided by their shards.
// utilityProcess is provided by the electron-utilities shard (see lazy loader above).
// Only the page-facing part of Electron's webContents: each window's webContents object (send, on, id).
export const webContents = {
  fromId(id: number) { return windows.get(id)?.webContents ?? null; },
  getAllWebContents() { return [...windows.values()].map(window => window.webContents); },
  getFocusedWebContents() { const win = BrowserWindow.getFocusedWindow(); return win ? win.webContents : null; },
  fromBrowserView(_view: any) { return null; },
  fromFrame(_frame: any) { return null; },
  fromWebContents(wc: any) { return wc ?? null; },
  fromDevToolsTargetId(_targetId: string) { return null; },
};
export const ipcMain = {
  handle(channel: string, fn: Function) { handle(channel, fn); },
  handleOnce(channel: string, fn: Function) { handle(channel, (...args: any[]) => { removeHandler(channel); return fn(...args); }); },
  removeHandler,
  on(channel: string, fn: Function) { listen(channel, fn); return this; },
  once(channel: string, fn: Function) { const wrapped = (...args: any[]) => { unlisten(channel, wrapped); fn(...args); }; (wrapped as any).listener = fn; listen(channel, wrapped); return this; },
  removeListener(channel: string, fn: Function) { unlisten(channel, fn); return this; }
};
const rendererListeners = new Map<string, Map<Function, Function[]>>();
function rendererOn(channel: string, fn: Function, once = false) { const bridge = page(); if (!bridge?.on) return unsupported('ipcRenderer.on'); const byFn = rendererListeners.get(channel) || new Map(); rendererListeners.set(channel, byFn); let unsubscribe: Function = () => {}; const wrapped = (payload: any) => { if (once) { unsubscribe(); const list = byFn.get(fn) || []; byFn.set(fn, list.filter(x => x !== unsubscribe)); } fn({}, payload); }; unsubscribe = bridge.on(channel, wrapped); byFn.set(fn, [...(byFn.get(fn) || []), unsubscribe]); }
function rendererRemoveAll(channel?: string) {
  const channels = channel ? [channel] : [...rendererListeners.keys()];
  for (const name of channels) {
    const map = rendererListeners.get(name);
    for (const unsubs of map?.values() || []) for (const unsub of unsubs) try { unsub(); } catch {}
    rendererListeners.delete(name);
  }
}
export const ipcRenderer = {
  invoke(channel: string, payload?: any) { return host('ipc.invoke', {channel, payload}); },
  send(channel: string, payload?: any) { void host('ipc.send', {channel, payload}); },
  sendSync(channel: string, ...args: any[]) { compatWarnings.push('ipcRenderer.sendSync is not supported by the async host bridge; use ipcRenderer.invoke instead'); return undefined; },
  postMessage(channel: string, message: any, transfer?: any[]) { void host('ipc.send', {channel, payload: message}); },
  on(channel: string, fn: Function) { rendererOn(channel, fn); return this; },
  once(channel: string, fn: Function) { rendererOn(channel, fn, true); return this; },
  removeListener(channel: string, fn: Function) { const map = rendererListeners.get(channel); for (const unsub of map?.get(fn) || []) unsub(); map?.delete(fn); return this; },
  removeAllListeners(channel?: string) { rendererRemoveAll(channel); return this; },
};
function copyBridgeValue(value: any, seen = new Map<any, any>()): any {
  if (typeof value === 'function') return (...args: any[]) => { const result = value(...args.map(arg => copyBridgeValue(arg))); return result && typeof result.then === 'function' ? Promise.resolve(result).then(v => copyBridgeValue(v)) : copyBridgeValue(result); };
  if (value === null || typeof value !== 'object') return value;
  if (seen.has(value)) return seen.get(value);
  if (!Array.isArray(value) && Object.prototype.toString.call(value) !== '[object Object]') throw new Error('contextBridge.exposeInMainWorld: unsupported value');
  const copy: any = Array.isArray(value) ? [] : {};
  seen.set(value, copy);
  for (const key of Object.keys(value)) Object.defineProperty(copy, key, { value: copyBridgeValue(value[key], seen), enumerable: true, writable: false, configurable: false });
  return Object.freeze(copy);
}
export const contextBridge = { exposeInMainWorld(key: string, value: any) { const target = g.__tsgpui_bridge_target || g.window || g; if (key in target) throw new Error(`contextBridge.exposeInMainWorld: ${key} already exists`); Object.defineProperty(target, key, { value: copyBridgeValue(value), enumerable: true, writable: false, configurable: false }); } };
function options(args: any[]) { return args.length > 1 ? args[1] : args[0] || {}; }
export const dialog = {
  async showOpenDialog(...args: any[]) { const o = options(args); const result = await host('dialog.openFile', {multiple: o.properties?.includes('multiSelections') || false, filters: o.filters}); return {canceled: result.canceled, filePaths: result.paths}; },
  async showSaveDialog(...args: any[]) { const o = options(args); const result = await host('dialog.saveFile', {defaultPath: o.defaultPath, filters: o.filters}); return {canceled: result.canceled, filePath: result.path}; },
  async showMessageBox(...args: any[]) { const o = options(args); return host('dialog.message', {type: o.type, title: o.title, message: o.message, buttons: o.buttons}); },
  showErrorBox(title: string, message: string) { void host('dialog.message', {type: 'error', title, message}); }
};
export const shell = {openExternal(url: string) { return host('shell.openExternal', {url}).then(() => undefined); }};
export const clipboard = { readText() { return host('clipboard.readText').then(r => r.text); }, writeText(text: string) { return host('clipboard.writeText', {text}).then(() => undefined); } };
// Windows only (DPAPI, current account). Every method is async because the host bridge is async; see the coverage ledger.
export const safeStorage = {
  isEncryptionAvailable(): Promise<boolean> { return host('safeStorage.isAvailable').then(Boolean); },
  encryptString(plainText: string): Promise<Uint8Array> { return host('safeStorage.encrypt', {value: String(plainText)}).then((encoded: string) => Uint8Array.from(fromBase64(encoded))); },
  decryptString(encrypted: Uint8Array): Promise<string> { return host('safeStorage.decrypt', {value: toBase64(Array.from(encrypted))}); },
};
function notificationToast(title: string, body: string) {
  const doc: any = g.document ?? g.window?.document;
  if (!doc || typeof doc.createElement !== 'function' || !doc.body) return false;
  const box = doc.createElement('div');
  box.setAttribute('role', 'status');
  box.setAttribute('aria-live', 'polite');
  box.setAttribute('data-tsgpui-notification-toast', '');
  box.textContent = title ? (body ? `${title} — ${body}` : title) : body;
  box.style.cssText = 'position:fixed;right:16px;bottom:16px;z-index:99999;max-width:320px;padding:10px 12px;border-radius:8px;background:#202124;color:#fff;font:13px/1.4 system-ui,sans-serif;box-shadow:0 6px 24px rgba(0,0,0,.35);';
  if (typeof box.addEventListener === 'function') box.addEventListener('click', () => { try { box.remove(); } catch (_ignored) { /* ignore */ } });
  doc.body.appendChild(box);
  if (typeof g.setTimeout === 'function') g.setTimeout(() => { try { box.remove(); } catch (_ignored) { /* ignore */ } }, 6000);
  return true;
}
export class Notification extends EventEmitter {
  options: any;
  constructor(options: any) { super(); this.options = options || {}; }
  show() {
    // Both hosts show an OS toast (native and WebView via notify-rust) when the grant is present.
    // A host can still reject when the grant is missing or the OS notification service is
    // unavailable. Electron's show() is void, so the rejection is handled here: log + compat
    // warning (the import report) instead of an unhandled rejection, plus a visible in-page toast
    // when a page document exists. The toast attempt itself is guarded, so show() never throws in
    // document-less (native/VM) contexts.
    const rawTitle = this.options?.title;
    const rawBody = this.options?.body;
    const title = rawTitle != null ? String(rawTitle) : '';
    const body = rawBody != null ? String(rawBody) : '';
    let pending: Promise<any>;
    try { pending = host('notifications.show', {title: rawTitle, body: rawBody}); } catch (error) { pending = Promise.reject(error); }
    void pending.then(
      () => { try { this.emit('show'); } catch (_ignored) { /* ignore */ } },
      (error: any) => {
        const message = error instanceof Error ? error.message : String(error);
        compatWarnings.push(`Notification.show host notification failed (${message}); showing an in-page toast instead`);
        try { console.error(`Notification.show: ${message}`); } catch (_ignored) { /* ignore */ }
        try { this.emit('error', error); } catch (_ignored) { /* ignore */ }
        try { notificationToast(title, body); } catch (toastError: any) { try { console.error('Notification fallback toast failed:', toastError); } catch (_ignored) { /* ignore */ } }
      },
    );
  }
  static isSupported() { return true; }
}
// Tray and globalShortcut implementations live in electron-system.ts.
// Everything else in Electron's surface is generated into electron-coverage.ts: each name is an
// `unsupportedModule` that throws a clear error on first use, instead of being undefined.
export * from './electron-coverage';
import * as coverage from './electron-coverage';
export default {...coverage, app, BrowserWindow, BaseWindow, BrowserView, WebContentsView, utilityProcess, webContents, screen, MessageChannelMain, webFrame, powerSaveBlocker, ipcMain, ipcRenderer, contextBridge, dialog, shell, clipboard, safeStorage, Notification, nativeTheme, Menu, MenuItem, net, nativeImage, Tray, globalShortcut, session, protocol, ShareMenu, systemPreferences, TouchBar, webUtils, autoUpdater};
