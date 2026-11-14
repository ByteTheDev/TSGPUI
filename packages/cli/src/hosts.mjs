// Prebuilt host executables: where they live, how they are downloaded, and whether they fit this CLI.
//
// A release publishes one host per renderer and target, plus a `hosts.json` manifest with the SHA-256
// of each. `rendrix host fetch` downloads one, checks the hash and the size, and stores it in the
// per-user cache next to a small `.json` record. That record is what the compatibility check reads.
import {createHash} from 'node:crypto';
import fs from 'node:fs';
import {mkdir, readFile, rename, rm, writeFile} from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';

// "rendrix" is the Rendrix host (the engine). "native" is the deprecated legacy host, kept until the
// Rendrix M1 gate. The "webview" renderer was removed; assertRenderer rejects it with guidance.
export const RENDERERS = ['rendrix', 'native'];
export const BUNDLE_FORMAT = 'rendrix-1';
export const REPO_URL = 'https://github.com/ByteTheDev/Rendrix';
const MAX_HOST_BYTES = 512 * 1024 * 1024;

export function currentTarget() {return `${process.platform}-${process.arch}`;}
export function exeSuffix(target = currentTarget()) {return target.startsWith('win32-') ? '.exe' : '';}
export function rendrixHome() {return process.env.RENDRIX_HOME || path.join(os.homedir(), '.rendrix');}
export function hostBinaryName(renderer = 'rendrix') {return renderer === 'native' ? 'rendrix-legacy-host' : 'rendrix-host';}
export function cachedHostPath(renderer = 'rendrix', target = currentTarget()) {
  return path.join(rendrixHome(), 'host', target, `${hostBinaryName(renderer)}${exeSuffix(target)}`);
}
// Release asset name, for example rendrix-host-win32-x64.exe.
export function assetName(renderer, target) {return `${hostBinaryName(renderer)}-${target}${exeSuffix(target)}`;}
function recordPath(hostFile) {return `${hostFile}.json`;}

export function assertRenderer(renderer) {
  if (renderer === 'webview') throw new Error('The "webview" renderer was removed; Rendrix renders natively with Rendrix. Remove the renderer key from rendrix.config.json (it defaults to "rendrix"), or set it to "rendrix".');
  if (!RENDERERS.includes(renderer)) throw new Error(`renderer must be "rendrix" (default) or "native" (deprecated legacy host), not ${JSON.stringify(renderer)}`);
}
export async function sha256File(file) {return createHash('sha256').update(await readFile(file)).digest('hex');}

// Hash names and contents with length delimiters so file boundaries are unambiguous.
export function sourceStamp(source, renderer) {
  assertRenderer(renderer);
  const files = ['Cargo.toml', 'Cargo.lock'];
  function walk(dir, prefix) {
    for (const entry of fs.readdirSync(dir, {withFileTypes: true})) {
      const name = `${prefix}/${entry.name}`;
      if (entry.isDirectory()) walk(path.join(dir, entry.name), name);
      else if (entry.isFile() && entry.name.endsWith('.rs')) files.push(name);
    }
  }
  walk(path.join(source, 'src'), 'src');
  const hash = createHash('sha256');
  const add = data => { const bytes = Buffer.from(data); hash.update(`${bytes.length}:`); hash.update(bytes); };
  add('rendrix-host-source-v1'); add(renderer);
  for (const file of files.sort()) { add(file); add(fs.readFileSync(path.join(source, ...file.split('/')))); }
  return hash.digest('hex');
}

export function installedSourceStamp(file) {
  try {return JSON.parse(fs.readFileSync(recordPath(file), 'utf8')).sourceStamp || null;}
  catch (_error) {return null;}
}

// Hosts and the CLI must agree on the bundle format and on major.minor (0.x minors may break).
function releaseSeries(version) {return String(version).split('.').slice(0, 2).join('.');}

