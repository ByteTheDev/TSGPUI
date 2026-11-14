import { readFile, writeFile, mkdir, realpath, rename, lstat } from 'node:fs/promises';
import path from 'node:path';
import { isIP } from 'node:net';
import { parseComponentLibrary, parseHTML } from './html.mjs';
import { compileStylesheet, styleTree } from './css.mjs';
import { compileBackend, bundleBackend } from './backend.mjs';
import { COMPONENT_LIBRARY_CSS, COMPONENT_LIBRARY_HTML } from './library.mjs';

const COMPONENT_LIBRARY = parseComponentLibrary(COMPONENT_LIBRARY_HTML);
const NAME_RE = /^[a-zA-Z][a-zA-Z0-9_-]{0,63}$/;
const HOST_RE = /^(?=.{1,253}$)(?:[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?)(?:\.(?:[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?))*$/i;
// Renderer key semantics (Project Rendrix charter): "rendrix" is the engine and the default (an
// absent key selects it). "native" is the deprecated legacy host, accepted with a warning until the
// Rendrix M1 gate. "gpu" is the old name of the engine, accepted with the same warning. "webview"
// is a hard error with migration guidance.
function rendererKey(raw) {
  if (raw === undefined || raw === null || raw === 'rendrix') return 'rendrix';
  if (raw === 'gpu') return 'rendrix';
  if (raw === 'native') return 'native';
  if (raw === 'webview') throw new Error('renderer "webview" was removed; Rendrix renders natively with Rendrix. Remove the renderer key from rendrix.config.json (it defaults to "rendrix"), or set it to "rendrix", and write the app UI in src/index.html + styles.css.');
  throw new Error(`renderer must be "rendrix" (default) or "native" (deprecated legacy host), not ${JSON.stringify(raw)}`);
}
export function validateConfig(raw) {
  if (!raw || typeof raw !== 'object' || Array.isArray(raw)) throw new Error('rendrix.config.json must be an object');
  for (const key of ['name','html','css','backend']) if (typeof raw[key] !== 'string' || !raw[key]) throw new Error(`Missing string config property: ${key}`);
  if (!NAME_RE.test(raw.name)) throw new Error('Invalid name; use letters, digits, _ or -');
  const version = raw.version ?? '1.0.0';
  if (typeof version !== 'string' || !/^\d{1,4}\.\d{1,4}\.\d{1,4}$/.test(version)) throw new Error('version must be a three-part numeric version, e.g. 1.2.3');
  const bundleId = raw.bundleId ?? `dev.rendrix.${raw.name.replaceAll('_','-')}`;
  if (typeof bundleId !== 'string' || !/^[A-Za-z0-9-]+(?:\.[A-Za-z0-9-]+)+$/.test(bundleId)) throw new Error('bundleId must be a reverse-DNS identifier');
  if (raw.icon !== undefined && (typeof raw.icon !== 'string' || !raw.icon.endsWith('.icns'))) throw new Error('icon must be a project-relative .icns file');
  const width = raw.window?.width ?? 900;
  const height = raw.window?.height ?? 600;
  if (![width,height].every(n => Number.isInteger(n) && n >= 200 && n <= 4096)) throw new Error('Window dimensions must be integers between 200 and 4096');
  if (raw.permissions !== undefined && (!raw.permissions || typeof raw.permissions !== 'object' || Array.isArray(raw.permissions))) {
    throw new Error('permissions must be an object');
  }
  const permissionWarnings = [];
  const legacyKeys = ['filesystem', 'processes', 'network', 'notifications', 'ipc', 'window'];
  for (const key of legacyKeys) {
    if (raw.permissions && Object.prototype.hasOwnProperty.call(raw.permissions, key)) {
      permissionWarnings.push(`legacy permission key '${key}' is deprecated; migrate to operation-scoped grants (filesystem.read, process.spawn, network.request, notifications.show, ipc.send, window.control)`);
    }
  }

  // New operation-scoped keys (optional) override legacy values.
  const newFsRead = raw.permissions?.['filesystem.read'];
  const newFsWrite = raw.permissions?.['filesystem.write'];
  const newProcessSpawn = raw.permissions?.['process.spawn'];
  const newProcessSpawnArgs = raw.permissions?.['process.spawnArgs'];
  const newNetworkRequest = raw.permissions?.['network.request'];
  const newNotifications = raw.permissions?.['notifications.show'];
  const newIpc = raw.permissions?.['ipc.send'];
  const newWindow = raw.permissions?.['window.control'];

  let network = raw.permissions?.network ?? [];
  if (Array.isArray(newNetworkRequest)) {
    network = newNetworkRequest.map(entry => (typeof entry === 'string' ? entry : entry?.host)).filter(Boolean);
  }
  if (!Array.isArray(network) || network.some(host => typeof host !== 'string' || !HOST_RE.test(host) || !host.includes('.') || isIP(host) || ['localhost','local'].includes(host.toLowerCase()) || /\.(?:local|localhost|internal)$/.test(host.toLowerCase()))) {
    throw new Error('permissions.network must contain exact public host names (no wildcards or IP addresses)');
  }
  const networkHosts = [...new Set(network.map(host => host.toLowerCase()))];
  const networkMethods = {};
  if (Array.isArray(newNetworkRequest)) {
    for (const entry of newNetworkRequest) {
      if (entry && typeof entry === 'object') {
        const host = String(entry.host).toLowerCase();
        const methods = Array.isArray(entry.methods) ? entry.methods : [];
        networkMethods[host] = methods.length ? methods.map(m => String(m).toUpperCase()) : [];
      }
    }
  }

  const filesystem = raw.permissions?.filesystem ?? {};
  if (filesystem && (typeof filesystem !== 'object' || Array.isArray(filesystem))) throw new Error('permissions.filesystem must be an object');
  const readRoots = (Array.isArray(newFsRead) ? newFsRead : (filesystem.read ?? []));
  const writeRoots = (Array.isArray(newFsWrite) ? newFsWrite : (filesystem.write ?? []));
  for (const [name, roots] of [['read', readRoots], ['write', writeRoots]]) {
    if (!Array.isArray(roots) || roots.some(root => typeof root !== 'string' || !root.trim() || root.includes('\0') || path.isAbsolute(root) || root.split(/[\\/]+/).includes('..')))
      throw new Error(`permissions.filesystem.${name} must contain project-relative directory paths without '..'`);
  }
  let processes = raw.permissions?.processes ?? [];
  if (Array.isArray(newProcessSpawn)) processes = newProcessSpawn;
  if (!Array.isArray(processes) || processes.some(command => typeof command !== 'string' || !/^[a-zA-Z0-9_.-]+$/.test(command)))
    throw new Error('permissions.processes must contain executable names');
  const processArgs = (newProcessSpawnArgs && typeof newProcessSpawnArgs === 'object' && !Array.isArray(newProcessSpawnArgs)) ? newProcessSpawnArgs : {};

  const notifications = (typeof newNotifications === 'boolean') ? newNotifications : (raw.permissions?.notifications ?? false);
  const ipc = (typeof newIpc === 'boolean') ? newIpc : (raw.permissions?.ipc ?? false);
  const nativeWindow = (typeof newWindow === 'boolean') ? newWindow : (raw.permissions?.window ?? false);
  if (typeof notifications !== 'boolean') throw new Error('permissions.notifications must be true or false');
  if (typeof ipc !== 'boolean') throw new Error('permissions.ipc must be true or false');
  if (typeof nativeWindow !== 'boolean') throw new Error('permissions.window must be true or false');
  const fontFiles = raw.fonts ?? [];
  if (!Array.isArray(fontFiles)) throw new Error('fonts must be an array of {family, src} entries');
  const families = new Set();
  for (const font of fontFiles) {
    if (!font || typeof font.family !== 'string' || !font.family.trim() || typeof font.src !== 'string' || !font.src) throw new Error('Each font must have a nonempty family and src');
    if (families.has(font.family.trim())) throw new Error(`Duplicate font family '${font.family.trim()}'`);
    families.add(font.family.trim());
  }
  if (raw.strictCss !== undefined && typeof raw.strictCss !== 'boolean') throw new Error('strictCss must be true or false');
  // Opt-in persistence of the backend state across app restarts.
  if (raw.persist !== undefined && typeof raw.persist !== 'boolean') throw new Error('persist must be true or false');
  // Where the backend runs. The WebView renderer ran it in an invisible second window ("hidden");
  // that option was removed with the WebView renderer.
  const backendWindow = raw.backendWindow ?? 'primary';
  if (!['primary'].includes(backendWindow)) {
    if (backendWindow === 'hidden') throw new Error('backendWindow "hidden" was removed with the WebView renderer; use "primary"');
    throw new Error('backendWindow must be "primary"');
  }
  const renderer = rendererKey(raw.renderer);
  const rendererWarnings = [];
  if (raw.renderer === 'native') rendererWarnings.push('renderer "native" selects the deprecated legacy host; migrate to "rendrix" (the default) before the Rendrix M1 gate');
  if (raw.renderer === 'gpu') rendererWarnings.push('renderer "gpu" is the old name of the Rendrix renderer; rename it to "rendrix" or remove the key');
  if (raw.webviewPages !== undefined) throw new Error('webviewPages was removed with the WebView renderer; delete the key from rendrix.config.json');
  const maxBundleSize = raw.maxBundleSize ?? 256 * 1024 * 1024;
  if (!Number.isSafeInteger(maxBundleSize) || maxBundleSize <= 0) throw new Error('maxBundleSize must be a positive integer in bytes');
  // Custom in-page schemes allowlisted for packaged assets (derived by the Electron importer).
  const protocolRaw = raw.protocolSchemes ?? [];
  if (!Array.isArray(protocolRaw) || protocolRaw.some(s => typeof s !== 'string' || !/^[a-z][a-z0-9+.-]*$/i.test(s) || ['http','https','ws','wss'].includes(s.toLowerCase()) || s.toLowerCase() === 'rendrix')) throw new Error('protocolSchemes must contain valid in-page scheme names (no http/https/ws/wss or rendrix)');
  const protocolSchemes = [...new Set(protocolRaw.map(s => s.toLowerCase()))].sort();
  // "unified" hides the system title bar; the app draws one and marks its drag area with data-window-drag.
  // "blur" asks the window for a blurred backdrop, which Windows supports only on some systems.
  const titlebar = raw.window?.titlebar ?? 'system';
  const backdrop = raw.window?.backdrop ?? 'opaque';
  if (!['system','unified'].includes(titlebar)) throw new Error('window.titlebar must be "system" or "unified"');
  if (!['opaque','blur'].includes(backdrop)) throw new Error('window.backdrop must be "opaque" or "blur"');
  const normalizeRoots = roots => [...new Set(roots.map(root => path.posix.normalize(root.replaceAll('\\', '/')).replace(/^\.\//, '')))];
  return {name:raw.name,title:raw.title || raw.name,version,bundleId,icon:raw.icon,width,height,html:raw.html,css:raw.css,backend:raw.backend,outDir:raw.outDir || 'dist',strictCss:raw.strictCss === true,persist:raw.persist === true,renderer,backendWindow,protocolSchemes,maxBundleSize,fontFiles,permissionWarnings:[...permissionWarnings,...rendererWarnings],permissions:{networkHosts,filesystem:{read:normalizeRoots(readRoots),write:normalizeRoots(writeRoots)},processes:[...new Set(processes)],notifications,ipc,window:nativeWindow,networkMethods,processArgs},window:{titlebar,backdrop}};
}
function inProject(root, relative) {
  if (path.isAbsolute(relative)) throw new Error(`Absolute file path forbidden in project config: ${relative}`);
  const p = path.resolve(root,relative);
  if (p !== root && !p.startsWith(root+path.sep)) throw new Error(`Path outside project is forbidden: ${relative}`);
  return p;
}
// Lexical checks in inProject do not see symlinks, so also resolve the real path. A file that
// does not exist yet (such as outDir) is checked through its nearest existing ancestor.
export async function assertRealInside(projectRoot, file, label) {
  const actualRoot = await realpath(projectRoot);
  let probe = file;
  for (;;) {
    try {
      const actual = await realpath(probe);
      if (actual !== actualRoot && !actual.startsWith(actualRoot+path.sep)) {
        throw new Error(`${label} resolves outside the project: ${path.relative(projectRoot,file)}`);
      }
      return;
    } catch (error) {
      if (error.code !== 'ENOENT') throw error;
      if ((await lstat(probe).catch(() => null))?.isSymbolicLink()) {
        throw new Error(`${label} is a dangling symlink: ${path.relative(projectRoot,file)}`);
      }
      const parent = path.dirname(probe);
      if (parent === probe) throw error;
      probe = parent;
    }
  }
}
async function readAsset(projectRoot, source) {
  const file = inProject(projectRoot,source);
  await assertRealInside(projectRoot,file,'Image asset');
  const ext = path.extname(file).toLowerCase();
  const formats = {'.png':'png','.jpg':'jpeg','.jpeg':'jpeg','.gif':'gif','.webp':'webp','.bmp':'bmp','.svg':'svg'};
  const format = formats[ext];
  if (!format) throw new Error(`Unsupported image format '${ext}' for ${source}`);
  const bytes = await readFile(file);
  if (bytes.length > 8*1024*1024) throw new Error(`Image asset exceeds 8 MiB: ${source}`);
  return {format, data:bytes.toString('base64')};
}
async function readFont(projectRoot, font) {
  if (!font || typeof font.family !== 'string' || !font.family.trim() || typeof font.src !== 'string' || !font.src) {
    throw new Error('Each font must have a nonempty family and src');
  }
  const file = inProject(projectRoot, font.src);
  await assertRealInside(projectRoot,file,'Font file');
  const ext = path.extname(file).toLowerCase();
  if (!['.ttf','.otf'].includes(ext)) throw new Error(`Unsupported font format '${ext}' for ${font.src}; use .ttf or .otf`);
  const bytes = await readFile(file);
  if (!bytes.length || bytes.length > 8*1024*1024) throw new Error(`Font file must be between 1 byte and 8 MiB: ${font.src}`);
  return [font.family.trim(), bytes.toString('base64')];
}
function collectImagePaths(node, paths = new Set()) {
  if (node.tag === 'img' && node.attrs.src) paths.add(node.attrs.src);
  for (const child of node.children) collectImagePaths(child,paths);
  return paths;
}
export async function readProject(projectPath) {
  const projectRoot = path.resolve(projectPath);
  // rendrix.config.json is the canonical name since the Rendrix rename; the old
  // rendrix.config.json still loads with a deprecation warning for one transition.
  const newFile = path.join(projectRoot,'rendrix.config.json');
  const legacyFile = path.join(projectRoot,'rendrix.config.json');
  const legacyExists = await readFile(legacyFile,'utf8').then(() => true,() => false);
  const newExists = await readFile(newFile,'utf8').then(() => true,() => false);
  if (!newExists && legacyExists) {
    console.error('Warning: this project uses rendrix.config.json; rename it to rendrix.config.json (the old name is deprecated by the Rendrix rename).');
  }
  const configFile = newExists ? newFile : legacyFile;
  const label = newExists ? 'rendrix.config.json' : 'rendrix.config.json';
  if (!newExists && !legacyExists) throw new Error('No rendrix.config.json found in the project root');
  await assertRealInside(projectRoot,configFile,label);
  const config = validateConfig(JSON.parse(await readFile(configFile,'utf8')));
  const paths = {
    html:inProject(projectRoot,config.html), css:inProject(projectRoot,config.css),
    backend:inProject(projectRoot,config.backend), outDir:inProject(projectRoot,config.outDir)
  };
  for (const [label,file] of [['HTML',paths.html],['CSS',paths.css],['Backend',paths.backend],['Output directory',paths.outDir]]) {
    await assertRealInside(projectRoot,file,label);
  }
  return {projectRoot,config,configFile,paths};
}
// `warnings` collects messages for CSS declarations that were skipped. They are not part of the bundle.
// The host re-serializes the parsed document tree through its Rust `Node` struct to verify the
// bundle's integrity hash. That serialization always emits every field in struct declaration order
// (tag, key, text, attrs, props, style, states, responsive, children) and sorts object-map keys,
// because serde's maps are sorted. Emit the tree in exactly that shape so the hash computed here
// byte-matches the host's recomputation.
function canonicalNode(node) {
  const sortedMap = value => {
    if (value && typeof value === 'object' && !Array.isArray(value)) {
      const out = {};
      for (const key of Object.keys(value).sort()) out[key] = value[key];
      return out;
    }
    return value;
  };
  const walk = node => ({
    tag: node.tag,
    key: node.key,
    text: node.text ?? '',
    attrs: sortedMap(node.attrs ?? {}),
    props: sortedMap(node.props ?? {}),
    style: sortedMap(node.style ?? {}),
    states: Object.fromEntries(Object.keys(node.states ?? {}).sort().map(name => [name, sortedMap(node.states[name])])),
    responsive: (node.responsive ?? []).map(rule => ({
      media: rule.media,
      specificity: rule.specificity,
      order: rule.order,
      declarations: sortedMap(rule.declarations ?? {}),
    })),
    children: (node.children ?? []).map(walk),
  });
  return walk(node);
}
// "file", "file:line" or "file:line:col", depending on what the error knows.
function where(file, error) {
  if (!error?.line) return file;
  return error.column ? `${file}:${error.line}:${error.column}` : `${file}:${error.line}`;
}
export function compileProject({html,css,backend,config,sourceFiles={},assets={},compiledTree,warnings=[],compiledBackend}) {
  for (const w of (config.permissionWarnings ?? [])) warnings.push(w);
  let root;
  try { root = compiledTree ?? parseHTML(html, {library: COMPONENT_LIBRARY}); }
  catch (error) { throw new Error(`${where(sourceFiles.html ?? 'HTML', error)}: ${error.message}`, {cause:error}); }
  try {
    const allowedHosts = new Set(config.permissions?.networkHosts ?? []);
    const checkNetwork = node => {
      if (node.attrs['data-fetch']) try {
        const url = new URL(node.attrs['data-fetch']);
        if (url.protocol !== 'https:' || url.username || url.password || url.port && url.port !== '443' || url.hash) {
          throw new Error('data-fetch requires an HTTPS URL on the default port with no credentials or fragment');
        }
        if (!allowedHosts.has(url.hostname.toLowerCase())) throw new Error(`Network host is not listed in permissions.network: ${url.hostname}`);
      } catch (error) { if (error.line === undefined && node.line) { error.line = node.line; error.column = node.column; } throw error; }
      for (const child of node.children) checkNetwork(child);
    };
    checkNetwork(root);
  } catch (error) { throw new Error(`${where(sourceFiles.html ?? 'HTML', error)}: ${error.message}`, {cause:error}); }
  const windowChrome = {titlebar:config.window?.titlebar ?? 'system', backdrop:config.window?.backdrop ?? 'opaque'};
  const hasAttribute = (node, name) => node.attrs[name] !== undefined || node.children.some(child => hasAttribute(child, name));
  if (windowChrome.titlebar === 'unified' && !hasAttribute(root, 'data-window-drag')) {
    warnings.push(`${sourceFiles.html ?? 'HTML'}: window.titlebar is "unified" but no element has data-window-drag, so the window cannot be moved`);
  }
  if (windowChrome.titlebar !== 'unified' && hasAttribute(root, 'data-window-control')) {
    warnings.push(`${sourceFiles.html ?? 'HTML'}: data-window-control has no effect unless window.titlebar is "unified"`);
  }
  let rules;
  try {
    const stylesheet = compileStylesheet(css, {strict:config.strictCss === true, prelude:COMPONENT_LIBRARY_CSS});
    rules = stylesheet.rules;
    for (const warning of stylesheet.warnings) warnings.push(`${sourceFiles.css ?? 'CSS'}: ${warning}`);
  } catch (error) { throw new Error(`${where(sourceFiles.css ?? 'CSS', error)}: ${error.message}`, {cause:error}); }
  root = styleTree(root,rules);
  let backend_js;
  try { backend_js = compiledBackend ?? compileBackend(backend); }
  catch (error) { throw new Error(`${sourceFiles.backend ?? 'backend.ts'}: ${error.message}`, {cause:error}); }
  root = canonicalNode(root);
  return {
    format:'rendrix-1',
    renderer:config.renderer ?? 'rendrix',
    name:config.name,title:config.title,version:config.version ?? "1.0.0",bundle_id:config.bundleId ?? `dev.rendrix.${config.name.replaceAll("_", "-")}`,
    width:config.width,height:config.height,
    root,backend_js,assets,fonts:config.fonts ?? {},
    persist:config.persist === true,
    dev:config.dev === true,
    diagnostics:config.dev === true ? [...warnings] : [],
    protocol_schemes: config.protocolSchemes ?? [],
    permissions:{network_hosts:config.permissions?.networkHosts ?? [],network_methods:config.permissions?.networkMethods ?? {},filesystem:config.permissions?.filesystem ?? {read:[],write:[]},processes:config.permissions?.processes ?? [],process_args:config.permissions?.processArgs ?? {},notifications:config.permissions?.notifications ?? false,ipc:config.permissions?.ipc ?? false,window:config.permissions?.window ?? false},
    window:windowChrome
  };
}
export async function buildPackage(projectPath, options = {}) {
  const project = await readProject(projectPath);
  const [html,css,backend] = await Promise.all([
    readFile(project.paths.html,'utf8'),readFile(project.paths.css,'utf8'),readFile(project.paths.backend,'utf8')
  ]);
  let iconFile;
  if (project.config.icon) {
    iconFile = inProject(project.projectRoot, project.config.icon);
    await assertRealInside(project.projectRoot, iconFile, 'Icon');
  }
  const bundledBackend = await bundleBackend(project.paths.backend, project.projectRoot, assertRealInside);
  let root;
  try { root = parseHTML(html, {library: COMPONENT_LIBRARY}); }
  catch (error) { throw new Error(`${where(project.config.html, error)}: ${error.message}`, {cause:error}); }
  const imagePaths = collectImagePaths(root);
  const assets = Object.fromEntries(await Promise.all([...imagePaths].map(async source=>[source,await readAsset(project.projectRoot,source)])));
  const fonts = Object.fromEntries(await Promise.all((project.config.fontFiles ?? []).map(font=>readFont(project.projectRoot,font))));
  const watchAssets = new Set([...imagePaths,...(project.config.fontFiles ?? []).map(font=>font.src),...bundledBackend.files.map(file=>path.relative(project.projectRoot,file))]);
  const warnings = [];
  const built = compileProject({html,css,backend,config:{...project.config,fonts,dev:options.dev === true},assets,compiledTree:root,warnings,compiledBackend:bundledBackend.js,sourceFiles:{html:project.config.html,css:project.config.css,backend:project.config.backend}});
  const {createHash} = await import('node:crypto');
  built.integrity = createHash('sha256').update(built.backend_js + JSON.stringify(built.root)).digest('hex');
  const json = JSON.stringify(built);
  const bytes = Buffer.from(json,'utf8');
  if (bytes.length > project.config.maxBundleSize) throw new Error(`App bundle exceeds maxBundleSize (${project.config.maxBundleSize} bytes)`);
  await mkdir(project.paths.outDir,{recursive:true});
  const output = path.join(project.paths.outDir,`${project.config.name}.tgp`);
  const temporary = `${output}.tmp`;
  for (const file of [output,temporary]) {
    if ((await lstat(file).catch(() => null))?.isSymbolicLink()) throw new Error(`Refusing to write through symlink: ${path.relative(project.projectRoot,file)}`);
  }
  await writeFile(temporary,bytes);
  await rename(temporary,output);
  return {...project,iconFile,bundle:built,bundleFile:output,bytes,warnings,watchAssets:[...watchAssets]};
}
