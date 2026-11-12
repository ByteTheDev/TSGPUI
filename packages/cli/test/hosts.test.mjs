import {test} from 'node:test';
import assert from 'node:assert/strict';
import {spawn, spawnSync} from 'node:child_process';
import {createHash} from 'node:crypto';
import fs from 'node:fs';
import http from 'node:http';
import os from 'node:os';
import path from 'node:path';
import {fileURLToPath} from 'node:url';
import {assetName, currentTarget, exeSuffix, sourceStamp, BUNDLE_FORMAT, validateManifest} from '../src/hosts.mjs';

const cliRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const bin = path.join(cliRoot, 'bin/tsgpu.mjs');
const version = JSON.parse(fs.readFileSync(path.join(cliRoot, 'package.json'), 'utf8')).version;
const target = currentTarget();

function run(args, env, binary = bin) {
  return new Promise(resolve => {
    const child = spawn(process.execPath, [binary, ...args], {env: {...process.env, ...env}});
    let stdout = '', stderr = '';
    child.stdout.on('data', chunk => stdout += chunk);
    child.stderr.on('data', chunk => stderr += chunk);
    child.on('close', status => resolve({status, stdout, stderr}));
  });
}
const sha = data => createHash('sha256').update(data).digest('hex');

// Serves a fake release: hosts.json plus the given asset bytes.
async function withRelease({files, manifestHosts, manifestVersion = version}, body) {
  const manifest = {schema: 2, version: manifestVersion, bundleFormat: BUNDLE_FORMAT, hosts: manifestHosts};
  const server = http.createServer((request, response) => {
    const name = request.url.slice(1);
    if (name === 'hosts.json') return response.end(JSON.stringify(manifest));
    if (files[name]) return response.end(files[name]);
    response.statusCode = 404; response.end('missing');
  });
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
  const home = fs.mkdtempSync(path.join(os.tmpdir(), 'tsgpu-hosts-'));
  try {
    await body({home, env: {TSGPU_HOME: home, TSGPU_HOST_BASE_URL: `http://127.0.0.1:${server.address().port}`}});
  } finally {
    server.close();
    fs.rmSync(home, {recursive: true, force: true});
  }
}
const entry = (renderer, data) => ({renderer, target, asset: assetName(renderer, target), sha256: sha(data), size: data.length});
// hosts.json v2 naming (charter): rendrix-host-<target>, and tsgpu-gpui-host-<target> while deprecated.
const cached = (home, renderer) => path.join(home, 'host', target, `${renderer === 'rendrix' ? 'rendrix-host' : 'tsgpu-gpui-host'}${exeSuffix()}`);

test('release asset names follow the hosts.json v2 naming', () => {
  assert.equal(assetName('rendrix', 'win32-x64'), 'rendrix-host-win32-x64.exe');
  assert.equal(assetName('rendrix', 'linux-arm64'), 'rendrix-host-linux-arm64');
  assert.equal(assetName('native', 'darwin-arm64'), 'tsgpu-gpui-host-darwin-arm64');
  assert.equal(assetName('rendrix', target), `rendrix-host-${target}${exeSuffix(target)}`);
});

test('hosts.json v2 accepts rendrix and deprecated gpui entries and rejects webview entries', () => {
  const ok = {schema: 2, version, bundleFormat: BUNDLE_FORMAT, hosts: [
    {renderer: 'rendrix', target, asset: `rendrix-host-${target}${exeSuffix(target)}`, sha256: sha('r'), size: 1},
    {renderer: 'native', target, asset: `tsgpu-gpui-host-${target}${exeSuffix(target)}`, sha256: sha('g'), size: 1},
  ]};
  assert.equal(validateManifest(ok, version), ok);
  for (const webviewEntry of [{renderer: 'webview', target, asset: 'tsgpu-webview-host.exe', sha256: sha('w'), size: 1}]) {
    assert.throws(() => validateManifest({...ok, hosts: webviewEntry}, version));
  }
});

