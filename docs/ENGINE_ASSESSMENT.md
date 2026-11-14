# Rendrix engine transformation — Phase 0 assessment and roadmap

Date: 2026-11-13. Checkpoint tag: `checkpoint/pre-engine-transformation` (HEAD `8d4bd59`).
Direction change: Rendrix is a standalone rendering engine and application runtime for TypeScript
apps. It is **not** an Electron alternative, wrapper, or compatibility layer.

## 1. What exists today (audited, not assumed)

| Area | Location | What it really is | Verdict |
|---|---|---|---|
| Authoring model | `packages/cli/src/html.mjs`, `css.mjs`, `compiler.mjs` | Build-time (Node/Bun) parser of a *supported subset* of HTML + CSS (PostCSS) that **resolves styles statically** into a `package::Node` tree stored in a JSON `.rdx` bundle. | Conflicts with the new goal: no runtime DOM, no runtime cascade, no pseudo/selector engine at runtime. Kept as legacy only. |
| App logic | `packages/cli/src/backend.mjs`, `native/src/runtime.rs` | A TypeScript file exporting `{ state, actions }`, transpiled to CommonJS with `ts.transpileModule`, run in QuickJS. Actions mutate a state map; the UI is bound via `data-bind` attributes. **No DOM access exists from JS.** | Declarative-state demo, not a web-style runtime. QuickJS integration (`rquickjs 0.14`) and its limits/permission model are reusable ideas. |
| Renderer (M0) | `native/src/gpu/*` (`winit 0.30`, `wgpu 25`, `taffy 0.9`, `cosmic-text 0.14`) | Walks the static `Node` tree → taffy → rounded-rect SDF quads + CPU glyph atlas. Re-rasterizes the *entire atlas every frame*; full re-layout + repaint every vsync; no images, gradients, shadows, transforms, scrolling, text editing, or animation; `display:grid` is laid out as block; `onclick` only logs to stdout (QuickJS not wired). 9 backend tests pass. | Proves winit+wgpu+taffy+cosmic-text work on Windows; the shader approach (SDF rounded rect) is sound. Code is tied to `package::Node` and a static tree, so it is superseded rather than extended. |
| Legacy host | `native/src/main.rs`, `view/*` | GPUI 0.2.2 renderer (deprecated in the previous charter). | Conflicts with "Rendrix owns the pipeline". Slated for removal. |
| Electron compat | `packages/cli/src/electron-compat/**` (≈15k lines incl. a 10k-line API surface snapshot), `import-electron.mjs`, `native/src/{tray,hotkeys,capabilities,platform,image_store}.rs` partially, examples/electron-sample*, many docs | Shims emulating Electron 33's JS API on top of the host call protocol. | Directly contradicts the new direction (no Electron API compatibility). Removed after the new stack passes the milestone (§4). |
| Native platform | `native/src/platform.rs`, `tray.rs`, `hotkeys.rs`, `updater.rs` | Clipboard, dialogs (`rfd`), power blockers, tray, global shortcuts, updater — all real Windows/macOS/Linux code. | Reusable. Migrated into the new platform layer on demand (clipboard/dialogs first). |
| CLI | `packages/cli` | `init/dev/build/compile/host/doctor/preview/sign` and cross-target packaging that appends the bundle to a prebuilt host executable (PE/ELF/Mach-O). | Packaging, signing and host-fetching logic is valuable and kept; commands are retargeted to the new runtime. |
| Tests | `packages/cli/test/*.mjs` (node:test), Rust unit tests in `native` | Mostly CLI/compiler/Electron tests. | New engine gets its own Rust test suites per crate. |

### Dependencies on Electron / Chromium / WebView
* **WebView:** already removed (branch `backup/webview-era`); no `wry`/`tao`/`gtk` in the tree.
* **Chromium:** none linked. `electron-33.json` is a *snapshot of API names*, not code.
* **Electron:** compat shims + importer + docs + two example projects only (§ table). No engine
  code depends on them at compile time except `capabilities.rs`/`runtime.rs` host-call plumbing
  that the shims were built on.

### How TypeScript becomes executable code today
`backend.ts` → TypeScript compiler API (`transpileModule`, target ES2020, CommonJS) in the Node/Bun
CLI → string embedded in the `.rdx` JSON → `rquickjs` `Context::eval` at runtime. Module resolution is
a build-time CommonJS concatenation limited to relative `.ts` files. No ES modules at runtime, no
bare specifiers, no TSX, no async job pump beyond actions.

### Limitations that block the vision
1. No DOM and no JS ↔ UI object model; UI cannot be created or mutated at runtime.
2. CSS resolved at build time; no selector matching, cascade, inheritance, variables, media queries,
   transitions or animations at runtime.
3. Renderer is a repainted-every-frame M0 (no dirty tracking, per-frame atlas rebuild).
4. No event loop (timers, microtasks, rAF), no Web APIs.
5. Layout only maps a flat CSS subset to taffy; no inline formatting context (text runs inside
   inline elements), no scrolling/overflow, no grid.
6. Two hosts + Electron layer = three overlapping runtimes.

## 2. Target architecture

