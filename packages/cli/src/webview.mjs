import { readFile, realpath, lstat } from 'node:fs/promises';
import path from 'node:path';
import { randomBytes } from 'node:crypto';

const MIME = {
  '.avif':'image/avif','.bmp':'image/bmp','.css':'text/css','.gif':'image/gif','.htm':'text/html','.html':'text/html','.ico':'image/x-icon',
  '.jpeg':'image/jpeg','.jpg':'image/jpeg','.js':'text/javascript','.json':'application/json',
  '.mjs':'text/javascript','.mp3':'audio/mpeg','.mp4':'video/mp4','.otf':'font/otf','.pdf':'application/pdf',
  '.png':'image/png','.svg':'image/svg+xml','.ttf':'font/ttf','.wasm':'application/wasm',
  '.webm':'video/webm','.webp':'image/webp','.woff':'font/woff','.woff2':'font/woff2'
};

async function assertRealInside(projectRoot, file, label) {
  const actualRoot = await realpath(projectRoot);
  let probe = file;
  for (;;) {
    try {
      const actual = await realpath(probe);
      if (actual !== actualRoot && !actual.startsWith(actualRoot + path.sep)) throw new Error(`${label} resolves outside the project: ${path.relative(projectRoot,file)}`);
      return;
    } catch (error) {
      if (error.code !== 'ENOENT') throw error;
      if ((await lstat(probe).catch(() => null))?.isSymbolicLink()) throw new Error(`${label} is a dangling symlink: ${path.relative(projectRoot,file)}`);
      const parent = path.dirname(probe);
      if (parent === probe) throw error;
      probe = parent;
    }
  }
}

