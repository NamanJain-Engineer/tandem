use std::sync::Mutex as StdMutex;

use super::*;
use tandem_core::approvals::{ApprovalKind, ApprovalScope};
use tandem_core::clock::MockClock;
use tandem_core::ids::ApprovalId;
use tandem_core::keychain::MemoryKeychain;
use tandem_core::origin::Origin;
use tandem_core::remote::protocol::{encode_hello, hello_tag, Secret32};
use tandem_core::source::SourceRef;

#[derive(Default)]
struct FakeHost {
    approvals: StdMutex<Vec<ApprovalView>>,
    resolved: StdMutex<Vec<(String, PhoneDecision)>>,
    sent: StdMutex<Vec<(String, String)>>,
    typed: StdMutex<Vec<(String, String)>>,
    notices: StdMutex<Vec<String>>,
    agents: StdMutex<Vec<PhoneAgent>>,
    ran: StdMutex<Vec<String>>,
}

#[async_trait]
impl PhoneHost for FakeHost {
    fn desktop_name(&self) -> String {
        "Work laptop".into()
    }
    async fn agents(&self) -> Vec<PhoneAgent> {
        self.agents.lock().expect("lock").clone()
    }
    fn approvals(&self) -> Vec<ApprovalView> {
        self.approvals.lock().expect("lock").clone()
    }
    async fn threads(&self) -> Vec<PhoneThread> {
        vec![PhoneThread {
            id: "th_1".into(),
            title: "Fix login".into(),
            state: ThreadState::Idle,
            updated_at: 1,
        }]
    }
    async fn routines(&self) -> Vec<PhoneRoutine> {
        vec![PhoneRoutine {
            id: "rt_1".into(),
            name: "Nightly triage".into(),
            paused: false,
            running: false,
        }]
    }
    async fn thread_read(
        &self,
        _thread_id: &str,
        _before: Option<i64>,
        limit: u32,
    ) -> Result<PhoneTranscript, String> {
        Ok(PhoneTranscript {
            items: (0..limit)
                .map(|i| PhoneTranscriptItem {
                    role: "assistant".into(),
                    text: "x".repeat(5000),
                    at: i as i64,
                })
                .collect(),
            before: Some(7),
        })
    }
    async fn thread_send(&self, thread_id: &str, text: &str) -> Result<(), String> {
        self.sent
            .lock()
            .expect("lock")
            .push((thread_id.into(), text.into()));
        Ok(())
    }
    async fn pane_type(&self, pane_id: &str, text: &str) -> Result<(), String> {
        if pane_id == "pn_shell" {
            return Err("Only agent panes take input from the phone".into());
        }
        self.typed
            .lock()
            .expect("lock")
            .push((pane_id.into(), text.into()));
        Ok(())
    }
    fn approval_resolve(&self, approval_id: &str, decision: PhoneDecision) -> Result<(), String> {
        let mut pending = self.approvals.lock().expect("lock");
        let before = pending.len();
        pending.retain(|a| a.id.as_str() != approval_id);
        if pending.len() == before {
            return Err("already answered".into());
        }
        self.resolved
            .lock()
            .expect("lock")
            .push((approval_id.into(), decision));
        Ok(())
    }
    async fn routine_run_now(&self, routine_id: &str) -> Result<(), String> {
        self.ran.lock().expect("lock").push(routine_id.into());
        Ok(())
    }
    async fn queued_notice(&self, approval: &ApprovalView, device: &str) {
        self.notices
            .lock()
            .expect("lock")
            .push(format!("{device}: {}", approval.summary));
    }
}

#[derive(Default)]
struct FakeNet {
    created: StdMutex<Vec<String>>,
    deleted: StdMutex<Vec<String>>,
    /// Keeps the fake sockets' senders alive so links stay open.
    held: StdMutex<Vec<mpsc::UnboundedSender<RelayIn>>>,
}

