import { EventEmitter } from './events';

// Partial shim for Electron's utilityProcess namespace. Real utility processes are not
// available in TSGPUI, so fork() returns a small child-process-like stub that emits
// the usual lifecycle events without spawning anything.
class UtilityProcessStub extends EventEmitter {
  pid: number;
  stdout: EventEmitter;
  stderr: EventEmitter;
  stdin: any = null;
  killed = false;
  exitCode: number | null = null;
  signalCode: string | null = null;
  connected = false;
  constructor() {
    super();
    this.pid = 0;
    this.stdout = new EventEmitter();
    this.stderr = new EventEmitter();
  }
  kill(_signal?: any): boolean {
    if (this.killed) return false;
    this.killed = true;
    this.exitCode = 0;
    setTimeout(() => {
      this.emit('exit', 0, null);
      this.emit('close', 0, null);
    }, 0);
    return true;
  }
  terminate(): boolean { return this.kill(); }
  postMessage(_message: any, _handle?: any): boolean { return false; }
  disconnect() { this.connected = false; }
}

let nextStubPid = 1000;
export const utilityProcess = {
  fork(_modulePath?: string, _args?: string[], _options?: any): UtilityProcessStub {
    const stub = new UtilityProcessStub();
    stub.pid = nextStubPid++;
    // Announce spawn asynchronously, mirroring a real child process.
    setTimeout(() => {
      stub.emit('spawn');
      stub.emit('exit', 0, null);
      stub.emit('close', 0, null);
      stub.exitCode = 0;
    }, 0);
    return stub;
  },
};

// Partial shim for Electron's webUtils. Dropped-file path lookup is not available, so
// return the supplied file's path when present and an empty string otherwise.
export const webUtils = {
  getPathForFile(file: any): string {
    if (file == null) return '';
    if (typeof file === 'string') return file;
    if (typeof file === 'object') {
      if (typeof file.path === 'string') return file.path;
      if (typeof file.filePath === 'string') return file.filePath;
      if (typeof file.name === 'string' && typeof file.path === 'undefined') return '';
    }
    return '';
  },
};

// Partial shim for Electron's autoUpdater. TSGPUI does not perform in-app updates, so
// every check reports that no update is available.
export class AutoUpdater extends EventEmitter {
  private feedURL = '';
  getFeedURL(): string { return this.feedURL; }
  setFeedURL(_options: any) { /* no-op */ }
  checkForUpdates(): Promise<any> {
    const result = { updateInfo: null, downloadPromise: null, isUpdateAvailable: false };
    this.emit('checking-for-update');
    this.emit('update-not-available', { version: undefined });
    return Promise.resolve(result);
  }
  checkForUpdatesAndNotify(): Promise<any> { return this.checkForUpdates(); }
  quitAndInstall(_isSilent?: boolean, _isForceRunAfter?: boolean) { /* no-op */ }
  downloadUpdate(): Promise<any> { return Promise.resolve(null); }
}

export const autoUpdater = new AutoUpdater();

export default { utilityProcess, webUtils, autoUpdater };