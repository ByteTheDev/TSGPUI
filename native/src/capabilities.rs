//! Host operations behind `api.native` in the TypeScript backend.
//!
//! Every call is checked against the grants in the bundle's `permissions`. Paths are relative to
//! a granted directory, may not contain `..`, and are resolved through symlinks before the check,
//! so a link cannot lead out of a granted root. Processes run only when their name is granted, and
//! they run without a shell.

use std::{
    cell::RefCell,
    fs,
    io::Read,
    path::{Component, Path, PathBuf},
    process::{Command, Stdio},
    sync::{Arc, Mutex},
    sync::atomic::{AtomicBool, AtomicU64, Ordering},
    thread,
    time::{Duration, Instant, UNIX_EPOCH},
};

use serde_json::{json, Value};

use image::{DynamicImage, ImageFormat};
use crate::{package::Permissions, platform};

thread_local! {
    static CURRENT_CANCEL: RefCell<Option<Arc<AtomicBool>>> = RefCell::new(None);
}

pub fn current_cancel_token() -> Option<Arc<AtomicBool>> {
    CURRENT_CANCEL.with(|cell| cell.borrow().clone())
}

pub fn set_current_cancel_token(token: Option<Arc<AtomicBool>>) {
    CURRENT_CANCEL.with(|cell| *cell.borrow_mut() = token);
}

/// Largest file read or written by one call, and largest captured process output per stream.
pub const MAX_FILE_BYTES: u64 = 1024 * 1024;
const MAX_LIST_ENTRIES: usize = 10_000;
const PROCESS_TIMEOUT: Duration = Duration::from_secs(10);

static IMAGE_STORE: Mutex<Vec<Option<StoredImage>>> = Mutex::new(Vec::new());
static IMAGE_CLOCK: AtomicU64 = AtomicU64::new(0);
static IMAGE_HANDLE: AtomicU64 = AtomicU64::new(0);

/// A decoded image kept for the lifetime of one or more host calls. Images are addressed by a
/// 1-based handle so the JavaScript side can hold an image across calls without shipping its
/// pixels back and forth. Handles are never reused, so an evicted handle reads as an empty image.
struct StoredImage {
    handle: u64,
    image: DynamicImage,
    last_used: u64,
}

/// Decoded images are capped by count and evicted least-recently-used. The cap keeps a long
/// session that creates many thumbnails from retaining every pixel buffer forever.
const MAX_STORED_IMAGES: usize = 128;
/// Largest image file or encoded buffer accepted by one call.
const MAX_IMAGE_BYTES: usize = 32 * 1024 * 1024;
/// Largest decoded width or height. The image crate also enforces this while decoding.
const MAX_IMAGE_DIMENSION: u32 = 16_384;
const MAX_IMAGE_ALLOC: u64 = 256 * 1024 * 1024;

fn store_image(img: DynamicImage) -> usize {
    let mut store = IMAGE_STORE.lock().unwrap_or_else(|e| e.into_inner());
    let stamp = IMAGE_CLOCK.fetch_add(1, Ordering::Relaxed);
    let handle = IMAGE_HANDLE.fetch_add(1, Ordering::Relaxed) + 1;
    let slot = StoredImage { handle, image: img, last_used: stamp };
    if let Some(index) = store.iter().position(Option::is_none) {
        store[index] = Some(slot);
        return handle as usize;
    }
    if store.len() >= MAX_STORED_IMAGES {
        if let Some(index) = store.iter().enumerate()
            .filter_map(|(index, slot)| slot.as_ref().map(|slot| (index, slot.last_used)))
            .min_by_key(|(_, last_used)| *last_used)
            .map(|(index, _)| index)
        {
            store[index] = Some(slot);
            return handle as usize;
        }
    }
    store.push(Some(slot));
    handle as usize
}

fn load_image(handle: usize) -> Option<DynamicImage> {
    let mut store = IMAGE_STORE.lock().unwrap_or_else(|e| e.into_inner());
    let slot = store.iter_mut().filter_map(Option::as_mut).find(|slot| slot.handle == handle as u64)?;
    slot.last_used = IMAGE_CLOCK.fetch_add(1, Ordering::Relaxed);
    Some(slot.image.clone())
}

/// Decodes an encoded image, or a BGRA bitmap when `width` and `height` are given. The bitmap
/// layout matches Electron's `toBitmap`/`createFromBitmap` on Windows and Chromium: blue, green,
/// red and alpha bytes per pixel.
fn decode_image(bytes: &[u8], width: Option<u32>, height: Option<u32>) -> Result<DynamicImage, String> {
    if let (Some(width), Some(height)) = (width, height) {
        if width == 0 || height == 0 {
            return Err("Bitmap width and height must be positive".into());
        }
        let expected = (width as u64).checked_mul(height as u64).and_then(|pixels| pixels.checked_mul(4))
            .ok_or("Bitmap dimensions are too large")?;
        if bytes.len() as u64 != expected {
            return Err(format!("Bitmap data is {} bytes but {width}x{height} needs {expected}", bytes.len()));
        }
        let mut rgba = bytes.to_vec();
        for pixel in rgba.chunks_exact_mut(4) {
            pixel.swap(0, 2);
        }
        let image = image::RgbaImage::from_raw(width, height, rgba).ok_or("Bitmap data could not be read")?;
        return Ok(DynamicImage::ImageRgba8(image));
    }
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_IMAGE_DIMENSION);
    limits.max_image_height = Some(MAX_IMAGE_DIMENSION);
    limits.max_alloc = Some(MAX_IMAGE_ALLOC);
    let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|error| format!("Image format could not be detected: {error}"))?;
    reader.limits(limits);
    reader.decode().map_err(|error| format!("Image could not be decoded: {error}"))
}

fn encode_png(image: &DynamicImage) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    image.write_to(&mut std::io::Cursor::new(&mut bytes), ImageFormat::Png)
        .map_err(|error| format!("PNG encoding failed: {error}"))?;
    Ok(bytes)
}

fn encode_jpeg(image: &DynamicImage, quality: u8) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    let rgb = image.to_rgb8();
    let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut bytes, quality.clamp(1, 100));
    encoder.encode_image(&rgb).map_err(|error| format!("JPEG encoding failed: {error}"))?;
    Ok(bytes)
}

/// Decodes the raw RGBA image and flips it to BGRA, the layout Electron and Chromium use for
/// bitmaps. The returned bytes are ready for `base64_encode`.
fn encode_bitmap(image: &DynamicImage) -> Vec<u8> {
    let mut bytes = image.to_rgba8().into_raw();
    for pixel in bytes.chunks_exact_mut(4) {
        pixel.swap(0, 2);
    }
    bytes
}

fn image_scale(options: &Value) -> f64 {
    options.get("scaleFactor").and_then(Value::as_f64).filter(|scale| *scale > 0.0).unwrap_or(1.0)
}

/// Scales one dimension of an image while preserving the aspect ratio, rounding to the nearest
/// pixel. Electron's `NativeImage.resize` does the same when one side is omitted.
fn scale_dimension(value: u32, from: u32, to: u32) -> u32 {
    if from == 0 {
        return value;
    }
    (((value as u64) * (to as u64) + (from as u64) / 2) / (from as u64)).max(1) as u32
}

