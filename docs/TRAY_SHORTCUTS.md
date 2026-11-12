# Tray icons and global shortcuts

TS-GPU backs Electron's `Tray` and `globalShortcut` with real OS objects instead of keeping them
in memory. The JS shims stay in
`packages/cli/src/electron-compat/runtime/electron-tray.ts`; everything platform-specific lives in
`native/src/tray.rs` and `native/src/hotkeys.rs`, so the Rendrix and GPUI hosts share
the same implementation.

Neither feature needs a permission grant. (Historical: the removed WebView renderer's bridge only
accepted `tray.*` and `globalShortcut.*` from the page that ran the app backend (the hidden
backend window when `backendWindow: "hidden"`, otherwise the primary page); renderer pages
could not watch keystrokes or add tray icons.)

## Tray

```ts
const { Tray, Menu, nativeImage } = require('electron');

const tray = new Tray(nativeImage.createFromPath('assets/tray.png'));
tray.setToolTip('Note Taker');
tray.setTitle('3');               // macOS menu-bar text
tray.setContextMenu(Menu.buildFromTemplate([
  { id: 'open', label: 'Open', click: () => window.show() },
  { type: 'separator' },
  { id: 'sound', label: 'Sounds', type: 'checkbox', checked: true, click: item => toggle(item.checked) },
  { id: 'quit', label: 'Quit', role: 'quit' },
]));
tray.on('click', (_event, bounds, position) => console.log(bounds, position));
```

### Images

`new Tray(image)` and `setImage`/`setPressedImage` accept:

* a `nativeImage` (the shim reads its PNG bytes),
* a `Uint8Array`/`ArrayBuffer` of PNG, JPEG, GIF or WebP bytes,
* a base64 data URL,
* a file path.

The shim sends the image as base64 or a path; the host decodes it once, stores it behind a handle
in `native/src/image_store.rs`, and reuses it when the OS asks for the icon again. Images larger
than 4096 × 4096 pixels are rejected and each stored image is cropped/fitted to a square icon by the
platform layer. `setPressedImage` is used by macOS as the alternate image; Windows and Linux ignore
it.

### Context menus

`setContextMenu` accepts a `Menu` object or a menu template. The shim serializes a snapshot:

```json
[{ "type": "normal", "label": "Open", "enabled": true, "visible": true,
   "checked": false, "itemId": 1, "submenu": [] }]
```

`itemId` is a stable command id for that snapshot. When the user chooses an item the host reports
`{ id, event: "menu-click", itemId }`; the shim looks the item up, toggles `checkbox`/`radio` state
before calling the handler (as Electron does), re-sends the menu when the check state changed, and
calls `MenuItem.click(menuItem, null, event)`.

Because the host renders a snapshot, mutating menu items after `setContextMenu` has no effect until
the menu is set again. This matches Electron's "call `setContextMenu` after changing the menu"
guidance.

### Events and state

| Event | Arguments |
| --- | --- |
| `click` | `(event, bounds, position)` on a left click |
| `right-click` | `(event, bounds, position)`, and opens the context menu on Windows |
| `double-click` | `(event, bounds, position)` unless `setIgnoreDoubleClickEvents(true)` |
| `destroyed` | emitted by `destroy()` |

`getBounds()` returns the last rectangle the host reported (Windows uses
`Shell_NotifyIconGetRect`; macOS and Linux report zeros because neither exposes a stable
screen-space rectangle through the APIs used here). `getToolTip`, `getTitle`, `getContextMenu`,
`isDestroyed`, `setIgnoreDoubleClickEvents` and `getIgnoreDoubleClickEvents` are local and
synchronous.

`destroy()` removes the OS object and emits `destroyed`. `popUpContextMenu(menu?)` shows the current
menu at the pointer on Windows and is a no-op on macOS and Linux (those shells open the menu on
their own); passing a menu first replaces the tray menu. `displayBalloon`, `removeBalloon` and
`focus` are accepted without native effects.

