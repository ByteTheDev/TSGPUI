# tsgpu

Compile HTML, CSS and TypeScript into a native window rendered by [Rendrix](#), TS-GPU's own GPU renderer, backed by the GPUI toolchain. Backend actions run in an embedded QuickJS engine inside the host.

## Install

With Bun:

```
bun add -g @bytethedeveloper/tsgpu
```

Or install a released archive with a script. It downloads `tsgpu-cli.tgz`, checks its SHA-256, and places the CLI in Bun's bin directory (`~/.bun/bin`). It needs Bun or Node.js 20+, and it does not use a package registry:

macOS and Linux:

```
curl -fsSL https://raw.githubusercontent.com/ByteTheDev/TSGPUI/main/packages/cli/install/install.sh | sh
```

Windows (PowerShell):

```
irm https://raw.githubusercontent.com/ByteTheDev/TSGPUI/main/packages/cli/install/install.ps1 | iex
```

Set `TSGPU_VERSION` (for example `0.4.0-beta.2`) to pin a release. Set `TSGPU_URL` to install a different archive, such as a mirror. `BUN_INSTALL` moves the install location. The Windows script adds the bin directory to your user PATH unless `TSGPU_NO_MODIFY_PATH=1` is set; the macOS and Linux script prints the PATH line for you to add.

## Check the setup

```
tsgpu --version
tsgpu doctor
```

## Hosts

Each app is packaged by copying a host executable and appending the app bundle. You normally do nothing: the first `tsgpu dev` or `tsgpu build` downloads the prebuilt host for your OS and architecture, checks it, and caches it in `~/.tsgpu/host/` (override with `TSGPU_HOME`). To do it ahead of time:

```
tsgpu host fetch                       # both renderers
tsgpu host fetch --renderer native     # one renderer
```

Release builds publish hosts for five targets, `win32-x64`, `win32-arm64`, `darwin-arm64`, `linux-x64` and `linux-arm64` (both renderers), with a `hosts.json` (schema 2) that lists every file's size and SHA-256. A download is installed only if its size and hash match the manifest. If the package ships its own `hosts.json` (release builds do), that copy is used instead of the one on GitHub, so a swapped release asset fails its hash. The cache keeps an install record beside each host; the CLI uses a host only if it was built for the same bundle format and the same `major.minor` release, and `tsgpu doctor` re-hashes it. Hashes prove the file is the one the release listed; the release's build provenance attestation (`gh attestation verify`) is what ties it to this repository's workflow.

Set `TSGPU_NO_DOWNLOAD=1` to forbid downloads, or `TSGPU_HOST_BASE_URL` to read `hosts.json` and the hosts from a mirror.

## Building a host from source

```
tsgpu host build                       # the Rendrix host for this machine
tsgpu host build --renderer native     # the deprecated GPUI host
```

That needs Rust (https://rustup.rs) and, on Windows, the MSVC C++ build tools. You can also point `TSGPU_HOST` at a host executable you trust.

## Commands

| Command | Purpose |
| --- | --- |
| `tsgpu run [--project PATH]` | Start a TS-GPU or Electron project (the default command) |
| `tsgpu init [dir]` | Create a starter project |
| `tsgpu import electron <dir> [--out DIR] [--build] [--force]` | Convert an Electron project into a TS-GPU project and write its review report |
| `tsgpu compile [--project PATH]` | Write `dist/<name>.tgp` |
| `tsgpu dev [--project PATH]` | Build, launch the host, and rebuild on change |
| `tsgpu preview [--project PATH]` | Live preview in a browser at 127.0.0.1 (`--out FILE` writes one HTML file) |
| `tsgpu build [--project PATH] [--target T]` | Write a single native executable for T: one target, a comma-separated list, or `all` (default: this machine) |
| `tsgpu <target>` or `tsgpu all` | Shortcut for `build --target` |
| `tsgpu host fetch [--renderer MODE] [--force]` | Download and verify the prebuilt host |
| `tsgpu host build [--renderer MODE]` | Compile the host from source instead (needs Rust) |
| `tsgpu host install --file PATH --sha256 HASH [--renderer MODE] [--target T]` | Install a host whose checksum you obtained from a trusted source |
| `tsgpu package [--installer nsis] [--sign] [--manifest-key PATH]` | Windows only: write the executable, optionally sign it and build an NSIS installer, then write a SHA-256 manifest |
| `tsgpu release manifest --file PATH` | Write a deterministic SHA-256 manifest |
| `tsgpu doctor [--renderer MODE]` | Check the setup and print the fix for anything missing |

## Starting a project

```
tsgpu init my-app

tsgpu dev        # build, launch, and rebuild on every save
tsgpu build      # produce a native executable, which runs without Bun, Node, or Rust
```

`tsgpu run` is the default command, so plain `tsgpu` starts the project. It also accepts an Electron project directory: the project is converted into a temporary TS-GPU project, the conversion report's path is printed, and the converted app starts. `tsgpu build` and the target shortcuts convert Electron projects the same way; `dev`, `compile` and `preview` expect a TS-GPU project.

Cross-target builds use `--target`, which takes one target, a comma-separated list, or `all`: `tsgpu build --target linux-x64,win32-arm64`, or the shortcut `tsgpu all`. `windows-*` and `macos-*` spellings are accepted. Each target needs a verified prebuilt host, downloaded when available. A failed target is reported, the remaining targets still build, and the command exits non-zero. Named targets are included in output file names. The [building guide](https://github.com/ByteTheDev/TSGPUI/blob/main/docs/BUILDING.md) covers hosts, packaging and Electron conversion in detail. Hosts for other targets are copied and appended to, never run.

## Project shape

Projects contain `tsgpui.config.json`, `src/index.html`, `src/styles.css`, and `src/backend.ts`. The renderer key is optional: an absent key selects `"rendrix"`, the Rendrix renderer. `"native"` still selects the deprecated GPUI host with a deprecation warning; `"webview"` is a hard error — the WebView renderer was removed, and TS-GPU renders natively with Rendrix. The HTML/CSS subset and the backend API are described in the [TS-GPU docs](https://github.com/ByteTheDev/TSGPUI/tree/main/docs).

## Trusted code

TS-GPU is an experimental prototype. Native executable packaging supports Windows PE, Linux ELF, and macOS Mach-O hosts. The Rendrix renderer supports a subset of HTML and CSS. Only bundle code you trust: backend actions run with the grants the project's `permissions` allow. See the repository docs for renderer details and platform requirements. Electron conversion is partial: read the import report before relying on a converted app.

## Installing a host by hand

```
tsgpu host install --file path/to/rendrix-host --sha256 YOUR_64_DIGIT_SHA256 --renderer rendrix
```

Use `--renderer native` for the deprecated GPUI host. The installer checks the supplied hash before copying the file into the host cache. A checksum obtained from the same untrusted source as the binary does not establish its provenance. Hosts are platform- and version-specific; the installer does not independently verify compatibility or signatures.

## Releasing

See the [release guide](https://github.com/ByteTheDev/TSGPUI/blob/main/docs/RELEASING.md) for metadata, icons, signing, notarization, and artifact manifests. Rebuild hosts after upgrading the CLI.
