# Rendrix

**If you can build it with TypeScript, you should be able to build it with Rendrix.**

Rendrix is a rendering engine and application runtime. Your TypeScript runs in an embedded JavaScript
engine (QuickJS), builds a real DOM, is styled by a real CSS engine, laid out with flexbox/grid/block/inline
layout and drawn directly on the GPU with wgpu. **No Chromium. No Electron. No WebView.** Rendrix owns the
runtime, the rendering pipeline and the application platform, and is not trying to be Electron or a browser.

> **Status: early.** The first end-to-end milestone works on Windows; most of the web platform is not there
> yet. [docs/COMPATIBILITY.md](docs/COMPATIBILITY.md) lists exactly what is supported, partial and missing,
> and only claims behaviour that has a test. macOS and Linux are unverified.

```tsx
import { render } from "rendrix/jsx-runtime";

let clicks = 0;
const label = (<p>No clicks yet.</p>) as HTMLParagraphElement;

render(
  <main>
    <h1>Hello, Rendrix</h1>
    {label}
    <button onClick={() => { label.textContent = `Clicked ${++clicks} times.`; }}>Click me</button>
  </main>,
  document.getElementById("root")!,
);
```

```text
TypeScript + CSS ──rendrix build──▶ ES modules ──▶ QuickJS ──▶ DOM ──▶ style ──▶ layout ──▶ display list ──▶ wgpu
                                                      ▲                                                       │
                                  click / key / wheel ┴──────── winit window, DOM events, default actions ◀────┘
```

## Try it

```sh
npm install
npm run runtime                       # cargo build --release -p rendrix-runtime  (needs Rust)
node packages/cli/bin/rendrix.mjs doctor
npm run example                       # examples/app-counter in a native window, rebuilding on change
npm run e2e                           # scripted headless run with screenshots in .rendrix-shots/
```

Create your own: `rendrix init my-app && cd my-app && rendrix dev`. Ship it: `rendrix package` produces one
standalone executable (≈14 MB on Windows) that needs no Node, Bun or browser.

CLI: `init`, `dev`, `build`, `run`, `check`, `package`, `doctor`. Details in [docs/BUILDING.md](docs/BUILDING.md).

## What is in the box

* **Runtime.** QuickJS behind a replaceable `JsEngine` trait, ES modules with top-level `await`, promises,
  timers, `requestAnimationFrame`, TypeScript and TSX compiled at build time.
* **DOM and events.** Node/Element/Document APIs, selectors, forms and text editing, capture/bubble events
  driven by real OS input, `MutationObserver`, `ResizeObserver`.
* **CSS.** Cascade, specificity, inheritance, `var()`, media queries, transitions, keyframes, transforms,
  gradients, shadows, flexbox, grid, positioning, overflow and scrolling.
* **Rendering.** SDF rounded-rect/border/shadow shader, glyph atlas, display list with culling, clipping and
  2D transforms, high-DPI. A 9,000-node page re-renders a hover in ~1 ms and a DOM mutation in ~8 ms
  ([measurements](docs/ENGINE_ASSESSMENT.md#6-milestone-1-results-2026-11-14)).
* **Web APIs.** `fetch`, `URL`, `localStorage`, `AbortController`, `TextEncoder`/`Decoder`, `matchMedia`,
  `structuredClone`, `crypto.getRandomValues`, images, clipboard.
* **Tooling.** Type-checked builds, headless scripted testing (`--headless --script`), frame benchmark.

Not yet: Canvas 2D/WebGL/WebGPU, WebSocket, Shadow DOM, tables and floats, accessibility, file dialogs,
multi-window, auto-update, framework adapters, macOS/Linux validation.

## Repository

| Path | |
|---|---|
| `engine/` | Rust workspace: `rendrix-css`, `-dom`, `-layout`, `-gfx`, `-core`, `-js`, `-runtime` |
| `packages/cli/` | The `rendrix` command (Node): build pipeline, packaging, templates, tests |
| `packages/types/` | TypeScript declarations and JSX types (`rendrix`, `rendrix/jsx-runtime`) |
| `examples/app-counter/` | The milestone application and its scripted end-to-end test |
| `docs/` | [Architecture](docs/ARCHITECTURE.md), [compatibility](docs/COMPATIBILITY.md), [building](docs/BUILDING.md), [safety](docs/SAFETY.md), [assessment and roadmap](docs/ENGINE_ASSESSMENT.md), [accessibility plan](docs/ACCESSIBILITY_PLAN.md) |

Earlier versions of this project compiled a static HTML subset to a native view tree and shipped an Electron
compatibility layer. That direction was replaced; the old code remains in git history
(tag `checkpoint/pre-engine-transformation`).

MIT licensed.