#[async_trait]
impl PhoneNet for FakeNet {
    async fn create_pairing(
        &self,
        pid: &str,
        _label: Option<&str>,
    ) -> Result<PairingTicket, IpcError> {
        self.created.lock().expect("lock").push(pid.into());
        Ok(PairingTicket {
            pairing_token: "tok_test".into(),
            relay_url: format!("ws://relay.test/v1/relay/{pid}"),
        })
    }
    async fn delete_pairing(&self, pid: &str) -> Result<(), String> {
        self.deleted.lock().expect("lock").push(pid.into());
        Ok(())
    }
    async fn dial(&self, _url: &str) -> Result<RelayLink, String> {
        let (tx, _out) = mpsc::unbounded_channel();
        let (in_tx, rx) = mpsc::unbounded_channel();
        self.held.lock().expect("lock").push(in_tx);
        std::mem::forget(_out);
        Ok(RelayLink { tx, rx })
    }
}

struct Rig {
    _dir: tempfile::TempDir,
    store: Store,
    keychain: Arc<MemoryKeychain>,
    clock: MockClock,
    host: Arc<FakeHost>,
    net: Arc<FakeNet>,
    svc: Arc<PhoneService>,
}

fn rig_with(rekey: RekeyPolicy) -> Rig {
    let dir = tempfile::tempdir().expect("tmp");
    let data = tandem_core::DataDir::at(dir.path().to_path_buf()).expect("data");
    let store = Store::open(&data).expect("store");
    let keychain = Arc::new(MemoryKeychain::new());
    let clock = MockClock::new(chrono::Utc::now());
    let host = Arc::new(FakeHost::default());
    let net = Arc::new(FakeNet::default());
    let svc = PhoneService::new(
        store.clone(),
        EventBus::new(),
        keychain.clone(),
        Arc::new(clock.clone()),
        "https://app.example.test".into(),
        rekey,
    );
    svc.bind(host.clone(), net.clone());
    Rig {
        _dir: dir,
        store,
        keychain,
        clock,
        host,
        net,
        svc,
    }
}

fn rig() -> Rig {
    rig_with(RekeyPolicy::default())
}

fn main_window() -> LocalReview {
    LocalReview::check(&Origin::main_window()).expect("main window")
}

struct Phone {
    pid: String,
    session: Session,
}

fn link_payload(link: &str) -> Value {
    let p = link.split("#p=").nth(1).expect("fragment");
    serde_json::from_slice(&URL_SAFE_NO_PAD.decode(p).expect("b64")).expect("json")
}

/// Scans the QR link the way the PWA does: returns the phone's session and its hello.
fn scan(link: &str) -> (Phone, Vec<u8>) {
    let v = link_payload(link);
    let pid = v["pid"].as_str().expect("pid").to_string();
    let dpk = key32(v["dpk"].as_str().expect("dpk")).expect("dpk");
    let s = Secret32(key32(v["s"].as_str().expect("s")).expect("s"));
    let me = KeyPair::generate();
    let root = root_secret(&s, &pid, &dpk, &me.public, &me.secret, &dpk).expect("root");
    let hello = encode_hello(&me.public, &hello_tag(&s, &pid, &me.public));
    (
        Phone {
            session: Session::new(&pid, Role::Phone, root),
            pid,
        },
        hello,
    )
}

impl Phone {
    fn frame(&mut self, msg: Value) -> Vec<u8> {
        self.session
            .seal_next(msg.to_string().as_bytes())
            .expect("seal")
    }

    fn read(&mut self, frames: Vec<Vec<u8>>) -> Vec<Value> {
        frames
            .into_iter()
            .map(|f| {
                serde_json::from_slice(&self.session.open_next(&f).expect("opens")).expect("json")
            })
            .collect()
    }
}

async fn paired_full(r: &Rig) -> (Phone, Vec<Value>) {
    let qr = r.svc.pair_begin().await.expect("pair");
    let (mut phone, hello) = scan(&qr.link);
    assert!(r.svc.on_frame(&phone.pid, &hello).await.is_empty());
    let hi = phone.frame(json!({"t": "hi", "device": "iPhone · Safari"}));
    let replies = phone.read(r.svc.on_frame(&phone.pid, &hi).await);
    assert_eq!(replies[0]["t"], "welcome");
    (phone, replies)
}

async fn paired(r: &Rig) -> Phone {
    paired_full(r).await.0
}

