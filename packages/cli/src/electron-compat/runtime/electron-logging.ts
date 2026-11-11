import { EventEmitter } from './events';

// Partial shims for Electron modules that TSGPUI does not back with host support.
// These keep application code runnable while documenting the missing behaviour.

export const netLog = {
  currentlyLogging: false,
  async startLogging(_path?: string, _options?: any): Promise<void> {
    // Network logging is not available in the embedded engines.
  },
  async stopLogging(): Promise<void> {
    // No-op: there is never an active network log.
  },
};

export const powerMonitor = Object.assign(new EventEmitter(), {
  onBatteryPower: false,
  getSystemIdleState(_idleThreshold?: number): string {
    return 'active';
  },
  getSystemIdleTime(): number {
    return 0;
  },
  getCurrentThermalState(): string {
    return 'nominal';
  },
  isOnBatteryPower(): boolean {
    return false;
  },
});

export const pushNotifications = Object.assign(new EventEmitter(), {
  register(): boolean {
    return false;
  },
  registerForAPNSNotifications(): boolean {
    return false;
  },
  unregisterForAPNSNotifications(): void {
    // No-op: registration is never active.
  },
});

export default { netLog, powerMonitor, pushNotifications };