fn image_handle(options: &Value) -> usize {
    options.get("handle").and_then(Value::as_u64).unwrap_or(0) as usize
}

fn image_result(handle: usize, width: u32, height: u32, scale: f64) -> Value {
    json!({ "handle": handle, "width": width, "height": height, "scaleFactor": scale })
}

fn empty_image_result(scale: f64) -> Value {
    image_result(0, 0, 0, scale)
}

fn store_image_result(image: DynamicImage, scale: f64) -> Value {
    let (width, height) = (image.width(), image.height());
    image_result(store_image(image), width, height, scale)
}

fn base64_encode(bytes: &[u8]) -> String {
    const ALPHA: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0] as usize;
        let b1 = chunk.get(1).copied().unwrap_or(0) as usize;
        let b2 = chunk.get(2).copied().unwrap_or(0) as usize;
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(ALPHA[(n >> 18) & 63] as char);
        out.push(ALPHA[(n >> 12) & 63] as char);
        if chunk.len() > 1 { out.push(ALPHA[(n >> 6) & 63] as char); } else { out.push('='); }
        if chunk.len() > 2 { out.push(ALPHA[n & 63] as char); } else { out.push('='); }
    }
    out
}

fn base64_decode(text: &str) -> Result<Vec<u8>, String> {
    const ALPHA: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = Vec::new();
    let mut acc: u32 = 0;
    let mut bits = 0;
    for c in text.chars() {
        if c == '=' { break; }
        let idx = ALPHA.iter().position(|&b| b == c as u8).ok_or("Invalid base64")? as u32;
        acc = (acc << 6) | idx;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push(((acc >> bits) & 0xff) as u8);
        }
    }
    Ok(out)
}

#[derive(Debug, Clone, Default)]
pub struct Capabilities {
    base: PathBuf,
    read_roots: Vec<PathBuf>,
    write_roots: Vec<PathBuf>,
    /// The same roots, keyed by their names in `permissions`, so `{ root, path }` can address them.
    read_named: Vec<(String, PathBuf)>,
    write_named: Vec<(String, PathBuf)>,
    processes: Vec<String>,
    process_args: std::collections::BTreeMap<String, Vec<String>>,
    network: Vec<(String, Vec<String>)>,
    app: AppIdentity,
    /// Files picked with a dialog. Clones share the list, so a grant lasts for the whole run.
    dialog_grants: Arc<Mutex<Vec<DialogGrant>>>,
    /// Opt-in grants for the window, notification and IPC operations.
    pub window: bool,
    pub notifications: bool,
    pub ipc: bool,
}

#[derive(Debug, Clone, Default)]
struct AppIdentity {
    name: String,
    version: String,
    user_data: PathBuf,
}

#[derive(Debug, Clone)]
struct DialogGrant {
    name: String,
    path: PathBuf,
    write: bool,
}

impl Capabilities {
    /// Builds the grants from a bundle. Relative roots resolve against `base`, which is the
    /// directory of the app executable, or of the `.tgp` file for `--bundle` runs.
    pub fn from_permissions(permissions: &Permissions, base: PathBuf) -> Self {
        let named = |roots: &[String]| -> Vec<(String, PathBuf)> { roots.iter().map(|root| (root.clone(), base.join(root))).collect() };
        let read_named = named(&permissions.filesystem.read);
        let write_named = named(&permissions.filesystem.write);
        let read_roots = read_named.iter().map(|(_, path)| path.clone()).collect();
        let write_roots: Vec<PathBuf> = write_named.iter().map(|(_, path)| path.clone()).collect();
        // Create writable roots up front so a fresh install can save its first file. Failure is
        // not fatal: later calls report that the path is outside a granted directory.
        for root in &write_roots {
            let _ = fs::create_dir_all(root);
        }
        let network: Vec<(String, Vec<String>)> = if permissions.network_hosts.is_empty() && permissions.network_methods.is_empty() {
            Vec::new()
        } else if permissions.network_methods.is_empty() {
            permissions.network_hosts.iter().map(|host| (host.clone(), Vec::new())).collect()
        } else {
            permissions.network_methods.iter().map(|(host, methods)| (host.clone(), methods.clone())).collect()
        };
        // Legacy network_hosts may be present alongside methods; merge hosts that only appear in network_hosts.
        let mut network = network;
        for host in &permissions.network_hosts {
            if !network.iter().any(|(h, _)| h.eq_ignore_ascii_case(host)) {
                network.push((host.clone(), Vec::new()));
            }
        }
        Self {
            base,
            read_roots,
            write_roots,
            read_named,
            write_named,
            processes: permissions.processes.clone(),
            process_args: permissions.process_args.clone(),
            network,
            app: AppIdentity::default(),
            dialog_grants: Arc::default(),
            window: permissions.window,
            notifications: permissions.notifications,
            ipc: permissions.ipc,
        }
    }

    /// Names the app for `app.info`. Its `userData` folder is the folder of its state file.
    pub fn with_app(mut self, name: &str, version: &str) -> Self {
        let user_data = crate::package::state_file(name).and_then(|file| file.parent().map(Path::to_path_buf)).unwrap_or_else(|| self.base.clone());
        // Electron apps write to userData without creating it first, so it exists from the start.
        let _ = fs::create_dir_all(&user_data);
        self.app = AppIdentity { name: name.into(), version: version.into(), user_data };
        self
    }

    fn user_data_dir(&self) -> PathBuf {
        if self.app.user_data.as_os_str().is_empty() { self.base.clone() } else { self.app.user_data.clone() }
    }

