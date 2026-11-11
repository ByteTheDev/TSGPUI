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

// powerMonitor state tracking (no power events are emitted).
let powerOnBatteryState = false;
let powerIdleTimeState = 0;
let powerIdleStateValue: string = 'active';
let powerThermalStateValue: string = 'nominal';

export class PowerMonitor extends EventEmitter {
  get onBatteryPower() {
    return powerOnBatteryState;
  }
  set onBatteryPower(value: boolean) {
    powerOnBatteryState = !!value;
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