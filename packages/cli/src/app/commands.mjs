// `rendrix` commands for application projects: init, check, build, run, dev, package, doctor.

import path from 'node:path';
import { spawn, spawnSync } from 'node:child_process';
import { existsSync, watch, statSync } from 'node:fs';
import { readFile, writeFile, mkdir, readdir, copyFile, chmod } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import { buildApp, writeBundle, bundleStats, typeCheck, loadProject, BuildError, BUNDLE_FORMAT } from './build.mjs';

const here = path.dirname(fileURLToPath(import.meta.url));
const cliRoot = path.resolve(here, '..', '..');
const repoRoot = path.resolve(cliRoot, '..', '..');
export const MAGIC = Buffer.from('RENDRAPP');
const exe = process.platform === 'win32' ? '.exe' : '';

const color = (c, s) => (process.stdout.isTTY ? `\x1b[${c}m${s}\x1b[0m` : s);
const ok = s => console.log(color(32, '✓'), s);
const info = s => console.log(color(36, '›'), s);
const warn = s => console.warn(color(33, '!'), s);
const fail = s => console.error(color(31, '✗'), s);

/** Locate the prebuilt runtime executable. */
export function findRuntime() {
  const candidates = [
    process.env.RENDRIX_RUNTIME,
    path.join(repoRoot, 'engine', 'target', 'release', 'rendrix-runtime' + exe),
    path.join(repoRoot, 'engine', 'target', 'debug', 'rendrix-runtime' + exe),
    process.env.RENDRIX_HOME && path.join(process.env.RENDRIX_HOME, 'runtime', `${process.platform}-${process.arch}`, 'rendrix-runtime' + exe),
  ].filter(Boolean);
  return candidates.find(p => existsSync(p)) || null;
}

function needRuntime() {
  const rt = findRuntime();
  if (!rt) throw new BuildError('The Rendrix runtime is not built. Run `cargo build --release -p rendrix-runtime --manifest-path engine/Cargo.toml`, or set RENDRIX_RUNTIME to a rendrix-runtime executable. `rendrix doctor` shows what was searched.');
  return rt;
}

export async function build(dir, opts = {}) {
  const t0 = Date.now();
  const { bundle, project, warnings, sources } = await buildApp(dir, { typecheck: opts.typecheck !== false });
  for (const w of warnings) warn(w);
  const out = path.resolve(opts.out || path.join(project.dir, 'dist', `${project.name}.rdxapp`));
  await writeBundle(bundle, out);
  const s = bundleStats(bundle);
  ok(`built ${project.name}: ${s.modules} module${s.modules === 1 ? '' : 's'} (${(s.moduleBytes / 1024).toFixed(1)} KB JS), ${s.assets} asset${s.assets === 1 ? '' : 's'} in ${Date.now() - t0} ms → ${path.relative(process.cwd(), out)}`);
  return { out, bundle, project, sources };
}

export async function check(dir) {
  const project = await loadProject(dir);
  const errors = await typeCheck(project);
  if (errors.length) throw new BuildError(`${errors.length} type error${errors.length === 1 ? '' : 's'}:\n${errors.join('\n')}`, errors);
  ok(`${project.name}: no type errors`);
}

export async function run(dir, opts = {}) {
  const { out } = await build(dir, opts);
  const rt = needRuntime();
  const args = ['run', out, ...(opts.runtimeArgs || [])];
  const child = spawnSync(rt, args, { stdio: 'inherit' });
  return child.status ?? 1;
}

/** Append the bundle to a copy of the runtime executable (`[json][u64 LE length][RENDRAPP]`). */
export async function packExecutable(runtimePath, bundle, outFile) {
  let bytes = await readFile(runtimePath);
  if (bytes.subarray(bytes.length - 8).equals(MAGIC)) {
    const len = Number(bytes.readBigUInt64LE(bytes.length - 16));
    bytes = bytes.subarray(0, bytes.length - 16 - len);
  }
  const json = Buffer.from(JSON.stringify(bundle));
  const trailer = Buffer.alloc(16);
  trailer.writeBigUInt64LE(BigInt(json.length), 0);
  MAGIC.copy(trailer, 8);
  await mkdir(path.dirname(outFile), { recursive: true });
  await writeFile(outFile, Buffer.concat([bytes, json, trailer]));
  if (process.platform !== 'win32') await chmod(outFile, 0o755);
}

export async function pack(dir, opts = {}) {
  const { bundle, project } = await build(dir, { ...opts, out: opts.bundleOut });
  const rt = opts.runtime || needRuntime();
  const out = path.resolve(opts.out || path.join(project.dir, 'dist', project.name + exe));
  await packExecutable(rt, bundle, out);
  ok(`packaged ${path.relative(process.cwd(), out)} (${(statSync(out).size / 1048576).toFixed(1)} MB, standalone; needs no Node, Bun or browser)`);
  return out;
}

