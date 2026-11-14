import { compatWarnings, host, onHostEvent, toBase64 } from './host';
import { EventEmitter } from './events';

type Rectangle = {x: number; y: number; width: number; height: number};

// Host bridge ids for Tray instances. The native host owns the OS tray icon; this shim keeps the
// Electron-facing state and forwards mutations through `tray.*` ops. Native events come back
// through `__rendrix_host_event('tray.event', { id, event, itemId })`.
let nextTrayId = 1;
const trays = new Map<number, Tray>();

function safeHost(op: string, args: any = {}): Promise<any> {
  try { return Promise.resolve(host(op, args)).catch(() => null); } catch { return Promise.resolve(null); }
}

function encodePng(bytes: Uint8Array): string {
  return toBase64(Array.from(bytes));
}

function isPromise(value: any): boolean {
  return !!value && typeof value.then === 'function';
}

// Accepts an Electron Tray image: a nativeImage, a PNG/Uint8Array, a data URL or a path.
function serializeImage(image: any): any {
  if (!image) return null;
  if (typeof image === 'string') {
    if (image.startsWith('data:')) {
      const comma = image.indexOf(',');
      if (comma >= 0) return {base64: image.slice(comma + 1)};
      return null;
    }
    return image ? {path: image} : null;
  }
  if (typeof ArrayBuffer !== 'undefined' && image instanceof ArrayBuffer) return image.byteLength ? {base64: toBase64(Array.from(new Uint8Array(image)))} : null;
  if (typeof ArrayBuffer !== 'undefined' && ArrayBuffer.isView(image)) {
    const bytes = new Uint8Array(image.buffer, image.byteOffset ?? 0, image.byteLength);
    return bytes.length ? {base64: toBase64(Array.from(bytes))} : null;
  }
  if (typeof image.toPNG === 'function') {
    try {
      const png = image.toPNG();
      if (typeof ArrayBuffer !== 'undefined' && ArrayBuffer.isView(png) && png.byteLength) return {base64: toBase64(Array.from(new Uint8Array(png.buffer, png.byteOffset, png.byteLength)))};
    } catch {}
  }
  if (typeof image.toDataURL === 'function') {
    try {
      const url = image.toDataURL();
      const comma = String(url).indexOf(',');
      if (comma >= 0) return {base64: String(url).slice(comma + 1)};
    } catch {}
  }
  return null;
}

type MenuBinding = { item: any; click?: Function; siblings?: any[] };

function menuItems(menu: any): any[] | null {
  if (Array.isArray(menu)) return menu;
  if (menu && Array.isArray(menu.items)) return menu.items;
  return null;
}

// Serializes a Menu (or a menu template) into the platform-neutral model the host renders. Each
// item gets a stable command id for this snapshot, and its click handler stays here.
function serializeMenu(menu: any, bindings: Map<number, MenuBinding>): any[] | null {
  const items = menuItems(menu);
  if (!items) return null;
  const serialized: any[] = [];
  for (const item of items) {
    if (!item) continue;
    const hasSubmenu = Array.isArray(item.submenu) || (item.submenu && Array.isArray(item.submenu.items));
    const type = hasSubmenu ? 'submenu' : (item.type || 'normal');
    // Reserve the command id before serializing a submenu: nested items must get later ids, never
    // the same id as their parent.
    const itemId = bindings.size + 1;
    const entry: any = {
      type,
      label: type === 'separator' ? '' : String(item.label ?? ''),
      enabled: item.enabled !== false && item.disabled !== true,
      visible: item.visible !== false && item.hidden !== true,
      checked: item.checked === true,
      itemId,
    };
    bindings.set(itemId, { item, click: typeof item.click === 'function' ? item.click : undefined, siblings: items });
    if (hasSubmenu) entry.submenu = serializeMenu(item.submenu, bindings) ?? [];
    serialized.push(entry);
  }
  return serialized;
}

