import http from 'node:http';
import fs from 'node:fs';
import { writeFile, mkdir } from 'node:fs/promises';
import path from 'node:path';
import { spawn } from 'node:child_process';
import { buildPackage } from './compiler.mjs';

// Browser preview. It renders the same bundle the native host runs, with the DOM, so browser tools
// (devtools, screenshots, a live reload) can be used on the subset the host supports. It is a
// reference for that subset rather than a second renderer: layout follows the host's defaults
// (column boxes, border-box sizing, no button chrome), but browser text metrics and flex details differ.

// Functions that run in the page. They are serialized with toString(), so they must not refer to
// anything outside their own bodies.
// Mirrors merge_preserved in native/src/runtime.rs: keep values the user changed, take new seeds for
// values that still equal their seed, drop keys the new script removed.
function mergePreserved(fresh, seed, current) {
  const isRecord = value => value !== null && typeof value === 'object' && !Array.isArray(value);
  const kindOf = value => value === null ? 'null' : Array.isArray(value) ? 'array' : typeof value;
  if (JSON.stringify(seed) === JSON.stringify(current)) return fresh;
  if (isRecord(fresh) && isRecord(current)) {
    const merged = {};
    for (const key of Object.keys(fresh)) {
      const fromSeed = isRecord(seed) && Object.prototype.hasOwnProperty.call(seed, key) ? seed[key] : null;
      merged[key] = Object.prototype.hasOwnProperty.call(current, key) ? mergePreserved(fresh[key], fromSeed, current[key]) : fresh[key];
    }
    return merged;
  }
  if (Array.isArray(fresh) && Array.isArray(current)) return current;
  if (kindOf(fresh) === kindOf(current) && ['string', 'number', 'boolean', 'null'].includes(kindOf(current))) return current;
  return fresh;
}

