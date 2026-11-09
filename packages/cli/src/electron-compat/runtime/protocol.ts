import { compatWarnings } from './host';

// Custom protocol shim for Electron -> TSGPUI transfer.
//
// Electron's `protocol.handle` / `registerFileProtocol` serve a custom scheme
// (commonly `app://` or `file://`) from the packaged app files. TSGPUI already
// packages renderer pages and assets and serves them from the WebView host via
// the `tsgpui://localhost/` URL space (see `window.load` in electron.ts and
// the host's `with_custom_protocol("tsgpui", ...)` handler). This module
// rewrites in-page custom-scheme URLs onto that space so
// `BrowserWindow.loadURL('app://...')` and fetch-style resolution keep
// working without a separate scheme registration on the host.
//
// Remote schemes (`http:`, `https:`, `ws:`, `wss:`) are not proxied through
// this shim: HTTPS goes through the permission-checked host transport
// (`net.request` / `permissions.network`), and plain HTTP proxying used to be
// provided by Electron's Chromium network stack, which TSGPUI does not have.
// Attempting to handle a remote scheme throws a descriptive error.

const SCHEME_RE = /^[a-z][a-z0-9+.-]*$/i;
const REMOTE_SCHEMES = new Set(['http', 'https', 'ws', 'wss']);

const handlers = new Map<string, Function>();
const intercepted = new Set<string>();
const privileged = new Set<string>();

function normalizeScheme(input: any): string {
  if (typeof input !== 'string' || !input.trim()) throw new TypeError('protocol: scheme must be a non-empty string');
  const scheme = input.trim().toLowerCase();
  if (!SCHEME_RE.test(scheme)) throw new TypeError(`protocol: invalid scheme '${input}'`);
  return scheme;
}

function errorIfRemote(scheme: string, api: string): void {
  if (REMOTE_SCHEMES.has(scheme)) {
    throw new Error(
      `${api} for remote scheme '${scheme}:' is not supported; `
      + 'remote URLs need permissions.network (HTTPS host transport) instead of a protocol handler. '
      + 'See docs/SUPPORTED_SYNTAX.md (Custom protocols).',
    );
  }
}

function register(schemeInput: any, handler: any, api: string): string {
  const scheme = normalizeScheme(schemeInput);
  if (typeof handler !== 'function') throw new TypeError(`${api} for '${scheme}:' requires a handler function`);
  errorIfRemote(scheme, api);
  if (handlers.has(scheme)) throw new Error(`Handler already registered for scheme '${scheme}:'`);
  handlers.set(scheme, handler);
  return scheme;
}

function unregister(schemeInput: any): void {
  const scheme = normalizeScheme(schemeInput);
  handlers.delete(scheme);
  intercepted.delete(scheme);
}

function isHandled(schemeInput: any): boolean {
  try {
    return handlers.has(normalizeScheme(schemeInput));
  } catch {
    return false;
  }
}

function isRegistered(schemeInput: any): boolean {
  try {
    const scheme = normalizeScheme(schemeInput);
    return handlers.has(scheme) || privileged.has(scheme);
  } catch {
    return false;
  }
}

function isIntercepted(schemeInput: any): boolean {
  try {
    return intercepted.has(normalizeScheme(schemeInput));
  } catch {
    return false;
  }
}

function pathFromCustomUrl(urlText: string): string | null {
  let parsed: URL;
  try {
    parsed = new URL(urlText);
  } catch {
    return null;
  }
  const scheme = parsed.protocol.slice(0, -1).toLowerCase();
  if (scheme === 'tsgpui') {
    // Already in the host's own space; hand back the packaged path.
    const path = decodedPath(parsed);
    return path.startsWith('/') ? path : '/' + path;
  }
  if (!handlers.has(scheme)) return null;
  if (REMOTE_SCHEMES.has(scheme)) return null;
  const path = decodedPath(parsed);
  return path.startsWith('/') ? path : '/' + path;
}

function decodedPath(parsed: URL): string {
  // `app://localhost/index.html` -> `/index.html`; `app:///dist/index.html` -> `/dist/index.html`.
  // A non-localhost host is dropped: custom schemes in Electron transfer apps
  // name the packaged file by path, not by host. Relative-ish `app://./x`
  // arrives with host `.`; the URL parser keeps it out of the pathname, so the
  // pathname alone is the packaged path in every common form.
  const raw = parsed.pathname || '/';
  try {
    return decodeURIComponent(raw);
  } catch {
    return raw;
  }
}

/** Rewrite a custom-scheme URL to the host's `tsgpui://` URL, or return null. */
export function rewriteCustomUrl(urlText: string): string | null {
  if (typeof urlText !== 'string') return null;
  // The host's own URLs pass through unchanged.
  if (urlText.startsWith('tsgpui://localhost/')) return urlText;
  const path = pathFromCustomUrl(urlText);
  if (path === null) return null;
  // Reject paths that would escape the packaged asset map.
  if (path.startsWith('//') || path.split('/').includes('..') || path.includes('\\')) return null;
  return `tsgpui://localhost${path}`;
}

