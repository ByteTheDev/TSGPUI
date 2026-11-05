// Generates runtime/electron-coverage.ts from the coverage ledger. The generated module exports one
// named binding per unsupported Electron API. Using one throws a clear error that names the API.
import {COVERAGE} from './coverage.mjs';

const quote = value => JSON.stringify(value);

export function coverageSource() {
  const lines = [
    '// Generated from packages/cli/src/electron-compat/coverage.mjs. Do not edit by hand.',
    '',
    'const suffix = " (see TSGPUI_IMPORT_REPORT.md for alternatives)";',
    '',
    'function unsupportedModule(name: string, reason: string): any {',
    '  const fail = (what: string): never => { throw new Error(`electron.${name}${what} is not supported by TSGPUI: ${reason}${suffix}`); };',
    '  const target = function () {};',
    '  return new Proxy(target, {',
    '    get(_target, property) { if (property === "then" || typeof property === "symbol") return undefined; return fail(`.${String(property)}`); },',
    '    set(_target, property) { return fail(`.${String(property)}`); },',
    '    apply() { return fail(""); },',
    '    construct() { return fail(""); },',
    '  });',
    '}',
    '',
  ];
  for (const [name, reason] of Object.entries(COVERAGE.unsupported).sort(([a], [b]) => a.localeCompare(b))) {
    lines.push(`export const ${name} = unsupportedModule(${quote(name)}, ${quote(reason)});`);
  }
  return lines.join('\n') + '\n';
}
