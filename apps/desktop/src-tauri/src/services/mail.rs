//! Teammate mail (F-AG-12, ARCHITECTURE §8.7). `tandem__send_message` and
//! the composer's `@name` go through `send`: refused mail (the sender may not
//! message, the recipient is deleted or terminal-only) gets a row and an
//! error naming the reason; anything else is queued, and one worker delivers
//! the queue oldest first:
//! - an idle chat of the recipient's in the same thread of work gets it as a
//!   user turn "From {sender}:", or the recipient wakes in a fresh unattended
//!   chat "{sender}: {first line}" at no more than Edits;
//! - a mail is claimed once, so it reaches one chat at most once;
//! - a failed wake stays failed until Retry (never retried in a loop);
//! - "Pause all teammate mail" holds the queue until it's off again;
//! - at most 20 mail-triggered turns per teammate an hour; the rest wait
//!   with "Rate limit reached".

use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tandem_core::bus::{BusEvent, EventBus, Topic};
use tandem_core::ids::{MailId, TeammateId, ThreadId};
use tandem_core::settings::SettingKey;
use tandem_core::teammates::fold_name;
use tandem_core::{ErrorCode, IpcError};
use tandem_engines::events::ThreadEvent;
use tandem_store::repo::mail::{self as repo, Mail, MailReason, MailRow, MailState, NewMail};
use tandem_store::repo::teammates::Teammate;
use tandem_store::Store;
use ts_rs::TS;

use crate::services::teammates::TeammatesService;
use crate::services::threads::ThreadService;
use tandem_core::access::{Access, ThreadMode};
use tandem_store::repo::threads::{ChatExtra, NewThread, ThreadKind, ThreadState};

/// Mail-triggered turns a teammate may start in an hour (the loop guard).
pub const RATE_LIMIT: u32 = 20;
const WINDOW_MS: i64 = 60 * 60 * 1000;
/// The longest message.
pub const MAX_BODY: usize = 8_000;

fn store_err(e: tandem_store::StoreError) -> IpcError {
    IpcError::new(ErrorCode::Internal, e.to_string())
}

/// Where mail lands: the app's threads, or a fake in tests.
#[async_trait]
pub trait MailHost: Send + Sync {
    /// An idle chat of `teammate`'s in the thread of work `conversation_ref`
    /// that mail may run in (at most Edits).
    async fn idle_chat(&self, teammate: &Teammate, conversation_ref: &str) -> Option<ThreadId>;
    /// A fresh unattended chat for `teammate` (access at most Edits).
    async fn wake(
        &self,
        teammate: &Teammate,
        title: String,
        conversation_ref: &str,
    ) -> Result<ThreadId, IpcError>;
    /// Sends `text` to `chat` as a user turn that shows `row`.
    async fn deliver(
        &self,
        chat: &ThreadId,
        text: String,
        row: ThreadEvent,
    ) -> Result<(), IpcError>;
    /// Shows `row` in the sender's chat.
    async fn note(&self, chat: &ThreadId, row: ThreadEvent);
}

/// Who a mail is for.
#[derive(Debug, Clone)]
pub enum Recipient {
    Id(TeammateId),
    /// `tandem__send_message`'s `to`.
    Name(String),
}

#[derive(Debug, Clone)]
pub struct Outgoing {
    /// `None`: the user.
    pub from: Option<TeammateId>,
    pub from_thread: Option<ThreadId>,
    /// The sender's thread of work (its lease's), when known.
    pub conversation_ref: Option<String>,
    pub to: Recipient,
    pub body: String,
}

/// `mail:changed`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct MailChanged {
    pub teammate_id: TeammateId,
    pub rows: Vec<MailRow>,
}

/// `tandem__list_teammates`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct Reachable {
    pub id: TeammateId,
    pub name: String,
    pub engine: String,
    pub status: &'static str,
    pub may_receive: bool,
}

