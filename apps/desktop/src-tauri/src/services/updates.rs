//! Auto-update (F-SH-06, ARCHITECTURE §17.2).
//!
//! - Checks the signed feed at launch + 60 s, every 4 h, and on Check now;
//!   a newer release downloads in the background and is verified (minisign)
//!   before it's offered as "Restart to update". Never a lower version.
//! - Stages are reported as `updates:state`; a failure names its stage
//!   (download, verify, install). Updates never ask for a plan.
//! - deb installs are managed by the package manager: no checks.
//! - `x-tandem-min-version` above this version starts a 72 h grace, after
//!   which the unsupported-version overlay is modal (UI-131).
//!
//! The feed and installer sit behind [`UpdateBackend`]: Tauri's updater in
//! the app, a fake in tests.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use tandem_core::bus::{BusEvent, EventBus, Topic};
use tandem_core::clock::Clock;
use tandem_core::{ErrorCode, IpcError};

/// The first check after launch.
pub const FIRST_CHECK: Duration = Duration::from_secs(60);
/// Later checks.
pub const CHECK_EVERY: Duration = Duration::from_secs(4 * 60 * 60);
/// Terminals keep working this long under a minimum-version notice.
pub const MIN_VERSION_GRACE_MS: i64 = 72 * 60 * 60 * 1000;
pub const CURRENT: &str = env!("CARGO_PKG_VERSION");

/// Where an update failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum UpdateStage {
    Check,
    Download,
    Verify,
    Install,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateError {
    pub stage: UpdateStage,
    pub message: String,
}

impl UpdateError {
    pub fn new(stage: UpdateStage, message: impl Into<String>) -> Self {
        Self {
            stage,
            message: message.into(),
        }
    }
}

/// An offered release.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    pub version: String,
    pub notes: Option<String>,
}

#[async_trait]
pub trait UpdateBackend: Send + Sync {
    /// The release `channel` offers above `current`, if any.
    async fn check(&self, channel: &str) -> Result<Option<Release>, UpdateError>;
    /// Downloads the last checked release and verifies its signature.
    /// `progress(done, total)` as bytes arrive.
    async fn download(
        &self,
        progress: Box<dyn Fn(u64, Option<u64>) + Send + Sync>,
    ) -> Result<Vec<u8>, UpdateError>;
    /// Installs downloaded bytes (the app restarts afterwards).
    fn install(&self, bytes: &[u8]) -> Result<(), UpdateError>;
}

/// The minimum-version notice (UI-131).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct MinVersionNotice {
    pub min_version: String,
    /// Until then terminals keep working under a dismissable banner.
    pub grace_until: i64,
    /// The grace is over: the overlay is modal.
    pub modal: bool,
}

/// `updates:state` (UI-115, UI-130).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct UpdateState {
    /// `idle` | `checking` | `downloading` | `ready` | `error` | `managed` |
    /// `unavailable` (this build has no update key)
    pub stage: String,
    pub current: String,
    pub channel: String,
    /// The release being downloaded, ready or failed.
    pub version: Option<String>,
    pub notes: Option<String>,
    /// Download progress, 0–100.
    pub percent: Option<u8>,
    pub error_stage: Option<UpdateStage>,
    pub error: Option<String>,
    pub last_check: Option<i64>,
    /// The last check found nothing newer.
    pub up_to_date: bool,
    pub min_version: Option<MinVersionNotice>,
}

/// `$DATA/update-state.json`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Persisted {
    last_check: Option<i64>,
    staged_version: Option<String>,
    channel: Option<String>,
    #[serde(default)]
    skipped: Vec<String>,
    min_version: Option<String>,
    min_version_seen_at: Option<i64>,
}