A Cargo workspace at `engine/`; every crate has one job and a narrow interface.

```
rendrix-css      CSS tokenizer/parser, selectors, specificity, values, cascade, computed style, media queries, keyframes
rendrix-dom      Arena DOM: nodes, attributes, tree ops, mutation records, selector queries, HTML parser, form state
rendrix-layout   LayoutEngine trait + Taffy backend (flex/grid/block + inline formatting contexts via a TextMeasurer trait)
rendrix-gfx      DisplayList, text shaping/atlas (cosmic-text), wgpu renderer (SDF shapes, glyphs, clips), offscreen capture
rendrix-core     Engine: DOM → style → layout box tree → display list; hit testing, focus, scrolling, transitions/animations
rendrix-js       JsEngine trait + QuickJS backend, ES module loader, event loop (tasks/microtasks/timers/rAF), DOM bindings, Web APIs
rendrix-runtime  Platform shell: winit window, input → DOM events, frame scheduler, app bundle loader; `rendrix-runtime` binary
```

Boundaries:
* **JS engine is replaceable:** `rendrix-js` exposes a `JsEngine` trait; only `quickjs.rs` names rquickjs.
  Web API surface is a JS prelude over a small native `__rendrix` binding table, so another engine
  needs only the trait implementation plus that table.
* **Layout engine is replaceable:** `LayoutEngine` consumes a neutral box tree (`LayoutNode` + `ComputedStyle`),
  not DOM types. Taffy is one implementation.
* **TypeScript:** compiled to ES-module JavaScript before execution (build step in the CLI using the
  TypeScript compiler API; the runtime never sees TS). The runtime loads ES modules from a bundle
  module map via QuickJS's module loader; bare specifiers are resolved by the CLI.
* **App format:** `rendrix-app/1` bundle (JSON: `html`, `modules`, `assets`, window config). The packager
  appends it to a prebuilt `rendrix-runtime` executable, reusing the existing PE/ELF/Mach-O trailer logic.

## 3. Roadmap and status (updated after Milestone 1)

| Phase | Scope | Status |
|---|---|---|
| 0 | Audit, target architecture, checkpoint | **Done** (this document; tag `checkpoint/pre-engine-transformation`) |
| 1 | Core runtime: JS execution, ES modules, TS compile, event loop | **Done for the milestone**: QuickJS behind a `JsEngine` trait, module loader, top-level await, timers/rAF/microtasks, unhandled-rejection reporting, build-time TS compile + type-check. Not done: runaway-script limits, workers, WebAssembly |
| 2 | Rendering foundation | **Done for the milestone**: wgpu SDF renderer (rounded rects, borders, gradients, shadows, clips, transforms, images, text), persistent glyph atlas, display list, culling, one-draw-call frames. Not done: dirty-rect presentation, offscreen compositing layers (group opacity), per-side border colours, `filter` |
| 3 | Web platform | **Partly done**: real DOM, CSS cascade/animations, events, forms, fetch/URL/storage/observers. See [COMPATIBILITY.md](COMPATIBILITY.md) for gaps (Canvas 2D, WebSocket, IntersectionObserver, Shadow DOM, tables, floats …) |
| 4 | Application platform | **Started**: winit window, DPI, input, IME commit, clipboard, standalone executables (Windows verified). Not done: file dialogs, multi-window, tray/shortcuts, touch, macOS/Linux validation |
| 5 | Developer ecosystem | **Started**: `init/dev/build/run/check/package/doctor`, TSX with a direct-DOM JSX runtime, project template, headless scripted testing. Not done: state-preserving hot reload, devtools/inspector, framework adapters (React/Solid/Vue need a DOM-host adapter each plus a real `react-dom`-style integration test), source maps |
| 6 | Production readiness | **Not started**: cross-platform CI results, memory soak tests, signed packaging, auto-update, accessibility |

### Next priorities (in order)

1. **Incremental box generation.** Style/structure changes rebuild the whole box tree (~5 ms / 9,000 nodes). Keep boxes per element and patch dirty subtrees.
2. **Accessibility tree** (AccessKit) — blocks real-world use.
3. **macOS and Linux validation** on real hardware; the CI workflow exists but has never run.
4. **Canvas 2D** (a `<canvas>` backed by the same renderer) and **IntersectionObserver**.
5. **Framework adapters**, starting with Solid (fine-grained DOM updates map directly onto this DOM) and a React reconciler.
6. **Hot reload with state preservation**, source maps and an in-app inspector.
7. Cold start: the first frame of the 1,000-item benchmark takes ~0.75 s (system font scan + shaping 3,000 paragraphs); cache the font database and shape lazily off-screen.

## 4. Migration: what was removed and where its job went

Removed in the Milestone 1 change set (everything is recoverable from the checkpoint tag
`checkpoint/pre-engine-transformation`):

