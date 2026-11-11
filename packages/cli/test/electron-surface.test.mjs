import {test} from 'node:test';
import assert from 'node:assert/strict';
import {mkdtemp, mkdir, writeFile, readFile, rm} from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import {spawnSync} from 'node:child_process';
import {fileURLToPath} from 'node:url';
import {COVERAGE, statusOf} from '../src/electron-compat/coverage.mjs';
import {coverageSource} from '../src/electron-compat/generate-coverage.mjs';
import {importElectron} from '../src/import-electron.mjs';

const here = path.dirname(fileURLToPath(import.meta.url));
const surface = JSON.parse(await readFile(path.join(here, '../src/electron-compat/surface/electron-33.json'), 'utf8'));
const cli = path.join(here, '../bin/tsgpui.mjs');
const surfaceNames = Object.keys(surface.exports);

test('every export of the Electron surface is classified in the coverage ledger', () => {
  const missing = surfaceNames.filter(name => !statusOf(name));
  assert.deepEqual(missing, [], `Classify these in src/electron-compat/coverage.mjs: ${missing.join(', ')}`);
});

test('the coverage ledger does not list names that Electron does not export', () => {
  const listed = [...Object.keys(COVERAGE.implemented), ...Object.keys(COVERAGE.partial), ...Object.keys(COVERAGE.unsupported)];
  const stale = listed.filter(name => !surfaceNames.includes(name));
  assert.deepEqual(stale, [], `Remove or rename these ledger entries: ${stale.join(', ')}`);
  const duplicated = listed.filter((name, index) => listed.indexOf(name) !== index);
  assert.deepEqual(duplicated, [], 'an API may have only one status');
});

test('the committed runtime coverage module is the generated one', async () => {
  // Git on Windows can check the file out with CRLF line endings, so compare with LF endings.
  const committed = (await readFile(path.join(here, '../src/electron-compat/runtime/electron-coverage.ts'), 'utf8')).replace(/\r\n/g, '\n');
  assert.equal(committed, coverageSource(), 'Regenerate runtime/electron-coverage.ts from coverage.mjs');
});

test('generated runtime binds every unsupported API and no implemented one', () => {
  const source = coverageSource();
  for (const name of Object.keys(COVERAGE.unsupported)) assert.match(source, new RegExp(`export const ${name} = unsupportedModule\\(`));
  for (const name of [...Object.keys(COVERAGE.implemented), ...Object.keys(COVERAGE.partial)]) {
    assert.doesNotMatch(source, new RegExp(`export const ${name} =`), `${name} must come from electron.ts, not the generated file`);
  }
});

test('the import report names every partial or unsupported Electron API the app uses', async t => {
  const tmp = await mkdtemp(path.join(os.tmpdir(), 'tsgpui-surface-')); t.after(() => rm(tmp, {recursive: true, force: true}));
  const app = path.join(tmp, 'app'), out = path.join(tmp, 'converted');
  await mkdir(path.join(app, 'dist'), {recursive: true});
  await writeFile(path.join(app, 'package.json'), JSON.stringify({name: 'api-app', version: '1.0.0', main: 'main.js'}));
  await writeFile(path.join(app, 'main.js'),
    "const { app, BrowserWindow, screen, Tray } = require('electron');\n" +
    "const electron = require('electron');\n" +
    "electron.powerMonitor.on('suspend', () => {});\n" +
    "electron.notARealApi.call();\n" +
    "app.whenReady().then(() => new BrowserWindow({ width: 500, height: 400 }));\n");
  await writeFile(path.join(app, 'dist', 'index.html'), '<html><head></head><body>x</body></html>');
  await importElectron(app, {out});
  const report = await readFile(path.join(out, 'TSGPUI_IMPORT_REPORT.md'), 'utf8');
  assert.doesNotMatch(report, /electron\.screen is partial/);
  assert.doesNotMatch(report, /electron\.Tray is partial/);
  assert.doesNotMatch(report, /electron\.powerMonitor is partial/);
  assert.match(report, /electron\.notARealApi: it is not in the TSGPUI coverage ledger|Unknown Electron API electron\.notARealApi/);
  assert.doesNotMatch(report, /electron\.BrowserWindow is partial/);
  assert.doesNotMatch(report, /electron\.app is unsupported/);
  const runtime = await readFile(path.join(out, 'src/tsgpui-runtime/electron-coverage.ts'), 'utf8');
  assert.doesNotMatch(runtime, /powerMonitor/);
});

test('build rejects an unknown target and lists the supported ones', () => {
  const result = spawnSync(process.execPath, [cli, 'build', '--project', here, '--target', 'plan9-x64'], {encoding: 'utf8'});
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /Unsupported target plan9-x64\. Use one of: win32-x64/);
});

test('a cross-target build without a host explains how to get one when downloads are off', async t => {
  const tmp = await mkdtemp(path.join(os.tmpdir(), 'tsgpui-target-')); t.after(() => rm(tmp, {recursive: true, force: true}));
  const project = path.join(tmp, 'app');
  await mkdir(path.join(project, 'src'), {recursive: true});
  await writeFile(path.join(project, 'tsgpui.config.json'), JSON.stringify({name: 'x', title: 'x', version: '1.0.0', renderer: 'webview', html: 'src/index.html', css: 'src/styles.css', backend: 'src/backend.ts', window: {width: 400, height: 300}}));
  await writeFile(path.join(project, 'src/index.html'), '<html><body>x</body></html>');
  await writeFile(path.join(project, 'src/styles.css'), '');
  await writeFile(path.join(project, 'src/backend.ts'), 'export default { state: {}, actions: {} };');
  const other = process.platform === 'win32' ? 'linux-x64' : 'win32-x64';
  const result = spawnSync(process.execPath, [cli, 'build', '--project', project, '--target', other, '--renderer', 'webview'],
    {encoding: 'utf8', env: {...process.env, TSGPUI_NO_DOWNLOAD: '1', TSGPUI_HOME: path.join(tmp, 'home')}});
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /downloads are disabled|No webview host for/);
});
