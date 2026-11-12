# Building, targets and Electron import

This page covers how a TSGPUI project is run and built: the default `run` command, builds for one or more platform targets, the prebuilt hosts those builds need, Windows packaging, and converting an Electron project. For every flag, see the [CLI reference](../packages/cli/README.md). For what each command trusts and what it can run, see [SAFETY.md](SAFETY.md).

## Run a project

```sh
tsgpui            # same as tsgpui run
tsgpui run --project path/to/app
```

`run` builds the project and starts it in the host for this machine. If no verified host is installed, it downloads one first, unless downloads are disabled with `TSGPUI_NO_DOWNLOAD=1`. Unlike `dev`, `run` does not rebuild on save.

`run` also accepts an Electron project directory. It converts the project first; see [Electron projects](#electron-projects).

## Build for a target

A target is an operating system and CPU pair. TSGPUI supports six:

| Target | Output on this machine's build |
|---|---|
| `win32-x64` (alias `windows-x64`) | `.exe` |
| `win32-arm64` (alias `windows-arm64`) | `.exe` |
| `darwin-x64` (alias `macos-x64`) | `.app` directory bundle |
| `darwin-arm64` (alias `macos-arm64`) | `.app` directory bundle |
| `linux-x64` | ELF executable |
| `linux-arm64` | ELF executable |

```sh
tsgpui build                                   # this machine's target
tsgpui build --target linux-x64                # one target
tsgpui build --target linux-x64,win32-arm64    # a comma-separated list
tsgpui all                                     # every supported target
tsgpui linux-arm64                             # shortcut for build --target
```

`win32` and `darwin` also work in place of `windows` and `macos`. When you name a target explicitly, the output file includes it, for example `dist/my-app-linux-x64`. Without `--target`, the output name has no suffix.

Several targets build one after another. A failure on one target is reported, the rest still build, and the command ends with a non-zero exit code. The final line says how many targets built.

### Hosts for each target

Each target needs a host: the prebuilt program that the bundle is appended to. A host for another target is checked against `hosts.json` from the release, then copied into the cache under `~/.tsgpui/host/` (or `TSGPUI_HOME`). The build copies and appends the bundle; it never runs the host for another target.

If downloads are disabled, install a host yourself, after checking its SHA-256 through a trusted channel:

```sh
tsgpui host install --file path/to/tsgpui-host --sha256 <64-digit hash> --target linux-x64 --renderer native
```

`--renderer webview` installs the WebView host. Use `tsgpui doctor` to see which hosts are installed and what each one needs.

## Package for Windows

```sh
tsgpui package                                  # dist/<name>.exe and dist/<name>.manifest.json
tsgpui package --installer nsis                 # also dist/<name>-setup.exe
tsgpui package --sign                           # sign each artifact with TSGPUI_SIGN_COMMAND
tsgpui package --installer nsis --sign          # signed installer (TSGPUI_SIGN_COMMAND) + manifest
tsgpui package --installer nsis --sign --publish-url https://example.com/app  # manual-publish reminder
```

`package` works on Windows only. It writes a SHA-256 manifest beside the artifacts. If you pass `--manifest-key` (or set `TSGPUI_MANIFEST_KEY`), the manifest also gets an Ed25519 signature. The NSIS installer needs `makensis` and the inetc plugin. Signing runs the command in `TSGPUI_SIGN_COMMAND` on each file; the release guide covers the details in [RELEASING.md](RELEASING.md).

Electron-builder counts differ here: `publish` has no TSGPUI equivalent. After `tsgpui package --installer nsis --sign`, upload the `.exe`, `-setup.exe` and `.manifest.json` (plus `.manifest.json.sig` when signed) to the old `publishUrl` by hand; `tsgpui package --publish-url <URL>` (or `packaging.publishUrl` in `tsgpui.config.json`) only prints that reminder, it does not upload. Electron `autoUpdater` is `unsupported` in the [coverage ledger](../packages/cli/src/electron-compat/coverage.mjs) ("TSGPUI updates are shipped by its release process, not by the app"): remove in-app update checks and ship updates through the TSGPUI release process instead.

For macOS, `build` produces an unsigned `.app`. Sign and notarize it separately.

## Electron projects

### Easy transfer in 3 steps

1. **Import:** `tsgpui import electron path/to/electron-app --out ./ported` (adds `-tsgpui` suffix by default; use `--build` to run the app's build first, `--force` to reuse an existing output directory, or `--fix` to preview fixes from the report without overwriting). The importer also emits a `packaging` section in `tsgpui.config.json` (`installer`/`signCmd`/`publishUrl`) for projects with electron-builder config.
2. **Review the report:** open `./ported/TSGPUI_IMPORT_REPORT.md`. Work the checklist top-down — each item now prints a concrete fix command, for example `add "bundleId": "com.example..."` (from `pkg.build.appId`), `set "outDir" to "release"` (from `pkg.build.directories.output`), `tsgpui package --installer nsis --sign` with `TSGPUI_SIGN_COMMAND`/ `TSGPUI_MANIFEST_KEY` (from `win.certificateFile`/`certificateSubjectName`/`publish`), a `publishUrl` manual-upload step, and an `autoUpdater` status line, plus `Fix:` lines for permissions and `tsgpui import electron <dir> --out <dir> --fix` to preview the next fixes. Apply edits to `./ported/tsgpui.config.json` (see [Signing, publishing and auto-update](#signing-publishing-and-auto-update)).
3. **Build:** `tsgpui build --project ./ported` (or `cd ./ported && tsgpui build`). If the build complains, re-check the report's `Fix:` lines and re-run with `--fix --force` to regenerate a fresh conversion. On Windows, finish the release with `tsgpui package --installer nsis --sign --project ./ported` (see [Signing, publishing and auto-update](#signing-publishing-and-auto-update)).

`run`, `build` and the target shortcuts detect an Electron project: a directory with a `package.json` and no `tsgpui.config.json`. They convert it into a new temporary directory and build or run that copy. The source project is not modified.

To convert a project yourself, keep the output:

```sh
tsgpui import electron path/to/electron-app                # writes path/to/electron-app-tsgpui
tsgpui import electron path/to/electron-app --out ./ported # chosen output directory
tsgpui import electron path/to/electron-app --force        # reuse an existing output directory
tsgpui import electron path/to/electron-app --build        # run the app's build script first
tsgpui import electron path/to/electron-app --out ./ported --fix  # preview fixes from the report (no overwrite without --force)
```

The output directory must not contain the source directory or lie inside it. An existing output directory is refused unless you pass `--force`.

### Signing, publishing and auto-update

Electron-builder signing, publishing and in-app updates have no TSGPUI equivalent. The 3-step guide above leaves them as manual release steps; the import report and the generated `packaging` section record what to do. Use the exact commands from the report:

```sh
# 1. Sign with your own signtool/Azure Trusted Signing command. The path is passed
#    via TSGPUI_SIGN_COMMAND (never committed); the command is not logged.
#    win.certificateFile / certificateSubjectName map here.
TSGPUI_SIGN_COMMAND="signtool sign /fd SHA256 /a /f <your-cert.pfx> /p <password>" \
  tsgpui package --installer nsis --sign

# 2. Sign the SHA-256 manifest with an Ed25519 key (TSGPUI_MANIFEST_KEY is another option).
TSGPUI_MANIFEST_KEY=<path-to-ed25519-key> tsgpui package --installer nsis --sign
# or: tsgpui package --installer nsis --sign --manifest-key <path-to-ed25519-key>
```

Final artifacts are `dist/<name>.exe`, `dist/<name>-setup.exe`, `dist/<name>.manifest.json` and, when signed, `dist/<name>.manifest.json.sig`. Publishing itself is manual: upload those files to the old electron-builder `publishUrl` (surfaced in the report as `publishUrl` and copied to `packaging.publishUrl` in `tsgpui.config.json`) and verify the `.exe`/manifest identity there. `tsgpui package --publish-url <URL>` only prints this reminder; it does not upload.

`autoUpdater` is `unsupported` in the [coverage ledger](../packages/cli/src/electron-compat/coverage.mjs): "TSGPUI updates are shipped by its release process, not by the app". Remove `autoUpdater`/`update-electron-app` checks and any `checkForUpdates`/`quitAndInstall` flow from the converted backend, and ship newer versions through the TSGPUI release process described in [RELEASING.md](RELEASING.md). Keep certificates, passwords and `TSGPUI_SIGN_COMMAND`/`TSGPUI_MANIFEST_KEY` values outside the repository.

### What the conversion does

- Main-process code is rewritten to TypeScript that uses TSGPUI's compatibility runtime. Each `require('electron')` or `import` from `electron` is replaced with a shim.
- Renderer HTML, CSS and scripts are copied, and the importer creates a `webview` project that points at the renderer page.
- Preload scripts, when they can be matched to a window, are bundled into the page. Preloads that cannot be matched are listed for manual migration.
- The importer derives `permissions` from what the code uses: network hosts from `fetch`, `axios` and `https` calls, processes from `child_process`, and filesystem and window use. Review each grant. The importer does not decide whether a grant is safe.

### Coverage and the report

Every Electron API the app uses is looked up in the coverage ledger ([`coverage.mjs`](../packages/cli/src/electron-compat/coverage.mjs)). Each API has one of three statuses:

| Status | Meaning |
|---|---|
| `implemented` | The shim behaves like Electron for normal use. |
| `partial` | The shim runs, but part of the behaviour is missing. The report says what. |
| `unsupported` | The shim exists but throws a clear error when used. |

The importer writes `TSGPUI_IMPORT_REPORT.md` into the converted directory and prints its path. The report lists:

- every partial, unsupported or unknown Electron API the app uses, with its file and line;
- rewritten module specifiers;
- the permissions it derived;
- manual steps, such as dynamic paths, preloads it could not match, and Electron Forge or electron-builder settings it does not read.

Read the report before you rely on a converted app. A conversion with warnings is not a finished port.

### Build scripts run project code

Importing runs the app's own build script in two cases:

- with `--build`, always;
- when `run` or `build` finds no built renderer, the importer runs `build` and tries again.

The script is `bun run build` when the project has a `bun.lock`, and `npm run build` otherwise. It runs with your user's rights. Convert only projects you trust.

### Commands that do not convert

`dev`, `compile` and `preview` read TSGPUI projects only. To work on a converted app with them, convert it first with `tsgpui import electron`, then run those commands in the output directory.

## Troubleshooting

| Message | What to do |
|---|---|
| `No verified <renderer> host is installed` | Run `tsgpui host fetch`, or install one with `tsgpui host install`. |
| `No <renderer> host for <target> is installed, and downloads are disabled` | Install a host for that target with `tsgpui host install --target <target>`. |
| `Output already exists` | Choose another `--out`, or pass `--force` to reuse the directory. |
| `Build script failed` | Fix the app's own build first. Run its `build` script directly to see the error. |
| `Windows .exe requires a PE (MZ) host` | The host for the target does not match. Check `--target` and the installed host. |
| `Unsupported target` | Use one of the six targets above, or their `windows-` and `macos-` aliases. |