function webviewRuntime(store, savedState) {
  const app = globalThis.__tsgpui_app;
  if (!app || !app.state || !app.actions) throw new Error('Backend must export { state, actions }');
  const has = (object, key) => object !== null && typeof object === 'object' && Object.prototype.hasOwnProperty.call(object, key);
  const asText = value => value === null || value === undefined ? '' : typeof value === 'string' ? value : JSON.stringify(value);
  const report = error => console.error('TSGPUI WebView: ' + ((error && error.message) || error));
  const networkHosts = new Set(globalThis.__tsgpui_network_hosts ?? []);
  // Keep user-changed values across a reload (session) or across restarts (persisted projects).
  // The merge mirrors the native host: a value the user changed is kept when the new script still
  // declares that key with the same kind of value.
  const mergeSaved = (fresh, seed, current) => {
    if (JSON.stringify(seed) === JSON.stringify(current)) return fresh;
    if (Array.isArray(fresh) && Array.isArray(current)) return current;
    if (fresh !== null && typeof fresh === 'object' && current !== null && typeof current === 'object'
        && !Array.isArray(fresh) && !Array.isArray(current)) {
      const merged = {};
      for (const key of Object.keys(fresh)) {
        merged[key] = has(current,key) ? mergeSaved(fresh[key], seed && has(seed,key) ? seed[key] : undefined, current[key]) : fresh[key];
      }
      return merged;
    }
    // A shape change (object to array, or array to object) keeps the new default.
    if (Array.isArray(fresh) !== Array.isArray(current)) return fresh;
    return typeof fresh === typeof current ? current : fresh;
  };
  // The saved record keeps the defaults of the script that wrote it, so the merge can tell a value the
  // user changed from one still at its old default, as the native host does. A record without those
  // defaults comes from an older build, and every saved value is kept, as before.
  const defaults = JSON.parse(JSON.stringify(app.state));
  const record = savedState && typeof savedState === 'object' ? savedState : null;
  let saved = null;
  if (record && has(record, '__tsgpui_state')) saved = mergeSaved(app.state, has(record, '__tsgpui_seed') ? record.__tsgpui_seed : app.state, record.__tsgpui_state);
  else if (record) saved = mergeSaved(app.state, app.state, record);
  if (saved) app.state = saved;
  const saveState = () => {
    if (!store) return;
    try { store.setItem('tsgpui:state', JSON.stringify({__tsgpui_seed: defaults, __tsgpui_state: app.state})); } catch (_error) { /* quota or disabled storage */ }
  };
  // The unload handler in the bootstrap saves through this, so both paths write the same record.
  globalThis.__tsgpui_save = saveState;
  const getPath = key => key.split('.').reduce((value, part) => {
    if (Array.isArray(value)) return /^\d+$/.test(part) ? value[Number(part)] : undefined;
    return value !== null && typeof value === 'object' ? value[part] : undefined;
  }, app.state);
  const setPath = (key, value) => {
    const parts = key.split('.');
    let target = app.state;
    for (const part of parts.slice(0, -1)) {
      const next = Array.isArray(target) ? (/^\d+$/.test(part) ? target[Number(part)] : undefined) : target?.[part];
      if (next === undefined) throw new Error('Unknown state key: ' + key);
      target = next;
    }
    const last = parts.at(-1);
    if (Array.isArray(target)) {
      if (!/^\d+$/.test(last) || Number(last) >= target.length) throw new Error('Unknown state key: ' + key);
      target[Number(last)] = value;
    } else if (target !== null && typeof target === 'object' && has(target, last)) target[last] = value;
    else throw new Error('Unknown state key: ' + key);
  };
  const api = Object.freeze({
    get: key => app.state[key],
    set: (key, value) => { if (!has(app.state, key)) throw new Error('Unknown state key: ' + key); app.state[key] = value; },
    update: (key, modify) => { if (!has(app.state, key)) throw new Error('Unknown state key: ' + key); app.state[key] = modify(app.state[key]); }
  });
  const invoke = (name, event) => {
    if (!has(app.actions, name) || typeof app.actions[name] !== 'function') { report(new Error('No action: ' + name)); return; }
    const before = JSON.stringify(app.state);
    const finish = error => {
      if (error) { app.state = JSON.parse(before); report(error); }
      render();
    };
    try {
      const result = app.actions[name](api, Object.freeze(event));
      if (result && typeof result.then === 'function') result.then(() => finish(null), finish);
      else finish(null);
    } catch (error) { finish(error); }
  };
  const textSources = new WeakMap();
  const interpolate = text => text.replace(/\{\{\s*([A-Za-z_$][\w$]*(?:\.(?:[A-Za-z_$][\w$]*|\d+))*)\s*\}\}/g,
    (_match, key) => asText(getPath(key)));
  function render() {
    document.title = document.documentElement.getAttribute('data-tsgpui-title') || document.title;
    const walker = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT);
    let node;
    while ((node = walker.nextNode())) {
      if (!textSources.has(node)) textSources.set(node, node.nodeValue);
      if (node.parentElement?.closest('script,style,textarea,[data-bind]')) continue;
      const source = textSources.get(node);
      if (source?.includes('{{')) node.nodeValue = interpolate(source);
    }
    for (const element of document.querySelectorAll('[data-bind]')) {
      const value = getPath(element.getAttribute('data-bind'));
      if (value === undefined) continue;
      if (element instanceof HTMLInputElement && element.type === 'checkbox') element.checked = value === true;
      else if (element instanceof HTMLInputElement || element instanceof HTMLTextAreaElement || element instanceof HTMLSelectElement) {
        const text = asText(value);
        if (element.value !== text) element.value = text;
      } else element.textContent = asText(value);
    }
    saveState();
  }
  const eventValue = element => element instanceof HTMLInputElement && element.type === 'checkbox'
    ? element.checked
    : element instanceof HTMLInputElement && element.type === 'number' && element.value.trim() !== '' && !Number.isNaN(Number(element.value))
      ? Number(element.value) : element.value;
  const formValid = form => [...form.querySelectorAll('input,textarea,select')].every(control => {
    if (control.required && (control.type === 'checkbox' ? !control.checked : control.value.trim() === '')) return false;
    if (control.type === 'number' && control.value.trim() !== '') {
      const value = Number(control.value);
      if (Number.isNaN(value) || control.min !== '' && value < Number(control.min) || control.max !== '' && value > Number(control.max)) return false;
      if (control.step && control.step !== 'any') {
        const base = control.min === '' ? 0 : Number(control.min), steps = (value - base) / Number(control.step);
        if (Math.abs(steps - Math.round(steps)) > 0.000001) return false;
      }
    }
    return true;
  });
  document.addEventListener('input', event => {
    const element = event.target;
    if (!(element instanceof HTMLElement)) return;
    if (element instanceof HTMLSelectElement || element instanceof HTMLInputElement && element.type === 'checkbox') return;
    const binding = element.getAttribute('data-bind');
    const action = element.getAttribute('data-oninput');
    if (binding) { try { setPath(binding, eventValue(element)); } catch (error) { report(error); } }
    if (action) invoke(action, {type:'input',value:eventValue(element),checked:element instanceof HTMLInputElement ? element.checked : undefined,name:element.getAttribute('name')});
    else render();
  });
  document.addEventListener('change', event => {
    const element = event.target;
    if (!(element instanceof HTMLSelectElement || element instanceof HTMLInputElement && element.type === 'checkbox')) return;
    if (!element.hasAttribute('data-bind') && !element.hasAttribute('data-oninput')) return;
    const binding = element.getAttribute('data-bind'), action = element.getAttribute('data-oninput');
    if (binding) { try { setPath(binding, eventValue(element)); } catch (error) { report(error); } }
    if (action) invoke(action, {type:'input',value:eventValue(element),checked:element instanceof HTMLInputElement ? element.checked : undefined,name:element.getAttribute('name')});
    else render();
  });
  document.addEventListener('submit', event => {
    const form = event.target;
    if (!(form instanceof HTMLFormElement) || !form.hasAttribute('data-submit')) return;
    event.preventDefault();
    if (!formValid(form)) { form.reportValidity(); return; }
    invoke(form.getAttribute('data-submit'), {type:'submit',submit:true});
  });
  document.addEventListener('click', event => {
    const button = event.target instanceof Element ? event.target.closest('button,[data-onclick]') : null;
    if (!button || button.disabled) return;
    const action = button.getAttribute('data-onclick');
    const confirmAction = button.getAttribute('data-confirm');
    if (confirmAction) {
      if (window.confirm(button.getAttribute('title') || 'Continue?')) invoke(action || confirmAction, {type:'confirm'});
      event.preventDefault(); return;
    }
    if (action) { invoke(action, {type:'click'}); if (button.type === 'submit') event.preventDefault(); }
    const copyKey = button.getAttribute('data-clipboard-copy');
    if (copyKey) { event.preventDefault(); navigator.clipboard?.writeText(asText(getPath(copyKey))).catch(report); }
    const pasteKey = button.getAttribute('data-clipboard-paste');
    if (pasteKey) { event.preventDefault(); navigator.clipboard?.readText().then(text => { setPath(pasteKey, text); render(); }).catch(report); }
    const fileKey = button.getAttribute('data-file-open');
    if (fileKey) {
      event.preventDefault();
      const picker = document.createElement('input');
      picker.type = 'file';
      picker.addEventListener('change', async () => {
        const file = picker.files?.[0];
        if (!file) return;
        setPath(fileKey,file.name);
        const readKey = button.getAttribute('data-file-read');
        if (readKey) {
          if (file.size > 1024 * 1024) { report(new Error('Text files must be at most 1 MiB')); return; }
          setPath(readKey,await file.text());
        }
        render();
      });
      picker.click();
    }
    const saveKey = button.getAttribute('data-file-save');
    if (saveKey) {
      event.preventDefault();
      const contents = asText(getPath(saveKey));
      if (new TextEncoder().encode(contents).length > 1024 * 1024) { report(new Error('Text files must be at most 1 MiB')); return; }
      const link = document.createElement('a');
      link.href = URL.createObjectURL(new Blob([contents],{type:'text/plain'}));
      link.download = button.getAttribute('title') || 'document.txt';
      link.click();
      setTimeout(() => URL.revokeObjectURL(link.href),1000);
    }
    const fetchUrl = button.getAttribute('data-fetch');
    if (fetchUrl) {
      event.preventDefault();
      const target = button.getAttribute('data-fetch-into');
      if (!target) { report(new Error('data-fetch requires data-fetch-into')); return; }
      const method = (button.getAttribute('data-fetch-method') || 'GET').toUpperCase();
      if (method !== 'GET' && method !== 'POST') { report(new Error('data-fetch-method must be GET or POST')); return; }
      const bodyKey = button.getAttribute('data-fetch-body');
      const contentType = button.getAttribute('data-fetch-content-type');
      const statusKey = button.getAttribute('data-fetch-status-into');
      const errorKey = button.getAttribute('data-fetch-error-into');
      let url;
      try { url = new URL(fetchUrl); } catch (_error) { report(new Error('Invalid fetch URL')); return; }
      if (url.protocol !== 'https:' || url.port && url.port !== '443' || url.username || url.password || url.hash || !networkHosts.has(url.hostname.toLowerCase())) {
        report(new Error('Network host is not permitted: ' + fetchUrl)); return;
      }
      const setKey = (key, value) => {
        if (!key) return;
        try { setPath(key, value); } catch (error) { report(error); }
      };
      const controller = new AbortController(), timer = setTimeout(() => controller.abort(),15000);
      const init = {method, redirect:'error', credentials:'omit', signal:controller.signal};
      if (method === 'POST') {
        let body = '';
        if (bodyKey) {
          const value = getPath(bodyKey);
          body = typeof value === 'string' ? value : JSON.stringify(value ?? null);
        }
        if (new TextEncoder().encode(body).length > 1024 * 1024) { report(new Error('Network request body exceeds 1 MiB')); clearTimeout(timer); return; }
        init.body = body;
        if (contentType) init.headers = {'Content-Type': contentType};
      }
      fetch(url.href, init).then(response => {
        setKey(statusKey, response.status);
        if (!response.ok) throw new Error('HTTP status ' + response.status);
        if (Number(response.headers.get('content-length')) > 1024 * 1024) throw new Error('Network response exceeds 1 MiB');
        return response.text();
      }).then(text => {
        if (new TextEncoder().encode(text).length > 1024 * 1024) throw new Error('Network response exceeds 1 MiB');
        setKey(target, text); setKey(errorKey, ''); render();
      }).catch(error => {
        setKey(errorKey, String((error && error.message) || error)); render();
        report(error);
      }).finally(() => clearTimeout(timer));
    }
  });
  render();
}

