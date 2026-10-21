//! Engines on this machine (ARCHITECTURE §6.1–6.4): the registry with its
//! remote overrides, detection (cached, refreshed off the UI thread), the
//! accounts' provider homes, and the launch spec for agent panes.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

use serde_json::Value;
use tandem_core::bus::{BusEvent, EventBus, Topic};
use tandem_core::error::{ErrorCode, IpcError};
use tandem_core::ids::AccountId;
use tandem_core::settings::SettingKey;
use tandem_engines::detect::{self, DetectEnv, DetectionReport};
use tandem_engines::homes::{ProviderHomes, Seed};
use tandem_engines::launch::{self, HostKind, LaunchSpec, StatusEnv, TerminalLaunchRequest};
use tandem_engines::{EngineRegistry, EngineSpec};
use tandem_pty::env::LoginEnv;
use tandem_store::repo::accounts::{self, Account, InstallRow};
use tandem_store::repo::sessions::LaunchConfig;
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
    /// The Team org's policy, read at each launch (F-BL-06).
    policy: std::sync::OnceLock<PolicySource>,
    /// Versions seen and the last good ones (F-EN-08), beside the homes.
    pub versions: crate::services::engine_versions::EngineVersions,
}

/// Reads the org policy in force (the entitlement service's).
pub type PolicySource = Arc<dyn Fn() -> Option<tandem_core::policy::OrgPolicy> + Send + Sync>;

/// What an agent pane launches with.
pub struct AgentLaunch {
    pub spec: LaunchSpec,
    pub account: Option<Account>,
    pub home: Option<PathBuf>,
    /// Saved with the session so Resume relaunches the same way (F-SH-07).
    pub config: LaunchConfig,
}

/// A relaunch of a saved session (F-SH-07): its own configuration, never
/// the current defaults.
pub struct Relaunch<'a> {
    /// The engine's session id to resume; `None` starts a new conversation.
    pub resume: Option<&'a str>,
    pub config: &'a LaunchConfig,
}

/// The card's message when a saved configuration is gone (UI-132).
pub const LAUNCH_CONFIG_UNAVAILABLE: &str = "Launch configuration unavailable";

impl EngineService {
    pub fn new(store: Store, bus: EventBus, providers: PathBuf) -> Arc<Self> {
        Arc::new(Self {
            registry: RwLock::new(EngineRegistry::bundled()),
            report: RwLock::new(None),
            detecting: tokio::sync::Mutex::new(()),
            last_detect: Mutex::new(None),
            versions: crate::services::engine_versions::EngineVersions::new(
                providers.parent().unwrap_or(&providers),
            ),
            homes: ProviderHomes::new(providers),
            hook_bin: hook_bin(),
            store,
            bus,
            policy: std::sync::OnceLock::new(),
        })
    }

    /// Where launches read the org policy from (set once at startup).
    pub fn set_policy_source(&self, source: PolicySource) {
        let _ = self.policy.set(source);
    }

    /// Refuses an engine the user's Team org doesn't allow (F-BL-06).
    pub fn check_allowed(&self, engine: &EngineSpec) -> Result<(), IpcError> {
        let policy = self.policy.get().and_then(|p| p());
        if tandem_core::policy::allows_engine(policy.as_ref(), &engine.id) {
            return Ok(());
        }
        Err(IpcError::new(
            ErrorCode::PermissionDenied,
            format!("Your organization doesn't allow {}.", engine.label),
        )
        .with_hint("Ask an admin of your organization to allow it on the Team page."))
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
        account: Option<&AccountId>,
    ) -> Result<AgentLaunch, IpcError> {
        self.agent_launch_with(engine, program, workspace_root, status, account, None)
            .await
    }

