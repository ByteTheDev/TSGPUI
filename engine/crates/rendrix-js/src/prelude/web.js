// ---------------------------------------------------------------------------------------------
// Web platform APIs: window, timers, console, URL, encoding, fetch, storage, observers, lifecycle.
// ---------------------------------------------------------------------------------------------

const windowTarget = new EventTarget();

// ----- console --------------------------------------------------------------------------------

function inspect(v, depth = 2, seen = new Set()) {
  switch (typeof v) {
    case 'string': return depth === 2 ? v : JSON.stringify(v);
    case 'undefined': return 'undefined';
    case 'bigint': return v + 'n';
    case 'symbol': return v.toString();
    case 'function': return `[Function${v.name ? ': ' + v.name : ' (anonymous)'}]`;
    case 'object': break;
    default: return String(v);
  }
  if (v === null) return 'null';
  if (seen.has(v)) return '[Circular]';
  if (v instanceof Error) return errorText(v);
  if (v instanceof Node) return v.nodeType === 1 ? `<${v.localName}${v.id ? '#' + v.id : ''}>` : `[${v.nodeName}]`;
  if (v instanceof Date) return v.toISOString();
  if (v instanceof RegExp) return String(v);
  if (depth < 0) return Array.isArray(v) ? '[Array]' : '[Object]';
  seen.add(v);
  let out;
  if (Array.isArray(v) || ArrayBuffer.isView(v)) {
    const items = Array.from(v).slice(0, 100).map(x => inspect(x, depth - 1, seen));
    out = `[ ${items.join(', ')}${v.length > 100 ? ', …' : ''} ]`;
    if (!items.length) out = '[]';
  } else if (v instanceof Map) {
    out = `Map(${v.size}) { ${[...v].map(([k, x]) => `${inspect(k, depth - 1, seen)} => ${inspect(x, depth - 1, seen)}`).join(', ')} }`;
  } else if (v instanceof Set) {
    out = `Set(${v.size}) { ${[...v].map(x => inspect(x, depth - 1, seen)).join(', ')} }`;
  } else {
    const keys = Object.keys(v);
    const ctor = v.constructor && v.constructor.name && v.constructor.name !== 'Object' ? v.constructor.name + ' ' : '';
    out = keys.length ? `${ctor}{ ${keys.map(k => `${/^[A-Za-z_$][\w$]*$/.test(k) ? k : JSON.stringify(k)}: ${inspect(v[k], depth - 1, seen)}`).join(', ')} }` : `${ctor}{}`;
  }
  seen.delete(v);
  return out;
}
function formatArgs(args) {
  if (typeof args[0] === 'string' && /%[sdifoOjc%]/.test(args[0]) && args.length > 1) {
    let i = 1;
    const head = args[0].replace(/%([sdifoOjc%])/g, (m, c) => {
      if (c === '%') return '%';
      if (i >= args.length) return m;
      const a = args[i++];
      switch (c) {
        case 's': return typeof a === 'string' ? a : inspect(a, 0);
        case 'd': case 'i': return String(c === 'i' ? parseInt(a, 10) : Number(a));
        case 'f': return String(parseFloat(a));
        case 'c': return '';
        default: return inspect(a, 2);
      }
    });
    return [head, ...args.slice(i).map(a => inspect(a))].join(' ');
  }
  return args.map(a => inspect(a)).join(' ');
}
const counts = new Map(), timersC = new Map(); let groupIndent = '';
const emit = (level, args) => N('console', level, groupIndent + formatArgs(args));
const consoleObj = {
  log: (...a) => emit('log', a), info: (...a) => emit('info', a), debug: (...a) => emit('debug', a),
  warn: (...a) => emit('warn', a), error: (...a) => emit('error', a),
  trace: (...a) => emit('trace', [...a, '\n' + (new Error().stack || '')]),
  assert: (c, ...a) => { if (!c) emit('error', ['Assertion failed' + (a.length ? ':' : ''), ...a]); },
  dir: (o) => emit('log', [o]), dirxml: (o) => emit('log', [o]),
  table: (d) => emit('log', [d]),
  count: (l = 'default') => { counts.set(l, (counts.get(l) || 0) + 1); emit('log', [`${l}: ${counts.get(l)}`]); },
  countReset: (l = 'default') => counts.delete(l),
  group: (...a) => { if (a.length) emit('log', a); groupIndent += '  '; }, groupCollapsed: (...a) => consoleObj.group(...a),
  groupEnd: () => { groupIndent = groupIndent.slice(2); },
  time: (l = 'default') => timersC.set(l, performance.now()),
  timeEnd: (l = 'default') => { const t = timersC.get(l); if (t !== undefined) { emit('log', [`${l}: ${(performance.now() - t).toFixed(3)}ms`]); timersC.delete(l); } },
  timeLog: (l = 'default', ...a) => { const t = timersC.get(l); if (t !== undefined) emit('log', [`${l}: ${(performance.now() - t).toFixed(3)}ms`, ...a]); },
  clear: () => {},
};

// ----- time, timers, animation frames ---------------------------------------------------------

const performanceObj = {
  now: () => N('now'), timeOrigin: Date.now(),
  mark(name) { marks.set(name, N('now')); return { name, startTime: marks.get(name), entryType: 'mark', duration: 0 }; },
  measure(name, a, b) { const s = marks.get(a) ?? 0, e = marks.get(b) ?? N('now'); const m = { name, entryType: 'measure', startTime: s, duration: e - s }; measures.push(m); return m; },
  getEntriesByName(n) { return measures.filter(m => m.name === n); }, getEntries() { return measures.slice(); }, clearMarks() { marks.clear(); }, clearMeasures() { measures.length = 0; },
  toJSON() { return { timeOrigin: this.timeOrigin }; },
};
const marks = new Map(), measures = [];

let timerSeq = 1; const timerCbs = new Map();
function addTimer(fn, delay, args, repeat) {
  if (typeof fn !== 'function') throw new TypeError('Timer callback must be a function');
  const id = timerSeq++;
  timerCbs.set(id, { fn, args, repeat });
  N('timer_set', id, Math.max(0, +delay || 0), repeat);
  return id;
}
const setTimeout = (fn, delay, ...args) => addTimer(fn, delay, args, false);
const setInterval = (fn, delay, ...args) => addTimer(fn, delay, args, true);
const clearTimeout = id => { if (timerCbs.delete(id)) N('timer_clear', id); };
const setImmediate = (fn, ...args) => addTimer(fn, 0, args, false);

