#![cfg_attr(all(target_os="windows",not(debug_assertions)),windows_subsystem="windows")]
mod package;
mod network;
mod runtime;
mod style;
mod text_input;
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
    let backend=match runtime::JsBackend::new(&doc.backend_js){
        Ok(backend)=>backend,
        Err(err)=>{eprintln!("TSGPUI backend: {err}");std::process::exit(3)}
    };
    view::run(doc,backend,bundle_file);
}