    /// Runs one operation. `operation` is `"<group>.<method>"`, and `args` is the JSON argument list.
    /// `env` is added to processes started by `process.spawn`. A `filesystem.*` call whose first
    /// argument is `{ root, path }` addresses a named root instead (see `named_filesystem`).
    pub fn call(&self, operation: &str, args: &[Value], env: &[(String, String)]) -> Result<Value, String> {
        if operation.starts_with("filesystem.") && args.first().is_some_and(Value::is_object) {
            return self.named_filesystem(operation, args);
        }
        if operation.starts_with("nativeImage.") {
            return self.native_image(operation, args);
        }
        match operation {
            "app.info" => Ok(self.app_info()),
            // Quitting the native GPUI process is driven by UiCommand::Quit in the runtime, so
            // only the accept-and-ignore behavior is needed here for the quit operations.
            "app.quit" | "app.exit" => Ok(Value::Null),
            "updater.download" => {
                // In-app update staging: the host streams the granted HTTPS artifact straight
                // into userData/updates while computing SHA-256; the page never handles the
                // bytes. The allowlist check mirrors `net.request`.
                let opts = options(args);
                let url = opts["url"].as_str().ok_or("url must be a string")?;
                let parsed_host = url::Url::parse(url).ok().and_then(|u| u.host_str().map(str::to_owned)).unwrap_or_default();
                let allowed = self.network.iter().any(|(granted_host, _)| granted_host.eq_ignore_ascii_case(&parsed_host));
                if !allowed {
                    return Err("PERMISSION_DENIED: Network host is not granted for this host and method".into());
                }
                let hosts = self.network.iter().map(|(host, _)| host.clone()).collect::<Vec<_>>();
                let staged = crate::updater::download(
                    &self.user_data_dir(),
                    url,
                    opts["name"].as_str().unwrap_or("update.bin"),
                    opts["sha256"].as_str(),
                    &hosts,
                )?;
                Ok(json!({ "staged": staged.to_string_lossy() }))
            }
            "updater.install" => {
                let opts = options(args);
                let staged = opts["path"].as_str().ok_or("path must be a string")?;
                let restart = opts["restart"].as_bool().unwrap_or(false);
                let backup = crate::updater::install(&self.user_data_dir(), staged, restart)?;
                Ok(json!({ "backup": backup.to_string_lossy() }))
            }
            "clipboard.readText" => platform::clipboard_read_text().map(Value::from),
            "clipboard.writeText" => platform::clipboard_write_text(text_arg(args, 0)?).map(|_| Value::Null),
            "dialog.openFile" => {
                let result = platform::open_file_dialog(&options(args))?;
                self.grant_dialog(&result, false);
                Ok(result)
            }
            "dialog.saveFile" => {
                let result = platform::save_file_dialog(&options(args))?;
                self.grant_dialog(&result, true);
                Ok(result)
            }
            "dialog.message" => platform::message_dialog(&options(args)),
            "shell.openExternal" => platform::open_external(text_arg(args, 0)?).map(|_| Value::Null),
            "net.request" => {
                let opts = options(args);
                let url = opts["url"].as_str().unwrap_or("");
                let method = opts["method"].as_str().unwrap_or("GET");
                // Parse host from url loosely; full validation happens in platform.
                let host = url::Url::parse(url).ok().and_then(|u| u.host_str().map(str::to_owned)).unwrap_or_default();
                let allowed = self.network.iter().any(|(granted_host, methods)| {
                    granted_host.eq_ignore_ascii_case(&host) && (methods.is_empty() || methods.iter().any(|m| m.eq_ignore_ascii_case(method)))
                });
                if !allowed {
                    return Err("PERMISSION_DENIED: Network host is not granted for this host and method".into());
                }
                platform::https_request(&opts, &self.network.iter().map(|(h, _)| h.clone()).collect::<Vec<_>>())
            }
            "power.setBlockers" => {
                platform::set_power_blockers(&platform::string_list(args.first().unwrap_or(&Value::Null)))?;
                Ok(Value::Null)
            }
            "power.state" => Ok(platform::power_snapshot()),
            "systemPreferences.info" => Ok(platform::system_preferences()),
            "safeStorage.isAvailable" | "safeStorage.encrypt" | "safeStorage.decrypt" => platform::safe_storage_call(operation, args),
            "filesystem.readText" => {
                let path = confine(&self.read_roots, text_arg(args, 0)?, true, false)?;
                let bytes = read_capped(&path)?;
                String::from_utf8(bytes).map(Value::from).map_err(|_| "File is not valid UTF-8 text".to_string())
            }
            "filesystem.writeText" => {
                let contents = text_arg(args, 1)?;
                check_size(contents.len())?;
                let path = confine(&self.write_roots, text_arg(args, 0)?, false, false)?;
                fs::write(&path, contents).map_err(io_error)?;
                Ok(Value::Null)
            }
            "filesystem.readBytes" => {
                let path = confine(&self.read_roots, text_arg(args, 0)?, true, false)?;
                let bytes = read_capped(&path)?;
                Ok(Value::Array(bytes.into_iter().map(Value::from).collect()))
            }
            "filesystem.writeBytes" => {
                let bytes = byte_array_arg(args, 1)?;
                check_size(bytes.len())?;
                let path = confine(&self.write_roots, text_arg(args, 0)?, false, false)?;
                fs::write(&path, bytes).map_err(io_error)?;
                Ok(Value::Null)
            }
            "filesystem.list" => {
                let path = confine(&self.read_roots, text_arg(args, 0)?, true, true)?;
                list_directory(&path)
            }
            "filesystem.stat" => {
                let path = confine(&self.read_roots, text_arg(args, 0)?, true, true)?;
                let metadata = fs::metadata(&path).map_err(io_error)?;
                let modified = metadata.modified().ok()
                    .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
                    .map(|elapsed| elapsed.as_millis() as u64);
                Ok(json!({ "kind": kind_of(&metadata.file_type()), "size": metadata.len(), "modified": modified }))
            }
            "filesystem.mkdir" => {
                let path = confine(&self.write_roots, text_arg(args, 0)?, false, false)?;
                fs::create_dir(&path).map_err(io_error)?;
                Ok(Value::Null)
            }
            "filesystem.remove" => {
                // Non-recursive: a directory must be empty before it can be removed.
                let path = confine(&self.write_roots, text_arg(args, 0)?, true, false)?;
                let metadata = fs::metadata(&path).map_err(io_error)?;
                let removed = if metadata.is_dir() { fs::remove_dir(&path) } else { fs::remove_file(&path) };
                removed.map_err(io_error)?;
                Ok(Value::Null)
            }
            "filesystem.rename" => {
                let from = confine(&self.write_roots, text_arg(args, 0)?, true, false)?;
                let to = confine(&self.write_roots, text_arg(args, 1)?, false, false)?;
                fs::rename(&from, &to).map_err(io_error)?;
                Ok(Value::Null)
            }
            "process.spawn" => {
                let command = text_arg(args, 0)?;
                if !self.processes.iter().any(|granted| granted == command) {
                    return Err(format!("PERMISSION_DENIED: Process '{command}' is not listed in permissions.processes"));
                }
                // Optional argument pattern check.
                if let Some(patterns) = self.process_args.get(command) {
                    if !patterns.is_empty() {
                        let extra = match args.get(1) { Some(Value::Array(items)) => items.iter().filter_map(Value::as_str).map(str::to_owned).collect::<Vec<_>>(), _ => Vec::new() };
                        let allowed = patterns.iter().any(|pattern| arg_pattern_matches(pattern, &extra));
                        if !allowed {
                            return Err(format!("PERMISSION_DENIED: Process '{command}' arguments are not allowed by permissions.process_args"));
                        }
                    }
                }
                let extra = match args.get(1) {
                    None | Some(Value::Null) => Vec::new(),
                    Some(Value::Array(items)) => items.iter()
                        .map(|item| item.as_str().map(str::to_owned).ok_or_else(|| "Process arguments must be strings".to_string()))
                        .collect::<Result<Vec<_>, _>>()?,
                    Some(_) => return Err("Process arguments must be an array of strings".into()),
                };
                run_process(command, &extra, &self.base, env)
            }
            "notifications.show" => {
                if !self.notifications {
                    return Err("PERMISSION_DENIED: Notifications are not granted: set permissions.notifications to true".into());
                }
                let options = args.first().and_then(Value::as_object).ok_or("Notification options must be an object")?;
                let title = options.get("title").and_then(Value::as_str).ok_or("Notification title must be a string")?;
                let body = options.get("body").and_then(Value::as_str).unwrap_or("");
                let mut notification = notify_rust::Notification::new();
                notification.appname("Rendrix").summary(title).body(body);
                notification.show().map(|_| Value::Null).map_err(|error| error.to_string())
            }
            // OS tray icons and global hotkeys. The backend is trusted app code, so no grant is
            // needed; only the backend can call these.
            "tray.create" | "tray.setIcon" | "tray.setToolTip" | "tray.setTitle" | "tray.setContextMenu"
            | "tray.popUpContextMenu" | "tray.destroy" | "tray.bounds" => {
                crate::tray::call(operation, args.first().unwrap_or(&Value::Null))
            }
            "globalShortcut.register" | "globalShortcut.unregister" | "globalShortcut.unregisterAll" | "globalShortcut.isRegistered" => {
                crate::hotkeys::call(operation, args.first().unwrap_or(&Value::Null))
            }
            other => Err(format!("Native operation '{other}' is not available in this build")),
        }
    }
}

