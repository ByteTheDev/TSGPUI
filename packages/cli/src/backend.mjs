import ts from 'typescript';

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
  return `(function(){\n"use strict";\nconst module = { exports: {} };\nconst exports = module.exports;\n${result.outputText}\nconst __tsgpui_export = module.exports.default ?? module.exports;\nif (!__tsgpui_export || typeof __tsgpui_export !== 'object' || !__tsgpui_export.state || !__tsgpui_export.actions) throw Error('Backend must export { state, actions }');\nif (typeof __tsgpui_export.state !== 'object' || Array.isArray(__tsgpui_export.state)) throw Error('Backend state must be a record');\nglobalThis.__tsgpui_app = __tsgpui_export;\n})();`;
}
