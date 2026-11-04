import {test} from 'node:test';
import assert from 'node:assert/strict';
import {mkdtemp, mkdir, writeFile, readFile, rm} from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import {spawnSync} from 'node:child_process';
import {fileURLToPath} from 'node:url';
import {importElectron} from '../src/import-electron.mjs';
import {buildPackage} from '../src/compiler.mjs';

const cli=fileURLToPath(new URL('../bin/tsgpui.mjs',import.meta.url));
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
  assert.equal(config.css,'src/renderer/style.css');
  assert.equal(await readFile(path.join(out,config.css),'utf8'),'body { color: red; }');
  assert.equal(await readFile(path.join(out,'src/renderer/css/second.css'),'utf8'),'p { color: blue; }');
  assert.ok(report.includes('src/renderer/style.css')); assert.ok(report.includes('src/renderer/css/second.css'));
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
