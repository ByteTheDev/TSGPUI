import test from 'node:test';
import assert from 'node:assert/strict';
import { validateConfig, buildPackage } from '../src/compiler.mjs';
import { mkdtemp, mkdir, writeFile, readFile, rm } from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';

const base = { name: 'Example', html: 'src/index.html', css: 'src/style.css', backend: 'src/backend.ts', renderer: 'webview' };

test('backendWindow accepts primary and hidden, and only with the WebView renderer', () => {
  assert.equal(validateConfig(base).backendWindow, 'primary');
  assert.equal(validateConfig({ ...base, backendWindow: 'hidden' }).backendWindow, 'hidden');
  assert.throws(() => validateConfig({ ...base, backendWindow: 'tray' }), /backendWindow must be/);
  assert.throws(() => validateConfig({ ...base, renderer: 'native', backendWindow: 'hidden' }), /requires the webview renderer/);
});

test('a hidden backend gets its own document and renderer pages carry no backend', async () => {
  const root = await mkdtemp(path.join(os.tmpdir(), 'tsgpui-hidden-'));
  try {
    await mkdir(path.join(root, 'src'));
    await writeFile(path.join(root, 'tsgpui.config.json'), JSON.stringify({ ...base, backendWindow: 'hidden' }));
    await writeFile(path.join(root, 'src/index.html'), '<html><head></head><body><p>Main</p></body></html>');
    await writeFile(path.join(root, 'src/style.css'), '');
    await writeFile(path.join(root, 'src/backend.ts'), 'export default { state: {}, actions: {} };');
    await buildPackage(root);
    const doc = JSON.parse(await readFile(path.join(root, 'dist/Example.tgp'), 'utf8'));
    assert.equal(doc.backend_window, 'hidden');
    assert.match(doc.webview_main_html, /data-tsgpui="backend"/);
    assert.doesNotMatch(doc.webview_html, /data-tsgpui="backend"/);
    assert.match(doc.webview_html, /<p>Main<\/p>/);
  } finally { await rm(root, { recursive: true, force: true }); }
});

test('the default primary backend keeps the backend in the renderer page', async () => {
  const root = await mkdtemp(path.join(os.tmpdir(), 'tsgpui-primary-'));
  try {
    await mkdir(path.join(root, 'src'));
    await writeFile(path.join(root, 'tsgpui.config.json'), JSON.stringify(base));
    await writeFile(path.join(root, 'src/index.html'), '<html><head></head><body></body></html>');
    await writeFile(path.join(root, 'src/style.css'), '');
    await writeFile(path.join(root, 'src/backend.ts'), 'export default { state: {}, actions: {} };');
    await buildPackage(root);
    const doc = JSON.parse(await readFile(path.join(root, 'dist/Example.tgp'), 'utf8'));
    assert.equal(doc.backend_window, undefined);
    assert.match(doc.webview_html, /data-tsgpui="backend"/);
  } finally { await rm(root, { recursive: true, force: true }); }
});
