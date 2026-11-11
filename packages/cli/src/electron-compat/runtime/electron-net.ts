import { host, compatWarnings } from './host';
import { EventEmitter } from './events';
import { recordNetLog } from './electron-logging';

// Permission-checked HTTPS transport shim. Deepened to support binary request
// bodies, response streaming, redirect handling, request cancellation and an
// online check. The host still returns a buffered response, so streaming is
// emulated by emitting the body in one or more chunks.

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
function decodeUtf8(bytes: Uint8Array): string {
  const g: any = globalThis;
  if (typeof g.TextDecoder === 'function') return new g.TextDecoder().decode(bytes);
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
function toBytes(chunk: any): Uint8Array | null {
  if (chunk == null) return null;
  if (typeof chunk === 'string') return encodeUtf8(chunk);
  if (chunk instanceof Uint8Array) return chunk;
  if (chunk instanceof ArrayBuffer) return new Uint8Array(chunk);
  if (ArrayBuffer.isView(chunk)) return new Uint8Array(chunk.buffer, chunk.byteOffset, chunk.byteLength);
  if (Array.isArray(chunk)) return new Uint8Array(chunk);
  return null;
}

function bodyToString(body: any): string {
  if (body == null) return '';
  if (typeof body === 'string') return body;
  const bytes = toBytes(body);
  if (bytes) {
    try { return decodeUtf8(bytes); } catch { return ''; }
  }
  if (Array.isArray(body)) return body.map((c: any) => (typeof c === 'string' ? c : '')).join('');
  return String(body);
}

function collectChunks(result: any): any[] {
  if (result == null) return [];
  if (Array.isArray(result.chunks)) return result.chunks;
  if (Array.isArray(result.bodyChunks)) return result.bodyChunks;
  if (result.body != null) return [result.body];
  if (result.data != null) return [result.data];
  return [];
}

export const net = {
  isOnline(): boolean {
    const nav: any = (globalThis as any).navigator;
    if (nav && typeof nav.onLine === 'boolean') return nav.onLine;
    return true;
  },
  async fetch(input: any, init?: any): Promise<any> {
    let url: string;
    let options: any = init && typeof init === 'object' ? { ...init } : {};
    if (typeof input === 'string') url = input;
    else if (input instanceof URL) url = input.toString();
    else if (input && typeof input === 'object') {
      url = input.url || input.href || String(input);
      options = { ...input, ...options };
      delete options.url;
    } else {
      url = String(input);
    }

    const method = String(options.method || 'GET').toUpperCase();
    const headers: any = {};
    const rawHeaders = options.headers;
    if (rawHeaders && typeof rawHeaders === 'object') {
      if (typeof rawHeaders.forEach === 'function' && typeof rawHeaders.get !== 'undefined') {
        try { rawHeaders.forEach((v: any, k: any) => { headers[k] = v; }); } catch {}
      } else {
        for (const k of Object.keys(rawHeaders)) headers[k] = rawHeaders[k];
      }
    }

    const redirect = options.redirect || 'follow';
    if (redirect !== 'follow' && redirect !== 'manual' && redirect !== 'error') throw new TypeError('net.fetch redirect must be follow, manual or error');

    let bodyToSend: any = undefined;
    if (options.body != null) {
      const b = options.body;
      if (typeof b === 'string') bodyToSend = b;
      else if (b instanceof Uint8Array) bodyToSend = b;
      else if (b instanceof ArrayBuffer) bodyToSend = new Uint8Array(b);
      else if (ArrayBuffer.isView(b)) bodyToSend = new Uint8Array((b as any).buffer, (b as any).byteOffset, (b as any).byteLength);
      else if (typeof b === 'object') {
        try {
          if (typeof URLSearchParams !== 'undefined' && b instanceof URLSearchParams) {
            bodyToSend = b.toString();
            if (!hasContentType(headers)) headers['content-type'] = 'application/x-www-form-urlencoded;charset=UTF-8';
          } else {
            bodyToSend = JSON.stringify(b);
            if (!hasContentType(headers)) headers['content-type'] = 'application/json';
          }
        } catch {
          bodyToSend = String(b);
        }
      } else {
        bodyToSend = String(b);
      }
    }

    const signal = options.signal;

    return new Promise((resolve, reject) => {
      let redirected = false;
      let finalUrl = url;

      const reqOpts: any = { url, method, headers, redirect };
      const req = (this as any).request(reqOpts, (res: any) => {
        const chunks: any[] = [];
        res.on('data', (c: any) => chunks.push(c));
        res.on('end', () => {
          const bytes = collectFetchBytes(chunks);
          resolve(makeFetchResponse(res, bytes, finalUrl, redirected));
        });
        res.on('error', (e: any) => reject(e instanceof Error ? e : new Error(String(e))));
      });

      if (typeof req.on === 'function') {
        req.on('redirect', (_status: any, _method: any, redirectUrl: any) => {
          redirected = true;
          finalUrl = String(redirectUrl);
        });
        req.on('error', (e: any) => reject(e instanceof Error ? e : new Error(String(e))));
      }

      if (signal) {
        const abort = () => {
          try { req.abort(); } catch {}
          const err: any = new Error('The operation was aborted.');
          err.name = 'AbortError';
          reject(err);
        };
        if (signal.aborted) { abort(); return; }
        if (typeof signal.addEventListener === 'function') signal.addEventListener('abort', abort, { once: true });
      }

      try {
        if (bodyToSend !== undefined) req.write(bodyToSend);
        req.end();
      } catch (e: any) {
        reject(e instanceof Error ? e : new Error(String(e)));
      }
    });
  },
  request(options: any, ...rest: any[]) {
    if (typeof options !== 'string' && (!options || typeof options !== 'object')) throw new TypeError('net.request requires a URL or options object');
    const callback = rest.find(a => typeof a === 'function');
    const extra = rest.find(a => a && typeof a === 'object') || {};
    const opts: any = typeof options === 'string' ? { url: options } : { ...options };
    if (extra && typeof extra === 'object') Object.assign(opts, extra);

    if (opts.session || opts.partition || opts.useSessionCookies || opts.credentials) throw new Error('net.request session and credential options are not supported');

    const redirectMode = opts.redirect || 'follow';
    if (redirectMode !== 'follow' && redirectMode !== 'manual' && redirectMode !== 'error') throw new TypeError('net.request redirect must be follow, manual or error');

    const target: any = { url: opts.url || opts.href, method: opts.method || 'GET', headers: { ...(opts.headers || {}) } };
    const req: any = new EventEmitter();
    // lazy import safe: events already imported above? use imported EventEmitter
    let ended = false;
    let aborted = false;
    let bodyParts: any[] = [];
    let bodyIsBinary = false;
    const pendingRedirects: any[] = [];

    const lowerHeaders = () => {
      const out: any = {};
      for (const k of Object.keys(target.headers)) out[k.toLowerCase()] = target.headers[k];
      return out;
    };

    req.setHeader = (name: string, value: any) => {
      if (ended) throw new Error('Cannot set headers after end');
      if (typeof value !== 'string' && typeof value !== 'number') throw new TypeError('header value must be a string');
      const old = Object.keys(target.headers).find(k => k.toLowerCase() === name.toLowerCase());
      if (old) delete target.headers[old];
      target.headers[name] = String(value);
    };
    req.getHeader = (name: string) => {
      const key = Object.keys(target.headers).find(k => k.toLowerCase() === name.toLowerCase());
      return key === undefined ? undefined : target.headers[key];
    };
    req.removeHeader = (name: string) => {
      if (ended) throw new Error('Cannot remove headers after end');
      const key = Object.keys(target.headers).find(k => k.toLowerCase() === name.toLowerCase());
      if (key !== undefined) delete target.headers[key];
    };

    req.write = (chunk: any, encoding?: any, cb?: Function) => {
      if (ended) throw new Error('write after end');
      if (aborted) return false;
      const bytes = toBytes(chunk);
      if (bytes && typeof chunk !== 'string') {
        bodyIsBinary = true;
        bodyParts.push(bytes);
      } else if (typeof chunk === 'string') {
        bodyParts.push(chunk);
      } else if (chunk != null) {
        throw new TypeError('net.request supports text and binary bodies only');
      }
      if (typeof cb === 'function') cb();
      return true;
    };
    req.end = (chunk?: any, encoding?: any, cb?: Function) => {
      if (ended) throw new Error('end after end');
      if (chunk !== undefined) req.write(chunk);
      ended = true;
      if (typeof cb === 'function') cb();
      doRequest(target.url, target.method, target.headers, redirectMode, 0);
      return req;
    };

    function buildBody(): any {
      if (bodyParts.length === 0) return '';
      if (!bodyIsBinary) return bodyParts.join('');
      // Concatenate bytes and send as base64 with an encoding hint.
      let total = 0;
      const arrays = bodyParts.map(p => (p instanceof Uint8Array ? p : encodeUtf8(String(p))));
      for (const a of arrays) total += a.length;
      const out = new Uint8Array(total);
      let off = 0;
      for (const a of arrays) { out.set(a, off); off += a.length; }
      let binaryString = '';
      for (let i = 0; i < out.length; i++) binaryString += String.fromCharCode(out[i]);
      return { __binary: true, base64: (globalThis as any).btoa ? btoa(binaryString) : toBase64(out) };
    }

    function toBase64(bytes: Uint8Array): string {
      const alpha = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/';
      let out = '';
      for (let i = 0; i < bytes.length; i += 3) {
        const n = (bytes[i] << 16) | ((bytes[i + 1] || 0) << 8) | (bytes[i + 2] || 0);
        out += alpha[n >> 18] + alpha[(n >> 12) & 63] + (i + 1 < bytes.length ? alpha[(n >> 6) & 63] : '=') + (i + 2 < bytes.length ? alpha[n & 63] : '=');
      }
      return out;
    }

    async function doRequest(url: string, method: string, headers: any, mode: string, depth: number) {
      if (aborted) return;
      const body = buildBody();
      const args: any = { url, method, headers: { ...headers } };
      if (body && (typeof body === 'string' ? body.length : true)) {
        if (typeof body === 'object' && body.__binary) {
          args.body = body.base64;
          args.bodyEncoding = 'base64';
        } else {
          args.body = body;
        }
      }
      try {
        const result: any = await host('net.request', args);
        if (aborted) return;
        const status = result?.status ?? result?.statusCode ?? 200;
        const respHeaders = result?.headers || {};
        recordNetLog({url, method, status, headers: respHeaders});
        // Redirect handling
        const loc = respHeaders.location || respHeaders.Location;
        if (loc && status >= 300 && status < 400) {
          const redirectUrl = new URL(loc, url).toString();
          req.emit('redirect', status, method, redirectUrl, respHeaders);
          if (mode === 'error') { req.emit('error', new Error(`Redirect ${status} to ${redirectUrl}`)); return; }
          if (mode === 'manual') {
            pendingRedirects.push({ url: redirectUrl, status, headers: respHeaders });
            // Expose followRedirect
            req.followRedirect = () => {
              const next = pendingRedirects.shift();
              if (!next) return;
              const nextMethod = (status === 303) ? 'GET' : method;
              doRequest(next.url, nextMethod, headers, mode, depth + 1);
            };
            // Emit response for manual redirect
            const response = makeResponse(status, respHeaders, collectChunks(result));
            req.emit('response', response);
            if (callback) callback(response);
            return;
          }
          // follow
          if (depth < 5) {
            const nextMethod = (status === 303) ? 'GET' : method;
            doRequest(redirectUrl, nextMethod, headers, mode, depth + 1);
            return;
          }
        }
        const response = makeResponse(status, respHeaders, collectChunks(result));
        req.emit('response', response);
        if (callback) callback(response);
      } catch (error: any) {
        if (aborted) return;
        recordNetLog({url, method, error: String(error?.message || error)});
        req.emit('error', error instanceof Error ? error : new Error(String(error)));
      }
    }

    function makeResponse(status: number, headers: any, chunks: any[]) {
      const response: any = new EventEmitter();
      response.statusCode = status;
      response.status = status;
      response.headers = headers;
      let endedEmitted = false;
      // Emit data chunks
      Promise.resolve().then(() => {
        for (const c of chunks.length ? chunks : ['']) {
          if (c == null) continue;
          const chunk = (typeof c === 'string') ? c : (toBytes(c) ?? c);
          response.emit('data', chunk);
        }
        if (!endedEmitted) { endedEmitted = true; response.emit('end'); }
      });
      response.on('end', () => {});
      return response;
    }

    req.abort = () => {
      if (aborted) return;
      aborted = true;
      req.emit('abort');
    };
    req.destroy = (err?: any) => { req.abort(); if (err) req.emit('error', err); };
    req.followRedirect = () => {};

    // If a callback was passed as (options, callback)
    if (callback) {
      // callback will be invoked on response
    }

    return req;
  },
};

function hasContentType(headers: any): boolean {
  return Object.keys(headers || {}).some(k => String(k).toLowerCase() === 'content-type');
}

function collectFetchBytes(chunks: any[]): Uint8Array {
  const arrays = chunks.map((c: any) => {
    if (c == null) return new Uint8Array(0);
    if (typeof c === 'string') return encodeUtf8(c);
    const b = toBytes(c);
    return b || new Uint8Array(0);
  });
  let total = 0;
  for (const a of arrays) total += a.length;
  const out = new Uint8Array(total);
  let off = 0;
  for (const a of arrays) { out.set(a, off); off += a.length; }
  return out;
}

const STATUS_TEXT: Record<number, string> = {
  200: 'OK', 201: 'Created', 202: 'Accepted', 204: 'No Content', 301: 'Moved Permanently',
  302: 'Found', 303: 'See Other', 304: 'Not Modified', 307: 'Temporary Redirect', 308: 'Permanent Redirect',
  400: 'Bad Request', 401: 'Unauthorized', 403: 'Forbidden', 404: 'Not Found', 405: 'Method Not Allowed',
  408: 'Request Timeout', 409: 'Conflict', 413: 'Payload Too Large', 429: 'Too Many Requests',
  500: 'Internal Server Error', 502: 'Bad Gateway', 503: 'Service Unavailable', 504: 'Gateway Timeout',
};

function createFetchHeaders(raw: any): any {
  const lower: any = {};
  for (const k of Object.keys(raw || {})) lower[String(k).toLowerCase()] = raw[k];
  const get = (name: any) => {
    const v = lower[String(name).toLowerCase()];
    if (v == null) return null;
    return Array.isArray(v) ? v.join(', ') : String(v);
  };
  const has = (name: any) => Object.prototype.hasOwnProperty.call(lower, String(name).toLowerCase());
  const forEach = (cb: Function) => { for (const k of Object.keys(lower)) cb(lower[k], k, headers); };
  const headers: any = { ...lower };
  headers.get = get;
  headers.has = has;
  headers.forEach = forEach;
  headers.entries = () => Object.entries(lower);
  headers.keys = () => Object.keys(lower);
  headers.values = () => Object.values(lower);
  return headers;
}

function makeFetchResponse(res: any, bytes: Uint8Array, url: string, redirected: boolean): any {
  const status = res?.statusCode ?? res?.status ?? 200;
  const rawHeaders = res?.headers || {};
  const headers = createFetchHeaders(rawHeaders);
  let used = false;
  const response: any = {
    ok: status >= 200 && status < 300,
    status,
    statusText: STATUS_TEXT[status] || '',
    url,
    redirected,
    type: 'basic',
    headers,
    bodyUsed: false,
    async text() {
      if (used) throw new TypeError('Body has already been consumed.');
      used = true;
      response.bodyUsed = true;
      return decodeUtf8(bytes);
    },
    async json() {
      const t = await response.text();
      return JSON.parse(t);
    },
    async arrayBuffer() {
      if (used) throw new TypeError('Body has already been consumed.');
      used = true;
      response.bodyUsed = true;
      return bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength);
    },
    async blob() {
      const ab = await response.arrayBuffer();
      return { size: bytes.length, type: headers.get('content-type') || '', arrayBuffer: async () => ab };
    },
    clone() {
      return makeFetchResponse({ statusCode: status, headers: rawHeaders }, new Uint8Array(bytes), url, redirected);
    },
  };
  return response;
}

