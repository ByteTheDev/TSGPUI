import { compatWarnings } from './host';

// Menu models are usable for inspecting and changing application menus. Native menu
// rendering is not yet provided by the host, and is reported explicitly below.
export class MenuItem {
  id: string; label: string; sublabel: string; type: string; role?: string;
  enabled: boolean; visible: boolean; checked: boolean; accelerator?: string;
  click?: Function; submenu: Menu | null; menu: Menu | null = null;
  constructor(options: any = {}) {
    if (!options || typeof options !== 'object') throw new TypeError('MenuItem options must be an object');
    this.type = options.type || (options.submenu ? 'submenu' : 'normal');
    if (!['normal', 'separator', 'submenu', 'checkbox', 'radio'].includes(this.type)) throw new TypeError(`Invalid menu item type: ${this.type}`);
    this.id = options.id || ''; this.label = options.label || ''; this.sublabel = options.sublabel || '';
    this.role = options.role; this.accelerator = options.accelerator; this.click = options.click;
    this.enabled = options.enabled !== false; this.visible = options.visible !== false; this.checked = options.checked === true;
    this.submenu = Array.isArray(options.submenu) ? Menu.fromTemplate(options.submenu) : options.submenu || null;
    if (this.submenu && !(this.submenu instanceof Menu)) throw new TypeError('submenu must be a Menu or an array of menu item options');
  }
}

export class Menu {
  items: MenuItem[] = [];
  private static applicationMenu: Menu | null = null;
  append(item: MenuItem) { this.insert(this.items.length, item); }
  insert(index: number, item: MenuItem) {
    if (!(item instanceof MenuItem)) throw new TypeError('Menu.insert requires a MenuItem');
    if (!Number.isInteger(index) || index < 0 || index > this.items.length) throw new RangeError('Invalid menu index');
    if (item.menu) throw new Error('MenuItem already belongs to a menu');
    item.menu = this; this.items.splice(index, 0, item);
  }
  getMenuItemById(id: string): MenuItem | null {
    for (const item of this.items) { if (item.id === id) return item; const child = item.submenu?.getMenuItemById(id); if (child) return child; }
    return null;
  }
  popup() { throw new Error('Menu.popup is not supported: native menus are not available'); }
  closePopup() { throw new Error('Menu.closePopup is not supported: native menus are not available'); }
  static fromTemplate(template: any[]) {
    if (!Array.isArray(template)) throw new TypeError('Menu template must be an array');
    const menu = new Menu(); for (const entry of template) menu.append(entry instanceof MenuItem ? entry : new MenuItem(entry)); return menu;
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
}
