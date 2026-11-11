import { compatWarnings, host } from './host';
import { EventEmitter } from './events';

const ROLES = [
  'quit', 'copy', 'paste', 'selectAll', 'undo', 'redo', 'cut', 'minimize', 'close', 'about',
  'hide', 'hideOthers', 'unhide', 'toggleDevTools', 'reload', 'forceReload', 'toggleFullScreen',
  'window', 'help', 'appMenu', 'fileMenu', 'editMenu', 'viewMenu', 'recentDocuments',
  'clearRecentDocuments', 'services', 'zoom', 'zoomIn', 'zoomOut', 'resetZoom',
];

const ROLE_LABELS: Record<string, string> = {
  quit: 'Quit',
  copy: 'Copy',
  paste: 'Paste',
  selectAll: 'Select All',
  undo: 'Undo',
  redo: 'Redo',
  cut: 'Cut',
  minimize: 'Minimize',
  close: 'Close',
  about: 'About',
  hide: 'Hide',
  hideOthers: 'Hide Others',
  unhide: 'Show All',
  toggleDevTools: 'Toggle Developer Tools',
  reload: 'Reload',
  forceReload: 'Force Reload',
  toggleFullScreen: 'Toggle Full Screen',
  window: 'Window',
  help: 'Help',
  appMenu: 'App',
  fileMenu: 'File',
  editMenu: 'Edit',
  viewMenu: 'View',
  recentDocuments: 'Open Recent',
  clearRecentDocuments: 'Clear Recent Documents',
  services: 'Services',
  zoom: 'Zoom',
  zoomIn: 'Zoom In',
  zoomOut: 'Zoom Out',
  resetZoom: 'Actual Size',
};

const ROLE_CLICKS: Record<string, () => void> = {
  quit: () => { try { void host('app.quit'); } catch {} },
  copy: () => { try { (globalThis as any).document?.execCommand?.('copy'); } catch {} },
  paste: () => { try { (globalThis as any).document?.execCommand?.('paste'); } catch {} },
  cut: () => { try { (globalThis as any).document?.execCommand?.('cut'); } catch {} },
  selectAll: () => { try { (globalThis as any).document?.execCommand?.('selectAll'); } catch {} },
  undo: () => { try { (globalThis as any).document?.execCommand?.('undo'); } catch {} },
  redo: () => { try { (globalThis as any).document?.execCommand?.('redo'); } catch {} },
  reload: () => { try { (globalThis as any).location?.reload?.(); } catch {} },
  forceReload: () => { try { (globalThis as any).location?.reload?.(); } catch {} },
};

function isPlainObject(value: any): boolean {
  return value !== null && typeof value === 'object' && !Array.isArray(value);
}

export class MenuItem {
  id: string;
  label: string;
  sublabel: string;
  type: string;
  role?: string;
  enabled: boolean;
  visible: boolean;
  checked: boolean;
  accelerator?: string;
  click?: Function;
  private _submenu: Menu | null = null;
  get submenu(): Menu | null { return this._submenu; }
  set submenu(value: any) {
    if (value === undefined || value === null) { this._submenu = null; return; }
    if (Array.isArray(value)) { this._submenu = Menu.fromTemplate(value); }
    else if (value instanceof Menu) { this._submenu = value; }
    else { throw new TypeError('submenu must be a Menu or an array of menu item options'); }
    if (this._submenu) this.type = 'submenu';
  }
  menu: Menu | null = null;
  commandId: number;
  icon?: any;
  toolTip?: string;
  registerAccelerator: boolean;
  userAccelerator?: string;
  sharingItem?: any;
  private static nextCommandId = 1;

  constructor(options: any = {}) {
    if (options === undefined) options = {};
    if (!isPlainObject(options)) throw new TypeError('MenuItem options must be an object');

    // type
    if (options.type !== undefined && typeof options.type !== 'string') throw new TypeError('MenuItem type must be a string');
    const hasSubmenu = Array.isArray(options.submenu) || options.submenu instanceof Menu;
    this.type = options.type || (hasSubmenu ? 'submenu' : 'normal');
    if (!['normal', 'separator', 'submenu', 'checkbox', 'radio'].includes(this.type)) throw new TypeError(`Invalid menu item type: ${this.type}`);

    // id
    if (options.id !== undefined && typeof options.id !== 'string') throw new TypeError('MenuItem id must be a string');
    this.id = options.id || '';

    // role
    if (options.role !== undefined) {
      if (typeof options.role !== 'string') throw new TypeError('MenuItem role must be a string');
      if (!ROLES.includes(options.role)) throw new TypeError(`Invalid menu item role: ${options.role}`);
    }
    this.role = options.role;

    // label / sublabel
    if (options.label !== undefined && typeof options.label !== 'string') throw new TypeError('MenuItem label must be a string');
    if (options.sublabel !== undefined && typeof options.sublabel !== 'string') throw new TypeError('MenuItem sublabel must be a string');
    if (options.label === undefined && this.role && ROLE_LABELS[this.role]) this.label = ROLE_LABELS[this.role];
    else this.label = options.label || '';
    this.sublabel = options.sublabel || '';

    // accelerator
    if (options.accelerator !== undefined && typeof options.accelerator !== 'string') throw new TypeError('MenuItem accelerator must be a string');
    this.accelerator = options.accelerator;
    if (options.userAccelerator !== undefined && typeof options.userAccelerator !== 'string') throw new TypeError('MenuItem userAccelerator must be a string');
    this.userAccelerator = options.userAccelerator;

    // click
    if (options.click !== undefined && typeof options.click !== 'function') throw new TypeError('MenuItem click must be a function');
    if (options.click) this.click = options.click;
    else if (this.role && ROLE_CLICKS[this.role]) this.click = ROLE_CLICKS[this.role];

    // booleans (aliases handled in constructor)
    this.enabled = options.enabled !== false && options.disabled !== true;
    this.visible = options.visible !== false && options.hidden !== true;
    this.checked = options.checked === true;

    // icon / toolTip
    this.icon = options.icon;
    if (options.toolTip !== undefined && typeof options.toolTip !== 'string') throw new TypeError('MenuItem toolTip must be a string');
    this.toolTip = options.toolTip;
    this.registerAccelerator = options.registerAccelerator !== false;
    this.sharingItem = options.sharingItem;

    // submenu
    if (options.submenu !== undefined) this.submenu = options.submenu;
    if (this.submenu) this.type = 'submenu';

    this.commandId = MenuItem.nextCommandId++;
  }

