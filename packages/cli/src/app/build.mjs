// Build pipeline for Rendrix applications.
//
//   project/  rendrix.json, index.html, src/**/*.{ts,tsx,js}, assets…
//      │  TypeScript compiler API: type-check, then per-module transpile to ES modules
//      ▼
//   rendrix-app/1 bundle (JSON): { html, modules{path→js}, assets{path→text|base64}, window, permissions }
//
// The runtime never sees TypeScript: every module is plain ES2022 JavaScript before it is executed.
// Bare imports are resolved here (node_modules ESM); the runtime resolves only bundle-relative ids.

import ts from 'typescript';
import path from 'node:path';
import { readFile, readdir, stat, writeFile, mkdir } from 'node:fs/promises';

export const BUNDLE_FORMAT = 'rendrix-app/1';
const BUILTIN_MODULES = new Set(['rendrix', 'rendrix/jsx-runtime', 'rendrix/jsx-dev-runtime']);
const SOURCE_EXT = ['.ts', '.tsx', '.mts', '.jsx', '.js', '.mjs'];
const SKIP_DIRS = new Set(['node_modules', 'dist', '.git', '.rendrix', 'target']);
const SKIP_FILES = new Set(['package.json', 'package-lock.json', 'bun.lock', 'bun.lockb', 'pnpm-lock.yaml', 'yarn.lock', 'tsconfig.json', 'rendrix.json', 'index.html']);
const MAX_ASSET = 64 * 1024 * 1024;

const posix = p => p.split(path.sep).join('/');
const isCode = f => /\.(tsx?|mts|jsx|mjs|js)$/.test(f) && !f.endsWith('.d.ts');

export class BuildError extends Error {
  constructor(message, diagnostics = []) { super(message); this.name = 'BuildError'; this.diagnostics = diagnostics; }
}

async function exists(p) { try { return (await stat(p)).isFile(); } catch { return false; } }

export async function loadProject(dir) {
  dir = path.resolve(dir);
  let raw;
  try { raw = JSON.parse(await readFile(path.join(dir, 'rendrix.json'), 'utf8')); }
  catch (e) { throw new BuildError(`${path.join(dir, 'rendrix.json')}: ${e.code === 'ENOENT' ? 'not found. Run this command inside a Rendrix project (or run `rendrix init`).' : e.message}`); }
  const name = raw.name;
  if (typeof name !== 'string' || !/^[A-Za-z][A-Za-z0-9_-]{0,63}$/.test(name)) throw new BuildError('rendrix.json: "name" must be 1-64 letters, digits, "-" or "_", starting with a letter');
  const win = raw.window || {};
  const size = (v, d) => (Number.isInteger(v) && v >= 100 && v <= 8192 ? v : d);
  return {
    dir, name,
    title: typeof win.title === 'string' ? win.title : name,
    width: size(win.width, 960), height: size(win.height, 640),
    entry: raw.entry || 'index.html',
    permissions: normalizePermissions(raw.permissions),
  };
}

function normalizePermissions(p = {}) {
  const list = v => (Array.isArray(v) ? v.map(String) : []);
  return { network: p.network === true, fs_read: list(p.fs_read), fs_write: list(p.fs_write) };
}

// ----- module resolution ----------------------------------------------------------------------

async function resolveFile(base) {
  const candidates = [base, ...SOURCE_EXT.map(e => base + e), ...SOURCE_EXT.map(e => path.join(base, 'index' + e))];
  // `./x.js` may refer to `./x.ts` (TypeScript convention).
  const m = /\.(m?js)$/.exec(base);
  if (m) candidates.push(base.slice(0, -m[0].length) + '.ts', base.slice(0, -m[0].length) + '.tsx');
  for (const c of candidates) if (await exists(c)) return c;
  return null;
}

