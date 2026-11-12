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
const bin = path.join(cliRoot, 'bin/tsgpu.mjs');
const version = JSON.parse(fs.readFileSync(path.join(cliRoot, 'package.json'), 'utf8')).version;

function tsgpu(args, options = {}) {
  return spawnSync(process.execPath, [bin, ...args], {encoding: 'utf8', ...options});
}

// Creates a project through the CLI, then returns its config file so tests can edit
// specific keys without depending on the template's renderer default.
function initProject(parent) {
  const dir = path.join(parent, 'my-app');
  const created = tsgpu(['init', dir]);
  assert.equal(created.status, 0, created.stderr);
  const configFile = fs.readdirSync(dir).find(name => name.endsWith('.config.json'));
  assert.ok(configFile, `init created a config file in ${dir}`);
  return {dir, configFile: path.join(dir, configFile)};
}

function writeProjectFiles(dir, config) {
  const configFile = fs.readdirSync(dir).find(name => name.endsWith('.config.json'));
  fs.writeFileSync(path.join(dir, configFile), JSON.stringify(config));
  fs.writeFileSync(path.join(dir, config.html), '<html><body><p>Hello</p></body></html>');
  fs.writeFileSync(path.join(dir, config.css), 'p { color: #336699; }');
  fs.writeFileSync(path.join(dir, config.backend), 'export default {state:{},actions:{}};');
}

test('--version prints the package version', () => {
  const result = tsgpu(['--version']);
  assert.equal(result.status, 0, result.stderr);
  assert.equal(result.stdout.trim(), version);
});

test('help lists the commands', () => {
  const result = tsgpu(['help']);
  assert.equal(result.status, 0, result.stderr);
  for (const command of ['init', 'compile', 'dev', 'build', 'host fetch', 'host build', 'doctor']) {
    assert.match(result.stdout, new RegExp(command));
  }
});

test('init creates a project from the packaged template and compile produces a bundle', () => {
  const dir = path.join(mkdtempSync(path.join(os.tmpdir(), 'tsgpu-init-')), 'my-app');
  const created = tsgpu(['init', dir]);
  assert.equal(created.status, 0, created.stderr);
  for (const file of ['src/index.html', 'src/styles.css', 'src/backend.ts', 'package.json']) {
    assert.ok(fs.existsSync(path.join(dir, file)), `missing ${file}`);
  }
  assert.ok(fs.readdirSync(dir).some(file => file.endsWith('.config.json')), 'a config file exists');
  assert.equal(JSON.parse(fs.readFileSync(path.join(dir, 'package.json'), 'utf8')).name, 'my-app');

  const compiled = tsgpu(['compile', '--project', dir]);
  assert.equal(compiled.status, 0, compiled.stderr);
  const bundle = JSON.parse(fs.readFileSync(path.join(dir, 'dist/my-app.tgp'), 'utf8'));
  assert.equal(bundle.name, 'my-app');
});

test('init refuses a non-empty destination', () => {
  const dir = mkdtempSync(path.join(os.tmpdir(), 'tsgpu-nonempty-'));
  fs.writeFileSync(path.join(dir, 'keep.txt'), 'x');
  const result = tsgpu(['init', dir]);
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /not empty/);
});

test('an absent renderer key defaults to rendrix', () => {
  const parent = mkdtempSync(path.join(os.tmpdir(), 'tsgpu-rendrix-default-'));
  const {dir: project, configFile} = initProject(parent);
  const config = JSON.parse(fs.readFileSync(configFile, 'utf8'));
  delete config.renderer;
  writeProjectFiles(project, config);
  const compiled = tsgpu(['compile', '--project', project]);
  assert.equal(compiled.status, 0, compiled.stderr);
  const bundle = JSON.parse(fs.readFileSync(path.join(project, 'dist', `${config.name}.tgp`), 'utf8'));
  assert.equal(bundle.renderer, 'rendrix');
});

