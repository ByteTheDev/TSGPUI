import { host, compatWarnings } from './host';
import { EventEmitter } from './events';

// Electron Session/cookies compatibility shim.

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

async function hostOp(op: string, args: Record<string, any> = {}): Promise<any> {
  try {
    const r = await host(op, args);
    return r === undefined ? null : r;
  } catch {
    return null;
  }
}

async function hostCookies(action: 'get' | 'set' | 'clear' | 'remove' | 'flush', payload: Record<string, any>): Promise<any | null> {
  try {
    const result = await host('session.cookies', { action, ...payload });
    if (result == null) return null;
    if (result && typeof result === 'object' && !Array.isArray(result)) {
      if (action === 'get' && result.cookies === undefined) return null;
      // Empty objects are how a host that does not implement an operation often answers a test
      // bridge; treat them as unsupported so the local store still works.
      if (!Object.keys(result).length) return null;
    }
    return result;
  } catch {
    return null;
  }
}

function noopApi(): any {
  return new Proxy({}, {
    get(_t, prop) {
      if (prop === Symbol.toPrimitive) return () => '';
      if (prop === 'then') return undefined;
      return (..._args: any[]) => undefined;
    },
  });
}

// Cookie files live directly under the host-provided userData root. The name includes a short
// hash so partitions such as "persist:a/b" and "persist:a_b" cannot collide after sanitizing.
function hashText(text: string): string {
  let hash = 5381;
  for (let i = 0; i < text.length; i++) hash = ((hash << 5) + hash + text.charCodeAt(i)) >>> 0;
  return hash.toString(16);
}

function cookieFileName(key: string): string {
  const safe = key.replace(/[^A-Za-z0-9._-]/g, '_').slice(0, 80) || 'default';
  return `rendrix-session-cookies-${safe}-${hashText(key)}.json`;
}

function sessionStoreKey(partition: string, storagePath: string | null): string {
  return storagePath ? `path:${storagePath}` : (partition || 'default');
}

function normalizeCookieRecord(value: any): Cookie | null {
  if (!value || typeof value !== 'object') return null;
  if (typeof value.name !== 'string' || !value.name) return null;
  if (value.url != null && typeof value.url !== 'string') return null;
  if (value.domain != null && typeof value.domain !== 'string') return null;
  return {
    name: value.name,
    value: value.value != null ? String(value.value) : '',
    url: value.url,
    domain: value.domain,
    path: typeof value.path === 'string' && value.path ? value.path : '/',
    secure: value.secure === true,
    httpOnly: value.httpOnly === true,
    expirationDate: typeof value.expirationDate === 'number' ? value.expirationDate : undefined,
    session: value.session === true,
    hostOnly: value.hostOnly === true,
  };
}

const WEB_REQUEST_EVENTS = [
  'onBeforeRequest', 'onBeforeSendHeaders', 'onSendHeaders', 'onHeadersReceived',
  'onResponseStarted', 'onBeforeRedirect', 'onCompleted', 'onErrorOccurred',
] as const;

function webFilterMatches(filter: any, url: string): boolean {
  if (!filter || !Array.isArray(filter.urls) || !filter.urls.length) return true;
  return filter.urls.some((pattern: any) => {
    if (typeof pattern !== 'string') return false;
    if (pattern === '<all_urls>' || pattern === '*') return true;
    const escaped = pattern.replace(/[.+?^${}()|[\]\\]/g, '\\$&').replace(/\*/g, '.*');
    try { return new RegExp('^' + escaped + '$').test(url); } catch { return false; }
  });
}

function createWebRequest(): any {
  const registered = new Map<string, Array<{ filter: any; listener: Function }>>();
  const warned = new Set<string>();
  const api: any = {};
  for (const event of WEB_REQUEST_EVENTS) {
    api[event] = (filterOrListener: any, maybeListener?: any) => {
      const listener = typeof filterOrListener === 'function' ? filterOrListener : maybeListener;
      if (typeof listener !== 'function') throw new TypeError(`session.webRequest.${event} requires a listener function`);
      const filter = typeof filterOrListener === 'function' ? {} : (filterOrListener || {});
      if (typeof filter !== 'object' || Array.isArray(filter)) throw new TypeError(`session.webRequest.${event} filter must be an object`);
      if (filter.urls != null && (!Array.isArray(filter.urls) || filter.urls.some((url: any) => typeof url !== 'string'))) {
        throw new TypeError(`session.webRequest.${event} filter.urls must be an array of strings`);
      }
      registered.set(event, [...(registered.get(event) || []), { filter, listener }]);
      if (!warned.has(event)) {
        warned.add(event);
        compatWarnings.push(`session.webRequest.${event} records listeners, but request interception is not supported by the WebView host`);
      }
    };
  }
  Object.defineProperty(api, '_listeners', { value: registered, enumerable: false });
  Object.defineProperty(api, '_emit', {
    enumerable: false,
    value: (event: string, details: any) => {
      const results: any[] = [];
      for (const { filter, listener } of registered.get(event) || []) {
        if (!webFilterMatches(filter, String(details?.url || ''))) continue;
        try {
          listener(details, (result: any) => results.push(result));
        } catch { /* listeners are user code; a throw does not stop other listeners */ }
      }
      return results;
    },
  });
  return api;
}

