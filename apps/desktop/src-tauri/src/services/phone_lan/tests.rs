//! F-RM-04 LAN mode: the desktop's own relay on the local network, with a
//! real phone service and a scripted phone over real sockets on loopback.

use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use futures::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

use tandem_core::approvals::ApprovalView;
use tandem_core::bus::EventBus;
use tandem_core::clock::MockClock;
use tandem_core::keychain::{Keychain, MemoryKeychain};
use tandem_core::remote::messages::PhoneDecision;
use tandem_core::remote::protocol::{
    encode_hello, hello_tag, root_secret, KeyPair, Role, Secret32, Session,
};
use tandem_core::IpcError;

use super::*;
use crate::services::phone::{
    PairVia, PhoneAgent, PhoneHost, PhoneNet, PhoneRoutine, PhoneService, PhoneThread,
    PhoneTranscript, RekeyPolicy,
};

struct Ifaces(Vec<IpAddr>);

impl Interfaces for Ifaces {
    fn addresses(&self) -> Vec<IpAddr> {
        self.0.clone()
    }
}

fn loopback_policy(ip: IpAddr) -> Option<LanAddressKind> {
    ip.is_loopback().then_some(LanAddressKind::Lan)
}

#[derive(Default)]
struct Host;

#[async_trait]
impl PhoneHost for Host {
    fn desktop_name(&self) -> String {
        "Desk".into()
    }
    async fn agents(&self) -> Vec<PhoneAgent> {
        Vec::new()
    }
    fn approvals(&self) -> Vec<ApprovalView> {
        Vec::new()
    }
    async fn threads(&self) -> Vec<PhoneThread> {
        Vec::new()
    }
    async fn routines(&self) -> Vec<PhoneRoutine> {
        Vec::new()
    }
    async fn thread_read(
        &self,
        _: &str,
        _: Option<i64>,
        _: u32,
    ) -> Result<PhoneTranscript, String> {
        Err("none".into())
    }
    async fn thread_send(&self, _: &str, _: &str) -> Result<(), String> {
        Ok(())
    }
    async fn pane_type(&self, _: &str, _: &str) -> Result<(), String> {
        Ok(())
    }
    fn approval_resolve(&self, _: &str, _: PhoneDecision) -> Result<(), String> {
        Ok(())
    }
    async fn routine_run_now(&self, _: &str) -> Result<(), String> {
        Ok(())
    }
    async fn queued_notice(&self, _: &ApprovalView, _: &str) {}
}

/// The app's routing, cut down to LAN pairings only.
struct Net(Arc<LanNet>);

#[async_trait]
impl PhoneNet for Net {
    async fn create_pairing(&self, _: &str, _: Option<&str>) -> Result<PairingTicket, IpcError> {
        Err(IpcError::new(ErrorCode::Unavailable, "no hosted relay"))
    }
    async fn delete_pairing(&self, pid: &str) -> Result<(), String> {
        self.0.delete_pairing(pid);
        Ok(())
    }
    async fn dial(&self, url: &str) -> Result<RelayLink, String> {
        self.0.dial(pid_of_relay_url(url).ok_or("no pid")?)
    }
    async fn create_lan_pairing(
        &self,
        pid: &str,
        address: Option<&str>,
    ) -> Result<PairingTicket, IpcError> {
        self.0.create_pairing(pid, address)
    }
    fn forget_local(&self, pid: &str) {
        self.0.delete_pairing(pid);
    }
}

struct Rig {
    _dir: tempfile::TempDir,
    svc: Arc<PhoneService>,
    lan: Arc<LanNet>,
    keychain: Arc<MemoryKeychain>,
}

async fn rig(web: WebFiles) -> Rig {
    let dir = tempfile::tempdir().expect("tmp");
    let data = tandem_core::DataDir::at(dir.path().to_path_buf()).expect("data");
    let store = tandem_store::Store::open(&data).expect("store");
    let keychain = Arc::new(MemoryKeychain::new());
    let server = LanServer::with_policy(
        LanHub::new(),
        web,
        Arc::new(Ifaces(vec![IpAddr::V4(Ipv4Addr::LOCALHOST)])),
        loopback_policy,
    );
    let state = server.start(0).await;
    assert!(state.running, "{state:?}");
    let lan = LanNet::new(server, keychain.clone());
    let svc = PhoneService::new(
        store,
        EventBus::new(),
        keychain.clone(),
        Arc::new(MockClock::new(chrono::Utc::now())),
        "https://app.test".into(),
        RekeyPolicy::default(),
    );
    svc.bind(Arc::new(Host), Arc::new(Net(lan.clone())));
    Rig {
        _dir: dir,
        svc,
        lan,
        keychain,
    }
}

