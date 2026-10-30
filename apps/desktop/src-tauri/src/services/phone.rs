//! The phone companion's desktop side (F-RM-03, ARCHITECTURE §13.3): QR
//! pairing, one relay link per pairing, sealed commands with grants, the
//! approval step-up queue, rekeying and revocation.
//!
//! The service sits behind two traits: [`PhoneHost`] (what the phone can
//! see and do in the app) and [`PhoneNet`] (the API's pairing routes and the
//! relay socket), so every rule is unit-tested with fakes. Commands from the
//! phone run with `Origin::Phone`: none of them is a local-review action,
//! and the step-up controls (arming, the queue, grants) need a
//! [`LocalReview`] (F-TL-06 rule 5).

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use async_trait::async_trait;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::sync::{mpsc, watch};
use ts_rs::TS;

use tandem_core::approvals::ApprovalView;

use crate::services::notifications::PANE_APPROVAL_BODY;
use tandem_core::bus::{BusEvent, EventBus, Topic};
use tandem_core::clock::Clock;
use tandem_core::ids::{PairingId, PhoneQueueId};
use tandem_core::keychain::{self, Keychain};
use tandem_core::origin::LocalReview;
use tandem_core::remote::messages::{
    approval_route, ApprovalRoute, CommandError, FromPhone, PhoneCommand, PhoneDecision,
    PhoneGrants, PushPlatform, ResError, ToPhone, ARM_HOURS,
};
use tandem_core::remote::protocol::{
    decode_hello, frame_kind, root_secret, verify_hello, KeyPair, ProtocolError, Role, Secret32,
    Session, KIND_HELLO, KIND_SEALED, MAX_PLAINTEXT,
};
use tandem_core::remote::push::{seal_push, PushAlert};
use tandem_core::status::AgentStatus;
use tandem_core::{ErrorCode, IpcError};
use tandem_store::repo::pairings as repo;
use tandem_store::repo::threads::ThreadState;
use tandem_store::Store;

/// How long a QR code (and its one-time secret) is valid.
pub const QR_TTL_MS: i64 = 10 * 60_000;
const IDENTITY: &str = "identity";
const MAX_AGENTS: usize = 50;
const MAX_APPROVALS: usize = 20;
const MAX_THREADS: usize = 30;
const MAX_ROUTINES: usize = 30;
/// Transcript pages are at most this many items of at most this many
/// characters, so a page always fits in one frame.
pub const MAX_TRANSCRIPT_ITEMS: u32 = 40;
const MAX_ITEM_CHARS: usize = 1000;
const MAX_DEVICE_NAME: usize = 60;

fn pairing_account(pid: &str) -> String {
    format!("pairing:{pid}")
}

fn push_account(pid: &str) -> String {
    format!("push:{pid}")
}

fn remote_service() -> String {
    keychain::service("remote")
}

// ── what the UI and the phone see ────────────────────────────────────────

/// A QR module grid: one string per row, `1` dark and `0` light.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct QrMatrix {
    pub size: u32,
    pub rows: Vec<String>,
}