impl Capabilities {
    /// `nativeImage.*` operations. Arguments are one object, as the WebView bridge sends them:
    /// `{ handle, path, root, base64, width, height, x, y, scaleFactor, quality }`. A handle is
    /// returned by every creation and transform call and addresses the decoded image until it is
    /// evicted from the store. Bitmaps are BGRA, matching Electron and Chromium.
    fn native_image(&self, operation: &str, args: &[Value]) -> Result<Value, String> {
        let options = options(args);
        let scale = image_scale(&options);
        match operation {
            "nativeImage.createFromPath" => {
                let path = self.image_path(&options)?;
                let metadata = fs::metadata(&path).map_err(io_error)?;
                if !metadata.is_file() {
                    return Err("Path is not a file".into());
                }
                if metadata.len() > MAX_IMAGE_BYTES as u64 {
                    return Err(format!("Image exceeds the {} MiB limit", MAX_IMAGE_BYTES / (1024 * 1024)));
                }
                let bytes = fs::read(&path).map_err(io_error)?;
                let image = decode_image(&bytes, None, None)?;
                Ok(store_image_result(image, scale))
            }
            "nativeImage.createFromBuffer" => {
                let encoded = options.get("base64").and_then(Value::as_str).ok_or("base64 must be a string")?;
                let bytes = base64_decode(encoded)?;
                if bytes.is_empty() {
                    return Ok(empty_image_result(scale));
                }
                if bytes.len() > MAX_IMAGE_BYTES {
                    return Err(format!("Image exceeds the {} MiB limit", MAX_IMAGE_BYTES / (1024 * 1024)));
                }
                let width = options.get("width").and_then(Value::as_u64).map(|value| value as u32);
                let height = options.get("height").and_then(Value::as_u64).map(|value| value as u32);
                let image = decode_image(&bytes, width, height)?;
                Ok(store_image_result(image, scale))
            }
            "nativeImage.getSize" => match load_image(image_handle(&options)) {
                Some(image) => Ok(json!({ "width": image.width(), "height": image.height(), "scaleFactor": scale })),
                None => Ok(json!({ "width": 0, "height": 0, "scaleFactor": scale })),
            },
            "nativeImage.getAspectRatio" => {
                let ratio = load_image(image_handle(&options))
                    .filter(|image| image.height() > 0)
                    .map(|image| image.width() as f64 / image.height() as f64)
                    .unwrap_or(0.0);
                Ok(json!(ratio))
            }
            "nativeImage.resize" => {
                let Some(image) = load_image(image_handle(&options)) else { return Ok(empty_image_result(scale)); };
                let requested_width = options.get("width").and_then(Value::as_u64).map(|value| value as u32);
                let requested_height = options.get("height").and_then(Value::as_u64).map(|value| value as u32);
                let (width, height) = match (requested_width, requested_height) {
                    (Some(width), Some(height)) => (width, height),
                    (Some(width), None) => (width, scale_dimension(image.height(), image.width(), width)),
                    (None, Some(height)) => (scale_dimension(image.width(), image.height(), height), height),
                    (None, None) => (image.width(), image.height()),
                };
                if width == 0 || height == 0 {
                    return Ok(empty_image_result(scale));
                }
                if width > MAX_IMAGE_DIMENSION || height > MAX_IMAGE_DIMENSION {
                    return Err(format!("Resized image must be at most {MAX_IMAGE_DIMENSION}x{MAX_IMAGE_DIMENSION}"));
                }
                let filter = match options.get("quality").and_then(Value::as_str).unwrap_or("best") {
                    "good" => image::imageops::FilterType::Triangle,
                    "better" => image::imageops::FilterType::CatmullRom,
                    _ => image::imageops::FilterType::Lanczos3,
                };
                Ok(store_image_result(image.resize_exact(width, height, filter), scale))
            }
            "nativeImage.crop" => {
                let Some(image) = load_image(image_handle(&options)) else { return Ok(empty_image_result(scale)); };
                let x = options.get("x").and_then(Value::as_i64).unwrap_or(0).max(0) as u32;
                let y = options.get("y").and_then(Value::as_i64).unwrap_or(0).max(0) as u32;
                let width = options.get("width").and_then(Value::as_i64).map(|value| value.max(0) as u32).unwrap_or(image.width());
                let height = options.get("height").and_then(Value::as_i64).map(|value| value.max(0) as u32).unwrap_or(image.height());
                let x = x.min(image.width());
                let y = y.min(image.height());
                let width = width.min(image.width() - x);
                let height = height.min(image.height() - y);
                if width == 0 || height == 0 {
                    return Ok(empty_image_result(scale));
                }
                Ok(store_image_result(image.crop_imm(x, y, width, height), scale))
            }
            "nativeImage.toPNG" => {
                let Some(image) = load_image(image_handle(&options)) else { return Ok(json!({ "base64": "" })); };
                Ok(json!({ "base64": base64_encode(&encode_png(&image)?) }))
            }
            "nativeImage.toJPEG" => {
                let Some(image) = load_image(image_handle(&options)) else { return Ok(json!({ "base64": "" })); };
                let quality = options.get("quality").and_then(Value::as_u64).unwrap_or(80).clamp(1, 100) as u8;
                Ok(json!({ "base64": base64_encode(&encode_jpeg(&image, quality)?) }))
            }
            "nativeImage.toDataURL" => {
                let Some(image) = load_image(image_handle(&options)) else { return Ok(json!({ "dataUrl": "data:image/png;base64," })); };
                Ok(json!({ "dataUrl": format!("data:image/png;base64,{}", base64_encode(&encode_png(&image)?)) }))
            }
            "nativeImage.toBitmap" => {
                let Some(image) = load_image(image_handle(&options)) else { return Ok(json!({ "base64": "", "width": 0, "height": 0 })); };
                Ok(json!({ "base64": base64_encode(&encode_bitmap(&image)), "width": image.width(), "height": image.height() }))
            }
            other => Err(format!("Native operation '{other}' is not available in this build")),
        }
    }

    /// Resolves the path for `nativeImage.createFromPath`. `{ root, path }` addresses a named
    /// permission root, `userData` or a dialog grant like `filesystem.*`; a bare path is relative
    /// to a read root or absolute, and it must still resolve inside one.
    fn image_path(&self, options: &Value) -> Result<PathBuf, String> {
        let path = options.get("path").and_then(Value::as_str).ok_or("path must be a string")?;
        if let Some(root) = options.get("root").and_then(Value::as_str) {
            let directory = if root == "userData" {
                self.user_data_dir()
            } else {
                self.read_named.iter().find(|(name, _)| name == root).map(|(_, directory)| directory.clone())
                    .ok_or("PERMISSION_DENIED: Filesystem root is not granted")?
            };
            return confine(&[directory], path, true, false);
        }
        if Path::new(path).is_absolute() {
            let resolved = Path::new(path).canonicalize().map_err(|error| format!("'{path}' could not be opened: {error}"))?;
            let allowed = self.read_roots.iter().any(|root| root.canonicalize().is_ok_and(|root| resolved.starts_with(root)));
            if !allowed {
                return Err(format!("PERMISSION_DENIED: '{path}' is not inside a directory granted in permissions.filesystem"));
            }
            return Ok(resolved);
        }
        confine(&self.read_roots, path, true, false)
    }
}

