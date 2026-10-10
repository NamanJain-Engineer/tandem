//! LAN mode for the phone companion (F-RM-04, D-2001 to D-2008): the
//! desktop plays the relay's part itself, on the local network, so a phone
//! can pair with no hosted service and no account.
//!
//! - **Off by default.** Nothing listens until a person turns LAN mode on in
//!   Settings › Remote (main window, [`LocalReview`]). The choice is kept in
//!   `$DATA/remote/lan.json`.
//! - **Only the LAN.** It binds one socket per private IPv4 address the
//!   machine has (RFC 1918, and 100.64.0.0/10 for a VPN such as Tailscale),
//!   never `0.0.0.0` and never a public address. Connections from anything
//!   but a private, VPN, link-local or loopback address are closed unread,
//!   and a request must name one of those sockets in `Host` (no DNS
//!   rebinding).
//! - **The same protocol.** `/v1/relay/:pairing_id` behaves like the hosted
//!   relay (F-BE-11, D-1901, D-1902): the phone authenticates with
//!   `Sec-WebSocket-Protocol: tandem-pair.<token>`, frames are binary and at
//!   most 64 KB (1009), text closes with 1003, a refused token is upgraded
//!   and closed with 1008, a newer phone socket replaces the older (4000),
//!   and `{type:"peer", up}` says whether the other end is there. The frames
//!   are the same end-to-end encrypted frames (§13.3): this module never
//!   sees a key. The desktop's end is in-process ([`LanHub::attach_desktop`]).
//! - **The phone web app** (`apps/web/remote-lan`, built into
//!   `resources/remote-web`) is served at `/remote`, because a page from an
//!   HTTPS site can't open a `ws://` socket on the LAN.
//!
//! Pairing tokens are random 32-byte values; only their SHA-256 is kept, in
//! the keychain (`NS.remote / lan:<pairing_id>`).

use std::collections::{HashMap, HashSet};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use futures::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::handshake::derive_accept_key;
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;
use tokio_tungstenite::tungstenite::protocol::{CloseFrame, Role, WebSocketConfig};
use tokio_tungstenite::tungstenite::{Error as WsError, Message};
use tokio_tungstenite::WebSocketStream;
use ts_rs::TS;

use tandem_core::keychain::{self, Keychain};
use tandem_core::remote::protocol::Secret32;
use tandem_core::{ErrorCode, IpcError};

use crate::services::phone::{PairingTicket, RelayIn, RelayLink};

/// The port LAN mode listens on unless the person picks another.
pub const DEFAULT_PORT: u16 = 47800;
/// The largest frame forwarded; larger closes the phone's socket with 1009.
pub const MAX_FRAME: usize = 64 * 1024;
/// Phone frames a second per pairing, with an equal burst (D-1902).
pub const FRAMES_PER_SECOND: f64 = 50.0;
const PING_EVERY: Duration = Duration::from_secs(30);
const DEAD_AFTER: Duration = Duration::from_secs(75);
const HEAD_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_HEAD: usize = 16 * 1024;
const RESCAN_EVERY: Duration = Duration::from_secs(30);
const PROTOCOL_PREFIX: &str = "tandem-pair.";

fn lan_account(pid: &str) -> String {
    format!("lan:{pid}")
}

fn remote_service() -> String {
    keychain::service("remote")
}

fn sha256(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

/// Compares two hashes without an early exit.
fn same(a: &[u8; 32], b: &[u8; 32]) -> bool {
    a.iter()
        .zip(b.iter())
        .fold(0u8, |acc, (x, y)| acc | (x ^ y))
        == 0
}

// ── what the UI sees ─────────────────────────────────────────────────────

/// Which kind of network an address is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum LanAddressKind {
    /// A home or office network (RFC 1918).
    Lan,
    /// 100.64.0.0/10, which VPNs such as Tailscale use.
    Vpn,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct LanAddress {
    pub ip: String,
    pub kind: LanAddressKind,
}

/// Settings › Remote › Local network.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct LanState {
    pub enabled: bool,
    /// At least one socket is listening.
    pub running: bool,
    pub port: u16,
    /// The addresses it listens on (a phone can reach any of them).
    pub addresses: Vec<LanAddress>,
    /// Why it isn't listening (a port in use, no private network).
    pub error: Option<String>,
}

/// What `$DATA/remote/lan.json` holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LanConfig {
    pub enabled: bool,
    pub port: u16,
}

impl Default for LanConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            port: DEFAULT_PORT,
        }
    }
}

impl LanConfig {
    pub fn path(data_root: &Path) -> PathBuf {
        data_root.join("remote").join("lan.json")
    }