/// The turn a delivery sends: "From {sender}:" and the body. A teammate's
/// message is another agent's words, so it's labelled untrusted and fenced
/// (F-TL-06 rule 8); the user's own mail isn't.
pub fn delivery_text(from: &str, from_teammate: bool, body: &str) -> String {
    if !from_teammate {
        return format!("From {from}:\n{body}");
    }
    format!(
        "From {from}:\n{}\n{}",
        tandem_core::untrusted::label(&format!("{from}, another teammate")),
        tandem_core::untrusted::fence("", body)
    )
}

/// The reason in words, for the sender's tool error and the UI.
pub fn reason_text(reason: MailReason, sender: &str, recipient: &str) -> String {
    match reason {
        MailReason::NotPermitted => format!("{sender} isn't allowed to message teammates."),
        MailReason::TerminalOnly => {
            format!("{recipient} can't receive mail: it runs as a terminal.")
        }
        MailReason::Deleted => format!("{recipient} was deleted."),
        MailReason::RateLimit => "Rate limit reached".into(),
        MailReason::GlobalPause => "Mail is paused".into(),
        MailReason::WakeFailed => format!("{recipient} couldn't start a chat."),
    }
}

pub struct MailService {
    store: Store,
    bus: EventBus,
    teammates: Arc<TeammatesService>,
    host: Arc<dyn MailHost>,
    /// One delivery pass at a time.
    worker: tokio::sync::Mutex<()>,
}

impl MailService {
    pub fn new(
        store: Store,
        bus: EventBus,
        teammates: Arc<TeammatesService>,
        host: Arc<dyn MailHost>,
    ) -> Arc<Self> {
        Arc::new(Self {
            store,
            bus,
            teammates,
            host,
            worker: tokio::sync::Mutex::new(()),
        })
    }

    async fn paused(&self) -> bool {
        tandem_store::repo::settings::get(&self.store, SettingKey::PauseAllMail)
            .await
            .ok()
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
    }

    async fn recipient(&self, to: &Recipient) -> Result<(Teammate, bool), IpcError> {
        match to {
            Recipient::Id(id) => tandem_store::repo::teammates::get_any(&self.store, id)
                .await
                .map_err(store_err)?
                .ok_or_else(|| IpcError::new(ErrorCode::NotFound, "There's no such teammate.")),
            Recipient::Name(name) => {
                let want = fold_name(name.trim().trim_start_matches('@'));
                self.teammates
                    .teammates()
                    .await?
                    .into_iter()
                    .find(|t| fold_name(&t.name) == want)
                    .map(|t| (t, false))
                    .ok_or_else(|| {
                        IpcError::new(
                            ErrorCode::NotFound,
                            format!("There's no teammate called {}.", name.trim()),
                        )
                    })
            }
        }
    }

    fn row_event(mail: &Mail, inbound: bool) -> ThreadEvent {
        ThreadEvent::MailRow {
            mail_id: mail.id.clone(),
            from: mail.from.clone(),
            to: mail.to.clone(),
            preview: repo::preview(&mail.body),
            inbound,
        }
    }

