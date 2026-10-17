use std::{collections::BTreeMap,fs::File,io::{Read,Seek,SeekFrom},path::PathBuf};
use serde::Deserialize;

pub const MAGIC: &[u8;8] = b"TSGPUI01";
pub const MAX_PAYLOAD: u64 = 16 * 1024 * 1024;

#[derive(Debug, Clone, Deserialize)]
pub struct Node {
    pub tag: String,
    pub key: u64,
    #[serde(default)] pub text: String,
    #[serde(default)] pub attrs: BTreeMap<String,String>,
    #[serde(default)] pub props: BTreeMap<String,String>,
    #[serde(default)] pub style: BTreeMap<String,String>,
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
}

#[derive(Debug, Clone, Deserialize)]
pub struct AppPackage {
    pub format: String,
    pub name: String,
    pub title: String,
    pub width: i32,
    pub height: i32,
    pub root: Node,
    pub backend_js: String,
    #[serde(default)] pub assets: BTreeMap<String,EmbeddedAsset>,
    #[serde(default)] pub permissions: Permissions,
}

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
    if doc.name.is_empty() || doc.root.tag.is_empty() {return Err("Invalid document content".into());}
    Ok(doc)
}
fn read_embedded() -> Result<Vec<u8>,String> {
    let exe: PathBuf = std::env::current_exe().map_err(|e| e.to_string())?;
    let mut f = File::open(&exe).map_err(|e| format!("Unable to open host: {e}"))?;
    let total = f.metadata().map_err(|e| e.to_string())?.len();
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