// 'missing' | 'unverified' (no record: a local build or a manual copy) | 'incompatible' | 'ok'
export function inspectCachedHost(renderer, version, target = currentTarget()) {
  const file = cachedHostPath(renderer, target);
  if (!fs.existsSync(file)) return {state: 'missing', file};
  let record;
  try {record = JSON.parse(fs.readFileSync(recordPath(file), 'utf8'));}
  catch (_error) {return {state: 'unverified', file, reason: 'it has no install record, so its version and checksum are unknown'};}
  if (record.bundleFormat !== BUNDLE_FORMAT) {
    return {state: 'incompatible', file, record, reason: `it reads bundle format ${record.bundleFormat}, this CLI writes ${BUNDLE_FORMAT}`};
  }
  if (releaseSeries(record.version) !== releaseSeries(version)) {
    return {state: 'incompatible', file, record, reason: `it is from release ${record.version}, this CLI is ${version}`};
  }
  return {state: 'ok', file, record};
}
// Re-hashes the cached file, which catches a corrupted or replaced host that still has its record.
export async function verifyCachedHost(info) {
  if (!info.record?.sha256) return null;
  return (await sha256File(info.file)) === info.record.sha256;
}

async function storeHost(renderer, target, data, record) {
  const destination = cachedHostPath(renderer, target);
  await mkdir(path.dirname(destination), {recursive: true});
  const temporary = `${destination}.${process.pid}.tmp`;
  await writeFile(temporary, data, {mode: 0o755});
  await rename(temporary, destination);
  await fs.promises.chmod(destination, 0o755);
  await writeFile(recordPath(destination), JSON.stringify(record, null, 2) + '\n');
  return destination;
}
export async function installHostFile({renderer, file, sha256, version, target = currentTarget()}) {
  assertRenderer(renderer);
  assertTarget(target);
  const data = await readFile(path.resolve(file));
  const actual = createHash('sha256').update(data).digest('hex');
  if (actual !== sha256.toLowerCase()) throw new Error(`Host SHA-256 mismatch: got ${actual}`);
  const destination = await storeHost(renderer, target, data, {
    version, bundleFormat: BUNDLE_FORMAT, renderer, target, sha256: actual, source: 'manual install',
  });
  return {file: destination, sha256: actual};
}
// Records a host built from source here, so that the compatibility check can vouch for it.
export async function recordLocalBuild({renderer, version, sourceStamp: stamp}) {
  const file = cachedHostPath(renderer);
  await writeFile(recordPath(file), JSON.stringify({
    version, bundleFormat: BUNDLE_FORMAT, renderer, target: currentTarget(), sha256: await sha256File(file), source: 'local cargo build', sourceStamp: stamp,
  }, null, 2) + '\n');
}

export function releaseBaseUrl(version) {
  return (process.env.RENDRIX_HOST_BASE_URL || `${REPO_URL}/releases/download/v${version}`).replace(/\/+$/, '');
}
export function downloadsDisabled() {return process.env.RENDRIX_NO_DOWNLOAD === '1';}

export function validateManifest(manifest, version) {
  if (!manifest || manifest.schema !== 2 || !Array.isArray(manifest.hosts)) throw new Error('hosts.json is not a recognised host manifest (schema 2)');
  if (manifest.bundleFormat !== BUNDLE_FORMAT) throw new Error(`hosts.json is for bundle format ${manifest.bundleFormat}, this CLI writes ${BUNDLE_FORMAT}`);
  if (releaseSeries(manifest.version) !== releaseSeries(version)) throw new Error(`hosts.json is for release ${manifest.version}, this CLI is ${version}`);
  for (const host of manifest.hosts) {
    if (!RENDERERS.includes(host.renderer) || typeof host.target !== 'string' || typeof host.asset !== 'string'
      || !/^[a-f0-9]{64}$/.test(host.sha256) || !Number.isSafeInteger(host.size) || host.size <= 0) {
      throw new Error('hosts.json has a malformed entry');
    }
    if (host.asset !== path.basename(host.asset) || host.asset !== assetName(host.renderer, host.target)) {
      throw new Error(`hosts.json lists an unexpected asset name '${host.asset}'`);
    }
  }
  return manifest;
}

