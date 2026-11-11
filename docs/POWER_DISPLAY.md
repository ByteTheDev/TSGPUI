# Power, Display, and System Preferences

This document describes TSGPUI's support for `powerMonitor`, `systemPreferences`, and `screen`
display events in both hosts: the native GPUI renderer and the system WebView renderer.

The Electron-facing shims live in `packages/cli/src/electron-compat/runtime`:

- `electron-logging.ts` - `powerMonitor`
- `electron-platform.ts` - `systemPreferences`
- `electron-system.ts` - `screen`, `powerSaveBlocker`
- `electron-webcontentsview.ts` - `contentTracing` (related file output)

The native platform queries are in `native/src/platform.rs`; the pollers that turn them into
events live in `native/src/view/mod.rs` (GPUI) and `native/src/webview_main.rs` (WebView).

## Host operations

| Operation | Result |
| --- | --- |
| `power.state` | `{ onBattery, idleTime, thermalState, locked }`. `onBattery` and `locked` are `null` when the OS cannot answer. `idleTime` is seconds. |
| `systemPreferences.info` | `{ darkMode, accentColor, colors, reduceMotion, invertedColors, highContrast, reduceTransparency, platform }`. `colors` maps Electron color names to `#rrggbb` strings. |

A shim that has no host support catches the rejection and falls back to renderer media queries and
a built-in light/dark palette. This keeps page-only builds and older hosts working.

## powerMonitor

`powerMonitor` is an `EventEmitter` that reports OS power transitions:

- `suspend` - the system is suspending.
- `resume` - the system resumed.
- `on-ac` - switched to AC power.
- `on-battery` - switched to battery power.
- `shutdown` - the system is shutting down (delivered only when the OS reports it).
- `lock-screen` - the session locked.
- `unlock-screen` - the session unlocked.

Cached state is exposed through:

- `onBatteryPower` / `isOnBatteryPower()`
- `getSystemIdleState(threshold)` and `getSystemIdleTime()`
- `getCurrentThermalState()`
- `refresh()` - TSGPUI addition: re-read `power.state` and update the cache.

The shim seeds its cache from `power.state` when it loads, then listens for host events through:

```js
globalThis.__tsgpui_power_event(name, ...args)
```

The shim updates the cached state and emits the matching event. Hosts poll state twice a second;
the platform layer caches slow OS queries so this does not run a command every tick.

### Platform behaviour

| Platform | AC/battery | Suspend/resume | Lock/unlock | Idle | Thermal |
| --- | --- | --- | --- | --- | --- |
| Windows | `GetSystemPowerStatus` plus `GUID_ACDC_POWER_SOURCE` notifications | `PowerRegisterSuspendResumeNotification` (`PBT_APMSUSPEND` / `PBT_APMRESUMESUSPEND`) | `GUID_SESSION_USER_PRESENCE` power-setting notification | `GetLastInputInfo` | `unknown` |
| macOS | `pmset -g batt` | Not observed while asleep; wake is seen as the next state poll | Not observed | Not observed | `pmset -g therm` CPU speed limit mapped to nominal/fair/serious/critical |
| Linux | `upower -d`, falling back to `/sys/class/power_supply` | Not observed while asleep; wake is seen as the next state poll | `loginctl show-session self -p LockedHint` | Not observed | `/sys/class/thermal/thermal_zone0/temp` mapped to nominal/fair/serious/critical |

Windows uses callback-style power notifications, so it does not need a hidden message window or a
message loop. `RegisterPowerSettingNotification` is the user32 API Electron itself uses;
TSGPUI reaches the same notifications through `PowerSettingRegisterNotification` with
`DEVICE_NOTIFY_CALLBACK`, which fits the GPUI/tao event loop.

Linux and macOS do not expose suspend/lock callbacks without extra D-Bus/Objective-C integration,
so those transitions are only reported when the polled state changes. Suspending and resuming
while the process is frozen cannot be observed until it runs again.

### Gaps

- Linux suspend/resume/lock/unlock currently depend on polling and may be missed across a full
  suspend. Add a logind `PrepareForSleep` / `Lock` / `Unlock` listener for exact delivery.
- macOS does not observe `NSWorkspace` sleep/lock notifications in this build. Wake is inferred
  from the next successful poll.
- Windows thermal state is reported as `unknown`; Electron mainly exposes it on macOS.
- `speed-limit-change` and `thermal-state-change` are not emitted.

## systemPreferences

`systemPreferences` exposes:

