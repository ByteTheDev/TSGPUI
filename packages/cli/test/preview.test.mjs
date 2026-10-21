import test from 'node:test';
import assert from 'node:assert/strict';
import os from 'node:os';
import path from 'node:path';
import { mkdtemp, writeFile, rm, mkdir } from 'node:fs/promises';
import { compileProject } from '../src/compiler.mjs';
import { mergePreserved, previewCss, renderPreviewHTML, startPreview } from '../src/preview.mjs';

const config = { name: 'preview-app', title: 'Preview <App>', width: 500, height: 400 };
const backend = `export default { state:{ count: 0, notes: '' }, actions:{ bump(api) { api.set('count', api.get('count') + 1); } } };`;

function bundleFor(html, css = '') {
  return compileProject({ html, css, backend, config, warnings: [] });
}

test('preview CSS targets each node by key, with its states and media rules', () => {
  const bundle = bundleFor(
    '<body><button id="go" data-onclick="bump">Go</button></body>',
    '#go { background: #112233; } #go:hover { background: #445566; } #go:disabled { opacity: 0.5; } @media (min-width: 300px) { #go { width: 120px; } }',
  );
  const key = bundle.root.children[0].key;
  const css = previewCss(bundle);
  assert.match(css, new RegExp(`\\[data-k="${key}"\\]\\{[^}]*background:#112233`));
  assert.match(css, new RegExp(`\\[data-k="${key}"\\]:hover\\{background:#445566\\}`));
  assert.match(css, new RegExp(`\\[data-k="${key}"\\]\\[disabled\\]\\{opacity:0.5\\}`));
  assert.match(css, new RegExp(`@media \\(min-width: 300px\\)\\{\\[data-k="${key}"\\]\\{width:120px\\}\\}`));
});

test('state rules come after media rules, so :hover beats a media rule', () => {
  const bundle = bundleFor(
    '<body><div id="box">x</div></body>',
    '#box { background: #000000; } @media (min-width: 100px) { #box { background: #111111; } } #box:hover { background: #222222; }',
  );
  const css = previewCss(bundle);
  assert.ok(css.indexOf('@media') < css.indexOf(':hover'));
});

test('the page embeds the bundle as JSON that cannot end its script element', () => {
  const bundle = compileProject({
    html: '<body><span data-bind="notes">x</span></body>',
    css: '',
    backend: `export default { state:{ notes: '</script><script>alert(1)</script>' }, actions:{} };`,
    config,
    warnings: [],
  });
  const page = renderPreviewHTML(bundle);
  const json = page.slice(page.indexOf('id="tsgpui-bundle">') + 'id="tsgpui-bundle">'.length, page.indexOf('</script>', page.indexOf('id="tsgpui-bundle">')));
  assert.doesNotMatch(json, /</, 'angle brackets are escaped in the JSON');
  assert.equal(JSON.parse(json).backend_js.includes('</script>'), true, 'the value survives as data');
  assert.match(page, /<title>Preview &lt;App&gt;<\/title>/);
});

test('the standalone page has no live polling and carries a content security policy', () => {
  const page = renderPreviewHTML(bundleFor('<body>x</body>'));
  assert.match(page, /http-equiv="Content-Security-Policy"/);
  assert.match(page, /"live":null/);
  assert.doesNotMatch(page, /connect-src[^"]*https:\/\/example/);
});

test('network hosts from the config are the only https origins the page may contact', () => {
  const bundle = compileProject({
    html: '<body>x</body>', css: '', backend,
    config: { ...config, permissions: { networkHosts: ['api.example.com'] } },
    warnings: [],
  });
  const page = renderPreviewHTML(bundle);
  assert.match(page, /connect-src 'self' https:\/\/api\.example\.com/);
});

test('preview state merge matches the native rules', () => {
  // Same cases as merge_keeps_user_changes_and_takes_new_seeds in native/src/runtime.rs.
  const seed = { count: 0, name: 'ada', items: [1] };
  const current = { count: 5, name: 'ada', items: [1, 2] };
  const fresh = { count: 10, name: 'grace', items: [3], extra: true };
  assert.deepEqual(mergePreserved(fresh, seed, current), { count: 5, name: 'grace', items: [1, 2], extra: true });
  assert.deepEqual(mergePreserved({ a: 1 }, { a: 0, b: 0 }, { a: 0, b: 4 }), { a: 1 });
  assert.deepEqual(mergePreserved({ v: 'text' }, { v: 0 }, { v: 7 }), { v: 'text' });
  assert.deepEqual(mergePreserved({ user: { name: 'grace', age: 1 } }, { user: { name: 'ada', age: 36 } }, { user: { name: 'ada', age: 37 } }),
    { user: { name: 'grace', age: 37 } });
});

test('startPreview serves the page and a version that changes only on rebuild', async () => {
  const dir = await mkdtemp(path.join(os.tmpdir(), 'tsgpui-preview-'));
  try {
    await mkdir(path.join(dir, 'src'), { recursive: true });
    await writeFile(path.join(dir, 'tsgpui.config.json'), JSON.stringify({ name: 'serve-test', title: 'Serve', html: 'src/index.html', css: 'src/styles.css', backend: 'src/backend.ts' }));
    await writeFile(path.join(dir, 'src/index.html'), '<body><p>Hello</p></body>');
    await writeFile(path.join(dir, 'src/styles.css'), 'p { color: #ff0000; }');
    await writeFile(path.join(dir, 'src/backend.ts'), backend);
    const logs = [];
    const session = await startPreview({ projectRoot: dir, port: 0, open: false, watch: false, log: line => logs.push(line), warn: line => logs.push(line) });
    try {
      const page = await fetch(session.url);
      assert.equal(page.status, 200);
      const html = await page.text();
      assert.match(html, /id="tsgpui-bundle"/);
      assert.match(html, /"live":1/);
      const version = await (await fetch(new URL('/version', session.url))).json();
      assert.deepEqual(version, { version: 1 });
      assert.equal((await fetch(new URL('/missing', session.url))).status, 404);
    } finally {
      await session.close();
    }
  } finally {
    await rm(dir, { recursive: true, force: true });
  }
});
