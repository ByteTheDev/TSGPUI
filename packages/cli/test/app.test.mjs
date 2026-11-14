import test from 'node:test';
import assert from 'node:assert/strict';
import path from 'node:path';
import os from 'node:os';
import { spawnSync } from 'node:child_process';
import { mkdtemp, mkdir, writeFile, readFile, rm } from 'node:fs/promises';
import { buildApp, BuildError } from '../src/app/build.mjs';
import { init, findRuntime, packExecutable, MAGIC } from '../src/app/commands.mjs';

async function project(files) {
  const dir = await mkdtemp(path.join(os.tmpdir(), 'rendrix-cli-test-'));
  const all = { 'rendrix.json': '{"name":"t","window":{"width":300,"height":200}}', 'index.html': '<html><body><div id=root></div><script type="module" src="src/main.ts"></script></body></html>', ...files };
  for (const [name, content] of Object.entries(all)) {
    await mkdir(path.dirname(path.join(dir, name)), { recursive: true });
    await writeFile(path.join(dir, name), content);
  }
  return dir;
}
const cleanup = dir => rm(dir, { recursive: true, force: true });

test('compiles TypeScript to ES modules and rewrites imports to bundle paths', async () => {
  const dir = await project({
    'src/main.ts': "import { add } from './util';\nimport data from './data.json';\nimport type { Shape } from './types';\nconst s: Shape = { n: add(1, 2) };\nexport const out: number = s.n + data.k;",
    'src/util.ts': 'export const add = (a: number, b: number): number => a + b;',
    'src/types.ts': 'export interface Shape { n: number }',
    'src/data.json': '{"k": 4}',
    'logo.png': 'not really a png',
    'styles.css': 'body{margin:0}',
  });
  try {
    const { bundle } = await buildApp(dir, { typecheck: false });
    assert.deepEqual(Object.keys(bundle.modules).sort(), ['src/data.json.js', 'src/main.js', 'src/util.js'], 'type-only imports vanish; json becomes a module');
    assert.match(bundle.modules['src/main.js'], /from ['"]\.\/util\.js['"]/);
    assert.match(bundle.modules['src/main.js'], /from ['"]\.\/data\.json\.js['"]/);
    assert.doesNotMatch(bundle.modules['src/main.js'], /: number|interface|import type/, 'types are erased');
    assert.match(bundle.html, /src="src\/main\.js"/);
    assert.equal(bundle.format, 'rendrix-app/1');
    assert.deepEqual([bundle.width, bundle.height], [300, 200]);
    assert.equal(bundle.assets['styles.css'].text, 'body{margin:0}');
    assert.ok(bundle.assets['logo.png'].base64, 'binary assets are base64');
    assert.ok(!('src/main.ts' in bundle.assets) && !('rendrix.json' in bundle.assets));
  } finally { await cleanup(dir); }
});

test('TSX compiles to the rendrix JSX runtime', async () => {
  const dir = await project({ 'src/main.ts': "import './view.tsx';", 'src/view.tsx': 'const el = <div class="a">hi</div>; export default el;' });
  try {
    const { bundle } = await buildApp(dir, { typecheck: false });
    assert.match(bundle.modules['src/view.js'], /from "rendrix\/jsx-runtime"/);
    assert.match(bundle.modules['src/view.js'], /_jsx\("div"/);
  } finally { await cleanup(dir); }
});

test('type errors fail the build with file:line:column diagnostics', async () => {
  const dir = await project({
    'src/main.ts': "const n: number = 'not a number';\nexport {};",
    'tsconfig.json': '{"compilerOptions":{"strict":true,"module":"ESNext","target":"ES2022","moduleResolution":"bundler","noEmit":true,"lib":["ES2022","DOM"],"types":[]},"include":["src"]}',
  });
  try {
    await assert.rejects(buildApp(dir), e => e instanceof BuildError && /src[\\/]main\.ts:1:7 - TS2322/.test(e.message));
    const { bundle } = await buildApp(dir, { typecheck: false });
    assert.ok(bundle.modules['src/main.js'], '--no-typecheck still emits');
  } finally { await cleanup(dir); }
});

test('syntax errors, missing files and unsupported imports are reported precisely', async () => {
  let dir = await project({ 'src/main.ts': 'const = ;' });
  try { await assert.rejects(buildApp(dir, { typecheck: false }), /Syntax errors:[\s\S]*main\.ts:1:/); } finally { await cleanup(dir); }
  dir = await project({ 'src/main.ts': "import './nope';" });
  try { await assert.rejects(buildApp(dir, { typecheck: false }), /cannot resolve '\.\/nope'/); } finally { await cleanup(dir); }
  dir = await project({ 'src/main.ts': "import fs from 'node:fs';" });
  try { await assert.rejects(buildApp(dir, { typecheck: false }), /not available in Rendrix/); } finally { await cleanup(dir); }
  dir = await project({ 'src/main.ts': "import 'left-pad';" });
  try { await assert.rejects(buildApp(dir, { typecheck: false }), /Cannot find package 'left-pad'/); } finally { await cleanup(dir); }
  dir = await project({ 'index.html': '<script type="module" src="src/missing.ts"></script>' });
  try { await assert.rejects(buildApp(dir, { typecheck: false }), /missing\.ts.*was not found/); } finally { await cleanup(dir); }
  await assert.rejects(buildApp(path.join(os.tmpdir(), 'definitely-not-a-project-xyz')), /rendrix\.json/);
});

test('resolves ESM packages from node_modules (exports map) and rejects CommonJS', async () => {
  const dir = await project({
    'src/main.ts': "import { hello } from 'greeter';\nimport sub from 'greeter/extra';\nhello(sub);",
    'node_modules/greeter/package.json': '{"name":"greeter","exports":{".":{"import":"./esm/index.js","require":"./cjs.js"},"./extra":"./esm/extra.js"}}',
    'node_modules/greeter/esm/index.js': "import { x } from './dep.js';\nexport const hello = s => x + s;",
    'node_modules/greeter/esm/dep.js': "export const x = 'hi ';",
    'node_modules/greeter/esm/extra.js': "export default 'there';",
    'node_modules/greeter/cjs.js': 'module.exports = {};',
  });
  try {
    const { bundle } = await buildApp(dir, { typecheck: false });
    assert.ok(bundle.modules['node_modules/greeter/esm/index.js'] && bundle.modules['node_modules/greeter/esm/dep.js']);
    assert.match(bundle.modules['src/main.js'], /from ['"]\.\.\/node_modules\/greeter\/esm\/index\.js['"]/);
  } finally { await cleanup(dir); }
  const cjs = await project({ 'src/main.ts': "import x from 'oldlib';\nx;", 'node_modules/oldlib/package.json': '{"name":"oldlib","main":"index.js"}', 'node_modules/oldlib/index.js': 'module.exports = function(){};' });
  try { await assert.rejects(buildApp(cjs, { typecheck: false }), /CommonJS/); } finally { await cleanup(cjs); }
});

test('permissions and window config are validated and normalised', async () => {
  const dir = await project({ 'rendrix.json': '{"name":"p","window":{"width":10,"title":"T"},"permissions":{"network":true,"fs_read":["data"]}}', 'src/main.ts': '' });
  try {
    const { bundle } = await buildApp(dir, { typecheck: false });
    assert.deepEqual(bundle.permissions, { network: true, fs_read: ['data'], fs_write: [] });
    assert.equal(bundle.width, 960, 'out-of-range width falls back to the default');
    assert.equal(bundle.title, 'T');
  } finally { await cleanup(dir); }
  const bad = await project({ 'rendrix.json': '{"name":"1bad name"}', 'src/main.ts': '' });
  try { await assert.rejects(buildApp(bad, { typecheck: false }), /"name" must be/); } finally { await cleanup(bad); }
});

test('packExecutable appends a trailer and replaces an existing one', async () => {
  const dir = await mkdtemp(path.join(os.tmpdir(), 'rendrix-pack-test-'));
  try {
    const runtime = path.join(dir, 'rt.bin');
    await writeFile(runtime, Buffer.from('MZ-fake-runtime'));
    const out = path.join(dir, 'app.bin');
    await packExecutable(runtime, { format: 'rendrix-app/1', name: 'one' }, out);
    await packExecutable(out, { format: 'rendrix-app/1', name: 'two' }, out);
    const bytes = await readFile(out);
    assert.ok(bytes.subarray(0, 15).equals(Buffer.from('MZ-fake-runtime')));
    assert.ok(bytes.subarray(bytes.length - 8).equals(MAGIC));
    const len = Number(bytes.readBigUInt64LE(bytes.length - 16));
    assert.equal(JSON.parse(bytes.subarray(bytes.length - 16 - len, bytes.length - 16).toString()).name, 'two');
    assert.equal(bytes.length, 15 + len + 16, 'the first bundle was replaced, not stacked');
  } finally { await cleanup(dir); }
});

test('`rendrix init` scaffolds a project that type-checks and builds', async () => {
  const dir = path.join(await mkdtemp(path.join(os.tmpdir(), 'rendrix-init-test-')), 'my-app');
  try {
    await init(dir, {});
    const { bundle } = await buildApp(dir, { typecheck: true });
    assert.equal(bundle.name, 'my-app');
    assert.match(bundle.modules['src/main.js'], /Hello, Rendrix/);
    await assert.rejects(init(dir, {}), /not empty/);
  } finally { await cleanup(path.dirname(dir)); }
});

test('a compiled bundle runs in the real runtime and responds to a scripted click', { skip: !findRuntime() && 'runtime binary not built' }, async () => {
  const dir = await project({
    'index.html': '<html><head><style>body{margin:0} #b{width:80px;height:30px}</style></head><body><button id="b">go</button><script type="module" src="src/main.ts"></script></body></html>',
    'src/main.ts': "let n = 0;\nconst b = document.getElementById('b') as HTMLButtonElement;\nb.addEventListener('click', () => { n += 1; b.textContent = 'n=' + n; });",
  });
  try {
    const { bundle } = await buildApp(dir, { typecheck: false });
    const bundlePath = path.join(dir, 't.rdxapp');
    await writeFile(bundlePath, JSON.stringify(bundle));
    const script = path.join(dir, 'steps.json');
    await writeFile(script, JSON.stringify([
      { type: 'click', selector: '#b' }, { type: 'click', selector: '#b' },
      { type: 'expect', js: "document.getElementById('b').textContent", equals: 'n=2' },
    ]));
    const r = spawnSync(findRuntime(), ['run', bundlePath, '--headless', '--script', script, '--size', '300x200'], { encoding: 'utf8' });
    assert.equal(r.status, 0, r.stderr + r.stdout);
    assert.match(r.stdout, /\[expect\] ok/);
  } finally { await cleanup(dir); }
});