impl Capabilities {
    /// `filesystem.*` addressed as `{ root, path }`. `root` is a permission root by name, `userData`,
    /// or the name of a file the user picked with a dialog. Paths are relative to that root, and
    /// `rename` takes two locations in the same root. The grant decides whether the call may run.
    fn named_filesystem(&self, operation: &str, args: &[Value]) -> Result<Value, String> {
        let write = matches!(operation, "filesystem.writeText" | "filesystem.writeBytes" | "filesystem.mkdir" | "filesystem.remove" | "filesystem.rename");
        let (root, path) = named_target(&args[0])?;
        let mut positional = vec![Value::String(path.clone())];
        if operation == "filesystem.rename" {
            let (to_root, to_path) = named_target(args.get(1).ok_or("rename needs a destination")?)?;
            if to_root != root {
                return Err("Rename must stay inside one granted root".into());
            }
            positional.push(Value::String(to_path));
        } else {
            positional.extend_from_slice(&args[1..]);
        }
        let picked = self.dialog_grants.lock().unwrap_or_else(|error| error.into_inner()).iter().find(|grant| grant.name == root).cloned();
        if let Some(grant) = picked {
            return dialog_file_operation(&grant, operation, &path, &positional);
        }
        let directory = if root == "userData" {
            Some(self.user_data_dir())
        } else {
            let named = if write { &self.write_named } else { &self.read_named };
            named.iter().find(|(name, _)| *name == root).map(|(_, directory)| directory.clone())
        };
        let directory = directory.ok_or("PERMISSION_DENIED: Filesystem root is not granted")?;
        let mut scoped = self.clone();
        scoped.read_roots = if write { Vec::new() } else { vec![directory.clone()] };
        scoped.write_roots = if write { vec![directory] } else { Vec::new() };
        scoped.call(operation, &positional, &[])
    }

    /// Records files the user picked. An open dialog grants read access, and a save dialog write access.
    fn grant_dialog(&self, result: &Value, write: bool) {
        let paths: Vec<&str> = if write {
            result["path"].as_str().into_iter().collect()
        } else {
            result["paths"].as_array().map(|items| items.iter().filter_map(Value::as_str).collect()).unwrap_or_default()
        };
        let mut grants = self.dialog_grants.lock().unwrap_or_else(|error| error.into_inner());
        for path in paths {
            let name = format!("dialog:{}", grants.len());
            grants.push(DialogGrant { name, path: PathBuf::from(path), write });
        }
    }

    /// The description `app.info` returns: the app, its folders, and every root the backend may use.
    fn app_info(&self) -> Value {
        let home = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from).unwrap_or_default();
        let user_data = self.user_data_dir();
        let picked: Vec<DialogGrant> = self.dialog_grants.lock().unwrap_or_else(|error| error.into_inner()).clone();
        let mut roots: Vec<Value> = self.read_named.iter().map(|(name, path)| json!({"name": name, "path": path.to_string_lossy(), "access": "read"})).collect();
        roots.extend(self.write_named.iter().map(|(name, path)| json!({"name": name, "path": path.to_string_lossy(), "access": "write"})));
        roots.push(json!({"name": "userData", "path": user_data.to_string_lossy(), "access": "write"}));
        roots.extend(picked.iter().map(|grant| json!({"name": grant.name, "path": grant.path.to_string_lossy(), "access": if grant.write { "write" } else { "read" }, "exact": true})));
        json!({
            "name": self.app.name,
            "version": self.app.version,
            "platform": std::env::consts::OS,
            "paths": {
                "home": home.to_string_lossy(),
                "userData": user_data.to_string_lossy(),
                "documents": home.join("Documents").to_string_lossy(),
                "downloads": home.join("Downloads").to_string_lossy(),
                "temp": std::env::temp_dir().to_string_lossy(),
                "appFolder": self.base.to_string_lossy(),
            },
            "roots": roots,
        })
    }
}

/// Splits a `{ root, path }` location. A missing path names the root itself.
fn named_target(value: &Value) -> Result<(String, String), String> {
    let root = value["root"].as_str().ok_or("File location needs a root name")?.to_owned();
    let path = value["path"].as_str().unwrap_or("").to_owned();
    Ok((root, path))
}

/// Operations on a file the user picked. The grant covers that one file, so `path` must be empty.
fn dialog_file_operation(grant: &DialogGrant, operation: &str, path: &str, positional: &[Value]) -> Result<Value, String> {
    let write = matches!(operation, "filesystem.writeText" | "filesystem.writeBytes" | "filesystem.mkdir" | "filesystem.remove" | "filesystem.rename");
    if write && !grant.write {
        return Err("PERMISSION_DENIED: Filesystem root is not granted for writing".into());
    }
    if !path.is_empty() {
        return Err("Dialog grant covers only the selected file".into());
    }
    match operation {
        "filesystem.readText" => {
            let bytes = read_capped(&grant.path)?;
            String::from_utf8(bytes).map(Value::from).map_err(|_| "File is not valid UTF-8 text".to_string())
        }
        "filesystem.readBytes" => Ok(Value::Array(read_capped(&grant.path)?.into_iter().map(Value::from).collect())),
        "filesystem.writeText" => {
            let contents = text_arg(positional, 1)?;
            check_size(contents.len())?;
            fs::write(&grant.path, contents).map_err(io_error)?;
            Ok(Value::Null)
        }
        "filesystem.writeBytes" => {
            let bytes = byte_array_arg(positional, 1)?;
            check_size(bytes.len())?;
            fs::write(&grant.path, bytes).map_err(io_error)?;
            Ok(Value::Null)
        }
        _ => Err("Operation is not supported for a dialog grant".into()),
    }
}

/// The first argument of an object-style operation, or an empty object when none was given.
fn options(args: &[Value]) -> Value {
    args.first().cloned().unwrap_or_else(|| json!({}))
}

/// Resolves `relative` under one of `roots`. Paths are relative to the granted directory, so
/// `notes.txt` means `<root>/notes.txt`. An empty path or `.` names the root itself, which only
/// read-only calls accept (`allow_root`). Every symlink must stay inside the root. `must_exist` is
/// false for paths about to be created; their parent must still exist inside the root.
fn confine(roots: &[PathBuf], relative: &str, must_exist: bool, allow_root: bool) -> Result<PathBuf, String> {
    let relative_path = Path::new(relative);
    let plain = relative_path.components().all(|part| matches!(part, Component::Normal(_) | Component::CurDir));
    if !plain {
        return Err(format!("Path '{relative}' must be relative to a granted directory and must not contain '..'"));
    }
    let names_root = relative_path.components().all(|part| matches!(part, Component::CurDir));
    if names_root && !allow_root {
        return Err(format!("Path '{relative}' must name a file or folder inside a granted directory"));
    }
    for root in roots {
        let Ok(root) = root.canonicalize() else { continue };
        let candidate = root.join(relative_path);
        match candidate.canonicalize() {
            Ok(resolved) => {
                if resolved.starts_with(&root) && (resolved != root || allow_root) {
                    return Ok(resolved);
                }
            }
            Err(_) if !must_exist => {
                // The target is new. Its parent must already exist inside the root, and the new
                // name must not be an existing symlink (a symlink target would be written instead).
                let parent_inside = candidate.parent()
                    .and_then(|parent| parent.canonicalize().ok())
                    .is_some_and(|parent| parent.starts_with(&root));
                let dangling_link = fs::symlink_metadata(&candidate).is_ok();
                if parent_inside && !dangling_link {
                    return Ok(candidate);
                }
            }
            Err(_) => {}
        }
    }
    Err(format!("'{relative}' is not inside a directory granted in permissions.filesystem"))
}

