import { EventEmitter } from './events';

// Browser media queries supply a best-effort theme when this runtime has a
// renderer window; the main-process host does not yet expose OS theme events.
class NativeTheme extends EventEmitter {
  private source = 'system';
  private dark: any;
  private contrast: any;
  private inverted: any;
  private reducedTransparency: any;
  private previous: string;
  constructor() {
    super();
    const win: any = (globalThis as any).window;
    this.dark = win?.matchMedia?.('(prefers-color-scheme: dark)');
    this.contrast = win?.matchMedia?.('(forced-colors: active)');
    this.inverted = win?.matchMedia?.('(inverted-colors: inverted)');
    this.reducedTransparency = win?.matchMedia?.('(prefers-reduced-transparency: reduce)');
    this.previous = this.snapshot();
    for (const query of [this.dark, this.contrast, this.inverted, this.reducedTransparency]) {
      const update = () => this.changed();
      if (query?.addEventListener) query.addEventListener('change', update);
      else if (query?.addListener) query.addListener(update);
    }
  }
  private snapshot() { return `${this.shouldUseDarkColors}:${this.shouldUseHighContrastColors}:${this.shouldUseInvertedColorScheme}:${this.prefersReducedTransparency}`; }
  private changed() { const next = this.snapshot(); if (next !== this.previous) { this.previous = next; this.emit('updated'); } }
  get themeSource() { return this.source; }
  set themeSource(value: string) {
    if (!['system', 'light', 'dark'].includes(value)) throw new TypeError('themeSource must be system, light, or dark');
    this.source = value;
    this.changed();
  }
  get shouldUseDarkColors() { return this.source === 'dark' || (this.source === 'system' && !!this.dark?.matches); }
  get shouldUseHighContrastColors() { return !!this.contrast?.matches; }
  get shouldUseInvertedColorScheme() { return !!this.inverted?.matches; }
  get inForcedColorsMode() { return !!this.contrast?.matches; }
  get prefersReducedTransparency() { return !!this.reducedTransparency?.matches; }
}
export const nativeTheme = new NativeTheme();