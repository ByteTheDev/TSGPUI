# rendrix

Compile HTML, CSS and TypeScript into a native window rendered by [Rendrix](#), Rendrix's own GPU renderer, backed by the Rendrix toolchain. Backend actions run in an embedded QuickJS engine inside the host.

## Install

With Bun:

```
bun add -g @bytethedeveloper/rendrix
```

Or install a released archive with a script. It downloads `rendrix-cli.tgz`, checks its SHA-256, and places the CLI in Bun's bin directory (`~/.bun/bin`). It needs Bun or Node.js 20+, and it does not use a package registry:

macOS and Linux:

```
curl -fsSL https://raw.githubusercontent.com/ByteTheDev/Rendrix/main/packages/cli/install/install.sh | sh
```

Windows (PowerShell):

```
irm https://raw.githubusercontent.com/ByteTheDev/Rendrix/main/packages/cli/install/install.ps1 | iex
```

Set `RENDRIX_VERSION` (for example `0.4.0-beta.2`) to pin a release. Set `RENDRIX_URL` to install a different archive, such as a mirror. `BUN_INSTALL` moves the install location. The Windows script adds the bin directory to your user PATH unless `RENDRIX_NO_MODIFY_PATH=1` is set; the macOS and Linux script prints the PATH line for you to add.

## Check the setup

```
rendrix --version
rendrix doctor
```

## Hosts

Each app is packaged by copying a host executable and appending the app bundle. You normally do nothing: the first `rendrix dev` or `rendrix build` downloads the prebuilt host for your OS and architecture, checks it, and caches it in `~/.rendrix/host/` (override with `RENDRIX_HOME`). To do it ahead of time:

```
rendrix host fetch                       # both renderers
rendrix host fetch --renderer native     # one renderer
```

Release builds publish hosts for five targets, `win32-x64`, `win32-arm64`, `darwin-arm64`, `linux-x64` and `linux-arm64` (both renderers), with a `hosts.json` (schema 2) that lists every file's size and SHA-256. A download is installed only if its size and hash match the manifest. If the package ships its own `hosts.json` (release builds do), that copy is used instead of the one on GitHub, so a swapped release asset fails its hash. The cache keeps an install record beside each host; the CLI uses a host only if it was built for the same bundle format and the same `major.minor` release, and `rendrix doctor` re-hashes it. Hashes prove the file is the one the release listed; the release's build provenance attestation (`gh attestation verify`) is what ties it to this repository's workflow.

Set `RENDRIX_NO_DOWNLOAD=1` to forbid downloads, or `RENDRIX_HOST_BASE_URL` to read `hosts.json` and the hosts from a mirror.

## Building a host from source

```
rendrix host build                       # the Rendrix host for this machine
rendrix host build --renderer native     # the deprecated legacy host
```

That needs Rust (https://rustup.rs) and, on Windows, the MSVC C++ build tools. You can also point `RENDRIX_HOST` at a host executable you trust.

## Commands

| Command | Purpose |
| --- | --- |
| `rendrix run [--project PATH]` | Start a Rendrix or Electron project (the default command) |
| `rendrix init [dir] [--template NAME]` | Create a starter: `counter` (default), `form`, `list-detail`, `electron-import` (`--list-templates`) |
| `rendrix import electron <dir> [--out DIR] [--build] [--force]` | Convert an Electron project into a Rendrix project and write its review report |
| `rendrix compile [--project PATH]` | Write `dist/<name>.rdx` |
| `rendrix dev [--project PATH]` | Build, launch the host, and rebuild on change |
| `rendrix preview [--project PATH]` | Live preview in a browser at 127.0.0.1 (`--out FILE` writes one HTML file) |
| `rendrix build [--project PATH] [--target T]` | Write a single native executable for T: one target, a comma-separated list, or `all` (default: this machine) |
| `rendrix <target>` or `rendrix all` | Shortcut for `build --target` |
| `rendrix host fetch [--renderer MODE] [--force]` | Download and verify the prebuilt host |
| `rendrix host build [--renderer MODE]` | Compile the host from source instead (needs Rust) |
| `rendrix host install --file PATH --sha256 HASH [--renderer MODE] [--target T]` | Install a host whose checksum you obtained from a trusted source |
| `rendrix package [--installer nsis] [--sign] [--manifest-key PATH]` | Windows only: write the executable, optionally sign it and build an NSIS installer, then write a SHA-256 manifest |
| `rendrix release manifest --file PATH` | Write a deterministic SHA-256 manifest |
| `rendrix doctor [--renderer MODE]` | Check the setup and print the fix for anything missing |

## Starting a project

```
rendrix init my-app

rendrix dev        # build, launch, and rebuild on every save
rendrix build      # produce a native executable, which runs without Bun, Node, or Rust
```

`rendrix run` is the default command, so plain `rendrix` starts the project. It also accepts an Electron project directory: the project is converted into a temporary Rendrix project, the conversion report's path is printed, and the converted app starts. `rendrix build` and the target shortcuts convert Electron projects the same way; `dev`, `compile` and `preview` expect a Rendrix project.

Cross-target builds use `--target`, which takes one target, a comma-separated list, or `all`: `rendrix build --target linux-x64,win32-arm64`, or the shortcut `rendrix all`. `windows-*` and `macos-*` spellings are accepted. Each target needs a verified prebuilt host, downloaded when available. A failed target is reported, the remaining targets still build, and the command exits non-zero. Named targets are included in output file names. The [building guide](https://github.com/ByteTheDev/Rendrix/blob/main/docs/BUILDING.md) covers hosts, packaging and Electron conversion in detail. Hosts for other targets are copied and appended to, never run.

## Project shape

Projects contain `rendrix.config.json`, `src/index.html`, `src/styles.css`, and `src/backend.ts`. The renderer key is optional: an absent key selects `"rendrix"`, the Rendrix renderer. `"native"` still selects the deprecated legacy host with a deprecation warning; `"webview"` is a hard error — the WebView renderer was removed, and Rendrix renders natively with Rendrix. The HTML/CSS subset and the backend API are described in the [Rendrix docs](https://github.com/ByteTheDev/Rendrix/tree/main/docs).

## Trusted code

Rendrix is an experimental prototype. Native executable packaging supports Windows PE, Linux ELF, and macOS Mach-O hosts. The Rendrix renderer supports a subset of HTML and CSS. Only bundle code you trust: backend actions run with the grants the project's `permissions` allow. See the repository docs for renderer details and platform requirements. Electron conversion is partial: read the import report before relying on a converted app.

## Installing a host by hand

```
rendrix host install --file path/to/rendrix-host --sha256 YOUR_64_DIGIT_SHA256 --renderer rendrix
```

Use `--renderer native` for the deprecated legacy host. The installer checks the supplied hash before copying the file into the host cache. A checksum obtained from the same untrusted source as the binary does not establish its provenance. Hosts are platform- and version-specific; the installer does not independently verify compatibility or signatures.

## Releasing

See the [release guide](https://github.com/ByteTheDev/Rendrix/blob/main/docs/RELEASING.md) for metadata, icons, signing, notarization, and artifact manifests. Rebuild hosts after upgrading the CLI.
