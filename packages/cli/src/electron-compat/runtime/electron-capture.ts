import { EventEmitter } from './events';

// Partial shim for Electron's crashReporter: records crash parameters locally and never uploads.
const crashExtraParameters = new Map<string, string>();
let crashParameters: Record<string, string> = {};
let crashUploadToServer = false;
const crashReports: Array<{ date: number; id: string }> = [];

function snapshotParameters(): Record<string, string> {
  const out: Record<string, string> = { ...crashParameters };
  for (const [key, value] of crashExtraParameters) out[key] = value;
  return out;
}

export class CrashReporter {
  start(options: any = {}) {
    const opts = options && typeof options === 'object' && !Array.isArray(options) ? options : {};
    crashParameters = {};
    crashExtraParameters.clear();
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
    // Local crash logging only: never upload.
    crashUploadToServer = false;
    try { console.error('crashReporter started (local only, no upload)', snapshotParameters()); } catch { /* ignore */ }
    return undefined;
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