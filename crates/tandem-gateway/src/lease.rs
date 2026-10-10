//! Leases (ARCHITECTURE §11.2): one per engine session. The token is 32
//! random bytes, base64url; only its SHA-256 is kept, and a revoked lease's
//! MCP sessions go with it.

use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard};

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use rand::RngCore;
use sha2::{Digest, Sha256};
use tandem_core::access::Access;
use tandem_core::ids::{ConnectionId, LeaseId, PaneId, TeammateId, ThreadId};

/// Who the session belongs to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Caller {
    Thread {
        thread_id: ThreadId,
    },
    TeammateChat {
        teammate_id: TeammateId,
        thread_id: ThreadId,
    },
    Pane {
        pane_id: PaneId,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Attended {
    Interactive,
    /// A routine, task, mail or Auto run.
    Unattended {
        origin: String,
    },
}

impl Caller {
    /// The teammate a session works for, if any.
    pub fn teammate(&self) -> Option<&TeammateId> {
        match self {
            Caller::TeammateChat { teammate_id, .. } => Some(teammate_id),
            _ => None,
        }
    }

    /// The thread or teammate chat a session belongs to, if any.
    pub fn thread(&self) -> Option<&ThreadId> {
        match self {
            Caller::Thread { thread_id } | Caller::TeammateChat { thread_id, .. } => {
                Some(thread_id)
            }
            Caller::Pane { .. } => None,
        }
    }
}

/// The built-in tools a session sees (§11.2): every Claude session gets the
/// approvals tool; teammate chats get memory, skills and session search,
/// plus mail when the teammate may message and the schedule tools when it
/// may create routines.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BuiltinSet {
    pub approvals: bool,
    pub session_search: bool,
    pub memory_write: bool,
    pub read_skill: bool,
    /// `tandem__send_message` and `tandem__list_teammates` (F-AG-12).
    pub mail: bool,
    /// `tandem__schedule_*` (F-RT-05, M11).
    pub schedule: bool,
}

impl BuiltinSet {
    pub fn for_thread() -> Self {
        Self {
            approvals: true,
            ..Self::default()
        }
    }

    /// A teammate chat's tools (F-AG-05: "May message teammates" and "May
    /// create routines" decide mail and schedules).
    pub fn for_teammate(may_message: bool, may_schedule: bool) -> Self {
        Self {
            approvals: false,
            session_search: true,
            memory_write: true,
            read_skill: true,
            mail: may_message,
            schedule: may_schedule,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionDescriptor {
    pub caller: Caller,
    pub attended: Attended,
    pub access: Access,
    /// The plugin connections this session may use (F-PL-05 grants, taken
    /// when the session starts).
    pub granted: Vec<ConnectionId>,
    pub builtins: BuiltinSet,
    /// The thread of work the session belongs to (mail threading, remember
    /// scope).
    pub conversation_ref: String,
}

impl SessionDescriptor {
    /// A session with nothing but its caller, access and built-ins.
    pub fn new(caller: Caller, attended: Attended, access: Access, builtins: BuiltinSet) -> Self {
        let conversation_ref = caller
            .thread()
            .map(|t| t.as_str().to_string())
            .unwrap_or_default();
        Self {
            caller,
            attended,
            access,
            granted: Vec::new(),
            builtins,
            conversation_ref,
        }
    }
}

/// A new lease. The token is shown once, for the session's MCP config.
pub struct Lease {
    pub id: LeaseId,
    pub token: String,
}

impl std::fmt::Debug for Lease {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Lease")
            .field("id", &self.id)
            .field("token", &"<redacted>")
            .finish()
    }
}

/// What a request's lease stands for (attached to the request by the guard).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LeaseContext {
    pub id: LeaseId,
    pub session: SessionDescriptor,
}

fn hash(token: &str) -> [u8; 32] {
    Sha256::digest(token.as_bytes()).into()
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    match m.lock() {
        Ok(g) => g,
        Err(poisoned) => poisoned.into_inner(),
    }
}

#[derive(Default)]
pub(crate) struct Leases {
    by_hash: Mutex<HashMap<[u8; 32], LeaseContext>>,
    /// MCP session id → the lease that opened it.
    sessions: Mutex<HashMap<String, LeaseId>>,
}

impl Leases {
    pub(crate) fn issue(&self, session: SessionDescriptor) -> Lease {
        let mut bytes = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut bytes);
        let token = URL_SAFE_NO_PAD.encode(bytes);
        let id = LeaseId::new();
        lock(&self.by_hash).insert(
            hash(&token),
            LeaseContext {
                id: id.clone(),
                session,
            },
        );
        Lease { id, token }
    }

    pub(crate) fn find(&self, token: &str) -> Option<LeaseContext> {
        lock(&self.by_hash).get(&hash(token)).cloned()
    }

    pub(crate) fn exists(&self, id: &LeaseId) -> bool {
        lock(&self.by_hash).values().any(|ctx| &ctx.id == id)
    }

    pub(crate) fn count(&self) -> usize {
        lock(&self.by_hash).len()
    }

    pub(crate) fn session_count(&self) -> usize {
        lock(&self.sessions).len()
    }

    pub(crate) fn revoke(&self, id: &LeaseId) {
        lock(&self.by_hash).retain(|_, ctx| &ctx.id != id);
        lock(&self.sessions).retain(|_, owner| owner != id);
    }

    pub(crate) fn session_owner(&self, session: &str) -> Option<LeaseId> {
        lock(&self.sessions).get(session).cloned()
    }

    pub(crate) fn bind_session(&self, session: &str, id: &LeaseId) {
        lock(&self.sessions).insert(session.to_string(), id.clone());
    }
}
