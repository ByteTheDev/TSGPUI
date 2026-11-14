# Compatibility matrix

Rendrix is **not a browser** and does not claim browser compatibility. This page lists what is
implemented and covered by an automated test (✅), implemented with known gaps (🟡), and not
implemented (❌). Anything not listed is not supported. Tests live next to the code:
`engine/crates/*/src` (unit), `engine/crates/rendrix-runtime/tests` (input/frame integration, GPU pixels),
`packages/cli/test` (build pipeline, runs bundles in the real runtime).

Verified platform: **Windows 11, NVIDIA GeForce RTX 3060, Vulkan** (windowed at 125 % DPI and headless).
macOS and Linux share the same code (winit, wgpu, arboard) but have **not been built or run**.

## JavaScript and modules

| Feature | Status | Notes |
|---|---|---|
| ES2023 language, classes, private fields, `async`/`await`, generators | ✅ | Whatever QuickJS-ng provides; `BigInt`, `WeakRef`, `FinalizationRegistry`, `Object.groupBy`, `toSorted`, `Array.fromAsync` are present |
| ES modules, relative imports, top-level `await` | ✅ | Bare specifiers resolved at build time (ESM packages only) |
| Dynamic `import()` | 🟡 | Provided by QuickJS; the build rewrites specifiers, not covered by a test |
| CommonJS / `require` / Node built-ins | ❌ | Build error with an explanation |
| TypeScript, TSX | ✅ | Type-checked + transpiled at build time; the runtime executes JavaScript only |
| Promises, microtask order, `queueMicrotask`, unhandled-rejection events | ✅ | |
| `setTimeout`/`setInterval`/`requestAnimationFrame` | ✅ | Frame-driven rAF; timers on the platform clock |
| `Intl`, `WebAssembly`, Workers, `Temporal` | ❌ | Not defined (checked in the runtime); `toLocaleString` works without locale data |

## DOM

| Feature | Status | Notes |
|---|---|---|
| Node, Element, Text, Comment, Document, DocumentFragment, tree mutation, errors as `DOMException` | ✅ | |
| Attributes, `classList`, `dataset`, `style` (live), `innerHTML`/`outerHTML`, `insertAdjacent*`, `cloneNode` | ✅ | HTML parser is tolerant, not the full HTML5 tree builder (no foster parenting) |
| `querySelector(All)`, `matches`, `closest`, `getElementsBy*` | ✅ | Type/class/id/attribute selectors, all combinators, `:not :is :where :nth-* :first/last-child :empty :root :hover :active :focus :disabled :checked`. `:has()` ❌ |
| Events: capture/target/bubble, `once`, `passive`, `signal`, `on*` properties, `stopImmediatePropagation`, `CustomEvent` | ✅ | |
| Mouse/Pointer/Wheel/Keyboard/Input/Focus/Submit events from the OS | ✅ | Touch and gamepad input are ❌; pointer capture is a no-op |
| Form controls: `input` (text-like, checkbox, radio, button), `textarea`, `select`, `button`, `label`, `form` | 🟡 | Typing, caret, selection, clipboard, `input`/`change`, label and submit activation work. No undo, no validation UI, `select` has no popup list, file/date/range inputs ❌ |
| `getBoundingClientRect`, `offset*`, `client*`, `scroll*`, `scrollIntoView`, `elementFromPoint`, `getComputedStyle` | 🟡 | `getComputedStyle` implements a fixed list of properties (display, position, colour, size, margin/padding/border, font, opacity, overflow, z-index, transform, custom properties); others return `""` |
| `MutationObserver` | ✅ | childList, attributes (+filter, old value), characterData, subtree |
| `ResizeObserver` | 🟡 | One content-box size; delivered per frame |
| `IntersectionObserver`, Shadow DOM, custom elements, `Range`/`Selection`, `contenteditable`, drag and drop | ❌ | APIs are absent or throw `NotSupportedError` so feature detection works |

## CSS

