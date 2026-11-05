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

const cli=fileURLToPath(new URL('../bin/tsgpui.mjs',import.meta.url));
test('each statically identified window page receives its own preload before page scripts', async t => {
  const tmp=await mkdtemp(path.join(os.tmpdir(),'tsgpui-preloads-')); t.after(()=>rm(tmp,{recursive:true,force:true}));
  const app=path.join(tmp,'app'), out=path.join(tmp,'out');
  await mkdir(path.join(app,'dist'),{recursive:true});
  await writeFile(path.join(app,'package.json'),JSON.stringify({name:'pages',version:'1.0.0',main:'main.js'}));
  await writeFile(path.join(app,'main.js'),`const {BrowserWindow} = require('electron'); const path = require('path'); const a = new BrowserWindow({webPreferences:{preload:path.join(__dirname,'one.js')}}); a.loadFile('dist/one.html'); const b = new BrowserWindow({webPreferences:{preload:path.join(__dirname,'two.js')}}); b.loadFile('dist/two.html');`);
  await writeFile(path.join(app,'one.js'),`globalThis.preloadIdentity = 'one';`);
  await writeFile(path.join(app,'two.js'),`globalThis.preloadIdentity = 'two';`);
  await writeFile(path.join(app,'dist/one.html'),'<html><body><script>window.pageScript = true;</script></body></html>');
  await writeFile(path.join(app,'dist/two.html'),'<html><body><script>window.pageScript = true;</script></body></html>');
  await importElectron(app,{out});
  const {bundle}=await buildPackage(out);
  const pages = [bundle.webview_html, Buffer.from(bundle.webview_files['/src/renderer/dist/two.html'].data,'base64').toString()];
  for (let i=0; i<pages.length; i++) {
    const match=/<script\b[^>]*data-tsgpui-preload[^>]*>([\s\S]*?)<\/script>/i.exec(pages[i]);
    assert.ok(match,`page ${i} has a preload`);
    assert.ok(match.index < pages[i].indexOf('window.pageScript'));
    const context=vm.createContext({}); vm.runInContext(match[1],context);
    assert.equal(context.preloadIdentity,i ? 'two' : 'one');
  }
});
test('reports instead of applying the wrong preload when windows share a page', async t => {
  const tmp=await mkdtemp(path.join(os.tmpdir(),'tsgpui-shared-page-')); t.after(()=>rm(tmp,{recursive:true,force:true}));
  const app=path.join(tmp,'app'), out=path.join(tmp,'out');
  await mkdir(path.join(app,'dist'),{recursive:true});
  await writeFile(path.join(app,'package.json'),JSON.stringify({name:'shared',version:'1.0.0',main:'main.js'}));
  await writeFile(path.join(app,'main.js'),`const {BrowserWindow}=require('electron'); const a=new BrowserWindow({webPreferences:{preload:'one.js'}}); a.loadFile('dist/index.html'); const b=new BrowserWindow(); b.loadFile('dist/index.html');`);
  await writeFile(path.join(app,'one.js'),`globalThis.preloadIdentity = 'one';`);
  await writeFile(path.join(app,'dist/index.html'),'<html><body>Hello</body></html>');
  const {report}=await importElectron(app,{out});
  assert.match(report,/multiple preload scripts; per-window association requires review/);
  const {bundle}=await buildPackage(out);
  assert.doesNotMatch(bundle.webview_html,/data-tsgpui-preload/);
});
test('packages multiple window pages and their assets with source-relative paths', async t => {
  const tmp=await mkdtemp(path.join(os.tmpdir(),'tsgpui-pages-')); t.after(()=>rm(tmp,{recursive:true,force:true}));
  const app=path.join(tmp,'app'), out=path.join(tmp,'out');
  await mkdir(path.join(app,'first'),{recursive:true}); await mkdir(path.join(app,'second'),{recursive:true});
  await writeFile(path.join(app,'package.json'),JSON.stringify({name:'pages',version:'1.0.0',main:'main.js'}));
  await writeFile(path.join(app,'main.js'),`const {BrowserWindow} = require('electron'); const a = new BrowserWindow(); a.loadFile('first/index.html'); const b = new BrowserWindow(); b.loadFile('second/index.html');`);
  await writeFile(path.join(app,'first/index.html'),'<html><body>First</body></html>');
  await writeFile(path.join(app,'second/index.html'),'<html><head><link rel="stylesheet" href="page.css"></head><body>Second<script src="page.js"></script></body></html>');
  await writeFile(path.join(app,'second/page.css'),'body { color: blue }');
  await writeFile(path.join(app,'second/page.js'),'globalThis.secondPage = true;');
  const {config}=await importElectron(app,{out});
  assert.equal(config.html,'src/renderer/first/index.html');
  assert.deepEqual(config.webviewPages,['src/renderer/second/index.html']);
  const {bundle}=await buildPackage(out);
  assert.equal(bundle.webview_entry,'/src/renderer/first/index.html');
  for (const file of ['second/index.html','second/page.css','second/page.js']) assert.ok(bundle.webview_files['/src/renderer/'+file],file);
  const second=Buffer.from(bundle.webview_files['/src/renderer/second/index.html'].data,'base64').toString();
  assert.match(second,/Second/);
  assert.match(second,/Content-Security-Policy/);
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
  assert.equal((main.match(/tsgpui-runtime\/electron/g) || []).length, 3);
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
  await assert.rejects(readFile(path.join(tmp,'tsgpui.config.json')),{code:'ENOENT'});
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
  const config=JSON.parse(await readFile(path.join(output,'tsgpui.config.json'),'utf8'));
  assert.equal(config.renderer,'webview'); assert.equal(config.name,'sample-app'); assert.equal(config.version,'2.3.4'); assert.equal(config.title,'Hello');
  assert.deepEqual(config.window,{width:800,height:650});
  assert.deepEqual(config.permissions.filesystem,{read:['./data'],write:['./data']});
  assert.deepEqual(config.permissions.processes,['git']); assert.deepEqual(config.permissions.network,['api.example.com','example.com']); assert.equal(config.permissions.ipc,true);
  const main=await readFile(path.join(output,'src/main/main.ts'),'utf8');
  assert.match(main,/import \* as __tsgpui_import_0 from "\.\.\/tsgpui-runtime\/electron"/);
  assert.doesNotMatch(main,/require\(/);
  assert.match(main,/tsgpui-runtime\/https/);
  const report=await readFile(path.join(output,'TSGPUI_IMPORT_REPORT.md'),'utf8');
  assert.match(report,/main\.js:4: Unsupported module unsupported-package/); assert.match(report,/node:fs/); assert.match(report,/tsgpui build/);
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
  assert.equal(result.bundle.renderer,'webview');
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
