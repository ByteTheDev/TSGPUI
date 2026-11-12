# Project Rendrix — TS-GPU rewrite charter

Decision (2026-11-11, owner-authorized): **Rendrix becomes the sole rendering engine. WebView
is removed completely. The product renames from TSGPUI to TS-GPU.** This file is the shared
contract for the rewrite agents: naming, ownership, interfaces, and the verification bar.

The pre-rewrite implementation is preserved at branch `backup/webview-era` (tag
`webview-era-checkpoint`, commit `0fc860b`). Nothing is unrecoverable.

## Naming decisions (binding)

| Thing                     | Old                       | New                              |
|---------------------------|---------------------------|----------------------------------|
| Product                   | TSGPUI                    | TS-GPU                           |
| Engine                    | gpu (module `native/src/gpu`) | **Rendrix**                  |
| Renderer config key       | `"webview"` / `"native"` / `"gpu"` | `"rendrix"` (default)   |
| Cargo package             | `tsgpui-host`             | `tsgpu-host`                     |
| Rendrix binary/feature    | `tsgpui-gpu-host` / `gpu-host` | `rendrix-host` / `rendrix`   |
| GPUI binary (deprecated)  | `tsgpui-host` / `native-host` | `tsgpu-gpui-host` / `native-host` |
| WebView binary            | `tsgpui-webview-host`     | **deleted**                      |
| Bundle magic (8 bytes)    | `TSGPUI01`                | `TSGPU001`                       |
| Bundle file extension     | `.tgp`                    | `.tgp` (unchanged)               |
| Home env var              | `TSGPUI_HOME`             | `TSGPU_HOME`                     |
| CLI entry                 | `packages/cli/bin/tsgpui.mjs` | `packages/cli/bin/tsgpu.mjs` |
| CLI package/command       | `tsgpui`                  | `tsgpu`                          |

Renderer key semantics: `"rendrix"` = Rendrix host (the engine). `"native"` = deprecated GPUI
host, accepted with a warning until the Rendrix M1 gate, then removed. `"webview"` = hard
error with migration guidance. Absent key = `"rendrix"`.

Preserved (works independent of WebView, kept through this rewrite): the Electron-compat
runtime (renderer-agnostic TypeScript), the updater (host-side download/verify/install),
QuickJS backend + permissions model, the GPUI host (deprecated, compiles, removed at the
Rendrix M1 gate), smoke/benchmark scripts (renderer-agnostic).

Removed with WebView: webview host bin + `webview-host` feature + `gtk`/`tao`/`wry` deps,
webview bridge/package/capabilities/protocol modules, CDP-based e2e, webview compiler/packaging
paths, webview tests, WebKitGTK Linux deps in CI.

Known deviation from the original brief: text shaping uses **cosmic-text** (already
implemented, tested, and by construction the same Linebender stack family as Parley) instead
of raw Parley. Re-evaluation is a later refactor, not a blocker.

## Phases

- **Phase A (now, 4 parallel agents):** Rust removal+rename (R), CLI removal+rename (J),
  docs/website rebrand (W), tests+CI rebuild (T).
- **Phase B (after A):** extract `native/src/gpu` into an independent, open-source `rendrix`
  crate (own document types; the host adapts `package::Node` into them), rename the internal
  module directory, `gpui` host removal gate review.
- **Phase C (after B):** Rendrix M1 — state-driven re-render, hover/active/focus, scroll,
  images; then the M2/M3 ladder (text editing, accesskit, animations).

## Agent ownership (strict — edit nothing outside your list)

- **Agent R (Rust):** `native/Cargo.toml`, `native/Cargo.lock`, everything under
  `native/src/` **except** `native/src/gpu/**` (read-only for you). Delete
  `webview_main.rs`/`webview_bridge.rs`/`webview_package.rs`/`webview_capabilities.rs` and
  `protocol.rs` **only if** webview-main is its sole consumer (verify first). Rename
  package/bins/feature/magic/env per the table. `mod updater` and `mod capabilities` stay.
- **Agent J (CLI/JS):** `packages/cli/**` **except** `packages/cli/test/**` (read-only).
  Root `package.json` also yours.
- **Agent W (docs/website):** `README.md`, `docs/**`, `website/**`, `CHANGELOG.md`.
  Run `node website/sync-docs.mjs` after doc changes.
- **Agent T (tests+CI):** `.github/**`, `packages/cli/test/**`, `scripts/e2e-windows.mjs`
  (delete), `scripts/webview-probe.mjs` (delete). `scripts/smoke-packaged.*` and
  `scripts/benchmark.mjs` are shared infrastructure — use them, do not edit them.

## Interfaces all agents code against

- Renderer key `"rendrix"` (see table). The Rendrix host (`gpu_main.rs`) already accepts
  exactly `"rendrix"`.
- Bundle magic `b"TSGPU001"` — Agent R writes it in `package.rs`, Agent J writes it in the
  packer; both sides must agree byte-for-byte.
- `hosts.json` v2: targets unchanged (`win32-x64`, `win32-arm64`, `darwin-arm64`, `linux-x64`,
  `linux-arm64`); entries reference `rendrix-host-<target>` (and `tsgpu-gpui-host-<target>`
  while deprecated). No webview entries.
- Host resolution in the CLI: cache dir `$TSGPU_HOME/host/<target>/`, binary names per the
  table; `tsgpu doctor` re-hashes as before.

## Verification bar (every agent, in your final report)

1. What you implemented / removed.
2. Which tests you ran and their results (commands + pass/fail counts). Do not weaken or
   bypass a test to make it pass — report the failure.
3. What remains incomplete.
4. The next engineering priority.

Baseline commands (from repo root):
- `cargo check --manifest-path native/Cargo.toml` (GPUI host compiles — deprecated, kept)
- `cargo check --manifest-path native/Cargo.toml --no-default-features --features rendrix`
- `cargo test --manifest-path native/Cargo.toml --features rendrix --bin rendrix-host`
- `bun run test` (JS suite; Agent T adjudicates which tests survive the WebView removal)
