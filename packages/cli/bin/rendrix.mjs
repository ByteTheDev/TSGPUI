#!/usr/bin/env node
import {spawn, spawnSync} from 'node:child_process';
import fs from 'node:fs';
import {createHash} from 'node:crypto';
import {copyFile,mkdir,mkdtemp,readFile,writeFile} from 'node:fs/promises';
import os from 'node:os';
import {createRequire} from 'node:module';
import path from 'node:path';
import {fileURLToPath} from 'node:url';
import {buildPackage,readProject} from '../src/compiler.mjs';
import {importElectron} from '../src/import-electron.mjs';
import {packageExecutable,packageMacApp} from '../src/pack.mjs';
import {buildNsis} from '../src/installer/nsis.mjs';
import {signArtifact,resolveSignCommand,notarizeArtifact,writeManifest,findSigntool,findCodesign,findGpg,findNotarytool} from '../src/signing.mjs';
import {startPreview,writePreviewFile} from '../src/preview.mjs';
import {SUPPORTED_TARGETS,loadManifest,releaseBaseUrl,assertRenderer,assertTarget,cachedHostPath,currentTarget,downloadsDisabled,exeSuffix,fetchHost,hostBinaryName,inspectCachedHost,installHostFile,installedSourceStamp,recordLocalBuild,sourceStamp,rendrixHome,verifyCachedHost} from '../src/hosts.mjs';

// This file works both from a monorepo checkout and from an installed package.
const here = path.dirname(fileURLToPath(import.meta.url));
const pkgRoot = path.resolve(here,'..');
const args = process.argv.slice(2);
const command = args.shift() || 'run';
const VERSION = JSON.parse(fs.readFileSync(path.join(pkgRoot,'package.json'),'utf8')).version;
function flag(name, fallback) { const i=args.indexOf(`--${name}`); return i>=0 ? args[i+1] || fallback : fallback; }
function projectDir() {return path.resolve(flag('project',process.cwd()));}
function rustAvailable() {return spawnSync('cargo',['--version'],{stdio:'ignore'}).status === 0;}

const suffix = exeSuffix();
function cachedHost(renderer='rendrix') {return cachedHostPath(renderer);}