function previewRuntime() {
  const doc = document;
  const bundle = JSON.parse(doc.getElementById('tsgpui-bundle').textContent);
  const SESSION_KEY = 'tsgpui-preview:' + bundle.name;
  const has = (object, key) => object !== null && typeof object === 'object' && Object.prototype.hasOwnProperty.call(object, key);
  const isRecord = value => value !== null && typeof value === 'object' && !Array.isArray(value);
  const asText = value => value === null || value === undefined ? '' : typeof value === 'string' ? value : JSON.stringify(value);
  const report = error => console.error('TSGPUI: ' + ((error && error.message) || error));

  // The backend runs with the browser's network and storage globals hidden, the way QuickJS has none.
  const hidden = ['fetch', 'XMLHttpRequest', 'WebSocket', 'localStorage', 'sessionStorage', 'indexedDB', 'Worker', 'require', 'process', 'Bun'];
  new Function(...hidden, bundle.backend_js)(...hidden.map(() => undefined));
  const app = globalThis.__tsgpui_app;
  const seed = JSON.parse(JSON.stringify(app.state));
  // A live reload stores the state before it reloads the page. The new script's seed is merged with it.
  let saved = null;
  try { saved = JSON.parse(sessionStorage.getItem(SESSION_KEY) || 'null'); } catch (_error) { saved = null; }
  sessionStorage.removeItem(SESSION_KEY);
  if (saved && saved.state !== undefined && saved.seed !== undefined) app.state = mergePreserved(app.state, saved.seed, saved.state);

  const lookup = (value, path) => path.split('.').reduce((current, part) => {
    if (Array.isArray(current)) return /^\d+$/.test(part) ? current[Number(part)] : undefined;
    return isRecord(current) && has(current, part) ? current[part] : undefined;
  }, value);

  // Same rules as the native set_state: every segment of the key must already exist.
  const setPath = (key, value) => {
    const parts = key.split('.');
    let target = app.state;
    for (const part of parts.slice(0, -1)) {
      const next = Array.isArray(target)
        ? (/^\d+$/.test(part) ? target[Number(part)] : undefined)
        : (isRecord(target) && has(target, part) ? target[part] : undefined);
      if (next === undefined) throw new Error('Unknown state key: ' + key);
      target = next;
    }
    const last = parts[parts.length - 1];
    if (Array.isArray(target)) {
      if (!/^\d+$/.test(last) || Number(last) >= target.length) throw new Error('Unknown state key: ' + key);
      target[Number(last)] = value;
    } else if (isRecord(target) && has(target, last)) {
      target[last] = value;
    } else {
      throw new Error('Unknown state key: ' + key);
    }
  };

  const api = Object.freeze({
    get: key => app.state[key],
    set: (key, value) => { if (!has(app.state, key)) throw new Error('Unknown state key: ' + key); app.state[key] = value; },
    update: (key, modify) => { if (!has(app.state, key)) throw new Error('Unknown state key: ' + key); app.state[key] = modify(app.state[key]); },
  });

  // Actions roll the state back when they throw or reject, as the native host does.
  const invoke = (name, event) => {
    if (!has(app.actions, name) || typeof app.actions[name] !== 'function') { report(new Error('No action: ' + name)); return; }
    const before = JSON.stringify(app.state);
    const finish = error => {
      if (error) { app.state = JSON.parse(before); report(error); }
      render();
    };
    // A synchronous action finishes before the click returns, as it does on the host. A promise
    // finishes when it settles.
    let result;
    try { result = app.actions[name](api, Object.freeze(event)); } catch (error) { finish(error); return; }
    if (result && typeof result.then === 'function') result.then(() => finish(null), finish);
    else finish(null);
  };

  const writeState = (key, value) => {
    try { setPath(key, value); } catch (error) { report(error); return false; }
    return true;
  };

  // Loop variables map to state paths, so `item.name` inside a repeat writes `records.3.name`.
  const resolveBinding = (key, ctx) => {
    const [head, ...rest] = key.split('.');
    const base = ctx.paths[head];
    return base === undefined ? key : [base, ...rest].join('.');
  };

  const boundValue = (node, ctx) => {
    const key = node.attrs['data-bind'];
    if (key === undefined) return undefined;
    const [head, ...rest] = key.split('.');
    if (has(ctx.locals, head) && rest.length) {
      const local = lookup(ctx.locals[head], rest.join('.'));
      if (local !== undefined) return local;
    }
    return lookup(app.state, resolveBinding(key, ctx));
  };

  const interpolate = (text, ctx) => {
    let rendered = '';
    let remaining = text;
    for (let start = remaining.indexOf('{{'); start !== -1; start = remaining.indexOf('{{')) {
      rendered += remaining.slice(0, start);
      const after = remaining.slice(start + 2);
      const end = after.indexOf('}}');
      if (end === -1) { rendered += remaining.slice(start); return rendered; }
      const path = after.slice(0, end).trim();
      const [head, ...rest] = path.split('.');
      let value;
      if (has(ctx.locals, head)) value = rest.length ? lookup(ctx.locals[head], rest.join('.')) : ctx.locals[head];
      if (value === undefined) value = lookup(app.state, path);
      rendered += asText(value);
      remaining = after.slice(end + 2);
    }
    return rendered + remaining;
  };

  const localValues = new Map();
  const invalidForms = new Set();
  let shortcuts = [];

  const matchesShortcut = (spec, event) => {
    const parts = spec.toLowerCase().split('-');
    const key = parts[parts.length - 1];
    const isMac = /mac/i.test(navigator.platform || '');
    const wantsMod = parts.includes('mod');
    const expectedCtrl = parts.includes('ctrl') || (wantsMod && !isMac);
    const expectedMeta = parts.includes('cmd') || (wantsMod && isMac);
    if (event.ctrlKey !== expectedCtrl || event.metaKey !== expectedMeta) return false;
    if (event.altKey !== parts.includes('alt') || event.shiftKey !== parts.includes('shift')) return false;
    return event.key.toLowerCase() === key;
  };

  const networkAllowed = url => {
    let parsed;
    try { parsed = new URL(url); } catch (_error) { return null; }
    const allowed = parsed.protocol === 'https:' && (parsed.port === '' || parsed.port === '443')
      && (bundle.permissions?.network_hosts ?? []).includes(parsed.hostname.toLowerCase());
    return allowed ? parsed : null;
  };

  const renderChildren = (node, ctx) => node.children.flatMap(child => renderNodes(child, ctx, false));

  const renderNodes = (node, ctx, skipRepeat) => {
    if (node.tag === '#text') return [doc.createTextNode(interpolate(node.text, ctx))];
    if (!skipRepeat && node.attrs['data-repeat']) {
      const [arrayPath, aliasWord, alias = 'item'] = node.attrs['data-repeat'].split(/\s+/);
      const resolved = resolveBinding(arrayPath, ctx);
      const items = lookup(app.state, resolved);
      const wrapper = doc.createElement('div');
      wrapper.className = 'tsgpui';
      // A data-virtual list scrolls, and its rows have the fixed height the host lays them out with.
      // The browser builds every row, so this only mirrors the layout.
      const rowHeight = node.attrs['data-virtual'];
      if (rowHeight !== undefined) wrapper.setAttribute('style', 'flex:1 1 0px;min-height:0;overflow:auto');
      if (Array.isArray(items)) {
        items.forEach((item, index) => {
          const locals = { ...ctx.locals, [aliasWord === 'as' ? alias : 'item']: item };
          const paths = { ...ctx.paths, [aliasWord === 'as' ? alias : 'item']: `${resolved}.${index}` };
          const row = renderElement(node, { ...ctx, locals, paths, suffix: `${ctx.suffix}:${index}` });
          if (rowHeight !== undefined) row.style.cssText += `;height:${rowHeight}px;flex-shrink:0`;
          wrapper.appendChild(row);
        });
      }
      return [wrapper];
    }
    if (node.tag === 'img') {
      const asset = bundle.assets?.[node.attrs.src];
      if (asset) {
        const image = doc.createElement('img');
        image.src = `data:image/${asset.format === 'jpeg' ? 'jpeg' : asset.format};base64,${asset.data}`;
        if (node.attrs.alt) image.alt = node.attrs.alt;
        return [styled(image, node, ctx)];
      }
      const fallback = doc.createElement('div');
      fallback.textContent = node.attrs.alt || '';
      return [styled(fallback, node, ctx)];
    }
    return [renderElement(node, ctx)];
  };

  // Gives the element its class, so the base sheet applies, and its key, so the node's styles apply.
  const styled = (element, node, ctx) => {
    element.classList.add('tsgpui');
    element.setAttribute('data-k', String(node.key));
    if (ctx.suffix) element.setAttribute('data-r', ctx.suffix);
    return element;
  };

  const renderElement = (node, ctx) => {
    const scoped = Object.keys(node.props ?? {}).length ? { ...ctx, locals: { ...ctx.locals, props: node.props } } : ctx;
    const tag = ['input', 'textarea', 'select', 'option', 'button', 'form', 'label', 'ul', 'ol', 'li', 'h1', 'h2', 'h3', 'p', 'span', 'strong', 'small', 'section', 'header', 'footer', 'article', 'main', 'div', 'body'].includes(node.tag) ? node.tag : 'div';
    const element = styled(doc.createElement(tag), node, scoped);
    for (const name of ['title', 'role', 'tabindex', 'name', 'placeholder', 'min', 'max', 'step', 'required', 'disabled', 'selected']) {
      if (node.attrs[name] !== undefined) element.setAttribute(name, node.attrs[name]);
    }
    if (node.attrs['aria-label']) element.setAttribute('aria-label', node.attrs['aria-label']);
    if (node.attrs.type && ['button', 'input'].includes(node.tag)) element.setAttribute('type', node.attrs.type);
    if (node.attrs['disabled'] !== undefined) element.disabled = true;
    const enabled = node.attrs['disabled'] === undefined;
    const action = node.attrs['data-onclick'];

    if (node.tag === 'input' && (node.attrs.type ?? 'text') === 'checkbox') {
      const bound = boundValue(node, scoped);
      const current = typeof bound === 'boolean' ? bound : localValues.has(node.key) ? localValues.get(node.key) : node.attrs.checked !== undefined;
      element.checked = current === true;
      element.disabled = !enabled;
      element.addEventListener('change', () => {
        const checked = element.checked;
        if (node.attrs['data-bind']) writeState(resolveBinding(node.attrs['data-bind'], scoped), checked);
        else localValues.set(node.key, checked);
        if (node.attrs['data-oninput']) invoke(node.attrs['data-oninput'], { type: 'input', checked, value: checked, name: node.attrs.name ?? null });
        render();
      });
      return element;
    }

    if (node.tag === 'input' || node.tag === 'textarea') {
      const isNumber = node.attrs.type === 'number';
      const fallback = node.tag === 'textarea' ? node.children.filter(child => child.tag === '#text').map(child => child.text).join(' ') : (node.attrs.value ?? '');
      const bound = boundValue(node, scoped);
      element.value = bound !== undefined ? asText(bound) : localValues.has(node.key) ? asText(localValues.get(node.key)) : fallback;
      if (node.attrs.type === 'password') element.type = 'password';
      element.addEventListener('input', () => {
        const state = isNumber && element.value.trim() !== '' && !Number.isNaN(Number(element.value)) ? Number(element.value) : element.value;
        if (node.attrs['data-bind']) writeState(resolveBinding(node.attrs['data-bind'], scoped), state);
        else localValues.set(node.key, state);
        if (node.attrs['data-oninput']) invoke(node.attrs['data-oninput'], { type: 'input', value: state, name: node.attrs.name ?? null });
        render();
      });
      return element;
    }

    if (node.tag === 'select') {
      const options = node.children.filter(child => child.tag === 'option');
      for (const option of options) {
        const choice = doc.createElement('option');
        const value = option.attrs.value ?? option.children.map(child => child.text ?? '').join(' ');
        choice.value = value;
        choice.textContent = option.children.map(child => child.text ?? '').join(' ');
        element.appendChild(choice);
      }
      const bound = boundValue(node, scoped);
      const selected = bound !== undefined ? asText(bound) : localValues.has(node.key) ? asText(localValues.get(node.key)) : (options.find(option => option.attrs.selected !== undefined)?.attrs.value ?? '');
      element.value = selected;
      element.addEventListener('change', () => {
        const value = element.value;
        if (node.attrs['data-bind']) writeState(resolveBinding(node.attrs['data-bind'], scoped), value);
        else localValues.set(node.key, value);
        if (node.attrs['data-oninput']) invoke(node.attrs['data-oninput'], { type: 'input', value });
        render();
      });
      return element;
    }

    if (node.attrs['data-bind'] !== undefined) {
      element.textContent = asText(boundValue(node, scoped));
    } else {
      for (const child of renderChildren(node, scoped)) element.appendChild(child);
    }
    if (node.tag === 'button' && node.children.length === 0 && node.attrs['aria-label']) element.textContent = node.attrs['aria-label'];

    if (node.tag === 'button' && node.attrs['data-shortcut'] && action && enabled) {
      shortcuts.push({ spec: node.attrs['data-shortcut'], action });
    }
    if (node.tag === 'button' && enabled) {
      const confirmMessage = node.attrs['data-confirm'];
      element.addEventListener('click', event => {
        if (node.attrs.type === 'submit') return;
        if (action && !confirmMessage && !node.attrs['data-fetch'] && !node.attrs['data-file-open'] && !node.attrs['data-file-save'] && !node.attrs['data-clipboard-copy'] && !node.attrs['data-clipboard-paste']) {
          invoke(action, { type: 'click' });
        }
        if (confirmMessage && action && window.confirm(node.attrs.title || confirmMessage)) invoke(action, { type: 'confirm' });
        const copyKey = node.attrs['data-clipboard-copy'];
        if (copyKey) navigator.clipboard?.writeText(asText(lookup(app.state, copyKey))).catch(report);
        const pasteKey = node.attrs['data-clipboard-paste'];
        if (pasteKey) navigator.clipboard?.readText().then(text => { if (writeState(pasteKey, text)) render(); }).catch(report);
        if (node.attrs['data-file-open']) openFile(node);
        if (node.attrs['data-file-save']) saveFile(node);
        if (node.attrs['data-fetch']) fetchInto(node);
        event.preventDefault();
      });
    }

    if (node.tag === 'form') {
      element.setAttribute('novalidate', '');
      element.addEventListener('submit', event => {
        event.preventDefault();
        const submitAction = node.attrs['data-submit'];
        if (!formValid(element)) {
          invalidForms.add(node.key);
          render();
          return;
        }
        invalidForms.delete(node.key);
        if (submitAction) invoke(submitAction, { type: 'submit', submit: true });
      });
      if (invalidForms.has(node.key)) {
        const message = doc.createElement('div');
        message.textContent = 'Check the required fields and values.';
        element.appendChild(message);
      }
    }
    return element;
  };

  // The same checks as the native host: required fields, number limits and step, and email shape.
  const formValid = form => [...form.querySelectorAll('input, textarea, select')].every(control => {
    const type = control.getAttribute('type') ?? 'text';
    if (control.hasAttribute('required')) {
      if (type === 'checkbox' ? !control.checked : control.value.trim() === '') return false;
    }
    if (control.tagName === 'INPUT' && type === 'number' && control.value.trim() !== '') {
      const number = Number(control.value);
      if (Number.isNaN(number)) return false;
      const min = control.hasAttribute('min') ? Number(control.getAttribute('min')) : undefined;
      const max = control.hasAttribute('max') ? Number(control.getAttribute('max')) : undefined;
      if (min !== undefined && number < min) return false;
      if (max !== undefined && number > max) return false;
      if (control.hasAttribute('step')) {
        const step = Number(control.getAttribute('step'));
        const base = min ?? 0;
        const steps = (number - base) / step;
        if (Math.abs(steps - Math.round(steps)) > 0.000001) return false;
      }
    }
    if (control.tagName === 'INPUT' && type === 'email' && control.value !== '') {
      const at = control.value.indexOf('@');
      if (!(at > 0 && control.value.slice(at + 1).includes('.'))) return false;
    }
    return true;
  });

  const openFile = node => {
    const input = doc.createElement('input');
    input.type = 'file';
    input.addEventListener('change', async () => {
      const file = input.files?.[0];
      if (!file) return;
      if (!writeState(node.attrs['data-file-open'], file.name)) return;
      const readKey = node.attrs['data-file-read'];
      if (readKey) {
        if (file.size > 1024 * 1024) { report(new Error('text files must be at most 1 MiB')); return; }
        writeState(readKey, await file.text());
      }
      render();
    });
    input.click();
  };

  const saveFile = node => {
    const contents = asText(lookup(app.state, node.attrs['data-file-save']));
    if (contents.length > 1024 * 1024) { report(new Error('text files must be at most 1 MiB')); return; }
    const link = doc.createElement('a');
    link.href = URL.createObjectURL(new Blob([contents], { type: 'text/plain' }));
    link.download = node.attrs.title || 'document.txt';
    link.click();
    setTimeout(() => URL.revokeObjectURL(link.href), 1000);
  };

  const fetchInto = node => {
    const parsed = networkAllowed(node.attrs['data-fetch']);
    if (!parsed) { report(new Error(`network host is not permitted: ${node.attrs['data-fetch']}`)); return; }
    const controller = new AbortController();
    const timer = setTimeout(() => controller.abort(), 15000);
    fetch(parsed.href, { redirect: 'error', credentials: 'omit', signal: controller.signal })
      .then(response => { if (!response.ok) throw new Error(`HTTP status ${response.status}`); return response.text(); })
      .then(text => { if (writeState(node.attrs['data-fetch-into'], text)) render(); })
      .catch(report)
      .finally(() => clearTimeout(timer));
  };

  // Rebuilds the tree from the state, keeping the focused field and its caret where they were.
  function render() {
    const active = doc.activeElement;
    const focus = active && active.getAttribute && active.getAttribute('data-k') !== null ? {
      key: active.getAttribute('data-k'),
      r: active.getAttribute('data-r') || '',
      start: active.selectionStart,
      end: active.selectionEnd,
    } : null;
    shortcuts = [];
    const root = bundle.root;
    const context = { locals: {}, paths: {}, suffix: '' };
    if (root.tag === 'body') {
      doc.body.replaceChildren(...renderChildren(root, context));
      doc.body.className = 'tsgpui';
      doc.body.setAttribute('data-k', String(root.key));
    } else {
      doc.body.replaceChildren(...renderNodes(root, context, false));
    }
    if (focus) {
      const match = [...doc.querySelectorAll(`[data-k="${focus.key}"]`)].find(element => (element.getAttribute('data-r') || '') === focus.r);
      if (match && typeof match.focus === 'function') {
        match.focus();
        if (focus.start !== null && focus.start !== undefined && match.setSelectionRange) match.setSelectionRange(focus.start, focus.end);
      }
    }
  }

  doc.addEventListener('keydown', event => {
    const found = shortcuts.find(shortcut => matchesShortcut(shortcut.spec, event));
    if (!found) return;
    event.preventDefault();
    invoke(found.action, { type: 'shortcut', key: event.key });
  });

  // Live reload: the server bumps the version when a rebuild succeeds. The page stores its state and
  // reloads; the new backend then merges the stored state, the same as the native host.
  const liveVersion = bundle.live;
  if (liveVersion !== null && liveVersion !== undefined) {
    let known = liveVersion;
    setInterval(() => {
      fetch('/version', { cache: 'no-store' }).then(response => response.json()).then(payload => {
        if (payload.version === known) return;
        known = payload.version;
        sessionStorage.setItem(SESSION_KEY, JSON.stringify({ state: app.state, seed }));
        location.reload();
      }).catch(() => {});
    }, 700);
  }

  doc.title = bundle.title;
  render();
}

