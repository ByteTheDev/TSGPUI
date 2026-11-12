# The TSGPU renderer — building our own (decision record + plan)

Decision (2026-11-11): TSGPUI builds its **own GPU renderer** ("tsgpu") that consumes the same
compiled document IR the GPUI renderer eats today (`package::Node` — a resolved style tree),
with the Electron-compat runtime on top. It replaces the GPUI native renderer as the native
path. WebView remains the compatibility renderer.

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

**Ours, written by us:** the paint pipeline (draw lists → GPU), the mapping from TSGPUI's CSS
subset to layout/paint, input + hit testing, text editing state, and the bridge that keeps the
QuickJS backend, permission model, and Electron-compat runtime working on top. That is the
renderer. Everything TSGPUI-specific stays ours; nothing else is re-derived.

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

Module ownership (one agent each; `types.rs` and `gpu_main.rs` are shared contracts):

- `gpu/types.rs` — shared draw-list types (written once, changed only with everyone's consent)
- `gpu/surface.rs` — winit event loop, wgpu device/swapchain, resize, frame pacing, DPI scale
- `gpu/paint.rs` — one quad shader + rounded-rect/border/clip/scissor encoding from `PaintFrame`
- `gpu/layout.rs` — `Node`/style map → taffy tree → absolute rects
- `gpu/text.rs` — cosmic-text shaping, glyph cache, measurement, caret/selection geometry
- `gpu/backend.rs` — glues document → layout → paint; re-layouts on state change
- `gpu/input.rs` — pointer/keyboard hit testing, button/checkbox/textarea state machines
- `gpu_main.rs` — the `tsgpui-gpu-host` binary shell

## Milestones (honest)

- **M0 (vertical slice, Windows):** window opens, GPU-cleared background, the counter example
  renders its static tree (rects + text + button), click dispatches a backend action, exit code
  0. `cargo check --features gpu-host` green.
- **M1:** state-driven re-render (bindings), hover/active/focus states, scrolling, images.
- **M2:** text input + textarea with caret/selection/clipboard via cosmic-text; select element.
- **M3:** accesskit tree, multi-window, per-display scale (free with winit), animations
  (rAF-driven interpolation of opacity/color/position — the thing GPUI 0.2.2 never gave us).
- **M4:** macOS/Linux verification on CI runners; renderer naming/rebrand decision (TSGPU).

## Non-goals (deliberately)

CSS transforms are M3+; video is never (use WebView); full CSS selectors stay in the compiler.
The webview renderer keeps owning anything the GPU path has not caught up with — that is the
compatibility floor while tsgpu matures.

## Risks, stated plainly

This is months of work to reach parity with what webview gives users today. M0 does not make
the product; it makes the direction testable. The existing GPUI renderer stays in the tree
until tsgpu passes it, then it is deleted — not before.
