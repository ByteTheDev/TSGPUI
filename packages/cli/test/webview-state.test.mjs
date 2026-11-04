import test from 'node:test';
import assert from 'node:assert/strict';
import vm from 'node:vm';
import { compileBackend } from '../src/backend.mjs';
import { prepareWebviewHtml } from '../src/webview.mjs';

// Runs a WebView page's backend bootstrap in a bare VM, with just enough DOM for the runtime to start.
// `storage` stands in for sessionStorage, which is where a page keeps its state between reloads.
function runPage(html, storage) {
  const bootstrap = /<script data-tsgpui="backend"[^>]*>([\s\S]*?)<\/script>/.exec(html)[1];
  const unloadListeners = [];
  const sandbox = {
    document: {
      title: '', body: {}, documentElement: { getAttribute: () => null },
      createTreeWalker: () => ({ nextNode: () => null }),
      querySelectorAll: () => [],
      addEventListener() {},
    },
    NodeFilter: { SHOW_TEXT: 4 },
    window: {
      sessionStorage: storage,
      localStorage: storage,
      addEventListener(type, listener) { if (type === 'beforeunload') unloadListeners.push(listener); },
      confirm: () => true,
    },
    console,
  };
  for (const name of ['HTMLInputElement', 'HTMLTextAreaElement', 'HTMLSelectElement', 'HTMLElement', 'HTMLFormElement', 'Element']) {
    sandbox[name] = class {};
  }
  const context = vm.createContext(sandbox);
  vm.runInContext(bootstrap, context);
  return {
    app: context.__tsgpui_app,
    // The window is closing or reloading: the page's unload handler saves its state.
    unload: () => unloadListeners.forEach(listener => listener()),
  };
}

// Objects made inside the VM belong to another realm, so compare plain JSON copies.
const snapshot = app => JSON.parse(JSON.stringify(app.state));

function memoryStorage() {
  const items = new Map();
  return {
    getItem: key => (items.has(key) ? items.get(key) : null),
    setItem: (key, value) => items.set(key, String(value)),
  };
}

async function pageFor(backend) {
  return prepareWebviewHtml('<body><main></main></body>', '', {
    backendJs: compileBackend(backend),
    title: 'state test',
  });
}

test('webview state keeps a user change and takes a new default for a value the user never changed', async () => {
  const storage = memoryStorage();
  const first = runPage(await pageFor('export default { state: { theme: "light", count: 0 }, actions: {} };'), storage);
  first.app.state.count = 5;
  first.unload();

  // The developer changes both defaults and reloads.
  const second = runPage(await pageFor('export default { state: { theme: "dark", count: 10 }, actions: {} };'), storage);
  assert.deepEqual(snapshot(second.app), { theme: 'dark', count: 5 });
});

test('webview state keeps a changed value even when the developer changes its default', async () => {
  const storage = memoryStorage();
  const first = runPage(await pageFor('export default { state: { theme: "light" }, actions: {} };'), storage);
  first.app.state.theme = 'blue';
  first.unload();

  const second = runPage(await pageFor('export default { state: { theme: "dark" }, actions: {} };'), storage);
  assert.deepEqual(snapshot(second.app), { theme: 'blue' });
});

test('webview state saved by an older build is still restored', async () => {
  const storage = memoryStorage();
  storage.setItem('tsgpui:state', JSON.stringify({ theme: 'blue', count: 3 }));
  const page = runPage(await pageFor('export default { state: { theme: "dark", count: 0 }, actions: {} };'), storage);
  assert.deepEqual(snapshot(page.app), { theme: 'blue', count: 3 });
});