async fn cmd(r: &Rig, phone: &mut Phone, id: u64, cmd: &str, args: Value) -> Value {
    let f = phone.frame(json!({"t": "cmd", "id": id, "cmd": cmd, "args": args}));
    let mut replies = phone.read(r.svc.on_frame(&phone.pid, &f).await);
    replies.remove(0)
}

fn approval(id: &str) -> ApprovalView {
    ApprovalView {
        id: id
            .parse::<ApprovalId>()
            .unwrap_or_else(|_| ApprovalId::new()),
        source: SourceRef::Pane {
            pane_id: "pn_1".into(),
        },
        kind: ApprovalKind::Command,
        summary: "Run pnpm test".into(),
        detail: "pnpm test".into(),
        options: vec![ApprovalScope::Once],
        created_at: 1,
        expires_at: i64::MAX,
    }
}

async fn grant(r: &Rig, phone: &Phone, grants: PhoneGrants) {
    r.svc
        .set_grants(&main_window(), &phone.pid, grants)
        .await
        .expect("grants");
}

#[tokio::test]
async fn f_rm_03_the_qr_carries_the_desktop_key_relay_and_token_and_the_secret_stays_off_disk() {
    let r = rig();
    let qr = r.svc.pair_begin().await.expect("pair");
    let v = link_payload(&qr.link);
    assert!(qr.link.starts_with("https://app.example.test/remote#p="));
    assert_eq!(v["v"], 1);
    assert_eq!(v["pid"], qr.pairing_id.as_str());
    assert_eq!(
        v["relay"],
        format!("ws://relay.test/v1/relay/{}", qr.pairing_id)
    );
    assert_eq!(v["ptok"], "tok_test");
    assert!(qr.qr.size >= 21 && qr.qr.rows.len() == qr.qr.size as usize);
    assert_eq!(qr.expires_at, r.clock.now_ms() + QR_TTL_MS);
    // The desktop's identity is in the keychain; the one-time secret isn't
    // anywhere but memory, and no pairing exists until a phone answers.
    assert!(r
        .keychain
        .get(&remote_service(), IDENTITY)
        .expect("kc")
        .is_some());
    assert!(r.svc.is_pending(qr.pairing_id.as_str()));
    assert!(r.svc.list().await.expect("list").is_empty());
    assert_eq!(
        *r.net.created.lock().expect("lock"),
        vec![qr.pairing_id.to_string()]
    );
}

#[tokio::test]
async fn f_rm_03_a_hello_without_the_qr_secret_pairs_nothing() {
    let r = rig();
    let qr = r.svc.pair_begin().await.expect("pair");
    let pid = qr.pairing_id.to_string();
    let forger = KeyPair::generate();
    let wrong = Secret32::random();
    let hello = encode_hello(&forger.public, &hello_tag(&wrong, &pid, &forger.public));
    r.svc.on_frame(&pid, &hello).await;
    assert!(r.svc.list().await.expect("list").is_empty());
    assert!(r.svc.is_pending(&pid), "the real phone can still pair");
    let (_phone, good) = scan(&qr.link);
    r.svc.on_frame(&pid, &good).await;
    assert!(
        !r.svc.is_pending(&pid),
        "the secret is discarded after pairing"
    );
    let list = r.svc.list().await.expect("list");
    assert_eq!(list.len(), 1);
    assert_eq!(
        list[0].grants,
        PhoneGrants::default(),
        "everything but View starts off"
    );
    // A second hello (say, replayed by the relay) changes nothing.
    r.svc.on_frame(&pid, &good).await;
    assert_eq!(r.svc.list().await.expect("list").len(), 1);
}

#[tokio::test]
async fn f_rm_03_an_expired_code_cannot_pair_and_is_dropped_from_the_api() {
    let r = rig();
    let qr = r.svc.pair_begin().await.expect("pair");
    let pid = qr.pairing_id.to_string();
    r.clock
        .advance(std::time::Duration::from_millis(QR_TTL_MS as u64 + 1));
    let (_phone, hello) = scan(&qr.link);
    r.svc.on_frame(&pid, &hello).await;
    assert!(r.svc.list().await.expect("list").is_empty());
    r.svc.expire_pending(&pid).await;
    assert!(!r.svc.is_pending(&pid));
    assert_eq!(*r.net.deleted.lock().expect("lock"), vec![pid]);
}