async function findPackageRoot(from, pkg) {
  let dir = from;
  for (;;) {
    const candidate = path.join(dir, 'node_modules', pkg);
    try { if ((await stat(candidate)).isDirectory()) return candidate; } catch { /* keep walking */ }
    const up = path.dirname(dir);
    if (up === dir) return null;
    dir = up;
  }
}

function pickExport(exp) {
  if (typeof exp === 'string') return exp;
  if (Array.isArray(exp)) { for (const x of exp) { const r = pickExport(x); if (r) return r; } return null; }
  if (exp && typeof exp === 'object') {
    for (const key of ['import', 'module', 'browser', 'default']) if (key in exp) { const r = pickExport(exp[key]); if (r) return r; }
  }
  return null;
}

async function resolveBare(spec, fromFile, projectDir) {
  const parts = spec.split('/');
  const pkgName = spec.startsWith('@') ? parts.slice(0, 2).join('/') : parts[0];
  const sub = parts.slice(spec.startsWith('@') ? 2 : 1).join('/');
  const root = await findPackageRoot(path.dirname(fromFile), pkgName) || await findPackageRoot(projectDir, pkgName);
  if (!root) throw new BuildError(`Cannot find package '${pkgName}' (imported from ${path.relative(projectDir, fromFile)}). Install it with your package manager.`);
  const pj = JSON.parse(await readFile(path.join(root, 'package.json'), 'utf8'));
  let target = null;
  if (pj.exports) {
    const key = sub ? './' + sub : '.';
    const e = typeof pj.exports === 'object' && !Array.isArray(pj.exports) && Object.keys(pj.exports).some(k => k.startsWith('.')) ? pj.exports[key] : (key === '.' ? pj.exports : null);
    target = pickExport(e);
  }
  if (!target) target = sub ? sub : (pj.module || pj.main || 'index.js');
  const file = await resolveFile(path.join(root, target));
  if (!file) throw new BuildError(`Cannot resolve '${spec}' (package entry '${target}' not found)`);
  return file;
}

// ----- compiler -------------------------------------------------------------------------------

const compilerOptions = {
  target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ESNext, jsx: ts.JsxEmit.ReactJSX, jsxImportSource: 'rendrix',
  sourceMap: false, inlineSourceMap: false, removeComments: false, verbatimModuleSyntax: false,
};

function moduleKey(projectDir, file) {
  const rel = posix(path.relative(projectDir, file));
  const base = rel.startsWith('..') ? 'external/' + posix(file).replace(/^[A-Za-z]:/, '').replace(/^\/+/, '') : rel;
  return base.replace(/\.(tsx?|mts|jsx|mjs)$/, '.js');
}

function relativeSpecifier(fromKey, toKey) {
  let r = posix(path.relative(path.posix.dirname(fromKey), toKey));
  if (!r.startsWith('.')) r = './' + r;
  return r;
}

/** Collect module specifiers (with source positions) from import/export declarations and dynamic imports. */
function collectSpecifiers(sf) {
  const out = [];
  const add = node => { if (node && ts.isStringLiteralLike(node)) out.push({ spec: node.text, start: node.getStart(sf) + 1, end: node.getEnd() - 1 }); };
  const visit = node => {
    if (ts.isImportDeclaration(node) && !(node.importClause && node.importClause.isTypeOnly)) add(node.moduleSpecifier);
    else if (ts.isExportDeclaration(node) && node.moduleSpecifier && !node.isTypeOnly) add(node.moduleSpecifier);
    else if (ts.isCallExpression(node) && node.expression.kind === ts.SyntaxKind.ImportKeyword) add(node.arguments[0]);
    ts.forEachChild(node, visit);
  };
  visit(sf);
  return out;
}

function formatDiagnostic(project, d) {
  const text = ts.flattenDiagnosticMessageText(d.messageText, '\n');
  if (d.file && d.start !== undefined) {
    const { line, character } = d.file.getLineAndCharacterOfPosition(d.start);
    return `${path.relative(project.dir, d.file.fileName)}:${line + 1}:${character + 1} - TS${d.code}: ${text}`;
  }
  return `TS${d.code}: ${text}`;
}

