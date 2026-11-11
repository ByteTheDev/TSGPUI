import { host, compatWarnings } from './host';

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
// Supports packaged-asset handlers, binary and streaming bodies, callback handlers, remote
// proxy responses through the permission-checked transport, and per-scheme privilege records.

const SCHEME_RE = /^[a-z][a-z0-9+.-]*$/i;
const REMOTE_SCHEMES = new Set(['http', 'https', 'ws', 'wss']);
const PRIVILEGE_KEYS = new Set([
  'standard', 'secure', 'bypassCSP', 'allowServiceWorkers',
  'supportFetchAPI', 'corsEnabled', 'stream', 'codeCache',
]);

type SchemePrivileges = {
  standard?: boolean;
  secure?: boolean;
  bypassCSP?: boolean;
  allowServiceWorkers?: boolean;
  supportFetchAPI?: boolean;
  corsEnabled?: boolean;
  stream?: boolean;
  codeCache?: boolean;
};

const handlers = new Map<string, Function>();
const handlerKinds = new Map<string, string>();
const intercepted = new Set<string>();
const privileged = new Map<string, SchemePrivileges>();

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

function register(schemeInput: any, handler: any, api: string, allowRemote = false, kind = 'handle'): string {
  const scheme = normalizeScheme(schemeInput);
  if (typeof handler !== 'function') throw new TypeError(`${api} for '${scheme}:' requires a handler function`);
  if (!allowRemote) errorIfRemote(scheme, api);
  if (handlers.has(scheme)) throw new Error(`Handler already registered for scheme '${scheme}:'`);
  handlers.set(scheme, handler);
  handlerKinds.set(scheme, kind);
  return scheme;
}

function unregister(schemeInput: any): void {
  const scheme = normalizeScheme(schemeInput);
  handlers.delete(scheme);
  handlerKinds.delete(scheme);
  intercepted.delete(scheme);
}

function isHandled(schemeInput: any): boolean {
  try { return handlers.has(normalizeScheme(schemeInput)); } catch { return false; }
}

function isRegistered(schemeInput: any): boolean {
  try {
    const scheme = normalizeScheme(schemeInput);
    return handlers.has(scheme) || intercepted.has(scheme);
  } catch { return false; }
}

function normalizePrivileges(scheme: string, input: any): SchemePrivileges {
  if (input == null) return {};
  if (typeof input !== 'object' || Array.isArray(input)) {
    throw new TypeError(`protocol.registerSchemesAsPrivileged: privileges for '${scheme}:' must be an object`);
  }
  const out: SchemePrivileges = {};
  for (const key of Object.keys(input)) {
    if (!PRIVILEGE_KEYS.has(key)) {
      compatWarnings.push(`protocol.registerSchemesAsPrivileged: unknown privilege '${key}' for scheme '${scheme}:' was ignored`);
      continue;
    }
    (out as any)[key] = input[key] === true;
  }
  return out;
}

export function getSchemePrivileges(schemeInput: any): SchemePrivileges | null {
  try {
    const scheme = normalizeScheme(schemeInput);
    const value = privileged.get(scheme);
    return value ? { ...value } : null;
  } catch { return null; }
}

