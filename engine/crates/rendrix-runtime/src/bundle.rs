//! The `rendrix-app/1` application bundle and the loaders for it.
//!
//! A bundle is JSON: the HTML document, compiled ES modules and static assets. It can be a file
//! (`.rdxapp`), appended to a copy of the runtime executable (packaged app), or reconstructed from a
//! plain project directory (`index.html` + `.js` files) for development without a build step.

use rendrix_core::AssetProvider;
use rendrix_js::Permissions;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub const FORMAT: &str = "rendrix-app/1";
/// Trailer magic for bundles appended to an executable: `[json][u64 LE length][RENDRAPP]`.
pub const MAGIC: &[u8; 8] = b"RENDRAPP";
const MAX_BUNDLE: u64 = 256 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Asset {
    Text { text: String },
    Binary { base64: String },
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PermissionConfig {
    #[serde(default)]
    pub network: bool,
    #[serde(default)]
    pub fs_read: Vec<String>,
    #[serde(default)]
    pub fs_write: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppBundle {
    pub format: String,
    pub name: String,
    #[serde(default)]
    pub title: String,
    #[serde(default = "default_width")]
    pub width: u32,
    #[serde(default = "default_height")]
    pub height: u32,
    /// The document served to the engine (`index.html`).
    pub html: String,
    /// ES modules (and classic scripts) by normalised path, e.g. `main.js`, `lib/util.js`.
    #[serde(default)]
    pub modules: BTreeMap<String, String>,
    #[serde(default)]
    pub assets: BTreeMap<String, Asset>,
    #[serde(default)]
    pub permissions: PermissionConfig,
}

fn default_width() -> u32 {
    960
}
fn default_height() -> u32 {
    640
}

impl AppBundle {
    pub fn from_json(text: &str) -> Result<AppBundle, String> {
        let b: AppBundle = serde_json::from_str(text).map_err(|e| format!("invalid app bundle: {e}"))?;
        if b.format != FORMAT {
            return Err(format!("unsupported bundle format '{}' (this runtime reads '{FORMAT}')", b.format));
        }
        Ok(b)
    }

    pub fn permissions(&self) -> Permissions {
        Permissions {
            network: self.permissions.network,
            fs_read: self.permissions.fs_read.iter().map(PathBuf::from).collect(),
            fs_write: self.permissions.fs_write.iter().map(PathBuf::from).collect(),
        }
    }

    pub fn window_title(&self) -> String {
        if self.title.is_empty() {
            self.name.clone()
        } else {
            self.title.clone()
        }
    }

    pub fn asset_provider(&self) -> Box<dyn AssetProvider> {
        let mut map = std::collections::HashMap::new();
        for (k, v) in &self.assets {
            let bytes = match v {
                Asset::Text { text } => text.clone().into_bytes(),
                Asset::Binary { base64 } => rendrix_js::host::base64_decode(base64).unwrap_or_default(),
            };
            map.insert(k.clone(), bytes);
        }
        Box::new(rendrix_core::MapAssets(map))
    }

    /// Build a bundle from a plain project directory: `index.html`, `.js`/`.mjs` modules, other files as assets.
    pub fn from_directory(dir: &Path) -> Result<AppBundle, String> {
        let manifest = dir.join("rendrix.json");
        let (mut name, mut title, mut width, mut height, mut perms) = (dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "app".into()), String::new(), default_width(), default_height(), PermissionConfig::default());
        if let Ok(text) = std::fs::read_to_string(&manifest) {
            let v: serde_json::Value = serde_json::from_str(&text).map_err(|e| format!("rendrix.json: {e}"))?;
            if let Some(n) = v["name"].as_str() {
                name = n.to_string();
            }
            if let Some(t) = v["window"]["title"].as_str().or(v["title"].as_str()) {
                title = t.to_string();
            }
            if let Some(w) = v["window"]["width"].as_u64() {
                width = w as u32;
            }
            if let Some(h) = v["window"]["height"].as_u64() {
                height = h as u32;
            }
            if let Ok(p) = serde_json::from_value::<PermissionConfig>(v["permissions"].clone()) {
                perms = p;
            }
        }
        let html = std::fs::read_to_string(dir.join("index.html")).map_err(|e| format!("{}: {e}", dir.join("index.html").display()))?;
        let mut modules = BTreeMap::new();
        let mut assets = BTreeMap::new();
        walk(dir, dir, &mut |rel, path| {
            if rel == "index.html" || rel == "rendrix.json" {
                return;
            }
            let ext = rel.rsplit('.').next().unwrap_or("");
            if matches!(ext, "js" | "mjs") {
                if let Ok(t) = std::fs::read_to_string(path) {
                    modules.insert(rel.to_string(), t);
                }
            } else if let Ok(bytes) = std::fs::read(path) {
                let asset = match String::from_utf8(bytes.clone()) {
                    Ok(text) if matches!(ext, "css" | "json" | "txt" | "svg" | "html" | "md") => Asset::Text { text },
                    _ => Asset::Binary { base64: rendrix_js::host::base64_encode(&bytes) },
                };
                assets.insert(rel.to_string(), asset);
            }
        })?;
        Ok(AppBundle { format: FORMAT.into(), name, title, width, height, html, modules, assets, permissions: perms })
    }

    /// Load from a `.rdxapp` file, a directory, or the trailer of the running executable.
    pub fn load(path: Option<&Path>) -> Result<AppBundle, String> {
        match path {
            Some(p) if p.is_dir() => AppBundle::from_directory(p),
            Some(p) => {
                let bytes = std::fs::read(p).map_err(|e| format!("{}: {e}", p.display()))?;
                // A packaged executable passed explicitly, or a bare JSON bundle.
                if let Some(json) = trailer(&bytes) {
                    AppBundle::from_json(std::str::from_utf8(json).map_err(|e| e.to_string())?)
                } else {
                    AppBundle::from_json(std::str::from_utf8(&bytes).map_err(|e| e.to_string())?)
                }
            }
            None => {
                let exe = std::env::current_exe().map_err(|e| e.to_string())?;
                let bytes = std::fs::read(&exe).map_err(|e| e.to_string())?;
                let json = trailer(&bytes).ok_or("no application bundle is attached to this executable")?;
                AppBundle::from_json(std::str::from_utf8(json).map_err(|e| e.to_string())?)
            }
        }
    }

    /// Append this bundle to a copy of `runtime_exe`, producing a standalone executable.
    pub fn pack(&self, runtime_exe: &Path, out: &Path) -> Result<(), String> {
        let mut bytes = std::fs::read(runtime_exe).map_err(|e| format!("{}: {e}", runtime_exe.display()))?;
        // Replace any bundle that is already attached.
        if let Some(json) = trailer(&bytes) {
            let cut = bytes.len() - json.len() - 16;
            bytes.truncate(cut);
        }
        let json = serde_json::to_vec(self).map_err(|e| e.to_string())?;
        bytes.extend_from_slice(&json);
        bytes.extend_from_slice(&(json.len() as u64).to_le_bytes());
        bytes.extend_from_slice(MAGIC);
        std::fs::write(out, bytes).map_err(|e| format!("{}: {e}", out.display()))
    }
}

/// The JSON payload of a trailer-bundle, if `bytes` ends with one.
pub fn trailer(bytes: &[u8]) -> Option<&[u8]> {
    let n = bytes.len();
    if n < 16 || &bytes[n - 8..] != MAGIC {
        return None;
    }
    let len = u64::from_le_bytes(bytes[n - 16..n - 8].try_into().ok()?);
    if len > MAX_BUNDLE || len as usize > n - 16 {
        return None;
    }
    Some(&bytes[n - 16 - len as usize..n - 16])
}

fn walk(root: &Path, dir: &Path, f: &mut dyn FnMut(&str, &Path)) -> Result<(), String> {
    for entry in std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))? {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') || name == "node_modules" || name == "target" || name == "dist" {
            continue;
        }
        if path.is_dir() {
            walk(root, &path, f)?;
        } else if let Ok(rel) = path.strip_prefix(root) {
            f(&rel.to_string_lossy().replace('\\', "/"), &path);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> AppBundle {
        AppBundle::from_json(r#"{"format":"rendrix-app/1","name":"t","html":"<p>x</p>","modules":{"a.js":"1"},"assets":{"s.css":{"text":"p{}"},"i.bin":{"base64":"AQID"}}}"#).unwrap()
    }

    #[test]
    fn parses_and_validates_format() {
        let b = sample();
        assert_eq!((b.width, b.height, b.window_title().as_str()), (960, 640, "t"));
        assert!(AppBundle::from_json(r#"{"format":"other/9","name":"t","html":""}"#).is_err());
        assert!(AppBundle::from_json("not json").is_err());
        let assets = b.asset_provider();
        assert_eq!(assets.read("s.css").unwrap(), b"p{}");
        assert_eq!(assets.read("./i.bin").unwrap(), vec![1, 2, 3]);
    }

    #[test]
    fn packs_and_reads_trailer_and_replaces_existing() {
        let dir = std::env::temp_dir().join(format!("rendrix-bundle-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let exe = dir.join("host.exe");
        std::fs::write(&exe, b"MZ-fake-executable-bytes").unwrap();
        let out = dir.join("app.exe");
        sample().pack(&exe, &out).unwrap();
        let loaded = AppBundle::load(Some(&out)).unwrap();
        assert_eq!(loaded.name, "t");
        let mut second = sample();
        second.name = "second".into();
        second.pack(&out, &dir.join("app2.exe")).unwrap();
        let bytes = std::fs::read(dir.join("app2.exe")).unwrap();
        assert!(bytes.starts_with(b"MZ-fake-executable-bytes"));
        assert_eq!(AppBundle::load(Some(&dir.join("app2.exe"))).unwrap().name, "second");
        assert_eq!(bytes.windows(8).filter(|w| *w == MAGIC).count(), 1, "old bundle was replaced, not stacked");
        assert!(trailer(b"short").is_none());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn directory_projects_become_bundles() {
        let dir = std::env::temp_dir().join(format!("rendrix-dir-test-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("lib")).unwrap();
        std::fs::create_dir_all(dir.join("node_modules/x")).unwrap();
        std::fs::write(dir.join("index.html"), "<script type=module src=main.js></script>").unwrap();
        std::fs::write(dir.join("main.js"), "import './lib/a.js'").unwrap();
        std::fs::write(dir.join("lib/a.js"), "").unwrap();
        std::fs::write(dir.join("style.css"), "p{}").unwrap();
        std::fs::write(dir.join("node_modules/x/i.js"), "skip me").unwrap();
        std::fs::write(dir.join("rendrix.json"), r#"{"name":"proj","window":{"title":"Proj","width":300,"height":200},"permissions":{"network":true}}"#).unwrap();
        let b = AppBundle::from_directory(&dir).unwrap();
        assert_eq!((b.name.as_str(), b.window_title().as_str(), b.width, b.height, b.permissions.network), ("proj", "Proj", 300, 200, true));
        assert_eq!(b.modules.keys().cloned().collect::<Vec<_>>(), vec!["lib/a.js", "main.js"]);
        assert!(b.assets.contains_key("style.css") && !b.assets.contains_key("node_modules/x/i.js"));
        let _ = std::fs::remove_dir_all(dir);
    }
}
