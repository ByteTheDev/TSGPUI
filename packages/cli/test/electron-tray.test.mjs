import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import vm from 'node:vm';
import ts from 'typescript';
import { fileURLToPath } from 'node:url';

// Tray and globalShortcut are host-backed shims. These tests run them in a VM with a fake bridge,
// the way both the WebView backend and the native backend load the compatibility runtime.
const dir = fileURLToPath(new URL('../src/electron-compat/runtime/', import.meta.url));

function runtime() {
  const calls = [];
  const listeners = new Map();
  const host = async (op, args = {}) => { calls.push([op, args]); return {}; };
  const context = vm.createContext({ console, Uint8Array, ArrayBuffer, Promise, Map, setTimeout, __tsgpui_host: host });
  const cache = new Map();
  function load(name) {
    name = name.replace(/^\.\//, '').replace(/\.ts$/, '');
    if (cache.has(name)) return cache.get(name).exports;
    const module = { exports: {} };
    cache.set(name, module);
    const source = fs.readFileSync(path.join(dir, name + '.ts'), 'utf8');
    const javascript = ts.transpileModule(source, { compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2020 } }).outputText;
    const fn = vm.runInContext(`(function(require,module,exports){${javascript}\n})`, context);
    fn(load, module, module.exports);
    return module.exports;
  }
  const emit = (channel, payload) => context.__tsgpui_host_event(channel, payload);
  return { load, calls, emit, context, listeners };
}

test('Tray forwards creation, icon, tooltip, title and menu mutations to the host', async () => {
  const { load, calls } = runtime();
  const e = load('electron');
  const tray = new e.Tray('tray.png');
  await Promise.resolve();
  const created = calls.find(([op]) => op === 'tray.create');
  assert.ok(created, 'tray.create was not called');
  assert.equal(created[1].id, 1);
  assert.equal(JSON.stringify(created[1].image), JSON.stringify({ path: 'tray.png' }));

  tray.setToolTip('Ready');
  tray.setTitle('99%');
  tray.setImage({ toPNG: () => new Uint8Array([137, 80, 78, 71]) });
  assert.equal(tray.getToolTip(), 'Ready');
  assert.equal(tray.getTitle(), '99%');
  const toolTip = calls.find(([op]) => op === 'tray.setToolTip');
  assert.equal(toolTip[1].toolTip, 'Ready');
  const title = calls.find(([op]) => op === 'tray.setTitle');
  assert.equal(title[1].title, '99%');
  const icon = calls.find(([op]) => op === 'tray.setIcon');
  assert.ok(icon[1].image.base64.startsWith('iVBOR'), `unexpected image payload: ${JSON.stringify(icon[1].image)}`);

  let checkedClicks = 0;
  const menu = e.Menu.buildFromTemplate([
    { id: 'show', label: 'Show', click: () => {} },
    { type: 'separator' },
    { id: 'toggle', label: 'Enabled', type: 'checkbox', checked: false, click: () => { checkedClicks++; } },
  ]);
  tray.setContextMenu(menu);
  const setMenu = calls.find(([op]) => op === 'tray.setContextMenu');
  assert.equal(setMenu[1].menu.length, 3);
  assert.equal(JSON.stringify(setMenu[1].menu.map((item) => item.itemId)), JSON.stringify([1, 2, 3]));
  assert.equal(setMenu[1].menu[2].type, 'checkbox');
  assert.equal(setMenu[1].menu[2].checked, false);
});

test('Tray host events dispatch click, right-click, double-click and menu clicks', () => {
  const { load, calls, emit } = runtime();
  const e = load('electron');
  const tray = new e.Tray('tray.png');
  // Load the module once so its host-event subscription is installed (it is installed by
  // electron-system when Tray was imported, but make that explicit for the test reader).
  const events = [];
  tray.on('click', (...args) => events.push(['click', args]));
  tray.on('right-click', () => events.push(['right-click']));
  tray.on('double-click', () => events.push(['double-click']));

  emit('tray.event', { id: tray.id, event: 'click' });
  emit('tray.event', { id: tray.id, event: 'right-click' });
  emit('tray.event', { id: tray.id, event: 'double-click' });
  assert.deepEqual(events.map(([name]) => name), ['click', 'right-click', 'double-click']);
  assert.equal(events[0][1].length, 3);
  assert.equal(JSON.stringify(events[0][1][1]), JSON.stringify({ x: 0, y: 0, width: 0, height: 0 }));

  tray.setIgnoreDoubleClickEvents(true);
  emit('tray.event', { id: tray.id, event: 'double-click' });
  assert.deepEqual(events.map(([name]) => name), ['click', 'right-click', 'double-click']);

  let clicked = 0;
  let toggled = 0;
  const menu = e.Menu.buildFromTemplate([
    { id: 'show', label: 'Show', click: () => { clicked++; } },
    { id: 'check', label: 'Check', type: 'checkbox', checked: false, click: (item) => { assert.equal(item.checked, true); toggled++; } },
  ]);
  tray.setContextMenu(menu);
  const snapshot = calls.filter(([op]) => op === 'tray.setContextMenu').at(-1);
  emit('tray.event', { id: tray.id, event: 'menu-click', itemId: snapshot[1].menu[0].itemId });
  assert.equal(clicked, 1);
  assert.equal(toggled, 0);
  emit('tray.event', { id: tray.id, event: 'menu-click', itemId: snapshot[1].menu[1].itemId });
  assert.equal(toggled, 1);
  assert.equal(menu.getMenuItemById('check').checked, true);

  tray.destroy();
  assert.equal(tray.isDestroyed(), true);
  assert.ok(calls.some(([op]) => op === 'tray.destroy'));
});

test('globalShortcut forwards registrations and dispatches host presses', () => {
  const { load, calls, emit } = runtime();
  const e = load('electron');
  let presses = 0;
  assert.equal(e.globalShortcut.register('CommandOrControl+Shift+P', () => { presses++; }), true);
  assert.equal(e.globalShortcut.isRegistered('CommandOrControl+Shift+P'), true);
  assert.equal(e.globalShortcut.register('CommandOrControl+Shift+P', () => {}), false);
  const registered = calls.find(([op]) => op === 'globalShortcut.register');
  assert.equal(registered[1].accelerator, 'CommandOrControl+Shift+P');

  emit('globalShortcut.event', { accelerator: 'CommandOrControl+Shift+P' });
  assert.equal(presses, 1);
  emit('globalShortcut.event', { accelerator: 'Unknown+Key' });
  assert.equal(presses, 1);

  const h = load('host');
  const warningsBefore = h.compatWarnings.length;
  e.globalShortcut.register('Alt+F9', () => {});
  emit('globalShortcut.failed', { accelerator: 'Alt+F9', error: 'already taken' });
  assert.equal(e.globalShortcut.isRegistered('Alt+F9'), false);
  assert.equal(h.compatWarnings.length, warningsBefore + 1);

  e.globalShortcut.unregister('CommandOrControl+Shift+P');
  assert.equal(e.globalShortcut.isRegistered('CommandOrControl+Shift+P'), false);
  e.globalShortcut.unregisterAll();
  assert.ok(calls.some(([op]) => op === 'globalShortcut.unregisterAll'));
});
