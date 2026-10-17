# Architecture and milestones

## Source pipeline

1. Parse the supported HTML subset into a keyed tree. Expand static components and collect image paths.
2. Parse CSS with PostCSS, reject unknown syntax, resolve selector specificity, and retain media rules for native resize handling.
3. Transpile the single-file TypeScript backend to JavaScript.
4. Emit a UTF-8 JSON `.tgp` bundle containing the tree, styles, embedded images, backend script, and explicit network host permissions.
5. The GPUI host loads the bundle, initializes embedded QuickJS, and renders the tree with native GPU-backed elements.
6. Controls dispatch actions and immutable event data to QuickJS. State changes redraw the native view.
7. Release packaging appends `[bundle][u64 LE length][TSGPUI01]` to a prebuilt host executable. The packaged app does not need Node or Bun at runtime.

## Build and development

Build the native host with `cargo build --release` when Rust host code changes. HTML, CSS, TypeScript, and image changes only rebuild the `.tgp` bundle.

`tsgpui dev` watches the configured source files and rewrites the bundle. The existing GPUI process polls the bundle and replaces the view when it changes, so the window stays open. The backend state resets on each reload; preserving state across edits is not implemented.

## Native controls and capabilities

The host implements buttons, text inputs, textareas, checkboxes, selects, form submission, numeric validation, keyboard shortcuts, Tab focus traversal, repeated state arrays, scrolling, CSS grid layout, and packaged images. Editable text is handled by a small key-driven control; IME composition, visible caret painting, and full native selection behavior are still future work. Selects cycle through options instead of opening a native dropdown.

The GPUI 0.2.2 API in this project does not expose an accessibility tree interface. HTML `aria-*` and `role` metadata is preserved, but screen-reader semantics are not wired to the native OS APIs yet.

Clipboard actions run only after a button click. File reads and writes require the native file picker/save dialog, operate on the user-selected path, and are limited to 1 MiB. Network actions are user-initiated HTTPS GETs to exact hosts listed in `permissions.network`; redirects are disabled, requests time out after 15 seconds, and responses are limited to 1 MiB. Backend JavaScript does not receive a general filesystem or `fetch` API.

## Backend design

The backend runs in one embedded QuickJS context with in-memory application state. Actions receive `get`, `set`, and `update`, plus a read-only event object. Promise-returning actions are supported for QuickJS microtasks; dispatch drains those jobs before redraw, so externally pending promises and long-running asynchronous work are not supported.

QuickJS memory is capped at 32 MiB. CPU time is not capped and the runtime is not a malicious-code sandbox. Only load backend code you trust.

## Versioned binary format

A packaged application consists of the native executable followed by JSON bytes, an 8-byte little-endian payload length, and `TSGPUI01`. `MAX_PAYLOAD=16 MiB` limits reads. This prototype container format is not signed, encrypted, or tamper-proof.

## Remaining roadmap

1. Run platform-level interaction and packaging checks on Windows, macOS, and Linux.
2. Replace the lightweight text controls with native text editing and a popup select component.
3. Connect role, label, and value metadata to a native accessibility tree when the supported GPUI API provides it.
4. Add list virtualization, exact mixed grid track sizing, and more CSS layout properties.
5. Add a nonblocking async backend bridge with cancellation and explicit per-operation permission grants.
6. Preserve app state across development reloads and add executable signing/update support.

## Alternatives considered

- Rewriting GPUI in TypeScript would abandon the existing native renderer and require a complete GUI toolkit.
- Transpiling arbitrary TypeScript to Rust is not practical with broad JavaScript semantics.
- Embedding a WebView would defeat the native rendering goal.
- Recompiling Rust for every developer edit is much slower than reusing one host executable.

The current design keeps the renderer native and keeps app iteration fast, with a small documented browser subset and explicit host capabilities.