const BASE_CSS = `*,*::before,*::after{box-sizing:border-box;margin:0;padding:0}
html,body{width:100%;height:100%}
body{display:flex;flex-direction:column;overflow:hidden;font-family:system-ui,"Segoe UI",sans-serif}
.tsgpui{display:flex;flex-direction:column;flex-shrink:1}
button,input,textarea,select{appearance:none;-webkit-appearance:none;background:none;border:0;border-radius:0;color:inherit;font:inherit;text-align:inherit;outline:none}
textarea{resize:none;white-space:pre-wrap;overflow:auto}
img{display:block}`;

const CSP = "default-src 'none'; script-src 'unsafe-inline' 'unsafe-eval'; style-src 'unsafe-inline'; img-src data:; font-src data:";

function declarations(map) {
  return Object.entries(map ?? {}).map(([key, value]) => `${key}:${value}`).join(';');
}

// Rules keyed by node: base styles, then media rules, then state rules, so a :hover beats a media rule.
export function previewCss(bundle) {
  const base = [];
  const media = [];
  const states = [];
  const visit = node => {
    if (node.tag === '#text') return;
    const selector = `[data-k="${node.key}"]`;
    if (Object.keys(node.style ?? {}).length) base.push(`${selector}{${declarations(node.style)}}`);
    for (const rule of node.responsive ?? []) media.push(`@media ${rule.media}{${selector}{${declarations(rule.declarations)}}}`);
    for (const [name, map] of Object.entries(node.states ?? {})) {
      const suffix = name === 'disabled' ? '[disabled]' : `:${name}`;
      states.push(`${selector}${suffix}{${declarations(map)}}`);
    }
    node.children.forEach(visit);
  };
  visit(bundle.root);
  const fonts = Object.entries(bundle.fonts ?? {}).map(([family, data]) =>
    `@font-face{font-family:"${family.replace(/["\\]/g, '')}";src:url(data:font/ttf;base64,${data}) format("truetype")}`);
  return [...fonts, BASE_CSS, ...base, ...media, ...states].join('\n');
}

