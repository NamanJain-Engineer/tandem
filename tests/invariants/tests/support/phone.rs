//! A phone companion on fakes, for the rule 5 and rule 7 phone cases: the
//! real `PhoneService`, a host that records what reached the app, and a
//! network that counts relay dials.
// Shared by rule_5.rs and rule_7.rs; each uses only part of it.
#![allow(dead_code)]

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use serde_json::{json, Value};
use tandem_app::services::phone::{
    PairingTicket, PhoneAgent, PhoneHost, PhoneNet, PhoneRoutine, PhoneService, PhoneThread,
    PhoneTranscript, RekeyPolicy, RelayIn, RelayLink,
};
use tandem_core::approvals::{ApprovalKind, ApprovalScope, ApprovalView};
use tandem_core::bus::EventBus;
use tandem_core::clock::MockClock;
use tandem_core::ids::ApprovalId;
use tandem_core::keychain::MemoryKeychain;
use tandem_core::remote::messages::PhoneDecision;
use tandem_core::remote::protocol::{
    encode_hello, hello_tag, root_secret, KeyPair, Role, Secret32, Session,
};
use tandem_core::source::SourceRef;
use tandem_core::IpcError;
use tokio::sync::mpsc;

#[derive(Default)]
pub struct Host {
    pub approvals: Mutex<Vec<ApprovalView>>,
    pub resolved: Mutex<Vec<(String, PhoneDecision)>>,
}

#[async_trait]
impl PhoneHost for Host {
    fn desktop_name(&self) -> String {
        "Desk".into()
    }
    async fn agents(&self) -> Vec<PhoneAgent> {
        Vec::new()
    }
    fn approvals(&self) -> Vec<ApprovalView> {
        self.approvals.lock().expect("lock").clone()
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
    fn approval_resolve(&self, id: &str, decision: PhoneDecision) -> Result<(), String> {
        self.resolved
            .lock()
            .expect("lock")
            .push((id.into(), decision));
        Ok(())
    }
    async fn routine_run_now(&self, _: &str) -> Result<(), String> {
        Err("none".into())
    }
    async fn queued_notice(&self, _: &ApprovalView, _: &str) {}
}

#[derive(Default)]
pub struct Net {
    pub dials: Mutex<u32>,
    held: Mutex<Vec<mpsc::UnboundedSender<RelayIn>>>,
}

#[async_trait]
impl PhoneNet for Net {
    async fn create_pairing(&self, pid: &str, _: Option<&str>) -> Result<PairingTicket, IpcError> {
        Ok(PairingTicket {
            pairing_token: "tok".into(),
            relay_url: format!("ws://relay.test/v1/relay/{pid}"),
            web_base: None,
        })
    }
    async fn delete_pairing(&self, _: &str) -> Result<(), String> {
        Ok(())
    }
    async fn dial(&self, _: &str) -> Result<RelayLink, String> {
        *self.dials.lock().expect("lock") += 1;
        let (tx, out) = mpsc::unbounded_channel();
        std::mem::forget(out);
        let (in_tx, rx) = mpsc::unbounded_channel();
        self.held.lock().expect("lock").push(in_tx);
        Ok(RelayLink { tx, rx })
    }
}

pub struct Rig {
    _dir: tempfile::TempDir,
    pub host: Arc<Host>,
    pub net: Arc<Net>,
    pub svc: Arc<PhoneService>,
}

pub fn rig() -> Rig {
    let dir = tempfile::tempdir().expect("tmp");
    let data = tandem_core::DataDir::at(dir.path().to_path_buf()).expect("data");
    let store = tandem_store::Store::open(&data).expect("store");
    let host = Arc::new(Host::default());
    let net = Arc::new(Net::default());
    let svc = PhoneService::new(
        store,
        EventBus::new(),
        Arc::new(MemoryKeychain::new()),
        Arc::new(MockClock::new(chrono::Utc::now())),
        "https://app.test".into(),
        RekeyPolicy::default(),
    );
    svc.bind(host.clone(), net.clone());
    Rig {
        _dir: dir,
        host,
        net,
        svc,
    }
}

pub struct Phone {
    pub pid: String,
    session: Session,
}

impl Phone {
    pub async fn send(&mut self, svc: &PhoneService, msg: Value) -> Vec<Value> {
        let frame = self
            .session
            .seal_next(msg.to_string().as_bytes())
            .expect("seal");
        svc.on_frame(&self.pid, &frame)
            .await
            .into_iter()
            .map(|f| {
                serde_json::from_slice(&self.session.open_next(&f).expect("open")).expect("json")
            })
            .collect()
    }
}

fn key(v: &Value) -> [u8; 32] {
    URL_SAFE_NO_PAD
        .decode(v.as_str().expect("str"))
        .expect("b64")
        .try_into()
        .expect("32")
}

/// Pairs a phone the way the PWA does, from the QR link.
pub async fn pair(rig: &Rig) -> Phone {
    let qr = rig.svc.pair_begin().await.expect("pair");
    let p = qr.link.split("#p=").nth(1).expect("fragment");
    let v: Value = serde_json::from_slice(&URL_SAFE_NO_PAD.decode(p).expect("b64")).expect("json");
    let pid = v["pid"].as_str().expect("pid").to_string();
    let (dpk, s) = (key(&v["dpk"]), Secret32(key(&v["s"])));
    let me = KeyPair::generate();
    let root = root_secret(&s, &pid, &dpk, &me.public, &me.secret, &dpk).expect("root");
    rig.svc
        .on_frame(
            &pid,
            &encode_hello(&me.public, &hello_tag(&s, &pid, &me.public)),
        )
        .await;
    let mut phone = Phone {
        session: Session::new(&pid, Role::Phone, root),
        pid,
    };
    phone
        .send(&rig.svc, json!({"t": "hi", "device": "Phone"}))
        .await;
    phone
}

pub fn approval() -> ApprovalView {
    ApprovalView {
        id: ApprovalId::new(),
        source: SourceRef::Thread {
            thread_id: "th_1".into(),
        },
        kind: ApprovalKind::Command,
        summary: "Run a command".into(),
        detail: "rm -rf build".into(),
        options: vec![ApprovalScope::Once, ApprovalScope::Conversation],
        created_at: 1,
        expires_at: i64::MAX,
    }
}
