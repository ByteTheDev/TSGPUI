use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use crate::{capabilities::Capabilities, package::Permissions, platform};

#[derive(Default)]
pub struct Session { grants: Vec<(String, PathBuf, bool)> }

fn read_dialog_file(path: &Path) -> Result<Vec<u8>, String> {
    use std::io::Read;
    let mut data = Vec::new();
    std::fs::File::open(path).map_err(|e|e.to_string())?.take(1024*1024+1).read_to_end(&mut data).map_err(|e|e.to_string())?;
    if data.len() > 1024*1024 { return Err("File exceeds 1 MiB".into()); }
    Ok(data)
}

pub const SCRIPT: &str = r#"
(()=>{const listeners=new Map(),pending=new Map();let id=0;
window.__tsgpui_receive=(channel,payload)=>{for(const fn of [...(listeners.get(channel)||[])])fn(payload)};
window.__tsgpui_resolve=(id,ok,value)=>{const p=pending.get(id);if(!p)return;pending.delete(id);ok?p[0](value):p[1](new Error(value))};
const invoke=(op,args={})=>{if((op==='ipc.invoke'||op==='ipc.send')&&!window.__tsgpui_secondary){if(!window.__tsgpui_ipc_allowed)return Promise.reject(new Error('IPC permission denied'));const dispatch=globalThis.__tsgpui_ipc_dispatch;if(typeof dispatch!=='function')return Promise.reject(new Error('IPC dispatcher unavailable'));if(op==='ipc.send'){Promise.resolve().then(()=>dispatch('send',args.channel,args.payload)).catch(console.error);return Promise.resolve()}return Promise.resolve().then(()=>dispatch('invoke',args.channel,args.payload))}
return new Promise((resolve,reject)=>{const key=++id;pending.set(key,[resolve,reject]);try{window.ipc.postMessage(JSON.stringify({id:key,op,args}))}catch(e){pending.delete(key);reject(e)}})};
window.tsgpui={invoke,on(channel,handler){let set=listeners.get(channel);if(!set)listeners.set(channel,set=new Set());set.add(handler);return ()=>{set.delete(handler);if(!set.size)listeners.delete(channel)}}};
globalThis.__tsgpui_host=invoke;globalThis.__tsgpui_emit=(channel,payload)=>window.__tsgpui_receive(channel,payload);
})();"#;

pub fn decode(s: &str) -> Result<Vec<u8>, String> {
    let mut out=Vec::new(); let mut acc=0u32; let mut bits=0;
    for b in s.bytes() {
        if b == b'=' { break; }
        let n=match b { b'A'..=b'Z'=>b-b'A', b'a'..=b'z'=>b-b'a'+26, b'0'..=b'9'=>b-b'0'+52, b'+'=>62, b'/'=>63, _=>return Err("Invalid base64".into()) };
        acc=(acc<<6)|u32::from(n); bits+=6;
        if bits>=8 { bits-=8; out.push((acc>>bits) as u8); acc &= (1<<bits)-1; }
    }
    Ok(out)
}
pub fn encode(bytes: &[u8]) -> String {
    const ABC: &[u8]=b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut s=String::new();
    for chunk in bytes.chunks(3) {
        let n=(u32::from(chunk[0])<<16)|(u32::from(*chunk.get(1).unwrap_or(&0))<<8)|u32::from(*chunk.get(2).unwrap_or(&0));
        s.push(ABC[((n>>18)&63) as usize] as char); s.push(ABC[((n>>12)&63) as usize] as char);
        s.push(if chunk.len()>1 {ABC[((n>>6)&63) as usize] as char} else {'='});
        s.push(if chunk.len()>2 {ABC[(n&63) as usize] as char} else {'='});
    } s
}

pub fn url_path(path: &str) -> Option<String> {
    let mut out=Vec::new(); let bytes=path.as_bytes(); let mut i=0;
    while i<bytes.len() {
        if bytes[i]==b'%' {
            let hex=std::str::from_utf8(bytes.get(i+1..i+3)?).ok()?;
            out.push(u8::from_str_radix(hex,16).ok()?); i+=3;
        } else { out.push(bytes[i]); i+=1; }
    }
    String::from_utf8(out).ok()
}

