# TSGPUI

### Build desktop apps with HTML, CSS, and TypeScript. Ship them as native executables.

TSGPUI is an **experimental, source-first framework prototype**. It turns a supported subset of HTML and CSS into a [GPUI](https://github.com/zed-industries/zed) interface, then runs TypeScript actions inside embedded QuickJS. Choose the native renderer for a Rust-rendered UI, or the WebView renderer for browser-style HTML and CSS.

> **Early-stage software:** this is not a production-ready GUI SDK. Native builds and packaging still need broader validation; see [current limitations](#known-limitations).

<p align="center">
  <img src="https://img.shields.io/badge/status-experimental-orange" alt="Experimental" />
  <img src="https://img.shields.io/badge/UI-GPUI%20%7C%20WebView-6f42c1" alt="GPUI and WebView renderers" />
  <img src="https://img.shields.io/badge/logic-TypeScript%20%2B%20QuickJS-3178c6" alt="TypeScript and QuickJS" />
  <img src="https://img.shields.io/badge/license-MIT-green" alt="MIT license" />
</p>

## The idea

```text
HTML + CSS + TypeScript  →  .tgp app bundle  →  one desktop executable
```

The `.tgp` bundle contains the validated app document and transpiled backend. At runtime, the native host renders the UI and embeds QuickJS for actions—there is no separate JavaScript process. Bun or Node is needed for development and building, but not to run a packaged app.

The host is compiled once per OS, architecture, and renderer. Later builds compile the app bundle and append it to a copy of the matching host. The packager supports PE on Windows, ELF on Linux, and Mach-O on macOS. A host build is still required locally or through CI; this repository does not ship prebuilt hosts.

## Pick a renderer

| Renderer | What it does | Choose it for |
|---|---|---|
| **Native (default)** | Compiles the supported HTML/CSS subset into GPUI elements. | A native-rendered interface and native host controls. |
| **WebView** | Loads authored HTML and CSS in the system WebView through Wry. | Browser layout, broader CSS behavior, or motion. |

Set the renderer in your project's `tsgpui.config.json`:

```json
{ "renderer": "webview" }
```

Use `"native"` or omit the setting for the default. The WebView uses WebView2 on Windows, WKWebView on macOS, and WebKitGTK on Linux. See the [renderer and syntax reference](docs/SUPPORTED_SYNTAX.md) for platform requirements and behavior.

## Get started

### From this checkout

Requires Bun or Node 20+ for the developer CLI.

```sh
bun install
bun run test
bun run gpui:compile
```

This builds `examples/counter/dist/tsgpui-counter.tgp` without Rust. To create a Windows executable, use a compatible prebuilt host or build one locally:

**Use a prebuilt host:** build it with the [Windows host workflow](.github/workflows/windows-host.yml), or obtain a trusted compatible build and verify its provenance and SHA-256.

```powershell
$env:TSGPUI_HOST = 'C:\path\to\tsgpui-host.exe'
bun run gpui
```

**Build the host locally:** install Rust and MSVC build tools, then run:

```powershell
bun run gpui:host
bun run gpui
```

Both routes produce `examples/counter/dist/tsgpui-counter.exe`. The first GPUI host build is substantial; app-only changes do not rebuild Rust.

### Install the CLI

The CLI lives in [`packages/cli`](packages/cli). Its dependencies are bundled into one file.

```sh
bun add -g ./packages/cli   # from this checkout
# bun add -g tsgpui         # after publication to npm
```

Then create and build a project:

```sh
tsgpui init my-app
cd my-app
tsgpui host build           # once per machine; requires Rust
tsgpui build                # writes dist/my-app.exe on Windows
```

The host is stored under `~/.tsgpui/`; set `TSGPUI_HOME` to change that location. Build the WebView host with `tsgpui host build --renderer webview`. See the [CLI guide](packages/cli/README.md) for commands and options.

## A tiny app

```html
<button data-onclick="increment">Increase</button>
<span data-bind="count">0</span>
```

```ts
export default {
  state: { count: 0 },
  actions: {
    increment(api: { get(key: string): number; set(key: string, value: number): void }) {
      api.set('count', api.get('count') + 1);
    }
  }
};
```

The compiler connects the button to the action and the span to state. In the native renderer, the host runs the action in QuickJS, updates state, and renders the resulting GPUI element tree.

## Development loop

```sh
bun run gpui:dev
```

The sample's HTML, CSS, and TypeScript are watched and rebuilt. The running host reloads the new bundle in place, preserving native state or WebView session state. A failed build shows an in-window error panel while the last good build keeps running. Press `F12` or `Ctrl+Shift+I` to open the problems/inspector panel. This is fast reload, not state-preserving HMR.

To use the CLI from another project, add scripts like these:

```json
{
  "scripts": {
    "gpui": "tsgpui build",
    "gpui:dev": "tsgpui dev"
  }
}
```

Point `TSGPUI_HOST` to a compatible precompiled host when building without a local Rust toolchain.

## Repository map

| Path | What's inside |
|---|---|
| `packages/cli/` | Compiler, CLI, tests, and type definitions |
| `native/` | Rust host, GPUI rendering, and embedded QuickJS actions |
| `examples/counter/` | Complete HTML/CSS/TypeScript example |
| `docs/` | Architecture, syntax support, security, and limitations |
| `.github/workflows/` | Host and platform build workflows |

## Commands

| Command | Purpose |
|---|---|
| `bun run gpui:compile` | Compile the example bundle without Rust |
| `bun run gpui:host` | Build the native host once with Cargo |
| `bun run gpui` | Package one executable (requires a compatible host) |
| `bun run gpui:dev` | Watch, compile, and launch the example |
| `bun run test` | Run compiler and CLI tests |
| `bun run gpui:doctor` | Check host and tool availability |

## Known limitations

- This is an experimental prototype, and native hosts and packaged apps have not been exercised in this delivery environment. The Windows and macOS workflows need successful runs before their artifacts can be considered verified.
- Native rendering supports a documented HTML/CSS subset; it is not browser-compatible. WebView behavior follows the platform engine. See the [supported syntax reference](docs/SUPPORTED_SYNTAX.md).
- `backend.ts` is a single file with synchronous actions and state. Runtime imports are unsupported except `import type`; Bun and Node APIs are not available inside the packaged app.
- There is no rich text input, video, accessibility semantics, CSS transitions or animations, or code signing. GPUI 0.2.2 lacks element transform and animation primitives; the native renderer warns on `transition`, `animation`, and `transform`. Use WebView when motion is required.
- Mixed `px`/`fr` grid tracks are emulated; pure `fr` tracks remain equal-width. The built-in component library is small (`Sidebar`, `Toolbar`, `SegmentedControl`, `SearchField`, `ListRow`, and `Sheet`). Custom TTF/OTF fonts can be bundled.
- Appending a bundle can invalidate executable signatures. Package an unsigned host, then sign the finished artifact.
- No RAM or startup benchmarks have been performed. Do not assume results reported for other GPUI apps apply here.
- The CLI is a local workspace package, not yet published to npm. `bun add -g tsgpui` works once publication is available.

## Build a Windows EXE in GitHub Actions

Push this source to a GitHub repository, then run **Actions → Compile TSGPUI counter to Windows EXE → Run workflow**. The workflow installs dependencies, runs compiler tests, builds the GPUI host, and packages the counter executable on a GitHub-hosted Windows runner. Download the artifact after the workflow succeeds; no local Rust installation is needed. This workflow has not yet been executed, and upstream dependency or API changes may require fixes.

## License

This project is licensed under the [MIT License](LICENSE). GPUI and the Rust/JavaScript dependencies have their own licenses; review their obligations before commercial redistribution.
