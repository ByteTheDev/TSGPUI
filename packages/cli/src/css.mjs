import postcss from 'postcss';

// Web CSS is normalized to the subset the native host renders. Values are canonical, so the host
// only parses these forms:
//   lengths      "12px" or "50%"; rem and em are converted at 16px; "auto" where allowed
//   colors       "#RRGGBB" or "#RRGGBBAA"
//   background   a color, or "linear-gradient(<deg>,<color>,<color>)"
//   box-shadow   "x y blur spread color" entries joined by ", "
//   line-height  "<n>px", or a unitless multiplier of the element's font-size
//   flex-*       plain numbers, and lengths for flex-basis
//
// Declarations TSGPUI cannot render are warnings and are skipped. With strictCss they are errors.

const ROOT_FONT_PX = 16;
const STATE_PSEUDOS = new Set(['hover', 'active', 'focus', 'disabled']);
// Only visual properties can change on a pseudo-class. Layout changes would move siblings.
const STATE_PROPERTIES = new Set(['background', 'color', 'border-color', 'border-width', 'border-style', 'box-shadow', 'opacity', 'cursor']);
// Inherited by children, as in CSS.
const INHERITED = ['color', 'font-size', 'font-weight', 'font-family', 'font-style', 'line-height', 'text-align', 'cursor', 'white-space'];
// Known web properties that TSGPUI does not render yet. They are reported, never silently dropped.
const UNSUPPORTED = new Set([
  'transition', 'transform', 'animation', 'filter', 'backdrop-filter', 'letter-spacing', 'z-index',
  'visibility', 'pointer-events', 'user-select', 'text-decoration', 'text-transform', 'outline',
  'will-change', 'vertical-align', 'float', 'clear', 'overflow-x', 'overflow-y',
  'border-top', 'border-right', 'border-bottom', 'border-left', 'border-top-width', 'border-top-color',
  'border-right-width', 'border-right-color', 'border-bottom-width', 'border-bottom-color',
  'border-left-width', 'border-left-color', 'border-top-style', 'border-right-style', 'border-bottom-style',
  'border-left-style',
]);
// Accepted without effect. TSGPUI already sizes boxes the way box-sizing: border-box does.
const IGNORED = new Set(['box-sizing']);

const NAMED_COLORS = { transparent: '#00000000', white: '#ffffff', black: '#000000' };
const BORDER_WIDTHS = { thin: '1px', medium: '3px', thick: '5px' };

function num(value) {
  return String(Number(Number(value).toFixed(4)));
}

// Splits on whitespace that is not inside parentheses, so rgba(0, 0, 0, .5) stays one token.
function words(value) {
  const out = [];
  let depth = 0, current = '';
  for (const ch of value.trim()) {
    if (ch === '(') depth++;
    if (ch === ')') depth--;
    if (/\s/.test(ch) && depth === 0) {
      if (current) out.push(current);
      current = '';
    } else {
      current += ch;
    }
  }
  if (current) out.push(current);
  return out;
}

// Splits on a separator that is not inside parentheses.
function splitTop(value, separator) {
  const out = [];
  let depth = 0, current = '';
  for (const ch of value) {
    if (ch === '(') depth++;
    if (ch === ')') depth--;
    if (ch === separator && depth === 0) {
      out.push(current.trim());
      current = '';
    } else {
      current += ch;
    }
  }
  out.push(current.trim());
  return out;
}

function oneOf(prop, value, allowed) {
  if (!allowed.includes(value)) throw new Error(`unsupported ${prop} value '${value}'`);
  return value;
}

function length(prop, value, { percent = false, auto = false, negative = false } = {}) {
  if (auto && value === 'auto') return 'auto';
  if (value === '0') return '0px';
  const match = /^(-?\d*\.?\d+)(px|rem|em|%)$/.exec(value);
  if (!match) throw new Error(`'${prop}: ${value}' needs a length in px, rem, em${percent ? ' or %' : ''}`);
  let amount = Number(match[1]);
  const unit = match[2];
  if (unit === '%' && !percent) throw new Error(`'${prop}' does not accept %`);
  if (amount < 0 && !negative) throw new Error(`'${prop}: ${value}' must not be negative`);
  if (unit === 'rem' || unit === 'em') amount *= ROOT_FONT_PX;
  return `${num(amount)}${unit === '%' ? '%' : 'px'}`;
}

