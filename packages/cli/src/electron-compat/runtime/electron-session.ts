import { host, compatWarnings } from './host';
import { EventEmitter } from './events';

// Electron Session/cookies compatibility shim.
//
// The WebView host does not yet provide a native cookie store, so by default
// cookies live in a per-session in-memory Map and are lost on restart unless
// the app enables persist (localStorage).  When the host does implement
// 'session.cookies', results are delegated there and the in-memory store is
// kept in sync as a fallback.
//
// Usage: `session.defaultSession.getCookies()` / `setCookies()` /
// `clearCookies()` are the flat aliases used after migration; the
// `session.cookies.get/set/remove` namespace matches the Electron surface.

type Cookie = {
  name: string;
  value: string;
  url?: string;
  domain?: string;
  path?: string;
  secure?: boolean;
  httpOnly?: boolean;
  expirationDate?: number;
  session?: boolean;
  hostOnly?: boolean;
};

type GetCookiesDetails = {
  url?: string;
  name?: string;
  domain?: string;
  path?: string;
  includeHttpOnly?: boolean;
  secure?: boolean;
  session?: boolean;
};

type SetCookieDetails = Cookie & { url: string; name: string; value: string };
type RemoveCookieDetails = { url: string; name: string; domain?: string; path?: string };

function cookieKey(cookie: { url?: string; name: string; domain?: string; path?: string }): string {
  const scope = cookie.domain ? `${cookie.domain}${cookie.path || '/'}` : cookie.url || '';
  return `${scope}::${cookie.name}`;
}

function normalizeCookie(input: SetCookieDetails): Cookie {
  if (!input || typeof input !== 'object') throw new TypeError('session.cookies.set requires a cookie object');
  if (!input.url || typeof input.url !== 'string') throw new TypeError('session.cookies.set requires a url');
  if (!input.name || typeof input.name !== 'string') throw new TypeError('session.cookies.set requires a name');
  return {
    name: input.name,
    value: input.value != null ? String(input.value) : '',
    url: input.url,
    domain: input.domain,
    path: input.path || '/',
    secure: input.secure === true,
    httpOnly: input.httpOnly === true,
    expirationDate: typeof input.expirationDate === 'number' ? input.expirationDate : undefined,
    session: input.session === true,
    hostOnly: false,
  };
}

function matches(cookie: Cookie, details: GetCookiesDetails = {}): boolean {
  if (details.name != null && cookie.name !== details.name) return false;
  if (details.domain != null && cookie.domain !== details.domain && cookie.domain !== details.domain?.replace(/^\./, '')) return false;
  if (details.path != null && cookie.path !== details.path) return false;
  if (details.url != null) {
    try {
      const want = new URL(details.url);
      if (cookie.url) {
        const have = new URL(cookie.url);
        if (have.hostname !== want.hostname) return false;
        if (details.path == null && have.pathname !== want.pathname && cookie.path) {
          if (!(want.pathname === cookie.path || want.pathname.startsWith(cookie.path + '/') || cookie.path === '/')) return false;
        }
      } else if (details.domain) {
        const domain = details.domain.replace(/^\./, '');
        if (!(want.hostname === domain || want.hostname.endsWith('.' + domain))) return false;
      }
    } catch {
      if (cookie.url !== details.url) return false;
    }
  }
  if (details.secure === true && !cookie.secure) return false;
  if (details.includeHttpOnly !== true && cookie.httpOnly) return false;
  if (details.session === true && !cookie.session) {
    if (cookie.expirationDate != null) return false;
  }
  return true;
}

async function hostCookies(action: 'get' | 'set' | 'clear' | 'remove' | 'flush', payload: Record<string, any>): Promise<any | null> {
  try {
    const result = await host('session.cookies', { action, ...payload });
    if (result == null) return null;
    if (action === 'get' && result && typeof result === 'object' && !Array.isArray(result) && result.cookies === undefined) return null;
    return result;
  } catch {
    return null;
  }
}

export class Session extends EventEmitter {
  partition: string;
  persist: boolean;
  private storagePathValue: string | null;
  // Per-session in-memory store. Keyed by domain/path+name or url+name.
  private store = new Map<string, Cookie>();
  private cookiesApi: {
    get: (details?: GetCookiesDetails | { url: string }) => Promise<Cookie[]>;
    set: (details: SetCookieDetails) => Promise<void>;
    remove: (details: RemoveCookieDetails) => Promise<void>;
    flush: () => Promise<void>;
    clear: () => Promise<void>;
  };

  constructor(partition = '') {
    super();
    this.partition = partition;
    this.persist = typeof partition === 'string' && partition.startsWith('persist:');
    this.storagePathValue = this.persist ? null : null;
    this.cookiesApi = {
      get: (details?: GetCookiesDetails) => this.getCookies(details ?? {}),
      set: (details: SetCookieDetails) => this.setCookies(details),
      remove: (details: RemoveCookieDetails) => this.removeCookie(details),
      flush: () => this.flushStorageData(),
      clear: () => this.clearCookies(),
    };
  }

