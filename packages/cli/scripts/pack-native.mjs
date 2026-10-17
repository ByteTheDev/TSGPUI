// Copies the Rust host sources into the package before `pack`, and removes the copy afterwards.
// The copy lives in packages/cli/native, which .gitignore excludes from the repository.
import {cp, rm} from 'node:fs/promises';
import path from 'node:path';
import {fileURLToPath} from 'node:url';

const cliRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const source = path.resolve(cliRoot, '../../native');
const target = path.join(cliRoot, 'native');
const mode = process.argv[2];

if (mode === 'copy') {
  await rm(target, {recursive: true, force: true});
  await cp(source, target, {recursive: true, filter: src => !src.split(path.sep).includes('target')});
  console.log(`Copied native host sources into ${target}`);
} else if (mode === 'clean') {
  await rm(target, {recursive: true, force: true});
} else {
  throw new Error('usage: pack-native.mjs copy|clean');
}
