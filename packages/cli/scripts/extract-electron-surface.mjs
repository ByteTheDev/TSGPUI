#!/usr/bin/env node
// Extracts the public Electron API surface from an electron.d.ts file, so TS-GPU's shims can be
// measured against the real API instead of a hand-kept list.
//
// Usage: node scripts/extract-electron-surface.mjs <path/to/electron.d.ts> [--out surface.json]
//
// Output: { source, exports: { <moduleExport>: { kind, members: [{ name, kind, static? }] } } }
// `exports` are the names a program can import from 'electron' (CrossProcessExports, Main and
// Renderer modules merged). Members are public methods, properties, accessors and events.
import ts from 'typescript';
import fs from 'node:fs';
import path from 'node:path';

const args = process.argv.slice(2);
const input = args.find(a => !a.startsWith('--'));
const outIndex = args.indexOf('--out');
const outFile = outIndex >= 0 ? args[outIndex + 1] : null;
if (!input) {
  console.error('Usage: extract-electron-surface.mjs <electron.d.ts> [--out surface.json]');
  process.exit(2);
}

const source = ts.createSourceFile(input, fs.readFileSync(input, 'utf8'), ts.ScriptTarget.Latest, true);

// Public members of a class or interface, skipping private and protected ones.
function memberNames(node) {
  const members = [];
  for (const m of node.members ?? []) {
    const mods = ts.canHaveModifiers(m) ? ts.getModifiers(m) ?? [] : [];
    if (mods.some(x => x.kind === ts.SyntaxKind.PrivateKeyword || x.kind === ts.SyntaxKind.ProtectedKeyword)) continue;
    if (!m.name) continue;
    const name = m.name.getText(source).replace(/^['"]|['"]$/g, '');
    if (ts.isMethodSignature(m) || ts.isMethodDeclaration(m)) members.push({ name, kind: 'method' });
    else if (ts.isPropertySignature(m) || ts.isPropertyDeclaration(m)) members.push({ name, kind: 'property' });
    else if (ts.isGetAccessor(m) || ts.isSetAccessor(m)) {
      if (!members.some(x => x.name === name)) members.push({ name, kind: 'property' });
    }
    else if (ts.isConstructorDeclaration(m)) continue;
  }
  // Events are declared as overloads of `on(event: 'x', ...)`; list the literal event names too.
  const events = new Set();
  for (const m of node.members ?? []) {
    if (!(ts.isMethodSignature(m) || ts.isMethodDeclaration(m))) continue;
    const name = m.name?.getText(source);
    if (!['on', 'once', 'addListener', 'off', 'removeListener'].includes(name)) continue;
    const first = m.parameters[0];
    if (first && first.type && ts.isLiteralTypeNode(first.type) && ts.isStringLiteral(first.type.literal)) events.add(first.type.literal.text);
  }
  for (const e of events) members.push({ name: e, kind: 'event' });
  return members;
}

// Collect every declaration inside `declare namespace Electron { ... }`, keyed by name.
const declarations = new Map();
function collectNamespace(node) {
  for (const stmt of node.body?.statements ?? []) {
    if (ts.isClassDeclaration(stmt) && stmt.name) {
      declarations.set(stmt.name.text, { kind: 'class', members: memberNames(stmt) });
    } else if (ts.isInterfaceDeclaration(stmt)) {
      const prior = declarations.get(stmt.name.text);
      const members = memberNames(stmt);
      declarations.set(stmt.name.text, { kind: prior?.kind ?? 'interface', members: [...(prior?.members ?? []), ...members] });
    } else if (ts.isVariableStatement(stmt)) {
      for (const d of stmt.declarationList.declarations) {
        const name = d.name.getText(source);
        // `const app: Electron.App` style exports: take members from the declared type when it is a named reference.
        declarations.set(name, { kind: 'value', members: [], typeName: d.type?.getText(source) ?? null });
      }
    } else if (ts.isFunctionDeclaration(stmt) && stmt.name) {
      declarations.set(stmt.name.text, { kind: 'function', members: [] });
    } else if (ts.isModuleDeclaration(stmt) && stmt.name) {
      declarations.set(stmt.name.getText(source), { kind: 'namespace', members: [] });
    }
  }
}
for (const stmt of source.statements) {
  if (ts.isModuleDeclaration(stmt) && stmt.name.getText(source) === 'Electron' && stmt.body) collectNamespace(stmt);
}

// The module surface is `namespace CrossProcessExports` (what `require('electron')` returns):
// `const app: App` and `class BaseWindow extends Electron.BaseWindow` declare values, and
// `type App = Electron.App` aliases point at the declarations collected above.
const exportsByName = {};
for (const stmt of source.statements) {
  if (!(ts.isModuleDeclaration(stmt) && stmt.name.getText(source) === 'Electron' && stmt.body)) continue;
  for (const inner of stmt.body.statements ?? []) {
    if (!(ts.isModuleDeclaration(inner) && inner.name.getText(source) === 'CrossProcessExports' && inner.body)) continue;
    const aliases = new Map();
    for (const s of inner.body.statements) {
      if (ts.isTypeAliasDeclaration(s)) aliases.set(s.name.text, s.type.getText(source).replace(/^Electron\./, '').split(/[<\[ ]/)[0]);
    }
    const resolve = ref => declarations.get(aliases.get(ref) ?? ref);
    for (const s of inner.body.statements) {
      if (ts.isVariableStatement(s)) {
        for (const d of s.declarationList.declarations) {
          const name = d.name.getText(source);
          const typeRef = d.type?.getText(source).replace(/^typeof /, '').split(/[<\[ ]/)[0] ?? '';
          const decl = resolve(typeRef) ?? { kind: 'value', members: [] };
          exportsByName[name] = { kind: decl.kind === 'interface' ? 'object' : decl.kind, type: aliases.get(typeRef) ?? typeRef, members: decl.members ?? [] };
        }
      } else if (ts.isClassDeclaration(s) && s.name) {
        const base = s.heritageClauses?.[0]?.types[0]?.expression.getText(source).replace(/^Electron\./, '');
        const decl = declarations.get(base) ?? { kind: 'class', members: [] };
        exportsByName[s.name.text] = { kind: 'class', type: base ?? null, members: decl.members ?? [] };
      } else if (ts.isFunctionDeclaration(s) && s.name) {
        exportsByName[s.name.text] = { kind: 'function', type: null, members: [] };
      }
    }
  }
}

const surface = { source: path.basename(input), exportCount: Object.keys(exportsByName).length, exports: exportsByName };
const json = JSON.stringify(surface, null, 2) + '\n';
if (outFile) fs.writeFileSync(outFile, json);
else process.stdout.write(json);