/// `a` is a higher version than `b` (`1.2.10` > `1.2.9`; a pre-release is
/// below its release). Unparseable versions are never higher.
pub fn newer(a: &str, b: &str) -> bool {
    fn parse(v: &str) -> Option<(Vec<u64>, Option<String>)> {
        let v = v.trim().trim_start_matches('v');
        let (core, pre) = match v.split_once('-') {
            Some((c, p)) => (c, Some(p.to_string())),
            None => (v.split('+').next().unwrap_or(v), None),
        };
        let nums: Option<Vec<u64>> = core.split('.').map(|n| n.parse().ok()).collect();
        let nums = nums.filter(|n| !n.is_empty() && n.len() <= 4)?;
        Some((nums, pre))
    }
    let (Some((mut x, xp)), Some((mut y, yp))) = (parse(a), parse(b)) else {
        return false;
    };
    let len = x.len().max(y.len());
    x.resize(len, 0);
    y.resize(len, 0);
    match x.cmp(&y) {
        std::cmp::Ordering::Greater => true,
        std::cmp::Ordering::Less => false,
        std::cmp::Ordering::Equal => match (xp, yp) {
            (None, Some(_)) => true,
            (Some(p), Some(q)) => p > q,
            _ => false,
        },
    }
}

/// A deb install: the package manager owns updates (`/usr/share/doc/tandem`
/// exists and the app isn't an AppImage).
pub fn managed_by_package_manager(doc_dir: &Path, appimage: Option<&str>) -> bool {
    cfg!(target_os = "linux") && doc_dir.is_dir() && appimage.is_none_or(str::is_empty)
}

pub struct UpdateService {
    /// Set at startup when this build has an update key (and isn't a deb).
    backend: std::sync::OnceLock<Arc<dyn UpdateBackend>>,
    managed: bool,
    state: Mutex<UpdateState>,
    persisted: Mutex<Persisted>,
    staged: Mutex<Option<Vec<u8>>>,
    /// One check or download at a time.
    busy: tokio::sync::Mutex<()>,
    path: PathBuf,
    clock: Arc<dyn Clock>,
    bus: EventBus,
}

impl UpdateService {
    /// `managed` = a deb. Until [`set_backend`](Self::set_backend) the
    /// state is `unavailable` (no update key in this build).
    pub fn new(managed: bool, path: PathBuf, clock: Arc<dyn Clock>, bus: EventBus) -> Arc<Self> {
        let persisted: Persisted = std::fs::read(&path)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        let stage = if managed { "managed" } else { "unavailable" };
        let state = UpdateState {
            stage: stage.into(),
            current: CURRENT.into(),
            channel: persisted.channel.clone().unwrap_or_else(|| "stable".into()),
            version: None,
            notes: None,
            percent: None,
            error_stage: None,
            error: None,
            last_check: persisted.last_check,
            up_to_date: false,
            min_version: None,
        };
        let me = Arc::new(Self {
            backend: std::sync::OnceLock::new(),
            managed,
            state: Mutex::new(state),
            persisted: Mutex::new(persisted),
            staged: Mutex::new(None),
            busy: tokio::sync::Mutex::new(()),
            path,
            clock,
            bus,
        });
        me.refresh_min_version();
        me
    }

    /// The feed and installer (ignored for a deb install).
    pub fn set_backend(&self, backend: Arc<dyn UpdateBackend>) {
        if self.managed || self.backend.set(backend).is_err() {
            return;
        }
        self.set(|s| s.stage = "idle".into());
    }

    pub fn state(&self) -> UpdateState {
        self.refresh_min_version();
        self.state
            .lock()
            .map(|s| s.clone())
            .unwrap_or_else(|e| e.into_inner().clone())
    }

    fn persist(&self) {
        let Ok(p) = self.persisted.lock().map(|p| p.clone()) else {
            return;
        };
        if let Ok(body) = serde_json::to_vec_pretty(&p) {
            if let Err(err) = tandem_core::atomic::atomic_write(&self.path, &body, Some(0o600)) {
                tracing::warn!(code = "updates.state_write_failed", error = %err, "couldn't save the update state");
            }
        }
    }

    fn set(&self, f: impl FnOnce(&mut UpdateState)) {
        let snapshot = {
            let Ok(mut s) = self.state.lock() else { return };
            f(&mut s);
            s.clone()
        };
        self.bus
            .publish(BusEvent::new(Topic::Updates, "updates:state", snapshot));
    }

    pub fn set_channel(&self, channel: &str) -> Result<UpdateState, IpcError> {
        if channel != "stable" && channel != "beta" {
            return Err(IpcError::new(ErrorCode::Invalid, "Choose Stable or Beta"));
        }
        if let Ok(mut p) = self.persisted.lock() {
            p.channel = Some(channel.into());
        }
        self.persist();
        self.set(|s| s.channel = channel.into());
        Ok(self.state())
    }

