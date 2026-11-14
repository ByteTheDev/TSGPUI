export const compatWarnings: string[] = [];

// Native events (tray clicks, global shortcut presses, failed registrations) arrive through one
// global that both hosts call: `__rendrix_host_event(channel, payload)`. Shards subscribe here so a
// later host feature can add its own channel without another runtime hook.
const hostEventHandlers: ((channel: string, payload: any) => void)[] = [];
export function onHostEvent(handler: (channel: string, payload: any) => void): void {
  hostEventHandlers.push(handler);
}
const hostGlobal: any = globalThis;
hostGlobal.__rendrix_host_event = (channel: string, payload: any) => {
  for (const handler of [...hostEventHandlers]) {
    try { handler(String(channel), payload); } catch (error) { try { console.error('Host event failed:', error); } catch {} }
  }
};

// Operations are named as the WebView bridge names them, and their arguments are objects. The native
// renderer exposes `api.native` instead (see NATIVE_SETUP in native/src/runtime.rs), which takes
// positional arguments and returns plain values. Each route below translates one operation to that
// API and reshapes the result to what the shims read. The native host checks the same grants.
type Route = (api: any, args: any) => Promise<any>;
const base64Alphabet = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/';
export function toBase64(bytes: number[]): string {
  let out = '';
  for (let i = 0; i < bytes.length; i += 3) {
    const n = (bytes[i] << 16) | ((bytes[i + 1] || 0) << 8) | (bytes[i + 2] || 0);
    out += base64Alphabet[n >> 18] + base64Alphabet[(n >> 12) & 63] + (i + 1 < bytes.length ? base64Alphabet[(n >> 6) & 63] : '=') + (i + 2 < bytes.length ? base64Alphabet[n & 63] : '=');
  }
  return out;
}
export function fromBase64(text: string): number[] {
  const bytes: number[] = [];
  let value = 0, bits = 0;
  for (const c of text) {
    if (c === '=') break;
    const n = base64Alphabet.indexOf(c);
    if (n < 0) continue;
    value = (value << 6) | n; bits += 6;
    if (bits >= 8) { bits -= 8; bytes.push((value >> bits) & 255); }
  }
  return bytes;
}
const location = (args: any) => ({root: args.root, path: args.path ?? ''});
const nativeRoutes: Record<string, Route> = {
  'app.info': api => api.app.info(),
  'app.quit': api => api.app.quit(),
  'clipboard.readText': api => api.clipboard.readText().then((text: string) => ({text})),
  'clipboard.writeText': (api, args) => api.clipboard.writeText(args.text),
  'dialog.openFile': (api, args) => api.dialog.openFile(args),
  'dialog.saveFile': (api, args) => api.dialog.saveFile(args),
  'dialog.message': (api, args) => api.dialog.message(args),
  'shell.openExternal': (api, args) => api.shell.openExternal(args.url),
  // The native API returns the same plain values the WebView bridge does, so these need no reshaping.
  'safeStorage.isAvailable': api => api.safeStorage.isAvailable(),
  'safeStorage.encrypt': (api, args) => api.safeStorage.encrypt(args.value),
  'safeStorage.decrypt': (api, args) => api.safeStorage.decrypt(args.value),
  'net.request': (api, args) => api.net.request(args),
  'power.setBlockers': (api, args) => api.power.setBlockers(args.types),
  'power.state': api => api.power.state(),
  'systemPreferences.info': api => api.systemPreferences.info(),
  // Tray objects and global hotkeys live in the native host. The tray options carry the image as
  // base64 so the host decodes it once; menu item ids are resolved by the shim on click.
  'tray.create': (api, args) => api.tray.create(args),
  'tray.setIcon': (api, args) => api.tray.setIcon(args),
  'tray.setToolTip': (api, args) => api.tray.setToolTip(args),
  'tray.setTitle': (api, args) => api.tray.setTitle(args),
  'tray.setContextMenu': (api, args) => api.tray.setContextMenu(args),
  'tray.popUpContextMenu': (api, args) => api.tray.popUpContextMenu(args),
  'tray.destroy': (api, args) => api.tray.destroy(args),
  'tray.bounds': (api, args) => api.tray.bounds(args),
  'globalShortcut.register': (api, args) => api.globalShortcut.register(args.accelerator),
  'globalShortcut.unregister': (api, args) => api.globalShortcut.unregister(args.accelerator),
  'globalShortcut.unregisterAll': (api, args) => api.globalShortcut.unregisterAll(),
  'globalShortcut.isRegistered': (api, args) => api.globalShortcut.isRegistered(args.accelerator),
  'screen.displays': api => api.screen.displays().then((displays: any[]) => ({displays})),
  'notifications.show': (api, args) => api.notifications.show(args),
  'filesystem.readText': (api, args) => api.filesystem.readText(location(args)).then((text: string) => ({text})),
  'filesystem.readBytes': (api, args) => api.filesystem.readBytes(location(args)).then((bytes: number[]) => ({base64: toBase64(bytes)})),
  'filesystem.writeText': (api, args) => api.filesystem.writeText(location(args), args.text),
  'filesystem.writeBytes': (api, args) => api.filesystem.writeBytes(location(args), fromBase64(args.base64)),
  'filesystem.list': (api, args) => api.filesystem.list(location(args)),
  'filesystem.stat': (api, args) => api.filesystem.stat(location(args)),
  'filesystem.mkdir': (api, args) => api.filesystem.mkdir(location(args)),
  'filesystem.remove': (api, args) => api.filesystem.remove(location(args)),
  'filesystem.rename': (api, args) => api.filesystem.rename({root: args.root, path: args.from}, {root: args.root, path: args.to}),
  // NativeImage handles live in the native host; these routes forward the option object unchanged
  // and return its `{ handle, width, height }` and `{ base64 }`/`{ dataUrl }` results.
  'nativeImage.createFromPath': (api, args) => api.nativeImage.createFromPath(args),
  'nativeImage.createFromBuffer': (api, args) => api.nativeImage.createFromBuffer(args),
  'nativeImage.getSize': (api, args) => api.nativeImage.getSize(args),
  'nativeImage.getAspectRatio': (api, args) => api.nativeImage.getAspectRatio(args),
  'nativeImage.resize': (api, args) => api.nativeImage.resize(args),
  'nativeImage.crop': (api, args) => api.nativeImage.crop(args),
  'nativeImage.toPNG': (api, args) => api.nativeImage.toPNG(args),
  'nativeImage.toJPEG': (api, args) => api.nativeImage.toJPEG(args),
  'nativeImage.toDataURL': (api, args) => api.nativeImage.toDataURL(args),
  'nativeImage.toBitmap': (api, args) => api.nativeImage.toBitmap(args),
  'process.spawn': (api, args) => api.process.spawn(args.command, args.args ?? []).then((result: any) => ({status: result.code, stdout: result.stdout, stderr: result.stderr})),
  'ipc.send': (api, args) => api.ipc.send(args.channel, args.payload),
  'window.minimize': api => api.window.minimize(),
  'window.maximize': api => api.window.maximize(),
  'window.restore': api => api.window.restore(),
  'window.close': api => api.window.close(),
  'window.setTitle': (api, args) => api.window.setTitle(args.title),
  // The native renderer has no page to receive messages and reports no close requests. Electron drops
  // a message when no page is loaded, so this does the same and records a warning instead of failing.
  'window.send': () => { compatWarnings.push('webContents.send has no page in the native renderer; the message was dropped'); return Promise.resolve(null); },
  'window.watchClose': () => { compatWarnings.push('BrowserWindow close listeners cannot cancel a close in the native renderer'); return Promise.resolve(null); },
  // The native renderer has a single GPUI window: show/hide, fullscreen, focus, bounds and geometry are
  // tracked locally in BrowserWindow. These routes record a warning instead of failing, so the shim's
  // local state (isVisible, bounds, focus) stays consistent without moving native chrome.
  'window.show': () => { compatWarnings.push('BrowserWindow show has no native chrome effect in the native renderer; visibility is tracked locally'); return Promise.resolve(null); },
  'window.hide': () => { compatWarnings.push('BrowserWindow hide has no native chrome effect in the native renderer; visibility is tracked locally'); return Promise.resolve(null); },
  'window.setFullScreen': () => { compatWarnings.push('BrowserWindow setFullScreen has no native chrome effect in the native renderer; fullscreen state is tracked locally'); return Promise.resolve(null); },
  'window.focus': () => { compatWarnings.push('BrowserWindow focus has no native chrome effect in the native renderer; focus is tracked locally'); return Promise.resolve(null); },
  'window.blur': () => { compatWarnings.push('BrowserWindow blur has no native chrome effect in the native renderer; focus is tracked locally'); return Promise.resolve(null); },
  'window.setBounds': () => { compatWarnings.push('BrowserWindow setBounds has no native chrome effect in the native renderer; bounds are tracked locally'); return Promise.resolve(null); },
  'window.setPosition': () => { compatWarnings.push('BrowserWindow setPosition has no native chrome effect in the native renderer; bounds are tracked locally'); return Promise.resolve(null); },
  'window.setSize': () => { compatWarnings.push('BrowserWindow setSize has no native chrome effect in the native renderer; bounds are tracked locally'); return Promise.resolve(null); },
  // Host-side queries are tracked locally in the shim; the native renderer has no chrome to read, so
  // these resolve to the last locally tracked values when the native API lacks them.
  'window.getBounds': (api: any) => (typeof api?.window?.getBounds === 'function' ? api.window.getBounds() : (compatWarnings.push('BrowserWindow getBounds has no native chrome source in the native renderer; returning locally tracked bounds is not available over the host'), Promise.resolve(null))),
  'window.isMaximized': (api: any) => (typeof api?.window?.isMaximized === 'function' ? api.window.isMaximized() : (compatWarnings.push('BrowserWindow isMaximized has no native chrome source in the native renderer; state is tracked locally'), Promise.resolve(null))),
  'window.isMinimized': (api: any) => (typeof api?.window?.isMinimized === 'function' ? api.window.isMinimized() : (compatWarnings.push('BrowserWindow isMinimized has no native chrome source in the native renderer; state is tracked locally'), Promise.resolve(null))),
  'window.isVisible': (api: any) => (typeof api?.window?.isVisible === 'function' ? api.window.isVisible() : (compatWarnings.push('BrowserWindow isVisible has no native chrome source in the native renderer; state is tracked locally'), Promise.resolve(null))),
  'window.isFullScreen': (api: any) => (typeof api?.window?.isFullScreen === 'function' ? api.window.isFullScreen() : (compatWarnings.push('BrowserWindow isFullScreen has no native chrome source in the native renderer; state is tracked locally'), Promise.resolve(null))),
  'window.isFocused': (api: any) => (typeof api?.window?.isFocused === 'function' ? api.window.isFocused() : (compatWarnings.push('BrowserWindow isFocused has no native chrome source in the native renderer; state is tracked locally'), Promise.resolve(null))),
};
function nativeHost(op: string, args: any): Promise<any> {
  const route = nativeRoutes[op];
  if (!route) return Promise.reject(new Error(`${op} is not available in this host. Pages, extra windows and page messages are not supported by the Rendrix host yet; run on the deprecated legacy host or restructure the app.`));
  try { return Promise.resolve(route((globalThis as any).__rendrix_native, args ?? {})); } catch (error) { return Promise.reject(error); }
}
export function host(op: string, args: any = {}): Promise<any> {
  const g: any = globalThis;
  const fn = g.__rendrix_host || g.window?.rendrix?.invoke;
  if (!fn && g.__rendrix_native) return nativeHost(op, args);
  if (!fn) return Promise.reject(new Error(`Host operation unavailable: ${op}`));
  try { return Promise.resolve(fn(op, args)); } catch (error) { return Promise.reject(error); }
}
const handlers = new Map<string, Function>();
const listeners = new Map<string, Function[]>();
export function handle(channel: string, fn: Function) { if (handlers.has(channel)) throw new Error(`Handler already registered: ${channel}`); handlers.set(channel, fn); }
export function removeHandler(channel: string) { handlers.delete(channel); }
export function listen(channel: string, fn: Function) { listeners.set(channel, [...(listeners.get(channel) || []), fn]); }
export function unlisten(channel: string, fn: Function) { listeners.set(channel, (listeners.get(channel) || []).filter(f => f !== fn && (f as any).listener !== fn)); }
const g: any = globalThis;
g.__rendrix_ipc_dispatch = async (kind: string, channel: string, payload: any, sourceId?: number) => {
  // In a hidden backend window (id 0) every page is a renderer, so replies always go through the host to the sender.
  const backendWindow = g.__rendrix_window_id === 0;
  const event = { sender: { send: (c: string, p: any) => sourceId !== undefined && (backendWindow || (sourceId && sourceId !== 1)) ? host('window.send', {windowId: sourceId, channel: c, payload: p}) : g.__rendrix_emit ? g.__rendrix_emit(c, p) : g.window?.__rendrix_receive?.(c, p) } };
  if (kind === 'invoke') { const fn = handlers.get(channel); if (!fn) throw new Error(`No handler registered for ${channel}`); return fn(event, payload); }
  for (const fn of [...(listeners.get(channel) || [])]) fn(event, payload);
};

