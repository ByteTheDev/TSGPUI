import ts from 'typescript';
import path from 'node:path';
import { readFile, stat } from 'node:fs/promises';

/** Compile a single-file TypeScript backend to CommonJS and bind it to the embedded runtime. */
export function compileBackend(source, filename = 'backend.ts') {
  const ast = ts.createSourceFile(filename, source, ts.ScriptTarget.ES2022, true, ts.ScriptKind.TS);
  const errors = ast.parseDiagnostics.map(d => ts.flattenDiagnosticMessageText(d.messageText, '\n'));
  function visit(node) {
    if (ts.isImportDeclaration(node) && !node.importClause?.isTypeOnly) {
      errors.push('Backend supports only `import type` in MVP (no Node/Bun module loader inside QuickJS).');
    }
    if (ts.isExportDeclaration(node) && node.moduleSpecifier) errors.push('Backend re-exports are not supported');
    if (ts.isCallExpression(node) && (node.expression.kind === ts.SyntaxKind.ImportKeyword ||
        (ts.isIdentifier(node.expression) && node.expression.text === 'require'))) {
      errors.push('Dynamic import / require are not supported in the embedded backend');
    }
    ts.forEachChild(node, visit);
  }
  visit(ast);
  if (!/\bexport\s+default\b/.test(source)) errors.push('backend.ts must export a default app object');
  if (errors.length) throw new Error([...new Set(errors)].join('\n'));
  const result = ts.transpileModule(source, {
    fileName:filename,
    reportDiagnostics:true,
    compilerOptions: {
      target:ts.ScriptTarget.ES2020,
      module:ts.ModuleKind.CommonJS,
      strict:true,
      removeComments:true,
      esModuleInterop:true,
    }
  });
  if (result.diagnostics?.length) throw new Error(result.diagnostics.map(d => ts.flattenDiagnosticMessageText(d.messageText, '\n')).join('\n'));
  // Evaluated in QuickJS; no filesystem / process / Node globals are installed.
  return `(function(){\n"use strict";\nconst module = { exports: {} };\nconst exports = module.exports;\n${result.outputText}\nconst __rendrix_export = module.exports.default ?? module.exports;\nif (!__rendrix_export || typeof __rendrix_export !== 'object' || !__rendrix_export.state || !__rendrix_export.actions) throw Error('Backend must export { state, actions }');\nif (typeof __rendrix_export.state !== 'object' || Array.isArray(__rendrix_export.state)) throw Error('Backend state must be a record');\nglobalThis.__rendrix_app = __rendrix_export;\n})();`;
}