/** Type-check the project with its tsconfig.json. Returns formatted error strings. */
export async function typeCheck(project) {
  const configPath = path.join(project.dir, 'tsconfig.json');
  if (!(await exists(configPath))) return [];
  const cfg = ts.getParsedCommandLineOfConfigFile(configPath, {}, { ...ts.sys, onUnRecoverableConfigFileDiagnostic: d => { throw new BuildError(formatDiagnostic(project, d)); } });
  const program = ts.createProgram({ rootNames: cfg.fileNames, options: { ...cfg.options, noEmit: true } });
  return ts.getPreEmitDiagnostics(program).filter(d => d.category === ts.DiagnosticCategory.Error).map(d => formatDiagnostic(project, d));
}

export async function buildApp(projectDir, { typecheck = true } = {}) {
  const project = await loadProject(projectDir);
  const warnings = [];
  const errors = typecheck ? await typeCheck(project) : [];
  if (errors.length) throw new BuildError(`Type errors in ${project.name}:\n${errors.join('\n')}`, errors);

  const htmlPath = path.join(project.dir, project.entry);
  let html;
  try { html = await readFile(htmlPath, 'utf8'); } catch { throw new BuildError(`${project.entry} not found in ${project.dir}`); }

  const modules = {};
  const compiled = new Map(); // abs file → key
  const sources = [];         // abs files that went into the bundle (for watching)

  async function compile(file) {
    if (compiled.has(file)) return compiled.get(file);
    const key = moduleKey(project.dir, file);
    compiled.set(file, key);
    sources.push(file);
    if (/\.css$/.test(file)) {
      const css = await readFile(file, 'utf8');
      modules[key + '.js'] = `const css = ${JSON.stringify(css)};\nconst s = document.createElement('style'); s.textContent = css; document.head.appendChild(s);\nexport default css;\n`;
      return key + '.js';
    }
    if (/\.json$/.test(file)) { modules[key + '.js'] = `export default ${await readFile(file, 'utf8')};\n`; return key + '.js'; }
    let source = await readFile(file, 'utf8');
    const kind = /\.tsx$/.test(file) ? ts.ScriptKind.TSX : /\.jsx$/.test(file) ? ts.ScriptKind.JSX : /\.(m?js)$/.test(file) ? ts.ScriptKind.JS : ts.ScriptKind.TS;
    const sf = ts.createSourceFile(file, source, ts.ScriptTarget.ES2022, true, kind);
    const syntax = (sf.parseDiagnostics || []).map(d => formatDiagnostic(project, { ...d, file: sf }));
    if (syntax.length) throw new BuildError(`Syntax errors:\n${syntax.join('\n')}`, syntax);

    // Resolve imports; rewrite each specifier to a path inside the bundle (back to front keeps offsets valid).
    const edits = [];
    for (const imp of collectSpecifiers(sf)) {
      if (BUILTIN_MODULES.has(imp.spec)) continue;
      let target;
      if (/^(node:|https?:)/.test(imp.spec)) throw new BuildError(`${path.relative(project.dir, file)}: '${imp.spec}' is not available in Rendrix (no Node.js or remote module loading). Use the Web APIs or the 'rendrix' module.`);
      if (imp.spec.startsWith('.') || path.isAbsolute(imp.spec)) {
        const base = path.resolve(path.dirname(file), imp.spec);
        target = (await resolveFile(base)) || ((await exists(base)) ? base : null);
        if (!target) throw new BuildError(`${path.relative(project.dir, file)}: cannot resolve '${imp.spec}'`);
      } else {
        target = await resolveBare(imp.spec, file, project.dir);
      }
      const targetKey = await compile(target);
      edits.push({ ...imp, replacement: relativeSpecifier(key, targetKey) });
    }
    edits.sort((a, b) => b.start - a.start);
    for (const e of edits) source = source.slice(0, e.start) + e.replacement + source.slice(e.end);

    const isTs = /\.(tsx?|mts)$/.test(file);
    const options = { ...compilerOptions };
    if (!/\.(tsx|jsx)$/.test(file)) { delete options.jsx; delete options.jsxImportSource; }
    const out = ts.transpileModule(source, { fileName: file, reportDiagnostics: true, compilerOptions: options });
    const diags = (out.diagnostics || []).filter(d => d.category === ts.DiagnosticCategory.Error).map(d => formatDiagnostic(project, d));
    if (diags.length) throw new BuildError(`Compile errors:\n${diags.join('\n')}`, diags);
    if (!isTs && /\brequire\s*\(|\bmodule\.exports\b/.test(out.outputText) && !/\bexport\b|\bimport\b/.test(out.outputText)) {
      throw new BuildError(`${path.relative(project.dir, file)} is a CommonJS module. Rendrix loads ES modules; use an ESM build of this dependency.`);
    }
    modules[key] = out.outputText;
    return key;
  }

  // Entry scripts from the HTML; rewrite their src to bundle keys.
  const scriptRe = /<script\b([^>]*?)\bsrc\s*=\s*("([^"]*)"|'([^']*)')([^>]*)>/gi;
  const replacements = [];
  for (const m of html.matchAll(scriptRe)) {
    const src = m[3] ?? m[4];
    if (/^[a-z]+:/i.test(src)) { warnings.push(`remote script '${src}' is not loaded`); continue; }
    const abs = path.resolve(project.dir, src.replace(/^\//, ''));
    const file = (await resolveFile(abs)) || abs;
    if (!(await exists(file))) throw new BuildError(`script '${src}' referenced from ${project.entry} was not found`);
    const key = await compile(file);
    replacements.push({ from: m[0], to: m[0].replace(m[2], `"${key}"`) });
  }
  for (const r of replacements) html = html.replace(r.from, r.to);

  // Assets: every other file in the project (styles, images, fonts, data).
  const assets = {};
  async function walk(dir) {
    for (const entry of await readdir(dir, { withFileTypes: true })) {
      if (entry.name.startsWith('.') || SKIP_DIRS.has(entry.name)) continue;
      const full = path.join(dir, entry.name);
      if (entry.isDirectory()) { await walk(full); continue; }
      if (SKIP_FILES.has(entry.name) && dir === project.dir) continue;
      if (isCode(full) || entry.name.endsWith('.d.ts')) continue;
      const rel = posix(path.relative(project.dir, full));
      const info = await stat(full);
      if (info.size > MAX_ASSET) { warnings.push(`${rel} is larger than ${MAX_ASSET >> 20} MB and was skipped`); continue; }
      const bytes = await readFile(full);
      const textual = /\.(css|json|txt|svg|html|md|csv|xml|glsl)$/i.test(entry.name);
      assets[rel] = textual ? { text: bytes.toString('utf8') } : { base64: bytes.toString('base64') };
      sources.push(full);
    }
  }
  await walk(project.dir);
  sources.push(htmlPath, path.join(project.dir, 'rendrix.json'));

  const bundle = {
    format: BUNDLE_FORMAT, name: project.name, title: project.title, width: project.width, height: project.height,
    html, modules, assets, permissions: project.permissions,
  };
  return { bundle, project, warnings, sources };
}

export async function writeBundle(bundle, outFile) {
  await mkdir(path.dirname(outFile), { recursive: true });
  await writeFile(outFile, JSON.stringify(bundle));
  return outFile;
}

export function bundleStats(bundle) {
  const sum = o => Object.values(o).reduce((a, v) => a + (typeof v === 'string' ? v.length : v.text ? v.text.length : v.base64.length), 0);
  return { modules: Object.keys(bundle.modules).length, assets: Object.keys(bundle.assets).length, moduleBytes: sum(bundle.modules), assetBytes: sum(bundle.assets) };
}