const STORAGE_NAMES = new Set([
  'appcache', 'cookies', 'filesystem', 'indexdb', 'localstorage',
  'shadercache', 'websql', 'serviceworkers', 'cachestorage',
]);
const COOKIE_STORAGES = new Set(['cookies']);
const PAGE_STORAGES = new Set(['localstorage']);

function normalizeClearOptions(api: string, options: any): { origin?: string; storages: string[] } {
  if (options == null) return { storages: [...STORAGE_NAMES] };
  if (typeof options !== 'object' || Array.isArray(options)) throw new TypeError(`${api} options must be an object`);
  if (options.origin != null && typeof options.origin !== 'string') throw new TypeError(`${api} options.origin must be a string`);
  let storages: string[];
  if (options.storages == null) storages = [...STORAGE_NAMES];
  else {
    if (!Array.isArray(options.storages)) throw new TypeError(`${api} options.storages must be an array`);
    storages = [];
    for (const storage of options.storages) {
      if (typeof storage !== 'string' || !STORAGE_NAMES.has(storage)) {
        throw new TypeError(`${api} has an unknown storage type '${storage}'`);
      }
      if (!storages.includes(storage)) storages.push(storage);
    }
  }
  return { origin: options.origin, storages };
}

export class Session extends EventEmitter {
  partition: string;
  persist: boolean;
  private storagePathValue: string | null;
  private store = new Map<string, Cookie>();
  private closedValue = false;

  // Tracked state for partial/no-op APIs.
  private proxyConfigValue: any = null;
  private userAgentValue = '';
  private acceptLanguagesValue: string | undefined;
  private preloadsValue: string[] = [];
  private ntlmDomainsValue: string[] = [];
  private certificateVerifyProcValue: any = null;
  private permissionRequestHandlerValue: any = null;
  private permissionCheckHandlerValue: any = null;
  private displayMediaRequestHandlerValue: any = null;
  private devicePermissionHandlerValue: any = null;
  private bluetoothPairingHandlerValue: any = null;
  private usbProtectedClassesValue: string[] = [];
  private usbProtectedClassesHandlerValue: any = null;
  private spellCheckerEnabledValue = false;
  private spellCheckerLanguagesValue: string[] = [];
  private spellCheckerDictionaryDownloadURLValue: string | null = null;
  private codeCachePathValue: string | null = null;
  private downloadPathValue: string | null = null;
  private sslConfigValue: any = null;
  private extensionsValue = new Map<string, any>();
  private spellCheckerWords = new Set<string>();

  // Cookie persistence. Persistent partitions mirror non-session cookies to one JSON file under
  // userData; the host's session.cookies operation remains authoritative when it exists.
  private loadPromise: Promise<void> | null = null;
  private writeQueue: Promise<void> = Promise.resolve();
  private persistenceWarned = false;
  private downloads = new Map<number, DownloadItem>();

  private cookiesApi: any;
  private webRequestValue: any;
  private protocolValue: any;
  private serviceWorkersValue: any;
  private netLogValue: any;

  constructor(partition = '') {
    super();
    this.partition = partition;
    this.persist = typeof partition === 'string' && partition.startsWith('persist:');
    this.storagePathValue = null;
    this.cookiesApi = {
      get: (details?: GetCookiesDetails | { url: string }) => this.getCookies((details as any) ?? {}),
      set: (details: SetCookieDetails) => this.setCookies(details),
      remove: (...args: any[]) => {
        if (args.length >= 2 && typeof args[0] === 'string') {
          return this.removeCookie({ url: args[0], name: args[1] });
        }
        return this.removeCookie(args[0]);
      },
      flushStore: () => this.flushCookiesStore(),
      clear: () => this.clearCookies(),
    };
    this.webRequestValue = createWebRequest();
    this.protocolValue = noopApi();
    this.serviceWorkersValue = noopApi();
    this.netLogValue = noopApi();
  }

