// Packaged-app smoke test: launches a built TSGPUI executable, waits for its main window,
// captures a frame, and asserts the window rendered something (not a blank frame), stays
// responsive, and appeared within 30 seconds.
//
// Usage: node scripts/smoke-packaged.mjs <exe> [report.json] [screenshot.png]
// Windows only (the capture uses PrintWindow); other platforms are pending CI runners.
import { execFileSync } from 'node:child_process';
import { writeFileSync, statSync, readFileSync, mkdtempSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';

const [exe, out = 'smoke-report.json', screenshot = ''] = process.argv.slice(2);
if (!exe) {
  console.error('Usage: node scripts/smoke-packaged.mjs <exe> [report.json] [screenshot.png]');
  process.exit(2);
}

const script = fileURLToPath(new URL('./smoke-packaged.ps1', import.meta.url));
// The report travels through a file, not stdout: PowerShell's console encoding can mangle
// non-ASCII window titles into control characters that break JSON.parse.
const raw = mkdtempSync(join(tmpdir(), 'tsgpui-smoke-'));
let report;
try {
  const json = execFileSync('powershell.exe', [
    '-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', script, '-ExePath', exe,
    ...(screenshot ? ['-ScreenshotPath', screenshot] : []),
    '-ReportPath', join(raw, 'report.json'),
  ], { encoding: 'utf8', timeout: 180000 });
  report = JSON.parse(readFileSync(join(raw, 'report.json'), 'utf8').replace(/^\uFEFF/, ''));
} catch (error) {
  try { report = JSON.parse(readFileSync(join(raw, 'report.json'), 'utf8').replace(/^\uFEFF/, '')); } catch {
    const stdout = String(error.stdout || '');
    try { report = JSON.parse(stdout.slice(stdout.indexOf('{'))); } catch { report = { exe, runs: 1, error: String(error) }; }
  }
}

const run = report?.results?.[0];
report.assertions = {
  windowCreated: !!run,
  rendered: !!run && run.distinctColors >= 3,
  responsive: !!run && run.responding === true,
  dimensions: !!run && run.width > 0 && run.height > 0,
};
if (screenshot && run) {
  try { report.screenshotBytes = statSync(screenshot).size; } catch { report.screenshotBytes = 0; }
}
const text = JSON.stringify(report, null, 2);
writeFileSync(out, text + '\n');
console.log(text);
if (Object.values(report.assertions).some(value => !value) || report.error) process.exitCode = 1;
