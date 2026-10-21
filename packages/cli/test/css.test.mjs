import test from 'node:test';
import assert from 'node:assert/strict';
import { parseHTML } from '../src/html.mjs';
import { compileStylesheet, styleTree } from '../src/css.mjs';

// Compiles one stylesheet against a small document and returns the styled root plus warnings.
function style(css, html = '<body><main class="card"><p>One</p><section><p>Two</p></section></main></body>') {
  const { rules, warnings } = compileStylesheet(css);
  return { root: styleTree(parseHTML(html), rules), warnings };
}

const firstChild = root => root.children[0];

test('CSS variables resolve at compile time, including fallbacks', () => {
  const { root, warnings } = style(':root { --accent: #3366ff; --gap: 8px; } main { color: var(--accent); gap: var(--gap); padding: var(--missing, 4px); }');
  assert.equal(firstChild(root).style.color, '#3366ff');
  assert.equal(firstChild(root).style.gap, '8px');
  assert.equal(firstChild(root).style.padding, '4px');
  assert.deepEqual(warnings, []);
});

test('an undefined CSS variable without a fallback is a warning', () => {
  const { warnings } = style('main { color: var(--nope); }');
  assert.match(warnings[0], /undefined CSS variable --nope/);
});

test('rem and em convert to px, and percentages are kept for sizes', () => {
  const { root } = style('main { font-size: 1.25rem; width: 50%; min-width: 2em; max-width: none; margin: -4px; }');
  assert.equal(firstChild(root).style['font-size'], '20px');
  assert.equal(firstChild(root).style.width, '50%');
  assert.equal(firstChild(root).style['min-width'], '32px');
  assert.equal(firstChild(root).style['max-width'], undefined);
  assert.equal(firstChild(root).style.margin, '-4px');
});

test('padding and margin shorthands expand to four sides in CSS order', () => {
  const { root } = style('main { padding: 1px 2px 3px; margin: 5px 6px; }');
  const main = firstChild(root).style;
  assert.equal(main['padding-top'], '1px');
  assert.equal(main['padding-right'], '2px');
  assert.equal(main['padding-bottom'], '3px');
  assert.equal(main['padding-left'], '2px');
  assert.equal(main['margin-top'], '5px');
  assert.equal(main['margin-right'], '6px');
  assert.equal(main['margin-left'], '6px');
});

test('colors normalize to hex, including rgba, 8-digit hex and named colors', () => {
  const { root } = style('main { color: rgb(255 0 0); background: rgba(0,0,0,.5); border-color: #abc; } p { color: transparent; }');
  assert.equal(firstChild(root).style.color, '#ff0000');
  assert.equal(firstChild(root).style.background, '#00000080');
  assert.equal(firstChild(root).style['border-color'], '#aabbcc');
  assert.equal(firstChild(root).children[0].style.color, '#00000000');
});

test('linear gradients compile to degrees and two stops', () => {
  const { root } = style('main { background: linear-gradient(90deg, #fff, #000); } section { background: linear-gradient(to bottom, #111, #222); }');
  assert.equal(firstChild(root).style.background, 'linear-gradient(90deg,#ffffff,#000000)');
  assert.equal(firstChild(root).children[1].style.background, 'linear-gradient(180deg,#111111,#222222)');
});

test('a gradient with three stops is a warning', () => {
  const { warnings } = style('main { background: linear-gradient(#fff, #000, #888); }');
  assert.match(warnings[0], /exactly two color stops/);
});

test('box-shadow lists normalize offsets, blur, spread and color', () => {
  const { root } = style('main { box-shadow: 0 2px 8px rgba(0,0,0,.2), 0 1px #fff; }');
  assert.equal(firstChild(root).style['box-shadow'], '0px 2px 8px 0px #00000033, 0px 1px 0px 0px #ffffff');
});

test('inset box-shadow is a warning, not a silent drop', () => {
  const { warnings } = style('main { box-shadow: inset 0 1px #000; }');
  assert.match(warnings[0], /inset box-shadow is not supported/);
});