  get cookies() { return this.cookiesApi; }

  get storagePath(): string | null { return this.storagePathValue; }
  set storagePath(value: string | null) {
    if (value === this.storagePathValue) return;
    this.storagePathValue = value;
    this.loadPromise = null;
    this.store.clear();
  }

  get spellCheckerEnabled(): boolean { return this.spellCheckerEnabledValue; }
  set spellCheckerEnabled(value: boolean) { this.spellCheckerEnabledValue = value === true; }

  get availableSpellCheckerLanguages(): string[] { return ['en-US']; }

  get webRequest(): any { return this.webRequestValue; }
  get protocol(): any { return this.protocolValue; }
  get serviceWorkers(): any { return this.serviceWorkersValue; }
  get netLog(): any { return this.netLogValue; }

  isPersistent(): boolean { return this.persist; }
  isClosed(): boolean { return this.closedValue; }

  // Cookie persistence
  private cookieStoreKey(): string {
    return sessionStoreKey(this.partition, this.storagePathValue);
  }

  private ensureCookiesLoaded(): Promise<void> {
    if (!this.persist) return Promise.resolve();
    if (!this.loadPromise) this.loadPromise = this.loadCookiesFromDisk();
    return this.loadPromise;
  }

  private async loadCookiesFromDisk(): Promise<void> {
    try {
      const result: any = await host('filesystem.readText', {
        root: 'userData',
        path: cookieFileName(this.cookieStoreKey()),
      });
      const text = typeof result === 'string' ? result : result?.text;
      if (typeof text === 'string' && text.trim()) {
        const records = JSON.parse(text);
        if (Array.isArray(records)) {
          for (const record of records) {
            const cookie = normalizeCookieRecord(record);
            if (cookie) this.store.set(cookieKey(cookie), cookie);
          }
        }
      }
    } catch {
      // No file yet (or the host has no filesystem access): the store starts empty.
    }
    this.purgeExpiredCookies();
  }

  private persistCookies(): Promise<void> {
    if (!this.persist) return Promise.resolve();
    // Session cookies are not written, matching Electron's persistent-cookie semantics.
    const snapshot = [...this.store.values()].filter(cookie => cookie.session !== true && typeof cookie.expirationDate === 'number');
    const key = this.cookieStoreKey();
    this.writeQueue = this.writeQueue.then(async () => {
      try {
        await host('filesystem.writeText', {
          root: 'userData',
          path: cookieFileName(key),
          text: JSON.stringify(snapshot),
        });
      } catch {
        if (!this.persistenceWarned) {
          this.persistenceWarned = true;
          compatWarnings.push('session cookie persistence is unavailable; persistent cookies stay in memory for this run');
        }
      }
    });
    return this.writeQueue;
  }

  private purgeExpiredCookies(): void {
    const now = Date.now() / 1000;
    for (const [key, cookie] of [...this.store]) {
      if (typeof cookie.expirationDate === 'number' && cookie.expirationDate <= now) this.store.delete(key);
    }
  }

  // Cookies
  async getCookies(details: GetCookiesDetails = {}): Promise<Cookie[]> {
    await this.ensureCookiesLoaded();
    this.purgeExpiredCookies();
    const hostResult = await hostCookies('get', { details: { ...details, partition: this.partition || undefined } });
    if (Array.isArray(hostResult)) return [...hostResult];
    if (hostResult && Array.isArray(hostResult.cookies)) return [...hostResult.cookies];
    return [...this.store.values()].filter(c => matches(c, details)).map(c => ({ ...c }));
  }

  async setCookies(cookie: SetCookieDetails): Promise<void> {
    const normalized = normalizeCookie(cookie);
    await this.ensureCookiesLoaded();
    const hostResult = await hostCookies('set', { cookie: normalized, partition: this.partition || undefined });
    if (hostResult !== null) return;
    this.store.set(cookieKey(normalized), normalized);
    await this.persistCookies();
  }

  async set(cookie: SetCookieDetails): Promise<void> { return this.setCookies(cookie); }

  async removeCookie(detailsOrUrl: any, name?: string): Promise<void> {
    const details = typeof detailsOrUrl === 'string' ? { url: detailsOrUrl, name } : detailsOrUrl;
    if (!details?.url || !details?.name) throw new TypeError('session.cookies.remove requires url and name');
    await this.ensureCookiesLoaded();
    const hostResult = await hostCookies('remove', { details: { ...details, partition: this.partition || undefined } });
    if (hostResult !== null) return;
    for (const [key, cookie] of [...this.store]) {
      if (cookie.name !== details.name) continue;
      if (details.domain != null && cookie.domain !== details.domain && cookie.domain !== String(details.domain).replace(/^\./, '')) continue;
      if (details.path != null && cookie.path !== details.path) continue;
      if (details.url != null && !matches(cookie, { url: details.url, name: details.name })) continue;
      this.store.delete(key);
    }
    await this.persistCookies();
  }

