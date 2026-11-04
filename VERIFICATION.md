# Verification

TSGPUI is experimental and is not a production-ready SDK.

On 2026-11-04, `node --test packages/cli/test/*.test.mjs` reported 147 tests: 142 passed, 5 failed, 0 skipped. The failures were the CSP and file-map tests in `packages/cli/test/webview-bundle.test.mjs` and all three state tests in `packages/cli/test/webview-state.test.mjs`. An earlier run the same day reported 134 passing tests; the shared working tree changed between runs. These tests do not establish GUI behavior.

Windows host build and execution have been performed by hand; no application was launched for this documentation review. Other platform host runs, release workflows, signing, and performance were not verified in this review.