    /// Off when the file is missing or unreadable.
    pub fn load(data_root: &Path) -> Self {
        std::fs::read(Self::path(data_root))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, data_root: &Path) -> std::io::Result<()> {
        let path = Self::path(data_root);
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let bytes = serde_json::to_vec_pretty(self).map_err(std::io::Error::other)?;
        tandem_core::atomic::atomic_write(&path, &bytes, None)
    }
}

// ── which addresses ──────────────────────────────────────────────────────

/// The kind of network an address to listen on belongs to; `None` for
/// every address LAN mode must never listen on (public, loopback,
/// link-local, unspecified, IPv6).
pub fn listen_kind(ip: IpAddr) -> Option<LanAddressKind> {
    let IpAddr::V4(v4) = ip else { return None };
    if v4.is_private() {
        return Some(LanAddressKind::Lan);
    }
    if is_cgnat(v4) {
        return Some(LanAddressKind::Vpn);
    }
    None
}

fn is_cgnat(v4: Ipv4Addr) -> bool {
    let [a, b, ..] = v4.octets();
    a == 100 && (64..128).contains(&b)
}

/// Whether a connection from this address may be served: the local network,
/// a VPN, link-local or this machine. Everything else is closed unread.
pub fn peer_allowed(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => v4.is_private() || is_cgnat(v4) || v4.is_loopback() || v4.is_link_local(),
        IpAddr::V6(v6) => match v6.to_ipv4_mapped() {
            Some(v4) => peer_allowed(IpAddr::V4(v4)),
            None => v6.is_loopback(),
        },
    }
}

/// The machine's addresses (a seam for tests).
pub trait Interfaces: Send + Sync {
    fn addresses(&self) -> Vec<IpAddr>;
}

/// The OS's interfaces.
pub struct SystemInterfaces;

impl Interfaces for SystemInterfaces {
    fn addresses(&self) -> Vec<IpAddr> {
        if_addrs::get_if_addrs()
            .map(|list| list.into_iter().map(|i| i.ip()).collect())
            .unwrap_or_default()
    }
}

/// The private addresses to listen on, LAN before VPN, each once.
pub fn listen_addresses(interfaces: &dyn Interfaces) -> Vec<(Ipv4Addr, LanAddressKind)> {
    let mut seen = HashSet::new();
    let mut out: Vec<(Ipv4Addr, LanAddressKind)> = interfaces
        .addresses()
        .into_iter()
        .filter_map(|ip| match (ip, listen_kind(ip)) {
            (IpAddr::V4(v4), Some(kind)) if seen.insert(v4) => Some((v4, kind)),
            _ => None,
        })
        .collect();
    out.sort_by_key(|(ip, kind)| (*kind == LanAddressKind::Vpn, *ip));
    out
}

// ── the hub: the relay's pairing table ───────────────────────────────────

enum Out {
    Binary(Vec<u8>),
    Text(String),
    Ping,
    Close(u16, &'static str),
}

struct PhoneConn {
    id: u64,
    tx: mpsc::UnboundedSender<Out>,
}

struct DesktopConn {
    id: u64,
    tx: mpsc::UnboundedSender<RelayIn>,
}

struct Slot {
    token: [u8; 32],
    desktop: Option<DesktopConn>,
    phone: Option<PhoneConn>,
    tokens: f64,
    refilled: Instant,
    noticed: Option<Instant>,
}

impl Slot {
    fn new(token: [u8; 32]) -> Self {
        Self {
            token,
            desktop: None,
            phone: None,
            tokens: FRAMES_PER_SECOND,
            refilled: Instant::now(),
            noticed: None,
        }
    }
}

fn peer_text(up: bool) -> String {
    serde_json::json!({ "type": "peer", "up": up }).to_string()
}

/// The LAN relay's pairings and the sockets joined to each.
#[derive(Default)]
pub struct LanHub {
    slots: Mutex<HashMap<String, Slot>>,
    next_id: std::sync::atomic::AtomicU64,
}

impl LanHub {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, Slot>> {
        self.slots.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn id(&self) -> u64 {
        self.next_id
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    }

    /// Accepts phones that present the token with this SHA-256.
    pub fn register(&self, pid: &str, token_sha256: [u8; 32]) {
        self.lock()
            .entry(pid.to_string())
            .and_modify(|s| s.token = token_sha256)
            .or_insert_with(|| Slot::new(token_sha256));
    }

    pub fn knows(&self, pid: &str) -> bool {
        self.lock().contains_key(pid)
    }

    /// Forgets a pairing: its phone socket closes with 1008 and the
    /// desktop's link is told the same, as the hosted relay does on revoke.
    pub fn remove(&self, pid: &str) {
        if let Some(slot) = self.lock().remove(pid) {
            if let Some(p) = slot.phone {
                let _ = p.tx.send(Out::Close(1008, "revoked"));
            }
            if let Some(d) = slot.desktop {
                let _ = d.tx.send(RelayIn::Closed(Some(1008)));
            }
        }
    }

