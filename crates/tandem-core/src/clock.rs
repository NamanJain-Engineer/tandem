//! Injectable time. Every timer-driven behaviour takes `Arc<dyn Clock>` so tests
//! can drive it with [`MockClock`].

use chrono::{DateTime, Duration as ChronoDuration, Utc};
use futures::future::BoxFuture;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::watch;

pub trait Clock: Send + Sync {
    fn now_utc(&self) -> DateTime<Utc>;
    fn sleep(&self, d: Duration) -> BoxFuture<'static, ()>;

    fn now_ms(&self) -> i64 {
        self.now_utc().timestamp_millis()
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now_utc(&self) -> DateTime<Utc> {
        Utc::now()
    }

    fn sleep(&self, d: Duration) -> BoxFuture<'static, ()> {
        Box::pin(tokio::time::sleep(d))
    }
}

/// A clock that only moves when told to. Sleepers wake when `advance` or
/// `set` moves time past their deadline.
#[derive(Clone)]
pub struct MockClock {
    tx: Arc<watch::Sender<DateTime<Utc>>>,
}

impl MockClock {
    pub fn new(start: DateTime<Utc>) -> Self {
        let (tx, _rx) = watch::channel(start);
        Self { tx: Arc::new(tx) }
    }

    /// Honours `TANDEM_FAKE_CLOCK=<rfc3339>` (debug builds and tests).
    pub fn from_env() -> Option<Self> {
        let raw = std::env::var("TANDEM_FAKE_CLOCK").ok()?;
        let start = DateTime::parse_from_rfc3339(&raw).ok()?.with_timezone(&Utc);
        Some(Self::new(start))
    }

    pub fn advance(&self, d: Duration) {
        let step = ChronoDuration::from_std(d).unwrap_or(ChronoDuration::MAX);
        self.tx.send_modify(|now| *now += step);
    }

    pub fn set(&self, t: DateTime<Utc>) {
        self.tx.send_replace(t);
    }
}

impl Clock for MockClock {
    fn now_utc(&self) -> DateTime<Utc> {
        *self.tx.borrow()
    }

    fn sleep(&self, d: Duration) -> BoxFuture<'static, ()> {
        let mut rx = self.tx.subscribe();
        let deadline = *rx.borrow() + ChronoDuration::from_std(d).unwrap_or(ChronoDuration::MAX);
        Box::pin(async move {
            loop {
                if *rx.borrow_and_update() >= deadline {
                    return;
                }
                if rx.changed().await.is_err() {
                    return;
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[tokio::test]
    async fn mock_clock_wakes_sleepers_on_advance() {
        let start = Utc
            .with_ymd_and_hms(2026, 1, 1, 9, 0, 0)
            .single()
            .expect("time");
        let clock = MockClock::new(start);
        let sleeper = {
            let clock = clock.clone();
            tokio::spawn(async move { clock.sleep(Duration::from_secs(60)).await })
        };
        tokio::task::yield_now().await;
        clock.advance(Duration::from_secs(30));
        tokio::task::yield_now().await;
        assert!(!sleeper.is_finished());
        clock.advance(Duration::from_secs(30));
        sleeper.await.expect("sleeper");
        assert_eq!(clock.now_utc(), start + ChronoDuration::seconds(60));
    }
}
