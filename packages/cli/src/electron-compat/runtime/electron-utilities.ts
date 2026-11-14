import { EventEmitter } from './events';
import { compatWarnings } from './host';
import { spawn as spawnChild } from './child_process';

// utilityProcess in Electron runs a Node module in a separate utility process. Rendrix has no Node
// module runtime, but it can start a normal child process (the same host operation
// `child_process` uses) and relay its streams and lifecycle. The executable must be listed in
// `permissions.processes`; it defaults to `node`, so grant `node` (or pass `options.execPath`).
let nextUtilityPid = 1000;

export class UtilityProcess extends EventEmitter {
  pid: number;
  stdout: EventEmitter;
  stderr: EventEmitter;
  stdin: any = null;
  killed = false;
  exitCode: number | null = null;
  signalCode: string | null = null;
  connected = true;
  private child: any = null;

  constructor(modulePath: string, args: string[] = [], options: any = {}) {
    super();
    this.stdout = new EventEmitter();
    this.stderr = new EventEmitter();
    this.pid = nextUtilityPid++;
    if (typeof modulePath !== 'string' || !modulePath) {
      Promise.resolve().then(() => this.emit('error', new TypeError('utilityProcess.fork requires a module path')));
      return;
    }
    const opts = options && typeof options === 'object' ? options : {};
    const execPath = typeof opts.execPath === 'string' && opts.execPath ? opts.execPath : 'node';
    const execArgv = Array.isArray(opts.execArgv) ? opts.execArgv.map(String) : [];
    try {
      // child_process.spawn does not support options; cwd/env/stdio are host-controlled.
      this.child = spawnChild(execPath, [...execArgv, modulePath, ...args.map(String)]);
      if (Number.isFinite(Number(this.child.pid)) && Number(this.child.pid) > 0) this.pid = Number(this.child.pid);
      this.child.stdout?.on?.('data', (data: any) => this.stdout.emit('data', data));
      this.child.stderr?.on?.('data', (data: any) => this.stderr.emit('data', data));
      this.child.on?.('error', (error: any) => {
        this.connected = false;
        this.emit('error', error);
      });
      this.child.on?.('exit', (code: number | null, signal: string | null) => {
        if (this.exitCode !== null) return;
        this.exitCode = code;
        this.signalCode = signal;
        this.connected = false;
        this.emit('exit', code, signal);
      });
      this.child.on?.('close', (code: number | null, signal: string | null) => {
        if (this.exitCode !== null && code === null) return;
        this.emit('close', code, signal);
      });
      Promise.resolve().then(() => { if (!this.killed) this.emit('spawn'); });
    } catch (error: any) {
      Promise.resolve().then(() => this.emit('error', error));
    }
  }

  kill(_signal?: any): boolean {
    if (this.killed) return false;
    this.killed = true;
    this.connected = false;
    if (this.child?.kill) {
      try { this.child.kill(_signal); } catch {}
    } else {
      setTimeout(() => {
        this.exitCode = 0;
        this.emit('exit', 0, null);
        this.emit('close', 0, null);
      }, 0);
    }
    return true;
  }
  terminate(): boolean { return this.kill(); }
  postMessage(_message: any, _handle?: any): boolean {
    compatWarnings.push('utilityProcess.postMessage has no message port in Rendrix; the message was dropped');
    return false;
  }
  disconnect() { this.connected = false; }
}

export const utilityProcess = {
  fork(modulePath?: string, args?: string[], options?: any): UtilityProcess {
    return new UtilityProcess(String(modulePath ?? ''), Array.isArray(args) ? args : [], options);
  },
};

// Partial shim for Electron's webUtils. Dropped-file path lookup is not available, so
// return the supplied file's path when present and an empty string otherwise.
export const webUtils = {
  getPathForFile(file: any): string {
    if (file == null) return '';
    if (typeof file === 'string') return file;
    if (typeof file !== 'object') return '';
    if (typeof file.path === 'string') return file.path;
    if (typeof file.filePath === 'string') return file.filePath;
    if (typeof file.webkitRelativePath === 'string' && file.webkitRelativePath) return file.webkitRelativePath;
    if (typeof file.fullPath === 'string') return file.fullPath;
    // Some dropped-file wrappers expose the path via a nested file.
    if (file.file != null && file.file !== file) return webUtils.getPathForFile(file.file);
    return '';
  },
};

