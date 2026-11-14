import ts from 'typescript';
import path from 'node:path';
import {fileURLToPath} from 'node:url';
import {spawnSync} from 'node:child_process';
import {createRequire} from 'node:module';
import {existsSync, readFileSync, statSync} from 'node:fs';
import {readFile, writeFile, mkdir, cp, stat, readdir, realpath} from 'node:fs/promises';
import {bundleBackend} from './backend.mjs';
import {statusOf} from './electron-compat/coverage.mjs';

const ELECTRON_SPECS = new Set(['electron','electron/main','electron/renderer','node:electron']);

const moduleDir = path.dirname(fileURLToPath(import.meta.url));
const runtimeDir = [path.join(moduleDir, 'electron-compat', 'runtime'), path.join(moduleDir, '..', 'src', 'electron-compat', 'runtime')].find(existsSync) || path.join(moduleDir, 'electron-compat', 'runtime');

// Which `app.X(...)` calls the runtime shim really has, read from its App class so this list cannot drift.
const EVENT_EMITTER_METHODS = ['on', 'once', 'off', 'addListener', 'removeListener', 'removeAllListeners', 'emit', 'listenerCount'];
let appMethodsCache;
function appMethods() {
  if (appMethodsCache) return appMethodsCache;
  appMethodsCache = new Set(EVENT_EMITTER_METHODS);
  try {
    const source = readFileSync(path.join(runtimeDir, 'electron.ts'), 'utf8').replace(/\r\n/g, '\n');
    const start = source.indexOf('class App extends EventEmitter {');
    const end = start < 0 ? -1 : source.indexOf('\n}\n', start);
    if (start >= 0 && end > start) {
      for (const match of source.slice(start, end).matchAll(/^  (?:async )?([A-Za-z]\w*)\(/gm)) if (match[1] !== 'constructor') appMethodsCache.add(match[1]);
    }
  } catch (_error) { /* with no readable runtime only the event methods are known, so other app.* calls are reviewed */ }
  return appMethodsCache;
}
const OTHER_KNOWN_CALLS = /^(?:dialog\.(?:showOpenDialog|showSaveDialog|showMessageBox)|shell\.openExternal|clipboard\.(?:readText|writeText)|BrowserWindow\.(?:getAllWindows|fromWebContents)|Notification\.isSupported)$/;
function isKnownCall(expr) {
  // Promise chaining on the result does not change which API was called.
  const call = expr.replace(/(?:\s*\(\)\s*\.\s*(?:then|catch|finally))+\s*$/, '').trim();
  const app = /^app\.(\w+)$/.exec(call);
  return app ? appMethods().has(app[1]) : OTHER_KNOWN_CALLS.test(call);
}
const builtins = new Map(['electron','fs','fs/promises','path','os','events','child_process','process','https'].flatMap(n => [[n,n === 'fs/promises' ? 'fs' : n],['node:'+n,n === 'fs/promises' ? 'fs' : n]]));
for (const spec of ELECTRON_SPECS) builtins.set(spec, 'electron');
const slash = p => p.replaceAll('\\','/');
const exists = async p => !!(await stat(p).catch(() => null));
const inside = (root,p) => p === root || p.startsWith(root + path.sep);
const literal = n => n && (ts.isStringLiteralLike(n) || ts.isNoSubstitutionTemplateLiteral(n)) ? n.text : undefined;
const relImport = (from,to) => { let p = slash(path.relative(path.dirname(from),to)).replace(/\.ts$/, ''); return p.startsWith('.') ? p : './'+p; };

// --- Rendrix page adaptation -------------------------------------------------------------
// Imported Electron renderer pages are browser documents; the Rendrix compiler accepts only the
// subset in docs/SUPPORTED_SYNTAX.md (ALLOWED_TAGS in src/html.mjs). Every copied page is
// rewritten here: the document wrappers are stripped, unsupported elements become neutral <div>
// containers (keeping id and class so structure survives), unsupported attributes are dropped,
// and every change is recorded for the import report.
const IMPORT_KEEP_TAGS = new Set([
  'main','header','footer','section','article','div','nav','aside',
  'h1','h2','h3','h4','h5','h6','p','span','strong','small','blockquote','pre','code','figure','figcaption',
  'details','summary','table','caption','thead','tbody','tfoot','tr','th','td',
  'button','label','form','ul','ol','li','hr','img','input','textarea','select','option',
]);
const IMPORT_VOID_TAGS = new Set(['input','img','hr']);
// Raw-text elements: their content is not markup. script/style are removed entirely (logic belongs
// in backend.ts / the CSS file); title is captured as a fallback title; textarea passes through.
const IMPORT_RAW_TEXT_TAGS = new Set(['script','style','title','textarea']);
// Elements dropped without their content: metadata/void tags with nothing to preserve.
const IMPORT_DROP_TAGS = new Set(['link','meta','base','br','wbr']);
// Document wrappers are unwrapped; their children become the page content.
const IMPORT_UNWRAP_TAGS = new Set(['html','head','body']);
const IMPORT_KEEP_ATTRS = new Set(['id','class','title','aria-label','alt','name','placeholder','value','disabled','required','selected','for','tabindex','role']);
const IMPORT_INPUT_TYPES = new Set(['text','number','checkbox','email','password','search']);
const IMPORT_BUTTON_TYPES = new Set(['button','submit']);

function parsePageAttrs(text) {
  const attrs = [];
  for (const match of text.matchAll(/([^\s=/<>"']+)(?:\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s"'>]+)))?/g)) {
    const name = match[1].toLowerCase();
    if (!/^[a-zA-Z_][\w:.-]*$/.test(name)) continue;
    if (!attrs.some(attr => attr.name === name)) attrs.push({name, value: match[2] ?? match[3] ?? match[4] ?? ''});
  }
  return attrs;
}
function escapePageAttr(value) { return String(value).replace(/&/g,'&amp;').replace(/"/g,'&quot;'); }
function escapePageText(value) { return String(value).replace(/&/g,'&amp;').replace(/</g,'&lt;'); }

// Rewrites one page and returns {html, pageTitle, notes}. `resolveImg` maps an img src to the
// copied asset's project-relative path, or null when the compiler could not load it.
function renderrizePage(source, resolveImg = null) {
  const replaced = new Map(), droppedAttrs = new Map();
  let removedScripts = 0, removedStyles = 0, removedMetadata = 0, removedBreaks = 0, removedTemplates = 0, rewrittenImgs = 0, pageTitle = null;
  const bump = (map, key) => map.set(key, (map.get(key) ?? 0) + 1);
  const counts = map => [...map.entries()].sort().map(([key, count]) => `${key}×${count}`).join(', ');
  const keptAttrs = (tag, attrs) => {
    const kept = [], seen = new Set();
    const effectiveType = tag === 'input' ? (attrs.find(attr => attr.name === 'type')?.value ?? 'text').toLowerCase() : undefined;
    for (const {name, value} of attrs) {
      if (seen.has(name)) continue;
      seen.add(name);
      if (name === 'id' || name === 'class') { kept.push(`${name}="${escapePageAttr(value)}"`); continue; }
      if (IMPORT_KEEP_ATTRS.has(name) && (name !== 'alt' || tag === 'img')) { kept.push(`${name}="${escapePageAttr(value)}"`); continue; }
      if (name === 'src' && tag === 'img' && value && !/^(?:[a-z]+:|[\\/])|\.\./i.test(value)) { kept.push(`src="${escapePageAttr(value)}"`); continue; }
      if (name === 'type' && tag === 'input' && IMPORT_INPUT_TYPES.has(effectiveType)) { kept.push(`type="${effectiveType}"`); continue; }
      if (name === 'type' && tag === 'button' && IMPORT_BUTTON_TYPES.has(value.toLowerCase())) { kept.push(`type="${value.toLowerCase()}"`); continue; }
      if (name === 'checked' && tag === 'input' && effectiveType === 'checkbox') { kept.push('checked=""'); continue; }
      if (['min','max','step'].includes(name) && tag === 'input' && effectiveType === 'number') { kept.push(`${name}="${escapePageAttr(value)}"`); continue; }
      bump(droppedAttrs, name);
    }
    return kept.length ? ' ' + kept.join(' ') : '';
  };
  const out = [];
  const topLevel = [];  // 'tag' for a top-level element, null for a top-level text run.
  const stack = [];     // {srcTag, emitted}; emitted is null for unwrapped wrappers.
  const depth = () => stack.filter(entry => entry.emitted !== null).length;
  const closeTop = () => { const entry = stack.pop(); if (entry.emitted !== null) out.push(`</${entry.emitted}>`); };
  let index = 0;
  while (index < source.length) {
    const rest = source.slice(index);
    if (rest.startsWith('<!--')) {
      const close = source.indexOf('-->', index + 4);
      index = close < 0 ? source.length : close + 3;
      continue;
    }
    const doctype = /^<!doctype\s+html\s*>/i.exec(rest);
    if (doctype) { index += doctype[0].length; continue; }
    if (rest[0] === '<') {
      const token = /^<\/?[a-zA-Z](?:[^'"=>]|=\s*"[^"]*"|=\s*'[^']*'|=)*>/.exec(rest);
      if (!token) { out.push('&lt;'); index++; continue; }
      index += token[0].length;
      const tag = /^<\/?([a-zA-Z][a-zA-Z0-9-]*)/.exec(token[0])[1].toLowerCase();
      const closing = token[0].startsWith('</');
      const selfClosing = token[0].endsWith('/>');
      const tagNameLength = /^<\/?[a-zA-Z][a-zA-Z0-9-]*/.exec(token[0])[0].length;
      if (closing) {
        const depthIndex = stack.map(entry => entry.srcTag).lastIndexOf(tag);
        if (depthIndex < 0) continue;  // A stray closing tag; browsers ignore it too.
        while (stack.length > depthIndex + 1) closeTop();
        closeTop();
        continue;
      }
      const attrs = parsePageAttrs(token[0].slice(tagNameLength, selfClosing ? -2 : -1));
      if (IMPORT_RAW_TEXT_TAGS.has(tag)) {
        const end = new RegExp(`</\\s*${tag}\\s*>`, 'i').exec(source.slice(index));
        const raw = end ? source.slice(index, index + end.index) : source.slice(index);
        index = end ? index + end.index + end[0].length : source.length;
        if (tag === 'title') { if (pageTitle === null) pageTitle = raw.replace(/\s+/g, ' ').trim(); }
        else if (tag === 'textarea') {
          if (depth() === 0) topLevel.push('textarea');
          out.push(`<textarea${keptAttrs('textarea', attrs)}>${escapePageText(raw)}</textarea>`);
        }
        else if (tag === 'script') removedScripts++;
        else if (tag === 'style') removedStyles++;
        continue;
      }
      if (tag === 'template') {
        const end = /<\/\s*template\s*>/i.exec(source.slice(index));
        index = end ? index + end.index + end[0].length : source.length;
        removedTemplates++;
        continue;
      }
      if (IMPORT_UNWRAP_TAGS.has(tag)) {
        if (!selfClosing) stack.push({srcTag: tag, emitted: null});
        continue;
      }
      if (IMPORT_DROP_TAGS.has(tag)) {
        if (tag === 'br' || tag === 'wbr') { removedBreaks++; out.push(' '); } else removedMetadata++;
        continue;
      }
      // An <img> the Rendrix compiler cannot load (no src, or a src that resolves to no copied
      // asset) becomes a <div> that keeps id/class and shows the alt text. A resolvable src is
      // rewritten to the copied asset's project-relative path, because the compiler resolves
      // image srcs against the project root.
      if (tag === 'img') {
        const rawSrc = attrs.find(attr => attr.name === 'src')?.value ?? '';
        const rewritten = resolveImg && rawSrc ? resolveImg(rawSrc) : (rawSrc && !/^(?:[a-z]+:|[\\/])|\.\./i.test(rawSrc) ? rawSrc : null);
        if (!rewritten) {
          const kept = keptAttrs('div', attrs);
          const alt = attrs.find(attr => attr.name === 'alt')?.value;
          if (depth() === 0) topLevel.push('div');
          bump(replaced, 'img');
          out.push(alt ? `<div${kept}>${escapePageText(alt)}</div>` : `<div${kept}/>`);
          continue;
        }
        const kept = keptAttrs('img', attrs.filter(attr => attr.name !== 'src')) + ` src="${escapePageAttr(rewritten)}"`;
        if (depth() === 0) topLevel.push('img');
        rewrittenImgs++;
        out.push(`<img${kept}/>`);
        continue;
      }
      const unsupported = !IMPORT_KEEP_TAGS.has(tag);
      const emitted = unsupported ? 'div' : tag;
      const attributes = keptAttrs(unsupported ? 'div' : tag, attrs);
      if (depth() === 0) topLevel.push(emitted);
      if (unsupported) bump(replaced, tag);
      if (selfClosing || IMPORT_VOID_TAGS.has(tag)) out.push(`<${emitted}${attributes}/>`);
      else { out.push(`<${emitted}${attributes}>`); stack.push({srcTag: tag, emitted}); }
      continue;
    }
    const next = source.indexOf('<', index);
    const raw = source.slice(index, next === -1 ? undefined : next);
    index = next === -1 ? source.length : next;
    if (raw.trim()) {
      if (depth() === 0) topLevel.push(null);
      out.push(raw);
    }
  }
  while (stack.length) closeTop();
  let html = out.join('');
  const elements = topLevel.filter(Boolean);
  const singleRoot = topLevel.length === 1 && elements.length === 1 && ['main','div','section'].includes(elements[0]);
  if (!topLevel.length) html = '<div></div>';
  else if (!singleRoot) html = `<div>${html}</div>`;
  const changes = [];
  if (replaced.size) changes.push(`replaced unsupported elements with <div> containers (${counts(replaced)})`);
  if (droppedAttrs.size) changes.push(`dropped unsupported attributes (${counts(droppedAttrs)})`);
  if (removedScripts) changes.push(`removed ${removedScripts} <script> element${removedScripts > 1 ? 's' : ''} (move the logic into backend.ts)`);
  if (removedStyles) changes.push(`removed ${removedStyles} inline <style> element${removedStyles > 1 ? 's' : ''} (move the rules into the CSS file)`);
  if (removedTemplates) changes.push(`removed ${removedTemplates} <template> block${removedTemplates > 1 ? 's' : ''} (inert browser templates have no renderer equivalent)`);
  if (rewrittenImgs) changes.push(`rewrote ${rewrittenImgs} image src path${rewrittenImgs > 1 ? 's' : ''} to the copied assets (the compiler resolves image srcs from the project root)`);
  if (removedMetadata) changes.push(`removed ${removedMetadata} <link>/<meta>/<base> tag${removedMetadata > 1 ? 's' : ''}`);
  if (removedBreaks) changes.push(`dropped ${removedBreaks} line-break tag${removedBreaks > 1 ? 's' : ''}`);
  const notes = changes.length ? [`adapted for the Rendrix renderer: ${changes.join('; ')}; stripped the <html>/<head>/<body> document wrappers`] : [];
  return {html, pageTitle, notes};
}
// --- end Rendrix page adaptation ---------------------------------------------------------


export async function importElectron(input, {out, build = false, force = false} = {}) {
  const root = await realpath(path.resolve(input)).catch(() => path.resolve(input)), dest = path.resolve(out || root + '-tsgpu');
  if (inside(root,dest) || inside(dest,root)) throw new Error('Output and source directories must not contain one another');
  const pkg = JSON.parse(await readFile(path.join(root,'package.json'),'utf8'));
  if (await exists(dest) && !force) throw new Error(`Output already exists: ${dest}; pass --force to reuse it`);
  if (build) {
    if (!pkg.scripts?.build) throw new Error('No build script found in package.json');
    const bun = await exists(path.join(root,'bun.lock')) || await exists(path.join(root,'bun.lockb'));
    const cmd = bun ? 'bun' : 'npm';
    console.log(`Running ${cmd} run build in ${root}`);
    const found = process.platform === 'win32' ? spawnSync('where.exe',[cmd === 'npm' ? 'npm.cmd' : 'bun.exe'],{encoding:'utf8'}).stdout?.split(/\r?\n/).find(Boolean) : null;
    const executable = process.platform === 'win32' && cmd === 'npm' ? path.join(path.dirname(found || process.execPath),'node_modules','npm','bin','npm-cli.js') : (found || cmd);
    const result = spawnSync(process.platform === 'win32' && cmd === 'npm' ? process.execPath : executable, process.platform === 'win32' && cmd === 'npm' ? [executable,'run','build'] : ['run','build'], {cwd:root, stdio:'inherit'});
    if (result.error || result.status !== 0) throw new Error(`Build script failed: ${result.error?.message || result.status}`);
  }
  const main = path.resolve(root,pkg.main || 'index.js');
  if (!inside(root,main) || !await exists(main)) throw new Error(`Main process entry not found: ${pkg.main || 'index.js'}`);
  const notes = [], rewrites = [], visited = new Set(), modules = new Map(), unsupported = new Map(), hosts = new Set(), commands = new Set();
  const usedRuntime = new Set();
  const filesystemPaths = [];
  let fsNeedsReview = false;
  const usage = {fs:false, relativeFs:false, window:false, notifications:false, ipc:false, session:false};
  let sessionPersist = false;
  const sessionNotes = [];
  const protocolSchemes = new Set();
  const protocolNotes = [];
  let renderer, width=900, height=600, title, titleBarStyle;
  let contextIsolation, nodeIntegration, webviewSandbox;
  const renderers = new Set();
  const remotePages = new Set(); // http(s) pages the app loads: there is no web view, so they cannot be imported
  const windowPreloads = new Map(), rendererWindows = new Map();
  function addRenderer(file) { renderers.add(file); if (!renderer) renderer = file; }
  const sourceDir = path.join(dest,'src'), mainDir = path.join(sourceDir,'main');
  const warn = (file,node,ast,msg) => notes.push(`${slash(path.relative(root,file))}:${ast.getLineAndCharacterOfPosition(node.getStart(ast)).line+1}: ${msg}`);
  async function resolveLocal(file,spec) {
    const base=path.resolve(path.dirname(file),spec);
    let baseReal = base;
    try { baseReal = await realpath(base); } catch {}
    if (!inside(root,baseReal)) return null;
    for (const p of [base,base+'.js',base+'.ts',base+'.mjs',base+'.cjs',path.join(base,'index.js'),path.join(base,'index.ts')]) if ((await stat(p).catch(()=>null))?.isFile()) { try { return await realpath(p); } catch { return p; } }
    return null;
  }
  // Lists every electron export the file uses (destructured require, named import, or `alias.name`)
  // and reports those the ledger does not mark as implemented.
  function electronApiReview(ast, file) {
    const aliases = new Set(), used = new Map();
    const note = (name, node) => { if (!used.has(name)) used.set(name, node); };
    const isElectronRequire = n => n && ts.isCallExpression(n) && ts.isIdentifier(n.expression) && n.expression.text === 'require' && ELECTRON_SPECS.has(literal(n.arguments[0]));
    (function walk(node) {
      if (ts.isVariableDeclaration(node) && isElectronRequire(node.initializer)) {
        if (ts.isObjectBindingPattern(node.name)) for (const el of node.name.elements) note((el.propertyName ?? el.name).getText(ast), el);
        else if (ts.isIdentifier(node.name)) aliases.add(node.name.text);
      }
      if (ts.isImportDeclaration(node) && ELECTRON_SPECS.has(literal(node.moduleSpecifier))) {
        const clause = node.importClause, bindings = clause?.namedBindings;
        if (clause?.name) aliases.add(clause.name.text);
        if (bindings && ts.isNamespaceImport(bindings)) aliases.add(bindings.name.text);
        if (bindings && ts.isNamedImports(bindings)) for (const el of bindings.elements) note((el.propertyName ?? el.name).getText(ast), el);
      }
      ts.forEachChild(node, walk);
    })(ast);
    (function access(node) {
      if (ts.isPropertyAccessExpression(node) && ts.isIdentifier(node.expression) && aliases.has(node.expression.text)) note(node.name.text, node.name);
      ts.forEachChild(node, access);
    })(ast);
    for (const [name, node] of used) {
      const found = statusOf(name);
      if (!found) warn(file,node,ast,`Unknown Electron API electron.${name}: it is not in the TS-GPU coverage ledger`);
      else if (found.status !== 'implemented') warn(file,node,ast,`electron.${name} is ${found.status}: ${found.reason}`);
    }
  }
  const targetFor = file => path.join(mainDir, path.relative(root,file).replace(/\.(?:mjs|cjs|js|jsx|tsx|json)$/i,'.ts'));
  async function visit(file) {
    if (visited.has(file)) return;
    visited.add(file);
    if (/\.json$/i.test(file)) { const value = JSON.parse(await readFile(file, 'utf8')); modules.set(file, `module.exports = ${JSON.stringify(value)};\n`); return; }
    const source=await readFile(file,'utf8'), ast=ts.createSourceFile(file,source,ts.ScriptTarget.Latest,true, /\.tsx?$/.test(file) ? ts.ScriptKind.TS : ts.ScriptKind.JS);
    const edits=[], dependencies=[], imports=[];
    const staticPath = n => {
      const v=literal(n); if(v !== undefined) return v;
      if(ts.isIdentifier(n) && n.text==='__dirname') return path.dirname(file);
      if(ts.isCallExpression(n) && /^(?:path\.)?(?:join|resolve)$/.test(n.expression.getText(ast))) {
        const parts=n.arguments.map(staticPath); if(parts.every(p=>p !== undefined)) return path.resolve(path.dirname(file),...parts);
      }
    };
    // A string built from literals, __dirname, `+` and template spans, such as 'file://' + __dirname + '/index.html'.
    const staticString = n => {
      if(!n) return undefined;
      const v=literal(n); if(v !== undefined) return v;
      if(ts.isIdentifier(n) && n.text==='__dirname') return path.dirname(file);
      if(ts.isIdentifier(n)) { // a const declared in this file: const url = '...'
        let found;
        (function find(node) {
          if(found) return;
          if(ts.isVariableDeclaration(node) && ts.isIdentifier(node.name) && node.name.text===n.text && node.initializer && node.parent.flags & ts.NodeFlags.Const) { found=node.initializer; return; }
          ts.forEachChild(node,find);
        })(ast);
        return found && found !== n ? staticString(found) : undefined;
      }
      if(ts.isParenthesizedExpression(n)) return staticString(n.expression);
      if(ts.isBinaryExpression(n) && n.operatorToken.kind===ts.SyntaxKind.PlusToken) {
        const a=staticString(n.left), b=staticString(n.right);
        return a !== undefined && b !== undefined ? a+b : undefined;
      }
      if(ts.isTemplateExpression(n)) {
        let out=n.head.text;
        for(const span of n.templateSpans) { const part=staticString(span.expression); if(part === undefined) return undefined; out+=part+span.literal.text; }
        return out;
      }
      return staticPath(n);
    };
    // The local file a loadURL argument points at, when it can be worked out statically: a file: URL built by
    // concatenation or a template, or url.format({protocol: 'file:', pathname}).
    const staticFileUrl = n => {
      if(!n) return undefined;
      if(ts.isCallExpression(n) && /(?:^|\.)format$/.test(n.expression.getText(ast)) && n.arguments[0] && ts.isObjectLiteralExpression(n.arguments[0])) {
        const props=n.arguments[0].properties.filter(ts.isPropertyAssignment);
        const get=key => props.find(p=>p.name.getText(ast).replace(/^['"]|['"]$/g,'')===key)?.initializer;
        const protocol=get('protocol') && literal(get('protocol'));
        const pathname=staticString(get('pathname'));
        return /^file:?$/.test(protocol ?? '') && pathname ? path.resolve(path.dirname(file),pathname) : undefined;
      }
      const text=staticString(n);
      return text?.startsWith('file://') ? path.resolve(text.slice('file://'.length).replace(/^\/(?=[A-Za-z]:)/,'')) : undefined;
    };
    for (const d of ast.parseDiagnostics) warn(file,ast,ast,`Parse error: ${ts.flattenDiagnosticMessageText(d.messageText,' ')}`);
    function scan(node) {
      if (ts.isPropertyAccessExpression(node)) {
        const propName = node.name.text;
        const baseText = node.expression.getText(ast);
        if (['defaultSession','fromPartition','fromPath','cookies','getCookies','setCookies','clearCookies','getSession'].includes(propName) && baseText !== 'document') {
          sessionPersist = true; usage.session = true;
          if (propName==='defaultSession') sessionNotes.push(`${slash(path.relative(root,file))}:${ast.getLineAndCharacterOfPosition(node.getStart(ast)).line+1}: session.${propName} detected; mapped to "persist": true`);
          else if (propName==='cookies' || propName==='getCookies' || propName==='setCookies' || propName==='clearCookies') sessionNotes.push(`${slash(path.relative(root,file))}:${ast.getLineAndCharacterOfPosition(node.getStart(ast)).line+1}: session cookie access detected; mapped to "persist": true`);
          else if (propName==='fromPartition' || propName==='getSession') sessionNotes.push(`${slash(path.relative(root,file))}:${ast.getLineAndCharacterOfPosition(node.getStart(ast)).line+1}: session.${propName} detected; mapped to "persist": true`);
        }
      }
      if (ts.isCallExpression(node)) {
        const exprText = node.expression.getText(ast);
        // session.fromPartition('persist:...'), session.getSession(...), session.cookies.* or *.getCookies/setCookies/clearCookies
        if (/session\.(?:fromPartition|getSession|defaultSession)/.test(exprText) || /\.cookies\.(?:get|set|remove|clear|flush)/.test(exprText) || /\b(?:getCookies|setCookies|clearCookies)\b/.test(exprText) || /\.session\b/.test(exprText) && /cookies/.test(exprText)) { sessionPersist = true; usage.session = true; }
        if (/session\.defaultSession|session\.fromPartition|session\.getSession/.test(exprText)) sessionNotes.push(`${slash(path.relative(root,file))}:${ast.getLineAndCharacterOfPosition(node.getStart(ast)).line+1}: session API ${exprText} detected; mapped to "persist": true`);
        // Custom protocols: protocol.handle / registerFileProtocol (incl. electron.protocol.* applies).
        // Literal schemes are allowlisted for packaged assets; dynamic schemes need review.
        const protocolMatch = exprText.match(/(?:^|\.)(?:protocol|electron\.protocol)\.(handle|registerFileProtocol|registerBufferProtocol|registerHttpProtocol|registerStreamProtocol|registerStringProtocol|interceptBufferProtocol|interceptFileProtocol|interceptHttpProtocol|interceptStreamProtocol|interceptStringProtocol|registerSchemesAsPrivileged)$/);
        if (protocolMatch) {
          const method = protocolMatch[1];
          const line = ast.getLineAndCharacterOfPosition(node.getStart(ast)).line + 1;
          const where = `${slash(path.relative(root,file))}:${line}`;
          if (method === 'registerSchemesAsPrivileged') {
            const first = node.arguments[0];
            let dynamicPrivileged = false;
            if (first && ts.isArrayLiteralExpression(first)) {
              for (const el of first.elements) {
                let scheme;
                if (ts.isObjectLiteralExpression(el)) {
                  for (const prop of el.properties) if (ts.isPropertyAssignment(prop) && prop.name.getText(ast).replace(/^['"]|['"]$/g,'') === 'scheme') scheme = literal(prop.initializer);
                } else scheme = literal(el);
                if (scheme !== undefined && /^[a-z][a-z0-9+.-]*$/i.test(scheme)) {
                  const normalized = scheme.toLowerCase();
                  protocolSchemes.add(normalized);
                  protocolNotes.push(`${where}: protocol.registerSchemesAsPrivileged scheme '${normalized}' allowlisted for packaged assets; privileged semantics are recorded only`);
                } else dynamicPrivileged = true;
              }
              if (dynamicPrivileged) warn(file,node,ast,'Dynamic protocol scheme in registerSchemesAsPrivileged requires manual migration');
            } else warn(file,node,ast,'Dynamic protocol schemes in registerSchemesAsPrivileged require manual migration');
          } else {
            const schemeLiteral = literal(node.arguments[0]);
            if (schemeLiteral !== undefined && /^[a-z][a-z0-9+.-]*$/i.test(schemeLiteral)) {
              const normalized = schemeLiteral.toLowerCase();
              if (['http','https','ws','wss'].includes(normalized)) {
                warn(file,node,ast,`protocol.${method} for remote scheme '${normalized}:' requires manual migration; use permissions.network for HTTPS instead of a protocol handler`);
              } else {
                protocolSchemes.add(normalized);
                protocolNotes.push(`${where}: protocol.${method} scheme '${normalized}' allowlisted; mapped to packaged app assets (app/file-style in-page scheme)`);
              }
            } else if (method === 'registerFileProtocol' && node.arguments[0] && literal(node.arguments[0]) === undefined) {
              warn(file,node,ast,'Dynamic protocol scheme in registerFileProtocol requires manual migration; literal schemes are allowlisted automatically');
            } else if (node.arguments[0] === undefined || node.arguments[0] === null) {
              warn(file,node,ast,`protocol.${method} without a literal scheme requires manual migration`);
            } else {
              warn(file,node,ast,`Dynamic or invalid protocol scheme in protocol.${method} requires manual migration`);
            }
          }
        }
      }
      if (ts.isNewExpression(node) && /(?:^|\.)BrowserWindow$/.test(node.expression.getText(ast))) {
        usage.window=true;
        const obj=node.arguments?.[0];
        if (obj && ts.isObjectLiteralExpression(obj)) for (const prop of obj.properties) if (ts.isPropertyAssignment(prop)) {
          const key=prop.name.getText(ast).replace(/^['"]|['"]$/g,'');
          if (key==='width' || key==='height') { const n=Number(prop.initializer.getText(ast)); if (Number.isInteger(n) && n>=200 && n<=4096) {if(key==='width') width=n; else height=n;} else warn(file,prop,ast,`Dynamic or invalid window ${key}`); }
          if (key==='title') title=literal(prop.initializer);
          if (key==='titleBarStyle') titleBarStyle=literal(prop.initializer);
          if (key==='webPreferences' && ts.isObjectLiteralExpression(prop.initializer)) for (const p of prop.initializer.properties) if (ts.isPropertyAssignment(p)) {
            const prefKey = p.name.getText(ast).replace(/^['"]|['"]$/g,'');
            if (prefKey==='partition') {
              const part = literal(p.initializer);
              if (part !== undefined) {
                sessionPersist = true; usage.session = true;
                sessionNotes.push(`${slash(path.relative(root,file))}:${ast.getLineAndCharacterOfPosition(p.getStart(ast)).line+1}: webPreferences.partition "${part}" mapped to "persist": true`);
              } else warn(file,p,ast,'Dynamic webPreferences.partition requires review; session persistence is assumed');
              continue;
            }
            if (prefKey==='contextIsolation' || prefKey==='nodeIntegration' || prefKey==='sandbox') {
              const flag = prefKey==='sandbox' ? 'webviewSandbox' : prefKey;
              if (prefKey==='contextIsolation') contextIsolation = p.initializer.getText(ast);
              else if (prefKey==='nodeIntegration') nodeIntegration = p.initializer.getText(ast);
              else if (prefKey==='sandbox') webviewSandbox = p.initializer.getText(ast);
              void flag;
            }
            if (prefKey !== 'preload') continue;
            const value=staticPath(p.initializer);
            if(value !== undefined) {
              const variable = ts.isVariableDeclaration(node.parent) && ts.isIdentifier(node.parent.name) ? node.parent.name.text : undefined;
              if (variable) {
                const key = `${file}\0${variable}`;
                const preload = path.resolve(path.dirname(file),value);
                if (windowPreloads.has(key) && windowPreloads.get(key) !== preload) warn(file,p,ast,`Multiple preload paths use window variable ${variable}; association requires review`);
                else windowPreloads.set(key,preload);
              } else warn(file,p,ast,'Preload on an unassigned BrowserWindow requires review');
            } else warn(file,p,ast,'Dynamic preload path requires manual migration');
          }
        }
      }
      if (ts.isCallExpression(node)) {
        const expr=node.expression.getText(ast), arg=literal(node.arguments[0]);
        if (/\.loadFile$/.test(expr)) { const value=node.arguments[0] && staticPath(node.arguments[0]); if(value) { const page=path.resolve(root,value); addRenderer(page); if (ts.isPropertyAccessExpression(node.expression) && ts.isIdentifier(node.expression.expression)) { const ids=rendererWindows.get(page) || new Set(); ids.add(`${file}\0${node.expression.expression.text}`); rendererWindows.set(page,ids); } } else warn(file,node,ast,'Dynamic loadFile target requires manual migration'); }
        if (/\.loadURL$/.test(expr)) { const target = arg ?? staticString(node.arguments[0]); if (/^https?:/i.test(target ?? '')) remotePages.add(target); const builtFile = arg === undefined ? staticFileUrl(node.arguments[0]) : undefined; if(builtFile) { addRenderer(builtFile); if (ts.isPropertyAccessExpression(node.expression) && ts.isIdentifier(node.expression.expression)) { const ids=rendererWindows.get(builtFile) || new Set(); ids.add(`${file}\0${node.expression.expression.text}`); rendererWindows.set(builtFile,ids); } } else if(arg?.startsWith('file:')) { try {const page=fileURLToPath(arg); addRenderer(page); if (ts.isPropertyAccessExpression(node.expression) && ts.isIdentifier(node.expression.expression)) { const ids=rendererWindows.get(page) || new Set(); ids.add(`${file}\0${node.expression.expression.text}`); rendererWindows.set(page,ids); }} catch {warn(file,node,ast,'Invalid loadURL file target');} } else if (arg && /^[a-z][a-z0-9+.-]*:/i.test(arg) && !/^(?:https?|data|blob|javascript|mailto|tel):/i.test(arg)) { const scheme = arg.slice(0, arg.indexOf(':')).toLowerCase(); protocolSchemes.add(scheme); protocolNotes.push(`${slash(path.relative(root,file))}:${ast.getLineAndCharacterOfPosition(node.getStart(ast)).line+1}: loadURL target '${arg}' uses custom scheme '${scheme}' allowlisted for packaged app assets`); } else warn(file,node,ast,`loadURL target ${arg || '(dynamic)'} requires a local renderer build`); }
        if (/^(?:fetch|axios(?:\.[\w]+)?|https\.(?:get|request))$/.test(expr)) {
          const addHost = (value, isUrl) => { try { const u = new URL(isUrl ? value : 'https://'+value); hosts.add(u.hostname.toLowerCase()); } catch { warn(file,node,ast,`Network value requires review: ${value}`); } };
          const first=node.arguments[0];
          if(arg !== undefined) addHost(arg,true);
          else if(first && ts.isObjectLiteralExpression(first)) {
            const props=first.properties.filter(p=>ts.isPropertyAssignment(p) && ['hostname','host','url'].includes(p.name.getText(ast).replace(/^['"]|['"]$/g,'')));
            if(!props.length) warn(file,node,ast,'Network options lack a literal host; review');
            for(const p of props) { const v=literal(p.initializer); if(v === undefined) warn(file,p,ast,'Dynamic network URL requires review'); else addHost(v,p.name.getText(ast).replace(/^['"]|['"]$/g,'')==='url'); }
          } else warn(file,node,ast,'Dynamic network URL requires review');
        }
        if (/(?:^|\.)(?:spawn|execFile|exec|fork|spawnSync|execFileSync|execSync)$/.test(expr)) { if(arg && /^[\w.-]+$/.test(arg)) commands.add(arg); else warn(file,node,ast,'Dynamic or shell process command requires review'); }
        if (/^(?:fs|filesystem)(?:\.promises)?\./.test(expr)) {
          usage.fs=true;
          const argText = node.arguments[0] ? node.arguments[0].getText(ast) : '';
          const isUserData = /app\.getPath\s*\(\s*['"]userData['"]\s*\)/.test(argText);
          const isDialogPicked = /\.filePaths?\b/.test(argText);
          if (isUserData || isDialogPicked) {
            const line = ast.getLineAndCharacterOfPosition(node.getStart(ast)).line+1;
            const kind = isUserData ? 'auto-granted (userData)' : 'auto-granted (dialog-selected file)';
            filesystemPaths.push(`${slash(path.relative(root,file))}:${line} ${expr} â€” ${kind}. No permissions.filesystem entry needed.`);
          } else if(arg !== undefined && !path.isAbsolute(arg)) { usage.relativeFs=true; fsNeedsReview=true; warn(file,node,ast,`Literal relative filesystem path ${arg} requires review; granting ./data placeholder`); }
          else { fsNeedsReview=true; warn(file,node,ast,'Filesystem path requires review; userData and dialog-selected files are auto-granted'); }
        }
        if (/^(?:ipcMain|ipcRenderer)\./.test(expr)) usage.ipc=true;
        if (/^(?:dialog|shell|clipboard|app|BrowserWindow|Notification)\./.test(expr) && !isKnownCall(expr)) warn(file,node,ast,`Review potentially unsupported API ${expr}`);
      }
      if (ts.isNewExpression(node) && /(?:^|\.)Notification$/.test(node.expression.getText(ast))) usage.notifications=true;
      let spec, specNode, isRequire=false;
      if ((ts.isImportDeclaration(node) || ts.isExportDeclaration(node)) && node.moduleSpecifier && ts.isStringLiteral(node.moduleSpecifier)) {spec=node.moduleSpecifier.text; specNode=node.moduleSpecifier;}
      if (ts.isCallExpression(node) && ts.isIdentifier(node.expression) && node.expression.text==='require') {isRequire=true; spec=literal(node.arguments[0]); specNode=node.arguments[0]; if(!spec) { if (slash(path.relative(root,file)).split('/').includes('node_modules')) throw new Error(`Dynamic require in package module '${slash(path.relative(root,file))}' is not supported; it may load a native addon`); warn(file,node,ast,'Dynamic require is unsupported'); edits.push([node.getStart(ast),node.end,'(() => { throw new Error("Dynamic require is unsupported"); })()']); }}
      if (spec) dependencies.push({spec,specNode,node,isRequire});
      ts.forEachChild(node,scan);
    }
    scan(ast);
    if (dependencies.some(dep => ELECTRON_SPECS.has(dep.spec))) electronApiReview(ast, file);
    for (const dep of dependencies) {
      let target, kind=builtins.get(dep.spec);
      const owner = slash(path.relative(root,file));
      if (/\.node$/i.test(dep.spec) || ['bindings','node-gyp-build','node-pre-gyp','@mapbox/node-pre-gyp'].includes(dep.spec)) throw new Error(`Native addon '${dep.spec}' required by ${owner} is not supported`);
      if (kind) {target=path.join(sourceDir,'tsgpu-runtime',kind+'.ts'); usedRuntime.add(kind); if(kind==='fs') usage.fs=true;}
      else {
        let resolved = dep.spec.startsWith('.') ? await resolveLocal(file,dep.spec) : null;
        if (!resolved) { try { resolved = createRequire(file).resolve(dep.spec); if (resolved) { try { resolved = await realpath(resolved); } catch {} } } catch {} }
        if (resolved && path.isAbsolute(resolved) && inside(root,resolved)) {
          if (/\.node$/i.test(resolved)) throw new Error(`Native addon '${dep.spec}' (${slash(path.relative(root,resolved))}) required by ${owner} is not supported`);
          if (!/\.(?:js|cjs|mjs|ts|json)$/i.test(resolved)) throw new Error(`Unsupported module '${dep.spec}' required by ${owner}: ${resolved}`);
          target=targetFor(resolved); await visit(resolved);
        } else if (dep.spec.startsWith('.')) warn(file,dep.node,ast,`Unresolved module ${dep.spec}`);
        else warn(file,dep.node,ast,`Unsupported module ${dep.spec}`);
      }
      if (!target) { target=path.join(sourceDir,`unsupported-${unsupported.size}.ts`); unsupported.set(target, dep.spec); }
      const replacement=relImport(targetFor(file),target);
      rewrites.push(`${slash(path.relative(root,file))}:${ast.getLineAndCharacterOfPosition(dep.node.getStart(ast)).line+1}: ${dep.spec} â†’ ${replacement}`);
      if(dep.isRequire) {const alias=`__tsgpui_import_${imports.length}`; imports.push(`import * as ${alias} from ${JSON.stringify(replacement)};`); edits.push([dep.node.getStart(ast),dep.node.end,alias]);}
      else edits.push([dep.specNode.getStart(ast),dep.specNode.end,JSON.stringify(replacement)]);
    }
    let converted=source;
    for (const [start,end,value] of edits.sort((a,b)=>b[0]-a[0])) converted=converted.slice(0,start)+value+converted.slice(end);
    // Node gives each module its own __dirname and __filename. The TS-GPU backend has no file system
    // layout, so define them per module as virtual paths that mirror the original project layout.
    // The backend also has no `process` global (it is absent in QuickJS),
    // so a bare `process` reference gets the compatibility shim.
    const virtualFile='/'+slash(path.relative(root,file));
    const definitions=[];
    if(/\b__dirname\b/.test(source)) definitions.push(`const __dirname = ${JSON.stringify(path.posix.dirname(virtualFile))};`);
    if(/\b__filename\b/.test(source)) definitions.push(`const __filename = ${JSON.stringify(virtualFile)};`);
    const providesProcess=dependencies.some(dep => builtins.get(dep.spec) === 'process');
    const declaresProcess=/\b(?:var|let|const)\s+process\b|\bfunction\s+process\b|\bclass\s+process\b/.test(source);
    if(/\bprocess\b/.test(source) && !providesProcess && !declaresProcess) {
      usedRuntime.add('process');
      const target=path.join(sourceDir,'tsgpu-runtime','process.ts');
      const replacement=relImport(targetFor(file),target);
      imports.push(`import * as __tsgpui_process from ${JSON.stringify(replacement)};`);
      definitions.push('const process = __tsgpui_process.default ?? __tsgpui_process;');
      rewrites.push(`${slash(path.relative(root,file))}: process â†’ ${replacement} (bare-global shim)`);
    }
    modules.set(file,imports.join('\n')+'\n'+(definitions.length ? definitions.join('\n')+'\n' : '')+converted);
  }
  await visit(main);
  if (!renderer) for (const candidate of ['dist/index.html','build/index.html','out/index.html','index.html','src/index.html','app/index.html','renderer/index.html','public/index.html']) if(await exists(path.join(root,candidate))) { addRenderer(path.join(root,candidate)); notes.push(`Renderer inferred from ${candidate}; verify the selected entry.`); break; }
  if (!renderer && remotePages.size) throw new Error(`This app only loads remote page(s) (${[...remotePages].join(', ')}). TS-GPU has no web view, so a remote site cannot be imported; only apps with a local HTML renderer can.`);
  if (!renderer || !inside(root,renderer) || !await exists(renderer)) throw new Error('No built renderer entry found; build the Electron app first or pass --build');
  for (const page of renderers) if (!inside(root,page) || !await exists(page)) throw new Error(`Renderer entry is missing or outside the project: ${page}`);
  const pagePreloads = new Map();
  for (const page of renderers) {
    const candidates = [...new Set([...(rendererWindows.get(page) || [])].map(id=>windowPreloads.get(id) || null))];
    if (candidates.length > 1) { notes.push(`Renderer ${slash(path.relative(root,page))} has multiple preload scripts; per-window association requires review.`); continue; }
    const preload = candidates[0];
    if (preload && inside(root,preload) && await exists(preload)) { pagePreloads.set(page,preload); await visit(preload); }
    else if (preload) notes.push(`Preload ${preload} was not found; migrate it manually.`);
  }
  await mkdir(sourceDir,{recursive:true});
  const rendererRoot=path.dirname(renderer);
  const rendererRoots = [...new Set([...renderers].map(page => path.dirname(page)))];
  const convertedRenderer = page => path.join(sourceDir, 'renderer', path.relative(root, page));
  for (const directory of rendererRoots) await cp(directory, path.join(sourceDir,'renderer',path.relative(root,directory)), {recursive:true,filter:p=>!inside(dest,path.resolve(p))});
  const htmlSource=await readFile(renderer,'utf8');
  const stylesheets=[];
  for (const match of htmlSource.replace(/<!--[\s\S]*?-->/g,'').matchAll(/<link\b[^>]*>/gi)) {
    const attrs=Object.fromEntries([...match[0].matchAll(/([^\s=<>"']+)\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s>]+))/g)].map(m=>[m[1].toLowerCase(),m[2] ?? m[3] ?? m[4]]));
    if (!attrs.rel?.split(/\s+/).some(x=>x.toLowerCase()==='stylesheet') || !attrs.href || /^(?:[a-z][a-z\d+.-]*:|\/\/|\/)/i.test(attrs.href)) continue;
    let relative;
    try { relative=decodeURIComponent(attrs.href.split(/[?#]/)[0]); } catch { continue; }
    const file=path.resolve(rendererRoot,relative);
    if (inside(rendererRoot,file) && (await stat(file).catch(()=>null))?.isFile()) {
      const target='src/renderer/'+slash(path.relative(root,file));
      if (!stylesheets.includes(target)) stylesheets.push(target);
    }
  }
  async function reviewRenderer(dir) {
    for(const item of await readdir(dir,{withFileTypes:true})) {
      const p=path.join(dir,item.name); if(item.isDirectory()) {await reviewRenderer(p); continue;}
      if(!item.isFile() || !/\.(?:js|mjs|cjs|html)$/.test(item.name)) continue;
      const content=await readFile(p,'utf8');
      for(const match of content.matchAll(/\b(?:require\s*\(\s*['"](?:electron|(?:node:)?(?:fs|path|os|events|child_process|process))|from\s*['"](?:electron|(?:node:)?(?:fs|path|os|events|child_process|process)))/g)) {
        const line=content.slice(0,match.index).split('\n').length;
        notes.push(`${slash(path.relative(root,p))}:${line}: Renderer Node/Electron import requires migration or bundling for the TS-GPU renderer.`);
      }
    }
  }
  for (const directory of rendererRoots) await reviewRenderer(directory);
  for (const [file,code] of modules) {const target=targetFor(file); await mkdir(path.dirname(target),{recursive:true}); await writeFile(target,code);}
  for (const [file, spec] of unsupported) await writeFile(file, `throw new Error(${JSON.stringify(`Unsupported module: ${spec}`)});\nexport default {};\n`);
  await mkdir(path.join(sourceDir,'tsgpu-runtime'),{recursive:true});
  const runtimeFiles=await readdir(runtimeDir).catch(()=>[]);
  // Include any tsgpu-runtime specifiers introduced in the converted bundles.
  for (const [, code] of modules) for (const m of code.matchAll(/tsgpu-runtime\/([\w-]+)/g)) usedRuntime.add(m[1]);
  // Expand to transitive runtime deps (e.g. electron -> host/os/events, fs -> host/os/path).
  let expanded = true;
  while (expanded) {
    expanded = false;
    for (const name of [...usedRuntime]) {
      let content;
      try { content = await readFile(path.join(runtimeDir, name + '.ts'), 'utf8'); } catch { continue; }
      for (const m of content.matchAll(/['"]\.\/([\w-]+)['"]/g)) {
        if (!usedRuntime.has(m[1])) { usedRuntime.add(m[1]); expanded = true; }
      }
    }
  }
  for (const file of runtimeFiles.filter(f=>f.endsWith('.ts'))) {
    const name = file.slice(0, -3);
    if (!usedRuntime.has(name)) continue;
    await cp(path.join(runtimeDir,file),path.join(sourceDir,'tsgpu-runtime',file));
  }
  for (const name of usedRuntime) if(!runtimeFiles.includes(name + '.ts')) notes.push(`Compatibility runtime missing ${name}.ts; copy it into src/tsgpu-runtime before building.`);
  await writeFile(path.join(sourceDir,'backend.ts'),`import ${JSON.stringify(relImport(path.join(sourceDir,'backend.ts'),targetFor(main)))};\nexport default { state: {}, actions: {} };\n`);
  // Adapt every copied renderer page to the HTML subset the Rendrix compiler accepts: strip the
  // document wrappers, drop <script>/<style>/<meta>/<link> (the backend and the CSS file carry
  // that content) and replace unsupported elements with neutral <div> containers. Image srcs are
  // rewritten to the copied assets, because the compiler resolves them from the project root.
  let pageTitle = null;
  const rendererCopyRoot = path.join(sourceDir,'renderer');
  async function adaptRendererPages(dir) {
    for (const item of await readdir(dir,{withFileTypes:true})) {
      const pageFile=path.join(dir,item.name);
      if (item.isDirectory()) { await adaptRendererPages(pageFile); continue; }
      if (!item.isFile() || !/\.(?:html|htm)$/i.test(item.name)) continue;
      const resolveImg = value => {
        let decoded;
        try { decoded = decodeURIComponent(value.split(/[?#]/)[0]); } catch { return null; }
        if (!decoded) return null;
        const candidates = /^[\\/]/.test(decoded)
          ? [path.join(rendererCopyRoot, decoded.replace(/^[\\/]+/, ''))]
          : [path.resolve(path.dirname(pageFile), decoded)];
        for (const candidate of candidates) {
          if (inside(rendererCopyRoot, candidate) && existsSync(candidate) && statSync(candidate).isFile()) {
            return 'src/renderer/' + slash(path.relative(rendererCopyRoot, candidate));
          }
        }
        return null;
      };
      const adapted=renderrizePage(await readFile(pageFile,'utf8'), resolveImg);
      await writeFile(pageFile,adapted.html);
      if (path.resolve(pageFile) === path.resolve(convertedRenderer(renderer)) && adapted.pageTitle) pageTitle=adapted.pageTitle;
      for (const note of adapted.notes) notes.push(`${slash('src/renderer/'+path.relative(rendererCopyRoot,pageFile))}: ${note}`);
    }
  }
  await adaptRendererPages(path.join(sourceDir,'renderer'));
  const preloadNotes=[];
  for (const [page,preload] of pagePreloads) {
    preloadNotes.push(`${slash(path.relative(root,page))}: preload ${slash(path.relative(root,preload))} was detected, but Electron preload scripts have no equivalent in the Rendrix renderer; move the preload logic into backend.ts actions.`);
  }
  for (const [id,preload] of windowPreloads) if (![...pagePreloads.values()].includes(preload)) notes.push(`Preload ${slash(path.relative(root,preload))} could not be associated with one static window page (${id.split('\0').at(-1)}); review manually.`);
  const preloadNote=preloadNotes.length ? preloadNotes.join('\n- ') : 'No statically associated preload was detected.';
  const name=(pkg.name || 'ImportedApp').replace(/[^a-zA-Z0-9_-]/g,'-').replace(/^[^a-zA-Z]+/,'') .slice(0,64) || 'ImportedApp';
  const version=/^\d{1,4}\.\d{1,4}\.\d{1,4}$/.test(pkg.version || '') ? pkg.version : '1.0.0';
  if(version!==pkg.version) notes.push(`Package version ${pkg.version || '(missing)'} was normalized to ${version}.`);
  const permissions={filesystem:{read:usage.relativeFs?['./data']:[],write:usage.relativeFs?['./data']:[]},processes:[...commands].sort(),network:[...hosts].sort(),notifications:usage.notifications,window:usage.window,ipc:usage.ipc};
  const css=stylesheets[0] || 'src/import.css';
  if (!stylesheets.length) await writeFile(path.join(sourceDir,'import.css'),'');
  const bundleId = typeof pkg.build?.appId === 'string' && pkg.build.appId ? pkg.build.appId : undefined;
  const outDir = typeof pkg.build?.directories?.output === 'string' && pkg.build.directories.output ? pkg.build.directories.output : undefined;
  const winBuild = pkg.build?.win && typeof pkg.build.win === 'object' ? pkg.build.win : {};
  const certificateFile = typeof winBuild.certificateFile === 'string' && winBuild.certificateFile ? winBuild.certificateFile : undefined;
  const certificateSubjectName = typeof winBuild.certificateSubjectName === 'string' && winBuild.certificateSubjectName ? winBuild.certificateSubjectName : undefined;
  const publishRaw = pkg.build?.publish;
  let publishUrl;
  if (typeof publishRaw === 'string' && publishRaw) publishUrl = publishRaw;
  else if (Array.isArray(publishRaw)) {
    for (const entry of publishRaw) {
      if (typeof entry === 'string' && entry) { publishUrl = entry; break; }
      if (entry && typeof entry === 'object') {
        if (typeof entry.url === 'string' && entry.url) { publishUrl = entry.url; break; }
        if (entry.provider === 'github' && entry.owner && entry.repo) { publishUrl = `https://github.com/${entry.owner}/${entry.repo}`; break; }
        if (typeof entry.provider === 'string' && entry.provider) { publishUrl = `(${entry.provider})`; break; }
      }
    }
  } else if (publishRaw && typeof publishRaw === 'object') {
    if (typeof publishRaw.url === 'string' && publishRaw.url) publishUrl = publishRaw.url;
    else if (publishRaw.provider === 'github' && publishRaw.owner && publishRaw.repo) publishUrl = `https://github.com/${publishRaw.owner}/${publishRaw.repo}`;
    else if (typeof publishRaw.provider === 'string' && publishRaw.provider) publishUrl = `(${publishRaw.provider})`;
  }
  const autoUpdaterStatus = statusOf('autoUpdater') || {status: 'implemented', reason: 'checks a configured update feed, emits update-available or update-not-available, has the host download and SHA-256-verify the artifact into userData/updates, and swaps it over the running executable on quitAndInstall (relaunch on request)'};
  const packaging = pkg.build ? {installer: 'nsis', signCmd: 'TSGPU_SIGN_COMMAND', signCommandEnv: 'TSGPU_SIGN_COMMAND', manifestKeyEnv: 'TSGPU_MANIFEST_KEY', certificateSubjectName: certificateSubjectName ?? null, publishUrl: publishUrl ?? null} : undefined;
  for (const note of sessionNotes) notes.push(note);
  if (sessionPersist) notes.push(`Session/partition detected: set "persist": true in tsgpu.config.json so backend state and cookies survive restarts (host 'session.cookies' falls back to an in-memory Map).`);
  for (const note of protocolNotes) notes.push(note);
  if (protocolSchemes.size) notes.push(`Custom protocols allowlisted (${[...protocolSchemes].sort().join(', ')}): rewritten to packaged app assets by the protocol shim; review handlers that compute paths dynamically.`);
  // Converted apps run on the Rendrix host: no renderer key (absent = "rendrix"), a primary backend
  // window, and the first renderer page as the packaged entry. Extra windows remain a review item.
  const config={name,title:title || pkg.build?.productName || pkg.productName || pageTitle || pkg.name || name,version,...(bundleId ? {bundleId} : {}),...(outDir ? {outDir} : {}),...(packaging ? {packaging} : {}),...(sessionPersist ? {persist:true} : {}),...(protocolSchemes.size ? {protocolSchemes:[...protocolSchemes].sort()} : {}),window:{width,height},html:'src/renderer/'+slash(path.relative(root,renderer)),css,backend:'src/backend.ts',permissions};
  await writeFile(path.join(dest,'tsgpu.config.json'),JSON.stringify(config,null,2)+'\n');
  const fsSection = filesystemPaths.length ? `\n## Filesystem paths\n\n${filesystemPaths.map(x=>'- '+x).join('\n')}\n` : (usage.fs ? `\n## Filesystem paths\n\n- No auto-granted filesystem paths were detected; review filesystem call sites manually.\n` : '');
  const fsFooter = usage.relativeFs ? 'The ./data filesystem root is a placeholder for detected relative paths; review original paths and access.\n' : (usage.fs && !fsNeedsReview && filesystemPaths.length ? 'All detected filesystem paths are auto-granted (userData/dialog-selected); no explicit permissions.filesystem entry was added. Review other filesystem paths.\n' : (usage.fs ? 'No ./data root was granted; userData and dialog-selected files are auto-granted. Review other filesystem paths.\n' : ''));
  const webPrefsNote = `Original webPreferences: contextIsolation ${contextIsolation ?? '(not specified)'}, nodeIntegration ${nodeIntegration ?? '(not specified)'}, sandbox ${webviewSandbox ?? '(not specified)'}.`;
  const builderFixes = [];
  if (pkg.build?.win?.target) builderFixes.push(`run \`tsgpu package --installer nsis\` for win.target ${pkg.build.win.target}`);
  if (pkg.build) {
    builderFixes.push(`run \`tsgpu package --installer nsis --sign\` with TSGPU_SIGN_COMMAND=<your sign command> (win.certificateFile ${certificateFile ?? '(unset)'} and certificateSubjectName ${certificateSubjectName ?? '(unset)'} map to TSGPU_SIGN_COMMAND; keep the certificate and password outside the repo) and TSGPU_MANIFEST_KEY=<path to Ed25519 key>`);
    builderFixes.push(`publish manually: upload the signed .exe, setup .exe and .manifest.json (plus .manifest.json.sig when signed) to publishUrl ${publishUrl ?? '(unset)'}; TS-GPU has no auto-publish`);
    builderFixes.push(`autoUpdater is ${autoUpdaterStatus.status}: ${autoUpdaterStatus.reason}; configure its feed URL with autoUpdater.setFeedURL(...) and publish the signed installer plus .manifest.json to packaging.publishUrl`);
  }
  const builderNote = pkg.build ? `Original electron-builder configuration: present (appId ${pkg.build.appId ?? '(unset)'}, productName ${pkg.build.productName ?? pkg.productName ?? '(unset)'}, directories.output ${pkg.build.directories?.output ?? '(unset)'}, win.target ${pkg.build.win?.target ?? '(unset)'}, win.certificateFile ${certificateFile ?? '(unset)'} -> TSGPU_SIGN_COMMAND, certificateSubjectName ${certificateSubjectName ?? '(unset)'}, publishUrl ${publishUrl ?? '(unset)'}, autoUpdater ${autoUpdaterStatus.status} â€” ${autoUpdaterStatus.reason}) . Auto-mapped into tsgpu.config.json: appId -> bundleId (${bundleId ?? 'default'}), directories.output -> outDir (${outDir ?? 'default'}), productName -> title fallback, packaging section -> signCmd (TSGPU_SIGN_COMMAND) / publishUrl (${publishUrl ?? '(unset)'}) / certificateSubjectName (${certificateSubjectName ?? '(unset)'}). Signing, publishing and auto-update require manual migration: produce the Windows installer with \`tsgpu package --installer nsis --sign\`, verify the .exe/manifest identity, and migrate signing/publishing/auto-update by hand. Manual steps: ${builderFixes.length ? builderFixes.join('; ') + '. ' : 'None. '}Re-run with \`tsgpu import electron ${root} --out ${dest} --fix\` to preview fixes (add \`--force\` to overwrite an existing output).` : `Original electron-builder configuration: not present. If you add one, map appId to "bundleId" in tsgpu.config.json and directories.output to "outDir"; preview with \`tsgpu import electron ${root} --out ${dest} --fix\`.`;
  const forgeNote = pkg.config?.forge ? `Original Electron Forge configuration: present but not converted; translate makers/publishers to \`tsgpu build\`/\`tsgpu package\` by hand. Fix: run \`tsgpu import electron ${root} --out ${dest} --fix\` to preview fixes, then run \`tsgpu build\` in ${dest}.` : `Original Electron Forge configuration: not present. If you add one, translate makers/publishers to \`tsgpu build\`/\`tsgpu package\`; preview with \`tsgpu import electron ${root} --out ${dest} --fix\`.`;
  const report=`# Electron import report\n\nSource: ${root}\n\n## Stylesheets\n\nConfig css: ${css}${stylesheets.length ? '' : ' (empty placeholder because no linked local stylesheet was found)'}.\n\nCopied linked stylesheets:\n${stylesheets.map(x=>'- '+x).join('\n') || '- None'}\n\n## Rewritten specifiers\n\n${rewrites.map(x=>'- '+x).join('\n') || '- None'}\n\n## Unsupported and review items\n\n${notes.map(x=>'- '+x).join('\n') || '- None detected; review dynamic code and Electron API usage.'}\n${fsSection}\n## Derived permissions â€” review every grant\n\n\`\`\`json\n${JSON.stringify(permissions,null,2)}\n\`\`\`\n\n${fsFooter}### Permission review checklist (all items required before \`tsgpu build\`)\n\nRe-run with \`tsgpu import electron ${root} --out ${dest} --fix\` to preview automatic fixes (add \`--force\` to overwrite an existing output). Edit tsgpu.config.json in ${dest}, then run \`tsgpu build\` in ${dest}.\n\n- [ ] \`permissions.network\` â€” every host is an exact hostname (no wildcard/subdomain/port/path). Derived hosts: ${permissions.network.length ? permissions.network.join(', ') : '(none)'}. Confirm each host against https.get/fetch/axios/data-fetch call sites and the renderer CSP; remove unneeded hosts and add any host the importer missed. Fix: edit "permissions.network" in ${dest}/tsgpu.config.json (e.g. ${JSON.stringify(permissions.network)}), then run \`tsgpu build\` in ${dest}. Preview fixes with \`tsgpu import electron ${root} --out ${dest} --fix\`.\n- [ ] \`permissions.notifications\` â€” is ${permissions.notifications}. Confirm a desktop notification is actually shown; the host may report notifications as unsupported, so test at runtime. Fix: set "permissions.notifications" in ${dest}/tsgpu.config.json, then run \`tsgpu build\` in ${dest} to test.\n- [ ] \`permissions.window\` â€” is ${permissions.window}. Confirm window creation/management use; check titleBarStyle and hidden show/hide/fullscreen/window-move uses the partial shim does not support. Fix: set "permissions.window" in ${dest}/tsgpu.config.json, then run \`tsgpu build\` in ${dest}.\n- [ ] \`permissions.ipc\` â€” is ${permissions.ipc}. Grant is boolean (channels cannot be allowlisted); enumerate every ipcMain/ipcRenderer channel and confirm each has a preload bridge; remove dead channels if any. Fix: set "permissions.ipc" in ${dest}/tsgpu.config.json, then run \`tsgpu build\` in ${dest}.\n- [ ] \`permissions.filesystem\` â€” read ${JSON.stringify(permissions.filesystem.read)}, write ${JSON.stringify(permissions.filesystem.write)}. Empty is correct only if every file call is userData-based or dialog-selected (auto-granted, see Filesystem paths section); otherwise add the real relative root and re-run build. Fix: edit "permissions.filesystem" (or "filesystem.read"/"filesystem.write") in ${dest}/tsgpu.config.json, then run \`tsgpu build\` in ${dest}.\n- [ ] \`permissions.processes\` â€” derived executables: ${permissions.processes.length ? permissions.processes.join(', ') : '(none)'}. Confirm no executable is spawned, or list executable names only. Fix: edit "permissions.processes" in ${dest}/tsgpu.config.json, then run \`tsgpu build\` in ${dest}.\n- [ ] Final pass: search for calls the pattern-scan can miss (aliased fetch/https.get/spawn/fs/Notification/BrowserWindow/ipcMain) and compare against tsgpu.config.json permissions. Fix: run \`tsgpu import electron ${root} --out ${dest} --fix\` to preview fixes, then run \`tsgpu build\` in ${dest}.\n\n## Manual steps\n\n- ${preloadNote}\n- [ ] Preload isolation: the bundled preload runs in the page JS realm before renderer scripts, not an isolated world. Confirm it only uses contextBridge/ipcRenderer and the exposed window.api surface â€” no direct DOM/page-global mutation and no reliance on isolated-world security or pre-DOM timing.\n- [ ] Preload Node/Electron APIs: verify no main-only APIs (ipcMain, app, BrowserWindow, dialog, fs, path, ...) are used in preload scope; move such logic to the backend.\n- [ ] Preloadâ†’backend channel contract: confirm every invoke/send channel in preload is still registered in the converted backend, and the page does not already define the exposed key.\n- [ ] ${webPrefsNote}\n- Review Electron APIs, renderer Node integration, dynamic paths, package dependencies, build/Forge settings, and permissions.\n- Original titleBarStyle: ${titleBarStyle || '(not specified)'}. Check any show/hide/fullscreen/window-move event use; BrowserWindow is partial and does not support show/hide/ready-to-show/fullscreen events or will-move/will-resize.\n- ${builderNote}\n- ${forgeNote}\n- Run \`tsgpu build\` in this directory. Fix: re-run \`tsgpu import electron ${root} --out ${dest} --fix\` for preview fixes, then \`tsgpu build\` in ${dest}.\n`;
  await writeFile(path.join(dest,'TSGPU_IMPORT_REPORT.md'),report);
  console.log(`Imported Electron project to ${dest}. Review TSGPU_IMPORT_REPORT.md.`);
  return {out:dest,config,report};
}