test('the deprecated native renderer compiles with a deprecation warning', () => {
  const parent = mkdtempSync(path.join(os.tmpdir(), 'tsgpu-native-warn-'));
  const {dir: project, configFile} = initProject(parent);
  const config = JSON.parse(fs.readFileSync(configFile, 'utf8'));
  config.renderer = 'native';
  fs.writeFileSync(configFile, JSON.stringify(config));
  const compiled = tsgpu(['compile', '--project', project]);
  assert.equal(compiled.status, 0, compiled.stderr);
  assert.match(compiled.stderr + compiled.stdout, /deprecat/i);
  const bundle = JSON.parse(fs.readFileSync(path.join(project, 'dist', `${config.name}.tgp`), 'utf8'));
  assert.equal(bundle.renderer, 'native');
});

test('the webview renderer is a hard error with migration guidance', () => {
  const parent = mkdtempSync(path.join(os.tmpdir(), 'tsgpu-webview-error-'));
  const {dir: project, configFile} = initProject(parent);
  const config = JSON.parse(fs.readFileSync(configFile, 'utf8'));
  config.renderer = 'webview';
  fs.writeFileSync(configFile, JSON.stringify(config));
  const compiled = tsgpu(['compile', '--project', project]);
  assert.notEqual(compiled.status, 0);
  assert.match(compiled.stderr + compiled.stdout, /webview/i);
  assert.match(compiled.stderr + compiled.stdout, /rendrix/);
});

test('the legacy tsgpui.config.json name alone still compiles, with a rename warning', () => {
  const parent = mkdtempSync(path.join(os.tmpdir(), 'tsgpu-legacy-config-'));
  const {dir: project, configFile} = initProject(parent);
  const config = JSON.parse(fs.readFileSync(configFile, 'utf8'));
  fs.writeFileSync(configFile, JSON.stringify(config)); // canonical name, as init created it
  fs.renameSync(configFile, path.join(project, 'tsgpui.config.json'));
  const compiled = tsgpu(['compile', '--project', project]);
  assert.equal(compiled.status, 0, compiled.stderr);
  assert.match(compiled.stderr + compiled.stdout, /tsgpui\.config\.json[\s\S]*tsgpu\.config\.json|rename it to tsgpu\.config\.json/);
  assert.ok(fs.existsSync(path.join(project, 'dist', `${config.name}.tgp`)), 'legacy config still compiles');
});

test('tsgpu.config.json wins when both config names exist', () => {
  const parent = mkdtempSync(path.join(os.tmpdir(), 'tsgpu-config-precedence-'));
  const {dir: project, configFile} = initProject(parent);
  const legacy = path.join(project, 'tsgpui.config.json');
  const config = JSON.parse(fs.readFileSync(configFile, 'utf8'));
  fs.writeFileSync(configFile, JSON.stringify({...config, name: 'new-name'}));
  fs.writeFileSync(legacy, JSON.stringify({...config, name: 'legacy-name'}));
  const compiled = tsgpu(['compile', '--project', project]);
  assert.equal(compiled.status, 0, compiled.stderr);
  assert.ok(fs.existsSync(path.join(project, 'dist', 'new-name.tgp')), 'the canonical config supplies the bundle name');
  assert.ok(!fs.existsSync(path.join(project, 'dist', 'legacy-name.tgp')), 'the legacy config is ignored when the canonical one exists');
});

test('doctor reports the rendrix renderer and no webview renderer', () => {
  const home = mkdtempSync(path.join(os.tmpdir(), 'tsgpu-home-'));
  const result = tsgpu(['doctor'], {env: {...process.env, TSGPU_HOME: home}});
  assert.equal(result.status, 0, result.stderr);
  assert.match(result.stdout, /rendrix/);
  assert.doesNotMatch(result.stdout, /webview/);
});

test('TSGPU_HOME selects the per-user host cache location', () => {
  const home = mkdtempSync(path.join(os.tmpdir(), 'tsgpu-cache-home-'));
  const result = tsgpu(['doctor'], {env: {...process.env, TSGPU_HOME: home}});
  assert.match(result.stdout, new RegExp(home.replaceAll('\\', '\\\\') + `[\\\\/]host`));
});

