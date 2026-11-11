import { host, compatWarnings } from './host';
import { EventEmitter } from './events';

// Partial shims for Electron APIs that TSGPUI cannot fully back with native chrome.
//
// WebContentsView / webFrameMain / contentTracing are exposed so apps can import and
// reference them without crashing at load time, but the underlying multi-view layout,
// frame internals and tracing facilities are not available. Top-level, commonly used
// operations (construction, bounds tracking, executeJavaScript, tracing no-ops) run;
// anything that needs native view chrome or frame internals throws a clear error.

function unsupported(api: string): never {
  throw new Error(`${api} is not supported by TSGPUI`);
}

let nextViewId = 1;

function makeWebContents(id: number): any {
  const emitter = new EventEmitter();
  const wc: any = Object.assign(emitter, {
    id,
    isDestroyed: () => false,
    getURL: () => '',
    getTitle: () => '',
    isLoading: () => false,
    send(channel: string, ..._args: any[]) {
      compatWarnings.push(`webContents.send(${channel}) has no page in this view; the message was dropped`);
    },
    loadURL(url: string): Promise<void> {
      return Promise.reject(new Error(`webContents.loadURL is not supported for WebContentsView: ${url}`));
    },
    loadFile(file: string): Promise<void> {
      return Promise.reject(new Error(`webContents.loadFile is not supported for WebContentsView: ${file}`));
    },
    reload() { compatWarnings.push('webContents.reload is not supported for WebContentsView'); },
    goBack() { compatWarnings.push('webContents.goBack is not supported for WebContentsView'); },
    goForward() { compatWarnings.push('webContents.goForward is not supported for WebContentsView'); },
    canGoBack() { return false; },
    canGoForward() { return false; },
    openDevTools() { compatWarnings.push('webContents.openDevTools is not supported for WebContentsView'); },
    closeDevTools() { compatWarnings.push('webContents.closeDevTools is not supported for WebContentsView'); },
    isDevToolsOpened() { return false; },
    executeJavaScript(code: string): Promise<any> {
      try { return Promise.resolve((0, (globalThis as any).eval)(code)); } catch (error) { return Promise.reject(error); }
    },
    on: emitter.on.bind(emitter),
    once: emitter.once.bind(emitter),
    off: emitter.off.bind(emitter),
    removeListener: emitter.removeListener.bind(emitter),
  });
  return wc;
}

export class WebContentsView extends EventEmitter {
  id: number;
  webContents: any;
  private _bounds = { x: 0, y: 0, width: 0, height: 0 };
  private _backgroundColor: string | null = null;
  options: any;

  constructor(options?: any) {
    super();
    this.id = nextViewId++;
    this.options = options || {};
    this.webContents = makeWebContents(this.id);
  }

  // View layout is not backed by native chrome; bounds are tracked locally.
  getBounds() { return { ...this._bounds }; }
  setBounds(bounds: any) {
    if (!bounds || typeof bounds !== 'object') throw new TypeError('WebContentsView.setBounds requires a bounds object');
    const next = {
      x: typeof bounds.x === 'number' ? bounds.x : this._bounds.x,
      y: typeof bounds.y === 'number' ? bounds.y : this._bounds.y,
      width: typeof bounds.width === 'number' ? Math.max(0, bounds.width) : this._bounds.width,
      height: typeof bounds.height === 'number' ? Math.max(0, bounds.height) : this._bounds.height,
    };
    this._bounds = next;
    this.emit('bounds-changed', next);
  }
  setBackgroundColor(color: any) { this._backgroundColor = color == null ? null : String(color); }
  getBackgroundColor() { return this._backgroundColor; }

  // View hierarchy is not supported (multiple views per window are not supported).
  addChildView(_view: any, _index?: number): never { return unsupported('WebContentsView.addChildView'); }
  removeChildView(_view: any): never { return unsupported('WebContentsView.removeChildView'); }
  addChildViews(_views: any[]): never { return unsupported('WebContentsView.addChildViews'); }
  removeChildViews(): never { return unsupported('WebContentsView.removeChildViews'); }
  getChildrenViews(): any[] { return []; }

