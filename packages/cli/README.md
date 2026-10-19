# tsgpui

Compile HTML, CSS and TypeScript into a native [GPUI](https://www.gpui.rs) window by default. Projects can select a system WebView renderer to keep browser layout and CSS behavior. Native apps run backend actions in embedded QuickJS; WebView apps run the compiled backend script in their WebView.

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
# or, for a project configured with renderer "webview":
tsgpui host build --renderer webview
```

This needs Rust (https://rustup.rs). On Windows it also needs the MSVC C++ build tools. The WebView host uses Wry and the platform WebView runtime: WebView2 on Windows, WKWebView on macOS, and WebKitGTK on Linux. Linux builds also need GTK and WebKitGTK development packages. The host is stored in `~/.tsgpui/host/` (override with `TSGPUI_HOME`). You can point `TSGPUI_HOST` or `TSGPUI_WEBVIEW_HOST` at a host executable you trust.

## Create and run an app

```sh
tsgpui init my-app
cd my-app
tsgpui dev        # build, launch, and rebuild on every save
tsgpui build      # produce dist/my-app.exe, which runs without Bun, Node, or Rust
```

Projects contain `tsgpui.config.json`, `src/index.html`, `src/styles.css`, and `src/backend.ts`. The HTML/CSS subset and the backend API are described in the [TSGPUI docs](https://github.com/ByteTheDev/TSGPUI/tree/main/docs).

Set `"renderer": "webview"` in the project config to select the WebView host. It renders the original HTML and CSS and embeds local image, font, script, and stylesheet resources into the app bundle. It supports the basic `data-bind`, `data-onclick`, `data-oninput`, form, file, clipboard, and `data-fetch` bindings. Compile-time components and `data-repeat` are native-renderer features. WebView pages can use ordinary HTML scripts and browser APIs, so only bundle code you trust; WebView network access follows browser rules and is not limited to `permissions.network` outside the declarative `data-fetch` binding.

## Commands

| Command | Purpose |
|---|---|
| `tsgpui init [dir]` | Create a starter project |
| `tsgpui compile [--project PATH]` | Write `dist/<name>.tgp` |
| `tsgpui dev [--project PATH]` | Build, launch the host, and rebuild on change |
| `tsgpui preview [--project PATH]` | Live preview in a browser at 127.0.0.1 (`--out FILE` writes one HTML file) |
| `tsgpui build [--project PATH]` | Write a single native executable |
| `tsgpui host build [--renderer MODE]` | Compile the native or WebView host once |
| `tsgpui doctor` | Show the toolchain and host status |

## Limits

TSGPUI is an experimental prototype. The native renderer supports a subset of HTML and CSS and runs backend code without CPU-time limits. The WebView renderer uses the system browser engine and lets authored scripts use browser APIs. Only bundle code you trust. See the repository docs for renderer differences and platform requirements.

## License

MIT
