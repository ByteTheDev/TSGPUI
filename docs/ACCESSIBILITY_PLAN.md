# Accessibility plan

## Status

**Not implemented.** Rendrix draws its own pixels, so assistive technology cannot see the interface:
there is no accessibility tree, no screen-reader announcements, no high-contrast or reduced-motion
adaptation (`prefers-reduced-motion` always reports `no-preference`). Keyboard use works (Tab order,
Enter/Space activation, text editing, caret and selection) and focus rings are drawn through normal CSS
`:focus` / `outline`.

## Plan

1. **Tree.** Build an [AccessKit](https://github.com/AccessKit/accesskit) tree from the DOM during
   `Engine::update`: role from tag/`role`, name from `aria-label`/`aria-labelledby`/`<label>`/text content,
   state from `disabled`, `checked`, `aria-*`, value/selection for text controls, bounds from layout. The
   box tree already carries the node ids needed to map layout to DOM.
2. **Platform bridge.** Hand the tree to `accesskit_winit` (UIA on Windows, NSAccessibility on macOS,
   AT-SPI on Linux) and route AT actions (focus, click, set value, scroll) into the same `Session` entry
   points real input uses.
3. **Preferences.** Query OS settings for reduced motion, contrast and text scale and expose them through
   `matchMedia`.
4. **Verification.** Narrator / NVDA on Windows, VoiceOver on macOS, Orca on Linux, plus automated tests
   that assert the tree for the example apps. No claim of support will be made before those pass.
