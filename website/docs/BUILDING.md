# Building and running

## Requirements

* Rust (stable) and a C/C++ toolchain: MSVC Build Tools on Windows, Xcode command line tools on macOS,
  `build-essential` plus X11/Wayland development headers on Linux.
* Node.js 18+ (the CLI and the TypeScript compiler). Bun or npm both work for installing.
* A GPU with a Vulkan, Metal or DirectX 12 driver. The runtime picks the adapter itself;
  `rendrix doctor` prints which one.

Only Windows has been exercised so far (see [COMPATIBILITY.md](COMPATIBILITY.md)).

## Build the runtime

```sh
cd engine
cargo test --workspace                      # ~160 Rust tests (GPU tests skip themselves with no adapter)
cargo build --release -p rendrix-runtime    # engine/target/release/rendrix-runtime[.exe]
```

The CLI finds the runtime through `RENDRIX_RUNTIME`, then `engine/target/{release,debug}`, then
`$RENDRIX_HOME/runtime/<platform>-<arch>/`.

## Create and run an application

```sh
npm install                                  # once, at the repository root (installs TypeScript)
node packages/cli/bin/rendrix.mjs init my-app
cd my-app
rendrix dev        # builds, opens a native window, rebuilds and restarts on every change
rendrix build      # type-check + compile to dist/my-app.rdxapp
rendrix run        # build and run once
rendrix check      # type-check only
rendrix package    # dist/my-app.exe: one standalone executable, no Node/Bun/browser needed
rendrix doctor     # Node, TypeScript, runtime, GPU adapter, project config
```

A project is `rendrix.json` (name, window size/title, permissions), an `index.html` (with `<link
rel="stylesheet">` and `<script type="module" src="…ts|tsx">`), `src/**`, and any assets. `tsconfig.json`
selects `"jsx": "react-jsx"` and `"jsxImportSource": "rendrix"`.

`rendrix dev` rebuilds and **restarts** the app on change; it does not preserve in-memory state. Hot state
preservation is not implemented.

## Verifying a build

```sh
npm test                 # cargo test --workspace + the CLI test suite (runs bundles in the real runtime)
npm run e2e              # builds examples/app-counter, replays scripted clicks/resizes headlessly on the GPU,
                         # fails if any expectation breaks, writes screenshots to .rendrix-shots/
npm run bench            # frame-pipeline timings on a 1,000-item list (release build)
```

Runtime flags useful for debugging: `--headless --screenshot out.png`, `--script steps.json`
(steps: `click`, `move`, `type`, `key`, `wheel`, `resize`, `wait`, `settle`, `eval`, `expect`, `screenshot`),
`--size WxH`, `--scale S`. Environment: `RENDRIX_DUMP_FRAME=file.png` writes the frame the window presents,
`RENDRIX_TRACE_EVENTS=1` logs window events.

## Reproducibility

`engine/Cargo.lock` is committed; Rust dependencies resolve from it. The JavaScript side uses the
TypeScript compiler version from `package.json`. Release profile: thin LTO, `opt-level = 3`.
