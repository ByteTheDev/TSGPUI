import {test} from 'node:test';
import assert from 'node:assert/strict';
import {spawn, spawnSync} from 'node:child_process';
import {createHash} from 'node:crypto';
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
  for (const command of ['init', 'compile', 'dev', 'build', 'host fetch', 'host build', 'doctor']) {
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
  assert.match(result.stdout, /native renderer:/);
  assert.match(result.stdout, /webview renderer:/);
});


test('TSGPUI_HOST names the native host only, so a WebView project does not use it', () => {
  const home = mkdtempSync(path.join(os.tmpdir(), 'tsgpui-env-host-'));
  try {
    const env = {...process.env, TSGPUI_HOME: home, TSGPUI_HOST: process.execPath, TSGPUI_WEBVIEW_HOST: ''};
    const native = tsgpui(['doctor', '--renderer', 'native'], {env});
    assert.match(native.stdout, /host from --host\/environment/);
    const webview = tsgpui(['doctor', '--renderer', 'webview'], {env});
    assert.doesNotMatch(webview.stdout, /host from --host\/environment/);
  } finally {fs.rmSync(home, {recursive: true, force: true});}
});

test('host installer verifies the hash before copying', () => {
  const home=mkdtempSync(path.join(os.tmpdir(),'tsgpui-host-install-'));
  try {
    const file=path.join(home,'input'); fs.writeFileSync(file,'host fixture');
    const hash=createHash('sha256').update('host fixture').digest('hex');
    const env={...process.env,TSGPUI_HOME:home};
    const bad=tsgpui(['host','install','--renderer','native','--file',file,'--sha256','0'.repeat(64)],{env});
    assert.notEqual(bad.status,0); assert.match(bad.stderr,/mismatch/);
    const good=tsgpui(['host','install','--renderer','native','--file',file,'--sha256',hash],{env});
    assert.equal(good.status,0,good.stderr);
    assert.equal(fs.readFileSync(path.join(home,'host',`${process.platform}-${process.arch}`,`tsgpui-host${process.platform==='win32'?'.exe':''}`),'utf8'),'host fixture');
  } finally {fs.rmSync(home,{recursive:true,force:true});}
});

test('dev recovers when a missing nested backend module is created and rebuilds on edits', {timeout:20000}, async () => {
  const dir=mkdtempSync(path.join(os.tmpdir(),'tsgpui-dev-'));
  let child;
  const waitFor=async predicate=>{ const until=Date.now()+8000; while(Date.now()<until){if(predicate())return; await new Promise(resolve=>setTimeout(resolve,50));} throw Error('Timed out waiting for dev rebuild'); };
  try {
    fs.mkdirSync(path.join(dir,'src'));
    fs.writeFileSync(path.join(dir,'tsgpui.config.json'),JSON.stringify({name:'watch-test',html:'src/index.html',css:'src/style.css',backend:'src/backend.ts'}));
    fs.writeFileSync(path.join(dir,'src/index.html'),'<body><p>Test</p></body>');
    fs.writeFileSync(path.join(dir,'src/style.css'),'');
    fs.writeFileSync(path.join(dir,'src/backend.ts'),"import {value} from './nested/helper'; export default {state:{value},actions:{}};");
    let logs=''; child=spawn(process.execPath,[bin,'dev','--project',dir,'--host',process.execPath],{env:process.env,stdio:['ignore','pipe','pipe']});
    child.stdout.on('data',chunk=>logs+=chunk); child.stderr.on('data',chunk=>logs+=chunk);
    await waitFor(()=>logs.includes('Watching project source directories'));
    fs.mkdirSync(path.join(dir,'src/nested')); fs.writeFileSync(path.join(dir,'src/nested/helper.ts'),'export const value = 17;');
    const bundle=path.join(dir,'dist/watch-test.tgp');
    await waitFor(()=>fs.existsSync(bundle) && fs.readFileSync(bundle,'utf8').includes('value = 17'));
    fs.writeFileSync(path.join(dir,'src/nested/helper.ts'),'export const value = 42;');
    await waitFor(()=>fs.readFileSync(bundle,'utf8').includes('value = 42'));
  } finally { if(child) {child.kill(); await new Promise(resolve=>child.once('exit',resolve));} fs.rmSync(dir,{recursive:true,force:true}); }
});


test('release manifest is deterministic and hashes artifact files', () => {
  const dir=mkdtempSync(path.join(os.tmpdir(),'tsgpui-release-'));
  try {
    fs.writeFileSync(path.join(dir,'b'),'B'); fs.writeFileSync(path.join(dir,'a'),'A');
    const a=tsgpui(['release','manifest','--file',dir]), b=tsgpui(['release','manifest','--file',dir]);
    assert.equal(a.status,0,a.stderr); assert.equal(a.stdout,b.stdout);
    const manifest=JSON.parse(a.stdout);
    assert.deepEqual(manifest.files.map(f=>f.path),['a','b']);
    assert.equal(manifest.files[0].sha256,createHash('sha256').update('A').digest('hex'));
  } finally {fs.rmSync(dir,{recursive:true,force:true});}
});
