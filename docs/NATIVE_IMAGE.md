# NativeImage real pixel support

Rendrix's Electron compatibility layer used to keep `nativeImage` metadata only: PNG bytes were
preserved and dimensions were parsed from PNG/JPEG/GIF/WebP headers, but `resize`, `crop`, `toPNG`,
`toJPEG` and `toDataURL` did not touch pixels.

The host now decodes, transforms and re-encodes images with the `image` crate
(`native/src/capabilities.rs`), and the Electron shim
(`packages/cli/src/electron-compat/runtime/electron-image.ts`) uses those operations. This document
describes the host operation contract, the JavaScript behaviour, and the remaining differences from
Electron.

## Architecture

```
Electron app
  └─ electron-image.ts (shim)                real pixels, async host calls
       └─ host('nativeImage.*', options)     __rendrix_host / api.native
            └─ native renderer:   native/src/runtime.rs → capabilities.rs
                 └─ IMAGE_STORE: decoded DynamicImage behind a handle
```

(The webview-era path through `native/src/webview_bridge.rs` and
`native/src/webview_capabilities.rs` was removed with the WebView renderer.)

All operations take **one option object** and return plain JSON. Decoded images stay in the host;
callers pass a numeric `handle` between calls instead of shipping pixels back and forth. Bitmaps
use the BGRA byte order Electron and Chromium use (`blue, green, red, alpha` per pixel).

## Host operations

| Operation | Options | Result |
| --- | --- | --- |
| `nativeImage.createFromPath` | `{ root?, path, scaleFactor? }` | `{ handle, width, height, scaleFactor }` |
| `nativeImage.createFromBuffer` | `{ base64, width?, height?, scaleFactor? }` | `{ handle, width, height, scaleFactor }` |
| `nativeImage.getSize` | `{ handle }` | `{ width, height, scaleFactor }` |
| `nativeImage.getAspectRatio` | `{ handle }` | `width / height` (number, `0` for an empty image) |
| `nativeImage.resize` | `{ handle, width?, height?, quality? }` | `{ handle, width, height, scaleFactor }` |
| `nativeImage.crop` | `{ handle, x?, y?, width?, height? }` | `{ handle, width, height, scaleFactor }` |
| `nativeImage.toPNG` | `{ handle }` | `{ base64 }` |
| `nativeImage.toJPEG` | `{ handle, quality? }` | `{ base64 }` |
| `nativeImage.toDataURL` | `{ handle }` | `{ dataUrl }` (PNG data URL) |
| `nativeImage.toBitmap` | `{ handle }` | `{ base64, width, height }` (BGRA) |

Notes:

- `handle` `0` means an empty image. Creation calls return it for empty or undecodable input
  instead of throwing, which matches `nativeImage.createFromBuffer(new Uint8Array([1, 2, 3]))`
  returning an empty image in Electron.
- `createFromBuffer` decodes PNG/JPEG/GIF/WebP when `width` and `height` are absent. With both
  present, `base64` is a raw BGRA bitmap of exactly `width * height * 4` bytes, matching
  `createFromBitmap`/`toBitmap`.
- `resize` stretches to the requested size (it does not preserve the aspect ratio). When a
  dimension is omitted the current dimension is used. `quality` maps to resampling filters:
  `good` → triangle, `better` → Catmull-Rom, `best` (default) → Lanczos3.
- `crop` clamps the rectangle to the image bounds. A zero-size result is an empty image.
- `toJPEG` defaults to quality 80; values are clamped to 1–100. Alpha is composited away by
  converting to RGB.
- `createFromPath` takes `{ root, path }` like `filesystem.*`, where `root` is a permission root,
  `userData`, or a dialog-grant name. A bare absolute path is also accepted when it resolves inside
  a granted read root. Paths outside the grants fail with `PERMISSION_DENIED`.
- Input is capped at 32 MiB, decoded dimensions at 16384 per side and 256 MiB of allocation.
  Decoding errors are reported instead of panicking.