test('border shorthand sets width, style and color; none removes the border', () => {
  const { root } = style('main { border: 1px solid #ccc; } section { border: none; }');
  assert.equal(firstChild(root).style['border-width'], '1px');
  assert.equal(firstChild(root).style['border-style'], 'solid');
  assert.equal(firstChild(root).style['border-color'], '#cccccc');
  assert.equal(firstChild(root).children[1].style['border-width'], '0px');
  assert.equal(firstChild(root).children[1].style['border-style'], 'none');
});

test('dashed borders are a warning', () => {
  const { warnings } = style('main { border: 2px dashed #000; }');
  assert.match(warnings[0], /border style 'dashed' is not supported/);
});

test('flex shorthand expands to grow, shrink and basis', () => {
  const { root } = style('main { flex: 1; } section { flex: 0 0 auto; }');
  assert.equal(firstChild(root).style['flex-grow'], '1');
  assert.equal(firstChild(root).style['flex-shrink'], '1');
  assert.equal(firstChild(root).style['flex-basis'], '0px');
  assert.equal(firstChild(root).children[1].style['flex-grow'], '0');
  assert.equal(firstChild(root).children[1].style['flex-basis'], 'auto');
});

test('absolute positioning and inset shorthand', () => {
  const { root } = style('main { position: absolute; inset: 0; } section { position: static; top: 10px; left: 50%; }');
  assert.equal(firstChild(root).style.position, 'absolute');
  assert.equal(firstChild(root).style.top, '0px');
  assert.equal(firstChild(root).style.right, '0px');
  assert.equal(firstChild(root).style.bottom, '0px');
  assert.equal(firstChild(root).style.left, '0px');
  assert.equal(firstChild(root).children[1].style.position, 'relative');
  assert.equal(firstChild(root).children[1].style.left, '50%');
});

test('text properties: line-height, font-style, font-family, white-space and ellipsis', () => {
  const { root } = style('main { line-height: 1.5; font-style: oblique; font-family: "Inter", sans-serif; white-space: nowrap; text-overflow: ellipsis; }');
  const main = firstChild(root).style;
  assert.equal(main['line-height'], '1.5');
  assert.equal(main['font-style'], 'italic');
  assert.equal(main['font-family'], 'Inter');
  assert.equal(main['white-space'], 'nowrap');
  assert.equal(main['text-overflow'], 'ellipsis');
});

test('line-height and text properties inherit to children', () => {
  const { root } = style('main { line-height: 20px; font-style: italic; }');
  assert.equal(firstChild(root).children[0].style['line-height'], '20px');
  assert.equal(firstChild(root).children[0].style['font-style'], 'italic');
});

test('!important beats a later rule with higher specificity', () => {
  const { root } = style('#app main { color: #111111; } main { color: #222222 !important; }', '<body id="app"><main><p>x</p></main></body>');
  assert.equal(firstChild(root).style.color, '#222222');
});

test('the child combinator matches only direct children', () => {
  const { root } = style('.card > p { color: #123456; }');
  assert.equal(firstChild(root).children[0].style.color, '#123456');
  assert.equal(firstChild(root).children[1].children[0].style.color, undefined);
});

test('the universal selector matches every element', () => {
  const { root } = style('* { margin: 0; }');
  assert.equal(firstChild(root).style.margin, '0px');
  assert.equal(firstChild(root).children[1].style.margin, '0px');
});

test('state pseudo-classes compile to per-node state styles', () => {
  const { root, warnings } = style(
    'button:hover { background: #123456; } button:active { background: #654321; } button:disabled { opacity: .4; } button:focus { border-color: #ff0000; }',
    '<body><button>Go</button></body>'
  );
  const button = firstChild(root);
  assert.equal(button.style.background, undefined);
  assert.equal(button.states.hover.background, '#123456');
  assert.equal(button.states.active.background, '#654321');
  assert.equal(button.states.disabled.opacity, '0.4');
  assert.equal(button.states.focus['border-color'], '#ff0000');
  assert.deepEqual(warnings, []);
});

