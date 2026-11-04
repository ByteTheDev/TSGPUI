use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use crate::{capabilities::Capabilities, package::Permissions};

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
const invoke=(op,args={})=>{if(op==='ipc.invoke'||op==='ipc.send'){if(!window.__tsgpui_ipc_allowed)return Promise.reject(new Error('IPC permission denied'));const dispatch=globalThis.__tsgpui_ipc_dispatch;if(typeof dispatch!=='function')return Promise.reject(new Error('IPC dispatcher unavailable'));if(op==='ipc.send'){Promise.resolve().then(()=>dispatch('send',args.channel,args.payload)).catch(console.error);return Promise.resolve()}return Promise.resolve().then(()=>dispatch('invoke',args.channel,args.payload))}
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
    use wry::http::Uri;

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
}

#[cfg(target_os="windows")]
fn windows_ui(op: &str, args: &Value) -> Result<Value,String> {
    match op {
        "clipboard.readText" => Ok(json!({"text": clipboard::read_text()?})),
        "clipboard.writeText" => { clipboard::write_text(args["text"].as_str().unwrap_or(""))?; Ok(Value::Null) },
        "dialog.openFile" => Ok(open_file_dialog(args)),
        "dialog.saveFile" => Ok(save_file_dialog(args)),
        "dialog.message" => message_dialog(args),
        _ => Err(format!("unsupported op: {op}")),
    }
}

#[cfg(target_os="windows")]
fn string_list(value: &Value) -> Vec<String> {
    value.as_array().map(|items| items.iter().filter_map(Value::as_str).map(str::to_owned).collect()).unwrap_or_default()
}

#[cfg(target_os="windows")]
fn apply_filters(mut dialog: rfd::FileDialog, filters: &Value) -> rfd::FileDialog {
    for filter in filters.as_array().map(Vec::as_slice).unwrap_or(&[]) {
        let name = filter["name"].as_str().unwrap_or("Files").to_owned();
        let extensions = string_list(&filter["extensions"]);
        dialog = dialog.add_filter(name, extensions.as_slice());
    }
    dialog
}

#[cfg(target_os="windows")]
fn open_file_dialog(args: &Value) -> Value {
    let dialog = apply_filters(rfd::FileDialog::new(), &args["filters"]);
    if args["multiple"].as_bool().unwrap_or(false) {
        match dialog.pick_files() {
            Some(paths) => json!({"canceled": false, "paths": paths.iter().map(|p| p.to_string_lossy().into_owned()).collect::<Vec<_>>()}),
            None => json!({"canceled": true, "paths": []}),
        }
    } else {
        match dialog.pick_file() {
            Some(path) => json!({"canceled": false, "paths": [path.to_string_lossy().into_owned()]}),
            None => json!({"canceled": true, "paths": []}),
        }
    }
}

#[cfg(target_os="windows")]
fn save_file_dialog(args: &Value) -> Value {
    let mut dialog = apply_filters(rfd::FileDialog::new(), &args["filters"]);
    if let Some(default) = args["defaultPath"].as_str() {
        let path = Path::new(default);
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) { dialog = dialog.set_directory(parent); }
        if let Some(name) = path.file_name() { dialog = dialog.set_file_name(name.to_string_lossy().into_owned()); }
    }
    match dialog.save_file() {
        Some(path) => json!({"canceled": false, "path": path.to_string_lossy().into_owned()}),
        None => json!({"canceled": true, "path": null}),
    }
}

/// Shows a message with up to three buttons and returns the index of the button the user chose.
/// Closing the dialog counts as choosing the last button, which is the usual cancel position.
#[cfg(target_os="windows")]
fn message_dialog(args: &Value) -> Result<Value,String> {
    use rfd::{MessageButtons, MessageDialog, MessageDialogResult, MessageLevel};
    let labels = match string_list(&args["buttons"]) { list if list.is_empty() => vec!["OK".to_owned()], list => list };
    let buttons = match labels.as_slice() {
        [a] => MessageButtons::OkCustom(a.clone()),
        [a, b] => MessageButtons::OkCancelCustom(a.clone(), b.clone()),
        [a, b, c] => MessageButtons::YesNoCancelCustom(a.clone(), b.clone(), c.clone()),
        other => return Err(format!("message dialogs support up to three buttons, not {}", other.len())),
    };
    let level = match args["type"].as_str() { Some("error") => MessageLevel::Error, Some("warning") => MessageLevel::Warning, _ => MessageLevel::Info };
    let result = MessageDialog::new()
        .set_title(args["title"].as_str().unwrap_or(""))
        .set_description(args["message"].as_str().unwrap_or(""))
        .set_level(level)
        .set_buttons(buttons)
        .show();
    let response = match result {
        MessageDialogResult::Custom(label) => labels.iter().position(|l| *l == label).unwrap_or(0),
        MessageDialogResult::Ok | MessageDialogResult::Yes => 0,
        MessageDialogResult::No => 1,
        MessageDialogResult::Cancel => labels.len() - 1,
    };
    Ok(json!({"response": response}))
}

