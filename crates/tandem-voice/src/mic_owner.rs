//! Who holds the microphone (F-DI-12, F-CN-01, F-CN-07). Dictate and a
//! Conductor call each claim it for as long as they record; the wake-phrase
//! listener never claims it. It watches this registry and lets go of the mic
//! whenever someone else holds it.

use std::sync::Arc;

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use tokio::sync::watch;
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum MicOwner {
    Dictate,
    Call,
}

/// The registry. Cheap to clone; every clone shares the state.
#[derive(Clone)]
pub struct MicOwners {
    inner: Arc<Mutex<Option<MicOwner>>>,
    tx: Arc<watch::Sender<Option<MicOwner>>>,
}

impl Default for MicOwners {
    fn default() -> Self {
        let (tx, _) = watch::channel(None);
        Self {
            inner: Arc::new(Mutex::new(None)),
            tx: Arc::new(tx),
        }
    }
}

impl MicOwners {
    pub fn new() -> Self {
        Self::default()
    }

    /// Takes the mic. Fails with the current owner while someone else holds
    /// it. The claim is released when the guard drops.
    pub fn claim(&self, owner: MicOwner) -> Result<MicClaim, MicOwner> {
        let mut cur = self.inner.lock();
        if let Some(held) = *cur {
            return Err(held);
        }
        *cur = Some(owner);
        self.tx.send_replace(Some(owner));
        Ok(MicClaim {
            owners: self.clone(),
            owner,
        })
    }

    pub fn current(&self) -> Option<MicOwner> {
        *self.inner.lock()
    }

    /// Changes to the owner, for listeners that yield the mic.
    pub fn watch(&self) -> watch::Receiver<Option<MicOwner>> {
        self.tx.subscribe()
    }

    fn release(&self, owner: MicOwner) {
        let mut cur = self.inner.lock();
        if *cur == Some(owner) {
            *cur = None;
            self.tx.send_replace(None);
        }
    }
}

/// Holds the mic until dropped.
pub struct MicClaim {
    owners: MicOwners,
    owner: MicOwner,
}

impl MicClaim {
    pub fn owner(&self) -> MicOwner {
        self.owner
    }
}

impl Drop for MicClaim {
    fn drop(&mut self) {
        self.owners.release(self.owner);
    }
}

impl std::fmt::Debug for MicClaim {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MicClaim")
            .field("owner", &self.owner)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f_cn_07_one_owner_at_a_time_and_release_on_drop() {
        let m = MicOwners::new();
        let rx = m.watch();
        let call = m.claim(MicOwner::Call).expect("test");
        assert_eq!(m.current(), Some(MicOwner::Call));
        assert_eq!(*rx.borrow(), Some(MicOwner::Call));
        assert_eq!(
            m.claim(MicOwner::Dictate).expect_err("test"),
            MicOwner::Call
        );
        drop(call);
        assert_eq!(m.current(), None);
        assert_eq!(*rx.borrow(), None);
        let d = m.clone().claim(MicOwner::Dictate).expect("test");
        assert_eq!(d.owner(), MicOwner::Dictate);
    }
}
