import {test} from 'node:test';
import assert from 'node:assert/strict';
import {mkdtemp, mkdir, writeFile, readFile, rm} from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import {spawnSync} from 'node:child_process';
import {fileURLToPath} from 'node:url';
import vm from 'node:vm';
import {importElectron} from '../src/import-electron.mjs';
import {buildPackage} from '../src/compiler.mjs';
import {bundleBackend} from '../src/backend.mjs';

const cli=fileURLToPath(new URL('../bin/tsgpu.mjs',import.meta.url));
test('bundles a pure JavaScript package with transitive CommonJS and JSON dependencies', async t => {
  const tmp=await mkdtemp(path.join(os.tmpdir(),'tsgpui-package-')); t.after(()=>rm(tmp,{recursive:true,force:true}));
  const app=path.join(tmp,'app'), out=path.join(tmp,'out'), pkg=path.join(app,'node_modules','example-package');
  await mkdir(pkg,{recursive:true}); await mkdir(path.join(app,'dist'));
  await writeFile(path.join(app,'package.json'),JSON.stringify({name:'app',main:'main.js'}));
  await writeFile(path.join(app,'main.js'),`globalThis.packageResult = require('example-package')('hello');`);
  await writeFile(path.join(pkg,'package.json'),JSON.stringify({name:'example-package',main:'index.js'}));
  await writeFile(path.join(pkg,'index.js'),`const data = require('./data.json'); module.exports = x => x + data.suffix;`);
  await writeFile(path.join(pkg,'data.json'),JSON.stringify({suffix:' world'}));
  await writeFile(path.join(app,'dist','index.html'),'<html><body>Hello</body></html>');
  await importElectron(app,{out});
  const {bundle}=await buildPackage(out); const context=vm.createContext({console, setTimeout, URL});
  vm.runInContext(bundle.backend_js,context); assert.equal(context.packageResult,'hello world');
});
test('rejects native addons and dynamic package loaders with a named diagnostic', async t => {
  const tmp=await mkdtemp(path.join(os.tmpdir(),'tsgpui-native-addon-')); t.after(()=>rm(tmp,{recursive:true,force:true}));
  const app=path.join(tmp,'app'), pkg=path.join(app,'node_modules','native-example');
  await mkdir(pkg,{recursive:true});
  await writeFile(path.join(app,'package.json'),JSON.stringify({main:'main.js'}));
  await writeFile(path.join(app,'main.js'),`require('native-example');`);
  await writeFile(path.join(pkg,'package.json'),JSON.stringify({main:'index.js'}));
  await writeFile(path.join(pkg,'index.js'),`module.exports = require('./build/addon.node');`);
  await assert.rejects(importElectron(app,{out:path.join(tmp,'out')}), /Native addon.*addon\.node.*native-example/);
  await writeFile(path.join(pkg,'index.js'),`module.exports = require(process.env.ADDON);`);
  await assert.rejects(importElectron(app,{out:path.join(tmp,'out')}), /Dynamic require.*native-example/);
});
test('backend bundling identifies direct native addon imports', async t => {
  const tmp=await mkdtemp(path.join(os.tmpdir(),'tsgpui-backend-native-')); t.after(()=>rm(tmp,{recursive:true,force:true}));
  const entry=path.join(tmp,'backend.ts');
  await writeFile(entry,`import addon from './native.node'; export default {state:{},actions:{}};`);
  await assert.rejects(bundleBackend(entry,tmp,async()=>{}), /Native addon '\.\/native\.node'/);
});
test('imported CommonJS modules preserve callable and object module.exports values', async t => {
  const tmp=await mkdtemp(path.join(os.tmpdir(),'tsgpui-cjs-')); t.after(()=>rm(tmp,{recursive:true,force:true}));
  const app=path.join(tmp,'app'), out=path.join(tmp,'out');
  await mkdir(path.join(app,'dist'),{recursive:true});
  await writeFile(path.join(app,'package.json'),JSON.stringify({name:'example',version:'1.0.0',main:'main.js'}));
  await writeFile(path.join(app,'main.js'),`const fn = require('./fn'); const obj = require('./obj'); globalThis.__result = [fn('ok'), obj.answer, obj.default];`);
  await writeFile(path.join(app,'fn.js'),`module.exports = value => value.toUpperCase();`);
  await writeFile(path.join(app,'obj.js'),`module.exports = {answer: 42, default: 'ordinary property'};`);
  await writeFile(path.join(app,'dist','index.html'),'<html><body>Hello</body></html>');
  await importElectron(app,{out});
  const bundle=await buildPackage(out);
  const context=vm.createContext({console, setTimeout, clearTimeout, URL});
  vm.runInContext(bundle.bundle.backend_js,context);
  assert.deepEqual(Array.from(context.__result),['OK',42,'ordinary property']);
});
test('Electron subpath imports resolve to the compatibility runtime', async t => {
  const tmp=await mkdtemp(path.join(os.tmpdir(),'tsgpui-subpaths-')); t.after(()=>rm(tmp,{recursive:true,force:true}));
  const app=path.join(tmp,'app'), out=path.join(tmp,'out');
  await mkdir(path.join(app,'dist'),{recursive:true});
  await writeFile(path.join(app,'package.json'),JSON.stringify({name:'example',version:'1.0.0',main:'main.js'}));
  await writeFile(path.join(app,'main.js'),`import {BrowserWindow} from 'electron/main';\nconst {ipcMain} = require('electron/renderer');\nconst electron = require('node:electron');\nconst win = new BrowserWindow();\nwin.loadFile('dist/index.html');\n`);
  await writeFile(path.join(app,'dist','index.html'),'<html><body>Hello</body></html>');
  const {report}=await importElectron(app,{out});
  assert.doesNotMatch(report,/Unsupported module (?:electron\/main|electron\/renderer|node:electron)/);
  const main=await readFile(path.join(out,'src/main/main.ts'),'utf8');
  assert.equal((main.match(/tsgpu-runtime\/electron/g) || []).length, 3);
  await buildPackage(out);
});
test('build detects an Electron project in the working directory without a migrate command', async t => {
  const tmp=await mkdtemp(path.join(os.tmpdir(),'tsgpui-auto-')); t.after(()=>rm(tmp,{recursive:true,force:true}));
  await mkdir(path.join(tmp,'dist'));
  await writeFile(path.join(tmp,'package.json'),JSON.stringify({name:'example',version:'1.0.0',main:'main.js'}));
  await writeFile(path.join(tmp,'main.js'),"const {BrowserWindow} = require('electron'); const win = new BrowserWindow(); win.loadFile('dist/index.html');");
  await writeFile(path.join(tmp,'dist','index.html'),'<html><body>Hello</body></html>');
  const host=path.join(tmp,'host'); const bytes=Buffer.alloc(1024); bytes.write('MZ'); await writeFile(host,bytes);
  const result=spawnSync(process.execPath,[cli,'build','--target','windows-x64','--host',host],{cwd:tmp,encoding:'utf8'});
  assert.equal(result.status,0,result.stderr);
  assert.match(result.stdout,/Electron compatibility is incomplete/);
  assert.match(result.stdout,/example-win32-x64\.exe/);
  await assert.rejects(readFile(path.join(tmp,'tsgpu.config.json')),{code:'ENOENT'});
  const help=spawnSync(process.execPath,[cli,'--help'],{encoding:'utf8'});
  assert.doesNotMatch(help.stdout,/^\s*migrate\b/m);
});
test('imports Electron configuration, modules and permissions, with a review report', async t => {
  const tmp=await mkdtemp(path.join(os.tmpdir(),'tsgpui-electron-')); t.after(()=>rm(tmp,{recursive:true,force:true}));
  const app=path.join(tmp,'app'), output=path.join(tmp,'converted');
  await mkdir(path.join(app,'dist'),{recursive:true});
  await writeFile(path.join(app,'package.json'),JSON.stringify({name:'sample-app',productName:'Sample App',version:'2.3.4',main:'main.js',build:{appId:'test.app'},config:{forge:{}}}));
  await writeFile(path.join(app,'main.js'),`const { BrowserWindow, ipcMain } = require('electron');\nconst fs = require('node:fs');\nconst { spawn } = require('child_process');\nconst x = require('unsupported-package');\nconst win = new BrowserWindow({width: 800, height: 650, title: 'Hello'});\nwin.loadFile('dist/index.html');\nipcMain.handle('read', () => fs.readFile('x'));\nspawn('git', ['status']);\nfetch('https://example.com/api');\nconst https = require('node:https');\nhttps.get({hostname: 'api.example.com', path: '/status'});\n`);
  await writeFile(path.join(app,'dist','index.html'),'<html><head></head><body>Hello</body></html>');
  const result=spawnSync(process.execPath,[cli,'import','electron',app,'--out',output],{encoding:'utf8'});
  assert.equal(result.status,0,result.stderr);
  const config=JSON.parse(await readFile(path.join(output,'tsgpu.config.json'),'utf8'));
  assert.ok(config.renderer === undefined || config.renderer === 'rendrix');
  assert.equal(config.name,'sample-app'); assert.equal(config.version,'2.3.4'); assert.equal(config.title,'Hello');
  assert.deepEqual(config.window,{width:800,height:650});
  assert.deepEqual(config.permissions.filesystem,{read:['./data'],write:['./data']});
  assert.deepEqual(config.permissions.processes,['git']); assert.deepEqual(config.permissions.network,['api.example.com','example.com']); assert.equal(config.permissions.ipc,true);
  const main=await readFile(path.join(output,'src/main/main.ts'),'utf8');
  assert.match(main,/import \* as __tsgpui_import_0 from "\.\.\/tsgpu-runtime\/electron"/);
  assert.doesNotMatch(main,/require\(/);
  assert.match(main,/tsgpu-runtime\/https/);
  const report=await readFile(path.join(output,'TSGPU_IMPORT_REPORT.md'),'utf8');
  assert.match(report,/main\.js:4: Unsupported module unsupported-package/); assert.match(report,/node:fs/); assert.match(report,/tsgpu build/);
  await assert.rejects(importElectron(app,{out:output}),/already exists/);
});

test('missing renderer gives build guidance and does not create output', async t => {
  const tmp=await mkdtemp(path.join(os.tmpdir(),'tsgpui-electron-')); t.after(()=>rm(tmp,{recursive:true,force:true}));
  await writeFile(path.join(tmp,'package.json'),JSON.stringify({main:'main.js'}));
  await writeFile(path.join(tmp,'main.js'),"win.loadFile('dist/index.html');");
  await assert.rejects(importElectron(tmp,{out:tmp+'-output'}),/build.*--build/i);
});

test('a simple imported project compiles with the available compatibility runtime', async t => {
  const tmp=await mkdtemp(path.join(os.tmpdir(),'tsgpui-electron-')); t.after(()=>rm(tmp,{recursive:true,force:true}));
  const app=path.join(tmp,'app'), out=path.join(tmp,'out');
  await mkdir(path.join(app,'dist'),{recursive:true});
  await writeFile(path.join(app,'package.json'),JSON.stringify({name:'example',version:'1.0.0',main:'main.js'}));
  await writeFile(path.join(app,'main.js'),"const {BrowserWindow, ipcMain} = require('electron');\nconst win = new BrowserWindow({width: 800, height: 600});\nwin.loadFile('dist/index.html');\nipcMain.handle('ping', () => 'pong');\n");
  await writeFile(path.join(app,'dist/index.html'),'<html><head></head><body>Hello</body></html>');
  await importElectron(app,{out});
  const result=await buildPackage(out);
  assert.equal(result.bundle.renderer,'rendrix');
});

test('uses and reports linked renderer stylesheets', async t => {
  const tmp=await mkdtemp(path.join(os.tmpdir(),'tsgpui-css-')); t.after(()=>rm(tmp,{recursive:true,force:true}));
  const app=path.join(tmp,'app'), out=path.join(tmp,'out');
  await mkdir(path.join(app,'dist','css'),{recursive:true});
  await writeFile(path.join(app,'package.json'),JSON.stringify({main:'main.js'}));
  await writeFile(path.join(app,'main.js'),"win.loadFile('dist/index.html');");
  await writeFile(path.join(app,'dist/index.html'),'<link rel="stylesheet" href="style.css"><link href="css/second.css" rel="stylesheet">');
  await writeFile(path.join(app,'dist/style.css'),'body { color: red; }');
  await writeFile(path.join(app,'dist/css/second.css'),'p { color: blue; }');
  const {config,report}=await importElectron(app,{out});
  assert.equal(config.css,'src/renderer/dist/style.css');
  assert.equal(await readFile(path.join(out,config.css),'utf8'),'body { color: red; }');
  assert.equal(await readFile(path.join(out,'src/renderer/dist/css/second.css'),'utf8'),'p { color: blue; }');
  assert.ok(report.includes('src/renderer/dist/style.css')); assert.ok(report.includes('src/renderer/dist/css/second.css'));
});

test('uses an explained empty placeholder when no stylesheet is linked', async t => {
  const tmp=await mkdtemp(path.join(os.tmpdir(),'tsgpui-css-')); t.after(()=>rm(tmp,{recursive:true,force:true}));
  const app=path.join(tmp,'app'), out=path.join(tmp,'out');
  await mkdir(path.join(app,'dist'),{recursive:true});
  await writeFile(path.join(app,'package.json'),JSON.stringify({main:'main.js'}));
  await writeFile(path.join(app,'main.js'),"win.loadFile('dist/index.html');");
  await writeFile(path.join(app,'dist/index.html'),'<html></html>');
  const {config,report}=await importElectron(app,{out});
  assert.equal(config.css,'src/import.css'); assert.equal(await readFile(path.join(out,config.css),'utf8'),'');
  assert.match(report,/empty placeholder because no linked local stylesheet was found/);
});

test('main-process modules get per-module __dirname and __filename', async t => {
  const tmp=await mkdtemp(path.join(os.tmpdir(),'tsgpui-electron-')); t.after(()=>rm(tmp,{recursive:true,force:true}));
  const app=path.join(tmp,'app'), output=path.join(tmp,'converted');
  await mkdir(path.join(app,'electron'),{recursive:true}); await mkdir(path.join(app,'dist'),{recursive:true});
  await writeFile(path.join(app,'package.json'),JSON.stringify({name:'dirname-app',main:'electron/main.js'}));
  await writeFile(path.join(app,'electron','main.js'),"const path = require('path');\nconst { BrowserWindow } = require('electron');\nconst preload = path.join(__dirname, 'preload.js');\nconst self = __filename;\nconst util = require('./util');\nnew BrowserWindow({ webPreferences: { preload } });\n");
  await writeFile(path.join(app,'electron','util.js'),"module.exports = { value: 1 };\n");
  await writeFile(path.join(app,'dist','index.html'),'<html><head></head><body>Hi</body></html>');
  const result=spawnSync(process.execPath,[cli,'import','electron',app,'--out',output],{encoding:'utf8'});
  assert.equal(result.status,0,result.stderr);
  const main=await readFile(path.join(output,'src/main/electron/main.ts'),'utf8');
  assert.match(main,/const __dirname = "\/electron";/);
  assert.match(main,/const __filename = "\/electron\/main\.js";/);
  const util=await readFile(path.join(output,'src/main/electron/util.ts'),'utf8');
  assert.doesNotMatch(util,/__dirname|__filename/);
});
