import { spawn } from 'node:child_process';
import net from 'node:net';
import { writeFile } from 'node:fs/promises';

const [exe, ...args] = process.argv.slice(2);
if (!exe || (args.length && (args.length !== 2 || args[0] !== '--out'))) {
  console.error('Usage: node scripts/webview-probe.mjs <path-to-exe> [--out <json-file>]'); process.exit(1);
}
if (typeof WebSocket !== 'function') { console.error('Node global WebSocket is unavailable; use Node 22 or newer.'); process.exit(1); }
const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
async function freePort(port) {
  return new Promise((resolve, reject) => {
    const server = net.createServer();
    server.once('error', reject);
    server.listen(port, '127.0.0.1', () => { const selected = server.address().port; server.close(() => resolve(selected)); });
  });
}
let port;
try { port = await freePort(9222); } catch { port = await freePort(0); }
console.error(`Debugging port: ${port}`);
const child = spawn(exe, [], { env: { ...process.env, WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${port} --remote-allow-origins=*` }, stdio: 'ignore' });
let spawnError;
child.on('error', error => { spawnError = error; });
let last = 'No response';
try {
  let page;
  for (let i = 0; i < 100; i++) {
    if (spawnError) throw spawnError;
    try {
      const response = await fetch(`http://127.0.0.1:${port}/json`);
      last = await response.text();
      const targets = JSON.parse(last);
      page = targets.find(target => target.type === 'page' && /tsgpui|localhost/i.test(target.url || '') && target.webSocketDebuggerUrl);
      if (page) break;
    } catch (error) { if (last === 'No response') last = String(error); }
    await sleep(200);
  }
  console.error(`/json: ${last}`);
  if (!page) throw new Error('Timed out waiting for a matching page at /json');
  const result = { console: [], exceptions: [], network: [], page: null };
  const ws = new WebSocket(page.webSocketDebuggerUrl);
  const pending = new Map(); let nextId = 0;
  ws.addEventListener('message', event => {
    const msg = JSON.parse(event.data);
    if (msg.id && pending.has(msg.id)) { const { resolve, reject } = pending.get(msg.id); pending.delete(msg.id); msg.error ? reject(new Error(JSON.stringify(msg.error))) : resolve(msg.result); }
    if (msg.method === 'Runtime.consoleAPICalled' || msg.method === 'Log.entryAdded') result.console.push({ method: msg.method, ...msg.params });
    if (msg.method === 'Runtime.exceptionThrown') result.exceptions.push(msg.params);
    if (msg.method === 'Network.responseReceived') result.network.push({ method: msg.method, requestId: msg.params.requestId, url: msg.params.response.url, status: msg.params.response.status, mimeType: msg.params.response.mimeType });
    if (msg.method === 'Network.loadingFailed') result.network.push({ method: msg.method, ...msg.params });
  });
  await new Promise((resolve, reject) => { ws.addEventListener('open', resolve, { once: true }); ws.addEventListener('error', reject, { once: true }); });
  const send = (method, params = {}) => new Promise((resolve, reject) => { const id = ++nextId; pending.set(id, { resolve, reject }); ws.send(JSON.stringify({ id, method, params })); });
  await Promise.all(['Runtime.enable', 'Log.enable', 'Network.enable', 'Page.enable'].map(method => send(method)));
  await sleep(3000);
  const evaluation = await send('Runtime.evaluate', { expression: `JSON.stringify({styleSheets: document.styleSheets.length, fontFamily: getComputedStyle(document.body).fontFamily, marginTop: getComputedStyle(document.body).marginTop, textareaHeight: document.querySelector('textarea') ? getComputedStyle(document.querySelector('textarea')).height : null, status: document.getElementById('status')?.textContent ?? null, scripts: Array.from(document.scripts, s => s.src), appExists: !!window.__tsgpui_app})`, returnByValue: true });
  if (evaluation.exceptionDetails) throw new Error(JSON.stringify(evaluation.exceptionDetails));
  result.page = JSON.parse(evaluation.result.value);
  const json = JSON.stringify(result, null, 2);
  console.log(json);
  if (args.length) await writeFile(args[1], json + '\n');
  ws.close();
} catch (error) { console.error(`Probe error: ${error?.stack || error}\nLast /json observation: ${last}`); process.exitCode = 1; }
finally { child.kill(); }
