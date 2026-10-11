//! "Hey Conductor" on the desktop (F-CN-07). Runs the wake listener from
//! `tandem-voice` on the Dictate microphone (DI-03 policy), stepping aside
//! whenever the mic-owner registry or a Dictate take holds the mic, and
//! starts a call through the Conductor's single entry point when the phrase
//! is heard.

use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use tandem_core::settings::SettingKey;
use tandem_store::repo::settings;
use tandem_store::Store;
use tandem_voice::capture::AudioBackend;
use tandem_voice::devices::MicPrefs;
use tandem_voice::mic_owner::{MicOwner, MicOwners};
use tandem_voice::wake::listener::{ListenerDeps, WakeConfig, WakeListener};
use tandem_voice::wake::{self, Sensitivity, WakeState, WakeUnavailable};

/// The Conductor's start-call entry point, as the wake listener sees it.
/// The call lifecycle owns the real one; a wake start makes Conductor answer
/// aloud once the call is ready (`conductor_start_call {wake: true}`).
pub trait CallStarter: Send + Sync {
    fn start_call_from_wake(&self) -> Result<(), String>;
}

/// Until the call lifecycle is wired in: nothing to start, said once in the
/// log.
pub struct NoCalls;

impl CallStarter for NoCalls {
    fn start_call_from_wake(&self) -> Result<(), String> {
        Err("Conductor calls aren't available in this build".into())
    }
}

/// `conductor_wake_status` and the `conductor:wake-state` event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct WakeStatus {
    pub state: WakeState,
    /// `None` when this build can offer the wake phrase.
    pub unavailable: Option<WakeUnavailable>,
}

pub type WakeEmitter = Arc<dyn Fn(&WakeStatus) + Send + Sync>;

/// What the listener needs from Dictate: the backend, whether a take
/// records, and the microphone setting.
pub trait MicSource: Send + Sync {
    fn backend(&self) -> Arc<dyn AudioBackend>;
    fn recording(&self) -> bool;
}

impl MicSource for crate::services::dictate::DictateService {
    fn backend(&self) -> Arc<dyn AudioBackend> {
        self.audio_backend()
    }
    fn recording(&self) -> bool {
        self.is_recording()
    }
}

pub struct WakeService {
    store: Store,
    owners: MicOwners,
    model_dir: PathBuf,
    sidecar: Option<PathBuf>,
    unavailable: Option<WakeUnavailable>,
    starter: Mutex<Arc<dyn CallStarter>>,
    listener: OnceLock<WakeListener>,
    emit: OnceLock<WakeEmitter>,
}

impl WakeService {
    /// `sidecar`: the `tandem-wake` keyword spotter, when this build has it.
    pub fn new(
        store: Store,
        owners: MicOwners,
        model_dir: PathBuf,
        sidecar: Option<PathBuf>,
    ) -> Arc<Self> {
        let unavailable = wake::availability(&model_dir, sidecar.is_some()).err();
        Arc::new(Self {
            store,
            owners,
            model_dir,
            sidecar,
            unavailable,
            starter: Mutex::new(Arc::new(NoCalls)),
            listener: OnceLock::new(),
            emit: OnceLock::new(),
        })
    }

    /// The Conductor call lifecycle registers its entry point here.
    pub fn set_starter(&self, starter: Arc<dyn CallStarter>) {
        *self.starter.lock() = starter;
    }

    pub fn set_emitter(&self, emit: WakeEmitter) {
        let _ = self.emit.set(emit);
    }

    pub fn status(&self) -> WakeStatus {
        WakeStatus {
            state: self
                .listener
                .get()
                .map(|l| l.state())
                .unwrap_or(match self.unavailable {
                    Some(u) => WakeState::Unavailable(u),
                    None => WakeState::Off,
                }),
            unavailable: self.unavailable,
        }
    }