async function download(url, limit) {
  let response;
  try {response = await fetch(url, {redirect: 'follow', signal: AbortSignal.timeout(10 * 60 * 1000)});}
  catch (error) {throw new Error(`could not reach ${url} (${error.cause?.code || error.message})`);}
  if (response.status === 404) throw new Error(`${url} does not exist (HTTP 404)`);
  if (!response.ok) throw new Error(`${url} answered HTTP ${response.status}`);
  const length = Number(response.headers.get('content-length'));
  if (length > limit) throw new Error(`${url} is larger than the ${limit} byte limit`);
  const data = Buffer.from(await response.arrayBuffer());
  if (data.length > limit) throw new Error(`${url} is larger than the ${limit} byte limit`);
  return data;
}

// The manifest shipped inside the npm package comes through a different channel than the release
// assets, so it is preferred when present: a swapped release asset then fails its hash.
export async function loadManifest({version, pkgRoot}) {
  const bundled = path.join(pkgRoot, 'hosts.json');
  if (fs.existsSync(bundled)) return {manifest: validateManifest(JSON.parse(fs.readFileSync(bundled, 'utf8')), version), origin: 'bundled with this package'};
  const url = `${releaseBaseUrl(version)}/hosts.json`;
  const data = await download(url, 1024 * 1024);
  let manifest;
  try {manifest = JSON.parse(data.toString('utf8'));} catch (_error) {throw new Error(`${url} is not valid JSON`);}
  return {manifest: validateManifest(manifest, version), origin: url};
}

// Downloads, verifies and installs the prebuilt host for this machine. Throws a message that says
// what to do next when no prebuilt host fits.
export const SUPPORTED_TARGETS = ['win32-x64', 'win32-arm64', 'darwin-arm64', 'linux-x64', 'linux-arm64'];
export function assertTarget(target) {
  if (!SUPPORTED_TARGETS.includes(target)) throw new Error(`Unsupported target ${target}. Use one of: ${SUPPORTED_TARGETS.join(', ')}`);
  return target;
}
export async function fetchHost({renderer, version, pkgRoot, target = currentTarget(), log = console.log}) {
  assertRenderer(renderer);
  assertTarget(target);
  if (downloadsDisabled()) throw new Error('downloads are disabled by RENDRIX_NO_DOWNLOAD=1');
  const {manifest, origin} = await loadManifest({version, pkgRoot});
  const entry = manifest.hosts.find(host => host.renderer === renderer && host.target === target);
  if (!entry) {
    const offered = [...new Set(manifest.hosts.filter(host => host.renderer === renderer).map(host => host.target))];
    throw new Error(`release ${manifest.version} has no prebuilt ${renderer} host for ${target}`
      + (offered.length ? ` (it has: ${offered.join(', ')})` : '') + '.');
  }
  const url = `${releaseBaseUrl(version)}/${entry.asset}`;
  log(`Downloading the ${renderer} host for ${target} (${(entry.size / 1048576).toFixed(1)} MiB)...`);
  const data = await download(url, Math.min(MAX_HOST_BYTES, entry.size));
  if (data.length !== entry.size) throw new Error(`${entry.asset} is ${data.length} bytes, the manifest says ${entry.size}`);
  const actual = createHash('sha256').update(data).digest('hex');
  if (actual !== entry.sha256) throw new Error(`${entry.asset} failed its SHA-256 check (expected ${entry.sha256}, got ${actual}); nothing was installed`);
  const file = await storeHost(renderer, target, data, {
    version: manifest.version, bundleFormat: BUNDLE_FORMAT, renderer, target, sha256: actual, source: url, manifest: origin,
  });
  log(`${renderer} host ${manifest.version} installed and verified: ${file}`);
  return file;
}

export async function removeCachedHost(renderer) {
  const file = cachedHostPath(renderer);
  await rm(file, {force: true});
  await rm(recordPath(file), {force: true});
}
