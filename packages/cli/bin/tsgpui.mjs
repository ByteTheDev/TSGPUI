#!/usr/bin/env node
import {spawn, spawnSync} from 'node:child_process';
import fs from 'node:fs';
import {copyFile,mkdir,readFile,writeFile} from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import {fileURLToPath} from 'node:url';
import {buildPackage,readProject} from '../src/compiler.mjs';
import {packageExecutable} from '../src/pack.mjs';
import {startPreview,writePreviewFile} from '../src/preview.mjs';

// This file works both from a monorepo checkout and from an installed package.
const here = path.dirname(fileURLToPath(import.meta.url));
const pkgRoot = path.resolve(here,'..');
const args = process.argv.slice(2);
const command = args.shift() || 'help';
const windows = process.platform === 'win32';
const suffix = windows ? '.exe' : '';
const VERSION = JSON.parse(fs.readFileSync(path.join(pkgRoot,'package.json'),'utf8')).version;
function flag(name, fallback) { const i=args.indexOf(`--${name}`); return i>=0 ? args[i+1] || fallback : fallback; }
function projectDir() {return path.resolve(flag('project',process.cwd()));}
function rustAvailable() {return spawnSync('cargo',['--version'],{stdio:'ignore'}).status === 0;}

// Per-user state: built host executables and the Cargo target directory.
function tsgpuiHome() {return process.env.TSGPUI_HOME || path.join(os.homedir(),'.tsgpui');}
function cachedHost() {return path.join(tsgpuiHome(),'host',`${process.platform}-${process.arch}`,`tsgpui-host${suffix}`);}

// Rust sources: shipped inside the package, or next to it in a monorepo checkout.
function nativeSource() {
  for (const dir of [path.join(pkgRoot,'native'), path.resolve(pkgRoot,'../../native')]) {
    if (fs.existsSync(path.join(dir,'Cargo.toml'))) return dir;
  }
  return null;
}