// Rust sources: shipped inside the package, or next to it in a monorepo checkout.
function nativeSource() {
  for (const dir of [path.join(pkgRoot,'native'), path.resolve(pkgRoot,'../../native')]) {
    if (fs.existsSync(path.join(dir,'Cargo.toml'))) return dir;
  }
  return null;
}
function explicitHost(renderer='rendrix') {
  return [flag('host',null), process.env.RENDRIX_HOST].filter(Boolean);
}
function sourceBuilds(renderer='rendrix') {
  const source = nativeSource();
  return source ? ['release','debug'].map(profile=>path.join(source,'target',profile,`${hostBinaryName(renderer)}${suffix}`)) : [];
}
// Explicit overrides are deliberate; cached hosts must match the available sources.
async function hostPath(autoBuild = false, renderer='rendrix') {
  for(const candidate of explicitHost(renderer)) if (fs.existsSync(candidate)) return path.resolve(candidate);
  const cached = inspectCachedHost(renderer,VERSION);
  const source = nativeSource();
  // A checkout must use a host built from its current sources. Installed packages can use
  // the release host, even though they may also ship Rust sources for optional local builds.
  const checkoutSource = source && path.resolve(source) !== path.resolve(pkgRoot,'native');
  if (checkoutSource) {
    const stamp = sourceStamp(source, renderer);
    if (cached.state === 'ok' && installedSourceStamp(cached.file) === stamp && await verifyCachedHost(cached) === true) return path.resolve(cached.file);
    if (!autoBuild || !rustAvailable()) throw new Error(`The installed ${renderer} host is missing or does not match the current source. Install Rust, then run \`rendrix host build --renderer ${renderer}\`.`);
    console.log(`Building the ${renderer} host to match the current source...`);
    await buildHost(renderer);
    return path.resolve(cachedHost(renderer));
  }
  if (cached.state === 'ok' && await verifyCachedHost(cached) === true) return path.resolve(cached.file);
  if (!autoBuild || downloadsDisabled()) throw new Error(`No verified ${renderer} host is installed. Run \`rendrix host fetch --renderer ${renderer}\` or install one with \`rendrix host install\`.`);
  return path.resolve(await fetchHost({renderer,version:VERSION,pkgRoot}));
}
// Host for a target. The current platform may build from source; other targets use a verified prebuilt host.
async function hostPathFor(autoBuild = false, renderer='rendrix', target=currentTarget()) {
  if (target === currentTarget()) return hostPath(autoBuild, renderer);
  for(const candidate of explicitHost(renderer)) if (fs.existsSync(candidate)) return path.resolve(candidate);
  const cached = inspectCachedHost(renderer, VERSION, target);
  if (cached.state === 'ok' && await verifyCachedHost(cached) === true) return path.resolve(cached.file);
  if (downloadsDisabled()) throw new Error(`No ${renderer} host for ${target} is installed, and downloads are disabled. Copy one in with \`rendrix host install --renderer ${renderer} --target ${target}\`.`);
  await fetchHost({renderer, version: VERSION, pkgRoot, target});
  return path.resolve(cachedHostPath(renderer, target));
}
// Compiles a project and packages it for one target. Returns the packaged application's info.
async function buildApp(dir, target=currentTarget(), namedTarget=false) {
  assertTarget(target);
  const result = await buildPackage(dir);
  printWarnings(result.warnings);
  console.log(`Compiled ${result.bundleFile} (${result.bytes.length} bytes)`);
  const host = await hostPathFor(true, result.bundle.renderer, target);
  const isMacApp = target.startsWith('darwin-');
  const output = path.join(result.paths.outDir, `${result.config.name}${namedTarget ? `-${target}` : ''}${isMacApp ? '.app' : exeSuffix(target)}`);
  const info = isMacApp
    ? await packageMacApp(host, result.bundleFile, output, {iconFile: result.iconFile})
    : await packageExecutable(host, result.bundleFile, output);
  console.log(`Built native application (${target}): ${info.file}`);
  return info;
}
// --target accepts one target, a comma list, or `all` (every supported target). Default: this machine.
function requestedTargets() {
  const value = flag('target', currentTarget());
  if (value === 'all') return [...SUPPORTED_TARGETS];
  const targets = value.split(',').map(t => t.trim()).filter(Boolean);
  if (!targets.length) throw new Error('--target needs a value: a target, a comma-separated list, or all');
  return [...new Set(targets.map(t => assertTarget(t.replace(/^windows-/, 'win32-').replace(/^macos-/, 'darwin-'))))];
}
// One target builds directly. Several targets build in turn; a failure is reported and the rest still build.
async function buildTargets(dir, targets) {
  if (targets.length === 1) return buildApp(dir, targets[0], args.includes('--target'));
  if (explicitHost('rendrix').length || explicitHost('native').length) throw new Error('A host override cannot be used for multiple targets; install a matching host for each target.');
  const failed = [];
  for (const target of targets) {
    console.log(`\n== ${target}`);
    try { await buildApp(dir, target, true); }
    catch (error) { failed.push({target, message: error.message}); console.error(`rendrix: ${target} failed: ${error.message}`); }
  }
  console.log(`\nBuilt ${targets.length - failed.length} of ${targets.length} targets.`);
  for (const {target, message} of failed) console.log(`  ${target}: ${message}`);
  if (failed.length) process.exitCode = 1;
}
// Prepare Electron projects in a fresh directory so the original project is never rewritten.
async function prepareProject(input) {
  // rendrix.config.json is the canonical name; rendrix.config.json is the pre-rename name, still read
  // by readProject with a deprecation warning.
  if (fs.existsSync(path.join(input, 'rendrix.config.json'))) return input;
  if (fs.existsSync(path.join(input, 'rendrix.config.json'))) return input;
  if (!fs.existsSync(path.join(input, 'package.json'))) throw new Error(`No rendrix.config.json or Electron package.json found in ${input}`);
  const temp = await mkdtemp(path.join(os.tmpdir(), 'rendrix-project-'));
  const options = {out: path.join(temp, 'app')};
  let imported;
  try {
    imported = await importElectron(input, options);
  } catch (error) {
    // No renderer has been built yet: run the app's own build script, then import again.
    if (!/No built renderer entry/.test(error.message)) throw error;
    console.log('No built renderer found; running the project build script first.');
    imported = await importElectron(input, {...options, build: true});
  }
  console.log(`Electron compatibility is incomplete. Review ${path.join(imported.out, 'RENDRIX_IMPORT_REPORT.md')} for limitations.`);
  return imported.out;
}