#[tokio::test]
async fn f_rm_03_hi_names_the_device_and_gets_a_welcome() {
    let r = rig();
    let mut phone = paired(&r).await;
    let list = r.svc.list().await.expect("list");
    assert_eq!(list[0].device_name, "iPhone · Safari");
    let hi = phone.frame(json!({"t": "hi", "device": "Evil\u{1b}[31m name"}));
    let replies = phone.read(r.svc.on_frame(&phone.pid, &hi).await);
    assert_eq!(replies[0]["desktop"], "Work laptop");
    assert_eq!(
        r.svc.list().await.expect("list")[0].device_name,
        "Evil[31m name",
        "control characters are dropped"
    );
}

#[tokio::test]
async fn f_rm_03_2_a_replayed_command_frame_is_rejected_and_runs_once() {
    let r = rig();
    let mut phone = paired(&r).await;
    grant(
        &r,
        &phone,
        PhoneGrants {
            chat: true,
            ..Default::default()
        },
    )
    .await;
    let f = phone.frame(json!({"t": "cmd", "id": 1, "cmd": "thread.send", "args": {"threadId": "th_1", "text": "ship it"}}));
    let first = phone.read(r.svc.on_frame(&phone.pid, &f).await);
    assert_eq!(first[0]["ok"], true);
    let again = phone.read(r.svc.on_frame(&phone.pid, &f).await);
    assert_eq!(again[0], json!({"t": "reject", "reason": "replay"}));
    assert_eq!(r.host.sent.lock().expect("lock").len(), 1);
}

#[tokio::test]
async fn f_rm_03_each_command_needs_its_grant_and_grants_change_only_from_the_main_window() {
    let r = rig();
    let mut phone = paired(&r).await;
    let snap = cmd(&r, &mut phone, 1, "view.snapshot", json!({})).await;
    assert_eq!(snap["ok"], true, "View comes with pairing");
    assert_eq!(snap["data"]["desktop"], "Work laptop");
    let send = cmd(
        &r,
        &mut phone,
        2,
        "thread.send",
        json!({"threadId": "th_1", "text": "hi"}),
    )
    .await;
    assert_eq!(send["error"]["code"], "forbidden");
    let typed = cmd(
        &r,
        &mut phone,
        3,
        "pane.type",
        json!({"paneId": "pn_1", "text": "go"}),
    )
    .await;
    assert_eq!(typed["error"]["code"], "forbidden");
    let unknown = cmd(
        &r,
        &mut phone,
        4,
        "lane.remove",
        json!({"laneId": "ws_1", "force": true}),
    )
    .await;
    assert_eq!(unknown["error"]["code"], "unknown_command");
    assert!(r.host.sent.lock().expect("lock").is_empty());
    assert!(r.host.typed.lock().expect("lock").is_empty());

    grant(
        &r,
        &phone,
        PhoneGrants {
            chat: true,
            terminal: true,
            ..Default::default()
        },
    )
    .await;
    let typed = cmd(
        &r,
        &mut phone,
        5,
        "pane.type",
        json!({"paneId": "pn_1", "text": "go on"}),
    )
    .await;
    assert_eq!(typed["ok"], true);
    let shell = cmd(
        &r,
        &mut phone,
        6,
        "pane.type",
        json!({"paneId": "pn_shell", "text": "ls"}),
    )
    .await;
    assert_eq!(shell["ok"], false, "never into plain shells");
    assert_eq!(
        *r.host.typed.lock().expect("lock"),
        vec![("pn_1".to_string(), "go on".to_string())]
    );
    let routine = cmd(
        &r,
        &mut phone,
        7,
        "routine.run_now",
        json!({"routineId": "rt_1"}),
    )
    .await;
    assert_eq!(routine["error"]["code"], "forbidden");
    assert!(r.host.ran.lock().expect("lock").is_empty());
}

