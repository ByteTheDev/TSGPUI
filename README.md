# Rendrix

### Cross-target builds

From the repository root, build a Rendrix project for a specific platform or all six targets:

```sh
bun run rendrix windows-x64 --project examples/counter
bun run rendrix linux-arm64 --project examples/counter
bun run rendrix all --project examples/counter
```

The targets are `windows-x64`, `windows-arm64`, `macos-x64`, `macos-arm64`, `linux-x64`, and `linux-arm64` (the `win32` and `darwin` spellings also work). Prebuilt hosts are published for every target except `macos-x64`; that target needs a locally built host. You can also use `build --target windows-x64,linux-arm64`. Explicit target builds include the target in their output names in `dist`. Cross-target packaging requires a verified prebuilt host for each target; missing hosts are downloaded when available. `all` reports failures and exits unsuccessfully if any target could not be built. macOS apps are unsigned. The [building guide](docs/BUILDING.md) covers targets, hosts, packaging and Electron conversion in detail.

Run `rendrix` or `rendrix run` in a Rendrix or Electron project directory to start it. `rendrix build` and the target shortcuts also detect Electron projects. Electron projects are converted into a new temporary directory; build output and the compatibility report are located there, and their paths are printed. Every Electron 33 export is classified as implemented, partial, or unsupported in the coverage ledger (`packages/cli/src/electron-compat/coverage.mjs`), and a test fails when the ledger is out of date. The conversion does not modify the source, although it may run the project's own build script if renderer assets are missing. Read the report before relying on the result, and import only projects you trust.

### Experimental desktop apps with HTML, CSS, and TypeScript

Rendrix is an **experimental, source-first framework prototype**. It turns a supported subset of HTML and CSS into a native interface rendered by the [Rendrix engine](docs/RENDERER_RENDRIX.md), then runs TypeScript actions inside embedded QuickJS.

> **Early-stage software:** this is not a production-ready GUI SDK. Rendrix is in **early beta** — feature coverage is limited and it is a long way from stable. The pre-rewrite renderer for webview users is preserved at branch `backup/webview-era` (tag `webview-era-checkpoint`, commit `0fc860b`).

<p align="center">
  <img src="https://img.shields.io/badge/status-experimental-orange" alt="Experimental" />
  <img src="https://img.shields.io/badge/renderer-Rendrix%20(beta)-6f42c1" alt="Rendrix renderer (beta)" />
  <img src="https://img.shields.io/badge/logic-TypeScript%20%2B%20QuickJS-3178c6" alt="TypeScript and QuickJS" />
  <img src="https://img.shields.io/badge/license-MIT-green" alt="MIT license" />
</p>

## The Rendrix rewrite (Unreleased)

**Rendrix renames from Rendrix; the WebView renderer is removed; Rendrix is the sole rendering engine** (owner-authorized decision of 2026-11-11, contract in [docs/REWRITE_RENDRIX.md](docs/REWRITE_RENDRIX.md)). What that means in practice:

- **One renderer.** The `renderer` key is `"rendrix"` (default). `"native"` selects the deprecated legacy host (`rendrix-legacy-host`) with a deprecation warning and is scheduled for removal at the Rendrix M1 gate. `"webview"` is a hard error with migration guidance.
- **Renamed interfaces.** Bundle magic `RENDRIX001`, home env var `RENDRIX_HOME`, CLI entry `packages/cli/bin/rendrix.mjs`, binaries `rendrix-host` / `rendrix-legacy-host` (deprecated); the webview host binary is deleted.
- **Preserved.** The Electron-compat runtime, the updater, the QuickJS backend + permissions model, and the smoke/benchmark scripts all keep working; they never depended on the WebView renderer.

## The idea

```text
HTML + CSS + TypeScript  →  .tgp app bundle  →  one desktop executable
```

The `.tgp` bundle contains the validated app document and transpiled backend. At runtime, the native host renders the UI and embeds QuickJS for actions—there is no separate JavaScript process. Bun or Node is needed for development and building, but not to run a packaged app.

