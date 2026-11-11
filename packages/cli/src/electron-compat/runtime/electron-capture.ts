import { EventEmitter } from './events';
import fs from './fs';
import path from './path';
import { info, infoReady } from './os';

// Partial shim for Electron's crashReporter. Native crash dumps need Crashpad/minidumps, which
// the embedded engines do not expose. The shim records the crash parameters and writes crash
// metadata under `<userData>/crashes`, where a host-side crash handler or a packaging step can
// collect it. Nothing is uploaded.
const crashExtraParameters = new Map<string, string>();
let crashParameters: Record<string, string> = {};
let crashUploadToServer = false;
let crashReports: Array<{ date: number; id: string }> = [];
let nextCrashId = 1;

function snapshotParameters(): Record<string, string> {
  const out: Record<string, string> = { ...crashParameters };
  for (const [key, value] of crashExtraParameters) out[key] = value;
  return out;
}

function crashesDirectory(): string {
  const root = info.paths?.userData || info.paths?.appFolder || '/';
  return path.join(root, 'crashes');
}

function crashPayload(event: string, extra: Record<string, any> = {}): Record<string, any> {
  return {
    event,
    productName: crashParameters.productName || info.name || '',
    version: crashParameters.version || info.version || '',
    platform: info.platform || '',
    uploaded: false,
    parameters: snapshotParameters(),
    ...extra,
  };
}

export class CrashReporter {
  private ready: Promise<void> = Promise.resolve();

  start(options: any = {}) {
    const opts = options && typeof options === 'object' && !Array.isArray(options) ? options : {};
    crashParameters = {};
    crashExtraParameters.clear();
    crashReports = [];
    nextCrashId = 1;
    for (const key of Object.keys(opts)) {
      const value = (opts as any)[key];
      if (value === undefined || value === null) continue;
      if ((key === 'extra' || key === 'globalExtra') && typeof value === 'object' && !Array.isArray(value)) {
        for (const [extraKey, extraValue] of Object.entries(value)) {
          if (extraValue !== undefined && extraValue !== null) crashExtraParameters.set(String(extraKey), String(extraValue));
        }
      } else {
        crashParameters[String(key)] = String(value);
      }
    }
    crashUploadToServer = false;
    // Create `<userData>/crashes` and persist the reporter settings. Failures are recorded in the
    // console, not thrown, because Electron's start() is synchronous.
    this.ready = infoReady.then(async () => {
      const directory = crashesDirectory();
      try { await fs.promises.mkdir(directory); } catch { /* already exists or no write root */ }
      try {
        await fs.promises.writeFile(path.join(directory, 'crash-reporter.json'),
          JSON.stringify(crashPayload('started', {startedAt: Date.now()}), null, 2));
      } catch (error: any) {
        try { console.error('crashReporter: cannot write the crashes folder:', error?.message || error); } catch {}
      }
    }).catch(() => {});
    try { console.error('crashReporter started (local only, no upload)', JSON.stringify(snapshotParameters())); } catch { /* ignore */ }
    return undefined;
  }

  /**
   * TSGPUI addition: records a crash and writes a JSON dump under `<userData>/crashes`. Call this
   * from a global error handler; the host does not intercept native crashes.
   */
  captureCrash(reason: any = 'unknown', extra: Record<string, any> = {}): { date: number; id: string } {
    const date = Date.now();
    const id = `${date}-${nextCrashId++}`;
    const report = {date, id};
    crashReports.push(report);
    void this.ready.then(async () => {
      const directory = crashesDirectory();
      try { await fs.promises.mkdir(directory); } catch { /* already exists */ }
      await fs.promises.writeFile(path.join(directory, `crash-${id}.json`),
        JSON.stringify(crashPayload('crash', {date, id, reason: String(reason), ...extra}), null, 2));
    }).catch((error: any) => {
      try { console.error('crashReporter: cannot write a crash dump:', error?.message || error); } catch {}
    });
    return report;
  }

  getLastCrashReport() {
    return crashReports.length ? crashReports[crashReports.length - 1] : null;
  }
  getParameters() {
    return snapshotParameters();
  }
  getUploadedReports() {
    return [];
  }
  getUploadToServer() {
    return false;
  }
  setUploadToServer(_uploadToServer?: boolean) {
    crashUploadToServer = false;
  }
  addExtraParameter(key: string, value: string) {
    crashExtraParameters.set(String(key), String(value));
  }
  removeExtraParameter(key: string) {
    crashExtraParameters.delete(String(key));
  }
}

export const crashReporter = new CrashReporter();

// Partial shim for Electron's desktopCapturer: capture is unavailable, so no sources are reported.
function emptySourcesResult() {
  const arr: any = [];
  arr.then = (resolve?: Function, reject?: Function) => Promise.resolve().then(() => {
    try {
      if (typeof resolve === 'function') return resolve(arr);
      return arr;
    } catch (error) {
      if (typeof reject === 'function') reject(error);
      throw error;
    }
  });
  arr.catch = (reject?: Function) => Promise.resolve().then(() => arr).catch(reject as any);
  arr.finally = (fn?: Function) => Promise.resolve().then(() => arr).finally(fn as any);
  return arr;
}

export class DesktopCapturer {
  getSources(options?: any, callback?: Function) {
    const cb = typeof options === 'function' ? options : (typeof callback === 'function' ? callback : undefined);
    const opts = (options && typeof options === 'object' && !Array.isArray(options)) ? options : {};
    // Accept and normalize the documented options shape without producing sources.
    const types = Array.isArray((opts as any).types) ? (opts as any).types : [];
    const thumbnailSize = (opts as any).thumbnailSize && typeof (opts as any).thumbnailSize === 'object'
      ? (opts as any).thumbnailSize
      : undefined;
    const fetchWindowIcons = (opts as any).fetchWindowIcons === true;
    void types; void thumbnailSize; void fetchWindowIcons;
    const result = emptySourcesResult();
    if (typeof cb === 'function') {
      try { cb(result); } catch { /* ignore */ }
    }
    return result;
  }
}

export const desktopCapturer = new DesktopCapturer();

// Partial shim for Electron's inAppPurchase: store purchases are unavailable.
export class InAppPurchase extends EventEmitter {
  canMakePayments() {
    return false;
  }
  getProducts(productIdentifiers: any, callback?: Function) {
    const cb = typeof productIdentifiers === 'function' ? productIdentifiers : callback;
    if (typeof cb === 'function') {
      try { cb([]); } catch { /* ignore */ }
    }
    return Promise.resolve([]);
  }
  purchaseProduct(_productID: string, _quantity?: any, _callback?: Function) {
    if (typeof _callback === 'function') {
      try { _callback(false); } catch { /* ignore */ }
    }
    return false;
  }
  finishTransactionByDate(_date: string) {
    // no-op
  }
  finishAllTransactions() {
    // no-op
  }
  restoreCompletedTransactions() {
    // no-op
  }
  getReceiptURL() {
    return '';
  }
}

export const inAppPurchase = new InAppPurchase();

export default { crashReporter, desktopCapturer, inAppPurchase, CrashReporter, DesktopCapturer, InAppPurchase };