    /// Check now (and the scheduled checks): a newer release downloads and
    /// is verified; the state ends `ready`, `idle` or `error`.
    pub async fn check_now(&self) -> UpdateState {
        let Some(backend) = self.backend.get().cloned() else {
            return self.state();
        };
        // A second Check now waits for the one in flight.
        let _one = self.busy.lock().await;
        if self.state().stage == "ready" {
            return self.state();
        }
        let channel = self.state().channel;
        self.set(|s| {
            s.stage = "checking".into();
            s.error = None;
            s.error_stage = None;
            s.percent = None;
            s.up_to_date = false;
        });
        let now = self.clock.now_ms();
        if let Ok(mut p) = self.persisted.lock() {
            p.last_check = Some(now);
        }
        self.persist();
        let release = match backend.check(&channel).await {
            Ok(r) => r,
            Err(e) => {
                self.fail(e, None);
                return self.state();
            }
        };
        // Anti-rollback: never offer this version or a lower one (AC3).
        let Some(release) = release.filter(|r| newer(&r.version, CURRENT)) else {
            self.set(|s| {
                s.stage = "idle".into();
                s.version = None;
                s.notes = None;
                s.last_check = Some(now);
                s.up_to_date = true;
            });
            return self.state();
        };
        tracing::info!(code = "updates.found", version = %release.version, "downloading an update");
        let (version, notes) = (release.version.clone(), release.notes.clone());
        self.set(|s| {
            s.stage = "downloading".into();
            s.version = Some(version.clone());
            s.notes = notes;
            s.last_check = Some(now);
            s.percent = Some(0);
        });
        let bus = self.bus.clone();
        let base = self.state();
        let last = Arc::new(Mutex::new(0u8));
        let progress = Box::new(move |done: u64, total: Option<u64>| {
            let Some(total) = total.filter(|t| *t > 0) else {
                return;
            };
            let pct = ((done.saturating_mul(100)) / total).min(100) as u8;
            let Ok(mut l) = last.lock() else { return };
            // At most one event per 5 %.
            if pct >= l.saturating_add(5) || (pct == 100 && *l != 100) {
                *l = pct;
                let mut s = base.clone();
                s.percent = Some(pct);
                bus.publish(BusEvent::new(Topic::Updates, "updates:state", s));
            }
        });
        match backend.download(progress).await {
            Ok(bytes) => {
                if let Ok(mut staged) = self.staged.lock() {
                    *staged = Some(bytes);
                }
                if let Ok(mut p) = self.persisted.lock() {
                    p.staged_version = Some(version.clone());
                }
                self.persist();
                self.set(|s| {
                    s.stage = "ready".into();
                    s.percent = Some(100);
                });
            }
            Err(e) => self.fail(e, Some(version)),
        }
        self.state()
    }

    fn fail(&self, e: UpdateError, version: Option<String>) {
        tracing::warn!(code = "updates.failed", stage = ?e.stage, "update {:?} failed", e.stage);
        let message = match e.stage {
            UpdateStage::Verify => "Update can't be verified, so it wasn't installed.".to_string(),
            UpdateStage::Check => "Couldn't check for updates.".to_string(),
            UpdateStage::Download => "The update didn't download.".to_string(),
            UpdateStage::Install => "The update didn't install.".to_string(),
        };
        self.set(|s| {
            s.stage = "error".into();
            s.error_stage = Some(e.stage);
            s.error = Some(message);
            s.percent = None;
            if version.is_some() {
                s.version = version;
            }
        });
    }

