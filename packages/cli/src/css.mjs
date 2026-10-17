import postcss from 'postcss';

const SUPPORTED = new Set([
  'display','flex-direction','align-items','justify-content','gap',
  'padding','margin','background','background-color','color','border-radius',
  'width','height','font-size','font-weight'
]);
const INHERITED = ['color', 'font-size', 'font-weight'];
const LENGTH_PROPERTIES = new Set(['gap','padding','margin','border-radius','width','height','font-size']);
const COLOR_PROPERTIES = new Set(['background','background-color','color']);

function parseSimple(token) {
  if (!/^(?:[a-zA-Z][\w-]*)?(?:[.#][\w-]+)*$/.test(token) || !token) throw new Error(`Unsupported CSS selector: '${token}'`);
  const tag = /^[a-zA-Z][\w-]*/.exec(token)?.[0]?.toLowerCase() ?? null;
  const ids = [...token.matchAll(/#([\w-]+)/g)].map(m => m[1]);
  const classes = [...token.matchAll(/\.([\w-]+)/g)].map(m => m[1]);
  return {tag, ids, classes, specificity: ids.length * 100 + classes.length * 10 + (tag ? 1 : 0)};
}
function parseSelector(selector) {
  if (/[>:~+\[\]*]/.test(selector)) throw new Error(`Unsupported CSS selector: '${selector}'`);
  const parts = selector.trim().split(/\s+/).map(parseSimple);
  if (!parts.length) throw new Error('Empty CSS selector');
  return { parts, specificity: parts.reduce((s,p) => s + p.specificity, 0) };
}
function matchesSimple(node, selector) {
  if (node.tag === '#text') return false;
  if (selector.tag && node.tag !== selector.tag) return false;
  if (selector.ids.some(id => node.attrs.id !== id)) return false;
  const classes = (node.attrs.class || '').split(/\s+/);
  return selector.classes.every(name => classes.includes(name));
}
function matchesSelector(node, ancestors, parts) {
  if (!matchesSimple(node, parts.at(-1))) return false;
  let search = ancestors.length - 1;
  for (let i=parts.length-2; i>=0; i--) {
    while (search >= 0 && !matchesSimple(ancestors[search], parts[i])) search--;
    if (search < 0) return false;
    search--;
  }
  return true;
}
function validate(name,value) {
  if (!SUPPORTED.has(name)) throw new Error(`Unsupported CSS property '${name}' in MVP (no WebView).`);
  if (name === 'display' && value !== 'flex' && value !== 'block') throw new Error(`Only display: flex and block are supported; received '${value}'`);
  if (name === 'flex-direction' && value !== 'row' && value !== 'column') throw new Error(`Unsupported flex-direction '${value}'`);
  if (name === 'align-items' && !['center','flex-start','flex-end','stretch'].includes(value)) throw new Error(`Unsupported align-items '${value}'`);
  if (name === 'justify-content' && !['center','flex-start','flex-end','space-between','space-around'].includes(value)) throw new Error(`Unsupported justify-content '${value}'`);
  if (LENGTH_PROPERTIES.has(name) && !/^\d+(?:\.\d+)?px$/.test(value)) throw new Error(`CSS '${name}: ${value}' needs a nonnegative px value in MVP`);
  if (COLOR_PROPERTIES.has(name) && !/^#(?:[0-9a-fA-F]{3}|[0-9a-fA-F]{6})$/.test(value)) throw new Error(`CSS '${name}: ${value}' needs #RGB or #RRGGBB`);
  if (name === 'font-weight' && !['400','500','600','700','normal','bold'].includes(value)) throw new Error(`Unsupported font-weight '${value}'`);
}
export function compileCSS(cssText) {
  const parsed = postcss.parse(cssText);
  const rules = [];
  let order = 0;
  parsed.each(rule => {
    if (rule.type === 'comment') return;
    if (rule.type !== 'rule') throw new Error(`Unsupported CSS @rule: ${rule.name || rule.type}`);
    const selectors = rule.selector.split(',').map(s => parseSelector(s));
    const declarations = [];
    rule.each(decl => {
      if (decl.type === 'comment') return;
      if (decl.type !== 'decl') throw new Error(`Unsupported CSS declaration syntax in ${rule.selector}`);
      const prop = decl.prop.toLowerCase().trim();
      const value = decl.value.trim();
      if (decl.important) throw new Error('!important is not supported in MVP');
      validate(prop,value);
      declarations.push([prop === 'background-color' ? 'background' : prop, value]);
    });
    for (const selector of selectors) rules.push({ ...selector, declarations, order: order++ });
  });
  return rules;
}

export function styleTree(root, rules) {
  function apply(node, ancestors, inherited) {
    const computed = { ...inherited };
    const candidates = rules.filter(r => matchesSelector(node,ancestors,r.parts));
    candidates.sort((a,b) => a.specificity-b.specificity || a.order-b.order);
    for (const candidate of candidates) for (const [key, value] of candidate.declarations) computed[key]=value;
    node.style = computed;
    const nextInherited = {};
    for (const key of INHERITED) if (computed[key] !== undefined) nextInherited[key]=computed[key];
    for (const child of node.children) apply(child,[...ancestors,node], nextInherited);
  }
  apply(root,[],{});
  return root;
}
