import { readFile, writeFile, mkdir, realpath, rename, lstat } from 'node:fs/promises';
import path from 'node:path';
import { isIP } from 'node:net';
import { parseComponentLibrary, parseHTML } from './html.mjs';
import { compileStylesheet, styleTree } from './css.mjs';
import { compileBackend } from './backend.mjs';
import { COMPONENT_LIBRARY_CSS, COMPONENT_LIBRARY_HTML } from './library.mjs';

const COMPONENT_LIBRARY = parseComponentLibrary(COMPONENT_LIBRARY_HTML);
const NAME_RE = /^[a-zA-Z][a-zA-Z0-9_-]{0,63}$/;
const HOST_RE = /^(?=.{1,253}$)(?:[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?)(?:\.(?:[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?))*$/i;
export function validateConfig(raw) {
  if (!raw || typeof raw !== 'object' || Array.isArray(raw)) throw new Error('tsgpui.config.json must be an object');
  for (const key of ['name','html','css','backend']) if (typeof raw[key] !== 'string' || !raw[key]) throw new Error(`Missing string config property: ${key}`);
  if (!NAME_RE.test(raw.name)) throw new Error('Invalid name; use letters, digits, _ or -');
  const width = raw.window?.width ?? 900;
  const height = raw.window?.height ?? 600;
  if (![width,height].every(n => Number.isInteger(n) && n >= 200 && n <= 4096)) throw new Error('Window dimensions must be integers between 200 and 4096');
  if (raw.permissions !== undefined && (!raw.permissions || typeof raw.permissions !== 'object' || Array.isArray(raw.permissions))) {
    throw new Error('permissions must be an object');
  }
  const network = raw.permissions?.network ?? [];
  if (!Array.isArray(network) || network.some(host => typeof host !== 'string' || !HOST_RE.test(host) || !host.includes('.') || isIP(host) || ['localhost','local'].includes(host.toLowerCase()) || /\.(?:local|localhost|internal)$/.test(host.toLowerCase()))) {
    throw new Error('permissions.network must contain exact public host names (no wildcards or IP addresses)');
  }
  const networkHosts = [...new Set(network.map(host => host.toLowerCase()))];
  const fontFiles = raw.fonts ?? [];
  if (!Array.isArray(fontFiles)) throw new Error('fonts must be an array of {family, src} entries');
  const families = new Set();
  for (const font of fontFiles) {
    if (!font || typeof font.family !== 'string' || !font.family.trim() || typeof font.src !== 'string' || !font.src) throw new Error('Each font must have a nonempty family and src');
    if (families.has(font.family.trim())) throw new Error(`Duplicate font family '${font.family.trim()}'`);
    families.add(font.family.trim());
  }
  if (raw.strictCss !== undefined && typeof raw.strictCss !== 'boolean') throw new Error('strictCss must be true or false');
  // "unified" hides the system title bar; the app draws one and marks its drag area with data-window-drag.
  // "blur" asks the window for a blurred backdrop, which Windows supports only on some systems.
  const titlebar = raw.window?.titlebar ?? 'system';
  const backdrop = raw.window?.backdrop ?? 'opaque';
  if (!['system','unified'].includes(titlebar)) throw new Error('window.titlebar must be "system" or "unified"');
  if (!['opaque','blur'].includes(backdrop)) throw new Error('window.backdrop must be "opaque" or "blur"');
  return {name:raw.name,title:raw.title || raw.name,width,height,html:raw.html,css:raw.css,backend:raw.backend,outDir:raw.outDir || 'dist',strictCss:raw.strictCss === true,fontFiles,permissions:{networkHosts},window:{titlebar,backdrop}};
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
  const configFile = path.join(projectRoot,'tsgpui.config.json');
  await assertRealInside(projectRoot,configFile,'tsgpui.config.json');
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
export function compileProject({html,css,backend,config,sourceFiles={},assets={},compiledTree,warnings=[]}) {
  let root;
  try { root = compiledTree ?? parseHTML(html, {library: COMPONENT_LIBRARY}); }
  catch (error) { throw new Error(`${sourceFiles.html ?? 'HTML'}: ${error.message}`, {cause:error}); }
  try {
    const allowedHosts = new Set(config.permissions?.networkHosts ?? []);
    const checkNetwork = node => {
      if (node.attrs['data-fetch']) {
        const url = new URL(node.attrs['data-fetch']);
        if (url.protocol !== 'https:' || url.username || url.password || url.port && url.port !== '443' || url.hash) {
          throw new Error('data-fetch requires an HTTPS URL on the default port with no credentials or fragment');
        }
        if (!allowedHosts.has(url.hostname.toLowerCase())) throw new Error(`Network host is not listed in permissions.network: ${url.hostname}`);
      }
      for (const child of node.children) checkNetwork(child);
    };
    checkNetwork(root);
  } catch (error) { throw new Error(`${sourceFiles.html ?? 'HTML'}: ${error.message}`, {cause:error}); }
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
  } catch (error) { throw new Error(`${sourceFiles.css ?? 'CSS'}: ${error.message}`, {cause:error}); }
  root = styleTree(root,rules);
  let backend_js;
  try { backend_js = compileBackend(backend); }
  catch (error) { throw new Error(`${sourceFiles.backend ?? 'backend.ts'}: ${error.message}`, {cause:error}); }
  return {
    format:'tsgpui-1',
    name:config.name,title:config.title,
    width:config.width,height:config.height,
    root,backend_js,assets,fonts:config.fonts ?? {},
    permissions:{network_hosts:config.permissions?.networkHosts ?? []},
    window:windowChrome
  };
}
export async function buildPackage(projectPath) {
  const project = await readProject(projectPath);
  const [html,css,backend] = await Promise.all([
    readFile(project.paths.html,'utf8'),readFile(project.paths.css,'utf8'),readFile(project.paths.backend,'utf8')
  ]);
  let root;
  try { root = parseHTML(html, {library: COMPONENT_LIBRARY}); }
  catch (error) { throw new Error(`${project.config.html}: ${error.message}`, {cause:error}); }
  const imagePaths = collectImagePaths(root);
  const assets = Object.fromEntries(await Promise.all([...imagePaths].map(async source=>[source,await readAsset(project.projectRoot,source)])));
  const fonts = Object.fromEntries(await Promise.all((project.config.fontFiles ?? []).map(font=>readFont(project.projectRoot,font))));
  const warnings = [];
  const built = compileProject({html,css,backend,config:{...project.config,fonts},assets,compiledTree:root,warnings,sourceFiles:{html:project.config.html,css:project.config.css,backend:project.config.backend}});
  const json = JSON.stringify(built);
  const bytes = Buffer.from(json,'utf8');
  if (bytes.length > 16 * 1024 * 1024) throw new Error('App bundle exceeds 16 MiB safety limit');
  await mkdir(project.paths.outDir,{recursive:true});
  const output = path.join(project.paths.outDir,`${project.config.name}.tgp`);
  const temporary = `${output}.tmp`;
  for (const file of [output,temporary]) {
    if ((await lstat(file).catch(() => null))?.isSymbolicLink()) throw new Error(`Refusing to write through symlink: ${path.relative(project.projectRoot,file)}`);
  }
  await writeFile(temporary,bytes);
  await rename(temporary,output);
  return {...project,bundle:built,bundleFile:output,bytes,warnings};
}