### Platform notes

| | Windows | macOS | Linux |
| --- | --- | --- | --- |
| API | `Shell_NotifyIcon` on a message-only window | `NSStatusItem` + `NSMenu` on the main thread | `StatusNotifierItem` + `com.canonical.dbusmenu` via `ksni` |
| Left click | `click` | `click` on the status button | `Activate` → `click` |
| Right click | `right-click` + menu | opens the menu | `SecondaryActivate` → `right-click` |
| Tooltip/title | tooltip (title is stored only) | tooltip and title | tooltip (`Title`/`ToolTip`) |
| Bounds | `Shell_NotifyIconGetRect` | zeros | zeros |
| Restart resilience | re-adds icons after `TaskbarCreated` | — | — |

macOS requires the tray to be created on the main thread. The host schedules every tray mutation on
the main dispatch queue, so the JS call returns immediately and the icon appears on the next run-loop
turn. Linux needs a StatusNotifier host (KDE Plasma, or GNOME with the AppIndicator extension) and a
session D-Bus. If either is missing, `tray.error` is reported and the app keeps running without an
icon.

On Windows the tray lives on its own message-loop thread. Menu callbacks and clicks are queued as
`tray.event` values and delivered to the backend thread, so a slow action never blocks the shell.

## Global shortcuts

```ts
const { globalShortcut } = require('electron');

const ok = globalShortcut.register('CommandOrControl+Shift+P', () => {
  window.show();
});
globalShortcut.registerAll(['Alt+F9', 'Alt+F10'], () => {});
globalShortcut.isRegistered('Alt+F9'); // true
globalShortcut.unregister('Alt+F9');
globalShortcut.unregisterAll();
```

### Accelerator syntax

* Modifiers: `CommandOrControl`/`CmdOrCtrl`, `Command`/`Cmd`/`Super`/`Meta`, `Control`/`Ctrl`,
  `Alt`/`Option`, `Shift`. `CommandOrControl` is Command on macOS and Control elsewhere. Modifiers
  are case-insensitive and may appear in any order.
* Keys: `A`–`Z`, `0`–`9`, `F1`–`F24`, `Space`, `Tab`, `Enter`/`Return`, `Escape`/`Esc`,
  `Backspace`, `Delete`, `Insert`, `Home`, `End`, `PageUp`, `PageDown`, `Up`/`Down`/`Left`/`Right`,
  `Plus`, `Minus`, `PrintScreen`, `Pause`, `NumLock`, `CapsLock`, `ScrollLock`, `Num0`–`Num9`,
  `MediaPlayPause`, `MediaNextTrack`, `MediaPreviousTrack`, `MediaStop`, `VolumeUp`, `VolumeDown`,
  `VolumeMute`, and the punctuation keys `= - , . / ; [ \ ] ' \``. `Ctrl++` is also accepted for
  the plus key.
* Exactly one non-modifier key is required. Unknown keys and malformed accelerators make
  `register()` throw and fail the host operation.

### Registration is asynchronous

Electron returns a boolean synchronously, but a global grab can only be attempted by the OS. The
shim therefore validates the syntax, records the accelerator, forwards the request, and returns
`true` optimistically. If the OS refuses the binding (another app owns it, a Wayland session has no
global grabs, macOS lacks Accessibility permission, …) the host sends:

```json
{ "channel": "globalShortcut.failed", "payload": { "accelerator": "Alt+F9", "error": "…" } }
```

The shim removes the accelerator from its registry and appends a `compatWarnings` entry, so
`isRegistered('Alt+F9')` becomes false shortly after registration. Registering the same accelerator
twice returns `false`, as in Electron.

When a registered hotkey fires the host sends
`{ channel: "globalShortcut.event", payload: { accelerator } }`, and the shim calls the registered
callback. Presses are delivered on the backend thread, so callbacks may update state, show windows
or send IPC.