    /// Closes every phone socket (LAN mode turned off).
    pub fn close_phones(&self, code: u16) {
        let mut slots = self.lock();
        for slot in slots.values_mut() {
            if let Some(p) = slot.phone.take() {
                let _ = p.tx.send(Out::Close(code, "going away"));
            }
            if let Some(d) = &slot.desktop {
                let _ = d.tx.send(RelayIn::Peer(false));
            }
        }
    }

    /// The token's subprotocol when one offered matches the pairing's token.
    pub fn authenticate(&self, pid: &str, offered: &[String]) -> Option<String> {
        let slots = self.lock();
        let slot = slots.get(pid)?;
        offered.iter().find_map(|p| {
            let token = p.strip_prefix(PROTOCOL_PREFIX)?;
            same(&sha256(token.as_bytes()), &slot.token).then(|| p.clone())
        })
    }

    pub fn phone_connected(&self, pid: &str) -> bool {
        self.lock().get(pid).is_some_and(|s| s.phone.is_some())
    }

    /// The desktop's end of a pairing, in-process: the same [`RelayLink`]
    /// a hosted relay socket gives. `None` for a pairing it doesn't know.
    pub fn attach_desktop(self: &Arc<Self>, pid: &str) -> Option<RelayLink> {
        let (tx, mut out_rx) = mpsc::unbounded_channel::<Vec<u8>>();
        let (in_tx, rx) = mpsc::unbounded_channel::<RelayIn>();
        let id = self.id();
        {
            let mut slots = self.lock();
            let slot = slots.get_mut(pid)?;
            if let Some(old) = slot.desktop.replace(DesktopConn {
                id,
                tx: in_tx.clone(),
            }) {
                let _ = old.tx.send(RelayIn::Closed(Some(4000)));
            }
            let _ = in_tx.send(RelayIn::Peer(slot.phone.is_some()));
            if let Some(p) = &slot.phone {
                let _ = p.tx.send(Out::Text(peer_text(true)));
            }
        }
        let hub = self.clone();
        let pid = pid.to_string();
        tokio::spawn(async move {
            while let Some(frame) = out_rx.recv().await {
                let slots = hub.lock();
                let Some(slot) = slots.get(&pid) else { break };
                if slot.desktop.as_ref().map(|d| d.id) != Some(id) {
                    break;
                }
                if let Some(p) = &slot.phone {
                    let _ = p.tx.send(Out::Binary(frame));
                }
            }
            let mut slots = hub.lock();
            if let Some(slot) = slots.get_mut(&pid) {
                if slot.desktop.as_ref().map(|d| d.id) == Some(id) {
                    slot.desktop = None;
                    if let Some(p) = &slot.phone {
                        let _ = p.tx.send(Out::Text(peer_text(false)));
                    }
                }
            }
        });
        Some(RelayLink { tx, rx })
    }

    /// A phone socket joins; an older one for the same pairing is replaced.
    fn join_phone(&self, pid: &str, tx: mpsc::UnboundedSender<Out>) -> Option<u64> {
        let id = self.id();
        let mut slots = self.lock();
        let slot = slots.get_mut(pid)?;
        let _ = tx.send(Out::Text(peer_text(slot.desktop.is_some())));
        if let Some(old) = slot.phone.replace(PhoneConn { id, tx }) {
            let _ = old.tx.send(Out::Close(4000, "replaced"));
        }
        if let Some(d) = &slot.desktop {
            let _ = d.tx.send(RelayIn::Peer(true));
        }
        Some(id)
    }

    fn leave_phone(&self, pid: &str, id: u64) {
        let mut slots = self.lock();
        if let Some(slot) = slots.get_mut(pid) {
            if slot.phone.as_ref().map(|p| p.id) == Some(id) {
                slot.phone = None;
                if let Some(d) = &slot.desktop {
                    let _ = d.tx.send(RelayIn::Peer(false));
                }
            }
        }
    }

