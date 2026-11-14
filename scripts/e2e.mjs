#!/usr/bin/env node
// End-to-end check of the first milestone: compile the TypeScript counter app, run it in the real
// runtime on the GPU (headless offscreen target), replay a scripted interaction through the same input
// path the window driver uses, and require every expectation to hold. Screenshots land in `.rendrix-shots/`.
import { spawnSync } from 'node:child_process';
import path from 'node:path';
import { mkdirSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { build } from '../packages/cli/src/app/commands.mjs';
import { findRuntime } from '../packages/cli/src/app/commands.mjs';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const example = path.join(root, 'examples', 'app-counter');
const shots = path.join(root, '.rendrix-shots');
mkdirSync(shots, { recursive: true });

const runtime = findRuntime();
if (!runtime) { console.error('runtime not built: npm run runtime'); process.exit(2); }
const { out } = await build(example, { typecheck: true });
const r = spawnSync(runtime, ['run', out, '--headless', '--script', path.join(example, 'e2e.json'), '--screenshot', path.join(shots, 'e2e-final.png'), '--size', '560x520'], { stdio: 'inherit' });
process.exit(r.status ?? 1);
