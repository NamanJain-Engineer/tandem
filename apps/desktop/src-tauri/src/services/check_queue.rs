//! One check at a time (F-EX-06): while `checks_one_at_a_time` is on, check
//! commands (Best of N's Run check, the review queue's checks) wait for
//! one turn, first come first served. Dropping a waiting check (Stop) gives
//! up its place.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock};

use tokio::sync::{OwnedSemaphorePermit, Semaphore};

static ON: AtomicBool = AtomicBool::new(false);
static TURN: LazyLock<Arc<Semaphore>> = LazyLock::new(|| Arc::new(Semaphore::new(1)));

/// The setting's value (at startup and when it changes).
pub fn set(on: bool) {
    ON.store(on, Ordering::Release);
}

pub fn is_on() -> bool {
    ON.load(Ordering::Acquire)
}

/// Waits for this check's turn while the setting is on; the turn lasts as
/// long as the permit. `None`: checks run at once.
pub async fn turn() -> Option<OwnedSemaphorePermit> {
    if !is_on() {
        return None;
    }
    TURN.clone().acquire_owned().await.ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;
    use std::time::Duration;

    #[tokio::test]
    async fn f_ex_06_5_checks_take_turns_only_while_the_setting_is_on() {
        async fn three() -> usize {
            let live = Arc::new(AtomicUsize::new(0));
            let most = Arc::new(AtomicUsize::new(0));
            let tasks: Vec<_> = (0..3)
                .map(|_| {
                    let (live, most) = (live.clone(), most.clone());
                    tokio::spawn(async move {
                        let _turn = turn().await;
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
        }
        set(true);
        assert_eq!(three().await, 1);
        // A check dropped while waiting gives its place up.
        let held = turn().await.expect("turn");
        let waiting = tokio::spawn(turn());
        tokio::time::sleep(Duration::from_millis(10)).await;
        waiting.abort();
        drop(held);
        assert!(tokio::time::timeout(Duration::from_secs(1), turn())
            .await
            .expect("not stuck")
            .is_some());
        set(false);
        assert_eq!(three().await, 3);
    }
}
