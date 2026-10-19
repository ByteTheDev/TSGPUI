const ALLOWED_TAGS = new Set([
  'html', 'body', 'main', 'header', 'footer', 'section', 'article', 'div',
  'h1', 'h2', 'h3', 'p', 'span', 'strong', 'small', 'button', 'label',
  'input', 'textarea', 'select', 'option', 'form', 'ul', 'ol', 'li', 'img', 'menu', 'menuitem', 'hr',
  'template', 'use', 'slot'
]);
// <use> is void when self-closed (<use component="X"/>). With content it takes slot children.
const VOID_TAGS = new Set(['input', 'img', 'use', 'hr']);
const ALLOWED_ATTRS = new Set([
  'id', 'class', 'data-onclick', 'data-oninput', 'data-submit', 'data-bind', 'data-repeat',
  'title', 'aria-label', 'aria-description', 'role', 'tabindex', 'type', 'name', 'value',
  'placeholder', 'checked', 'disabled', 'required', 'min', 'max', 'step',
  'alt', 'src', 'for', 'data-shortcut', 'data-component', 'component', 'selected',
  'data-clipboard-copy', 'data-clipboard-paste', 'data-file-open', 'data-file-read', 'data-file-save',
  'data-confirm', 'data-fetch', 'data-fetch-into', 'slot', 'data-virtual',
  'data-window-drag', 'data-window-control', 'data-app-menu', 'data-contextmenu', 'data-ondrop'
]);
const WINDOW_CONTROLS = ['minimize', 'maximize', 'close'];
const ACTION_RE = /^[A-Za-z_$][A-Za-z0-9_$]*$/;
const BIND_RE = /^[A-Za-z_$][A-Za-z0-9_$]*(?:\.(?:[A-Za-z_$][A-Za-z0-9_$]*|\d+))*$/;
// Slot content is substituted at compile time, because props are literal strings.
const PROP_TEXT_RE = /\{\{\s*props\.([A-Za-z_][A-Za-z0-9_-]*)\s*\}\}/g;

