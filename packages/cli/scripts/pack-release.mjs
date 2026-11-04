// Builds the archive that install/install.sh and install/install.ps1 download.
// The archive is the npm package (bundle, templates, types, and Rust host sources) packed by
// `bun pm pack`, plus a SHA-256 checksum file. Upload both files to a GitHub release.
import {execFileSync} from 'node:child_process';
import {createHash} from 'node:crypto';
import {mkdir, readFile, rm, writeFile} from 'node:fs/promises';
import path from 'node:path';
import {fileURLToPath} from 'node:url';

const cliRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
// Outside dist/, because package.json publishes dist/ and the archive must not be packed inside itself.
const outDir = path.join(cliRoot, 'release');
const asset = 'tsgpui-cli.tgz';
const archive = path.join(outDir, asset);

await rm(outDir, {recursive: true, force: true});
await mkdir(outDir, {recursive: true});
// bun rejects --destination together with --filename, so the full output path goes in --filename.
execFileSync('bun', ['pm', 'pack', '--filename', archive], {cwd: cliRoot, stdio: 'inherit'});

const digest = createHash('sha256').update(await readFile(archive)).digest('hex');
await writeFile(`${archive}.sha256`, `${digest}  ${asset}\n`);
console.log(`Wrote ${archive}`);
console.log(`SHA-256 ${digest}`);
