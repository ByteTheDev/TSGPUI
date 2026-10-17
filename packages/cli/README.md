# tsgpui

Compile HTML, CSS and TypeScript into a native [GPUI](https://www.gpui.rs) window. There is no browser engine. The app's backend runs in an embedded QuickJS.

## Install

Install once, globally, with Bun:

```sh
bun add -g @rs/tsgpui
```

Until the package is on npm, install from a checkout of the repository:

```sh
bun add -g ./packages/cli
```

Check the install:

```sh
tsgpui --version
tsgpui doctor
```

## Build the native host (once per machine)

Each app is packaged by copying a prebuilt native host and appending the app bundle. The host is built from the Rust sources bundled with this package:

```sh
tsgpui host build
```

This needs Rust (https://rustup.rs). On Windows it also needs the MSVC C++ build tools. The host is stored in `~/.tsgpui/host/` (override with `TSGPUI_HOME`). You can also point `TSGPUI_HOST` at a host executable you trust.

## Create and run an app

```sh
tsgpui init my-app
cd my-app
tsgpui dev        # build, launch, and rebuild on every save
tsgpui build      # produce dist/my-app.exe, which runs without Bun, Node, or Rust
```

Projects contain `tsgpui.config.json`, `src/index.html`, `src/styles.css`, and `src/backend.ts`. The HTML/CSS subset and the backend API are described in the [TSGPUI docs](https://github.com/ByteTheDev/TSGPUI/tree/main/docs).

## Commands

| Command | Purpose |
|---|---|
| `tsgpui init [dir]` | Create a starter project |
| `tsgpui compile [--project PATH]` | Write `dist/<name>.tgp` |
| `tsgpui dev [--project PATH]` | Build, launch the host, and rebuild on change |
| `tsgpui build [--project PATH]` | Write a single native executable |
| `tsgpui host build` | Compile the native host once |
| `tsgpui doctor` | Show the toolchain and host status |

## Limits

TSGPUI is an experimental prototype. It supports a subset of HTML and CSS, has no keyboard or text input yet, and runs backend code without CPU-time limits, so only run backends you trust. See the repository for the full list.

## License

MIT