#[cfg(target_os="windows")]
mod clipboard {
    use windows_sys::Win32::System::{
        DataExchange::{CloseClipboard, EmptyClipboard, GetClipboardData, OpenClipboard, SetClipboardData},
        Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE},
        Ole::CF_UNICODETEXT,
    };

    /// Reads Unicode text from the clipboard. An empty or non-text clipboard reads as an empty string.
    pub fn read_text() -> Result<String, String> {
        unsafe {
            if OpenClipboard(std::ptr::null_mut()) == 0 { return Err("Unable to open the clipboard".into()); }
            let text = (|| {
                let handle = GetClipboardData(CF_UNICODETEXT as u32);
                if handle.is_null() { return Ok(String::new()); }
                let pointer = GlobalLock(handle) as *const u16;
                if pointer.is_null() { return Err("Unable to read the clipboard text".to_owned()); }
                let mut length = 0usize;
                while *pointer.add(length) != 0 { length += 1; }
                let text = String::from_utf16_lossy(std::slice::from_raw_parts(pointer, length));
                GlobalUnlock(handle);
                Ok(text)
            })();
            CloseClipboard();
            text
        }
    }

    /// Replaces the clipboard contents with the given Unicode text.
    pub fn write_text(text: &str) -> Result<(), String> {
        let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
        unsafe {
            let memory = GlobalAlloc(GMEM_MOVEABLE, wide.len() * std::mem::size_of::<u16>());
            if memory.is_null() { return Err("Unable to allocate clipboard memory".into()); }
            let target = GlobalLock(memory) as *mut u16;
            if target.is_null() { return Err("Unable to lock clipboard memory".into()); }
            std::ptr::copy_nonoverlapping(wide.as_ptr(), target, wide.len());
            GlobalUnlock(memory);
            if OpenClipboard(std::ptr::null_mut()) == 0 { return Err("Unable to open the clipboard".into()); }
            EmptyClipboard();
            let placed = SetClipboardData(CF_UNICODETEXT as u32, memory);
            CloseClipboard();
            if placed.is_null() { return Err("Unable to set the clipboard text".into()); }
            Ok(())
        }
    }
}

pub fn call(op: &str, args: &Value, permissions: &Permissions, base: &Path, name: &str, version: &str, session: &mut Session) -> Result<Value,String> {
    #[cfg(target_os="windows")]
    if matches!(op,"clipboard.readText"|"clipboard.writeText"|"dialog.openFile"|"dialog.saveFile"|"dialog.message") {
        let result = windows_ui(op,args)?;
        if op == "dialog.openFile" {
            if let Some(paths) = result["paths"].as_array() { for path in paths.iter().filter_map(Value::as_str) { session.grants.push((format!("dialog:{}",session.grants.len()), PathBuf::from(path), false)); } }
        } else if op == "dialog.saveFile" {
            if let Some(path) = result["path"].as_str() { session.grants.push((format!("dialog:{}",session.grants.len()), PathBuf::from(path), true)); }
        }
        return Ok(result);
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
        use std::io::Read;
        let url = args["url"].as_str().ok_or("url must be a string")?;
        let parsed = url::Url::parse(url).map_err(|e|e.to_string())?;
        let host = parsed.host_str().ok_or("URL has no host")?;
        if parsed.scheme() != "https" || !permissions.network_hosts.iter().any(|allowed| allowed.eq_ignore_ascii_case(host)) { return Err("Network host is not granted or URL is not HTTPS".into()); }
        let body = args["body"].as_str().unwrap_or("");
        if body.len() > 1024*1024 { return Err("Request body exceeds 1 MiB".into()); }
        let agent = ureq::AgentBuilder::new().timeout(std::time::Duration::from_secs(15)).redirects(0).build();
        let mut request = agent.request(args["method"].as_str().unwrap_or("GET"), url);
        if let Some(headers) = args["headers"].as_object() { for (key,value) in headers { request = request.set(key, value.as_str().ok_or("Header values must be strings")?); } }
        let response = match request.send_string(body) { Ok(r) => r, Err(ureq::Error::Status(_,r)) => r, Err(e) => return Err(e.to_string()) };
        let status = response.status();
        let mut headers = serde_json::Map::new();
        for key in response.headers_names() { if let Some(value) = response.header(&key) { headers.insert(key, json!(value)); } }
        let mut bytes = Vec::new(); response.into_reader().take(1024*1024+1).read_to_end(&mut bytes).map_err(|e|e.to_string())?;
        if bytes.len() > 1024*1024 { return Err("Response body exceeds 1 MiB".into()); }
        return Ok(json!({"status":status,"headers":headers,"body":String::from_utf8_lossy(&bytes)}));
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
        if !(url.starts_with("https://") || url.starts_with("mailto:")) || url.chars().any(char::is_control) {return Err("Only https and mailto URLs are allowed".into());}
        #[cfg(target_os="windows")] let (command, argv)=("rundll32",vec!["url.dll,FileProtocolHandler",url]);
        #[cfg(target_os="macos")] let (command, argv)=("open",vec![url]);
        #[cfg(not(any(target_os="windows",target_os="macos")))] let (command, argv)=("xdg-open",vec![url]);
        std::process::Command::new(command).args(argv).spawn().map_err(|e|e.to_string())?;
        return Ok(Value::Null);
    }
    Err(format!("unsupported op: {op}"))
}
