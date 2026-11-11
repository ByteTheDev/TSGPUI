import { host } from './host';
import { EventEmitter } from './events';

// Display geometry is read from the host. Electron's screen API is synchronous, so the shim keeps
// the last reported displays and throws until the first read has completed.
type Point = {x: number; y: number};
type Rectangle = {x: number; y: number; width: number; height: number};
type Display = {id: number; bounds: Rectangle; workArea: Rectangle; size: {width: number; height: number}; workAreaSize: {width: number; height: number}; scaleFactor: number; rotation: number; internal: boolean; touchSupport: string; colorDepth: number};
let displays: Display[] = [];
let primaryDisplay: Display | null = null;
let displaysError: Error | null = null;
let displaysLoaded = false;
let cursorPoint: {x: number; y: number} | null = null;
let cursorLoaded = false;
let cursorError: Error | null = null;

function toDisplay(raw: any, index: number): Display {
  const bounds = {x: Number(raw.x), y: Number(raw.y), width: Number(raw.width), height: Number(raw.height)};
  const size = {width: bounds.width, height: bounds.height};
  return {id: index + 1, bounds, workArea: {...bounds}, size, workAreaSize: {...size}, scaleFactor: Number(raw.scaleFactor) || 1, rotation: 0, internal: false, touchSupport: 'unknown', colorDepth: 24};
}

function displayKey(d: Display) { return `${d.bounds.x},${d.bounds.y},${d.bounds.width},${d.bounds.height}`; }
async function refreshCursor() {
  const ops = ['screen.cursor', 'screen.getCursorScreenPoint', 'screen.cursorScreenPoint'];
  for (const op of ops) {
    try {
      const result: any = await host(op);
      const point = result && typeof result === 'object' && result.point && typeof result.point.x === 'number'
        ? result.point
        : (result && typeof result.x === 'number' && typeof result.y === 'number' ? {x: result.x, y: result.y} : null);
      if (point) { cursorPoint = {x: Number(point.x), y: Number(point.y)}; cursorError = null; cursorLoaded = true; return; }
    } catch (error) {
      cursorError = error instanceof Error ? error : new Error(String(error));
    }
  }
  cursorLoaded = true;
}
async function refreshDisplays() {
  const previous = displaysLoaded ? [...displays] : [];
  try {
    const result = await host('screen.displays');
    const list = Array.isArray(result?.displays) ? result.displays : [];
    const next = list.map(toDisplay);
    // Emit display change events based on the previous snapshot.
    if (displaysLoaded) {
      const prevKeys = new Map(previous.map(d => [displayKey(d), d]));
      const nextKeys = new Map(next.map(d => [displayKey(d), d]));
      for (const [key, d] of nextKeys) {
        if (!prevKeys.has(key)) screen.emit('display-added', {}, d);
        else {
          const old = prevKeys.get(key)!;
          if (old.scaleFactor !== d.scaleFactor || old.bounds.width !== d.bounds.width || old.bounds.height !== d.bounds.height) {
            screen.emit('display-metrics-changed', {}, d, {bounds: true, workArea: true, scaleFactor: old.scaleFactor !== d.scaleFactor, rotation: false});
          }
        }
      }
      for (const [key, d] of prevKeys) {
        if (!nextKeys.has(key)) screen.emit('display-removed', {}, d);
      }
    }
    displays = next;
    const primary = list.findIndex((raw: any) => raw.primary === true);
    primaryDisplay = displays[primary >= 0 ? primary : 0] ?? null;
    displaysError = null;
  } catch (error) {
    displaysError = error instanceof Error ? error : new Error(String(error));
  }
  displaysLoaded = true;
  await refreshCursor().catch(() => { cursorLoaded = true; });
}
const loading = refreshDisplays();

function requireDisplays(): Display[] {
  if (!displaysLoaded) throw new Error('screen information is still loading; await screen.refresh() first');
  if (displaysError) throw displaysError;
  if (!displays.length) throw new Error('the host reported no displays');
  return displays;
}

function intersectionArea(a: Rectangle, b: Rectangle) {
  const width = Math.min(a.x + a.width, b.x + b.width) - Math.max(a.x, b.x);
  const height = Math.min(a.y + a.height, b.y + b.height) - Math.max(a.y, b.y);
  return width > 0 && height > 0 ? width * height : 0;
}

