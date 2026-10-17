import { readFile, writeFile, mkdir } from 'node:fs/promises';
import path from 'node:path';
import { parseHTML } from './html.mjs';
import { compileCSS, styleTree } from './css.mjs';
import { compileBackend } from './backend.mjs';

const NAME_RE = /^[a-zA-Z][a-zA-Z0-9_-]{0,63}$/;
export function validateConfig(raw) {
  if (!raw || typeof raw !== 'object' || Array.isArray(raw)) throw new Error('tsgpui.config.json must be an object');
  for (const key of ['name','html','css','backend']) if (typeof raw[key] !== 'string' || !raw[key]) throw new Error(`Missing string config property: ${key}`);
  if (!NAME_RE.test(raw.name)) throw new Error('Invalid name; use letters, digits, _ or -');
  const width = raw.window?.width ?? 900;
  const height = raw.window?.height ?? 600;
  if (![width,height].every(n => Number.isInteger(n) && n >= 200 && n <= 4096)) throw new Error('Window dimensions must be integers between 200 and 4096');
  return {name:raw.name,title:raw.title || raw.name,width,height,html:raw.html,css:raw.css,backend:raw.backend,outDir:raw.outDir || 'dist'};
}
function inProject(root, relative) {
  if (path.isAbsolute(relative)) throw new Error(`Absolute file path forbidden in project config: ${relative}`);
  const p = path.resolve(root,relative);
  if (p !== root && !p.startsWith(root+path.sep)) throw new Error(`Path outside project is forbidden: ${relative}`);
  return p;
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
export function compileProject({html,css,backend,config}) {
  const root = styleTree(parseHTML(html), compileCSS(css));
  const backend_js = compileBackend(backend);
  return {
    format:'tsgpui-1',
    name:config.name,title:config.title,
    width:config.width,height:config.height,
    root,backend_js
  };
}
export async function buildPackage(projectPath) {
  const project = await readProject(projectPath);
  const [html,css,backend] = await Promise.all([
    readFile(project.paths.html,'utf8'),readFile(project.paths.css,'utf8'),readFile(project.paths.backend,'utf8')
  ]);
  const built = compileProject({html,css,backend,config:project.config});
  const json = JSON.stringify(built);
  const bytes = Buffer.from(json,'utf8');
  if (bytes.length > 16 * 1024 * 1024) throw new Error('App bundle exceeds 16 MiB safety limit');
  await mkdir(project.paths.outDir,{recursive:true});
  const output = path.join(project.paths.outDir,`${project.config.name}.tgp`);
  await writeFile(output,bytes);
  return {...project,bundle:built,bundleFile:output,bytes};
}
