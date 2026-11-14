import { EventEmitter } from './events';
import { host, compatWarnings } from './host';
import fs from './fs';
import path from './path';
import { info, infoReady } from './os';

// netLog: Electron exposes a network log file for traffic captured by the browser process.
// Rendrix records the `net.request` traffic its shim sees and writes a JSON log file. Chromium's
// internal sockets, DNS and QUIC events are not available in the embedded engines.
let netLogCurrentlyLogging = false;
let netLogFilePath = '';
let netLogStartedAt = 0;
let netLogStoppedAt = 0;
let netLogEntries: any[] = [];
const NET_LOG_MAX_ENTRIES = 10_000;

function netLogUserDataPath(filePath: string): string {
  if (path.isAbsolute(filePath)) return filePath;
  const root = info.paths?.userData || info.paths?.appFolder || '/';
  return path.join(root, filePath);
}

/** Records one request in the active net log. Exported for the `net` shim. */
export function recordNetLog(entry: any): void {
  if (!netLogCurrentlyLogging || netLogEntries.length >= NET_LOG_MAX_ENTRIES) return;
  try {
    netLogEntries.push({time: Date.now(), ...entry});
  } catch { /* ignore entries that cannot be serialized */ }
}

function netLogText(state: string): string {
  const payload: any = {
    version: 1,
    source: 'rendrix',
    state,
    startedAt: new Date(netLogStartedAt || Date.now()).toISOString(),
    entries: netLogEntries,
  };
  if (netLogStoppedAt) payload.stoppedAt = new Date(netLogStoppedAt).toISOString();
  try { return JSON.stringify(payload, null, 2); } catch { return JSON.stringify({version: 1, source: 'rendrix', state}); }
}

async function writeNetLog(state: string): Promise<void> {
  if (!netLogFilePath) return;
  await fs.promises.writeFile(netLogFilePath, netLogText(state));
}

export class NetLog {
  get currentlyLogging() {
    return netLogCurrentlyLogging;
  }
  set currentlyLogging(value: boolean) {
    netLogCurrentlyLogging = !!value;
  }
  /** Starts the network log. The path must end in `.json`, as in Electron. */
  async startLogging(filePath?: string, _options?: any): Promise<void> {
    if (typeof filePath !== 'string' || !filePath) throw new TypeError('netLog.startLogging requires a .json file path');
    if (!/\.json$/i.test(filePath)) throw new Error('netLog.startLogging path must end with .json');
    netLogFilePath = netLogUserDataPath(filePath);
    netLogStartedAt = Date.now();
    netLogStoppedAt = 0;
    netLogEntries = [];
    netLogCurrentlyLogging = true;
    try {
      await writeNetLog('started');
    } catch (error: any) {
      netLogCurrentlyLogging = false;
      netLogFilePath = '';
      throw new Error(`netLog.startLogging cannot write ${filePath}: ${error?.message || error}`);
    }
  }
  /** Stops the network log and writes the final JSON file. */
  async stopLogging(): Promise<void> {
    if (!netLogCurrentlyLogging) {
      netLogCurrentlyLogging = false;
      return;
    }
    netLogCurrentlyLogging = false;
    netLogStoppedAt = Date.now();
    try {
      await writeNetLog('stopped');
    } catch (error: any) {
      compatWarnings.push(`netLog.stopLogging could not write ${netLogFilePath}: ${error?.message || error}`);
    } finally {
      netLogFilePath = '';
    }
  }
}

export const netLog = new NetLog();

// powerMonitor state tracking. The host answers `power.state` with the current AC/battery,
// idle, thermal and lock state, and pushes transitions through
// `globalThis.__rendrix_power_event`. The shim re-emits the matching Electron event.
let powerOnBatteryState = false;
let powerStateKnown = false;
let powerIdleTimeState = 0;
let powerThermalStateValue: string = 'unknown';
let powerLockedState = false;
let powerLockStateKnown = false;

