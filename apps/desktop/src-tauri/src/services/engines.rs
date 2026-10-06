//! Engines on this machine (ARCHITECTURE §6.1–6.4): the registry with its
//! remote overrides, detection (cached, refreshed off the UI thread), the
//! accounts' provider homes, and the launch spec for agent panes.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

use serde_json::Value;
use tandem_core::bus::{BusEvent, EventBus, Topic};
use tandem_core::error::{ErrorCode, IpcError};
use tandem_core::settings::SettingKey;
use tandem_engines::detect::{self, DetectEnv, DetectionReport};
use tandem_engines::homes::{ProviderHomes, Seed};
use tandem_engines::launch::{self, HostKind, LaunchSpec, StatusEnv, TerminalLaunchRequest};
use tandem_engines::{EngineRegistry, EngineSpec};
use tandem_pty::env::LoginEnv;
use tandem_store::repo::accounts::{self, Account, InstallRow};
use tandem_store::repo::settings;
use tandem_store::Store;

/// Detection on window focus runs at most this often (F-EN-01).
pub const FOCUS_THROTTLE: Duration = Duration::from_secs(60);

pub struct EngineService {
    registry: RwLock<EngineRegistry>,
    report: RwLock<Option<DetectionReport>>,
    detecting: tokio::sync::Mutex<()>,
    last_detect: Mutex<Option<Instant>>,
    pub homes: ProviderHomes,
    /// `tandem-hook`, when it ships next to the app.
    hook_bin: Option<PathBuf>,
    store: Store,
    bus: EventBus,
}

/// What an agent pane launches with.
pub struct AgentLaunch {
    pub spec: LaunchSpec,
    pub account: Option<Account>,
    pub home: Option<PathBuf>,
}

impl EngineService {
    pub fn new(store: Store, bus: EventBus, providers: PathBuf) -> Arc<Self> {
        Arc::new(Self {
            registry: RwLock::new(EngineRegistry::bundled()),
            report: RwLock::new(None),
            detecting: tokio::sync::Mutex::new(()),
            last_detect: Mutex::new(None),
            homes: ProviderHomes::new(providers),
            hook_bin: hook_bin(),
            store,
            bus,
        })
    }

    pub fn registry(&self) -> EngineRegistry {
        self.registry
            .read()
            .map(|r| r.clone())
            .unwrap_or_else(|_| EngineRegistry::bundled())
    }

    pub fn get(&self, id: &str) -> Option<EngineSpec> {
        self.registry.read().ok()?.get(id).cloned()
    }

    /// Applies the remote config's `engines` (F-EN-02 AC1).
    pub fn apply_config(&self, config: &Value) {
        let next = EngineRegistry::from_config(config);
        if let Ok(mut r) = self.registry.write() {
            *r = next;
        }
    }

    pub fn report(&self) -> Option<DetectionReport> {
        self.report.read().ok()?.clone()
    }

    /// The detected executable for an engine.
    pub fn installed(&self, engine_id: &str) -> Option<PathBuf> {
        self.report
            .read()
            .ok()?
            .as_ref()?
            .get(engine_id)
            .map(|f| PathBuf::from(&f.path))
    }

    /// The last report from the cache, so the picker has answers at once.
    pub async fn load_cached(&self) {
        let Ok(Some(json)) = accounts::last_detection(&self.store, "local").await else {
            return;
        };
        if let Ok(report) = serde_json::from_str::<DetectionReport>(&json) {
            if let Ok(mut r) = self.report.write() {
                r.get_or_insert(report);
            }
        }
    }

    /// True when a focus-triggered detection is due (≤ once a minute).
    pub fn focus_due(&self) -> bool {
        self.last_detect
            .lock()
            .map(|l| l.is_none_or(|t| t.elapsed() >= FOCUS_THROTTLE))
            .unwrap_or(false)
    }

    /// Detects engines (one run at a time), caches the result and sends
    /// `engines:detected`.
    pub async fn detect(&self, login: &LoginEnv) -> DetectionReport {
        let _one = self.detecting.lock().await;
        if let Ok(mut l) = self.last_detect.lock() {
            *l = Some(Instant::now());
        }
        let env = detect_env(login);
        let engines = self.registry().all().to_vec();
        let report = detect::detect(&engines, &env).await;
        let installs = report
            .found
            .iter()
            .map(|f| InstallRow {
                engine_id: f.engine_id.clone(),
                path: f.path.clone(),
                version: f.version.clone(),
            })
            .collect();
        if let Ok(json) = serde_json::to_string(&report) {
            if let Err(err) = accounts::save_detection(&self.store, "local", installs, json).await {
                tracing::warn!(error = %err, "couldn't cache the detection report");
            }
        }
        if let Ok(mut r) = self.report.write() {
            *r = Some(report.clone());
        }
        tracing::info!(
            code = "engines.detected",
            found = report.found.len(),
            slow_shell = report.slow_shell.is_some(),
            "engine detection finished"
        );
        self.bus.publish(BusEvent::new(
            Topic::Engines,
            "engines:detected",
            serde_json::json!({ "hostId": "local", "report": report }),
        ));
        report
    }

    /// The user's extra launch args for an engine (Settings › Engines).
    pub async fn extra_args(&self, engine_id: &str) -> Vec<String> {
        settings::get(&self.store, SettingKey::EngineExtraArgs)
            .await
            .ok()
            .and_then(|v| v.get(engine_id).cloned())
            .and_then(|v| serde_json::from_value::<Vec<String>>(v).ok())
            .unwrap_or_default()
    }