export function getPrivilegedSchemes(): string[] {
  return [...privileged.keys()].sort();
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

function chunkToBytes(value: any): Uint8Array | null {
  if (value == null) return null;
  if (typeof value === 'string') return encodeUtf8(value);
  if (value instanceof Uint8Array) return value;
  if (value instanceof ArrayBuffer) return new Uint8Array(value);
  if (ArrayBuffer.isView(value)) return new Uint8Array(value.buffer, value.byteOffset, value.byteLength);
  if (Array.isArray(value)) return new Uint8Array(value);
  return null;
}

function concatBytes(chunks: Uint8Array[]): Uint8Array {
  let total = 0;
  for (const chunk of chunks) total += chunk.length;
  const out = new Uint8Array(total);
  let offset = 0;
  for (const chunk of chunks) { out.set(chunk, offset); offset += chunk.length; }
  return out;
}

async function toBytes(body: any): Promise<Uint8Array | null> {
  if (body == null) return null;
  const direct = chunkToBytes(body);
  if (direct) return direct;
  if (typeof body.arrayBuffer === 'function') {
    try { return new Uint8Array(await body.arrayBuffer()); } catch { return null; }
  }
  if (typeof body.getReader === 'function') {
    const reader = body.getReader();
    const chunks: Uint8Array[] = [];
    // eslint-disable-next-line no-constant-condition
    while (true) {
      const { done, value } = await reader.read();
      if (done) break;
      const bytes = chunkToBytes(value);
      if (bytes) chunks.push(bytes);
    }
    return concatBytes(chunks);
  }
  if (typeof body[Symbol.asyncIterator] === 'function') {
    const chunks: Uint8Array[] = [];
    for await (const value of body) {
      const bytes = chunkToBytes(value);
      if (bytes) chunks.push(bytes);
    }
    return concatBytes(chunks);
  }
  return null;
}

function decodeUtf8(bytes: Uint8Array): string {
  const g: any = globalThis;
  if (typeof g.TextDecoder === 'function') {
    try { return new g.TextDecoder().decode(bytes); } catch { /* fall through */ }
  }
  let out = '';
  let i = 0;
  while (i < bytes.length) {
    const b = bytes[i++];
    if (b < 0x80) out += String.fromCharCode(b);
    else if ((b & 0xE0) === 0xC0) { const b2 = bytes[i++] & 0x3F; out += String.fromCharCode(((b & 0x1F) << 6) | b2); }
    else if ((b & 0xF0) === 0xE0) { const b2 = bytes[i++] & 0x3F; const b3 = bytes[i++] & 0x3F; out += String.fromCharCode(((b & 0x0F) << 12) | (b2 << 6) | b3); }
    else { const b2 = bytes[i++] & 0x3F; const b3 = bytes[i++] & 0x3F; const b4 = bytes[i++] & 0x3F; const cp = ((b & 0x07) << 18) | (b2 << 12) | (b3 << 6) | b4; const v = cp - 0x10000; out += String.fromCharCode(0xD800 + (v >> 10), 0xDC00 + (v & 0x3FF)); }
  }
  return out;
}

async function invokeHandler(handler: Function, request: any): Promise<any> {
  if (handler.length >= 2) {
    return await new Promise((resolve, reject) => {
      let done = false;
      const cb = (response: any) => { if (!done) { done = true; resolve(response); } };
      try {
        const ret = handler(request, cb);
        // A callback-style handler may also be async and return the response directly.
        if (ret && typeof ret.then === 'function') {
          ret.then((value: any) => { if (value !== undefined) cb(value); }, (error: any) => { if (!done) { done = true; reject(error); } });
        } else if (ret !== undefined) {
          cb(ret);
        }
      } catch (error) {
        if (!done) { done = true; reject(error); }
      }
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

function headerKey(headers: any, name: string): string | undefined {
  const lower = name.toLowerCase();
  return Object.keys(headers || {}).find(key => key.toLowerCase() === lower);
}

function hasHeader(headers: any, name: string): boolean {
  return headerKey(headers, name) !== undefined;
}

function isProxyResponse(result: any): boolean {
  return !!result
    && typeof result === 'object'
    && typeof result.url === 'string'
    && /^https?:/i.test(result.url)
    && result.status == null
    && result.statusCode == null
    && result.body == null
    && result.data == null
    && result.mimeType == null;
}

async function proxyBody(spec: any): Promise<string> {
  const upload = spec.uploadData;
  if (Array.isArray(upload) && upload.length) {
    const chunks: Uint8Array[] = [];
    for (const entry of upload) {
      if (typeof entry === 'string') { chunks.push(encodeUtf8(entry)); continue; }
      const direct = chunkToBytes(entry);
      if (direct) { chunks.push(direct); continue; }
      if (entry && typeof entry === 'object') {
        if (entry.file != null || entry.blobUUID != null) {
          throw new Error('protocol proxy uploadData entries for files/blobs are not supported; send bytes instead');
        }
        const bytes = chunkToBytes(entry.bytes);
        if (bytes) { chunks.push(bytes); continue; }
      }
      throw new TypeError('protocol proxy uploadData entries must contain bytes or strings');
    }
    return decodeUtf8(concatBytes(chunks));
  }
  const body = spec.body != null ? spec.body : spec.data;
  if (body == null) return '';
  if (typeof body === 'string') return body;
  const bytes = await toBytes(body);
  return bytes ? decodeUtf8(bytes) : String(body);
}

async function proxyRemote(spec: any): Promise<any> {
  const url = String(spec.url);
  let parsed: URL;
  try { parsed = new URL(url); } catch { throw new TypeError(`protocol proxy response has an invalid url '${url}'`); }
  if (!/^https?:$/i.test(parsed.protocol)) {
    throw new Error(`protocol proxy response must target http: or https:, got '${parsed.protocol}'`);
  }
  const method = String(spec.method || 'GET').toUpperCase();
  const headers = normalizeHeaders(spec.headers);
  if (spec.referrer != null && !hasHeader(headers, 'referer')) headers['referer'] = String(spec.referrer);
  const body = await proxyBody(spec);
  const args: any = { url: parsed.toString(), method, headers };
  if (body) args.body = body;
  let result: any;
  try {
    result = await host('net.request', args);
  } catch (error: any) {
    throw new Error(`protocol proxy request to ${parsed.origin} failed: ${error?.message || error}`);
  }
  return makeResponse(
    result?.status ?? result?.statusCode ?? 200,
    result?.headers || {},
    result?.body ?? result?.data,
    result?.mimeType,
  );
}

function makeResponse(status: number, headerInput: any, body: any, mimeType?: any, privileges?: SchemePrivileges): any {
  const headers = normalizeHeaders(headerInput);
  if (mimeType && !hasHeader(headers, 'content-type')) headers['content-type'] = mimeType;
  if (privileges?.corsEnabled && !hasHeader(headers, 'access-control-allow-origin')) {
    headers['access-control-allow-origin'] = '*';
  }
  const bytes = body instanceof Uint8Array ? body : null;
  const text = typeof body === 'string' ? body : (bytes ? decodeUtf8(bytes) : (body == null ? '' : String(body)));
  return {
    statusCode: status,
    status,
    headers,
    mimeType: mimeType || headers['content-type'] || headers['Content-Type'],
    body: bytes ?? body,
    text() { return text; },
    arrayBuffer() { return (bytes ?? encodeUtf8(text)).slice().buffer; },
    stream() {
      const g: any = globalThis;
      const data = bytes ?? encodeUtf8(text);
      if (typeof g.ReadableStream !== 'function') return null;
      return new g.ReadableStream({
        start(controller: any) { controller.enqueue(data); controller.close(); },
      });
    },
  };
}

async function normalizeResponse(result: any, privileges?: SchemePrivileges, kind = 'handle'): Promise<any> {
  if (result == null) return null;
  if (typeof result === 'string') {
    // String/stream protocol handlers return page content; file handlers return paths.
    if (kind === 'string' || kind === 'stream') {
      return makeResponse(200, { 'content-type': 'text/plain' }, result, 'text/plain', privileges);
    }
    const rewritten = rewriteCustomUrl(
      result.includes('://') ? result : `app://localhost${result.startsWith('/') ? result : '/' + result}`,
    );
    // Keep legacy behavior: return rewritten string when possible.
    // But only do this if it looks like a path/URL string from a file protocol.
    return rewritten ?? result;
  }
  if (kind === 'stream' && result && typeof result === 'object'
    && (typeof result.getReader === 'function' || typeof result[Symbol.asyncIterator] === 'function')) {
    return makeResponse(200, {}, await toBytes(result), undefined, privileges);
  }
  if (kind === 'buffer' && (result instanceof Uint8Array || result instanceof ArrayBuffer || ArrayBuffer.isView(result) || Array.isArray(result))) {
    return makeResponse(200, {}, await toBytes(result), undefined, privileges);
  }
  if (isProxyResponse(result)) return await proxyRemote(result);
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
    return makeResponse(result.statusCode ?? result.status ?? 200, result.mimeType ? { 'content-type': result.mimeType } : {}, bytes, result.mimeType, privileges);
  }

  // Response-like
  const status = result.status ?? result.statusCode ?? 200;
  const headers = normalizeHeaders(result.headers);
  let body: any = result.body;
  if (body == null && result.data != null) body = result.data;
  // If body is a stream, collect it so callers get a stable body plus text/arrayBuffer helpers.
  const bytes = await toBytes(body);
  const finalBody = (typeof body === 'string') ? body : (bytes ?? body);
  return makeResponse(status, headers, finalBody, result.mimeType, privileges);
}

export async function dispatchRequest(urlText: string): Promise<any> {
  let parsed: URL;
  try { parsed = new URL(urlText); } catch { throw new TypeError(`protocol: invalid URL '${urlText}'`); }
  const scheme = parsed.protocol.slice(0, -1).toLowerCase();
  const handler = handlers.get(scheme);
  if (!handler) throw new Error(`protocol: no handler registered for scheme '${scheme}:'`);
  const privileges = getSchemePrivileges(scheme) || undefined;
  const request: any = {
    url: urlText,
    scheme,
    host: parsed.host,
    path: parsed.pathname,
    method: 'GET',
    referrer: '',
    isSecure: privileges?.secure === true,
    privileges: privileges ? { ...privileges } : undefined,
  };
  const result = await invokeHandler(handler, request);
  return await normalizeResponse(result, privileges || undefined, handlerKinds.get(scheme) || 'handle');
}

export function getRegisteredSchemes(): string[] {
  return [...handlers.keys()].sort();
}

export const protocol = {
  handle(scheme: string, handler: Function) { register(scheme, handler, 'protocol.handle', false, 'handle'); },
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
      handlerKinds.set(normalized, 'file');
      return;
    }
    register(scheme, handler, 'protocol.registerFileProtocol', false, 'file');
  },
  registerBufferProtocol(scheme: string, handler: Function) { register(scheme, handler, 'protocol.registerBufferProtocol', false, 'buffer'); },
  registerStringProtocol(scheme: string, handler: Function) { register(scheme, handler, 'protocol.registerStringProtocol', false, 'string'); },
  registerStreamProtocol(scheme: string, handler: Function) { register(scheme, handler, 'protocol.registerStreamProtocol', false, 'stream'); },
  registerHttpProtocol(scheme: string, handler: Function) { register(scheme, handler, 'protocol.registerHttpProtocol', false, 'http'); },
  interceptFileProtocol(scheme: string, handler: Function) {
    const normalized = register(scheme, handler, 'protocol.interceptFileProtocol', true, 'file');
    intercepted.add(normalized);
  },
  interceptBufferProtocol(scheme: string, handler: Function) {
    const normalized = register(scheme, handler, 'protocol.interceptBufferProtocol', true, 'buffer');
    intercepted.add(normalized);
  },
  interceptStringProtocol(scheme: string, handler: Function) {
    const normalized = register(scheme, handler, 'protocol.interceptStringProtocol', true, 'string');
    intercepted.add(normalized);
  },
  interceptStreamProtocol(scheme: string, handler: Function) {
    const normalized = register(scheme, handler, 'protocol.interceptStreamProtocol', true, 'stream');
    intercepted.add(normalized);
  },
  interceptHttpProtocol(scheme: string, handler: Function) {
    const normalized = register(scheme, handler, 'protocol.interceptHttpProtocol', true, 'http');
    intercepted.add(normalized);
  },
  registerSchemesAsPrivileged(schemes: any[]) {
    if (!Array.isArray(schemes)) throw new TypeError('protocol.registerSchemesAsPrivileged requires an array of schemes');
    for (const entry of schemes) {
      if (entry == null) throw new TypeError('protocol.registerSchemesAsPrivileged entries need a scheme');
      const scheme = typeof entry === 'string' ? entry : entry.scheme;
      const normalized = normalizeScheme(scheme);
      const declared = typeof entry === 'string' ? {} : entry.privileges;
      const privileges = normalizePrivileges(normalized, declared);
      privileged.set(normalized, { ...(privileged.get(normalized) || {}), ...privileges });
    }
    compatWarnings.push(
      'protocol.registerSchemesAsPrivileged records standard/secure/bypassCSP/allowServiceWorkers/supportFetchAPI/corsEnabled/stream/codeCache per scheme; '
      + 'standard, secure and corsEnabled affect shim dispatch, while browser-level CORS, service-worker, CSP and code-cache behaviour is not applied by the WebView host',
    );
  },
  unregisterProtocol(scheme: string) { unregister(scheme); },
  uninterceptProtocol(scheme: string) { unregister(scheme); },
};

function rewrittenOrPath(urlValue: any): string {
  const urlText = (typeof urlValue === 'string')
    ? urlValue
    : (urlValue && typeof urlValue.url === 'string' ? urlValue.url : String(urlValue ?? ''));
  if (!urlText) throw new TypeError('protocol.registerFileProtocol handler expects a URL string');
  return rewriteCustomUrl(urlText) ?? urlText;
}

export default protocol;

const g: any = globalThis;
g.__tsgpui_protocol = protocol;
g.__tsgpui_protocol_rewrite = rewriteCustomUrl;