function applyHostPowerState(state: any): void {
  if (!state || typeof state !== 'object') return;
  if (typeof state.onBattery === 'boolean') {
    if (powerStateKnown && state.onBattery !== powerOnBatteryState) {
      powerMonitor.emit(state.onBattery ? 'on-battery' : 'on-ac');
    }
    powerOnBatteryState = state.onBattery;
    powerStateKnown = true;
  }
  if (typeof state.idleTime === 'number' && Number.isFinite(state.idleTime)) {
    powerIdleTimeState = Math.max(0, state.idleTime);
  }
  if (typeof state.thermalState === 'string' && state.thermalState) {
    powerThermalStateValue = state.thermalState;
  }
  if (typeof state.locked === 'boolean') {
    if (powerLockStateKnown && state.locked !== powerLockedState) {
      powerMonitor.emit(state.locked ? 'lock-screen' : 'unlock-screen');
    }
    powerLockedState = state.locked;
    powerLockStateKnown = true;
  }
}

/** Rendrix addition: re-read the host's power state. */
export function refreshPowerState(): Promise<void> {
  return host('power.state').then(applyHostPowerState, () => undefined);
}

export class PowerMonitor extends EventEmitter {
  get onBatteryPower() {
    return powerOnBatteryState;
  }
  set onBatteryPower(value: boolean) {
    const next = !!value;
    if (next === powerOnBatteryState) return;
    powerOnBatteryState = next;
    powerStateKnown = true;
    this.emit(next ? 'on-battery' : 'on-ac');
  }
  /** Called by the host bridge when the OS reports a power transition. */
  handlePowerEvent(name: string, ...args: any[]): void {
    switch (name) {
      case 'on-ac':
        powerOnBatteryState = false;
        powerStateKnown = true;
        break;
      case 'on-battery':
        powerOnBatteryState = true;
        powerStateKnown = true;
        break;
      case 'lock-screen':
        powerLockedState = true;
        powerLockStateKnown = true;
        break;
      case 'unlock-screen':
        powerLockedState = false;
        powerLockStateKnown = true;
        break;
    }
    this.emit(name, ...args);
  }
  getSystemIdleState(idleThreshold?: number): string {
    if (powerLockStateKnown && powerLockedState) return 'locked';
    const threshold = typeof idleThreshold === 'number' && Number.isFinite(idleThreshold) ? idleThreshold : 0;
    if (threshold <= 0) return 'active';
    return powerIdleTimeState >= threshold ? 'idle' : 'active';
  }
  getSystemIdleTime(): number {
    return powerIdleTimeState;
  }
  getCurrentThermalState(): string {
    return powerThermalStateValue;
  }
  isOnBatteryPower(): boolean {
    return powerOnBatteryState;
  }
  /** Rendrix addition: re-read the host's power state. */
  refresh(): Promise<void> {
    return refreshPowerState();
  }
}

export const powerMonitor = new PowerMonitor();

// Host-driven power events. The native/WebView host calls this global when the OS reports a
// transition; it updates cached state and re-emits the Electron event.
const gPower: any = globalThis;
gPower.__rendrix_power_event = (name: string, ...args: any[]) => {
  try { (powerMonitor as any).handlePowerEvent?.(String(name), ...args); }
  catch (error) { try { console.error('powerMonitor event failed:', error); } catch {} }
};
// Aliases that hosts or tests may use.
gPower.__rendrix_powerEvent = gPower.__rendrix_power_event;
gPower.__rendrix_power = gPower.__rendrix_power_event;

// Seed the cached state from the host once the shim loads. Failures are expected in page-only
// builds and leave the media-query/local defaults in place.
void refreshPowerState();

export class PushNotifications extends EventEmitter {
  register(..._args: any[]): boolean {
    return false;
  }
  registerForAPNSNotifications(..._args: any[]): boolean {
    return false;
  }
  unregisterForAPNSNotifications(): void {
    // No-op: registration is never active.
  }
}

export const pushNotifications = new PushNotifications();

export default { netLog, powerMonitor, pushNotifications, NetLog, PowerMonitor, PushNotifications, recordNetLog, refreshPowerState };
