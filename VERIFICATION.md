# Verification

Rendrix is experimental and is not a production-ready SDK.

On 2026-11-04, `node --test packages/cli/test/*.test.mjs` reported 173 tests: 173 passed, 0 failed, 0 skipped, on the current working tree. Earlier runs the same day reported 147 tests with 5 failing (the CSP and file-map tests in `packages/cli/test/webview-bundle.test.mjs` and the state tests in `packages/cli/test/webview-state.test.mjs`) and 134 passing; the shared working tree changed between runs. These tests do not establish GUI behavior.

Windows host build and execution have been performed by hand; no application was launched for this documentation review. Other platform host runs, release workflows, signing, and performance were not verified in this review.