export const screen = Object.assign(new EventEmitter(), {
  /** TSGPUI addition: re-read the displays from the host and wait for the result. */
  async refresh() { await refreshDisplays(); if (displaysError) throw displaysError; },
  getPrimaryDisplay(): Display { requireDisplays(); return primaryDisplay as Display; },
  getAllDisplays(): Display[] { return [...requireDisplays()]; },
  getDisplayNearestPoint(point: Point): Display {
    const list = requireDisplays();
    let best = list[0], bestDistance = Infinity;
    for (const display of list) {
      const b = display.bounds;
      const dx = Math.max(b.x - point.x, 0, point.x - (b.x + b.width));
      const dy = Math.max(b.y - point.y, 0, point.y - (b.y + b.height));
      const distance = dx * dx + dy * dy;
      if (distance < bestDistance) { best = display; bestDistance = distance; }
    }
    return best;
  },
  getDisplayMatching(rect: Rectangle): Display {
    const list = requireDisplays();
    let best = list[0], bestArea = -1;
    for (const display of list) { const area = intersectionArea(display.bounds, rect); if (area > bestArea) { best = display; bestArea = area; } }
    return best;
  },
  getCursorScreenPoint(): Point {
    if (!cursorLoaded) throw new Error('screen information is still loading; await screen.refresh() first');
    if (cursorError && !cursorPoint) throw cursorError;
    return cursorPoint ? {...cursorPoint} : {x: 0, y: 0};
  },
  async refreshCursor() { await refreshCursor(); if (cursorError) throw cursorError; },
});

// Ports joined in the same process. Messages are structured-cloned, delivered asynchronously, and never transferred.
export class MessagePortMain extends EventEmitter {
  private peer: MessagePortMain | null = null;
  private started = false;
  private closed = false;
  private transferred = false;
  private queue: Array<{ data: any; ports: any[] }> = [];
  /** Starts delivery. Adding a 'message' listener also starts the port, as in Electron. */
  on(name: string | symbol, fn: Function): this {
    super.on(name, fn);
    if (name === 'message') this.start();
    return this;
  }
  start() { this.started = true; this.flush(); }
  close() {
    if (this.closed) return;
    this.closed = true; this.queue = [];
    this.emit('close');
    const peer = this.peer; this.peer = null;
    if (peer && !peer.closed) peer.close();
  }
  postMessage(message: any, transfer?: any[]) {
    if (this.transferred) throw new Error('MessagePortMain has been transferred');
    if (this.closed || !this.peer) return;
    const ports: any[] = [];
    const transferables: any[] = [];
    for (const item of transfer || []) {
      if (item instanceof MessagePortMain) {
        ports.push(item);
      } else if (item instanceof ArrayBuffer) {
        transferables.push(item);
      } else if (ArrayBuffer.isView(item)) {
        transferables.push((item as any).buffer);
      } else {
        throw new Error('MessagePortMain.postMessage does not transfer this value');
      }
    }
    let data: any;
    try {
      data = typeof structuredClone === 'function'
        ? structuredClone(message, transferables.length ? { transfer: transferables } : undefined)
        : message;
    } catch {
      data = message;
    }
    this.peer.deliver(data, ports);
  }
  /** @internal */ deliver(data: any, ports: any[] = []) { if (this.closed) return; this.queue.push({ data, ports }); this.flush(); }
  /** @internal */ link(peer: MessagePortMain) { this.peer = peer; }
  private flush() {
    if (!this.started) return;
    while (this.queue.length) {
      const entry = this.queue.shift() as any;
      const ports = entry.ports || [];
      Promise.resolve().then(() => this.emit('message', { data: entry.data, ports }));
    }
  }
}

export class MessageChannelMain {
  port1 = new MessagePortMain();
  port2 = new MessagePortMain();
  constructor() { this.port1.link(this.port2); this.port2.link(this.port1); }
}