### Platform notes

| | Windows | macOS | Linux |
| --- | --- | --- | --- |
| Mechanism | `RegisterHotKey` on a dedicated message-loop thread | listen-only `CGEventTap` on a dedicated CFRunLoop thread | `XGrabKey` on the X11 root window, polled every 5 ms |
| Requirements | none beyond a normal desktop session | Accessibility permission for the app | X11 (`DISPLAY`); Wayland sessions cannot grab global keys |
| Cleanup | `UnregisterHotKey` on unregister and thread exit | bindings are matched in the tap callback | `XUngrabKey` on unregister and layout changes |

macOS taps are listen-only: keys still reach the focused application. The tap matches the modifier
mask exactly, so `CommandOrControl+Shift+P` fires on Command+Shift+P and not on Command+P. Linux
grabs include the CapsLock/NumLock modifier variants and are re-installed after a keyboard-layout
change (`MappingNotify`).

## Host operations

| Operation | Arguments | Result |
| --- | --- | --- |
| `tray.create` | `{ id, image, toolTip?, title?, menu? }` | `{ id }` |
| `tray.setIcon` | `{ id, image, pressed? }` | `null` |
| `tray.setToolTip` | `{ id, toolTip }` | `null` |
| `tray.setTitle` | `{ id, title }` | `null` |
| `tray.setContextMenu` | `{ id, menu }` (array or `null`) | `null` |
| `tray.popUpContextMenu` | `{ id }` | `null` |
| `tray.destroy` | `{ id }` | `null` |
| `tray.bounds` | `{ id }` | `{ x, y, width, height }` |
| `globalShortcut.register` | `{ accelerator }` or the string | `true` |
| `globalShortcut.unregister` | `{ accelerator }` or the string | `null` |
| `globalShortcut.unregisterAll` | — | `null` |
| `globalShortcut.isRegistered` | `{ accelerator }` or the string | boolean |

The hosts expose them as `api.native.tray.*` and `api.native.globalShortcut.*`.
Both are wired in
`packages/cli/src/electron-compat/runtime/host.ts`.

Host events are delivered through one hook,
`globalThis.__tsgpu_host_event(channel, payload)`. The native runtime evaluates it as a task in the
backend VM (`native/src/runtime.rs`). (Historical: the removed WebView renderer evaluated it on
the backend page in `native/src/webview_main.rs`.)

## Code map

* `packages/cli/src/electron-compat/runtime/electron-tray.ts` — Electron-facing `Tray` and
  `globalShortcut`, menu serialization, event dispatch.
* `native/src/tray.rs` — tray model and the Windows/macOS/Linux implementations.
* `native/src/hotkeys.rs` — accelerator parser and the Windows/macOS/Linux hotkey services.
* `native/src/image_store.rs` — decoded tray images and handles.
* `native/src/capabilities.rs` — operation routing (`native/src/webview_bridge.rs` was removed with the WebView renderer).
* `native/src/runtime.rs` — native event delivery (`native/src/webview_main.rs` was removed with the WebView renderer).

## Troubleshooting

* **No icon on Linux** — check that a StatusNotifier host is running and `DBUS_SESSION_BUS_ADDRESS`
  is set. GNOME needs the AppIndicator extension.
* **No tray update on Windows** — `Shell_NotifyIcon` failures are reported as `tray.error`; check
  `compatWarnings`. Explorer restarts are handled through `TaskbarCreated`.
* **Hotkeys never fire on macOS** — grant Accessibility permission to the packaged app in System
  Settings → Privacy & Security → Accessibility, then restart it.
* **Hotkeys fail on Linux** — a Wayland session has no X11 root window. Log in to an X11 session or
  use a portal-based shortcut mechanism supplied by the desktop.
* **`register()` returned true but `isRegistered()` is false** — the OS refused the binding; look for
  the `globalShortcut.failed` compat warning.
