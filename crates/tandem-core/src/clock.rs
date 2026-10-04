use std::sync::Mutex;

use chrono::{DateTime, Duration, Utc};

pub trait Clock: Send + Sync {
    fn now(&self) -> DateTime<Utc>;
}

#[derive(Debug, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> DateTime<Utc> {
        Utc::now()
    }
}

#[derive(Debug)]
pub struct FakeClock {
    instant: Mutex<DateTime<Utc>>,
}

impl FakeClock {
    pub fn new(start: DateTime<Utc>) -> Self {
        Self {
            instant: Mutex::new(start),
        }
    }

    pub fn advance(&self, by: Duration) {
        let mut guard = self.instant.lock().unwrap_or_else(|err| err.into_inner());
        *guard += by;
    }
}

impl Clock for FakeClock {
    fn now(&self) -> DateTime<Utc> {
        *self.instant.lock().unwrap_or_else(|err| err.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fake_clock_advances() {
        let start = DateTime::parse_from_rfc3339("2026-01-01T00:00:00Z")
            .expect("rfc3339")
            .with_timezone(&Utc);
        let clock = FakeClock::new(start);
        assert_eq!(clock.now(), start);
        clock.advance(Duration::seconds(30));
        assert_eq!(clock.now(), start + Duration::seconds(30));
    }
}