#[tokio::test]
async fn f_rm_03_routines_are_listed_with_view_and_run_now_needs_the_routines_grant() {
    let r = rig();
    let mut phone = paired(&r).await;
    let snap = cmd(&r, &mut phone, 1, "view.snapshot", json!({})).await;
    assert_eq!(snap["data"]["routines"][0]["name"], "Nightly triage");
    let off = cmd(
        &r,
        &mut phone,
        2,
        "routine.run_now",
        json!({"routineId": "rt_1"}),
    )
    .await;
    assert_eq!(off["error"]["code"], "forbidden");
    grant(
        &r,
        &phone,
        PhoneGrants {
            routines: true,
            ..Default::default()
        },
    )
    .await;
    let ran = cmd(
        &r,
        &mut phone,
        3,
        "routine.run_now",
        json!({"routineId": "rt_1"}),
    )
    .await;
    assert_eq!(ran["ok"], true);
    assert_eq!(*r.host.ran.lock().expect("lock"), vec!["rt_1".to_string()]);
}

#[tokio::test]
async fn f_rm_03_3_a_phone_allow_without_arming_is_queued_not_applied() {
    let r = rig();
    let mut phone = paired(&r).await;
    let ap = approval("ap_01JTESTAPPROVAL00000000000");
    let id = ap.id.to_string();
    r.host.approvals.lock().expect("lock").push(ap);
    let off = cmd(
        &r,
        &mut phone,
        1,
        "approval.decide",
        json!({"approvalId": id, "decision": "allow"}),
    )
    .await;
    assert_eq!(
        off["error"]["code"], "forbidden",
        "the Approvals grant starts off"
    );
    grant(
        &r,
        &phone,
        PhoneGrants {
            approvals: true,
            ..Default::default()
        },
    )
    .await;

    let allow = cmd(
        &r,
        &mut phone,
        2,
        "approval.decide",
        json!({"approvalId": id, "decision": "allow"}),
    )
    .await;
    assert_eq!(allow["data"], json!({"state": "queued"}));
    assert!(
        r.host.resolved.lock().expect("lock").is_empty(),
        "nothing applied"
    );
    assert_eq!(
        *r.host.notices.lock().expect("lock"),
        vec!["iPhone · Safari: Run pnpm test".to_string()]
    );
    let queue = r.svc.queue_list().await.expect("queue");
    assert_eq!(queue.len(), 1);
    assert_eq!(queue[0].approval_id, id);
    let snap = cmd(&r, &mut phone, 3, "view.snapshot", json!({})).await;
    assert_eq!(snap["data"]["approvals"][0]["queued"], true);

    // The desktop confirms it.
    r.svc
        .queue_decide(&main_window(), queue[0].id.as_str(), true)
        .await
        .expect("decide");
    assert_eq!(
        *r.host.resolved.lock().expect("lock"),
        vec![(id, PhoneDecision::Allow)]
    );
    assert!(r.svc.queue_list().await.expect("queue").is_empty());
}

#[tokio::test]
async fn f_rm_03_3_deny_always_applies_and_arming_lets_allow_through_for_8_hours() {
    let r = rig();
    let mut phone = paired(&r).await;
    grant(
        &r,
        &phone,
        PhoneGrants {
            approvals: true,
            ..Default::default()
        },
    )
    .await;
    let a = approval("ap_01JTESTAPPROVALAAAAAAAAAAA");
    let b = approval("ap_01JTESTAPPROVALBBBBBBBBBBB");
    let c = approval("ap_01JTESTAPPROVALCCCCCCCCCCC");
    let (ia, ib, ic) = (a.id.to_string(), b.id.to_string(), c.id.to_string());
    r.host.approvals.lock().expect("lock").extend([a, b, c]);

    let deny = cmd(
        &r,
        &mut phone,
        1,
        "approval.decide",
        json!({"approvalId": ia, "decision": "deny"}),
    )
    .await;
    assert_eq!(deny["data"], json!({"state": "applied"}));

    let armed = r.svc.arm(&main_window(), ARM_HOURS).expect("arm");
    assert_eq!(armed.armed_until, Some(r.clock.now_ms() + 8 * 3_600_000));
    let allow = cmd(
        &r,
        &mut phone,
        2,
        "approval.decide",
        json!({"approvalId": ib, "decision": "allow"}),
    )
    .await;
    assert_eq!(allow["data"], json!({"state": "applied"}));

    r.clock.advance(std::time::Duration::from_secs(8 * 3600));
    let late = cmd(
        &r,
        &mut phone,
        3,
        "approval.decide",
        json!({"approvalId": ic, "decision": "allow"}),
    )
    .await;
    assert_eq!(
        late["data"],
        json!({"state": "queued"}),
        "arming lapses after 8 hours"
    );
    assert_eq!(
        *r.host.resolved.lock().expect("lock"),
        vec![(ia, PhoneDecision::Deny), (ib, PhoneDecision::Allow)]
    );
    assert!(r.svc.arm(&main_window(), 3).is_err(), "8 hours or off");
}

