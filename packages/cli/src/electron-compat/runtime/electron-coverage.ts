// Generated from packages/cli/src/electron-compat/coverage.mjs. Do not edit by hand.

const suffix = " (see TSGPUI_IMPORT_REPORT.md for alternatives)";

function unsupportedModule(name: string, reason: string): any {
  const fail = (what: string): never => { throw new Error(`electron.${name}${what} is not supported by TSGPUI: ${reason}${suffix}`); };
  const target = function () {};
  return new Proxy(target, {
    get(_target, property) { if (property === "then" || typeof property === "symbol") return undefined; return fail(`.${String(property)}`); },
    set(_target, property) { return fail(`.${String(property)}`); },
    apply() { return fail(""); },
    construct() { return fail(""); },
  });
}

export const autoUpdater = unsupportedModule("autoUpdater", "TSGPUI updates are shipped by its release process, not by the app");
export const BaseWindow = unsupportedModule("BaseWindow", "the separate Electron BaseWindow API is not implemented");
export const BrowserView = unsupportedModule("BrowserView", "multiple views per window are not supported");
export const contentTracing = unsupportedModule("contentTracing", "tracing is not available in the embedded engines");
export const crashReporter = unsupportedModule("crashReporter", "no crash upload; start a crash reporter in your own code");
export const desktopCapturer = unsupportedModule("desktopCapturer", "screen and window capture is not available");
export const inAppPurchase = unsupportedModule("inAppPurchase", "store purchases are not available");
export const netLog = unsupportedModule("netLog", "network logging is not available");
export const powerMonitor = unsupportedModule("powerMonitor", "power events are not available");
export const pushNotifications = unsupportedModule("pushNotifications", "push notifications are not available");
export const ShareMenu = unsupportedModule("ShareMenu", "the system share menu is not available");
export const systemPreferences = unsupportedModule("systemPreferences", "system preference queries are not available");
export const TouchBar = unsupportedModule("TouchBar", "the Touch Bar is not available");
export const utilityProcess = unsupportedModule("utilityProcess", "utility processes are not supported; run work in the backend");
export const View = unsupportedModule("View", "view layout is not supported");
export const WebContentsView = unsupportedModule("WebContentsView", "multiple views per window are not supported");
export const webFrameMain = unsupportedModule("webFrameMain", "no access to frame internals");
export const webUtils = unsupportedModule("webUtils", "file path lookup for dropped files is not available; use data-ondrop");