test('installed CLI fetches a host on first build with or without packaged native sources', async t => {
  const bun = process.platform === 'win32' ? 'bun.exe' : 'bun';
  if (spawnSync(bun, ['--version'], {stdio:'ignore'}).status !== 0) return t.skip('Bun is needed to build the installed CLI fixture');
  const data = Buffer.alloc(1024);
  if (process.platform === 'win32') data.write('MZ');
  else if (process.platform === 'darwin') data.writeUInt32BE(0xfeedfacf);
  else data.set([0x7f, 0x45, 0x4c, 0x46]);
  await withRelease({files: {[assetName('rendrix', target)]: data}, manifestHosts: [entry('rendrix', data)]}, async ({home, env}) => {
    const pkg = path.join(home, 'installed');
    const binary = path.join(pkg, 'dist', 'tsgpu.mjs');
    fs.mkdirSync(path.dirname(binary), {recursive:true});
    fs.copyFileSync(path.join(cliRoot, 'package.json'), path.join(pkg, 'package.json'));
    const bundled = spawnSync(bun, ['build', bin, '--target=node', `--outfile=${binary}`], {encoding:'utf8'});
    assert.equal(bundled.status, 0, bundled.stderr);
    const project = path.join(home, 'app');
    fs.mkdirSync(path.join(project, 'src'), {recursive:true});
    fs.writeFileSync(path.join(project, 'tsgpu.config.json'), JSON.stringify({name:'app', title:'app', version:'1.0.0', html:'src/index.html', css:'src/style.css', backend:'src/backend.ts'}));
    fs.writeFileSync(path.join(project, 'src', 'index.html'), '<html><body>Hello</body></html>');
    fs.writeFileSync(path.join(project, 'src', 'style.css'), '');
    fs.writeFileSync(path.join(project, 'src', 'backend.ts'), 'export default {state:{}, actions:{}};');
    for (const packagedSource of [false, true]) {
      if (packagedSource) {
        fs.mkdirSync(path.join(pkg, 'native', 'src'), {recursive:true});
        fs.writeFileSync(path.join(pkg, 'native', 'Cargo.toml'), 'manifest');
        fs.writeFileSync(path.join(pkg, 'native', 'Cargo.lock'), 'lock');
      }
      const cacheHome = path.join(home, packagedSource ? 'with-source' : 'without-source');
      const result = await run(['build', '--project', project], {...env, TSGPU_HOME:cacheHome, TSGPU_NO_DOWNLOAD:'', TSGPU_HOST:''}, binary);
      assert.equal(result.status, 0, result.stdout + result.stderr);
      assert.deepEqual(fs.readFileSync(cached(cacheHome, 'rendrix')), data);
    }
  });
});

test('host fetch downloads, verifies and records the rendrix host', async () => {
  const data = Buffer.from('rendrix host bytes');
  await withRelease({files: {[assetName('rendrix', target)]: data}, manifestHosts: [entry('rendrix', data)]}, async ({home, env}) => {
    const result = await run(['host', 'fetch', '--renderer', 'rendrix'], env);
    assert.equal(result.status, 0, result.stderr);
    assert.deepEqual(fs.readFileSync(cached(home, 'rendrix')), data);
    const record = JSON.parse(fs.readFileSync(`${cached(home, 'rendrix')}.json`, 'utf8'));
    assert.equal(record.version, version);
    assert.equal(record.sha256, sha(data));
    const doctor = await run(['doctor', '--renderer', 'rendrix'], env);
    assert.equal(doctor.status, 0, doctor.stdout);
    assert.match(doctor.stdout, /checksum verified/);
    const again = await run(['host', 'fetch', '--renderer', 'rendrix'], env);
    assert.match(again.stdout, /already installed/);
  });
});

