import { host, compatWarnings, handle, removeHandler, listen, unlisten } from './host';
import { info, infoReady } from './os';
import { EventEmitter } from './events';
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
  quit() { this.emit('before-quit', {}); void host('window.close'); }
  getVersion() { return info.version || ''; }
  getName() { return info.name || ''; }
  getPath(name: string) { const paths = info.paths || {}; if (!Object.prototype.hasOwnProperty.call(paths, name) || typeof paths[name] !== 'string' || !paths[name]) return unsupported(`app.getPath(${name}): path not provided by host`); return paths[name]; }
  getAppPath() { return info.paths?.appFolder || ''; }
}
export const app = new App();
let activeWindow: BrowserWindow | undefined;
export class BrowserWindow extends EventEmitter {
  options: any; destroyed = false; webContents: any;
  constructor(options: any = {}) { super(); if (activeWindow && !activeWindow.destroyed) throw new Error('multiple windows are not supported: BrowserWindow is limited to one live window'); activeWindow = this; this.options = options; const events = new EventEmitter(); this.webContents = {send: (channel: string, payload: any) => { if (g.__tsgpui_emit) g.__tsgpui_emit(channel, payload); else g.window?.__tsgpui_receive?.(channel, payload); }, on: events.on.bind(events), once: events.once.bind(events), removeListener: events.removeListener.bind(events)}; }
  loadFile(file: string) { this.options.loadedFile = file; return Promise.resolve(); }
  loadURL(url: string) { this.options.loadedURL = url; return Promise.resolve(); }
  setTitle(title: string) { void host('window.setTitle', {title}); }
  close() { if (this.destroyed) return; this.emit('close', {}); this.destroyed = true; void host('window.close'); this.emit('closed'); app.emit('window-all-closed'); }
  minimize() { void host('window.minimize'); }
  maximize() { void host('window.maximize'); }
  restore() { void host('window.restore'); }
  isDestroyed() { return this.destroyed; }
  static getAllWindows() { return activeWindow && !activeWindow.destroyed ? [activeWindow] : []; }
  static getFocusedWindow() { return activeWindow && !activeWindow.destroyed ? activeWindow : null; }
}
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
export const nativeTheme = {themeSource: 'system', get shouldUseDarkColors() { return this.themeSource === 'dark' || (this.themeSource === 'system' && !!g.window?.matchMedia?.('(prefers-color-scheme: dark)').matches); }};
export const Menu = {setApplicationMenu(_menu: any) { warn('Menu.setApplicationMenu'); }, buildFromTemplate(_template: any) { warn('Menu.buildFromTemplate'); return null; }};
export class Tray { constructor(_image?: any) { warn('Tray'); } setToolTip(_s: string) { warn('Tray.setToolTip'); } setContextMenu(_m: any) { warn('Tray.setContextMenu'); } on(_event: string, _fn: Function) { warn('Tray.on'); return this; } destroy() { warn('Tray.destroy'); } }
export const globalShortcut = {register(_accelerator: string, _fn: Function) { warn('globalShortcut.register'); return false; }, unregister(_accelerator: string) { warn('globalShortcut.unregister'); }, unregisterAll() { warn('globalShortcut.unregisterAll'); }, isRegistered(_accelerator: string) { warn('globalShortcut.isRegistered'); return false; }};
export default {app, BrowserWindow, ipcMain, ipcRenderer, contextBridge, dialog, shell, clipboard, Notification, nativeTheme, Menu, Tray, globalShortcut};
