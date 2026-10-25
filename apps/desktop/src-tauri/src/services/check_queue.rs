//! One check at a time (F-EX-06): while `checks_one_at_a_time` is on, check
//! commands (Best of N's Run check, the review queue's checks) wait for
//! one turn, first come first served. Dropping a waiting check (Stop) gives
//! up its place.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock};

use tokio::sync::{OwnedSemaphorePermit, Semaphore};

/// The setting and the one turn.
pub struct CheckQueue {
    on: AtomicBool,
    turn: Arc<Semaphore>,
}

impl Default for CheckQueue {
    fn default() -> Self {
        Self {
            on: AtomicBool::new(false),
            turn: Arc::new(Semaphore::new(1)),
        }
    }
}

impl CheckQueue {
    pub fn set(&self, on: bool) {
        self.on.store(on, Ordering::Release);
    }

    pub fn is_on(&self) -> bool {
        self.on.load(Ordering::Acquire)
    }

    /// Waits for this check's turn while the setting is on; the turn lasts
    /// as long as the permit. `None`: checks run at once.
    pub async fn turn(&self) -> Option<OwnedSemaphorePermit> {
        if !self.is_on() {
            return None;
        }
        self.turn.clone().acquire_owned().await.ok()
    }
}

/// The app's queue: every check command takes its turn here.
static APP: LazyLock<CheckQueue> = LazyLock::new(CheckQueue::default);

/// The setting's value (at startup and when it changes).
pub fn set(on: bool) {
    APP.set(on);
}

pub fn is_on() -> bool {
    APP.is_on()
}

/// [`CheckQueue::turn`] on the app's queue.
pub async fn turn() -> Option<OwnedSemaphorePermit> {
    APP.turn().await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;
    use std::time::Duration;

    #[tokio::test]
    async fn f_ex_06_5_checks_take_turns_only_while_the_setting_is_on() {
        // Its own queue: other tests' checks never wait on this one's.
        let q = Arc::new(CheckQueue::default());
        let three = |q: Arc<CheckQueue>| async move {
            let live = Arc::new(AtomicUsize::new(0));
            let most = Arc::new(AtomicUsize::new(0));
            let tasks: Vec<_> = (0..3)
                .map(|_| {
                    let (live, most, q) = (live.clone(), most.clone(), q.clone());
                    tokio::spawn(async move {
                        let _turn = q.turn().await;
                        let now = live.fetch_add(1, Ordering::SeqCst) + 1;
                        most.fetch_max(now, Ordering::SeqCst);
                        tokio::time::sleep(Duration::from_millis(30)).await;
                        live.fetch_sub(1, Ordering::SeqCst);
                    })
                })
                .collect();
            for t in tasks {
                t.await.expect("task");
            }
            most.load(Ordering::SeqCst)
        };
        q.set(true);
        assert_eq!(three(q.clone()).await, 1);
        // A check dropped while waiting gives its place up.
        let held = q.turn().await.expect("turn");
        let waiting = {
            let q = q.clone();
            tokio::spawn(async move { q.turn().await })
        };
        tokio::time::sleep(Duration::from_millis(10)).await;
        waiting.abort();
        drop(held);
        assert!(tokio::time::timeout(Duration::from_secs(1), q.turn())
            .await
            .expect("not stuck")
            .is_some());
        q.set(false);
        assert_eq!(three(q.clone()).await, 3);
    }
}