function number(prop, value) {
  if (!/^\d*\.?\d+$/.test(value)) throw new Error(`'${prop}: ${value}' needs a number`);
  return num(value);
}

function color(prop, value) {
  const text = value.trim().toLowerCase();
  if (NAMED_COLORS[text]) return NAMED_COLORS[text];
  const hex = /^#([0-9a-f]{3,4}|[0-9a-f]{6}|[0-9a-f]{8})$/.exec(text);
  if (hex) {
    let digits = hex[1];
    if (digits.length <= 4) digits = [...digits].map(ch => ch + ch).join('');
    if (digits.length === 8 && digits.endsWith('ff')) digits = digits.slice(0, 6);
    return `#${digits}`;
  }
  const rgb = /^rgba?\((.*)\)$/.exec(text);
  if (rgb) {
    const parts = rgb[1].split(/[\s,/]+/).filter(Boolean);
    if (parts.length < 3 || parts.length > 4) throw new Error(`'${prop}: ${value}' needs three channels and an optional alpha`);
    const channel = part => {
      const amount = part.endsWith('%') ? parseFloat(part) * 2.55 : Number(part);
      if (!Number.isFinite(amount)) throw new Error(`'${prop}: ${value}' has an invalid channel`);
      return Math.min(255, Math.max(0, Math.round(amount)));
    };
    const [r, g, b] = parts.slice(0, 3).map(channel);
    const alphaText = parts[3];
    const alpha = alphaText === undefined ? 255 : Math.min(255, Math.max(0, Math.round((alphaText.endsWith('%') ? parseFloat(alphaText) / 100 : Number(alphaText)) * 255)));
    const hexOf = n => n.toString(16).padStart(2, '0');
    return `#${hexOf(r)}${hexOf(g)}${hexOf(b)}${alpha === 255 ? '' : hexOf(alpha)}`;
  }
  throw new Error(`'${prop}: ${value}' needs a color (#hex, rgb(), rgba() or transparent)`);
}

function background(prop, value) {
  const gradient = /^linear-gradient\((.*)\)$/i.exec(value.trim());
  if (!gradient) return color(prop, value);
  const args = splitTop(gradient[1], ',');
  let angle = 180;
  let stops = args;
  if (/^-?\d*\.?\d+deg$/i.test(args[0])) {
    angle = parseFloat(args[0]);
    stops = args.slice(1);
  } else if (/^to\s+(top|right|bottom|left)$/i.test(args[0])) {
    angle = { top: 0, right: 90, bottom: 180, left: 270 }[args[0].split(/\s+/)[1].toLowerCase()];
    stops = args.slice(1);
  }
  if (stops.length !== 2) throw new Error('linear-gradient needs exactly two color stops');
  return `linear-gradient(${num(angle)}deg,${color(prop, stops[0])},${color(prop, stops[1])})`;
}

function borderStyle(value) {
  const style = value === 'hidden' ? 'none' : value;
  return oneOf('border-style', style, ['solid', 'none']);
}

// Shorthand: `1px solid #ccc`, in any order. Like CSS, a missing style means no border.
function border(value) {
  let width, style, borderColor;
  for (const token of words(value)) {
    const lower = token.toLowerCase();
    if (BORDER_WIDTHS[lower]) width = BORDER_WIDTHS[lower];
    else if (/^-?\d*\.?\d+(px|rem|em)?$/.test(lower)) width = length('border', lower);
    else if (['solid', 'none', 'hidden'].includes(lower)) style = borderStyle(lower);
    else if (['dashed', 'dotted', 'double', 'groove', 'ridge', 'inset', 'outset'].includes(lower)) throw new Error(`border style '${lower}' is not supported; use solid`);
    else borderColor = color('border', token);
  }
  if (style === 'none' || style === undefined) return [['border-width', '0px'], ['border-style', 'none']];
  const out = [['border-width', width ?? BORDER_WIDTHS.medium], ['border-style', style]];
  if (borderColor !== undefined) out.push(['border-color', borderColor]);
  return out;
}

