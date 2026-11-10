import { compatWarnings } from './host';

function encodeUtf8(text: string): Uint8Array {
  const g: any = globalThis;
  if (typeof g.TextEncoder === 'function') return new g.TextEncoder().encode(text);
  const out: number[] = [];
  for (let i = 0; i < text.length; i++) {
    let cp = text.charCodeAt(i);
    if (cp >= 0xD800 && cp <= 0xDBFF && i + 1 < text.length) {
      const next = text.charCodeAt(i + 1);
      if (next >= 0xDC00 && next <= 0xDFFF) { cp = 0x10000 + ((cp - 0xD800) << 10) + (next - 0xDC00); i++; }
    }
    if (cp < 0x80) out.push(cp);
    else if (cp < 0x800) { out.push(0xC0 | (cp >> 6)); out.push(0x80 | (cp & 0x3F)); }
    else if (cp < 0x10000) { out.push(0xE0 | (cp >> 12)); out.push(0x80 | ((cp >> 6) & 0x3F)); out.push(0x80 | (cp & 0x3F)); }
    else { out.push(0xF0 | (cp >> 18)); out.push(0x80 | ((cp >> 12) & 0x3F)); out.push(0x80 | ((cp >> 6) & 0x3F)); out.push(0x80 | (cp & 0x3F)); }
  }
  return new Uint8Array(out);
}

// Custom protocol shim for Electron -> TSGPUI transfer.
// Deepened to support binary and streaming response bodies.

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
  try { return handlers.has(normalizeScheme(schemeInput)); } catch { return false; }
}

function isRegistered(schemeInput: any): boolean {
  try {
    const scheme = normalizeScheme(schemeInput);
    return handlers.has(scheme) || privileged.has(scheme);
  } catch { return false; }
}

function isIntercepted(schemeInput: any): boolean {
  try { return intercepted.has(normalizeScheme(schemeInput)); } catch { return false; }
}

function pathFromCustomUrl(urlText: string): string | null {
  let parsed: URL;
  try { parsed = new URL(urlText); } catch { return null; }
  const scheme = parsed.protocol.slice(0, -1).toLowerCase();
  if (scheme === 'tsgpui') {
    const path = decodedPath(parsed);
    return path.startsWith('/') ? path : '/' + path;
  }
  if (!handlers.has(scheme)) return null;
  if (REMOTE_SCHEMES.has(scheme)) return null;
  const path = decodedPath(parsed);
  return path.startsWith('/') ? path : '/' + path;
}

function decodedPath(parsed: URL): string {
  const raw = parsed.pathname || '/';
  try { return decodeURIComponent(raw); } catch { return raw; }
}

export function rewriteCustomUrl(urlText: string): string | null {
  if (typeof urlText !== 'string') return null;
  if (urlText.startsWith('tsgpui://localhost/')) return urlText;
  const path = pathFromCustomUrl(urlText);
  if (path === null) return null;
  if (path.startsWith('//') || path.split('/').includes('..') || path.includes('\\')) return null;
  return `tsgpui://localhost${path}`;
}

async function toBytes(body: any): Promise<Uint8Array | null> {
  if (body == null) return null;
  if (typeof body === 'string') return encodeUtf8(body);
  if (body instanceof Uint8Array) return body;
  if (body instanceof ArrayBuffer) return new Uint8Array(body);
  if (ArrayBuffer.isView(body)) return new Uint8Array(body.buffer, body.byteOffset, body.byteLength);
  if (Array.isArray(body)) return new Uint8Array(body);
  if (typeof body.arrayBuffer === 'function') {
    try { return new Uint8Array(await body.arrayBuffer()); } catch { return null; }
  }
  if (typeof body.getReader === 'function') {
    const reader = body.getReader();
    const chunks: Uint8Array[] = [];
    let total = 0;
    // eslint-disable-next-line no-constant-condition
    while (true) {
      const { done, value } = await reader.read();
      if (done) break;
      const b = value instanceof Uint8Array ? value : new Uint8Array(value);
      chunks.push(b); total += b.length;
    }
    const out = new Uint8Array(total);
    let off = 0;
    for (const c of chunks) { out.set(c, off); off += c.length; }
    return out;
  }
  if (body && typeof body[Symbol.asyncIterator] === 'function') {
    const chunks: Uint8Array[] = [];
    let total = 0;
    for await (const value of body) {
      const b = value instanceof Uint8Array ? value : new Uint8Array(value);
      chunks.push(b); total += b.length;
    }
    const out = new Uint8Array(total);
    let off = 0;
    for (const c of chunks) { out.set(c, off); off += c.length; }
    return out;
  }
  return null;
}