  get disabled(): boolean { return !this.enabled; }
  set disabled(value: boolean) { this.enabled = !value; }
  get hidden(): boolean { return !this.visible; }
  set hidden(value: boolean) { this.visible = !value; }
  get hasRole(): boolean { return !!this.role; }
}

export class Menu extends EventEmitter {
  private _items: MenuItem[] = [];
  private static applicationMenu: Menu | null = null;

  get items(): MenuItem[] { return this._items; }
  set items(value: any) {
    if (!Array.isArray(value)) throw new TypeError('Menu items must be an array');
    const normalized: MenuItem[] = [];
    for (const entry of value) {
      const item = entry instanceof MenuItem ? entry : new MenuItem(entry);
      if (item.menu && item.menu !== this) throw new Error('MenuItem already belongs to a menu');
      item.menu = this;
      normalized.push(item);
    }
    // detach old
    for (const old of this._items) if (!normalized.includes(old)) old.menu = null;
    this._items = normalized;
  }

  append(item: MenuItem) { this.insert(this._items.length, item); }
  insert(index: number, item: MenuItem) {
    if (!(item instanceof MenuItem)) throw new TypeError('Menu.insert requires a MenuItem');
    if (!Number.isInteger(index) || index < 0 || index > this._items.length) throw new RangeError('Invalid menu index');
    if (item.menu) throw new Error('MenuItem already belongs to a menu');
    item.menu = this;
    this._items.splice(index, 0, item);
  }
  remove(arg: any): any {
    if (arg instanceof MenuItem) return this.removeMenuItem(arg);
    return this.removeAt(arg);
  }
  removeAt(index: number): MenuItem | undefined {
    if (!Number.isInteger(index) || index < 0 || index >= this._items.length) throw new RangeError('Invalid menu index');
    const [removed] = this._items.splice(index, 1);
    if (removed) removed.menu = null;
    return removed;
  }
  delete(index: number): MenuItem | undefined { return this.removeAt(index); }
  removeMenuItem(item: MenuItem): boolean {
    const index = this._items.indexOf(item);
    if (index < 0) return false;
    this.removeAt(index);
    return true;
  }
  getMenuItemById(id: string): MenuItem | null {
    for (const item of this._items) {
      if (item.id === id) return item;
      const child = item.submenu?.getMenuItemById(id);
      if (child) return child;
    }
    return null;
  }
  popup(..._args: any[]) { throw new Error('Menu.popup is not supported: native menus are not available'); }
  closePopup(..._args: any[]) { throw new Error('Menu.closePopup is not supported: native menus are not available'); }
  static fromTemplate(template: any[]) {
    if (!Array.isArray(template)) throw new TypeError('Menu template must be an array');
    const menu = new Menu();
    for (const entry of template) menu.append(entry instanceof MenuItem ? entry : new MenuItem(entry));
    return menu;
  }
  static buildFromTemplate(template: any[]) {
    compatWarnings.push('Menu.buildFromTemplate is not supported and has no effect on native UI');
    return Menu.fromTemplate(template);
  }
  static setApplicationMenu(menu: Menu | null) {
    if (menu !== null && !(menu instanceof Menu)) throw new TypeError('Application menu must be a Menu or null');
    Menu.applicationMenu = menu;
    compatWarnings.push('Menu.setApplicationMenu is not supported and has no effect on native UI');
  }
  static getApplicationMenu() { return Menu.applicationMenu; }
  static sendActionToFirstResponder(_action?: string) {
    compatWarnings.push('Menu.sendActionToFirstResponder is not supported and has no effect on native UI');
  }
  sendActionToFirstResponder(_action?: string) {
    compatWarnings.push('Menu.sendActionToFirstResponder is not supported and has no effect on native UI');
  }
}