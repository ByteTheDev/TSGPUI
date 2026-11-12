# Native CSS motion: current state and feasibility

> **Update (2026-11-11, Rendrix rewrite):** the WebView renderer was removed, so the
> "use `renderer: "webview"`" guidance below no longer works — it is a hard error. CSS
> `transition`, `animation`, and `transform` are unsupported in every in-tree engine today.
> The Rendrix roadmap (M3) plans rAF-driven interpolation of opacity, color, and position
> ([RENDERER_TSGPU.md](RENDERER_TSGPU.md)). The GPUI investigation below is kept as the
> record of why no native path was chosen for GPUI.

This note records the investigation of CSS motion support on TS-GPU's native
renderer (`renderer: "native"`): transitions, animations, and transforms. It
complements [`SUPPORTED_SYNTAX.md`](./SUPPORTED_SYNTAX.md) and
[`ARCHITECTURE.md`](./ARCHITECTURE.md).

## Summary

- The native renderer does **not** support CSS `transition`, `animation`, or
  `transform`. The CSS compiler reports all three as unsupported (warnings in
  lenient mode, errors under `strictCss`), and the native host never applies
  them.
- The gap is not a missing parser or a small rendering fix: GPUI 0.2.2 exposes
  **no element-level transform primitive** (rotation, scale, or translate on a
  `div`). The only transform API in the crate is the SVG element's transform.
- GPUI *does* ship animation primitives (`Animation`, `AnimationExt`,
  `with_animation`, `with_spring`), but they animate properties that GPUI
  elements actually have (position via insets, opacity, colors). They cannot
  produce a CSS `transform`.
- No newer GPUI version available in the registry or on crates.io exposes an
  element transform. Upgrading would be a large, API-breaking migration that
  would not close this gap.
- **Recommendation (historical): keep CSS motion out of the GPUI path.** Do not attempt to implement
  native CSS transitions/animations/transforms on GPUI. Before the rewrite this pointed at
  `renderer: "webview"`, which is now a hard error; motion is unsupported in-tree until Rendrix M3.

## Current state

### What the native renderer does

- `packages/cli/src/css.mjs` lists `transition`, `transform`, and `animation`
  in its `UNSUPPORTED` set. `normalizeDeclaration` throws
  `'${prop}' is not supported yet` for each, which the compiler turns into a
  warning (or an error in strict mode). The declaration is skipped; the rest of
  the rule still applies.
- `native/src/view/rendering.rs` and `native/src/view/layout.rs` apply the
  supported subset (layout, paint, opacity, shadows, borders, etc.) to GPUI
  `div`/`img` elements. There is no code path that reads or applies
  `transition`, `transform`, or `animation`.
- `docs/SUPPORTED_SYNTAX.md` and the website copy document these as "Known but
  not supported yet".

### What GPUI 0.2.2 actually provides

Verified against the cached crate at
`~/.cargo/registry/src/.../gpui-0.2.2`:

- `src/elements/animation.rs`: `Animation`, `AnimationExt` (`with_animation`,
  `with_animations`, `with_spring`), `AnimationElement`, `SpringAnimationElement`,
  easing helpers. These re-render an element with an animator closure and call
  `window.request_animation_frame()`.
- `src/styled.rs` / `src/style.rs`: no `transform`, `rotation`, `scale`, or
  `translate` property on `Styled`/`StyleRefinement`. The only transform API is
  `src/elements/svg.rs` (`Svg` transform).
- `src/elements/div.rs`: no transform method.

So GPUI can drive a render loop and animate values, but only into properties the
element model exposes. A CSS `transform: rotate(...)`/`scale(...)`/`translate(...)`
has nowhere to land.

### Why a partial shim is not worth it

A `translate` could in principle be approximated by changing `left`/`top`
insets, but that mutates layout (and affects siblings and hit-testing) rather
than painting, and would not match CSS semantics (transforms do not affect
layout, create stacking contexts, or change the containing block). `rotate` and
`scale` have no approximation at all. Implementing a correct subset would still
leave the common case (`transform: translateX(...)`) behaving subtly wrong, and
would widen the "supported" claim beyond what is actually correct. We therefore
keep all three properties unsupported.

## GPUI version constraints

- Pinned in `native/Cargo.toml`: `gpui = { version = "=0.2.2", optional = true }`,
  behind the `native-host` feature. The `=` pin is deliberate: GPUI's API is
  unstable and a minor bump can break the host.
- Edition 2024, Rust 1.98 available in this environment.
- The host also depends on `zed-reqwest`, `rquickjs`, `notify-rust`, etc. that
  are version-locked to the 0.2.2-era GPUI ecosystem.

## Upgrade feasibility

Versions found in the local registry cache and via `cargo search` (network
available):

| Crate | Version | Element transform? | Notes |
|---|---|---|---|
| `gpui` (Zed) | 0.2.2 (only published) | No (SVG only) | Current pin. |
| `gpui-pre` (Zed pre-release) | 0.3.10 | No (SVG only) | Adds synced animations, springs, reduce-motion handling. Still no `div` transform. API differs (crate name `gpui-pre`, extra sub-crates). |
| `gpui-ce` (community fork) | 0.3.0 | No (SVG only) | Requires Rust 1.95+. Different crate family (`gpui_ce_*`), Apache-2.0. Still no element transform. |
| `gpui-base` | 0.9.1 | No `.transform()` method found | Different, newer API lineage; not a drop-in. |

Additional checks:

- Searched the entire cargo registry source cache for a `.transform(` method on
  elements: **zero** matches outside of SVG.
- Searched for `struct Transform` / `enum Transform` / `Transform` traits in
  `gpui*` crates: only `Svg`'s transform type.

Conclusion: **no available GPUI version provides the primitive we need.**
Upgrading to `gpui-pre` or `gpui-ce` would be a substantial migration (crate
rename, feature flags, module paths, dependency ecosystem) and would at best
gain animation conveniences we already have equivalents for, while leaving
transforms unsupported. It is not justified for motion alone.

## Recommendation

1. **Keep CSS motion out of the GPUI path.** GPUI cannot express it (see above). The historical
   "supported path for motion" — `renderer: "webview"` with WebView2 / WKWebView / WebKitGTK —
   was removed with the WebView renderer and is now a hard error.
2. **Do not implement native CSS motion on GPUI now.** The cost (custom animation loop,
   property interpolation, paint-time transform, stacking-context changes) is
   large and the primitive is absent from GPUI.
3. **Deliver motion through the Rendrix roadmap instead.** M3 plans rAF-driven interpolation of
   opacity/color/position. Track this in
   `docs/ROADMAP_TO_1.0.md` item 3.
4. **Small improvement made:** the CSS compiler's unsupported-property warning
   for `transition`, `transform`, and `animation` now names the cause and points
   at the Rendrix animation plan (`packages/cli/src/css.mjs`), so users hitting this
   in `tsgpu dev`/`build` get an actionable message. Other unsupported
   properties keep the shorter message. Existing tests still match the
   `'...' is not supported yet` prefix.

## How to use motion today

You cannot, on any in-tree engine. The webview-era instructions (setting
`{ "renderer": "webview" }` in `tsgpu.config.json` and relying on the platform
browser engine, as in `examples/electron-sample`) are historical — that renderer
was removed and the key now fails the build. Until Rendrix M3 ships animations,
CSS `transition`, `animation`, and `transform` remain unsupported.