    /// Opens the listener (once, from setup) with the current settings.
    pub async fn start(self: &Arc<Self>, mic: Arc<dyn MicSource>, prefs: MicPrefs) {
        if self.listener.get().is_some() {
            return;
        }
        let config = self.config(prefs).await;
        let owners = self.owners.clone();
        let m = mic.clone();
        let dir = self.model_dir.clone();
        let bin = self.sidecar.clone();
        let me = Arc::downgrade(self);
        let st = Arc::downgrade(self);
        let unavailable = self.unavailable;
        let listener = WakeListener::start(
            ListenerDeps {
                backend: mic.backend(),
                mic_owner: Arc::new(move || {
                    owners
                        .current()
                        .or_else(|| m.recording().then_some(MicOwner::Dictate))
                }),
                route: wake::route::native(),
                detector: Arc::new(move |s| wake::load_detector(bin.as_deref(), &dir, s)),
                unavailable,
                on_wake: Arc::new(move || {
                    if let Some(me) = me.upgrade() {
                        me.heard();
                    }
                }),
                on_state: Arc::new(move |state| {
                    if let Some(me) = st.upgrade() {
                        if let Some(emit) = me.emit.get() {
                            emit(&WakeStatus { state, unavailable });
                        }
                    }
                }),
            },
            config,
        );
        let _ = self.listener.set(listener);
    }

    /// After a `conductor_wake*` or microphone setting changes.
    pub async fn reload(&self, prefs: MicPrefs) {
        if let Some(l) = self.listener.get() {
            l.configure(self.config(prefs).await);
        }
    }

    /// The phrase was heard: start a call through the single entry point.
    pub fn heard(&self) {
        let starter = self.starter.lock().clone();
        if let Err(e) = starter.start_call_from_wake() {
            tracing::info!(reason = %e, "wake phrase heard; no call started");
        }
    }

    async fn config(&self, prefs: MicPrefs) -> WakeConfig {
        let get = |k| async move { settings::get(&self.store, k).await.ok() };
        WakeConfig {
            enabled: get(SettingKey::ConductorWake)
                .await
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
            sensitivity: Sensitivity::parse(
                get(SettingKey::ConductorWakeSensitivity)
                    .await
                    .and_then(|v| v.as_str().map(str::to_string))
                    .as_deref()
                    .unwrap_or("medium"),
            ),
            pause_while_videos: get(SettingKey::ConductorWakePauseVideos)
                .await
                .and_then(|v| v.as_bool())
                .unwrap_or(true),
            prefs,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct Counting(AtomicUsize);
    impl CallStarter for Counting {
        fn start_call_from_wake(&self) -> Result<(), String> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    fn rig() -> (tempfile::TempDir, Arc<WakeService>) {
        let dir = tempfile::tempdir().expect("test");
        let data = tandem_core::DataDir::at(dir.path().join("d")).expect("test");
        let store = Store::open(&data).expect("test");
        let svc = WakeService::new(
            store,
            MicOwners::new(),
            wake::source_model_dir(),
            Some(PathBuf::from("/bin/true")),
        );
        (dir, svc)
    }

    #[test]
    fn f_cn_07_heard_starts_a_call_through_the_entry_point() {
        let (_d, svc) = rig();
        // The placeholder starts nothing and doesn't panic.
        svc.heard();
        let calls = Arc::new(Counting(AtomicUsize::new(0)));
        svc.set_starter(calls.clone());
        svc.heard();
        assert_eq!(calls.0.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn f_cn_07_status_before_start_is_off_or_unavailable() {
        let (_d, svc) = rig();
        // With a spotter, the model and a passing benchmark, it's offered.
        let s = svc.status();
        assert_eq!((s.state, s.unavailable), (WakeState::Off, None));
        // Without the sidecar it isn't.
        let dir = tempfile::tempdir().expect("test");
        let data = tandem_core::DataDir::at(dir.path().join("d")).expect("test");
        let none = WakeService::new(
            Store::open(&data).expect("test"),
            MicOwners::new(),
            wake::source_model_dir(),
            None,
        );
        let s = none.status();
        assert_eq!(s.unavailable, Some(WakeUnavailable::NotBuilt));
        assert_eq!(s.state, WakeState::Unavailable(WakeUnavailable::NotBuilt));
    }
}