The host is compiled once per OS and architecture. Later builds compile the app bundle and append it to a copy of the matching host. The packager supports PE on Windows, ELF on Linux, and Mach-O on macOS. The CLI can fetch a checksummed host from a configured release, or build one locally (`rendrix host fetch` / `rendrix doctor`); availability depends on published release artifacts.

## The renderer

Rendrix is the engine. It consumes the same compiled document IR (`package::Node` — a resolved style tree) and renders it with winit + wgpu + taffy + cosmic-text, with the Electron-compat runtime on top.

| Renderer key | Host | Status |
|---|---|---|
| `"rendrix"` (default) | `rendrix-host` | The engine. **Early beta:** feature coverage is limited — state-driven re-render, hover/active/focus, scrolling, images, text editing, accessibility, and animations are on the [M1–M3 ladder](docs/RENDERER_RENDRIX.md). |
| `"native"` | `rendrix-legacy-host` | Deprecated legacy host. Accepted with a warning; removed at the Rendrix M1 gate. |
| `"webview"` | — | **Removed.** Hard error. See [`backup/webview-era`](https://github.com/ByteTheDev/Rendrix/tree/backup/webview-era) for the pre-rewrite implementation. |

Set it in your project's `rendrix.config.json`:

```json
{ "renderer": "rendrix" }
```

Omit the key for the same result. See the [renderer and syntax reference](docs/SUPPORTED_SYNTAX.md).

## Get started

### From this checkout

Requires Bun or Node 20+ for the developer CLI.

```sh
bun install
bun run test
bun run rendrix:compile
```

This builds `examples/counter/dist/rendrix-counter.tgp` without Rust. To create a Windows executable, use a compatible prebuilt host or build one locally:

**Use a prebuilt host:** build it with the [Windows host workflow](.github/workflows/windows-host.yml), or obtain a trusted compatible build and verify its provenance and SHA-256.

```powershell
$env:RENDRIX_HOST = 'C:\path\to\rendrix-host.exe'
bun run rendrix
```

**Build the host locally:** install Rust and MSVC build tools, then run:

```powershell
bun run rendrix:host
bun run rendrix
```

Both routes produce `examples/counter/dist/rendrix-counter.exe`. The CLI can fetch a host if a compatible release artifact is available; app-only changes do not require rebuilding Rust.

### Install the CLI

The CLI lives in [`packages/cli`](packages/cli).

```sh
bun add -g ./packages/cli   # from this checkout
```

Installer scripts for release archives are provided (see the [CLI guide](packages/cli/README.md#install)); their use requires a suitable published archive:

```sh
curl -fsSL https://raw.githubusercontent.com/ByteTheDev/Rendrix/main/packages/cli/install/install.sh | sh    # macOS, Linux
irm https://raw.githubusercontent.com/ByteTheDev/Rendrix/main/packages/cli/install/install.ps1 | iex        # Windows
```

Then create and build a project:

```sh
rendrix init my-app
cd my-app
rendrix host fetch           # optional: the first dev/build does this; no Rust needed
rendrix host build           # alternative: compile from source; requires Rust
rendrix build                # writes dist/my-app.exe on Windows
```

The host is stored under `~/.rendrix/`; set `RENDRIX_HOME` to change that location. See the [CLI guide](packages/cli/README.md) for commands and options.

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

The compiler connects the button to the action and the span to state. The host runs the action in QuickJS, updates state, and renders the resulting element tree.

## Development loop

```sh
bun run rendrix:dev
```

The sample's HTML, CSS, and TypeScript are watched and rebuilt. The running host reloads the new bundle in place, preserving state (the Rendrix host follows the same merge model as it matures; the deprecated legacy host merges user-changed values into the new script). A failed build shows an in-window error panel while the last good build keeps running. Press `F12` or `Ctrl+Shift+I` to open the problems/inspector panel. This is fast reload, not state-preserving HMR.

To use the CLI from another project, add scripts like these:

```json
{
  "scripts": {
    "rendrix": "rendrix build",
    "rendrix:dev": "rendrix dev"
  }
}
```

Point `RENDRIX_HOST` to a compatible precompiled host when building without a local Rust toolchain.

## Repository map

| Path | What's inside |
|---|---|
| `packages/cli/` | Compiler, CLI, tests, and type definitions |
| `native/` | Rust hosts (`rendrix-host`, deprecated `rendrix-legacy-host`), the Rendrix engine (`native/src/gpu/`), and embedded QuickJS actions |
| `examples/counter/` | Complete HTML/CSS/TypeScript example |
| `docs/` | Architecture, syntax support, security, and limitations |
| `.github/workflows/` | Host and platform build workflows |

## Commands

| Command | Purpose |
|---|---|
| `bun run rendrix:compile` | Compile the example bundle without Rust |
| `bun run rendrix:host` | Build the native host once with Cargo |
| `bun run rendrix` | Package one executable (requires a compatible host) |
| `bun run rendrix:dev` | Watch, compile, and launch the example |
| `bun run test` | Run compiler and CLI tests |
| `bun run rendrix:doctor` | Check host and tool availability |
| `bun run rendrix <target>` or `bun run rendrix all` | Build for one platform target, or for all six (`--project` selects the app) |

## Known limitations

- This is an experimental prototype. Rendrix is in early beta: the legacy renderer had broader verified coverage, and automated CLI tests do not establish GUI behavior for Rendrix yet. Platform release artifacts and workflows need independent verification.
- Rendrix renders a documented HTML/CSS subset; it is not browser-compatible, and its feature coverage is limited (see the [beta banner](docs/SUPPORTED_SYNTAX.md) and the [M1–M3 ladder](docs/RENDERER_RENDRIX.md)). The deprecated legacy host has its own documented subset in the meantime.
- Backends can import project-local TypeScript modules; package imports, dynamic imports and `require` are unsupported. Bun and Node APIs are not available inside the packaged app.
- Neither engine supports rich text input parity, video, CSS transitions, or animations today. Rendrix M3 plans accesskit accessibility and rAF-driven animations. The WebView renderer — the former motion path — was removed; consult `backup/webview-era` if you needed it.
- Fixed, automatic, and unequal `fr` grid tracks are emulated with proportional flex sizing. The built-in component library includes `Sidebar`, `Toolbar`, `SegmentedControl`, `SearchField`, `ListRow`, table layout components, `CodeEditor`, `EmptyState`, and `Sheet`. Custom TTF/OTF fonts can be bundled.
- Use a current host, package it unsigned, then sign the finished artifact. See the [release guide](docs/RELEASING.md) for platform signing and notarization.
- No RAM or startup benchmarks have been performed. Do not assume results reported for other GPU-rendered apps apply here.
- Electron import covers all Electron 33 exports: each is implemented, partial, or unsupported in the coverage ledger, and the compatibility shims (`require('electron')`, `fs`, `path`, `os`, `child_process`, `https`, `net`) run on the host backends. The renderer has one window and no page, so extra windows, page messages and `ipcRenderer` are unavailable (multi-window is on the Rendrix roadmap). Conversion writes a report listing each API the app uses that is partial or unsupported and runs the project's build script when no built renderer exists. Read the report and import only projects you trust. See the [supported syntax reference](docs/SUPPORTED_SYNTAX.md#renderer-selection), the [safety notes](docs/SAFETY.md), and the [changelog](CHANGELOG.md).

## Build a Windows EXE in GitHub Actions

Push this source to a GitHub repository, then run the repository's Windows packaging workflow (**Actions → Compile Rendrix counter to Windows EXE → Run workflow**; the workflow name follows the repository's workflows, owned by the tests+CI agent). The workflow installs dependencies, runs compiler tests, builds the host, and packages the counter executable on a GitHub-hosted Windows runner. Download the artifact after the workflow succeeds; no local Rust installation is needed. Workflow results and artifacts must be checked for the specific run; upstream dependency or API changes may require fixes.

## License

This project is licensed under the [MIT License](LICENSE). GPUI, Rendrix, and the Rust/JavaScript dependencies have their own licenses; review their obligations before commercial redistribution.
