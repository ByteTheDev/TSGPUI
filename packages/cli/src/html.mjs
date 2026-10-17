const ALLOWED_TAGS = new Set([
  'html', 'body', 'main', 'header', 'footer', 'section', 'article', 'div',
  'h1', 'h2', 'h3', 'p', 'span', 'strong', 'small', 'button', 'label',
  'input', 'textarea', 'select', 'option', 'form', 'ul', 'ol', 'li', 'img',
  'template', 'use'
]);
const VOID_TAGS = new Set(['input', 'img', 'use']);
const ALLOWED_ATTRS = new Set([
  'id', 'class', 'data-onclick', 'data-oninput', 'data-submit', 'data-bind', 'data-repeat',
  'title', 'aria-label', 'aria-description', 'role', 'tabindex', 'type', 'name', 'value',
  'placeholder', 'checked', 'disabled', 'required', 'min', 'max', 'step',
  'alt', 'src', 'for', 'data-shortcut', 'data-component', 'component', 'selected',
  'data-clipboard-copy', 'data-clipboard-paste', 'data-file-open', 'data-file-read', 'data-file-save',
  'data-confirm', 'data-fetch', 'data-fetch-into'
]);
const ACTION_RE = /^[A-Za-z_$][A-Za-z0-9_$]*$/;
const BIND_RE = /^[A-Za-z_$][A-Za-z0-9_$]*(?:\.(?:[A-Za-z_$][A-Za-z0-9_$]*|\d+))*$/;

function decodeEntities(input) {
  return input.replace(/&(?:amp|lt|gt|quot|apos|#\d+|#x[0-9a-f]+);/gi, entity => {
    const names = { '&amp;': '&', '&lt;': '<', '&gt;': '>', '&quot;': '"', '&apos;': "'" };
    const lower = entity.toLowerCase();
    if (names[lower] !== undefined) return names[lower];
    const code = lower.startsWith('&#x') ? Number.parseInt(lower.slice(3, -1), 16) : Number.parseInt(lower.slice(2, -1), 10);
    return Number.isInteger(code) && code >= 0 && code <= 0x10ffff && !(code >= 0xd800 && code <= 0xdfff)
      ? String.fromCodePoint(code) : '\ufffd';
  });
}

