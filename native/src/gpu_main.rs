//! The Rendrix host: the Rendrix renderer for Rendrix bundles.
//! Milestone 0 shell: load the package, hand it to gpu::surface::run. See RENDERER_RENDRIX.md.
#![cfg_attr(all(target_os="windows",not(debug_assertions)),windows_subsystem="windows")]
#[path = "package.rs"] mod package;
mod gpu;

fn main() {
    let doc = match package::load() {
        Ok(doc) => doc,
        Err(err) => { eprintln!("rendrix-host: {err}"); std::process::exit(2); }
    };
    if doc.renderer != "rendrix" {
        eprintln!("rendrix-host: app bundle selects the '{}' renderer; the Rendrix host expects renderer \"rendrix\"", doc.renderer);
        std::process::exit(2);
    }
    std::process::exit(gpu::surface::run(&doc));
}