#[cfg(test)]
mod protocol_tests {
    use super::{decode, url_path};
    use serde_json::json;
    use wry::http::Uri;

    use crate::package::Permissions;

    #[test]
    fn request_paths_match_bundle_keys() {
        for (uri, expected) in [
            ("http://tsgpui.localhost/src/renderer/index.html", "/src/renderer/index.html"),
            ("http://tsgpui.localhost/src/renderer/style.css", "/src/renderer/style.css"),
            ("http://tsgpui.localhost/src/renderer/app.js", "/src/renderer/app.js"),
            ("tsgpui://localhost/src/renderer/style.css", "/src/renderer/style.css"),
            ("http://tsgpui.localhost/src/renderer/style.css?v=1", "/src/renderer/style.css"),
            ("http://tsgpui.localhost/src/renderer/st%79le.css", "/src/renderer/style.css"),
        ] {
            let uri: Uri = uri.parse().unwrap();
            assert_eq!(url_path(uri.path()).as_deref(), Some(expected));
        }
    }

    #[test]
    fn asset_base64_decodes() {
        assert_eq!(decode("Ym9keSB7fQ==").unwrap(), b"body {}");
        assert!(decode("normal text").is_err());
    }

    #[test]
    fn native_image_operations_route_through_the_bridge() {
        let base = std::env::temp_dir().join(format!("tsgpui-bridge-image-{}", std::process::id()));
        std::fs::create_dir_all(base.join("assets")).unwrap();
        let source = image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(2, 2, image::Rgba([1, 2, 3, 255])));
        let mut png = Vec::new();
        source.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
        std::fs::write(base.join("assets/icon.png"), &png).unwrap();
        let mut permissions = Permissions::default();
        permissions.filesystem.read.push("assets".into());
        let mut session = super::Session::default();
        let created = super::call("nativeImage.createFromPath", &json!({ "root": "assets", "path": "icon.png" }), &permissions, &base, "App", "1.0", &mut session).unwrap();
        assert_eq!(created["width"], json!(2));
        let encoded = super::call("nativeImage.toPNG", &json!({ "handle": created["handle"] }), &permissions, &base, "App", "1.0", &mut session).unwrap();
        assert!(!encoded["base64"].as_str().unwrap().is_empty());
        std::fs::write(base.join("outside.png"), &png).unwrap();
        let denied = super::call("nativeImage.createFromPath", &json!({ "path": base.join("outside.png").to_string_lossy() }), &permissions, &base, "App", "1.0", &mut session);
        assert!(denied.unwrap_err().contains("PERMISSION_DENIED"));
    }
}

