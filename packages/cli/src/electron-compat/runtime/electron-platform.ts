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

// Partial shim for Electron's systemPreferences. Reports a light, default appearance.
class SystemPreferences extends EventEmitter {
  private defaults: Record<string, any> = {};
  isDarkMode(): boolean { return false; }
  getAccentColor(): string { return this.accentColor; }
  accentColor: string = '';
  getEffectiveAppearance(): string { return 'light'; }
  effectiveAppearance: string = 'light';
  isAeroGlassEnabled(): boolean { return false; }
  getColor(_color: string): string { return ''; }
  getSystemColor(_color: string): string { return ''; }
  getAnimationSettings(): any { return {shouldRenderRichAnimation: true, scrollAnimationsEnabledBySystem: true, prefersReducedMotion: false}; }
  getMediaAccessStatus(_mediaType: string): string { return 'not-determined'; }
  askForMediaAccess(_mediaType: string): Promise<boolean> { return Promise.resolve(false); }
  canPromptTouchID(): boolean { return false; }
  promptTouchID(_reason: string): Promise<void> { return Promise.resolve(); }
  isTrustedAccessibilityClient(_prompt?: boolean): boolean { return false; }
  isSwipeTrackingFromScrollEventsEnabled(): boolean { return false; }
  getUserDefault(_key: string, _type: string): any { return null; }
  setUserDefault(_key: string, _type: string, _value: any): void { /* no-op */ }
  removeUserDefault(_key: string): void { /* no-op */ }
  registerDefaults(defaults: Record<string, any>): void { Object.assign(this.defaults, defaults || {}); }
  postNotification(_event: string, _userInfo?: any, _deliverImmediately?: boolean): void { /* no-op */ }
  postLocalNotification(_event: string, _userInfo?: any): void { /* no-op */ }
  postWorkspaceNotification(_event: string, _userInfo?: any): void { /* no-op */ }
  subscribeNotification(_event: string, _callback: Function): number { return 0; }
  subscribeLocalNotification(_event: string, _callback: Function): number { return 0; }
  subscribeWorkspaceNotification(_event: string, _callback: Function): number { return 0; }
  unsubscribeNotification(_id: number): void { /* no-op */ }
  unsubscribeLocalNotification(_id: number): void { /* no-op */ }
  unsubscribeWorkspaceNotification(_id: number): void { /* no-op */ }
}
export const systemPreferences = new SystemPreferences();

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
  escapeItem: any;
  constructor(options?: any) {
    const instance: any = {};
    const opts = options || {};
    instance.escapeItem = opts.escapeItem ?? null;
    for (const key of Object.keys(opts)) instance[key] = opts[key];
    return instance;
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
