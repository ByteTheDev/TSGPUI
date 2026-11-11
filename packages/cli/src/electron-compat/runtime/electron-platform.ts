import { EventEmitter } from './events';

// Partial shim for Electron's ShareMenu. Native sharing is unavailable, so popup is a no-op.
export class ShareMenu {
  sharingItem: any;
  constructor(sharingItem?: any) {
    this.sharingItem = (sharingItem && typeof sharingItem === 'object' && sharingItem.sharingItem)
      ? sharingItem.sharingItem
      : (sharingItem ?? null);
  }
  popup(_options?: any): void { /* no-op */ }
  closePopup(): void { /* no-op */ }
}

// Partial shim for Electron's systemPreferences. Reports a light appearance by default and
// derives dark mode from the renderer's prefers-color-scheme media query when available.
class SystemPreferences extends EventEmitter {
  private dark: any;
  private reducedMotion: any;
  private inverted: any;
  private userDefaults = new Map<string, any>();
  defaults: Record<string, any> = {};
  accentColor: string = '';
  private _accessibilityDisplayShouldReduceMotion: boolean = false;
  isAeroGlassEnabled(): boolean { return false; }
  private notificationSubs = new Map<number, { event: string; callback: Function; scope: string }>();
  private nextNotificationId = 0;

  constructor() {
    super();
    const win: any = (globalThis as any)?.window;
    this.dark = win?.matchMedia?.('(prefers-color-scheme: dark)');
    if (this.dark?.addEventListener) this.dark.addEventListener('change', () => this.emit('color-changed'));
    else if (this.dark?.addListener) this.dark.addListener(() => this.emit('color-changed'));
    this.reducedMotion = win?.matchMedia?.('(prefers-reduced-motion: reduce)');
    if (this.reducedMotion?.addEventListener) this.reducedMotion.addEventListener('change', () => this.handleSystemEvent('accessibility-display-options-changed'));
    else if (this.reducedMotion?.addListener) this.reducedMotion.addListener(() => this.handleSystemEvent('accessibility-display-options-changed'));
    this.inverted = win?.matchMedia?.('(inverted-colors: inverted)');
    if (this.inverted?.addEventListener) this.inverted.addEventListener('change', () => this.emit('color-changed'));
    else if (this.inverted?.addListener) this.inverted.addListener(() => this.emit('color-changed'));
  }

  isDarkMode(): boolean { return !!this.dark?.matches; }
  getAccentColor(): string {
    // Best-effort: read the CSS accent-color of the root element when a document is present.
    try {
      const doc: any = (globalThis as any)?.document;
      const value = doc?.documentElement ? (globalThis as any)?.getComputedStyle?.(doc.documentElement)?.accentColor : '';
      if (typeof value === 'string' && value && value !== 'auto') return value.replace(/\s/g, '').toLowerCase();
    } catch {}
    return this.accentColor;
  }
  get effectiveAppearance(): string { return this.isDarkMode() ? 'dark' : 'light'; }
  set effectiveAppearance(_value: string) { /* read-only best-effort; ignored */ }
  getColor(_color: string): string { return ''; }
  getSystemColor(_color: string): string { return ''; }
  isInvertedColorScheme(): boolean { return !!this.inverted?.matches; }
  get accessibilityDisplayShouldReduceMotion(): boolean {
    if (this._accessibilityDisplayShouldReduceMotion) return true;
    return !!this.reducedMotion?.matches;
  }
  set accessibilityDisplayShouldReduceMotion(value: boolean) { this._accessibilityDisplayShouldReduceMotion = !!value; }
  getAnimationSettings(): any { return { shouldRenderRichAnimation: !this.accessibilityDisplayShouldReduceMotion, scrollAnimationsEnabledBySystem: true, prefersReducedMotion: this.accessibilityDisplayShouldReduceMotion }; }
  getMediaAccessStatus(_mediaType: string): string { return 'not-determined'; }
  askForMediaAccess(_mediaType: string): Promise<boolean> { return Promise.resolve(false); }
  canPromptTouchID(): boolean { return false; }
  promptTouchID(_reason: string): Promise<void> { return Promise.resolve(); }
  isTrustedAccessibilityClient(_prompt?: boolean): boolean { return false; }
  isSwipeTrackingFromScrollEventsEnabled(): boolean { return false; }
  accessibilityDisplayShouldReduceTransparency: boolean = false;

  handleSystemEvent(name: string, ...args: any[]): void {
    if (name === 'accessibility-display-options-changed') {
      this.emit('accessibility-display-options-changed' as any);
    }
    this.emit(name, ...args);
  }

