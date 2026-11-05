# tsgpui

Compile HTML, CSS and TypeScript into a native [GPUI](https://www.gpui.rs) window by default. Projects can select a system WebView renderer to keep browser layout and CSS behavior. Native apps run backend actions in embedded QuickJS; WebView apps run the compiled backend script in their WebView.

## Install

Install once, globally, with Bun:

```sh
bun add -g @bytethedeveloper/tsgpui
```

To work on the CLI itself, install from a checkout of the repository:

```sh
bun add -g ./packages/cli
```

Or install a released archive with a script. It downloads `tsgpui-cli.tgz`, checks its SHA-256, and places the CLI in Bun's bin directory (`~/.bun/bin`). It needs Bun or Node.js 20+, and it does not use a package registry:

```sh
# macOS and Linux
curl -fsSL https://raw.githubusercontent.com/ByteTheDev/TSGPUI/main/packages/cli/install/install.sh | sh
```

```powershell
# Windows (PowerShell)
irm https://raw.githubusercontent.com/ByteTheDev/TSGPUI/main/packages/cli/install/install.ps1 | iex
```

If you save `install.ps1` and run it as a file, Windows can block it because it was downloaded. Run `Unblock-File .\install.ps1` first, or use the `irm | iex` line above, which doesn't need the file.

Set `TSGPUI_VERSION` (for example `0.4.0-beta.1`) to pin a release. Set `TSGPUI_URL` to install a different archive, such as a mirror. `BUN_INSTALL` moves the install location. The Windows script adds the bin directory to your user PATH unless `TSGPUI_NO_MODIFY_PATH=1` is set; the macOS and Linux script prints the PATH line for you to add.

Check the install:

```sh
tsgpui --version
tsgpui doctor
```

To publish an installer-ready build, run `bun run release:archive` here. It writes `release/tsgpui-cli.tgz` and `release/tsgpui-cli.tgz.sha256`. Upload both as assets of a GitHub release tagged `v<version>`. The installers download `releases/latest/download/tsgpui-cli.tgz` unless `TSGPUI_VERSION` pins a tag, so the newest release must include both files.

## The host (no Rust needed)

Each app is packaged by copying a host executable and appending the app bundle. You normally do nothing: the first `tsgpui dev` or `tsgpui build` downloads the prebuilt host for your OS, architecture and renderer, checks it, and caches it in `~/.tsgpui/host/` (override with `TSGPUI_HOME`). To do it ahead of time:

```sh
tsgpui host fetch                       # both renderers
tsgpui host fetch --renderer webview    # one renderer
```

Release builds publish hosts for six targets, `win32-x64`, `win32-arm64`, `darwin-x64`, `darwin-arm64`, `linux-x64` and `linux-arm64` (both renderers), with a `hosts.json` that lists every file's size and SHA-256. A download is installed only if its size and hash match the manifest. If the package ships its own `hosts.json` (release builds do), that copy is used instead of the one on GitHub, so a swapped release asset fails its hash. The cache keeps an install record beside each host; the CLI uses a host only if it was built for the same bundle format and the same `major.minor` release, and `tsgpui doctor` re-hashes it. Hashes prove the file is the one the release listed; the release's build provenance attestation (`gh attestation verify`) is what ties it to this repository's workflow.

Set `TSGPUI_NO_DOWNLOAD=1` to forbid downloads, or `TSGPUI_HOST_BASE_URL` to read `hosts.json` and the hosts from a mirror.

If no prebuilt host fits (for example, a release that does not list your platform), the CLI says so and falls back to compiling one when Rust is installed:

```sh
tsgpui host build [--renderer webview]
```

That needs Rust (https://rustup.rs) and, on Windows, the MSVC C++ build tools. The WebView host uses the platform WebView runtime: WebView2 on Windows, WKWebView on macOS, and WebKitGTK on Linux (with GTK and WebKitGTK development packages to build it). You can also point `TSGPUI_HOST` or `TSGPUI_WEBVIEW_HOST` at a host executable you trust.

`tsgpui doctor` checks Node, the host for your project's renderer, system libraries and, if relevant, Rust, and prints the exact command that fixes each problem. It exits non-zero only for a problem that stops the app from running.

## Create and run an app

```sh
tsgpui init my-app
cd my-app
tsgpui dev        # build, launch, and rebuild on every save
tsgpui build      # produce a native executable, which runs without Bun, Node, or Rust
```

`tsgpui run` is the default command, so plain `tsgpui` starts the project. It also accepts an Electron project directory: the project is converted into a temporary TSGPUI project, the conversion report's path is printed, and the converted app starts. `tsgpui build` and the target shortcuts convert Electron projects the same way; `dev`, `compile` and `preview` expect a TSGPUI project.

Cross-target builds use `--target`, which takes one target, a comma-separated list, or `all`: `tsgpui build --target linux-x64,win32-arm64`, or the shortcut `tsgpui all`. `windows-*` and `macos-*` spellings are accepted. Each target needs a verified prebuilt host, downloaded when available. A failed target is reported, the remaining targets still build, and the command exits non-zero. Named targets are included in output file names. The [building guide](https://github.com/ByteTheDev/TSGPUI/blob/main/docs/BUILDING.md) covers hosts, packaging and Electron conversion in detail. Hosts for other targets are copied and appended to, never run.

On macOS, the output is an unsigned `.app` bundle. The host must be compiled for the target Mac architecture. Sign and notarize the finished bundle using the release guide; the repository workflow builds separate x64 and Apple Silicon `.app` bundles.

Projects contain `tsgpui.config.json`, `src/index.html`, `src/styles.css`, and `src/backend.ts`. The HTML/CSS subset and the backend API are described in the [TSGPUI docs](https://github.com/ByteTheDev/TSGPUI/tree/main/docs).

Set `"renderer": "webview"` in the project config to select the WebView host. It renders the original HTML and CSS and embeds local image, font, script, and stylesheet resources into the app bundle. It supports the basic `data-bind`, `data-onclick`, `data-oninput`, form, file, clipboard, and `data-fetch` bindings. Compile-time components and `data-repeat` are native-renderer features. WebView pages can use ordinary HTML scripts and browser APIs, so only bundle code you trust; WebView network access follows browser rules and is not limited to `permissions.network` outside the declarative `data-fetch` binding.

## Commands

| Command | Purpose |
|---|---|
| `tsgpui run [--project PATH]` | Start a TSGPUI or Electron project (the default command) |
| `tsgpui init [dir]` | Create a starter project |
| `tsgpui import electron <dir> [--out DIR] [--build] [--force]` | Convert an Electron project into a TSGPUI project and write its review report |
| `tsgpui compile [--project PATH]` | Write `dist/<name>.tgp` |
| `tsgpui dev [--project PATH]` | Build, launch the host, and rebuild on change |
| `tsgpui preview [--project PATH]` | Live preview in a browser at 127.0.0.1 (`--out FILE` writes one HTML file) |
| `tsgpui build [--project PATH] [--target T]` | Write a single native executable for T: one target, a comma-separated list, or `all` (default: this machine) |
| `tsgpui <target>` or `tsgpui all` | Shortcut for `build --target` |
| `tsgpui host fetch [--renderer MODE] [--force]` | Download and verify the prebuilt host |
| `tsgpui host build [--renderer MODE]` | Compile the host from source instead (needs Rust) |
| `tsgpui host install --file PATH --sha256 HASH [--renderer MODE] [--target T]` | Install a host whose checksum you obtained from a trusted source |
| `tsgpui package [--installer nsis] [--sign] [--manifest-key PATH]` | Windows only: write the executable, optionally sign it and build an NSIS installer, then write a SHA-256 manifest |
| `tsgpui release manifest --file PATH` | Write a deterministic SHA-256 manifest |
| `tsgpui doctor [--renderer MODE]` | Check the setup and print the fix for anything missing |

## Limits

TSGPUI is an experimental prototype. Native executable packaging supports Windows PE, Linux ELF, and macOS Mach-O hosts. The native renderer supports a subset of HTML and CSS. The WebView renderer uses the system browser engine and lets authored scripts use browser APIs. Only bundle code you trust. See the repository docs for renderer differences and platform requirements. Electron conversion is partial: read the import report before relying on a converted app.

## License

MIT


### Install a prebuilt host

Offline or air-gapped? Download the host artifact matching your OS, architecture, renderer, and CLI version from a trusted release or CI run. Verify its provenance and obtain its SHA-256 through a trusted channel, then run:

```sh
tsgpui host install --file path/to/tsgpui-host --sha256 YOUR_64_DIGIT_SHA256 --renderer native
```

Use `--renderer webview` for a WebView host. The installer checks the supplied hash before copying the file into the host cache. A checksum obtained from the same untrusted source as the binary does not establish its provenance. Hosts are platform- and version-specific; the installer does not independently verify compatibility or signatures.

## Distribution

See the [release guide](https://github.com/ByteTheDev/TSGPUI/blob/main/docs/RELEASING.md) for metadata, icons, signing, notarization, and artifact manifests. Rebuild hosts after upgrading the CLI.