| Feature | Status | Notes |
|---|---|---|
| Cascade: origin, specificity, `!important`, source order, inheritance, `inherit`/`initial`/`unset`, custom properties and `var()` with fallbacks | ✅ | |
| Media queries (`width`, `height`, `orientation`, `prefers-color-scheme`, `resolution`, `and`/`not`/`,`) | ✅ | Re-evaluated on resize |
| Box model, `box-sizing`, margin/padding/border, `border-radius` | ✅ | Per-side border colour and dashed/dotted styles render as one solid colour |
| `display`: block, inline, inline-block, flex, inline-flex, grid, none, contents | ✅ | `table*` and `list-item` are not implemented (lists get a marker run) |
| Flexbox (all properties) | ✅ | Taffy |
| Grid (tracks, `repeat`, `minmax`, `fr`, auto-fill/fit, line and span placement, gaps) | ✅ | `grid-template-areas` ❌ |
| Positioning: relative, absolute, fixed, z-index and stacking order | 🟡 | `sticky` behaves as relative; static position of auto-inset absolutes is the container origin |
| Overflow: hidden/clip/scroll/auto, wheel scrolling, nested scrollers | ✅ | Scrollbars are overlay indicators, not draggable |
| Floats, multi-column, tables, `writing-mode` | ❌ | |
| Text: font family/size/weight/style, line-height, letter-spacing, alignment, decoration, transform, `white-space`, wrapping | ✅ | Shaping is cosmic-text (bidi/complex scripts are not covered by tests). `text-overflow: ellipsis` parsed, not drawn; `text-shadow`, `word-break`, `line-clamp` ❌ |
| Colours (hex, `rgb`, `hsl`, named, alpha), gradients (linear/radial, ≤ 4 stops), `box-shadow` (outer + inset), `opacity`, `background-image: url()`, `object-fit` | 🟡 | Shadow blur is an analytic approximation; opacity applies per item, not as an offscreen group |
| 2D `transform`, `transform-origin` | ✅ | No 3D, no `perspective` |
| Transitions and `@keyframes` animations | ✅ | Animatable: opacity, colours, lengths, margin/padding, inset, transform, box-shadow, gap, flex-grow, font-size. Others switch discretely |
| `calc()` | 🟡 | Only when all operands are the same kind (px-only or %-only) |
| `@font-face` | 🟡 | Local bundled TTF/OTF/TTC only; WOFF/WOFF2 ❌ |
| `::before`/`::after` with string `content` | ✅ | `attr()`, counters, images ❌ |
| `filter`, `backdrop-filter`, `clip-path`, `mask`, `@container`, `@layer` ordering, nesting, `@import` | ❌ | `@supports`/`@layer` blocks are flattened |

## Web platform APIs

| API | Status | Notes |
|---|---|---|
| `URL`, `URLSearchParams` | ✅ | No IDNA/punycode |
| `TextEncoder`, `TextDecoder` | ✅ | UTF-8, latin1/ascii |
| `AbortController`/`AbortSignal` (`timeout`, `any`) | ✅ | |
| `fetch`, `Headers`, `Request`, `Response`, `Blob` | 🟡 | Relative URLs read bundled assets; `data:` URLs; `http(s)` only when `permissions.network` is true. No streaming bodies, `FormData`, cookies, CORS or caching |
| `localStorage`, `sessionStorage` | ✅ | Local storage is a per-user JSON file keyed by app name; no `storage` events |
| `crypto.getRandomValues`, `randomUUID` | ✅ | OS CSPRNG; `crypto.subtle` ❌ |
| `structuredClone`, `atob`/`btoa`, `performance.now`, `matchMedia`, `console` | ✅ | |
| `navigator.clipboard` | 🟡 | Text only; fed by Copy/Cut in text controls |
| Images via `<img>`, CSS backgrounds, `new Image()` with `load`/`error` | 🟡 | PNG is tested. JPEG, GIF (first frame) and WebP go through the same `image`-crate decoder but have no tests. No SVG, no animation |
| `location`, `history` | 🟡 | In-memory; there is no navigation |
| Canvas 2D, WebGL, WebGPU, `<video>`/`<audio>` | ❌ | `getContext()` returns `null` |
| WebSocket, XMLHttpRequest, Web Workers, Service Workers, IndexedDB, Notifications, Geolocation | ❌ | Not defined |
| `rendrix` module: `window`, `clipboard`, `quit`, `fs` | ✅ | `fs` is limited to directories listed in `permissions.fs_read`/`fs_write` |

## Application platform

| Capability | Status |
|---|---|
| Native window (winit), DPI scaling, resize, title, close | ✅ Windows (verified); macOS/Linux ❓ untested |
| Mouse, keyboard, wheel, text input, IME commit | ✅ Windows (IME composition itself untested) |
| System clipboard | ✅ via `arboard` (text) |
| Single standalone executable (`rendrix package`) | ✅ Windows (13.8 MB, no external runtime) |
| File dialogs, multi-window, tray, global shortcuts, auto-update, accessibility tree | ❌ planned ([ENGINE_ASSESSMENT.md](ENGINE_ASSESSMENT.md)) |
| Code signing / installers | ❌ (helper scripts from the previous tooling remain in `packages/cli/src`, unwired) |
