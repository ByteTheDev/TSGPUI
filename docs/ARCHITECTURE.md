# Architecture and milestones

## Source pipeline

1. Parse a **strict HTML subset** into an element tree with explicit keys and attributes.
2. Parse CSS with PostCSS; reject unsupported syntax. Resolve rules with descendant matching, class/id selectors, inheritance and specificity.
3. Transpile single-file TypeScript backend actions with the TypeScript compiler (`CommonJS`, ES2020).
4. Emit UTF-8 JSON `.tgp` bundle: `{ format, name, title, width, height, root, backend_js }`.
5. Native GPUI host parses the `.tgp`, loads backend JS in embedded QuickJS, opens a GPU-accelerated GPUI window and renders the tree.
6. Clicks invoke JS actions synchronously; native view snapshots state and redraws.
7. Release packaging appends `[bundle][u64 LE length][TSGPUI01]` to a prebuilt host executable. Host reads trailer from its own executable; **no Node/Bun runtime is needed in the distributed app**.

## Build separation

Native host changes: `cargo build --release` (expensive, uncommon).
HTML/CSS/TS changes: `bun run gpui` (compile app JSON + copy host + append overlay, no Rust rebuild).
Development: `bun run gpui:dev` (recompile app, restart host; not full hot module replacement).

## Backend design

The backend is TypeScript translated to JS and run in one embedded QuickJS context with in-memory application state.
Actions execute in the GPUI UI thread. Only `get`, `set`, and `update` are currently exposed. Avoid long-running computations inside event handlers.
The runtime does not expose Bun or Node globals, `fetch`, disk access or network permissions.

**Important:** QuickJS execution limits currently set an approximate heap cap but do **not** impose hard CPU-time limits or malicious-script isolation. Only bundle trusted backend code.

## Versioned binary format

A packaged application consists of the native binary followed by JSON bytes, 8-byte little-endian payload size, then `TSGPUI01`. `MAX_PAYLOAD=16 MiB` protects against unintended oversized reads. This is a **prototype container format**, not a mature signed, encrypted, or tamper-proof distribution format.

## Next roadmap

1. Build and validate the GPUI Rust host on Windows using Actions; fix any platform-specific API drift.
2. Verify keyboard/mouse events and font availability under Windows.
3. Support image assets and native accessibility semantics.
4. Add a JS-native I/O bridge with per-app filesystem/network permissions and strict path validation.
5. Add input fields, scrolling, dynamic lists, layout primitives, drag/drop.
6. Implement browser-compatible CSS subsets or a design-system-style standard library; do not claim arbitrary HTML/CSS support.
7. Add incremental patch/HMR without restarting host, plus packaging signing and update support.
8. Publish TSGPUI CLI and trusted prebuilt GPUI host assets with checksums.

## Alternatives considered

- Rewriting GPUI in TypeScript: would abandon tested native renderer and demand a full GUI implementation.
- Transpiling arbitrary TypeScript to Rust: not tractable with broad JavaScript semantics.
- Embedding WebView: defeats the native-rendering requirement.
- Compiling Rust per developer change: excessively slow compared with a reusable host.

The current architecture keeps native rendering and efficient app-logic iteration, trading some backend-native API breadth for a smaller initial system.
