import {test} from 'node:test';
import assert from 'node:assert/strict';
import {mkdtemp, mkdir, writeFile, rm, access} from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import {spawnSync} from 'node:child_process';
import {fileURLToPath} from 'node:url';
import {SUPPORTED_TARGETS} from '../src/hosts.mjs';

const cli = path.join(path.dirname(fileURLToPath(import.meta.url)), '../bin/rendrix.mjs');

test('the supported targets cover Windows, macOS and Linux on x64 and arm64', () => {
  assert.deepEqual(SUPPORTED_TARGETS, ['win32-x64', 'win32-arm64', 'darwin-arm64', 'linux-x64', 'linux-arm64']);
});

test('build --target all attempts every target and reports each outcome', async t => {
  const tmp = await mkdtemp(path.join(os.tmpdir(), 'rendrix-all-')); t.after(() => rm(tmp, {recursive: true, force: true}));
  const project = path.join(tmp, 'app');
  await mkdir(path.join(project, 'src'), {recursive: true});
  await writeFile(path.join(project, 'rendrix.config.json'), JSON.stringify({name: 'allapp', title: 'allapp', version: '1.0.0', renderer: 'rendrix', html: 'src/index.html', css: 'src/styles.css', backend: 'src/backend.ts', window: {width: 400, height: 300}}));
  await writeFile(path.join(project, 'src/index.html'), '<html><body>x</body></html>');
  await writeFile(path.join(project, 'src/styles.css'), '');
  await writeFile(path.join(project, 'src/backend.ts'), 'export default { state: {}, actions: {} };');
  const env = {...process.env, RENDRIX_NO_DOWNLOAD: '1', RENDRIX_HOME: path.join(tmp, 'home')};
  // This test checks failure reporting, not a potentially expensive local Rust build.
  for (const key of Object.keys(env)) if (key.toLowerCase() === 'path') delete env[key];
  env.PATH = tmp;
  const result = spawnSync(process.execPath, [cli, 'build', '--project', project, '--target', 'all', '--renderer', 'rendrix'], {encoding: 'utf8', env});
  const output = result.stdout + result.stderr;
  for (const target of SUPPORTED_TARGETS) assert.match(output, new RegExp(`== ${target}`));
  assert.match(output, /Built \d+ of 5 targets\./);
});

test('a comma-separated target list is accepted and an unknown entry is rejected', () => {
  const bad = spawnSync(process.execPath, [cli, 'build', '--target', 'linux-x64,beos-x64'], {encoding: 'utf8'});
  assert.notEqual(bad.status, 0);
  assert.match(bad.stderr, /Unsupported target beos-x64/);
});

test('target shortcuts preserve separate output files', async t => {
  const tmp = await mkdtemp(path.join(os.tmpdir(), 'rendrix-target-')); t.after(() => rm(tmp, {recursive: true, force: true}));
  const project = path.join(tmp, 'app');
  await mkdir(path.join(project, 'src'), {recursive: true});
  await writeFile(path.join(project, 'rendrix.config.json'), JSON.stringify({name:'demo', title:'demo', version:'1.0.0', renderer:'rendrix', html:'src/index.html', css:'src/styles.css', backend:'src/backend.ts', window:{width:400,height:300}}));
  await writeFile(path.join(project, 'src/index.html'), '<html><body>x</body></html>');
  await writeFile(path.join(project, 'src/styles.css'), '');
  await writeFile(path.join(project, 'src/backend.ts'), 'export default { state: {}, actions: {} };');
  const host = path.join(tmp, 'host');
  const bytes = Buffer.alloc(1024); bytes.write('MZ'); await writeFile(host, bytes);
  for (const target of ['windows-x64', 'windows-arm64']) {
    const result = spawnSync(process.execPath, [cli, target, '--project', project, '--host', host], {encoding:'utf8'});
    assert.equal(result.status, 0, result.stderr);
  }
  await access(path.join(project, 'dist', 'demo-win32-x64.exe'));
  await access(path.join(project, 'dist', 'demo-win32-arm64.exe'));
});