  async clearCookies(): Promise<void> {
    await this.ensureCookiesLoaded();
    const hostResult = await hostCookies('clear', { partition: this.partition || undefined });
    this.store.clear();
    if (hostResult === null || this.persist) await this.persistCookies();
  }

  async flushCookiesStore(): Promise<void> {
    const hostResult = await hostCookies('flush', { partition: this.partition || undefined });
    if (hostResult === null) {
      if (this.persist) await this.persistCookies();
      else compatWarnings.push('session.cookies.flushStore has no durable store for a non-persistent partition');
      await this.flushStorageData().catch(() => {});
    }
  }

  // Cache / storage controls
  async clearCache(): Promise<void> {
    const r = await hostOp('session.clearCache', { partition: this.partition || undefined });
    if (r == null) compatWarnings.push('session.clearCache is not supported and has no effect');
  }

  async clearStorageData(options: any = {}): Promise<void> {
    const normalized = normalizeClearOptions('session.clearStorageData', options);
    const r = await hostOp('session.clearStorageData', { partition: this.partition || undefined, options: normalized });
    if (r == null || (typeof r === 'object' && !Array.isArray(r) && !Object.keys(r).length)) {
      await this.clearStorageFallback('session.clearStorageData', normalized);
    }
  }

  async clearData(options: any = {}): Promise<void> {
    const normalized = normalizeClearOptions('session.clearData', options);
    const r = await hostOp('session.clearData', { partition: this.partition || undefined, options: normalized });
    if (r == null || (typeof r === 'object' && !Array.isArray(r) && !Object.keys(r).length)) {
      await this.clearStorageFallback('session.clearData', normalized);
    }
  }

