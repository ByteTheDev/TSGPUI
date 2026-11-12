import { host, compatWarnings } from './host';
import { EventEmitter } from './events';
import { View } from './electron-views';
import fs from './fs';
import path from './path';
import { info } from './os';

// Partial shims for Electron APIs that TS-GPU cannot fully back with native chrome.
//
// WebContentsView / webFrameMain / contentTracing are exposed so apps can import and
// reference them without crashing at load time, but the underlying multi-view layout,
// frame internals and tracing facilities are not available. Top-level, commonly used
// operations (construction, bounds tracking, executeJavaScript, tracing no-ops) run;
// anything that needs native view chrome or frame internals throws a clear error.

function unsupported(api: string): never {
  throw new Error(`${api} is not supported by TS-GPU`);
}

function numberOr(value: any, fallback: number): number {
  return typeof value === 'number' && Number.isFinite(value) ? value : fallback;
}

let nextViewId = 1;

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
    addListener: emitter.on.bind(emitter),
  });
  return wc;
}

export class WebContentsView extends View {
  webContents: any;

  constructor(options?: any) {
    super(options);
    this.webContents = makeWebContents(this.id, () => this.isDestroyed());
  }

  // View hierarchy is not supported (multiple views per window are not supported).
  addChildView(_view: any, _index?: number): never { return unsupported('WebContentsView.addChildView'); }
  removeChildView(_view: any): never { return unsupported('WebContentsView.removeChildView'); }
  addChildViews(_views: any[]): never { return unsupported('WebContentsView.addChildViews'); }
  removeChildViews(): never { return unsupported('WebContentsView.removeChildViews'); }
  getChildrenViews(): any[] { return []; }

  isDestroyed(): boolean { return super.isDestroyed(); }
  destroy(): void {
    if (this.isDestroyed()) return;
    super.destroy();
  }
}

// webFrameMain: frame internals are unavailable. Construction and lookup are exposed
// as no-ops; the main frame can still deliver messages to the page.
const frames = new Map<string, WebFrameMain>();

function deliverToPage(channel: string, payload: any): void {
  const g: any = globalThis;
  try {
    if (typeof g.__tsgpui_emit === 'function' && g.__tsgpui_window_id !== 0) {
      g.__tsgpui_emit(channel, payload);
      return;
    }
    if (g.window && typeof g.window.__tsgpui_receive === 'function') {
      g.window.__tsgpui_receive(channel, payload);
      return;
    }
    void host('window.send', {channel, payload}).catch((error: any) => {
      compatWarnings.push(`webFrameMain could not deliver '${channel}': ${error?.message || error}`);
    });
  } catch (error: any) {
    compatWarnings.push(`webFrameMain could not deliver '${channel}': ${error?.message || error}`);
  }
}

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
    const key = `${Number(processId) || 0}:${Number(routingId) || 1}`;
    const frame = frames.get(key);
    if (!frame || frame.isDestroyed()) {
      if (frame) frames.delete(key);
      return null;
    }
    return frame;
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
  get ipc(): any {
    const self = this;
    return {
      send(channel: string, ...args: any[]) { self.send(channel, ...args); },
      postMessage(channel: string, message: any, transfer?: any[]) { self.postMessage(channel, message, transfer); },
    };
  }

  isDestroyed() { return this._destroyed; }
  destroy() { if (this._destroyed) return; this._destroyed = true; frames.delete(`${this.processId}:${this.routingId}`); this.emit('destroyed'); }

  executeJavaScript(code: string, _userGesture?: boolean): Promise<any> {
    try { return Promise.resolve((0, (globalThis as any).eval)(code)); } catch (error) { return Promise.reject(error); }
  }
  executeJavaScriptInIsolatedWorld(_worldId: number, _scripts: any[], _userGesture?: boolean): Promise<any> {
    return Promise.reject(new Error('WebFrameMain.executeJavaScriptInIsolatedWorld is not supported by TS-GPU'));
  }
  collectJavaScriptCallStack(): never { return unsupported('WebFrameMain.collectJavaScriptCallStack'); }
  reload() { compatWarnings.push('WebFrameMain.reload is not supported and has no effect'); }
  send(channel: string, ...args: any[]) {
    if (this._destroyed) throw new Error('WebFrameMain has been destroyed');
    const payload = args.length <= 1 ? args[0] : args;
    deliverToPage(String(channel), payload);
  }
  postMessage(channel: string, message: any, transfer?: any[]) {
    if (this._destroyed) throw new Error('WebFrameMain has been destroyed');
    if (Array.isArray(transfer) && transfer.length) {
      compatWarnings.push('WebFrameMain.postMessage copies transferables; ports and buffers are not transferred');
    }
    deliverToPage(String(channel), message);
  }
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

