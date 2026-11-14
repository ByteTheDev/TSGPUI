// Rendrix JSX runtime: JSX compiles to direct DOM construction (no virtual DOM).
// `import { jsx } from "rendrix/jsx-runtime"` is what `"jsx": "react-jsx", "jsxImportSource": "rendrix"` emits.

const EVENT_ALIASES = { doubleclick: 'dblclick' };

function setProp(el, key, value) {
  if (key === 'children' || key === 'key') return;
  if (key === 'ref') { if (typeof value === 'function') value(el); else if (value && typeof value === 'object') value.current = el; return; }
  if (key.length > 2 && key.startsWith('on') && typeof value === 'function') {
    let name = key.slice(2).toLowerCase(); name = EVENT_ALIASES[name] || name;
    el.addEventListener(name, value);
    return;
  }
  if (key === 'className' || key === 'class') { if (value != null && value !== false) el.setAttribute('class', Array.isArray(value) ? value.filter(Boolean).join(' ') : String(value)); return; }
  if (key === 'style') {
    if (typeof value === 'string') el.setAttribute('style', value);
    else if (value && typeof value === 'object') for (const k of Object.keys(value)) { if (k.startsWith('--')) el.style.setProperty(k, value[k]); else el.style[k] = value[k]; }
    return;
  }
  if (key === 'dangerouslySetInnerHTML') { el.innerHTML = value && value.__html || ''; return; }
  if (key === 'htmlFor') { el.setAttribute('for', value); return; }
  if (value === false || value == null) return;
  if (key === 'value' || key === 'checked' || key === 'selected') { el[key] = value; if (key === 'value') el.setAttribute('value', String(value)); else el.toggleAttribute(key, !!value); return; }
  el.setAttribute(key, value === true ? '' : String(value));
}

function appendChild(parent, child) {
  if (child == null || child === false || child === true) return;
  if (Array.isArray(child)) { for (const c of child) appendChild(parent, c); return; }
  if (child instanceof Node) { parent.appendChild(child); return; }
  parent.appendChild(document.createTextNode(String(child)));
}

export const Fragment = Symbol('rendrix.fragment');

export function jsx(type, props, key) {
  props = props || {};
  if (typeof type === 'function') return type(props);
  if (type === Fragment) { const f = document.createDocumentFragment(); appendChild(f, props.children); return f; }
  const el = document.createElement(type);
  for (const k of Object.keys(props)) setProp(el, k, props[k]);
  appendChild(el, props.children);
  return el;
}
export const jsxs = jsx;
export const jsxDEV = jsx;

/** Mount a node (or component output) into a container, replacing its children. */
export function render(node, container) {
  container.replaceChildren();
  appendChild(container, typeof node === 'function' ? node() : node);
  return container;
}
