import { spawn, execFileSync } from 'node:child_process';
import net from 'node:net';
import { writeFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
const [exe, out = 'e2e-report.json'] = process.argv.slice(2);
const report = { exceptions: [], page: null, responding: null, dialog: null, running: false, assertions: {}, error: null };
const sleep = ms => new Promise(r => setTimeout(r, ms));
let child, ws;
try {
  if (!exe) throw new Error('Usage: node scripts/e2e-windows.mjs <exe> [report.json]');
  const port = await new Promise((resolve, reject) => { const s = net.createServer(); s.on('error', reject); s.listen(0, '127.0.0.1', () => { const p = s.address().port; s.close(() => resolve(p)); }); });
  child = spawn(exe, [], { env: { ...process.env, WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${port} --remote-allow-origins=*` }, stdio: 'ignore' });
  let spawnError; child.on('error', e => { spawnError = e; });
  let target;
  for (let i = 0; i < 100 && !target; i++) { if (spawnError) throw spawnError; try { const list = await (await fetch(`http://127.0.0.1:${port}/json`)).json(); target = list.find(t => t.type === 'page' && /tsgpui|localhost/i.test(t.url) && t.webSocketDebuggerUrl); } catch {} if (!target) await sleep(300); }
  if (!target) throw new Error('Timed out waiting for page');
  ws = new WebSocket(target.webSocketDebuggerUrl);
  await new Promise((resolve, reject) => { ws.onopen = resolve; ws.onerror = reject; });
  let id = 0; const pending = new Map();
  ws.onmessage = e => { const m = JSON.parse(e.data); if (m.method === 'Runtime.exceptionThrown') report.exceptions.push(m.params); if (m.id && pending.has(m.id)) { const p = pending.get(m.id); pending.delete(m.id); m.error ? p.reject(new Error(JSON.stringify(m.error))) : p.resolve(m.result); } };
  const send = (method, params = {}) => new Promise((resolve, reject) => { const n = ++id; const timer = setTimeout(() => { pending.delete(n); reject(new Error(`CDP timeout: ${method}`)); }, 10000); pending.set(n, {resolve: value => { clearTimeout(timer); resolve(value); }, reject: error => { clearTimeout(timer); reject(error); }}); ws.send(JSON.stringify({id:n, method, params})); });
  const evaluate = async expression => { const r = await send('Runtime.evaluate', {expression, returnByValue:true}); if (r.exceptionDetails) throw new Error(JSON.stringify(r.exceptionDetails)); return r.result.value; };
  await send('Runtime.enable'); await sleep(3000);
  report.page = JSON.parse(await evaluate(`JSON.stringify({font: getComputedStyle(document.body).fontFamily, height: document.querySelector('textarea') ? getComputedStyle(document.querySelector('textarea')).height : null})`));
  await evaluate("window.__openResult='pending'; window.api.openFile().then(v => window.__openResult = v === null ? 'canceled' : 'resolved').catch(e => window.__openResult = 'error: '+e); 'started'");
  await sleep(8000);
  report.responding = execFileSync('powershell.exe', ['-NoProfile','-Command', `(Get-Process -Id ${child.pid} -ErrorAction SilentlyContinue).Responding`], {encoding:'utf8'}).trim() === 'True';
  report.dialog = execFileSync('powershell.exe', ['-NoProfile','-ExecutionPolicy','Bypass','-File',fileURLToPath(new URL('./close-child-dialog.ps1', import.meta.url)),'-ParentPid',String(child.pid)], {encoding:'utf8'}).trim();
  await sleep(3000); report.running = child.exitCode === null; report.openResult = await evaluate('window.__openResult');
  report.assertions = { exceptions: report.exceptions.length === 0, styles: !/^(serif|"?Times New Roman"?)$/i.test(report.page.font.trim()) && report.page.height === '300px', responding: report.responding, canceled: report.dialog.startsWith('canceled dialog ') && report.openResult === 'canceled' && report.running };
} catch (e) { report.error = String(e?.stack || e); } finally { if (ws) ws.close(); if (child && child.exitCode === null && child.pid) child.kill(); }
const json = JSON.stringify(report, null, 2); await writeFile(out, json + '\n'); console.log(json);
if (report.error || Object.keys(report.assertions).length !== 4 || Object.values(report.assertions).some(v => !v)) process.exitCode = 1;