async function invokeHandler(handler: Function, request: any): Promise<any> {
  if (handler.length >= 2) {
    return await new Promise((resolve) => {
      let done = false;
      const cb = (response: any) => { if (!done) { done = true; resolve(response); } };
      const ret = handler(request, cb);
      Promise.resolve(ret).then((v: any) => { if (v !== undefined && !done) { done = true; resolve(v); } }).catch(() => {});
      // If callback is never invoked, resolve with ret after a microtask.
      Promise.resolve().then(() => { if (!done) { done = true; resolve(ret); } });
    });
  }
  return await handler(request);
}

function normalizeHeaders(h: any): any {
  if (!h) return {};
  if (h instanceof Map) {
    const out: any = {};
    for (const [k, v] of h.entries()) out[k] = v;
    return out;
  }
  if (typeof h.forEach === 'function' && !Array.isArray(h)) {
    const out: any = {};
    try { h.forEach((v: any, k: any) => { out[k] = v; }); } catch { /* ignore */ }
    if (Object.keys(out).length) return out;
  }
  return { ...h };
}

async function normalizeResponse(result: any): Promise<any> {
  if (result == null) return null;
  if (typeof result === 'string') {
    const rewritten = rewriteCustomUrl(
      result.includes('://') ? result : `app://localhost${result.startsWith('/') ? result : '/' + result}`,
    );
    // Keep legacy behavior: return rewritten string when possible.
    // But only do this if it looks like a path/URL string from a file protocol.
    return rewritten ?? result;
  }
  if (result && typeof result === 'object' && typeof result.path === 'string' && !result.body && !result.data) {
    const rewritten = rewriteCustomUrl(
      result.path.includes('://')
        ? result.path
        : `app://localhost${result.path.startsWith('/') ? result.path : '/' + result.path}`,
    );
    return rewritten ?? result.path;
  }

  // Buffer protocol style: { data, mimeType, ... }
  if (result && typeof result === 'object' && (result.data != null) && result.body == null && result.status == null && result.statusCode == null && !result.headers) {
    const bytes = await toBytes(result.data);
    return {
      statusCode: result.statusCode || result.status || 200,
      status: result.statusCode || result.status || 200,
      headers: result.mimeType ? { 'content-type': result.mimeType } : {},
      mimeType: result.mimeType,
      body: bytes,
    };
  }

  // Response-like
  const status = result.status ?? result.statusCode ?? 200;
  const headers = normalizeHeaders(result.headers);
  if (result.mimeType && !headers['content-type'] && !headers['Content-Type']) headers['content-type'] = result.mimeType;

  let body: any = result.body;
  if (body == null && result.data != null) body = result.data;
  // If body is a stream, collect.
  const bytes = await toBytes(body);
  const finalBody = (typeof body === 'string') ? body : (bytes ?? body);

  return {
    statusCode: status,
    status,
    headers,
    mimeType: result.mimeType || headers['content-type'] || headers['Content-Type'],
    body: finalBody,
  };
}

export async function dispatchRequest(urlText: string): Promise<any> {
  let parsed: URL;
  try { parsed = new URL(urlText); } catch { throw new TypeError(`protocol: invalid URL '${urlText}'`); }
  const scheme = parsed.protocol.slice(0, -1).toLowerCase();
  const handler = handlers.get(scheme);
  if (!handler) throw new Error(`protocol: no handler registered for scheme '${scheme}:'`);
  errorIfRemote(scheme, 'protocol.handle');
  const request: any = { url: urlText, scheme, host: parsed.host, path: parsed.pathname, method: 'GET' };
  const result = await invokeHandler(handler, request);
  return await normalizeResponse(result);
}

export function getRegisteredSchemes(): string[] {
  return [...handlers.keys()].sort();
}

export const protocol = {
  handle(scheme: string, handler: Function) { register(scheme, handler, 'protocol.handle'); },
  unhandle(scheme: string) { unregister(scheme); },
  isProtocolHandled(scheme: string) { return isHandled(scheme); },
  isProtocolRegistered(scheme: string) { return isRegistered(scheme); },
  isProtocolIntercepted(scheme: string) { return isIntercepted(scheme); },
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
  registerBufferProtocol(scheme: string, handler: Function) { register(scheme, handler, 'protocol.registerBufferProtocol'); },
  registerStringProtocol(scheme: string, handler: Function) { register(scheme, handler, 'protocol.registerStringProtocol'); },
  registerStreamProtocol(scheme: string, handler: Function) { register(scheme, handler, 'protocol.registerStreamProtocol'); },
  registerHttpProtocol(scheme: string, handler: Function) { register(scheme, handler, 'protocol.registerHttpProtocol'); },
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
  unregisterProtocol(scheme: string) { unregister(scheme); },
  uninterceptProtocol(scheme: string) { unregister(scheme); },
};

function rewrittenOrPath(urlValue: any): string {
  if (typeof urlValue !== 'string') throw new TypeError('protocol.registerFileProtocol handler expects a URL string');
  return rewriteCustomUrl(urlValue) ?? urlValue;
}

export default protocol;

const g: any = globalThis;
g.__tsgpui_protocol = protocol;
g.__tsgpui_protocol_rewrite = rewriteCustomUrl;