    /// A frame from the phone: rate-limited, then handed to the desktop.
    fn phone_frame(&self, pid: &str, id: u64, frame: Vec<u8>) {
        let mut slots = self.lock();
        let Some(slot) = slots.get_mut(pid) else {
            return;
        };
        let Some(phone) = slot.phone.as_ref().filter(|p| p.id == id) else {
            return;
        };
        let now = Instant::now();
        let elapsed = now.duration_since(slot.refilled).as_secs_f64();
        slot.tokens = (slot.tokens + elapsed * FRAMES_PER_SECOND).min(FRAMES_PER_SECOND);
        slot.refilled = now;
        if slot.tokens < 1.0 {
            if slot
                .noticed
                .is_none_or(|t| now.duration_since(t) >= Duration::from_secs(1))
            {
                slot.noticed = Some(now);
                let _ = phone
                    .tx
                    .send(Out::Text(r#"{"type":"rate_limited"}"#.to_string()));
            }
            return;
        }
        slot.tokens -= 1.0;
        if let Some(d) = &slot.desktop {
            let _ = d.tx.send(RelayIn::Frame(frame));
        }
    }

    /// Serves one authenticated phone socket until it closes.
    pub async fn serve_phone<S>(self: Arc<Self>, pid: String, ws: WebSocketStream<S>)
    where
        S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    {
        let (mut sink, mut stream) = ws.split();
        let (tx, mut rx) = mpsc::unbounded_channel::<Out>();
        let Some(id) = self.join_phone(&pid, tx.clone()) else {
            let _ = sink
                .send(Message::Close(Some(CloseFrame {
                    code: CloseCode::from(1008),
                    reason: "unauthorized".into(),
                })))
                .await;
            return;
        };
        tracing::info!(code = "phone.lan_connect", pairing = %pid, "phone connected over the LAN");
        let writer = tokio::spawn(async move {
            while let Some(out) = rx.recv().await {
                let sent = match out {
                    Out::Binary(b) => sink.send(Message::Binary(b.into())).await,
                    Out::Text(t) => sink.send(Message::Text(t.into())).await,
                    Out::Ping => sink.send(Message::Ping(Vec::new().into())).await,
                    Out::Close(code, reason) => {
                        let _ = sink
                            .send(Message::Close(Some(CloseFrame {
                                code: CloseCode::from(code),
                                reason: reason.into(),
                            })))
                            .await;
                        break;
                    }
                };
                if sent.is_err() {
                    break;
                }
            }
            let _ = sink.close().await;
        });
        let mut ping = tokio::time::interval(PING_EVERY);
        ping.tick().await;
        let mut seen = Instant::now();
        loop {
            tokio::select! {
                msg = stream.next() => match msg {
                    Some(Ok(Message::Binary(bytes))) => {
                        seen = Instant::now();
                        self.phone_frame(&pid, id, bytes.to_vec());
                    }
                    Some(Ok(Message::Text(_))) => {
                        let _ = tx.send(Out::Close(1003, "binary frames only"));
                        break;
                    }
                    Some(Ok(Message::Ping(_) | Message::Pong(_) | Message::Frame(_))) => {
                        seen = Instant::now();
                    }
                    Some(Err(WsError::Capacity(_))) => {
                        let _ = tx.send(Out::Close(1009, "frame too large"));
                        break;
                    }
                    Some(Ok(Message::Close(_))) | Some(Err(_)) | None => break,
                },
                _ = ping.tick() => {
                    if seen.elapsed() > DEAD_AFTER {
                        break;
                    }
                    let _ = tx.send(Out::Ping);
                }
            }
        }
        self.leave_phone(&pid, id);
        drop(tx);
        let _ = tokio::time::timeout(Duration::from_secs(2), writer).await;
        tracing::info!(code = "phone.lan_disconnect", pairing = %pid, "phone disconnected");
    }
}

// ── the phone web app's files ────────────────────────────────────────────

/// The phone web app (`resources/remote-web`), read once into memory. Only
/// these files are ever served, so no request path reaches the disk.
#[derive(Default)]
pub struct WebFiles {
    files: HashMap<String, Vec<u8>>,
}

const MISSING_APP: &str = "<!doctype html><meta charset=utf-8><meta name=viewport content=\"width=device-width\"><title>Remote</title><body style=\"font-family:system-ui;background:#060a12;color:#e2e8f0;padding:24px\"><h1>Vibezeo Remote</h1><p>This build of the desktop app doesn't include the phone web app. Open the code with the Vibezeo Remote app instead.</p>";

fn content_type(name: &str) -> &'static str {
    match name.rsplit('.').next().unwrap_or_default() {
        "html" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" | "webmanifest" => "application/manifest+json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "ico" => "image/x-icon",
        "woff2" => "font/woff2",
        "woff" => "font/woff",
        _ => "application/octet-stream",
    }
}

impl WebFiles {
    /// Reads every regular file under `dir` (two levels of folders at most).
    pub fn load(dir: &Path) -> Self {
        fn walk(dir: &Path, prefix: &str, depth: u8, out: &mut HashMap<String, Vec<u8>>) {
            let Ok(entries) = std::fs::read_dir(dir) else {
                return;
            };
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if name.starts_with('.') {
                    continue;
                }
                let Ok(meta) = entry.metadata() else { continue };
                let key = format!("{prefix}{name}");
                if meta.is_dir() && depth < 2 {
                    walk(&entry.path(), &format!("{key}/"), depth + 1, out);
                } else if meta.is_file() && meta.len() <= 8 * 1024 * 1024 {
                    if let Ok(bytes) = std::fs::read(entry.path()) {
                        out.insert(key, bytes);
                    }
                }
            }
        }
        let mut files = HashMap::new();
        walk(dir, "", 0, &mut files);
        files.remove("README.md");
        Self { files }
    }

