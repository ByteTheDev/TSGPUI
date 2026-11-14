# Rendrix — the Rendrix rendering engine (decision record + plan)

Original decision (2026-11-11): Rendrix builds its **own GPU renderer** that consumes the same
compiled document IR the GPUI renderer eats today (`package::Node` — a resolved style tree),
with the Electron-compat runtime on top. Rewritten the same day by
[REWRITE_RENDRIX.md](REWRITE_RENDRIX.md): **Rendrix becomes the sole rendering engine** and the
product renames from Rendrix to Rendrix. The renderer key is `"rendrix"` (default); the legacy host
is deprecated and is removed at the Rendrix M1 gate.

## Why the assembly, and what is genuinely ours

No renderer is written from absolute zero — Electron bundles Chromium; Zed spent years on
GPUI. The commodity parts we assemble (and would never rewrite):

| Concern    | Crate         | Why |
|------------|---------------|-----|
| Windowing  | `winit`       | Cross-platform windows, events, DPI |
| GPU        | `wgpu`        | Vulkan/Metal/DX12 abstraction |
| Layout     | `taffy`       | Spec-tested flexbox/grid |
| Text       | `cosmic-text` | Shaping, bidi, font fallback |
| A11y       | `accesskit`   | UIA/NSAccessibility/AT-SPI bridges |

**Ours, written by us:** the paint pipeline (draw lists → GPU), the mapping from Rendrix's CSS
subset to layout/paint, input + hit testing, text editing state, and the bridge that keeps the
QuickJS backend, permission model, and Electron-compat runtime working on top. That is the
renderer. Everything Rendrix-specific stays ours; nothing else is re-derived.

Known deviation from the original brief: text shaping uses **cosmic-text** (already
implemented, tested, and by construction the same Linebender stack family as Parley) instead
of raw Parley. Re-evaluation is a later refactor, not a blocker.

## Architecture

```
.tgp bundle ──► package::load ──► Node tree (resolved styles)
                                    │
                                    ▼
   gpu::backend (doc → taffy tree → paint list)   ← state changes re-drive this
        │                 │
        ▼                 ▼
   gpu::layout+text   gpu::paint (draw list → wgpu)
        │                 │
        └────► gpu::surface (winit event loop, frame pacing, DPI)
                    │
                    ▼
             gpu::input (hit test → backend action dispatch)
```

Module ownership (one agent each; `types.rs` and `gpu_main.rs` are shared contracts). The
internal module directory is still `native/src/gpu/` — Phase B renames it and extracts it into
an independent, open-source `rendrix` crate with its own document types; the host adapts
`package::Node` into them:

- `gpu/types.rs` — shared draw-list types (written once, changed only with everyone's consent)
- `gpu/surface.rs` — winit event loop, wgpu device/swapchain, resize, frame pacing, DPI scale
- `gpu/paint.rs` — one quad shader + rounded-rect/border/clip/scissor encoding from `PaintFrame`
- `gpu/layout.rs` — `Node`/style map → taffy tree → absolute rects
- `gpu/text.rs` — cosmic-text shaping, glyph cache, measurement, caret/selection geometry
- `gpu/backend.rs` — glues document → layout → paint; re-layouts on state change
- `gpu/input.rs` — pointer/keyboard hit testing, button/checkbox/textarea state machines
- `gpu_main.rs` — the `rendrix-host` binary shell (feature `rendrix`; package `rendrix-host`)

## Milestones (honest)

- **M0 (vertical slice, Windows):** window opens, GPU-cleared background, the counter example
  renders its static tree (rects + text + button), click dispatches a backend action, exit code
  0. `cargo check --features rendrix` green. **Done.**
- **M1:** state-driven re-render (bindings), hover/active/focus states, scrolling, images.
- **M2:** text input + textarea with caret/selection/clipboard via cosmic-text; select element.
- **M3:** accesskit tree, multi-window, per-display scale (free with winit), animations
  (rAF-driven interpolation of opacity/color/position — the thing GPUI 0.2.2 never gave us).
- **M4:** macOS/Linux verification on CI runners; renderer naming/rebrand decision. **Done —
  the engine is named Rendrix and the product is Rendrix.**

## Non-goals (deliberately)

CSS transforms are M3+; video is never (open it in the system browser); full CSS selectors stay
in the compiler. With the WebView renderer removed, Rendrix owns everything in-tree; the
deprecated legacy host is the temporary fallback for anything Rendrix has not caught up with,
until the M1 gate.

## Risks, stated plainly

This is months of work to reach the coverage the removed WebView renderer gave users, and more
beyond it. M0 does not make the product; it makes the direction testable. The deprecated GPUI
host stays in the tree until Rendrix passes the M1 gate, then it is deleted — not before.
