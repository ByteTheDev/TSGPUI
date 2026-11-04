import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import vm from 'node:vm';
import ts from 'typescript';
import { fileURLToPath } from 'node:url';
const dir = fileURLToPath(new URL('../src/electron-compat/runtime/', import.meta.url));
function runtime(page = false) {
  const calls = [], subscriptions = new Map(), files = new Map();
  const info = {name:'Example', version:'1.2', platform:'linux', paths:{home:'/home', temp:'/tmp', appFolder:'/app', userData:'/data'}, roots:[{name:'userData',path:'/data',access:'write'}]};
  const host = async (op, args = {}) => { calls.push([op,args]); switch(op) { case 'app.info': return info; case 'filesystem.writeText': files.set(args.path,args.text); return {}; case 'filesystem.readText': return {text:files.get(args.path)}; case 'filesystem.writeBytes': files.set(args.path,args.base64); return {}; case 'filesystem.readBytes': return {base64:files.get(args.path)}; case 'filesystem.list': return {entries:['a']}; case 'filesystem.stat': return {type:'file',size:3}; case 'process.spawn': return {status:0,stdout:'ok',stderr:''}; case 'dialog.openFile': return {canceled:false,paths:['/data/a']}; case 'dialog.saveFile': return {canceled:false,path:'/data/a'}; case 'dialog.message': return {response:1}; case 'clipboard.readText': return {text:'clip'}; case 'ipc.invoke': return context.__tsgpui_ipc_dispatch('invoke',args.channel,args.payload); case 'ipc.send': return context.__tsgpui_ipc_dispatch('send',args.channel,args.payload); default: return {}; } };
  const context = vm.createContext({console, Uint8Array, Promise, Map, setTimeout, __tsgpui_host:host});
  context.__tsgpui_emit = (channel,payload) => { for (const fn of subscriptions.get(channel) || []) fn(payload); };
  if (page) context.window = {tsgpui:{invoke:host,on(channel,fn) { const a = subscriptions.get(channel) || []; a.push(fn); subscriptions.set(channel,a); return () => { const i = a.indexOf(fn); if (i >= 0) a.splice(i,1); }; }}, matchMedia:() => ({matches:true})};
  const cache = new Map();
  function load(name) { name = name.replace(/^\.\//,'').replace(/\.ts$/,''); if(cache.has(name)) return cache.get(name).exports; const mod = {exports:{}}; cache.set(name,mod); const source = fs.readFileSync(path.join(dir,name+'.ts'),'utf8'); const js = ts.transpileModule(source,{compilerOptions:{module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2020}}).outputText; const fn = vm.runInContext(`(function(require,module,exports){${js}\n})`,context); fn(load,mod,mod.exports); return mod.exports; }
  return {load,calls,context,files};
}
test('path and events shims', () => { const {load} = runtime(); const p = load('path'); assert.equal(p.join('/a','../b'),'/b'); assert.equal(p.relative('/a/b','/a/c'),'../c'); assert.equal(p.extname('/a/.profile'),''); assert.equal(p.format(p.parse('/a/x.txt')),'/a/x.txt'); const {EventEmitter} = load('events'); const e = new EventEmitter(); let n = 0; const f = () => n++; e.once('x',f); e.emit('x'); e.emit('x'); assert.equal(n,1); e.on('x',f); e.off('x',f); assert.equal(e.listenerCount('x'),0); });
test('filesystem maps roots, bytes and permissions', async () => { const {load,calls} = runtime(); const f = load('fs'); await f.promises.writeFile('/data/a','hello'); assert.equal(await f.promises.readFile('/data/a','utf8'),'hello'); await f.promises.writeFile('/data/b',new Uint8Array([0,255,42])); assert.deepEqual(Array.from(await f.promises.readFile('/data/b')),[0,255,42]); await assert.rejects(f.promises.readFile('/elsewhere/a'), {code:'EACCES'}); assert.throws(() => f.readFileSync('/data/a'), {code:'ENOTSUP'}); await f.promises.rename('/data/a','/data/c'); assert.ok(calls.some(([op,a]) => op === 'filesystem.rename' && a.from === 'a' && a.to === 'c')); assert.equal((await f.promises.stat('/data/a')).isFile(),true); });
test('app, IPC, dialogs, windows and warnings', async () => { const {load,context,calls} = runtime(true); const e = load('electron'); const h = load('host'); await e.app.whenReady(); assert.equal(e.app.getName(),'Example'); assert.equal(e.app.getPath('userData'),'/data'); e.ipcMain.handleOnce('sum',(_event,n) => n+1); assert.equal(await e.ipcRenderer.invoke('sum',2),3); await assert.rejects(e.ipcRenderer.invoke('sum',2),/No handler/); let received; e.ipcRenderer.once('notice',(_event,p) => received=p); const w = new e.BrowserWindow({width:400}); w.webContents.send('notice',42); assert.equal(received,42); assert.throws(() => new e.BrowserWindow(),/multiple windows/); await w.loadFile('index.html'); w.setTitle('Title'); assert.equal((await e.dialog.showOpenDialog({properties:['multiSelections']})).filePaths[0],'/data/a'); assert.equal((await e.dialog.showSaveDialog({})).filePath,'/data/a'); assert.equal((await e.dialog.showMessageBox({message:'hello'})).response,1); assert.equal(await e.clipboard.readText(),'clip'); await e.clipboard.writeText('x'); await e.shell.openExternal('https://example.com'); new e.Notification({title:'T',body:'B'}).show(); e.Menu.setApplicationMenu(null); new e.Tray('x'); e.globalShortcut.register('a',()=>{}); assert.equal(h.compatWarnings.length,3); assert.equal(e.nativeTheme.shouldUseDarkColors,true); e.contextBridge.exposeInMainWorld('api',{x:1}); assert.equal(context.window.api.x,1); w.close(); assert.equal(w.isDestroyed(),true); assert.ok(calls.some(([op]) => op === 'window.close')); });
test('os, process and child process', async () => { const {load} = runtime(); const os = load('os'); await os.infoReady; assert.equal(os.platform(),'linux'); assert.equal(os.homedir(),'/home'); const proc = load('process'); assert.equal(proc.default.cwd(),'/app'); assert.throws(() => proc.exit(),/not supported/); let tick = false; proc.nextTick(() => tick = true); await Promise.resolve(); assert.equal(tick,true); const cp = load('child_process'); const result = await new Promise((resolve,reject) => cp.execFile('allowed',['arg'],(err,out) => err ? reject(err) : resolve(out))); assert.equal(result,'ok'); });
test('https forwards requests and emits responses and errors', async () => {
  const {load,context,calls} = runtime();
  context.__tsgpui_host = async (op,args) => { calls.push([op,args]); if (args.url.includes('bad')) throw new Error('denied'); return {status:201,headers:{'content-type':'text/plain'},body:'hello'}; };
  const https = load('https');
  const result = await new Promise((resolve,reject) => { const req = https.request({hostname:'example.com',path:'/x',method:'POST',headers:{x:'y'}}, res => { let body=''; res.on('data', chunk => body+=chunk); res.on('end', () => resolve([res.statusCode,res.headers['content-type'],body])); }); req.on('error',reject); req.write('abc'); req.end('d'); });
  assert.deepEqual(result,[201,'text/plain','hello']);
  assert.equal(calls.at(-1)[0],'net.request'); assert.equal(calls.at(-1)[1].url,'https://example.com/x'); assert.equal(calls.at(-1)[1].body,'abcd');
  await new Promise(resolve => https.get('https://bad.example/', () => assert.fail('unexpected response')).on('error', error => { assert.match(error.message,/denied/); resolve(); }));
});
test('userData root maps app paths to filesystem operations', async () => {
  const {load,calls} = runtime(); const app=load('electron').app; await app.whenReady();
  const fs=load('fs'); await fs.promises.writeFile(app.getPath('userData')+'/settings.json','{}');
  const call=calls.find(([op]) => op==='filesystem.writeText'); assert.equal(call[1].root,'userData'); assert.equal(call[1].path,'settings.json');
  await assert.rejects(fs.promises.readFile('/outside/file'), {code:'EACCES'});
});

test('dialog roots map only the selected paths', async () => {
  const {load,context,calls} = runtime();
  const roots = [{name:'read-selection',path:'/picked/input.txt',access:'read',exact:true},{name:'write-selection',path:'/picked/output.txt',access:'write',exact:true}];
  context.__tsgpui_host = async (op,args) => { calls.push([op,args]); if (op==='app.info') return {paths:{},roots}; if (op==='filesystem.readText') return {text:'selected'}; return {}; };
  const fs=load('fs'); assert.equal(await fs.promises.readFile('/picked/input.txt','utf8'),'selected');
  await fs.promises.writeFile('/picked/output.txt','saved');
  assert.equal(calls.find(([op]) => op==='filesystem.writeText')[1].root,'write-selection');
  await assert.rejects(fs.promises.readFile('/picked/input.txt/child'),{code:'EACCES'});
  await assert.rejects(fs.promises.writeFile('/picked/input.txt','bad'),{code:'EACCES'});
  await assert.rejects(fs.promises.readFile('/picked/other.txt'),{code:'EACCES'});
});

test('standard paths use only host-provided values', async () => {
  const {load,context} = runtime();
  const names = ['home','appData','userData','temp','desktop','documents','downloads','music','pictures','videos','logs','exe','module','sessionData'];
  const paths = Object.fromEntries(names.map(name => [name, '/host/' + name]));
  context.__tsgpui_host = async op => op === 'app.info' ? {paths} : {};
  const app = load('electron').app; await app.whenReady();
  for (const name of names) assert.equal(app.getPath(name), paths[name]);
  delete paths.music;
  assert.throws(() => app.getPath('music'), /path not provided by host/);
  assert.throws(() => app.getPath('toString'), /not supported/);
});
test('sync filesystem calls explain the asynchronous alternative', () => {
  const f = runtime().load('fs');
  for (const name of ['existsSync','statSync','readFileSync','writeFileSync','mkdirSync'])
    assert.throws(() => f[name]('/data/a'), error => error.code === 'ENOTSUP' && /fs\.promises/.test(error.message));
});
test('spawn emits collected output before closing', async () => {
  const {load,context,calls} = runtime();
  context.__tsgpui_host = async (op,args) => { calls.push([op,args]); return {status:7,stdout:'out',stderr:'err'}; };
  const child = load('child_process').spawn('command',['arg']);
  const events = [];
  child.stdout.on('data', data => events.push(['stdout',data]));
  child.stderr.on('data', data => events.push(['stderr',data]));
  await new Promise(resolve => child.on('close', code => { events.push(['close',code]); resolve(); }));
  assert.deepEqual(events,[['stdout','out'],['stderr','err'],['close',7]]);
  assert.equal(calls[0][0], 'process.spawn'); assert.equal(calls[0][1].command,'command');
});
test('window limitation and unsupported UI warnings are explicit', () => {
  const {load} = runtime(); const e = load('electron'), warnings = load('host').compatWarnings;
  const w = new e.BrowserWindow(); assert.equal(e.BrowserWindow.getAllWindows()[0],w);
  assert.throws(() => new e.BrowserWindow(), /one live window/);
  e.Menu.buildFromTemplate([]); e.Menu.setApplicationMenu(null);
  const tray = new e.Tray(); tray.setToolTip('x'); tray.setContextMenu(null); tray.on('click',()=>{}); tray.destroy();
  e.globalShortcut.register('x',()=>{}); e.globalShortcut.unregister('x'); e.globalShortcut.unregisterAll(); assert.equal(e.globalShortcut.isRegistered('x'),false);
  for (const name of ['Menu.buildFromTemplate','Menu.setApplicationMenu','Tray','Tray.setToolTip','Tray.setContextMenu','Tray.on','Tray.destroy','globalShortcut.register','globalShortcut.unregister','globalShortcut.unregisterAll','globalShortcut.isRegistered']) assert.ok(warnings.some(w => w.startsWith(name + ' ')),name);
  w.close(); assert.equal(e.BrowserWindow.getAllWindows().length,0);
});