#[tokio::test]
async fn f_rm_03_3_a_queued_allow_for_an_approval_answered_elsewhere_is_dropped() {
    let r = rig();
    let mut phone = paired(&r).await;
    grant(
        &r,
        &phone,
        PhoneGrants {
            approvals: true,
            ..Default::default()
        },
    )
    .await;
    let a = approval("ap_01JTESTAPPROVALDDDDDDDDDDD");
    let id = a.id.to_string();
    r.host.approvals.lock().expect("lock").push(a);
    cmd(
        &r,
        &mut phone,
        1,
        "approval.decide",
        json!({"approvalId": id, "decision": "allow"}),
    )
    .await;
    let item = r.svc.queue_list().await.expect("queue")[0].id.clone();
    r.host.approvals.lock().expect("lock").clear();
    assert!(r.svc.queue_list().await.expect("queue").is_empty());
    assert!(r
        .svc
        .queue_decide(&main_window(), item.as_str(), true)
        .await
        .is_err());
    let gone = cmd(
        &r,
        &mut phone,
        2,
        "approval.decide",
        json!({"approvalId": id, "decision": "deny"}),
    )
    .await;
    assert_eq!(gone["error"]["code"], "not_found");
}

#[tokio::test]
async fn f_rm_03_4_after_revoke_the_next_phone_command_is_rejected() {
    let r = rig();
    let mut phone = paired(&r).await;
    grant(
        &r,
        &phone,
        PhoneGrants {
            chat: true,
            ..Default::default()
        },
    )
    .await;
    r.svc.revoke(&phone.pid, "revoked").await.expect("revoke");
    assert!(r.svc.list().await.expect("list").is_empty());
    assert!(r
        .keychain
        .get(&remote_service(), &pairing_account(&phone.pid))
        .expect("kc")
        .is_none());
    assert_eq!(
        *r.net.deleted.lock().expect("lock"),
        vec![phone.pid.clone()]
    );
    let f = phone.frame(json!({"t": "cmd", "id": 9, "cmd": "thread.send", "args": {"threadId": "th_1", "text": "after"}}));
    assert!(
        r.svc.on_frame(&phone.pid, &f).await.is_empty(),
        "no answer, nothing run"
    );
    assert!(r.host.sent.lock().expect("lock").is_empty());
    let row = repo::get(&r.store, &phone.pid)
        .await
        .expect("get")
        .expect("row");
    assert!(row.revoked_at.is_some());
}

#[tokio::test]
async fn f_rm_03_the_phone_unpairing_itself_revokes_the_pairing() {
    let r = rig();
    let mut phone = paired(&r).await;
    let bye = phone.frame(json!({"t": "bye"}));
    assert!(r.svc.on_frame(&phone.pid, &bye).await.is_empty());
    assert!(r.svc.list().await.expect("list").is_empty());
    assert_eq!(
        *r.net.deleted.lock().expect("lock"),
        vec![phone.pid.clone()]
    );
}