pub(crate) fn text_arg(args: &[Value], index: usize) -> Result<&str, String> {
    args.get(index).and_then(Value::as_str).ok_or_else(|| format!("Argument {} must be a string", index + 1))
}

fn byte_array_arg(args: &[Value], index: usize) -> Result<Vec<u8>, String> {
    let items = args.get(index).and_then(Value::as_array)
        .ok_or_else(|| format!("Argument {} must be an array of bytes", index + 1))?;
    items.iter()
        .map(|item| item.as_u64().and_then(|byte| u8::try_from(byte).ok()).ok_or_else(|| "Byte values must be 0 to 255".to_string()))
        .collect()
}

fn check_size(length: usize) -> Result<(), String> {
    if length as u64 > MAX_FILE_BYTES {
        Err("File exceeds the 1 MiB limit".into())
    } else {
        Ok(())
    }
}

fn read_capped(path: &Path) -> Result<Vec<u8>, String> {
    use std::io::Read;
    let metadata = fs::metadata(path).map_err(io_error)?;
    if !metadata.is_file() {
        return Err("Path is not a file".into());
    }
    check_size(metadata.len() as usize)?;
    let mut file = fs::File::open(path).map_err(io_error)?;
    let mut kept = Vec::new();
    let mut chunk = [0u8; 8192];
    loop {
        if current_cancel_token().map(|t| t.load(std::sync::atomic::Ordering::SeqCst)).unwrap_or(false) {
            return Err("CANCELLED: file read was cancelled".into());
        }
        match file.read(&mut chunk) {
            Ok(0) => break,
            Ok(read) => kept.extend_from_slice(&chunk[..read]),
            Err(error) => return Err(io_error(error)),
        }
    }
    Ok(kept)
}

fn list_directory(path: &Path) -> Result<Value, String> {
    let mut entries = Vec::new();
    for entry in fs::read_dir(path).map_err(io_error)? {
        let entry = entry.map_err(io_error)?;
        if entries.len() == MAX_LIST_ENTRIES {
            return Err(format!("Directory has more than {MAX_LIST_ENTRIES} entries"));
        }
        let metadata = entry.metadata().map_err(io_error)?;
        entries.push(json!({
            "name": entry.file_name().to_string_lossy(),
            "kind": kind_of(&metadata.file_type()),
            "size": metadata.len(),
        }));
    }
    entries.sort_by(|a, b| a["name"].as_str().cmp(&b["name"].as_str()));
    Ok(Value::Array(entries))
}

fn kind_of(file_type: &fs::FileType) -> &'static str {
    if file_type.is_dir() {
        "directory"
    } else if file_type.is_file() {
        "file"
    } else {
        "other"
    }
}

fn io_error(error: std::io::Error) -> String {
    error.to_string()
}

/// Runs an allowlisted executable with explicit arguments. There is no shell, so arguments are not
/// expanded or split. Output is capped per stream, and the process is killed after the timeout.
fn run_process(command: &str, args: &[String], working_dir: &Path, env: &[(String, String)]) -> Result<Value, String> {
    if command.contains(['/', '\\']) {
        return Err("Process names in permissions.processes must not contain path separators".into());
    }
    let path_var = std::env::var_os("PATH").unwrap_or_default();
    let pathext = std::env::var("PATHEXT").unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".into());
    let program = resolve_in(command, &path_var, &pathext);
    let mut child = Command::new(&program)
        .args(args)
        .envs(env.iter().map(|(key, value)| (key.as_str(), value.as_str())))
        .current_dir(working_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("Unable to start '{command}': {error}"))?;
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let stdout_reader = thread::spawn(move || read_stream(stdout));
    let stderr_reader = thread::spawn(move || read_stream(stderr));

    let deadline = Instant::now() + PROCESS_TIMEOUT;
    let status = loop {
        if current_cancel_token().map(|t| t.load(std::sync::atomic::Ordering::SeqCst)).unwrap_or(false) {
            let _ = child.kill();
            let _ = child.wait();
            return Err("CANCELLED: process was cancelled".into());
        }
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("'{command}' did not finish within {} seconds", PROCESS_TIMEOUT.as_secs()));
            }
            Ok(None) => thread::sleep(Duration::from_millis(10)),
            Err(error) => return Err(error.to_string()),
        }
    };
    let stdout = stdout_reader.join().unwrap_or_default();
    let stderr = stderr_reader.join().unwrap_or_default();
    Ok(json!({
        "code": status.code(),
        "stdout": String::from_utf8_lossy(&stdout),
        "stderr": String::from_utf8_lossy(&stderr),
    }))
}

/// Finds the file a command name refers to. Windows only runs `.exe` names directly, so `npm`
/// must be resolved to `npm.cmd` through `PATHEXT` first. Batch files are then run by Rust's
/// `Command`, which refuses arguments it cannot quote safely. Other names are left to the OS.
fn resolve_in(command: &str, path_var: &std::ffi::OsStr, pathext: &str) -> std::ffi::OsString {
    if !cfg!(windows) {
        return command.into();
    }
    let extensions: Vec<&str> = pathext.split(';').filter(|ext| !ext.is_empty()).collect();
    for dir in std::env::split_paths(path_var) {
        let direct = dir.join(command);
        if Path::new(command).extension().is_some() && direct.is_file() {
            return direct.into_os_string();
        }
        for extension in &extensions {
            let candidate = dir.join(format!("{command}{extension}"));
            if candidate.is_file() {
                return candidate.into_os_string();
            }
        }
    }
    command.into()
}

/// Keeps the first `MAX_FILE_BYTES` of a stream but keeps reading to the end, so a chatty process
/// never blocks on a full pipe.
fn read_stream(stream: Option<impl Read>) -> Vec<u8> {
    let mut kept = Vec::new();
    let Some(mut stream) = stream else { return kept };
    let mut chunk = [0u8; 8192];
    loop {
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(read) => {
                let room = (MAX_FILE_BYTES as usize).saturating_sub(kept.len());
                kept.extend_from_slice(&chunk[..read.min(room)]);
            }
        }
    }
    kept
}

