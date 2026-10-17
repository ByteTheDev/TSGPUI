#!/usr/bin/env node
import {spawn, spawnSync} from 'node:child_process';
import fs from 'node:fs';
import {copyFile,mkdir,readFile,writeFile} from 'node:fs/promises';
import path from 'node:path';
import {fileURLToPath} from 'node:url';
import {buildPackage,readProject} from '../src/compiler.mjs';
import {packageExecutable} from '../src/pack.mjs';

const here = path.dirname(fileURLToPath(import.meta.url));
const repo = path.resolve(here,'../../..');
const args = process.argv.slice(2);
const command = args.shift() || 'help';
const windows = process.platform === 'win32';
const suffix = windows ? '.exe' : '';
function flag(name, fallback) { const i=args.indexOf(`--${name}`); return i>=0 ? args[i+1] || fallback : fallback; }
function projectDir() {return path.resolve(flag('project',process.cwd()));}
function rustAvailable() {return spawnSync('cargo',['--version'],{stdio:'ignore',shell:windows}).status === 0;}
function hostCandidates() {
  const envHost = process.env.TSGPUI_HOST;
  return [flag('host',null), envHost, path.join(repo,'native/target/release',`tsgpui-host${suffix}`),
    path.join(repo,'native/target/debug',`tsgpui-host${suffix}`),
    path.join(repo,'prebuilt',`${process.platform}-${process.arch}`,`tsgpui-host${suffix}`)].filter(Boolean);
}
async function hostPath(autoBuild = false) {
  for(const candidate of hostCandidates()) if (fs.existsSync(candidate)) return path.resolve(candidate);
  if (autoBuild && rustAvailable()) {
    console.log('Native host not found: compiling GPUI host once (subsequent app builds reuse it)...');
    await buildHost();
    for(const candidate of hostCandidates()) if (fs.existsSync(candidate)) return path.resolve(candidate);
  }
  throw new Error('No prebuilt GPUI host found. Set TSGPUI_HOST to your tsgpui-host.exe, or run `bun run gpui:host` once with Rust installed. A native executable cannot be made from TypeScript alone. See README.md.');
}
async function buildHost() {
  if (!rustAvailable()) throw new Error('Rust/Cargo is required to compile the GPUI host once. Alternatively use a verified prebuilt host from CI.');
  const res = spawnSync('cargo',['build','--manifest-path',path.join(repo,'native/Cargo.toml'),'--release'],{stdio:'inherit',shell:windows});
  if (res.status !== 0) throw new Error(`Native host build failed (code ${res.status})`);
}
async function init() {
  const folder = args.find(x => !x.startsWith('--')) ?? 'my-tsgpui-app';
  const destination = path.resolve(folder);
  if (fs.existsSync(destination) && fs.readdirSync(destination).length) throw new Error('Project destination is not empty');
  await mkdir(path.join(destination,'src'),{recursive:true});
  const example = path.join(repo,'examples/counter');
  const template = fs.existsSync(path.join(repo,'packages/cli/templates/basic/tsgpui.config.json'))
    ? path.join(repo,'packages/cli/templates/basic') : example;
  for(const file of ['tsgpui.config.json','src/index.html','src/styles.css','src/backend.ts']) {
    const input = await readFile(path.join(template,file),'utf8');
    await writeFile(path.join(destination,file),file === 'tsgpui.config.json' ? input.replace(/tsgpui-counter/g, 'my-tsgpui-app') : input);
  }
  const pkg = {
    name: 'my-tsgpui-app',private:true,type:'module',
    scripts:{gpui:'tsgpui build','gpui:dev':'tsgpui dev','gpui:build':'tsgpui build','gpui:compile':'tsgpui compile'},
    devDependencies:{tsgpui:'^0.1.0'}
  };
  await writeFile(path.join(destination,'package.json'),JSON.stringify(pkg,null,2)+'\n');
  console.log(`Created ${destination}. Run bun install, then bun run gpui:dev (native host required).`);
}
function runHost(host,bundleFile) {
  const child=spawn(host,['--bundle',bundleFile],{stdio:'inherit',windowsHide:false});
  child.on('error',e => console.error(`GPUI host failed: ${e.message}`));
  return child;
}
async function dev() {
  const project = await readProject(projectDir());
  const host = await hostPath(true);
  let child, refreshing=false, queued=false;
  async function rebuild() {
    if (refreshing) {queued=true;return;}
    refreshing=true;
    try {
      const output=await buildPackage(project.projectRoot);
      if (child && !child.killed) child.kill();
      child=runHost(host,output.bundleFile);
      console.log(`GPUI dev: ${output.bundleFile}`);
    } catch(e) {console.error(`Build failed: ${e.message}`);}
    finally {refreshing=false;if(queued){queued=false;await rebuild();}}
  }
  await rebuild();
  const watchFiles=[project.paths.html,project.paths.css,project.paths.backend,project.configFile];
  const watchers=watchFiles.map(file=>fs.watch(file,()=>{void rebuild();}));
  process.on('SIGINT',()=>{watchers.forEach(w=>w.close());child?.kill();process.exit(0);});
  console.log('Watching HTML, CSS, backend TypeScript and config. Ctrl+C to stop.');
}
async function doctor() {
  console.log(`Node: ${process.version} | OS: ${process.platform}/${process.arch}`);
  console.log(`Cargo: ${rustAvailable() ? 'available' : 'not found'}`);
  console.log('Host paths checked:');
  for(const file of hostCandidates()) console.log(`  ${fs.existsSync(file)?'[found]':'[missing]'} ${file}`);
  console.log('Rust is needed once to create a host, or supply a prebuilt host via TSGPUI_HOST.');
}
async function main() {
  if(command==='help' || args.includes('--help')) {
    console.log('TSGPUI 0.1.0\n  init [dir]                  Create a project\n  compile [--project PATH]     Compile HTML/CSS/TS to .tgp\n  dev [--project PATH]         Launch native host and watch files\n  build [--project PATH]       Embed bundle in one native executable\n  host build                   Compile native host once (Cargo)\n  doctor                       Check environment\nOptions: --host PATH | TSGPUI_HOST=PATH');
    return;
  }
  if(command==='init') return init();
  if(command==='doctor') return doctor();
  if(command==='host' && args[0]==='build') return buildHost();
  if(command==='compile' || command==='build') {
    const result=await buildPackage(projectDir());
    console.log(`Compiled ${result.bundleFile} (${result.bytes.length} bytes)`);
    if(command==='compile') return;
    const host=await hostPath(true);
    const output=path.join(result.paths.outDir,`${result.config.name}${suffix}`);
    const info=await packageExecutable(host,result.bundleFile,output);
    console.log(`Built native application: ${info.file}`);
    return;
  }
  if(command==='dev') return dev();
  throw new Error(`Unknown command: ${command}`);
}
main().catch(error=>{console.error(`TSGPUI: ${error.message}`);process.exitCode=1;});