function keyboardEvent() {
  return { shiftKey: false, ctrlKey: false, altKey: false, metaKey: false, preventDefault() {}, stopPropagation() {} };
}

function handleTrayEvent(payload: any): void {
  const tray = trays.get(Number(payload?.id));
  if (!tray || tray.destroyed) return;
  switch (payload?.event) {
    case 'click':
      tray.emit('click', keyboardEvent(), tray.getBounds(), tray.position());
      break;
    case 'right-click':
      tray.emit('right-click', keyboardEvent(), tray.getBounds(), tray.position());
      break;
    case 'double-click':
      if (!tray.ignoreDoubleClickEvents) tray.emit('double-click', keyboardEvent(), tray.getBounds(), tray.position());
      break;
    case 'menu-click':
      tray.handleMenuClick(Number(payload?.itemId));
      break;
  }
}

export class Tray extends EventEmitter {
  id: number;
  image: any;
  pressedImage: any = null;
  private _toolTip = '';
  private _title = '';
  private _contextMenu: any = null;
  private _menuBindings = new Map<number, MenuBinding>();
  bounds: Rectangle = {x: 0, y: 0, width: 0, height: 0};
  destroyed = false;
  ignoreDoubleClickEvents = false;
  constructor(image?: any) {
    super();
    const pending = isPromise(image);
    this.image = pending ? null : (image ?? null);
    this.id = nextTrayId++;
    trays.set(this.id, this);
    safeHost('tray.create', {id: this.id, image: serializeImage(this.image)});
    if (pending) {
      void Promise.resolve(image).then((resolved: any) => { if (!this.destroyed) this.setImage(resolved); }).catch(() => {});
    }
    this.refreshBounds();
  }
  private refreshBounds() {
    void safeHost('tray.bounds', {id: this.id}).then((result: any) => {
      if (this.destroyed || !result) return;
      const number = (value: any) => Number.isFinite(Number(value)) ? Number(value) : 0;
      this.bounds = {x: number(result.x), y: number(result.y), width: number(result.width), height: number(result.height)};
    });
  }
  position() {
    return {x: this.bounds.x + Math.round(this.bounds.width / 2), y: this.bounds.y + Math.round(this.bounds.height / 2)};
  }
  setImage(image: any) {
    if (isPromise(image)) {
      void Promise.resolve(image).then((resolved: any) => { if (!this.destroyed) this.setImage(resolved); }).catch(() => {});
      return;
    }
    this.image = image ?? null;
    safeHost('tray.setIcon', {id: this.id, image: serializeImage(this.image)});
  }
  setPressedImage(image: any) {
    if (isPromise(image)) {
      void Promise.resolve(image).then((resolved: any) => { if (!this.destroyed) this.setPressedImage(resolved); }).catch(() => {});
      return;
    }
    this.pressedImage = image ?? null;
    safeHost('tray.setIcon', {id: this.id, image: serializeImage(this.pressedImage), pressed: true});
  }
  setToolTip(toolTip: string) { this._toolTip = String(toolTip ?? ''); safeHost('tray.setToolTip', {id: this.id, toolTip: this._toolTip}); }
  getToolTip() { return this._toolTip; }
  setTitle(title: string) { this._title = String(title ?? ''); safeHost('tray.setTitle', {id: this.id, title: this._title}); }
  getTitle() { return this._title; }
  setContextMenu(menu: any) {
    this._contextMenu = menu ?? null;
    this._menuBindings = new Map();
    const serialized = this._contextMenu ? serializeMenu(this._contextMenu, this._menuBindings) : null;
    safeHost('tray.setContextMenu', {id: this.id, menu: serialized});
  }
  getContextMenu() { return this._contextMenu; }
  popUpContextMenu(menu?: any, _position?: any) {
    if (menu) {
      this.setContextMenu(menu);
    }
    safeHost('tray.popUpContextMenu', {id: this.id});
  }
  closeContextMenu() { /* the OS closes its own popup */ }
  getBounds() { return {x: this.bounds.x, y: this.bounds.y, width: this.bounds.width, height: this.bounds.height}; }
  isDestroyed() { return this.destroyed; }
  destroy() {
    if (this.destroyed) return;
    this.destroyed = true;
    trays.delete(this.id);
    this._menuBindings.clear();
    safeHost('tray.destroy', {id: this.id});
    this.emit('destroyed');
  }
  focus() { /* macOS-only in Electron; not exposed by the host */ }
  displayBalloon(_options?: any) { /* Windows balloons are not implemented */ }
  removeBalloon() { /* no-op */ }
  getIgnoreDoubleClickEvents() { return this.ignoreDoubleClickEvents; }
  setIgnoreDoubleClickEvents(flag: boolean) { this.ignoreDoubleClickEvents = !!flag; }
  /** @internal Called by the host event dispatcher. */
  handleMenuClick(commandId: number) {
    const binding = this._menuBindings.get(commandId);
    if (!binding || this.destroyed) return;
    const item = binding.item;
    if (item.type === 'checkbox') {
      item.checked = !item.checked;
      this.setContextMenu(this._contextMenu);
    } else if (item.type === 'radio') {
      for (const sibling of binding.siblings ?? []) {
        if (sibling !== item && sibling.type === 'radio') sibling.checked = false;
      }
      item.checked = true;
      this.setContextMenu(this._contextMenu);
    }
    if (binding.click) {
      try { binding.click(item, null, keyboardEvent()); } catch (error) { try { console.error('Tray menu click failed:', error); } catch {} }
    }
  }
}