test('state rules cannot change layout, and the layout part is a warning', () => {
  const { warnings } = style('button:hover { width: 100px; background: #123456; }', '<body><button>Go</button></body>');
  assert.match(warnings[0], /width cannot change on :hover/);
});

test('pseudo-classes inside @media are a warning', () => {
  const { warnings } = style('@media (min-width: 600px) { button:hover { background: #123456; } }', '<body><button>Go</button></body>');
  assert.match(warnings[0], /:hover inside @media is not supported/);
});

test('unsupported selectors are warnings and the rest of the stylesheet still applies', () => {
  const { root, warnings } = style('main[role] { color: #111111; } p::before { color: #222222; } main { color: #333333; }');
  assert.equal(firstChild(root).style.color, '#333333');
  assert.equal(warnings.length, 2);
  assert.match(warnings[0], /Unsupported CSS selector/);
});

test('a skipped declaration does not drop the other declarations in its rule', () => {
  const { root, warnings } = style('main { transition: all .2s; color: #444444; padding: 8px; }');
  assert.equal(firstChild(root).style.color, '#444444');
  assert.equal(firstChild(root).style.padding, '8px');
  assert.equal(warnings.length, 1);
  assert.match(warnings[0], /line 1: 'transition' is not supported yet/);
});

test('box-sizing is accepted silently because boxes already use border-box sizing', () => {
  const { warnings } = style('main { box-sizing: border-box; }');
  assert.deepEqual(warnings, []);
});

test('strict mode turns every warning into an error with its line', () => {
  assert.throws(() => compileStylesheet('main {\n  color: #111111;\n  transition: all 1s;\n}', { strict: true }), /line 3: 'transition' is not supported yet/);
});

test('opacity and grid tracks keep their values', () => {
  const { root } = style('main { opacity: .5; grid-template-columns: 1fr 240px auto; display: grid; }');
  assert.equal(firstChild(root).style.opacity, '0.5');
  assert.equal(firstChild(root).style['grid-template-columns'], '1fr 240px auto');
});

test('cursor and gap shorthands', () => {
  const { root } = style('main { cursor: pointer; gap: 8px 12px; }');
  assert.equal(firstChild(root).style.cursor, 'pointer');
  assert.equal(firstChild(root).style['row-gap'], '8px');
  assert.equal(firstChild(root).style['column-gap'], '12px');
});

test('per-side padding and margin longhands are accepted', () => {
  const { root, warnings } = style('main { padding-top: 4px; margin-bottom: 8px; margin-left: -2px; padding-right: 10%; }');
  const main = firstChild(root).style;
  assert.equal(main['padding-top'], '4px');
  assert.equal(main['margin-bottom'], '8px');
  assert.equal(main['margin-left'], '-2px');
  assert.equal(main['padding-right'], '10%');
  assert.deepEqual(warnings, []);
});

test('margin accepts auto for centering, padding does not', () => {
  const { root, warnings } = style('main { margin: 0 auto; padding: auto; }');
  assert.equal(firstChild(root).style['margin-left'], 'auto');
  assert.equal(firstChild(root).style['margin-right'], 'auto');
  assert.match(warnings[0], /needs a length in px, rem, em or %/);
});

test('viewport units are preserved for the host to resolve', () => {
  const { root, warnings } = style('main { width: 50vw; height: 25vh; font-size: 2vmin; gap: 3vmax; }');
  const main = firstChild(root).style;
  assert.equal(main.width, '50vw');
  assert.equal(main.height, '25vh');
  assert.equal(main['font-size'], '2vmin');
  assert.equal(main.gap, '3vmax');
  assert.deepEqual(warnings, []);
});

test('z-index is accepted and auto is omitted', () => {
  const { root, warnings } = style('main { z-index: 4; } .x { z-index: -2; } .y { z-index: auto; }', '<body><main><p class="x">One</p><p class="y">Two</p></main></body>');
  assert.equal(firstChild(root).style['z-index'], '4');
  assert.deepEqual(warnings, []);
  assert.equal(firstChild(root).children[0].style['z-index'], '-2');
  assert.equal(firstChild(root).children[1].style['z-index'], undefined);
});
