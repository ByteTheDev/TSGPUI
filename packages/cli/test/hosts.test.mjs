import {test} from 'node:test';
import assert from 'node:assert/strict';
import {spawn} from 'node:child_process';
import {createHash} from 'node:crypto';
import fs from 'node:fs';
import http from 'node:http';
import os from 'node:os';
import path from 'node:path';
import {fileURLToPath} from 'node:url';
import {assetName, currentTarget, exeSuffix} from '../src/hosts.mjs';

const cliRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const bin = path.join(cliRoot, 'bin/tsgpui.mjs');
const version = JSON.parse(fs.readFileSync(path.join(cliRoot, 'package.json'), 'utf8')).version;
const target = currentTarget();

function run(args, env) {
  return new Promise(resolve => {
    const child = spawn(process.execPath, [bin, ...args], {env: {...process.env, ...env}});
    let stdout = '', stderr = '';
    child.stdout.on('data', chunk => stdout += chunk);
    child.stderr.on('data', chunk => stderr += chunk);
    child.on('close', status => resolve({status, stdout, stderr}));
  });
}
const sha = data => createHash('sha256').update(data).digest('hex');

// Serves a fake release: hosts.json plus the given asset bytes.
async function withRelease({files, manifestHosts, manifestVersion = version}, body) {
  const manifest = {schema: 1, version: manifestVersion, bundleFormat: 'tsgpui-1', hosts: manifestHosts};
  const server = http.createServer((request, response) => {
    const name = request.url.slice(1);
    if (name === 'hosts.json') return response.end(JSON.stringify(manifest));
    if (files[name]) return response.end(files[name]);
    response.statusCode = 404; response.end('missing');
  });
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
  const home = fs.mkdtempSync(path.join(os.tmpdir(), 'tsgpui-hosts-'));
  try {
    await body({home, env: {TSGPUI_HOME: home, TSGPUI_HOST_BASE_URL: `http://127.0.0.1:${server.address().port}`}});
  } finally {
    server.close();
    fs.rmSync(home, {recursive: true, force: true});
  }
}
const entry = (renderer, data) => ({renderer, target, asset: assetName(renderer, target), sha256: sha(data), size: data.length});
const cached = (home, renderer) => path.join(home, 'host', target, `${renderer === 'webview' ? 'tsgpui-webview-host' : 'tsgpui-host'}${exeSuffix()}`);

test('host fetch downloads, verifies and records a prebuilt host', async () => {
  const data = Buffer.from('native host bytes');
  await withRelease({files: {[assetName('native', target)]: data}, manifestHosts: [entry('native', data)]}, async ({home, env}) => {
    const result = await run(['host', 'fetch', '--renderer', 'native'], env);
    assert.equal(result.status, 0, result.stderr);
    assert.deepEqual(fs.readFileSync(cached(home, 'native')), data);
    const record = JSON.parse(fs.readFileSync(`${cached(home, 'native')}.json`, 'utf8'));
    assert.equal(record.version, version);
    assert.equal(record.sha256, sha(data));
    const doctor = await run(['doctor', '--renderer', 'native'], env);
    assert.equal(doctor.status, 0, doctor.stdout);
    assert.match(doctor.stdout, /checksum verified/);
    const again = await run(['host', 'fetch', '--renderer', 'native'], env);
    assert.match(again.stdout, /already installed/);
  });
});

test('host fetch rejects a download that does not match the manifest hash', async () => {
  const expected = Buffer.from('the real host');
  await withRelease({files: {[assetName('native', target)]: Buffer.from('a tampered host')}, manifestHosts: [{...entry('native', expected), size: 'a tampered host'.length}]}, async ({home, env}) => {
    const result = await run(['host', 'fetch', '--renderer', 'native'], env);
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /SHA-256 check/);
    assert.ok(!fs.existsSync(cached(home, 'native')), 'nothing may be installed after a failed check');
  });
});

test('host fetch says which targets a release does have when yours is missing', async () => {
  await withRelease({files: {}, manifestHosts: [{renderer: 'native', target: 'linux-arm64', asset: assetName('native', 'linux-arm64'), sha256: sha('x'), size: 1}]}, async ({env}) => {
    const result = await run(['host', 'fetch', '--renderer', 'native'], env);
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, new RegExp(`no prebuilt native host for ${target}`));
    assert.match(result.stderr, /linux-arm64/);
  });
});

test('host fetch refuses a manifest from another release series', async () => {
  const data = Buffer.from('host');
  await withRelease({files: {[assetName('native', target)]: data}, manifestHosts: [entry('native', data)], manifestVersion: '9.9.0'}, async ({env}) => {
    const result = await run(['host', 'fetch', '--renderer', 'native'], env);
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /9\.9\.0/);
  });
});

test('doctor fails, with the fix, for a cached host from an incompatible release', async () => {
  await withRelease({files: {}, manifestHosts: []}, async ({home, env}) => {
    const file = cached(home, 'native');
    fs.mkdirSync(path.dirname(file), {recursive: true});
    fs.writeFileSync(file, 'old host');
    fs.writeFileSync(`${file}.json`, JSON.stringify({version: '0.1.0', bundleFormat: 'tsgpui-1', sha256: sha('old host')}));
    const doctor = await run(['doctor', '--renderer', 'native'], env);
    assert.equal(doctor.status, 1);
    assert.match(doctor.stdout, /does not fit this CLI/);
    assert.match(doctor.stdout, /fix: tsgpui host fetch --renderer native --force/);
  });
});

test('doctor flags a cached host whose bytes no longer match the recorded hash', async () => {
  await withRelease({files: {}, manifestHosts: []}, async ({home, env}) => {
    const file = cached(home, 'native');
    fs.mkdirSync(path.dirname(file), {recursive: true});
    fs.writeFileSync(file, 'replaced');
    fs.writeFileSync(`${file}.json`, JSON.stringify({version, bundleFormat: 'tsgpui-1', sha256: sha('original')}));
    const doctor = await run(['doctor', '--renderer', 'native'], env);
    assert.equal(doctor.status, 1);
    assert.match(doctor.stdout, /fails its SHA-256 check/);
  });
});