  getUserDefault(key: string, _type: string): any {
    if (this.userDefaults.has(key)) return this.userDefaults.get(key);
    if (Object.prototype.hasOwnProperty.call(this.defaults, key)) return this.defaults[key];
    return null;
  }
  setUserDefault(key: string, _type: string, value: any): void { this.userDefaults.set(key, value); }
  removeUserDefault(key: string): void { this.userDefaults.delete(key); }
  registerDefaults(defaults: Record<string, any>): void { Object.assign(this.defaults, defaults || {}); }

  postNotification(event: string, _userInfo?: any, _deliverImmediately?: boolean): void {
    for (const sub of this.notificationSubs.values()) {
      if (sub.scope === 'notification' && sub.event === event) try { sub.callback(_userInfo); } catch { /* ignore */ }
    }
  }
  postLocalNotification(event: string, _userInfo?: any): void {
    for (const sub of this.notificationSubs.values()) {
      if (sub.scope === 'local' && sub.event === event) try { sub.callback(_userInfo); } catch { /* ignore */ }
    }
  }
  postWorkspaceNotification(event: string, _userInfo?: any): void {
    for (const sub of this.notificationSubs.values()) {
      if (sub.scope === 'workspace' && sub.event === event) try { sub.callback(_userInfo); } catch { /* ignore */ }
    }
  }
  subscribeNotification(event: string, callback: Function): number {
    const id = ++this.nextNotificationId;
    this.notificationSubs.set(id, { event, callback, scope: 'notification' });
    return id;
  }
  subscribeLocalNotification(event: string, callback: Function): number {
    const id = ++this.nextNotificationId;
    this.notificationSubs.set(id, { event, callback, scope: 'local' });
    return id;
  }
  subscribeWorkspaceNotification(event: string, callback: Function): number {
    const id = ++this.nextNotificationId;
    this.notificationSubs.set(id, { event, callback, scope: 'workspace' });
    return id;
  }
  unsubscribeNotification(id: number): void { this.notificationSubs.delete(id); }
  unsubscribeLocalNotification(id: number): void { this.notificationSubs.delete(id); }
  unsubscribeWorkspaceNotification(id: number): void { this.notificationSubs.delete(id); }
}
export const systemPreferences = new SystemPreferences();

const gSystem: any = globalThis;
gSystem.__tsgpui_system_preferences_event = (name: string, ...args: any[]) => {
  try {
    const sp = systemPreferences as any;
    if (typeof sp.handleSystemEvent === 'function') sp.handleSystemEvent(String(name), ...args);
    else sp.emit?.(String(name), ...args);
  } catch (error) { try { console.error('systemPreferences event failed:', error); } catch {} }
};
gSystem.__tsgpui_system_event = gSystem.__tsgpui_system_preferences_event;
gSystem.__tsgpui_theme_event = gSystem.__tsgpui_system_preferences_event;

// Partial shim for Electron's TouchBar. Returns an inspectable object.
export class TouchBarButton { constructor(options?: any) { Object.assign(this, options || {}); } }
export class TouchBarColorPicker { constructor(options?: any) { Object.assign(this, options || {}); } }
export class TouchBarGroup { constructor(options?: any) { Object.assign(this, options || {}); } }
export class TouchBarLabel { constructor(options?: any) { Object.assign(this, options || {}); } }
export class TouchBarOtherItemsProxy { constructor(options?: any) { Object.assign(this, options || {}); } }
export class TouchBarPopover { constructor(options?: any) { Object.assign(this, options || {}); } }
export class TouchBarScrubber { constructor(options?: any) { Object.assign(this, options || {}); } }
export class TouchBarSegmentedControl { constructor(options?: any) { Object.assign(this, options || {}); } }
export class TouchBarSlider { constructor(options?: any) { Object.assign(this, options || {}); } }
export class TouchBarSpacer { constructor(options?: any) { Object.assign(this, options || {}); } }

export class TouchBar {
  items: any;
  escapeItem: any;
  constructor(options?: any) {
    const opts = options || {};
    this.items = opts.items ?? [];
    this.escapeItem = opts.escapeItem ?? null;
    for (const key of Object.keys(opts)) {
      if (key !== 'items' && key !== 'escapeItem') (this as any)[key] = opts[key];
    }
  }
  static TouchBarButton = TouchBarButton;
  static TouchBarColorPicker = TouchBarColorPicker;
  static TouchBarGroup = TouchBarGroup;
  static TouchBarLabel = TouchBarLabel;
  static TouchBarOtherItemsProxy = TouchBarOtherItemsProxy;
  static TouchBarPopover = TouchBarPopover;
  static TouchBarScrubber = TouchBarScrubber;
  static TouchBarSegmentedControl = TouchBarSegmentedControl;
  static TouchBarSlider = TouchBarSlider;
  static TouchBarSpacer = TouchBarSpacer;
}

export default { ShareMenu, systemPreferences, TouchBar };