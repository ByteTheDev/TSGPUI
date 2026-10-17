# Supported HTML/CSS/TypeScript subset (v0.1)

**This is not Chromium.** Unsupported syntax causes an explicit compile error.

## HTML

Elements: `html`, `body`, `main`, `header`, `footer`, `section`, `article`, `div`, `h1`, `h2`, `h3`, `p`, `span`, `strong`, `small`, `button`, `label`.

Attributes: `id`, `class`, `title`, `data-onclick` (**button only**), `data-bind`.

- `data-onclick="actionName"` dispatches a synchronous backend action.
- `data-bind="stateKey"` renders a value from backend state.
- Text also supports `{{stateKey}}` interpolation.
- Use properly nested, explicitly closed tags. No void elements, scripts, DOM events, ARIA or HTML forms yet.

## CSS

Selectors: element, `.class`, `#id`, compounds (`button.primary`), comma lists and whitespace descendants (`.card h1`). Specificity and declaration order are respected. No pseudo-classes, combinators beyond descendants, attribute selectors or media queries.

Supported properties:

| Property | Values |
|---|---|
| `display` | `flex`, `block` (both rendered as native layout containers) |
| `flex-direction` | `row`, `column` |
| `align-items` | `center`, `flex-start`, `flex-end`, `stretch` |
| `justify-content` | `center`, `flex-start`, `flex-end`, `space-between`, `space-around` |
| `gap`, `padding`, `margin`, `border-radius` | single nonnegative `px` value |
| `width`, `height`, `font-size` | single nonnegative `px` value |
| `background`, `background-color`, `color` | `#RGB`, `#RRGGBB` |
| `font-weight` | `normal`, `bold`, `400`, `500`, `600`, `700` |

No shorthands with multiple values, gradients, shadows, grid, transitions, keyframe animations or typography beyond the subset.

## Backend TypeScript

Single `backend.ts` with a `default export` of `{ state: {...}, actions: { fn(api) {...} } }`.
Type-only imports allowed. Ordinary runtime imports, `require` and dynamic imports rejected.

QuickJS runs JS inside the GPUI process. It does not expose Bun/Node APIs, DOM or browser APIs. Actions should be synchronous and quick. State must be JSON serializable.