let rafSeq = 1; let rafQueue = new Map();
const requestAnimationFrame = cb => { if (typeof cb !== 'function') throw new TypeError('requestAnimationFrame callback must be a function'); const id = rafSeq++; rafQueue.set(id, cb); N('raf_request'); return id; };
const cancelAnimationFrame = id => { rafQueue.delete(id); };
const requestIdleCallback = cb => setTimeout(() => cb({ didTimeout: false, timeRemaining: () => 10 }), 1);
const queueMicrotask = fn => { Promise.resolve().then(() => { try { fn(); } catch (e) { reportError(e); } }); };

// ----- encoding -------------------------------------------------------------------------------

class TextEncoder {
  get encoding() { return 'utf-8'; }
  encode(str = '') {
    str = String(str); const out = [];
    for (let i = 0; i < str.length; i++) {
      let c = str.codePointAt(i);
      if (c > 0xffff) i++;
      if (c >= 0xd800 && c <= 0xdfff) c = 0xfffd;
      if (c < 0x80) out.push(c);
      else if (c < 0x800) out.push(0xc0 | c >> 6, 0x80 | c & 63);
      else if (c < 0x10000) out.push(0xe0 | c >> 12, 0x80 | c >> 6 & 63, 0x80 | c & 63);
      else out.push(0xf0 | c >> 18, 0x80 | c >> 12 & 63, 0x80 | c >> 6 & 63, 0x80 | c & 63);
    }
    return new Uint8Array(out);
  }
  encodeInto(str, dest) { const b = this.encode(str); const n = Math.min(b.length, dest.length); dest.set(b.subarray(0, n)); return { read: str.length, written: n }; }
}
class TextDecoder {
  constructor(label = 'utf-8', opts = {}) { this.encoding = String(label).toLowerCase().replace('utf8', 'utf-8'); this.fatal = !!opts.fatal; this.ignoreBOM = !!opts.ignoreBOM; }
  decode(buf) {
    if (buf === undefined) return '';
    const b = buf instanceof ArrayBuffer ? new Uint8Array(buf) : new Uint8Array(buf.buffer, buf.byteOffset, buf.byteLength);
    if (this.encoding === 'latin1' || this.encoding === 'iso-8859-1' || this.encoding === 'ascii') { let s = ''; for (const x of b) s += String.fromCharCode(x); return s; }
    let s = '', i = 0;
    if (!this.ignoreBOM && b[0] === 0xef && b[1] === 0xbb && b[2] === 0xbf) i = 3;
    while (i < b.length) {
      const c = b[i++]; let cp;
      if (c < 0x80) cp = c;
      else if (c >= 0xc2 && c < 0xe0 && (b[i] & 0xc0) === 0x80) cp = (c & 31) << 6 | b[i++] & 63;
      else if (c >= 0xe0 && c < 0xf0 && (b[i] & 0xc0) === 0x80 && (b[i + 1] & 0xc0) === 0x80) { cp = (c & 15) << 12 | (b[i] & 63) << 6 | b[i + 1] & 63; i += 2; }
      else if (c >= 0xf0 && c < 0xf5 && (b[i] & 0xc0) === 0x80 && (b[i + 1] & 0xc0) === 0x80 && (b[i + 2] & 0xc0) === 0x80) { cp = (c & 7) << 18 | (b[i] & 63) << 12 | (b[i + 1] & 63) << 6 | b[i + 2] & 63; i += 3; }
      else { if (this.fatal) throw new TypeError('The encoded data was not valid utf-8'); cp = 0xfffd; }
      s += String.fromCodePoint(cp);
    }
    return s;
  }
}
const B64 = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/';
function bytesToB64(b) {
  let s = '';
  for (let i = 0; i < b.length; i += 3) {
    const n = b[i] << 16 | (b[i + 1] || 0) << 8 | (b[i + 2] || 0);
    s += B64[n >> 18 & 63] + B64[n >> 12 & 63] + (i + 1 < b.length ? B64[n >> 6 & 63] : '=') + (i + 2 < b.length ? B64[n & 63] : '=');
  }
  return s;
}
function b64ToBytes(s) {
  s = String(s).replace(/[\s=]/g, '').replace(/-/g, '+').replace(/_/g, '/');
  const out = []; let acc = 0, bits = 0;
  for (const ch of s) { const v = B64.indexOf(ch); if (v < 0) throw new DOMException('The string to be decoded is not correctly encoded.', 'InvalidCharacterError'); acc = acc << 6 | v; bits += 6; if (bits >= 8) { bits -= 8; out.push(acc >> bits & 255); acc &= (1 << bits) - 1; } }
  return new Uint8Array(out);
}
const btoa = s => { s = String(s); const b = new Uint8Array(s.length); for (let i = 0; i < s.length; i++) { if (s.charCodeAt(i) > 255) throw new DOMException('Invalid character', 'InvalidCharacterError'); b[i] = s.charCodeAt(i); } return bytesToB64(b); };
const atob = s => { let o = ''; for (const x of b64ToBytes(s)) o += String.fromCharCode(x); return o; };

// ----- URL ------------------------------------------------------------------------------------

