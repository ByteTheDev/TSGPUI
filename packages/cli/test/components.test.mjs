import test from 'node:test';
import assert from 'node:assert/strict';
import { parseComponentLibrary, parseHTML } from '../src/html.mjs';
import { compileStylesheet, styleTree } from '../src/css.mjs';
import { COMPONENT_LIBRARY_CSS, COMPONENT_LIBRARY_HTML } from '../src/library.mjs';
import { compileProject } from '../src/compiler.mjs';

const library = parseComponentLibrary(COMPONENT_LIBRARY_HTML);
const parse = html => parseHTML(html, { library });
const config = { name: 'test-app', title: 'Test', width: 500, height: 400 };
const backend = `export default { state:{ query:'', count: 0 }, actions:{} };`;

// Returns the element at `path` (child indexes) below `root`.
const at = (root, ...path) => path.reduce((node, index) => node.children[index], root);

test('self-closed <use> with attributes parses', () => {
  const root = parse(`<body><template data-component="Card"><div class="card">x</div></template><use component="Card" data-prop-title="Hello"/></body>`);
  assert.equal(root.children[0].tag, 'div');
  assert.equal(root.children[0].props.title, 'Hello');
});

test('slots take default and named content, and fall back when nothing is given', () => {
  const root = parse(`<body>
    <template data-component="Card"><div><slot></slot><footer><slot name="footer">Default footer</slot></footer></div></template>
    <use component="Card"><p>Body</p><span slot="footer">Custom</span></use>
    <use component="Card"/>
  </body>`);
  const [filled, empty] = root.children;
  assert.equal(at(filled, 0).tag, 'p');
  assert.equal(at(filled, 1, 0).tag, 'span');
  assert.equal(at(filled, 1, 0, 0).text, 'Custom');
  assert.equal(at(empty, 0, 0).text, 'Default footer');
});

test('slot content takes the props of the place it is written, not of the component it lands in', () => {
  const root = parse(`<body>
    <template data-component="Row"><div><slot></slot></div></template>
    <use component="Row" data-prop-title="Inbox"><p>{{props.title}}</p></use>
  </body>`);
  assert.equal(at(root.children[0], 0, 0).text, '', 'the top level has no props, even though Row has title Inbox');
});

test('a component can pass its own props into slot content', () => {
  const root = parse(`<body>
    <template data-component="Inner"><div><slot></slot></div></template>
    <template data-component="Outer"><section><use component="Inner"><p>Hi {{props.name}}</p></use></section></template>
    <use component="Outer" data-prop-name="Ada"/>
  </body>`);
  const paragraph = at(root.children[0], 0, 0);
  assert.equal(paragraph.children[0].text, 'Hi Ada');
});

test('a component used inside slot content keeps its own {{props.*}} text', () => {
  const root = parse(`<body>
    <template data-component="Inner"><span>{{props.label}}</span></template>
    <template data-component="Outer"><div><slot></slot></div></template>
    <use component="Outer"><use component="Inner" data-prop-label="Notes"/></use>
  </body>`);
  const inner = at(root.children[0], 0);
  assert.equal(inner.props.label, 'Notes');
  assert.equal(inner.children[0].text, '{{props.label}}', 'runtime interpolation reads the Inner instance props');
});

test('slot misuse is rejected', () => {
  assert.throws(() => parse('<body><slot></slot></body>'), /only allowed inside a component template/);
  assert.throws(() => parse('<body><template data-component="A"><div></div></template><use component="A"><p>hi</p></use></body>'), /no default slot/);
  assert.throws(() => parse('<body><template data-component="A"><div><slot name="x"></slot></div></template><use component="A"><p slot="y">z</p></use></body>'), /no slot named 'y'/);
  assert.throws(() => parse('<body><template data-component="A"><div><slot class="x"></slot></div></template><use component="A"/></body>'), /only accepts a name attribute/);
  assert.throws(() => parse('<body><div slot="x"></div></body>'), /only applies to direct children of <use>/);
});

test('recursive components are still rejected, including through slots', () => {
  assert.throws(() => parse('<body><template data-component="A"><div><use component="A"/></div></template><use component="A"/></body>'), /Recursive component use/);
});

