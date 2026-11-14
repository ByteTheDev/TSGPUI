//! Asset access and decoded image storage.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

/// Where the engine reads relative resources (stylesheets, images, fonts).
pub trait AssetProvider {
    fn read(&self, path: &str) -> Option<Vec<u8>>;
}

fn normalize(path: &str) -> String {
    let p = path.split(['?', '#']).next().unwrap_or(path);
    let p = p.trim_start_matches("./").trim_start_matches('/');
    p.replace('\\', "/")
}

#[derive(Default)]
pub struct MapAssets(pub HashMap<String, Vec<u8>>);

impl AssetProvider for MapAssets {
    fn read(&self, path: &str) -> Option<Vec<u8>> {
        self.0.get(&normalize(path)).cloned()
    }
}

/// Reads from a directory, refusing paths that escape it.
pub struct DirAssets(pub PathBuf);

impl AssetProvider for DirAssets {
    fn read(&self, path: &str) -> Option<Vec<u8>> {
        let rel = normalize(path);
        if rel.split('/').any(|c| c == "..") {
            return None;
        }
        std::fs::read(self.0.join(rel)).ok()
    }
}

#[derive(Clone)]
pub struct ImageEntry {
    pub id: u32,
    pub width: u32,
    pub height: u32,
    pub rgba: Arc<Vec<u8>>,
}

#[derive(Default)]
pub struct Images {
    by_url: HashMap<String, Option<ImageEntry>>,
    next_id: u32,
    pub pending_upload: Vec<ImageEntry>,
}

impl Images {
    /// Decode an image the first time it is requested. Failed loads are remembered.
    pub fn get(&mut self, url: &str, assets: &dyn AssetProvider) -> Option<ImageEntry> {
        if let Some(e) = self.by_url.get(url) {
            return e.clone();
        }
        let entry = assets.read(url).and_then(|bytes| decode(&bytes)).map(|(w, h, rgba)| {
            self.next_id += 1;
            ImageEntry { id: self.next_id, width: w, height: h, rgba: Arc::new(rgba) }
        });
        if let Some(e) = &entry {
            self.pending_upload.push(e.clone());
        }
        self.by_url.insert(url.to_string(), entry.clone());
        entry
    }
}

pub fn decode(bytes: &[u8]) -> Option<(u32, u32, Vec<u8>)> {
    let img = image::load_from_memory(bytes).ok()?.to_rgba8();
    let (w, h) = img.dimensions();
    Some((w, h, img.into_raw()))
}