/// Simple wildcard matching for process argument patterns. A pattern token of "*" matches any single
/// argument token; otherwise tokens must match exactly.
fn arg_pattern_matches(pattern: &str, args: &[String]) -> bool {
    let pattern_tokens: Vec<&str> = pattern.split_whitespace().collect();
    if pattern_tokens.len() != args.len() {
        return false;
    }
    pattern_tokens.iter().zip(args.iter()).all(|(p, a)| *p == "*" || *p == a)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::SystemTime;

    use crate::package::FilesystemPermissions;

    fn temp_dir(name: &str) -> PathBuf {
        let stamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let dir = std::env::temp_dir().join(format!("rendrix-cap-{name}-{stamp}"));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn grants(base: &Path, read: &[&str], write: &[&str], processes: &[&str]) -> Capabilities {
        let permissions = Permissions {
            filesystem: FilesystemPermissions {
                read: read.iter().map(|s| s.to_string()).collect(),
                write: write.iter().map(|s| s.to_string()).collect(),
            },
            processes: processes.iter().map(|s| s.to_string()).collect(),
            ..Permissions::default()
        };
        Capabilities::from_permissions(&permissions, base.to_path_buf())
    }

    fn call(caps: &Capabilities, op: &str, args: Value) -> Result<Value, String> {
        caps.call(op, args.as_array().unwrap(), &[])
    }

    #[test]
    fn notifications_need_their_grant() {
        let base = temp_dir("notify");
        let caps = grants(&base, &[], &[], &[]);
        let error = call(&caps, "notifications.show", json!([{ "title": "hi" }])).unwrap_err();
        assert!(error.contains("permissions.notifications"), "{error}");
    }

    #[test]
    fn command_names_resolve_through_pathext() {
        let dir = temp_dir("resolve");
        fs::write(dir.join("tool.cmd"), "@echo off").unwrap();
        let resolved = resolve_in("tool", dir.as_os_str(), ".EXE;.CMD");
        if cfg!(windows) {
            // Windows file names are case-insensitive, and PATHEXT may give the extension in capitals.
            assert_eq!(resolved.to_string_lossy().to_lowercase(), dir.join("tool.cmd").to_string_lossy().to_lowercase());
        } else {
            assert_eq!(resolved, std::ffi::OsString::from("tool"));
        }
        assert_eq!(resolve_in("missing", dir.as_os_str(), ".EXE;.CMD"), std::ffi::OsString::from("missing"));
    }

    #[test]
    fn writes_and_reads_text_inside_a_granted_root() {
        let base = temp_dir("rw");
        let caps = grants(&base, &["data"], &["data"], &[]);
        call(&caps, "filesystem.writeText", json!(["note.txt", "hello"])).unwrap();
        assert_eq!(call(&caps, "filesystem.readText", json!(["note.txt"])).unwrap(), json!("hello"));
    }

    #[test]
    fn rejects_parent_traversal_and_absolute_paths() {
        let base = temp_dir("traversal");
        let caps = grants(&base, &["data"], &["data"], &[]);
        assert!(call(&caps, "filesystem.writeText", json!(["../escape.txt", "x"])).is_err());
        assert!(call(&caps, "filesystem.readText", json!(["../../etc"])).is_err());
        let absolute = base.join("abs.txt").to_string_lossy().into_owned();
        assert!(call(&caps, "filesystem.writeText", json!([absolute, "x"])).is_err());
        assert!(!base.join("escape.txt").exists());
    }

    #[test]
    fn denies_paths_outside_the_granted_roots() {
        let base = temp_dir("roots");
        fs::create_dir_all(base.join("private")).unwrap();
        fs::write(base.join("private/secret.txt"), "s").unwrap();
        let caps = grants(&base, &["data"], &[], &[]);
        assert!(call(&caps, "filesystem.readText", json!(["private/secret.txt"])).is_err());
    }

    #[test]
    fn read_only_root_cannot_be_written() {
        let base = temp_dir("readonly");
        fs::create_dir_all(base.join("data")).unwrap();
        fs::write(base.join("data/a.txt"), "a").unwrap();
        let caps = grants(&base, &["data"], &[], &[]);
        assert_eq!(call(&caps, "filesystem.readText", json!(["a.txt"])).unwrap(), json!("a"));
        assert!(call(&caps, "filesystem.writeText", json!(["b.txt", "b"])).is_err());
        assert!(call(&caps, "filesystem.remove", json!(["a.txt"])).is_err());
    }

    #[test]
    fn lists_stats_creates_renames_and_removes() {
        let base = temp_dir("manage");
        let caps = grants(&base, &["files"], &["files"], &[]);
        call(&caps, "filesystem.mkdir", json!(["docs"])).unwrap();
        call(&caps, "filesystem.writeText", json!(["b.txt", "bb"])).unwrap();
        call(&caps, "filesystem.writeText", json!(["a.txt", "a"])).unwrap();
        let listing = call(&caps, "filesystem.list", json!([""])).unwrap();
        let names: Vec<&str> = listing.as_array().unwrap().iter().map(|e| e["name"].as_str().unwrap()).collect();
        assert_eq!(names, ["a.txt", "b.txt", "docs"]);
        assert_eq!(call(&caps, "filesystem.stat", json!(["b.txt"])).unwrap()["size"], json!(2));
        call(&caps, "filesystem.rename", json!(["a.txt", "c.txt"])).unwrap();
        call(&caps, "filesystem.remove", json!(["c.txt"])).unwrap();
        assert!(call(&caps, "filesystem.stat", json!(["c.txt"])).is_err());
    }

    #[test]
    fn binary_round_trip_uses_byte_arrays() {
        let base = temp_dir("bytes");
        let caps = grants(&base, &["data"], &["data"], &[]);
        call(&caps, "filesystem.writeBytes", json!(["x.bin", [0, 255, 7]])).unwrap();
        assert_eq!(call(&caps, "filesystem.readBytes", json!(["x.bin"])).unwrap(), json!([0, 255, 7]));
        assert!(call(&caps, "filesystem.writeBytes", json!(["y.bin", [256]])).is_err());
    }

    #[test]
    fn enforces_the_one_mebibyte_limit() {
        let base = temp_dir("limit");
        let caps = grants(&base, &["data"], &["data"], &[]);
        let big = "x".repeat(MAX_FILE_BYTES as usize + 1);
        assert!(call(&caps, "filesystem.writeText", json!(["big.txt", big])).is_err());
    }

    #[test]
    fn processes_must_be_granted_by_name() {
        let base = temp_dir("proc");
        let caps = grants(&base, &[], &[], &["definitely-not-granted"]);
        let error = call(&caps, "process.spawn", json!(["cargo", ["--version"]])).unwrap_err();
        assert!(error.contains("not listed in permissions.processes"), "{error}");
    }

    #[test]
    fn granted_process_runs_without_a_shell() {
        let base = temp_dir("run");
        let (name, args) = if cfg!(windows) { ("cmd", json!(["/C", "echo hi"])) } else { ("sh", json!(["-c", "echo hi"])) };
        let caps = grants(&base, &[], &[], &[name]);
        let result = call(&caps, "process.spawn", json!([name, args])).unwrap();
        assert_eq!(result["code"], json!(0));
        assert!(result["stdout"].as_str().unwrap().contains("hi"));
    }

    #[test]
    fn unknown_operations_are_reported() {
        let base = temp_dir("unknown");
        let caps = grants(&base, &[], &[], &[]);
        assert!(call(&caps, "window.minimize", json!([])).is_err());
    }

    #[test]
    fn named_roots_need_a_grant_and_stay_inside_the_root() {
        let base = temp_dir("named");
        let caps = grants(&base, &["data"], &["data"], &[]);
        call(&caps, "filesystem.writeText", json!([{"root": "data", "path": "a.txt"}, "hi"])).unwrap();
        assert_eq!(call(&caps, "filesystem.readText", json!([{"root": "data", "path": "a.txt"}])).unwrap(), json!("hi"));
        assert!(call(&caps, "filesystem.readText", json!([{"root": "other", "path": "a.txt"}])).unwrap_err().contains("not granted"));
        assert!(call(&caps, "filesystem.readText", json!([{"root": "data", "path": "../a.txt"}])).is_err());
        assert!(base.join("data/a.txt").exists());
    }

    #[test]
    fn a_read_only_root_refuses_named_writes() {
        let base = temp_dir("named-read-only");
        let caps = grants(&base, &["data"], &[], &[]);
        assert!(call(&caps, "filesystem.writeText", json!([{"root": "data", "path": "a.txt"}, "hi"])).unwrap_err().contains("not granted"));
    }

    #[test]
    fn renames_must_stay_in_one_named_root() {
        let base = temp_dir("named-rename");
        let caps = grants(&base, &[], &["data", "other"], &[]);
        call(&caps, "filesystem.mkdir", json!([{"root": "data", "path": "d"}])).unwrap();
        let moved = call(&caps, "filesystem.rename", json!([{"root": "data", "path": "d"}, {"root": "other", "path": "d"}]));
        assert!(moved.unwrap_err().contains("one granted root"));
    }

    #[test]
    fn dialog_grants_cover_only_the_picked_file() {
        let base = temp_dir("dialog");
        let picked = base.join("picked.txt");
        fs::write(&picked, "chosen").unwrap();
        let caps = grants(&base, &[], &[], &[]);
        caps.grant_dialog(&json!({"canceled": false, "paths": [picked.to_string_lossy()]}), false);
        assert_eq!(call(&caps, "filesystem.readText", json!([{"root": "dialog:0", "path": ""}])).unwrap(), json!("chosen"));
        assert!(call(&caps, "filesystem.readText", json!([{"root": "dialog:0", "path": "other.txt"}])).is_err());
        assert!(call(&caps, "filesystem.writeText", json!([{"root": "dialog:0", "path": ""}, "x"])).unwrap_err().contains("not granted for writing"));
        assert_eq!(fs::read_to_string(&picked).unwrap(), "chosen");
        let info = call(&caps, "app.info", json!([])).unwrap();
        assert!(info["roots"].as_array().unwrap().iter().any(|root| root["name"] == json!("dialog:0") && root["exact"] == json!(true)));
    }

    #[test]
    fn app_info_lists_permission_roots_and_user_data() {
        let base = temp_dir("app-info");
        let caps = grants(&base, &["data"], &["out"], &[]).with_app("Example", "1.2.3");
        let info = call(&caps, "app.info", json!([])).unwrap();
        assert_eq!(info["name"], json!("Example"));
        assert_eq!(info["version"], json!("1.2.3"));
        let names: Vec<&str> = info["roots"].as_array().unwrap().iter().filter_map(|root| root["name"].as_str()).collect();
        assert_eq!(names, vec!["data", "out", "userData"]);
    }

    #[test]
    fn native_image_round_trips_pixels_through_the_store() {
        let caps = Capabilities::default();
        let source = DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(4, 3, image::Rgba([10, 20, 30, 255])));
        let png = encode_png(&source).unwrap();
        let created = call(&caps, "nativeImage.createFromBuffer", json!([{ "base64": base64_encode(&png) }])).unwrap();
        assert_eq!((created["width"].as_u64(), created["height"].as_u64()), (Some(4), Some(3)));
        let handle = created["handle"].as_u64().unwrap();
        assert_eq!(call(&caps, "nativeImage.getSize", json!([{ "handle": handle }])).unwrap()["width"], json!(4));
        let ratio = call(&caps, "nativeImage.getAspectRatio", json!([{ "handle": handle }])).unwrap();
        assert!((ratio.as_f64().unwrap() - 4.0 / 3.0).abs() < 1e-9);
        let resized = call(&caps, "nativeImage.resize", json!([{ "handle": handle, "width": 2, "height": 2 }])).unwrap();
        assert_eq!((resized["width"].as_u64(), resized["height"].as_u64()), (Some(2), Some(2)));
        // One dimension keeps the aspect ratio: 4x3 resized to width 2 becomes 2x2.
        let proportional = call(&caps, "nativeImage.resize", json!([{ "handle": handle, "width": 2 }])).unwrap();
        assert_eq!((proportional["width"].as_u64(), proportional["height"].as_u64()), (Some(2), Some(2)));
        let cropped = call(&caps, "nativeImage.crop", json!([{ "handle": handle, "x": 1, "y": 1, "width": 2, "height": 2 }])).unwrap();
        assert_eq!((cropped["width"].as_u64(), cropped["height"].as_u64()), (Some(2), Some(2)));
        let png_out = call(&caps, "nativeImage.toPNG", json!([{ "handle": resized["handle"] }])).unwrap();
        let decoded = image::load_from_memory(&base64_decode(png_out["base64"].as_str().unwrap()).unwrap()).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (2, 2));
        let jpeg_out = call(&caps, "nativeImage.toJPEG", json!([{ "handle": handle, "quality": 90 }])).unwrap();
        let jpeg_bytes = base64_decode(jpeg_out["base64"].as_str().unwrap()).unwrap();
        assert_eq!(&jpeg_bytes[..2], &[0xff, 0xd8]);
        let data_url = call(&caps, "nativeImage.toDataURL", json!([{ "handle": handle }])).unwrap();
        assert!(data_url["dataUrl"].as_str().unwrap().starts_with("data:image/png;base64,"));
    }

    #[test]
    fn native_image_bitmaps_are_bgra_and_paths_respect_grants() {
        let base = temp_dir("image");
        fs::create_dir_all(base.join("data")).unwrap();
        let caps = grants(&base, &["data"], &[], &[]);
        // One opaque BGRA pixel followed by three transparent ones.
        let bitmap = vec![30u8, 20, 10, 255, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        let created = call(&caps, "nativeImage.createFromBuffer", json!([{ "base64": base64_encode(&bitmap), "width": 2, "height": 2 }])).unwrap();
        assert_eq!(created["width"], json!(2));
        let rgba = load_image(created["handle"].as_u64().unwrap() as usize).unwrap().to_rgba8();
        assert_eq!(rgba.get_pixel(0, 0).0, [10, 20, 30, 255]);
        let png = encode_png(&DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(2, 2, image::Rgba([1, 2, 3, 255])))).unwrap();
        fs::write(base.join("data/icon.png"), &png).unwrap();
        let named = call(&caps, "nativeImage.createFromPath", json!([{ "root": "data", "path": "icon.png" }])).unwrap();
        assert_eq!(named["width"], json!(2));
        let absolute = base.join("data/icon.png").to_string_lossy().into_owned();
        assert!(call(&caps, "nativeImage.createFromPath", json!([{ "path": absolute }])).is_ok());
        fs::write(base.join("outside.png"), &png).unwrap();
        let outside = base.join("outside.png").to_string_lossy().into_owned();
        let error = call(&caps, "nativeImage.createFromPath", json!([{ "path": outside }])).unwrap_err();
        assert!(error.contains("PERMISSION_DENIED"), "{error}");
    }
}
