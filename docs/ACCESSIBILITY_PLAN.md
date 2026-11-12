# Native Accessibility Plan

## Status

**Unsupported in the native (GPUI 0.2.2) renderer.** TS-GPU preserves HTML
`aria-*` and `role` metadata and uses `aria-label` as a visible fallback for
unlabeled controls, but it does **not** expose an OS accessibility tree. Screen
readers cannot see native-rendered UI.

**Assistive-technology support:** the removed WebView renderer used to provide the platform
browser's accessibility behavior (`tsgpu-webview-host`); it is gone, so there is no AT path
today. The Rendrix roadmap (M3) adds an accesskit tree — see
[RENDERER_TSGPU.md](RENDERER_TSGPU.md) and [REWRITE_RENDRIX.md](REWRITE_RENDRIX.md).

## Audit findings

### 1. `native/src/accessibility.rs`

- `AriaMetadata` parses `role`, `aria-label`, `aria-describedby` /
  `aria-description`, `aria-hidden`, `aria-live`, `aria-checked`,
  `aria-disabled`/`disabled`, and `aria-valuenow`.
- `effective_role()` returns an explicit `role` or an implicit role derived from
  the tag (`implicit_role()`).
- `effective_label()` returns `aria-label`, falling back to `title` then `alt`.
- `apply()` is an intentional no-op.

These helpers are pure data extraction. `effective_role`, `effective_label`, and
`implicit_role` are not wired to any OS API because GPUI 0.2.2 offers none.

### 2. Rendering flow (`native/src/view/rendering.rs`)

- `render_node()` builds an `AriaMetadata` from every node and calls
  `accessibility::apply()`, which does nothing.
- `aria-label` is used as a visible text fallback for empty buttons and
  checkboxes (so unlabeled controls still show a label on screen).
- No other `aria-*` attribute affects rendering, focus, or OS semantics.

### 3. GPUI capability check (`native/Cargo.toml`)

- Pinned dependency: `gpui = "=0.2.2"`.
- GPUI 0.2.2 features (`default`, `wayland`, `x11`, `windows-manifest`, …)
  contain **no accessibility feature**.
- Source audit of the `gpui-0.2.2` crate found no `accessib*` / `AccessKit` /
  `Role` / `aria_*` API on `Element`, `Div`, `Window`, or `App`. The only
  related symbols are `titlebar_double_click` and font scaling comments.

Conclusion: **GPUI 0.2.2 cannot expose role/label/value/state to the OS.** There
is no supported hook to implement a bridge against without changing the GPUI
version.

### 4. Newer GPUI (out of scope for this pin)

The registry contains `gpui-pre-0.3.10`, whose `examples/a11y.rs` demonstrates a
real AccessKit surface: `div().role(Role::…)`, `.aria_label(...)`,
`.aria_numeric_value(...)`, `.aria_min_numeric_value(...)`,
`.on_a11y_action(...)`, plus `Role`/`Toggled`/`AccessibleAction` exports. That
is a viable migration target, but it is a **major dependency upgrade** with API
changes across rendering, window creation, and application startup. It is not a
small or safe change and is deliberately not attempted here.

## Exact gap

To deliver native screen-reader support we need, at minimum, an upstream GPUI
release that exposes:

1. An element-level accessibility role (`role` / `a11y_role`).
2. Accessible name/description (`aria_label`, `aria_description`).
3. State and value (`aria_checked`, `aria_disabled`, `aria_valuenow`,
   `aria_valuemin`, `aria_valuemax`, `aria_live`).
4. Focus/selection integration and accessible actions (`on_a11y_action`).
5. A window/application-level accessibility adapter (AccessKit tree updates).

GPUI 0.2.2 provides none of (1)–(5). `AriaMetadata` and `apply()` already
isolate the mapping point, so a future upgrade only needs to implement
`apply()` against the new GPUI API and call it from `render_node()` (already
wired).

## What was changed in this pass

- `native/src/view/rendering.rs`: unlabeled-button and checkbox fallbacks now
  use `AriaMetadata::effective_label()` instead of ad-hoc attribute lookups.
  This centralizes label resolution (aria-label → title → alt) and exercises the
  previously-unused helper. No behavior change for existing bundles.
- No GPUI dependency or feature changes.
- No attempt to synthesize an OS accessibility tree (would be unsafe/incorrect
  without upstream support).

## Future work (ordered)

1. **Short term (done):** keep metadata + visible fallback labels; document
   limitation. (Historical: this plan recommended the WebView renderer for AT;
   that renderer was removed on 2026-11-11, so today no engine exposes an OS
   accessibility tree.)
2. **Medium term:** prototype on `gpui-pre` 0.3.x in a separate branch/worktree:
   map `AriaMetadata`/`implicit_role` to `Role`/`aria_*` in `accessibility.rs`,
   implement `apply()` to mutate the element builder, and add accessible actions
   for buttons/checkboxes/sliders. Validate on Windows (Narrator), macOS
   (VoiceOver), and Linux (Orca).
3. **Alternative (not recommended):** a standalone AccessKit adapter running
   beside the GPUI window. High complexity and platform-specific; only consider
   if the GPUI upgrade is blocked.
4. Update `docs/SUPPORTED_SYNTAX.md`, `docs/ARCHITECTURE.md`, and
   `website/docs/*` wording when support actually ships. Until then they correctly
   state "not yet announced as native screen-reader semantics."