    /// `mail_send` and `tandem__send_message`.
    pub async fn send(self: &Arc<Self>, out: Outgoing) -> Result<MailRow, IpcError> {
        let body = out.body.trim().to_string();
        if body.is_empty() {
            return Err(IpcError::new(ErrorCode::Invalid, "The message is empty."));
        }
        if body.chars().count() > MAX_BODY {
            return Err(IpcError::new(
                ErrorCode::Invalid,
                "A message can be at most 8,000 characters.",
            ));
        }
        let sender = match &out.from {
            Some(id) => Some(self.teammates.get(id).await?),
            None => None,
        };
        let (recipient, deleted) = self.recipient(&out.to).await?;
        if sender.as_ref().is_some_and(|s| s.id == recipient.id) {
            return Err(IpcError::new(
                ErrorCode::Invalid,
                "A teammate can't message itself.",
            ));
        }
        let refusal = if sender.as_ref().is_some_and(|s| !s.settings.may_message) {
            Some(MailReason::NotPermitted)
        } else if deleted {
            Some(MailReason::Deleted)
        } else if !self.teammates.chat_capable(&recipient.engine_id) {
            Some(MailReason::TerminalOnly)
        } else {
            None
        };
        let id = MailId::new();
        let conversation_ref = out
            .conversation_ref
            .clone()
            .or_else(|| out.from_thread.as_ref().map(|t| t.as_str().to_string()))
            .unwrap_or_else(|| id.as_str().to_string());
        let (state, reason) = match refusal {
            Some(r) => (MailState::Refused, Some(r)),
            None if self.paused().await => (MailState::Queued, Some(MailReason::GlobalPause)),
            None => (MailState::Queued, None),
        };
        let mail = repo::insert(
            &self.store,
            NewMail {
                id,
                from_teammate_id: out.from.clone(),
                to_teammate_id: recipient.id.clone(),
                from_thread_id: out.from_thread.clone(),
                body,
                conversation_ref,
                state,
                reason,
            },
        )
        .await
        .map_err(store_err)?;
        tracing::info!(code = "mail.sent", state = state.as_str(), "mail");
        if let Some(chat) = &out.from_thread {
            self.host.note(chat, Self::row_event(&mail, false)).await;
        }
        self.publish(&mail).await;
        if let Some(r) = refusal {
            return Err(IpcError::new(
                ErrorCode::Invalid,
                reason_text(r, &mail.from, &mail.to),
            ));
        }
        self.pump_soon();
        Ok(mail.row())
    }

    /// Delivers the queue in the background.
    pub fn pump_soon(self: &Arc<Self>) {
        let svc = self.clone();
        tauri::async_runtime::spawn(async move { svc.pump().await });
    }

    /// One delivery pass over the queue, oldest first.
    pub async fn pump(&self) {
        let _one = self.worker.lock().await;
        let queue = match repo::queued(&self.store).await {
            Ok(q) => q,
            Err(e) => {
                tracing::warn!(code = "mail.queue_failed", error = %e, "couldn't read the mail queue");
                return;
            }
        };
        let paused = self.paused().await;
        for mail in queue {
            if paused {
                self.hold(&mail, MailReason::GlobalPause).await;
                continue;
            }
            self.deliver(mail).await;
        }
    }

    /// Keeps a mail queued with `reason`.
    async fn hold(&self, mail: &Mail, reason: MailReason) {
        if mail.reason == Some(reason) {
            return;
        }
        let _ = repo::set_state(&self.store, &mail.id, MailState::Queued, Some(reason)).await;
        self.publish(mail).await;
    }

    async fn set(&self, mail: &Mail, state: MailState, reason: Option<MailReason>) {
        if let Err(e) = repo::set_state(&self.store, &mail.id, state, reason).await {
            tracing::warn!(code = "mail.state_failed", error = %e, "couldn't update a mail");
        }
        self.publish(mail).await;
    }

