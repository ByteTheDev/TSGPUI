import { host, compatWarnings } from './host';
import { EventEmitter } from './events';

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
  fetch() { return Promise.reject(new Error('net.fetch is not supported')); },
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

