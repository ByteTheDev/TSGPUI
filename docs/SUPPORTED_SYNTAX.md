# Supported HTML, CSS, and TypeScript

The default native renderer compiles a documented subset to GPUI elements. Use the WebView renderer when you need platform browser layout and CSS behavior.

## Renderer selection

Set the project renderer in `tsgpui.config.json`:

```json
{
  "renderer": "webview"
}
```

`renderer` accepts `native` (the default) or `webview`. `tsgpui dev` and `tsgpui build` select the matching host automatically. Build one explicitly with `tsgpui host build --renderer webview` or `tsgpui host build --renderer native`.

The WebView renderer passes authored HTML and CSS to [Wry](https://github.com/tauri-apps/wry), which uses WebView2 on Windows, WKWebView on macOS, and WebKitGTK on Linux. Windows needs the WebView2 runtime; Linux builds need GTK and WebKitGTK development packages. It embeds referenced local images, scripts, stylesheets, CSS URLs, and configured fonts in the bundle, which has a 16 MiB size limit. The document is loaded from an app-local custom URL. A single HTML document is supported; relative links to other local pages are not packaged as a multi-page site.

It supports `data-bind`, `{{state.path}}` text interpolation, `data-onclick`, `data-oninput`, forms with `data-submit`, `data-fetch`, file selection/read/save, and clipboard bindings. The native renderer's `data-repeat`, `data-virtual`, compile-time components, native window controls, and per-element CSS warnings are not used in WebView mode. WebView state resets when `dev` reloads the document.

WebView pages execute ordinary page JavaScript in the browser context. Their scripts and backend code are trusted application code. `permissions.network` restricts the declarative `data-fetch` binding; it cannot restrict fetches or other network requests made by page scripts. WebView fetches follow browser CORS rules. The WebView host uses the system frame; the native `window.titlebar` and `window.backdrop` options do not apply.

The remaining HTML, CSS, and backend reference describes the native renderer unless a section says otherwise.

## HTML

Supported elements: `html`, `body`, `main`, `header`, `footer`, `nav`, `aside`, `section`, `article`, `div`, `h1` through `h6`, `p`, `span`, `strong`, `small`, `blockquote`, `pre`, `code`, `figure`, `figcaption`, `details`, `summary`, `table`, `caption`, `thead`, `tbody`, `tfoot`, `tr`, `th`, `td`, `button`, `label`, `input`, `textarea`, `select`, `option`, `form`, `ul`, `ol`, `li`, `img`, `menu`, `menuitem`, `hr`, `template`, `use`, and `slot`.

Semantic and table elements currently render through the native container layout; use CSS flex or grid to control their layout. The renderer does not yet provide browser table algorithms, disclosure behavior, or heading defaults.

Common attributes include `id`, `class`, `title`, `aria-label`, `role`, `tabindex`, `name`, and `value`. Controls also support `type`, `placeholder`, `checked`, `selected`, `disabled`, `required`, `min`, `max`, and `step`.

- `data-bind="stateKey"` displays backend state. Text supports `{{stateKey}}` interpolation. Dotted paths work for nested objects and repeated items.
- `data-onclick="actionName"` runs a button action. `data-oninput="actionName"` runs when an input, textarea, or select value changes. Actions receive an event object with `type` plus fields such as `value`, `checked`, `name`, and `submit`.
- `<form data-submit="save">` submits through a `button type="submit"`. Required fields and numeric `min`, `max`, and `step` constraints are checked before dispatch.
- Single-line text inputs support typing through the platform text input (including IME composition), a blinking caret, selection by mouse or Shift+arrow, Home/End, word-wise movement and deletion, Ctrl/Cmd+A/C/X/V/Z, Shift+Ctrl/Cmd+Z and Ctrl/Cmd+Y, Tab focus traversal, and password masking. Undo history is per field, bounded, and reset on app reload. Textareas use the same native text path: multi-line caret and selection, IME, clipboard, wrapping to the field width, scrolling by wheel and by caret, Enter for a new line, Up/Down by visual row, Home/End by line, Ctrl/Cmd+Home/End for the whole text, and double-click to select a word. The default height is about three lines unless the element sets `height`.
- A select cycles through its options on click and supports Up/Down keys. It does not use a native dropdown popup yet.
- `data-repeat="items as item"` renders one node for each array element in backend state. Use `{{item.name}}` for item values. Nested repeats and state-bound inputs use the array index in their state path. Add `data-virtual="36"` (a row height in pixels, from 1 to 1000) to build only the visible rows of a scrolling list. Every row must be exactly that height, and rows fill the list's width unless they set one. Without `data-virtual`, every row is built.
- `<template data-component="Card">...</template>` and `<use component="Card" data-prop-title="Hello"/>` expand static reusable components at compile time. A component can read string props with `{{props.title}}`. Recursive components and duplicate expanded IDs are rejected.

- Slots: `<slot></slot>` inside a template receives the children written between `<use>` tags. `<slot name="footer">Default</slot>` receives children marked `slot="footer"`. Text inside a slot is its fallback, used when the caller gives nothing. Content with no matching slot is an error. `<use .../>` with no children is self-closed, and `<use ...>...</use>` takes slot content.
- Attributes written on `<use>`, such as `class`, `data-bind`, `data-oninput`, `data-onclick`, `placeholder`, or `title`, go onto the component's first element. `class` names are merged. The element is checked again, so `data-onclick` on a component whose first element is not a `<button>` is an error.
- Slot content keeps the props of the place where it is written. `{{props.x}}` in content written at the top level is empty, and a nested component's own `{{props.*}}` text shows that component's props.
- Built-in library: `Sidebar`, `Toolbar` (slots `start` and `end`; `title` is the fallback text), `SegmentedControl`, `SearchField` (an `<input type="search">`, so `data-bind` and `data-oninput` go on the `<use>`), `ListRow` (`title`, with a default slot for detail text), and `Sheet` (`title`, with slots for the body and `footer`). Use them like any template. An app template with the same name replaces the library one. Library styles are plain `.ts-*` class selectors, and their colors are `--ts-*` custom properties, so `:root` restyles them.
- `<img src="assets/photo.png" alt="...">` embeds a project-relative PNG, JPEG, GIF, WebP, BMP, or SVG. Each image is limited to 8 MiB; bundles are limited to 16 MiB.
- `data-shortcut="mod-s"` binds a button action to a key. `mod` means Ctrl on Windows/Linux and Cmd on macOS. `ctrl`, `alt`, `shift`, and `cmd` can also be written explicitly.
- Native menus use `<menu data-app-menu="File">` for an application menu and `<menu id="edit">` for a context menu. Put `<menuitem data-onclick="actionName">` entries and `<hr>` separators inside. Menu items may contain nested menu items for submenus. Add `data-shortcut="mod-s"` to an item to bind its shortcut. Attach `data-contextmenu="edit"` to an element to show that context menu on right-click. Context menus support disabled entries; native application menu items cannot be disabled. Checked menu items are not supported.
- `data-ondrop="actionName"` on a rendered container or button dispatches a `drop` event with `files`, an array of `{name, path}` objects for the files the user dropped. The host reports paths only and does not read file contents. File drops are native-renderer behavior; browser preview and WebView support may differ by platform and renderer.
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

The permission schema also reserves explicit grants for the native bridge:

```json
{
  "permissions": {
    "filesystem": { "read": ["./data"], "write": ["./data"] },
    "processes": ["git"],
    "notifications": true,
    "ipc": true,
    "window": true
  }
}
```

These grants are serialized into the app bundle. Filesystem entries are project-relative directory roots, process entries are executable names, and notifications, IPC, and native window APIs are opt-in booleans. The TypeScript declarations expose the `api.native` contract; the host does not yet dispatch these calls. Existing picker-based file operations, window title-bar controls, and network permissions continue to use their current host paths.

ARIA and role attributes are accepted as metadata, and `aria-label` is shown as a visible fallback for unlabeled buttons and checkboxes. GPUI 0.2.2 does not expose an accessibility tree API, so these attributes are not yet announced as native screen-reader semantics. WebView mode uses the platform browser's accessibility behavior.

Tags must be properly nested and explicitly closed, except void tags such as `input` and `img`, and self-closed `<use .../>`. The document must have one root element.

## Window

`tsgpui.config.json` accepts `"window": { "width": 900, "height": 600, "titlebar": "system", "backdrop": "opaque" }`. `titlebar` is `system` (the OS title bar) or `unified` (the app draws its own). `backdrop` is `opaque` or `blur`.

With `"titlebar": "unified"`, mark the bar's drag area with `data-window-drag`. That element must not contain buttons, inputs, or other interactive elements, because Windows treats its whole area as the title bar, and clicks inside it would never reach the control. Put the window buttons beside the drag area, with `data-window-control="close"`, `"minimize"`, or `"maximize"`. A unified bar without a drag area warns that the window cannot be moved. Window buttons under the system title bar warn that they have no effect.

`"backdrop": "blur"` asks the window for a blurred backdrop. Windows applies it only on some systems, and the root background must be translucent for it to show, for example `body { background: #f5f5f799; }`.

## CSS

Selectors: element, `.class`, `#id`, `*`, compounds such as `button.primary`, comma lists, descendant (`.card h1`) and child (`.card > p`) combinators. Specificity, `!important`, and source order are respected.

State pseudo-classes: `:hover`, `:active`, `:focus`, and `:disabled`, on the last compound of a selector (`button.nav:hover`). They can change only `background`, `color`, `border-color`, `border-width`, `border-style`, `box-shadow`, `opacity`, and `cursor`. They do not work inside `@media`.

CSS variables: declare `--name: value` (in `:root` or any rule) and use `var(--name)` or `var(--name, fallback)`. Variables resolve at compile time, and a later declaration of the same name wins. The component library's colors are `--ts-*` variables, so `:root { --ts-accent: #ff375f; }` restyles every library component.

Units: `px`, `rem`, and `em` (both 16px), and `%` for sizes, insets, and padding and margin. `auto` works for `width`, `height`, `margin`, `flex-basis`, and insets.

In the native renderer, declarations GPUI cannot render are skipped with a warning that names the file, line, and property, and the rest of the rule still applies. Set `"strictCss": true` in `tsgpui.config.json` to turn those warnings into build errors. The WebView renderer sends the stylesheet to the platform browser engine instead.

| Property | Values |
|---|---|
| `display` | `flex`, `block`, `grid` |
| `flex-direction` | `row`, `column` |
| `flex`, `flex-grow`, `flex-shrink`, `flex-basis` | numbers, and lengths or `%` for the basis; `flex: 1`, `flex: none`, and `flex: auto` |
| `align-items` | `center`, `flex-start`, `flex-end`, `stretch` |
| `justify-content` | `center`, `flex-start`, `flex-end`, `space-between`, `space-around` |
| `gap`, `row-gap`, `column-gap`, `border-radius`, `font-size` | nonnegative lengths |
| `padding`, `padding-top`, `padding-right`, `padding-bottom`, `padding-left` | nonnegative lengths or `%`, one to four values |
| `margin`, `margin-top`, `margin-right`, `margin-bottom`, `margin-left` | lengths, `%`, or `auto`, one to four values, may be negative |
| `width`, `height`, `min-width`, `min-height`, `max-width`, `max-height` | lengths or `%`; `auto` and `none` where they apply |
| `position` | `relative`, `absolute` (`static` is treated as `relative`) |
| `top`, `right`, `bottom`, `left`, `inset` | lengths or `%` |
| `grid-template-columns`, `grid-template-rows` | up to 12 space-separated `fr`, `px`, or `auto` tracks |
| `overflow` | `visible`, `hidden`, `scroll`, `auto` |
| `opacity` | `0` through `1` |
| `background`, `background-color` | a color, or `linear-gradient(<deg>, <color>, <color>)` (exactly two stops; `to top/right/bottom/left` also work) |
| `color` | `#RGB`, `#RGBA`, `#RRGGBB`, `#RRGGBBAA`, `rgb()`, `rgba()`, `transparent`, `white`, `black` |
| `border`, `border-width`, `border-style`, `border-color` | solid borders with a width and a color; `none` removes the border. `dashed`, `dotted`, and the other styles are not supported |
| `border-top`, `border-right`, `border-bottom`, `border-left` and their `-width` and `-style` properties | solid per-edge borders. Per-edge colors are not supported because GPUI paints one border color per element |
| `box-shadow` | comma-separated `x y [blur] [spread] color` entries. `inset` is not supported |
| `text-align` | `left`, `center`, `right` (`start` and `end` map to left and right) |
| `line-height` | a length, or a unitless multiplier of `font-size` |
| `font-style` | `normal`, `italic` (`oblique` maps to italic) |
| `white-space` | `normal`, `nowrap` |
| `text-overflow` | `clip`, `ellipsis` (the ellipsis glyph is not drawn yet, so text is clipped) |
| `cursor` | `default`, `pointer`, `text`, `not-allowed`, `grab`, `move`, `crosshair` |
| `font-weight` | `normal`, `bold`, `400`, `500`, `600`, `700` |
| `font-family` | one family name (the first in a list; quotes are optional) |

Accepted with no effect: `box-sizing`, because boxes already size the way `border-box` does. Known but not supported yet (reported as warnings): `transition`, `animation`, `transform`, `filter`, `letter-spacing`, `z-index`, and others.

Custom properties with the same name in different places are global: the last declaration in source order wins.

Custom fonts are bundled with the app. Add entries to `fonts` in `tsgpui.config.json`, for example `{"family":"Inter","src":"assets/Inter-Regular.ttf"}`, then use `font-family: Inter` in CSS. TTF and OTF files up to 8 MiB each are supported. The CSS family must match the configured family name.

`@media` supports `min-width`, `max-width`, `min-height`, and `max-height` in pixels. Rules are reevaluated on window resize. Grid track counts are applied, but GPUI currently lays tracks out at equal sizes, so mixed `fr` and `px` dimensions are not exact.

## Browser preview

`tsgpui preview --project PATH` serves the same bundle in a browser at `http://127.0.0.1:4173/` (`--port` and `--no-open` are optional). It rebuilds on save and reloads the page, and the app's state carries over, merged as the native host merges it. `tsgpui preview --out FILE` writes one self-contained HTML file with no live reload.

The preview is a reference for the shared subset, not an exact match. Its layout follows the host's defaults (column boxes, border-box sizing, no button chrome), but text metrics, flex edge cases, and grid tracks differ. A `data-virtual` list builds every row, `data-window-*` attributes do nothing, file pickers use the browser's own dialogs, and `data-fetch` depends on the browser allowing the request. The backend runs in the page with the host's API and rollback rules, and with the browser's network and storage globals hidden. The page's content security policy permits script from the page itself, which the preview needs, and network requests only to the hosts in `permissions.network`.

## Backend TypeScript

One `backend.ts` file exports a default `{ state, actions }` object. Type-only imports are allowed; runtime imports, dynamic imports, and `require` are unsupported. In native mode, Node/Bun APIs and browser globals are not available. Actions receive `get`, `set`, and `update` plus a read-only event. Native actions may return a Promise; QuickJS microtasks are drained during dispatch before the view redraws, so long-running or externally pending promises are not supported. WebView mode executes the compiled backend script in the browser page and therefore uses the platform JavaScript engine and page security model.

The native backend has no general filesystem or `fetch` API. File access is limited to native picker events, and network access is limited to the declarative `data-fetch` control with an exact host allowlist. WebView pages use browser APIs and browser network rules.