/** Dispatch a request for a registered scheme through its handler (async). */
export async function dispatchRequest(urlText: string): Promise<any> {
  let parsed: URL;
  try {
    parsed = new URL(urlText);
  } catch {
    throw new TypeError(`protocol: invalid URL '${urlText}'`);
  }
  const scheme = parsed.protocol.slice(0, -1).toLowerCase();
  const handler = handlers.get(scheme);
  if (!handler) throw new Error(`protocol: no handler registered for scheme '${scheme}:'`);
  errorIfRemote(scheme, 'protocol.handle');
  const request: any = { url: urlText, scheme, host: parsed.host, path: parsed.pathname };
  // Legacy file-protocol handlers may use a `(request, callback)` signature
  // with a string path. New `handle` handlers return a path or a Response.
  const result = await (handler as any)(request);
  if (typeof result === 'string') {
    const rewritten = rewriteCustomUrl(
      result.includes('://') ? result : `${scheme}://localhost${result.startsWith('/') ? result : '/' + result}`,
    );
    return rewritten ?? result;
  }
  if (result && typeof result === 'object' && typeof result.path === 'string') {
    const rewritten = rewriteCustomUrl(
      result.path.includes('://')
        ? result.path
        : `${scheme}://localhost${result.path.startsWith('/') ? result.path : '/' + result.path}`,
    );
    return rewritten ?? result.path;
  }
  return result;
}

export function getRegisteredSchemes(): string[] {
  return [...handlers.keys()].sort();
}

export const protocol = {
  handle(scheme: string, handler: Function) {
    register(scheme, handler, 'protocol.handle');
  },
  unhandle(scheme: string) {
    unregister(scheme);
  },
  isProtocolHandled(scheme: string) {
    return isHandled(scheme);
  },
  isProtocolRegistered(scheme: string) {
    return isRegistered(scheme);
  },
  isProtocolIntercepted(scheme: string) {
    return isIntercepted(scheme);
  },
  registerFileProtocol(scheme: string, handler?: Function) {
    if (handler === undefined) {
      const normalized = normalizeScheme(scheme);
      errorIfRemote(normalized, 'protocol.registerFileProtocol');
      if (handlers.has(normalized)) throw new Error(`Handler already registered for scheme '${normalized}:'`);
      handlers.set(normalized, (request: any) => rewrittenOrPath(request.url ?? request));
      return;
    }
    register(scheme, handler, 'protocol.registerFileProtocol');
  },
  registerBufferProtocol(scheme: string, handler: Function) {
    register(scheme, handler, 'protocol.registerBufferProtocol');
  },
  registerStringProtocol(scheme: string, handler: Function) {
    register(scheme, handler, 'protocol.registerStringProtocol');
  },
  registerStreamProtocol(scheme: string, handler: Function) {
    register(scheme, handler, 'protocol.registerStreamProtocol');
  },
  registerHttpProtocol(scheme: string, handler: Function) {
    register(scheme, handler, 'protocol.registerHttpProtocol');
  },
  interceptFileProtocol(scheme: string, handler: Function) {
    const normalized = register(scheme, handler, 'protocol.interceptFileProtocol');
    intercepted.add(normalized);
  },
  interceptBufferProtocol(scheme: string, handler: Function) {
    const normalized = register(scheme, handler, 'protocol.interceptBufferProtocol');
    intercepted.add(normalized);
  },
  interceptStringProtocol(scheme: string, handler: Function) {
    const normalized = register(scheme, handler, 'protocol.interceptStringProtocol');
    intercepted.add(normalized);
  },
  interceptStreamProtocol(scheme: string, handler: Function) {
    const normalized = register(scheme, handler, 'protocol.interceptStreamProtocol');
    intercepted.add(normalized);
  },
  interceptHttpProtocol(scheme: string, handler: Function) {
    register(scheme, handler, 'protocol.interceptHttpProtocol');
  },
  registerSchemesAsPrivileged(schemes: any[], _privileges?: any) {
    if (!Array.isArray(schemes)) throw new TypeError('protocol.registerSchemesAsPrivileged requires an array of schemes');
    for (const entry of schemes) {
      const scheme = typeof entry === 'string' ? entry : entry?.scheme;
      privileged.add(normalizeScheme(scheme));
    }
    compatWarnings.push(
      'protocol.registerSchemesAsPrivileged is recorded only; privileged-scheme semantics (CORS, service workers) are not applied by the WebView host',
    );
  },
  unregisterProtocol(scheme: string) {
    unregister(scheme);
  },
  uninterceptProtocol(scheme: string) {
    unregister(scheme);
  },
};

function rewrittenOrPath(urlValue: any): string {
  if (typeof urlValue !== 'string') throw new TypeError('protocol.registerFileProtocol handler expects a URL string');
  return rewriteCustomUrl(urlValue) ?? urlValue;
}

export default protocol;

const g: any = globalThis;
g.__tsgpui_protocol = protocol;
g.__tsgpui_protocol_rewrite = rewriteCustomUrl;