function safeJson(value) {
  return JSON.stringify(value).replace(/</g,'\\u003c').replace(/>/g,'\\u003e').replace(/&/g,'\\u0026').replace(/\u2028/g,'\\u2028').replace(/\u2029/g,'\\u2029');
}

function findHtmlTag(source, name, closing = false) {
  const rawTextTags = new Set(['script','style','textarea','title']);
  let cursor = 0;
  while (cursor < source.length) {
    const start = source.indexOf('<',cursor);
    if (start < 0) return null;
    if (source.startsWith('<!--',start)) {
      const end = source.indexOf('-->',start + 4);
      cursor = end < 0 ? source.length : end + 3;
      continue;
    }
    const token = /^<(\/)?\s*([A-Za-z][A-Za-z0-9-]*)\b[^>]*>/.exec(source.slice(start));
    if (!token) { cursor = start + 1; continue; }
    const isClosing = Boolean(token[1]), tag = token[2].toLowerCase();
    if (tag === name && isClosing === closing) return {index:start,length:token[0].length};
    if (!isClosing && rawTextTags.has(tag) && !token[0].endsWith('/>')) {
      const endPattern = new RegExp(`<\\/\\s*${tag}\\s*>`,'ig');
      endPattern.lastIndex = start + token[0].length;
      const end = endPattern.exec(source);
      cursor = end ? end.index + end[0].length : source.length;
    } else cursor = start + token[0].length;
  }
  return null;
}

