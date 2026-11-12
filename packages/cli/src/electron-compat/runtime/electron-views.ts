import { compatWarnings } from './host';
import { EventEmitter } from './events';

// Partial shims for Electron's view/window APIs: BaseWindow, BrowserView and View.
//
// Multi-view layout (composing child views inside a window, tabbed windows, view
// hierarchies) is NOT supported by the TS-GPU host. These shims keep inspectable
// state (bounds, visibility, title, destroyed flags) and emit the events Electron
// exposes, so apps can import and reference the APIs without crashing at load
// time. addChildView/removeChildView throw a clear error because native view
// chrome is unavailable. Basic construction never throws.

function unsupported(api: string): never {
  throw new Error(`${api} is not supported by TS-GPU`);
}

function numberOr(value: any, fallback: number): number {
  return typeof value === 'number' && Number.isFinite(value) ? value : fallback;
}

let nextId = 1;

function makeWebContents(id: number, isDestroyed: () => boolean): any {
  const emitter = new EventEmitter();
  const wc: any = Object.assign(emitter, {
    id,
    isDestroyed,
    getURL: () => '',
    getTitle: () => '',
    isLoading: () => false,
    send(channel: string, ..._args: any[]) {
      compatWarnings.push(`webContents.send(${channel}) has no page in this view; the message was dropped`);
    },
    loadURL(url: string): Promise<void> {
      return Promise.reject(new Error(`webContents.loadURL is not supported for this view: ${url}`));
    },
    loadFile(file: string): Promise<void> {
      return Promise.reject(new Error(`webContents.loadFile is not supported for this view: ${file}`));
    },
    reload() { compatWarnings.push('webContents.reload is not supported for this view'); },
    goBack() { compatWarnings.push('webContents.goBack is not supported for this view'); },
    goForward() { compatWarnings.push('webContents.goForward is not supported for this view'); },
    canGoBack() { return false; },
    canGoForward() { return false; },
    openDevTools() { compatWarnings.push('webContents.openDevTools is not supported for this view'); },
    closeDevTools() { compatWarnings.push('webContents.closeDevTools is not supported for this view'); },
    isDevToolsOpened() { return false; },
    executeJavaScript(code: string): Promise<any> {
      try { return Promise.resolve((0, (globalThis as any).eval)(code)); } catch (error) { return Promise.reject(error); }
    },
    on: emitter.on.bind(emitter),
    once: emitter.once.bind(emitter),
    off: emitter.off.bind(emitter),
    removeListener: emitter.removeListener.bind(emitter),
    addListener: emitter.on.bind(emitter),
  });
  return wc;
}

function toBounds(options: any): { x: number; y: number; width: number; height: number } {
  const src = options?.bounds && typeof options.bounds === 'object' ? options.bounds : (options || {});
  const x = numberOr(src.x, 0);
  const y = numberOr(src.y, 0);
  let width = src.width === undefined ? (options?.width === undefined ? 800 : numberOr(options?.width, 800)) : numberOr(src.width, 0);
  let height = src.height === undefined ? (options?.height === undefined ? 600 : numberOr(options?.height, 600)) : numberOr(src.height, 0);
  if (width === 0 && options?.width === undefined && src.width === undefined) width = 800;
  if (height === 0 && options?.height === undefined && src.height === undefined) height = 600;
  width = Math.max(0, width);
  height = Math.max(0, height);
  return { x, y, width, height };
}

// Keep a shared base for bounds/visibility/title/destroyed state.
export class View extends EventEmitter {
  id: number;
  options: any;
  private _bounds: { x: number; y: number; width: number; height: number };
  private _backgroundColor: any = null;
  private _borderRadius = 0;
  private _visible = true;
  private _title = '';
  private _destroyed = false;

  constructor(options: any = {}) {
    super();
    this.id = nextId++;
    this.options = options || {};
    this._bounds = toBounds(this.options);
    if (typeof this.options.visible === 'boolean') this._visible = this.options.visible;
    if (typeof this.options.show === 'boolean') this._visible = this.options.show;
    if (this.options.title != null) this._title = String(this.options.title);
    if (this.options.backgroundColor != null) this._backgroundColor = this.options.backgroundColor;
  }

  get destroyed(): boolean { return this._destroyed; }

  addListener(name: string | symbol, fn: Function): this { return this.on(name, fn); }
  removeListener(name: string | symbol, fn: Function): this { return this.off(name, fn); }

  getBounds(): { x: number; y: number; width: number; height: number } {
    return { ...this._bounds };
  }
  setBounds(bounds: any): void {
    if (!bounds || typeof bounds !== 'object') throw new TypeError('View.setBounds requires a bounds object');
    const next = {
      x: typeof bounds.x === 'number' ? bounds.x : this._bounds.x,
      y: typeof bounds.y === 'number' ? bounds.y : this._bounds.y,
      width: typeof bounds.width === 'number' ? Math.max(0, bounds.width) : this._bounds.width,
      height: typeof bounds.height === 'number' ? Math.max(0, bounds.height) : this._bounds.height,
    };
    this._bounds = next;
    this.emit('bounds-changed', { ...next });
  }
  getPosition(): [number, number] { return [this._bounds.x, this._bounds.y]; }
  setPosition(x: number, y: number): void {
    this.setBounds({ x: numberOr(x, this._bounds.x), y: numberOr(y, this._bounds.y), width: this._bounds.width, height: this._bounds.height });
  }
  getSize(): { width: number; height: number } { return { width: this._bounds.width, height: this._bounds.height }; }
  setSize(width: number, height: number): void {
    this.setBounds({ x: this._bounds.x, y: this._bounds.y, width: numberOr(width, this._bounds.width), height: numberOr(height, this._bounds.height) });
  }