    async fn deliver(&self, mail: Mail) {
        let recipient =
            match tandem_store::repo::teammates::get_any(&self.store, &mail.to_teammate_id).await {
                Ok(Some((t, false))) => t,
                _ => {
                    return self
                        .set(&mail, MailState::Refused, Some(MailReason::Deleted))
                        .await
                }
            };
        if !self.teammates.chat_capable(&recipient.engine_id) {
            return self
                .set(&mail, MailState::Refused, Some(MailReason::TerminalOnly))
                .await;
        }
        let now = tandem_store::now_ms();
        let recent = repo::turns_since(&self.store, &recipient.id, now - WINDOW_MS)
            .await
            .unwrap_or(0);
        if recent >= RATE_LIMIT {
            tracing::info!(
                code = "mail.rate_limited",
                "mail waits for the hourly limit"
            );
            return self.hold(&mail, MailReason::RateLimit).await;
        }
        let chat = match self
            .host
            .idle_chat(&recipient, &mail.conversation_ref)
            .await
        {
            Some(c) => c,
            None => {
                let title = format!("{}: {}", mail.from, repo::preview(&mail.body));
                match self
                    .host
                    .wake(&recipient, title, &mail.conversation_ref)
                    .await
                {
                    Ok(c) => c,
                    Err(e) => {
                        tracing::warn!(code = "mail.wake_failed", error = %e.message, "couldn't wake a teammate");
                        return self
                            .set(&mail, MailState::Failed, Some(MailReason::WakeFailed))
                            .await;
                    }
                }
            }
        };
        match repo::claim(&self.store, &mail.id, &chat).await {
            Ok(true) => {}
            Ok(false) => return,
            Err(e) => {
                tracing::warn!(code = "mail.claim_failed", error = %e, "couldn't claim a mail");
                return;
            }
        }
        if let Err(e) = repo::log_turn(&self.store, &recipient.id, now).await {
            tracing::warn!(code = "mail.log_failed", error = %e, "couldn't log a mail turn");
        }
        let text = delivery_text(&mail.from, mail.from_teammate_id.is_some(), &mail.body);
        if let Err(e) = self
            .host
            .deliver(&chat, text, Self::row_event(&mail, true))
            .await
        {
            tracing::warn!(code = "mail.deliver_failed", error = %e.message, "couldn't deliver a mail");
            return self
                .set(&mail, MailState::Failed, Some(MailReason::WakeFailed))
                .await;
        }
        tracing::info!(code = "mail.delivered", "mail delivered");
        self.publish(&mail).await;
    }

    /// `mail_retry`: a failed or waiting mail goes back in the queue.
    pub async fn retry(self: &Arc<Self>, id: &MailId) -> Result<(), IpcError> {
        let mail = self.get(id).await?;
        match mail.state {
            MailState::Failed | MailState::Queued => {}
            _ => {
                return Err(IpcError::new(
                    ErrorCode::Conflict,
                    "Only queued or failed mail can be retried.",
                ))
            }
        }
        repo::set_state(&self.store, id, MailState::Queued, None)
            .await
            .map_err(store_err)?;
        self.pump().await;
        Ok(())
    }

    pub async fn get(&self, id: &MailId) -> Result<Mail, IpcError> {
        repo::get(&self.store, id)
            .await
            .map_err(store_err)?
            .ok_or_else(|| IpcError::new(ErrorCode::NotFound, "That message is gone."))
    }

    /// `mail_list`: a teammate's mail, newest first.
    pub async fn list(&self, teammate: &TeammateId) -> Result<Vec<MailRow>, IpcError> {
        Ok(repo::list_for(&self.store, teammate, 200)
            .await
            .map_err(store_err)?
            .iter()
            .map(Mail::row)
            .collect())
    }

    /// `tandem__list_teammates`: everyone else, and whether they can take mail.
    pub async fn reachable(&self, asking: &TeammateId) -> Result<Vec<Reachable>, IpcError> {
        Ok(self
            .teammates
            .list()
            .await?
            .into_iter()
            .filter(|s| &s.teammate.id != asking)
            .map(|s| Reachable {
                status: if s.waiting {
                    "waiting"
                } else if s.working > 0 {
                    "working"
                } else {
                    "idle"
                },
                may_receive: s.chat_capable,
                id: s.teammate.id,
                name: s.teammate.name,
                engine: s.teammate.engine_id,
            })
            .collect())
    }

    /// `mail:changed` for the mail's sender and recipient.
    async fn publish(&self, mail: &Mail) {
        let mut who = vec![mail.to_teammate_id.clone()];
        if let Some(f) = &mail.from_teammate_id {
            who.push(f.clone());
        }
        for teammate_id in who {
            let Ok(rows) = self.list(&teammate_id).await else {
                continue;
            };
            self.bus.publish(BusEvent::new(
                Topic::Threads,
                "mail:changed",
                MailChanged { teammate_id, rows },
            ));
        }
    }
}

