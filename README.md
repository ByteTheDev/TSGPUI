# TSGPUI — HTML + CSS + TypeScript → Native GPUI

**An experimental, source-first framework prototype**, not a production-ready GUI SDK.

TSGPUI compiles a **supported subset** of HTML and CSS to a GPUI element tree, and transpiles TypeScript backend actions to JavaScript executed by **embedded QuickJS**. Bun or Node is needed for development; the native application is designed to run without either installed. Native rendering is handled by Rust GPUI, **not Chromium or a WebView**.

The Rust GPUI host is precompiled once. Subsequent `bun run gpui` commands rebuild only the app bundle and **append it to a copy of the native host** to produce a single executable. A host build is still required once per OS/architecture/GPUI version (either locally or via CI). This repository does **not** include a prebuilt Windows `.exe`.

## Quick start (from this ZIP's root)

Requires Bun or Node 20+ for the developer CLI.

```powershell
bun install
bun run test
bun run gpui:compile
```

That produces `examples/counter/dist/tsgpui-counter.tgp` (a validated app document plus transpiled backend). This step does **not** need Rust.

To create a native `.exe` on Windows, choose **one** of these paths:

### A. Prebuilt host — fastest developer workflow

Build the host using this repository's `.github/workflows/windows-host.yml` on GitHub Actions or obtain a trusted compatible build of this exact host. Check its provenance and SHA-256.

```powershell
$env:TSGPUI_HOST = 'C:\path\to\tsgpui-host.exe'
bun run gpui
```

Output: `examples/counter/dist/tsgpui-counter.exe`.

### B. Compile the host once locally

Install Rust/MSVC build tools **only if you choose local native compilation**:

```powershell
bun run gpui:host
bun run gpui
```

The first GPUI build is substantial. Later changes to HTML/CSS/TypeScript do not rebuild GPUI or Rust, unless you modify the host itself.

### Development

```powershell
bun run gpui:dev
```

The dev command watches the sample HTML/CSS/TS files, recompiles the `.tgp`, and restarts the host. This is a **fast reload**, not state-preserving HMR.

### In your own example project

Once published or installed locally, scripts can be:

```json
{
  "scripts": {
    "gpui": "tsgpui build",
    "gpui:dev": "tsgpui dev"
  }
}
```

Then `bun run gpui` (or `bun run GPUI`, both aliases are included) produces a native build if `TSGPUI_HOST` points to a precompiled host.

## Project layout

```text
packages/cli/      Bun/Node-compatible compiler, CLI, tests, types
native/            Rust GPUI host + embedded QuickJS actions
examples/counter/  Complete HTML/CSS/TypeScript example
docs/              Architecture, CSS subset, security, limitations
.github/workflows/ Windows GPUI host build through GitHub Actions
```

## Example

```html
<button data-onclick="increment">Increase</button>
<span data-bind="count">0</span>
```

```ts
export default {
  state: { count: 0 },
  actions: {
    increment(api: {get(key: string): number; set(key: string, value: number): void}) {
      api.set('count', api.get('count') + 1);
    }
  }
};
```

The button event is handled by JavaScript in QuickJS, with the new state rendered as native GPUI elements. No extra JavaScript **process** remains after packaging: the JS engine runs **inside** the one native executable.

## Scope and known limitations

- **HTML/CSS subset only**, not browser-standard compatibility. See [CSS reference](docs/SUPPORTED_SYNTAX.md).
- Native host has **not been compiled or exercised in this delivery environment** (no Rust compiler or Windows runner present). Its source is provided for a real build/test; do not treat it as a verified Windows executable.
- `backend.ts` supports **a single file**, synchronous actions and state; runtime imports are unsupported except `import type`. No Bun/Node backend APIs in the executable yet. Future versions can expose explicitly permissioned native APIs.
- No images, rich text inputs, video, accessibility semantics, high-level component library, CSS Grid, `@media`, CSS animations, fonts loading, or packaging code signing yet.
- Windows `.exe` overlay packaging works conceptually with PE files but **must be tested** on Windows. Appending bytes may invalidate signatures; do not patch signed executables.
- No actual RAM/startup benchmarks have been performed. Don't assume the ~50 MB observed for other GPUI apps applies here.
- The CLI is currently a **local workspace package**, not a published `bun add tsgpui` dependency. Publishing to npm is required for `bun add -d tsgpui` to work on an ordinary machine.

## Useful commands

| Command | Purpose |
|---|---|
| `bun run gpui:compile` | Build the `.tgp` bundle without Rust |
| `bun run gpui:host` | Build native host once with Cargo |
| `bun run gpui` | Package one native executable (requires host) |
| `bun run gpui:dev` | Watch, compile and launch through host |
| `bun run test` | Run CLI/compiler tests |
| `bun run gpui:doctor` | Show host/tool availability |

## License

MIT. GPUI and the Rust/JS dependencies have their own licenses; audit dependency obligations before commercial redistribution.

## Build a Windows EXE without installing Rust locally

Push the ZIP's source into a GitHub repository and run **Actions → Compile TSGPUI counter to Windows EXE → Run workflow**. The workflow installs dependencies on the GitHub-hosted Windows runner, runs the compiler tests, builds GPUI and packages the counter executable. Download the Windows artifact when the workflow succeeds. No Rust toolchain is required on your own PC. **This workflow has not yet been executed**, and upstream GPUI dependencies or API changes may need fixes first.
