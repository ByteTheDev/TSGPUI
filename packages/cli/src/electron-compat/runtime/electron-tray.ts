import { host } from './host';
import { EventEmitter } from './events';

type Rectangle = {x: number; y: number; width: number; height: number};

// Host bridge ids for Tray instances. The native host owns the OS tray icon; this shim keeps the
// Electron-facing state and forwards mutations through `tray.*` ops.
let nextTrayId = 1;

function safeHost(op: string, args: any = {}): void {
  try { void host(op, args).catch(() => {}); } catch {}
}

function serializeMenu(menu: any): any {
  if (!menu) return null;
  if (Array.isArray(menu)) return menu;
  if (Array.isArray(menu.items)) return menu.items.map((item: any) => ({
    id: item?.id ?? null,
    label: item?.label ?? '',
    type: item?.type ?? 'normal',
    enabled: item?.enabled !== false,
    visible: item?.visible !== false,
    checked: !!item?.checked,
    submenu: serializeMenu(item?.submenu ?? null),
  }));
  return menu;
}

export class Tray extends EventEmitter {
  id: number;
  image: any;
  pressedImage: any = null;
  private _toolTip = '';
  private _title = '';
  private _contextMenu: any = null;
  bounds: Rectangle = {x: 0, y: 0, width: 0, height: 0};
  destroyed = false;
  ignoreDoubleClickEvents = false;
  constructor(image?: any) {
    super();
    this.image = image ?? null;
    this.id = nextTrayId++;
    safeHost('tray.create', {id: this.id, image: this.image});
  }
  setImage(image: any) { this.image = image ?? null; safeHost('tray.setIcon', {id: this.id, image: this.image}); }
  setPressedImage(image: any) { this.pressedImage = image ?? null; safeHost('tray.setIcon', {id: this.id, image: this.pressedImage}); }
  setToolTip(toolTip: string) { this._toolTip = String(toolTip ?? ''); safeHost('tray.setToolTip', {id: this.id, toolTip: this._toolTip}); }
  getToolTip() { return this._toolTip; }
  setTitle(title: string) { this._title = String(title ?? ''); }
  getTitle() { return this._title; }
  setContextMenu(menu: any) { this._contextMenu = menu ?? null; safeHost('tray.setContextMenu', {id: this.id, menu: serializeMenu(menu)}); }
  getContextMenu() { return this._contextMenu; }
  popUpContextMenu(menu?: any, position?: any) { /* native popup is not available in the host renderer */ }
  closeContextMenu() { /* no-op */ }
  getBounds() { return {x: this.bounds.x, y: this.bounds.y, width: this.bounds.width, height: this.bounds.height}; }
  isDestroyed() { return this.destroyed; }
  destroy() {
    if (this.destroyed) return;
    this.destroyed = true;
    safeHost('tray.destroy', {id: this.id});
    this.emit('destroyed');
  }
  focus() { /* no-op */ }
  displayBalloon(_options?: any) { /* no-op */ }
  removeBalloon() { /* no-op */ }
  getIgnoreDoubleClickEvents() { return this.ignoreDoubleClickEvents; }
  setIgnoreDoubleClickEvents(flag: boolean) { this.ignoreDoubleClickEvents = !!flag; }
}

// Global shortcuts are process-global. The shim keeps an in-memory registry for synchronous
// queries and forwards registration to the host, which performs the OS-level hotkey binding on
// platforms that support it (Windows first).
const globalShortcuts = new Map<string, Function>();
export const globalShortcut = {
  register(accelerator: string, callback: Function): boolean {
    if (typeof accelerator !== 'string' || !accelerator) throw new TypeError('globalShortcut.register requires an accelerator string');
    if (typeof callback !== 'function') throw new TypeError('globalShortcut.register requires a callback');
    globalShortcuts.set(accelerator, callback);
    safeHost('globalShortcut.register', {accelerator});
    return true;
  },
  registerAll(accelerators: string[], callback: Function) {
    if (!Array.isArray(accelerators)) throw new TypeError('globalShortcut.registerAll requires an array');
    for (const accelerator of accelerators) this.register(String(accelerator), callback);
    // Forward the batch as well, in case the host optimises multi-registration.
    safeHost('globalShortcut.register', {accelerators: accelerators.map(String)});
  },
  unregister(accelerator: string) {
    globalShortcuts.delete(String(accelerator));
    safeHost('globalShortcut.unregister', {accelerator: String(accelerator)});
  },
  unregisterAll() {
    globalShortcuts.clear();
    safeHost('globalShortcut.unregisterAll', {});
  },
  isRegistered(accelerator: string): boolean { return globalShortcuts.has(String(accelerator)); },
};