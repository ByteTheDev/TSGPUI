// Track 8 benchmark: measures launch-to-window latency and idle resident memory for a
// packaged TSGPUI app by launching it N times and taking min/median/max.
//
// Usage: node scripts/benchmark.mjs <exe> [runs=5] [report.json]
// The numbers are per-machine: record the host CPU, RAM, OS build and the package name
// alongside the report (see docs/BENCHMARKS.md for the methodology and how to publish).
import { execFileSync } from 'node:child_process';
import { writeFileSync, readFileSync, mkdtempSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';

const [exe, runsArg = '5', out = 'benchmark-report.json'] = process.argv.slice(2);
if (!exe) {
  console.error('Usage: node scripts/benchmark.mjs <exe> [runs=5] [report.json]');
  process.exit(2);
}
const runs = Math.max(1, Math.min(50, parseInt(runsArg, 10) || 5));

const script = fileURLToPath(new URL('./smoke-packaged.ps1', import.meta.url));
// The report travels through a file, not stdout, so non-ASCII window titles survive.
const raw = mkdtempSync(join(tmpdir(), 'tsgpui-bench-'));
let report;
try {
  execFileSync('powershell.exe', [
    '-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', script,
    '-ExePath', exe, '-Runs', String(runs), '-WaitSeconds', '4',
    '-ReportPath', join(raw, 'report.json'),
  ], { encoding: 'utf8', timeout: 600000 });
  report = JSON.parse(readFileSync(join(raw, 'report.json'), 'utf8').replace(/^\uFEFF/, ''));
} catch (error) {
  try { report = JSON.parse(readFileSync(join(raw, 'report.json'), 'utf8').replace(/^\uFEFF/, '')); } catch { report = { exe, runs, error: String(error) }; }
}

report.methodology = {
  launcher: 'scripts/smoke-packaged.ps1 (Start-Process; window-visible latency is first poll where MainWindowHandle != 0, 50 ms polls)',
  idleRss: 'process.WorkingSet64 after the window has been idle for 4 seconds',
  runs,
  note: 'Numbers are per-machine. First run includes cold-start file-cache effects; the min is the warm number.',
};
const text = JSON.stringify(report, null, 2);
writeFileSync(out, text + '\n');
console.log(text);
if (report.error || !report.summary) process.exitCode = 1;