test('host installer verifies the hash before copying', () => {
  const home=mkdtempSync(path.join(os.tmpdir(),'tsgpu-host-install-'));
  try {
    const file=path.join(home,'input'); fs.writeFileSync(file,'host fixture');
    const hash=createHash('sha256').update('host fixture').digest('hex');
    const env={...process.env,TSGPU_HOME:home};
    const bad=tsgpu(['host','install','--renderer','rendrix','--file',file,'--sha256','0'.repeat(64)],{env});
    assert.notEqual(bad.status,0); assert.match(bad.stderr,/mismatch/);
    const good=tsgpu(['host','install','--renderer','rendrix','--file',file,'--sha256',hash],{env});
    assert.equal(good.status,0,good.stderr);
    assert.equal(fs.readFileSync(path.join(home,'host',`${process.platform}-${process.arch}`,`rendrix-host${process.platform==='win32'?'.exe':''}`),'utf8'),'host fixture');
  } finally {fs.rmSync(home,{recursive:true,force:true});}
});

test('dev recovers when a missing nested backend module is created and rebuilds on edits', {timeout:20000}, async () => {
  const parent = mkdtempSync(path.join(os.tmpdir(), 'tsgpu-dev-'));
  const {dir: project, configFile} = initProject(parent);
  const config = JSON.parse(fs.readFileSync(configFile, 'utf8'));
  config.name = 'watch-test';
  fs.writeFileSync(configFile, JSON.stringify(config));
  let child;
  const waitFor=async predicate=>{ const until=Date.now()+8000; while(Date.now()<until){if(predicate())return; await new Promise(resolve=>setTimeout(resolve,50));} throw Error('Timed out waiting for dev rebuild'); };
  try {
    fs.writeFileSync(path.join(project, config.html),'<body><p>Test</p></body>');
    fs.writeFileSync(path.join(project, config.css),'');
    fs.writeFileSync(path.join(project, config.backend),"import {value} from './nested/helper'; export default {state:{value},actions:{}};");
    let logs=''; child=spawn(process.execPath,[bin,'dev','--project',project,'--host',process.execPath],{env:process.env,stdio:['ignore','pipe','pipe']});
    child.stdout.on('data',chunk=>logs+=chunk); child.stderr.on('data',chunk=>logs+=chunk);
    await waitFor(()=>logs.includes('Watching project source directories'));
    fs.mkdirSync(path.join(project,'src/nested')); fs.writeFileSync(path.join(project,'src/nested/helper.ts'),'export const value = 17;');
    const bundle=path.join(project,'dist/watch-test.tgp');
    await waitFor(()=>fs.existsSync(bundle) && fs.readFileSync(bundle,'utf8').includes('value = 17'));
    fs.writeFileSync(path.join(project,'src/nested/helper.ts'),'export const value = 42;');
    await waitFor(()=>fs.readFileSync(bundle,'utf8').includes('value = 42'));
  } finally { if(child) {child.kill(); await new Promise(resolve=>child.once('exit',resolve));} fs.rmSync(parent,{recursive:true,force:true}); }
});

test('release manifest is deterministic and hashes artifact files', () => {
  const dir=mkdtempSync(path.join(os.tmpdir(),'tsgpu-release-'));
  try {
    fs.writeFileSync(path.join(dir,'b'),'B'); fs.writeFileSync(path.join(dir,'a'),'A');
    const a=tsgpu(['release','manifest','--file',dir]), b=tsgpu(['release','manifest','--file',dir]);
    assert.equal(a.status,0,a.stderr); assert.equal(a.stdout,b.stdout);
    const manifest=JSON.parse(a.stdout);
    assert.deepEqual(manifest.files.map(f=>f.path),['a','b']);
    assert.equal(manifest.files[0].sha256,createHash('sha256').update('A').digest('hex'));
  } finally {fs.rmSync(dir,{recursive:true,force:true});}
});