function decodeEntities(input) {
  return input.replace(/&(?:amp|lt|gt|quot|apos|#\d+|#x[0-9a-f]+);/gi, entity => {
    const names = { '&amp;': '&', '&lt;': '<', '&gt;': '>', '&quot;': '"', '&apos;': "'" };
    const lower = entity.toLowerCase();
    if (names[lower] !== undefined) return names[lower];
    const code = lower.startsWith('&#x') ? Number.parseInt(lower.slice(3, -1), 16) : Number.parseInt(lower.slice(2, -1), 10);
    return Number.isInteger(code) && code >= 0 && code <= 0x10ffff && !(code >= 0xd800 && code <= 0xdfff)
      ? String.fromCodePoint(code) : '�';
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
  if (attrs['data-ondrop'] && !ACTION_RE.test(attrs['data-ondrop'])) throw new Error(`Invalid data-ondrop action: ${attrs['data-ondrop']}`);
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

// Rules that apply to one element. `parent` and `inComponent` describe where it sits.
function checkNode(node, parent, inComponent) {
  if (node.attrs.slot !== undefined) {
    if (parent?.tag !== 'use') throw new Error('slot only applies to direct children of <use>');
    if (!ACTION_RE.test(node.attrs.slot)) throw new Error(`Invalid slot name '${node.attrs.slot}'`);
  }
  if (node.tag === 'use') {
    // Other attributes on <use> are forwarded to the component's element, and checked there.
    if (!node.attrs.component) throw new Error('<use> requires a component attribute');
    return;
  }
  if (node.attrs['data-onclick'] && !['button', 'menuitem'].includes(node.tag)) throw new Error('data-onclick currently supports <button> and <menuitem> only');
  if (node.attrs['data-oninput'] && !['input','textarea','select'].includes(node.tag)) throw new Error('data-oninput only applies to editable controls');
  if (node.attrs['data-submit'] && node.tag !== 'form') throw new Error('data-submit only applies to <form>');
  if (node.attrs['data-ondrop'] && ['menu', 'menuitem', 'hr', 'input', 'textarea', 'select', 'option'].includes(node.tag)) throw new Error('data-ondrop only applies to a rendered container or button');
  if (node.attrs['data-shortcut'] && !['button', 'menuitem'].includes(node.tag)) throw new Error('data-shortcut only applies to <button> or <menuitem>');
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
  if (node.tag === 'menuitem' && parent?.tag !== 'menu' && parent?.tag !== 'menuitem') throw new Error('<menuitem> must be inside <menu> or another <menuitem>');
  if (node.tag === 'menu' && node.attrs['data-app-menu'] === undefined && node.attrs.id === undefined) throw new Error('<menu> requires an id for a context menu or data-app-menu for an application menu');
  if (node.attrs['data-app-menu'] !== undefined && (node.tag !== 'menu' || !node.attrs['data-app-menu'].trim())) throw new Error('data-app-menu requires a nonempty label on <menu>');
  if (node.attrs['data-contextmenu'] !== undefined && !/^[A-Za-z_$][A-Za-z0-9_$-]*$/.test(node.attrs['data-contextmenu'])) throw new Error('data-contextmenu must name a menu id');
  if (node.tag === 'menuitem' && !node.children.some(child => child.tag === 'menuitem') && !node.attrs['data-onclick']) throw new Error('<menuitem> requires data-onclick unless it contains submenu items');
  if (node.attrs.selected && node.tag !== 'option') throw new Error('selected only applies to <option>');
  if (node.attrs['data-confirm'] && node.attrs['data-onclick']) throw new Error('Use either data-confirm or data-onclick on a button, not both');
  if (node.tag === 'img' && !node.attrs.src) throw new Error('<img> requires src');
  if (node.attrs.src && /^(?:[a-z]+:|[\\/])|\.\./i.test(node.attrs.src)) throw new Error('Image src must be a project relative path');
  if (node.tag === 'slot') {
    if (!inComponent) throw new Error('<slot> is only allowed inside a component template');
    for (const name of Object.keys(node.attrs)) {
      if (name !== 'name') throw new Error(`<slot> only accepts a name attribute, not '${name}'`);
    }
    if (node.attrs.name !== undefined && !ACTION_RE.test(node.attrs.name)) throw new Error(`Invalid slot name '${node.attrs.name}'`);
  }
  if (node.attrs['data-virtual'] !== undefined) {
    if (!node.attrs['data-repeat']) throw new Error('data-virtual requires data-repeat');
    if (!/^\d+$/.test(node.attrs['data-virtual']) || Number(node.attrs['data-virtual']) < 1 || Number(node.attrs['data-virtual']) > 1000) {
      throw new Error('data-virtual is the row height in pixels, from 1 to 1000');
    }
  }
  if (node.attrs['data-window-control'] !== undefined && !WINDOW_CONTROLS.includes(node.attrs['data-window-control'])) {
    throw new Error(`data-window-control must be one of: ${WINDOW_CONTROLS.join(', ')}`);
  }
}

function parseDocument(source, library) {
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
      const selfClosing = token[0].endsWith('/>');
      if (selfClosing && !VOID_TAGS.has(tag)) throw new Error(`Self-closing <${tag}/> is not supported`);
      const attrsText = token[0].slice(tagMatch[0].length, selfClosing ? -2 : -1);
      const node = { tag, key: serial++, attrs: readAttributes(attrsText), children: [] };
      stack.at(-1).children.push(node);
      if (!selfClosing && !(VOID_TAGS.has(tag) && tag !== 'use')) stack.push(node);
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
  const visit = (node, parent = null, inComponent = false) => {
    if (node.attrs.id) {
      if (ids.has(node.attrs.id)) throw new Error(`Duplicate HTML id: ${node.attrs.id}`);
      ids.add(node.attrs.id);
    }
    checkNode(node, parent, inComponent);
    const childInComponent = inComponent || (node.tag === 'template' && node.attrs['data-component'] !== undefined);
    for (const child of node.children) visit(child, node, childInComponent);
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
    if (node.tag === 'use' && !components.has(node.attrs.component) && !library.has(node.attrs.component)) {
      throw new Error(`Unknown component '${node.attrs.component}'`);
    }
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
  // An app's own component wins over a library component with the same name.
  const lookup = name => definitions.get(name) ?? library.get(name);
  let nextKey = serial;
  const cloneWithKeys = node => ({...node,key:nextKey++,attrs:{...node.attrs},props:node.props ? {...node.props} : undefined,style:node.style ? {...node.style} : undefined,children:node.children.map(cloneWithKeys)});
  const substituteProps = (node, scope) => {
    // Without this, content written at the top level would show the props of the component it is placed in.
    if (node.tag === '#text') node.text = node.text.replace(PROP_TEXT_RE, (_match, key) => Object.hasOwn(scope, key) ? scope[key] : '');
    for (const child of node.children) substituteProps(child, scope);
    return node;
  };
  // Expands `use` nodes in a list. `scope` holds the props of the component whose template contains the list.
  const expandList = (nodes, stack, scope) => nodes.flatMap(node => {
    if (node.tag === 'use') return expandUse(node, stack, scope);
    expand(node, stack, scope);
    return [node];
  });
  const expand = (node, stack, scope) => {
    node.children = expandList(node.children, stack, scope);
  };
  const expandUse = (useNode, stack, scope) => {
    const name = useNode.attrs.component;
    if (stack.includes(name)) throw new Error(`Recursive component use: ${[...stack,name].join(' -> ')}`);
    // Content between <use> tags belongs to the caller. Its props are substituted before any nested
    // component is expanded, so a nested template's own {{props.*}} text is not blanked.
    for (const child of useNode.children) substituteProps(child, scope);
    expand(useNode, stack, scope);
    const defaultContent = [];
    const namedContent = new Map();
    for (const child of useNode.children) {
      const content = cloneWithKeys(child);
      if (child.attrs.slot === undefined) {
        defaultContent.push(content);
      } else {
        if (!namedContent.has(child.attrs.slot)) namedContent.set(child.attrs.slot, []);
        namedContent.get(child.attrs.slot).push(content);
      }
    }
    const filled = new Set();
    const fillSlot = slotNode => {
      const slotName = slotNode.attrs.name ?? '';
      filled.add(slotName);
      const provided = slotName ? namedContent.get(slotName) : defaultContent;
      if (provided?.length) return provided.map(cloneWithKeys);
      return fillList(slotNode.children);
    };
    const fillList = nodes => nodes.flatMap(node => {
      if (node.tag === 'slot') return fillSlot(node);
      node.children = fillList(node.children);
      return [node];
    });
    const template = lookup(name) ?? [];
    const instances = fillList(template.map(cloneWithKeys));
    for (const slotName of namedContent.keys()) {
      if (!filled.has(slotName)) throw new Error(`Component ${name} has no slot named '${slotName}'`);
    }
    if (defaultContent.length && !filled.has('')) throw new Error(`Component ${name} has no default slot, but content was given`);
    const props = Object.fromEntries(Object.entries(useNode.attrs).filter(([key])=>key.startsWith('data-prop-')).map(([key,value])=>[key.slice(10),value]));
    for (const instance of instances) instance.props = {...(instance.props ?? {}),...props};
    const expanded = expandList(instances, [...stack,name], props);
    // Attributes written on <use> go onto the first element of the component, so a library
    // component such as SearchField can take data-bind or data-oninput. The result is checked again.
    const forwarded = Object.entries(useNode.attrs).filter(([key]) => key !== 'component' && key !== 'slot' && !key.startsWith('data-prop-'));
    if (forwarded.length) {
      const target = expanded.find(node => node.tag !== '#text');
      if (!target) throw new Error(`<use component="${name}"> has attributes, but the component has no element to receive them`);
      for (const [key, value] of forwarded) {
        if (key === 'class') target.attrs.class = [target.attrs.class, value].filter(Boolean).join(' ');
        else target.attrs[key] = value;
      }
      checkNode(target, null, false);
    }
    return expanded;
  };
  root.children = expandList(root.children, [], {});
  const validateAppMenus = (node, inAppMenu = false) => {
    const insideAppMenu = inAppMenu || node.tag === 'menu' && node.attrs['data-app-menu'] !== undefined;
    if (insideAppMenu && node.tag === 'menuitem' && node.attrs.disabled !== undefined) {
      throw new Error('disabled menu items are not supported by the native application menu API');
    }
    for (const child of node.children) validateAppMenus(child, insideAppMenu);
  };
  validateAppMenus(root);
  // On Windows a drag area is caption, so clicks inside it never reach controls. Components are
  // checked too, so this runs after expansion.
  const INTERACTIVE_ATTRS = ['data-onclick', 'data-oninput', 'data-submit', 'data-window-control', 'data-fetch', 'data-file-open', 'data-file-save', 'data-clipboard-copy', 'data-clipboard-paste', 'data-confirm', 'tabindex'];
  const isInteractive = node => ['button', 'input', 'textarea', 'select', 'form'].includes(node.tag) || INTERACTIVE_ATTRS.some(name => node.attrs[name] !== undefined);
  const checkWindowRegions = node => {
    if (node.attrs['data-window-drag'] !== undefined) {
      const inside = node.children.flatMap(function collect(child) { return [child, ...child.children.flatMap(collect)]; });
      if (inside.some(isInteractive)) {
        throw new Error('data-window-drag cannot contain buttons, inputs, or other interactive elements; put them beside the drag area');
      }
    }
    for (const child of node.children) checkWindowRegions(child);
  };
  checkWindowRegions(root);
  const expandedIds = new Set();
  const checkExpandedIds = node => {
    if (node.attrs.id) {
      if (expandedIds.has(node.attrs.id)) throw new Error(`Duplicate HTML id after component expansion: ${node.attrs.id}`);
      expandedIds.add(node.attrs.id);
    }
    for (const child of node.children) checkExpandedIds(child);
  };
  checkExpandedIds(root);
  const menuIds = new Set();
  const collectMenus = node => {
    if (node.tag === 'menu' && node.attrs.id) menuIds.add(node.attrs.id);
    for (const child of node.children) collectMenus(child);
  };
  collectMenus(root);
  const checkContextMenus = node => {
    if (node.attrs['data-contextmenu'] && !menuIds.has(node.attrs['data-contextmenu'])) {
      throw new Error(`Unknown context menu '${node.attrs['data-contextmenu']}'`);
    }
    for (const child of node.children) checkContextMenus(child);
  };
  checkContextMenus(root);
  return { root, definitions };
}

/** Strict, intentionally small HTML subset. Reject unsupported HTML rather than silently misrendering it. */
export function parseHTML(source, { library = new Map() } = {}) {
  return parseDocument(source, library).root;
}

/** Parses a component library file (a <div> holding <template data-component> entries) into a Map of name to nodes. */
export function parseComponentLibrary(source) {
  return parseDocument(source, new Map()).definitions;
}