/// Mail runs unattended: never with Full access (F-AG-12, F-TH-09).
pub fn mail_access(default: Access) -> Access {
    match default {
        Access::Full => Access::Edits,
        a => a,
    }
}

/// The app's threads.
pub struct AppMailHost {
    pub store: Store,
    pub threads: Arc<ThreadService>,
    pub teammates: Arc<TeammatesService>,
    /// Files each mail run with the gateway, so what it holds for review says
    /// which mail it came from (F-PL-08).
    pub gateway: Arc<crate::services::gateway::GatewayService>,
}

/// `mail:<mail-id>`: the run a delivered mail starts, as held drafts name it.
pub fn mail_origin(row: &ThreadEvent) -> Option<String> {
    match row {
        ThreadEvent::MailRow { mail_id, .. } => Some(format!("mail:{}", mail_id.as_str())),
        _ => None,
    }
}

#[async_trait]
impl MailHost for AppMailHost {
    async fn idle_chat(&self, teammate: &Teammate, conversation_ref: &str) -> Option<ThreadId> {
        let chats = tandem_store::repo::threads::teammate_chats(&self.store, &teammate.id, false)
            .await
            .ok()?;
        chats
            .into_iter()
            .find(|c| {
                (c.conversation_ref.as_deref() == Some(conversation_ref)
                    || c.id.as_str() == conversation_ref)
                    && !matches!(c.state, ThreadState::Working | ThreadState::Waiting)
                    && c.access != Access::Full
            })
            .map(|c| c.id)
    }

    async fn wake(
        &self,
        teammate: &Teammate,
        title: String,
        conversation_ref: &str,
    ) -> Result<ThreadId, IpcError> {
        let cwd = TeammatesService::place_for(teammate, None)?;
        std::fs::create_dir_all(&cwd).map_err(|e| IpcError::new(ErrorCode::Io, e.to_string()))?;
        let thread = self
            .threads
            .create_chat(
                NewThread {
                    id: ThreadId::new(),
                    kind: ThreadKind::TeammateChat,
                    workspace_id: None,
                    cwd,
                    engine_id: teammate.engine_id.clone(),
                    account_id: teammate.preferred_account_id.clone(),
                    model: teammate.model.clone(),
                    effort: teammate.effort.clone(),
                    access: mail_access(teammate.settings.default_access),
                    mode: ThreadMode::Act,
                    attended: false,
                },
                ChatExtra {
                    teammate_id: Some(teammate.id.clone()),
                    conversation_ref: Some(conversation_ref.to_string()),
                    title: Some((title, "mail")),
                },
            )
            .await?;
        self.teammates.publish().await;
        Ok(thread.id)
    }

    async fn deliver(
        &self,
        chat: &ThreadId,
        text: String,
        row: ThreadEvent,
    ) -> Result<(), IpcError> {
        // An unattended chat may carry several mails: the turn this one
        // starts is filed under it. An attended chat asks instead of holding.
        if let Some(origin) = mail_origin(&row) {
            self.gateway.set_unattended_origin(chat, &origin);
        }
        self.threads.send_mail(chat, text, row).await.map(|_| ())
    }

    async fn note(&self, chat: &ThreadId, row: ThreadEvent) {
        if let Err(e) = self.threads.note_mail(chat, row).await {
            tracing::warn!(code = "mail.note_failed", error = %e.message, "couldn't show a mail row");
        }
    }
}

/// Test helpers other services' tests share.
#[cfg(test)]
pub(crate) mod tests_support {
    use super::*;
    use std::sync::Mutex;

    #[derive(Default)]
    pub struct Host {
        pub idle: Mutex<Option<(String, ThreadId)>>,
        pub fail_wake: std::sync::atomic::AtomicBool,
        pub wakes: Mutex<Vec<(TeammateId, String, String)>>,
        pub delivered: Mutex<Vec<(ThreadId, String, ThreadEvent)>>,
        pub notes: Mutex<Vec<(ThreadId, ThreadEvent)>>,
    }