function readAttributes(src) {
  const attrs = {};
  let index = 0;
  while (index < src.length) {
    while (/\s/.test(src[index] ?? '')) index++;
    if (index >= src.length) break;
    const match = /^[^\s=<>"'`/]+/.exec(src.slice(index));
    if (!match) throw new Error(`Invalid HTML attribute near: ${src.slice(index, index + 25)}`);
    const name = match[0].toLowerCase();
    index += match[0].length;
    while (/\s/.test(src[index] ?? '')) index++;
    let value = '';
    if (src[index] === '=') {
      index++;
      while (/\s/.test(src[index] ?? '')) index++;
      const quote = src[index] === '"' || src[index] === "'" ? src[index++] : null;
      if (quote) {
        const end = src.indexOf(quote, index);
        if (end === -1) throw new Error(`Unterminated ${name} attribute`);
        value = src.slice(index, end);
        index = end + 1;
      } else {
        const valueMatch = /^[^\s<>"'`=]+/.exec(src.slice(index));
        if (!valueMatch) throw new Error(`Missing ${name} attribute value`);
        value = valueMatch[0];
        index += value.length;
      }
    }
    if (!ALLOWED_ATTRS.has(name) && !/^data-prop-[a-z][a-z0-9-]*$/.test(name)) throw new Error(`Unsupported HTML attribute '${name}'. Supported: ${[...ALLOWED_ATTRS].join(', ')}`);
    if (Object.hasOwn(attrs, name)) throw new Error(`Duplicate HTML attribute: ${name}`);
    attrs[name] = decodeEntities(value);
  }
  if (attrs['data-onclick'] && !ACTION_RE.test(attrs['data-onclick'])) throw new Error(`Invalid data-onclick action: ${attrs['data-onclick']}`);
  if (attrs['data-oninput'] && !ACTION_RE.test(attrs['data-oninput'])) throw new Error(`Invalid data-oninput action: ${attrs['data-oninput']}`);
  if (attrs['data-submit'] && !ACTION_RE.test(attrs['data-submit'])) throw new Error(`Invalid data-submit action: ${attrs['data-submit']}`);
  if (attrs['data-bind'] && !BIND_RE.test(attrs['data-bind'])) throw new Error(`Invalid data-bind key: ${attrs['data-bind']}`);
  for (const name of ['data-clipboard-copy', 'data-clipboard-paste', 'data-file-open']) {
    if (attrs[name] && !BIND_RE.test(attrs[name])) throw new Error(`Invalid ${name} state key: ${attrs[name]}`);
  }
  if (attrs['data-confirm'] && !ACTION_RE.test(attrs['data-confirm'])) throw new Error(`Invalid data-confirm action: ${attrs['data-confirm']}`);
  if (attrs['data-fetch-into'] && !BIND_RE.test(attrs['data-fetch-into'])) throw new Error(`Invalid data-fetch-into state key: ${attrs['data-fetch-into']}`);
  if (attrs['data-fetch'] && !attrs['data-fetch-into']) throw new Error('data-fetch requires data-fetch-into');
  if (attrs['data-fetch-into'] && !attrs['data-fetch']) throw new Error('data-fetch-into requires data-fetch');
  if (attrs['data-file-read'] && !BIND_RE.test(attrs['data-file-read'])) throw new Error(`Invalid data-file-read state key: ${attrs['data-file-read']}`);
  if (attrs['data-file-save'] && !BIND_RE.test(attrs['data-file-save'])) throw new Error(`Invalid data-file-save state key: ${attrs['data-file-save']}`);
  if (attrs['data-file-read'] && !attrs['data-file-open']) throw new Error('data-file-read requires data-file-open');
  if (attrs['data-file-save'] && attrs['data-file-open']) throw new Error('Use separate buttons for opening and saving files');
  if (attrs['data-fetch'] && attrs['data-onclick']) throw new Error('Use either data-fetch or data-onclick on a button, not both');
  const clickOperations = ['data-confirm','data-fetch','data-file-open','data-file-save','data-clipboard-copy','data-clipboard-paste'].filter(name => attrs[name]);
  if (clickOperations.length > 1) throw new Error(`A button can use only one native operation: ${clickOperations.join(', ')}`);
  if (attrs.type === 'submit' && clickOperations.length) throw new Error('Submit buttons only dispatch their form data-submit action');
  if (attrs['data-repeat'] && !/^[A-Za-z_$][A-Za-z0-9_$]*(?:\.(?:[A-Za-z_$][A-Za-z0-9_$]*|\d+))*(?:\s+as\s+[A-Za-z_$][A-Za-z0-9_$]*)?$/.test(attrs['data-repeat'])) throw new Error(`Invalid data-repeat expression: ${attrs['data-repeat']}`);
  if (attrs['data-shortcut'] && !/^(?:(?:ctrl|alt|shift|cmd|mod)-)*[\w]+$/i.test(attrs['data-shortcut'])) throw new Error(`Invalid data-shortcut: ${attrs['data-shortcut']}`);
  return attrs;
}

/** Strict, intentionally small HTML subset. Reject unsupported HTML rather than silently misrendering it. */
export function parseHTML(source) {
  let index = 0;
  let serial = 0;
  const document = { tag: '#document', key: serial++, attrs: {}, children: [] };
  const stack = [document];
  while (index < source.length) {
    const rest = source.slice(index);
    if (rest.startsWith('<!--')) {
      const close = source.indexOf('-->', index + 4);
      if (close < 0) throw new Error('Unclosed HTML comment');
      index = close + 3; continue;
    }
    if (/^<!doctype\s+html\s*>/i.test(rest)) {
      index += /^<!doctype\s+html\s*>/i.exec(rest)[0].length; continue;
    }
    if (rest[0] === '<') {
      const token = /^<\/?[a-zA-Z][^>]*>/.exec(rest);
      if (!token) throw new Error(`Malformed or unsupported HTML at offset ${index}`);
      index += token[0].length;
      const closing = token[0].startsWith('</');
      const tagMatch = /^<\/?([a-zA-Z][a-zA-Z0-9-]*)/.exec(token[0]);
      const tag = tagMatch[1].toLowerCase();
      if (!ALLOWED_TAGS.has(tag)) throw new Error(`Unsupported HTML element <${tag}>. HTML is translated to native GPUI, not Chromium.`);
      if (closing) {
        if (stack.length <= 1 || stack.at(-1).tag !== tag) throw new Error(`Unexpected closing tag </${tag}>`);
        stack.pop();
        continue;
      }
      if (token[0].endsWith('/>') && !VOID_TAGS.has(tag)) throw new Error(`Self-closing <${tag}/> is not supported`);
      const attrsText = token[0].slice(tagMatch[0].length, -1);
      const node = { tag, key: serial++, attrs: readAttributes(attrsText), children: [] };
      stack.at(-1).children.push(node);
      if (!VOID_TAGS.has(tag)) stack.push(node);
      continue;
    }
    const next = source.indexOf('<', index);
    const raw = source.slice(index, next === -1 ? undefined : next);
    index += raw.length;
    const text = decodeEntities(raw.replace(/\s+/g, ' ').trim());
    if (text) stack.at(-1).children.push({ tag: '#text', key: serial++, text, attrs: {}, children: [] });
  }
  if (stack.length !== 1) throw new Error(`Unclosed HTML element <${stack.at(-1).tag}>`);
  if (document.children.length !== 1) throw new Error('HTML must have one root element');
  let root = document.children[0];
  if (root.tag === 'html') {
    const bodies = root.children.filter(child => child.tag === 'body');
    if (bodies.length !== 1 || root.children.length !== 1) throw new Error('<html> must contain exactly one <body> for MVP');
    root = bodies[0];
  }
  if (!['body', 'main', 'div', 'section'].includes(root.tag)) throw new Error('Root element must be <body>, <main>, <div>, or <section>');
  const ids = new Set();
  const visit = (node, parent = null) => {
    if (node.attrs.id) {
      if (ids.has(node.attrs.id)) throw new Error(`Duplicate HTML id: ${node.attrs.id}`);
      ids.add(node.attrs.id);
    }
    if (node.attrs['data-onclick'] && node.tag !== 'button') throw new Error('data-onclick currently supports <button> only');
    if (node.attrs['data-oninput'] && !['input','textarea','select'].includes(node.tag)) throw new Error('data-oninput only applies to editable controls');
    if (node.attrs['data-submit'] && node.tag !== 'form') throw new Error('data-submit only applies to <form>');
    if (node.attrs['data-shortcut'] && node.tag !== 'button') throw new Error('data-shortcut only applies to <button>');
    if (node.attrs['data-shortcut'] && !node.attrs['data-onclick']) throw new Error('data-shortcut requires data-onclick');
    if (node.attrs['data-component'] && node.tag !== 'template') throw new Error('data-component only applies to <template>');
    if (node.attrs.component && node.tag !== 'use') throw new Error('component only applies to <use>');
    if (Object.keys(node.attrs).some(name => name.startsWith('data-prop-')) && node.tag !== 'use') throw new Error('data-prop-* attributes only apply to <use>');
    for (const name of ['data-clipboard-copy', 'data-clipboard-paste', 'data-file-open', 'data-file-read', 'data-file-save', 'data-confirm', 'data-fetch', 'data-fetch-into']) {
      if (node.attrs[name] && node.tag !== 'button') throw new Error(`${name} only applies to <button>`);
    }
    if (node.tag === 'button' && node.attrs.type && !['button','submit'].includes(node.attrs.type)) throw new Error(`Unsupported button type '${node.attrs.type}'`);
    if (node.tag === 'button' && node.attrs.type === 'submit' && node.attrs['data-onclick']) throw new Error('Submit buttons use the enclosing form data-submit action');
    if (node.tag === 'input' && !['text','number','checkbox','email','password','search'].includes(node.attrs.type ?? 'text')) throw new Error(`Unsupported input type '${node.attrs.type}'`);
    if (node.attrs.type && !['button','input'].includes(node.tag)) throw new Error(`type is not supported on <${node.tag}>`);
    if (node.attrs.checked && !(node.tag === 'input' && (node.attrs.type ?? 'text') === 'checkbox')) throw new Error('checked only applies to checkbox inputs');
    if (['min','max','step'].some(name => node.attrs[name] !== undefined) && !(node.tag === 'input' && node.attrs.type === 'number')) throw new Error('min, max, and step only apply to number inputs');
    for (const name of ['min','max','step']) {
      if (node.attrs[name] !== undefined && !/^-?\d+(?:\.\d+)?$/.test(node.attrs[name])) throw new Error(`${name} must be a number`);
    }
    if (node.attrs.step !== undefined && Number(node.attrs.step) <= 0) throw new Error('step must be greater than zero');
    if (node.tag === 'option' && parent?.tag !== 'select') throw new Error('<option> must be inside <select>');
    if (node.attrs.selected && node.tag !== 'option') throw new Error('selected only applies to <option>');
    if (node.attrs['data-confirm'] && node.attrs['data-onclick']) throw new Error('Use either data-confirm or data-onclick on a button, not both');
    if (node.tag === 'img' && !node.attrs.src) throw new Error('<img> requires src');
    if (node.attrs.src && /^(?:[a-z]+:|[\\/])|\.\./i.test(node.attrs.src)) throw new Error('Image src must be a project relative path');
    if (node.tag === 'use' && !node.attrs.component) throw new Error('<use> requires a component attribute');
    for (const child of node.children) visit(child, node);
  };
  visit(root);
  const components = new Set();
  const collect = node => {
    if (node.tag === 'template' && node.attrs['data-component']) {
      const name = node.attrs['data-component'];
      if (!ACTION_RE.test(name) || components.has(name)) throw new Error(`Invalid or duplicate component '${name}'`);
      components.add(name);
    }
    for (const child of node.children) collect(child);
  };
  collect(root);
  const checkUses = node => {
    if (node.tag === 'use' && !components.has(node.attrs.component)) throw new Error(`Unknown component '${node.attrs.component}'`);
    for (const child of node.children) checkUses(child);
  };
  checkUses(root);
  const definitions = new Map();
  const stripTemplates = node => {
    node.children = node.children.filter(child => {
      if (child.tag === 'template' && child.attrs['data-component']) {
        definitions.set(child.attrs['data-component'], child.children);
        return false;
      }
      stripTemplates(child);
      return true;
    });
  };
  stripTemplates(root);
  let nextKey = serial;
  const cloneWithKeys = node => ({...node,key:nextKey++,attrs:{...node.attrs},props:node.props ? {...node.props} : undefined,style:node.style ? {...node.style} : undefined,children:node.children.map(cloneWithKeys)});
  const expand = (node, stack = []) => {
    const result = [];
    for (const child of node.children) {
      if (child.tag !== 'use') { expand(child, stack); result.push(child); continue; }
      const name = child.attrs.component;
      if (stack.includes(name)) throw new Error(`Recursive component use: ${[...stack,name].join(' -> ')}`);
      const instances = definitions.get(name).map(cloneWithKeys);
      if (!instances.length) continue;
      const props = Object.fromEntries(Object.entries(child.attrs).filter(([key])=>key.startsWith('data-prop-')).map(([key,value])=>[key.slice(10),value]));
      const forward = Object.fromEntries(Object.entries(child.attrs).filter(([key])=>['id','class','title','aria-label'].includes(key)));
      instances[0].attrs = {...instances[0].attrs,...forward};
      for (const instance of instances) instance.props = {...(instance.props ?? {}),...props};
      for (const instance of instances) { expand(instance, [...stack,name]); result.push(instance); }
    }
    node.children = result;
  };
  expand(root);
  const expandedIds = new Set();
  const checkExpandedIds = node => {
    if (node.attrs.id) {
      if (expandedIds.has(node.attrs.id)) throw new Error(`Duplicate HTML id after component expansion: ${node.attrs.id}`);
      expandedIds.add(node.attrs.id);
    }
    for (const child of node.children) checkExpandedIds(child);
  };
  checkExpandedIds(root);
  return root;
}