type Ws = WebSocketStream<MaybeTlsStream<TcpStream>>;

async fn open(url: &str, token: &str) -> Ws {
    let mut req = url.into_client_request().expect("req");
    req.headers_mut().insert(
        "sec-websocket-protocol",
        format!("tandem-pair.{token}").parse().expect("header"),
    );
    let (ws, resp) = tokio_tungstenite::connect_async(req)
        .await
        .expect("connect");
    assert_eq!(
        resp.headers()
            .get("sec-websocket-protocol")
            .and_then(|v| v.to_str().ok()),
        Some(format!("tandem-pair.{token}").as_str())
    );
    ws
}

/// The next message, or what closed the socket.
async fn next(ws: &mut Ws) -> Message {
    tokio::time::timeout(Duration::from_secs(5), ws.next())
        .await
        .expect("in time")
        .expect("open")
        .expect("message")
}

async fn next_close_code(ws: &mut Ws) -> u16 {
    loop {
        match tokio::time::timeout(Duration::from_secs(5), ws.next())
            .await
            .expect("in time")
        {
            Some(Ok(Message::Close(Some(frame)))) => return u16::from(frame.code),
            Some(Ok(Message::Close(None))) | None | Some(Err(_)) => return 0,
            Some(Ok(_)) => {}
        }
    }
}

struct Phone {
    ws: Ws,
    session: Session,
    pid: String,
    relay: String,
    token: String,
}

impl Phone {
    async fn send(&mut self, msg: Value) {
        let frame = self
            .session
            .seal_next(msg.to_string().as_bytes())
            .expect("seal");
        self.ws
            .send(Message::Binary(frame.into()))
            .await
            .expect("send");
    }

    /// The next sealed message from the desktop.
    async fn recv(&mut self) -> Value {
        loop {
            match next(&mut self.ws).await {
                Message::Binary(b) => {
                    let plain = self.session.open_next(&b).expect("open");
                    return serde_json::from_slice(&plain).expect("json");
                }
                Message::Text(_) => {}
                other => panic!("unexpected {other:?}"),
            }
        }
    }

    async fn command(&mut self, id: u64, cmd: &str) -> Value {
        self.send(json!({"t": "cmd", "id": id, "cmd": cmd, "args": {}}))
            .await;
        loop {
            let v = self.recv().await;
            if v["t"] == "res" {
                return v;
            }
        }
    }
}

fn link_payload(link: &str) -> Value {
    let p = link.split("#p=").nth(1).expect("fragment");
    serde_json::from_slice(&URL_SAFE_NO_PAD.decode(p).expect("b64")).expect("json")
}

fn key(v: &Value) -> [u8; 32] {
    URL_SAFE_NO_PAD
        .decode(v.as_str().expect("str"))
        .expect("b64")
        .try_into()
        .expect("32")
}

/// Scans the code the way the apps do and pairs over the LAN relay.
async fn pair(rig: &Rig) -> (Phone, String) {
    let qr = rig
        .svc
        .pair_begin_via(PairVia::Lan { address: None })
        .await
        .expect("pair");
    let v = link_payload(&qr.link);
    let pid = v["pid"].as_str().expect("pid").to_string();
    let relay = v["relay"].as_str().expect("relay").to_string();
    let token = v["ptok"].as_str().expect("ptok").to_string();
    let (dpk, s) = (key(&v["dpk"]), Secret32(key(&v["s"])));
    let me = KeyPair::generate();
    let root = root_secret(&s, &pid, &dpk, &me.public, &me.secret, &dpk).expect("root");
    let mut ws = open(&relay, &token).await;
    // The desktop's in-process end is there before the phone arrives.
    let mut up = false;
    for _ in 0..5 {
        if let Message::Text(t) = next(&mut ws).await {
            let v: Value = serde_json::from_str(t.as_str()).expect("json");
            if v["type"] == "peer" && v["up"] == true {
                up = true;
                break;
            }
        }
    }
    assert!(up, "the relay says the desktop is there");
    ws.send(Message::Binary(
        encode_hello(&me.public, &hello_tag(&s, &pid, &me.public)).into(),
    ))
    .await
    .expect("hello");
    let mut phone = Phone {
        ws,
        session: Session::new(&pid, Role::Phone, root),
        pid,
        relay,
        token,
    };
    phone.send(json!({"t": "hi", "device": "Test phone"})).await;
    let welcome = phone.recv().await;
    assert_eq!(welcome["t"], "welcome", "{welcome}");
    assert_eq!(welcome["desktop"], "Desk");
    (phone, qr.link)
}

