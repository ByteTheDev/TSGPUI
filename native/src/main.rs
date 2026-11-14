#![cfg_attr(all(target_os="windows",not(debug_assertions)),windows_subsystem="windows")]
mod capabilities;
mod platform;
mod updater;
mod ipc;
mod package;
mod network;
mod runtime;
mod style;
mod accessibility;
mod text_input;
mod menus;
mod view;
mod image_store;
mod tray;
mod hotkeys;

fn main(){
    let bundle_file=match package::bundle_path(){
        Ok(path)=>path,
        Err(err)=>{eprintln!("rendrix-legacy-host: {err}");std::process::exit(2)}
    };
    let doc=match package::load(){
        Ok(doc)=>doc,
        Err(err)=>{eprintln!("rendrix-legacy-host: {err}");std::process::exit(2)}
    };
    if doc.renderer != "native" {
        eprintln!("rendrix-legacy-host: app bundle selects the '{}' renderer; this deprecated host only runs renderer \"native\" bundles (use rendrix-host)",doc.renderer);
        std::process::exit(2);
    }
    // The GPUI host is deprecated: accepted with a warning until the Rendrix M1 gate, then removed.
    eprintln!("rendrix-legacy-host: the native renderer is deprecated; migrate bundles to renderer \"rendrix\"");
    // Relative filesystem grants resolve against the bundle's folder for `--bundle` runs, and
    // against the app's own folder otherwise.
    let base=bundle_file.as_ref().and_then(|file|file.parent().map(|p|p.to_path_buf()))
        .or_else(||std::env::current_exe().ok().and_then(|exe|exe.parent().map(|p|p.to_path_buf())))
        .unwrap_or_default();
    let grants=capabilities::Capabilities::from_permissions(&doc.permissions,base).with_app(&doc.name,&doc.version);
    let backend=match runtime::JsBackend::with_capabilities(&doc.backend_js,grants){
        Ok(backend)=>backend,
        Err(err)=>{eprintln!("Rendrix backend: {err}");std::process::exit(3)}
    };
    view::run(doc,backend,bundle_file);
}