// JSON that can sit inside a <script> element: no markup-significant characters can end the element.
function scriptJson(value) {
  return JSON.stringify(value)
    .replace(/</g, '\\u003c').replace(/>/g, '\\u003e').replace(/&/g, '\\u0026')
    .replace(/\u2028/g, '\\u2028').replace(/\u2029/g, '\\u2029');
}

// For text content and for values inside double-quoted attributes.
function escapeHtml(text) {
  return String(text).replace(/[&<>"]/g, ch => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' })[ch]);
}

// A standalone page. `live` is the version to poll, or null for a page with no server behind it.
export function renderPreviewHTML(bundle, { live = null } = {}) {
  const { fonts, ...rest } = bundle;
  const payload = { ...rest, live };
  // 'self' keeps the live-reload poll working; data-fetch hosts are the same allowlist the native host uses.
  const hosts = (bundle.permissions?.network_hosts ?? []).map(host => `https://${host}`);
  const csp = `${CSP}; connect-src 'self' ${hosts.join(' ')}`.trim();
  return `<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<meta http-equiv="Content-Security-Policy" content="${escapeHtml(csp)}">
<title>${escapeHtml(bundle.title)}</title>
<style>
${previewCss(bundle)}
</style>
</head>
<body></body>
<script type="application/json" id="tsgpui-bundle">${scriptJson(payload)}</script>
<script>${mergePreserved.toString()}
(${previewRuntime.toString()})();</script>
</html>
`;
}

function openCommand(url) {
  if (process.platform === 'win32') return ['cmd', ['/c', 'start', '', url]];
  if (process.platform === 'darwin') return ['open', [url]];
  return ['xdg-open', [url]];
}

// Serves the preview on 127.0.0.1. The page polls /version, and a successful rebuild bumps it.
export async function startPreview({ projectRoot, port = 4173, open = false, watch = true, log = console.log, warn = console.warn }) {
  let current = null;
  let version = 0;
  let building = false;
  let queued = false;
  let timer = null;
  const rebuild = async () => {
    if (building) { queued = true; return; }
    building = true;
    try {
      const built = await buildPackage(projectRoot);
      for (const warning of built.warnings) warn(`Warning: ${warning}`);
      version += 1;
      current = renderPreviewHTML(built.bundle, { live: version });
      log(`Preview updated (${built.bundleFile})`);
    } catch (error) {
      log(`Build failed: ${error.message}`);
    } finally {
      building = false;
      if (queued) { queued = false; await rebuild(); }
    }
  };
  await rebuild();
  if (!current) throw new Error('The preview could not be built');
  const server = http.createServer((request, response) => {
    const url = new URL(request.url, 'http://127.0.0.1');
    if (url.pathname === '/') {
      response.writeHead(200, { 'content-type': 'text/html; charset=utf-8', 'cache-control': 'no-store' });
      response.end(current);
    } else if (url.pathname === '/version') {
      response.writeHead(200, { 'content-type': 'application/json', 'cache-control': 'no-store' });
      response.end(JSON.stringify({ version }));
    } else {
      response.writeHead(404, { 'content-type': 'text/plain' });
      response.end('Not found');
    }
  });
  await new Promise((resolve, reject) => {
    server.once('error', reject);
    server.listen(port, '127.0.0.1', resolve);
  });
  const address = server.address();
  const url = `http://127.0.0.1:${address.port}/`;
  log(`TSGPUI preview: ${url}`);
  let watcher = null;
  if (watch) {
    const ignored = name => /^(dist|node_modules|\.git)(\/|\\|$)/.test(name);
    watcher = fs.watch(projectRoot, { recursive: true }, (_event, name) => {
      if (!name || ignored(name.toString())) return;
      clearTimeout(timer);
      timer = setTimeout(() => void rebuild(), 120);
    });
  }
  if (open) {
    const [command, args] = openCommand(url);
    spawn(command, args, { detached: true, stdio: 'ignore', shell: false }).unref();
  }
  return {
    url,
    version: () => version,
    close: () => new Promise(resolve => {
      clearTimeout(timer);
      watcher?.close();
      server.close(() => resolve());
    }),
  };
}

// Writes one self-contained HTML file with no live reload, for opening in any browser.
export async function writePreviewFile({ projectRoot, file }) {
  const built = await buildPackage(projectRoot);
  await mkdir(path.dirname(file), { recursive: true });
  await writeFile(file, renderPreviewHTML(built.bundle), 'utf8');
  return { file, warnings: built.warnings };
}

export { mergePreserved };
