#!/usr/bin/env node
import {spawn, spawnSync} from 'node:child_process';
import fs from 'node:fs';
import {createHash} from 'node:crypto';
import {copyFile,mkdir,readFile,writeFile} from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import {fileURLToPath} from 'node:url';
import {buildPackage,readProject} from '../src/compiler.mjs';
import {importElectron} from '../src/import-electron.mjs';
import {packageExecutable,packageMacApp} from '../src/pack.mjs';
import {buildNsis} from '../src/installer/nsis.mjs';
import {signArtifact,writeManifest} from '../src/signing.mjs';
import {startPreview,writePreviewFile} from '../src/preview.mjs';
import {assertRenderer,cachedHostPath,currentTarget,downloadsDisabled,exeSuffix,fetchHost,hostBinaryName,inspectCachedHost,installHostFile,installedSourceStamp,recordLocalBuild,sourceStamp,tsgpuiHome,verifyCachedHost} from '../src/hosts.mjs';

// This file works both from a monorepo checkout and from an installed package.
const here = path.dirname(fileURLToPath(import.meta.url));
const pkgRoot = path.resolve(here,'..');
const args = process.argv.slice(2);
const command = args.shift() || 'help';
const VERSION = JSON.parse(fs.readFileSync(path.join(pkgRoot,'package.json'),'utf8')).version;
function flag(name, fallback) { const i=args.indexOf(`--${name}`); return i>=0 ? args[i+1] || fallback : fallback; }
function projectDir() {return path.resolve(flag('project',process.cwd()));}
function rustAvailable() {return spawnSync('cargo',['--version'],{stdio:'ignore'}).status === 0;}

const suffix = exeSuffix();
function cachedHost(renderer='native') {return cachedHostPath(renderer);}