    #[async_trait]
    impl MailHost for Host {
        async fn idle_chat(&self, _t: &Teammate, conversation_ref: &str) -> Option<ThreadId> {
            let idle = self.idle.lock().expect("idle");
            idle.as_ref()
                .filter(|(r, _)| r == conversation_ref)
                .map(|(_, c)| c.clone())
        }
        async fn wake(&self, t: &Teammate, title: String, r: &str) -> Result<ThreadId, IpcError> {
            if self.fail_wake.load(std::sync::atomic::Ordering::SeqCst) {
                return Err(IpcError::new(ErrorCode::EngineMissing, "no engine"));
            }
            self.wakes
                .lock()
                .expect("wakes")
                .push((t.id.clone(), title, r.to_string()));
            Ok(ThreadId::new())
        }
        async fn deliver(
            &self,
            chat: &ThreadId,
            text: String,
            row: ThreadEvent,
        ) -> Result<(), IpcError> {
            self.delivered
                .lock()
                .expect("delivered")
                .push((chat.clone(), text, row));
            Ok(())
        }
        async fn note(&self, chat: &ThreadId, row: ThreadEvent) {
            self.notes.lock().expect("notes").push((chat.clone(), row));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::tests_support::Host;

    #[test]
    fn f_pl_08_a_mail_run_is_filed_under_its_mail() {
        let dir = tempfile::tempdir().expect("dir");
        let data = tandem_core::paths::DataDir::at(dir.path().to_path_buf()).expect("data");
        let store = Store::open(&data).expect("store");
        let gateway = crate::services::gateway::GatewayService::new(store, data.run());
        let mail = MailId::new();
        let row = ThreadEvent::MailRow {
            mail_id: mail.clone(),
            from: "Ada".into(),
            to: "Bea".into(),
            preview: "hi".into(),
            inbound: true,
        };
        let origin = mail_origin(&row).expect("a mail row names its run");
        let chat = ThreadId::new();
        gateway.set_unattended_origin(&chat, &origin);
        assert_eq!(gateway.unattended_origin(&chat), Some(origin.clone()));
        // Held drafts read it back as a mail run with that id.
        assert_eq!(
            crate::services::plugins::parse_origin(&origin),
            Some((
                tandem_store::repo::held_drafts::OriginKind::Mail,
                mail.as_str().to_string(),
                None
            ))
        );
        assert_eq!(
            mail_origin(&ThreadEvent::Assistant {
                markdown: "hi".into()
            }),
            None
        );
    }
    use super::*;
    use crate::services::teammates::tests_support::{input, service};

    async fn setup() -> (
        tempfile::TempDir,
        Arc<TeammatesService>,
        Arc<Host>,
        Arc<MailService>,
        Teammate,
        Teammate,
    ) {
        let (d, teammates) = service(None).await;
        let ada = teammates.create(input("Ada", "claude")).await.expect("ada");
        let bea = teammates.create(input("Bea", "claude")).await.expect("bea");
        let host = Arc::new(Host::default());
        let mail = MailService::new(
            teammates.store().clone(),
            EventBus::new(),
            teammates.clone(),
            host.clone(),
        );
        (d, teammates, host, mail, ada, bea)
    }

    fn from(ada: &Teammate, chat: &ThreadId, to: &str, body: &str) -> Outgoing {
        Outgoing {
            from: Some(ada.id.clone()),
            from_thread: Some(chat.clone()),
            conversation_ref: None,
            to: Recipient::Name(to.into()),
            body: body.into(),
        }
    }

    async fn set_pause(teammates: &TeammatesService, on: bool) {
        tandem_store::repo::settings::set(
            teammates.store(),
            SettingKey::PauseAllMail,
            serde_json::json!(on),
        )
        .await
        .expect("pause");
    }

    #[tokio::test]
    async fn f_ag_12_1_a_to_b_wakes_b_in_a_fresh_chat_with_rows_in_both_transcripts() {
        let (_d, _t, host, mail, ada, bea) = setup().await;
        let ada_chat = ThreadId::new();
        let row = mail
            .send(from(
                &ada,
                &ada_chat,
                "bea",
                "Can you check CI on main?\nIt's red.",
            ))
            .await
            .expect("send");
        mail.pump().await;
        let wakes = host.wakes.lock().expect("w").clone();
        assert_eq!(wakes.len(), 1);
        assert_eq!(wakes[0].0, bea.id);
        assert_eq!(wakes[0].1, "Ada: Can you check CI on main?");
        assert_eq!(wakes[0].2, ada_chat.as_str());
        let delivered = host.delivered.lock().expect("d").clone();
        assert_eq!(delivered.len(), 1);
        assert_eq!(
            delivered[0].1,
            delivery_text("Ada", true, "Can you check CI on main?\nIt's red.")
        );
        assert!(delivered[0]
            .1
            .contains("Untrusted content from Ada, another teammate"));
        assert!(delivered[0]
            .1
            .ends_with("```\nCan you check CI on main?\nIt's red.\n```"));
        assert!(
            matches!(&delivered[0].2, ThreadEvent::MailRow { inbound: true, from, to, .. } if from == "Ada" && to == "Bea")
        );
        let notes = host.notes.lock().expect("n").clone();
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].0, ada_chat);
        assert!(
            matches!(&notes[0].1, ThreadEvent::MailRow { inbound: false, preview, .. } if preview == "Can you check CI on main?")
        );
        let got = mail.get(&row.id).await.expect("get");
        assert_eq!(got.state, MailState::Delivered);
        assert_eq!(got.delivered_thread_id.as_ref(), Some(&delivered[0].0));
        // Both teammates list it.
        assert_eq!(mail.list(&ada.id).await.expect("a").len(), 1);
        assert_eq!(mail.list(&bea.id).await.expect("b").len(), 1);
    }

