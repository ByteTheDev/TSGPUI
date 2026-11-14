# Roadmap to 1.0 (out of experimental)

Tracking work to graduate Rendrix from experimental. Each track is independently shippable.
Status is checked against the code and docs as of 2026-11-11.

## Native renderer strategy (decided 2026-11-11, superseded 2026-11-11)

**Superseded by [REWRITE_RENDRIX.md](REWRITE_RENDRIX.md):** Rendrix (winit + wgpu + taffy +
cosmic-text, per [RENDERER_RENDRIX.md](RENDERER_RENDRIX.md)) is now the sole rendering engine; the
WebView renderer was removed the same day; the legacy host is deprecated and is removed at the
Rendrix M1 gate. The original decision is kept below as history:

> GPUI stays. We do **not** build our own renderer or fork GPUI. The native path is treated as an
> upgradeable dependency: check GPUI upstream quarterly for the primitives we lack (element
> transforms, accessibility APIs, per-display scale factors, multi-view layout) and adopt when
> they land. Until then, WebView is the renderer for anything GPUI 0.2.2 cannot do (motion,
> screen readers, video, multi-window). Re-evaluate the "assemble winit + vello + parley +
> taffy + accesskit" route only if GPUI upstream goes quiet — not before. The compiler's
> renderer-agnostic IR is the asset; the renderer is a commodity.

Standing item (recurring): track the Rendrix M1–M3 ladder in [RENDERER_RENDRIX.md](RENDERER_RENDRIX.md);
update Track 2/3 notes as it lands.

