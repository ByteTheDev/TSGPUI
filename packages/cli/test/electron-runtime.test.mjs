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
test('unsupported process event registration fails explicitly', () => {
  const process = runtime().load('process');
  assert.throws(() => process.on('exit', () => {}), /process\.on\("exit"\) is not supported/);
});
test('process exposes host architecture after initialization', async () => {
  const {load, context} = runtime();
  context.__tsgpui_host = async () => ({platform:'linux', arch:'x64', paths:{}});
  const process = load('process');
  await load('os').infoReady;
  await Promise.resolve();
  assert.equal(process.arch, 'x64');
  assert.equal(process.default.arch, 'x64');
});
test('node shims copy bytes and reject unsupported process operations', async () => {
  const {load} = runtime();
  const fs = load('fs');
  await fs.promises.writeFile('/data/source', new Uint8Array([0, 128, 255]));
  await fs.promises.copyFile('/data/source', '/data/copy');
  assert.deepEqual(Array.from(await fs.promises.readFile('/data/copy')), [0, 128, 255]);
  await assert.rejects(fs.promises.copyFile('/data/source', '/data/copy', 1), /fs.copyFile/);
  assert.throws(() => fs.existsSync('/data/source'), /fs.existsSync/);
  const child = load('child_process');
  assert.throws(() => child.exec('echo hello'), /child_process.exec/);
  assert.throws(() => child.spawn('echo', [], {cwd:'/tmp'}), /child_process.spawn/);
});
test('https accepts URL plus request options and rejects binary bodies', async () => {
  const {load, context, calls} = runtime();
  context.__tsgpui_host = async (op, args) => { calls.push([op, args]); return {status:200, headers:{}, body:'ok'}; };
  const https = load('https');
  await new Promise((resolve, reject) => {
    const req = https.request('https://example.com/', {method:'POST', headers:{x:'y'}}, res => res.on('end', resolve));
    req.on('error', reject);
    assert.throws(() => req.write(new Uint8Array([1])), /https.request.write/);
    req.end('hello');
  });
  assert.equal(calls.at(-1)[1].method, 'POST');
  assert.equal(calls.at(-1)[1].body, 'hello');
});
test('IPC dispatch replies to the originating secondary window', async () => {
  const {load,context,calls} = runtime(); const e = load('electron');
  e.ipcMain.handle('ping', (event, value) => { event.sender.send('notice', value); return value + 1; });
  assert.equal(await context.__tsgpui_ipc_dispatch('invoke','ping',41,2),42);
  const sent = calls.find(([op]) => op === 'window.send');
  assert.equal(sent[1].windowId,2); assert.equal(sent[1].channel,'notice'); assert.equal(sent[1].payload,41);
});
test('failed secondary window creation does not leave a phantom window', async () => {
  const {load,context} = runtime(); const e = load('electron');
  context.__tsgpui_host = async (op) => { if (op === 'window.create') throw new Error('creation failed'); return {}; };
  const first = new e.BrowserWindow(); const second = new e.BrowserWindow();
  await assert.rejects(second.loadFile('dist/dialog.html'), /creation failed/);
  await Promise.resolve(); assert.equal(second.isDestroyed(), true); assert.deepEqual(Array.from(e.BrowserWindow.getAllWindows()), [first]);
});
test('secondary loadFile maps project-relative and virtual absolute paths without escaping assets', async () => {
  const {load,calls} = runtime(); const e = load('electron');
  new e.BrowserWindow(); const child = new e.BrowserWindow();
  await child.loadFile('/electron/dialog/index.html');
  assert.equal(calls.find(([op]) => op === 'window.load')[1].path, '/src/renderer/electron/dialog/index.html');
  await child.loadFile('dist/another page.html');
  assert.equal(calls.filter(([op]) => op === 'window.load').at(-1)[1].path, '/src/renderer/dist/another%20page.html');
  await assert.rejects(child.loadFile('../escape.html'), /project-relative/);
  await assert.rejects(child.loadFile('C:\\private.html'), /project-relative/);
  await assert.rejects(child.loadFile('//server/share.html'), /project-relative/);
});
test('menu templates create inspectable nested items and reject invalid operations', () => {
  const e = runtime().load('electron');
  const menu = e.Menu.buildFromTemplate([{label:'File', submenu:[{id:'open', label:'Open', enabled:false}, {type:'separator'}]}]);
  assert.equal(menu.items.length,1); assert.equal(menu.items[0].type,'submenu');
  const open = menu.getMenuItemById('open'); assert.equal(open.label,'Open'); assert.equal(open.enabled,false);
  open.enabled = true; assert.equal(menu.items[0].submenu.items[0].enabled,true);
  const item = new e.MenuItem({id:'quit',label:'Quit'}); menu.insert(0,item);
  assert.equal(menu.items[0],item); assert.equal(item.menu,menu);
  assert.throws(() => menu.append(item), /already belongs/);
  assert.throws(() => new e.MenuItem({type:'invalid'}), /Invalid menu item type/);
  e.Menu.setApplicationMenu(menu); assert.equal(e.Menu.getApplicationMenu(),menu);
  assert.throws(() => menu.popup(), /native menus are not available/);
  e.Menu.setApplicationMenu(null); assert.equal(e.Menu.getApplicationMenu(),null);
});
test('nativeImage preserves PNG bytes and reports dimensions, while unsupported conversions fail', async () => {
  const image = runtime().load('electron').nativeImage;
  const base64 = 'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+/aXkAAAAASUVORK5CYII=';
  const value = image.createFromDataURL('data:image/png;base64,' + base64);
  assert.equal(value.isEmpty(),false); assert.equal(value.getSize().width,1); assert.equal(value.getSize().height,1);
  assert.equal(value.toDataURL(),'data:image/png;base64,' + base64);
  const copy = value.toPNG(); copy[0] = 0; assert.equal(value.toPNG()[0],137);
  assert.equal(image.createFromBuffer(new Uint8Array([1,2,3])).isEmpty(),true);
  assert.equal(image.createEmpty().getSize().width,0);
  assert.equal(value.resize({width:2,height:2}).getSize().width, 2);
  assert.equal((await image.createFromPath('x.png')).isEmpty(), true);
});
test('nativeTheme tracks effective theme and media changes', () => {
  const {load,context} = runtime(true);
  const queries = new Map();
  context.window.matchMedia = name => { const query = {matches:false, addEventListener(_event, fn) { this.update = fn; }}; queries.set(name, query); return query; };
  const theme = load('electron').nativeTheme;
  let updates = 0; theme.on('updated', () => updates++);
  assert.equal(theme.shouldUseDarkColors,false);
  const dark = queries.get('(prefers-color-scheme: dark)'); dark.matches = true; dark.update();
  assert.equal(theme.shouldUseDarkColors,true); assert.equal(updates,1);
  theme.themeSource = 'light'; assert.equal(theme.shouldUseDarkColors,false); assert.equal(updates,2);
  dark.matches = false; dark.update(); assert.equal(updates,2);
  const contrast = queries.get('(forced-colors: active)'); contrast.matches = true; contrast.update();
  assert.equal(theme.inForcedColorsMode,true); assert.equal(theme.shouldUseHighContrastColors,true); assert.equal(updates,3);
  assert.throws(() => { theme.themeSource = 'nope'; }, /themeSource/);
});
test('filesystem maps roots, bytes and permissions', async () => { const {load,calls} = runtime(); const f = load('fs'); await f.promises.writeFile('/data/a','hello'); assert.equal(await f.promises.readFile('/data/a','utf8'),'hello'); await f.promises.writeFile('/data/b',new Uint8Array([0,255,42])); assert.deepEqual(Array.from(await f.promises.readFile('/data/b')),[0,255,42]); await assert.rejects(f.promises.readFile('/elsewhere/a'), {code:'EACCES'}); assert.throws(() => f.readFileSync('/data/a'), {code:'ENOTSUP'}); await f.promises.rename('/data/a','/data/c'); assert.ok(calls.some(([op,a]) => op === 'filesystem.rename' && a.from === 'a' && a.to === 'c')); assert.equal((await f.promises.stat('/data/a')).isFile(),true); });
test('app, IPC, dialogs, windows and warnings', async () => { const {load,context,calls} = runtime(true); const e = load('electron'); const h = load('host'); await e.app.whenReady(); assert.equal(e.app.getName(),'Example'); assert.equal(e.app.getPath('userData'),'/data'); e.ipcMain.handleOnce('sum',(_event,n) => n+1); assert.equal(await e.ipcRenderer.invoke('sum',2),3); await assert.rejects(e.ipcRenderer.invoke('sum',2),/No handler/); let received; e.ipcRenderer.once('notice',(_event,p) => received=p); const w = new e.BrowserWindow({width:400}); w.webContents.send('notice',42); assert.equal(received,42); const second = new e.BrowserWindow(); assert.equal(e.BrowserWindow.getAllWindows().length,2); second.close(); await w.loadFile('index.html'); w.setTitle('Title'); assert.equal((await e.dialog.showOpenDialog({properties:['multiSelections']})).filePaths[0],'/data/a'); assert.equal((await e.dialog.showSaveDialog({})).filePath,'/data/a'); assert.equal((await e.dialog.showMessageBox({message:'hello'})).response,1); assert.equal(await e.clipboard.readText(),'clip'); await e.clipboard.writeText('x'); await e.shell.openExternal('https://example.com'); new e.Notification({title:'T',body:'B'}).show(); e.Menu.setApplicationMenu(null); assert.equal(new e.Tray('x').isDestroyed(), false); assert.equal(e.globalShortcut.register('a',()=>{}), true); assert.equal(h.compatWarnings.length,1); assert.equal(e.nativeTheme.shouldUseDarkColors,true); e.contextBridge.exposeInMainWorld('api',{x:1}); assert.equal(context.window.api.x,1); w.close(); assert.equal(w.isDestroyed(),true); assert.ok(calls.some(([op]) => op === 'window.close')); });
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
test('electron net.request sends text and headers through the permission-checked host', async () => {
  const {load,context,calls} = runtime();
  context.__tsgpui_host = async (op,args) => { calls.push([op,args]); return {status:201,headers:{'x-result':'ok'},body:'reply'}; };
  const e = load('electron');
  const req = e.net.request({url:'https://example.com/resource',method:'POST'});
  req.setHeader('X-Token','one'); req.setHeader('x-token','two');
  assert.equal(req.getHeader('X-TOKEN'),'two');
  const received = new Promise((resolve,reject) => { req.on('response', res => { let body=''; res.on('data', chunk => body+=chunk); res.on('end', () => resolve([res.statusCode,body])); }); req.on('error',reject); });
  req.write('hello'); req.end(' world'); assert.deepEqual(await received,[201,'reply']);
  const [op, args] = calls.find(([name]) => name === 'net.request');
  assert.equal(op,'net.request'); assert.equal(args.url,'https://example.com/resource'); assert.equal(args.body,'hello world'); assert.equal(args.headers['x-token'],'two');
  assert.throws(() => e.net.request({url:'https://example.com',session:{}}), /session/);
  assert.doesNotThrow(() => e.net.request('https://example.com').write(new Uint8Array([1])));
  const fetched = await e.net.fetch('https://example.com/resource');
  assert.equal(fetched.status, 201);
  assert.equal(await fetched.text(), 'reply');
  assert.equal(fetched.headers.get('x-result'), 'ok');
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
test('window targets and unsupported UI warnings are explicit', async () => {
  const {load} = runtime(); const e = load('electron'), warnings = load('host').compatWarnings;
  const w = new e.BrowserWindow(); assert.equal(e.BrowserWindow.getAllWindows()[0],w);
  const second = new e.BrowserWindow({title:'Second'}); await second.loadFile('second.html'); second.setTitle('Different'); await Promise.resolve(); assert.equal(e.BrowserWindow.getAllWindows().length,2); second.close(); await Promise.resolve();
  e.Menu.buildFromTemplate([]); e.Menu.setApplicationMenu(null);
  assert.equal(new e.Tray().isDestroyed(), false);
  assert.equal(e.globalShortcut.isRegistered('x'), false); e.globalShortcut.register('x',()=>{}); assert.equal(e.globalShortcut.isRegistered('x'), true); e.globalShortcut.unregister('x'); assert.equal(e.globalShortcut.isRegistered('x'), false);
  w.close(); assert.equal(e.BrowserWindow.getAllWindows().length,0);
});

const flush = () => new Promise(resolve => setTimeout(resolve, 0));
test('a user close request reaches close listeners, which can prevent it', async () => {
  const {load,context,calls} = runtime(); const e = load('electron');
  new e.BrowserWindow(); const child = new e.BrowserWindow();
  let prevent = true; child.on('close', event => { if (prevent) event.preventDefault(); });
  await flush();
  assert.ok(calls.some(([op,args]) => op === 'window.watchClose' && args.windowId === child.id));
  context.__tsgpui_window_event(child.id, 'close-requested'); await flush();
  assert.equal(child.isDestroyed(), false);
  assert.equal(calls.some(([op]) => op === 'window.close'), false);
  prevent = false; context.__tsgpui_window_event(child.id, 'close-requested'); await flush();
  assert.equal(child.isDestroyed(), true);
  assert.ok(calls.some(([op,args]) => op === 'window.close' && args.windowId === child.id));
});
test('webContents exposes navigation and devtools stubs', async () => {
  const {load} = runtime(true); const e = load('electron');
  const first = new e.BrowserWindow();
  for (const name of ['goBack', 'goForward', 'canGoBack', 'canGoForward', 'reload', 'reloadIgnoringCache', 'openDevTools', 'closeDevTools', 'isDevToolsOpened']) {
    assert.doesNotThrow(() => first.webContents[name]());
  }
  assert.equal(first.webContents.canGoBack(), false);
  assert.equal(first.webContents.isDevToolsOpened(), false);
  await assert.rejects(first.webContents.loadURL('https://example.com'), /webContents.loadURL/);
  const second = new e.BrowserWindow();
  await second.webContents.loadFile('second.html');
});

test('native lifecycle events reach BrowserWindow listeners', () => {
  const {load,context} = runtime(); const e = load('electron');
  const w = new e.BrowserWindow(); const seen = [];
  const names = ['focus','blur','move','resize','minimize','maximize','restore'];
  for (const name of names) w.on(name, () => seen.push(name));
  for (const name of names) context.__tsgpui_window_event(w.id, name);
  assert.deepEqual(seen, names);
  w.close(); context.__tsgpui_window_event(w.id, 'focus'); assert.deepEqual(seen, names);
});
test('closing the last window quits unless the app handles window-all-closed', async () => {
  const quits = runtime(); const first = quits.load('electron');
  new first.BrowserWindow().close(); await flush();
  assert.ok(quits.calls.some(([op]) => op === 'app.quit'));
  const handled = runtime(); const second = handled.load('electron');
  let closedAll = 0; second.app.on('window-all-closed', () => closedAll++);
  new second.BrowserWindow().close(); await flush();
  assert.equal(closedAll, 1); assert.equal(handled.calls.some(([op]) => op === 'app.quit'), false);
});
test('parent and modal options are validated and sent to the host', async () => {
  const {load,calls} = runtime(); const e = load('electron');
  const main = new e.BrowserWindow(); const dialog = new e.BrowserWindow({parent: main, modal: true, title: 'Preferences'});
  await flush();
  const create = calls.find(([op,args]) => op === 'window.create' && args.windowId === dialog.id);
  assert.equal(create[1].parentWindowId, main.id); assert.equal(create[1].modal, true);
  assert.equal(dialog.getParentWindow(), main);
  assert.throws(() => new e.BrowserWindow({parent: {}}), /must be a BrowserWindow/);
  const warnings = load('host').compatWarnings;
  new e.BrowserWindow({modal: true}); assert.ok(warnings.some(w => /modal without a parent/.test(w)));
});
test('webContents exposes each window by id', () => {
  const {load} = runtime(); const e = load('electron');
  new e.BrowserWindow(); const child = new e.BrowserWindow();
  assert.equal(e.webContents.fromId(child.id), child.webContents);
  assert.equal(e.webContents.getAllWebContents().length, 2);
  assert.equal(e.webContents.fromId(99), null);
  assert.equal(child.webContents.id, child.id);
});

test('screen reports cached host displays with Electron-style lookups', async () => {
  const {load,context} = runtime();
  context.__tsgpui_host = async (op) => op === 'screen.displays' ? {displays:[{x:0,y:0,width:1920,height:1080,scaleFactor:1.5,primary:true},{x:1920,y:0,width:1280,height:720,scaleFactor:1,primary:false}]} : {};
  const e = load('electron');
  assert.throws(() => e.screen.getPrimaryDisplay(), /still loading/);
  await e.screen.refresh();
  const primary = e.screen.getPrimaryDisplay();
  assert.equal(primary.bounds.width, 1920); assert.equal(primary.scaleFactor, 1.5);
  assert.equal(e.screen.getAllDisplays().length, 2);
  assert.equal(e.screen.getDisplayNearestPoint({x:2500,y:100}).bounds.x, 1920);
  assert.equal(e.screen.getDisplayMatching({x:1800,y:0,width:400,height:100}).bounds.x, 1920);
  assert.equal(JSON.stringify(e.screen.getCursorScreenPoint()), JSON.stringify({x: 0, y: 0}));
});

test('webFrame zooms the page and injects and removes CSS', async () => {
  const {load,context} = runtime(true);
  const added = [];
  context.document = {
    documentElement: {style: {setProperty(name, value) { this[name] = value; }}},
    head: {appendChild(el) { added.push(el); }},
    createElement(tag) { return {tag, textContent: '', remove() { const i = added.indexOf(this); if (i >= 0) added.splice(i, 1); }}; }
  };
  const {webFrame} = load('electron');
  webFrame.setZoomFactor(1.5); assert.equal(webFrame.getZoomFactor(), 1.5); assert.equal(context.document.documentElement.style.zoom, '1.5');
  webFrame.setZoomLevel(1); assert.ok(Math.abs(webFrame.getZoomFactor() - 1.2) < 1e-9);
  const key = await webFrame.insertCSS('body{color:red}');
  assert.equal(added.length, 1); assert.equal(added[0].textContent, 'body{color:red}');
  await webFrame.removeInsertedCSS(key); assert.equal(added.length, 0);
  assert.equal(await webFrame.executeJavaScript('1+2'), 3);
  assert.throws(() => webFrame.setZoomFactor(0), /positive/);
});

test('MessageChannelMain delivers cloned messages between ports', async () => {
  const e = runtime().load('electron');
  const channel = new e.MessageChannelMain();
  const received = [];
  channel.port2.on('message', event => received.push(event.data));
  channel.port1.postMessage({n: 1});
  channel.port1.postMessage({n: 2});
  await new Promise(resolve => setTimeout(resolve, 0));
  assert.deepEqual(received, [{n: 1}, {n: 2}]);
  assert.throws(() => channel.port1.postMessage('x', [{}]), /does not transfer/);
  channel.port2.close();
  channel.port1.postMessage({n: 3});
  await new Promise(resolve => setTimeout(resolve, 0));
  assert.deepEqual(received, [{n: 1}, {n: 2}]);
});

test('powerSaveBlocker tracks blockers and sends the active set to the host', async () => {
  const {load,context,calls} = runtime();
  context.__tsgpui_host = async (op, args) => { calls.push([op, args]); return {}; };
  const e = load('electron');
  const id = e.powerSaveBlocker.start('prevent-display-sleep');
  assert.equal(e.powerSaveBlocker.isStarted(id), true);
  e.powerSaveBlocker.stop(id); assert.equal(e.powerSaveBlocker.isStarted(id), false);
  assert.throws(() => e.powerSaveBlocker.start('nope'), /Invalid power save blocker type/);
  await new Promise(resolve => setTimeout(resolve, 0));
  // Arrays from the VM realm differ in prototype, so compare serialized values.
  assert.equal(JSON.stringify(calls.filter(([op]) => op === 'power.setBlockers').map(([, a]) => a.types)), JSON.stringify([['prevent-display-sleep'], []]));
});

test('contextBridge copies and protects exposed values', () => { const {load,context} = runtime(); const bridge = load('electron').contextBridge; const source = { nested: { value: 1 } }; bridge.exposeInMainWorld('isolatedApi', source); source.nested.value = 2; assert.equal((context.window || context).isolatedApi.nested.value, 1); assert.ok(Object.isFrozen((context.window || context).isolatedApi.nested)); assert.throws(() => bridge.exposeInMainWorld('isolatedApi', {}), /already exists/); });

test('BaseWindow lifecycle and unsupported standalone views and utility processes', () => {
  const {load} = runtime(true); const e = load('electron');
  const window = new e.BaseWindow({title: 'Base'});
  assert.equal(window.isDestroyed(), false);
  assert.throws(() => window.contentView, /BaseWindow\.contentView/);
  assert.equal(new e.BrowserView().isDestroyed(), false);
  assert.equal(new e.WebContentsView().isDestroyed(), false);
  assert.doesNotThrow(() => e.utilityProcess.fork('child.js'));
  window.close(); assert.equal(window.isDestroyed(), true);
});

test('contextBridge copies values passed through exposed functions', async () => { const {load,context} = runtime(); const bridge = load('electron').contextBridge; const original = {value: 1}; let received; bridge.exposeInMainWorld('bridgeFunctions', { get: () => original, echo: value => { received = value; return value; }, asyncGet: async () => original }); const api = (context.window || context).bridgeFunctions; const result = api.get(); assert.notEqual(result, original); const input = {value: 2}; assert.notEqual(api.echo(input), input); assert.notEqual(received, input); assert.notEqual(await api.asyncGet(), original); });

test('ready-to-show fires after window creation for primary and secondary windows', async () => {
  const {load} = runtime(); const e = load('electron');
  const primary = new e.BrowserWindow();
  let primaryReady = 0; primary.on('ready-to-show', () => primaryReady++);
  const secondary = new e.BrowserWindow();
  let secondaryReady = 0; secondary.on('ready-to-show', () => secondaryReady++);
  await flush();
  assert.equal(primaryReady, 1);
  assert.equal(secondaryReady, 1);
});

test('ready-to-show fires after loadFile for primary and secondary windows', async () => {
  const {load} = runtime(); const e = load('electron');
  const primary = new e.BrowserWindow(); const secondary = new e.BrowserWindow();
  let primaryCount = 0, secondaryCount = 0;
  primary.on('ready-to-show', () => primaryCount++); secondary.on('ready-to-show', () => secondaryCount++);
  await flush();
  assert.equal(primaryCount, 1); assert.equal(secondaryCount, 1);
  primaryCount = 0; secondaryCount = 0;
  await primary.loadFile('index.html'); await secondary.loadFile('page.html'); await flush();
  assert.equal(primaryCount, 1); assert.equal(secondaryCount, 1);
});

test('show and hide emit events and route to the host for secondary windows only', async () => {
  const {load,calls} = runtime(); const e = load('electron');
  const primary = new e.BrowserWindow(); const secondary = new e.BrowserWindow({show: false});
  await flush();
  const primaryEvents = [];
  primary.on('show', () => primaryEvents.push('show')); primary.on('hide', () => primaryEvents.push('hide'));
  await primary.show(); await primary.hide(); await flush();
  assert.deepEqual(primaryEvents, []); assert.equal(primary.isVisible(), true);
  assert.equal(calls.some(([op]) => op === 'window.show' || op === 'window.hide'), false);
  const events = [];
  secondary.on('show', () => events.push('show')); secondary.on('hide', () => events.push('hide'));
  assert.equal(secondary.isVisible(), false);
  await secondary.show(); await flush();
  assert.deepEqual(events, ['show']); assert.equal(secondary.isVisible(), true);
  assert.ok(calls.some(([op,a]) => op === 'window.show' && a.windowId === secondary.id));
  events.length = 0;
  await secondary.hide(); await flush();
  assert.deepEqual(events, ['hide']); assert.equal(secondary.isVisible(), false);
  assert.ok(calls.some(([op,a]) => op === 'window.hide' && a.windowId === secondary.id));
  events.length = 0;
  const hideCallsBefore = calls.filter(([op]) => op === 'window.hide').length;
  await secondary.hide(); await flush();
  assert.deepEqual(events, []);
  assert.equal(calls.filter(([op]) => op === 'window.hide').length, hideCallsBefore + 1);
});

test('setFullScreen throws on the primary window and drives secondary windows', async () => {
  const {load,calls} = runtime(); const e = load('electron');
  const primary = new e.BrowserWindow();
  assert.throws(() => primary.setFullScreen(true), /fullscreen not supported/);
  assert.throws(() => primary.setFullScreen(false), /fullscreen not supported/);
  assert.equal(primary.isFullScreen(), false);
  const secondary = new e.BrowserWindow(); await flush();
  const events = [];
  secondary.on('enter-full-screen', () => events.push('enter')); secondary.on('leave-full-screen', () => events.push('leave'));
  secondary.setFullScreen(true); await flush();
  assert.equal(secondary.isFullScreen(), true); assert.deepEqual(events, ['enter']);
  assert.ok(calls.some(([op,a]) => op === 'window.setFullScreen' && a.windowId === secondary.id && a.fullScreen === true));
  secondary.setFullScreen(false); await flush();
  assert.equal(secondary.isFullScreen(), false); assert.deepEqual(events, ['enter','leave']);
  assert.ok(calls.some(([op,a]) => op === 'window.setFullScreen' && a.windowId === secondary.id && a.fullScreen === false));
});

test('setBounds emits will-move/move and will-resize/resize and getBounds reflects updates', async () => {
  const {load,calls} = runtime(); const e = load('electron');
  const primary = new e.BrowserWindow(); const secondary = new e.BrowserWindow({x:0,y:0,width:800,height:600});
  await flush();
  const events = [];
  for (const name of ['will-move','move','will-resize','resize']) secondary.on(name, () => events.push(name));
  secondary.setBounds({x:10,y:20,width:800,height:600});
  assert.deepEqual(events, ['will-move','move']);
  assert.deepEqual(JSON.parse(JSON.stringify(secondary.getBounds())), {x:10,y:20,width:800,height:600});
  await flush();
  assert.ok(calls.some(([op,a]) => op === 'window.setBounds' && a.windowId === secondary.id && a.x === 10 && a.y === 20));
  events.length = 0;
  secondary.setBounds({x:10,y:20,width:1000,height:700});
  assert.deepEqual(events, ['will-resize','resize']);
  assert.deepEqual(JSON.parse(JSON.stringify(secondary.getBounds())), {x:10,y:20,width:1000,height:700});
  events.length = 0;
  secondary.setBounds({x:30,y:40,width:900,height:800});
  assert.deepEqual(events, ['will-move','will-resize','move','resize']);
  assert.deepEqual(JSON.parse(JSON.stringify(secondary.getBounds())), {x:30,y:40,width:900,height:800});
  events.length = 0;
  secondary.setBounds({x:30,y:40,width:900,height:800});
  assert.deepEqual(events, []);
  const copy = secondary.getBounds(); copy.x = 999;
  assert.equal(secondary.getBounds().x, 30);
  await flush();
  const primaryEvents = [];
  for (const name of ['will-move','move','will-resize','resize']) primary.on(name, () => primaryEvents.push(name));
  const boundsCalls = calls.filter(([op]) => op === 'window.setBounds').length;
  primary.setBounds({x:5,y:6,width:500,height:400});
  assert.deepEqual(primaryEvents, ['will-move','will-resize','move','resize']);
  assert.deepEqual(JSON.parse(JSON.stringify(primary.getBounds())), {x:5,y:6,width:500,height:400});
  await flush();
  assert.equal(calls.filter(([op]) => op === 'window.setBounds').length, boundsCalls);
});

test('setPosition and setSize emit move and resize and route to the host for secondary windows', async () => {
  const {load,calls} = runtime(); const e = load('electron');
  const primary = new e.BrowserWindow(); const secondary = new e.BrowserWindow({x:0,y:0,width:800,height:600});
  await flush();
  const events = [];
  for (const name of ['will-move','move','will-resize','resize']) secondary.on(name, () => events.push(name));
  secondary.setPosition(100,200);
  assert.deepEqual(events, ['will-move','move']);
  assert.deepEqual(Array.from(secondary.getPosition()), [100,200]);
  assert.deepEqual(JSON.parse(JSON.stringify(secondary.getBounds())), {x:100,y:200,width:800,height:600});
  await flush();
  assert.ok(calls.some(([op,a]) => op === 'window.setPosition' && a.windowId === secondary.id && a.x === 100 && a.y === 200));
  events.length = 0;
  secondary.setPosition(100,200);
  assert.deepEqual(events, []);
  events.length = 0;
  secondary.setSize(1024,768);
  assert.deepEqual(events, ['will-resize','resize']);
  assert.deepEqual(JSON.parse(JSON.stringify(secondary.getSize())), {width:1024,height:768});
  assert.deepEqual(JSON.parse(JSON.stringify(secondary.getBounds())), {x:100,y:200,width:1024,height:768});
  await flush();
  assert.ok(calls.some(([op,a]) => op === 'window.setSize' && a.windowId === secondary.id && a.width === 1024 && a.height === 768));
  events.length = 0;
  secondary.setSize(1024,768);
  assert.deepEqual(events, []);
  const primaryEvents = [];
  for (const name of ['will-move','move','will-resize','resize']) primary.on(name, () => primaryEvents.push(name));
  const geometryCalls = calls.filter(([op]) => op === 'window.setPosition' || op === 'window.setSize').length;
  primary.setPosition(10,10);
  assert.deepEqual(primaryEvents, ['will-move','move']);
  primaryEvents.length = 0;
  primary.setSize(600,500);
  assert.deepEqual(primaryEvents, ['will-resize','resize']);
  await flush();
  assert.equal(calls.filter(([op]) => op === 'window.setPosition' || op === 'window.setSize').length, geometryCalls);
});

test('focus and blur emit events and getFocusedWindow tracks the focused window', async () => {
  const {load,calls} = runtime(); const e = load('electron');
  const primary = new e.BrowserWindow(); const secondary = new e.BrowserWindow();
  await flush();
  const events = [];
  for (const name of ['focus','blur']) secondary.on(name, () => events.push(name));
  secondary.focus(); await flush();
  assert.deepEqual(events, ['focus']); assert.equal(e.BrowserWindow.getFocusedWindow(), secondary);
  assert.ok(calls.some(([op,a]) => op === 'window.focus' && a.windowId === secondary.id));
  events.length = 0;
  secondary.focus(); await flush();
  assert.deepEqual(events, []);
  secondary.blur(); await flush();
  assert.deepEqual(events, ['blur']); assert.notEqual(e.BrowserWindow.getFocusedWindow(), secondary);
  assert.ok(calls.some(([op,a]) => op === 'window.blur' && a.windowId === secondary.id));
  events.length = 0;
  secondary.blur(); await flush();
  assert.deepEqual(events, []);
  const primaryEvents = [];
  for (const name of ['focus','blur']) primary.on(name, () => primaryEvents.push(name));
  const focusCalls = calls.filter(([op]) => op === 'window.focus').length;
  primary.focus();
  assert.deepEqual(primaryEvents, ['focus']); assert.equal(e.BrowserWindow.getFocusedWindow(), primary);
  await flush();
  assert.equal(calls.filter(([op]) => op === 'window.focus').length, focusCalls);
});

test('minimize, maximize and restore update isMinimized and isMaximized', async () => {
  const {load,calls} = runtime(); const e = load('electron');
  const primary = new e.BrowserWindow(); const secondary = new e.BrowserWindow();
  await flush();
  const events = [];
  for (const name of ['minimize','maximize','restore']) secondary.on(name, () => events.push(name));
  assert.equal(secondary.isMinimized(), false); assert.equal(secondary.isMaximized(), false);
  secondary.minimize(); await flush();
  assert.equal(secondary.isMinimized(), true); assert.equal(secondary.isMaximized(), false);
  assert.deepEqual(events, ['minimize']);
  assert.ok(calls.some(([op,a]) => op === 'window.minimize' && a.windowId === secondary.id));
  events.length = 0;
  secondary.maximize(); await flush();
  assert.equal(secondary.isMinimized(), false); assert.equal(secondary.isMaximized(), true);
  assert.deepEqual(events, ['maximize']);
  assert.ok(calls.some(([op,a]) => op === 'window.maximize' && a.windowId === secondary.id));
  events.length = 0;
  secondary.restore(); await flush();
  assert.equal(secondary.isMinimized(), false); assert.equal(secondary.isMaximized(), false);
  assert.deepEqual(events, ['restore']);
  assert.ok(calls.some(([op,a]) => op === 'window.restore' && a.windowId === secondary.id));
  events.length = 0;
  secondary.minimize(); secondary.minimize(); await flush();
  assert.deepEqual(events, ['minimize']); assert.equal(secondary.isMinimized(), true);
  events.length = 0;
  secondary.restore(); await flush();
  assert.deepEqual(events, ['restore']); assert.equal(secondary.isMinimized(), false);
  secondary.maximize(); await flush();
  events.length = 0;
  secondary.maximize(); await flush();
  assert.deepEqual(events, []); assert.equal(secondary.isMaximized(), true);
});
