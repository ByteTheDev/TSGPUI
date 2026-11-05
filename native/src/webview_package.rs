use std::{collections::BTreeMap,fs::File,io::{Read,Seek,SeekFrom},path::PathBuf};
use serde::Deserialize;

pub const MAGIC: &[u8;8] = b"TSGPUI01";
pub const MAX_PAYLOAD: u64 = 1024 * 1024 * 1024;
/// Persisted backend state is small by design; larger values are not written.
pub const MAX_STATE_BYTES: u64 = 256 * 1024;

#[derive(Debug, Clone, Deserialize)]
pub struct Node {
    pub tag: String,
    pub key: u64,
    #[serde(default)] pub text: String,
    #[serde(default)] pub attrs: BTreeMap<String,String>,
    #[serde(default)] pub props: BTreeMap<String,String>,
    #[serde(default)] pub style: BTreeMap<String,String>,
    // Declarations that apply while a pointer, focus, or disabled state holds: "hover", "active", "focus", "disabled".
    #[serde(default)] pub states: BTreeMap<String, BTreeMap<String,String>>,
    #[serde(default)] pub responsive: Vec<ResponsiveRule>,
    #[serde(default)] pub children: Vec<Node>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ResponsiveRule {
    pub media: String,
    pub specificity: u32,
    pub order: u32,
    pub declarations: BTreeMap<String,String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct EmbeddedAsset {
    pub format: String,
    pub data: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WebviewAsset {
    pub mime: String,
    pub data: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Permissions {
    #[serde(default)] pub network_hosts: Vec<String>,
    #[serde(default)] pub filesystem: FilesystemPermissions,
    #[serde(default)] pub processes: Vec<String>,
    #[serde(default)] pub notifications: bool,
    #[serde(default)] pub ipc: bool,
    #[serde(default)] pub window: bool,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct FilesystemPermissions {
    #[serde(default)] pub read: Vec<String>,
    #[serde(default)] pub write: Vec<String>,
}

// "system" keeps the OS title bar. "unified" hides it, and the app marks its drag area and window
// buttons. "blur" asks for a blurred backdrop; "opaque" is the default.
#[derive(Debug, Clone, Deserialize)]
pub struct WindowChrome {
    #[serde(default = "system_titlebar")] pub titlebar: String,
    #[serde(default = "opaque_backdrop")] pub backdrop: String,
}

fn system_titlebar() -> String { "system".into() }
fn opaque_backdrop() -> String { "opaque".into() }

impl Default for WindowChrome {
    fn default() -> Self {
        Self { titlebar: system_titlebar(), backdrop: opaque_backdrop() }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct AppPackage {
    pub format: String,
    #[serde(default = "native_renderer")] pub renderer: String,
    pub name: String,
    #[serde(default = "default_version")] pub version: String,
    pub title: String,
    pub width: i32,
    pub height: i32,
    pub root: Node,
    pub backend_js: String,
    #[serde(default)] pub webview_html: Option<String>,
    #[serde(default)] pub webview_files: BTreeMap<String, WebviewAsset>,
    #[serde(default)] pub webview_entry: Option<String>,
    #[serde(default)] pub webview_format_version: u32,
    /// "hidden" runs the backend in an invisible window, so the primary window can close while others stay open.
    #[serde(default)] pub backend_window: Option<String>,
    /// The backend document loaded by the hidden backend window.
    #[serde(default)] pub webview_main_html: Option<String>,
    /// Page that runs the backend when `hidden_backend` is set. Renderer pages then carry no backend code.
    #[serde(default)] pub webview_backend_html: Option<String>,
    /// Runs the backend in an invisible page, so closing windows does not stop IPC handlers.
    #[serde(default)] pub hidden_backend: bool,
    #[serde(default)] pub assets: BTreeMap<String,EmbeddedAsset>,
    #[serde(default)] pub fonts: BTreeMap<String,String>,
    // Preferences for the development overlay and for persisting backend state between runs.
    #[serde(default)] pub persist: bool,
    #[serde(default)] pub dev: bool,
    #[serde(default)] pub diagnostics: Vec<String>,
    #[serde(default)] pub permissions: Permissions,
    #[serde(default)] pub window: WindowChrome,
}

fn native_renderer() -> String { "native".into() }
fn default_version() -> String { "1.0.0".into() }

pub fn load() -> Result<AppPackage,String> {
    if let Some(file) = bundle_path()? {
        load_bundle(&file)
    } else {
        parse_bundle(read_embedded()?)
    }
}

pub fn bundle_path() -> Result<Option<PathBuf>,String> {
    let args: Vec<String> = std::env::args().collect();
    if let Some(i) = args.iter().position(|arg| arg == "--bundle") {
        let file = args.get(i+1).ok_or("--bundle needs a .tgp path")?;
        Ok(Some(PathBuf::from(file)))
    } else {
        Ok(None)
    }
}

/// Per-user location for opt-in persisted backend state. The app name is sanitized so a bundle
/// cannot escape the configuration directory.
pub fn state_file(name: &str) -> Option<PathBuf> {
    let safe: String = name.chars()
        .map(|character| if character.is_ascii_alphanumeric() || character == '_' || character == '-' { character } else { '_' })
        .collect();
    let safe = if safe.is_empty() { "app".to_string() } else { safe };
    let root = if cfg!(target_os = "windows") {
        std::env::var_os("APPDATA").map(PathBuf::from)
    } else if cfg!(target_os = "macos") {
        std::env::var_os("HOME").map(|home| PathBuf::from(home).join("Library/Application Support"))
    } else {
        std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
    };
    root.map(|root| root.join("tsgpui").join(safe).join("state.json"))
}

pub fn load_bundle(file: &std::path::Path) -> Result<AppPackage,String> {
    let bytes = std::fs::read(file).map_err(|e| format!("Unable to read bundle {}: {e}",file.display()))?;
    parse_bundle(bytes)
}

fn parse_bundle(bytes: Vec<u8>) -> Result<AppPackage,String> {
    let doc: AppPackage = serde_json::from_slice(&bytes)
        .map_err(|e| format!("Invalid .tgp document: {e}"))?;
    if doc.format != "tsgpui-1" {return Err(format!("Incompatible bundle format: {}",doc.format));}
    if !(200..=4096).contains(&doc.width) || !(200..=4096).contains(&doc.height) {
        return Err("Invalid window dimensions".into());
    }
    if !["native", "webview"].contains(&doc.renderer.as_str()) {return Err(format!("Unknown renderer: {}",doc.renderer));}
    if doc.name.is_empty() || doc.renderer == "native" && doc.root.tag.is_empty() {return Err("Invalid document content".into());}
    if doc.renderer == "webview" && doc.webview_html.as_ref().is_none_or(String::is_empty) {return Err("WebView app bundle is missing webview_html".into());}
    if doc.backend_window.as_deref().is_some_and(|w| w != "primary" && w != "hidden") {return Err(format!("Unknown backend window: {}", doc.backend_window.as_deref().unwrap_or_default()));}
    if doc.backend_window.as_deref() == Some("hidden") && (doc.renderer != "webview" || doc.webview_main_html.as_ref().is_none_or(String::is_empty)) {return Err("Hidden backend window requires a WebView bundle with webview_main_html".into());}
    if doc.renderer == "webview" && doc.hidden_backend && doc.webview_backend_html.as_ref().is_none_or(String::is_empty) {return Err("WebView app bundle has a hidden backend but no webview_backend_html".into());}
    Ok(doc)
}
fn read_embedded() -> Result<Vec<u8>,String> {
    let exe: PathBuf = std::env::current_exe().map_err(|e| e.to_string())?;
    #[cfg(target_os = "macos")]
    if let Some(resources) = exe.parent().and_then(|dir| dir.parent()).map(|contents| contents.join("Resources/tsgpui.tgp")) {
        if resources.is_file() { return std::fs::read(&resources).map_err(|e| format!("Unable to read app resource: {e}")); }
    }
    let mut f = File::open(&exe).map_err(|e| format!("Unable to open host: {e}"))?;
    read_overlay(&mut f)
}

// Authenticode places the PE certificate table after the original file, with up to seven
// zero padding bytes. Read only at the end or immediately before that table; never scan the
// signature for a magic string.
fn read_overlay<R: Read + Seek>(f: &mut R) -> Result<Vec<u8>,String> {
    let total = f.seek(SeekFrom::End(0)).map_err(|e| e.to_string())?;
    match read_footer_at(f, total) { Ok(payload) => return Ok(payload), Err(error) if error.starts_with("Invalid embedded") => return Err(error), _ => {} }
    if let Some(cert_offset) = pe_certificate_offset(f, total) {
        for padding in 0..=7 {
            if cert_offset < padding + 16 { break; }
            if padding > 0 {
                f.seek(SeekFrom::Start(cert_offset - padding)).map_err(|e| e.to_string())?;
                let mut bytes = vec![0; padding as usize];
                f.read_exact(&mut bytes).map_err(|e| e.to_string())?;
                if bytes.iter().any(|&b| b != 0) { continue; }
            }
            if let Ok(payload) = read_footer_at(f, cert_offset - padding) { return Ok(payload); }
        }
    }
    Err("No embedded app bundle. Use --bundle file.tgp".into())
}
fn read_footer_at<R: Read + Seek>(f: &mut R, end: u64) -> Result<Vec<u8>,String> {
    if end < 16 { return Err("No embedded app bundle. Use --bundle file.tgp".into()); }
    f.seek(SeekFrom::Start(end - 16)).map_err(|e| e.to_string())?;
    let mut footer=[0u8;16]; f.read_exact(&mut footer).map_err(|e| e.to_string())?;
    if &footer[8..] != MAGIC { return Err("No embedded app bundle. Use --bundle file.tgp".into()); }
    let count = u64::from_le_bytes(footer[0..8].try_into().unwrap());
    if count == 0 || count > MAX_PAYLOAD || count > end-16 { return Err("Invalid embedded app bundle length".into()); }
    f.seek(SeekFrom::Start(end-16-count)).map_err(|e| e.to_string())?;
    let mut buffer=vec![0;count as usize]; f.read_exact(&mut buffer).map_err(|e| e.to_string())?;
    Ok(buffer)
}
fn pe_certificate_offset<R: Read + Seek>(f: &mut R, total: u64) -> Option<u64> {
    fn read_at<R: Read + Seek, const N: usize>(f: &mut R, pos:u64) -> Option<[u8;N]> {
        f.seek(SeekFrom::Start(pos)).ok()?; let mut data=[0;N]; f.read_exact(&mut data).ok()?; Some(data)
    }
    if read_at::<_,2>(f,0)? != *b"MZ" { return None; }
    let pe=u32::from_le_bytes(read_at(f,0x3c)?) as u64;
    if pe.checked_add(24)? > total || read_at::<_,4>(f,pe)? != *b"PE\0\0" { return None; }
    let optional_size=u16::from_le_bytes(read_at(f,pe+20)?) as u64;
    let optional=pe+24;
    let directory=match u16::from_le_bytes(read_at(f,optional)?) { 0x10b=>96, 0x20b=>112, _=>return None };
    if optional_size < directory+40 || optional.checked_add(optional_size)? > total { return None; }
    let entry=read_at::<_,8>(f,optional+directory+32)?;
    let offset=u32::from_le_bytes(entry[..4].try_into().ok()?) as u64;
    let size=u32::from_le_bytes(entry[4..].try_into().ok()?) as u64;
    if offset == 0 || size == 0 || offset.checked_add(size)? > total { return None; }
    Some(offset)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    const VALID_DOC: &str = r#"{"format":"tsgpui-1","name":"app","title":"App","width":400,"height":300,"root":{"tag":"div","key":1},"backend_js":""}"#;

    fn overlay(payload: &[u8], count: u64) -> Vec<u8> {
        let mut bytes = payload.to_vec();
        bytes.extend_from_slice(&count.to_le_bytes());
        bytes.extend_from_slice(MAGIC);
        bytes
    }

    #[test]
    fn overlay_round_trips_payload() {
        let payload = VALID_DOC.as_bytes();
        let mut file = Cursor::new(overlay(payload, payload.len() as u64));
        assert_eq!(read_overlay(&mut file).unwrap(), payload);
    }

    #[test]
    fn overlay_ignores_host_bytes_before_payload() {
        let payload = VALID_DOC.as_bytes();
        let mut host = b"MZ fake host prefix".to_vec();
        host.extend(overlay(payload, payload.len() as u64));
        assert_eq!(read_overlay(&mut Cursor::new(host)).unwrap(), payload);
    }

    #[test]
    fn overlay_reads_before_pe_certificate_and_padding() {
        let payload = VALID_DOC.as_bytes();
        let mut host = vec![0u8; 512];
        host[0..2].copy_from_slice(b"MZ");
        host[0x3c..0x40].copy_from_slice(&128u32.to_le_bytes());
        host[128..132].copy_from_slice(b"PE\0\0");
        host[148..150].copy_from_slice(&240u16.to_le_bytes());
        host[152..154].copy_from_slice(&0x20bu16.to_le_bytes());
        host.extend(overlay(payload, payload.len() as u64));
        while host.len() % 8 != 0 { host.push(0); }
        let offset = host.len() as u32;
        host[296..300].copy_from_slice(&offset.to_le_bytes());
        host[300..304].copy_from_slice(&32u32.to_le_bytes());
        host.extend([0x55; 32]);
        assert_eq!(read_overlay(&mut Cursor::new(host)).unwrap(), payload);
    }

    #[test]
    fn overlay_rejects_missing_magic() {
        let mut bytes = overlay(b"abc", 3);
        let last = bytes.len() - 1;
        bytes[last] ^= 0xff;
        assert!(read_overlay(&mut Cursor::new(bytes)).unwrap_err().contains("No embedded app bundle"));
    }

    #[test]
    fn overlay_rejects_short_file() {
        assert!(read_overlay(&mut Cursor::new(vec![0u8; 8])).unwrap_err().contains("No embedded app bundle"));
    }

    #[test]
    fn overlay_rejects_zero_and_oversized_counts() {
        assert!(read_overlay(&mut Cursor::new(overlay(b"abc", 0))).unwrap_err().contains("Invalid embedded"));
        assert!(read_overlay(&mut Cursor::new(overlay(b"abc", 4))).unwrap_err().contains("Invalid embedded"));
        let huge = MAX_PAYLOAD + 1;
        assert!(read_overlay(&mut Cursor::new(overlay(b"abc", huge))).unwrap_err().contains("Invalid embedded"));
    }

    #[test]
    fn parses_valid_document() {
        let doc = parse_bundle(VALID_DOC.as_bytes().to_vec()).unwrap();
        assert_eq!(doc.name, "app");
        assert_eq!((doc.width, doc.height), (400, 300));
    }

    #[test]
    fn parses_legacy_webview_without_asset_map() {
        let json = VALID_DOC.replace("\"name\":\"app\"", "\"renderer\":\"webview\",\"webview_html\":\"<html>legacy</html>\",\"name\":\"app\"");
        let doc = parse_bundle(json.into_bytes()).unwrap();
        assert_eq!(doc.webview_format_version, 0);
        assert!(doc.webview_files.is_empty());
        assert_eq!(doc.webview_html.as_deref(), Some("<html>legacy</html>"));
    }

    #[test]
    fn rejects_wrong_format_version() {
        let json = VALID_DOC.replace("tsgpui-1", "tsgpui-2");
        assert!(parse_bundle(json.into_bytes()).unwrap_err().contains("Incompatible bundle format"));
    }

    #[test]
    fn rejects_out_of_range_dimensions() {
        let small = VALID_DOC.replace("\"width\":400", "\"width\":199");
        assert!(parse_bundle(small.into_bytes()).unwrap_err().contains("Invalid window dimensions"));
        let large = VALID_DOC.replace("\"height\":300", "\"height\":4097");
        assert!(parse_bundle(large.into_bytes()).unwrap_err().contains("Invalid window dimensions"));
    }

    #[test]
    fn rejects_empty_name_or_root_tag() {
        let no_name = VALID_DOC.replace("\"name\":\"app\"", "\"name\":\"\"");
        assert!(parse_bundle(no_name.into_bytes()).unwrap_err().contains("Invalid document content"));
        let no_tag = VALID_DOC.replace("\"tag\":\"div\"", "\"tag\":\"\"");
        assert!(parse_bundle(no_tag.into_bytes()).unwrap_err().contains("Invalid document content"));
    }

    #[test]
    fn rejects_malformed_json() {
        assert!(parse_bundle(b"{not json".to_vec()).unwrap_err().contains("Invalid .tgp document"));
    }
}