/// Runs one WebView host operation. Clipboard, dialogs, power, links and HTTPS are shared with the
/// native host through `platform`; dialog-selected files and the app description stay here.
pub fn call(op: &str, args: &Value, permissions: &Permissions, base: &Path, name: &str, version: &str, session: &mut Session) -> Result<Value,String> {
    if matches!(op,"clipboard.readText"|"clipboard.writeText"|"dialog.openFile"|"dialog.saveFile"|"dialog.message") {
        let result = match op {
            "clipboard.readText" => json!({"text": platform::clipboard_read_text()?}),
            "clipboard.writeText" => { platform::clipboard_write_text(args["text"].as_str().unwrap_or(""))?; Value::Null },
            "dialog.openFile" => platform::open_file_dialog(args)?,
            "dialog.saveFile" => platform::save_file_dialog(args)?,
            _ => platform::message_dialog(args)?,
        };
        if op == "dialog.openFile" {
            if let Some(paths) = result["paths"].as_array() { for path in paths.iter().filter_map(Value::as_str) { session.grants.push((format!("dialog:{}",session.grants.len()), PathBuf::from(path), false)); } }
        } else if op == "dialog.saveFile" {
            if let Some(path) = result["path"].as_str() { session.grants.push((format!("dialog:{}",session.grants.len()), PathBuf::from(path), true)); }
        }
        return Ok(result);
    }
    if matches!(op, "safeStorage.isAvailable" | "safeStorage.encrypt" | "safeStorage.decrypt") {
        return platform::safe_storage_call(op, &[args["value"].clone()]);
    }
    if op.starts_with("nativeImage.") {
        // Decoded images live in the host. `{ root, path }` addresses a permission root, userData
        // or a dialog-selected file exactly as `filesystem.*` does; the root is resolved here and
        // the image call sees one absolute read root.
        let mut scoped = permissions.clone();
        let mut actual = args.clone();
        if op == "nativeImage.createFromPath" {
            let path = args["path"].as_str().ok_or("path must be a string")?;
            if let Some(root) = args.get("root").and_then(Value::as_str) {
                let (directory, resolved) = if let Some((_, file, _)) = session.grants.iter().find(|(name, _, _)| name == root) {
                    if !path.is_empty() { return Err("Dialog grant covers only the selected file".into()); }
                    (file.clone(), file.to_string_lossy().into_owned())
                } else if root == "userData" {
                    let directory = crate::package::state_file(name).and_then(|p| p.parent().map(Path::to_path_buf)).unwrap_or_else(|| base.to_path_buf());
                    (directory, path.to_string())
                } else if permissions.filesystem.read.iter().any(|granted| granted == root) {
                    (base.join(root), path.to_string())
                } else {
                    return Err("PERMISSION_DENIED: Filesystem root is not granted".into());
                };
                scoped.filesystem.read = vec![directory.to_string_lossy().into_owned()];
                actual["path"] = Value::String(resolved);
                if let Some(object) = actual.as_object_mut() { object.remove("root"); }
            }
        }
        return Capabilities::from_permissions(&scoped, base.to_path_buf()).call(op, &[actual], &[]);
    }
    if op == "power.setBlockers" {
        platform::set_power_blockers(&platform::string_list(&args["types"]))?;
        return Ok(Value::Null);
    }
    if op == "power.state" {
        return Ok(platform::power_snapshot());
    }
    if op == "systemPreferences.info" {
        return Ok(platform::system_preferences());
    }
    if op == "app.info" {
        let home=std::env::var_os(if cfg!(windows) {"USERPROFILE"} else {"HOME"}).map(PathBuf::from).unwrap_or_default();
        let user_data=crate::package::state_file(name).and_then(|p|p.parent().map(Path::to_path_buf)).unwrap_or_else(||base.to_path_buf());
        let roots: Vec<Value> = permissions.filesystem.read.iter().map(|p|json!({"name":p,"path":base.join(p).to_string_lossy(),"access":"read"}))
            .chain(permissions.filesystem.write.iter().map(|p|json!({"name":p,"path":base.join(p).to_string_lossy(),"access":"write"})))
            .chain(std::iter::once(json!({"name":"userData","path":user_data,"access":"write"})))
            .chain(session.grants.iter().map(|(name,path,write)|json!({"name":name,"path":path,"access":if *write {"write"} else {"read"},"exact":true}))).collect();
        return Ok(json!({"name":name,"version":version,"platform":std::env::consts::OS,"paths":{"home":home,"userData":user_data,"documents":home.join("Documents"),"downloads":home.join("Downloads"),"temp":std::env::temp_dir(),"appFolder":base},"roots":roots}));
    }
    if op == "net.request" {
        return platform::https_request(args, &permissions.network_hosts);
    }
    if op == "process.spawn" {
        let result=Capabilities::from_permissions(permissions,base.to_path_buf()).call(op,&[args["command"].clone(),args["args"].clone()],&[])?;
        return Ok(json!({"status":result["code"],"stdout":result["stdout"],"stderr":result["stderr"]}));
    }
    if op.starts_with("filesystem.") {
        let root=args["root"].as_str().ok_or("root must be a string")?;
        let write=matches!(op,"filesystem.writeText"|"filesystem.writeBytes"|"filesystem.mkdir"|"filesystem.remove"|"filesystem.rename");
        if let Some((_,file,grant_write)) = session.grants.iter().find(|(name,_,_)|name == root) {
            if write && !grant_write { return Err("Filesystem root is not granted for writing".into()); }
            if args["path"].as_str().unwrap_or("") != "" { return Err("Dialog grant covers only the selected file".into()); }
            return match op {
                "filesystem.readText" if !grant_write => { let data=read_dialog_file(file)?; Ok(json!({"text":String::from_utf8(data).map_err(|e|e.to_string())?})) },
                "filesystem.readBytes" if !grant_write => { let data=read_dialog_file(file)?; Ok(json!({"base64":encode(&data)})) },
                "filesystem.writeText" if *grant_write => { let data=args["text"].as_str().ok_or("text must be a string")?.as_bytes(); if data.len()>1024*1024 { return Err("File exceeds 1 MiB".into()); } std::fs::write(file,data).map_err(|e|e.to_string())?; Ok(Value::Null) },
                "filesystem.writeBytes" if *grant_write => { let data=decode(args["base64"].as_str().ok_or("base64 must be a string")?)?; if data.len()>1024*1024 { return Err("File exceeds 1 MiB".into()); } std::fs::write(file,data).map_err(|e|e.to_string())?; Ok(Value::Null) },
                _ => Err("Operation is not supported for a dialog grant".into())
            };
        }
        let grants=if write {&permissions.filesystem.write} else {&permissions.filesystem.read};
        if root != "userData" && !grants.iter().any(|r|r==root) { return Err("Filesystem root is not granted".into()); }
        let mut restricted=permissions.clone();
        let actual_root=if root == "userData" { crate::package::state_file(name).and_then(|p|p.parent().map(Path::to_path_buf)).unwrap_or_else(||base.to_path_buf()).to_string_lossy().into_owned() } else { root.into() };
        // Electron apps write to userData without creating it first, so it exists before the first call.
        if root == "userData" { let _ = std::fs::create_dir_all(&actual_root); }
        restricted.filesystem.read=if write {vec![]} else {vec![actual_root.clone()]};
        restricted.filesystem.write=if write || root == "userData" {vec![actual_root]} else {vec![]};
        let path=args["path"].as_str().unwrap_or("");
        let mut values=vec![Value::String(path.into())];
        match op {
            "filesystem.writeText"=>values.push(args["text"].clone()),
            "filesystem.writeBytes"=>values.push(Value::Array(decode(args["base64"].as_str().ok_or("base64 must be a string")?)?.into_iter().map(Value::from).collect())),
            "filesystem.rename"=>values=vec![args["from"].clone(),args["to"].clone()],
            _=>{}
        }
        let result=Capabilities::from_permissions(&restricted,base.to_path_buf()).call(op,&values,&[])?;
        if op=="filesystem.readText" { return Ok(json!({"text":result})); }
        if op=="filesystem.readBytes" {
            let bytes: Vec<u8>=result.as_array().ok_or("Invalid byte result")?.iter().map(|v|v.as_u64().unwrap_or(0) as u8).collect();
            return Ok(json!({"base64":encode(&bytes)}));
        }
        return Ok(result);
    }
    if op=="shell.openExternal" {
        let url=args["url"].as_str().ok_or("url must be a string")?;
        platform::open_external(url)?;
        return Ok(Value::Null);
    }
    if op=="notifications.show" {
        // `args` arrives as the bridge object {title, body}; Capabilities expects one positional
        // object. The grant check and notify-rust toast live in webview_capabilities.rs; a grant
        // miss or an unavailable OS notification service returns Err, which the shim turns into
        // its in-page toast fallback.
        return Capabilities::from_permissions(permissions,base.to_path_buf()).call(op,&[args.clone()],&[]);
    }
    if matches!(op, "app.quit" | "app.exit" | "updater.install" | "updater.quitAndInstall") {
        return Ok(Value::Null);
    }
    if op.starts_with("tray.") {
        return crate::tray::call(op, args);
    }
    if op.starts_with("globalShortcut.") {
        return crate::hotkeys::call(op, args);
    }
    Err(format!("unsupported op: {op}"))
}
