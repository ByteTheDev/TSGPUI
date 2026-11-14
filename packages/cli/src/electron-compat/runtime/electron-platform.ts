import { EventEmitter } from './events';
import { host } from './host';

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

// Electron reports system colors as `#rrggbb`. These are the documented names; a host that
// answers `systemPreferences.info` with a `colors` map overrides them.
const LIGHT_COLORS: Record<string, string> = {
  '3d-dark-shadow': '#696969', '3d-face': '#f0f0f0', '3d-highlight': '#ffffff', '3d-light': '#e3e3e3',
  '3d-shadow': '#a0a0a0', 'active-border': '#707070', 'active-caption': '#99b4d1',
  'active-caption-gradient': '#99b4d1', 'app-workspace': '#ababab', 'button-face': '#f0f0f0',
  'button-text': '#000000', 'caption-text': '#000000', 'desktop': '#000000', 'highlight': '#0078d7',
  'highlight-text': '#ffffff', 'hotlight': '#0066cc', 'inactive-border': '#acacac',
  'inactive-caption': '#bfcddb', 'inactive-caption-gradient': '#bfcddb', 'inactive-caption-text': '#434e54',
  'info-background': '#ffffe1', 'info-text': '#000000', 'menu': '#f0f0f0', 'menu-highlight': '#0078d7',
  'menubar': '#f0f0f0', 'menu-text': '#000000', 'scrollbar': '#c8c8c8', 'window': '#ffffff',
  'window-frame': '#646464', 'window-text': '#000000',
};
const DARK_COLORS: Record<string, string> = {
  '3d-dark-shadow': '#202020', '3d-face': '#2d2d2d', '3d-highlight': '#454545', '3d-light': '#3a3a3a',
  '3d-shadow': '#202020', 'active-border': '#555555', 'active-caption': '#202020',
  'active-caption-gradient': '#202020', 'app-workspace': '#1e1e1e', 'button-face': '#2d2d2d',
  'button-text': '#ffffff', 'caption-text': '#ffffff', 'desktop': '#000000', 'highlight': '#0078d7',
  'highlight-text': '#ffffff', 'hotlight': '#4da3ff', 'inactive-border': '#444444',
  'inactive-caption': '#2b2b2b', 'inactive-caption-gradient': '#2b2b2b', 'inactive-caption-text': '#b8b8b8',
  'info-background': '#2b2b2b', 'info-text': '#ffffff', 'menu': '#202020', 'menu-highlight': '#0078d7',
  'menubar': '#202020', 'menu-text': '#ffffff', 'scrollbar': '#3a3a3a', 'window': '#1e1e1e',
  'window-frame': '#444444', 'window-text': '#ffffff',
};

function paletteColor(name: string, dark: boolean): string {
  return (dark ? DARK_COLORS : LIGHT_COLORS)[String(name)] || '';
}

