#![cfg_attr(all(target_os="windows",not(debug_assertions)),windows_subsystem="windows")]
mod capabilities;
mod ipc;
mod package;
mod network;
mod runtime;
mod style;
mod text_input;
mod menus;
mod view;

fn main(){
    let bundle_file=match package::bundle_path(){
        Ok(path)=>path,
        Err(err)=>{eprintln!("TSGPUI host: {err}");std::process::exit(2)}
    };
    let doc=match package::load(){
        Ok(doc)=>doc,
        Err(err)=>{eprintln!("TSGPUI host: {err}");std::process::exit(2)}
    };
    if doc.renderer != "native" {
        eprintln!("TSGPUI host: app bundle selects the '{}' renderer; use tsgpui-webview-host",doc.renderer);
        std::process::exit(2);
    }
    // Relative filesystem grants resolve against the bundle's folder for `--bundle` runs, and
    // against the app's own folder otherwise.
    let base=bundle_file.as_ref().and_then(|file|file.parent().map(|p|p.to_path_buf()))
        .or_else(||std::env::current_exe().ok().and_then(|exe|exe.parent().map(|p|p.to_path_buf())))
        .unwrap_or_default();
    let grants=capabilities::Capabilities::from_permissions(&doc.permissions,base);
    let backend=match runtime::JsBackend::with_capabilities(&doc.backend_js,grants){
        Ok(backend)=>backend,
        Err(err)=>{eprintln!("TSGPUI backend: {err}");std::process::exit(3)}
    };
    view::run(doc,backend,bundle_file);
}