    pub fn from_map(files: HashMap<String, Vec<u8>>) -> Self {
        Self { files }
    }

    pub fn has_app(&self) -> bool {
        self.files.contains_key("index.html")
    }

    fn get(&self, name: &str) -> Option<(&[u8], &'static str)> {
        self.files
            .get(name)
            .map(|b| (b.as_slice(), content_type(name)))
    }
}

// ── HTTP ─────────────────────────────────────────────────────────────────

struct Head {
    method: String,
    path: String,
    headers: Vec<(String, String)>,
}

impl Head {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    fn has_token(&self, name: &str, token: &str) -> bool {
        self.header(name)
            .is_some_and(|v| v.split(',').any(|t| t.trim().eq_ignore_ascii_case(token)))
    }
}

/// Reads a request head; returns it and any bytes read past it.
async fn read_head<S: AsyncRead + Unpin>(stream: &mut S) -> Option<(Head, Vec<u8>)> {
    let mut buf = Vec::with_capacity(2048);
    let mut chunk = [0u8; 2048];
    loop {
        let n = stream.read(&mut chunk).await.ok()?;
        if n == 0 {
            return None;
        }
        buf.extend_from_slice(&chunk[..n]);
        let mut headers = [httparse::EMPTY_HEADER; 48];
        let mut req = httparse::Request::new(&mut headers);
        match req.parse(&buf) {
            Ok(httparse::Status::Complete(len)) => {
                let head = Head {
                    method: req.method?.to_string(),
                    path: req.path?.to_string(),
                    headers: req
                        .headers
                        .iter()
                        .map(|h| {
                            (
                                h.name.to_string(),
                                String::from_utf8_lossy(h.value).to_string(),
                            )
                        })
                        .collect(),
                };
                return Some((head, buf[len..].to_vec()));
            }
            Ok(httparse::Status::Partial) if buf.len() < MAX_HEAD => continue,
            _ => return None,
        }
    }
}

fn response(status: &str, headers: &[(&str, String)], body: &[u8]) -> Vec<u8> {
    let mut out = format!("HTTP/1.1 {status}\r\n");
    for (k, v) in headers {
        out.push_str(&format!("{k}: {v}\r\n"));
    }
    out.push_str(&format!(
        "Content-Length: {}\r\nConnection: close\r\nX-Content-Type-Options: nosniff\r\nReferrer-Policy: no-referrer\r\n\r\n",
        body.len()
    ));
    let mut bytes = out.into_bytes();
    bytes.extend_from_slice(body);
    bytes
}

/// The pairing id in `/v1/relay/:pairing_id`, if the path is that.
fn relay_pid(path: &str) -> Option<&str> {
    let pid = path.split('?').next()?.strip_prefix("/v1/relay/")?;
    let ok = pid.len() >= 7
        && pid.len() <= 67
        && pid.starts_with("pr_")
        && pid[3..]
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    ok.then_some(pid)
}

/// Serves one connection: the relay's WebSocket or a web app file.
pub async fn serve_connection<S>(
    hub: Arc<LanHub>,
    web: Arc<WebFiles>,
    mut stream: S,
    local: SocketAddr,
) where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let Ok(Some((head, rest))) = tokio::time::timeout(HEAD_TIMEOUT, read_head(&mut stream)).await
    else {
        return;
    };
    // DNS rebinding: the request must be addressed to this socket by IP.
    let host_ok = head
        .header("host")
        .is_some_and(|h| h.trim() == local.to_string());
    if !host_ok {
        let _ = stream
            .write_all(&response(
                "421 Misdirected Request",
                &[("Content-Type", "text/plain".into())],
                b"",
            ))
            .await;
        return;
    }
    if let Some(pid) = relay_pid(&head.path) {
        let upgrade = head.method == "GET"
            && head.has_token("upgrade", "websocket")
            && head.has_token("connection", "upgrade");
        let Some(key) = head.header("sec-websocket-key").filter(|_| upgrade) else {
            let _ = stream
                .write_all(&response("426 Upgrade Required", &[], b""))
                .await;
            return;
        };
        let offered: Vec<String> = head
            .header("sec-websocket-protocol")
            .unwrap_or_default()
            .split(',')
            .map(|p| p.trim().to_string())
            .filter(|p| !p.is_empty())
            .collect();
        let chosen = hub.authenticate(pid, &offered);
        // Echo the offered subprotocol even when refusing, so the phone sees
        // the 1008 close rather than a failed handshake (D-1901).
        let echo = chosen.clone().or_else(|| offered.first().cloned());
        let mut reply = format!(
            "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {}\r\n",
            derive_accept_key(key.as_bytes())
        );
        if let Some(p) = &echo {
            reply.push_str(&format!("Sec-WebSocket-Protocol: {p}\r\n"));
        }
        reply.push_str("\r\n");
        if stream.write_all(reply.as_bytes()).await.is_err() {
            return;
        }
        let config = WebSocketConfig::default()
            .max_message_size(Some(MAX_FRAME))
            .max_frame_size(Some(MAX_FRAME));
        let mut ws =
            WebSocketStream::from_partially_read(stream, rest, Role::Server, Some(config)).await;
        if chosen.is_none() {
            tracing::info!(code = "phone.lan_refused", "LAN relay refused a phone");
            let _ = ws
                .send(Message::Close(Some(CloseFrame {
                    code: CloseCode::from(1008),
                    reason: "unauthorized".into(),
                })))
                .await;
            let _ = ws.close(None).await;
            return;
        }
        hub.serve_phone(pid.to_string(), ws).await;
        return;
    }
    if head.method != "GET" && head.method != "HEAD" {
        let _ = stream
            .write_all(&response("405 Method Not Allowed", &[], b""))
            .await;
        return;
    }
    let path = head.path.split(['?', '#']).next().unwrap_or("/");
    if path == "/" {
        let _ = stream
            .write_all(&response(
                "302 Found",
                &[("Location", "/remote".into())],
                b"",
            ))
            .await;
        return;
    }
    let name = match path {
        "/remote" | "/remote/" => "index.html".to_string(),
        p => match p.strip_prefix("/remote/") {
            Some(rest) if !rest.contains("..") => rest.to_string(),
            _ => String::new(),
        },
    };
    let csp = format!(
        "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; font-src 'self' data:; connect-src 'self' ws://{local}; frame-ancestors 'none'; base-uri 'none'; form-action 'none'"
    );
    let found = web.get(&name);
    let (status, body, ctype) = match found {
        Some((bytes, ctype)) => ("200 OK", bytes, ctype),
        None if name == "index.html" => {
            ("200 OK", MISSING_APP.as_bytes(), "text/html; charset=utf-8")
        }
        None => ("404 Not Found", &b"Not found"[..], "text/plain"),
    };
    let cache = if name.starts_with("assets/") {
        "public, max-age=31536000, immutable"
    } else {
        "no-store"
    };
    let headers = [
        ("Content-Type", ctype.to_string()),
        ("Cache-Control", cache.to_string()),
        ("Content-Security-Policy", csp),
        ("X-Frame-Options", "DENY".to_string()),
    ];
    let mut bytes = response(status, &headers, body);
    if head.method == "HEAD" {
        let cut = bytes.len() - body.len();
        bytes.truncate(cut);
    }
    let _ = stream.write_all(&bytes).await;
    let _ = stream.shutdown().await;
}

// ── the server ───────────────────────────────────────────────────────────

struct Listener {
    kind: LanAddressKind,
    addr: SocketAddr,
    task: tokio::task::JoinHandle<()>,
}

struct Running {
    port: u16,
    listeners: HashMap<Ipv4Addr, Listener>,
    rescan: Option<tokio::task::JoinHandle<()>>,
    error: Option<String>,
}

/// Which addresses may be listened on: [`listen_kind`] in the app.
pub type ListenPolicy = fn(IpAddr) -> Option<LanAddressKind>;

/// Listens on the machine's private addresses while LAN mode is on.
pub struct LanServer {
    hub: Arc<LanHub>,
    web: Mutex<Arc<WebFiles>>,
    interfaces: Arc<dyn Interfaces>,
    policy: ListenPolicy,
    running: Mutex<Option<Running>>,
}

impl LanServer {
    pub fn new(hub: Arc<LanHub>, web: WebFiles, interfaces: Arc<dyn Interfaces>) -> Arc<Self> {
        Self::with_policy(hub, web, interfaces, listen_kind)
    }