    #[test]
    fn f_ag_12_4_a_mail_run_never_gets_full_access() {
        assert_eq!(mail_access(Access::Full), Access::Edits);
        assert_eq!(mail_access(Access::Edits), Access::Edits);
        assert_eq!(mail_access(Access::Read), Access::Read);
    }

    #[tokio::test]
    async fn f_ag_12_an_idle_chat_in_the_same_thread_of_work_gets_the_mail() {
        let (_d, _t, host, mail, ada, _bea) = setup().await;
        let ada_chat = ThreadId::new();
        let bea_chat = ThreadId::new();
        *host.idle.lock().expect("idle") = Some((ada_chat.as_str().to_string(), bea_chat.clone()));
        mail.send(from(&ada, &ada_chat, "Bea", "And the flaky test?"))
            .await
            .expect("send");
        mail.pump().await;
        assert!(host.wakes.lock().expect("w").is_empty());
        assert_eq!(host.delivered.lock().expect("d")[0].0, bea_chat);
    }

    #[tokio::test]
    async fn f_ag_12_2_with_the_global_pause_on_mail_is_queued_and_on_resume_delivered_once() {
        let (_d, teammates, host, mail, ada, _bea) = setup().await;
        set_pause(&teammates, true).await;
        let row = mail
            .send(from(&ada, &ThreadId::new(), "Bea", "hello"))
            .await
            .expect("send");
        assert_eq!(row.state, MailState::Queued);
        assert_eq!(row.reason, Some(MailReason::GlobalPause));
        mail.pump().await;
        assert!(host.delivered.lock().expect("d").is_empty());
        set_pause(&teammates, false).await;
        mail.pump().await;
        mail.pump().await;
        assert_eq!(host.delivered.lock().expect("d").len(), 1);
        assert_eq!(
            mail.get(&row.id).await.expect("get").state,
            MailState::Delivered
        );
    }