// contentTracing: Chromium's tracing internals are not available. The shim records a Chrome
// tracing-event JSON timeline of the recording window, which tools such as Perfetto or
// chrome://tracing can open. It contains TS-GPU markers, not renderer/GPU thread events.
const DEFAULT_CATEGORIES = ['*', 'blink', 'cc', 'gpu', 'netlog', 'renderer', 'toplevel', 'tracing', 'ui', 'v8', 'viz', 'views', 'webview'];
let tracingRecording = false;
let tracingStartedAt = 0;
let tracingCategories: string[] = [];

function traceFilePath(requested?: string): string {
  if (typeof requested === 'string' && requested) {
    if (path.isAbsolute(requested)) return requested;
    return path.join(info.paths?.userData || info.paths?.appFolder || '/', requested);
  }
  return path.join(info.paths?.userData || info.paths?.appFolder || '/', `trace-${Date.now()}.json`);
}

export const contentTracing: any = {
  getCategories(): Promise<string[]> {
    return Promise.resolve([...new Set([...DEFAULT_CATEGORIES, ...tracingCategories])]);
  },
  getTraceBufferUsage(): Promise<{ value: number; percentageUsed: number }> {
    return Promise.resolve({ value: tracingRecording ? 1 : 0, percentageUsed: tracingRecording ? 1 : 0 });
  },
  startRecording(options?: any): Promise<void> {
    if (tracingRecording) return Promise.reject(new Error('contentTracing.startRecording is already recording'));
    const opts = options && typeof options === 'object' ? options : {};
    tracingCategories = Array.isArray(opts.categories) ? opts.categories.map(String) : [];
    tracingStartedAt = Date.now();
    tracingRecording = true;
    compatWarnings.push('contentTracing records TS-GPU timing markers, not Chromium renderer or GPU events');
    return Promise.resolve();
  },
  async stopRecording(resultFilePath?: string): Promise<string> {
    if (!tracingRecording) return '';
    tracingRecording = false;
    const stoppedAt = Date.now();
    const duration = Math.max(0, stoppedAt - tracingStartedAt);
    const target = traceFilePath(resultFilePath);
    const events: any[] = [
      {name: 'process_name', cat: '__metadata', ph: 'M', pid: 1, tid: 1, ts: 0, args: {name: info.name || 'TS-GPU app'}},
      {
        name: 'TS-GPU content trace', cat: 'tsgpu', ph: 'X', pid: 1, tid: 1,
        ts: tracingStartedAt * 1000, dur: duration * 1000,
        args: {categories: tracingCategories, platform: info.platform || '', version: info.version || ''},
      },
    ];
    const payload = {
      traceEvents: events,
      displayTimeUnit: 'ms',
      metadata: {source: 'tsgpu', categories: tracingCategories, startedAt: new Date(tracingStartedAt).toISOString(), stoppedAt: new Date(stoppedAt).toISOString()},
    };
    try {
      await fs.promises.writeFile(target, JSON.stringify(payload));
      return target;
    } catch (error: any) {
      compatWarnings.push(`contentTracing.stopRecording could not write ${target}: ${error?.message || error}`);
      return '';
    }
  },
};

export default { WebContentsView, webFrameMain, contentTracing, WebFrameMain };