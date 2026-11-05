import { host, compatWarnings, handle, removeHandler, listen, unlisten } from './host';
import { info, infoReady } from './os';
import { EventEmitter } from './events';
import { Menu, MenuItem } from './electron-menu';
export { Menu, MenuItem } from './electron-menu';
import { net } from './electron-net';
export { net } from './electron-net';
import { nativeImage } from './electron-image';
export { nativeImage } from './electron-image';
import { nativeTheme } from './electron-theme';
export { nativeTheme } from './electron-theme';
import { screen, MessageChannelMain, MessagePortMain, webFrame, powerSaveBlocker } from './electron-system';
export { screen, MessageChannelMain, MessagePortMain, webFrame, powerSaveBlocker } from './electron-system';
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
// The host reports native window lifecycle changes (focus, blur, resize, move, minimize, maximize, restore)
// and user close requests (close-requested). Only windows with a 'close' listener receive close-requested.
g.__tsgpui_window_event = (id: number, name: string) => windows.get(id)?.dispatchNative(name);
export class BrowserWindow extends EventEmitter {
  id: number; options: any; destroyed = false; webContents: any; parent: BrowserWindow | null = null; private created: Promise<any>; private closeWatched = false;
  constructor(options: any = {}) {
    super();
    const parent = options.parent ?? null;
    if (parent !== null && !(parent instanceof BrowserWindow)) throw new TypeError('BrowserWindow parent must be a BrowserWindow');
    if (parent && parent.destroyed) throw new Error('BrowserWindow parent is destroyed');
    this.id = nextWindowId++; this.options = options; this.parent = parent; windows.set(this.id, this);
    if (parent && this.id === 1) compatWarnings.push('BrowserWindow parent is ignored for the first window');
    if (options.modal && !parent) compatWarnings.push('BrowserWindow modal without a parent has no effect');
    const link = parent && this.id !== 1 ? {parentWindowId: parent.id, modal: options.modal === true} : {};
    this.created = this.id === 1 ? Promise.resolve() : host('window.create', {windowId:this.id, title:options.title, width:options.width, height:options.height, ...link});
    this.created.catch(error => { console.error('Unable to create window:', error); this.didClose(); });
    const events = new EventEmitter();
    this.webContents = {id: this.id, isDestroyed: () => this.destroyed, send: (channel: string, payload: any) => { if (this.id === 1 && g.__tsgpui_emit && g.__tsgpui_window_id !== 0) g.__tsgpui_emit(channel, payload); else void this.command('window.send', {channel,payload}).catch(console.error); }, on: events.on.bind(events), once: events.once.bind(events), removeListener: events.removeListener.bind(events)};
  }
  /** Subscribing to 'close' asks the host to report user close requests, so listeners can cancel them. */
  on(name: string | symbol, fn: Function): this {
    if (name === 'close' && !this.closeWatched && !this.destroyed) { this.closeWatched = true; void this.command('window.watchClose').catch(console.error); }
    return super.on(name, fn);
  }
  dispatchNative(name: string) {
    if (this.destroyed) return;
    if (name === 'close-requested') this.requestClose(); else this.emit(name);
  }
  getParentWindow() { return this.parent; }
  private command(op: string, args: any = {}) { return this.created.then(() => host(op, {...args, windowId:this.id})); }
  loadFile(file: string) { this.options.loadedFile = file; if (this.id === 1) return Promise.resolve(); const normalized = file.replaceAll('\\','/'); const segments = normalized.split('/').filter(part => part && part !== '.'); if (!segments.length || segments.includes('..') || /^[a-z]:/i.test(normalized) || normalized.startsWith('//')) return Promise.reject(new Error('Additional windows require a project-relative packaged HTML path')); return this.command('window.load', {path:'/src/renderer/' + segments.map(encodeURIComponent).join('/')}); }
  loadURL(url: string) { this.options.loadedURL = url; if (this.id === 1) return Promise.resolve(); if (!url.startsWith('tsgpui://localhost/')) return Promise.reject(new Error('Only packaged local URLs are supported for additional windows')); return this.command('window.load', {path:url.slice('tsgpui://localhost'.length)}); }
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
  minimize() { void this.command('window.minimize').catch(console.error); }
  maximize() { void this.command('window.maximize').catch(console.error); }
  restore() { void this.command('window.restore').catch(console.error); }
  isDestroyed() { return this.destroyed; }
  static getAllWindows() { return [...windows.values()]; }
  static getFocusedWindow() { return windows.values().next().value || null; }
}
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
export const contextBridge = { exposeInMainWorld(key: string, value: any) { const target = g.window || g; if (key in target) throw new Error(`contextBridge.exposeInMainWorld: ${key} already exists`); target[key] = value; } };
function options(args: any[]) { return args.length > 1 ? args[1] : args[0] || {}; }
export const dialog = {
  async showOpenDialog(...args: any[]) { const o = options(args); const result = await host('dialog.openFile', {multiple: o.properties?.includes('multiSelections') || false, filters: o.filters}); return {canceled: result.canceled, filePaths: result.paths}; },
  async showSaveDialog(...args: any[]) { const o = options(args); const result = await host('dialog.saveFile', {defaultPath: o.defaultPath, filters: o.filters}); return {canceled: result.canceled, filePath: result.path}; },
  async showMessageBox(...args: any[]) { const o = options(args); return host('dialog.message', {type: o.type, title: o.title, message: o.message, buttons: o.buttons}); },
  showErrorBox(title: string, message: string) { void host('dialog.message', {type: 'error', title, message}); }
};
export const shell = {openExternal(url: string) { return host('shell.openExternal', {url}).then(() => undefined); }};
export const clipboard = { readText() { return host('clipboard.readText').then(r => r.text); }, writeText(text: string) { return host('clipboard.writeText', {text}).then(() => undefined); } };
export class Notification extends EventEmitter { options: any; constructor(options: any) { super(); this.options = options; } show() { void host('notifications.show', {title: this.options.title, body: this.options.body}); } static isSupported() { return true; } }
export class Tray { constructor(_image?: any) { warn('Tray'); } setToolTip(_s: string) { warn('Tray.setToolTip'); } setContextMenu(_m: any) { warn('Tray.setContextMenu'); } on(_event: string, _fn: Function) { warn('Tray.on'); return this; } destroy() { warn('Tray.destroy'); } }
export const globalShortcut = {register(_accelerator: string, _fn: Function) { warn('globalShortcut.register'); return false; }, unregister(_accelerator: string) { warn('globalShortcut.unregister'); }, unregisterAll() { warn('globalShortcut.unregisterAll'); }, isRegistered(_accelerator: string) { warn('globalShortcut.isRegistered'); return false; }};
// Everything else in Electron's surface is generated into electron-coverage.ts: each name is an
// `unsupportedModule` that throws a clear error on first use, instead of being undefined.
export * from './electron-coverage';
import * as coverage from './electron-coverage';
export default {...coverage, app, BrowserWindow, webContents, screen, MessageChannelMain, webFrame, powerSaveBlocker, ipcMain, ipcRenderer, contextBridge, dialog, shell, clipboard, Notification, nativeTheme, Menu, MenuItem, net, nativeImage, Tray, globalShortcut};
