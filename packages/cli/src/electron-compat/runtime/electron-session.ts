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
    if (action === 'get' && result && typeof result === 'object' && !Array.isArray(result) && result.cookies === undefined) return null;
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
    this.webRequestValue = noopApi();
    this.protocolValue = noopApi();
    this.serviceWorkersValue = noopApi();
    this.netLogValue = noopApi();
  }

  get cookies() { return this.cookiesApi; }

  get storagePath(): string | null { return this.storagePathValue; }
  set storagePath(value: string | null) { this.storagePathValue = value; }

  get spellCheckerEnabled(): boolean { return this.spellCheckerEnabledValue; }
  set spellCheckerEnabled(value: boolean) { this.spellCheckerEnabledValue = value === true; }

  get availableSpellCheckerLanguages(): string[] { return ['en-US']; }

  get webRequest(): any { return this.webRequestValue; }
  get protocol(): any { return this.protocolValue; }
  get serviceWorkers(): any { return this.serviceWorkersValue; }
  get netLog(): any { return this.netLogValue; }

  isPersistent(): boolean { return this.persist; }
  isClosed(): boolean { return this.closedValue; }

  // Cookies
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

  async removeCookie(detailsOrUrl: any, name?: string): Promise<void> {
    const details = typeof detailsOrUrl === 'string' ? { url: detailsOrUrl, name } : detailsOrUrl;
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

  async flushCookiesStore(): Promise<void> {
    const hostResult = await hostCookies('flush', { partition: this.partition || undefined });
    if (hostResult === null) {
      compatWarnings.push('session.cookies.flushStore is not supported and has no effect');
      await this.flushStorageData().catch(() => {});
    }
  }

  // Cache / storage controls
  async clearCache(): Promise<void> {
    const r = await hostOp('session.clearCache', { partition: this.partition || undefined });
    if (r == null) compatWarnings.push('session.clearCache is not supported and has no effect');
  }

  async clearStorageData(options: any = {}): Promise<void> {
    const r = await hostOp('session.clearStorageData', { partition: this.partition || undefined, options });
    if (r == null) {
      compatWarnings.push('session.clearStorageData is not supported and has no effect');
      this.store.clear();
    }
  }

  async clearData(options: any = {}): Promise<void> {
    const r = await hostOp('session.clearData', { partition: this.partition || undefined, options });
    if (r == null) {
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
    this.userAgentValue = userAgent;
    this.acceptLanguagesValue = acceptLanguages;
    const r = await hostOp('session.setUserAgent', { partition: this.partition || undefined, userAgent, acceptLanguages });
    if (r == null) compatWarnings.push('session.setUserAgent is not supported and has no effect');
  }
  getUserAgent(): string { return this.userAgentValue; }

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
    this.permissionRequestHandlerValue = handler ?? null;
    compatWarnings.push('session.setPermissionRequestHandler is not supported and has no effect');
  }
  setPermissionCheckHandler(handler: any): void {
    this.permissionCheckHandlerValue = handler ?? null;
    compatWarnings.push('session.setPermissionCheckHandler is not supported and has no effect');
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
  downloadURL(url: string): void {
    void hostOp('session.downloadURL', { partition: this.partition || undefined, url }).then(r => {
      if (r == null) compatWarnings.push('session.downloadURL is not supported and has no effect');
    });
  }
  createInterruptedDownload(options: any): void {
    compatWarnings.push('session.createInterruptedDownload is not supported and has no effect');
  }
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

const partitions = new Map<string, Session>();

function native(): Session {
  return new Session('persist:tsgpui-default');
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