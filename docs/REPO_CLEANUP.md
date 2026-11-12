# Repository cleanup inventory

> **Historical snapshot (2026-11-04).** File names and search results below use the pre-rename
> spellings (`tsgpui-*`, `TSGPUI-*`, `check-webview.mjs`) because they quote real paths as they
> existed then; they are not rebranded. Since this snapshot, the WebView renderer was removed
> and the product renamed to TS-GPU (see [REWRITE_RENDRIX.md](REWRITE_RENDRIX.md)); the
> `scripts/e2e-windows.mjs` and `scripts/webview-probe.mjs` candidates listed here were deleted
> with the rewrite, and the `webview.mjs` references are historical.

Snapshot: 2026-11-04. No files were deleted. These are candidates, not a claim that the list covers every generated file. A name search cannot rule out dynamic paths, external consumers, or unsaved work. `needs decision` means preserve until its owner decides; `keep` means the evidence shows a current purpose. Ignored build outputs are listed separately from tracked and untracked files.

The inventory used `git status --short --untracked-files=all`, `git ls-files examples`, `git status --short --ignored examples`, and `Get-ChildItem examples -Recurse -File`. Reference searches below used `rg -n -F` from the nested repo root, excluding this inventory and `e2e-report.json` itself; default ripgrep ignores ignored build directories and binary files. Each quoted name is a search term; absence of a result means no text match in that search scope, not proof of no possible use.

| Candidate and status | Reference search and evidence | Decision |
| --- | --- | --- |
| `e2e-report.json` (untracked, root) | `rg -n -F 'e2e-report.json' .` finds `scripts/e2e-windows.mjs`, which uses this as its default output. | needs decision: report may contain useful results. |
| `scripts/webview-probe.mjs` (untracked) | `rg -n -F 'webview-probe.mjs' .` finds its own usage string; no other text match. It is a runnable probe script. | needs decision |
| `examples/electron-sample/` files (untracked) | `rg -n -F 'electron-sample' .` finds its package name. Its `package.json` names `electron/main.js` and `scripts/build.mjs`; the build script copies `renderer` to `dist`. The sample README describes its purpose. | keep |
| `examples/notes/src/notes.ts` (untracked) | `rg -n -F 'notes.ts' .` finds `examples/notes/README.md`; `rg -n -F './notes' .` finds the import in `src/backend.ts`. | keep |
| `examples/notes/README.md` (untracked) | `rg -n -F 'examples/notes' .` finds the syntax reference and its own build instructions; it describes the example. | keep |
| `examples/counter/dist/tsgpui-counter.tgp` and `tsgpui-counter.exe` (ignored) | `rg -n -F -e 'tsgpui-counter.tgp' -e 'tsgpui-counter.exe' .` finds README and website references. | keep: documented outputs; may be in use. |
| `examples/counter/dist/tsgpui-counter-windows-x64.exe.sha256` (ignored) | `rg -n -F 'tsgpui-counter-windows-x64.exe.sha256' .` finds no text reference. | needs decision: checksum may belong to a retained build. |
| `examples/notes/dist/tsgpui-notes.exe` and `tsgpui-notes.tgp` (ignored) | `rg -n -F -e 'tsgpui-notes.exe' -e 'tsgpui-notes.tgp' .` finds no text reference. `examples/notes/tsgpui.config.json` names the app and its `dist` output directory. | needs decision: generated outputs may be in use. |
| `examples/electron-sample/dist/app.js`, `index.html`, `style.css` (ignored) | `rg -n -F -e 'renderer/app.js' -e 'renderer/index.html' -e 'renderer/style.css' .` finds similarly named paths in bridge tests; `examples/electron-sample/scripts/build.mjs` copies the `renderer` directory to `dist`. Individual names alone are ambiguous. | needs decision: generated sample output may be in use. |
| Tracked example source and configuration | `git ls-files examples` lists counter and notes source/configuration. `rg -n -F -e 'examples/counter' -e 'examples/notes' .` finds root scripts and documentation; each project's configuration points to its source files. | keep |
| `examples/*/node_modules/` (ignored) | `.gitignore` excludes `node_modules/`; package workspace dependencies are declared in root `package.json`. Filename searches cannot establish which installed files are needed. | needs decision |

## Outer folder

`C:\Users\spoil\OneDrive\Desktop\TSGPUI-full-source` contains `.claude/`, `.git/`, `TSGPUI/`, and `check-webview.mjs`. The latter is outside the nested repository. Its contents import `./packages/cli/src/webview.mjs` relative to the outer folder and inspect generated script text. `rg -n -F 'check-webview.mjs' .` in the nested repository found no text reference. **Needs decision**: it resembles an ad hoc check, but its owner and intended use are unknown. Nothing in the outer folder was changed.
