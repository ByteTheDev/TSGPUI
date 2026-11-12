# Releasing

`tsgpu package` (Windows only) connects the packaging helpers: it builds the executable, signs it with `--sign`, builds an NSIS installer with `--installer nsis`, and writes a SHA-256 manifest. The NSIS generator requires makensis and the inetc NSIS plugin. Review and test the generated installer on Windows before distribution.

The `release-hosts` workflow builds the Rendrix host (`rendrix-host`) for six targets (win32-x64, win32-arm64, darwin-x64, darwin-arm64, linux-x64 and linux-arm64), attests their build provenance, and attaches them with `hosts.json` to the release. The deprecated GPUI host (`tsgpu-gpui-host`) is published alongside until the Rendrix M1 gate removes it. The CLI checks each downloaded host against that manifest.

The signing helper reads `TSGPU_SIGN_COMMAND` from the environment, appends the artifact path, and does not log its command or output. Keep certificates and credentials outside the repository. If the variable is missing, the artifact is unsigned. As a convenience, you can also set `TSGPU_CERT_FILE` (plus optional `TSGPU_CERT_PASSWORD`) on Windows to auto-build a `signtool` command, `TSGPU_CODESIGN_IDENTITY` on macOS to auto-build a `codesign` command, or `TSGPU_GPG_KEY` on Linux to auto-build a `gpg --detach-sign` command; `TSGPU_SIGN_COMMAND` always takes precedence.

Any artifact can be signed directly with `tsgpu sign --file PATH` (add `--notarize` on macOS to run `xcrun notarytool`, which needs `APPLE_ID`, `APPLE_TEAM_ID` and `APPLE_APP_PASSWORD`). On macOS, `TS-GPU package` is Windows-only; build with `tsgpu build` and then `tsgpu sign --file ... --notarize`.

The manifest helper writes SHA-256 hashes for supplied artifacts. An optional Ed25519 private key path produces a detached base64 signature over the exact manifest bytes. The host does not verify signatures yet; verification requires a Rust change owned by another agent.

A future update feed can use JSON such as `{"version":"1.2.3","url":"https://example.com/app.exe","sha256":"...","signature":"..."}`. Feed authentication, trusted key distribution, rollback protection, and the updater are future work.

Electron-builder migration: `win.certificateFile` and `certificateSubjectName` map to the `TSGPU_SIGN_COMMAND` environment variable (keep the certificate and password outside the repository) and `publish` maps to a manual upload of `tsgpu package --installer nsis --sign` artifacts and the `.manifest.json` to `packaging.publishUrl`. Electron `autoUpdater` is `unsupported` in the coverage ledger ("TS-GPU updates are shipped by its release process, not by the app"): remove in-app update checks and ship updates through this release process. See the signing/auto-update subsection in [BUILDING.md](BUILDING.md) for the exact commands.