  isDestroyed() { return false; }
  destroy() { this.emit('destroyed'); }
}

// webFrameMain: frame internals are unavailable. Construction and lookup are exposed
// as no-ops; executeJavaScript falls back to the page when possible.
const frames = new Map<string, WebFrameMain>();

export class WebFrameMain extends EventEmitter {
  processId: number;
  routingId: number;
  private _destroyed = false;

  constructor(processId = 0, routingId = 1) {
    super();
    this.processId = Number(processId) || 0;
    this.routingId = Number(routingId) || 1;
    frames.set(`${this.processId}:${this.routingId}`, this);
  }

  static fromId(processId: number, routingId: number): WebFrameMain | null {
    return frames.get(`${Number(processId) || 0}:${Number(routingId) || 1}`) ?? null;
  }

  get url(): string {
    try { return (globalThis as any).location?.href || ''; } catch { return ''; }
  }
  get origin(): string {
    try { return (globalThis as any).location?.origin || ''; } catch { return ''; }
  }
  get name(): string { return ''; }
  get parent(): WebFrameMain | null { return null; }
  get top(): WebFrameMain { return this; }
  get frames(): WebFrameMain[] { return []; }
  get framesInSubtree(): WebFrameMain[] { return []; }
  get detached(): boolean { return false; }
  get visibilityState(): string { return 'visible'; }
  get osProcessId(): number { return this.processId; }
  get frameTreeNodeId(): number { return this.routingId; }

  isDestroyed() { return this._destroyed; }
  destroy() { this._destroyed = true; frames.delete(`${this.processId}:${this.routingId}`); this.emit('destroyed'); }

  executeJavaScript(code: string): Promise<any> {
    try { return Promise.resolve((0, (globalThis as any).eval)(code)); } catch (error) { return Promise.reject(error); }
  }
  executeJavaScriptInIsolatedWorld(_worldId: number, _scripts: any[]): Promise<any> {
    return Promise.reject(new Error('WebFrameMain.executeJavaScriptInIsolatedWorld is not supported by TSGPUI'));
  }
  collectJavaScriptCallStack(): never { return unsupported('WebFrameMain.collectJavaScriptCallStack'); }
  reload() { compatWarnings.push('WebFrameMain.reload is not supported and has no effect'); }
  send(_channel: string, ..._args: any[]) { compatWarnings.push('WebFrameMain.send is not supported and has no effect'); }
  postMessage(_channel: string, _message: any, _transfer?: any[]) { compatWarnings.push('WebFrameMain.postMessage is not supported and has no effect'); }
}

// webFrameMain module-level object (Electron exports this as an object, not a class).
export const webFrameMain: any = Object.assign(
  function webFrameMainFactory(processId?: number, routingId?: number) {
    return new WebFrameMain(processId, routingId);
  },
  {
    fromId(processId: number, routingId: number) { return WebFrameMain.fromId(processId, routingId); },
  },
);

// contentTracing: tracing is unavailable; expose no-op/empty implementations.
export const contentTracing: any = {
  getCategories(): Promise<string[]> {
    compatWarnings.push('contentTracing.getCategories is not supported; returning an empty list');
    return Promise.resolve([]);
  },
  getTraceBufferUsage(): Promise<{ value: number; percentageUsed: number }> {
    compatWarnings.push('contentTracing.getTraceBufferUsage is not supported');
    return Promise.resolve({ value: 0, percentageUsed: 0 });
  },
  startRecording(_options?: any): Promise<void> {
    compatWarnings.push('contentTracing.startRecording is not supported and has no effect');
    return Promise.resolve();
  },
  stopRecording(_resultFilePath?: string): Promise<string> {
    compatWarnings.push('contentTracing.stopRecording is not supported');
    return Promise.resolve('');
  },
};

export default { WebContentsView, webFrameMain, contentTracing, WebFrameMain };