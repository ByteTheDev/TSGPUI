# Power, Display, and System Preferences

This document describes TSGPUI's support for `powerMonitor`, `systemPreferences`, and `screen` display events.

## powerMonitor

`powerMonitor` is an `EventEmitter` that reports OS power transitions:

- `suspend` — system is suspending.
- `resume` — system resumed.
- `on-ac` — switched to AC power.
- `on-battery` — switched to battery power.
- `shutdown` — system is shutting down.
- `lock-screen` — screen locked.
- `unlock-screen` — screen unlocked.

Cached state is exposed through `onBatteryPower` / `isOnBatteryPower()`, plus `getSystemIdleState`, `getSystemIdleTime`, and `getCurrentThermalState`.

Host wiring: the native/WebView host notifies the shim through
`globalThis.__tsgpui_power_event(name, ...args)`. The shim updates cached state
and re-emits the matching event. On Windows the host should drive this from
`RegisterPowerSettingNotification` / WMI power events; macOS and Linux support is
best-effort and may be absent in this build.

Gaps: idle time and thermal state are not queried from the OS; `speed-limit-change`,
`thermal-state-change`, `user-did-become-active`, and `user-did-resign-active` are
not emitted.

## systemPreferences

`systemPreferences` exposes:

- `isDarkMode()` — derived from the renderer's `prefers-color-scheme: dark` media query.
- `getAccentColor()` — best-effort from CSS `accent-color`, otherwise the cached value.
- `getColor(color)` / `getSystemColor(color)` — return `''` (not available from the host).
- `isInvertedColorScheme()` — from `inverted-colors: inverted`.
- `accessibilityDisplayShouldReduceMotion` — boolean, from `prefers-reduced-motion: reduce`.
- `getAnimationSettings()` — reflects the reduced-motion flag.

Host wiring: `globalThis.__tsgpui_system_preferences_event(name, ...args)`.

Gaps: Windows registry and macOS `NSUserDefaults` queries are not wired in this
build; TouchID and media access remain stubs.

## screen

`screen` reports display geometry from the host and caches it. Call `screen.refresh()`
to re-read. The following events are emitted when the display set changes:

- `display-added`
- `display-removed`
- `display-metrics-changed`

Host wiring: `globalThis.__tsgpui_display_event(name, display, details)` and
`globalThis.__tsgpui_display_changed(payload)`.

Gaps: the native renderer cannot detect hot-plug monitor changes automatically;
apps should call `screen.refresh()` or react to the host events above. Per-display
scale factors are not available in GPUI, so every display reports the app window's
scale factor.