  private async clearStorageFallback(api: string, options: { origin?: string; storages: string[] }): Promise<void> {
    let clearedCookies = false;
    let clearedPage = false;
    let unsupported = false;
    const g: any = globalThis;
    const location = g.location ?? g.window?.location;
    const origin = typeof location?.origin === 'string' ? location.origin : '';
    const originMatches = !options.origin || !origin || options.origin === origin;
    for (const storage of options.storages) {
      if (COOKIE_STORAGES.has(storage)) {
        this.store.clear();
        await this.persistCookies();
        clearedCookies = true;
      } else if (PAGE_STORAGES.has(storage)) {
        if (originMatches) {
          for (const name of ['localStorage', 'sessionStorage']) {
            try {
              const area = g[name] ?? g.window?.[name];
              if (area && typeof area.clear === 'function') { area.clear(); clearedPage = true; }
            } catch { /* storage may be unavailable or blocked */ }
          }
        } else {
          unsupported = true;
        }
      } else {
        unsupported = true;
      }
    }
    if (!clearedCookies && !clearedPage) {
      compatWarnings.push(`${api} is not supported and has no effect`);
    } else if (unsupported) {
      compatWarnings.push(`${api} cleared cookies/localStorage for the current origin only; other storage types need host support`);
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

  async clearSharedDictionaryCache(): Promise<void> {
    const r = await hostOp('session.clearSharedDictionaryCache', { partition: this.partition || undefined });
    if (r == null) compatWarnings.push('session.clearSharedDictionaryCache is not supported and has no effect');
  }

  async clearSharedDictionaryCacheForIsolationKey(isolationKey: any): Promise<void> {
    const r = await hostOp('session.clearSharedDictionaryCacheForIsolationKey', { partition: this.partition || undefined, isolationKey });
    if (r == null) compatWarnings.push('session.clearSharedDictionaryCacheForIsolationKey is not supported and has no effect');
  }

  async flushStorageData(): Promise<void> {
    try { await host('session.flushStorageData', { partition: this.partition || undefined }); } catch { /* ignore */ }
  }

  async getCacheSize(): Promise<number> {
    const r = await hostOp('session.getCacheSize', { partition: this.partition || undefined });
    if (typeof r === 'number') return r;
    if (r && typeof r === 'object') {
      if (typeof r.size === 'number') return r.size;
      if (typeof r.cacheSize === 'number') return r.cacheSize;
    }
    return 0;
  }

  getStoragePath(): string | null { return this.storagePathValue; }

  // Proxy
  private proxyConfig: any = null;
  async setProxy(config: any): Promise<void> {
    this.proxyConfig = config;
    const r = await hostOp('session.setProxy', { partition: this.partition || undefined, proxyConfig: config });
    if (r == null) compatWarnings.push('session.setProxy is not supported and has no effect');
  }
  getProxyConfig(): any { return this.proxyConfig; }

  async resolveProxy(url: string): Promise<string> {
    const r = await hostOp('session.resolveProxy', { partition: this.partition || undefined, url });
    if (typeof r === 'string') return r;
    if (r && typeof r.proxy === 'string') return r.proxy;
    if (r && typeof r.resolveProxy === 'string') return r.resolveProxy;
    return 'DIRECT';
  }

  async forceReloadProxyConfig(): Promise<void> {
    const r = await hostOp('session.forceReloadProxyConfig', { partition: this.partition || undefined });
    if (r == null) { /* no-op */ }
  }

  // User agent
  async setUserAgent(userAgent: string, acceptLanguages?: string): Promise<void> {
    if (typeof userAgent !== 'string' || !userAgent) throw new TypeError('session.setUserAgent requires a non-empty user agent string');
    if (acceptLanguages !== undefined && typeof acceptLanguages !== 'string') throw new TypeError('session.setUserAgent acceptLanguages must be a string');
    this.userAgentValue = userAgent;
    this.acceptLanguagesValue = acceptLanguages;
    const g: any = globalThis;
    g.__rendrix_user_agent = userAgent;
    const r = await hostOp('session.setUserAgent', { partition: this.partition || undefined, userAgent, acceptLanguages });
    if (r == null || (typeof r === 'object' && !Array.isArray(r) && !Object.keys(r).length)) {
      let applied = false;
      try {
        const navigatorValue = g.navigator ?? g.window?.navigator;
        if (navigatorValue) {
          Object.defineProperty(navigatorValue, 'userAgent', { value: userAgent, configurable: true });
          applied = true;
        }
      } catch { /* the page user agent may be read-only */ }
      compatWarnings.push(applied
        ? 'session.setUserAgent applies to the current page only; the WebView host does not share it with new windows'
        : 'session.setUserAgent is stored but the page user agent cannot be changed by the WebView host');
    }
  }
  getUserAgent(): string {
    if (this.userAgentValue) return this.userAgentValue;
    try {
      const g: any = globalThis;
      return (g.navigator ?? g.window?.navigator)?.userAgent || '';
    } catch { return ''; }
  }

  // Preloads
  getPreloads(): string[] { return [...this.preloadsValue]; }
  async setPreloads(preloads: string[]): Promise<void> {
    this.preloadsValue = Array.isArray(preloads) ? [...preloads] : [];
    const r = await hostOp('session.setPreloads', { partition: this.partition || undefined, preloads: this.preloadsValue });
    if (r == null) compatWarnings.push('session.setPreloads is not supported and has no effect');
  }

  // Connections
  async closeAllConnections(): Promise<void> {
    const r = await hostOp('session.closeAllConnections', { partition: this.partition || undefined });
    if (r == null) compatWarnings.push('session.closeAllConnections is not supported and has no effect');
  }

  async close(): Promise<void> {
    this.closedValue = true;
    const r = await hostOp('session.close', { partition: this.partition || undefined });
    if (r == null) compatWarnings.push('session.close is not supported and has no effect');
  }

  // Spell checker
  isSpellCheckerEnabled(): boolean { return this.spellCheckerEnabledValue; }
  setSpellCheckerEnabled(enabled: boolean): void {
    this.spellCheckerEnabledValue = enabled === true;
    compatWarnings.push('session.setSpellCheckerEnabled is not supported and has no effect');
  }
  getSpellCheckerLanguages(): string[] { return [...this.spellCheckerLanguagesValue]; }
  setSpellCheckerLanguages(languages: string[]): void {
    this.spellCheckerLanguagesValue = Array.isArray(languages) ? [...languages] : [];
    compatWarnings.push('session.setSpellCheckerLanguages is not supported and has no effect');
  }
  setSpellCheckerDictionaryDownloadURL(url: string): void {
    this.spellCheckerDictionaryDownloadURLValue = url;
    compatWarnings.push('session.setSpellCheckerDictionaryDownloadURL is not supported and has no effect');
  }
  addWordToSpellCheckerDictionary(word: string): boolean {
    this.spellCheckerWords.add(word);
    compatWarnings.push('session.addWordToSpellCheckerDictionary is not supported and has no effect');
    return false;
  }
  removeWordFromSpellCheckerDictionary(word: string): boolean {
    this.spellCheckerWords.delete(word);
    compatWarnings.push('session.removeWordFromSpellCheckerDictionary is not supported and has no effect');
    return false;
  }
  listWordsInSpellCheckerDictionary(): string[] {
    return [...this.spellCheckerWords];
  }

  // NTLM
  allowNTLMCredentialsForDomains(domains: string | string[]): void {
    this.ntlmDomainsValue = Array.isArray(domains) ? [...domains] : [domains];
    compatWarnings.push('session.allowNTLMCredentialsForDomains is not supported and has no effect');
  }

  // Handlers (tracked state, no host)
  setCertificateVerifyProc(proc: any): void {
    this.certificateVerifyProcValue = proc ?? null;
    compatWarnings.push('session.setCertificateVerifyProc is not supported and has no effect');
  }
  setPermissionRequestHandler(handler: any): void {
    if (handler != null && typeof handler !== 'function') throw new TypeError('session.setPermissionRequestHandler requires a function or null');
    this.permissionRequestHandlerValue = handler ?? null;
    compatWarnings.push('session.setPermissionRequestHandler stores the handler; the WebView host does not emit permission prompts, so it is only reachable through session.requestPermission');
  }
  getPermissionRequestHandler(): any { return this.permissionRequestHandlerValue; }
  async requestPermission(details: any): Promise<boolean> {
    const handler = this.permissionRequestHandlerValue;
    if (typeof handler !== 'function') return false;
    return await new Promise<boolean>((resolve) => {
      let done = false;
      const callback = (granted: any) => { if (!done) { done = true; resolve(granted === true); } };
      try {
        const ret = handler(null, details?.permission, callback, details);
        if (ret && typeof ret.then === 'function') ret.then((value: any) => { if (value !== undefined) callback(value); }, () => callback(false));
        else if (ret !== undefined) callback(ret);
      } catch { callback(false); }
    });
  }
  setPermissionCheckHandler(handler: any): void {
    if (handler != null && typeof handler !== 'function') throw new TypeError('session.setPermissionCheckHandler requires a function or null');
    this.permissionCheckHandlerValue = handler ?? null;
    compatWarnings.push('session.setPermissionCheckHandler stores the handler; the WebView host does not consult it, so it is only reachable through session.checkPermission');
  }
  getPermissionCheckHandler(): any { return this.permissionCheckHandlerValue; }
  async checkPermission(permission: string, requestingOrigin: string, details?: any): Promise<boolean> {
    const handler = this.permissionCheckHandlerValue;
    if (typeof handler !== 'function') return false;
    try {
      const result = handler(null, permission, requestingOrigin, details);
      return result && typeof result.then === 'function' ? !!(await result) : !!result;
    } catch { return false; }
  }
  setDisplayMediaRequestHandler(handler: any): void {
    this.displayMediaRequestHandlerValue = handler ?? null;
    compatWarnings.push('session.setDisplayMediaRequestHandler is not supported and has no effect');
  }
  setDevicePermissionHandler(handler: any): void {
    this.devicePermissionHandlerValue = handler ?? null;
    compatWarnings.push('session.setDevicePermissionHandler is not supported and has no effect');
  }
  setBluetoothPairingHandler(handler: any): void {
    this.bluetoothPairingHandlerValue = handler ?? null;
    compatWarnings.push('session.setBluetoothPairingHandler is not supported and has no effect');
  }
  setUSBProtectedClassesHandler(handler: any): void {
    if (Array.isArray(handler)) this.usbProtectedClassesValue = [...handler];
    else this.usbProtectedClassesHandlerValue = handler ?? null;
    compatWarnings.push('session.setUSBProtectedClassesHandler is not supported and has no effect');
  }

  // Extensions
  async loadExtension(path: string, options?: any): Promise<any> {
    const r = await hostOp('session.loadExtension', { partition: this.partition || undefined, path, options });
    if (r == null) {
      compatWarnings.push('session.loadExtension is not supported and has no effect');
      return null;
    }
    const ext = r;
    if (ext && ext.id) this.extensionsValue.set(ext.id, ext);
    return ext;
  }
  async removeExtension(id: string): Promise<void> {
    this.extensionsValue.delete(id);
    const r = await hostOp('session.removeExtension', { partition: this.partition || undefined, id });
    if (r == null) compatWarnings.push('session.removeExtension is not supported and has no effect');
  }
  getAllExtensions(): any[] { return [...this.extensionsValue.values()]; }
  getExtension(id: string): any { return this.extensionsValue.get(id) ?? null; }

  // Network emulation / download / misc
  enableNetworkEmulation(options: any): void {
    compatWarnings.push('session.enableNetworkEmulation is not supported and has no effect');
  }
  disableNetworkEmulation(): void {
    compatWarnings.push('session.disableNetworkEmulation is not supported and has no effect');
  }
  downloadURL(url: string, options?: any): void {
    if (typeof url !== 'string' || !url) throw new TypeError('session.downloadURL requires a URL string');
    const item = new DownloadItem(this, url, options);
    this.downloads.set(item.id, item);
    let cancelled = false;
    const event = { preventDefault() { cancelled = true; } };
    const hasListener = this.listenerCount('will-download') > 0;
    this.emit('will-download', event, item, null);
    if (cancelled || item.getState() === 'cancelled') {
      item._finish('cancelled');
      return;
    }
    if (!hasListener) {
      compatWarnings.push('session.downloadURL needs a will-download listener; the WebView host does not start downloads on its own');
      item._finish('interrupted');
      return;
    }
    void hostOp('session.downloadURL', {
      partition: this.partition || undefined,
      url,
      options,
      savePath: item.getSavePath(),
    }).then(result => {
      if (item.isDone()) return;
      if (result == null || (typeof result === 'object' && !Array.isArray(result) && !Object.keys(result).length)) {
        compatWarnings.push('session.downloadURL could not start a download in the WebView host; the item is marked interrupted');
        item._finish('interrupted');
        return;
      }
      if (typeof result.savePath === 'string') item.setSavePath(result.savePath);
      if (typeof result.receivedBytes === 'number') item._received(result.receivedBytes);
      if (typeof result.totalBytes === 'number') item._total(result.totalBytes);
      item._finish(result.state === 'cancelled' ? 'cancelled' : 'completed');
    });
  }
  createInterruptedDownload(options: any): void {
    if (!options || typeof options !== 'object' || typeof options.url !== 'string' || !options.url) {
      throw new TypeError('session.createInterruptedDownload requires an options object with a url');
    }
    const item = new DownloadItem(this, options.url, options);
    if (typeof options.totalBytes === 'number') item._total(options.totalBytes);
    if (typeof options.receivedBytes === 'number') item._received(options.receivedBytes);
    if (typeof options.mimeType === 'string') item._mime(options.mimeType);
    item._finish('interrupted');
    this.downloads.set(item.id, item);
  }
  getDownloadItems(): DownloadItem[] { return [...this.downloads.values()]; }
  preconnect(options: any): void {
    compatWarnings.push('session.preconnect is not supported and has no effect');
  }
  async resolveHost(hostName: string, options?: any): Promise<any> {
    const r = await hostOp('session.resolveHost', { partition: this.partition || undefined, host: hostName, options });
    if (r == null) {
      compatWarnings.push('session.resolveHost is not supported and has no effect');
      return [];
    }
    return Array.isArray(r) ? r : (r.addresses || []);
  }
  async fetch(input: any, init?: any): Promise<any> {
    const r = await hostOp('session.fetch', { partition: this.partition || undefined, url: input, options: init });
    if (r == null) {
      compatWarnings.push('session.fetch is not supported and has no effect');
      return null;
    }
    return r;
  }
  async getBlobData(blobUUID: string): Promise<any> {
    const r = await hostOp('session.getBlobData', { partition: this.partition || undefined, blobUUID });
    if (r == null) {
      compatWarnings.push('session.getBlobData is not supported and has no effect');
      return null;
    }
    return r;
  }

  // Paths / code cache
  async setCodeCachePath(path: string): Promise<void> {
    this.codeCachePathValue = path;
    const r = await hostOp('session.setCodeCachePath', { partition: this.partition || undefined, path });
    if (r == null) compatWarnings.push('session.setCodeCachePath is not supported and has no effect');
  }
  async setDownloadPath(path: string): Promise<void> {
    this.downloadPathValue = path;
    const r = await hostOp('session.setDownloadPath', { partition: this.partition || undefined, path });
    if (r == null) compatWarnings.push('session.setDownloadPath is not supported and has no effect');
  }
  async setSSLConfig(config: any): Promise<void> {
    this.sslConfigValue = config;
    compatWarnings.push('session.setSSLConfig is not supported and has no effect');
  }

  // Shared dictionaries info
  getSharedDictionaryInfo(options?: any): any[] { return []; }
  getSharedDictionaryUsageInfo(): any[] { return []; }

  // Other tracked props
  get codeCachePath(): string | null { return this.codeCachePathValue; }
  get downloadPath(): string | null { return this.downloadPathValue; }
}

let nextDownloadId = 1;

function downloadFileName(url: string): string {
  try {
    const pathname = new URL(url).pathname;
    const name = pathname.split('/').filter(Boolean).pop() || '';
    return decodeURIComponent(name) || 'download';
  } catch { return 'download'; }
}

function joinDownloadPath(directory: string | null, name: string): string {
  if (!directory) return name;
  const separator = directory.includes('\\') && !directory.includes('/') ? '\\' : '/';
  return directory.replace(/[\\/]+$/, '') + separator + name;
}

/// A DownloadItem lifecycle stub. The host may report completion, but the shim never invents
/// received bytes for a download the host did not perform.
export class DownloadItem extends EventEmitter {
  id: number;
  private urlValue: string;
  private filenameValue: string;
  private savePathValue: string;
  private stateValue = 'progressing';
  private receivedBytesValue = 0;
  private totalBytesValue = 0;
  private mimeTypeValue = '';
  private pausedValue = false;
  private doneValue = false;

  constructor(session: Session, url: string, options: any = {}) {
    super();
    this.id = nextDownloadId++;
    this.urlValue = url;
    this.filenameValue = typeof options.filename === 'string' && options.filename ? options.filename : downloadFileName(url);
    this.savePathValue = typeof options.savePath === 'string' && options.savePath
      ? options.savePath
      : joinDownloadPath(session.downloadPath, this.filenameValue);
    if (typeof options.totalBytes === 'number') this.totalBytesValue = options.totalBytes;
    if (typeof options.mimeType === 'string') this.mimeTypeValue = options.mimeType;
  }

  getURL(): string { return this.urlValue; }
  getFilename(): string { return this.filenameValue; }
  getSavePath(): string { return this.savePathValue; }
  setSavePath(path: string): void {
    if (typeof path !== 'string' || !path) throw new TypeError('DownloadItem.setSavePath requires a non-empty path');
    if (this.doneValue) return;
    this.savePathValue = path;
    this.emit('updated', {}, this.stateValue);
  }
  getState(): string { return this.stateValue; }
  getReceivedBytes(): number { return this.receivedBytesValue; }
  getTotalBytes(): number { return this.totalBytesValue; }
  getMimeType(): string { return this.mimeTypeValue; }
  isPaused(): boolean { return this.pausedValue; }
  isDone(): boolean { return this.doneValue; }
  canResume(): boolean { return this.stateValue === 'interrupted'; }
  pause(): void {
    if (this.doneValue || this.pausedValue) return;
    this.pausedValue = true;
    this.emit('updated', {}, this.stateValue);
  }
  resume(): void {
    if (this.doneValue || !this.pausedValue) return;
    this.pausedValue = false;
    this.emit('updated', {}, this.stateValue);
  }
  cancel(): void {
    if (this.doneValue) return;
    this.pausedValue = false;
    this._finish('cancelled');
  }
  _received(bytes: number): void { this.receivedBytesValue = Math.max(0, bytes); }
  _total(bytes: number): void { this.totalBytesValue = Math.max(0, bytes); }
  _mime(mimeType: string): void { this.mimeTypeValue = mimeType; }
  _finish(state: string, receivedBytes?: number): void {
    if (this.doneValue) return;
    if (typeof receivedBytes === 'number') this._received(receivedBytes);
    this.doneValue = true;
    this.pausedValue = false;
    this.stateValue = state;
    this.emit('updated', {}, state);
    this.emit('done', {}, state);
  }
}

const partitions = new Map<string, Session>();

function native(): Session {
  return new Session('persist:rendrix-default');
}

export const session = Object.assign(native(), {
  defaultSession: native(),
  fromPartition(partition: string, _options?: any): Session {
    if (typeof partition !== 'string' || !partition) throw new TypeError('session.fromPartition requires a partition string');
    let sess = partitions.get(partition);
    if (!sess) {
      sess = new Session(partition);
      partitions.set(partition, sess);
    }
    return sess;
  },
  fromPath(path: string, _options?: any): Session {
    if (typeof path !== 'string' || !path) throw new TypeError('session.fromPath requires a path string');
    const key = `path:${path}`;
    let sess = partitions.get(key);
    if (!sess) {
      sess = new Session('');
      sess.persist = true;
      sess.storagePath = path;
      partitions.set(key, sess);
    }
    return sess;
  },
});

export const DefaultSession = session.defaultSession;