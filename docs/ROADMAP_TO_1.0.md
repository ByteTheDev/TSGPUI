# Roadmap to 1.0 (out of experimental)

Tracking work to graduate TSGPUI from experimental. Each track is independently shippable.

## Track 1 — Platform verification (GUI-level)
- Run the imported Electron sample e2e on Windows/macOS/Linux in CI and fix the page-timeout.
- Add a smoke test that launches the packaged counter and asserts a rendered frame.
- Verify macOS/Linux hosts actually run (not just build).

## Track 2 — Native motion
- Upgrade GPUI to a version with transform/animation primitives, OR implement CSS transitions/animations/transforms on the native path.
- Until then, keep the explicit "unsupported, use WebView" stance and make warnings actionable.

## Track 3 — Accessibility
- Expose native aria-* / role metadata to the OS accessibility tree (or document that screen readers are unsupported).
- Decide and document the supported level.

## Track 4 — Native input
- Rich text input (multi-line, selection, clipboard integration).
- Better text editing parity with WebView.

## Track 5 — Media
- Video element support (or document unsupported and recommend WebView).

## Track 6 — Electron compatibility closure
- Close remaining partial APIs: tray, global shortcuts, nativeImage decoding, cursor position, display events.
- Either implement or move to "permanently unsupported" with reasons in the ledger.

## Track 7 — Signing & distribution
- Built-in signing/notarization pipeline (or a tested, documented one).
- In-app update feed (or keep manual and document).

## Track 8 — Performance
- RAM/startup benchmarks published with methodology.

## Track 9 — Host verification
- Document and test `gh attestation verify` flow.

## Definition of "1.0"
All of Track 1 + 6 + 7 done, plus an explicit supported/unsupported decision for Tracks 2, 3, 4, 5 (documented, not necessarily implemented).