    /// For tests that must listen on loopback; the app always uses
    /// [`LanServer::new`].
    pub fn with_policy(
        hub: Arc<LanHub>,
        web: WebFiles,
        interfaces: Arc<dyn Interfaces>,
        policy: ListenPolicy,
    ) -> Arc<Self> {
        Arc::new(Self {
            hub,
            web: Mutex::new(Arc::new(web)),
            interfaces,
            policy,
            running: Mutex::new(None),
        })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Option<Running>> {
        self.running.lock().unwrap_or_else(|p| p.into_inner())
    }

    pub fn hub(&self) -> &Arc<LanHub> {
        &self.hub
    }

    /// The phone web app's files (read at app start from the resources).
    pub fn set_web(&self, web: WebFiles) {
        *self.web.lock().unwrap_or_else(|p| p.into_inner()) = Arc::new(web);
    }

    fn web(&self) -> Arc<WebFiles> {
        self.web.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }

    /// Starts listening on `port` (0 picks one per address) or keeps
    /// listening. Returns the state.
    pub async fn start(self: &Arc<Self>, port: u16) -> LanState {
        let same_port = self.lock().as_ref().map(|r| r.port) == Some(port);
        if !same_port {
            self.stop();
            *self.lock() = Some(Running {
                port,
                listeners: HashMap::new(),
                rescan: None,
                error: None,
            });
            self.rescan().await;
            let me = Arc::downgrade(self);
            let task = tokio::spawn(async move {
                loop {
                    tokio::time::sleep(RESCAN_EVERY).await;
                    let Some(me) = me.upgrade() else { return };
                    me.rescan().await;
                }
            });
            if let Some(r) = self.lock().as_mut() {
                r.rescan = Some(task);
            }
        }
        self.state(true)
    }

    /// Stops listening and closes every phone socket.
    pub fn stop(&self) {
        let Some(running) = self.lock().take() else {
            return;
        };
        if let Some(task) = running.rescan {
            task.abort();
        }
        for (_, l) in running.listeners {
            l.task.abort();
        }
        self.hub.close_phones(1001);
    }

    pub fn is_running(&self) -> bool {
        self.lock()
            .as_ref()
            .is_some_and(|r| !r.listeners.is_empty())
    }

    /// The private addresses to listen on, LAN before VPN, each once.
    fn wanted(&self) -> Vec<(Ipv4Addr, LanAddressKind)> {
        let mut seen = HashSet::new();
        let mut out: Vec<(Ipv4Addr, LanAddressKind)> = self
            .interfaces
            .addresses()
            .into_iter()
            .filter_map(|ip| match (ip, (self.policy)(ip)) {
                (IpAddr::V4(v4), Some(kind)) if seen.insert(v4) => Some((v4, kind)),
                _ => None,
            })
            .collect();
        out.sort_by_key(|(ip, kind)| (*kind == LanAddressKind::Vpn, *ip));
        out
    }

    /// Binds new private addresses and lets go of ones that went away.
    pub async fn rescan(self: &Arc<Self>) {
        let Some(port) = self.lock().as_ref().map(|r| r.port) else {
            return;
        };
        let wanted = self.wanted();
        let current: HashSet<Ipv4Addr> = self
            .lock()
            .as_ref()
            .map(|r| r.listeners.keys().copied().collect())
            .unwrap_or_default();
        let mut error = None;
        for (ip, kind) in &wanted {
            if current.contains(ip) {
                continue;
            }
            let bound = TcpListener::bind(SocketAddr::new(IpAddr::V4(*ip), port))
                .await
                .and_then(|l| l.local_addr().map(|a| (l, a)));
            match bound {
                Ok((listener, addr)) => {
                    let task = self.accept_loop(listener);
                    if let Some(r) = self.lock().as_mut() {
                        r.listeners.insert(
                            *ip,
                            Listener {
                                kind: *kind,
                                addr,
                                task,
                            },
                        );
                    } else {
                        task.abort();
                    }
                }
                Err(err) => {
                    tracing::warn!(code = "phone.lan_bind_failed", ip = %ip, port, error = %err, "couldn't listen");
                    error = Some(if err.kind() == std::io::ErrorKind::AddrInUse {
                        format!("Port {port} is in use. Pick another port.")
                    } else {
                        format!("Couldn't listen on {ip}:{port}.")
                    });
                }
            }
        }
        let keep: HashSet<Ipv4Addr> = wanted.iter().map(|(ip, _)| *ip).collect();
        if let Some(r) = self.lock().as_mut() {
            r.listeners.retain(|ip, l| {
                let k = keep.contains(ip);
                if !k {
                    l.task.abort();
                }
                k
            });
            r.error = if wanted.is_empty() {
                Some("This computer isn't on a private network.".into())
            } else {
                error
            };
        }
    }

    fn accept_loop(self: &Arc<Self>, listener: TcpListener) -> tokio::task::JoinHandle<()> {
        let me = Arc::downgrade(self);
        tokio::spawn(async move {
            loop {
                let Ok((stream, peer)) = listener.accept().await else {
                    tokio::time::sleep(Duration::from_millis(200)).await;
                    continue;
                };
                let Some(server) = me.upgrade() else { return };
                accept_one(server.hub.clone(), server.web(), stream, peer);
            }
        })
    }

    /// Where it listens, LAN before VPN.
    fn sockets(running: &Running) -> Vec<(SocketAddr, LanAddressKind)> {
        let mut out: Vec<(SocketAddr, LanAddressKind)> = running
            .listeners
            .values()
            .map(|l| (l.addr, l.kind))
            .collect();
        out.sort_by_key(|(addr, kind)| (*kind == LanAddressKind::Vpn, *addr));
        out
    }

    pub fn state(&self, enabled: bool) -> LanState {
        let running = self.lock();
        let sockets = running.as_ref().map(Self::sockets).unwrap_or_default();
        LanState {
            enabled,
            running: !sockets.is_empty(),
            port: sockets
                .first()
                .map(|(a, _)| a.port())
                .or(running.as_ref().map(|r| r.port))
                .unwrap_or(DEFAULT_PORT),
            addresses: sockets
                .iter()
                .map(|(a, kind)| LanAddress {
                    ip: a.ip().to_string(),
                    kind: *kind,
                })
                .collect(),
            error: running.as_ref().and_then(|r| r.error.clone()),
        }
    }

    /// The socket a pairing code should use: the address asked for if it's
    /// listening, else the first (LAN before VPN).
    pub fn address_for(&self, wanted: Option<&str>) -> Option<SocketAddr> {
        let running = self.lock();
        let sockets = Self::sockets(running.as_ref()?);
        wanted
            .and_then(|w| sockets.iter().find(|(a, _)| a.ip().to_string() == w))
            .or_else(|| sockets.first())
            .map(|(a, _)| *a)
    }
}

/// Accepts one connection if its peer is on the LAN; drops it unread
/// otherwise.
pub fn accept_one(hub: Arc<LanHub>, web: Arc<WebFiles>, stream: TcpStream, peer: SocketAddr) {
    if !peer_allowed(peer.ip()) {
        tracing::info!(
            code = "phone.lan_peer_refused",
            "closed a connection from outside the LAN"
        );
        drop(stream);
        return;
    }
    let Ok(local) = stream.local_addr() else {
        return;
    };
    let _ = stream.set_nodelay(true);
    tokio::spawn(serve_connection(hub, web, stream, local));
}

// ── the phone service's network for LAN pairings ─────────────────────────

/// Creates, opens and deletes LAN pairings for the phone service.
pub struct LanNet {
    server: Arc<LanServer>,
    keychain: Arc<dyn Keychain>,
}

impl LanNet {
    pub fn new(server: Arc<LanServer>, keychain: Arc<dyn Keychain>) -> Arc<Self> {
        Arc::new(Self { server, keychain })
    }

