use std::{collections::BTreeMap,fs::File,io::{Read,Seek,SeekFrom},path::PathBuf};
use serde::Deserialize;

pub const MAGIC: &[u8;8] = b"TSGPUI01";
pub const MAX_PAYLOAD: u64 = 16 * 1024 * 1024;
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
    pub title: String,
    pub width: i32,
    pub height: i32,
    pub root: Node,
    pub backend_js: String,
    #[serde(default)] pub webview_html: Option<String>,
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
    Ok(doc)
}
fn read_embedded() -> Result<Vec<u8>,String> {
    let exe: PathBuf = std::env::current_exe().map_err(|e| e.to_string())?;
    let mut f = File::open(&exe).map_err(|e| format!("Unable to open host: {e}"))?;
    read_overlay(&mut f)
}

// The overlay is `payload ++ count (u64 LE) ++ MAGIC`, so the footer is read first.
fn read_overlay<R: Read + Seek>(f: &mut R) -> Result<Vec<u8>,String> {
    let total = f.seek(SeekFrom::End(0)).map_err(|e| e.to_string())?;
    if total < 16 {return Err("No embedded app bundle. Use --bundle file.tgp".into());}
    f.seek(SeekFrom::End(-16)).map_err(|e| e.to_string())?;
    let mut footer=[0u8;16];
    f.read_exact(&mut footer).map_err(|e| e.to_string())?;
    if &footer[8..] != MAGIC {
        return Err("No embedded app bundle. Use --bundle file.tgp".into());
    }
    let count = u64::from_le_bytes(footer[0..8].try_into().unwrap());
    if count == 0 || count > MAX_PAYLOAD || count > total-16 {return Err("Invalid embedded app bundle length".into());}
    f.seek(SeekFrom::Start(total-16-count)).map_err(|e| e.to_string())?;
    let mut buffer=vec![0;count as usize];
    f.read_exact(&mut buffer).map_err(|e| e.to_string())?;
    Ok(buffer)
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