test('the deprecated gpui host is still fetchable under its v2 name', async () => {
  const data = Buffer.from('gpui host bytes');
  await withRelease({files: {[assetName('native', target)]: data}, manifestHosts: [entry('native', data)]}, async ({home, env}) => {
    const result = await run(['host', 'fetch', '--renderer', 'native'], env);
    assert.equal(result.status, 0, result.stderr);
    assert.deepEqual(fs.readFileSync(cached(home, 'native')), data);
  });
});

test('host fetch refuses the removed webview renderer with migration guidance', async () => {
  await withRelease({files: {}, manifestHosts: []}, async ({env}) => {
    const result = await run(['host', 'fetch', '--renderer', 'webview'], env);
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /webview/i);
    assert.match(result.stderr, /rendrix/);
  });
});

test('host fetch rejects a download that does not match the manifest hash', async () => {
  const expected = Buffer.from('the real host');
  await withRelease({files: {[assetName('rendrix', target)]: Buffer.from('a tampered host')}, manifestHosts: [{...entry('rendrix', expected), size: 'a tampered host'.length}]}, async ({home, env}) => {
    const result = await run(['host', 'fetch', '--renderer', 'rendrix'], env);
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /SHA-256 check/);
    assert.ok(!fs.existsSync(cached(home, 'rendrix')), 'nothing may be installed after a failed check');
  });
});

test('host fetch says which targets a release does have when yours is missing', async () => {
  await withRelease({files: {}, manifestHosts: [{renderer: 'rendrix', target: 'linux-arm64', asset: assetName('rendrix', 'linux-arm64'), sha256: sha('x'), size: 1}]}, async ({env}) => {
    const result = await run(['host', 'fetch', '--renderer', 'rendrix'], env);
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, new RegExp(`no prebuilt rendrix host for ${target}`));
    assert.match(result.stderr, /linux-arm64/);
  });
});

test('host fetch refuses a manifest from another release series', async () => {
  const data = Buffer.from('host');
  await withRelease({files: {[assetName('rendrix', target)]: data}, manifestHosts: [entry('rendrix', data)], manifestVersion: '9.9.0'}, async ({env}) => {
    const result = await run(['host', 'fetch', '--renderer', 'rendrix'], env);
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /9\.9\.0/);
  });
});

test('doctor fails, with the fix, for a cached host from an incompatible release', async () => {
  await withRelease({files: {}, manifestHosts: []}, async ({home, env}) => {
    const file = cached(home, 'rendrix');
    fs.mkdirSync(path.dirname(file), {recursive: true});
    fs.writeFileSync(file, 'old host');
    fs.writeFileSync(`${file}.json`, JSON.stringify({version: '0.1.0', bundleFormat: BUNDLE_FORMAT, sha256: sha('old host')}));
    const doctor = await run(['doctor', '--renderer', 'rendrix'], env);
    assert.equal(doctor.status, 1);
    assert.match(doctor.stdout, /does not fit this CLI/);
    assert.match(doctor.stdout, /fix: tsgpu host fetch --renderer rendrix --force/);
  });
});

test('doctor flags a cached host whose bytes no longer match the recorded hash', async () => {
  await withRelease({files: {}, manifestHosts: []}, async ({home, env}) => {
    const file = cached(home, 'rendrix');
    fs.mkdirSync(path.dirname(file), {recursive: true});
    fs.writeFileSync(file, 'replaced');
    fs.writeFileSync(`${file}.json`, JSON.stringify({version, bundleFormat: BUNDLE_FORMAT, sha256: sha('original')}));
    const doctor = await run(['doctor', '--renderer', 'rendrix'], env);
    assert.equal(doctor.status, 1);
    assert.match(doctor.stdout, /fails its SHA-256 check/);
  });
});

test('source stamps are keyed by the new renderer names', () => {
  assert.notEqual(sourceStamp(path.resolve(cliRoot, '../../native'), 'rendrix'), sourceStamp(path.resolve(cliRoot, '../../native'), 'native'));
});
