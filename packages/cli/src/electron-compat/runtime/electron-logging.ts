import { EventEmitter } from './events';

// Partial shims for Electron modules that TSGPUI does not back with host support.
// These keep application code runnable while documenting the missing behaviour.

let netLogCurrentlyLogging = false;

export class NetLog {
  get currentlyLogging() {
    return netLogCurrentlyLogging;
  }
  set currentlyLogging(value: boolean) {
    netLogCurrentlyLogging = !!value;
  }
  async startLogging(_path?: string, _options?: any): Promise<void> {
    // Network logging is not available in the embedded engines.
    netLogCurrentlyLogging = true;
  }
  async stopLogging(): Promise<void> {
    netLogCurrentlyLogging = false;
  }
}

export const netLog = new NetLog();

// powerMonitor state tracking. Power events are delivered by the host through
// `globalThis.__tsgpui_power_event`; the setters below keep the cached state in
// sync and emit the matching Electron event when it changes.
let powerOnBatteryState = false;
let powerIdleTimeState = 0;
let powerIdleStateValue: string = 'active';
let powerThermalStateValue: string = 'nominal';

export class PowerMonitor extends EventEmitter {
  get onBatteryPower() {
    return powerOnBatteryState;
  }
  set onBatteryPower(value: boolean) {
    const next = !!value;
    if (next === powerOnBatteryState) return;
    powerOnBatteryState = next;
    this.emit(next ? 'on-battery' : 'on-ac');
  }
  /** Called by the host bridge when the OS reports a power transition. */
  handlePowerEvent(name: string, ...args: any[]): void {
    switch (name) {
      case 'suspend':
      case 'resume':
      case 'shutdown':
      case 'lock-screen':
      case 'unlock-screen':
        break;
      case 'on-ac':
        powerOnBatteryState = false;
        break;
      case 'on-battery':
        powerOnBatteryState = true;
        break;
    }
    this.emit(name, ...args);
  }
  getSystemIdleState(_idleThreshold?: number): string {
    const threshold = typeof _idleThreshold === 'number' && Number.isFinite(_idleThreshold) ? _idleThreshold : 0;
    if (powerIdleTimeState >= threshold && threshold > 0) {
      powerIdleStateValue = 'idle';
    } else if (powerIdleTimeState === 0) {
      powerIdleStateValue = 'active';
    }
    return powerIdleStateValue;
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
}

export const powerMonitor = new PowerMonitor();

// Host-driven power events. The native/WebView host calls this global when the OS
// reports a transition; it updates cached state and re-emits the Electron event.
const gPower: any = globalThis;
gPower.__tsgpui_power_event = (name: string, ...args: any[]) => {
  try { (powerMonitor as any).handlePowerEvent?.(String(name), ...args); }
  catch (error) { try { console.error('powerMonitor event failed:', error); } catch {} }
};
// Aliases that hosts or tests may use.
gPower.__tsgpui_powerEvent = gPower.__tsgpui_power_event;
gPower.__tsgpui_power = gPower.__tsgpui_power_event;

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

export default { netLog, powerMonitor, pushNotifications, NetLog, PowerMonitor, PushNotifications };