// In-app update support. Rendrix checks a configured update feed (JSON such as
// {"version":"1.2.3","url":"https://example.com/app.exe","sha256":"..."}), emits the
// usual Electron autoUpdater events, asks the host to download the update artifact into
// userData/updates and verify its SHA-256, and swaps it over the running executable on
// quitAndInstall() (relaunching when isForceRunAfter is set). Without a staged artifact the
// fallback opens the artifact in the system browser and quits.
import { host } from './host';

function compareVersions(left: any, right: any): number {
  const a = String(left ?? '').split('.').map((part: any) => parseInt(part, 10) || 0);
  const b = String(right ?? '').split('.').map((part: any) => parseInt(part, 10) || 0);
  const length = Math.max(a.length, b.length);
  for (let i = 0; i < length; i++) {
    const x = a[i] || 0;
    const y = b[i] || 0;
    if (x > y) return 1;
    if (x < y) return -1;
  }
  return 0;
}

export class AutoUpdater extends EventEmitter {
  private feedURLValue = '';
  private currentVersionValue = '';
  private pendingUpdate: any = null;
  private downloadedPath: string | null = null;

  /// Derives a plain file name for the staged artifact from the feed's URL.
  private artifactName(info: any): string {
    const raw = String(info?.url ?? '');
    const base = raw.split(/[?#]/)[0].split('/').pop() || '';
    const safe = base.replace(/[^A-Za-z0-9._-]/g, '_');
    return safe || 'update.bin';
  }

  getFeedURL(): string { return this.feedURLValue; }
  setFeedURL(options: any): void {
    if (typeof options === 'string') this.feedURLValue = options;
    else if (options && typeof options === 'object') this.feedURLValue = String(options.url ?? options.feedURL ?? '');
    else this.feedURLValue = String(options ?? '');
  }

  private async resolveCurrentVersion(): Promise<string> {
    if (this.currentVersionValue) return this.currentVersionValue;
    try {
      const info: any = await host('app.info', {});
      this.currentVersionValue = String(info?.version ?? '');
    } catch {
      this.currentVersionValue = '';
    }
    return this.currentVersionValue;
  }

  private async fetchFeed(url: string): Promise<any> {
    const g: any = globalThis;
    if (typeof g.fetch === 'function') {
      const response = await g.fetch(url);
      if (!response || !response.ok) throw new Error(`Update feed request failed: ${response ? response.status : 'no response'}`);
      const text = await response.text();
      return JSON.parse(text);
    }
    const result: any = await host('net.request', { url, method: 'GET' });
    const status = result?.status ?? result?.statusCode ?? 200;
    if (status < 200 || status >= 300) throw new Error(`Update feed request failed: ${status}`);
    let text = '';
    if (typeof result?.body === 'string') text = result.body;
    else if (typeof result?.text === 'string') text = result.text;
    else if (result?.body && typeof result.body.text === 'function') text = await result.body.text();
    else if (Array.isArray(result?.chunks)) text = result.chunks.map((c: any) => (typeof c === 'string' ? c : '')).join('');
    else if (result != null) {
      try { text = JSON.stringify(result); } catch { text = String(result); }
    }
    if (result && typeof result === 'object' && (result.version || result.url)) return result;
    if (!text) throw new Error('Update feed returned no body');
    return JSON.parse(text);
  }

  async checkForUpdates(): Promise<any> {
    this.emit('checking-for-update');
    const feedURL = this.feedURLValue;
    if (!feedURL) {
      const error = new Error('No update feed URL has been set.');
      this.emit('error', error);
      throw error;
    }
    const currentVersion = await this.resolveCurrentVersion();
    let updateInfo: any = null;
    try {
      updateInfo = await this.fetchFeed(feedURL);
    } catch (error) {
      this.emit('error', error);
      throw error;
    }
    if (updateInfo && compareVersions(updateInfo.version, currentVersion) > 0) {
      this.pendingUpdate = updateInfo;
      this.emit('update-available', updateInfo);
      const downloadPromise = this.downloadUpdate(updateInfo).catch((error: any) => {
        this.emit('error', error);
        throw error;
      });
      return { updateInfo, downloadPromise, isUpdateAvailable: true };
    }
    this.emit('update-not-available', { version: currentVersion });
    return { updateInfo: null, downloadPromise: null, isUpdateAvailable: false };
  }

  checkForUpdatesAndNotify(): Promise<any> { return this.checkForUpdates(); }

  async downloadUpdate(updateInfo?: any): Promise<any> {
    const info = updateInfo || this.pendingUpdate;
    if (!info || !info.url) return null;
    const name = this.artifactName(info);
    // Preferred path: the host streams the artifact straight into userData/updates while
    // verifying SHA-256, so the bytes never round-trip through the JS bridge. When the host
    // cannot stage it (an older host, or the feed host is not granted), fall back to an
    // in-memory download with the same verification; quitAndInstall then offers the artifact
    // through the system browser instead of swapping it in.
    try {
      const request: any = { url: String(info.url), name };
      if (info.sha256) request.sha256 = String(info.sha256);
      const staged: any = await host('updater.download', request);
      const file = String(staged?.staged ?? '').split(/[\\/]/).pop() || '';
      if (file) {
        const downloaded = { ...info, stagedPath: `updates/${file}` };
        this.pendingUpdate = downloaded;
        this.downloadedPath = `updates/${file}`;
        this.emit('update-downloaded', downloaded);
        return downloaded;
      }
    } catch { /* fall through to the in-memory download */ }
    let bytes: Uint8Array | null = null;
    const g: any = globalThis;
    if (typeof g.fetch === 'function') {
      const response = await g.fetch(info.url);
      if (!response || !response.ok) throw new Error(`Update download failed: ${response ? response.status : 'no response'}`);
      const buf = await response.arrayBuffer();
      bytes = new Uint8Array(buf);
    } else {
      const result: any = await host('net.request', { url: info.url, method: 'GET' });
      const status = result?.status ?? result?.statusCode ?? 200;
      if (status < 200 || status >= 300) throw new Error(`Update download failed: ${status}`);
      if (typeof result?.body === 'string') bytes = new TextEncoder().encode(result.body);
      else if (Array.isArray(result?.chunks)) {
        const parts = result.chunks.map((c: any) => (typeof c === 'string' ? new TextEncoder().encode(c) : (c instanceof Uint8Array ? c : new Uint8Array(0))));
        let total = 0; for (const p of parts) total += p.length;
        const out = new Uint8Array(total); let off = 0; for (const p of parts) { out.set(p, off); off += p.length; }
        bytes = out;
      }
    }
    if (!bytes) throw new Error('Update download returned no data');
    if (info.sha256) {
      const g2: any = globalThis;
      if (g2.crypto && g2.crypto.subtle) {
        const digest = await g2.crypto.subtle.digest('SHA-256', bytes);
        const hex = Array.from(new Uint8Array(digest)).map((b) => b.toString(16).padStart(2, '0')).join('');
        if (hex.toLowerCase() !== String(info.sha256).toLowerCase()) {
          throw new Error('Update artifact SHA-256 mismatch');
        }
      }
    }
    this.downloadedPath = info.url;
    this.emit('update-downloaded', info);
    return info;
  }

  quitAndInstall(isSilent?: boolean, isForceRunAfter?: boolean): void {
    this.emit('before-quit-for-update');
    const staged = typeof this.pendingUpdate?.stagedPath === 'string' ? String(this.pendingUpdate.stagedPath) : '';
    const fileName = staged ? staged.replace(/^updates[\\/]/, '') : '';
    const install = fileName
      ? host('updater.install', { path: fileName, restart: !!isForceRunAfter })
      : Promise.reject(new Error('No staged update artifact'));
    install
      .then(() => host('app.quit', {}))
      .catch(() => {
        // No staged artifact, or the host could not swap it: open the artifact and quit, the
        // pre-1.0 behavior, so the user still ends up with the new version in hand.
        const info = this.pendingUpdate;
        if (info && info.url) {
          try { host('shell.openExternal', { url: info.url }); } catch {}
        }
        try { host('app.quit', {}); } catch {}
      });
  }
}

export const autoUpdater = new AutoUpdater();

export default { utilityProcess, webUtils, autoUpdater };