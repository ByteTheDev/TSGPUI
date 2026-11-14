import test from 'node:test';
import assert from 'node:assert/strict';
import { validateConfig } from '../src/compiler.mjs';
import { MAGIC } from '../src/pack.mjs';

test('bundle magic is RENDRI01 (Rust package.rs and the JS packer must agree byte-for-byte)', () => {
  assert.equal(Buffer.from(MAGIC).toString('ascii'), 'RENDRI01');
  assert.equal(MAGIC.length, 8);
});

test('bundle size limit defaults to 256 MiB and accepts positive byte limits', () => {
  const config = { name: 'Example', html: 'index.html', css: 'style.css', backend: 'backend.ts', renderer: 'rendrix' };
  assert.equal(validateConfig(config).maxBundleSize, 256 * 1024 * 1024);
  assert.equal(validateConfig({ ...config, maxBundleSize: 1024 }).maxBundleSize, 1024);
  for (const value of [0, -1, 1.5, '1024']) {
    assert.throws(() => validateConfig({ ...config, maxBundleSize: value }), /maxBundleSize/);
  }
});

test('the "webview" renderer is a hard error with migration guidance', () => {
  const config = { name: 'Example', html: 'index.html', css: 'style.css', backend: 'backend.ts', renderer: 'webview' };
  assert.throws(() => validateConfig(config), (error) => {
    const message = String(error);
    return /webview/i.test(message) && /rendrix/.test(message);
  });
});
