// Writes hosts.json for a release: one entry per host executable found in a directory.
//   node scripts/make-host-manifest.mjs <directory-with-hosts> <version>
// Hosts must be named like tsgpui-host-native-win32-x64.exe (see assetName in src/hosts.mjs).
import {createHash} from 'node:crypto';
import {readdir, readFile, stat, writeFile} from 'node:fs/promises';
import path from 'node:path';
import {BUNDLE_FORMAT, assetName} from '../src/hosts.mjs';

const [directory, version] = process.argv.slice(2);
if (!directory || !/^\d+\.\d+\.\d+/.test(version ?? '')) {
  console.error('usage: make-host-manifest.mjs <directory> <version>');
  process.exit(2);
}
const pattern = /^tsgpui-host-(native|webview)-((?:win32|darwin|linux)-(?:x64|arm64))(?:\.exe)?$/;
const hosts = [];
for (const name of (await readdir(directory)).sort()) {
  const match = pattern.exec(name);
  if (!match) continue;
  const [, renderer, target] = match;
  if (assetName(renderer, target) !== name) throw new Error(`${name} has the wrong extension for ${target}`);
  const file = path.join(directory, name);
  hosts.push({renderer, target, asset: name, sha256: createHash('sha256').update(await readFile(file)).digest('hex'), size: (await stat(file)).size});
}
if (!hosts.length) throw new Error(`no host executables found in ${directory}`);
const manifest = {schema: 1, version, bundleFormat: BUNDLE_FORMAT, hosts};
await writeFile(path.join(directory, 'hosts.json'), JSON.stringify(manifest, null, 2) + '\n');
console.log(`Wrote hosts.json with ${hosts.length} hosts:`);
for (const host of hosts) console.log(`  ${host.asset}  ${host.sha256}`);
