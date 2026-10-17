# Supported HTML, CSS, and TypeScript

TSGPUI compiles a documented subset to native GPUI elements. It is not Chromium and rejects browser syntax that the host cannot render.

## HTML

Supported elements: `html`, `body`, `main`, `header`, `footer`, `section`, `article`, `div`, `h1`, `h2`, `h3`, `p`, `span`, `strong`, `small`, `button`, `label`, `input`, `textarea`, `select`, `option`, `form`, `ul`, `ol`, `li`, `img`, `template`, and `use`.

Common attributes include `id`, `class`, `title`, `aria-label`, `role`, `tabindex`, `name`, and `value`. Controls also support `type`, `placeholder`, `checked`, `selected`, `disabled`, `required`, `min`, `max`, and `step`.

- `data-bind="stateKey"` displays backend state. Text supports `{{stateKey}}` interpolation. Dotted paths work for nested objects and repeated items.
- `data-onclick="actionName"` runs a button action. `data-oninput="actionName"` runs when an input, textarea, or select value changes. Actions receive an event object with `type` plus fields such as `value`, `checked`, `name`, and `submit`.
- `<form data-submit="save">` submits through a `button type="submit"`. Required fields and numeric `min`, `max`, and `step` constraints are checked before dispatch.
- Text inputs and textareas support typing, backspace/delete, arrow keys, Home/End, Ctrl/Cmd+A, Tab focus traversal, and password masking. They are rendered by the current GPUI host's lightweight text control; IME composition, a visible caret, and full native text selection are not available yet.
- A select cycles through its options on click and supports Up/Down keys. It does not use a native dropdown popup yet.
- `data-repeat="items as item"` renders one node for each array element in backend state. Use `{{item.name}}` for item values. Nested repeats and state-bound inputs use the array index in their state path. The current renderer builds all rows at once; long lists are not virtualized.
- `<template data-component="Card">...</template>` and `<use component="Card" data-prop-title="Hello"/>` expand static reusable components at compile time. A component can read string props with `{{props.title}}`. Recursive components and duplicate expanded IDs are rejected.
- `<img src="assets/photo.png" alt="...">` embeds a project-relative PNG, JPEG, GIF, WebP, BMP, or SVG. Each image is limited to 8 MiB; bundles are limited to 16 MiB.
- `data-shortcut="mod-s"` binds a button action to a key. `mod` means Ctrl on Windows/Linux and Cmd on macOS. `ctrl`, `alt`, `shift`, and `cmd` can also be written explicitly.
- `data-confirm="deleteItem"` shows a native confirmation prompt, then runs the action only when confirmed.
- `data-clipboard-copy="stateKey"` copies a state value from a button. `data-clipboard-paste="stateKey"` reads clipboard text into that state value after a button click.
- `data-file-open="pathKey"` opens the native file chooser and stores the selected path. Add `data-file-read="textKey"` to read the chosen UTF-8 text file into another state key. `data-file-save="textKey"` opens a native save dialog and writes that state's text. File reads and writes are limited to 1 MiB and require a user-selected path.
- `data-fetch="https://api.example.com/path" data-fetch-into="result"` performs a user-initiated HTTPS GET and stores its text response. The exact host must be listed in `permissions.network` in `tsgpui.config.json`. Redirects are disabled, requests time out after 15 seconds, and responses are limited to 1 MiB.

Example network permission:

```json
{
  "permissions": {
    "network": ["api.example.com"]
  }
}
```

ARIA and role attributes are accepted as metadata, and `aria-label` is shown as a visible fallback for unlabeled buttons and checkboxes. GPUI 0.2.2 does not expose an accessibility tree API, so these attributes are not yet announced as native screen-reader semantics.

Tags must be properly nested and explicitly closed, except void tags such as `input`, `img`, and `use`. The document must have one root element.

## CSS

Selectors: element, `.class`, `#id`, compounds such as `button.primary`, comma lists, and whitespace descendants such as `.card h1`. Specificity and source order are respected.

| Property | Values |
|---|---|
| `display` | `flex`, `block`, `grid` |
| `flex-direction` | `row`, `column` |
| `align-items` | `center`, `flex-start`, `flex-end`, `stretch` |
| `justify-content` | `center`, `flex-start`, `flex-end`, `space-between`, `space-around` |
| `gap`, `column-gap`, `row-gap`, `padding`, `margin`, `border-radius`, `width`, `height`, `font-size` | nonnegative `px` values |
| `grid-template-columns`, `grid-template-rows` | up to 12 space-separated `fr`, `px`, or `auto` tracks |
| `overflow` | `visible`, `hidden`, `scroll`, `auto` |
| `opacity` | `0` through `1` |
| `text-align` | `left`, `center`, `right` |
| `background`, `background-color`, `color` | `#RGB`, `#RRGGBB` |
| `font-weight` | `normal`, `bold`, `400`, `500`, `600`, `700` |

`@media` supports `min-width`, `max-width`, `min-height`, and `max-height` in pixels. Rules are reevaluated on window resize. Grid track counts are applied, but GPUI currently lays tracks out at equal sizes, so mixed `fr` and `px` dimensions are not exact.

## Backend TypeScript

One `backend.ts` file exports a default `{ state, actions }` object. Type-only imports are allowed; runtime imports, dynamic imports, `require`, Node/Bun APIs, and browser globals are not available. Actions receive `get`, `set`, and `update` plus a read-only event. Actions may return a Promise; QuickJS microtasks are drained during dispatch before the view redraws, so long-running or externally pending promises are not supported.

There is no general filesystem or `fetch` API in backend JavaScript. File access is limited to native picker events, and network access is limited to the declarative `data-fetch` control with an exact host allowlist.