// Expands one to four values in CSS order: top, right, bottom, left.
function fourValues(values) {
  const [top, right = top, bottom = top, left = right] = values;
  return { top, right, bottom, left };
}

// Margins may be negative and may be auto (for centering). Padding may be neither.
function sides(prop, value, isMargin) {
  const values = words(value).map(part => length(prop, part, { percent: true, auto: isMargin, negative: isMargin }));
  if (values.length < 1 || values.length > 4) throw new Error(`${prop} takes one to four lengths`);
  if (values.length === 1) return [[prop, values[0]]];
  const { top, right, bottom, left } = fourValues(values);
  return [[`${prop}-top`, top], [`${prop}-right`, right], [`${prop}-bottom`, bottom], [`${prop}-left`, left]];
}

function insetSides(value) {
  const values = words(value).map(part => length('inset', part, { percent: true, auto: true, negative: true }));
  if (values.length < 1 || values.length > 4) throw new Error('inset takes one to four lengths');
  const { top, right, bottom, left } = fourValues(values);
  return [['top', top], ['right', right], ['bottom', bottom], ['left', left]].filter(([, amount]) => amount !== 'auto');
}

function shadows(value) {
  return splitTop(value, ',').map(part => {
    const tokens = words(part);
    if (tokens.includes('inset')) throw new Error('inset box-shadow is not supported');
    const lengths = tokens.filter(token => /^-?[\d.]/.test(token));
    const others = tokens.filter(token => !/^-?[\d.]/.test(token));
    if (lengths.length < 2 || lengths.length > 4) throw new Error(`box-shadow needs two to four lengths: '${part}'`);
    if (others.length > 1) throw new Error(`box-shadow has unexpected tokens: '${part}'`);
    const [x, y, blur = '0px', spread = '0px'] = lengths.map(item => length('box-shadow', item, { negative: true }));
    const shadowColor = others.length ? color('box-shadow', others[0]) : '#000000';
    return `${x} ${y} ${blur} ${spread} ${shadowColor}`;
  }).join(', ');
}

function gridTracks(prop, value) {
  const track = '(?:\\d+(?:\\.\\d+)?fr|\\d+(?:\\.\\d+)?px|auto)';
  if (!new RegExp(`^${track}(?:\\s+${track})*$`).test(value)) throw new Error(`Unsupported ${prop} value '${value}'`);
  return value;
}

