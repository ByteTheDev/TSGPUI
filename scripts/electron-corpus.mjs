#!/usr/bin/env node
// Runs `tsgpu import electron` and then `tsgpu compile` on a corpus of open-source Electron apps and
// prints a Markdown table. Results are published in docs/ELECTRON_IMPORT_RESULTS.md.
//
//   node scripts/electron-corpus.mjs <work-dir> [--json results.json]
//
// <work-dir> receives shallow clones (clones/) and the converted projects (out/). Nothing from the apps is
// executed: the importer only reads files, and --build is never passed, so no install or build script runs.
import { spawnSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const here = path.dirname(fileURLToPath(import.meta.url));
const cli = path.resolve(here, '../packages/cli/bin/tsgpu.mjs');

// [repository, sub-directories to import (empty means the repository root)]
export const CORPUS = [
  ['electron/electron-quick-start', ['']],
  ['electron/electron-api-demos', ['']],
  ['hokein/electron-sample-apps', ['helloworld', 'menus', 'mini-code-editor', 'notifications', 'tray', 'cookies', 'file-explorer', 'frameless-window', 'power-save-blocker', 'desktop-capture']],
  ['electron/simple-samples', ['activity-monitor', 'electron-tray', 'hash', 'mirror', 'prices', 'url']],
  // Larger apps whose repositories hold only sources: their entry points are build outputs.
  ['sindresorhus/caprine', ['']],
  ['minbrowser/min', ['']],
  ['webtorrent/webtorrent-desktop', ['']],
  ['marktext/marktext', ['']],
  ['Automattic/simplenote-electron', ['']],
];

const run = (command, args, options = {}) => spawnSync(command, args, { encoding: 'utf8', timeout: 300000, ...options });
const first = text => (text ?? '').trim().split('\n')[0].replace(/^tsgpu: /, '');

function main() {
  const argv = process.argv.slice(2);
  const workDir = argv.find(arg => !arg.startsWith('--'));
  if (!workDir) { console.error('usage: node scripts/electron-corpus.mjs <work-dir> [--json FILE]'); process.exit(2); }
  const jsonFile = argv.includes('--json') ? argv[argv.indexOf('--json') + 1] : null;
  const clones = path.resolve(workDir, 'clones'), outRoot = path.resolve(workDir, 'out');
  fs.mkdirSync(clones, { recursive: true });
  fs.mkdirSync(outRoot, { recursive: true });

  const rows = [];
  for (const [repo, dirs] of CORPUS) {
    const checkout = path.join(clones, repo.replace('/', '__'));
    if (!fs.existsSync(checkout)) {
      const clone = run('git', ['clone', '--depth', '1', '--quiet', `https://github.com/${repo}.git`, checkout]);
      if (clone.status !== 0) { rows.push({ repo, dir: '', import: 'clone failed', detail: first(clone.stderr) }); continue; }
    }
    const commit = run('git', ['-C', checkout, 'rev-parse', '--short', 'HEAD']).stdout.trim();
    for (const dir of dirs) {
      const input = path.join(checkout, dir), out = path.join(outRoot, repo.replace('/', '__'), dir || '_root');
      fs.rmSync(out, { recursive: true, force: true });
      const imported = run(process.execPath, [cli, 'import', 'electron', input, '--out', out]);
      const row = { repo, dir, commit, import: imported.status === 0 ? 'ok' : 'failed', detail: imported.status === 0 ? '' : first(imported.stderr) };
      if (imported.status === 0) {
        const compiled = run(process.execPath, [cli, 'compile', '--project', out]);
        row.compile = compiled.status === 0 ? 'ok' : 'failed';
        if (compiled.status !== 0) row.detail = first(compiled.stderr);
        row.warnings = (compiled.stderr.match(/^Warning:/gm) ?? []).length;
        row.cssWarnings = (compiled.stderr.match(/^Warning: .*(?:CSS|selector)/gm) ?? []).length;
        const report = fs.readFileSync(path.join(out, 'TSGPU_IMPORT_REPORT.md'), 'utf8');
        row.scriptsRemoved = [...report.matchAll(/^- (?!node_modules\/)[^\n]*removed (\d+) <script>/gm)].reduce((sum, match) => sum + Number(match[1]), 0);
        row.unsupportedModules = [...new Set([...report.matchAll(/Unsupported module (\S+)/g)].map(match => match[1]))];
        row.reviewApis = [...new Set([...report.matchAll(/Review potentially unsupported API (\S+)/g)].map(match => match[1]))];
      }
      rows.push(row);
      console.error(`${repo}${dir ? '/' + dir : ''}: import ${row.import}${row.compile ? `, compile ${row.compile}` : ''}`);
    }
  }
  if (jsonFile) fs.writeFileSync(jsonFile, JSON.stringify(rows, null, 2));
  console.log('| App | Commit | Import | Compile | Renderer scripts removed | Needs |');
  console.log('| --- | --- | --- | --- | ---: | --- |');
  for (const row of rows) {
    const needs = row.import === 'ok' ? [...row.unsupportedModules.map(m => `module \`${m}\``), ...row.reviewApis.map(a => `\`${a}\``)].join(', ') : row.detail;
    console.log(`| ${row.repo}${row.dir ? '/' + row.dir : ''} | ${row.commit ?? ''} | ${row.import} | ${row.compile ?? '-'} | ${row.scriptsRemoved ?? '-'} | ${needs.replace(/\|/g, '\\|')} |`);
  }
}
main();