- **2026-11-11 check:** Zed's `gpui` crate is still frozen at 0.2.2 (last publish 2025-10-22,
  nothing newer in a year). However, **[GPUI-CE](https://github.com/gpui-ce/gpui-ce)** — an
  independently maintained community fork — released 0.3.0 on 2026-10-20 and states that an
  **accessibility tree ships in the core crate** (plus animation primitives). That targets our
  Track 2/3 gaps directly. Action: evaluate `gpui-ce` 0.3.0 as a potential swap for
  `gpui = "=0.2.2"` — verify the a11y claim per platform (Windows UIA / macOS NSAccessibility /
  Linux AT-SPI), check API breakage against `native/src`, and confirm the fork's maintenance
  cadence before committing. Not a 1.0 gate; a post-1.0 upgrade candidate.


## Track 1 — Platform verification (GUI-level) — OPEN (the 1.0 blocker)
- [x] Cross-platform CI: hosts build and unit-test on Windows/macOS/Linux (`.github/workflows/platform-validation.yml`).
- [x] Imported Electron sample e2e ran on Windows in CI (`scripts/e2e-windows.mjs`, page-timeout fixed). **Note:** that CDP-based webview-era script was deleted with the WebView renderer; the renderer-agnostic `scripts/smoke-packaged.*` replaces the coverage.
- [ ] Run the imported Electron sample e2e on macOS/Linux (currently Windows only).
- [ ] Add a smoke test that launches the packaged counter and asserts a rendered frame.
- [ ] Verify macOS/Linux hosts actually run (not just build).

## Track 2 — Native motion — DECIDED, documented
- [x] Decision: CSS transitions/animations/transforms stay unsupported in-tree; see [NATIVE_MOTION.md](NATIVE_MOTION.md) (GPUI 0.2.2 has no element transform primitive; the webview-era workaround was removed with the WebView renderer; Rendrix M3 plans rAF-driven animation).
- [x] Warnings are explicit and actionable (unsupported → points at the Rendrix animation plan).

## Track 3 — Accessibility — DECIDED, documented
- [x] Decision documented in [ACCESSIBILITY_PLAN.md](ACCESSIBILITY_PLAN.md): no OS accessibility tree in the native (GPUI 0.2.2) renderer; `aria-*`/`role` metadata is preserved and `aria-label` is used as a visible fallback. (Historical: the plan recommended the WebView renderer for screen readers; that renderer was removed. Rendrix M3 adds an accesskit tree.)

## Track 4 — Native input — IMPLEMENTED
- [x] Multi-line `<textarea>`, mouse/keyboard selection, clipboard integration, undo/redo, IME composition, word-wise navigation (see ARCHITECTURE.md).
- Remaining polish (optional for 1.0): closer text-editing edge cases (the webview-era browser comparison baseline is gone).

## Track 5 — Media — DECIDED, documented
- [x] Decision documented in [SUPPORTED_SYNTAX.md](SUPPORTED_SYNTAX.md): `<video>`, `<audio>`, `<iframe>` and `<canvas>` are unsupported and fail compilation on both renderers (`Unsupported HTML element <video>`); media playback goes through the system browser or an external player. Native media decoding is out of scope for 1.0.

## Track 6 — Electron compatibility closure — DONE (ledger current)
- [x] Every Electron 33 export classified implemented/partial/unsupported with a test that fails when the ledger is out of date (`electron-compat/coverage.mjs`).
- [x] Tray, nativeImage, protocol/session, power/display, autoUpdater shims implemented.
- Remaining items that need GPUI/platform upgrades are documented with reasons in [ELECTRON_COMPAT_CLOSURE.md](ELECTRON_COMPAT_CLOSURE.md) section (b); permanently unsupported APIs with reasons in section (c).

## Track 7 — Signing & distribution — DONE (with a documented 1.x descope)
- [x] Built-in cross-platform signing: `rendrix sign` (Windows signtool auto-detect, macOS codesign/notarize, Linux gpg).
- [x] In-app update client: configured feed check, `update-available`/`update-not-available`/`update-downloaded` events, host-side download with SHA-256 verification into `userData/updates`, and a real install on `quitAndInstall` — the host renames the running executable to `<exe>.old`, copies the staged artifact over it, and relaunches when asked (`native/src/updater.rs`).
- Descope decision (2026-11-11, same pattern as Tracks 2/3/5): `download-progress`, cancellation, retries, and signed update manifests are **1.x work**, not 1.0 gates. Reasons: the update artifact is already hash-verified end to end by the host, the swap is confined to `userData/updates`, and progress/cancel are UX polish rather than correctness. The descope is recorded in [ELECTRON_COMPAT_CLOSURE.md](ELECTRON_COMPAT_CLOSURE.md) and the coverage ledger.

## Track 8 — Performance — NOT STARTED
- [ ] RAM/startup benchmarks published with methodology.

## Track 9 — Host verification — PARTIAL
- [x] Release hosts get build-provenance attestations (`release-hosts.yml`, `attest-build-provenance`) and `hosts.json` SHA-256 verification on download.
- [ ] Document and test the `gh attestation verify` flow end to end (mentioned in the CLI README, but never exercised).

## Definition of "1.0"
All of Track 1 + 6 + 7 done, plus an explicit supported/unsupported decision for Tracks 2, 3, 4, 5 (documented, not necessarily implemented).

**Current standing:** Tracks 2, 3, 4, 5 are all resolved (decided and documented). Track 6 is done. Track 7 is done except installer-grade update polish. **Track 1 is the blocker**; Tracks 8 and 9 are small, independent add-ons.

## The plan (three sprints, no new features)

1.0 is a truthfulness release, not a feature release. The gate is: every claim in the ledger
is true, every platform we name is verified, and the version says 1.0.0.

### Sprint 1 — Truth pass (all doable from the Windows machine)
1. Fix the `quitAndInstall` gap: ledger and docs stop claiming install; either implement the
   artifact swap (preferred, ~a day) or reclassify `autoUpdater` as partial with the honest
   reason.
2. Track 7 decision: descope `download-progress`/cancellation/retries/signed manifests to 1.x
   with reasons in the closure doc (same pattern as Tracks 2/3/5).
3. Track 9: write the `gh attestation verify` doc page and add a CI step that runs it against
   the latest release artifacts.
4. Packaged smoke test: script that builds the counter, launches the packaged exe, asserts a
   rendered frame; wire into `platform-validation.yml` (Windows first).
5. Track 8: benchmark script (startup-to-first-frame, idle RSS) + methodology doc. Published
   numbers, flattering or not.

Exit: no ledger/doc claim that the code contradicts; smoke test green in CI on Windows.

### Sprint 2 — Platform truth (the unknown-risk sprint)
1. Add macOS and Linux legs to the e2e: run the imported sample through the renderer-agnostic smoke script (`scripts/smoke-packaged.*`; the webview host is removed) on a macOS GUI runner and Linux with `xvfb-run`. Budget this sprint for fixing what surfaces —
   the macOS fix history suggests runtime surprises live here.
2. macOS/Linux packaged runs recorded in VERIFICATION.md with dates and runner versions.
3. Update CHANGELOG "Known limits" to match reality (stale since beta.2).

Exit: Track 1 closed; the "built by CI but never run by hand" caveat is gone.

### Sprint 3 — Graduation
1. Bump to 1.0.0; remove "experimental" from the README's first sentence and VERIFICATION.md.
2. Final CHANGELOG, sync website docs, tag v1.0.0.
3. `release-hosts.yml` attaches hosts + attestations; run the new Track 9 verify flow against
   the published release — that run is the closing evidence.
4. Discord announcement (pattern already established for v0.4.0).

### Explicit cut line for 1.0 (post-1.0 backlog)
- Native multi-window / `window.create` (documented boundary instead)
- OS tray chrome and OS global shortcut delivery (ledger stays partial)
- `gpui-ce` 0.3.0 evaluation and any renderer upgrade work
- Update progress/cancel/retries if Sprint 1 descopes them

Rule: nothing new enters scope after Sprint 1 starts. If something in Sprint 2 turns out to be
bigger than one sprint, 1.0 ships with the platform marked verified-partial in VERIFICATION.md
rather than slipping the date indefinitely.
