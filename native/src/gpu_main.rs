//! The tsgpu host: our own GPU renderer for TSGPUI bundles.
//! Milestone 0 shell: load the package, hand it to gpu::surface::run. See RENDERER_TSGPU.md.
#![cfg_attr(all(target_os="windows",not(debug_assertions)),windows_subsystem="windows")]
#[path = "package.rs"] mod package;
mod gpu;

fn main() {
    let doc = match package::load() {
        Ok(doc) => doc,
        Err(err) => { eprintln!("tsgpui-gpu-host: {err}"); std::process::exit(2); }
    };
    if doc.renderer != "gpu" {
        eprintln!("tsgpui-gpu-host: app bundle selects the '{}' renderer; the gpu host expects renderer \"gpu\"", doc.renderer);
        std::process::exit(2);
    }
    std::process::exit(gpu::surface::run(&doc));
}