async function runProject() {
  const project = await prepareProject(projectDir());
  const result = await buildPackage(project);
  printWarnings(result.warnings);
  const host = await hostPath(true, result.bundle.renderer);
  const child = runHost(host, result.bundleFile);
  child.once('error', () => { process.exitCode = 1; });
  child.once('close', (code, signal) => { if (code !== 0 || signal) process.exitCode = code || 1; });
}

async function buildHost(renderer='rendrix') {
  const source = nativeSource();
  if (!source) throw new Error('Host sources not found in this installation. Run `rendrix host fetch` for a prebuilt host, or use a checkout of the Rendrix repository.');
  assertRenderer(renderer);
  if (!rustAvailable()) throw new Error(`Rust/Cargo is required to compile the ${renderer} host. Install it from https://rustup.rs, or run \`rendrix host fetch --renderer ${renderer}\` for a prebuilt host.`);
  const targetDir = path.join(rendrixHome(),'build');
  const stamp = sourceStamp(source, renderer);
  const cargoArgs = ['build','--manifest-path',path.join(source,'Cargo.toml'),'--release','--target-dir',targetDir];
  if (renderer==='native') cargoArgs.push('--features','native-host','--bin','rendrix-legacy-host');
  else cargoArgs.push('--no-default-features','--features','rendrix','--bin','rendrix-host');
  const res = spawnSync('cargo',cargoArgs,{stdio:'inherit'});
  if (res.status !== 0) throw new Error(`Host build failed (code ${res.status})`);
  if (sourceStamp(source, renderer) !== stamp) throw new Error('Host source changed during compilation; run the host build again.');
  const destination = cachedHost(renderer);
  await mkdir(path.dirname(destination),{recursive:true});
  await copyFile(path.join(targetDir,'release',`${hostBinaryName(renderer)}${suffix}`),destination);
  await recordLocalBuild({renderer,version:VERSION,sourceStamp:stamp});
  console.log(`${renderer} host installed: ${destination}`);
}
async function fetchHosts() {
  const requested = flag('renderer',null);
  const renderers = requested ? [requested] : ['rendrix','native'];
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
async function installHost(renderer='rendrix') {
  const file=flag('file',null), expected=flag('sha256',null);
  if (!file || !expected || !/^[a-fA-F0-9]{64}$/.test(expected)) throw new Error('Use `rendrix host install --file PATH --sha256 HASH [--renderer rendrix|native]` with a checksum from a trusted source.');
  const result=await installHostFile({renderer,file,sha256:expected,version:VERSION,target:flag('target',currentTarget())});
  console.log(`${renderer} host installed: ${result.file} (SHA-256 ${result.sha256})`);
}
// Deterministic SHA-256 manifest of a file or directory, for publishing beside a release artifact.
async function releaseManifest() {
  const target = flag('file',null);
  if (!target) throw new Error('Use `rendrix release manifest --file PATH`.');
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
// Starter templates live in packages/cli/templates/<directory>. `rendrix init --template NAME` copies one.
// "app" templates are Rendrix projects and get a package.json with the rendrix scripts; the electron template is
// a plain Electron project (it carries its own package.json) meant to be run through `rendrix import electron`.
const TEMPLATES = {
  counter: {dir:'basic', kind:'app', summary:'a click counter: state, actions and styling in the smallest app'},
  form: {dir:'form', kind:'app', summary:'a validated form: text, email, number, select, checkbox and a repeated result list'},
  'list-detail': {dir:'list-detail', kind:'app', summary:'a searchable list next to a detail pane, driven by backend state'},
  'electron-import': {dir:'electron-import', kind:'electron', summary:'a plain Electron app to try `rendrix import electron` on'}
};
async function copyTree(from,to,transform) {
  await mkdir(to,{recursive:true});
  for (const entry of await fs.promises.readdir(from,{withFileTypes:true})) {
    const source = path.join(from,entry.name), target = path.join(to,entry.name);
    if (entry.isDirectory()) await copyTree(source,target,transform);
    else await writeFile(target,transform(await readFile(source,'utf8'),entry.name));
  }
}
async function init() {
  if (args.includes('--list-templates')) {
    for (const [name,info] of Object.entries(TEMPLATES)) console.log(`  ${name.padEnd(16)} ${info.summary}`);
    return;
  }
  const requested = flag('template','counter');
  const templateName = requested === 'basic' ? 'counter' : requested;
  const info = TEMPLATES[templateName];
  if (!info) throw new Error(`Unknown template '${requested}'. Available: ${Object.keys(TEMPLATES).join(', ')} (see \`rendrix init --list-templates\`)`);
  // The folder is the first argument that is not a flag or a flag's value.
  const positional = args.filter((x,i) => !x.startsWith('--') && !(i>0 && args[i-1]==='--template'));
  const folder = positional[0] ?? (info.kind==='electron' ? 'my-electron-app' : 'my-rendrix-app');
  const destination = path.resolve(folder);
  if (fs.existsSync(destination) && fs.readdirSync(destination).length) throw new Error('Project destination is not empty');
  const appName = path.basename(destination);
  const template = path.join(pkgRoot,'templates',info.dir);
  const relative = path.relative(process.cwd(),destination) || '.';
  const cd = relative === '.' ? '' : `cd ${relative.includes(' ') ? `"${relative}"` : relative}\n  `;
  if (info.kind === 'electron') {
    await copyTree(template,destination,(text,name) => name==='package.json' ? text.replace(/"name": "electron-import-demo"/,`"name": ${JSON.stringify(appName)}`) : text);
    console.log(`Created ${destination}

This is a plain Electron app. Try the importer on it:
  ${cd}rendrix import electron . --out ../${appName}-rendrix --build
  rendrix run ../${appName}-rendrix

Or run it in place (Rendrix converts it into a temporary project first):
  rendrix run .

The import writes RENDRIX_IMPORT_REPORT.md, which lists every Electron API the app uses and how well Rendrix supports it.`);
    return;
  }
  await copyTree(template,destination,(text,name) => name==='rendrix.config.json' ? text.replace(/"name": "[^"]*"/,`"name": ${JSON.stringify(appName)}`) : text);
  const pkg = {
    name: appName,private:true,type:'module',
    scripts:{dev:'rendrix dev',build:'rendrix build',compile:'rendrix compile'},
    devDependencies:{rendrix:`^${VERSION}`}
  };
  await writeFile(path.join(destination,'package.json'),JSON.stringify(pkg,null,2)+'\n');
  console.log(`Created ${destination} (template: ${templateName})

Next:
  ${cd}rendrix dev      # opens the app and reloads on every save
  rendrix build   # packages a single executable into dist/

The first run downloads a verified prebuilt host (no Rust needed). \`rendrix doctor\` checks your setup.
Other starters: \`rendrix init --list-templates\`.`);
}
// Skipped CSS declarations are reported, not silent. With strictCss in the config they are errors instead.
function printWarnings(warnings=[]) {
  for (const warning of warnings) console.warn(`Warning: ${warning}`);
}
function runHost(host,bundleFile) {
  const child=spawn(host,['--bundle',bundleFile],{stdio:'inherit',windowsHide:false});
  child.on('error',e => console.error(`Rendrix host failed: ${e.message}`));
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
    const wanted = ['xkbcommon','wayland-client','xcb'];
    if (spawnSync('pkg-config',['--version'],{stdio:'ignore'}).status !== 0) return {level:'warn',text:'pkg-config is not installed, so system libraries could not be checked',fix:'sudo apt install pkg-config'};
    const missing = wanted.filter(lib=>spawnSync('pkg-config',['--exists',lib],{stdio:'ignore'}).status !== 0);
    if (!missing.length) return {level:'ok',text:`system libraries present (${wanted.join(', ')})`};
    return {level:'fail',text:`missing system libraries: ${missing.join(', ')}`,fix:'sudo apt install libxkbcommon-dev libwayland-dev libxcb1-dev   (or your distribution\'s equivalent)'};
  }
  return null;
}
// The release manifest (hosts.json) is what a download is verified against. Check that it is readable,
// matches this CLI, and lists a host for this machine; with --online, also that the asset can be reached.
async function checkReleaseManifest(renderer,report,{explicit,cached}) {
  if (explicit) return;
  let loaded;
  try { loaded = await loadManifest({version:VERSION,pkgRoot}); }
  catch (error) {
    // A repository checkout has no bundled manifest and its version has no release, so a missing one is only a note there.
    const offline = downloadsDisabled();
    if (offline || nativeSource()) { report('ok',`release manifest not checked (${offline ? 'downloads are disabled' : 'repository checkout'}): ${error.message}`); return; }
    report(cached.state==='ok' ? 'warn' : 'fail',`cannot read the host manifest: ${error.message}`,'update rendrix (npm i -g @bytethedeveloper/rendrix@latest) or check your network and RENDRIX_HOST_BASE_URL');
    return;
  }
  const entry = loaded.manifest.hosts.find(host=>host.renderer===renderer && host.target===currentTarget());
  if (!entry) {
    const offered = [...new Set(loaded.manifest.hosts.filter(host=>host.renderer===renderer).map(host=>host.target))];
    report('warn',`release ${loaded.manifest.version} has no prebuilt ${renderer} host for ${currentTarget()}${offered.length?` (it has: ${offered.join(', ')})`:''}`,`rendrix host build --renderer ${renderer}   (compiles one; needs Rust)`);
    return;
  }
  report('ok',`manifest ${loaded.manifest.version} (${loaded.origin}) lists ${entry.asset}, ${(entry.size/1048576).toFixed(1)} MiB, sha256 ${entry.sha256.slice(0,12)}...`);
  if (cached.state==='ok' && cached.record?.sha256 && cached.record.source !== 'local cargo build' && cached.record.sha256 !== entry.sha256) {
    report('warn',`the cached ${renderer} host differs from the manifest's hash for this release`,`rendrix host fetch --renderer ${renderer} --force`);
  }
  if (args.includes('--online')) {
    const url = `${releaseBaseUrl(VERSION)}/${entry.asset}`;
    try {
      const response = await fetch(url,{method:'HEAD',redirect:'follow',signal:AbortSignal.timeout(15000)});
      if (response.ok) report('ok',`download reachable: ${url}`);
      else report('fail',`download answered HTTP ${response.status}: ${url}`,'check your network, a proxy, or RENDRIX_HOST_BASE_URL');
    } catch (error) {
      report('fail',`cannot reach ${url} (${error.cause?.code || error.message})`,'check your network, a proxy, or RENDRIX_HOST_BASE_URL; or install a host with `rendrix host install --file PATH --sha256 HASH`');
    }
  }
}
// What compiling a project needs: TypeScript to lower the backend and PostCSS to read the stylesheet.
function checkToolchain(report) {
  for (const [name,fix] of [['typescript','npm install typescript'],['postcss','npm install postcss']]) {
    try {
      const file = createRequire(import.meta.url).resolve(`${name}/package.json`);
      report('ok',`${name} ${JSON.parse(fs.readFileSync(file,'utf8')).version}`);
    } catch (_error) {
      // The published CLI inlines its dependencies, so a missing package only matters in a source checkout.
      if (nativeSource()) report('fail',`${name} is not installed`,fix);
    }
  }
  const gh = spawnSync('gh',['--version'],{stdio:'ignore'});
  if (gh.status !== 0) console.log('Note: GitHub CLI (gh) not found; it is only needed to run `gh attestation verify` on release hosts.');
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
  console.log(`rendrix ${VERSION} | Node ${process.version} | ${target}`);
  const nodeMajor = Number(process.versions.node.split('.')[0]);
  if (nodeMajor < 20) report('fail',`Node ${process.version} is too old (20 or newer is needed)`,'install Node 20+ from https://nodejs.org');
  else report('ok',`Node ${process.version}`);
  if (!['win32','darwin','linux'].includes(process.platform) || !['x64','arm64'].includes(process.arch)) {
    report('fail',`${target} has no prebuilt hosts and is not a tested build target`,'build the host from source on a supported machine');
  }

  // Which renderers matter: the current project's, when there is one, otherwise both.
  let projectRenderer = null;
  try {projectRenderer = (await readProject(projectDir())).config.renderer;} catch(_error) {}
  const renderers = flag('renderer',null) ? [flag('renderer')] : ['rendrix','native'];
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
    if (currentStamp && installedStamp !== currentStamp) report('warn', `installed ${renderer} host does not match the current source`, `rendrix host build --renderer ${renderer}`);
    if (explicit) report('ok',`host from --host/environment: ${path.resolve(explicit)} (not checked for compatibility)`);
    else if (cached.state==='ok') {
      const intact = await verifyCachedHost(cached);
      if (intact===false) report('fail',`cached host fails its SHA-256 check: ${cached.file}`,`rendrix host fetch --renderer ${renderer} --force`);
      else report('ok',`host ${cached.record.version} (${cached.record.source==='local cargo build'?'built locally':'checksum verified'}): ${cached.file}`);
    }
    else if (cached.state==='unverified') report('warn',`host present but ${cached.reason}: ${cached.file}`,`rendrix host fetch --renderer ${renderer} --force   (replaces it with a verified one)`);
    else if (cached.state==='incompatible') report('fail',`cached host does not fit this CLI: ${cached.reason}`,`rendrix host fetch --renderer ${renderer} --force`);
    else {
      const built = sourceBuilds(renderer).find(file=>fs.existsSync(file));
      if (built) report('ok',`host from a repository build: ${built}`);
      else {
        missingHosts++;
        report('warn',`no host installed yet; \`rendrix dev\` and \`rendrix build\` will fetch one on first use`,`rendrix host fetch --renderer ${renderer}`);
      }
    }
    const libs = systemLibraryCheck(renderer);
    if (libs) report(libs.level,libs.text,libs.fix);
    await checkReleaseManifest(renderer,report,{ explicit: Boolean(explicit), cached });
  }
  checkToolchain(report);
  console.log(lines.join('\n'));
  const rust = rustAvailable();
  console.log(`Rust: ${rust ? 'available' : 'not installed'} (only needed to compile a host yourself${rust||nativeSource()?'':'; this installation also has no Rust sources'})`);
  if (missingHosts && downloadsDisabled()) console.log('Note: RENDRIX_NO_DOWNLOAD=1 is set, so hosts will not be downloaded.');
  console.log(blocking ? `\n${blocking} problem${blocking>1?'s':''} to fix before the app can run.` : '\nNo blocking problems.');
  if (blocking) process.exitCode = 1;
}
// rendrix package [--installer nsis] [--sign] [--manifest-key PATH] [--publish-url URL]
// Builds the Windows executable, optionally signs it, builds an NSIS installer, and writes a SHA-256 manifest.
// Signing uses RENDRIX_SIGN_COMMAND (from win.certificateFile/certificateSubjectName in migrated projects).
// The manifest is signed with an Ed25519 key from --manifest-key or RENDRIX_MANIFEST_KEY.
// Publishing is manual: upload the artifacts and manifest to publishUrl (from packaging.publishUrl or --publish-url).
// In-app updates use autoUpdater against the published installer plus .manifest.json; Rendrix provides the runtime shim and release artifacts.
async function packageApp() {
  const installer = flag('installer', '');
  if (installer && installer !== 'nsis') throw new Error(`Unsupported installer: ${installer} (supported: nsis)`);
  if (process.platform !== 'win32') throw new Error('rendrix package currently supports Windows only; build macOS apps with rendrix build');
  const sign = args.includes('--sign');
  const result = await buildPackage(projectDir());
  printWarnings(result.warnings);
  let packaging = {};
  const configFilePath = ['rendrix.config.json', 'rendrix.config.json'].map(name => path.join(projectDir(), name)).find(file => fs.existsSync(file)) ?? path.join(projectDir(), 'rendrix.config.json');
  try { packaging = JSON.parse(fs.readFileSync(configFilePath, 'utf8')).packaging ?? {}; } catch {}
  const publishUrl = flag('publish-url', packaging.publishUrl || '');
  const certificateSubjectName = packaging.certificateSubjectName || '';
  const host = await hostPath(true, result.bundle.renderer);
  const output = path.join(result.paths.outDir, `${result.config.name}${suffix}`);
  const info = await packageExecutable(host, result.bundleFile, output);
  console.log(`Built native application: ${info.file}`);
  if (sign) await signArtifact(info.file);
  if (sign && !resolveSignCommand()) console.log('Signing skipped: set RENDRIX_SIGN_COMMAND=<your sign command>, or RENDRIX_CERT_FILE (+RENDRIX_CERT_PASSWORD) on Windows / RENDRIX_CODESIGN_IDENTITY on macOS (win.certificateFile / certificateSubjectName map here; keep the certificate and password outside the repo) and re-run `rendrix package --installer nsis --sign`.');
  if (certificateSubjectName) console.log(`Certificate subject (packaging.certificateSubjectName): ${certificateSubjectName}. Set RENDRIX_SIGN_COMMAND to sign with that certificate.`);
  const artifacts = [info.file];
  if (installer === 'nsis') {
    const script = path.join(result.paths.outDir, `${result.config.name}.nsi`);
    const installerFile = path.join(result.paths.outDir, `${result.config.name}-setup${suffix}`);
    await buildNsis({name: result.config.title || result.config.name, version: result.config.version, executable: info.file, output: installerFile, publishUrl, certificateSubjectName}, script);
    console.log(`Built installer: ${installerFile}`);
    if (sign) await signArtifact(installerFile);
    artifacts.push(installerFile);
  }
  const manifest = path.join(result.paths.outDir, `${result.config.name}.manifest.json`);
  await writeManifest(artifacts, manifest, flag('manifest-key', process.env.RENDRIX_MANIFEST_KEY || ''), result.config.version);
  console.log(`Wrote manifest: ${manifest}`);
  if (publishUrl) console.log(`Publish (manual): upload ${[...artifacts, manifest, ...(fs.existsSync(manifest + '.sig') ? [manifest + '.sig'] : [])].map(file => path.basename(file)).join(', ')} to publishUrl ${publishUrl}. Configure the app's autoUpdater feed URL to this publishUrl (plus the manifest) so installed apps can update in place.`);
  if (installer === 'nsis' || sign || publishUrl) console.log('Auto-update: configure autoUpdater.setFeedURL(...) to the published manifest URL; Rendrix downloads, verifies and installs the update on quitAndInstall().');
}

// Sign (and optionally notarize) an arbitrary artifact on any platform.
async function signCommand() {
  const file = flag('file', null);
  if (!file) throw new Error('Use `rendrix sign --file PATH [--notarize]`.');
  const target = path.resolve(file);
  if (!fs.existsSync(target)) throw new Error(`File not found: ${target}`);
  const signed = await signArtifact(target);
  if (signed) console.log(`Signed: ${target}`);
  if (args.includes('--notarize')) await notarizeArtifact(target);
}
function help() {
  console.log(`rendrix ${VERSION}
  init [dir] [--template NAME] Create a starter project (counter, form, list-detail, electron-import; --list-templates)
  import electron <dir> [--out DIR] [--build] [--force] [--fix]  Import an Electron app
  run [--project PATH]         Run a Rendrix or Electron project (default)
  compile [--project PATH]     Compile HTML/CSS/TS to a .tgp bundle
  dev [--project PATH]         Build, launch the selected host, rebuild on change
  preview [--project PATH]     Live preview in a browser (--out FILE writes one HTML file)
  build [--project PATH] [--target T]  Package for T: a target, a comma list, or "all" (win32-x64, win32-arm64, darwin-arm64, linux-x64, linux-arm64; default: this machine)
  <target>|all [--project PATH]  Shortcut for build --target (windows-* and macos-* aliases accepted)
  host fetch [--renderer MODE] [--force]  Download and verify the prebuilt host (no Rust needed)
  host build [--renderer MODE] Compile the host from source instead (needs Rust)
  host install --file PATH --sha256 HASH [--renderer MODE]  Install a verified prebuilt host
  release manifest --file PATH Write a deterministic SHA-256 manifest to stdout
  package [--installer nsis] [--sign] [--manifest-key PATH] [--publish-url URL]  Build the exe, sign it, make an installer and manifest (Windows)
  sign --file PATH [--notarize]  Sign an artifact (Windows/macOS/Linux via RENDRIX_SIGN_COMMAND or platform env vars)
  doctor [--renderer MODE] [--online]  Check the host, its hashes, the manifest and the toolchain; --online also probes the download URL
Options: renderer is "rendrix" (default) or "native" (deprecated legacy host); "webview" was removed; --host PATH overrides the selected host.`);
}
async function main() {
  if (command === 'all' || /^(win32|windows|darwin|macos|linux)-(x64|arm64)$/.test(command)) {
    args.push('--target', command);
    const targets = requestedTargets();
    return buildTargets(await prepareProject(projectDir()), targets);
  }
  if(command==='help' || command==='--help' || args.includes('--help')) return help();
  if(command==='--version' || command==='-v') return console.log(VERSION);
  if(command==='init') return init();
  if(command==='import') {
    if(args[0] !== 'electron' || !args[1] || args[1].startsWith('--')) throw new Error('Usage: rendrix import electron <dir> [--out <dir>] [--build] [--force] [--fix]');
    const options = args.slice(2);
    for(let i=0;i<options.length;i++) {
      if(options[i] === '--out') { if(!options[++i] || options[i].startsWith('--')) throw new Error('--out requires a directory'); }
      else if(!['--build','--force','--fix'].includes(options[i])) throw new Error(`Unknown import option: ${options[i]}`);
    }
    if(args.includes('--fix')) {
      const input = args[1];
      const out = flag('out') || path.resolve(input) + '-rendrix';
      const force = args.includes('--force');
      console.log(`--fix (dry-run): would apply fixes from RENDRIX_IMPORT_REPORT.md in ${out}.`);
      try {
        const pkg = JSON.parse(fs.readFileSync(path.join(path.resolve(input), 'package.json'), 'utf8'));
        if (pkg.build?.appId) console.log(`--fix: add "bundleId": "${pkg.build.appId}" to rendrix.config.json`);
        if (pkg.build?.directories?.output) console.log(`--fix: set "outDir" to "${pkg.build.directories.output}" in rendrix.config.json`);
        if (pkg.build?.productName) console.log(`--fix: set "title" to "${pkg.build.productName}" in rendrix.config.json`);
        if (pkg.build?.win?.target) console.log(`--fix: run \`rendrix package --installer nsis\` for win.target ${pkg.build.win.target}`);
        if (pkg.build?.win?.certificateFile) console.log(`--fix: win.certificateFile "${pkg.build.win.certificateFile}" maps to RENDRIX_SIGN_COMMAND=<your sign command>; run \`rendrix package --installer nsis --sign\` (keep the certificate and password outside the repo)`);
        if (pkg.build?.win?.certificateSubjectName) console.log(`--fix: certificateSubjectName "${pkg.build.win.certificateSubjectName}" maps to RENDRIX_SIGN_COMMAND=<your sign command>; run \`rendrix package --installer nsis --sign\``);
        if (pkg.build?.publish) console.log(`--fix: publish maps to packaging.publishUrl; upload signed artifacts and .manifest.json to the publish URL by hand (Rendrix has no auto-publish), after \`rendrix package --installer nsis --sign\` with RENDRIX_MANIFEST_KEY=<path to Ed25519 key>`);
        if (pkg.build?.win?.target && !pkg.build?.win?.certificateFile && !pkg.build?.win?.certificateSubjectName) console.log('--fix: signing is manual: set RENDRIX_SIGN_COMMAND=<your sign command> and RENDRIX_MANIFEST_KEY=<path to Ed25519 key>, then run `rendrix package --installer nsis --sign`');
        if (pkg.build?.win?.target) console.log('--fix: autoUpdater is unsupported in Rendrix; ship updates via the Rendrix release process (docs/BUILDING.md signing/auto-update subsection)');
      } catch {}
      console.log('--fix: edit permissions (network/filesystem/processes) in rendrix.config.json, then run `rendrix build` in ' + out + '.');
      console.log('--fix: review RENDRIX_IMPORT_REPORT.md for file:line Fix: commands.');
      if (!force) {
        console.log('--fix: no files written. Add --force to allow overwriting an existing output directory.');
        return;
      }
      console.log('--fix: --force given, proceeding with import (overwrite allowed).');
    }
    return importElectron(args[1],{out:flag('out'),build:args.includes('--build'),force:args.includes('--force')});
  }
  if(command==='doctor') return doctor();
  if(command==='release' && args[0]==='manifest') return releaseManifest();
  if(command==='host' && args[0]==='fetch') return fetchHosts();
  if(command==='host' && args[0]==='install') return installHost(flag('renderer','rendrix'));
  if(command==='host' && args[0]==='build') return buildHost(flag('renderer','rendrix'));
  if(command==='run') return runProject();
  if(command==='compile') {
    const result=await buildPackage(projectDir());
    printWarnings(result.warnings);
    console.log(`Compiled ${result.bundleFile} (${result.bytes.length} bytes)`);
    return;
  }
  if(command==='build') {
    const targets = requestedTargets();
    return buildTargets(await prepareProject(projectDir()), targets);
  }
  if(command==='package') return packageApp();
  if(command==='sign') return signCommand();
  if(command==='dev') return dev();
  if(command==='preview') return preview();
  throw new Error(`Unknown command: ${command}. Run \`rendrix help\`.`);
}
main().catch(error=>{console.error(`rendrix: ${error.message}`);process.exitCode=1;});
