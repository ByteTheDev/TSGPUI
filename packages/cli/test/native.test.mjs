import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { createNativeGrants, performNative } from '../src/native.mjs';

function sandbox(name) {
  const base = fs.mkdtempSync(path.join(os.tmpdir(), `tsgpui-native-${name}-`));
  fs.mkdirSync(path.join(base, 'data'));
  return base;
}

function grants(base, { read = ['data'], write = ['data'], processes = [] } = {}) {
  return createNativeGrants({ filesystem: { read, write }, processes }, base);
}

test('text round trips inside a granted root', async () => {
  const base = sandbox('roundtrip');
  const g = grants(base);
  await performNative(g, 'filesystem.writeText', ['note.txt', 'hello']);
  assert.equal(await performNative(g, 'filesystem.readText', ['note.txt']), 'hello');
  assert.equal(fs.readFileSync(path.join(base, 'data', 'note.txt'), 'utf8'), 'hello');
});

test('traversal and absolute paths are refused', async () => {
  const base = sandbox('traversal');
  const g = grants(base);
  await assert.rejects(performNative(g, 'filesystem.writeText', ['../escape.txt', 'x']), /must not contain '\.\.'/);
  await assert.rejects(performNative(g, 'filesystem.readText', ['../../etc']), /must not contain '\.\.'/);
  await assert.rejects(performNative(g, 'filesystem.writeText', [path.join(base, 'abs.txt'), 'x']), /must be relative/);
  assert.equal(fs.existsSync(path.join(base, 'escape.txt')), false);
});

test('paths outside the granted roots are refused', async () => {
  const base = sandbox('roots');
  fs.mkdirSync(path.join(base, 'private'));
  fs.writeFileSync(path.join(base, 'private', 'secret.txt'), 's');
  const g = grants(base);
  await assert.rejects(performNative(g, 'filesystem.readText', ['../private/secret.txt']), /must not contain/);
  await assert.rejects(performNative(createNativeGrants({}, base), 'filesystem.readText', ['x.txt']), /not inside a directory granted/);
});

test('a read-only root cannot be written', async () => {
  const base = sandbox('readonly');
  fs.writeFileSync(path.join(base, 'data', 'a.txt'), 'a');
  const g = grants(base, { write: [] });
  assert.equal(await performNative(g, 'filesystem.readText', ['a.txt']), 'a');
  await assert.rejects(performNative(g, 'filesystem.writeText', ['b.txt', 'b']), /not inside a directory granted/);
  await assert.rejects(performNative(g, 'filesystem.remove', ['a.txt']), /not inside a directory granted/);
});

test('symlinks cannot lead out of a granted root', async t => {
  const base = sandbox('symlink');
  const outside = path.join(base, 'outside.txt');
  fs.writeFileSync(outside, 'secret');
  try {
    fs.symlinkSync(outside, path.join(base, 'data', 'link.txt'), 'file');
  } catch (_error) {
    t.skip('symlinks need extra privileges on this machine');
    return;
  }
  const g = grants(base);
  await assert.rejects(performNative(g, 'filesystem.readText', ['link.txt']), /not inside a directory granted/);
  await assert.rejects(performNative(g, 'filesystem.writeText', ['link.txt', 'x']), /not inside a directory granted/);
  assert.equal(fs.readFileSync(outside, 'utf8'), 'secret');
});

test('listing, stat, folders, renames and removal', async () => {
  const base = sandbox('manage');
  const g = grants(base);
  await performNative(g, 'filesystem.mkdir', ['docs']);
  await performNative(g, 'filesystem.writeText', ['b.txt', 'bb']);
  await performNative(g, 'filesystem.writeText', ['a.txt', 'a']);
  const listing = await performNative(g, 'filesystem.list', ['']);
  assert.deepEqual(listing.map(entry => entry.name), ['a.txt', 'b.txt', 'docs']);
  assert.deepEqual(listing[2].kind, 'directory');
  assert.equal((await performNative(g, 'filesystem.stat', ['b.txt'])).size, 2);
  await performNative(g, 'filesystem.rename', ['a.txt', 'c.txt']);
  await performNative(g, 'filesystem.remove', ['c.txt']);
  await assert.rejects(performNative(g, 'filesystem.stat', ['c.txt']));
  await performNative(g, 'filesystem.remove', ['docs']);
  await assert.rejects(performNative(g, 'filesystem.remove', ['']), /must name a file or folder/);
});

test('bytes round trip as number arrays and reject out-of-range values', async () => {
  const base = sandbox('bytes');
  const g = grants(base);
  await performNative(g, 'filesystem.writeBytes', ['x.bin', [0, 255, 7]]);
  assert.deepEqual(await performNative(g, 'filesystem.readBytes', ['x.bin']), [0, 255, 7]);
  await assert.rejects(performNative(g, 'filesystem.writeBytes', ['y.bin', [256]]), /0 to 255/);
});

test('the one mebibyte limit applies to writes', async () => {
  const base = sandbox('limit');
  const g = grants(base);
  await assert.rejects(performNative(g, 'filesystem.writeText', ['big.txt', 'x'.repeat(1024 * 1024 + 1)]), /1 MiB/);
});

test('processes must be granted by name and run without a shell', async () => {
  const base = sandbox('process');
  await assert.rejects(performNative(grants(base), 'process.spawn', ['node', ['-v']]), /not listed in permissions.processes/);
  const g = grants(base, { processes: ['node'] });
  const result = await performNative(g, 'process.spawn', ['node', ['-e', 'console.log(7)']]);
  assert.equal(result.code, 0);
  assert.equal(result.stdout.trim(), '7');
  const failed = await performNative(g, 'process.spawn', ['node', ['-e', 'process.exit(3)']]);
  assert.equal(failed.code, 3);
});

test('unknown operations are reported', async () => {
  await assert.rejects(performNative(grants(sandbox('unknown')), 'window.minimize', []), /not available in the browser preview/);
});