  getTitle(): string { return this._title; }
  setTitle(title: any): void { this._title = title == null ? '' : String(title); this.emit('title-changed', this._title); }

  getBackgroundColor(): any { return this._backgroundColor; }
  setBackgroundColor(color: any): void { this._backgroundColor = color == null ? null : color; }

  getBorderRadius(): number { return this._borderRadius; }
  setBorderRadius(radius: number): void { this._borderRadius = Number(radius) || 0; }

  isVisible(): boolean { return this._visible; }
  setVisible(visible: boolean): void {
    const next = !!visible;
    if (next === this._visible) return;
    this._visible = next;
    this.emit(next ? 'show' : 'hide');
  }
  show(): void { this.setVisible(true); }
  hide(): void { this.setVisible(false); }

  // View hierarchy is not supported (multi-view layout is unsupported).
  get children(): any[] { return []; }
  addChildView(_view: any, _index?: number): never { return unsupported('View.addChildView'); }
  removeChildView(_view: any): never { return unsupported('View.removeChildView'); }

  isDestroyed(): boolean { return this._destroyed; }
  destroy(): void {
    if (this._destroyed) return;
    this._destroyed = true;
    this.emit('destroyed');
  }
  close(): void { this.destroy(); }
}

export class BrowserView extends View {
  webContents: any;
  autoResize: { width: boolean; height: boolean };

  constructor(options: any = {}) {
    super(options);
    this.autoResize = options?.autoResize || { width: false, height: false };
    if (typeof this.autoResize.width !== 'boolean') this.autoResize.width = !!this.autoResize.width;
    if (typeof this.autoResize.height !== 'boolean') this.autoResize.height = !!this.autoResize.height;
    this.webContents = makeWebContents(this.id, () => this.isDestroyed());
  }

  setAutoResize(options: any): void {
    this.autoResize = { width: !!options?.width, height: !!options?.height };
  }

  destroy(): void {
    if (this.isDestroyed()) return;
    super.destroy();
  }
}

const baseWindows = new Set<BaseWindow>();

export class BaseWindow extends View {
  options: any;

  constructor(options: any = {}) {
    super(options);
    this.options = options || {};
    baseWindows.add(this);
  }

  // Multi-view layout is not supported; the content view cannot be provided.
  get contentView(): never { return unsupported('BaseWindow.contentView'); }
  getContentView(): never { return unsupported('BaseWindow.getContentView'); }
  setContentView(_view: any): never { return unsupported('BaseWindow.setContentView'); }

  getParentWindow(): BaseWindow | null { return null; }
  getChildWindows(): BaseWindow[] { return []; }

  // Window-level convenience, mirroring BrowserWindow where cheap.
  isFocused(): boolean { return false; }
  focus(): void { if (this.isDestroyed()) return; this.emit('focus'); }
  blur(): void { if (this.isDestroyed()) return; this.emit('blur'); }

  isMinimized(): boolean { return false; }
  isMaximized(): boolean { return false; }
  isFullScreen(): boolean { return false; }
  isFullScreenable(): boolean { return true; }
  setFullScreenable(_value: boolean): void { /* tracked locally only */ }
  setFullScreen(_flag: boolean): void {
    compatWarnings.push('BaseWindow.setFullScreen is not supported and has no effect');
  }

  minimize(): void { if (this.isDestroyed()) return; this.emit('minimize'); }
  maximize(): void { if (this.isDestroyed()) return; this.emit('maximize'); }
  unmaximize(): void { if (this.isDestroyed()) return; this.emit('unmaximize'); }
  restore(): void { if (this.isDestroyed()) return; this.emit('restore'); }

  center(): void { /* no-op without native chrome */ }
  flashFrame(_flag: boolean): void { /* no-op */ }

  close(): void {
    if (this.isDestroyed()) return;
    let prevented = false;
    const event: any = { preventDefault() { prevented = true; } };
    this.emit('close', event);
    if (prevented) return;
    this.destroy();
  }

  destroy(): void {
    if (this.isDestroyed()) return;
    baseWindows.delete(this);
    super.destroy();
    this.emit('closed');
  }

  static getAllWindows(): BaseWindow[] {
    return [...baseWindows].filter(w => !w.isDestroyed());
  }
  static getFocusedWindow(): BaseWindow | null {
    return BaseWindow.getAllWindows()[0] || null;
  }
  static fromId(id: number): BaseWindow | null {
    return [...baseWindows].find(w => w.id === id && !w.isDestroyed()) || null;
  }
}

export default { View, BrowserView, BaseWindow };