function escapeStyleEnd(css) { return css.replace(/<\/style/gi, '<\\/style'); }
function escapeAttribute(value) { return String(value).replace(/[&<>"']/g, character => ({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[character])); }

function dataUrl(reference) {
  return /^(?:data:|blob:|https?:|mailto:|tel:|javascript:|\/\/|#)/i.test(reference.trim());
}

function insideComment(source, index, open, close) {
  return source.lastIndexOf(open,index) > source.lastIndexOf(close,index);
}

async function embeddedUrl(reference, {projectRoot, baseFile, cache, watchedAssets}) {
  const raw = reference.trim();
  if (!raw || dataUrl(raw)) return reference;
  const suffixAt = raw.search(/[?#]/);
  const fileRef = suffixAt < 0 ? raw : raw.slice(0, suffixAt);
  const fragmentAt = raw.indexOf('#');
  const suffix = fragmentAt < 0 ? '' : raw.slice(fragmentAt);
  let decoded;
  try { decoded = decodeURIComponent(fileRef); } catch { throw new Error(`Invalid local asset URL: ${raw}`); }
  const rooted = /^[/\\]/.test(decoded);
  const relative = decoded.replace(/^[/\\]+/, '');
  const absolute = path.resolve(rooted ? projectRoot : path.dirname(baseFile), relative);
  const actualRoot = path.resolve(projectRoot) + path.sep;
  if (!absolute.startsWith(actualRoot)) throw new Error(`Local WebView asset is outside the project: ${raw}`);
  watchedAssets?.add(path.relative(projectRoot,absolute));
  let bytes = cache.get(absolute);
  if (!bytes) {
    await assertRealInside(projectRoot, absolute, 'WebView asset');
    bytes = await readFile(absolute);
    if (bytes.length > 8 * 1024 * 1024) throw new Error(`WebView asset exceeds 8 MiB: ${raw}`);
    cache.set(absolute, bytes);
  }
  const mime = MIME[path.extname(absolute).toLowerCase()];
  if (!mime) throw new Error(`Unsupported WebView asset type: ${raw}`);
  return `data:${mime};base64,${bytes.toString('base64')}${suffix}`;
}

async function inlineCssUrls(css, options, importStack = new Set([path.resolve(options.baseFile)])) {
  const imports = [...css.matchAll(/@import\s+(?:url\(\s*)?(?:(["'])(.*?)\1|([^)\s;]+))\s*\)?\s*([^;]*);/gi)]
    .filter(match => !insideComment(css,match.index,'/*','*/'));
  for (const match of imports.reverse()) {
    const reference = match[2] ?? match[3];
    if (!reference || dataUrl(reference)) continue;
    const suffixAt = reference.search(/[?#]/);
    const fileRef = suffixAt < 0 ? reference : reference.slice(0,suffixAt);
    let decoded;
    try { decoded = decodeURIComponent(fileRef); } catch { throw new Error(`Invalid local CSS import URL: ${reference}`); }
    const rooted = /^[/\\]/.test(decoded);
    const absolute = path.resolve(rooted ? options.projectRoot : path.dirname(options.baseFile),decoded.replace(/^[/\\]+/,''));
    if (!absolute.startsWith(path.resolve(options.projectRoot) + path.sep)) throw new Error(`Local CSS import is outside the project: ${reference}`);
    options.watchedAssets?.add(path.relative(options.projectRoot,absolute));
    if (importStack.has(absolute)) throw new Error(`Circular local CSS import: ${reference}`);
    await assertRealInside(options.projectRoot,absolute,'WebView stylesheet');
    let bytes = options.cache.get(absolute);
    if (!bytes) {
      bytes = await readFile(absolute);
      if (bytes.length > 8 * 1024 * 1024) throw new Error(`WebView stylesheet exceeds 8 MiB: ${reference}`);
      options.cache.set(absolute,bytes);
    }
    const nextStack = new Set(importStack);
    nextStack.add(absolute);
    const imported = await inlineCssUrls(bytes.toString('utf8'),{...options,baseFile:absolute},nextStack);
    const media = match[4].trim();
    const replacement = media ? `@media ${media}{${imported}}` : imported;
    css = css.slice(0,match.index) + replacement + css.slice(match.index + match[0].length);
  }
  const replacements = [];
  const regex = /url\(\s*(['"]?)(.*?)\1\s*\)/gi;
  for (const match of css.matchAll(regex)) {
    if (insideComment(css,match.index,'/*','*/')) continue;
    replacements.push([match.index, match[0].length, `url("${await embeddedUrl(match[2], options)}")`]);
  }
  for (const [index, length, value] of replacements.reverse()) css = css.slice(0,index) + value + css.slice(index + length);
  return css;
}

async function inlineHtmlUrls(html, options) {
  const pattern = /\b(src|poster|xlink:href|style)\s*=\s*(["'])(.*?)\2/gi;
  const matches = [...html.matchAll(pattern)].filter(match => !insideComment(html,match.index,'<!--','-->'));
  for (const match of matches.reverse()) {
    const start = match.index + match[0].indexOf(match[3]);
    const value = match[1].toLowerCase() === 'style'
      ? await inlineCssUrls(match[3],options)
      : await embeddedUrl(match[3], options);
    html = html.slice(0,start) + escapeAttribute(value) + html.slice(start + match[3].length);
  }
  return html;
}

/** Collect statically identifiable local references, following CSS and JS dependencies. */
export async function collectWebviewFiles(html, css, {projectRoot, htmlFile, cssFile, watchedAssets} = {}) {
  const files = {};
  const seen = new Set();
  const local = value => value && !/^(?:[a-z][a-z\d+.-]*:|\/\/|#)/i.test(value);
  async function visit(value, base) {
    if (!local(value)) return;
    let decoded;
    try { decoded = decodeURIComponent(value.split(/[?#]/, 1)[0]); } catch { throw new Error(`Invalid WebView URL: ${value}`); }
    if (!decoded) return;
    const root = path.resolve(projectRoot);
    const file = path.resolve(decoded.startsWith('/') ? root : path.dirname(base), decoded.replace(/^\/+/, ''));
    if (!file.startsWith(root + path.sep)) throw new Error(`Local WebView asset is outside the project: ${value}`);
    await assertRealInside(root, file, 'WebView asset');
    if (seen.has(file)) return;
    seen.add(file);
    const bytes = await readFile(file);
    const key = '/' + path.relative(root, file).split(path.sep).join('/');
    watchedAssets?.add(path.relative(root, file));
    const ext = path.extname(file).toLowerCase();
    files[key] = {mime:MIME[ext] ?? 'application/octet-stream', data:bytes.toString('base64')};
    if (ext === '.css' || ext === '.js' || ext === '.mjs') await scan(bytes.toString('utf8'), file, ext);
  }
  async function scan(source, base, type) {
    const refs = [];
    if (type === 'html') {
      for (const tag of source.matchAll(/<(?:script|link|img|source|video|audio|iframe)\b[^>]*>/gi)) {
        for (const attr of tag[0].matchAll(/\b(?:src|href|poster)\s*=\s*["']([^"']+)["']/gi)) refs.push(attr[1]);
        for (const attr of tag[0].matchAll(/\bsrcset\s*=\s*["']([^"']+)["']/gi)) refs.push(...attr[1].split(',').map(item => item.trim().split(/\s+/)[0]));
      }
      for (const style of source.matchAll(/<style\b[^>]*>([\s\S]*?)<\/style\s*>|\bstyle\s*=\s*["']([^"']*)["']/gi)) await scan(style[1] ?? style[2], base, '.css');
      for (const script of source.matchAll(/<script\b[^>]*>([\s\S]*?)<\/script\s*>/gi)) await scan(script[1], base, '.js');
    } else if (type === '.css') {
      for (const match of source.matchAll(/url\(\s*["']?([^"'\s)]+)["']?\s*\)|@import\s*["']([^"']+)["']/gi)) refs.push(match[1] ?? match[2]);
    } else {
      for (const match of source.matchAll(/\b(?:import\s*(?:\(\s*)?|export\s+[^;]*?\s+from\s+|import\s+[^;]*?\s+from\s+)["']([^"']+)["']/g)) {
        if (match[1].startsWith('.') || match[1].startsWith('/')) refs.push(match[1]);
      }
    }
    for (const ref of refs) await visit(ref, base);
  }
  await scan(html, htmlFile, 'html');
  await scan(css, cssFile, '.css');
  return files;
}

/** Prepare the page while preserving local URLs for the app-local protocol. */
export async function prepareWebviewHtml(html, css, {backendJs, title, networkHosts = [], fonts = {}, persist = false, projectRoot, htmlFile, cssFile, watchedAssets, preserveUrls = false} = {}) {
  const cache = new Map();
  if (!preserveUrls && projectRoot && htmlFile) html = await inlineHtmlUrls(html,{projectRoot,baseFile:htmlFile,cache,watchedAssets});
  if (projectRoot && cssFile) css = await inlineCssUrls(css,{projectRoot,baseFile:cssFile,cache,watchedAssets});
  if (!preserveUrls && projectRoot && htmlFile) {
    const styleBlocks = [...html.matchAll(/<style\b([^>]*)>([\s\S]*?)<\/style\s*>/gi)]
      .filter(match => !insideComment(html,match.index,'<!--','-->'));
    for (const match of styleBlocks.reverse()) {
      const inline = await inlineCssUrls(match[2],{projectRoot,baseFile:htmlFile,cache,watchedAssets});
      html = html.slice(0,match.index) + `<style${match[1]}>${escapeStyleEnd(inline)}</style>` + html.slice(match.index + match[0].length);
    }
    const links = [...html.matchAll(/<link\b[^>]*>/gi)].filter(match => !insideComment(html,match.index,'<!--','-->'));
    for (const match of links.reverse()) {
      const rel = /\brel\s*=\s*(["'])(.*?)\1/i.exec(match[0])?.[2]?.toLowerCase().split(/\s+/) ?? [];
      const href = /\bhref\s*=\s*(["'])(.*?)\1/i.exec(match[0])?.[2];
      if (!href || !rel.includes('stylesheet') || dataUrl(href)) continue;
      const media = /\bmedia\s*=\s*(["'])(.*?)\1/i.exec(match[0])?.[2];
      const decoded = decodeURIComponent(href.split(/[?#]/,1)[0]);
      const file = path.resolve(/^[/\\]/.test(decoded) ? projectRoot : path.dirname(htmlFile),decoded.replace(/^[/\\]+/,''));
      if (!file.startsWith(path.resolve(projectRoot) + path.sep)) throw new Error(`Local WebView stylesheet is outside the project: ${href}`);
      await assertRealInside(projectRoot,file,'WebView stylesheet');
      watchedAssets?.add(path.relative(projectRoot,file));
      const linkedCss = await readFile(file,'utf8');
      const styled = await inlineCssUrls(linkedCss,{projectRoot,baseFile:file,cache,watchedAssets});
      const mediaAttribute = media ? ` media="${escapeAttribute(media)}"` : '';
      html = html.slice(0,match.index) + `<style${mediaAttribute}>${escapeStyleEnd(styled)}</style>` + html.slice(match.index + match[0].length);
    }
  }
  const embeddedFonts = Object.entries(fonts).map(([family,font]) => {
    const safeFamily = [...family].map(character => /^[A-Za-z0-9 _-]$/.test(character)
      ? character : `\\${character.codePointAt(0).toString(16)} `).join('');
    const data = typeof font === 'string' ? font : font.data;
    const mime = typeof font === 'string' ? 'font/ttf' : font.mime;
    const format = mime === 'font/otf' ? 'opentype' : 'truetype';
    return `@font-face{font-family:"${safeFamily}";src:url(data:${mime};base64,${data}) format("${format}")}`;
  }).join('\n');
  const nonce = randomBytes(16).toString('base64');
  // Author-provided script elements are part of the trusted application. Inline
  // event handlers remain blocked because a nonce only authorizes script tags.
  let scriptCursor = 0;
  for (;;) {
    const found = findHtmlTag(html.slice(scriptCursor), 'script');
    if (!found) break;
    const start = scriptCursor + found.index;
    const external = /\bsrc\s*=/i.test(html.slice(start, start + found.length));
    if (!external) html = html.slice(0, start + 7) + ` nonce="${nonce}"` + html.slice(start + 7);
    const end = /<\/\s*script\s*>/ig;
    end.lastIndex = start + found.length + (external ? 0 : nonce.length + 9);
    const closing = end.exec(html);
    scriptCursor = closing ? end.lastIndex : html.length;
  }
  const csp = `default-src 'none'; script-src 'self' 'nonce-${nonce}' 'unsafe-eval'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; font-src 'self' data:; media-src 'self' data:; connect-src 'self'${networkHosts.map(host => ' https://' + host).join('')}; form-action 'self'; frame-src 'self'; object-src 'none'; base-uri 'self'`;
  const policy = `<meta http-equiv="Content-Security-Policy" content="${escapeAttribute(csp)}">`;
  const style = `<style data-tsgpui="source-css">${escapeStyleEnd(`${embeddedFonts}\n${css}`)}</style>`;
  const bootstrap = `<script data-tsgpui="backend" nonce="${nonce}">(()=>{document.title=${safeJson(title)};globalThis.__tsgpui_network_hosts=${safeJson(networkHosts)};const source=${safeJson(backendJs)};const hidden=['fetch','XMLHttpRequest','WebSocket','localStorage','sessionStorage','indexedDB','Worker','require','process','Bun'];const store=(()=>{try{return ${persist ? 'window.localStorage' : 'window.sessionStorage'}}catch(_error){return null}})();let saved=null;try{saved=JSON.parse(store&&store.getItem('tsgpui:state')||'null')}catch(_error){}new Function(...hidden,source)(...hidden.map(()=>undefined));window.addEventListener('beforeunload',()=>{try{globalThis.__tsgpui_save&&globalThis.__tsgpui_save()}catch(_error){}});(${webviewRuntime.toString()})(store,saved);})();</script>`;
  const headEnd = findHtmlTag(html,'head',true), headStart = findHtmlTag(html,'head'), htmlStart = findHtmlTag(html,'html');
  if (headStart) {
    const afterHead = headStart.index + headStart.length;
    html = html.slice(0,afterHead) + policy + style + html.slice(afterHead);
  } else if (htmlStart) {
    const afterHtml = htmlStart.index + htmlStart.length;
    html = html.slice(0,afterHtml) + `<head>${policy}${style}</head>` + html.slice(afterHtml);
  }
  else html = `${policy}${style}${html}`;
  const scripts = bootstrap;
  const bodyEnd = findHtmlTag(html,'body',true), htmlEnd = findHtmlTag(html,'html',true);
  if (bodyEnd) html = html.slice(0,bodyEnd.index) + scripts + html.slice(bodyEnd.index);
  else if (htmlEnd) html = html.slice(0,htmlEnd.index) + scripts + html.slice(htmlEnd.index);
  else html += scripts;
  return html;
}
