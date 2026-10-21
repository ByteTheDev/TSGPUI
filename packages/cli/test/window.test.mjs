import test from 'node:test';
import assert from 'node:assert/strict';
import { parseComponentLibrary, parseHTML } from '../src/html.mjs';
import { COMPONENT_LIBRARY_HTML } from '../src/library.mjs';
import { compileProject, validateConfig } from '../src/compiler.mjs';

const backend = `export default { state:{}, actions:{} };`;
const base = { name: 'win-app', title: 'Win', width: 500, height: 400 };

test('window chrome defaults to the system title bar and an opaque backdrop', () => {
  const config = validateConfig({ ...base, html: 'a.html', css: 'a.css', backend: 'b.ts' });
  assert.deepEqual(config.window, { titlebar: 'system', backdrop: 'opaque' });
});

test('window chrome accepts unified title bars and blur, and rejects other values', () => {
  const settings = { ...base, html: 'a.html', css: 'a.css', backend: 'b.ts' };
  assert.deepEqual(validateConfig({ ...settings, window: { titlebar: 'unified', backdrop: 'blur' } }).window, { titlebar: 'unified', backdrop: 'blur' });
  assert.throws(() => validateConfig({ ...settings, window: { titlebar: 'custom' } }), /window.titlebar must be/);
  assert.throws(() => validateConfig({ ...settings, window: { backdrop: 'mica' } }), /window.backdrop must be/);
});

test('the bundle records the window chrome', () => {
  const doc = compileProject({
    html: '<body><div>Hi</div></body>', css: '', backend,
    config: { ...base, window: { titlebar: 'unified', backdrop: 'blur' } },
    warnings: [],
  });
  assert.deepEqual(doc.window, { titlebar: 'unified', backdrop: 'blur' });
});

test('a unified title bar warns when nothing can drag the window', () => {
  const warnings = [];
  compileProject({
    html: '<body><div>Hi</div></body>', css: '', backend,
    config: { ...base, window: { titlebar: 'unified' } },
    warnings,
  });
  assert.ok(warnings.some(warning => /no element has data-window-drag/.test(warning)), warnings.join('\n'));
});

test('a unified title bar with a drag area and window buttons does not warn', () => {
  const warnings = [];
  compileProject({
    html: `<body><header><div data-window-drag><div>Title</div></div><div data-window-control="close">x</div></header></body>`,
    css: '', backend,
    config: { ...base, window: { titlebar: 'unified' } },
    warnings,
  });
  assert.deepEqual(warnings, []);
});

test('a drag area cannot contain buttons, controls, or components that contain them', () => {
  assert.throws(() => parseHTML('<body><header data-window-drag><button data-onclick="go">Go</button></header></body>'), /cannot contain buttons/);
  assert.throws(() => parseHTML('<body><header data-window-drag><div data-window-control="close">x</div></header></body>'), /cannot contain buttons/);
  assert.throws(() => parseHTML('<body><header data-window-drag><div tabindex="0">x</div></header></body>'), /cannot contain buttons/);
  assert.throws(() => parseHTML('<body><header data-window-drag><div><p><span data-onclick="go">x</span></p></div></header></body>'), /data-onclick currently supports/);
  const library = parseComponentLibrary(COMPONENT_LIBRARY_HTML);
  assert.throws(() => parseHTML('<body><header data-window-drag><use component="ListRow" data-prop-title="x"/></header></body>', { library }), /cannot contain buttons/);
});

test('window buttons beside a drag area are accepted', () => {
  const root = parseHTML(`<body><header><div class="lights"><div data-window-control="close">x</div></div><div data-window-drag><div>Title</div></div></header></body>`);
  assert.equal(root.children[0].children[1].attrs['data-window-drag'], '');
});

test('window buttons without a unified title bar warn, because the system bar handles them', () => {
  const warnings = [];
  compileProject({
    html: `<body><header><div data-window-control="minimize">-</div></header></body>`,
    css: '', backend,
    config: { ...base },
    warnings,
  });
  assert.ok(warnings.some(warning => /has no effect unless window.titlebar is "unified"/.test(warning)), warnings.join('\n'));
});

test('data-virtual needs data-repeat and a sensible row height', () => {
  assert.throws(() => parseHTML('<body><div data-virtual="40">x</div></body>'), /requires data-repeat/);
  assert.throws(() => parseHTML('<body><div data-repeat="items" data-virtual="0"></div></body>'), /from 1 to 1000/);
  assert.equal(parseHTML('<body><div data-repeat="items" data-virtual="40"></div></body>').children[0].attrs['data-virtual'], '40');
});