test('attributes on <use> go to the component element, and class names merge', () => {
  const root = parse(`<body><use component="SearchField" data-bind="query" data-oninput="search" placeholder="Find"/></body>`);
  const field = root.children[0];
  assert.equal(field.tag, 'input');
  assert.equal(field.attrs['data-bind'], 'query');
  assert.equal(field.attrs.placeholder, 'Find');
  assert.equal(field.attrs.class, 'ts-search');
  const card = parse(`<body><template data-component="Card"><div class="card">x</div></template><use component="Card" class="wide"/></body>`);
  assert.equal(card.children[0].attrs.class, 'card wide');
});

test('forwarded attributes are checked on the element that receives them', () => {
  assert.throws(() => parse('<body><use component="SearchField" data-onclick="go"/></body>'), /currently supports <button> only/);
  assert.throws(() => parse('<body><use component="ListRow" data-prop-title="x" data-bind="a" data-shortcut="mod-s"/></body>'), /data-shortcut requires data-onclick/);
});

test('an app template replaces a library component with the same name', () => {
  const root = parse(`<body><template data-component="Sidebar"><section class="mine"><slot></slot></section></template><use component="Sidebar"><p>Item</p></use></body>`);
  assert.equal(root.children[0].tag, 'section');
  assert.equal(root.children[0].attrs.class, 'mine');
});

test('library components expand to native markup with their props and slots', () => {
  const root = parse(`<body>
    <use component="Toolbar" data-prop-title="Inbox"/>
    <use component="ListRow" data-prop-title="Drafts" data-onclick="openDrafts"><span>3</span></use>
    <use component="Sheet" data-prop-title="Share"><p>Body</p><button slot="footer" data-onclick="close">Done</button></use>
  </body>`);
  const toolbar = root.children[0];
  assert.equal(toolbar.tag, 'header');
  assert.equal(at(toolbar, 1, 0).text, '{{props.title}}', 'the fallback keeps the prop reference for runtime');
  assert.equal(root.children[1].tag, 'button');
  assert.equal(root.children[1].attrs['data-onclick'], 'openDrafts');
  assert.equal(root.children[2].attrs.class, 'ts-sheet', 'the sheet element is the component root');
  assert.equal(at(root.children[2], 2, 0).tag, 'button');
});

test('library CSS compiles strictly with no warnings, and uses plain class selectors only', () => {
  const { warnings } = compileStylesheet(COMPONENT_LIBRARY_CSS, { strict: true });
  assert.deepEqual(warnings, []);
  assert.doesNotMatch(COMPONENT_LIBRARY_CSS, /(^|[{};])\s*(button|input|div|span|section|header|h3)\s*[{,]/m, 'no element-wide rules');
});

test('project custom properties restyle library rules', () => {
  const { rules } = compileStylesheet(':root { --ts-accent: #ff0000; }', { prelude: COMPONENT_LIBRARY_CSS });
  const selectedRule = rules.find(rule => JSON.stringify(rule).includes('ts-selected') && rule.declarations.some(([key, value]) => key === 'background' && value.toLowerCase() === '#ff0000'));
  assert.ok(selectedRule, 'the library selected row should use the project accent');
});

test('project rules come after library rules, so they win at equal specificity', () => {
  const { rules } = compileStylesheet('.ts-sidebar { width: 300px; }', { prelude: COMPONENT_LIBRARY_CSS });
  const root = parse('<body><use component="Sidebar"/></body>');
  styleTree(root, rules);
  assert.equal(root.children[0].style.width, '300px');
});

test('compileProject bundles library components with no CSS warnings', () => {
  const doc = compileProject({
    html: `<body><use component="Sidebar"><use component="ListRow" data-prop-title="Inbox"/></use><use component="SearchField" data-bind="query"/></body>`,
    css: 'body { background: #ffffff; }',
    backend,
    config,
    warnings: [],
  });
  assert.equal(doc.root.children[0].tag, 'section');
  assert.equal(doc.root.children[0].style.width, '240px');
  assert.equal(doc.root.children[1].attrs.type, 'search');
});
