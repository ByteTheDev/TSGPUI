import {test} from 'node:test';
import assert from 'node:assert/strict';
import {spawnSync} from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {fileURLToPath} from 'node:url';
import {currentTarget, exeSuffix, sourceStamp} from '../src/hosts.mjs';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const bin = path.join(root, 'bin', 'tsgpui.mjs');
const source = path.resolve(root, '../../native');
const run = (args, env) => spawnSync(process.execPath, [bin, ...args], {encoding: 'utf8', env: {...process.env, ...env, TSGPUI_HOST: '', TSGPUI_WEBVIEW_HOST: ''}});

test('source stamp is deterministic and includes nested Rust files and renderer', () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'tsgpui-stamp-'));
  try {
    fs.mkdirSync(path.join(dir, 'src', 'nested'), {recursive: true});
    fs.writeFileSync(path.join(dir, 'Cargo.toml'), 'manifest');
    fs.writeFileSync(path.join(dir, 'Cargo.lock'), 'lock');
    const file = path.join(dir, 'src', 'nested', 'main.rs');
    fs.writeFileSync(file, 'one');
    const stamp = sourceStamp(dir, 'native');
    assert.equal(sourceStamp(dir, 'native'), stamp);
    assert.notEqual(sourceStamp(dir, 'webview'), stamp);
    fs.writeFileSync(file, 'two');
    assert.notEqual(sourceStamp(dir, 'native'), stamp);
  } finally {fs.rmSync(dir, {recursive: true, force: true});}
});

test('build rejects a stale cached host when Cargo is unavailable', () => {
  const home = fs.mkdtempSync(path.join(os.tmpdir(), 'tsgpui-version-'));
  try {
    const project = path.join(home, 'app');
    const env = {TSGPUI_HOME: home, PATH: home};
    const init = run(['init', project], env);
    assert.equal(init.status, 0, init.stderr);
    const file = path.join(home, 'host', currentTarget(), `tsgpui-host${exeSuffix()}`);
    fs.mkdirSync(path.dirname(file), {recursive: true});
    fs.writeFileSync(file, 'stale');
    fs.writeFileSync(`${file}.json`, JSON.stringify({sourceStamp: 'stale'}));
    const result = run(['build', '--project', project], env);
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /tsgpui host build --renderer native/);
    assert.match(result.stderr, /does not match the current source/);
  } finally {fs.rmSync(home, {recursive: true, force: true});}
});

test('doctor reports both host paths and source stamps', () => {
  const home = fs.mkdtempSync(path.join(os.tmpdir(), 'tsgpui-doctor-'));
  try {
    const file = path.join(home, 'host', currentTarget(), `tsgpui-host${exeSuffix()}`);
    fs.mkdirSync(path.dirname(file), {recursive: true});
    fs.writeFileSync(file, 'host');
    fs.writeFileSync(`${file}.json`, JSON.stringify({sourceStamp: sourceStamp(source, 'native')}));
    const result = run(['doctor'], {TSGPUI_HOME: home});
    assert.match(result.stdout, /native renderer:[\s\S]*installed host path:/);
    assert.match(result.stdout, /source stamps match: yes/);
    assert.match(result.stdout, /webview renderer:[\s\S]*installed source stamp: missing[\s\S]*current source stamp: [a-f0-9]{64}[\s\S]*source stamps match: no/);
  } finally {fs.rmSync(home, {recursive: true, force: true});}
});