// Rust sources: shipped inside the package, or next to it in a monorepo checkout.
function nativeSource() {
  for (const dir of [path.join(pkgRoot,'native'), path.resolve(pkgRoot,'../../native')]) {
    if (fs.existsSync(path.join(dir,'Cargo.toml'))) return dir;
  }
  return null;
}
function explicitHost(renderer='native') {
  // TSGPUI_HOST names the native host only; a WebView project must not pick it up.
  const override = renderer==='webview' ? process.env.TSGPUI_WEBVIEW_HOST : process.env.TSGPUI_HOST;
  return [flag('host',null), override].filter(Boolean);
}
function sourceBuilds(renderer='native') {
  const source = nativeSource();
  return source ? ['release','debug'].map(profile=>path.join(source,'target',profile,`${hostBinaryName(renderer)}${suffix}`)) : [];
}
// Explicit overrides are deliberate; cached hosts must match the available sources.
async function hostPath(autoBuild = false, renderer='native') {
  for(const candidate of explicitHost(renderer)) if (fs.existsSync(candidate)) return path.resolve(candidate);
  const cached = inspectCachedHost(renderer,VERSION);
  const source = nativeSource();
  if (source) {
    const stamp = sourceStamp(source, renderer);
    if (cached.state === 'ok' && installedSourceStamp(cached.file) === stamp && await verifyCachedHost(cached) === true) return path.resolve(cached.file);
    if (!autoBuild || !rustAvailable()) throw new Error(`The installed ${renderer} host is missing or does not match the current source. Install Rust, then run \`tsgpui host build --renderer ${renderer}\`.`);
    console.log(`Building the ${renderer} host to match the current source...`);
    await buildHost(renderer);
    return path.resolve(cachedHost(renderer));
  }
  throw new Error(`Cannot verify the ${renderer} host because native sources are unavailable. Use a source checkout and run \`tsgpui host build --renderer ${renderer}\`.`);
}
async function buildHost(renderer='native') {
  const source = nativeSource();
  if (!source) throw new Error('Native host sources not found in this installation. Run `tsgpui host fetch` for a prebuilt host, or use a checkout of the TSGPUI repository.');
  assertRenderer(renderer);
  if (!rustAvailable()) throw new Error(`Rust/Cargo is required to compile the ${renderer} host. Install it from https://rustup.rs, or run \`tsgpui host fetch --renderer ${renderer}\` for a prebuilt host.`);
  const targetDir = path.join(tsgpuiHome(),'build');
  const stamp = sourceStamp(source, renderer);
  const cargoArgs = ['build','--manifest-path',path.join(source,'Cargo.toml'),'--release','--target-dir',targetDir];
  if (renderer==='webview') cargoArgs.push('--no-default-features','--features','webview-host','--bin','tsgpui-webview-host');
  else cargoArgs.push('--features','native-host','--bin','tsgpui-host');
  const res = spawnSync('cargo',cargoArgs,{stdio:'inherit'});
  if (res.status !== 0) throw new Error(`Native host build failed (code ${res.status})`);
  if (sourceStamp(source, renderer) !== stamp) throw new Error('Host source changed during compilation; run the host build again.');
  const destination = cachedHost(renderer);
  await mkdir(path.dirname(destination),{recursive:true});
  await copyFile(path.join(targetDir,'release',`${hostBinaryName(renderer)}${suffix}`),destination);
  await recordLocalBuild({renderer,version:VERSION,sourceStamp:stamp});
  console.log(`${renderer} host installed: ${destination}`);
}
async function fetchHosts() {
  const requested = flag('renderer',null);
  const renderers = requested ? [requested] : ['native','webview'];
  let failed = false;
  for (const renderer of renderers) {
    assertRenderer(renderer);
    const cached = inspectCachedHost(renderer,VERSION);
    if (cached.state==='ok' && !args.includes('--force')) {console.log(`${renderer} host ${cached.record.version} is already installed: ${cached.file}`);continue;}
    try {await fetchHost({renderer,version:VERSION,pkgRoot});}
    catch(error) {
      // Asking for both renderers should not fail just because one has no prebuilt host here.
      if (requested) throw error;
      failed = true; console.error(`${renderer}: ${error.message}`);
    }
  }
  if (failed) process.exitCode = 1;
}
async function installHost(renderer='native') {
  const file=flag('file',null), expected=flag('sha256',null);
  if (!file || !expected || !/^[a-fA-F0-9]{64}$/.test(expected)) throw new Error('Use `tsgpui host install --file PATH --sha256 HASH [--renderer native|webview]` with a checksum from a trusted source.');
  const result=await installHostFile({renderer,file,sha256:expected,version:VERSION});
  console.log(`${renderer} host installed: ${result.file} (SHA-256 ${result.sha256})`);
}
// Deterministic SHA-256 manifest of a file or directory, for publishing beside a release artifact.
async function releaseManifest() {
  const target = flag('file',null);
  if (!target) throw new Error('Use `tsgpui release manifest --file PATH`.');
  const root = path.resolve(target);
  const files = [];
  async function visit(absolute, relative) {
    const info = await fs.promises.lstat(absolute);
    if (info.isSymbolicLink()) throw new Error(`Refusing to hash symbolic link: ${absolute}`);
    if (info.isDirectory()) {
      for (const name of (await fs.promises.readdir(absolute)).sort()) await visit(path.join(absolute,name),relative ? `${relative}/${name}` : name);
    } else if (info.isFile()) {
      files.push({path:relative,size:info.size,sha256:createHash('sha256').update(await readFile(absolute)).digest('hex')});
    }
  }
  const rootInfo = await fs.promises.lstat(root);
  await visit(root, rootInfo.isDirectory() ? '' : path.basename(root));
  files.sort((a,b)=>a.path<b.path?-1:a.path>b.path?1:0);
  console.log(JSON.stringify({schema:1,files},null,2));
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
  const relative = path.relative(process.cwd(),destination) || '.';
  const cd = relative === '.' ? '' : `cd ${relative.includes(' ') ? `"${relative}"` : relative}
  `;
  console.log(`Created ${destination}

Next:
  ${cd}tsgpui dev      # opens the app and reloads on every save
  tsgpui build    # packages a single executable into dist/

The first run downloads a verified prebuilt host (no Rust needed). \`tsgpui doctor\` checks your setup.`);
}
// Skipped CSS declarations are reported, not silent. With strictCss in the config they are errors instead.
function printWarnings(warnings=[]) {
  for (const warning of warnings) console.warn(`Warning: ${warning}`);
}
function runHost(host,bundleFile) {
  const child=spawn(host,['--bundle',bundleFile],{stdio:'inherit',windowsHide:false});
  child.on('error',e => console.error(`TSGPUI host failed: ${e.message}`));
  return child;
}
// A failed compile cannot replace the bundle, so leave a sidecar that the running host watches.
function buildErrorPath(project,bundleFile) {
  return bundleFile ? `${bundleFile}.error` : path.join(project.paths.outDir,`${project.config.name}.tgp.error`);
}
async function writeBuildError(file,error) {
  try {
    await mkdir(path.dirname(file),{recursive:true});
    await writeFile(file,error?.message || String(error));
  } catch(e) {console.error(`Unable to write build error: ${e.message}`);}
}
async function clearBuildError(file) {
  try {await fs.promises.unlink(file);}
  catch(e) {if(e.code !== 'ENOENT') console.error(`Unable to clear build error: ${e.message}`);}
}
// Modification times of the files that exist now, keyed by path.
async function mtimesOf(files) {
  const times=new Map();
  for (const file of files) {
    const info=await fs.promises.stat(file).catch(()=>null);
    if (info) times.set(file,info.mtimeMs);
  }
  return times;
}
// True when a watched file changed after `before` was taken. A file that was not watched then counts as
// changed when it was modified after `startedAt`. Comparing against the earlier mtime, rather than the
// clock alone, keeps a file with a future timestamp (a clock-skewed network drive) from rebuilding forever.
async function changedSince(before,files,startedAt) {
  for (const file of files) {
    const info=await fs.promises.stat(file).catch(()=>null);
    if (!info) continue;
    const previous=before.get(file);
    if (previous === undefined ? info.mtimeMs > startedAt : info.mtimeMs > previous) return true;
  }
  return false;
}
async function dev() {
  const project = await readProject(projectDir());
  let activeRenderer = project.config.renderer;
  let host = await hostPath(true,activeRenderer);
  let child, refreshing=false, queued=false, debounceTimer, watchers=[];
  let watchedPaths = [];
  async function refreshWatchers(assetPaths=[]) {
    const latest=await readProject(project.projectRoot);
    const watchFiles=[...new Set([
      latest.paths.html,latest.paths.css,latest.paths.backend,latest.configFile,
      ...assetPaths.map(source=>path.resolve(latest.projectRoot,source))
    ])];
    watchedPaths = watchFiles;
    const dirs=[...new Set(watchFiles.map(file=>{ let dir=path.dirname(file); while (!fs.existsSync(dir) && dir !== path.dirname(dir)) dir=path.dirname(dir); return dir; }))];
    const nextWatchers=[];
    try { for (const dir of dirs) nextWatchers.push(fs.watch(dir,(_event,filename)=>{
      if (filename && !watchFiles.some(file=>{ const relative=path.relative(dir,file); return relative && relative.split(path.sep)[0]===filename.toString(); })) return;
      clearTimeout(debounceTimer);
      debounceTimer=setTimeout(()=>void rebuild(),90);
    })); } catch (error) { nextWatchers.forEach(watcher=>watcher.close()); throw error; }
    watchers.forEach(watcher=>watcher.close());
    watchers=nextWatchers;
  }
  async function rebuild() {
    if (refreshing) {queued=true;return;}
    refreshing=true;
    const startedAt=Date.now();
    const before=await mtimesOf(watchedPaths);
    let errorFile=buildErrorPath(project);
    try {
      const output=await buildPackage(project.projectRoot,{dev:true});
      errorFile=buildErrorPath(output,output.bundleFile);
      await clearBuildError(errorFile);
      printWarnings(output.warnings);
      await refreshWatchers(output.watchAssets ?? Object.keys(output.bundle.assets));
      const rendererChanged = output.bundle.renderer !== activeRenderer;
      if (rendererChanged) {
        child?.kill();
        activeRenderer = output.bundle.renderer;
        host = await hostPath(true,activeRenderer);
      }
      const launch = rendererChanged || !child || child.exitCode !== null;
      if (launch) child=runHost(host,output.bundleFile);
      console.log(`${activeRenderer} dev: ${launch ? 'launched' : 'updated'} ${output.bundleFile}`);
    } catch(e) {
      console.error(`Build failed: ${e.message}`);
      if (e.watchPaths) {
        try { await refreshWatchers([...watchedPaths, ...e.watchPaths]); }
        catch (watchError) { console.error(`Unable to watch missing modules: ${watchError.message}`); }
      }
      await writeBuildError(errorFile,e);
      // With no running window yet, open the last good bundle so the error is visible in-app.
      if (!child && errorFile.endsWith('.error')) {
        const lastBundle=errorFile.slice(0,-'.error'.length);
        if (fs.existsSync(lastBundle)) {
          try {
            host=await hostPath(true,activeRenderer);
            child=runHost(host,lastBundle);
            console.log(`Build failed; showing the last successful build from ${lastBundle}.`);
          } catch(hostError) {console.error(`Unable to open the last build: ${hostError.message}`);}
        }
      }
    }
    finally {
      refreshing=false;
      // Watchers are refreshed only after a build, so a save made during the build can land before they
      // cover its file (a newly created nested module, say). No event follows that save, so check times.
      if (!queued && await changedSince(before,watchedPaths,startedAt)) queued=true;
      if(queued){queued=false;await rebuild();}
    }
  }
  await rebuild();
  if (!watchers.length) {
    try { await refreshWatchers(); }
    catch(e) { console.error(`Unable to watch project sources: ${e.message}`); }
  }
  process.on('SIGINT',()=>{clearTimeout(debounceTimer);watchers.forEach(w=>w.close());child?.kill();process.exit(0);});
  console.log('Watching project source directories. The running window reloads rebuilt bundles; Ctrl+C stops the host.');
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
// Prints what is ready and, for anything that is not, the exact command that fixes it.
// Only a problem that stops the app from running makes the exit code non-zero.
function systemLibraryCheck(renderer) {
  if (process.platform==='linux') {
    const wanted = renderer==='webview' ? ['webkit2gtk-4.1','gtk+-3.0'] : ['xkbcommon','wayland-client','xcb'];
    if (spawnSync('pkg-config',['--version'],{stdio:'ignore'}).status !== 0) return {level:'warn',text:'pkg-config is not installed, so system libraries could not be checked',fix:'sudo apt install pkg-config'};
    const missing = wanted.filter(lib=>spawnSync('pkg-config',['--exists',lib],{stdio:'ignore'}).status !== 0);
    if (!missing.length) return {level:'ok',text:`system libraries present (${wanted.join(', ')})`};
    const apt = renderer==='webview' ? 'libwebkit2gtk-4.1-dev libgtk-3-dev' : 'libxkbcommon-dev libwayland-dev libxcb1-dev';
    return {level:'fail',text:`missing system libraries: ${missing.join(', ')}`,fix:`sudo apt install ${apt}   (or your distribution's equivalent)`};
  }
  if (process.platform==='win32' && renderer==='webview') {
    const clients='Microsoft\\EdgeUpdate\\Clients\\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}';
    const keys=[`HKLM\\SOFTWARE\\WOW6432Node\\${clients}`,`HKLM\\SOFTWARE\\${clients}`,`HKCU\\Software\\${clients}`];
    const found = keys.some(key=>spawnSync('reg',['query',key,'/v','pv'],{stdio:'ignore'}).status === 0);
    return found ? {level:'ok',text:'WebView2 runtime installed'}
      : {level:'warn',text:'WebView2 runtime not detected',fix:'install it from https://developer.microsoft.com/microsoft-edge/webview2/ (Windows 11 includes it)'};
  }
  return null;
}
async function doctor() {
  const target = currentTarget();
  const lines = [];
  let blocking = 0;
  const report = (level,text,fix) => {
    lines.push(`  [${level}] ${text}`);
    if (fix) lines.push(`         fix: ${fix}`);
    if (level==='fail') blocking++;
  };
  console.log(`tsgpui ${VERSION} | Node ${process.version} | ${target}`);
  const nodeMajor = Number(process.versions.node.split('.')[0]);
  if (nodeMajor < 20) report('fail',`Node ${process.version} is too old (20 or newer is needed)`,'install Node 20+ from https://nodejs.org');
  else report('ok',`Node ${process.version}`);
  if (!['win32','darwin','linux'].includes(process.platform) || !['x64','arm64'].includes(process.arch)) {
    report('fail',`${target} has no prebuilt hosts and is not a tested build target`,'build the host from source on a supported machine');
  }

  // Which renderers matter: the current project's, when there is one, otherwise both.
  let projectRenderer = null;
  try {projectRenderer = (await readProject(projectDir())).config.renderer;} catch(_error) {}
  const renderers = flag('renderer',null) ? [flag('renderer')] : ['native','webview'];
  if (projectRenderer) lines.push(`Project renderer: ${projectRenderer}`);
  let missingHosts = 0;
  for (const renderer of renderers) {
    assertRenderer(renderer);
    lines.push(`${renderer} renderer:`);
    const explicit = explicitHost(renderer).find(file=>fs.existsSync(file));
    const cached = inspectCachedHost(renderer,VERSION);
    const installedStamp = installedSourceStamp(cached.file);
    const currentStamp = nativeSource() ? sourceStamp(nativeSource(), renderer) : null;
    lines.push(`  installed host path: ${cached.file}`);
    lines.push(`  installed source stamp: ${installedStamp || 'missing'}`);
    lines.push(`  current source stamp: ${currentStamp || 'unavailable'}`);
    lines.push(`  source stamps match: ${currentStamp && installedStamp === currentStamp && fs.existsSync(cached.file) ? 'yes' : 'no'}`);
    if (currentStamp && installedStamp !== currentStamp) report('warn', `installed ${renderer} host does not match the current source`, `tsgpui host build --renderer ${renderer}`);
    if (explicit) report('ok',`host from --host/environment: ${path.resolve(explicit)} (not checked for compatibility)`);
    else if (cached.state==='ok') {
      const intact = await verifyCachedHost(cached);
      if (intact===false) report('fail',`cached host fails its SHA-256 check: ${cached.file}`,`tsgpui host fetch --renderer ${renderer} --force`);
      else report('ok',`host ${cached.record.version} (${cached.record.source==='local cargo build'?'built locally':'checksum verified'}): ${cached.file}`);
    }
    else if (cached.state==='unverified') report('warn',`host present but ${cached.reason}: ${cached.file}`,`tsgpui host fetch --renderer ${renderer} --force   (replaces it with a verified one)`);
    else if (cached.state==='incompatible') report('fail',`cached host does not fit this CLI: ${cached.reason}`,`tsgpui host fetch --renderer ${renderer} --force`);
    else {
      const built = sourceBuilds(renderer).find(file=>fs.existsSync(file));
      if (built) report('ok',`host from a repository build: ${built}`);
      else {
        missingHosts++;
        report('warn',`no host installed yet; \`tsgpui dev\` and \`tsgpui build\` will fetch one on first use`,`tsgpui host fetch --renderer ${renderer}`);
      }
    }
    const libs = systemLibraryCheck(renderer);
    if (libs) report(libs.level,libs.text,libs.fix);
  }
  console.log(lines.join('\n'));
  const rust = rustAvailable();
  console.log(`Rust: ${rust ? 'available' : 'not installed'} (only needed to compile a host yourself${rust||nativeSource()?'':'; this installation also has no Rust sources'})`);
  if (missingHosts && downloadsDisabled()) console.log('Note: TSGPUI_NO_DOWNLOAD=1 is set, so hosts will not be downloaded.');
  console.log(blocking ? `\n${blocking} problem${blocking>1?'s':''} to fix before the app can run.` : '\nNo blocking problems.');
  if (blocking) process.exitCode = 1;
}
// tsgpui package [--installer nsis] [--sign] [--manifest-key PATH]
// Builds the Windows executable, optionally signs it, builds an NSIS installer, and writes a SHA-256 manifest.
// Signing uses TSGPUI_SIGN_COMMAND. The manifest is signed with an Ed25519 key from --manifest-key or TSGPUI_MANIFEST_KEY.
async function packageApp() {
  const installer = flag('installer', '');
  if (installer && installer !== 'nsis') throw new Error(`Unsupported installer: ${installer} (supported: nsis)`);
  if (process.platform !== 'win32') throw new Error('tsgpui package currently supports Windows only; build macOS apps with tsgpui build');
  const sign = args.includes('--sign');
  const result = await buildPackage(projectDir());
  printWarnings(result.warnings);
  const host = await hostPath(true, result.bundle.renderer);
  const output = path.join(result.paths.outDir, `${result.config.name}${suffix}`);
  const info = await packageExecutable(host, result.bundleFile, output);
  console.log(`Built native application: ${info.file}`);
  if (sign) await signArtifact(info.file);
  const artifacts = [info.file];
  if (installer === 'nsis') {
    const script = path.join(result.paths.outDir, `${result.config.name}.nsi`);
    const installerFile = path.join(result.paths.outDir, `${result.config.name}-setup${suffix}`);
    await buildNsis({name: result.config.title || result.config.name, version: result.config.version, executable: info.file, output: installerFile}, script);
    console.log(`Built installer: ${installerFile}`);
    if (sign) await signArtifact(installerFile);
    artifacts.push(installerFile);
  }
  const manifest = path.join(result.paths.outDir, `${result.config.name}.manifest.json`);
  await writeManifest(artifacts, manifest, flag('manifest-key', process.env.TSGPUI_MANIFEST_KEY || ''));
  console.log(`Wrote manifest: ${manifest}`);
}

function help() {
  console.log(`tsgpui ${VERSION}
  init [dir]                   Create a starter project
  import electron <dir> [--out DIR] [--build] [--force]  Import an Electron app
  compile [--project PATH]     Compile HTML/CSS/TS to a .tgp bundle
  dev [--project PATH]         Build, launch the selected host, rebuild on change
  preview [--project PATH]     Live preview in a browser (--out FILE writes one HTML file)
  build [--project PATH]       Package a single executable for the selected renderer
  host fetch [--renderer MODE] [--force]  Download and verify the prebuilt host (no Rust needed)
  host build [--renderer MODE] Compile the host from source instead (needs Rust)
  host install --file PATH --sha256 HASH [--renderer MODE]  Install a verified prebuilt host
  release manifest --file PATH Write a deterministic SHA-256 manifest to stdout
  package [--installer nsis] [--sign] [--manifest-key PATH]  Build the exe, sign it, make an installer and manifest (Windows)
  doctor [--renderer MODE]     Check the setup and print the exact fix for anything missing
Options: renderer is "native" (default) or "webview"; --host PATH overrides the selected host.`);
}
async function main() {
  if(command==='help' || command==='--help' || args.includes('--help')) return help();
  if(command==='--version' || command==='-v') return console.log(VERSION);
  if(command==='init') return init();
  if(command==='import') {
    if(args[0] !== 'electron' || !args[1] || args[1].startsWith('--')) throw new Error('Usage: tsgpui import electron <dir> [--out <dir>] [--build] [--force]');
    const options = args.slice(2);
    for(let i=0;i<options.length;i++) {
      if(options[i] === '--out') { if(!options[++i] || options[i].startsWith('--')) throw new Error('--out requires a directory'); }
      else if(!['--build','--force'].includes(options[i])) throw new Error(`Unknown import option: ${options[i]}`);
    }
    return importElectron(args[1],{out:flag('out'),build:args.includes('--build'),force:args.includes('--force')});
  }
  if(command==='doctor') return doctor();
  if(command==='release' && args[0]==='manifest') return releaseManifest();
  if(command==='host' && args[0]==='fetch') return fetchHosts();
  if(command==='host' && args[0]==='install') return installHost(flag('renderer','native'));
  if(command==='host' && args[0]==='build') return buildHost(flag('renderer','native'));
  if(command==='compile' || command==='build') {
    const result=await buildPackage(projectDir());
    printWarnings(result.warnings);
    console.log(`Compiled ${result.bundleFile} (${result.bytes.length} bytes)`);
    if(command==='compile') return;
    const host=await hostPath(true,result.bundle.renderer);
    const isMacApp = process.platform === 'darwin';
    const output=path.join(result.paths.outDir,`${result.config.name}${isMacApp ? '.app' : suffix}`);
    const info=isMacApp
      ? await packageMacApp(host,result.bundleFile,output,{iconFile:result.iconFile})
      : await packageExecutable(host,result.bundleFile,output);
    console.log(`Built native application: ${info.file}`);
    return;
  }
  if(command==='package') return packageApp();
  if(command==='dev') return dev();
  if(command==='preview') return preview();
  throw new Error(`Unknown command: ${command}. Run \`tsgpui help\`.`);
}
main().catch(error=>{console.error(`TSGPUI: ${error.message}`);process.exitCode=1;});
