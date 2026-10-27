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
use tandem_engines::acp_agents::AcpAgent;
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
    /// ACP agents the user added (F-EN-10), from `$DATA/acp-agents.json`.
    acp_agents: RwLock<Vec<AcpAgent>>,
    acp_file: PathBuf,
    /// Held across each Add and Remove's read, change and write.
    acp_edit: Mutex<()>,
}

/// Where the ACP agents the user added are kept.
pub const ACP_AGENTS_FILE: &str = "acp-agents.json";

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
        let acp_file = providers
            .parent()
            .unwrap_or(&providers)
            .join(ACP_AGENTS_FILE);
        Arc::new(Self {
            acp_agents: RwLock::new(read_agents(&acp_file)),
            acp_file,
            acp_edit: Mutex::new(()),
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

    /// The registry with the ACP agents the user added after it.
    pub fn registry(&self) -> EngineRegistry {
        let mut reg = self.base_registry();
        for agent in self.acp_agents() {
            if let Ok(spec) = agent.spec() {
                reg.add(spec);
            }
        }
        reg
    }

    /// The registry without added agents: what hosts are asked for, what
    /// versions, instructions and usage cover, and what terminal panes
    /// launch (D-2513).
    pub fn base_registry(&self) -> EngineRegistry {
        self.registry
            .read()
            .map(|r| r.clone())
            .unwrap_or_else(|_| EngineRegistry::bundled())
    }

    pub fn get(&self, id: &str) -> Option<EngineSpec> {
        if let Some(spec) = self.registry.read().ok()?.get(id).cloned() {
            return Some(spec);
        }
        self.acp_agent(id).and_then(|a| a.spec().ok())
    }

    /// The ACP agents the user added (F-EN-10).
    pub fn acp_agents(&self) -> Vec<AcpAgent> {
        self.acp_agents
            .read()
            .map(|a| a.clone())
            .unwrap_or_default()
    }

    pub fn acp_agent(&self, id: &str) -> Option<AcpAgent> {
        self.acp_agents().into_iter().find(|a| a.id == id)
    }

    /// Saves an agent (already tested) under a new id, and lists it as
    /// installed at its path. The id never repeats one in the registry, and
    /// `nonce` keeps it from repeating a removed one's (D-2511).
    pub fn add_acp_agent(&self, mut agent: AcpAgent, nonce: u32) -> Result<AcpAgent, IpcError> {
        let _edit = self.acp_edit.lock().map_err(|_| busy())?;
        let mut list = self.acp_agents();
        if list.len() >= tandem_engines::acp_agents::MAX_AGENTS {
            return Err(IpcError::new(
                ErrorCode::LimitReached,
                format!(
                    "Vibezeo keeps up to {} ACP agents",
                    tandem_engines::acp_agents::MAX_AGENTS
                ),
            ));
        }
        let taken: Vec<String> = self.registry().all().iter().map(|e| e.id.clone()).collect();
        agent.id = tandem_engines::acp_agents::id_for(&agent.label, &taken, nonce);
        agent
            .spec()
            .map_err(|e| IpcError::new(ErrorCode::Invalid, e.to_string()))?;
        list.push(agent.clone());
        self.save_agents(list)?;
        if let Ok(mut r) = self.report.write() {
            if let Some(report) = r.as_mut() {
                with_agents(report, std::slice::from_ref(&agent));
            }
        }
        Ok(agent)
    }

    pub fn remove_acp_agent(&self, id: &str) -> Result<(), IpcError> {
        let _edit = self.acp_edit.lock().map_err(|_| busy())?;
        let list = self.acp_agents();
        if !list.iter().any(|a| a.id == id) {
            return Err(IpcError::new(ErrorCode::NotFound, "No agent with that id"));
        }
        let list: Vec<AcpAgent> = list.into_iter().filter(|a| a.id != id).collect();
        self.save_agents(list)?;
        if let Ok(mut r) = self.report.write() {
            if let Some(report) = r.as_mut() {
                report.found.retain(|f| f.engine_id != id);
                report.missing.retain(|m| m != id);
            }
        }
        Ok(())
    }

    fn save_agents(&self, list: Vec<AcpAgent>) -> Result<(), IpcError> {
        let json = serde_json::to_vec_pretty(&list)
            .map_err(|e| IpcError::new(ErrorCode::Internal, e.to_string()))?;
        tandem_core::atomic::atomic_write(&self.acp_file, &json, Some(0o600))
            .map_err(|e| IpcError::new(ErrorCode::Io, format!("Couldn't save the agent: {e}")))?;
        if let Ok(mut a) = self.acp_agents.write() {
            *a = list;
        }
        Ok(())
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
        // Added ACP agents aren't looked for: each is where it was added.
        let engines = self.base_registry().all().to_vec();
        let mut report = detect::detect(&engines, &env).await;
        with_agents(&mut report, &self.acp_agents());
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
        self.launch(
            engine,
            program,
            workspace_root,
            status,
            account,
            relaunch,
            true,
        )
        .await
    }

    /// [`agent_launch`](Self::agent_launch) in a folder that's no workspace
    /// (Lead's answer sessions, F-TK-08): the account's provider home and
    /// its env, but the folder is never marked trusted, whatever "Trust
    /// workspaces I add" says.
    pub async fn agent_launch_untrusted(
        &self,
        engine: &EngineSpec,
        program: PathBuf,
        folder: &Path,
        account: Option<&AccountId>,
    ) -> Result<AgentLaunch, IpcError> {
        self.launch(engine, program, folder, None, account, None, false)
            .await
    }

    #[allow(clippy::too_many_arguments)]
    async fn launch(
        &self,
        engine: &EngineSpec,
        program: PathBuf,
        workspace_root: &Path,
        status: Option<StatusEnv>,
        account: Option<&AccountId>,
        relaunch: Option<Relaunch<'_>>,
        trust_folder: bool,
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
            let trust = trust_folder
                && settings::get(&self.store, SettingKey::TrustWorkspacesIAdd)
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
            local: None,
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

fn busy() -> IpcError {
    IpcError::new(ErrorCode::Internal, "Couldn't change the ACP agents")
}

/// The saved agents, or none when the file is missing or unreadable. Each
/// is held to the form's rules again: one that fails them is left out.
fn read_agents(path: &Path) -> Vec<AcpAgent> {
    std::fs::read(path)
        .ok()
        .and_then(|b| serde_json::from_slice::<Vec<AcpAgent>>(&b).ok())
        .unwrap_or_default()
        .into_iter()
        .filter(|a| a.check().is_ok() && a.spec().is_ok())
        .take(tandem_engines::acp_agents::MAX_AGENTS)
        .collect()
}

/// Lists each agent whose program is still there as found.
fn with_agents(report: &mut DetectionReport, agents: &[AcpAgent]) {
    for agent in agents {
        report.found.retain(|f| f.engine_id != agent.id);
        report.missing.retain(|m| m != &agent.id);
        if Path::new(&agent.program).is_file() {
            report.found.push(detect::FoundEngine {
                engine_id: agent.id.clone(),
                path: agent.program.clone(),
                version: None,
                source: detect::FoundSource::Shell,
            });
        } else {
            report.missing.push(agent.id.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
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
            local: None,
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

    /// F-EN-10: an added agent joins the registry as a Thread engine through
    /// ACP, is found at the path it was added with, survives a restart, and
    /// leaves when removed.
    #[tokio::test]
    async fn f_en_10_1_an_added_agent_is_a_listed_thread_engine_until_removed() {
        let dir = tempfile::tempdir().expect("tempdir");
        let data = tandem_core::DataDir::at(dir.path().join("data")).expect("data");
        let store = Store::open(&data).expect("store");
        let program = dir.path().join("my-agent");
        std::fs::write(&program, "#!/bin/sh\n").expect("program");
        let providers = data.root().join("providers");
        let svc = EngineService::new(store.clone(), EventBus::new(), providers.clone());
        let agent = AcpAgent {
            id: String::new(),
            label: "My agent".into(),
            program: program.to_string_lossy().into_owned(),
            args: vec!["acp".into()],
            env: [("MY_AGENT_MODE".to_string(), "fast".to_string())].into(),
        };
        if let Ok(mut r) = svc.report.write() {
            *r = Some(DetectionReport::default());
        }
        let agent = svc.add_acp_agent(agent, 1).expect("add");
        assert_eq!(agent.id, "acp-my-agent-00000001");
        let spec = svc.get(&agent.id).expect("in the registry");
        assert_eq!(spec.chat_adapter.as_deref(), Some("acp"));
        assert!(svc.registry().get(&agent.id).is_some());
        assert_eq!(svc.installed(&agent.id), Some(program.clone()));
        // The file is the user's only, and a new run reads it back.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(data.root().join(ACP_AGENTS_FILE))
                .expect("file")
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        let again = EngineService::new(store, EventBus::new(), providers);
        assert_eq!(again.acp_agents(), vec![agent.clone()]);
        svc.remove_acp_agent(&agent.id).expect("remove");
        assert!(svc.get(&agent.id).is_none());
        assert!(svc.installed(&agent.id).is_none());
        assert_eq!(
            svc.remove_acp_agent(&agent.id).expect_err("gone").code,
            ErrorCode::NotFound
        );
    }

    fn agent_at(program: &Path, label: &str) -> AcpAgent {
        AcpAgent {
            id: String::new(),
            label: label.into(),
            program: program.to_string_lossy().into_owned(),
            args: vec!["acp".into()],
            env: BTreeMap::new(),
        }
    }

    /// F-EN-10 review: Remove, then Add with the same label, gives a new id,
    /// so the old threads never run the new agent (D-2511).
    #[test]
    fn f_en_10_3_a_removed_agents_id_isnt_given_again() {
        let dir = tempfile::tempdir().expect("tempdir");
        let data = tandem_core::DataDir::at(dir.path().join("data")).expect("data");
        let store = Store::open(&data).expect("store");
        let program = dir.path().join("goose");
        std::fs::write(&program, "#!/bin/sh\n").expect("program");
        let svc = EngineService::new(store, EventBus::new(), data.root().join("providers"));
        let first = svc
            .add_acp_agent(agent_at(&program, "Goose"), 5)
            .expect("add");
        svc.remove_acp_agent(&first.id).expect("remove");
        let second = svc
            .add_acp_agent(agent_at(&program, "Goose"), 6)
            .expect("add");
        assert_ne!(first.id, second.id);
        assert!(svc.get(&first.id).is_none());
    }

    /// F-EN-10 review: concurrent Adds each land in the file.
    #[test]
    fn f_en_10_1_adds_at_once_all_land() {
        let dir = tempfile::tempdir().expect("tempdir");
        let data = tandem_core::DataDir::at(dir.path().join("data")).expect("data");
        let store = Store::open(&data).expect("store");
        let program = dir.path().join("agent");
        std::fs::write(&program, "#!/bin/sh\n").expect("program");
        let providers = data.root().join("providers");
        let svc = EngineService::new(store.clone(), EventBus::new(), providers.clone());
        std::thread::scope(|s| {
            for n in 0..8u32 {
                let (svc, program) = (&svc, &program);
                s.spawn(move || {
                    svc.add_acp_agent(agent_at(program, &format!("Agent {n}")), n)
                        .expect("add")
                });
            }
        });
        assert_eq!(svc.acp_agents().len(), 8);
        let again = EngineService::new(store, EventBus::new(), providers);
        assert_eq!(again.acp_agents().len(), 8);
    }

    /// F-EN-10 review: an agent edited into the file past the form's rules
    /// isn't read back.
    #[test]
    fn f_en_10_4_a_saved_agent_that_breaks_the_rules_isnt_read() {
        let dir = tempfile::tempdir().expect("tempdir");
        let file = dir.path().join(ACP_AGENTS_FILE);
        let program = dir.path().join("agent");
        let mut ok = agent_at(&program, "Fine");
        ok.id = "acp-fine-00000001".into();
        let mut bad = agent_at(&program, "Preload");
        bad.id = "acp-preload-00000002".into();
        bad.env.insert("LD_PRELOAD".into(), "/tmp/x.so".into());
        std::fs::write(
            &file,
            serde_json::to_vec(&vec![ok.clone(), bad]).expect("json"),
        )
        .expect("write");
        assert_eq!(read_agents(&file), vec![ok]);
    }

    /// F-EN-10 review: hosts, versions and terminal panes see the registry
    /// without added agents (D-2513).
    #[test]
    fn f_en_10_1_the_base_registry_has_no_added_agents() {
        let dir = tempfile::tempdir().expect("tempdir");
        let data = tandem_core::DataDir::at(dir.path().join("data")).expect("data");
        let store = Store::open(&data).expect("store");
        let program = dir.path().join("agent");
        std::fs::write(&program, "#!/bin/sh\n").expect("program");
        let svc = EngineService::new(store, EventBus::new(), data.root().join("providers"));
        let agent = svc
            .add_acp_agent(agent_at(&program, "Mine"), 3)
            .expect("add");
        assert!(svc.registry().get(&agent.id).is_some());
        assert!(svc.base_registry().get(&agent.id).is_none());
        assert!(svc.base_registry().get("claude").is_some());
    }
}