#[test]
fn f_rm_04_1_listens_only_on_private_and_vpn_addresses() {
    let v4 = |a, b, c, d| IpAddr::V4(Ipv4Addr::new(a, b, c, d));
    assert_eq!(listen_kind(v4(192, 168, 1, 20)), Some(LanAddressKind::Lan));
    assert_eq!(listen_kind(v4(10, 0, 0, 5)), Some(LanAddressKind::Lan));
    assert_eq!(listen_kind(v4(172, 16, 4, 1)), Some(LanAddressKind::Lan));
    assert_eq!(listen_kind(v4(100, 101, 2, 3)), Some(LanAddressKind::Vpn));
    for never in [
        v4(0, 0, 0, 0),
        v4(8, 8, 8, 8),
        v4(127, 0, 0, 1),
        v4(169, 254, 1, 1),
        v4(100, 128, 0, 1),
        v4(172, 32, 0, 1),
        IpAddr::V6(Ipv6Addr::UNSPECIFIED),
        IpAddr::V6(Ipv6Addr::LOCALHOST),
    ] {
        assert_eq!(listen_kind(never), None, "{never}");
    }
    // Who may connect: the LAN, a VPN, link-local and this machine.
    assert!(peer_allowed(v4(192, 168, 1, 30)));
    assert!(peer_allowed(v4(100, 64, 0, 9)));
    assert!(peer_allowed(v4(127, 0, 0, 1)));
    assert!(peer_allowed(v4(169, 254, 3, 3)));
    assert!(!peer_allowed(v4(8, 8, 8, 8)));
    assert!(!peer_allowed(v4(100, 128, 0, 1)));
    assert!(!peer_allowed(IpAddr::V6(
        "2001:db8::1".parse().expect("v6")
    )));
    assert!(peer_allowed(IpAddr::V6(
        "::ffff:192.168.1.4".parse().expect("mapped")
    )));
    let picked = listen_addresses(&Ifaces(vec![
        v4(100, 70, 1, 1),
        v4(8, 8, 4, 4),
        v4(192, 168, 1, 20),
        v4(192, 168, 1, 20),
        v4(127, 0, 0, 1),
    ]));
    assert_eq!(
        picked,
        vec![
            (Ipv4Addr::new(192, 168, 1, 20), LanAddressKind::Lan),
            (Ipv4Addr::new(100, 70, 1, 1), LanAddressKind::Vpn),
        ]
    );
}

#[tokio::test]
async fn f_rm_04_1_no_private_network_means_nothing_listens() {
    let server = LanServer::new(
        LanHub::new(),
        WebFiles::default(),
        Arc::new(Ifaces(vec![
            IpAddr::V4(Ipv4Addr::LOCALHOST),
            IpAddr::V4(Ipv4Addr::UNSPECIFIED),
            IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8)),
            IpAddr::V6(Ipv6Addr::UNSPECIFIED),
        ])),
    );
    let state = server.start(0).await;
    assert!(!state.running);
    assert!(state.addresses.is_empty());
    assert_eq!(
        state.error.as_deref(),
        Some("This computer isn't on a private network.")
    );
    assert!(server.address_for(None).is_none());
}

#[test]
fn f_rm_04_1_lan_mode_starts_off() {
    let dir = tempfile::tempdir().expect("tmp");
    let config = LanConfig::load(dir.path());
    assert!(!config.enabled);
    assert_eq!(config.port, DEFAULT_PORT);
    LanConfig {
        enabled: true,
        port: 48000,
    }
    .save(dir.path())
    .expect("save");
    assert_eq!(
        LanConfig::load(dir.path()),
        LanConfig {
            enabled: true,
            port: 48000
        }
    );
}

#[tokio::test]
async fn f_rm_04_2_a_phone_pairs_and_commands_over_the_lan_relay() {
    let rig = rig(WebFiles::default()).await;
    let (mut phone, link) = pair(&rig).await;
    // The code points at the desktop itself: its web app and its relay.
    assert!(link.starts_with("http://127.0.0.1:"), "{link}");
    assert!(link.contains("/remote#p="), "{link}");
    assert!(
        phone.relay.starts_with("ws://127.0.0.1:"),
        "{}",
        phone.relay
    );
    assert!(phone.relay.ends_with(&format!("/v1/relay/{}", phone.pid)));
    let res = phone.command(1, "view.snapshot").await;
    assert_eq!(res["ok"], true, "{res}");
    assert!(res["data"]["agents"].is_array());
    // Only the token's hash is kept.
    let saved = rig
        .keychain
        .get(&keychain::service("remote"), &format!("lan:{}", phone.pid))
        .expect("keychain")
        .expect("saved");
    assert_eq!(saved.as_slice(), sha256(phone.token.as_bytes()).as_slice());
    let listed = rig.svc.list().await.expect("list");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].device_name, "Test phone");
}

