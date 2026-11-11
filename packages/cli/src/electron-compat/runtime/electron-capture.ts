import { EventEmitter } from './events';

// Partial shim for Electron's crashReporter: records crash parameters locally and never uploads.
const crashExtraParameters = new Map<string, string>();
const crashParameters: Record<string, string> = {};
let crashUploadToServer = false;
const crashReports: Array<{ date: number; id: string }> = [];

export const crashReporter = {
  start(options: any = {}) {
    const opts = options && typeof options === 'object' ? options : {};
    for (const key of Object.keys(opts)) {
      const value = (opts as any)[key];
      if (value !== undefined && value !== null) crashParameters[String(key)] = String(value);
    }
    // Local crash logging only: never upload.
    crashUploadToServer = false;
    try { console.error('crashReporter started (local only, no upload)'); } catch { /* ignore */ }
  },
  getLastCrashReport() {
    return crashReports.length ? crashReports[crashReports.length - 1] : null;
  },
  getParameters() {
    const out: Record<string, string> = { ...crashParameters };
    for (const [key, value] of crashExtraParameters) out[key] = value;
    return out;
  },
  getUploadedReports() {
    return [];
  },
  getUploadToServer() {
    return false;
  },
  setUploadToServer(_uploadToServer?: boolean) {
    // Uploads are disabled in this shim; keep the setting local but never enable it.
    crashUploadToServer = false;
  },
  addExtraParameter(key: string, value: string) {
    crashExtraParameters.set(String(key), String(value));
  },
  removeExtraParameter(key: string) {
    crashExtraParameters.delete(String(key));
  },
};

// Partial shim for Electron's desktopCapturer: capture is unavailable, so no sources are reported.
export const desktopCapturer = {
  getSources(_options?: any) {
    return [];
  },
};

// Partial shim for Electron's inAppPurchase: store purchases are unavailable.
class InAppPurchase extends EventEmitter {
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

export default { crashReporter, desktopCapturer, inAppPurchase };