function hostCandidates() {
  const source = nativeSource();
  return [flag('host',null), process.env.TSGPUI_HOST, cachedHost(),
    source && path.join(source,'target/release',`tsgpui-host${suffix}`),
    source && path.join(source,'target/debug',`tsgpui-host${suffix}`)].filter(Boolean);
}
async function hostPath(autoBuild = false) {
  for(const candidate of hostCandidates()) if (fs.existsSync(candidate)) return path.resolve(candidate);
  if (autoBuild && rustAvailable()) {
    console.log('Native host not found: compiling GPUI host once (later builds reuse it)...');
    await buildHost();
    for(const candidate of hostCandidates()) if (fs.existsSync(candidate)) return path.resolve(candidate);
  }
  throw new Error(`No GPUI host found. Run \`tsgpui host build\` once (needs Rust and the MSVC/C++ build tools on Windows), or set TSGPUI_HOST to a trusted tsgpui-host executable. Checked: ${hostCandidates().join(', ')}`);
}
async function buildHost() {
  const source = nativeSource();
  if (!source) throw new Error('Native host sources not found in this installation. Reinstall tsgpui or use a checkout of the TSGPUI repository.');
  if (!rustAvailable()) throw new Error('Rust/Cargo is required to compile the GPUI host once. Install it from https://rustup.rs or supply a trusted prebuilt host via TSGPUI_HOST.');
  const targetDir = path.join(tsgpuiHome(),'build');
  const res = spawnSync('cargo',['build','--manifest-path',path.join(source,'Cargo.toml'),'--release','--target-dir',targetDir],{stdio:'inherit'});
  if (res.status !== 0) throw new Error(`Native host build failed (code ${res.status})`);
  const destination = cachedHost();
  await mkdir(path.dirname(destination),{recursive:true});
  await copyFile(path.join(targetDir,'release',`tsgpui-host${suffix}`),destination);
  console.log(`GPUI host installed: ${destination}`);
}
async function init() {
  const folder = args.find(x => !x.startsWith('--')) ?? 'my-tsgpui-app';
  const destination = path.resolve(folder);
  if (fs.existsSync(destination) && fs.readdirSync(destination).length) throw new Error('Project destination is not empty');
  await mkdir(path.join(destination,'src'),{recursive:true});
  const template = path.join(pkgRoot,'templates/basic');
  for(const file of ['tsgpui.config.json','src/index.html','src/styles.css','src/backend.ts']) {
    const input = await readFile(path.join(template,file),'utf8');
    await writeFile(path.join(destination,file),file === 'tsgpui.config.json' ? input.replace(/tsgpui-counter/g, path.basename(destination)) : input);
  }
  const pkg = {
    name: path.basename(destination),private:true,type:'module',
    scripts:{gpui:'tsgpui build','gpui:dev':'tsgpui dev','gpui:build':'tsgpui build','gpui:compile':'tsgpui compile'},
    devDependencies:{tsgpui:`^${VERSION}`}
  };
  await writeFile(path.join(destination,'package.json'),JSON.stringify(pkg,null,2)+'\n');
  console.log(`Created ${destination}. Run \`tsgpui dev --project ${folder}\` or \`tsgpui build --project ${folder}\`.`);
}
// Skipped CSS declarations are reported, not silent. With strictCss in the config they are errors instead.
function printWarnings(warnings=[]) {
  for (const warning of warnings) console.warn(`Warning: ${warning}`);
}
function runHost(host,bundleFile) {
  const child=spawn(host,['--bundle',bundleFile],{stdio:'inherit',windowsHide:false});
  child.on('error',e => console.error(`GPUI host failed: ${e.message}`));
  return child;
}
async function dev() {
  const project = await readProject(projectDir());
  const host = await hostPath(true);
  let child, refreshing=false, queued=false, debounceTimer, watchers=[];
  async function refreshWatchers(assetPaths=[]) {
    const latest=await readProject(project.projectRoot);
    const watchFiles=[...new Set([
      latest.paths.html,latest.paths.css,latest.paths.backend,latest.configFile,
      ...assetPaths.map(source=>path.resolve(latest.projectRoot,source))
    ])];
    watchers.forEach(watcher=>watcher.close());
    const dirs=[...new Set(watchFiles.map(file=>path.dirname(file)))];
    watchers=dirs.map(dir=>fs.watch(dir,(_event,filename)=>{
      if (filename && !watchFiles.some(file=>path.dirname(file)===dir && path.basename(file)===filename.toString())) return;
      clearTimeout(debounceTimer);
      debounceTimer=setTimeout(()=>void rebuild(),90);
    }));
  }
  async function rebuild() {
    if (refreshing) {queued=true;return;}
    refreshing=true;
    try {
      const output=await buildPackage(project.projectRoot);
      printWarnings(output.warnings);
      await refreshWatchers(Object.keys(output.bundle.assets));
      const launch = !child || child.exitCode !== null;
      if (launch) child=runHost(host,output.bundleFile);
      console.log(`GPUI dev: ${launch ? 'launched' : 'updated'} ${output.bundleFile}`);
    } catch(e) {console.error(`Build failed: ${e.message}`);}
    finally {refreshing=false;if(queued){queued=false;await rebuild();}}
  }
  await rebuild();
  if (!watchers.length) {
    try { await refreshWatchers(); }
    catch(e) { console.error(`Unable to watch project sources: ${e.message}`); }
  }
  process.on('SIGINT',()=>{clearTimeout(debounceTimer);watchers.forEach(w=>w.close());child?.kill();process.exit(0);});
  console.log('Watching project source directories. The running window reloads rebuilt bundles; Ctrl+C stops the dev host.');
}
// Live browser preview of the same bundle. --out writes one HTML file instead, with no live reload.
async function preview() {
  const project = projectDir();
  const out = flag('out', null);
  if (out) {
    const result = await writePreviewFile({projectRoot: project, file: path.resolve(out)});
    printWarnings(result.warnings);
    console.log(`Wrote ${result.file}`);
    return;
  }
  const session = await startPreview({projectRoot: project, port: Number(flag('port', '4173')), open: !args.includes('--no-open')});
  console.log('Watching project sources. Ctrl+C stops the preview.');
  process.on('SIGINT', async () => { await session.close(); process.exit(0); });
}
async function doctor() {
  console.log(`tsgpui ${VERSION} | Node: ${process.version} | OS: ${process.platform}/${process.arch}`);
  console.log(`Cargo: ${rustAvailable() ? 'available' : 'not found'}`);
  console.log(`Native sources: ${nativeSource() ?? 'not found'}`);
  console.log('Host paths checked:');
  for(const file of hostCandidates()) console.log(`  ${fs.existsSync(file)?'[found]':'[missing]'} ${file}`);
  console.log('Run `tsgpui host build` once to create a host, or supply one with TSGPUI_HOST.');
}
function help() {
  console.log(`tsgpui ${VERSION}
  init [dir]                   Create a starter project
  compile [--project PATH]     Compile HTML/CSS/TS to a .tgp bundle
  dev [--project PATH]         Build, launch the native host, rebuild on change
  preview [--project PATH]     Live preview in a browser (--out FILE writes one HTML file)
  build [--project PATH]       Package a single native executable
  host build                   Compile the native host once (needs Rust)
  doctor                       Show toolchain and host status
Options: --host PATH, or the TSGPUI_HOST / TSGPUI_HOME environment variables.`);
}
async function main() {
  if(command==='help' || command==='--help' || args.includes('--help')) return help();
  if(command==='--version' || command==='-v') return console.log(VERSION);
  if(command==='init') return init();
  if(command==='doctor') return doctor();
  if(command==='host' && args[0]==='build') return buildHost();
  if(command==='compile' || command==='build') {
    const result=await buildPackage(projectDir());
    printWarnings(result.warnings);
    console.log(`Compiled ${result.bundleFile} (${result.bytes.length} bytes)`);
    if(command==='compile') return;
    const host=await hostPath(true);
    const output=path.join(result.paths.outDir,`${result.config.name}${suffix}`);
    const info=await packageExecutable(host,result.bundleFile,output);
    console.log(`Built native application: ${info.file}`);
    return;
  }
  if(command==='dev') return dev();
  if(command==='preview') return preview();
  throw new Error(`Unknown command: ${command}. Run \`tsgpui help\`.`);
}
main().catch(error=>{console.error(`TSGPUI: ${error.message}`);process.exitCode=1;});
