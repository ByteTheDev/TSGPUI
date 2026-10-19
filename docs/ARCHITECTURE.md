# Architecture and milestones

## Source pipeline

The `renderer` setting selects one of two paths:

- `native` (default): parse the supported HTML subset into a keyed tree, expand static components, parse CSS with PostCSS, and render GPUI elements. Actions run in embedded QuickJS.
- `webview`: keep the original HTML and CSS, inline local assets, and render them with the operating system's WebView engine through Wry. The compiled backend script runs in that page, which also supports the basic TSGPUI data bindings.

Both paths transpile the single-file TypeScript backend and emit a UTF-8 JSON `.tgp` bundle. Release packaging appends `[bundle][u64 LE length][TSGPUI01]` to the matching prebuilt host executable. Packaged apps do not need Node or Bun at runtime.

## Build and development

Build a host when Rust host code changes. Use `tsgpui host build --renderer native` for GPUI or `tsgpui host build --renderer webview` for Wry. HTML, CSS, TypeScript, and asset changes only rebuild the `.tgp` bundle. Hosts are target-specific: Windows produces PE, Linux ELF, and macOS Mach-O. `tsgpui build` packages the matching host and creates a macOS `.app` directory bundle; it does not sign or notarize that bundle.

`tsgpui dev` watches the configured source files and rewrites the bundle. Both host processes reload the bundle in the existing window. The GPUI host preserves backend state across reloads; the WebView host currently resets page and backend state. Native `tsgpui preview` preserves state on reload; a WebView preview reloads the raw document and resets state.

## Native controls and capabilities

The host implements buttons, text inputs, textareas, checkboxes, selects, form submission, numeric validation, keyboard shortcuts, Tab focus traversal, repeated state arrays, scrolling, CSS grid layout, and packaged images. Single-line `<input>` fields register with GPUI's text input handler, so platform text input and IME composition go through the OS text services. They draw a blinking caret and selection highlight, support mouse click and drag selection, Shift+arrow, Home/End, word-wise navigation and deletion, Ctrl/Cmd+A/C/X/V/Z, Shift+Ctrl/Cmd+Z, Ctrl/Cmd+Y, and Backspace/Delete. Undo history is bounded and kept per field. Caret and selection are measured with the element's inherited text style. Double-click selects a word. Multi-line `<textarea>` fields share the same path: each logical line is shaped to the field width in GPUI's text system, and paint draws the selection, text, and caret from that layout, so positions match what is drawn. The field scrolls with the wheel and keeps the caret in view.

Application menus are declared in the document and installed through GPUI's application menu API. Menu actions and shortcuts dispatch through the backend action path. Context menus render as host popups; disabled entries work there. Native application menu items cannot currently be disabled, and checked menu items are not supported. Elements with `data-ondrop` receive user-dropped local file paths and names through GPUI's `ExternalPaths`; the host does not read file contents.

Lists marked `data-virtual` become GPUI uniform lists, so only the visible rows are built. A unified title bar is an app-drawn bar with `appears_transparent`. Its drag area and window buttons are reported to Windows as caption and caption-button hit regions, so no control can sit inside a drag area. An optional blurred backdrop uses GPUI's `Blurred` background, which Windows applies only on some systems. Selects cycle through options instead of opening a native dropdown.

The GPUI 0.2.2 API in this project does not expose an accessibility tree interface. HTML `aria-*` and `role` metadata is preserved, but screen-reader semantics are not wired to the native OS APIs yet. The WebView path uses the platform browser accessibility behavior.

Clipboard actions run only after a button click. File reads and writes require the native file picker/save dialog, operate on the user-selected path, and are limited to 1 MiB. Network actions are user-initiated HTTPS GETs to exact hosts listed in `permissions.network`; redirects are disabled, requests time out after 15 seconds, and responses are limited to 1 MiB. Backend JavaScript does not receive a general filesystem or `fetch` API.

## Backend design

The backend runs in one embedded QuickJS context with in-memory application state. Actions receive `get`, `set`, and `update`, plus a read-only event object. Promise-returning actions are supported for QuickJS microtasks; dispatch drains those jobs before redraw, so externally pending promises and long-running asynchronous work are not supported.

QuickJS memory is capped at 32 MiB. CPU time is not capped and the runtime is not a malicious-code sandbox. Only load backend code you trust.

## Versioned binary format

A packaged application consists of the native executable followed by JSON bytes, an 8-byte little-endian payload length, and `TSGPUI01`. `MAX_PAYLOAD=16 MiB` limits reads. This prototype container format is not signed, encrypted, or tamper-proof.

## Remaining roadmap

1. Run platform-level interaction and packaging checks on Windows, macOS, and Linux. The Windows interaction checks so far were done by hand with a screenshot harness and are not yet automated.
2. Connect role, label, and value metadata to a native accessibility tree when the supported GPUI API provides it.
3. Add exact mixed grid track sizing, CSS transitions, and more CSS layout properties.
4. Add a nonblocking async backend bridge with cancellation and explicit per-operation permission grants.
5. Add a native code editor or terminal component for apps that need one, and executable signing/update support.

## Alternatives considered

- Rewriting GPUI in TypeScript would abandon the existing native renderer and require a complete GUI toolkit.
- Transpiling arbitrary TypeScript to Rust is not practical with broad JavaScript semantics.
- A WebView cannot provide the native-rendering benefits of the GPUI path; it is available as an opt-in choice when browser fidelity matters more.
- Recompiling Rust for every developer edit is much slower than reusing one host executable.

The default design keeps rendering native and iteration fast, with a documented browser subset and explicit host capabilities. The optional WebView path trades native rendering for browser-level layout and CSS support.