// Main-frame page access for renderer preloads. Zoom is applied to the document with CSS zoom.
let pageZoom = 1;
let insertedCount = 0;
const insertedStyles = new Map<string, any>();
const webFrameFrames: any[] = [];
const pageDocument = () => (globalThis as any).document;
export const webFrame: any = {
  setZoomFactor(factor: number) {
    if (!(factor > 0)) throw new RangeError('zoom factor must be positive');
    pageZoom = factor;
    const root = pageDocument()?.documentElement;
    if (root) root.style.setProperty('zoom', String(factor));
  },
  getZoomFactor() { return pageZoom; },
  setZoomLevel(level: number) { webFrame.setZoomFactor(Math.pow(1.2, level)); },
  getZoomLevel() { return Math.log(pageZoom) / Math.log(1.2); },
  insertCSS(css: string): Promise<string> {
    const doc = pageDocument();
    if (!doc) return Promise.reject(new Error('webFrame.insertCSS requires a page'));
    const key = 'tsgpui-css-' + (++insertedCount);
    const style = doc.createElement('style');
    style.textContent = css;
    (doc.head || doc.documentElement).appendChild(style);
    insertedStyles.set(key, style);
    return Promise.resolve(key);
  },
  removeInsertedCSS(key: string): Promise<void> {
    insertedStyles.get(key)?.remove();
    insertedStyles.delete(key);
    return Promise.resolve();
  },
  executeJavaScript(code: string): Promise<any> {
    try { return Promise.resolve((0, (globalThis as any).eval)(code)); } catch (error) { return Promise.reject(error); }
  },
  // Frame tree basics.
  name: '',
  routingId: 1,
  get parent() { return null; },
  get opener() { return null; },
  get top() { return webFrame; },
  get firstChild() { return null; },
  get nextSibling() { return null; },
  get frames() { return webFrameFrames; },
  get url() {
    try { return (globalThis as any).location?.href || ''; } catch { return ''; }
  },
  findFrameByName(_name: string) { return null; },
  findFrameByRoutingId(routingId: number) { return routingId === webFrame.routingId ? webFrame : null; },
};

// Keeps power blockers alive while the app runs. The host applies the combined state with SetThreadExecutionState (Windows).
const blockerTypes = ['prevent-app-suspension', 'prevent-display-sleep'];
const activeBlockers = new Map<number, string>();
let nextBlockerId = 1;
function applyBlockers() {
  const types = [...new Set(activeBlockers.values())];
  host('power.setBlockers', {types}).catch((error: any) => console.error('powerSaveBlocker:', error?.message || error));
}
export const powerSaveBlocker = {
  start(type: string): number {
    if (!blockerTypes.includes(type)) throw new TypeError(`Invalid power save blocker type: ${type}`);
    const id = nextBlockerId++;
    activeBlockers.set(id, type);
    applyBlockers();
    return id;
  },
  stop(id: number) {
    if (!activeBlockers.delete(id)) return;
    applyBlockers();
  },
  isStarted(id: number): boolean { return activeBlockers.has(id); },
};

// Tray icon shim. Native tray chrome is not available, so this keeps inspectable state and emits
// the events Electron exposes without blocking on the host.
export class Tray extends EventEmitter {
  image: any;
  private _toolTip = '';
  private _title = '';
  private _contextMenu: any = null;
  destroyed = false;
  constructor(image?: any) { super(); this.image = image ?? null; }
  setImage(image: any) { this.image = image ?? null; }
  setToolTip(toolTip: string) { this._toolTip = String(toolTip ?? ''); }
  getToolTip() { return this._toolTip; }
  setTitle(title: string) { this._title = String(title ?? ''); }
  getTitle() { return this._title; }
  setContextMenu(menu: any) { this._contextMenu = menu ?? null; }
  getContextMenu() { return this._contextMenu; }
  popUpContextMenu(menu?: any, position?: any) { /* no native popup */ }
  getBounds() { return {x: 0, y: 0, width: 0, height: 0}; }
  isDestroyed() { return this.destroyed; }
  destroy() {
    if (this.destroyed) return;
    this.destroyed = true;
    this.emit('destroyed');
  }
}

// Global shortcuts are process-global in Electron. The shim keeps an in-memory registry so apps can
// register, query and unregister accelerators; delivery of global key events is not available here.
const globalShortcuts = new Map<string, Function>();
export const globalShortcut = {
  register(accelerator: string, callback: Function): boolean {
    if (typeof accelerator !== 'string' || !accelerator) throw new TypeError('globalShortcut.register requires an accelerator string');
    if (typeof callback !== 'function') throw new TypeError('globalShortcut.register requires a callback');
    globalShortcuts.set(accelerator, callback);
    return true;
  },
  registerAll(accelerators: string[], callback: Function) {
    if (!Array.isArray(accelerators)) throw new TypeError('globalShortcut.registerAll requires an array');
    for (const accelerator of accelerators) this.register(String(accelerator), callback);
  },
  unregister(accelerator: string) { globalShortcuts.delete(String(accelerator)); },
  unregisterAll() { globalShortcuts.clear(); },
  isRegistered(accelerator: string): boolean { return globalShortcuts.has(String(accelerator)); },
};