    #[tokio::test]
    async fn f_ag_12_3_the_21st_mail_triggered_turn_in_an_hour_is_queued_not_run() {
        let (_d, teammates, host, mail, ada, bea) = setup().await;
        let now = tandem_store::now_ms();
        for i in 0..20 {
            repo::log_turn(teammates.store(), &bea.id, now - i * 60_000)
                .await
                .expect("log");
        }
        let row = mail
            .send(from(&ada, &ThreadId::new(), "Bea", "one more"))
            .await
            .expect("send");
        mail.pump().await;
        assert!(host.delivered.lock().expect("d").is_empty());
        let got = mail.get(&row.id).await.expect("get");
        assert_eq!(got.state, MailState::Queued);
        assert_eq!(got.reason, Some(MailReason::RateLimit));
        assert_eq!(
            reason_text(MailReason::RateLimit, "Ada", "Bea"),
            "Rate limit reached"
        );
    }

    #[tokio::test]
    async fn f_ag_12_refusals_name_the_reason_and_leave_a_refused_row() {
        let (_d, teammates, host, mail, ada, bea) = setup().await;
        // Terminal-only recipient.
        let term = teammates
            .create(input("Tess", "aider"))
            .await
            .expect("tess");
        let e = mail
            .send(from(&ada, &ThreadId::new(), "Tess", "hi"))
            .await
            .expect_err("terminal");
        assert!(e.message.contains("runs as a terminal"), "{}", e.message);
        // Deleted recipient (by id: the UI's mail_send).
        teammates.delete(&bea.id).await.expect("delete");
        let e = mail
            .send(Outgoing {
                to: Recipient::Id(bea.id.clone()),
                ..from(&ada, &ThreadId::new(), "", "hi")
            })
            .await
            .expect_err("deleted");
        assert!(e.message.contains("was deleted"), "{}", e.message);
        // A sender that may not message.
        let mut settings = ada.settings;
        settings.may_message = false;
        teammates
            .update(
                &ada.id,
                crate::services::teammates::TeammatePatch {
                    settings: Some(settings),
                    ..Default::default()
                },
            )
            .await
            .expect("update");
        let e = mail
            .send(from(&ada, &ThreadId::new(), "Tess", "hi"))
            .await
            .expect_err("not permitted");
        assert!(e.message.contains("isn't allowed"), "{}", e.message);
        // No such teammate: an error, no row.
        let e = mail
            .send(from(&ada, &ThreadId::new(), "Nobody", "hi"))
            .await
            .expect_err("unknown");
        assert_eq!(e.code, ErrorCode::NotFound);
        let rows = mail.list(&ada.id).await.expect("rows");
        assert_eq!(rows.len(), 3);
        assert!(rows.iter().all(|r| r.state == MailState::Refused));
        let _ = term;
        mail.pump().await;
        assert!(host.delivered.lock().expect("d").is_empty());
    }

    #[tokio::test]
    async fn f_ag_12_a_failed_wake_waits_for_retry_and_is_never_retried_in_a_loop() {
        let (_d, _t, host, mail, ada, _bea) = setup().await;
        host.fail_wake
            .store(true, std::sync::atomic::Ordering::SeqCst);
        let row = mail
            .send(from(&ada, &ThreadId::new(), "Bea", "wake up"))
            .await
            .expect("send");
        mail.pump().await;
        mail.pump().await;
        let got = mail.get(&row.id).await.expect("get");
        assert_eq!(got.state, MailState::Failed);
        assert_eq!(got.reason, Some(MailReason::WakeFailed));
        assert!(host.delivered.lock().expect("d").is_empty());
        host.fail_wake
            .store(false, std::sync::atomic::Ordering::SeqCst);
        mail.pump().await;
        assert!(host.delivered.lock().expect("d").is_empty());
        mail.retry(&row.id).await.expect("retry");
        assert_eq!(host.delivered.lock().expect("d").len(), 1);
        assert!(mail.retry(&row.id).await.is_err());
    }
}