    /// [`agent_launch`](Self::agent_launch), or a relaunch of a saved session
    /// in exactly its provider home with its own args.
    pub async fn agent_launch_with(
        &self,
        engine: &EngineSpec,
        program: PathBuf,
        workspace_root: &Path,
        status: Option<StatusEnv>,
        account: Option<&AccountId>,
        relaunch: Option<Relaunch<'_>>,
    ) -> Result<AgentLaunch, IpcError> {
        self.check_allowed(engine)?;
        if let Some(r) = relaunch {
            return self.relaunch(engine, program, status, account, r).await;
        }
        let (account, home) = if engine.provider_home_env.is_some()
            || engine.home_strategy == tandem_engines::registry::HomeStrategy::Xdg
        {
            let account = match account {
                Some(id) => {
                    let a = accounts::get(&self.store, id).await?;
                    if a.engine_id != engine.id {
                        return Err(IpcError::new(
                            ErrorCode::Invalid,
                            "That account is for another engine",
                        ));
                    }
                    a
                }
                None => {
                    let root = self.homes.root().to_path_buf();
                    let engine_id = engine.id.clone();
                    accounts::ensure_default(&self.store, &engine.id, move |id| {
                        root.join(&engine_id)
                            .join(id.as_str())
                            .to_string_lossy()
                            .into_owned()
                    })
                    .await?
                }
            };
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
        let extra_args = self.extra_args(&engine.id).await;
        let config = LaunchConfig {
            extra_args: extra_args.clone(),
            home: home.as_ref().map(|h| h.to_string_lossy().into_owned()),
            own_home: false,
        };
        let req = TerminalLaunchRequest {
            engine,
            program,
            extra_args,
            resume: None,
            host: HostKind::Local,
            home: home.clone(),
            providers_root: self.homes.root().to_path_buf(),
            status,
            pinned: self.pinned(&engine.id).await,
        };
        let spec = launch::terminal_launch(&req).map_err(launch_error)?;
        Ok(AgentLaunch {
            spec,
            account,
            home,
            config,
        })
    }

    /// Whether the user pinned this engine (F-EN-08, `engine_pins`).
    pub async fn pinned(&self, engine_id: &str) -> bool {
        settings::get(&self.store, SettingKey::EnginePins)
            .await
            .ok()
            .and_then(|v| v.get(engine_id).and_then(serde_json::Value::as_bool))
            .unwrap_or(false)
    }

    async fn relaunch(
        &self,
        engine: &EngineSpec,
        program: PathBuf,
        status: Option<StatusEnv>,
        account: Option<&AccountId>,
        r: Relaunch<'_>,
    ) -> Result<AgentLaunch, IpcError> {
        let unavailable = || IpcError::new(ErrorCode::Unavailable, LAUNCH_CONFIG_UNAVAILABLE);
        let home = match &r.config.home {
            Some(h) => {
                let home = PathBuf::from(h);
                if !home.is_dir() {
                    return Err(unavailable().with_stage("provider_home"));
                }
                if let Some(hook) = &self.hook_bin {
                    if let Err(err) = self.homes.install_hooks(engine, &home, hook) {
                        tracing::warn!(code = "engines.hooks_failed", error = %err, "couldn't install status hooks");
                    }
                }
                Some(home)
            }
            // A library session from the engine's own home (F-TH-17).
            None if r.config.own_home => None,
            // An engine that keeps its sign-in in a home always ran in one.
            None if engine.provider_home_env.is_some()
                || engine.home_strategy == tandem_engines::registry::HomeStrategy::Xdg =>
            {
                return Err(unavailable().with_stage("provider_home"));
            }
            None => None,
        };
        let account = match account {
            Some(id) => accounts::get(&self.store, id).await.ok(),
            None => None,
        };
        let req = TerminalLaunchRequest {
            engine,
            program,
            extra_args: r.config.extra_args.clone(),
            resume: r.resume.map(str::to_string),
            host: HostKind::Local,
            home: home.clone(),
            providers_root: self.homes.root().to_path_buf(),
            status,
            pinned: self.pinned(&engine.id).await,
        };
        let spec = launch::terminal_launch(&req).map_err(launch_error)?;
        Ok(AgentLaunch {
            spec,
            account,
            home,
            config: r.config.clone(),
        })
    }

    /// The account a new agent pane of `engine` in `workspace` runs on: the
    /// explicit pick, else the workspace's override, else the engine's
    /// default (created on first use). `None` for engines without homes.
    /// The pane keeps it for as long as it lives (F-AC-03).
    pub async fn account_for_new_session(
        &self,
        engine: &EngineSpec,
        workspace: &tandem_core::ids::WorkspaceId,
        explicit: Option<&AccountId>,
    ) -> Result<Option<AccountId>, IpcError> {
        if explicit.is_some() {
            return Ok(explicit.cloned());
        }
        if engine.provider_home_env.is_none()
            && engine.home_strategy != tandem_engines::registry::HomeStrategy::Xdg
        {
            return Ok(None);
        }
        // A worktree follows its repository's choice unless it has its own.
        let mut workspace_pick =
            accounts::workspace_override(&self.store, workspace, &engine.id).await?;
        if workspace_pick.is_none() {
            if let Some(parent) = tandem_store::repo::workspaces::get(&self.store, workspace)
                .await
                .ok()
                .and_then(|w| w.parent_id)
            {
                workspace_pick =
                    accounts::workspace_override(&self.store, &parent, &engine.id).await?;
            }
        }
        let default = match workspace_pick {
            Some(_) => None,
            None => {
                let root = self.homes.root().to_path_buf();
                let engine_id = engine.id.clone();
                Some(
                    accounts::ensure_default(&self.store, &engine.id, move |id| {
                        root.join(&engine_id)
                            .join(id.as_str())
                            .to_string_lossy()
                            .into_owned()
                    })
                    .await?
                    .id,
                )
            }
        };
        Ok(tandem_core::accounts::resolve(
            None,
            None,
            workspace_pick.as_ref(),
            default.as_ref(),
        ))
    }

    /// A named account with its own home (the full sign-in flow is F-AC-02).
    pub async fn create_account(
        &self,
        engine: &EngineSpec,
        label: &str,
    ) -> Result<Account, IpcError> {
        let root = self.homes.root().to_path_buf();
        let engine_id = engine.id.clone();
        let account = accounts::create(&self.store, &engine.id, label, false, move |id| {
            root.join(&engine_id)
                .join(id.as_str())
                .to_string_lossy()
                .into_owned()
        })
        .await?;
        let seed = Seed {
            theme: self.theme().await,
        };
        self.homes
            .create(engine, account.id.as_str(), &seed)
            .map_err(|e| {
                IpcError::new(ErrorCode::Internal, e.to_string()).with_stage("provider_home")
            })?;
        Ok(account)
    }

    pub(crate) async fn theme(&self) -> String {
        let theme = settings::get(&self.store, SettingKey::Theme)
            .await
            .ok()
            .and_then(|v| v.as_str().map(str::to_string))
            .unwrap_or_default();
        if theme == "light" { "light" } else { "dark" }.to_string()
    }
}

fn launch_error(e: launch::LaunchError) -> IpcError {
    let code = match e {
        launch::LaunchError::SecretInArgs => ErrorCode::Invalid,
        _ => ErrorCode::Unavailable,
    };
    IpcError::new(code, e.to_string())
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
pub(crate) fn hook_bin() -> Option<PathBuf> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use tandem_core::policy::OrgPolicy;

    /// D-828: release bundles carry `tandem-hook` beside the app, where
    /// `hook_bin` looks; without it panes fall back to heuristics.
    #[test]
    fn f_en_05_the_release_bundle_carries_tandem_hook() {
        let conf: serde_json::Value =
            serde_json::from_str(include_str!("../../tauri.sidecars.conf.json")).expect("json");
        let bins = conf["bundle"]["externalBin"]
            .as_array()
            .expect("externalBin");
        for name in ["binaries/tandem-hook", "binaries/tandem-tasks"] {
            assert!(bins.iter().any(|b| b == name), "{name} isn't bundled");
        }
    }

    #[test]
    fn f_bl_06_an_engine_the_org_doesnt_allow_never_launches() {
        let dir = tempfile::tempdir().expect("tmp");
        let data = tandem_core::DataDir::at(dir.path().to_path_buf()).expect("data");
        let store = Store::open(&data).expect("store");
        let svc = EngineService::new(store, EventBus::new(), dir.path().join("providers"));
        let claude = svc.get("claude").expect("claude");
        let codex = svc.get("codex").expect("codex");
        // No policy (not on Team): everything launches.
        assert!(svc.check_allowed(&codex).is_ok());
        svc.set_policy_source(Arc::new(|| {
            Some(OrgPolicy {
                allowed_engines: Some(vec!["claude".into()]),
                ..OrgPolicy::default()
            })
        }));
        assert!(svc.check_allowed(&claude).is_ok());
        let err = svc.check_allowed(&codex).expect_err("refused");
        assert_eq!(err.code, ErrorCode::PermissionDenied);
        assert_eq!(err.message, "Your organization doesn't allow Codex.");
    }

    /// A library session from Claude Code's own home resumes with no
    /// provider home and no hooks written into the user's own home (F-TH-17,
    /// D-2412).
    #[tokio::test]
    async fn f_th_17_2_an_own_home_session_resumes_with_no_provider_home() {
        let dir = tempfile::tempdir().expect("tempdir");
        let data = tandem_core::DataDir::at(dir.path().join("data")).expect("data");
        let store = Store::open(&data).expect("store");
        let svc = EngineService::new(store, EventBus::new(), dir.path().join("providers"));
        let reg = EngineRegistry::bundled();
        let claude = reg.get("claude").expect("claude").clone();
        let config = LaunchConfig {
            extra_args: Vec::new(),
            home: None,
            own_home: true,
        };
        let launch = svc
            .agent_launch_with(
                &claude,
                PathBuf::from("/usr/local/bin/claude"),
                dir.path(),
                None,
                None,
                Some(Relaunch {
                    resume: Some("aaaa-1111"),
                    config: &config,
                }),
            )
            .await
            .expect("launch");
        assert_eq!(launch.home, None);
        assert!(launch.account.is_none());
        assert!(!launch.spec.env.contains_key("CLAUDE_CONFIG_DIR"));
        let argv = launch.spec.argv().join(" ");
        assert!(argv.contains("--resume aaaa-1111"), "{argv}");
        // Without own_home, a Claude Code relaunch with no home is refused.
        let plain = LaunchConfig::default();
        assert!(svc
            .agent_launch_with(
                &claude,
                PathBuf::from("/usr/local/bin/claude"),
                dir.path(),
                None,
                None,
                Some(Relaunch {
                    resume: Some("aaaa-1111"),
                    config: &plain,
                }),
            )
            .await
            .is_err());
    }
}