  get cookies() { return this.cookiesApi; }

  get storagePath(): string | null { return this.storagePathValue; }
  set storagePath(value: string | null) { this.storagePathValue = value; }

  isPersistent(): boolean { return this.persist; }

  async getCookies(details: GetCookiesDetails = {}): Promise<Cookie[]> {
    const hostResult = await hostCookies('get', { details: { ...details, partition: this.partition || undefined } });
    if (Array.isArray(hostResult)) return [...hostResult];
    if (hostResult && Array.isArray(hostResult.cookies)) return [...hostResult.cookies];
    return [...this.store.values()].filter(c => matches(c, details));
  }

  async setCookies(cookie: SetCookieDetails): Promise<void> {
    const normalized = normalizeCookie(cookie);
    const hostResult = await hostCookies('set', { cookie: normalized, partition: this.partition || undefined });
    if (hostResult !== null) return;
    this.store.set(cookieKey(normalized), normalized);
  }

  async set(cookie: SetCookieDetails): Promise<void> { return this.setCookies(cookie); }

  async removeCookie(details: RemoveCookieDetails): Promise<void> {
    if (!details?.url || !details?.name) throw new TypeError('session.cookies.remove requires url and name');
    const hostResult = await hostCookies('remove', { details: { ...details, partition: this.partition || undefined } });
    if (hostResult !== null) return;
    this.store.delete(cookieKey({ ...details, domain: details.domain, path: details.path }));
  }

  async clearCookies(): Promise<void> {
    const hostResult = await hostCookies('clear', { partition: this.partition || undefined });
    if (hostResult !== null) return;
    this.store.clear();
  }

  async clearCache(): Promise<void> {
    try { const r = await host('session.clearCache', { partition: this.partition || undefined }); if (r == null) compatWarnings.push('session.clearCache is not supported and has no effect'); } catch { compatWarnings.push('session.clearCache is not supported and has no effect'); }
  }

  async clearStorageData(options: any = {}): Promise<void> {
    try { const r = await host('session.clearStorageData', { partition: this.partition || undefined, options }); if (r == null) { compatWarnings.push('session.clearStorageData is not supported and has no effect'); this.store.clear(); } } catch { compatWarnings.push('session.clearStorageData is not supported and has no effect'); this.store.clear(); }
  }

  async clearData(options: any = {}): Promise<void> {
    const hostResult = await hostCookies('clear', { partition: this.partition || undefined, data: true, options });
    if (hostResult === null) {
      compatWarnings.push('session.clearData is not supported and has no effect');
      this.store.clear();
    }
  }

  async clearAuthCache(): Promise<void> {
    compatWarnings.push('session.clearAuthCache is not supported and has no effect');
  }

  async clearHostResolverCache(): Promise<void> {
    compatWarnings.push('session.clearHostResolverCache is not supported and has no effect');
  }

  async clearCodeCaches(): Promise<void> {
    compatWarnings.push('session.clearCodeCaches is not supported and has no effect');
  }

  async flushStorageData(): Promise<void> {
    try { await host('session.flushStorageData', { partition: this.partition || undefined }); } catch { /* ignore */ }
  }

  async getCacheSize(): Promise<number> {
    try {
      const r = await host('session.getCacheSize', { partition: this.partition || undefined });
      if (typeof r === 'number') return r;
      if (r && typeof r === 'object') {
        if (typeof r.size === 'number') return r.size;
        if (typeof r.cacheSize === 'number') return r.cacheSize;
      }
      return 0;
    } catch {
      return 0;
    }
  }

  async setProxy(config: any): Promise<void> {
    compatWarnings.push('session.setProxy is not supported and has no effect');
  }
  async resolveProxy(url: string): Promise<string> {
    return 'DIRECT';
  }
  async forceReloadProxyConfig(): Promise<void> { return; }

  async setUserAgent(userAgent: string, acceptLanguages?: string): Promise<void> {
    compatWarnings.push('session.setUserAgent is not supported and has no effect');
  }
  getUserAgent(): string { return ''; }

  getPreloads(): string[] { return []; }
  setPreloads(preloads: string[]): void { compatWarnings.push('session.setPreloads is not supported and has no effect'); }

  async closeAllConnections(): Promise<void> { return; }
}

const partitions = new Map<string, Session>();
function native(): Session {
  return new Session('persist:tsgpui-default');
}

export const session = Object.assign(native(), {
  defaultSession: native(),
  fromPartition(partition: string): Session {
    if (typeof partition !== 'string' || !partition) throw new TypeError('session.fromPartition requires a partition string');
    let sess = partitions.get(partition);
    if (!sess) {
      sess = new Session(partition);
      partitions.set(partition, sess);
    }
    return sess;
  },
  fromPath(): Session {
    compatWarnings.push('session.fromPath is not supported; returning the default session');
    return session.defaultSession;
  },
});

export const DefaultSession = session.defaultSession;