    /// "Restart now": installs the staged release. The caller restarts.
    pub fn install(&self) -> Result<String, IpcError> {
        let state = self.state();
        let (Some(backend), Some(version)) = (self.backend.get().cloned(), state.version.clone())
        else {
            return Err(
                IpcError::new(ErrorCode::Unavailable, "There's no update to install")
                    .with_stage("install"),
            );
        };
        if state.stage != "ready" {
            return Err(
                IpcError::new(ErrorCode::Unavailable, "The update isn't ready yet")
                    .with_stage("install"),
            );
        }
        let bytes = self
            .staged
            .lock()
            .ok()
            .and_then(|s| s.clone())
            .ok_or_else(|| {
                IpcError::new(ErrorCode::Unavailable, "The update isn't ready yet")
                    .with_stage("install")
            })?;
        match backend.install(&bytes) {
            Ok(()) => {
                tracing::info!(code = "updates.installed", version = %version, "update installed");
                if let Ok(mut p) = self.persisted.lock() {
                    p.staged_version = None;
                }
                self.persist();
                if let Ok(mut staged) = self.staged.lock() {
                    *staged = None;
                }
                // The app restarts next; until then nothing is staged.
                self.set(|s| {
                    s.stage = "idle".into();
                    s.version = None;
                    s.percent = None;
                });
                Ok(version)
            }
            Err(e) => {
                let stage = e.stage;
                self.fail(e, Some(version));
                Err(
                    IpcError::new(ErrorCode::Unavailable, "The update didn't install").with_stage(
                        match stage {
                            UpdateStage::Verify => "verify",
                            _ => "install",
                        },
                    ),
                )
            }
        }
    }

    /// An API response named the oldest supported version.
    pub fn observe_min_version(&self, min: &str) {
        let min = min.trim();
        let changed = {
            let Ok(mut p) = self.persisted.lock() else {
                return;
            };
            if newer(min, CURRENT) {
                if p.min_version.as_deref() == Some(min) {
                    false
                } else {
                    p.min_version = Some(min.to_string());
                    p.min_version_seen_at = Some(self.clock.now_ms());
                    true
                }
            } else if p.min_version.is_some() {
                p.min_version = None;
                p.min_version_seen_at = None;
                true
            } else {
                false
            }
        };
        if changed {
            self.persist();
            self.refresh_min_version();
            let notice = self.state().min_version;
            self.bus.publish(BusEvent::new(
                Topic::App,
                "app:min-version",
                serde_json::json!({
                    "minVersion": notice.as_ref().map(|n| n.min_version.clone()),
                    "graceUntil": notice.as_ref().map(|n| n.grace_until),
                    "modal": notice.as_ref().is_some_and(|n| n.modal),
                }),
            ));
        }
    }

    /// Test harness: as if the notice had arrived `ms` earlier.
    pub fn shift_min_version_seen(&self, ms: i64) {
        if let Ok(mut p) = self.persisted.lock() {
            if let Some(seen) = p.min_version_seen_at.as_mut() {
                *seen -= ms;
            }
        }
        self.persist();
        self.refresh_min_version();
    }

    fn refresh_min_version(&self) {
        let notice = self.persisted.lock().ok().and_then(|p| {
            let (min, seen) = (p.min_version.clone()?, p.min_version_seen_at?);
            newer(&min, CURRENT).then(|| MinVersionNotice {
                min_version: min,
                grace_until: seen + MIN_VERSION_GRACE_MS,
                modal: self.clock.now_ms() >= seen + MIN_VERSION_GRACE_MS,
            })
        });
        if let Ok(mut s) = self.state.lock() {
            s.min_version = notice;
        }
    }