// Global shortcuts are process-global. The shim keeps an in-memory registry for synchronous
// queries and forwards registration to the host, which performs the OS-level hotkey binding.
const globalShortcuts = new Map<string, Function>();
function validAccelerator(accelerator: any): accelerator is string {
  return typeof accelerator === 'string' && accelerator.trim().length > 0;
}

export const globalShortcut = {
  register(accelerator: string, callback: Function): boolean {
    if (!validAccelerator(accelerator)) throw new TypeError('globalShortcut.register requires an accelerator string');
    if (typeof callback !== 'function') throw new TypeError('globalShortcut.register requires a callback');
    // Electron returns false when the accelerator is already taken by this app.
    if (globalShortcuts.has(accelerator)) return false;
    globalShortcuts.set(accelerator, callback);
    const pending = safeHost('globalShortcut.register', {accelerator});
    // A host that cannot even reach the platform layer answers null; drop the optimistic entry so
    // isRegistered() does not lie. OS refusals arrive later as globalShortcut.failed.
    void pending.then((result: any) => {
      if (result === null && globalShortcuts.get(accelerator) === callback) {
        globalShortcuts.delete(accelerator);
        compatWarnings.push(`globalShortcut.register('${accelerator}') could not reach the host`);
      }
    });
    return true;
  },
  registerAll(accelerators: string[], callback: Function) {
    if (!Array.isArray(accelerators)) throw new TypeError('globalShortcut.registerAll requires an array');
    for (const accelerator of accelerators) this.register(String(accelerator), callback);
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

onHostEvent((channel: string, payload: any) => {
  if (channel === 'tray.event') {
    handleTrayEvent(payload);
  } else if (channel === 'tray.error') {
    compatWarnings.push(`Tray ${payload?.id ?? ''} failed: ${payload?.error ?? 'unknown error'}`);
  } else if (channel === 'globalShortcut.event') {
    const callback = globalShortcuts.get(String(payload?.accelerator ?? ''));
    if (callback) {
      try { callback(); } catch (error) { try { console.error('Global shortcut callback failed:', error); } catch {} }
    }
  } else if (channel === 'globalShortcut.failed') {
    const accelerator = String(payload?.accelerator ?? '');
    if (globalShortcuts.delete(accelerator)) {
      compatWarnings.push(`globalShortcut.register('${accelerator}') failed: ${payload?.error ?? 'the OS refused the hotkey'}`);
    }
  }
});
