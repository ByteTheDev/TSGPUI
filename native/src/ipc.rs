//! Messages between an app and other local processes, for apps that grant `ipc`.
//!
//! The host listens on 127.0.0.1 on a free port and requires a random token. Both are passed to
//! processes started with `process.spawn` as `TSGPUI_IPC_ADDR` and `TSGPUI_IPC_TOKEN`. A peer
//! connects and sends `{"token":"..."}` as its first line, then exchanges newline-delimited
//! `{"channel":"...","payload":...}` messages. Messages the app sends reach every connected peer
//! and the app's own listeners. Messages from peers reach the app's listeners only.

use std::{
    collections::hash_map::RandomState,
    hash::{BuildHasher, Hasher},
    io::{BufRead, BufReader, Read, Write},
    net::{Shutdown, SocketAddr, TcpListener, TcpStream},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::Sender,
        Arc, Mutex,
    },
    thread,
    time::Duration,
};

use serde_json::{json, Value};

use crate::runtime::HostEvent;

// Longest line a peer may send, and how long a broadcast waits on a peer that is not reading.
const MAX_LINE_BYTES: usize = 1024 * 1024;
const WRITE_TIMEOUT: Duration = Duration::from_secs(2);

type Peers = Arc<Mutex<Vec<(u64, TcpStream)>>>;

pub struct IpcHub {
    addr: SocketAddr,
    token: String,
    peers: Peers,
    stopping: Arc<AtomicBool>,
}

impl IpcHub {
    /// Starts listening. Messages from peers are sent to `events` as they arrive.
    pub fn start(events: Sender<HostEvent>) -> std::io::Result<Arc<Self>> {
        let listener = TcpListener::bind(("127.0.0.1", 0))?;
        let addr = listener.local_addr()?;
        let token = new_token();
        let peers: Peers = Arc::new(Mutex::new(Vec::new()));
        let stopping = Arc::new(AtomicBool::new(false));
        let hub = Arc::new(Self { addr, token: token.clone(), peers: peers.clone(), stopping: stopping.clone() });
        let next_peer = Arc::new(AtomicU64::new(0));
        thread::spawn(move || {
            for stream in listener.incoming() {
                if stopping.load(Ordering::SeqCst) {
                    break;
                }
                let Ok(stream) = stream else { continue };
                let id = next_peer.fetch_add(1, Ordering::SeqCst);
                let (events, peers, token) = (events.clone(), peers.clone(), token.clone());
                thread::spawn(move || serve_peer(id, stream, &token, &events, &peers));
            }
        });
        Ok(hub)
    }

    /// The variables a child process needs to find this hub.
    pub fn environment(&self) -> Vec<(String, String)> {
        vec![
            ("TSGPUI_IPC_ADDR".to_string(), self.addr.to_string()),
            ("TSGPUI_IPC_TOKEN".to_string(), self.token.clone()),
        ]
    }

    /// Sends `payload` to every connected peer. A peer that fails a write is disconnected.
    pub fn broadcast(&self, channel: &str, payload: &Value) {
        let line = format!("{}\n", json!({ "channel": channel, "payload": payload }));
        let mut peers = self.peers.lock().unwrap_or_else(|error| error.into_inner());
        peers.retain_mut(|(_, stream)| stream.write_all(line.as_bytes()).is_ok());
    }
}

impl Drop for IpcHub {
    fn drop(&mut self) {
        self.stopping.store(true, Ordering::SeqCst);
        // The accept loop is blocked in `accept`; one local connection wakes it so it sees the flag.
        let _ = TcpStream::connect(self.addr);
        let peers = self.peers.lock().unwrap_or_else(|error| error.into_inner());
        for (_, stream) in peers.iter() {
            let _ = stream.shutdown(Shutdown::Both);
        }
    }
}

fn serve_peer(id: u64, stream: TcpStream, token: &str, events: &Sender<HostEvent>, peers: &Peers) {
    let _ = stream.set_write_timeout(Some(WRITE_TIMEOUT));
    let Ok(writer) = stream.try_clone() else { return };
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    if read_bounded_line(&mut reader, &mut line).is_err() {
        return;
    }
    let presented = serde_json::from_str::<Value>(line.trim()).ok().and_then(|value| value["token"].as_str().map(str::to_owned));
    if presented.as_deref() != Some(token) {
        return;
    }
    peers.lock().unwrap_or_else(|error| error.into_inner()).push((id, writer));
    while read_bounded_line(&mut reader, &mut line).is_ok() {
        let Ok(message) = serde_json::from_str::<Value>(line.trim()) else { continue };
        let Some(channel) = message["channel"].as_str() else { continue };
        let payload = message.get("payload").cloned().unwrap_or(Value::Null);
        if events.send(HostEvent::Inbound { channel: channel.to_string(), payload }).is_err() {
            break;
        }
    }
    peers.lock().unwrap_or_else(|error| error.into_inner()).retain(|(peer, _)| *peer != id);
}

// Reads one line into `line`, failing at end of stream or past the length limit.
fn read_bounded_line(reader: &mut impl BufRead, line: &mut String) -> std::io::Result<()> {
    line.clear();
    let read = reader.by_ref().take(MAX_LINE_BYTES as u64 + 1).read_line(line)?;
    if read == 0 || line.len() > MAX_LINE_BYTES {
        return Err(std::io::Error::other("peer line ended or was too long"));
    }
    Ok(())
}

// 128 bits from the standard library's per-process random keys. The token only keeps other local
// processes from injecting messages, so this does not need a cryptographic generator.
fn new_token() -> String {
    let high = RandomState::new().build_hasher().finish();
    let low = RandomState::new().build_hasher().finish();
    format!("{high:016x}{low:016x}")
}