### Handle lifetime

The host keeps at most 128 decoded images in a process-wide store and evicts the
least-recently-used entry when a new decode arrives. Handles stay stable for as long as their slot
is alive; a handle that has been evicted reads as an empty image. Create and transform results are
both stored, so a chain like `resize → crop → toPNG` reuses the same mechanism.

## Calling from the native API

Native renderer (`api.native` in a Rendrix app):

```ts
const created = await api.native.nativeImage.createFromBuffer({ base64, scaleFactor: 1 });
const resized = await api.native.nativeImage.resize({ handle: created.handle, width: 64, height: 64 });
const { base64: png } = await api.native.nativeImage.toPNG({ handle: resized.handle });
```

(Historical: the removed WebView renderer exposed the same operations as
`window.rendrix.invoke('nativeImage.*', …)`, permission-checked the same way.)

`nativeImage.createFromPath` is permission-checked on every renderer. Buffer and transform
operations need no grant because they do no I/O.

## Electron shim behaviour

`require('electron').nativeImage` keeps its Electron-shaped synchronous surface, backed by the host
where possible:

```js
const image = await nativeImage.createFromPath('/data/icon.png'); // waits for the host
image.getSize();          // decoded dimensions
image.getAspectRatio();   // decoded ratio
image.toPNG();            // re-encoded PNG
image.toJPEG();           // re-encoded JPEG at the host's default quality (80)
image.toDataURL();        // PNG data URL
const small = image.resize({ width: 32 }); // preserves aspect ratio when one side is omitted
small.getSize();          // known immediately from the requested size
await small.whenReady();  // host finished decoding/encoding the new image
small.toPNG();            // pixels of the resized image
```

- `createFromPath` is asynchronous in this shim (as it was before), and now resolves only after the
  host has decoded the image and cached its PNG and JPEG encodings.
- `createFromBuffer`, `createFromDataURL` and `createFromBitmap` stay synchronous. They return the
  local image immediately and ask the host to decode in the background. Call
  `await image.whenReady()` before relying on re-encoded output (`toPNG`, `toJPEG`, `toDataURL`)
  from those creators.
- `resize` and `crop` return a new `NativeImage` synchronously with the right size; its pixels
  arrive asynchronously through `whenReady()`.
- Without host image support (the browser preview, or an older host) the shim falls back to the
  previous metadata behaviour: PNG bytes round-trip, dimensions are parsed locally, and
  JPEG/WebP/GIF inputs report dimensions but cannot be re-encoded.
- `toBitmap()` returns the original bytes for images created with `createFromBitmap`. For encoded
  inputs it returns zeros unless the host `nativeImage.toBitmap` operation is used directly.
- `toJPEG()` returns the host's cached encoding (quality 80). The shim cannot re-encode
  synchronously for a different quality; call the host `nativeImage.toJPEG` operation with
  `{ handle, quality }` when a specific quality is required.
- Only the creation scale factor is tracked. Multiple representations/scale factors are not
  supported; `getScaleFactors()` reports the factor the image was created with.
- GIF/WebP decoding yields the first frame.

Compatibility coverage is recorded in
[`packages/cli/src/electron-compat/coverage.mjs`](../packages/cli/src/electron-compat/coverage.mjs);
the native host implementation is in
[`native/src/capabilities.rs`](../native/src/capabilities.rs). The webview host's copy
(`native/src/webview_capabilities.rs`) and the bridge wiring
(`native/src/webview_bridge.rs`) were removed with the WebView renderer; the native-side
wiring stays in [`native/src/runtime.rs`](../native/src/runtime.rs).

## Verification

- `cargo test --manifest-path native/Cargo.toml native_image` covers decode, resize, crop, PNG/JPEG
  encoding, BGRA bitmap conversion, named roots, absolute paths inside grants and path denial.
- `bun test` covers the JS shim fallback and the host-backed path with a mocked host.