/** Bundle project-local TypeScript modules for the embedded runtime. */
export async function bundleBackend(entry, projectRoot, assertInside) {
  const modules = new Map();
  async function visit(file) {
    if (modules.has(file)) return;
    await assertInside(projectRoot, file, 'Backend module');
    const source = await readFile(file, 'utf8');
    const ast = ts.createSourceFile(file, source, ts.ScriptTarget.ES2022, true, ts.ScriptKind.TS);
    const location = pos => { const {line, character} = ast.getLineAndCharacterOfPosition(pos); return `${path.relative(projectRoot,file)}:${line + 1}:${character + 1}`; };
    const errors = ast.parseDiagnostics.map(d => `${location(d.start ?? 0)}: ${ts.flattenDiagnosticMessageText(d.messageText, '\n')}`);
    const imports = new Map();
    function scan(node) {
      const specifier = (ts.isImportDeclaration(node) || ts.isExportDeclaration(node)) ? node.moduleSpecifier : ts.isCallExpression(node) && ts.isIdentifier(node.expression) && node.expression.text === 'require' ? node.arguments[0] : undefined;
      if (specifier && ts.isStringLiteralLike(specifier) && /\.node$/i.test(specifier.text)) errors.push(`${location(specifier.getStart(ast))}: Native addon '${specifier.text}' is not supported`);
      if (ts.isImportDeclaration(node) && !node.importClause?.isTypeOnly) {
        if (!node.importClause || !node.importClause.namedBindings || !ts.isNamedImports(node.importClause.namedBindings) || node.importClause.name || node.importClause.namedBindings.elements.some(e => !e.isTypeOnly)) imports.set(node.moduleSpecifier.text, node.moduleSpecifier.getStart(ast));
      }
      if (ts.isExportDeclaration(node) && node.moduleSpecifier && !node.isTypeOnly && (!node.exportClause || !ts.isNamedExports(node.exportClause) || node.exportClause.elements.some(e => !e.isTypeOnly))) imports.set(node.moduleSpecifier.text, node.moduleSpecifier.getStart(ast));
      if (ts.isImportEqualsDeclaration(node) || ts.isCallExpression(node) && (node.expression.kind === ts.SyntaxKind.ImportKeyword || ts.isIdentifier(node.expression) && node.expression.text === 'require')) errors.push(`${location(node.getStart(ast))}: Dynamic import / require are not supported in the embedded backend`);
      ts.forEachChild(node, scan);
    }
    scan(ast);
    if (file === entry && !ast.statements.some(node => ts.isExportAssignment(node) && !node.isExportEquals)) errors.push(`${location(0)}: backend.ts must export a default app object`);
    if (errors.length) throw new Error(errors.join('\n'));
    const mappings = {};
    modules.set(file, {source, mappings});
    for (const [specifier, pos] of imports) {
      if (!specifier.startsWith('./') && !specifier.startsWith('../')) throw new Error(`${location(pos)}: Backend runtime imports must be relative project-local TypeScript modules: ${specifier}`);
      const base = path.resolve(path.dirname(file), specifier);
      if (base !== projectRoot && !base.startsWith(projectRoot + path.sep)) throw new Error(`${location(pos)}: Backend module path outside project: ${specifier}`);
      const candidates = path.extname(base) ? [base, ...(base.endsWith('.js') ? [base.slice(0,-3)+'.ts'] : [])] : [base+'.ts', path.join(base,'index.ts')];
      let resolved;
      for (const candidate of candidates) {
        await assertInside(projectRoot, candidate, 'Backend module');
        if (candidate.endsWith('.ts') && !candidate.endsWith('.d.ts') && (await stat(candidate).catch(() => null))?.isFile()) { resolved = candidate; break; }
      }
      if (!resolved) { const error = new Error(`${location(pos)}: Cannot resolve backend module '${specifier}'`); error.watchPaths = [...modules.keys(), ...candidates]; throw error; }
      mappings[specifier] = resolved;
      await visit(resolved);
    }
  }
  await visit(entry);
  const ids = new Map([...modules.keys()].map((file, i) => [file, i]));
  const definitions = [...modules].map(([file, {source, mappings}]) => {
    // Preserve the exact module.exports value for imported CommonJS modules, including functions.
    // The internal loader already provides CommonJS modules; interop wrapping changes require()
    // into a namespace object and breaks callable exports in imported Electron applications.
    const result = ts.transpileModule(source, {fileName:file, reportDiagnostics:true, compilerOptions:{target:ts.ScriptTarget.ES2020, module:ts.ModuleKind.CommonJS, strict:true, removeComments:true, esModuleInterop:false}});
    if (result.diagnostics?.length) throw new Error(`${path.relative(projectRoot,file)}: ${result.diagnostics.map(d => ts.flattenDiagnosticMessageText(d.messageText, '\n')).join('\n')}`);
    return `${ids.get(file)}:[function(module,exports,require){\n${result.outputText}\n},${JSON.stringify(Object.fromEntries(Object.entries(mappings).map(([k,v]) => [k,ids.get(v)])))}]`;
  });
  const js = `(function(){"use strict";const modules={${definitions.join(',')}};const cache={};function load(id){if(cache[id])return cache[id].exports;const module=cache[id]={exports:{}};const [fn,map]=modules[id];fn(module,module.exports,function(name){if(!Object.prototype.hasOwnProperty.call(map,name))throw Error('Unknown backend module: '+name);return load(map[name]);});return module.exports;}const exported=load(${ids.get(entry)});const app=exported.default;if(!app||typeof app!=='object'||!app.state||!app.actions)throw Error('Backend must export { state, actions }');if(typeof app.state!=='object'||Array.isArray(app.state))throw Error('Backend state must be a record');globalThis.__rendrix_app=app;})();`;
  return {js, files:[...modules.keys()]};
}