/// `phone_pair_begin`: what Settings › Remote shows while a phone pairs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PairingQr {
    pub pairing_id: PairingId,
    /// The link the code encodes, for a phone that can't scan.
    pub link: String,
    pub qr: QrMatrix,
    pub expires_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Pairing {
    pub id: PairingId,
    pub device_name: String,
    pub grants: PhoneGrants,
    pub created_at: i64,
    pub last_seen: Option<i64>,
    /// The phone's socket is open now.
    pub online: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct QueuedPhoneAction {
    pub id: PhoneQueueId,
    pub pairing_id: PairingId,
    pub device_name: String,
    pub approval_id: String,
    pub summary: String,
    pub created_at: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PhoneArmState {
    pub armed_until: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PhoneAgent {
    /// `pane:<id>` or `thread:<id>`.
    pub key: String,
    pub title: String,
    pub status: AgentStatus,
    pub engine: Option<String>,
    pub workspace: Option<String>,
    /// Set for agent panes, which `pane.type` can type into.
    pub pane_id: Option<String>,
    pub thread_id: Option<String>,
    /// The latest activity line, redacted.
    pub preview: Option<String>,
    pub since: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PhoneApproval {
    pub id: String,
    pub summary: String,
    pub detail: String,
    /// Where it comes from, for people ("Pane · auth tests").
    pub source: String,
    /// The phone allowed it and it waits for the desktop to confirm.
    pub queued: bool,
    /// A pane's `vibezeo run`: answered on the computer only, where the
    /// whole line shows. The phone gets the fixed body and no Allow.
    pub desk_only: bool,
    pub created_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PhoneThread {
    pub id: String,
    pub title: String,
    pub state: ThreadState,
    pub updated_at: i64,
    /// A chat with a teammate rather than a thread (F-RM-03 Chat control).
    #[serde(default)]
    pub teammate: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PhoneRoutine {
    pub id: String,
    pub name: String,
    pub paused: bool,
    /// A run is in progress.
    pub running: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PhoneTranscriptItem {
    /// `user`, `assistant` or `tool`.
    pub role: String,
    pub text: String,
    pub at: i64,
}

/// F-CN-09: the review item the phone shows, read only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PhoneReviewItem {
    pub key: String,
    pub title: String,
    pub engine: Option<String>,
    pub workspace: String,
    pub branch: Option<String>,
    pub finished_at: i64,
    pub closing: Option<String>,
    pub files: u32,
    pub additions: u32,
    pub deletions: u32,
    pub risks: Vec<crate::services::review_queue::ReviewRisk>,
    pub check: Option<crate::services::review_queue::ReviewCheck>,
    /// Approved once and changed since.
    pub changed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PhoneDiffFile {
    pub path: String,
    /// `added`, `modified`, `deleted`, `renamed`…
    pub change: String,
    pub additions: u32,
    pub deletions: u32,
    pub patch: String,
    /// Cut here (or by git) before it reached the phone.
    pub truncated: bool,
    /// Not sent at all: a `.env` file's values stay on the computer.
    pub withheld: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PhoneReview {
    pub item: Option<PhoneReviewItem>,
    pub files: Vec<PhoneDiffFile>,
    /// The item's place among those to review (1-based; 0: not among them).
    pub position: u32,
    pub total: u32,
    /// Files left out so the reply fits (they're on the computer).
    pub omitted: u32,
}

/// Each file's patch on the phone (F-CN-09).
pub const MAX_DIFF_FILE_CHARS: usize = 20_000;
/// What a review reply may serialize to: one sealed frame, with room for
/// the `res` envelope around it (D-2424).
pub const REVIEW_BUDGET: usize = MAX_PLAINTEXT - 1_024;

fn review_bytes(r: &PhoneReview) -> usize {
    serde_json::to_string(r)
        .map(|s| s.len())
        .unwrap_or(usize::MAX)
}

/// Redacts a review's diff for the phone, as written and past what's
/// removable (D-3171), withholds `.env` files, and cuts patches (the longest
/// first, then whole files from the end) until the reply fits in one sealed
/// frame.
pub fn review_for_phone(mut r: PhoneReview) -> PhoneReview {
    use tandem_core::untrusted::redacted;
    for f in &mut r.files {
        if crate::services::review_queue::is_env(&f.path) {
            f.patch.clear();
            f.withheld = true;
            continue;
        }
        let text = redacted(&f.patch);
        if text.chars().count() > MAX_DIFF_FILE_CHARS {
            f.patch = text.chars().take(MAX_DIFF_FILE_CHARS).collect();
            f.truncated = true;
        } else {
            f.patch = text;
        }
    }
    if let Some(item) = &mut r.item {
        // The work's title is the agent's own (D-3173).
        item.title = redacted(&item.title);
        item.closing = item
            .closing
            .take()
            .map(|c| clip(&redacted(&c), MAX_ITEM_CHARS));
    }
    loop {
        let bytes = review_bytes(&r);
        if bytes <= REVIEW_BUDGET {
            break;
        }
        let longest = r
            .files
            .iter_mut()
            .max_by_key(|f| f.patch.len())
            .filter(|f| f.patch.len() > 512);
        match longest {
            Some(f) => {
                let keep = f.patch.chars().count() / 2;
                f.patch = f.patch.chars().take(keep).collect();
                f.truncated = true;
            }
            // Patches are all small: whole files go, from the end, as many
            // as the overshoot needs at their average size.
            None => {
                if r.files.is_empty() {
                    break;
                }
                let each = (bytes / r.files.len()).max(1);
                let drop = ((bytes - REVIEW_BUDGET) / each + 1).min(r.files.len());
                r.files.truncate(r.files.len() - drop);
                r.omitted += drop as u32;
            }
        }
    }
    r
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PhoneTranscript {
    pub items: Vec<PhoneTranscriptItem>,
    /// Pass back as `before` for the page before this one.
    pub before: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PhoneSnapshot {
    pub desktop: String,
    pub grants: PhoneGrants,
    pub armed_until: Option<i64>,
    pub agents: Vec<PhoneAgent>,
    pub approvals: Vec<PhoneApproval>,
    pub threads: Vec<PhoneThread>,
    pub routines: Vec<PhoneRoutine>,
    /// F-CN-09: changes whenever the review queue does, so the Review tab
    /// asks again only then.
    #[ts(type = "number")]
    pub review_rev: u64,
}

// ── the seams ────────────────────────────────────────────────────────────

/// What the phone can see and do in the app. Every call comes from the
/// phone (`Origin::Phone`).
#[async_trait]
pub trait PhoneHost: Send + Sync {
    fn desktop_name(&self) -> String;
    async fn agents(&self) -> Vec<PhoneAgent>;
    fn approvals(&self) -> Vec<ApprovalView>;
    async fn threads(&self) -> Vec<PhoneThread>;
    async fn routines(&self) -> Vec<PhoneRoutine>;
    async fn thread_read(
        &self,
        thread_id: &str,
        before: Option<i64>,
        limit: u32,
    ) -> Result<PhoneTranscript, String>;
    async fn thread_send(&self, thread_id: &str, text: &str) -> Result<(), String>;
    /// Types into an agent pane; plain shells refuse.
    async fn pane_type(&self, pane_id: &str, text: &str) -> Result<(), String>;
    /// Answers an approval as decided on the phone (once, never wider).
    fn approval_resolve(&self, approval_id: &str, decision: PhoneDecision) -> Result<(), String>;
    async fn routine_run_now(&self, routine_id: &str) -> Result<(), String>;
    /// Tells the person at the desktop that the phone's allow waits for them.
    async fn queued_notice(&self, approval: &ApprovalView, device: &str);
    /// The review item Conductor is on, else the first to review, and its
    /// diff (F-CN-09). Read only.
    async fn review_current(&self) -> Result<PhoneReview, String> {
        Err("The review queue isn't available here.".into())
    }
    /// The review queue's revision (F-CN-09); 0 without one.
    fn review_rev(&self) -> u64 {
        0
    }
}

pub use crate::services::api::PairingTicket;

/// What a relay socket delivers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RelayIn {
    Frame(Vec<u8>),
    /// The relay's notice that the phone's socket opened or closed.
    Peer(bool),
    Closed(Option<u16>),
}

/// An open relay socket: frames to send, and what arrives.
pub struct RelayLink {
    pub tx: mpsc::UnboundedSender<Vec<u8>>,
    pub rx: mpsc::UnboundedReceiver<RelayIn>,
}

#[async_trait]
pub trait PhoneNet: Send + Sync {
    async fn create_pairing(
        &self,
        pairing_id: &str,
        label: Option<&str>,
    ) -> Result<PairingTicket, IpcError>;
    async fn delete_pairing(&self, pairing_id: &str) -> Result<(), String>;
    async fn dial(&self, relay_url: &str) -> Result<RelayLink, String>;
    /// A pairing the desktop relays itself on the local network (F-RM-04),
    /// on `address` when that's one it listens on.
    async fn create_lan_pairing(
        &self,
        _pairing_id: &str,
        _address: Option<&str>,
    ) -> Result<PairingTicket, IpcError> {
        Err(IpcError::new(
            ErrorCode::Unavailable,
            "Pairing over the local network isn't available",
        ))
    }
    /// A pairing revoked on this side only (a dropped session): forget any
    /// local relay state for it. The hosted relay learns from the API.
    fn forget_local(&self, _pairing_id: &str) {}
    /// Forwards a sealed alert to the phone's push service (F-RM-06): only
    /// through the hosted API, which can't read it. LAN mode has no push.
    async fn push(
        &self,
        _pairing_id: &str,
        _platform: PushPlatform,
        _token: &str,
        _payload: &str,
    ) -> Result<PushOutcome, String> {
        Err("push isn't available".into())
    }
}

/// What the push service said about a phone's token.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PushOutcome {
    Sent,
    /// The token is gone (app removed, push turned off): forget it.
    Unregistered,
}

/// The keychain entry `NS.remote / push:<id>`: where and how to reach a
/// phone that turned push on.
#[derive(Serialize, Deserialize)]
struct StoredPush {
    platform: PushPlatform,
    token: String,
    key: Secret32,
    seq: u64,
}

/// Which relay a new pairing uses.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum PairVia {
    /// The hosted relay (F-BE-11; comes with Vibezeo accounts).
    #[default]
    Relay,
    /// The desktop itself, on the local network (F-RM-04).
    Lan { address: Option<String> },
}

/// When the desktop starts a rekey.
#[derive(Debug, Clone, Copy)]
pub struct RekeyPolicy {
    pub interval_ms: i64,
    pub frames: u64,
}

impl Default for RekeyPolicy {
    fn default() -> Self {
        Self {
            interval_ms: 3_600_000,
            frames: 1 << 20,
        }
    }
}

// ── state ────────────────────────────────────────────────────────────────

/// The keychain entry `NS.remote / pairing:<id>`.
#[derive(Serialize, Deserialize)]
struct StoredPairing {
    phone_pk: String,
    relay_url: String,
    session: Session,
}

struct Pending {
    s: Secret32,
    relay_url: String,
    expires_at: i64,
}

struct Live {
    session: Session,
    relay_url: String,
    phone_pk: String,
    grants: PhoneGrants,
    device: String,
    /// The desktop's ephemeral secret while a rekey waits for the phone.
    rekey: Option<Secret32>,
    rekeyed_at: i64,
    frames: u64,
}

struct Link {
    out: Option<mpsc::UnboundedSender<Vec<u8>>>,
    online: bool,
    stop: watch::Sender<bool>,
}

#[derive(Default)]
struct Inner {
    pending: HashMap<String, Pending>,
    live: HashMap<String, Arc<tokio::sync::Mutex<Live>>>,
    links: HashMap<String, Link>,
    armed_until: Option<i64>,
    /// What the last push check saw, so only changes become alerts.
    seen: Option<Seen>,
    /// Alerts other services queued (the Night Shift summary, F-RT-06).
    queued: Vec<PushAlert>,
}

#[derive(Default)]
struct Seen {
    approvals: std::collections::HashSet<String>,
    agents: HashMap<String, AgentStatus>,
}

pub struct PhoneService {
    store: Store,
    bus: EventBus,
    keychain: Arc<dyn Keychain>,
    clock: Arc<dyn Clock>,
    web_base: String,
    rekey: RekeyPolicy,
    host: OnceLock<Arc<dyn PhoneHost>>,
    net: OnceLock<Arc<dyn PhoneNet>>,
    inner: Mutex<Inner>,
}

fn store_err(e: tandem_store::StoreError) -> IpcError {
    IpcError::new(ErrorCode::Internal, e.to_string())
}

fn b64(b: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(b)
}

fn key32(text: &str) -> Option<[u8; 32]> {
    URL_SAFE_NO_PAD.decode(text).ok()?.try_into().ok()
}

fn clip(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let mut out: String = text.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

fn device_name(raw: &str) -> String {
    let clean: String = raw.chars().filter(|c| !c.is_control()).collect();
    let clean = clean.trim();
    if clean.is_empty() {
        "Phone".into()
    } else {
        clip(clean, MAX_DEVICE_NAME)
    }
}

fn grants_of(json: &str) -> PhoneGrants {
    serde_json::from_str(json).unwrap_or_default()
}

fn res_err(id: u64, code: &str, message: &str) -> ToPhone {
    ToPhone::Res {
        id,
        ok: false,
        data: None,
        error: Some(ResError {
            code: code.into(),
            message: message.into(),
        }),
    }
}

fn res_ok(id: u64, data: Value) -> ToPhone {
    ToPhone::Res {
        id,
        ok: true,
        data: Some(data),
        error: None,
    }
}

/// The pairing link the QR code carries: the secret rides in the fragment,
/// which browsers never send to a server.
pub fn pairing_link(
    web_base: &str,
    pid: &str,
    dpk: &[u8; 32],
    s: &Secret32,
    relay: &str,
    ptok: &str,
) -> String {
    let payload = json!({ "v": 1, "pid": pid, "dpk": b64(dpk), "s": b64(&s.0), "relay": relay, "ptok": ptok });
    format!(
        "{}/remote#p={}",
        web_base.trim_end_matches('/'),
        b64(payload.to_string().as_bytes())
    )
}

pub fn qr_matrix(text: &str) -> Result<QrMatrix, IpcError> {
    let code = qrcode::QrCode::with_error_correction_level(text.as_bytes(), qrcode::EcLevel::M)
        .map_err(|e| IpcError::new(ErrorCode::Internal, format!("QR code: {e}")))?;
    let size = code.width();
    let colors = code.to_colors();
    let rows = colors
        .chunks(size)
        .map(|row| {
            row.iter()
                .map(|c| if *c == qrcode::Color::Dark { '1' } else { '0' })
                .collect()
        })
        .collect();
    Ok(QrMatrix {
        size: size as u32,
        rows,
    })
}

impl PhoneService {
    pub fn new(
        store: Store,
        bus: EventBus,
        keychain: Arc<dyn Keychain>,
        clock: Arc<dyn Clock>,
        web_base: String,
        rekey: RekeyPolicy,
    ) -> Arc<Self> {
        Arc::new(Self {
            store,
            bus,
            keychain,
            clock,
            web_base,
            rekey,
            host: OnceLock::new(),
            net: OnceLock::new(),
            inner: Mutex::new(Inner::default()),
        })
    }

    /// Binds the app and the network (once, at setup).
    pub fn bind(&self, host: Arc<dyn PhoneHost>, net: Arc<dyn PhoneNet>) {
        let _ = self.host.set(host);
        let _ = self.net.set(net);
    }

    fn host(&self) -> Result<&Arc<dyn PhoneHost>, IpcError> {
        self.host
            .get()
            .ok_or_else(|| IpcError::new(ErrorCode::Unavailable, "The phone service isn't ready"))
    }

    fn net(&self) -> Result<&Arc<dyn PhoneNet>, IpcError> {
        self.net
            .get()
            .ok_or_else(|| IpcError::new(ErrorCode::Unavailable, "The phone service isn't ready"))
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn emit(&self, pairing_id: &str, event: &str) {
        self.bus.publish(BusEvent::new(
            Topic::Remote,
            "remote:phone",
            json!({ "pairingId": pairing_id, "event": event }),
        ));
    }

    /// The desktop's static X25519 key, made on first pairing.
    fn identity(&self) -> Result<KeyPair, IpcError> {
        let service = remote_service();
        let kc_err =
            |e: keychain::KeychainError| IpcError::new(ErrorCode::Unavailable, e.to_string());
        if let Some(raw) = self.keychain.get(&service, IDENTITY).map_err(kc_err)? {
            if let Ok(b) = <[u8; 32]>::try_from(raw.as_slice()) {
                return Ok(KeyPair::from_secret(Secret32(b)));
            }
        }
        let pair = KeyPair::generate();
        self.keychain
            .set(&service, IDENTITY, &pair.secret.0)
            .map_err(kc_err)?;
        Ok(pair)
    }

    fn armed(&self) -> Option<i64> {
        let now = self.clock.now_ms();
        let mut inner = self.lock();
        match inner.armed_until {
            Some(t) if t > now => Some(t),
            Some(_) => {
                inner.armed_until = None;
                None
            }
            None => None,
        }
    }

    // ── pairing ──────────────────────────────────────────────────────────

    /// Starts a pairing: registers it with the API, opens its relay link and
    /// returns the QR code. The one-time secret stays in memory and is
    /// dropped when the handshake completes or the code expires.
    pub async fn pair_begin(self: &Arc<Self>) -> Result<PairingQr, IpcError> {
        self.pair_begin_via(PairVia::Relay).await
    }

    /// [`Self::pair_begin`] through the hosted relay or the LAN (F-RM-04).
    pub async fn pair_begin_via(self: &Arc<Self>, via: PairVia) -> Result<PairingQr, IpcError> {
        let host = self.host()?.clone();
        let net = self.net()?.clone();
        let identity = self.identity()?;
        let pid = PairingId::new();
        let ticket = match &via {
            PairVia::Relay => {
                net.create_pairing(pid.as_str(), Some(&host.desktop_name()))
                    .await?
            }
            PairVia::Lan { address } => {
                net.create_lan_pairing(pid.as_str(), address.as_deref())
                    .await?
            }
        };
        let s = Secret32::random();
        let link = pairing_link(
            ticket.web_base.as_deref().unwrap_or(&self.web_base),
            pid.as_str(),
            &identity.public,
            &s,
            &ticket.relay_url,
            &ticket.pairing_token,
        );
        let qr = qr_matrix(&link)?;
        let expires_at = self.clock.now_ms() + QR_TTL_MS;
        self.lock().pending.insert(
            pid.as_str().to_string(),
            Pending {
                s,
                relay_url: ticket.relay_url.clone(),
                expires_at,
            },
        );
        self.start_link(pid.as_str(), &ticket.relay_url);
        let me = self.clone();
        let id = pid.as_str().to_string();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(Duration::from_millis(QR_TTL_MS as u64)).await;
            me.expire_pending(&id).await;
        });
        Ok(PairingQr {
            pairing_id: pid,
            link,
            qr,
            expires_at,
        })
    }

    /// A code nobody scanned in time: forget it and its relay link.
    pub async fn expire_pending(&self, pid: &str) {
        let stale = {
            let mut inner = self.lock();
            let stale = inner
                .pending
                .get(pid)
                .is_some_and(|p| p.expires_at <= self.clock.now_ms());
            if stale {
                inner.pending.remove(pid);
            }
            stale
        };
        if stale {
            self.stop_link(pid);
            if let Ok(net) = self.net() {
                if let Err(err) = net.delete_pairing(pid).await {
                    tracing::warn!(code = "phone.expire_failed", error = %err, "couldn't drop an unused pairing");
                }
            }
        }
    }

    /// Cancels a pairing that hasn't completed (the dialog closed).
    pub async fn pair_cancel(&self, pid: &str) {
        let was_pending = self.lock().pending.remove(pid).is_some();
        if was_pending {
            self.stop_link(pid);
            if let Ok(net) = self.net() {
                let _ = net.delete_pairing(pid).await;
            }
        }
    }

    pub async fn list(&self) -> Result<Vec<Pairing>, IpcError> {
        let rows = repo::list_active(&self.store).await.map_err(store_err)?;
        let inner = self.lock();
        Ok(rows
            .into_iter()
            .map(|r| Pairing {
                online: inner.links.get(&r.id).is_some_and(|l| l.online),
                id: PairingId::from_db(r.id),
                device_name: r.device_name,
                grants: grants_of(&r.grants_json),
                created_at: r.created_at,
                last_seen: r.last_seen,
            })
            .collect())
    }

    /// Changes a pairing's grants. Only a person in the main window can.
    pub async fn set_grants(
        &self,
        _review: &LocalReview,
        pid: &str,
        grants: PhoneGrants,
    ) -> Result<(), IpcError> {
        let json = serde_json::to_string(&grants)
            .map_err(|e| IpcError::new(ErrorCode::Internal, e.to_string()))?;
        if !repo::set_grants(&self.store, pid, &json)
            .await
            .map_err(store_err)?
        {
            return Err(IpcError::new(
                ErrorCode::NotFound,
                "That phone isn't paired",
            ));
        }
        let live = self.lock().live.get(pid).cloned();
        if let Some(live) = live {
            live.lock().await.grants = grants;
        }
        self.push(pid, ToPhone::Changed).await;
        self.emit(pid, "changed");
        Ok(())
    }

    /// "Allow approvals from phone for the next 8 hours" (`hours` 8), or off (0).
    pub fn arm(&self, _review: &LocalReview, hours: i64) -> Result<PhoneArmState, IpcError> {
        let armed_until = match hours {
            0 => None,
            ARM_HOURS => Some(self.clock.now_ms() + ARM_HOURS * 3_600_000),
            _ => {
                return Err(IpcError::new(
                    ErrorCode::Invalid,
                    "Arm for 8 hours or turn it off",
                ))
            }
        };
        self.lock().armed_until = armed_until;
        self.bus.publish(BusEvent::new(
            Topic::Remote,
            "remote:phone",
            json!({ "pairingId": null, "event": "armed" }),
        ));
        Ok(PhoneArmState { armed_until })
    }

    pub fn arm_state(&self) -> PhoneArmState {
        PhoneArmState {
            armed_until: self.armed(),
        }
    }

    // ── the step-up queue ────────────────────────────────────────────────

    /// Allows from the phone waiting for the desktop. Items whose approval
    /// was answered elsewhere meanwhile are settled as rejected.
    pub async fn queue_list(&self) -> Result<Vec<QueuedPhoneAction>, IpcError> {
        let pending: HashMap<String, ApprovalView> = self
            .host()?
            .approvals()
            .into_iter()
            .map(|a| (a.id.as_str().to_string(), a))
            .collect();
        let rows = repo::queue_list(&self.store).await.map_err(store_err)?;
        let names: HashMap<String, String> = repo::list_active(&self.store)
            .await
            .map_err(store_err)?
            .into_iter()
            .map(|r| (r.id, r.device_name))
            .collect();
        let mut out = Vec::new();
        for row in rows {
            let approval_id = serde_json::from_str::<Value>(&row.action_json)
                .ok()
                .and_then(|v| v["approvalId"].as_str().map(str::to_string))
                .unwrap_or_default();
            match pending.get(&approval_id) {
                Some(view) => out.push(QueuedPhoneAction {
                    id: PhoneQueueId::from_db(row.id),
                    device_name: names.get(&row.pairing_id).cloned().unwrap_or_default(),
                    pairing_id: PairingId::from_db(row.pairing_id),
                    approval_id,
                    summary: view.summary.clone(),
                    created_at: row.created_at,
                }),
                None => {
                    let _ = repo::queue_settle(&self.store, &row.id, false).await;
                }
            }
        }
        Ok(out)
    }

    /// The desktop confirms (`apply`) or drops a phone's queued allow.
    pub async fn queue_decide(
        &self,
        _review: &LocalReview,
        item_id: &str,
        apply: bool,
    ) -> Result<(), IpcError> {
        let row = repo::queue_get(&self.store, item_id)
            .await
            .map_err(store_err)?
            .filter(|r| r.state == "queued")
            .ok_or_else(|| {
                IpcError::new(ErrorCode::NotFound, "That request was already settled")
            })?;
        let approval_id = serde_json::from_str::<Value>(&row.action_json)
            .ok()
            .and_then(|v| v["approvalId"].as_str().map(str::to_string))
            .unwrap_or_default();
        let mut applied = false;
        if apply {
            self.host()?
                .approval_resolve(&approval_id, PhoneDecision::Allow)
                .map_err(|m| IpcError::new(ErrorCode::Conflict, m))?;
            applied = true;
        }
        repo::queue_settle(&self.store, item_id, applied)
            .await
            .map_err(store_err)?;
        self.emit(&row.pairing_id, "queued");
        self.push(&row.pairing_id, ToPhone::Changed).await;
        Ok(())
    }

    // ── revocation ───────────────────────────────────────────────────────

    /// Revokes a pairing: the phone hears `bye` if it's connected, the keys
    /// leave the keychain, the API drops the relay sockets, and any later
    /// frame for it is ignored (AC4).
    pub async fn revoke(&self, pid: &str, reason: &str) -> Result<(), IpcError> {
        self.push(
            pid,
            ToPhone::Bye {
                reason: reason.into(),
            },
        )
        .await;
        self.revoke_local(pid).await?;
        if let Ok(net) = self.net() {
            if let Err(err) = net.delete_pairing(pid).await {
                tracing::warn!(code = "phone.revoke_remote_failed", error = %err, "couldn't tell the API about a revoked pairing");
            }
        }
        Ok(())
    }

    async fn revoke_local(&self, pid: &str) -> Result<(), IpcError> {
        {
            let mut inner = self.lock();
            inner.pending.remove(pid);
            inner.live.remove(pid);
        }
        self.stop_link(pid);
        if let Ok(net) = self.net() {
            net.forget_local(pid);
        }
        let _ = self.keychain.delete(&remote_service(), &push_account(pid));
        if let Err(err) = self
            .keychain
            .delete(&remote_service(), &pairing_account(pid))
        {
            tracing::warn!(code = "phone.key_delete_failed", error = %err, "couldn't delete a pairing key");
        }
        repo::revoke(&self.store, pid, self.clock.now_ms())
            .await
            .map_err(store_err)?;
        tracing::info!(code = "phone.revoked", "phone pairing revoked");
        self.emit(pid, "revoked");
        Ok(())
    }

    /// Sign-out, an account change, or "Reset remote access": every pairing
    /// goes. `reset` also replaces the desktop's identity key.
    pub async fn revoke_all(&self, reason: &str, reset: bool) -> Result<(), IpcError> {
        let ids: Vec<String> = repo::list_active(&self.store)
            .await
            .map_err(store_err)?
            .into_iter()
            .map(|r| r.id)
            .collect();
        let pending: Vec<String> = self.lock().pending.keys().cloned().collect();
        for id in pending {
            self.pair_cancel(&id).await;
        }
        for id in ids {
            self.revoke(&id, reason).await?;
        }
        if reset {
            let _ = self.keychain.delete(&remote_service(), IDENTITY);
        }
        Ok(())
    }

    /// Revokes everything without reaching the API (the session is gone).
    pub async fn revoke_all_local(&self) {
        let Ok(rows) = repo::list_active(&self.store).await else {
            return;
        };
        let pending: Vec<String> = self.lock().pending.drain().map(|(k, _)| k).collect();
        for id in pending {
            self.stop_link(&id);
        }
        for row in rows {
            self.push(
                &row.id,
                ToPhone::Bye {
                    reason: "signed_out".into(),
                },
            )
            .await;
            let _ = self.revoke_local(&row.id).await;
        }
    }

    // ── push (F-RM-06) ───────────────────────────────────────────────────

    fn save_push(&self, pid: &str, push: &StoredPush) -> Result<(), String> {
        let bytes = serde_json::to_vec(push).map_err(|e| e.to_string())?;
        self.keychain
            .set(&remote_service(), &push_account(pid), &bytes)
            .map_err(|e| e.to_string())
    }

    fn load_push(&self, pid: &str) -> Option<StoredPush> {
        let raw = self
            .keychain
            .get(&remote_service(), &push_account(pid))
            .ok()??;
        serde_json::from_slice(&raw).ok()
    }

    /// Whether this phone turned push on.
    pub fn push_enabled(&self, pid: &str) -> bool {
        self.load_push(pid).is_some()
    }

    /// What changed since the last check that a phone should hear about:
    /// new approvals, and agents that finished, need input or failed.
    async fn alerts(&self, host: &dyn PhoneHost) -> Vec<PushAlert> {
        let approvals = host.approvals();
        let agents = host.agents().await;
        let now = self.clock.now_ms();
        let mut inner_seen = Seen::default();
        for a in &approvals {
            inner_seen.approvals.insert(a.id.to_string());
        }
        for a in &agents {
            inner_seen.agents.insert(a.key.clone(), a.status);
        }
        let prev = self.lock().seen.replace(inner_seen);
        let Some(prev) = prev else { return Vec::new() };
        let mut out = Vec::new();
        for a in &approvals {
            if !prev.approvals.contains(&a.id.to_string()) {
                out.push(PushAlert {
                    kind: "approval".into(),
                    id: a.id.to_string(),
                    title: "Approval needed".into(),
                    body: if desk_only(a) {
                        PANE_APPROVAL_BODY.into()
                    } else {
                        clip(&tandem_core::untrusted::redacted(&a.summary), 160)
                    },
                    at: now,
                });
            }
        }
        for a in &agents {
            let Some(before) = prev.agents.get(&a.key) else {
                continue;
            };
            let what = match (before, a.status) {
                (b, s) if *b == s => continue,
                (_, AgentStatus::Waiting) => "needs you",
                (AgentStatus::Working, AgentStatus::Done) => "finished",
                (_, AgentStatus::Failed) => "stopped with an error",
                _ => continue,
            };
            // The agent's title is its own (D-3173).
            let title = format!("{} {what}", tandem_core::untrusted::redacted(&a.title));
            out.push(PushAlert {
                kind: "agent".into(),
                id: a.key.clone(),
                title: clip(&title, 80),
                body: clip(
                    &tandem_core::untrusted::redacted(a.preview.as_deref().unwrap_or_default()),
                    160,
                ),
                at: now,
            });
        }
        out.append(&mut self.lock().queued);
        out
    }

    /// Queues an alert for the next push check (the Night Shift summary,
    /// F-RT-06). Phones get it only where push is on.
    pub fn queue_alert(&self, alert: PushAlert) {
        self.lock().queued.push(alert);
    }

    /// Seals each new alert for every phone that turned push on and isn't
    /// connected now, and hands it to the API (F-RM-06).
    pub async fn push_alerts(&self) {
        let Ok(host) = self.host().cloned() else {
            return;
        };
        let alerts = self.alerts(host.as_ref()).await;
        if alerts.is_empty() {
            return;
        }
        let Ok(net) = self.net().cloned() else { return };
        let Ok(rows) = repo::list_active(&self.store).await else {
            return;
        };
        for row in rows {
            let online = self.lock().links.get(&row.id).is_some_and(|l| l.online);
            if online {
                continue;
            }
            let Some(mut push) = self.load_push(&row.id) else {
                continue;
            };
            for alert in &alerts {
                push.seq += 1;
                let Ok(plain) = serde_json::to_vec(alert) else {
                    continue;
                };
                let Ok(frame) = seal_push(&row.id, &push.key, push.seq, &plain) else {
                    continue;
                };
                // The sequence number is saved before the alert leaves.
                if self.save_push(&row.id, &push).is_err() {
                    break;
                }
                match net
                    .push(&row.id, push.platform, &push.token, &b64(&frame))
                    .await
                {
                    Ok(PushOutcome::Sent) => {}
                    Ok(PushOutcome::Unregistered) => {
                        let _ = self
                            .keychain
                            .delete(&remote_service(), &push_account(&row.id));
                        break;
                    }
                    Err(err) => {
                        tracing::debug!(code = "phone.push_failed", error = %err, "push not sent");
                        break;
                    }
                }
            }
        }
    }

    // ── links ────────────────────────────────────────────────────────────

    /// Reopens every live pairing's link, and follows the app for `changed`
    /// hints and sign-out.
    pub async fn start(self: &Arc<Self>) {
        if let Ok(rows) = repo::list_active(&self.store).await {
            for row in rows {
                match self.load_live(&row.id).await {
                    Some(live) => {
                        let url = live.lock().await.relay_url.clone();
                        self.start_link(&row.id, &url);
                    }
                    None => {
                        let _ = self.revoke_local(&row.id).await;
                    }
                }
            }
        }
        // What's there at start isn't news.
        if let Ok(host) = self.host().cloned() {
            let _ = self.alerts(host.as_ref()).await;
        }
        let me = self.clone();
        let mut approvals = self.bus.subscribe(Topic::Approvals);
        let mut status = self.bus.subscribe(Topic::Status);
        let mut threads = self.bus.subscribe(Topic::Threads);
        tauri::async_runtime::spawn(async move {
            loop {
                let open = tokio::select! {
                    r = approvals.recv() => !matches!(r, Err(tokio::sync::broadcast::error::RecvError::Closed)),
                    r = status.recv() => !matches!(r, Err(tokio::sync::broadcast::error::RecvError::Closed)),
                    r = threads.recv() => !matches!(r, Err(tokio::sync::broadcast::error::RecvError::Closed)),
                };
                if !open {
                    return;
                }
                // Coalesce a burst into one hint.
                tokio::time::sleep(Duration::from_millis(500)).await;
                while approvals.try_recv().is_ok()
                    || status.try_recv().is_ok()
                    || threads.try_recv().is_ok()
                {}
                me.push_all(ToPhone::Changed).await;
                me.push_alerts().await;
            }
        });
        let me = self.clone();
        let mut app = self.bus.subscribe(Topic::App);
        tauri::async_runtime::spawn(async move {
            loop {
                match app.recv().await {
                    Ok(ev) if ev.name == "auth:changed" => {
                        if ev.payload["signedIn"] == Value::Bool(false) {
                            me.revoke_all_local().await;
                        }
                    }
                    Ok(_) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                    Err(_) => return,
                }
            }
        });
    }

    fn start_link(self: &Arc<Self>, pid: &str, relay_url: &str) {
        let (stop_tx, mut stop_rx) = watch::channel(false);
        {
            let mut inner = self.lock();
            if let Some(old) = inner.links.insert(
                pid.to_string(),
                Link {
                    out: None,
                    online: false,
                    stop: stop_tx,
                },
            ) {
                let _ = old.stop.send(true);
            }
        }
        let me = self.clone();
        let pid = pid.to_string();
        let url = relay_url.to_string();
        tauri::async_runtime::spawn(async move {
            let mut backoff = 1u64;
            loop {
                if *stop_rx.borrow() {
                    return;
                }
                let Ok(net) = me.net() else { return };
                match net.dial(&url).await {
                    Ok(mut link) => {
                        backoff = 1;
                        me.set_link_out(&pid, Some(link.tx.clone()));
                        let mut refused = false;
                        loop {
                            let ev = tokio::select! {
                                _ = stop_rx.changed() => None,
                                ev = link.rx.recv() => ev,
                            };
                            match ev {
                                Some(RelayIn::Frame(frame)) => {
                                    for reply in me.on_frame(&pid, &frame).await {
                                        let _ = link.tx.send(reply);
                                    }
                                }
                                Some(RelayIn::Peer(up)) => me.set_online(&pid, up),
                                Some(RelayIn::Closed(code)) => {
                                    refused = code == Some(1008);
                                    break;
                                }
                                None => break,
                            }
                        }
                        me.set_link_out(&pid, None);
                        me.set_online(&pid, false);
                        if refused {
                            backoff = 30;
                        }
                    }
                    Err(err) => {
                        tracing::debug!(code = "phone.dial_failed", error = %err, "relay unreachable");
                    }
                }
                if *stop_rx.borrow() {
                    return;
                }
                tokio::select! {
                    _ = stop_rx.changed() => return,
                    _ = tokio::time::sleep(Duration::from_secs(backoff)) => {}
                }
                backoff = (backoff * 2).min(60);
            }
        });
    }

    /// Tests: drops every link, as a shutdown does.
    #[cfg(test)]
    pub fn stop_all_links_for_tests(&self) {
        let ids: Vec<String> = self.lock().links.keys().cloned().collect();
        for id in ids {
            self.stop_link(&id);
        }
    }

    fn stop_link(&self, pid: &str) {
        if let Some(link) = self.lock().links.remove(pid) {
            let _ = link.stop.send(true);
        }
    }

    fn set_link_out(&self, pid: &str, out: Option<mpsc::UnboundedSender<Vec<u8>>>) {
        if let Some(link) = self.lock().links.get_mut(pid) {
            link.out = out;
        }
    }

    fn set_online(&self, pid: &str, up: bool) {
        let changed = match self.lock().links.get_mut(pid) {
            Some(link) if link.online != up => {
                link.online = up;
                true
            }
            _ => false,
        };
        if changed {
            self.emit(pid, if up { "connected" } else { "disconnected" });
        }
    }

    // ── sessions ─────────────────────────────────────────────────────────

    async fn load_live(&self, pid: &str) -> Option<Arc<tokio::sync::Mutex<Live>>> {
        if let Some(live) = self.lock().live.get(pid).cloned() {
            return Some(live);
        }
        let row = repo::get(&self.store, pid).await.ok()??;
        if row.revoked_at.is_some() {
            return None;
        }
        let raw = self
            .keychain
            .get(&remote_service(), &pairing_account(pid))
            .ok()??;
        let stored: StoredPairing = serde_json::from_slice(&raw).ok()?;
        let mut session = stored.session;
        // The database counters are written after every frame; the keychain's
        // only at handshake and rekey. Take the larger.
        session.tx_ctr = session.tx_ctr.max(row.tx_counter);
        session.rx_ctr = session.rx_ctr.max(row.rx_counter);
        let live = Arc::new(tokio::sync::Mutex::new(Live {
            session,
            relay_url: stored.relay_url,
            phone_pk: stored.phone_pk,
            grants: grants_of(&row.grants_json),
            device: row.device_name,
            rekey: None,
            rekeyed_at: self.clock.now_ms(),
            frames: 0,
        }));
        Some(
            self.lock()
                .live
                .entry(pid.to_string())
                .or_insert(live)
                .clone(),
        )
    }

    fn save_keys(&self, pid: &str, live: &Live) {
        let stored = StoredPairing {
            phone_pk: live.phone_pk.clone(),
            relay_url: live.relay_url.clone(),
            session: live.session.clone(),
        };
        match serde_json::to_vec(&stored) {
            Ok(bytes) => {
                if let Err(err) =
                    self.keychain
                        .set(&remote_service(), &pairing_account(pid), &bytes)
                {
                    tracing::warn!(code = "phone.key_save_failed", error = %err, "couldn't save pairing keys");
                }
            }
            Err(err) => {
                tracing::warn!(code = "phone.key_save_failed", error = %err, "couldn't encode pairing keys")
            }
        }
    }

    async fn save_counters(&self, pid: &str, live: &Live, seen: bool) {
        let s = &live.session;
        let seen = seen.then(|| self.clock.now_ms());
        if let Err(err) =
            repo::set_counters(&self.store, pid, s.tx_epoch(), s.tx_ctr, s.rx_ctr, seen).await
        {
            tracing::warn!(code = "phone.counter_save_failed", error = %err, "couldn't save frame counters");
        }
    }

    async fn seal(&self, pid: &str, live: &mut Live, msg: &ToPhone) -> Option<Vec<u8>> {
        let bytes = serde_json::to_vec(msg).ok()?;
        match live.session.seal_next(&bytes) {
            Ok(frame) => {
                live.frames += 1;
                self.save_counters(pid, live, false).await;
                Some(frame)
            }
            Err(err) => {
                tracing::warn!(
                    code = "phone.seal_failed",
                    reason = err.as_str(),
                    "couldn't seal a message"
                );
                None
            }
        }
    }

    /// Seals and sends one message to a connected phone.
    pub async fn push(&self, pid: &str, msg: ToPhone) {
        let out = self
            .lock()
            .links
            .get(pid)
            .filter(|l| l.online)
            .and_then(|l| l.out.clone());
        let Some(out) = out else { return };
        let Some(live) = self.lock().live.get(pid).cloned() else {
            return;
        };
        let mut live = live.lock().await;
        if let Some(frame) = self.seal(pid, &mut live, &msg).await {
            let _ = out.send(frame);
        }
    }

    async fn push_all(&self, msg: ToPhone) {
        let ids: Vec<String> = self
            .lock()
            .links
            .iter()
            .filter(|(_, l)| l.online)
            .map(|(k, _)| k.clone())
            .collect();
        for id in ids {
            self.push(&id, msg.clone()).await;
        }
    }

    /// Handles one frame from the relay and returns the frames to send back.
    pub async fn on_frame(&self, pid: &str, frame: &[u8]) -> Vec<Vec<u8>> {
        match frame_kind(frame) {
            Some(KIND_HELLO) => {
                self.on_hello(pid, frame).await;
                Vec::new()
            }
            Some(KIND_SEALED) => self.on_sealed(pid, frame).await,
            _ => Vec::new(),
        }
    }

    async fn on_hello(&self, pid: &str, frame: &[u8]) {
        let Ok((phone_pub, tag)) = decode_hello(frame) else {
            return;
        };
        let now = self.clock.now_ms();
        let (s, relay_url) = {
            let inner = self.lock();
            match inner.pending.get(pid) {
                Some(p) if p.expires_at > now => (p.s.clone(), p.relay_url.clone()),
                _ => return,
            }
        };
        // Proves the sender scanned the code; the relay can't fake it.
        if verify_hello(&s, pid, &phone_pub, &tag).is_err() {
            tracing::warn!(
                code = "phone.hello_refused",
                "a pairing hello failed its check"
            );
            return;
        }
        let Ok(identity) = self.identity() else {
            return;
        };
        let Ok(root) = root_secret(
            &s,
            pid,
            &identity.public,
            &phone_pub,
            &identity.secret,
            &phone_pub,
        ) else {
            return;
        };
        // The one-time secret is done with.
        if self.lock().pending.remove(pid).is_none() {
            return;
        }
        let live = Live {
            session: Session::new(pid, Role::Desktop, root),
            relay_url,
            phone_pk: b64(&phone_pub),
            grants: PhoneGrants::default(),
            device: "Phone".into(),
            rekey: None,
            rekeyed_at: now,
            frames: 0,
        };
        let grants = serde_json::to_string(&PhoneGrants::default()).unwrap_or_else(|_| "{}".into());
        if let Err(err) = repo::insert(&self.store, pid, "Phone", &grants, now).await {
            tracing::warn!(code = "phone.pair_save_failed", error = %err, "couldn't save a pairing");
            return;
        }
        self.save_keys(pid, &live);
        self.lock()
            .live
            .insert(pid.to_string(), Arc::new(tokio::sync::Mutex::new(live)));
        tracing::info!(code = "phone.paired", "phone paired");
        self.emit(pid, "paired");
    }

    async fn on_sealed(&self, pid: &str, frame: &[u8]) -> Vec<Vec<u8>> {
        let Some(live) = self.load_live(pid).await else {
            return Vec::new();
        };
        let mut live = live.lock().await;
        let plaintext = match live.session.open_next(frame) {
            Ok(p) => p,
            Err(err) => {
                tracing::info!(
                    code = "phone.frame_refused",
                    reason = err.as_str(),
                    "refused a phone frame"
                );
                let reject = ToPhone::Reject {
                    reason: err.as_str().into(),
                };
                return self
                    .seal(pid, &mut live, &reject)
                    .await
                    .into_iter()
                    .collect();
            }
        };
        live.frames += 1;
        // The counter is saved before the command runs, so a crash can't
        // let the same frame run twice.
        self.save_counters(pid, &live, true).await;
        let Ok(msg) = serde_json::from_slice::<FromPhone>(&plaintext) else {
            let reject = ToPhone::Reject {
                reason: ProtocolError::BadFrame.as_str().into(),
            };
            return self
                .seal(pid, &mut live, &reject)
                .await
                .into_iter()
                .collect();
        };
        let mut out = Vec::new();
        match msg {
            FromPhone::Hi { device } => {
                let name = device_name(&device);
                if name != live.device {
                    live.device = name.clone();
                    let _ = repo::set_device_name(&self.store, pid, &name).await;
                }
                self.set_online(pid, true);
                self.emit(pid, "connected");
                let welcome = ToPhone::Welcome {
                    desktop: self.host().map(|h| h.desktop_name()).unwrap_or_default(),
                    grants: live.grants,
                    armed_until: self.armed(),
                };
                out.extend(self.seal(pid, &mut live, &welcome).await);
            }
            FromPhone::Cmd { id, cmd, args } => {
                let reply = self.command(pid, &live, id, &cmd, args).await;
                self.emit(pid, "command");
                out.extend(self.seal(pid, &mut live, &reply).await);
            }
            FromPhone::RekeyAck { eph } => {
                if let (Some(mine), Some(theirs)) = (live.rekey.take(), key32(&eph)) {
                    if live.session.complete_rekey(&mine, &theirs).is_ok() {
                        live.rekeyed_at = self.clock.now_ms();
                        live.frames = 0;
                        // The first frame under the new keys moves the phone over.
                        out.extend(self.seal(pid, &mut live, &ToPhone::Changed).await);
                        self.save_keys(pid, &live);
                    }
                }
            }
            FromPhone::Bye => {
                drop(live);
                if let Err(err) = self.revoke(pid, "unpaired").await {
                    tracing::warn!(code = "phone.revoke_failed", error = %err.message, "couldn't revoke a pairing");
                }
                return Vec::new();
            }
        }
        // Rekey every hour or 2^20 frames, one at a time.
        let due = self.clock.now_ms() - live.rekeyed_at >= self.rekey.interval_ms
            || live.frames >= self.rekey.frames;
        if due && live.rekey.is_none() && live.session.next.is_none() {
            let eph = KeyPair::generate();
            let msg = ToPhone::Rekey {
                eph: b64(&eph.public),
            };
            live.rekey = Some(eph.secret);
            out.extend(self.seal(pid, &mut live, &msg).await);
        }
        out
    }

    async fn command(&self, pid: &str, live: &Live, id: u64, cmd: &str, args: Value) -> ToPhone {
        let parsed = match PhoneCommand::parse(cmd, args) {
            Ok(c) => c,
            Err(CommandError::Unknown) => {
                return res_err(
                    id,
                    "unknown_command",
                    "Your computer doesn't know that command.",
                )
            }
            Err(CommandError::Invalid(m)) => return res_err(id, "invalid", m),
        };
        if !live.grants.allows(parsed.grant()) {
            return res_err(
                id,
                "forbidden",
                "That's turned off for this phone. Turn it on in Settings › Remote on your computer.",
            );
        }
        let Ok(host) = self.host() else {
            return res_err(id, "unavailable", "Your computer isn't ready yet.");
        };
        match parsed {
            PhoneCommand::ViewSnapshot => match self.snapshot(host.as_ref(), live).await {
                Ok(v) => res_ok(id, v),
                Err(m) => res_err(id, "failed", &m),
            },
            PhoneCommand::ThreadRead {
                thread_id,
                before,
                limit,
            } => match host
                .thread_read(&thread_id, before, limit.min(MAX_TRANSCRIPT_ITEMS))
                .await
            {
                Ok(mut page) => {
                    for item in &mut page.items {
                        item.text = clip(&item.text, MAX_ITEM_CHARS);
                    }
                    res_ok(id, serde_json::to_value(page).unwrap_or(Value::Null))
                }
                Err(m) => res_err(id, "not_found", &m),
            },
            PhoneCommand::ThreadSend { thread_id, text } => {
                match host.thread_send(&thread_id, &text).await {
                    Ok(()) => res_ok(id, json!({})),
                    Err(m) => res_err(id, "failed", &m),
                }
            }
            PhoneCommand::PaneType { pane_id, text } => match host.pane_type(&pane_id, &text).await
            {
                Ok(()) => res_ok(id, json!({})),
                Err(m) => res_err(id, "failed", &m),
            },
            PhoneCommand::ApprovalDecide {
                approval_id,
                decision,
            } => {
                let Some(view) = host
                    .approvals()
                    .into_iter()
                    .find(|a| a.id.as_str() == approval_id)
                else {
                    return res_err(id, "not_found", "That approval was already answered.");
                };
                if desk_only(&view) && decision == PhoneDecision::Allow {
                    return res_err(id, "desk_only", "Answer this on your computer.");
                }
                match approval_route(decision, self.armed().is_some()) {
                    ApprovalRoute::Apply => match host.approval_resolve(&approval_id, decision) {
                        Ok(()) => res_ok(id, json!({ "state": "applied" })),
                        Err(m) => res_err(id, "failed", &m),
                    },
                    ApprovalRoute::Queue => {
                        let qid = PhoneQueueId::new();
                        let action =
                            json!({ "approvalId": approval_id, "decision": "allow" }).to_string();
                        if let Err(err) = repo::queue_insert(
                            &self.store,
                            qid.as_str(),
                            pid,
                            &action,
                            self.clock.now_ms(),
                        )
                        .await
                        {
                            return res_err(id, "failed", &err.to_string());
                        }
                        host.queued_notice(&view, &live.device).await;
                        self.emit(pid, "queued");
                        res_ok(id, json!({ "state": "queued" }))
                    }
                }
            }
            PhoneCommand::RoutineRunNow { routine_id } => {
                match host.routine_run_now(&routine_id).await {
                    Ok(()) => res_ok(id, json!({})),
                    Err(m) => res_err(id, "unavailable", &m),
                }
            }
            PhoneCommand::ReviewCurrent => match host.review_current().await {
                Ok(r) => res_ok(
                    id,
                    serde_json::to_value(review_for_phone(r)).unwrap_or(Value::Null),
                ),
                Err(m) => res_err(id, "unavailable", &m),
            },
            PhoneCommand::PushRegister {
                platform,
                token,
                key,
            } => {
                if !live.relay_url.starts_with("wss://") {
                    return res_err(
                        id,
                        "unavailable",
                        "Push needs pairing through the Vibezeo relay. LAN pairings can't push.",
                    );
                }
                let stored = StoredPush {
                    platform,
                    token,
                    key: Secret32(key),
                    seq: 0,
                };
                match self.save_push(pid, &stored) {
                    Ok(()) => {
                        tracing::info!(
                            code = "phone.push_on",
                            platform = platform.as_str(),
                            "phone turned push on"
                        );
                        res_ok(id, json!({}))
                    }
                    Err(m) => res_err(id, "failed", &m),
                }
            }
            PhoneCommand::PushUnregister => {
                let _ = self.keychain.delete(&remote_service(), &push_account(pid));
                res_ok(id, json!({}))
            }
        }
    }

    async fn snapshot(&self, host: &dyn PhoneHost, live: &Live) -> Result<Value, String> {
        let queued: Vec<String> = repo::queue_list(&self.store)
            .await
            .map_err(|e| e.to_string())?
            .into_iter()
            .filter_map(|r| {
                serde_json::from_str::<Value>(&r.action_json)
                    .ok()
                    .and_then(|v| v["approvalId"].as_str().map(str::to_string))
            })
            .collect();
        let mut agents = host.agents().await;
        agents.truncate(MAX_AGENTS);
        let mut approvals: Vec<PhoneApproval> = host
            .approvals()
            .into_iter()
            .take(MAX_APPROVALS)
            .map(|a| {
                let desk = desk_only(&a);
                PhoneApproval {
                    queued: queued.iter().any(|q| q == a.id.as_str()),
                    id: a.id.as_str().to_string(),
                    // What the phone gets is off this machine (D-3171).
                    summary: if desk {
                        PANE_APPROVAL_BODY.into()
                    } else {
                        clip(&tandem_core::untrusted::redacted(&a.summary), 200)
                    },
                    detail: if desk {
                        String::new()
                    } else {
                        clip(&tandem_core::untrusted::redacted(&a.detail), 1000)
                    },
                    source: source_label(&a),
                    desk_only: desk,
                    created_at: a.created_at,
                }
            })
            .collect();
        let mut threads = host.threads().await;
        threads.truncate(MAX_THREADS);
        let mut routines = host.routines().await;
        routines.truncate(MAX_ROUTINES);
        for r in &mut routines {
            r.name = clip(&r.name, 120);
        }
        // An agent's title and latest line, and a thread's title, are what
        // an agent, a model or a person's message wrote, and the phone is off
        // this machine: redacted as written and as read, before the cut
        // (D-3171, D-3173).
        for a in &mut agents {
            a.title = clip(&tandem_core::untrusted::redacted(&a.title), 120);
            a.preview = a
                .preview
                .as_deref()
                .map(|p| clip(&tandem_core::untrusted::redacted(p), 200));
        }
        for t in &mut threads {
            t.title = clip(&tandem_core::untrusted::redacted(&t.title), 120);
        }
        let mut snap = PhoneSnapshot {
            desktop: host.desktop_name(),
            grants: live.grants,
            armed_until: self.armed(),
            agents,
            approvals: std::mem::take(&mut approvals),
            threads,
            routines,
            review_rev: host.review_rev(),
        };
        // Stay within one frame however much there is.
        loop {
            let v = serde_json::to_value(&snap).map_err(|e| e.to_string())?;
            if v.to_string().len() + 256 <= MAX_PLAINTEXT {
                return Ok(v);
            }
            if snap.routines.len() > 5 {
                snap.routines.truncate(snap.routines.len() / 2);
            } else if snap.threads.len() > 5 {
                snap.threads.truncate(snap.threads.len() / 2);
            } else if snap.agents.len() > 5 {
                snap.agents.truncate(snap.agents.len() / 2);
            } else if !snap.approvals.is_empty() {
                for a in &mut snap.approvals {
                    a.detail = clip(&a.detail, 100);
                }
                snap.approvals
                    .truncate(snap.approvals.len().saturating_sub(1).max(1));
            } else {
                return Err("Too much to send at once".into());
            }
        }
    }

    /// Test helper: whether a pairing has a code waiting to be scanned.
    #[cfg(test)]
    fn is_pending(&self, pid: &str) -> bool {
        self.lock().pending.contains_key(pid)
    }
}

/// A pane's approval (`vibezeo run`) is answered on the computer, where
/// the whole line shows: a clipped line could hide its second command.
fn desk_only(a: &ApprovalView) -> bool {
    matches!(a.source, tandem_core::source::SourceRef::Pane { .. })
}

fn source_label(a: &ApprovalView) -> String {
    use tandem_core::source::SourceRef;
    match &a.source {
        SourceRef::Pane { .. } => "Agent pane".into(),
        SourceRef::Thread { .. } => "Thread".into(),
        SourceRef::TeammateChat { .. } => "Teammate chat".into(),
        SourceRef::Routine { .. } => "Routine".into(),
        SourceRef::Task { .. } => "Task".into(),
        _ => "Vibezeo".into(),
    }
}

#[cfg(test)]
mod tests;