#[tokio::test]
async fn f_rm_04_2_a_replayed_frame_is_rejected_over_the_lan() {
    let rig = rig(WebFiles::default()).await;
    let (mut phone, _) = pair(&rig).await;
    let frame = phone
        .session
        .seal_next(
            json!({"t": "cmd", "id": 7, "cmd": "view.snapshot", "args": {}})
                .to_string()
                .as_bytes(),
        )
        .expect("seal");
    phone
        .ws
        .send(Message::Binary(frame.clone().into()))
        .await
        .expect("send");
    assert_eq!(phone.recv().await["t"], "res");
    phone
        .ws
        .send(Message::Binary(frame.into()))
        .await
        .expect("replay");
    let v = phone.recv().await;
    assert_eq!(v["t"], "reject", "{v}");
    assert_eq!(v["reason"], "replay");
}

#[tokio::test]
async fn f_rm_04_3_the_lan_relay_refuses_bad_tokens_text_and_big_frames() {
    let rig = rig(WebFiles::default()).await;
    let (mut phone, _) = pair(&rig).await;
    // A wrong token: upgraded, then closed with 1008.
    let mut bad = open(&phone.relay, "not-the-token").await;
    assert_eq!(next_close_code(&mut bad).await, 1008);
    // An unknown pairing: the same.
    let unknown = phone.relay.replace(&phone.pid, "pr_unknownpairing0");
    let mut other = open(&unknown, &phone.token).await;
    assert_eq!(next_close_code(&mut other).await, 1008);
    // A text frame closes with 1003.
    phone
        .ws
        .send(Message::Text("hello".into()))
        .await
        .expect("text");
    assert_eq!(next_close_code(&mut phone.ws).await, 1003);
    // A frame over 64 KB closes with 1009.
    let mut again = open(&phone.relay, &phone.token).await;
    again
        .send(Message::Binary(vec![0u8; MAX_FRAME + 1024].into()))
        .await
        .expect("big");
    assert_eq!(next_close_code(&mut again).await, 1009);
    // A newer phone socket replaces the older with 4000.
    let mut first = open(&phone.relay, &phone.token).await;
    let _ = next(&mut first).await;
    let _second = open(&phone.relay, &phone.token).await;
    assert_eq!(next_close_code(&mut first).await, 4000);
    drop(rig);
}

#[tokio::test]
async fn f_rm_04_4_after_revoke_the_lan_relay_refuses_the_phone() {
    let rig = rig(WebFiles::default()).await;
    let (mut phone, _) = pair(&rig).await;
    let pid = phone.pid.clone();
    rig.svc.revoke(&pid, "revoked").await.expect("revoke");
    let bye = phone.recv().await;
    assert_eq!(bye["t"], "bye", "{bye}");
    assert_eq!(next_close_code(&mut phone.ws).await, 1008);
    let mut back = open(&phone.relay, &phone.token).await;
    assert_eq!(next_close_code(&mut back).await, 1008);
    assert!(!rig.lan.owns(&pid));
    assert!(rig
        .keychain
        .get(&keychain::service("remote"), &format!("lan:{pid}"))
        .expect("keychain")
        .is_none());
}

#[tokio::test]
async fn f_rm_04_4_a_lan_pairing_survives_a_restart_of_the_hub() {
    let rig = rig(WebFiles::default()).await;
    let (phone, _) = pair(&rig).await;
    // A fresh hub (the app restarted) learns the token's hash from the
    // keychain when the phone service reopens the link.
    let fresh = LanNet::new(rig.lan.server().clone(), rig.keychain.clone());
    rig.lan.server().hub().lock().clear();
    assert!(fresh.owns(&phone.pid));
    assert!(fresh.dial(&phone.pid).is_ok());
    let mut ws = open(&phone.relay, &phone.token).await;
    match next(&mut ws).await {
        Message::Text(t) => assert!(t.as_str().contains("\"peer\""), "{t}"),
        other => panic!("unexpected {other:?}"),
    }
}

