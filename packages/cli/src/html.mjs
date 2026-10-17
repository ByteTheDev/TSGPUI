const ALLOWED_TAGS = new Set([
  'html', 'body', 'main', 'header', 'footer', 'section', 'article', 'div',
  'h1', 'h2', 'h3', 'p', 'span', 'strong', 'small', 'button', 'label'
]);
const VOID_TAGS = new Set([]);
const ALLOWED_ATTRS = new Set(['id', 'class', 'data-onclick', 'data-bind', 'title']);
const ACTION_RE = /^[A-Za-z_$][A-Za-z0-9_$]*$/;
const BIND_RE = /^[A-Za-z_$][A-Za-z0-9_$]*$/;

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
    if (!ALLOWED_ATTRS.has(name)) throw new Error(`Unsupported HTML attribute '${name}'. Supported: ${[...ALLOWED_ATTRS].join(', ')}`);
    if (Object.hasOwn(attrs, name)) throw new Error(`Duplicate HTML attribute: ${name}`);
    attrs[name] = decodeEntities(value);
  }
  if (attrs['data-onclick'] && !ACTION_RE.test(attrs['data-onclick'])) throw new Error(`Invalid data-onclick action: ${attrs['data-onclick']}`);
  if (attrs['data-bind'] && !BIND_RE.test(attrs['data-bind'])) throw new Error(`Invalid data-bind key: ${attrs['data-bind']}`);
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
  const visit = node => {
    if (node.attrs.id) {
      if (ids.has(node.attrs.id)) throw new Error(`Duplicate HTML id: ${node.attrs.id}`);
      ids.add(node.attrs.id);
    }
    if (node.attrs['data-onclick'] && node.tag !== 'button') throw new Error('data-onclick currently supports <button> only');
    for (const child of node.children) visit(child);
  };
  visit(root);
  return root;
}
