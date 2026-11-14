# Electron importer: results on real open-source apps

Run on 2026-11-12 against TS-GPU 0.4.0 (Electron 33 surface), branch `dev`.
Reproduce with `node scripts/electron-corpus.mjs <work-dir>` (clones shallow, imports, compiles, prints the table).

**Method.** 23 import targets from 9 public repositories. For each: `tsgpu import electron <dir>`, then
`tsgpu compile` on the result. Nothing from these apps was executed: the importer only reads files, and
`--build` was never passed, so no `npm install` or third-party build script ran. **No converted app was
launched**, so "compiles" below means the bundle was produced, not that the app works.

## Headline

| | Before this pass | After |
| --- | ---: | ---: |
| Import and compile succeed | 1 of 23 | **14 of 23** |

The jump came from importer fixes made while running the corpus (below), not from app changes.
Imported does not mean working: in 13 of the 14, the renderer's `<script>` elements were removed (see item 1).

## Results

| App | Commit | Import | Compile | Renderer scripts removed | Needs |
| --- | --- | --- | --- | ---: | --- |
| electron/electron-quick-start | afcc34c | ok | ok | 1 | |
| electron/electron-api-demos | 26b3d1d | ok | ok | 2 | modules `update-electron-app`, `electron-log`, `glob`, `devtron` |
| hokein/electron-sample-apps/helloworld | cdaa96c | ok | ok | 0 | |
| hokein/electron-sample-apps/menus | cdaa96c | ok | ok | 1 | |
| hokein/electron-sample-apps/mini-code-editor | cdaa96c | ok | ok | 269 | |
| hokein/electron-sample-apps/notifications | cdaa96c | ok | ok | 1 | |
| hokein/electron-sample-apps/cookies | cdaa96c | ok | ok | 1 | |
| hokein/electron-sample-apps/file-explorer | cdaa96c | ok | ok | 4 | |
| hokein/electron-sample-apps/frameless-window | cdaa96c | ok | ok | 2 | |
| hokein/electron-sample-apps/desktop-capture | cdaa96c | ok | ok | 3 | |
| electron/simple-samples/activity-monitor | 3e4372b | ok | ok | 5 | module `url` |
| electron/simple-samples/hash | 3e4372b | ok | ok | 3 | module `url` |
| electron/simple-samples/mirror | 3e4372b | ok | ok | 3 | module `url` |
| electron/simple-samples/prices | 3e4372b | ok | ok | 3 | module `url` |
| hokein/electron-sample-apps/tray | cdaa96c | failed | | | no window or page at all (tray only) |
| hokein/electron-sample-apps/power-save-blocker | cdaa96c | failed | | | no page (hidden window only) |
| electron/simple-samples/electron-tray | 3e4372b | failed | | | only loads a remote site (https://electronjs.org) |
| electron/simple-samples/url | 3e4372b | failed | | | page URL comes from `process.argv` |
| sindresorhus/caprine | c3cbcf9 | failed | | | main entry is a build output (`dist-js`) |
| minbrowser/min | 7ba23e7 | failed | | | main entry is a build output (`main.build.js`) |
| marktext/marktext | f3b15b9 | failed | | | main entry is a build output (electron-vite) |
| webtorrent/webtorrent-desktop | 6b671e8 | failed | | | no built renderer (needs `--build`) |
| Automattic/simplenote-electron | 9d0cb34 | failed | | | no built renderer (needs `--build`) |

The five large apps were not built: that needs a full dependency install and running their build scripts,
which I did not do on this machine. They are untested, not proven unsupported.

## Importer fixes made during this run

- `loadURL('file://' + __dirname + '/index.html')`, template literals, `const` URLs and
  `url.format({protocol: 'file:', pathname})` now resolve to the local page (previously only a literal
  `file:` URL did). This unblocked 12 apps.
- When no `loadFile`/`loadURL` resolves, `index.html`, `src/`, `app/`, `renderer/` and `public/` are tried
  after `dist/`, `build/` and `out/`.
- An app that only loads `http(s)` pages now gets an explicit message (no web view exists), not a build hint.
- The "Review potentially unsupported API `app.*`" check was a stale hardcoded list. It now reads the
  runtime's `App` class, so `app.setName`, `app.setPath`, `app.requestSingleInstanceLock`, `app.once`,
  `app.quit` and the rest are no longer flagged. `app.setAppUserModelId` is still flagged, correctly.

## Shim and importer work still needed, by priority

1. **Renderer scripts are dropped (13 of 14 imported apps).** The converted UI is static; page logic must be
   rewritten into `backend.ts`. This, not any single API, is what stops an imported app from working. The
   only real fix is a way to run page behaviour (a DOM-lite runtime or a guided port), and it is the largest item.
2. **Node `url` module** (4 of 14): `url.format` / `url.parse` / `fileURLToPath` / `pathToFileURL`. Small, self-contained.
3. **The importer writes deprecated permission keys.** Every imported project prints six "legacy permission
   key" warnings at compile (84 warnings across 14 apps). Emit the operation-scoped keys
   (`filesystem.read`, `network.request`, `ipc.send`, ...), and update `--fix` and its tests with it.
4. **CSS skipped by the compiler** (each warned, none silent):
   - Vendor properties: `-webkit-app-region` (should map to `data-window-drag`), `-webkit-user-select`,
     `-moz-/-ms-user-select`, `-webkit-user-drag`, `*-text-size-adjust`. These should be ignored quietly or mapped.
   - Pseudo-elements and combinators: `::before`, `::after`, `:empty`, `:not()`, attribute selectors, `::-webkit-scrollbar*`.
   - Properties: `float`, `overflow-y`, `letter-spacing`, `outline`, `outline-offset`, `object-fit`, `zoom`,
     `list-style-*`, `animation`/`@keyframes`/`transform` (known, see NATIVE_MOTION.md), `white-space: pre-wrap`,
     `display: none | inline | inline-block`, and named colours (`gold`, `silver`).
5. **Windowless and tray-only apps** (2): the importer requires a page. Needs a placeholder page or a
   no-window mode.
6. **Third-party npm modules** (`glob`, `electron-log`, `update-electron-app`, `devtron`): reported as
   unsupported and replaced with a throwing stub. `electron-log` and `glob` are common enough to shim.
7. **Dynamic page URLs** (`process.argv`, computed paths): cannot be resolved statically; document a manual override.
8. **Ledger drift:** the `app` entry in `electron-compat/coverage.mjs` says there is no single-instance lock,
   but the runtime implements `requestSingleInstanceLock`. Re-verify each claim in that entry.
9. **Out of scope by design:** apps that wrap a remote website (no web view).

## Caveats

- Samples are small and old (some archived); they under-represent React/webpack/Vite apps, which is where
  the five unbuilt apps would show a different failure profile.
- Counts of warnings are per compile and include duplicates across files.
- Script counts exclude `node_modules`; `mini-code-editor` ships a vendored editor, hence 269.