async fn raw_http(addr: SocketAddr, request: &str) -> String {
    let mut s = TcpStream::connect(addr).await.expect("connect");
    s.write_all(request.as_bytes()).await.expect("write");
    let mut out = Vec::new();
    let _ = tokio::time::timeout(Duration::from_secs(5), s.read_to_end(&mut out)).await;
    String::from_utf8_lossy(&out).to_string()
}

#[tokio::test]
async fn f_rm_04_5_serves_the_phone_web_app_and_nothing_else() {
    let mut files = HashMap::new();
    files.insert("index.html".to_string(), b"<!doctype html>app".to_vec());
    files.insert("assets/app.js".to_string(), b"console.log(1)".to_vec());
    let rig = rig(WebFiles::from_map(files)).await;
    let addr = rig.lan.server().address_for(None).expect("listening");
    let get = |path: &str| format!("GET {path} HTTP/1.1\r\nHost: {addr}\r\n\r\n");
    let page = raw_http(addr, &get("/remote")).await;
    assert!(page.starts_with("HTTP/1.1 200"), "{page}");
    assert!(page.contains("<!doctype html>app"));
    assert!(page.contains(&format!("connect-src 'self' ws://{addr}")));
    assert!(page.contains("frame-ancestors 'none'"));
    let js = raw_http(addr, &get("/remote/assets/app.js")).await;
    assert!(js.contains("text/javascript"), "{js}");
    assert!(raw_http(addr, &get("/remote/../Cargo.toml"))
        .await
        .starts_with("HTTP/1.1 404"));
    assert!(raw_http(addr, &get("/etc/passwd"))
        .await
        .starts_with("HTTP/1.1 404"));
    assert!(raw_http(addr, &get("/")).await.starts_with("HTTP/1.1 302"));
    assert!(raw_http(
        addr,
        &format!("POST /remote HTTP/1.1\r\nHost: {addr}\r\nContent-Length: 0\r\n\r\n")
    )
    .await
    .starts_with("HTTP/1.1 405"));
    // DNS rebinding: a name, not this socket's address, is refused.
    let rebound = raw_http(
        addr,
        "GET /remote HTTP/1.1\r\nHost: evil.example:47800\r\n\r\n",
    )
    .await;
    assert!(rebound.starts_with("HTTP/1.1 421"), "{rebound}");
}

#[tokio::test]
async fn f_rm_04_5_without_the_web_app_the_page_says_to_use_the_app() {
    let rig = rig(WebFiles::default()).await;
    let addr = rig.lan.server().address_for(None).expect("listening");
    let page = raw_http(
        addr,
        &format!("GET /remote/ HTTP/1.1\r\nHost: {addr}\r\n\r\n"),
    )
    .await;
    assert!(page.contains("Vibezeo Remote app"), "{page}");
}

#[tokio::test]
async fn f_rm_04_1_a_peer_outside_the_lan_is_closed_unread() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");
    let client = tokio::spawn(async move {
        raw_http(
            addr,
            &format!("GET /remote HTTP/1.1\r\nHost: {addr}\r\n\r\n"),
        )
        .await
    });
    let (stream, _) = listener.accept().await.expect("accept");
    // As if it came from the internet.
    accept_one(
        LanHub::new(),
        Arc::new(WebFiles::default()),
        stream,
        "8.8.8.8:5555".parse().expect("addr"),
    );
    assert_eq!(client.await.expect("join"), "");
}

#[tokio::test]
async fn f_rm_04_1_turning_lan_mode_off_closes_phones() {
    let rig = rig(WebFiles::default()).await;
    let (mut phone, _) = pair(&rig).await;
    rig.lan.server().stop();
    assert_eq!(next_close_code(&mut phone.ws).await, 1001);
    assert!(!rig.lan.server().is_running());
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(TcpStream::connect(
        phone
            .relay
            .trim_start_matches("ws://")
            .split('/')
            .next()
            .expect("host")
    )
    .await
    .is_err());
}

#[tokio::test]
async fn f_rm_04_2_when_the_desktop_lets_go_of_its_link_the_phone_hears_it() {
    let rig = rig(WebFiles::default()).await;
    let (mut phone, _) = pair(&rig).await;
    // The phone service stops the link (as on revoke or shutdown): the
    // relay tells the phone the desktop went away.
    rig.svc.stop_all_links_for_tests();
    let mut down = false;
    for _ in 0..5 {
        if let Message::Text(t) = next(&mut phone.ws).await {
            if t.as_str().contains("\"up\":false") {
                down = true;
                break;
            }
        }
    }
    assert!(down);
}