// systemPreferences reads the OS appearance, accent color, accessibility flags and system color
// palette from the host (`systemPreferences.info`). A renderer without a host falls back to media
// queries and a built-in light/dark palette.
class SystemPreferences extends EventEmitter {
  private dark: any;
  private reducedMotion: any;
  private inverted: any;
  private userDefaults = new Map<string, any>();
  defaults: Record<string, any> = {};
  accentColor: string = '';
  private hostDark: boolean | null = null;
  private hostAccent = '';
  private hostReduceMotion: boolean | null = null;
  private hostInverted: boolean | null = null;
  private hostHighContrast = false;
  private hostReduceTransparency = false;
  private hostColors: Record<string, string> = {};
  private _accessibilityDisplayShouldReduceMotion: boolean = false;
  private _accessibilityDisplayShouldReduceTransparency: boolean = false;
  private lastDark = false;
  private lastAccent = '';
  private lastReduceMotion = false;
  private lastInverted = false;
  private lastHighContrast = false;
  private lastReduceTransparency = false;
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
    this.lastDark = !!this.dark?.matches;
    this.lastReduceMotion = !!this.reducedMotion?.matches;
    this.lastInverted = !!this.inverted?.matches;
    void this.refresh();
  }

  /** Rendrix addition: re-read the host's system preference values. */
  async refresh(): Promise<void> {
    try {
      const values = await host('systemPreferences.info');
      if (values && typeof values === 'object') this.applyHostValues(values);
    } catch {
      // No host, or the host predates this operation: media queries and the palette still work.
    }
  }

  private applyHostValues(values: any): void {
    if (typeof values.darkMode === 'boolean') this.hostDark = values.darkMode;
    if (typeof values.accentColor === 'string' && values.accentColor) { this.hostAccent = values.accentColor; this.accentColor = values.accentColor; }
    if (typeof values.reduceMotion === 'boolean') this.hostReduceMotion = values.reduceMotion;
    if (typeof values.invertedColors === 'boolean') this.hostInverted = values.invertedColors;
    if (typeof values.highContrast === 'boolean') this.hostHighContrast = values.highContrast;
    if (typeof values.reduceTransparency === 'boolean') this.hostReduceTransparency = values.reduceTransparency;
    if (values.colors && typeof values.colors === 'object') {
      const next: Record<string, string> = {};
      for (const key of Object.keys(values.colors)) {
        if (typeof values.colors[key] === 'string' && values.colors[key]) next[key] = values.colors[key];
      }
      this.hostColors = next;
    }
    const dark = this.isDarkMode();
    const accent = this.getAccentColor();
    const reduceMotion = this.accessibilityDisplayShouldReduceMotion;
    const inverted = this.isInvertedColorScheme();
    let changed = false;
    if (dark !== this.lastDark) { this.lastDark = dark; changed = true; }
    if (accent !== this.lastAccent) { this.lastAccent = accent; changed = true; this.emit('accent-color-changed', {}, accent); }
    if (reduceMotion !== this.lastReduceMotion) { this.lastReduceMotion = reduceMotion; this.emit('accessibility-display-options-changed'); }
    if (inverted !== this.lastInverted) { this.lastInverted = inverted; changed = true; }
    if (this.hostHighContrast !== this.lastHighContrast) { this.lastHighContrast = this.hostHighContrast; changed = true; }
    if (this.hostReduceTransparency !== this.lastReduceTransparency) { this.lastReduceTransparency = this.hostReduceTransparency; changed = true; }
    if (changed) this.emit('color-changed');
  }

  isAeroGlassEnabled(): boolean { return false; }
  isDarkMode(): boolean {
    if (this.hostDark !== null) return this.hostDark;
    return !!this.dark?.matches;
  }
  getAccentColor(): string {
    if (this.hostAccent) return this.hostAccent;
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
  getColor(color: string): string {
    const key = String(color);
    if (this.hostColors[key]) return this.hostColors[key];
    return paletteColor(key, this.isDarkMode());
  }
  getSystemColor(color: string): string { return this.getColor(color); }
  isInvertedColorScheme(): boolean {
    if (this.hostInverted !== null) return this.hostInverted;
    return !!this.inverted?.matches;
  }
  isHighContrast(): boolean {
    if (this.hostHighContrast) return true;
    try { return !!(globalThis as any)?.matchMedia?.('(forced-colors: active)')?.matches; } catch { return false; }
  }
  get accessibilityDisplayShouldReduceMotion(): boolean {
    if (this._accessibilityDisplayShouldReduceMotion) return true;
    if (this.hostReduceMotion !== null) return this.hostReduceMotion;
    return !!this.reducedMotion?.matches;
  }
  set accessibilityDisplayShouldReduceMotion(value: boolean) { this._accessibilityDisplayShouldReduceMotion = !!value; }
  get accessibilityDisplayShouldReduceTransparency(): boolean {
    if (this._accessibilityDisplayShouldReduceTransparency) return true;
    return this.hostReduceTransparency;
  }
  set accessibilityDisplayShouldReduceTransparency(value: boolean) { this._accessibilityDisplayShouldReduceTransparency = !!value; }
  getAnimationSettings(): any {
    const reduced = this.accessibilityDisplayShouldReduceMotion;
    return { shouldRenderRichAnimation: !reduced, scrollAnimationsEnabledBySystem: !reduced, prefersReducedMotion: reduced };
  }
  getMediaAccessStatus(_mediaType: string): string { return 'not-determined'; }
  askForMediaAccess(_mediaType: string): Promise<boolean> { return Promise.resolve(false); }
  canPromptTouchID(): boolean { return false; }
  promptTouchID(_reason: string): Promise<void> { return Promise.resolve(); }
  isTrustedAccessibilityClient(_prompt?: boolean): boolean { return false; }
  isSwipeTrackingFromScrollEventsEnabled(): boolean { return false; }

  handleSystemEvent(name: string, ...args: any[]): void {
    const event = String(name);
    if (event === 'color-changed' || event === 'accent-color-changed' || event === 'accessibility-display-options-changed') {
      void this.refresh();
    }
    if (event === 'accessibility-display-options-changed') {
      this.emit('accessibility-display-options-changed' as any);
    }
    this.emit(event, ...args);
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
gSystem.__rendrix_system_preferences_event = (name: string, ...args: any[]) => {
  try {
    const sp = systemPreferences as any;
    if (typeof sp.handleSystemEvent === 'function') sp.handleSystemEvent(String(name), ...args);
    else sp.emit?.(String(name), ...args);
  } catch (error) { try { console.error('systemPreferences event failed:', error); } catch {} }
};
gSystem.__rendrix_system_event = gSystem.__rendrix_system_preferences_event;
gSystem.__rendrix_theme_event = gSystem.__rendrix_system_preferences_event;

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