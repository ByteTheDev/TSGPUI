import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import vm from 'node:vm';
import ts from 'typescript';
import { fileURLToPath } from 'node:url';

// The compatibility runtime as the native (GPUI) backend runs it: `api.native` is the only host bridge,
// and there is no `__tsgpui_host`. These tests check that the shims reach `api.native` with the
// arguments it expects and read its results in the shapes the shims use.
const dir = fileURLToPath(new URL('../src/electron-compat/runtime/', import.meta.url));

const info = {
  name: 'Example', version: '1.2', platform: 'linux',
  paths: { home: '/home', temp: '/tmp', appFolder: '/app', userData: '/data' },
  roots: [{ name: 'userData', path: '/data', access: 'write' }],
};

// A fake `api.native`. Each method records its name and arguments, then answers from `answers`.
function fakeNative(files = new Map()) {
  const calls = [];
  const answer = (name, value) => (...args) => {
    calls.push([name, args]);
    return Promise.resolve(typeof value === 'function' ? value(...args) : value);
  };
  const api = {
    app: { info: answer('app.info', info), quit: answer('app.quit', null) },
    clipboard: { readText: answer('clipboard.readText', 'clip'), writeText: answer('clipboard.writeText', null) },
    safeStorage: {
      isAvailable: answer('safeStorage.isAvailable', true),
      encrypt: answer('safeStorage.encrypt', text => Buffer.from(`enc:${text}`).toString('base64')),
      decrypt: answer('safeStorage.decrypt', encoded => Buffer.from(encoded, 'base64').toString('utf8').replace(/^enc:/, '')),
    },
    dialog: {
      openFile: answer('dialog.openFile', { canceled: false, paths: ['/data/picked'] }),
      saveFile: answer('dialog.saveFile', { canceled: false, path: '/data/saved' }),
      message: answer('dialog.message', { response: 1 }),
    },
    filesystem: {
      readText: answer('filesystem.readText', loc => files.get(`${loc.root}/${loc.path}`)),
      readBytes: answer('filesystem.readBytes', loc => files.get(`${loc.root}/${loc.path}`)),
      writeText: answer('filesystem.writeText', (loc, text) => files.set(`${loc.root}/${loc.path}`, text) && null),
      writeBytes: answer('filesystem.writeBytes', (loc, bytes) => files.set(`${loc.root}/${loc.path}`, bytes) && null),
      list: answer('filesystem.list', [{ name: 'a.txt', kind: 'file', size: 3 }, { name: 'sub', kind: 'directory', size: 0 }]),
      stat: answer('filesystem.stat', { kind: 'file', size: 3, modified: 1700000000000 }),
      mkdir: answer('filesystem.mkdir', null),
      remove: answer('filesystem.remove', null),
      rename: answer('filesystem.rename', null),
    },
    process: { spawn: answer('process.spawn', { code: 0, stdout: 'ok', stderr: '' }) },
    screen: { displays: answer('screen.displays', [{ x: 0, y: 0, width: 1280, height: 800, scaleFactor: 1, primary: true }]) },
    shell: { openExternal: answer('shell.openExternal', null) },
    notifications: { show: answer('notifications.show', null) },
    ipc: { send: answer('ipc.send', null), on: () => () => {} },
    window: {
      minimize: answer('window.minimize', null), maximize: answer('window.maximize', null),
      restore: answer('window.restore', null), close: answer('window.close', null),
      setTitle: answer('window.setTitle', null),
    },
    net: { request: answer('net.request', { status: 200, headers: {}, body: 'pong' }) },
    power: { setBlockers: answer('power.setBlockers', null) },
  };
  return { api, calls };
}

// Loads the runtime modules the way the backend bundle does, with `api` as the only host bridge.
function nativeRuntime(api) {
  const context = vm.createContext({ console, Uint8Array, Promise, Map, setTimeout, __tsgpui_native: api });
  const cache = new Map();
  function load(name) {
    name = name.replace(/^\.\//, '').replace(/\.ts$/, '');
    if (cache.has(name)) return cache.get(name).exports;
    const mod = { exports: {} };
    cache.set(name, mod);
    const source = fs.readFileSync(path.join(dir, name + '.ts'), 'utf8');
    const js = ts.transpileModule(source, { compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2020 } }).outputText;
    const fn = vm.runInContext(`(function(require,module,exports){${js}\n})`, context);
    fn(load, mod, mod.exports);
    return mod.exports;
  }
  return { load, context };
}

test('native file operations address named roots and carry bytes as base64 on the shim side', async () => {
  const files = new Map();
  const { api, calls } = fakeNative(files);
  const { load } = nativeRuntime(api);
  const f = load('fs');
  await f.promises.writeFile('/data/a', 'hello');
  assert.equal(await f.promises.readFile('/data/a', 'utf8'), 'hello');
  await f.promises.writeFile('/data/b', new Uint8Array([0, 255, 42]));
  assert.deepEqual(Array.from(await f.promises.readFile('/data/b')), [0, 255, 42]);
  // The shims run in a VM realm, so compare serialized values rather than prototypes.
  const sent = name => JSON.stringify(calls.find(([called]) => called === name)[1]);
  assert.equal(sent('filesystem.writeText'), JSON.stringify([{ root: 'userData', path: 'a' }, 'hello']));
  assert.equal(sent('filesystem.writeBytes'), JSON.stringify([{ root: 'userData', path: 'b' }, [0, 255, 42]]));
  await assert.rejects(f.promises.readFile('/elsewhere/a'), { code: 'EACCES' });
});

