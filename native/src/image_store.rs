//! Decoded images shared by host operations.
//!
//! Tray icons keep a handle (1-based) instead of the decoded pixels, so a `setIcon` image is
//! decoded once and read back by the platform layer whenever the OS asks for it again (an Explorer
//! restart, a menu rebuild, a D-Bus property read). Handles are never reused. The store is bounded:
//! when it is full, the oldest image that no live tray refers to is dropped.

use std::sync::Mutex;

use image::{DynamicImage, ImageBuffer, Rgba};
use serde_json::Value;

/// Upper bound on stored images. Each entry is one decoded icon, and Electron apps rarely create
/// more than a handful; the cap only stops a runaway loop of `setImage` calls.
const MAX_IMAGES: usize = 256;
/// Largest icon accepted, in pixels. Tray icons are tiny; anything past this is not a tray icon.
const MAX_PIXELS: u64 = 4096 * 4096;

static IMAGE_STORE: Mutex<Vec<Option<DynamicImage>>> = Mutex::new(Vec::new());

/// Stores `image` and returns its 1-based handle. `live` holds the handles current tray icons use;
/// they are never evicted.
pub fn store(image: DynamicImage, live: &[usize]) -> usize {
    let mut store = IMAGE_STORE.lock().unwrap_or_else(|error| error.into_inner());
    if store.len() >= MAX_IMAGES {
        let reusable = store.iter().position(|slot| match slot {
            Some(_) => true,
            None => false,
        }).filter(|index| !live.contains(&(index + 1)));
        if let Some(index) = reusable {
            store[index] = Some(image);
            return index + 1;
        }
    }
    store.push(Some(image));
    store.len()
}

/// Returns the image behind `handle`, if it has not been dropped.
pub fn load(handle: usize) -> Option<DynamicImage> {
    let store = IMAGE_STORE.lock().unwrap_or_else(|error| error.into_inner());
    handle.checked_sub(1).and_then(|index| store.get(index)).and_then(|slot| slot.clone())
}

/// Decodes the image shapes the compatibility bridge accepts:
///
/// * `{ "path": "icon.png" }` reads a file (the backend is trusted app code).
/// * `{ "base64": "..." }` decodes PNG, JPEG, GIF or WebP bytes.
/// * `{ "rgba": [r, g, b, a, ...], "width": w, "height": h }` wraps raw pixels.
/// * a plain string is treated as a path.
///
/// Returns `None` for a missing or empty image.
pub fn decode(value: &Value) -> Result<Option<DynamicImage>, String> {
    let image = match value {
        Value::Null => return Ok(None),
        Value::String(path) if path.is_empty() => return Ok(None),
        Value::String(path) => image::open(path).map_err(|error| format!("Unable to read the tray image '{path}': {error}"))?,
        Value::Object(_) => decode_object(value)?,
        _ => return Err("The tray image must be a path, a data URL or an object with base64 or raw pixels".into()),
    };
    let (width, height) = (image.width() as u64, image.height() as u64);
    if width == 0 || height == 0 {
        return Err("The tray image is empty".into());
    }
    if width * height > MAX_PIXELS {
        return Err(format!("The tray image is too large ({width}x{height})"));
    }
    Ok(Some(image))
}

fn decode_object(value: &Value) -> Result<DynamicImage, String> {
    if let Some(base64) = value.get("base64").and_then(Value::as_str) {
        if base64.is_empty() {
            return Err("The tray image is empty".into());
        }
        let bytes = crate::platform::base64_decode(base64)?;
        return image::load_from_memory(&bytes).map_err(|error| format!("The tray image is not a supported PNG, JPEG, GIF or WebP image: {error}"));
    }
    if let Some(path) = value.get("path").and_then(Value::as_str) {
        if path.is_empty() {
            return Err("The tray image is empty".into());
        }
        return image::open(path).map_err(|error| format!("Unable to read the tray image '{path}': {error}"));
    }
    let pixels = value.get("rgba").or_else(|| value.get("data")).and_then(Value::as_array).ok_or("The tray image object needs a base64 string, a path or raw rgba pixels")?;
    let width = value.get("width").and_then(Value::as_u64).ok_or("Image width is required for raw pixels")?;
    let height = value.get("height").and_then(Value::as_u64).ok_or("Image height is required for raw pixels")?;
    let width = u32::try_from(width).map_err(|_| "Image width is out of range")?;
    let height = u32::try_from(height).map_err(|_| "Image height is out of range")?;
    let bytes: Vec<u8> = pixels
        .iter()
        .map(|pixel| pixel.as_u64().and_then(|byte| u8::try_from(byte).ok()).ok_or_else(|| "Raw image pixels must be bytes".to_string()))
        .collect::<Result<_, _>>()?;
    let expected = width as usize * height as usize * 4;
    if bytes.len() != expected {
        return Err(format!("Raw image has {} bytes, expected {expected} for {width}x{height} RGBA", bytes.len()));
    }
    let buffer: ImageBuffer<Rgba<u8>, Vec<u8>> = ImageBuffer::from_raw(width, height, bytes).ok_or("Raw image dimensions do not match its pixels")?;
    Ok(DynamicImage::ImageRgba8(buffer))
}

/// RGBA pixels for a square `size` icon. Larger images are scaled down, smaller ones are kept and
/// centered on a transparent canvas, and the alpha channel is preserved.
pub fn rgba_square(image: &DynamicImage, size: u32) -> (u32, u32, Vec<u8>) {
    let source = image.to_rgba8();
    let (width, height) = source.dimensions();
    if width == size && height == size {
        return (size, size, source.into_raw());
    }
    let scale = (size as f64 / width.max(1) as f64).min(size as f64 / height.max(1) as f64);
    let target_width = ((width as f64 * scale).round() as u32).clamp(1, size);
    let target_height = ((height as f64 * scale).round() as u32).clamp(1, size);
    let resized = if target_width == width && target_height == height {
        source
    } else {
        image::imageops::resize(&source, target_width, target_height, image::imageops::FilterType::Lanczos3)
    };
    let mut canvas = ImageBuffer::from_pixel(size, size, Rgba([0, 0, 0, 0]));
    let x = ((size - target_width) / 2) as i64;
    let y = ((size - target_height) / 2) as i64;
    image::imageops::overlay(&mut canvas, &resized, x, y);
    (size, size, canvas.into_raw())
}

/// The image as PNG bytes, for platforms that decode images themselves (macOS `NSImage`).
#[cfg(target_os = "macos")]
pub fn png_bytes(image: &DynamicImage) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    let mut cursor = std::io::Cursor::new(&mut bytes);
    image.write_to(&mut cursor, image::ImageFormat::Png).map_err(|error| format!("Unable to encode the tray image: {error}"))?;
    Ok(bytes)
}
