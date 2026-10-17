import { readFile, writeFile, mkdir, realpath, rename } from 'node:fs/promises';
import path from 'node:path';
import { isIP } from 'node:net';
import { parseHTML } from './html.mjs';
import { compileCSS, styleTree } from './css.mjs';
import { compileBackend } from './backend.mjs';

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
  return {name:raw.name,title:raw.title || raw.name,width,height,html:raw.html,css:raw.css,backend:raw.backend,outDir:raw.outDir || 'dist',permissions:{networkHosts}};
}
function inProject(root, relative) {
  if (path.isAbsolute(relative)) throw new Error(`Absolute file path forbidden in project config: ${relative}`);
  const p = path.resolve(root,relative);
  if (p !== root && !p.startsWith(root+path.sep)) throw new Error(`Path outside project is forbidden: ${relative}`);
  return p;
}
async function readAsset(projectRoot, source) {
  const file = inProject(projectRoot,source);
  const actualRoot = await realpath(projectRoot);
  const actualFile = await realpath(file);
  if (actualFile !== actualRoot && !actualFile.startsWith(actualRoot+path.sep)) throw new Error(`Image asset resolves outside the project: ${source}`);
  const ext = path.extname(file).toLowerCase();
  const formats = {'.png':'png','.jpg':'jpeg','.jpeg':'jpeg','.gif':'gif','.webp':'webp','.bmp':'bmp','.svg':'svg'};
  const format = formats[ext];
  if (!format) throw new Error(`Unsupported image format '${ext}' for ${source}`);
  const bytes = await readFile(file);
  if (bytes.length > 8*1024*1024) throw new Error(`Image asset exceeds 8 MiB: ${source}`);
  return {format, data:bytes.toString('base64')};
}
function collectImagePaths(node, paths = new Set()) {
  if (node.tag === 'img' && node.attrs.src) paths.add(node.attrs.src);
  for (const child of node.children) collectImagePaths(child,paths);
  return paths;
}
export async function readProject(projectPath) {
  const projectRoot = path.resolve(projectPath);
  const configFile = path.join(projectRoot,'tsgpui.config.json');
  const config = validateConfig(JSON.parse(await readFile(configFile,'utf8')));
  const paths = {
    html:inProject(projectRoot,config.html), css:inProject(projectRoot,config.css),
    backend:inProject(projectRoot,config.backend), outDir:inProject(projectRoot,config.outDir)
  };
  return {projectRoot,config,configFile,paths};
}
export function compileProject({html,css,backend,config,sourceFiles={},assets={},compiledTree}) {
  let root;
  try { root = compiledTree ?? parseHTML(html); }
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
  let rules;
  try { rules = compileCSS(css, {width:config.width,height:config.height}); }
  catch (error) { throw new Error(`${sourceFiles.css ?? 'CSS'}: ${error.message}`, {cause:error}); }
  root = styleTree(root,rules);
  let backend_js;
  try { backend_js = compileBackend(backend); }
  catch (error) { throw new Error(`${sourceFiles.backend ?? 'backend.ts'}: ${error.message}`, {cause:error}); }
  return {
    format:'tsgpui-1',
    name:config.name,title:config.title,
    width:config.width,height:config.height,
    root,backend_js,assets,
    permissions:{network_hosts:config.permissions?.networkHosts ?? []}
  };
}
export async function buildPackage(projectPath) {
  const project = await readProject(projectPath);
  const [html,css,backend] = await Promise.all([
    readFile(project.paths.html,'utf8'),readFile(project.paths.css,'utf8'),readFile(project.paths.backend,'utf8')
  ]);
  let root;
  try { root = parseHTML(html); }
  catch (error) { throw new Error(`${project.config.html}: ${error.message}`, {cause:error}); }
  const imagePaths = collectImagePaths(root);
  const assets = Object.fromEntries(await Promise.all([...imagePaths].map(async source=>[source,await readAsset(project.projectRoot,source)])));
  const built = compileProject({html,css,backend,config:project.config,assets,compiledTree:root,sourceFiles:{html:project.config.html,css:project.config.css,backend:project.config.backend}});
  const json = JSON.stringify(built);
  const bytes = Buffer.from(json,'utf8');
  if (bytes.length > 16 * 1024 * 1024) throw new Error('App bundle exceeds 16 MiB safety limit');
  await mkdir(project.paths.outDir,{recursive:true});
  const output = path.join(project.paths.outDir,`${project.config.name}.tgp`);
  const temporary = `${output}.tmp`;
  await writeFile(temporary,bytes);
  await rename(temporary,output);
  return {...project,bundle:built,bundleFile:output,bytes};
}
