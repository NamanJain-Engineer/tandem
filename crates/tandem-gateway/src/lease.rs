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
use tandem_core::ids::{LeaseId, PaneId, TeammateId, ThreadId};

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

/// The built-in tools a session sees (§11.2). Threads get the approvals
/// tool; teammate and plugin built-ins join with their features.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BuiltinSet {
    pub approvals: bool,
}

impl BuiltinSet {
    pub fn for_thread() -> Self {
        Self { approvals: true }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionDescriptor {
    pub caller: Caller,
    pub attended: Attended,
    pub access: Access,
    pub builtins: BuiltinSet,
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
