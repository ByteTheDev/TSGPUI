import {test} from 'node:test';
import assert from 'node:assert/strict';
import {spawnSync} from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {fileURLToPath} from 'node:url';
import {currentTarget, exeSuffix, sourceStamp} from '../src/hosts.mjs';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const bin = path.join(root, 'bin', 'rendrix.mjs');
const source = path.resolve(root, '../../native');
const run = (args, env) => spawnSync(process.execPath, [bin, ...args], {encoding: 'utf8', env: {...process.env, ...env, RENDRIX_HOST: ''}});

test('source stamp is deterministic and includes nested Rust files and renderer', () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'rendrix-stamp-'));
  try {
    fs.mkdirSync(path.join(dir, 'src', 'nested'), {recursive: true});
    fs.writeFileSync(path.join(dir, 'Cargo.toml'), 'manifest');
    fs.writeFileSync(path.join(dir, 'Cargo.lock'), 'lock');
    const file = path.join(dir, 'src', 'nested', 'main.rs');
    fs.writeFileSync(file, 'one');
    const stamp = sourceStamp(dir, 'rendrix');
    assert.equal(sourceStamp(dir, 'rendrix'), stamp);
    assert.notEqual(sourceStamp(dir, 'native'), stamp);
    fs.writeFileSync(file, 'two');
    assert.notEqual(sourceStamp(dir, 'rendrix'), stamp);
  } finally {fs.rmSync(dir, {recursive: true, force: true});}
});

test('build rejects a stale cached rendrix host when Cargo is unavailable', () => {
  const home = fs.mkdtempSync(path.join(os.tmpdir(), 'rendrix-version-'));
  try {
    const project = path.join(home, 'app');
    const env = {RENDRIX_HOME: home, PATH: home};
    const init = run(['init', project], env);
    assert.equal(init.status, 0, init.stderr);
    const configFile = path.join(project, fs.readdirSync(project).find(name => name.endsWith('.config.json')));
    const config = JSON.parse(fs.readFileSync(configFile, 'utf8'));
    config.renderer = 'rendrix';
    fs.writeFileSync(configFile, JSON.stringify(config));
    const file = path.join(home, 'host', currentTarget(), `rendrix-host${exeSuffix()}`);
    fs.mkdirSync(path.dirname(file), {recursive: true});
    fs.writeFileSync(file, 'stale');
    fs.writeFileSync(`${file}.json`, JSON.stringify({sourceStamp: 'stale'}));
    const result = run(['build', '--project', project], env);
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /rendrix host build --renderer rendrix/);
    assert.match(result.stderr, /does not match the current source/);
  } finally {fs.rmSync(home, {recursive: true, force: true});}
});

test('doctor reports the rendrix host path and source stamps, and no webview renderer', () => {
  const home = fs.mkdtempSync(path.join(os.tmpdir(), 'rendrix-doctor-'));
  try {
    const file = path.join(home, 'host', currentTarget(), `rendrix-host${exeSuffix()}`);
    fs.mkdirSync(path.dirname(file), {recursive: true});
    fs.writeFileSync(file, 'host');
    fs.writeFileSync(`${file}.json`, JSON.stringify({sourceStamp: sourceStamp(source, 'rendrix')}));
    const result = run(['doctor'], {RENDRIX_HOME: home});
    assert.match(result.stdout, /rendrix renderer:[\s\S]*installed host path:/);
    assert.match(result.stdout, /source stamps match: yes/);
    assert.doesNotMatch(result.stdout, /webview/);
  } finally {fs.rmSync(home, {recursive: true, force: true});}
});