export async function dev(dir, opts = {}) {
  const rt = needRuntime();
  let child = null;
  let building = false, again = false, watched = new Set();
  const watchers = [];
  const start = out => {
    if (child) { child.removeAllListeners('exit'); child.kill(); }
    child = spawn(rt, ['run', out, ...(opts.runtimeArgs || [])], { stdio: 'inherit' });
    child.on('exit', code => { if (code !== null) { info(`app exited (${code}); press Ctrl+C to stop watching`); child = null; } });
  };
  async function rebuild(first) {
    if (building) { again = true; return; }
    building = true;
    try {
      const { out, sources } = await build(dir, opts);
      start(out);
      sources.forEach(s => watched.add(path.dirname(s)));
      if (first) attachWatchers();
    } catch (e) {
      fail(e instanceof BuildError ? e.message : e.stack || String(e));
      if (first && !watchers.length) { watched.add(path.resolve(dir)); attachWatchers(); }
      info('waiting for changes… (the last working build keeps running)');
    }
    building = false;
    if (again) { again = false; rebuild(false); }
  }
  let timer;
  function attachWatchers() {
    const root = path.resolve(dir);
    try {
      watchers.push(watch(root, { recursive: true }, (_ev, file) => {
        if (!file || /(^|[\\/])(node_modules|dist|\.git|target)([\\/]|$)/.test(file)) return;
        clearTimeout(timer);
        timer = setTimeout(() => { info(`change in ${file}`); rebuild(false); }, 120);
      }));
    } catch (e) { warn(`file watching unavailable: ${e.message}`); }
  }
  await rebuild(true);
  info('watching for changes (Ctrl+C to stop)');
  await new Promise(resolve => { process.on('SIGINT', () => { watchers.forEach(w => w.close()); if (child) child.kill(); resolve(); }); });
}

// ----- init -----------------------------------------------------------------------------------

export async function init(target, opts = {}) {
  const dir = path.resolve(target);
  const name = (opts.name || path.basename(dir)).replace(/[^A-Za-z0-9_-]/g, '-').replace(/^[^A-Za-z]+/, 'app-') || 'app';
  const template = path.join(cliRoot, 'templates', opts.template || 'app');
  if (!existsSync(template)) throw new BuildError(`unknown template '${opts.template || 'app'}'`);
  if (existsSync(dir) && (await readdir(dir)).length) throw new BuildError(`${dir} already exists and is not empty`);
  const typesDir = path.join(repoRoot, 'packages', 'types');
  const typesRel = existsSync(path.join(typesDir, 'index.d.ts')) ? path.relative(dir, typesDir).split(path.sep).join('/') : null;
  async function copyDir(src, dst) {
    await mkdir(dst, { recursive: true });
    for (const e of await readdir(src, { withFileTypes: true })) {
      const s = path.join(src, e.name), d = path.join(dst, e.name.replace(/\.tpl$/, ''));
      if (e.isDirectory()) await copyDir(s, d);
      else if (/\.(json|html|tsx?|css|md|tpl)$/.test(e.name)) {
        let text = await readFile(s, 'utf8');
        text = text.replaceAll('__NAME__', name).replaceAll('__TYPES__', typesRel || 'node_modules/rendrix');
        await writeFile(d, text);
      } else await copyFile(s, d);
    }
  }
  await copyDir(template, dir);
  ok(`created ${path.relative(process.cwd(), dir) || '.'} from the '${opts.template || 'app'}' template`);
  console.log(`\n  cd ${path.relative(process.cwd(), dir) || '.'}\n  rendrix dev      # build, run and rebuild on change\n  rendrix package  # produce a standalone executable\n`);
}

// ----- doctor ---------------------------------------------------------------------------------

export async function doctor(dir) {
  let bad = 0;
  const row = (good, label, detail = '') => { console.log(`${good ? color(32, '✓') : color(31, '✗')} ${label}${detail ? color(90, '  ' + detail) : ''}`); if (!good) bad++; };
  row(Number(process.versions.node.split('.')[0]) >= 18, 'Node.js', process.version);
  let tsv = '';
  try { tsv = (await import('typescript')).default.version; } catch { /* reported below */ }
  row(!!tsv, 'TypeScript compiler', tsv || 'not installed (npm i -D typescript)');
  const rt = findRuntime();
  row(!!rt, 'Rendrix runtime', rt || 'not found. Build with: cargo build --release -p rendrix-runtime --manifest-path engine/Cargo.toml (or set RENDRIX_RUNTIME)');
  if (rt) {
    const r = spawnSync(rt, ['doctor'], { encoding: 'utf8' });
    if (r.status === 0) for (const line of r.stdout.trim().split('\n')) row(!/no suitable|error|failed/i.test(line), line.split(':')[0].trim(), line.split(':').slice(1).join(':').trim());
    else row(false, 'runtime self-check', (r.stderr || r.stdout || 'failed to start').trim());
  }
  try {
    const p = await loadProject(dir);
    row(true, `project '${p.name}'`, `${p.width}x${p.height}, network ${p.permissions.network ? 'allowed' : 'denied'}`);
  } catch (e) { row(true, 'project', 'no rendrix.json here (fine outside a project)'); void e; }
  console.log(bad ? `\n${bad} problem${bad === 1 ? '' : 's'} found.` : '\nEverything looks good.');
  return bad ? 1 : 0;
}

export { BUNDLE_FORMAT };