#[tokio::test]
async fn f_rm_03_signing_out_revokes_every_pairing() {
    let r = rig();
    let one = paired(&r).await;
    let two = paired(&r).await;
    assert_eq!(r.svc.list().await.expect("list").len(), 2);
    r.svc
        .revoke_all("signed_out", false)
        .await
        .expect("revoke all");
    assert!(r.svc.list().await.expect("list").is_empty());
    let deleted = r.net.deleted.lock().expect("lock").clone();
    assert!(deleted.contains(&one.pid) && deleted.contains(&two.pid));
    assert!(r
        .keychain
        .get(&remote_service(), IDENTITY)
        .expect("kc")
        .is_some());
    r.svc.revoke_all("reset", true).await.expect("reset");
    assert!(
        r.keychain
            .get(&remote_service(), IDENTITY)
            .expect("kc")
            .is_none(),
        "reset replaces the identity"
    );
}

#[tokio::test]
async fn f_rm_03_rekeying_moves_to_a_new_epoch_and_counters_survive_a_restart() {
    let r = rig_with(RekeyPolicy {
        interval_ms: 0,
        frames: u64::MAX,
    });
    let (mut phone, welcome) = paired_full(&r).await;
    // The welcome came with a rekey; a command meanwhile still works.
    let rekey = welcome
        .iter()
        .find(|m| m["t"] == "rekey")
        .expect("rekey")
        .clone();
    let f = phone.frame(json!({"t": "cmd", "id": 1, "cmd": "view.snapshot", "args": {}}));
    let replies = phone.read(r.svc.on_frame(&phone.pid, &f).await);
    assert_eq!(replies.len(), 1, "one rekey at a time");
    let eph = key32(rekey["eph"].as_str().expect("eph")).expect("eph");
    let mine = phone.session.accept_rekey(&eph).expect("accept");
    let ack = phone.frame(json!({"t": "rekey_ack", "eph": b64(&mine)}));
    let replies = phone.read(r.svc.on_frame(&phone.pid, &ack).await);
    assert_eq!(replies[0]["t"], "changed");
    assert_eq!(
        phone.session.current.epoch, 1,
        "the desktop's next frame moved the phone over"
    );
    let replayed = f.clone();

    // A new service on the same store and keychain carries on.
    let svc2 = PhoneService::new(
        r.store.clone(),
        EventBus::new(),
        r.keychain.clone(),
        Arc::new(r.clock.clone()),
        "https://app.example.test".into(),
        RekeyPolicy::default(),
    );
    svc2.bind(r.host.clone(), r.net.clone());
    let f = phone.frame(json!({"t": "cmd", "id": 2, "cmd": "view.snapshot", "args": {}}));
    let replies = phone.read(svc2.on_frame(&phone.pid, &f).await);
    assert_eq!(replies[0]["ok"], true);
    let again = phone.read(svc2.on_frame(&phone.pid, &replayed).await);
    assert_eq!(
        again[0]["reason"], "replay",
        "counters survive rekey and restart"
    );
    let row = repo::get(&r.store, &phone.pid)
        .await
        .expect("get")
        .expect("row");
    assert_eq!(row.epoch, 1);
    assert!(row.rx_counter >= 4);
}

#[tokio::test]
async fn f_rm_03_snapshots_and_transcript_pages_fit_in_one_frame() {
    let r = rig();
    let mut phone = paired(&r).await;
    *r.host.agents.lock().expect("lock") = (0..200)
        .map(|i| PhoneAgent {
            key: format!("pane:pn_{i}"),
            title: "t".repeat(500),
            status: AgentStatus::Working,
            engine: Some("Claude Code".into()),
            workspace: Some("w".repeat(500)),
            pane_id: Some(format!("pn_{i}")),
            thread_id: None,
            preview: Some("p".repeat(2000)),
            since: 0,
        })
        .collect();
    let snap = cmd(&r, &mut phone, 1, "view.snapshot", json!({})).await;
    assert_eq!(snap["ok"], true);
    assert!(snap["data"]["agents"].as_array().expect("agents").len() <= MAX_AGENTS);
    let page = cmd(
        &r,
        &mut phone,
        2,
        "thread.read",
        json!({"threadId": "th_1", "limit": 200}),
    )
    .await;
    let items = page["data"]["items"].as_array().expect("items");
    assert_eq!(items.len(), MAX_TRANSCRIPT_ITEMS as usize);
    assert!(items[0]["text"].as_str().expect("text").chars().count() <= 1000);
    assert_eq!(page["data"]["before"], 7);
}