    /// Stores extra args, refusing secret-shaped ones.
    pub async fn set_extra_args(&self, engine_id: &str, args: Vec<String>) -> Result<(), IpcError> {
        if args.iter().any(|a| launch::looks_secret(a)) {
            return Err(IpcError::new(
                ErrorCode::Invalid,
                "That looks like a secret. Engines sign in by themselves; don't pass keys as arguments.",
            ));
        }
        let mut all = settings::get(&self.store, SettingKey::EngineExtraArgs)
            .await?
            .as_object()
            .cloned()
            .unwrap_or_default();
        if args.is_empty() {
            all.remove(engine_id);
        } else {
            all.insert(engine_id.to_string(), serde_json::json!(args));
        }
        settings::set(&self.store, SettingKey::EngineExtraArgs, Value::Object(all)).await?;
        Ok(())
    }

    /// The launch for an agent pane: the engine's default account's home
    /// (created on first use, hooks installed), status variables and the
    /// user's extra args (F-EN-04, F-AC-01).
    pub async fn agent_launch(
        &self,
        engine: &EngineSpec,
        program: PathBuf,
        workspace_root: &Path,
        status: Option<StatusEnv>,
    ) -> Result<AgentLaunch, IpcError> {
        let (account, home) = if engine.provider_home_env.is_some()
            || engine.home_strategy == tandem_engines::registry::HomeStrategy::Xdg
        {
            let root = self.homes.root().to_path_buf();
            let engine_id = engine.id.clone();
            let account = accounts::ensure_default(&self.store, &engine.id, move |id| {
                root.join(&engine_id)
                    .join(id.as_str())
                    .to_string_lossy()
                    .into_owned()
            })
            .await?;
            let seed = Seed {
                theme: self.theme().await,
            };
            let home = self
                .homes
                .create(engine, account.id.as_str(), &seed)
                .map_err(|e| {
                    IpcError::new(ErrorCode::Internal, e.to_string()).with_stage("provider_home")
                })?;
            if let Some(hook) = &self.hook_bin {
                if let Err(err) = self.homes.install_hooks(engine, &home, hook) {
                    // Status falls back to heuristics; the launch goes on.
                    tracing::warn!(code = "engines.hooks_failed", error = %err, "couldn't install status hooks");
                }
            }
            let trust = settings::get(&self.store, SettingKey::TrustWorkspacesIAdd)
                .await
                .ok()
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            if trust {
                let _ = self.homes.trust_workspace(engine, &home, workspace_root);
            }
            (Some(account), Some(home))
        } else {
            (None, None)
        };
        let req = TerminalLaunchRequest {
            engine,
            program,
            extra_args: self.extra_args(&engine.id).await,
            resume: None,
            host: HostKind::Local,
            home: home.clone(),
            providers_root: self.homes.root().to_path_buf(),
            status,
        };
        let spec = launch::terminal_launch(&req).map_err(|e| {
            let code = match e {
                launch::LaunchError::SecretInArgs => ErrorCode::Invalid,
                _ => ErrorCode::Unavailable,
            };
            IpcError::new(code, e.to_string())
        })?;
        Ok(AgentLaunch {
            spec,
            account,
            home,
        })
    }

    async fn theme(&self) -> String {
        let theme = settings::get(&self.store, SettingKey::Theme)
            .await
            .ok()
            .and_then(|v| v.as_str().map(str::to_string))
            .unwrap_or_default();
        if theme == "light" { "light" } else { "dark" }.to_string()
    }
}

/// Where to look, from the captured login environment.
pub fn detect_env(login: &LoginEnv) -> DetectEnv {
    let home = login
        .vars
        .get("HOME")
        .or_else(|| login.vars.get("USERPROFILE"))
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
        .unwrap_or_default();
    let mut env = DetectEnv::local(login.shell.clone(), home, login.vars.clone());
    // Tests point detection at their own folders (debug builds only).
    if cfg!(debug_assertions) {
        if let Some(dirs) = std::env::var_os("TANDEM_DETECT_SYSTEM_DIRS") {
            env.system_dirs = std::env::split_paths(&dirs)
                .filter(|p| !p.as_os_str().is_empty())
                .collect();
        }
        if std::env::var("TANDEM_DETECT_NO_NPM").as_deref() == Ok("1") {
            env.probe_npm = false;
        }
        if let Some(secs) = std::env::var("TANDEM_DETECT_DEADLINE_SECS")
            .ok()
            .and_then(|s| s.parse().ok())
        {
            env.deadline = Duration::from_secs(secs);
        }
    }
    env
}

/// `tandem-hook` next to the app binary (`TANDEM_HOOK_BIN` in debug builds).
fn hook_bin() -> Option<PathBuf> {
    if cfg!(debug_assertions) {
        if let Some(p) = std::env::var_os("TANDEM_HOOK_BIN") {
            return Some(PathBuf::from(p));
        }
    }
    let exe = std::env::current_exe().ok()?;
    let name = if cfg!(windows) {
        "tandem-hook.exe"
    } else {
        "tandem-hook"
    };
    let p = exe.parent()?.join(name);
    p.is_file().then_some(p)
}