| Removed | What it did | Replacement / disposition |
|---|---|---|
| `packages/cli/src/electron-compat/**`, `import-electron.mjs`, Electron examples, Electron docs | Emulated Electron 33's API and imported Electron projects | **Dropped on purpose** (no compatibility goal) |
| `native/` GPUI host (`main.rs`, `view/*`), M0 `gpu/*`, `package.rs` | The previous renderer(s) and static `Node` IR | `engine/` (DOM → CSS → layout → wgpu) |
| `packages/cli/src/{html,css,compiler,backend,library,pack,native,hosts,preview}.mjs`, old templates, `types/app.d.ts` | Build-time static HTML/CSS compiler, `{state, actions}` backend model, host fetching | `packages/cli/src/app/*` (TS build pipeline, bundle, packaging) |
| `native/src/platform.rs` (clipboard, dialogs, power blockers, external links) | Native services | Clipboard re-implemented via `arboard`; **dialogs, power blockers, external links still to port** (Phase 4) |
| `native/src/{tray,hotkeys}.rs` | Tray icon, global shortcuts | **Still to port** (Phase 4) |
| `native/src/updater.rs`, `packages/cli/src/signing.mjs` (kept), `installer/` (kept) | Update download/verify/install, signing helpers | Updater **still to port**; signing and NSIS helpers remain in the CLI, unwired |
| `native/src/capabilities.rs` permission model | Filesystem/network/process grants | Re-implemented smaller in `rendrix-js/host.rs` (network, fs read/write); process grants intentionally not carried over |
| `scripts/*` (smoke/benchmark/drive scripts), `.github/workflows/*` | Old host smoke tests and release automation | `scripts/e2e.mjs`, `engine` benchmark, `.github/workflows/ci.yml` |

The website under `website/` still carries the previous product copy and needs a rebrand pass; only its
documentation tabs were repointed.

## 5. Verification policy
Every milestone: `cargo test --workspace`, a release build, an actual rendered screenshot inspected by
eye and compared against layout-derived expectations, and the interactive window run. A feature is
"done" only when exercised by a test or the demo app; everything else is listed in `COMPATIBILITY.md`
as partial/unsupported.

## 6. Milestone 1 results (2026-11-14)

**The milestone application** (`examples/app-counter`, TSX + CSS + flexbox) runs through Rendrix's own stack:
native winit window → TypeScript-compiled ES modules in QuickJS → DOM created and mutated by script → CSS
cascade, variables, `:hover`/`:active`, transitions and a media query → flexbox layout → wgpu renderer.
Click handlers update state and the DOM and the updated interface is re-rendered. No Electron, Chromium or
WebView is linked or launched anywhere.

How it was verified (all on this machine: Windows 11, RTX 3060, Vulkan):

* `cargo test --workspace`: 38 CSS, 22 DOM, 8 layout, 12 graphics (real GPU pixel checks), 34 core,
  27 JS runtime, 4 bundle + 10 session integration tests; `node --test packages/cli/test`: 9 tests
  (including a bundle run in the real runtime).
* `npm run e2e`: 26 scripted steps (hover, three clicks, state and class assertions, a mid-transition colour
  sample, resize into the `max-width: 480px` media query) all pass; screenshots were inspected by eye.
* The real window was started at 125 % DPI and driven with window messages posted to its own handle; the
  presented frame (`RENDRIX_DUMP_FRAME`) showed the count at 3 with the hover state after three clicks.
* `rendrix package` produced a 13.8 MB standalone executable that renders the app with no other runtime.

Measured with `npm run bench` (release build, 1,000 list items = 9,009 DOM nodes, 1000×700 at 1×).
Median per frame; "total" is CPU pipeline plus GPU submission and completion, excluding vsync:

| Scenario | total ms | style | layout | paint | GPU |
|---|---|---|---|---|---|
| idle (nothing changed) | 1.1 | 0 | 0 | 0 | 0.4 |
| hover moves between items | 1.2 | 0.03 | 0 | 0 | 0.4 |
| one wheel notch | 1.2 | 0.02 | 0 | 0.14 | 0.3 |
| toggle a class on one item | 7.5–8.4 | 0.06 | 5.7–6.3 | 0.2 | 0.5 |
| change text of one item | 7.4–8.9 | 0.01 | 5.7–6.8 | 0.2 | 0.5 |
| add + remove 10 items | 7.3–8.0 | 0.03 | 5.4–6.0 | 0.2 | 0.5 |
| JS `classList.toggle` from script | 8.0–8.4 | 0.06 | 6.1–6.3 | 0.2 | 0.7 |
| viewport resize (full restyle + relayout) | 39 | 24.5 | 12.4 | 0.2 | 0.7 |

Interactive scenarios fit a 60 FPS budget (16.7 ms); 120 FPS (8.3 ms) is met for hover and scrolling and
sits at the limit for mutations, whose cost is dominated by whole-tree box generation (next priority).
Cold start for this list is ~0.6 s plus a ~0.75 s first frame. Numbers vary by several tenths of a
millisecond between runs; they were measured only on one machine.

Bugs the verification work found and fixed (kept here because they shaped tests): text drawn double-spaced
(glyph offset counted twice), listener loss when wrappers were garbage-collected, `MutationObserver` subtree
matching done after removal, stale styles after re-parenting, a `.ts` compile error for non-JSX files, a
predictable PRNG behind `crypto.getRandomValues`, and a debug path that rendered in the wrong colour format.
