# Releasing

Packaging helpers are not yet connected to the CLI. The NSIS generator requires makensis and the inetc NSIS plugin. It downloads Microsoft's evergreen WebView2 bootstrapper only when the runtime is missing. Review and test the generated installer on Windows before distribution.

The signing helper reads `TSGPUI_SIGN_COMMAND` from the environment, appends the artifact path, and does not log its command or output. Keep certificates and credentials outside the repository. If the variable is missing, the artifact is unsigned.

The manifest helper writes SHA-256 hashes for supplied artifacts. An optional Ed25519 private key path produces a detached base64 signature over the exact manifest bytes. The host does not verify signatures yet; verification requires a Rust change owned by another agent.

A future update feed can use JSON such as `{"version":"1.2.3","url":"https://example.com/app.exe","sha256":"...","signature":"..."}`. Feed authentication, trusted key distribution, rollback protection, and the updater are future work.