    pub fn server(&self) -> &Arc<LanServer> {
        &self.server
    }

    /// Whether this pairing is a LAN pairing (this run, or saved).
    pub fn owns(&self, pid: &str) -> bool {
        self.server.hub().knows(pid) || self.load(pid)
    }

    /// Restores a saved pairing's token hash into the hub.
    fn load(&self, pid: &str) -> bool {
        let Ok(Some(raw)) = self.keychain.get(&remote_service(), &lan_account(pid)) else {
            return false;
        };
        let Ok(hash) = <[u8; 32]>::try_from(raw.as_slice()) else {
            return false;
        };
        self.server.hub().register(pid, hash);
        true
    }

    /// A pairing on the LAN: a fresh token, and the code's links on the
    /// chosen address (`ws://` relay, `http://` web app).
    pub fn create_pairing(
        &self,
        pid: &str,
        address: Option<&str>,
    ) -> Result<PairingTicket, IpcError> {
        let addr = self.server.address_for(address).ok_or_else(|| {
            IpcError::new(
                ErrorCode::Unavailable,
                "Turn on pairing over your local network first.",
            )
        })?;
        let token = URL_SAFE_NO_PAD.encode(Secret32::random().0);
        let hash = sha256(token.as_bytes());
        self.keychain
            .set(&remote_service(), &lan_account(pid), &hash)
            .map_err(|e| IpcError::new(ErrorCode::Unavailable, e.to_string()))?;
        self.server.hub().register(pid, hash);
        Ok(PairingTicket {
            pairing_token: token,
            relay_url: format!("ws://{addr}/v1/relay/{pid}"),
            web_base: Some(format!("http://{addr}")),
        })
    }

    pub fn delete_pairing(&self, pid: &str) {
        let _ = self.keychain.delete(&remote_service(), &lan_account(pid));
        self.server.hub().remove(pid);
    }

    /// The desktop's in-process end of a LAN pairing.
    pub fn dial(&self, pid: &str) -> Result<RelayLink, String> {
        if !self.server.hub().knows(pid) && !self.load(pid) {
            return Err("not a LAN pairing".into());
        }
        self.server
            .hub()
            .attach_desktop(pid)
            .ok_or_else(|| "not a LAN pairing".into())
    }
}

/// The pairing id at the end of a relay URL.
pub fn pid_of_relay_url(url: &str) -> Option<&str> {
    url.split('?')
        .next()?
        .rsplit('/')
        .next()
        .filter(|p| p.starts_with("pr_"))
}

#[cfg(test)]
mod tests;
