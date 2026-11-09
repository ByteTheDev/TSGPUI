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
export const MessageChannelMain: any = _lazyValue('electron-system', 'MessageChannelMain');
export const MessagePortMain: any = _lazyValue('electron-system', 'MessagePortMain');
export const webFrame: any = _lazyValue('electron-system', 'webFrame');
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
  constructor() { super(); infoReady.then(() => { this.ready = true; this.emit('ready'); }); }
  whenReady() { return infoReady.then(() => undefined); }
  isReady() { return this.ready; }
  quit() { this.emit('before-quit', {}); void host('app.quit').catch(console.error); }
  getVersion() { return info.version || ''; }
  getName() { return info.name || ''; }
  getPath(name: string) { const paths = info.paths || {}; if (!Object.prototype.hasOwnProperty.call(paths, name) || typeof paths[name] !== 'string' || !paths[name]) return unsupported(`app.getPath(${name}): path not provided by host`); return paths[name]; }
  getAppPath() { return info.paths?.appFolder || ''; }
}
export const app = new App();
const windows = new Map<number, BrowserWindow>();
let nextWindowId = 1;
g.__tsgpui_window_closed = (id: number) => windows.get(id)?.didClose();
// The host reports native window lifecycle changes (focus, blur, resize, move, minimize, maximize, restore,
// show, hide, ready-to-show, enter-full-screen, leave-full-screen, will-move, will-resize)
// and user close requests (close-requested). Only windows with a 'close' listener receive close-requested.
g.__tsgpui_window_event = (id: number, name: string) => windows.get(id)?.dispatchNative(name);
function numberOr(value: any, fallback: number): number { return typeof value === 'number' && Number.isFinite(value) ? value : fallback; }
export class BrowserWindow extends EventEmitter {
  id: number; options: any; destroyed = false; webContents: any; parent: BrowserWindow | null = null; private created: Promise<any>; private closeWatched = false; private visible: boolean; private fullScreen: boolean; private fullScreenable: boolean;
  private bounds: {x: number; y: number; width: number; height: number};
  private minimized: boolean;
  private maximized: boolean;
  private focused: boolean;
  private readyToShowEmitted: boolean;
  constructor(options: any = {}) {
    super();
    const parent = options.parent ?? null;
    if (parent !== null && !(parent instanceof BrowserWindow)) throw new TypeError('BrowserWindow parent must be a BrowserWindow');
    if (parent && parent.destroyed) throw new Error('BrowserWindow parent is destroyed');
    this.id = nextWindowId++; this.options = options; this.parent = parent; windows.set(this.id, this);
    this.visible = options.show !== false; this.fullScreen = options.fullScreen === true; this.fullScreenable = options.fullScreenable !== false;
    this.bounds = {x: numberOr(options.x, 0), y: numberOr(options.y, 0), width: Math.max(200, Math.min(4096, numberOr(options.width, 800))), height: Math.max(200, Math.min(4096, numberOr(options.height, 600)))};
    this.minimized = false; this.maximized = false; this.focused = false; this.readyToShowEmitted = false;
    if (parent && this.id === 1) compatWarnings.push('BrowserWindow parent is ignored for the first window');
    if (options.modal && !parent) compatWarnings.push('BrowserWindow modal without a parent has no effect');
    const link = parent && this.id !== 1 ? {parentWindowId: parent.id, modal: options.modal === true} : {};
    this.created = this.id === 1 ? Promise.resolve() : host('window.create', {windowId:this.id, title:options.title, width:this.bounds.width, height:this.bounds.height, x:this.bounds.x, y:this.bounds.y, show:this.visible === true, fullScreen:this.fullScreen === true, ...link});
    this.created.then(() => { if (!this.destroyed) this.emitReadyToShow(); }).catch(() => {});
    this.created.catch(error => { console.error('Unable to create window:', error); this.didClose(); });
    const events = new EventEmitter();
    this.webContents = {id: this.id, isDestroyed: () => this.destroyed, send: (channel: string, payload: any) => { if (this.id === 1 && g.__tsgpui_emit && g.__tsgpui_window_id !== 0) g.__tsgpui_emit(channel, payload); else void this.command('window.send', {channel,payload}).catch(console.error); }, on: events.on.bind(events), once: events.once.bind(events), removeListener: events.removeListener.bind(events),
      loadURL: (url: string) => this.id === 1 ? Promise.reject(new Error('webContents.loadURL is not supported for the first window')) : this.loadURL(url),
      loadFile: (file: string) => this.id === 1 ? Promise.reject(new Error('webContents.loadFile is not supported for the first window')) : this.loadFile(file),
      goBack: () => unsupported('webContents.goBack'), goForward: () => unsupported('webContents.goForward'),
      canGoBack: () => unsupported('webContents.canGoBack'), canGoForward: () => unsupported('webContents.canGoForward'),
      reload: () => unsupported('webContents.reload'), reloadIgnoringCache: () => unsupported('webContents.reloadIgnoringCache'),
      openDevTools: () => unsupported('webContents.openDevTools'), closeDevTools: () => unsupported('webContents.closeDevTools'),
      isDevToolsOpened: () => unsupported('webContents.isDevToolsOpened')};
  }
  /** Subscribing to 'close' asks the host to report user close requests, so listeners can cancel them. */
  on(name: string | symbol, fn: Function): this {
    if (name === 'close' && !this.closeWatched && !this.destroyed) { this.closeWatched = true; void this.command('window.watchClose').catch(console.error); }
    return super.on(name, fn);
  }
  dispatchNative(name: string) {
    if (this.destroyed) return;
    if (name === 'close-requested') { this.requestClose(); return; }
    if (name === 'ready-to-show') { this.emitReadyToShow(); return; }
    if (name === 'show') { if (!this.visible) { this.visible = true; this.emit('show'); } return; }
    if (name === 'hide') { if (this.visible) { this.visible = false; this.emit('hide'); } return; }
    if (name === 'enter-full-screen') { if (!this.fullScreen) { this.fullScreen = true; this.emit('enter-full-screen'); } return; }
    if (name === 'leave-full-screen') { if (this.fullScreen) { this.fullScreen = false; this.emit('leave-full-screen'); } return; }
    if (name === 'focus') { if (!this.focused) { this.focused = true; this.emit('focus'); } return; }
    if (name === 'blur') { if (this.focused) { this.focused = false; this.emit('blur'); } return; }
    if (name === 'minimize') { if (!this.minimized) { this.minimized = true; this.emit('minimize'); } return; }
    if (name === 'maximize') { if (!this.maximized || this.minimized) { this.maximized = true; this.minimized = false; this.emit('maximize'); } return; }
    if (name === 'restore') { if (this.minimized || this.maximized) { this.minimized = false; this.maximized = false; this.emit('restore'); } return; }
    if (name === 'move') { this.emit('will-move'); this.emit('move'); return; }
    if (name === 'resize') { this.emit('will-resize'); this.emit('resize'); return; }
    this.emit(name);
  }
  private emitReadyToShow() { this.readyToShowEmitted = true; this.emit('ready-to-show'); }
  getParentWindow() { return this.parent; }
  private command(op: string, args: any = {}) { return this.created.then(() => host(op, {...args, windowId:this.id})); }
  loadFile(file: string) { this.options.loadedFile = file; if (this.id === 1) { return Promise.resolve().then(() => { if (!this.destroyed) this.emitReadyToShow(); }); } const normalized = file.replaceAll('\\','/'); const segments = normalized.split('/').filter(part => part && part !== '.'); if (!segments.length || segments.includes('..') || /^[a-z]:/i.test(normalized) || normalized.startsWith('//')) return Promise.reject(new Error('Additional windows require a project-relative packaged HTML path')); return this.command('window.load', {path:'/src/renderer/' + segments.map(encodeURIComponent).join('/')}).then(value => { if (!this.destroyed) this.emitReadyToShow(); return value; }); }
  loadURL(url: string) { this.options.loadedURL = url; if (this.id === 1) { return Promise.resolve().then(() => { if (!this.destroyed) this.emitReadyToShow(); }); } if (url.startsWith('tsgpui://localhost/')) return this.command('window.load', {path:url.slice('tsgpui://localhost'.length)}).then(value => { if (!this.destroyed) this.emitReadyToShow(); return value; }); const rewritten = rewriteCustomUrl(url); if (rewritten) return this.command('window.load', {path: rewritten.slice('tsgpui://localhost'.length)}).then(value => { if (!this.destroyed) this.emitReadyToShow(); return value; }); return Promise.reject(new Error('Only packaged local URLs are supported for additional windows')); }
  setTitle(title: string) { void this.command('window.setTitle', {title}).catch(console.error); }
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
  setFullScreenable(fullScreenable: boolean) { this.fullScreenable = !!fullScreenable; }
  isMaximized(): boolean { return this.maximized; }
  isMinimized(): boolean { return this.minimized; }
  getBounds(): {x: number; y: number; width: number; height: number} { return {x: this.bounds.x, y: this.bounds.y, width: this.bounds.width, height: this.bounds.height}; }
  getSize(): {width: number; height: number} { return {width: this.bounds.width, height: this.bounds.height}; }
  getPosition(): [number, number] { return [this.bounds.x, this.bounds.y]; }
  setBounds(bounds: any, _animate?: boolean) { if (this.destroyed) return; if (!bounds || typeof bounds !== 'object') throw new TypeError('BrowserWindow.setBounds requires a bounds object'); const next = {x: numberOr((bounds as any).x, this.bounds.x), y: numberOr((bounds as any).y, this.bounds.y), width: Math.max(200, Math.min(4096, numberOr((bounds as any).width, this.bounds.width))), height: Math.max(200, Math.min(4096, numberOr((bounds as any).height, this.bounds.height)))}; const prev = this.bounds; const moved = next.x !== prev.x || next.y !== prev.y; const resized = next.width !== prev.width || next.height !== prev.height; if (!moved && !resized) return; if (moved) this.emit('will-move'); if (resized) this.emit('will-resize'); this.bounds = next; if (moved) this.emit('move'); if (resized) this.emit('resize'); if (this.id !== 1) void this.command('window.setBounds', {...next}).catch(console.error); }
  setPosition(x: number, y: number, _animate?: boolean) { if (this.destroyed) return; const nextX = numberOr(x, this.bounds.x); const nextY = numberOr(y, this.bounds.y); if (nextX === this.bounds.x && nextY === this.bounds.y) return; this.emit('will-move'); this.bounds = {x: nextX, y: nextY, width: this.bounds.width, height: this.bounds.height}; this.emit('move'); if (this.id !== 1) void this.command('window.setPosition', {x: nextX, y: nextY}).catch(console.error); }
  setSize(width: number, height: number, _animate?: boolean) { if (this.destroyed) return; const nextWidth = Math.max(200, Math.min(4096, numberOr(width, this.bounds.width))); const nextHeight = Math.max(200, Math.min(4096, numberOr(height, this.bounds.height))); if (nextWidth === this.bounds.width && nextHeight === this.bounds.height) return; this.emit('will-resize'); this.bounds = {x: this.bounds.x, y: this.bounds.y, width: nextWidth, height: nextHeight}; this.emit('resize'); if (this.id !== 1) void this.command('window.setSize', {width: nextWidth, height: nextHeight}).catch(console.error); }
  focus() { if (this.destroyed) return; if (this.focused) return; this.focused = true; this.emit('focus'); if (this.id !== 1) void this.command('window.focus').catch(console.error); }
  blur() { if (this.destroyed) return; if (!this.focused) return; this.focused = false; this.emit('blur'); if (this.id !== 1) void this.command('window.blur').catch(console.error); }
  isDestroyed() { return this.destroyed; }
  static getAllWindows() { return [...windows.values()]; }
  static getFocusedWindow() { for (const win of windows.values()) if (win.focused) return win; return [...windows.values()].find(win => !win.destroyed && win.visible && !win.minimized) || windows.values().next().value || null; }
}
// BaseWindow shares the native window lifecycle; composing child views is not yet supported.
export class BaseWindow extends BrowserWindow {
  get contentView(): never { return unsupported('BaseWindow.contentView'); }
}
export class BrowserView { constructor(_options?: any) { unsupported('BrowserView'); } }
export class WebContentsView { constructor(_options?: any) { unsupported('WebContentsView'); } }
export const utilityProcess = { fork(..._args: any[]): never { return unsupported('utilityProcess.fork'); } };
// Only the page-facing part of Electron's webContents: each window's webContents object (send, on, id).
export const webContents = {
  fromId(id: number) { return windows.get(id)?.webContents ?? null; },
  getAllWebContents() { return [...windows.values()].map(window => window.webContents); }
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
export const ipcRenderer = {
  invoke(channel: string, payload?: any) { return host('ipc.invoke', {channel, payload}); },
  send(channel: string, payload?: any) { void host('ipc.send', {channel, payload}); },
  on(channel: string, fn: Function) { rendererOn(channel, fn); return this; },
  once(channel: string, fn: Function) { rendererOn(channel, fn, true); return this; },
  removeListener(channel: string, fn: Function) { const map = rendererListeners.get(channel); for (const unsub of map?.get(fn) || []) unsub(); map?.delete(fn); return this; }
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
export class Tray { constructor(_image?: any) { throw new Error('Tray is not supported by TSGPUI'); } }
export const globalShortcut = { register(_accelerator: string, _fn: Function): boolean { throw new Error('globalShortcut.register is not supported by TSGPUI'); }, unregister(_accelerator: string) { throw new Error('globalShortcut.unregister is not supported by TSGPUI'); }, unregisterAll() { throw new Error('globalShortcut.unregisterAll is not supported by TSGPUI'); }, isRegistered(_accelerator: string): boolean { throw new Error('globalShortcut.isRegistered is not supported by TSGPUI'); } };
// Everything else in Electron's surface is generated into electron-coverage.ts: each name is an
// `unsupportedModule` that throws a clear error on first use, instead of being undefined.
export * from './electron-coverage';
import * as coverage from './electron-coverage';
export default {...coverage, app, BrowserWindow, BaseWindow, BrowserView, WebContentsView, utilityProcess, webContents, screen, MessageChannelMain, webFrame, powerSaveBlocker, ipcMain, ipcRenderer, contextBridge, dialog, shell, clipboard, safeStorage, Notification, nativeTheme, Menu, MenuItem, net, nativeImage, Tray, globalShortcut, session, protocol};