const HANDLERS = {
  display: v => [['display', oneOf('display', v, ['flex', 'block', 'grid'])]],
  'flex-direction': v => [['flex-direction', oneOf('flex-direction', v, ['row', 'column'])]],
  'align-items': v => [['align-items', oneOf('align-items', v, ['center', 'flex-start', 'flex-end', 'stretch'])]],
  'justify-content': v => [['justify-content', oneOf('justify-content', v, ['center', 'flex-start', 'flex-end', 'space-between', 'space-around'])]],
  gap: v => {
    const values = words(v).map(part => length('gap', part));
    if (values.length === 1) return [['gap', values[0]]];
    if (values.length === 2) return [['row-gap', values[0]], ['column-gap', values[1]]];
    throw new Error('gap takes one or two lengths');
  },
  'row-gap': v => [['row-gap', length('row-gap', v)]],
  'column-gap': v => [['column-gap', length('column-gap', v)]],
  padding: v => sides('padding', v, false),
  margin: v => sides('margin', v, true),
  'padding-top': v => [['padding-top', length('padding-top', v, { percent: true })]],
  'padding-right': v => [['padding-right', length('padding-right', v, { percent: true })]],
  'padding-bottom': v => [['padding-bottom', length('padding-bottom', v, { percent: true })]],
  'padding-left': v => [['padding-left', length('padding-left', v, { percent: true })]],
  'margin-top': v => [['margin-top', length('margin-top', v, { percent: true, auto: true, negative: true })]],
  'margin-right': v => [['margin-right', length('margin-right', v, { percent: true, auto: true, negative: true })]],
  'margin-bottom': v => [['margin-bottom', length('margin-bottom', v, { percent: true, auto: true, negative: true })]],
  'margin-left': v => [['margin-left', length('margin-left', v, { percent: true, auto: true, negative: true })]],
  width: v => [['width', length('width', v, { percent: true, auto: true })]],
  height: v => [['height', length('height', v, { percent: true, auto: true })]],
  'min-width': v => [['min-width', length('min-width', v, { percent: true })]],
  'min-height': v => [['min-height', length('min-height', v, { percent: true })]],
  'max-width': v => (v === 'none' ? [] : [['max-width', length('max-width', v, { percent: true })]]),
  'max-height': v => (v === 'none' ? [] : [['max-height', length('max-height', v, { percent: true })]]),
  'border-radius': v => {
    if (words(v).length !== 1) throw new Error('border-radius takes one length');
    return [['border-radius', length('border-radius', v)]];
  },
  'font-size': v => [['font-size', length('font-size', v)]],
  'font-weight': v => [['font-weight', oneOf('font-weight', v, ['400', '500', '600', '700', 'normal', 'bold'])]],
  'font-family': v => {
    const family = v.split(',')[0].trim().replace(/^["']|["']$/g, '');
    if (!family || /[;{}]/.test(family)) throw new Error(`invalid font-family '${v}'`);
    return [['font-family', family]];
  },
  'font-style': v => [['font-style', oneOf('font-style', v === 'oblique' ? 'italic' : v, ['normal', 'italic'])]],
  'line-height': v => {
    if (v === 'normal') return [];
    if (/^\d*\.?\d+$/.test(v)) return [['line-height', num(v)]];
    return [['line-height', length('line-height', v)]];
  },
  'text-align': v => [['text-align', oneOf('text-align', v === 'start' ? 'left' : v === 'end' ? 'right' : v, ['left', 'center', 'right'])]],
  'white-space': v => [['white-space', oneOf('white-space', v, ['normal', 'nowrap'])]],
  'text-overflow': v => [['text-overflow', oneOf('text-overflow', v, ['clip', 'ellipsis'])]],
  overflow: v => [['overflow', oneOf('overflow', v, ['visible', 'hidden', 'scroll', 'auto'])]],
  'grid-template-columns': v => [['grid-template-columns', gridTracks('grid-template-columns', v)]],
  'grid-template-rows': v => [['grid-template-rows', gridTracks('grid-template-rows', v)]],
  opacity: v => {
    const amount = Number(v);
    if (!/^(\d*\.?\d+|1(\.0+)?)$/.test(v) || amount < 0 || amount > 1) throw new Error(`Unsupported opacity '${v}'`);
    return [['opacity', num(amount)]];
  },
  color: v => [['color', color('color', v)]],
  background: v => [['background', background('background', v)]],
  'background-color': v => [['background', color('background-color', v)]],
  border: v => border(v),
  'border-width': v => [['border-width', length('border-width', v)]],
  'border-color': v => [['border-color', color('border-color', v)]],
  'border-style': v => [['border-style', borderStyle(v)]],
  'box-shadow': v => (v === 'none' ? [] : [['box-shadow', shadows(v)]]),
  position: v => {
    const position = oneOf('position', v, ['static', 'relative', 'absolute']);
    return [['position', position === 'static' ? 'relative' : position]];
  },
  top: v => insetSides(`${v} auto auto auto`).filter(([key]) => key === 'top'),
  right: v => insetSides(`auto ${v} auto auto`).filter(([key]) => key === 'right'),
  bottom: v => insetSides(`auto auto ${v} auto`).filter(([key]) => key === 'bottom'),
  left: v => insetSides(`auto auto auto ${v}`).filter(([key]) => key === 'left'),
  inset: v => insetSides(v),
  flex: v => {
    const parts = words(v);
    if (parts.length === 1 && parts[0] === 'none') return [['flex-grow', '0'], ['flex-shrink', '0'], ['flex-basis', 'auto']];
    if (parts.length === 1 && parts[0] === 'auto') return [['flex-grow', '1'], ['flex-shrink', '1'], ['flex-basis', 'auto']];
    if (parts.length === 1 && /^\d*\.?\d+$/.test(parts[0])) return [['flex-grow', num(parts[0])], ['flex-shrink', '1'], ['flex-basis', '0px']];
    if (parts.length === 1) return [['flex-grow', '1'], ['flex-shrink', '1'], ['flex-basis', length('flex', parts[0], { percent: true, auto: true })]];
    if (parts.length === 2 && /^\d*\.?\d+$/.test(parts[1])) return [['flex-grow', number('flex', parts[0])], ['flex-shrink', num(parts[1])], ['flex-basis', '0px']];
    if (parts.length === 2) return [['flex-grow', number('flex', parts[0])], ['flex-shrink', '1'], ['flex-basis', length('flex', parts[1], { percent: true, auto: true })]];
    if (parts.length === 3) return [['flex-grow', number('flex', parts[0])], ['flex-shrink', number('flex', parts[1])], ['flex-basis', length('flex', parts[2], { percent: true, auto: true })]];
    throw new Error(`unsupported flex value '${v}'`);
  },
  'flex-grow': v => [['flex-grow', number('flex-grow', v)]],
  'flex-shrink': v => [['flex-shrink', number('flex-shrink', v)]],
  'flex-basis': v => [['flex-basis', length('flex-basis', v, { percent: true, auto: true })]],
  cursor: v => {
    const cursor = v === 'auto' ? 'default' : v;
    return [['cursor', oneOf('cursor', cursor, ['default', 'pointer', 'text', 'not-allowed', 'grab', 'move', 'crosshair'])]];
  },
};

function normalizeDeclaration(prop, value) {
  if (IGNORED.has(prop)) return [];
  if (UNSUPPORTED.has(prop)) throw new Error(`'${prop}' is not supported yet`);
  const handler = HANDLERS[prop];
  if (!handler) throw new Error(`unknown CSS property '${prop}'`);
  return handler(value);
}

function resolveVariables(value, variables, depth = 0) {
  if (!value.includes('var(')) return value;
  if (depth > 8) throw new Error('CSS variables are nested too deeply');
  return value.replace(/var\(\s*(--[\w-]+)\s*(?:,([^()]*))?\)/g, (_, name, fallback) => {
    if (variables.has(name)) return resolveVariables(variables.get(name), variables, depth + 1);
    if (fallback !== undefined) return resolveVariables(fallback.trim(), variables, depth + 1);
    throw new Error(`undefined CSS variable ${name}`);
  });
}

function collectVariables(container, variables) {
  container.each(node => {
    if (node.type === 'atrule' && node.nodes) collectVariables(node, variables);
    else if (node.type === 'rule') {
      node.each(decl => {
        if (decl.type === 'decl' && decl.prop.trim().startsWith('--')) variables.set(decl.prop.trim(), decl.value.trim());
      });
    }
  });
}

function parseCompound(token, text) {
  const match = /^(\*|[a-zA-Z][\w-]*)?((?:[.#:][\w-]+)*)$/.exec(token);
  if (!match) throw new Error(`Unsupported CSS selector: '${text}'`);
  const tag = match[1] && match[1] !== '*' ? match[1].toLowerCase() : null;
  const ids = [], classes = [], pseudos = [];
  for (const [, mark, name] of match[2].matchAll(/([.#:])([\w-]+)/g)) {
    if (mark === '#') ids.push(name);
    else if (mark === '.') classes.push(name);
    else pseudos.push(name);
  }
  if (pseudos.length > 1 || pseudos.some(name => !STATE_PSEUDOS.has(name))) throw new Error(`Unsupported CSS selector: '${text}'`);
  return { tag, ids, classes, pseudo: pseudos[0] ?? null };
}

// Supports compound selectors, descendant (space) and child (>) combinators, and a state pseudo-class
// (:hover, :active, :focus, :disabled) on the last compound only.
function parseSelector(selector) {
  const text = selector.trim();
  if (!text) throw new Error('Empty CSS selector');
  if (/[+~[\]|]|::/.test(text)) throw new Error(`Unsupported CSS selector: '${text}'`);
  const tokens = text.replace(/\s*>\s*/g, ' > ').split(/\s+/);
  const parts = [];
  let combinator = ' ';
  for (const token of tokens) {
    if (token === '>') {
      if (!parts.length) throw new Error(`Unsupported CSS selector: '${text}'`);
      combinator = '>';
      continue;
    }
    const part = parseCompound(token, text);
    part.combinator = parts.length ? combinator : null;
    parts.push(part);
    combinator = ' ';
  }
  if (combinator === '>') throw new Error(`Unsupported CSS selector: '${text}'`);
  if (parts.slice(0, -1).some(part => part.pseudo)) throw new Error(`Unsupported CSS selector: '${text}'`);
  const specificity = parts.reduce((sum, part) => sum + part.ids.length * 100 + (part.classes.length + (part.pseudo ? 1 : 0)) * 10 + (part.tag ? 1 : 0), 0);
  return { parts, pseudo: parts.at(-1).pseudo, specificity };
}

function matchesSimple(node, part) {
  if (node.tag === '#text') return false;
  if (part.tag && node.tag !== part.tag) return false;
  if (part.ids.some(id => node.attrs.id !== id)) return false;
  const classes = (node.attrs.class || '').split(/\s+/);
  return part.classes.every(name => classes.includes(name));
}

// `node` matches parts[i]. The ancestors[0..depth) are the candidates for parts[i - 1].
function matchesAt(node, ancestors, depth, parts, i) {
  if (!matchesSimple(node, parts[i])) return false;
  if (i === 0) return true;
  if (parts[i].combinator === '>') return depth > 0 && matchesAt(ancestors[depth - 1], ancestors, depth - 1, parts, i - 1);
  for (let d = depth - 1; d >= 0; d--) if (matchesAt(ancestors[d], ancestors, d, parts, i - 1)) return true;
  return false;
}

function matches(node, ancestors, parts) {
  return matchesAt(node, ancestors, ancestors.length, parts, parts.length - 1);
}

function mediaSupported(query) {
  const pattern = /\((min|max)-(width|height):\s*\d+(?:\.\d+)?px\)/g;
  const conditions = query.match(pattern) ?? [];
  const leftover = query.replace(pattern, '').replace(/\band\b/gi, '').trim();
  return conditions.length > 0 && leftover === '';
}

// Compiles a stylesheet. Returns the rules and the warnings for declarations that were skipped.
// `prelude` is built-in CSS, such as the component library. It is compiled strictly, and its rules
// come before this stylesheet's rules. Its custom properties are defaults: this stylesheet's custom
// properties override them, so `:root { --ts-accent: red }` also restyles the library's rules.
// `overrides` is the same mechanism, passed down when this call is compiling a prelude.
export function compileStylesheet(cssText, { strict = false, prelude = '', overrides = null } = {}) {
  const parsed = postcss.parse(cssText);
  const warnings = [];
  const own = new Map();
  collectVariables(parsed, own);
  const variables = new Map();
  if (prelude) collectVariables(postcss.parse(prelude), variables);
  for (const [name, value] of own) variables.set(name, value);
  for (const [name, value] of overrides ?? []) variables.set(name, value);
  const base = prelude ? compileStylesheet(prelude, { strict: true, overrides: own }) : { rules: [] };
  const report = (node, message) => {
    const line = node?.source?.start?.line;
    const text = line ? `line ${line}: ${message}` : message;
    if (strict) throw new Error(text);
    warnings.push(text);
  };
  const rules = [...base.rules];
  let order = rules.length;
  const walk = (container, media = null) => container.each(node => {
    if (node.type === 'comment') return;
    if (node.type === 'atrule') {
      const query = (node.params || '').trim();
      if (node.name !== 'media' || !node.nodes) {
        report(node, `unsupported @${node.name}`);
        return;
      }
      if (!mediaSupported(query)) {
        report(node, `unsupported @media condition '${query}'`);
        return;
      }
      walk(node, query);
      return;
    }
    if (node.type !== 'rule') {
      report(node, 'unsupported CSS syntax');
      return;
    }
    if (node.selector.trim() === ':root') {
      // Custom properties were collected above. Other declarations on :root are not applied.
      node.each(decl => {
        if (decl.type === 'decl' && !decl.prop.trim().startsWith('--')) report(decl, `declarations on :root are not supported; use a class or element selector`);
      });
      return;
    }
    let selectors;
    try {
      selectors = node.selector.split(',').map(parseSelector);
    } catch (error) {
      report(node, error.message);
      return;
    }
    const declarations = [];
    node.each(decl => {
      if (decl.type === 'comment') return;
      if (decl.type !== 'decl') {
        report(decl, 'unsupported CSS declaration syntax');
        return;
      }
      const prop = decl.prop.trim().toLowerCase();
      if (prop.startsWith('--')) return;
      try {
        const value = resolveVariables(decl.value.trim(), variables);
        for (const [key, normalized] of normalizeDeclaration(prop, value)) declarations.push([key, normalized, Boolean(decl.important)]);
      } catch (error) {
        report(decl, error.message);
      }
    });
    for (const selector of selectors) {
      if (selector.pseudo) {
        if (media) {
          report(node, `:${selector.pseudo} inside @media is not supported`);
          continue;
        }
        const rejected = [...new Set(declarations.filter(([key]) => !STATE_PROPERTIES.has(key)).map(([key]) => key))];
        if (rejected.length) report(node, `${rejected.join(', ')} cannot change on :${selector.pseudo}`);
        rules.push({ ...selector, declarations: declarations.filter(([key]) => STATE_PROPERTIES.has(key)), order: order++, media: null });
      } else {
        rules.push({ ...selector, declarations, order: order++, media });
      }
    }
  });
  walk(parsed);
  return { rules, warnings };
}

// Kept for callers that only need the rules. Use compileStylesheet to receive the warnings.
export function compileCSS(cssText) {
  return compileStylesheet(cssText).rules;
}

// Non-important declarations first, then !important ones, so !important wins regardless of order.
function applyDeclarations(target, sortedRules) {
  for (const rule of sortedRules) for (const [key, value, important] of rule.declarations) if (!important) target[key] = value;
  for (const rule of sortedRules) for (const [key, value, important] of rule.declarations) if (important) target[key] = value;
}

function sortRules(candidates) {
  return candidates.sort((a, b) => a.specificity - b.specificity || a.order - b.order);
}

export function styleTree(root, rules) {
  function apply(node, ancestors, inherited, inheritedResponsive = []) {
    const candidates = sortRules(rules.filter(rule => !rule.media && !rule.pseudo && matches(node, ancestors, rule.parts)));
    const computed = { ...inherited };
    applyDeclarations(computed, candidates);
    node.style = computed;

    const states = {};
    for (const name of STATE_PSEUDOS) {
      const stateRules = sortRules(rules.filter(rule => !rule.media && rule.pseudo === name && matches(node, ancestors, rule.parts)));
      if (!stateRules.length) continue;
      const values = {};
      applyDeclarations(values, stateRules);
      states[name] = values;
    }
    if (Object.keys(states).length) node.states = states;
    else delete node.states;

    const ownProperties = new Set(candidates.flatMap(rule => rule.declarations.map(([key]) => key)));
    const responsiveInheritance = inheritedResponsive.map(rule => ({
      ...rule,
      specificity: 0,
      order: 0,
      declarations: Object.fromEntries(Object.entries(rule.declarations).filter(([key]) => INHERITED.includes(key) && !ownProperties.has(key)))
    })).filter(rule => Object.keys(rule.declarations).length);
    const responsiveOwn = rules
      .filter(rule => rule.media && matches(node, ancestors, rule.parts))
      .sort((a, b) => a.specificity - b.specificity || a.order - b.order)
      .map(rule => ({
        media: rule.media,
        specificity: rule.specificity,
        order: rule.order,
        declarations: Object.fromEntries(rule.declarations.map(([key, value]) => [key, value]))
      }));
    node.responsive = [...responsiveInheritance, ...responsiveOwn];

    const nextInherited = {};
    for (const key of INHERITED) if (computed[key] !== undefined) nextInherited[key] = computed[key];
    for (const child of node.children) apply(child, [...ancestors, node], nextInherited, node.responsive);
  }
  apply(root, [], {});
  return root;
}