test('readdir and stat return what Electron returns: names, Dirent-like entries and mtime', async () => {
  const { api } = fakeNative();
  const { load } = nativeRuntime(api);
  const f = load('fs');
  assert.deepEqual(JSON.parse(JSON.stringify(await f.promises.readdir('/data'))), ['a.txt', 'sub']);
  const entries = await f.promises.readdir('/data', { withFileTypes: true });
  assert.deepEqual(entries.map(entry => [entry.name, entry.isDirectory()]), [['a.txt', false], ['sub', true]]);
  const stats = await f.promises.stat('/data/a.txt');
  assert.equal(stats.mtimeMs, 1700000000000);
  assert.equal(stats.mtime.getTime(), 1700000000000);
  assert.equal(stats.isFile(), true);
});

test('native app, dialog, clipboard and screen calls return the shapes the shims read', async () => {
  const { api } = fakeNative();
  const { load } = nativeRuntime(api);
  const e = load('electron');
  await e.app.whenReady();
  assert.equal(e.app.getName(), 'Example');
  assert.equal(e.app.getPath('userData'), '/data');
  assert.equal((await e.dialog.showOpenDialog({ properties: ['multiSelections'] })).filePaths[0], '/data/picked');
  assert.equal((await e.dialog.showSaveDialog({})).filePath, '/data/saved');
  assert.equal((await e.dialog.showMessageBox({ message: 'hello' })).response, 1);
  assert.equal(await e.clipboard.readText(), 'clip');
  await e.screen.refresh();
  assert.equal(e.screen.getPrimaryDisplay().bounds.width, 1280);
});

test('safeStorage encrypts and decrypts through the native API as promises and bytes', async () => {
  const { api, calls } = fakeNative();
  const { load } = nativeRuntime(api);
  const { safeStorage } = load('electron');
  assert.equal(await safeStorage.isEncryptionAvailable(), true);
  const encrypted = await safeStorage.encryptString('secret');
  assert.ok(encrypted instanceof Uint8Array, 'ciphertext is returned as bytes');
  assert.equal(await safeStorage.decryptString(encrypted), 'secret');
  const sent = calls.filter(([name]) => name === 'safeStorage.decrypt')[0][1][0];
  assert.equal(sent, Buffer.from(encrypted).toString('base64'), 'decrypt receives the same bytes as base64');
});

test('native process results expose the exit status the shims emit', async () => {
  const { api } = fakeNative();
  const { load } = nativeRuntime(api);
  const cp = load('child_process');
  const output = await new Promise((resolve, reject) => cp.execFile('allowed', ['arg'], (error, out) => (error ? reject(error) : resolve(out))));
  assert.equal(output, 'ok');
});

test('native net.request sends the request options and returns the response body', async () => {
  const { api, calls } = fakeNative();
  const { load } = nativeRuntime(api);
  const e = load('electron');
  const body = await new Promise((resolve, reject) => {
    const req = e.net.request({ url: 'https://example.com/ping', method: 'GET' });
    req.on('response', response => {
      let text = '';
      response.on('data', chunk => (text += chunk));
      response.on('end', () => resolve(text));
    });
    req.on('error', reject);
    req.end();
  });
  assert.equal(body, 'pong');
  assert.equal(calls.find(([name]) => name === 'net.request')[1][0].url, 'https://example.com/ping');
});

test('operations that need a page or an extra window name the renderer that provides them', async () => {
  const { api } = fakeNative();
  const { load } = nativeRuntime(api);
  const host = load('host');
  await assert.rejects(host.host('window.create', {}), /renderer: "webview"/);
  await assert.rejects(host.host('ipc.invoke', { channel: 'x' }), /not available in the native renderer/);
});

test('page messages and close listeners warn in the native renderer instead of failing', async () => {
  const { api } = fakeNative();
  const { load } = nativeRuntime(api);
  const e = load('electron');
  const host = load('host');
  const window = new e.BrowserWindow();
  window.webContents.send('notice', 1);
  window.on('close', () => {});
  await new Promise(resolve => setTimeout(resolve, 0));
  assert.ok(host.compatWarnings.some(message => /no page in the native renderer/.test(message)));
  assert.ok(host.compatWarnings.some(message => /cannot cancel a close/.test(message)));
});

test('the WebView bridge keeps its object protocol when it is present', async () => {
  const seen = [];
  const context = vm.createContext({ console, Promise, __tsgpui_host: async (op, args) => { seen.push([op, args]); return { ok: true }; }, __tsgpui_native: { app: {} } });
  const mod = { exports: {} };
  const source = fs.readFileSync(path.join(dir, 'host.ts'), 'utf8');
  const js = ts.transpileModule(source, { compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2020 } }).outputText;
  vm.runInContext(`(function(require,module,exports){${js}\n})`, context)(() => ({}), mod, mod.exports);
  assert.deepEqual(await mod.exports.host('app.info', {}), { ok: true });
  assert.deepEqual(seen, [['app.info', {}]]);
});
