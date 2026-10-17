import {test} from 'node:test';
import assert from 'node:assert/strict';
import {spawnSync} from 'node:child_process';
import fs from 'node:fs';
import {mkdtempSync} from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {fileURLToPath} from 'node:url';

const cliRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const bin = path.join(cliRoot, 'bin/tsgpui.mjs');
const version = JSON.parse(fs.readFileSync(path.join(cliRoot, 'package.json'), 'utf8')).version;

function tsgpui(args, options = {}) {
  return spawnSync(process.execPath, [bin, ...args], {encoding: 'utf8', ...options});
}

test('--version prints the package version', () => {
  const result = tsgpui(['--version']);
  assert.equal(result.status, 0, result.stderr);
  assert.equal(result.stdout.trim(), version);
});

test('help lists the commands', () => {
  const result = tsgpui(['help']);
  assert.equal(result.status, 0, result.stderr);
  for (const command of ['init', 'compile', 'dev', 'build', 'host build', 'doctor']) {
    assert.match(result.stdout, new RegExp(command));
  }
});

test('init creates a project from the packaged template and compile produces a bundle', () => {
  const dir = path.join(mkdtempSync(path.join(os.tmpdir(), 'tsgpui-init-')), 'my-app');
  const created = tsgpui(['init', dir]);
  assert.equal(created.status, 0, created.stderr);
  for (const file of ['tsgpui.config.json', 'src/index.html', 'src/styles.css', 'src/backend.ts', 'package.json']) {
    assert.ok(fs.existsSync(path.join(dir, file)), `missing ${file}`);
  }
  assert.equal(JSON.parse(fs.readFileSync(path.join(dir, 'package.json'), 'utf8')).name, 'my-app');

  const compiled = tsgpui(['compile', '--project', dir]);
  assert.equal(compiled.status, 0, compiled.stderr);
  const bundle = JSON.parse(fs.readFileSync(path.join(dir, 'dist/my-app.tgp'), 'utf8'));
  assert.equal(bundle.format, 'tsgpui-1');
  assert.equal(bundle.name, 'my-app');
});

test('init refuses a non-empty destination', () => {
  const dir = mkdtempSync(path.join(os.tmpdir(), 'tsgpui-nonempty-'));
  fs.writeFileSync(path.join(dir, 'keep.txt'), 'x');
  const result = tsgpui(['init', dir]);
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /not empty/);
});

test('doctor reports the native sources and the per-user host location', () => {
  const home = mkdtempSync(path.join(os.tmpdir(), 'tsgpui-home-'));
  const result = tsgpui(['doctor'], {env: {...process.env, TSGPUI_HOME: home}});
  assert.equal(result.status, 0, result.stderr);
  assert.match(result.stdout, /Native sources: /);
  assert.ok(result.stdout.includes(path.join(home, 'host')), 'host candidates should use TSGPUI_HOME');
});
