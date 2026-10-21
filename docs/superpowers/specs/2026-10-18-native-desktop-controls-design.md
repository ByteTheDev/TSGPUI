# Native Desktop Controls and Behaviors

## Goal

Make TSGPUI apps feel like desktop apps while preserving its HTML/CSS/TypeScript authoring model and native GPUI renderer. Add declarative native menus and context menus, file drag and drop, platform accessibility semantics, and complete everyday text editing behavior.

## Existing behavior

The host already provides action shortcuts, Tab focus traversal, native confirmation and file dialogs, clipboard copy/paste actions, selection, IME input routing, and basic text fields. Text fields lack undo/redo and some standard editor navigation. The app tree preserves ARIA and role attributes but exposes no screen-reader tree. There is no app menu or general drop-event API.

## Design

### Shared authoring and event model

Keep HTML as the source of the view. New interactions reference named backend actions and deliver immutable JSON events through the existing `invoke_with_event` path. Compiler validation rejects invalid action names, unsupported menu structures, and conflicting drop attributes at build time. The browser preview should provide reasonable DOM-based equivalents where possible and clearly document native-only behavior.

### Native menus

Add a declarative menu description to the application document, with labels, separators, enabled state, action names, and optional shortcuts. Use the GPUI application menu API for the platform menu bar. Add a context-menu attribute to eligible elements that opens a native popup menu at the pointer. Menu commands dispatch through the same backend action mechanism as buttons and shortcuts. Disabled and checked states are reflected in the native menu and may be bound to backend state.

The compiler validates menu trees and action references. Menu labels and shortcut strings are included in the bundle, so applications do not need a separate Rust host build for menu edits.

### File drag and drop

Add a drop target attribute that names a backend action. A drop event contains the dropped local file paths and display names. Dropping is user initiated; the host does not read file contents automatically. Existing explicit file read APIs remain the mechanism for reading selected files. The host reports unsupported platform payloads as a recoverable event error rather than crashing or silently discarding the drop.

The native platform layer must normalize file drops across supported operating systems. If the pinned GPUI release does not expose a platform's drop events, add the smallest host adapter needed for that platform rather than coupling app code to OS-specific APIs.

### Accessibility

Map HTML `role`, `aria-label`, `aria-labelledby`, `aria-describedby`, `aria-valuenow`, `aria-valuemin`, `aria-valuemax`, `aria-checked`, and disabled/selected state to native accessibility nodes. Use existing element text and labels when an explicit accessible name is absent. Preserve focus order and expose text inputs' current value and selection where supported. Keep decorative elements out of the accessibility tree and mark them hidden when requested.

Use GPUI's AccessKit integration if available in a compatible release. The repository currently pins GPUI 0.2.2, whose public API is documented here as lacking an accessibility tree. Before implementation, verify whether an upstream release exposes the needed API. If not, isolate the dependency change or host adapter behind a small module and avoid claiming screen-reader support until exercised on each target platform.

### Text editing

Extend existing native input and textarea behavior with undo/redo, caret blinking, word-wise navigation and deletion, and standard select-line and document navigation shortcuts. Keep selection, composition, and clipboard behavior integrated with GPUI's native text input handler. Undo history is per field, bounded, and reset when the field identity changes or the app reloads. A change coalescing policy groups adjacent typed characters while keeping paste, deletion, and composition edits as sensible undo units.

This phase concerns editing plain text. Rich text formatting, syntax highlighting, and an embedded code editor remain separate features.

## Architecture and data flow

The CLI parses new menu and drop declarations into the `.tgp` document. The native host turns menu definitions into GPUI menu items, registers native drop handlers on eligible elements, and maps accessibility metadata into GPUI/AccessKit nodes. Menu and drop commands call the backend with typed event payloads. Text undo state remains in the native view model next to its existing caret and selection state.

## Failure handling and permissions

Menu action failures use the existing host error reporting path. Invalid declarations fail compilation with file and line context. A file drop only grants the app the paths the user dropped; it does not grant ambient filesystem access. Accessibility metadata is descriptive and does not grant additional capabilities. If native accessibility or drag/drop support is unavailable on a target, the host reports that limitation and keeps the app usable with mouse and keyboard input.

## Delivery stages

1. Text editing behavior, because the project already owns its text-field implementation and can validate it without changing the bundle contract.
2. Declarative menus and context menus using GPUI's application and popup menu facilities.
3. File drop events, including platform-specific coverage where GPUI does not expose a uniform API.
4. Accessibility tree integration after confirming the compatible GPUI/AccessKit API and platform support.

Each stage updates the supported-syntax reference, architecture notes, and browser-preview behavior where applicable. Each stage should be independently reviewable and preserve the existing authoring surface.

## Acceptance criteria

- Existing text input, textarea, clipboard, and keyboard behavior continues to work while undo/redo and standard navigation are added.
- Applications can define app and context menus in the documented source format; activating an enabled item invokes its configured backend action.
- Dropping one or more files on a declared drop target dispatches their names and paths to its backend action without reading contents.
- Accessible names, roles, values, and states are exposed through the native accessibility API on platforms supported by the selected GPUI integration.
- Unsupported syntax and platform limitations are reported clearly; documentation does not promise behavior that is not available.

## Risks and open implementation constraints

- GPUI 0.2.2 may not expose menu, drag/drop, or accessibility APIs needed by all targets. Dependency upgrades or a narrowly scoped platform adapter may be necessary.
- Native menu conventions differ among Windows, macOS, and Linux, especially the location and lifecycle of application menus.
- File-drop event support and payloads vary across window backends.
- AccessKit plumbing alone does not guarantee a useful screen-reader experience; semantic node construction, focus movement, and actual platform testing are required.
- This is four related but independently testable capabilities. Implementation should proceed in the delivery stages above rather than landing as one high-risk change.