    /// Launch + 60 s, then every 4 h, while "Check for updates
    /// automatically" is on.
    pub async fn run_schedule(
        self: Arc<Self>,
        auto: impl Fn() -> futures::future::BoxFuture<'static, bool>,
    ) {
        self.clock.sleep(FIRST_CHECK).await;
        loop {
            if auto().await {
                self.check_now().await;
            }
            self.clock.sleep(CHECK_EVERY).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tandem_core::clock::MockClock;

    #[derive(Default)]
    struct FakeFeed {
        offer: Mutex<Option<Release>>,
        download: Mutex<Option<Result<Vec<u8>, UpdateError>>>,
        install: Mutex<Option<UpdateError>>,
        installed: Mutex<Vec<Vec<u8>>>,
        checks: Mutex<Vec<String>>,
    }

    #[async_trait]
    impl UpdateBackend for FakeFeed {
        async fn check(&self, channel: &str) -> Result<Option<Release>, UpdateError> {
            self.checks.lock().expect("checks").push(channel.into());
            Ok(self.offer.lock().expect("offer").clone())
        }
        async fn download(
            &self,
            progress: Box<dyn Fn(u64, Option<u64>) + Send + Sync>,
        ) -> Result<Vec<u8>, UpdateError> {
            progress(50, Some(100));
            progress(100, Some(100));
            self.download
                .lock()
                .expect("dl")
                .clone()
                .unwrap_or(Ok(b"bytes".to_vec()))
        }
        fn install(&self, bytes: &[u8]) -> Result<(), UpdateError> {
            if let Some(e) = self.install.lock().expect("install").clone() {
                return Err(e);
            }
            self.installed
                .lock()
                .expect("installed")
                .push(bytes.to_vec());
            Ok(())
        }
    }

    fn service(feed: Arc<FakeFeed>) -> (tempfile::TempDir, Arc<UpdateService>, MockClock) {
        let dir = tempfile::tempdir().expect("tempdir");
        let start = chrono::DateTime::parse_from_rfc3339("2026-10-01T09:00:00Z")
            .expect("time")
            .with_timezone(&chrono::Utc);
        let clock = MockClock::new(start);
        let svc = UpdateService::new(
            false,
            dir.path().join("update-state.json"),
            Arc::new(clock.clone()),
            EventBus::new(),
        );
        svc.set_backend(feed);
        (dir, svc, clock)
    }

    fn offer(v: &str) -> Option<Release> {
        Some(Release {
            version: v.into(),
            notes: Some("notes".into()),
        })
    }

    #[test]
    fn versions_compare_numerically_and_pre_releases_sort_below() {
        assert!(newer("0.1.1", "0.1.0"));
        assert!(newer("0.10.0", "0.9.9"));
        assert!(newer("1.0.0", "1.0.0-beta.2"));
        assert!(newer("v2.0.0", "1.9.9"));
        assert!(!newer("0.1.0", "0.1.0"));
        assert!(!newer("0.0.1", "0.1.0"));
        assert!(!newer("garbage", "0.1.0"));
    }

    #[tokio::test]
    async fn f_sh_06_1_a_newer_signed_release_downloads_and_is_ready_then_installs() {
        let feed = Arc::new(FakeFeed::default());
        *feed.offer.lock().expect("offer") = offer("9.9.0");
        let (_dir, svc, _) = service(feed.clone());
        let s = svc.check_now().await;
        assert_eq!(s.stage, "ready");
        assert_eq!(s.version.as_deref(), Some("9.9.0"));
        assert_eq!(s.percent, Some(100));
        assert_eq!(svc.install().expect("install"), "9.9.0");
        assert_eq!(feed.installed.lock().expect("installed").len(), 1);
        assert_eq!(
            svc.state().stage,
            "idle",
            "nothing is staged after the install"
        );
    }

    #[tokio::test]
    async fn f_sh_06_2_a_bad_signature_never_installs_and_names_the_verify_stage() {
        let feed = Arc::new(FakeFeed::default());
        *feed.offer.lock().expect("offer") = offer("9.9.0");
        *feed.download.lock().expect("dl") =
            Some(Err(UpdateError::new(UpdateStage::Verify, "bad sig")));
        let (_dir, svc, _) = service(feed.clone());
        let s = svc.check_now().await;
        assert_eq!(s.stage, "error");
        assert_eq!(s.error_stage, Some(UpdateStage::Verify));
        assert_eq!(
            s.error.as_deref(),
            Some("Update can't be verified, so it wasn't installed.")
        );
        assert!(svc.install().is_err());
        assert!(feed.installed.lock().expect("installed").is_empty());
    }

    #[tokio::test]
    async fn f_sh_06_3_a_lower_or_equal_version_is_never_offered() {
        for v in ["0.0.1", CURRENT] {
            let feed = Arc::new(FakeFeed::default());
            *feed.offer.lock().expect("offer") = offer(v);
            let (_dir, svc, _) = service(feed);
            let s = svc.check_now().await;
            assert_eq!(s.stage, "idle", "{v}");
            assert!(s.up_to_date);
            assert_eq!(s.version, None);
        }
    }

    #[tokio::test]
    async fn f_sh_06_4_a_min_version_starts_a_72_hour_grace_then_turns_modal() {
        let (dir, svc, clock) = service(Arc::new(FakeFeed::default()));
        svc.observe_min_version("99.0.0");
        let n = svc.state().min_version.expect("notice");
        assert!(!n.modal);
        clock.advance(Duration::from_secs(71 * 3600));
        assert!(!svc.state().min_version.expect("notice").modal);
        clock.advance(Duration::from_secs(2 * 3600));
        assert!(svc.state().min_version.expect("notice").modal);
        // The same header again doesn't restart the grace; it survives a relaunch.
        svc.observe_min_version("99.0.0");
        assert!(svc.state().min_version.expect("notice").modal);
        let again = UpdateService::new(
            false,
            dir.path().join("update-state.json"),
            Arc::new(clock.clone()),
            EventBus::new(),
        );
        assert!(again.state().min_version.expect("notice").modal);
        // A minimum at or below this version clears it.
        svc.observe_min_version("0.0.1");
        assert_eq!(svc.state().min_version, None);
    }

    #[tokio::test]
    async fn f_sh_06_channel_and_last_check_persist() {
        let feed = Arc::new(FakeFeed::default());
        let (dir, svc, clock) = service(feed.clone());
        svc.set_channel("beta").expect("beta");
        assert!(svc.set_channel("nightly").is_err());
        svc.check_now().await;
        assert_eq!(
            *feed.checks.lock().expect("checks"),
            vec!["beta".to_string()]
        );
        let again = UpdateService::new(
            false,
            dir.path().join("update-state.json"),
            Arc::new(clock),
            EventBus::new(),
        );
        again.set_backend(feed);
        let s = again.state();
        assert_eq!(s.channel, "beta");
        assert!(s.last_check.is_some());
    }

    #[tokio::test]
    async fn f_sh_06_without_an_update_key_nothing_is_checked() {
        let dir = tempfile::tempdir().expect("tempdir");
        let svc = UpdateService::new(
            false,
            dir.path().join("u.json"),
            Arc::new(tandem_core::clock::SystemClock),
            EventBus::new(),
        );
        assert_eq!(svc.check_now().await.stage, "unavailable");
    }

    #[tokio::test]
    async fn f_sh_06_deb_installs_never_check() {
        let feed = Arc::new(FakeFeed::default());
        let dir = tempfile::tempdir().expect("tempdir");
        let svc = UpdateService::new(
            true,
            dir.path().join("u.json"),
            Arc::new(tandem_core::clock::SystemClock),
            EventBus::new(),
        );
        svc.set_backend(feed.clone());
        assert_eq!(svc.check_now().await.stage, "managed");
        assert!(feed.checks.lock().expect("checks").is_empty());
        let doc = dir.path().join("doc");
        std::fs::create_dir_all(&doc).expect("doc");
        assert_eq!(
            managed_by_package_manager(&doc, None),
            cfg!(target_os = "linux")
        );
        assert!(!managed_by_package_manager(
            &doc,
            Some("/tmp/Tandem.AppImage")
        ));
        assert!(!managed_by_package_manager(&dir.path().join("none"), None));
    }

    #[tokio::test]
    async fn f_sh_06_an_install_failure_names_the_install_stage() {
        let feed = Arc::new(FakeFeed::default());
        *feed.offer.lock().expect("offer") = offer("9.9.0");
        *feed.install.lock().expect("install") =
            Some(UpdateError::new(UpdateStage::Install, "disk"));
        let (_dir, svc, _) = service(feed);
        svc.check_now().await;
        let err = svc.install().expect_err("fails");
        assert_eq!(err.stage.as_deref(), Some("install"));
        let s = svc.state();
        assert_eq!(s.stage, "error");
        assert_eq!(s.error_stage, Some(UpdateStage::Install));
    }
}

/// The app's backend: Tauri's updater against the signed feed.
pub mod tauri_backend {
    use super::*;
    use tauri_plugin_updater::UpdaterExt;

    pub struct TauriUpdater {
        app: tauri::AppHandle,
        /// `…/v1/updates`; the channel, target, arch and version follow.
        base: String,
        pubkey: String,
        last: tokio::sync::Mutex<Option<tauri_plugin_updater::Update>>,
        /// Test harness: record the install here instead of replacing the
        /// running debug binary (it isn't an AppImage).
        record_install: Option<PathBuf>,
    }

    impl TauriUpdater {
        pub fn new(
            app: tauri::AppHandle,
            base: String,
            pubkey: String,
            record_install: Option<PathBuf>,
        ) -> Self {
            Self {
                app,
                base: base.trim_end_matches('/').to_string(),
                pubkey,
                last: tokio::sync::Mutex::new(None),
                record_install,
            }
        }
    }

    /// Signature problems are the verify stage; anything else is `fallback`.
    fn classify(e: tauri_plugin_updater::Error, fallback: UpdateStage) -> UpdateError {
        use tauri_plugin_updater::Error as E;
        let stage = match &e {
            E::Minisign(_)
            | E::Base64(_)
            | E::SignatureUtf8(_)
            | E::SignedVersionMismatch { .. }
            | E::MissingSignedVersion => UpdateStage::Verify,
            _ => fallback,
        };
        UpdateError::new(stage, e.to_string())
    }

    #[async_trait]
    impl UpdateBackend for TauriUpdater {
        async fn check(&self, channel: &str) -> Result<Option<Release>, UpdateError> {
            let check = |m: String| UpdateError::new(UpdateStage::Check, m);
            let url = format!(
                "{}/{channel}/{{{{target}}}}/{{{{arch}}}}/{{{{current_version}}}}",
                self.base
            );
            let url = tauri::Url::parse(&url).map_err(|e| check(e.to_string()))?;
            let updater = self
                .app
                .updater_builder()
                .pubkey(self.pubkey.clone())
                .endpoints(vec![url])
                .map_err(|e| check(e.to_string()))?
                // Anti-rollback: only ever a higher version (AC3).
                .version_comparator(|current, release| release.version > current)
                .timeout(Duration::from_secs(60))
                .build()
                .map_err(|e| check(e.to_string()))?;
            let found = updater
                .check()
                .await
                .map_err(|e| classify(e, UpdateStage::Check))?;
            let release = found.as_ref().map(|u| Release {
                version: u.version.clone(),
                notes: u.body.clone(),
            });
            *self.last.lock().await = found;
            Ok(release)
        }

        async fn download(
            &self,
            progress: Box<dyn Fn(u64, Option<u64>) + Send + Sync>,
        ) -> Result<Vec<u8>, UpdateError> {
            let last = self.last.lock().await;
            let update = last.as_ref().ok_or_else(|| {
                UpdateError::new(UpdateStage::Download, "No update was found to download")
            })?;
            let mut done: u64 = 0;
            update
                .download(
                    |chunk, total| {
                        done += chunk as u64;
                        progress(done, total);
                    },
                    || {},
                )
                .await
                .map_err(|e| classify(e, UpdateStage::Download))
        }

        fn install(&self, bytes: &[u8]) -> Result<(), UpdateError> {
            let last = self.last.try_lock().map_err(|_| {
                UpdateError::new(UpdateStage::Install, "The update is still downloading")
            })?;
            let update = last.as_ref().ok_or_else(|| {
                UpdateError::new(UpdateStage::Install, "No update was downloaded")
            })?;
            if let Some(path) = &self.record_install {
                return std::fs::write(path, update.version.as_bytes())
                    .map_err(|e| UpdateError::new(UpdateStage::Install, e.to_string()));
            }
            update
                .install(bytes)
                .map_err(|e| classify(e, UpdateStage::Install))
        }
    }
}

/// The minisign key releases must be signed with: the release key compiled
/// in from `TAURI_SIGNING_PUBLIC_KEY`, or (debug builds only) the dev key in
/// `resources/keys.json`. A release build without a key never updates.
pub fn pubkey(bundled_keys_json: &str) -> Option<String> {
    if let Some(key) = option_env!("TAURI_SIGNING_PUBLIC_KEY")
        .map(str::trim)
        .filter(|k| !k.is_empty())
    {
        return Some(key.to_string());
    }
    if !cfg!(debug_assertions) {
        return None;
    }
    serde_json::from_str::<serde_json::Value>(bundled_keys_json)
        .ok()?
        .get("updater")?
        .as_str()
        .filter(|k| !k.is_empty())
        .map(str::to_string)
}

/// `…/v1/updates` on the API, or `TANDEM_UPDATE_URL` in debug builds (the
/// local test feed).
pub fn feed_base(api: &str) -> String {
    if cfg!(debug_assertions) {
        if let Ok(url) = std::env::var("TANDEM_UPDATE_URL") {
            if !url.is_empty() {
                return url;
            }
        }
    }
    format!("{}/v1/updates", api.trim_end_matches('/'))
}

/// Only a debug build talking to a feed on this machine may skip HTTPS.
pub fn allows_insecure_feed(base: &str) -> bool {
    cfg!(debug_assertions) && base.starts_with("http://127.0.0.1:")
}