const encodeForm = s => encodeURIComponent(s).replace(/%20/g, '+').replace(/[!'()~]/g, c => '%' + c.charCodeAt(0).toString(16).toUpperCase());
const decodeForm = s => { try { return decodeURIComponent(s.replace(/\+/g, ' ')); } catch (_) { return s; } };
class URLSearchParams {
  constructor(init = '') {
    this._list = []; this._url = null;
    if (typeof init === 'string') this._parse(init);
    else if (init instanceof URLSearchParams) this._list = init._list.map(p => p.slice());
    else if (Array.isArray(init)) for (const [k, v] of init) this._list.push([String(k), String(v)]);
    else if (init && typeof init === 'object') for (const k of Object.keys(init)) this._list.push([k, String(init[k])]);
  }
  _parse(s) { this._list = []; s = s.replace(/^\?/, ''); if (!s) return; for (const part of s.split('&')) { if (!part) continue; const i = part.indexOf('='); this._list.push(i < 0 ? [decodeForm(part), ''] : [decodeForm(part.slice(0, i)), decodeForm(part.slice(i + 1))]); } }
  _update() { if (this._url) this._url._search = this._list.length ? '?' + this.toString() : ''; }
  get size() { return this._list.length; }
  append(k, v) { this._list.push([String(k), String(v)]); this._update(); }
  delete(k) { this._list = this._list.filter(p => p[0] !== String(k)); this._update(); }
  get(k) { const p = this._list.find(p => p[0] === String(k)); return p ? p[1] : null; }
  getAll(k) { return this._list.filter(p => p[0] === String(k)).map(p => p[1]); }
  has(k) { return this._list.some(p => p[0] === String(k)); }
  set(k, v) { k = String(k); const i = this._list.findIndex(p => p[0] === k); if (i < 0) this._list.push([k, String(v)]); else { this._list[i][1] = String(v); this._list = this._list.filter((p, j) => j <= i || p[0] !== k); } this._update(); }
  sort() { this._list.sort((a, b) => a[0] < b[0] ? -1 : a[0] > b[0] ? 1 : 0); this._update(); }
  forEach(fn, t) { for (const [k, v] of this._list) fn.call(t, v, k, this); }
  entries() { return this._list.map(p => p.slice())[Symbol.iterator](); }
  keys() { return this._list.map(p => p[0])[Symbol.iterator](); }
  values() { return this._list.map(p => p[1])[Symbol.iterator](); }
  [Symbol.iterator]() { return this.entries(); }
  toString() { return this._list.map(([k, v]) => encodeForm(k) + '=' + encodeForm(v)).join('&'); }
}
const DEFAULT_PORTS = { 'http:': '80', 'https:': '443', 'ws:': '80', 'wss:': '443', 'ftp:': '21' };
function normalizePath(p) {
  const out = [];
  for (const seg of p.split('/')) { if (seg === '..') { if (out.length > 1) out.pop(); } else if (seg !== '.') out.push(seg); }
  let r = out.join('/'); if (/\/\.\.?$/.test(p) && !r.endsWith('/')) r += '/';
  return r.startsWith('/') ? r : '/' + r;
}
class URL {
  constructor(url, base) {
    url = String(url).trim();
    let m = /^([a-zA-Z][a-zA-Z0-9+.-]*:)(\/\/([^/?#]*))?([^?#]*)(\?[^#]*)?(#.*)?$/.exec(url);
    if (!m) {
      if (base === undefined) throw new TypeError(`Invalid URL: ${url}`);
      const b = new URL(base);
      if (b._opaque) throw new TypeError(`Invalid URL: ${url}`);
      let path, search = '', hash = '';
      let rest = url; const hi = rest.indexOf('#'); if (hi >= 0) { hash = rest.slice(hi); rest = rest.slice(0, hi); }
      const qi = rest.indexOf('?'); if (qi >= 0) { search = rest.slice(qi); rest = rest.slice(0, qi); }
      if (rest.startsWith('//')) return new URL(b.protocol + url);
      if (rest === '') { path = b._pathname; if (qi < 0) search = b._search; }
      else if (rest.startsWith('/')) path = rest;
      else path = b._pathname.replace(/[^/]*$/, '') + rest;
      Object.assign(this, { _protocol: b._protocol, _username: b._username, _password: b._password, _hostname: b._hostname, _port: b._port, _pathname: normalizePath(path), _search: search === '?' ? '' : search, _hash: hash === '#' ? '' : hash, _opaque: false });
    } else {
      const [, proto, , authority, path, search, hash] = m;
      const p = proto.toLowerCase();
      if (authority === undefined) { Object.assign(this, { _protocol: p, _username: '', _password: '', _hostname: '', _port: '', _pathname: path, _search: search || '', _hash: hash || '', _opaque: true }); }
      else {
        let userinfo = '', host = authority; const at = authority.lastIndexOf('@');
        if (at >= 0) { userinfo = authority.slice(0, at); host = authority.slice(at + 1); }
        const [u, pw = ''] = userinfo.split(':');
        let hostname = host, port = ''; const pm = /^(.*?):(\d*)$/.exec(host); if (pm && !host.endsWith(']')) { hostname = pm[1]; port = pm[2]; }
        if (port === DEFAULT_PORTS[p]) port = '';
        Object.assign(this, { _protocol: p, _username: u || '', _password: pw, _hostname: hostname.toLowerCase(), _port: port, _pathname: normalizePath(path || '/'), _search: search === '?' ? '' : (search || ''), _hash: hash === '#' ? '' : (hash || ''), _opaque: false });
      }
    }
    this._params = new URLSearchParams(this._search); this._params._url = this;
  }
  static canParse(u, b) { try { new URL(u, b); return true; } catch (_) { return false; } }
  get protocol() { return this._protocol; } set protocol(v) { this._protocol = String(v).replace(/:?$/, ':'); }
  get username() { return this._username; } get password() { return this._password; }
  get hostname() { return this._hostname; } set hostname(v) { this._hostname = String(v); }
  get port() { return this._port; } set port(v) { this._port = String(v); }
  get host() { return this._hostname + (this._port ? ':' + this._port : ''); }
  get origin() { return this._opaque || !this._hostname ? 'null' : this._protocol + '//' + this.host; }
  get pathname() { return this._pathname; } set pathname(v) { this._pathname = normalizePath(String(v)); }
  get search() { return this._search; } set search(v) { v = String(v); this._search = v && v !== '?' ? (v[0] === '?' ? v : '?' + v) : ''; this._params._parse(this._search); }
  get hash() { return this._hash; } set hash(v) { v = String(v); this._hash = v && v !== '#' ? (v[0] === '#' ? v : '#' + v) : ''; }
  get searchParams() { return this._params; }
  get href() {
    if (this._opaque) return this._protocol + this._pathname + this._search + this._hash;
    const auth = this._username ? this._username + (this._password ? ':' + this._password : '') + '@' : '';
    return this._protocol + '//' + auth + this.host + this._pathname + this._search + this._hash;
  }
  set href(v) { Object.assign(this, new URL(v)); }
  toString() { return this.href; } toJSON() { return this.href; }
}

// ----- AbortController ------------------------------------------------------------------------

class AbortSignal extends EventTarget {
  constructor() { super(); this.aborted = false; this.reason = undefined; }
  throwIfAborted() { if (this.aborted) throw this.reason; }
  static abort(reason) { const s = new AbortSignal(); s.aborted = true; s.reason = reason === undefined ? new DOMException('This operation was aborted', 'AbortError') : reason; return s; }
  static timeout(ms) { const s = new AbortSignal(); setTimeout(() => s._abort(new DOMException('The operation timed out.', 'TimeoutError')), ms); return s; }
  static any(signals) { const s = new AbortSignal(); for (const x of signals) { if (x.aborted) { s._abort(x.reason); break; } x.addEventListener('abort', () => s._abort(x.reason), { once: true }); } return s; }
  _abort(reason) { if (this.aborted) return; this.aborted = true; this.reason = reason === undefined ? new DOMException('This operation was aborted', 'AbortError') : reason; this.dispatchEvent(new Event('abort')); }
}
class AbortController {
  constructor() { this.signal = new AbortSignal(); }
  abort(reason) { this.signal._abort(reason); }
}

// ----- fetch ----------------------------------------------------------------------------------

class Headers {
  constructor(init) { this._m = new Map(); if (init instanceof Headers) init.forEach((v, k) => this.append(k, v)); else if (Array.isArray(init)) for (const [k, v] of init) this.append(k, v); else if (init) for (const k of Object.keys(init)) this.append(k, init[k]); }
  append(k, v) { k = String(k).toLowerCase(); this._m.set(k, this._m.has(k) ? this._m.get(k) + ', ' + v : String(v)); }
  set(k, v) { this._m.set(String(k).toLowerCase(), String(v)); }
  get(k) { const v = this._m.get(String(k).toLowerCase()); return v === undefined ? null : v; }
  has(k) { return this._m.has(String(k).toLowerCase()); }
  delete(k) { this._m.delete(String(k).toLowerCase()); }
  forEach(fn, t) { for (const [k, v] of this._m) fn.call(t, v, k, this); }
  entries() { return [...this._m].sort((a, b) => a[0] < b[0] ? -1 : 1)[Symbol.iterator](); }
  keys() { return [...this._m.keys()].sort()[Symbol.iterator](); }
  values() { return [...this.entries()].map(e => e[1])[Symbol.iterator](); }
  [Symbol.iterator]() { return this.entries(); }
}
function bodyToBytes(body) {
  if (body === undefined || body === null) return null;
  if (typeof body === 'string') return new TextEncoder().encode(body);
  if (body instanceof Uint8Array) return body;
  if (body instanceof ArrayBuffer) return new Uint8Array(body);
  if (ArrayBuffer.isView(body)) return new Uint8Array(body.buffer, body.byteOffset, body.byteLength);
  if (body instanceof URLSearchParams) return new TextEncoder().encode(body.toString());
  if (body instanceof Blob) return body._bytes;
  return new TextEncoder().encode(String(body));
}
class Body {
  _initBody(bytes) { this._bytes = bytes; this.bodyUsed = false; }
  _consume() { if (this.bodyUsed) return Promise.reject(new TypeError('Body has already been consumed')); this.bodyUsed = true; return Promise.resolve(this._bytes || new Uint8Array(0)); }
  text() { return this._consume().then(b => new TextDecoder().decode(b)); }
  json() { return this.text().then(t => JSON.parse(t)); }
  arrayBuffer() { return this._consume().then(b => b.buffer.slice(b.byteOffset, b.byteOffset + b.byteLength)); }
  bytes() { return this._consume(); }
  blob() { return this._consume().then(b => new Blob([b], { type: this.headers.get('content-type') || '' })); }
  get body() { return null; }  // ReadableStream bodies are not implemented; use text()/json()/arrayBuffer().
}
class Request extends Body {
  constructor(input, init = {}) {
    super();
    const src = input instanceof Request ? input : null;
    this.url = new URL(src ? src.url : String(input), location.href).href;
    this.method = String(init.method || (src && src.method) || 'GET').toUpperCase();
    this.headers = new Headers(init.headers || (src && src.headers));
    this.signal = init.signal || (src && src.signal) || new AbortSignal();
    this.mode = init.mode || 'cors'; this.credentials = init.credentials || 'same-origin'; this.cache = init.cache || 'default'; this.redirect = init.redirect || 'follow';
    const b = init.body !== undefined ? init.body : (src ? src._bytes : null);
    if ((this.method === 'GET' || this.method === 'HEAD') && b != null) throw new TypeError('Request with GET/HEAD method cannot have body.');
    this._initBody(bodyToBytes(b));
    if (typeof init.body === 'string' && !this.headers.has('content-type')) this.headers.set('content-type', 'text/plain;charset=UTF-8');
    if (init.body instanceof URLSearchParams && !this.headers.has('content-type')) this.headers.set('content-type', 'application/x-www-form-urlencoded;charset=UTF-8');
  }
  clone() { return new Request(this); }
}
class Response extends Body {
  constructor(body = null, init = {}) {
    super();
    this.status = init.status === undefined ? 200 : init.status; this.statusText = init.statusText || '';
    this.headers = new Headers(init.headers); this.url = init.url || ''; this.type = 'basic'; this.redirected = false;
    this._initBody(bodyToBytes(body));
    if (typeof body === 'string' && !this.headers.has('content-type')) this.headers.set('content-type', 'text/plain;charset=UTF-8');
  }
  get ok() { return this.status >= 200 && this.status < 300; }
  clone() { const r = new Response(this._bytes, { status: this.status, statusText: this.statusText, headers: this.headers, url: this.url }); return r; }
  static json(data, init = {}) { const h = new Headers(init.headers); if (!h.has('content-type')) h.set('content-type', 'application/json'); return new Response(JSON.stringify(data), { ...init, headers: h }); }
  static error() { const r = new Response(null, { status: 0 }); r.type = 'error'; return r; }
}
class Blob {
  constructor(parts = [], opts = {}) {
    const bufs = parts.map(bodyToBytes); const n = bufs.reduce((a, b) => a + b.length, 0);
    this._bytes = new Uint8Array(n); let o = 0; for (const b of bufs) { this._bytes.set(b, o); o += b.length; }
    this.type = opts.type || '';
  }
  get size() { return this._bytes.length; }
  text() { return Promise.resolve(new TextDecoder().decode(this._bytes)); }
  arrayBuffer() { return Promise.resolve(this._bytes.buffer.slice(0)); }
  bytes() { return Promise.resolve(this._bytes.slice()); }
  slice(s = 0, e = this.size, type = '') { const b = new Blob([], { type }); b._bytes = this._bytes.slice(s, e); return b; }
}
let fetchSeq = 1; const pendingFetches = new Map();
function fetch(input, init) {
  return new Promise((resolve, reject) => {
    let req;
    try { req = new Request(input, init); } catch (e) { reject(e); return; }
    if (req.signal.aborted) { reject(req.signal.reason); return; }
    const id = fetchSeq++;
    pendingFetches.set(id, { resolve, reject, req });
    req.signal.addEventListener('abort', () => { if (pendingFetches.delete(id)) reject(req.signal.reason); }, { once: true });
    const flat = []; req.headers.forEach((v, k) => flat.push(k, v));
    let url = req.url; if (url.startsWith('rendrix://app/')) url = url.slice('rendrix://app/'.length).replace(/[?#].*$/, '');
    N('fetch_start', id, url, req.method, flat, req._bytes ? bytesToB64(req._bytes) : null);
  });
}
function fetchDone(id, status, statusText, headersFlat, bodyB64, err) {
  const f = pendingFetches.get(id); if (!f) return; pendingFetches.delete(id);
  if (err) { f.reject(new TypeError('Failed to fetch: ' + err)); return; }
  const h = new Headers(); for (let i = 0; i < headersFlat.length; i += 2) h.append(headersFlat[i], headersFlat[i + 1]);
  f.resolve(new Response(b64ToBytes(bodyB64 || ''), { status, statusText, headers: h, url: f.req.url }));
}

// ----- storage --------------------------------------------------------------------------------

class Storage {
  constructor(area) { Object.defineProperty(this, '_area', { value: area, configurable: true }); }
  get length() { return N('storage_keys', this._area).length; }
  key(i) { const k = N('storage_keys', this._area); return i < k.length ? k[i] : null; }
  getItem(k) { return N('storage_get', this._area, String(k)); }
  setItem(k, v) { N('storage_set', this._area, String(k), String(v)); }
  removeItem(k) { N('storage_remove', this._area, String(k)); }
  clear() { N('storage_clear', this._area); }
}
function makeStorage(area) {
  const s = new Storage(area);
  return new Proxy(s, {
    get(t, k) { if (typeof k === 'symbol' || k in t) return Reflect.get(t, k, t); const v = t.getItem(k); return v === null ? undefined : v; },
    set(t, k, v) { t.setItem(k, v); return true; },
    deleteProperty(t, k) { t.removeItem(k); return true; },
    has(t, k) { return k in t || (typeof k === 'string' && t.getItem(k) !== null); },
    ownKeys(t) { return N('storage_keys', t._area); },
    getOwnPropertyDescriptor(t, k) { const v = t.getItem(String(k)); return v === null ? undefined : { value: v, enumerable: true, configurable: true, writable: true }; },
  });
}

// ----- observers ------------------------------------------------------------------------------

const mutationObservers = new Set();
class MutationObserver {
  constructor(cb) { this._cb = cb; this._targets = []; this._queue = []; }
  observe(target, opts = {}) {
    if (!opts.childList && !opts.attributes && !opts.characterData && !(opts.attributeFilter || opts.attributeOldValue)) throw new TypeError("The options object must set at least one of 'attributes', 'characterData', or 'childList' to true.");
    const o = { ...opts }; if (o.attributeOldValue || o.attributeFilter) o.attributes = o.attributes !== false;
    if (o.characterDataOldValue) o.characterData = o.characterData !== false;
    this._targets = this._targets.filter(t => t.target !== target); this._targets.push({ target, opts: o });
    mutationObservers.add(this); N('observe_mutations', true);
  }
  disconnect() { this._targets = []; this._queue = []; mutationObservers.delete(this); if (!mutationObservers.size) N('observe_mutations', false); }
  takeRecords() { const q = this._queue; this._queue = []; return q; }
}
function deliverMutations() {
  if (!mutationObservers.size) return false;
  const raw = N('take_mutations'); if (!raw.length) return false;
  for (const [type, target, added, removed, prev, next, attr, old, ancestors] of raw) {
    for (const obs of mutationObservers) {
      for (const { target: t, opts } of obs._targets) {
        const direct = t[ID] === target;
        if (!direct && !(opts.subtree && ancestors.includes(t[ID]))) continue;
        if (type === 'childList' && !opts.childList) continue;
        if (type === 'attributes' && (!opts.attributes || (opts.attributeFilter && !opts.attributeFilter.includes(attr)))) continue;
        if (type === 'characterData' && !opts.characterData) continue;
        obs._queue.push({ type, target: wrap(target), addedNodes: NodeList.from(wrapAll(added)), removedNodes: NodeList.from(wrapAll(removed)), previousSibling: wrap(prev), nextSibling: wrap(next), attributeName: attr, attributeNamespace: null,
          oldValue: (type === 'attributes' && opts.attributeOldValue) || (type === 'characterData' && opts.characterDataOldValue) ? old : null });
        break;
      }
    }
  }
  for (const obs of [...mutationObservers]) { const q = obs.takeRecords(); if (q.length) { try { obs._cb.call(obs, q, obs); } catch (e) { reportError(e); } } }
  return true;
}

const resizeObservers = new Set();
class ResizeObserver {
  constructor(cb) { this._cb = cb; this._targets = new Map(); }
  observe(el) { this._targets.set(el, null); resizeObservers.add(this); N('raf_request'); }
  unobserve(el) { this._targets.delete(el); }
  disconnect() { this._targets.clear(); resizeObservers.delete(this); }
}
function checkResize() {
  for (const obs of [...resizeObservers]) {
    const entries = [];
    for (const [el, last] of obs._targets) {
      const s = N('layout_size', el[ID]); if (!s) continue;
      if (!last || last[0] !== s[0] || last[1] !== s[1]) {
        obs._targets.set(el, s);
        entries.push({ target: el, contentRect: new DOMRect(0, 0, s[0], s[1]), contentBoxSize: [{ inlineSize: s[0], blockSize: s[1] }], borderBoxSize: [{ inlineSize: s[0], blockSize: s[1] }], devicePixelContentBoxSize: [{ inlineSize: s[0], blockSize: s[1] }] });
      }
    }
    if (entries.length) { try { obs._cb.call(obs, entries, obs); } catch (e) { reportError(e); } }
  }
}

// ----- window / navigator / location ----------------------------------------------------------

const mediaLists = new Set();
class MediaQueryList extends EventTarget {
  constructor(q) { super(); this.media = q; this._m = N('media_matches', q); }
  get matches() { return this._m; }
  addListener(f) { this.addEventListener('change', f); } removeListener(f) { this.removeEventListener('change', f); }
}
const matchMedia = q => { const m = new MediaQueryList(String(q)); mediaLists.add(new WeakRef(m)); return m; };

class ComputedStyle {
  constructor(el) { Object.defineProperty(this, '_id', { value: el[ID] }); }
  getPropertyValue(n) { return N('computed', this._id, String(n)); }
  getPropertyPriority() { return ''; }
  get length() { return 0; }
}
const getComputedStyle = el => {
  if (!el || el[ID] === undefined) throw new TypeError('getComputedStyle requires an Element');
  return new Proxy(new ComputedStyle(el), {
    get(t, k) { if (typeof k === 'symbol' || k in t) return Reflect.get(t, k, t); return t.getPropertyValue(k === 'cssFloat' ? 'float' : kebab(k)); },
  });
};

const locationObj = {
  href: 'rendrix://app/index.html', origin: 'rendrix://app', protocol: 'rendrix:', host: 'app', hostname: 'app', port: '', pathname: '/index.html', search: '', hash: '',
  assign() {}, replace() {}, reload() {}, toString() { return this.href; },
};
const historyObj = { length: 1, state: null, scrollRestoration: 'auto',
  pushState(s, t, url) { this.state = s; if (url) updateLocation(url); }, replaceState(s, t, url) { this.state = s; if (url) updateLocation(url); },
  back() {}, forward() {}, go() {} };
function updateLocation(url) {
  try { const u = new URL(url, locationObj.href); locationObj.href = u.href; locationObj.pathname = u.pathname; locationObj.search = u.search; locationObj.hash = u.hash; } catch (_) {}
}

const navigatorObj = {
  userAgent: 'Mozilla/5.0 (Rendrix) Rendrix/0.5', appName: 'Rendrix', appVersion: '0.5', vendor: 'Rendrix', platform: 'Rendrix', product: 'Gecko',
  language: 'en-US', languages: ['en-US', 'en'], onLine: true, cookieEnabled: false, hardwareConcurrency: 4, maxTouchPoints: 0, webdriver: false,
  clipboard: { writeText: t => { N('clipboard_write', String(t)); return Promise.resolve(); }, readText: () => Promise.resolve(N('clipboard_read')) },
};

const cryptoObj = {
  getRandomValues(arr) { const bytes = new Uint8Array(arr.buffer, arr.byteOffset, arr.byteLength); for (let i = 0; i < bytes.length; i++) bytes[i] = Math.floor(N('random') * 256); return arr; },
  randomUUID() { const b = new Uint8Array(16); this.getRandomValues(b); b[6] = b[6] & 15 | 64; b[8] = b[8] & 63 | 128; const h = [...b].map(x => x.toString(16).padStart(2, '0')).join(''); return `${h.slice(0, 8)}-${h.slice(8, 12)}-${h.slice(12, 16)}-${h.slice(16, 20)}-${h.slice(20)}`; },
};

function structuredCloneImpl(v, seen = new Map()) {
  if (v === null || typeof v !== 'object') { if (typeof v === 'function' || typeof v === 'symbol') throw new DOMException('The object could not be cloned.', 'DataCloneError'); return v; }
  if (seen.has(v)) return seen.get(v);
  let out;
  if (v instanceof Date) return new Date(v.getTime());
  if (v instanceof RegExp) return new RegExp(v.source, v.flags);
  if (v instanceof Map) { out = new Map(); seen.set(v, out); for (const [k, x] of v) out.set(structuredCloneImpl(k, seen), structuredCloneImpl(x, seen)); return out; }
  if (v instanceof Set) { out = new Set(); seen.set(v, out); for (const x of v) out.add(structuredCloneImpl(x, seen)); return out; }
  if (ArrayBuffer.isView(v)) return new v.constructor(v);
  if (v instanceof ArrayBuffer) return v.slice(0);
  if (v instanceof Error) { const e = new v.constructor(v.message); e.stack = v.stack; return e; }
  if (v instanceof Node) throw new DOMException('Nodes cannot be cloned.', 'DataCloneError');
  out = Array.isArray(v) ? [] : {}; seen.set(v, out);
  for (const k of Object.keys(v)) out[k] = structuredCloneImpl(v[k], seen);
  return out;
}

// ----- lifecycle & native entry points --------------------------------------------------------

function setFocus(el) {
  const prev = N('focused'); const next = el ? el[ID] : null;
  if (prev === next) return;
  if (el && !N('is_focusable', next)) return;
  const prevEl = wrap(prev);
  if (prevEl) { N('blur', prev); prevEl.dispatchEvent(new FocusEvent('blur', { relatedTarget: el })); prevEl.dispatchEvent(new FocusEvent('focusout', { bubbles: true, relatedTarget: el })); }
  if (el) { N('focus', next); el.dispatchEvent(new FocusEvent('focus', { relatedTarget: prevEl })); el.dispatchEvent(new FocusEvent('focusin', { bubbles: true, relatedTarget: prevEl })); }
}

function fireInputEvents(el, inputType = 'insertText', data = null) {
  el.dispatchEvent(new InputEvent('input', { bubbles: true, data, inputType }));
}

// Click with activation behaviour (checkbox/radio toggling, labels, form submission).
function dispatchClick(el, init) {
  const type = el.type;
  const isCheck = el.localName === 'input' && type === 'checkbox';
  const isRadio = el.localName === 'input' && type === 'radio';
  let prevChecked, prevRadios;
  if (isCheck) { prevChecked = el.checked; el.checked = !prevChecked; }
  if (isRadio) {
    prevChecked = el.checked;
    prevRadios = radioGroup(el).filter(r => r !== el && r.checked);
    for (const r of prevRadios) r.checked = false;
    el.checked = true;
  }
  const ev = new PointerEvent('click', { bubbles: true, cancelable: true, composed: true, detail: 1, ...init });
  ev.isTrusted = !!init.trusted;
  const ok = el.dispatchEvent(ev);
  if (!ok) {
    if (isCheck) el.checked = prevChecked;
    if (isRadio) { el.checked = prevChecked; for (const r of prevRadios) r.checked = true; }
    return false;
  }
  if (isCheck || (isRadio && !prevChecked)) { fireInputEvents(el, 'insertReplacementText'); el.dispatchEvent(new Event('change', { bubbles: true })); }
  else if (el.localName === 'button' || (el.localName === 'input' && (type === 'submit' || type === 'reset' || type === 'button'))) {
    const form = el.closest('form'); const btnType = el.localName === 'button' ? el.type : type;
    if (form && btnType === 'submit') form.requestSubmit(el); else if (form && btnType === 'reset') form.reset();
  }
  return true;
}
function radioGroup(r) {
  const name = r.getAttribute('name'); if (!name) return [r];
  const scope = r.closest('form') || document;
  return [...scope.querySelectorAll('input[type=radio]')].filter(x => x.getAttribute('name') === name);
}
// A click bubbling up from inside a <label> activates its control.
function labelActivation(ev) {
  if (ev.defaultPrevented || ev.target._labelForwarded) return;
  const label = ev.target.closest && ev.target.closest('label');
  if (!label || ev.target.closest('input, button, select, textarea')) return;
  const ctl = label.control;
  if (ctl && !ctl.disabled) { if (ctl.localName === 'input' && (ctl.type === 'checkbox' || ctl.type === 'radio')) dispatchClick(ctl, {}); else ctl.focus(); }
}
document.addEventListener('click', labelActivation);

const EVENT_CLASSES = {
  click: PointerEvent, dblclick: MouseEvent, auxclick: PointerEvent, contextmenu: PointerEvent, mousedown: MouseEvent, mouseup: MouseEvent, mousemove: MouseEvent, mouseover: MouseEvent, mouseout: MouseEvent, mouseenter: MouseEvent, mouseleave: MouseEvent,
  pointerdown: PointerEvent, pointerup: PointerEvent, pointermove: PointerEvent, pointerover: PointerEvent, pointerout: PointerEvent, pointerenter: PointerEvent, pointerleave: PointerEvent, pointercancel: PointerEvent,
  wheel: WheelEvent, keydown: KeyboardEvent, keyup: KeyboardEvent, keypress: KeyboardEvent, input: InputEvent, beforeinput: InputEvent, focus: FocusEvent, blur: FocusEvent, focusin: FocusEvent, focusout: FocusEvent,
};
const BUBBLES = new Set(['click', 'dblclick', 'auxclick', 'contextmenu', 'mousedown', 'mouseup', 'mousemove', 'mouseover', 'mouseout', 'pointerdown', 'pointerup', 'pointermove', 'pointerover', 'pointerout', 'wheel', 'keydown', 'keyup', 'keypress', 'input', 'beforeinput', 'focusin', 'focusout']);
const NON_CANCELABLE = new Set(['mouseenter', 'mouseleave', 'pointerenter', 'pointerleave', 'focus', 'blur', 'focusin', 'focusout', 'input']);

// Called by the platform layer: dispatch a trusted event; returns true if default is NOT prevented.
function rendrixDispatch(type, targetId, initJson) {
  const init = initJson ? JSON.parse(initJson) : {};
  const target = targetId === null || targetId === undefined || targetId < 0 ? windowTarget : wrap(targetId);
  if (!target) return true;
  if (type === 'click' && target instanceof Element) return dispatchClick(target, { ...init, trusted: true });
  const Cls = EVENT_CLASSES[type] || Event;
  const ev = new Cls(type, { bubbles: BUBBLES.has(type), cancelable: !NON_CANCELABLE.has(type), composed: true, ...init });
  if (init.relatedId !== undefined && init.relatedId !== null) ev.relatedTarget = wrap(init.relatedId);
  ev.isTrusted = true;
  return target.dispatchEvent(ev);
}

function afterTask() {
  upgradePending();
  let again = true, guard = 0;
  while (again && guard++ < 8) { again = deliverMutations(); }
  reconcileStrong();
  nodesFreed();
}

function viewportChanged() {
  windowTarget.dispatchEvent(new Event('resize'));
  for (const ref of [...mediaLists]) {
    const m = ref.deref(); if (!m) { mediaLists.delete(ref); continue; }
    const now = N('media_matches', m.media);
    if (now !== m._m) { m._m = now; const ev = new Event('change'); ev.matches = now; ev.media = m.media; m.dispatchEvent(ev); }
  }
}

function boot() {
  for (const img of document.querySelectorAll('img[src]')) scheduleImageEvents(img);
  readyState = 'interactive';
  document.dispatchEvent(new Event('readystatechange'));
  document.dispatchEvent(new Event('DOMContentLoaded', { bubbles: true }));
  readyState = 'complete';
  document.dispatchEvent(new Event('readystatechange'));
  windowTarget.dispatchEvent(new Event('load'));
}

function runTimer(id) {
  const t = timerCbs.get(id); if (!t) return;
  if (!t.repeat) timerCbs.delete(id);
  try { t.fn(...t.args); } catch (e) { reportError(e); }
}
function runRaf(ts) {
  const q = rafQueue; rafQueue = new Map();
  for (const [, cb] of q) { try { cb(ts); } catch (e) { reportError(e); } }
  checkResize();
  return rafQueue.size > 0;
}

function unhandledRejection(message) {
  const ev = new PromiseRejectionEvent('unhandledrejection', { cancelable: true, reason: message });
  if (windowTarget.dispatchEvent(ev)) N('console', 'error', 'Uncaught (in promise) ' + message);
}

// ----- install globals ------------------------------------------------------------------------

const define = (name, value) => Object.defineProperty(global, name, { value, writable: true, configurable: true, enumerable: false });
for (const [k, v] of Object.entries({
  EventTarget, Event, CustomEvent, UIEvent, FocusEvent, MouseEvent, PointerEvent, WheelEvent, KeyboardEvent, InputEvent, SubmitEvent, ErrorEvent, MessageEvent, PromiseRejectionEvent, ProgressEvent, DOMException,
  Node, CharacterData, Text, Comment, DocumentFragment, Element, HTMLElement, Document, NodeList, HTMLCollection, DOMRect, DOMTokenList, CSSStyleDeclaration,
  HTMLInputElement, HTMLTextAreaElement, HTMLSelectElement, HTMLOptionElement, HTMLButtonElement, HTMLFormElement, HTMLAnchorElement, HTMLLabelElement, HTMLImageElement, HTMLCanvasElement, HTMLStyleElement, HTMLLinkElement, HTMLScriptElement,
  MutationObserver, ResizeObserver, MediaQueryList,
  URL, URLSearchParams, TextEncoder, TextDecoder, AbortController, AbortSignal, Headers, Request, Response, Blob, Storage,
  setTimeout, setInterval, clearTimeout, clearInterval: clearTimeout, setImmediate, clearImmediate: clearTimeout, requestAnimationFrame, cancelAnimationFrame, requestIdleCallback, cancelIdleCallback: clearTimeout, queueMicrotask,
  fetch, atob, btoa, getComputedStyle, matchMedia, reportError,
  structuredClone: v => structuredCloneImpl(v),
  document, console: consoleObj, performance: performanceObj, location: locationObj, history: historyObj, navigator: navigatorObj, crypto: cryptoObj,
  localStorage: makeStorage('local'), sessionStorage: makeStorage('session'),
  window: global, self: global, globalThis: global, top: global, parent: global, frames: global,
  customElements: { define() { throw new DOMException('Custom elements are not supported', 'NotSupportedError'); }, get() { return undefined; }, whenDefined() { return new Promise(() => {}); } },
  alert: m => N('console', 'warn', 'alert: ' + m), confirm: () => false, prompt: () => null,
  Image: class Image { constructor(w, h) { const i = document.createElement('img'); if (w !== undefined) i.width = w; if (h !== undefined) i.height = h; return i; } },
  Option: function Option(text = '', value, defaultSelected, selected) { const o = document.createElement('option'); o.textContent = text; if (value !== undefined) o.value = value; if (defaultSelected || selected) o.setAttribute('selected', ''); return o; },
  CSS: { supports: () => true, escape: s => String(s).replace(/[^\w-]/g, c => '\\' + c) },
})) define(k, v);

// Window-level geometry and scroll.
const root = () => document.documentElement;
Object.defineProperties(global, {
  innerWidth: { get: () => N('viewport')[0], configurable: true }, innerHeight: { get: () => N('viewport')[1], configurable: true },
  outerWidth: { get: () => N('viewport')[0], configurable: true }, outerHeight: { get: () => N('viewport')[1], configurable: true },
  devicePixelRatio: { get: () => N('viewport')[2], configurable: true },
  scrollX: { get: () => N('get_scroll', root()[ID])[0], configurable: true }, scrollY: { get: () => N('get_scroll', root()[ID])[1], configurable: true },
  pageXOffset: { get: () => N('get_scroll', root()[ID])[0], configurable: true }, pageYOffset: { get: () => N('get_scroll', root()[ID])[1], configurable: true },
  screenX: { value: 0, configurable: true }, screenY: { value: 0, configurable: true },
  screen: { get: () => ({ width: N('viewport')[0], height: N('viewport')[1], availWidth: N('viewport')[0], availHeight: N('viewport')[1], colorDepth: 24, pixelDepth: 24 }), configurable: true },
  visualViewport: { get: () => ({ width: N('viewport')[0], height: N('viewport')[1], scale: 1, offsetLeft: 0, offsetTop: 0 }), configurable: true },
});
define('scrollTo', (x, y) => { if (typeof x === 'object') { y = x.top; x = x.left; } N('set_scroll', root()[ID], x, y); });
define('scroll', global.scrollTo);
define('scrollBy', (x, y) => { if (typeof x === 'object') { y = x.top; x = x.left; } const [cx, cy] = N('get_scroll', root()[ID]); N('set_scroll', root()[ID], cx + (x || 0), cy + (y || 0)); });
define('close', () => N('window_close'));
define('focus', () => {}); define('blur', () => {});
define('open', () => null);
define('getSelection', () => document.getSelection());
define('postMessage', (data) => { setTimeout(() => windowTarget.dispatchEvent(new MessageEvent('message', { data, origin: locationObj.origin })), 0); });
for (const m of ['addEventListener', 'removeEventListener', 'dispatchEvent']) define(m, (...a) => windowTarget[m](...a));
for (const type of ON_EVENTS) {
  Object.defineProperty(global, 'on' + type, { configurable: true, get() { return windowTarget['on' + type]; }, set(v) { windowTarget['on' + type] = v; } });
}

// Entry points for the Rust side.
const rendrixApi = { dispatch: rendrixDispatch, timer: runTimer, raf: runRaf, afterTask, fetchDone, viewportChanged, boot, unhandledRejection,
  focus: id => setFocus(id === null ? null : wrap(id)), fireInput: (id, type, data) => fireInputEvents(wrap(id), type, data),
  checkResize,
  submitForm: id => { const f = wrap(id).closest('form'); if (f) f.requestSubmit(); } };
for (const [k, v] of Object.entries(rendrixApi)) Object.defineProperty(global, '__rendrix_' + k, { value: v, configurable: true, enumerable: false, writable: false });

// Public application API (also importable as the `rendrix` module).
const rendrixPublic = Object.freeze({
  version: '0.5.0', engine: 'rendrix',
  quit: () => N('quit'),
  window: Object.freeze({ setTitle: t => { document.title = String(t); }, close: () => N('window_close'), resize: (w, h) => N('window_resize', +w, +h) }),
  clipboard: navigatorObj.clipboard,
  fs: Object.freeze({
    readTextFile: p => new Promise((res, rej) => { try { res(N('fs_read_text', String(p))); } catch (e) { rej(e); } }),
    writeTextFile: (p, d) => new Promise((res, rej) => { try { N('fs_write_text', String(p), String(d)); res(); } catch (e) { rej(e); } }),
    exists: p => Promise.resolve(N('fs_exists', String(p))),
    readDir: p => new Promise((res, rej) => { try { res(N('fs_read_dir', String(p))); } catch (e) { rej(e); } }),
  }),
});
define('rendrix', rendrixPublic);
