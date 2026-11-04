import test from 'node:test';
import assert from 'node:assert/strict';
import { validateConfig, buildPackage } from '../src/compiler.mjs';
import { extractEmbeddedBundleForTest, MAGIC } from '../src/pack.mjs';
import { collectWebviewFiles, prepareWebviewHtml } from '../src/webview.mjs';
import { mkdtemp, mkdir, writeFile, readFile, rm } from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';

test('bundle size limit defaults to 256 MiB and accepts positive byte limits', () => {
  const config = { name: 'Example', html: 'index.html', css: 'style.css', backend: 'backend.ts', renderer: 'webview' };
  assert.equal(validateConfig(config).maxBundleSize, 256 * 1024 * 1024);
  assert.equal(validateConfig({ ...config, maxBundleSize: 1024 }).maxBundleSize, 1024);
  for (const value of [0, -1, 1.5, '1024']) {
    assert.throws(() => validateConfig({ ...config, maxBundleSize: value }), /maxBundleSize/);
  }
});

test('collects local module, stylesheet, CSS image, and dynamic chunk dependencies', async () => {
  const root = await mkdtemp(path.join(os.tmpdir(), 'tsgpui-webview-'));
  try {
    await mkdir(path.join(root, 'src'));
    await writeFile(path.join(root, 'src', 'main.js'), "import './dep.js'; import('./chunk.js')");
    await writeFile(path.join(root, 'src', 'dep.js'), 'export const x = 1');
    await writeFile(path.join(root, 'src', 'chunk.js'), 'export default 2');
    await writeFile(path.join(root, 'src', 'site.css'), 'body { background: url(/image.png) }');
    await writeFile(path.join(root, 'image.png'), Buffer.from([0, 1, 2]));
    const html = '<link rel="stylesheet" href="./site.css"><link rel="modulepreload" href="./dep.js"><script type="module" src="/src/main.js"></script>';
    const files = await collectWebviewFiles(html, '', {projectRoot:root,htmlFile:path.join(root,'src','index.html'),cssFile:path.join(root,'src','style.css')});
    assert.deepEqual(Object.keys(files).sort(), ['/image.png','/src/chunk.js','/src/dep.js','/src/main.js','/src/site.css']);
    assert.equal(files['/image.png'].data, 'AAEC');
    assert.equal(files['/src/main.js'].mime, 'text/javascript');
  } finally { await rm(root,{recursive:true,force:true}); }
});

test('CSP precedes authored scripts and lists only configured network hosts', async () => {
  const html = await prepareWebviewHtml('<html><head><script>window.started=true</script></head><body></body></html>', '', {backendJs:'',title:'Test',networkHosts:['api.example.com']});
  assert.ok(html.indexOf('Content-Security-Policy') < html.indexOf('window.started'));
  assert.match(html, /default-src &#39;none&#39;/);
  assert.match(html, /script-src &#39;self&#39; &#39;nonce-[A-Za-z0-9+/=]+&#39;/);
  assert.match(html, /connect-src &#39;self&#39; https:\/\/api\.example\.com/);
});

test('build records the versioned file map and configured entry', async () => {
  const root = await mkdtemp(path.join(os.tmpdir(), 'tsgpui-build-'));
  try {
    await mkdir(path.join(root,'src'));
    await writeFile(path.join(root,'tsgpui.config.json'),JSON.stringify({name:'Example',renderer:'webview',html:'src/index.html',css:'src/style.css',backend:'src/backend.ts'}));
    await writeFile(path.join(root,'src/index.html'),'<html><head><link rel="stylesheet" href="site.css"></head><body><script src="main.js"></script></body></html>');
    await writeFile(path.join(root,'src/site.css'),'body { color: red }');
    await writeFile(path.join(root,'src/style.css'),'');
    await writeFile(path.join(root,'src/backend.ts'),'export default {state:{},actions:{}}');
    await writeFile(path.join(root,'src/main.js'),'export default 42');
    await buildPackage(root);
    const doc=JSON.parse(await readFile(path.join(root,'dist/Example.tgp'),'utf8'));
    assert.equal(doc.webview_format_version,2);
    assert.equal(doc.webview_entry,'/src/index.html');
    assert.match(doc.webview_html, /<link rel="stylesheet" href="site\.css">/);
    assert.match(doc.webview_html, /<script src="main\.js"><\/script>/);
    assert.equal(Buffer.from(doc.webview_files['/src/site.css'].data,'base64').toString(),'body { color: red }');
    assert.equal(Buffer.from(doc.webview_files['/src/main.js'].data,'base64').toString(),'export default 42');
  } finally { await rm(root,{recursive:true,force:true}); }
});

test('legacy single-document bundle remains extractable', () => {
  const old={format:'tsgpui-1',renderer:'webview',webview_html:'<html>old</html>'};
  const payload=Buffer.from(JSON.stringify(old));
  const footer=Buffer.alloc(16); footer.writeBigUInt64LE(BigInt(payload.length)); MAGIC.copy(footer,8);
  assert.deepEqual(extractEmbeddedBundleForTest(Buffer.concat([payload,footer])),old);
});