- `isDarkMode()` - the host's app appearance, or the `prefers-color-scheme: dark` media query.
- `getAccentColor()` - the host accent color as `rrggbb` (no `#`), or CSS `accent-color`.
- `getColor(color)` / `getSystemColor(color)` - the host's system color palette, with a built-in
  light/dark palette as fallback. Names follow Electron (`window`, `window-text`, `highlight`,
  `button-face`, ...).
- `isInvertedColorScheme()` - host value, or `inverted-colors: inverted`.
- `isHighContrast()` - host value, or `forced-colors: active` (TSGPUI addition).
- `accessibilityDisplayShouldReduceMotion` - host value, or `prefers-reduced-motion: reduce`.
- `accessibilityDisplayShouldReduceTransparency` - host value.
- `getAnimationSettings()` - reflects the reduced-motion flag.
- `refresh()` - TSGPUI addition: re-read `systemPreferences.info`.

Host wiring:

```js
globalThis.__tsgpui_system_preferences_event(name, ...args)
```

The handlers `color-changed`, `accent-color-changed` and
`accessibility-display-options-changed` trigger a host refresh before listeners run.

### Platform behaviour

| Platform | Dark mode | Accent color | Reduce motion | Other accessibility |
| --- | --- | --- | --- | --- |
| Windows | `HKCU\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize\AppsUseLightTheme` | `HKCU\Software\Microsoft\Windows\DWM\ColorizationColor` | `SystemParametersInfoW(SPI_GETCLIENTAREAANIMATION)` | High contrast from `Control Panel\Accessibility\HighContrast\Flags`; transparency from `EnableTransparency` |
| macOS | `defaults read -g AppleInterfaceStyle` | `defaults read -g AppleAccentColor` mapped to the standard macOS accents | `defaults read com.apple.universalaccess reduceMotion` | `invertColors`, `increaseContrast`, `reduceTransparency` |
| Linux | GNOME `color-scheme` / `gtk-theme` via `gsettings` | `gsettings get org.gnome.desktop.interface accent-color` | `gsettings get org.gnome.desktop.interface enable-animations` | GNOME `high-contrast`; inverted/reduced transparency are not queried |

### Gaps

- TouchID, media access, `effectiveAppearance` mutation and notification-center preferences remain
  stubs.
- Linux colors are the TSGPUI palette, not the GTK theme's actual colors.
- `getAccentColor()` normalizes CSS values to lower case without spaces, matching Electron's
  `rrggbb` form.

## screen

`screen` reports display geometry from the host and caches it. Call `screen.refresh()` to re-read.
The following events are emitted when the display set changes:

- `display-added`
- `display-removed`
- `display-metrics-changed` - carries a details object with `bounds`, `workArea`, `scaleFactor`
  and `rotation` flags.

Host wiring:

```js
globalThis.__tsgpui_display_event(name, display, details)
globalThis.__tsgpui_display_changed(payload)
```

Both hosts poll the monitor list every three seconds and emit events when it changes. The shim
re-emits them with normalized `Display` objects, then refreshes its cached list without emitting a
duplicate event. A `Display` includes `id`, `bounds`, `workArea`, `size`, `workAreaSize`,
`scaleFactor`, `rotation`, `internal`, `touchSupport` and `colorDepth`.

The WebView host uses the monitor's index as `id` (tao does not expose a stable monitor id). GPUI
uses `DisplayId`. If monitors are reordered, an app may see a remove/add pair instead of a metrics
change.

### Gaps

- The native renderer cannot detect hot-plug monitor changes without polling; events arrive on the
  next poll (within three seconds).
- Per-display scale factors are not available in GPUI, so every display reports the app window's
  scale factor.
- `workArea` equals `bounds`; GPUI/tao do not report a separate work area.
- `screen.getCursorScreenPoint()` returns the host-reported point from the last refresh. It is not
  a live global cursor position.

## Related file output

The same host polling loop also improves the miscellaneous Electron shims:

- `contentTracing.startRecording()` / `stopRecording(path)` write a Chrome tracing-event JSON file
  with TSGPUI markers. Open it in Perfetto or `chrome://tracing`; Chromium renderer/GPU events are
  not available.
- `crashReporter.start()` writes `crash-reporter.json` under `<userData>/crashes`.
  `crashReporter.captureCrash(reason)` (TSGPUI addition) writes a timestamped JSON dump there.
  Native minidumps and uploads are not supported.
- `netLog.startLogging(path)` / `stopLogging()` write a JSON net log. The `net` shim records each
  `net.request` URL, method, status and headers. Chromium socket, DNS and QUIC internals are not
  available.

All three use the filesystem permission roots. Relative paths resolve under `userData`.
