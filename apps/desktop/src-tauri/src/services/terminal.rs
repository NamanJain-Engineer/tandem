//! Plain-shell terminals (F-CD-03, M2 slice): one login shell per pane with
//! the captured login environment, streaming over a per-pane channel.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde::Serialize;

use tandem_core::bus::EventBus;
use tandem_core::clock::SystemClock;
use tandem_core::ids::PaneId;
use tandem_core::settings::SettingKey;
use tandem_core::shellquote::{self, ShellDialect};
use tandem_core::status::{AgentStatus, SessionKey, WaitingReason};
use tandem_core::{ErrorCode, IpcError};
use tandem_engines::launch::StatusEnv;
use tandem_engines::EngineSpec;

use crate::services::engines::{EngineService, Relaunch, LAUNCH_CONFIG_UNAVAILABLE};
use crate::services::status::StatusService;
use tandem_pty::env::{self, LoginEnv};
use tandem_pty::integration::{self, IntegratedShell};
use tandem_pty::{typing, OutputSink, PtyInfo, PtyManager, SpawnSpec};
use tandem_store::repo::panes::PaneRecord;
use tandem_store::repo::sessions::{LaunchConfig, PaneSession};
use tandem_store::repo::{panes, restore, sessions, settings, workspaces};
use tandem_store::Store;

async fn capture_login() -> LoginEnv {
    let shell = env::default_shell();
    tokio::task::spawn_blocking(move || env::capture(&shell, env::CAPTURE_TIMEOUT))
        .await
        .unwrap_or_default()
}

/// A fresh secret for one launch's marks (handback, shell integration).
pub fn launch_token() -> String {
    use rand::distributions::{Alphanumeric, DistString};
    Alphanumeric.sample_string(&mut rand::thread_rng(), 32)
}

/// Quiet after the first output that counts as ready (§6.5).
pub const READY_QUIET_MS: i64 = 1_500;
/// An agent's prompt waits at most this long for its checkpoint (F-EX-01).
pub const PROMPT_WAIT: Duration = Duration::from_secs(2);
/// How long `type_prompt` waits for a TUI to become ready.
pub const READY_TIMEOUT: Duration = Duration::from_secs(10);

pub const FOLDER_NOT_FOUND: &str = "Folder not found — opened at workspace root";
/// Lines kept per pane in `$DATA/scrollback/<pane>.bin` (F-CD-03).
pub const SNAPSHOT_LINES: usize = 2_000;
/// Live panes are also snapshotted this often, to survive a crash (§5.4).
pub const SNAPSHOT_EVERY: std::time::Duration = std::time::Duration::from_secs(300);

/// [`SNAPSHOT_EVERY`], or `TANDEM_SNAPSHOT_EVERY_MS` in debug builds (the
/// crash-restore E2E can't wait five minutes).
pub fn snapshot_every() -> Duration {
    if cfg!(debug_assertions) {
        if let Some(ms) = std::env::var("TANDEM_SNAPSHOT_EVERY_MS")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .filter(|ms| *ms >= 100)
        {
            return Duration::from_millis(ms);
        }
    }
    SNAPSHOT_EVERY
}

/// How a pane starts (F-SH-07).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaunchMode {
    /// A shell, or a new agent: refused while an agent waits on its card.
    Normal,
    /// The pane's saved conversation, with its saved configuration.
    Resume,
    /// A new conversation; the saved one moves to Saved sessions.
    Fresh,
}

/// What Resume relaunches (F-SH-07).
#[derive(Debug, Clone)]
pub struct ResumePlan {
    pub session: PaneSession,
    pub engine_session_id: String,
    pub config: LaunchConfig,
}

pub const AGENT_PAUSED: &str = "This agent is paused. Resume it or start fresh from its card.";

pub struct TerminalService {
    pub pty: PtyManager,
    pub engines: Arc<EngineService>,
    pub status: Arc<StatusService>,
    /// The quoting dialect of each pane's shell (drops, staged images).
    dialects: std::sync::Mutex<std::collections::HashMap<String, ShellDialect>>,
    store: Store,
    /// The login environment (§5.2): captured on first use, again on Check again.
    login: tokio::sync::RwLock<Option<Arc<LoginEnv>>>,
    capturing: tokio::sync::Mutex<()>,
    spawn_log: Option<PathBuf>,
    /// `$DATA/shell-integration` (F-CD-10); `None` turns integration off.
    integration_root: Option<PathBuf>,
    /// The installed scripts' folder, written on first use.
    integration_dir: std::sync::OnceLock<Option<PathBuf>>,
    /// F-EX-01: saves the pane's folder before an agent gets a prompt.
    prompt_hook: std::sync::OnceLock<PromptHook>,
    /// Agent panes whose input goes through an ordered queue, so Enter can
    /// wait for its checkpoint without anything typed after it overtaking.
    agent_inputs: std::sync::Mutex<
        std::collections::HashMap<String, tokio::sync::mpsc::UnboundedSender<Vec<u8>>>,
    >,
    agent_panes: std::sync::Mutex<std::collections::HashSet<String>>,
    /// SSH hosts: panes of remote workspaces run there (F-RM-01).
    remote: std::sync::OnceLock<Arc<crate::services::remote::RemoteService>>,
    /// WSL distributions: panes of WSL workspaces run there (F-RM-02).
    wsl: std::sync::OnceLock<Arc<crate::services::wsl::WslService>>,
    /// Panes running in a WSL distro: drops are Linux paths there.
    wsl_panes: std::sync::Mutex<std::collections::HashSet<String>>,
    /// The tasks server terminal agents get (F-TK-02).
    tasks: std::sync::OnceLock<Arc<crate::services::tasks::TasksService>>,
}

/// Takes a checkpoint of an agent pane's folder (F-EX-01). The flag turns
/// true when the prompt went ahead without waiting for it.
pub type PromptHook = Arc<
    dyn Fn(PaneId, Arc<std::sync::atomic::AtomicBool>) -> futures::future::BoxFuture<'static, ()>
        + Send
        + Sync,
>;

/// One line of `TANDEM_SPAWN_LOG` (debug builds): what was started, with
/// env keys only.
#[derive(Debug, Clone, Serialize, serde::Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SpawnRecord {
    pub argv: Vec<String>,
    pub env_keys: Vec<String>,
    /// Env keys whose values look like secrets (F-EN-04 AC1: must be empty).
    pub secret_env_keys: Vec<String>,
    pub provider_home: Option<String>,
    pub cwd: String,
}

impl TerminalService {
    pub fn new(pty: PtyManager, store: Store) -> Arc<Self> {
        Self::with_integration(pty, store, None)
    }

    /// Where a pane's relative paths start (F-CD-09): the shell's folder as
    /// its last OSC 7 said, else the folder the pane opens in.
    pub async fn link_cwd(&self, pane: &PaneId) -> Option<PathBuf> {
        if let Some(cwd) = self.pty.state(pane.as_str()).and_then(|s| s.cwd) {
            return Some(PathBuf::from(cwd));
        }
        let record = panes::get(&self.store, pane).await.ok()?;
        match record.cwd {
            Some(cwd) => Some(PathBuf::from(cwd)),
            None => workspaces::get(&self.store, &record.workspace_id)
                .await
                .ok()
                .map(|w| PathBuf::from(w.root)),
        }
    }

    /// `integration_root` holds the shell-integration scripts (F-CD-10).
    pub fn with_integration(
        pty: PtyManager,
        store: Store,
        integration_root: Option<PathBuf>,
    ) -> Arc<Self> {
        let bus = EventBus::new();
        let providers = store
            .path()
            .parent()
            .map(|d| d.join("providers"))
            .unwrap_or_else(|| PathBuf::from("providers"));
        let engines = EngineService::new(store.clone(), bus.clone(), providers);
        let status = StatusService::new(bus, Arc::new(SystemClock));
        Self::with_services(pty, store, integration_root, engines, status)
    }

    /// With the app's engine and status services.
    pub fn with_services(
        pty: PtyManager,
        store: Store,
        integration_root: Option<PathBuf>,
        engines: Arc<EngineService>,
        status: Arc<StatusService>,
    ) -> Arc<Self> {
        let spawn_log = if cfg!(debug_assertions) {
            std::env::var_os("TANDEM_SPAWN_LOG").map(PathBuf::from)
        } else {
            None
        };
        Arc::new(Self {
            pty,
            engines,
            status,
            dialects: Default::default(),
            store,
            login: tokio::sync::RwLock::new(None),
            capturing: tokio::sync::Mutex::new(()),
            spawn_log,
            integration_root,
            integration_dir: std::sync::OnceLock::new(),
            prompt_hook: std::sync::OnceLock::new(),
            agent_inputs: Default::default(),
            agent_panes: Default::default(),
            remote: std::sync::OnceLock::new(),
            wsl: std::sync::OnceLock::new(),
            wsl_panes: Default::default(),
            tasks: std::sync::OnceLock::new(),
        })
    }

    pub fn set_prompt_hook(&self, hook: PromptHook) {
        let _ = self.prompt_hook.set(hook);
    }

    /// Runs the prompt hook, waiting at most [`PROMPT_WAIT`]; the
    /// checkpoint carries on (marked late) if it takes longer.
    pub async fn before_prompt(&self, pane: &PaneId) {
        let Some(hook) = self.prompt_hook.get() else {
            return;
        };
        let late = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let task = tokio::spawn(hook(pane.clone(), late.clone()));
        if tokio::time::timeout(PROMPT_WAIT, task).await.is_err() {
            late.store(true, std::sync::atomic::Ordering::SeqCst);
            tracing::info!(
                code = "checkpoint.late",
                "a prompt went ahead before its checkpoint"
            );
        }
    }

    /// `pty_write`. Agent panes with a prompt hook get their input in order
    /// through a queue, and Enter waits for the checkpoint (F-EX-01 AC9).
    pub fn write_input(self: &Arc<Self>, pane: &PaneId, data: &[u8]) -> Result<(), IpcError> {
        let gated = self.prompt_hook.get().is_some()
            && self
                .agent_panes
                .lock()
                .is_ok_and(|a| a.contains(pane.as_str()));
        if !gated {
            return Ok(self.pty.write(pane.as_str(), data)?);
        }
        let mut inputs = self.agent_inputs.lock().unwrap_or_else(|e| e.into_inner());
        let tx = inputs
            .entry(pane.as_str().to_string())
            .or_insert_with(|| {
                let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<Vec<u8>>();
                let this = self.clone();
                let pane = pane.clone();
                tauri::async_runtime::spawn(async move {
                    while let Some(data) = rx.recv().await {
                        if data.contains(&b'\r') {
                            this.before_prompt(&pane).await;
                        }
                        if this.pty.write(pane.as_str(), &data).is_err() {
                            break;
                        }
                    }
                    if let Ok(mut inputs) = this.agent_inputs.lock() {
                        inputs.remove(pane.as_str());
                    }
                });
                tx
            })
            .clone();
        drop(inputs);
        tx.send(data.to_vec())
            .map_err(|_| IpcError::new(ErrorCode::NotFound, "This pane isn't running"))
    }

    pub fn set_wsl(&self, wsl: Arc<crate::services::wsl::WslService>) {
        let _ = self.wsl.set(wsl);
    }

    pub fn wsl(&self) -> Option<&Arc<crate::services::wsl::WslService>> {
        self.wsl.get()
    }

    pub fn store(&self) -> &Store {
        &self.store
    }

    pub fn set_remote(&self, remote: Arc<crate::services::remote::RemoteService>) {
        let _ = self.remote.set(remote);
    }

    pub fn remote(&self) -> Option<&Arc<crate::services::remote::RemoteService>> {
        self.remote.get()
    }

    /// The tracker whose MCP server terminal agents get.
    pub fn bind_tasks(&self, tasks: &Arc<crate::services::tasks::TasksService>) {
        let _ = self.tasks.set(tasks.clone());
    }

    /// The pane's agent session ended or the pane closed: its task
    /// connection is revoked.
    pub fn end_tasks_session(&self, pane: &str) {
        if let Some(tasks) = self.tasks.get() {
            tasks.end_session(&format!("pn:{pane}"));
        }
    }

    /// "Expose tasks to terminal agents" (F-TK-02): the tasks server goes in
    /// (or out of) the engine's own config in the pane's provider home, and
    /// the pane gets a fresh connection in its env.
    async fn tasks_env(
        &self,
        engine: &tandem_engines::registry::EngineSpec,
        home: Option<&Path>,
        pane: &str,
    ) -> Vec<(String, String)> {
        let (Some(tasks), Some(home)) = (self.tasks.get(), home) else {
            return Vec::new();
        };
        let on = tasks.expose_to_terminals().await;
        let server = if on { tasks.terminal_server() } else { None };
        if let Err(e) = self.engines.homes.set_stdio_server(
            engine,
            home,
            crate::services::tasks::SERVER_NAME,
            server.as_ref(),
        ) {
            tracing::warn!(code = "tasks.native_config_failed", error = %e, "couldn't update the engine's MCP config");
            return Vec::new();
        }
        if server.is_none() {
            return Vec::new();
        }
        match tasks.begin_session(&format!("pn:{pane}"), &engine.label, "me") {
            Some(conn) => vec![(crate::services::tasks::CONNECTION_ENV.to_string(), conn)],
            None => Vec::new(),
        }
    }

    /// The shell's integration and the scripts' folder, unless the shell has
    /// none or Settings › Terminal turned it off for that shell.
    async fn integration_for(&self, shell: &Path) -> Option<(IntegratedShell, PathBuf)> {
        let kind = IntegratedShell::detect(shell)?;
        let root = self.integration_root.as_ref()?;
        let enabled = settings::get(&self.store, SettingKey::TermShellIntegration)
            .await
            .ok()
            .and_then(|v| v.get(kind.setting_key()).and_then(|b| b.as_bool()))
            .unwrap_or(true);
        if !enabled {
            return None;
        }
        let dir = self
            .integration_dir
            .get_or_init(|| match integration::install(root) {
                Ok(dir) => Some(dir),
                Err(err) => {
                    tracing::warn!(code = "pty.integration_install_failed", error = %err, "shell integration is off: couldn't write its scripts");
                    None
                }
            })
            .clone()?;
        Some((kind, dir))
    }

    /// Uses `login` instead of capturing one (tests).
    pub fn set_login_env(&self, login: LoginEnv) {
        if let Ok(mut l) = self.login.try_write() {
            *l = Some(Arc::new(login));
        }
    }

    /// Captured once per launch (§5.2); the first caller waits ≤3 s.
    pub async fn login_env(&self) -> Arc<LoginEnv> {
        if let Some(l) = self.login.read().await.clone() {
            return l;
        }
        let _one = self.capturing.lock().await;
        if let Some(l) = self.login.read().await.clone() {
            return l;
        }
        let l = Arc::new(capture_login().await);
        *self.login.write().await = Some(l.clone());
        l
    }

    /// Captures the login environment again (Check again, §6.2): a PATH
    /// fixed in the profile counts without restarting the app.
    pub async fn refresh_login_env(&self) -> Arc<LoginEnv> {
        let _one = self.capturing.lock().await;
        let fresh = capture_login().await;
        let mut slot = self.login.write().await;
        // A shell that's slow right now doesn't throw away a good capture.
        if fresh.degraded {
            if let Some(prev) = slot.as_ref().filter(|p| !p.degraded) {
                return prev.clone();
            }
        }
        let l = Arc::new(fresh);
        *slot = Some(l.clone());
        l
    }

    /// Writes each pane's last 2,000 lines to `dir/<pane>.bin` (restore display only).
    pub fn snapshot_all(&self, dir: &Path) {
        for pane in self.pty.panes() {
            // Pane ids are `pn_<ulid>`; anything else never becomes a file name.
            if !pane.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                continue;
            }
            let Ok(tail) = self.pty.tail(&pane, SNAPSHOT_LINES) else {
                continue;
            };
            if tail.is_empty() {
                continue;
            }
            let _ = std::fs::create_dir_all(dir);
            if let Err(err) = tandem_core::atomic::atomic_write(
                &dir.join(format!("{pane}.bin")),
                &tail,
                Some(0o600),
            ) {
                tracing::warn!(code = "pty.snapshot_failed", error = %err, "couldn't save a scrollback snapshot");
            }
        }
    }

    /// The quit path: snapshots first, then every process tree.
    pub fn shutdown(&self, snapshots: &Path) {
        self.snapshot_all(snapshots);
        self.pty.shutdown();
    }

    /// The plan's live-pane limit: at the limit no pane or process is
    /// created and the picker shows the upgrade card (F-CD-03).
    /// Room for `opening` more live panes under the plan's cap: a lineup
    /// launches whole or not at all (F-CD-14).
    pub fn check_room(&self, max_live: Option<u32>, opening: u32) -> Result<(), IpcError> {
        match max_live {
            Some(max) if self.pty.live_count() as u32 + opening > max => Err(IpcError::new(
                ErrorCode::LimitReached,
                format!("Free includes {max} live panes"),
            )
            .with_stage("panes")),
            _ => Ok(()),
        }
    }

    /// Types `text` as the pane's first prompt once its view has started it
    /// and its TUI is ready (F-CD-14 AC1). Timeout when it never starts.
    pub async fn type_prompt_when_live(
        &self,
        pane: &PaneId,
        text: &str,
        timeout: Duration,
    ) -> Result<(), IpcError> {
        let until = std::time::Instant::now() + timeout;
        while !self.pty.is_live(pane.as_str()) {
            if std::time::Instant::now() >= until {
                return Err(IpcError::new(ErrorCode::Timeout, "The pane didn't start"));
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        self.type_prompt(pane, text).await
    }

    pub fn check_live_limit(&self, max_live: Option<u32>) -> Result<(), IpcError> {
        match max_live {
            Some(max) if self.pty.live_count() as u32 >= max => Err(IpcError::new(
                ErrorCode::LimitReached,
                format!("Free includes {max} live panes"),
            )
            .with_stage("panes")),
            _ => Ok(()),
        }
    }

    pub async fn spawn(
        &self,
        pane: &PaneId,
        cols: u16,
        rows: u16,
        max_live: Option<u32>,
        sink: Arc<dyn OutputSink>,
    ) -> Result<PtyInfo, IpcError> {
        self.spawn_with(pane, cols, rows, max_live, Some(sink), LaunchMode::Normal)
            .await
    }

    /// Whether a paused agent can resume, and with what (F-SH-07). Errors
    /// are the card's reason: "Launch configuration unavailable" when the
    /// saved configuration is gone, never a fallback to defaults.
    pub async fn resume_plan(&self, record: &PaneRecord) -> Result<ResumePlan, IpcError> {
        let engine = record
            .engine_id
            .as_deref()
            .and_then(|id| self.engines.get(id))
            .ok_or_else(|| IpcError::new(ErrorCode::NotFound, "This pane's engine isn't known"))?;
        if !engine.supports_resume && engine.id != "codex" {
            return Err(IpcError::new(
                ErrorCode::Unavailable,
                format!("{} can't resume sessions", engine.label),
            )
            .with_stage("resume"));
        }
        let no_session = || {
            IpcError::new(
                ErrorCode::Unavailable,
                "There's no saved conversation to resume",
            )
            .with_stage("resume")
        };
        let session = sessions::latest_for_pane(&self.store, &record.id)
            .await?
            .ok_or_else(no_session)?;
        let engine_session_id = session.engine_session_id.clone().ok_or_else(no_session)?;
        let unavailable = || {
            IpcError::new(ErrorCode::Unavailable, LAUNCH_CONFIG_UNAVAILABLE)
                .with_stage("launch_config")
        };
        let config = match &session.launch_config {
            Some(Ok(c)) => c.clone(),
            Some(Err(_)) => return Err(unavailable()),
            // Rows from before configs were saved: the exact home they used.
            None => LaunchConfig {
                extra_args: Vec::new(),
                home: session.provider_home.clone(),
            },
        };
        if config
            .home
            .as_deref()
            .is_some_and(|h| !Path::new(h).is_dir())
        {
            return Err(unavailable());
        }
        Ok(ResumePlan {
            session,
            engine_session_id,
            config,
        })
    }

    /// Starts a pane's process. `sink: None` keeps output in the ring until
    /// a view attaches (restores started from the queue).
    pub async fn spawn_with(
        &self,
        pane: &PaneId,
        cols: u16,
        rows: u16,
        max_live: Option<u32>,
        sink: Option<Arc<dyn OutputSink>>,
        mode: LaunchMode,
    ) -> Result<PtyInfo, IpcError> {
        self.check_live_limit(max_live)?;
        let record = panes::get_open(&self.store, pane).await?;
        if record.kind == "agent" {
            if let Ok(mut agents) = self.agent_panes.lock() {
                agents.insert(pane.as_str().to_string());
            }
        }
        let waiting_on_card = record.kind == "agent"
            && matches!(record.restore_state.as_str(), "paused" | "cancelled");
        if mode == LaunchMode::Normal && waiting_on_card {
            // Nothing an agent runs starts again without consent (F-SH-07).
            return Err(IpcError::new(ErrorCode::Conflict, AGENT_PAUSED).with_stage("restore"));
        }
        let plan = match (mode, record.kind.as_str()) {
            (LaunchMode::Resume, "agent") => Some(self.resume_plan(&record).await?),
            (LaunchMode::Resume, _) => {
                return Err(IpcError::new(ErrorCode::Invalid, "Only agent panes resume"))
            }
            _ => None,
        };
        if mode == LaunchMode::Fresh && record.kind == "agent" {
            // The old conversation stays under Saved sessions.
            sessions::save_on_handback(&self.store, pane, None).await?;
        }
        let ws = workspaces::get(&self.store, &record.workspace_id).await?;
        if ws.kind == "wsl" {
            return self
                .spawn_in_wsl(pane, &record, &ws, cols, rows, sink, mode)
                .await;
        }
        if ws.kind == "ssh" {
            if plan.is_some() {
                return Err(IpcError::new(
                    ErrorCode::Unavailable,
                    "Agents on remote hosts start fresh: they can't resume a saved conversation",
                )
                .with_stage("resume"));
            }
            return self
                .spawn_on_host(pane, &record, &ws, cols, rows, sink, mode)
                .await;
        }
        let root = PathBuf::from(&ws.root);
        if !root.is_dir() {
            return Err(IpcError::new(
                ErrorCode::NotFound,
                "This workspace's folder is missing. Locate it to open terminals.",
            ));
        }
        let wanted = record
            .cwd
            .as_ref()
            .map(PathBuf::from)
            .unwrap_or_else(|| root.clone());
        let (cwd, notice) = if wanted.is_dir() {
            (wanted, None)
        } else {
            (root.clone(), Some(FOLDER_NOT_FOUND))
        };

        if record.kind != "agent" {
            if let Some(distro) = self.plain_wsl_distro().await {
                if let Some(linux) = tandem_core::wslpath::to_wsl_path(&cwd.to_string_lossy()) {
                    return self
                        .launch_wsl(pane, &distro, &linux, None, cols, rows, sink, mode)
                        .await;
                }
            }
        }
        let login = self.login_env().await;
        let configured = settings::get(&self.store, SettingKey::DefaultShell).await?;
        let shell = configured
            .as_str()
            .filter(|s| !s.is_empty() && Path::new(s).is_absolute())
            .map(PathBuf::from)
            .unwrap_or_else(|| login.shell.clone());
        let scrollback = settings::get(&self.store, SettingKey::TermScrollback)
            .await?
            .as_u64()
            .unwrap_or(10_000) as usize;
        // Every launch gets its own token; shell-integration and handback
        // marks must carry it (§5.8).
        let token = launch_token();
        let integration = self.integration_for(&shell).await;
        // Agent panes run the engine under the login shell and hand back to
        // it when the engine exits (F-CD-03 AC3).
        let (args, extra_env, engine, launched) = if record.kind == "agent" {
            let engine = record
                .engine_id
                .as_deref()
                .and_then(|id| self.engines.get(id))
                .ok_or_else(|| {
                    IpcError::new(ErrorCode::NotFound, "This pane's engine isn't known")
                })?;
            let path = self.resolve_engine(&engine).await.ok_or_else(|| {
                IpcError::new(
                    ErrorCode::EngineMissing,
                    format!("{} isn't installed on this machine.", engine.label),
                )
            })?;
            // Status hooks report with this launch's secret (§6.6).
            let status = self
                .status
                .watch(
                    pane.as_str(),
                    &engine.waiting_patterns,
                    &engine.limit_patterns,
                    record.account_id.as_ref().map(|a| a.as_str()),
                )
                .map(|(url, secret)| StatusEnv { url, secret });
            let relaunch = plan.as_ref().map(|p| Relaunch {
                resume: Some(p.engine_session_id.as_str()),
                config: &p.config,
            });
            // A resumed session keeps its account; a new one takes the pane's
            // explicit pick, then the workspace's override, then the
            // engine's default (F-AC-03; teammates arrive with M11).
            let workspace_pick = match (&plan, &record.account_id) {
                (None, None) => tandem_store::repo::accounts::workspace_override(
                    &self.store,
                    &record.workspace_id,
                    &engine.id,
                )
                .await
                .ok()
                .flatten(),
                _ => None,
            };
            let chosen = tandem_core::accounts::resolve(
                record.account_id.as_ref(),
                None,
                workspace_pick.as_ref(),
                None,
            );
            let account = match &plan {
                Some(p) => p.session.account_id.as_ref(),
                None => chosen.as_ref(),
            };
            let launch = self
                .engines
                .agent_launch_with(&engine, path, &root, status, account, relaunch)
                .await
                .inspect_err(|_| self.status.unwatch(pane.as_str()))?;
            let args = env::agent_wrapper(
                &shell,
                &launch.spec.argv(),
                integration.as_ref().map(|(_, dir)| dir.as_path()),
            )
            .map_err(|_| {
                self.status.unwatch(pane.as_str());
                IpcError::new(
                    ErrorCode::Invalid,
                    "The engine's path can't be launched safely",
                )
            })?;
            let mut env: Vec<(String, String)> = launch.spec.env.clone().into_iter().collect();
            env.extend(
                self.tasks_env(&engine, launch.home.as_deref(), pane.as_str())
                    .await,
            );
            (args, env, Some(engine), Some(launch))
        } else if let Some((kind, dir)) = &integration {
            let launch =
                integration::launch(*kind, dir, login.vars.get("ZDOTDIR").map(String::as_str))
                    .map_err(|_| {
                        IpcError::new(
                            ErrorCode::Invalid,
                            "The shell-integration folder can't be used safely",
                        )
                    })?;
            (launch.args, launch.env, None, None)
        } else {
            (env::login_args(&shell), Vec::new(), None, None)
        };
        if let Ok(mut d) = self.dialects.lock() {
            d.insert(
                pane.to_string(),
                ShellDialect::for_shell(&shell.to_string_lossy()),
            );
        }
        let mut child_env = env::pane_env(&login, pane.as_str(), env!("CARGO_PKG_VERSION"));
        if integration.is_some() || engine.is_some() {
            child_env.insert(env::TOKEN_VAR.to_string(), token.clone());
        }
        child_env.extend(extra_env);
        let spec = SpawnSpec {
            pane_id: pane.to_string(),
            args,
            program: shell,
            cwd,
            env: child_env,
            cols,
            rows,
            scrollback_lines: scrollback,
            mark_token: Some(token),
        };
        let home = launched.as_ref().and_then(|l| l.home.clone());
        self.log_spawn(&spec, home.as_deref());
        // The session row exists before the engine starts: its first hook
        // (with the session id Resume needs) can arrive at once (F-SH-07).
        let new_session = match (engine.as_ref(), launched.as_ref(), &plan) {
            (Some(_), Some(_), Some(p)) => {
                sessions::reopen(&self.store, &p.session.id).await?;
                None
            }
            (Some(engine), Some(launched), None) => {
                let account = launched.account.as_ref().map(|a| a.id.clone());
                Some(
                    sessions::start_with_config(
                        &self.store,
                        pane,
                        &record.workspace_id,
                        &engine.id,
                        account.as_ref(),
                        &launched.config,
                    )
                    .await?,
                )
            }
            _ => None,
        };
        let spawned = self.pty.spawn(spec, sink);
        let info = match spawned {
            Ok(info) => info,
            Err(err) => {
                if engine.is_some() {
                    self.status.unwatch(pane.as_str());
                }
                if let Some(id) = &new_session {
                    let _ = sessions::discard(&self.store, id).await;
                }
                return Err(err.into());
            }
        };
        // The pane (or its workspace) may have closed while the shell
        // started: never leave a process nobody can see.
        if panes::get_open(&self.store, pane).await.is_err() {
            let _ = self.pty.kill(pane.as_str());
            if let Some(id) = &new_session {
                let _ = sessions::discard(&self.store, id).await;
            }
            return Err(IpcError::new(ErrorCode::NotFound, "This pane was closed"));
        }
        if mode != LaunchMode::Normal {
            restore::set_state(&self.store, pane, "live").await?;
        }
        if let Some(text) = notice {
            let _ = self.pty.notice(pane.as_str(), text);
        }
        tracing::info!(code = "pty.spawned", pid = info.pid, "terminal started");
        Ok(info)
    }

    /// A pane of a WSL workspace (F-RM-02): `wsl.exe -d <distro> --cd
    /// <folder>`, an agent first when it's an agent pane (the engine found
    /// inside the distro). Only `WSLENV`'s variables cross into the distro.
    #[allow(clippy::too_many_arguments)]
    async fn spawn_in_wsl(
        &self,
        pane: &PaneId,
        record: &PaneRecord,
        ws: &tandem_store::repo::workspaces::Workspace,
        cols: u16,
        rows: u16,
        sink: Option<Arc<dyn OutputSink>>,
        mode: LaunchMode,
    ) -> Result<PtyInfo, IpcError> {
        if self.wsl.get().is_none() || crate::services::wsl::system().is_none() {
            return Err(
                IpcError::new(ErrorCode::Unavailable, "WSL folders open on Windows only")
                    .with_stage("wsl"),
            );
        }
        let distro = ws.wsl_distro.clone().ok_or_else(|| {
            IpcError::new(ErrorCode::Invalid, "This WSL workspace has no distribution")
        })?;
        let host = tandem_core::ids::HostId::from_db(ws.host_id.clone());
        let cwd = record.cwd.clone().unwrap_or_else(|| ws.root.clone());
        let mut command = None;
        if record.kind == "agent" {
            let engine = record
                .engine_id
                .as_deref()
                .and_then(|id| self.engines.get(id))
                .ok_or_else(|| {
                    IpcError::new(ErrorCode::NotFound, "This pane's engine isn't known")
                })?;
            self.engines.check_allowed(&engine)?;
            let found = match self.remote.get() {
                Some(r) => r.report(&host).await,
                None => None,
            }
            .and_then(|r| r.get(&engine.id).map(|f| PathBuf::from(&f.path)))
            .ok_or_else(|| {
                IpcError::new(
                    ErrorCode::EngineMissing,
                    format!("{} isn't installed in {distro}.", engine.label),
                )
            })?;
            let req = tandem_engines::launch::TerminalLaunchRequest {
                engine: &engine,
                program: found,
                extra_args: self.engines.extra_args(&engine.id).await,
                resume: None,
                host: tandem_engines::launch::HostKind::Wsl,
                home: None,
                providers_root: PathBuf::new(),
                status: None,
            };
            let launch = tandem_engines::launch::terminal_launch(&req).map_err(|e| {
                IpcError::new(ErrorCode::Invalid, e.to_string()).with_stage("launch")
            })?;
            let _ = self.status.watch(
                pane.as_str(),
                &engine.waiting_patterns,
                &engine.limit_patterns,
                None,
            );
            command = Some(tandem_host::quote_argv(&launch.argv()));
        }
        self.launch_wsl(
            pane,
            &distro,
            &cwd,
            command.as_deref(),
            cols,
            rows,
            sink,
            mode,
        )
        .await
    }

    /// Settings › Remote "Use for plain terminals" (F-RM-02): the distro a
    /// local workspace's plain terminals open in, on Windows.
    async fn plain_wsl_distro(&self) -> Option<String> {
        self.wsl.get()?;
        crate::services::wsl::system()?;
        let value = settings::get(&self.store, SettingKey::WslDefaultDistro)
            .await
            .ok()?;
        plain_distro(&value)
    }

    /// `wsl.exe` in a pane: the distro, its folder and, for an agent, its
    /// command. Only `WSLENV`'s variables cross into the distro.
    #[allow(clippy::too_many_arguments)]
    async fn launch_wsl(
        &self,
        pane: &PaneId,
        distro: &str,
        cwd: &str,
        command: Option<&str>,
        cols: u16,
        rows: u16,
        sink: Option<Arc<dyn OutputSink>>,
        mode: LaunchMode,
    ) -> Result<PtyInfo, IpcError> {
        let scrollback = settings::get(&self.store, SettingKey::TermScrollback)
            .await?
            .as_u64()
            .unwrap_or(10_000) as usize;
        let (program, args) = crate::services::wsl::pane_command(distro, cwd, command);
        let login = self.login_env().await;
        let mut child_env = env::pane_env(&login, pane.as_str(), env!("CARGO_PKG_VERSION"));
        child_env.insert("WSLENV".into(), tandem_core::wslpath::WSLENV.into());
        if let Ok(mut d) = self.dialects.lock() {
            d.insert(pane.to_string(), ShellDialect::Posix);
        }
        if let Ok(mut w) = self.wsl_panes.lock() {
            w.insert(pane.to_string());
        }
        let spec = SpawnSpec {
            pane_id: pane.to_string(),
            program: PathBuf::from(program),
            args,
            cwd: std::env::var_os("USERPROFILE")
                .map(PathBuf::from)
                .unwrap_or_else(std::env::temp_dir),
            env: child_env,
            cols,
            rows,
            scrollback_lines: scrollback,
            mark_token: None,
        };
        self.log_spawn(&spec, None);
        let info = self
            .pty
            .spawn(spec, sink)
            .inspect_err(|_| self.status.unwatch(pane.as_str()))?;
        if mode != LaunchMode::Normal {
            restore::set_state(&self.store, pane, "live").await?;
        }
        Ok(info)
    }

    /// A pane of an SSH workspace (F-RM-01): a login shell on the host, an
    /// agent first when it's an agent pane. Only the allowlisted variables
    /// cross (`tandem_host::env`): never a provider home or Vibezeo's own
    /// paths; the engine signs in with the host's own sign-in.
    #[allow(clippy::too_many_arguments)]
    async fn spawn_on_host(
        &self,
        pane: &PaneId,
        record: &PaneRecord,
        ws: &tandem_store::repo::workspaces::Workspace,
        cols: u16,
        rows: u16,
        sink: Option<Arc<dyn OutputSink>>,
        mode: LaunchMode,
    ) -> Result<PtyInfo, IpcError> {
        let remote = self.remote.get().cloned().ok_or_else(|| {
            IpcError::new(
                ErrorCode::Unavailable,
                "Remote hosts aren't available in this build",
            )
        })?;
        let host = tandem_core::ids::HostId::from_db(ws.host_id.clone());
        let scrollback = settings::get(&self.store, SettingKey::TermScrollback)
            .await?
            .as_u64()
            .unwrap_or(10_000) as usize;
        let mut env = std::collections::BTreeMap::new();
        env.insert("TERM".to_string(), "xterm-256color".to_string());
        env.insert("COLORTERM".to_string(), "truecolor".to_string());
        env.insert("TERM_PROGRAM".to_string(), "Vibezeo".to_string());
        env.insert("TANDEM_PANE_ID".to_string(), pane.to_string());
        let mut command = None;
        if record.kind == "agent" {
            let engine = record
                .engine_id
                .as_deref()
                .and_then(|id| self.engines.get(id))
                .ok_or_else(|| {
                    IpcError::new(ErrorCode::NotFound, "This pane's engine isn't known")
                })?;
            // The org's engine policy holds on hosts too (F-BL-06).
            self.engines.check_allowed(&engine)?;
            let found = remote
                .report(&host)
                .await
                .and_then(|r| r.get(&engine.id).map(|f| PathBuf::from(&f.path)))
                .ok_or_else(|| {
                    IpcError::new(
                        ErrorCode::EngineMissing,
                        format!("{} isn't installed on this host.", engine.label),
                    )
                })?;
            let req = tandem_engines::launch::TerminalLaunchRequest {
                engine: &engine,
                program: found,
                extra_args: self.engines.extra_args(&engine.id).await,
                resume: None,
                host: tandem_engines::launch::HostKind::Ssh,
                home: None,
                providers_root: PathBuf::new(),
                status: None,
            };
            let launch = tandem_engines::launch::terminal_launch(&req).map_err(|e| {
                IpcError::new(ErrorCode::Invalid, e.to_string()).with_stage("launch")
            })?;
            // Heuristics from the relayed output; a hook's report would
            // need this secret (D-406).
            if let Some((_, secret)) = self.status.watch(
                pane.as_str(),
                &engine.waiting_patterns,
                &engine.limit_patterns,
                None,
            ) {
                env.insert("TANDEM_STATUS_SECRET".to_string(), secret);
            }
            command = Some(tandem_host::quote_argv(&launch.argv()));
        }
        if let Ok(mut d) = self.dialects.lock() {
            d.insert(pane.to_string(), ShellDialect::Posix);
        }
        let spec = SpawnSpec {
            pane_id: pane.to_string(),
            program: PathBuf::from("ssh"),
            args: command.iter().cloned().collect(),
            cwd: PathBuf::from(record.cwd.as_deref().unwrap_or(&ws.root)),
            env,
            cols,
            rows,
            scrollback_lines: scrollback,
            mark_token: None,
        };
        self.log_spawn(
            &SpawnSpec {
                env: tandem_host::env::allowed(&spec.env),
                ..spec.clone()
            },
            None,
        );
        let cwd = record.cwd.clone().unwrap_or_else(|| ws.root.clone());
        let spawned = remote
            .spawn_pane(&host, spec, &cwd, command.clone(), sink.clone())
            .await;
        let info = match spawned {
            Err(e) if e.code == ErrorCode::NotFound && cwd != ws.root => {
                // The pane's folder went: open at the workspace's root.
                let spec = SpawnSpec {
                    pane_id: pane.to_string(),
                    program: PathBuf::from("ssh"),
                    args: Vec::new(),
                    cwd: PathBuf::from(&ws.root),
                    env: std::collections::BTreeMap::from([
                        ("TERM".to_string(), "xterm-256color".to_string()),
                        ("TANDEM_PANE_ID".to_string(), pane.to_string()),
                    ]),
                    cols,
                    rows,
                    scrollback_lines: scrollback,
                    mark_token: None,
                };
                let info = remote
                    .spawn_pane(&host, spec, &ws.root, command, sink)
                    .await?;
                let _ = self.pty.notice(pane.as_str(), FOLDER_NOT_FOUND);
                info
            }
            Err(e) => {
                self.status.unwatch(pane.as_str());
                return Err(e);
            }
            Ok(info) => info,
        };
        if panes::get_open(&self.store, pane).await.is_err() {
            let _ = self.pty.kill(pane.as_str());
            return Err(IpcError::new(ErrorCode::NotFound, "This pane was closed"));
        }
        if mode != LaunchMode::Normal {
            restore::set_state(&self.store, pane, "live").await?;
        }
        tracing::info!(
            code = "pty.spawned_remote",
            pid = info.pid,
            "remote terminal started"
        );
        Ok(info)
    }

    /// Readiness (§6.5): the TUI set a title, showed a ready pattern, or went
    /// quiet for 1.5 s after its first output. False when `timeout` passes.
    pub async fn wait_ready(&self, pane: &PaneId, engine: &EngineSpec, timeout: Duration) -> bool {
        let patterns: Vec<regex::Regex> = engine
            .ready_patterns
            .iter()
            .filter_map(|p| regex::RegexBuilder::new(p).multi_line(true).build().ok())
            .collect();
        let until = std::time::Instant::now() + timeout;
        loop {
            let id = pane.as_str();
            if self.pty.state(id).is_some_and(|s| s.title.is_some()) {
                return true;
            }
            if !patterns.is_empty() {
                if let Some(screen) = self.pty.screen_text(id) {
                    if patterns.iter().any(|re| re.is_match(&screen)) {
                        return true;
                    }
                }
            }
            if let Some(last) = self.status.last_output(id) {
                if chrono::Utc::now().timestamp_millis() - last >= READY_QUIET_MS {
                    return true;
                }
            }
            if std::time::Instant::now() >= until || !self.pty.is_live(id) {
                return false;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }

    /// Types `command` and Enter into a shell pane once it's ready: its view
    /// has started it and it has been quiet for 1.5 s after its first output
    /// (the prompt is up). False when `timeout` passes first.
    pub async fn type_command_when_ready(
        &self,
        pane: &PaneId,
        command: &str,
        timeout: Duration,
    ) -> bool {
        let id = pane.as_str();
        let until = std::time::Instant::now() + timeout;
        // A plain shell isn't watched by the status store, so its output
        // time is never recorded: a screen that has stopped changing says
        // the prompt is up just as well (D-2208).
        let mut quiet = QuietScreen::default();
        loop {
            if self.pty.is_live(id) {
                let now = chrono::Utc::now().timestamp_millis();
                if let Some(last) = self.status.last_output(id) {
                    if now - last >= READY_QUIET_MS {
                        break;
                    }
                }
                if quiet.settled(now, self.pty.screen_text(id).as_deref()) {
                    break;
                }
            }
            if std::time::Instant::now() >= until {
                return false;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        self.pty
            .write(id, format!("{command}\r").as_bytes())
            .is_ok()
    }

    /// `type_prompt` (§6.5, F-EN-06 AC1–2).
    pub async fn type_prompt(&self, pane: &PaneId, text: &str) -> Result<(), IpcError> {
        self.type_prompt_with(pane, text, true).await
    }

    /// `type_prompt`, with Enter only when `send` (Conductor's `prompt_pane
    /// send:false` leaves the text in the agent's prompt).
    pub async fn type_prompt_with(
        &self,
        pane: &PaneId,
        text: &str,
        send: bool,
    ) -> Result<(), IpcError> {
        let record = panes::get_open(&self.store, pane).await?;
        let engine = record
            .engine_id
            .as_deref()
            .and_then(|id| self.engines.get(id));
        let name = record
            .human_name
            .clone()
            .or_else(|| engine.as_ref().map(|e| e.label.clone()))
            .unwrap_or_else(|| "This agent".into());
        let waiting = self
            .status
            .store
            .get(&SessionKey::pane(pane.as_str()))
            // A prompt waiting for input takes typing (Auto, F-AU-01); a
            // permission dialog or a question never does (F-EN-06 AC2).
            .is_some_and(|r| {
                r.status == AgentStatus::Waiting && r.reason != Some(WaitingReason::Input)
            });
        if waiting {
            return Err(IpcError::new(
                ErrorCode::Waiting,
                format!("{name} is waiting for your approval"),
            ));
        }
        if !self.pty.is_live(pane.as_str()) {
            return Err(IpcError::new(
                ErrorCode::NotFound,
                "This pane isn't running",
            ));
        }
        let newline = engine
            .as_ref()
            .map(|e| e.newline_sequence.clone())
            .unwrap_or_else(|| "\u{1b}\r".into());
        if let Some(engine) = &engine {
            self.wait_ready(pane, engine, READY_TIMEOUT).await;
        }
        let bracketed = self
            .pty
            .state(pane.as_str())
            .is_some_and(|s| s.bracketed_paste);
        let (body, enter) = typing::prompt_writes(text, bracketed, &newline);
        self.before_prompt(pane).await;
        self.status.quiet_window(pane.as_str());
        self.pty.write(pane.as_str(), &body)?;
        if send {
            tokio::time::sleep(typing::ENTER_DELAY).await;
            self.pty.write(pane.as_str(), &enter)?;
        }
        Ok(())
    }

    /// Quotes dropped paths for the pane's shell (F-CD-06): space-separated,
    /// no trailing newline; a name with control characters refuses the drop.
    pub fn quote_paths(&self, pane: &PaneId, paths: &[String]) -> Result<String, IpcError> {
        let dialect = self
            .dialects
            .lock()
            .ok()
            .and_then(|d| d.get(pane.as_str()).copied())
            .unwrap_or(if cfg!(windows) {
                ShellDialect::PowerShell
            } else {
                ShellDialect::Posix
            });
        // A WSL pane gets the distro's own paths (F-CD-06, F-RM-02 AC2).
        let in_wsl = self
            .wsl_panes
            .lock()
            .map(|w| w.contains(pane.as_str()))
            .unwrap_or(false);
        if in_wsl {
            let linux: Vec<String> = paths
                .iter()
                .map(|p| {
                    tandem_core::wslpath::to_wsl_path(p).ok_or_else(|| {
                        IpcError::new(ErrorCode::Invalid, format!("{p} can't be reached from WSL"))
                    })
                })
                .collect::<Result<_, _>>()?;
            return Ok(shellquote::quote_paths(&linux, ShellDialect::Posix)?);
        }
        Ok(shellquote::quote_paths(paths, dialect)?)
    }

    /// Marks a pane as running inside WSL (its drops are Linux paths).
    pub fn mark_wsl_pane(&self, pane: &str) {
        if let Ok(mut w) = self.wsl_panes.lock() {
            w.insert(pane.to_string());
        }
    }

    /// The engine's executable from the login PATH only, never the workspace
    /// folder (F-CD-03 AC6).
    pub async fn resolve_engine(&self, engine: &EngineSpec) -> Option<PathBuf> {
        // Where detection found it (known install folders included).
        if let Some(p) = self.engines.installed(&engine.id).filter(|p| p.is_file()) {
            return Some(p);
        }
        // The login PATH next; then the app's own PATH, which can hold
        // installs the login profile resets away (Linux /etc/profile).
        let login = self.login_env().await;
        let sep = if cfg!(windows) { ";" } else { ":" };
        let path = [login.vars.get("PATH").cloned(), std::env::var("PATH").ok()]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(sep);
        let pathext = if cfg!(windows) {
            std::env::var("PATHEXT").ok()
        } else {
            None
        };
        engine
            .commands
            .iter()
            .find_map(|c| env::resolve_on_path(c, &path, pathext.as_deref()))
    }

    fn log_spawn(&self, spec: &SpawnSpec, home: Option<&Path>) {
        let Some(path) = &self.spawn_log else { return };
        let mut argv = vec![spec.program.to_string_lossy().into_owned()];
        argv.extend(spec.args.iter().cloned());
        let record = SpawnRecord {
            argv,
            env_keys: spec.env.keys().cloned().collect(),
            secret_env_keys: spec
                .env
                .iter()
                .filter(|(_, v)| tandem_engines::launch::looks_secret(v))
                .map(|(k, _)| k.clone())
                .collect(),
            provider_home: home.map(|h| h.to_string_lossy().into_owned()),
            cwd: spec.cwd.to_string_lossy().into_owned(),
        };
        if let Ok(line) = serde_json::to_string(&record) {
            use std::io::Write;
            if let Ok(mut f) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
            {
                let _ = writeln!(f, "{line}");
            }
        }
    }

    pub fn spawn_records(&self) -> Vec<SpawnRecord> {
        let Some(path) = &self.spawn_log else {
            return Vec::new();
        };
        std::fs::read_to_string(path)
            .unwrap_or_default()
            .lines()
            .filter_map(|l| serde_json::from_str(l).ok())
            .collect()
    }
}

/// The `wsl_default_distro` setting as a distro name `wsl.exe -d` can take.
fn plain_distro(value: &serde_json::Value) -> Option<String> {
    value
        .as_str()
        .map(str::trim)
        .filter(|d| tandem_core::wslpath::valid_distro(d))
        .map(str::to_string)
}

/// Readiness from the screen alone: it has some text and hasn't changed for
/// [`READY_QUIET_MS`].
#[derive(Debug, Default)]
pub(crate) struct QuietScreen {
    last: Option<(String, i64)>,
}

impl QuietScreen {
    pub(crate) fn settled(&mut self, now: i64, screen: Option<&str>) -> bool {
        let text = screen.map(str::trim_end).unwrap_or("");
        if text.trim().is_empty() {
            self.last = None;
            return false;
        }
        match &self.last {
            Some((seen, since)) if seen == text => now - since >= READY_QUIET_MS,
            _ => {
                self.last = Some((text.to_string(), now));
                false
            }
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::time::Duration;
    use tandem_core::DataDir;
    use tandem_pty::{wait_until, VecSink};
    use tandem_store::repo::tabs;

    #[tokio::test(flavor = "multi_thread")]
    async fn f_cd_03_shell_starts_in_the_workspace_and_echoes() {
        let dir = tempfile::tempdir().expect("tempdir");
        let data = DataDir::at(dir.path().join("data")).expect("data");
        let store = Store::open(&data).expect("store");
        let root = dir.path().join("ws1");
        std::fs::create_dir_all(&root).expect("mkdir");
        settings::set(
            &store,
            SettingKey::DefaultShell,
            serde_json::json!("/bin/sh"),
        )
        .await
        .expect("shell");
        let added = crate::services::workspaces::add(&store, root.to_str().expect("utf8"), Some(3))
            .await
            .expect("ws");
        let tab = tabs::list(&store, &added.workspace.id)
            .await
            .expect("tabs")
            .remove(0);
        let pane = panes::create(&store, &tab.id, "terminal", Some("/does/not/exist".into()))
            .await
            .expect("pane");

        let svc = TerminalService::new(PtyManager::new(), store.clone());
        let sink = Arc::new(VecSink::default());
        let info = svc
            .spawn(&pane.id, 80, 24, Some(4), sink.clone())
            .await
            .expect("spawn");
        assert!(info.pid > 0);
        svc.pty
            .write(pane.id.as_str(), b"echo hi-$((40+2)); pwd\n")
            .expect("write");
        let text = || String::from_utf8_lossy(&sink.data()).into_owned();
        assert!(
            wait_until(Duration::from_secs(5), || text().contains("hi-42")),
            "{}",
            text()
        );
        assert!(
            text().contains("ws1"),
            "starts in the workspace root when cwd is gone"
        );
        let notices: Vec<String> = sink
            .frames
            .lock()
            .expect("frames")
            .iter()
            .filter(|f| f.kind == tandem_pty::FrameKind::Notice)
            .filter_map(|f| serde_json::from_slice::<serde_json::Value>(&f.payload).ok())
            .filter_map(|v| v["message"].as_str().map(str::to_string))
            .collect();
        assert_eq!(notices, vec![FOLDER_NOT_FOUND.to_string()]);
        svc.pty.kill(pane.id.as_str()).expect("kill");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn f_cd_09_3_a_panes_links_start_from_its_shells_latest_folder() {
        if !Path::new("/bin/bash").is_file() {
            return;
        }
        let dir = tempfile::tempdir().expect("tempdir");
        let data = DataDir::at(dir.path().join("data")).expect("data");
        let store = Store::open(&data).expect("store");
        let root = std::fs::canonicalize(dir.path())
            .expect("canonical")
            .join("ws1");
        std::fs::create_dir_all(root.join("sub")).expect("mkdir");
        settings::set(
            &store,
            SettingKey::DefaultShell,
            serde_json::json!("/bin/bash"),
        )
        .await
        .expect("shell");
        let added = crate::services::workspaces::add(&store, root.to_str().expect("utf8"), Some(3))
            .await
            .expect("ws");
        let tab = tabs::list(&store, &added.workspace.id)
            .await
            .expect("tabs")
            .remove(0);
        let pane = panes::create(&store, &tab.id, "terminal", None)
            .await
            .expect("pane");
        let svc = TerminalService::with_integration(
            PtyManager::new(),
            store.clone(),
            Some(data.shell_integration()),
        );
        // Before its shell says where it is: the folder the pane opens in.
        assert_eq!(svc.link_cwd(&pane.id).await, Some(root.clone()));
        svc.spawn(&pane.id, 80, 24, Some(4), Arc::new(VecSink::default()))
            .await
            .expect("spawn");
        let cwd = || svc.pty.state(pane.id.as_str()).and_then(|s| s.cwd);
        let root_text = root.to_string_lossy().into_owned();
        assert!(
            wait_until(Duration::from_secs(10), || cwd().as_deref()
                == Some(root_text.as_str())),
            "the shell never said where it was: {:?}",
            cwd()
        );
        // `cd sub`: the shell's next OSC 7 moves where links start.
        svc.pty.write(pane.id.as_str(), b"cd sub\n").expect("write");
        let sub = root.join("sub").to_string_lossy().into_owned();
        assert!(
            wait_until(Duration::from_secs(10), || cwd().as_deref()
                == Some(sub.as_str())),
            "{:?}",
            cwd()
        );
        assert_eq!(svc.link_cwd(&pane.id).await, Some(root.join("sub")));
        svc.pty.kill(pane.id.as_str()).expect("kill");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn f_cd_03_3_an_agent_hands_back_to_a_shell_and_is_saved() {
        let dir = tempfile::tempdir().expect("tempdir");
        let data = DataDir::at(dir.path().join("data")).expect("data");
        let store = Store::open(&data).expect("store");
        let root = dir.path().join("ws-agent");
        std::fs::create_dir_all(&root).expect("mkdir");
        // The test engine, on a PATH only the login env knows.
        let bin = dir.path().join("bin");
        std::fs::create_dir_all(&bin).expect("bin");
        let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../crates/tandem-engines/tests/fixtures/bin/claude");
        std::fs::copy(&fixture, bin.join("claude")).expect("copy fake engine");
        settings::set(
            &store,
            SettingKey::DefaultShell,
            serde_json::json!("/bin/sh"),
        )
        .await
        .expect("shell");
        let added = crate::services::workspaces::add(&store, root.to_str().expect("utf8"), None)
            .await
            .expect("ws");
        let tab = tabs::list(&store, &added.workspace.id)
            .await
            .expect("tabs")
            .remove(0);
        let pane = panes::create_with(&store, &tab.id, "agent", None, Some("claude".into()))
            .await
            .expect("pane");

        let handbacks: Arc<std::sync::Mutex<Vec<i32>>> = Arc::default();
        let seen = handbacks.clone();
        let pty = PtyManager::new().with_handback_sink(Arc::new(move |_pane, code| {
            if let Ok(mut v) = seen.lock() {
                v.push(code);
            }
        }));
        let svc = TerminalService::new(pty, store.clone());
        let mut vars = std::collections::BTreeMap::new();
        vars.insert(
            "PATH".to_string(),
            format!("{}:/usr/bin:/bin", bin.display()),
        );
        vars.insert("HOME".to_string(), dir.path().display().to_string());
        svc.set_login_env(LoginEnv {
            vars,
            shell: "/bin/sh".into(),
            degraded: false,
        });
        let sink = Arc::new(VecSink::default());
        svc.spawn(&pane.id, 80, 24, None, sink.clone())
            .await
            .expect("spawn");
        let text = || String::from_utf8_lossy(&sink.data()).into_owned();
        assert!(
            wait_until(Duration::from_secs(10), || text()
                .contains("Fake Claude Code")),
            "{}",
            text()
        );
        let state = svc.pty.state(pane.id.as_str()).expect("state");
        assert!(state.bracketed_paste, "the engine's modes reach pty:state");
        svc.pty.write(pane.id.as_str(), b"/exit\n").expect("write");
        assert!(wait_until(Duration::from_secs(10), || !handbacks
            .lock()
            .expect("handbacks")
            .is_empty()));
        assert_eq!(*handbacks.lock().expect("handbacks"), vec![0]);
        // The pane is a live shell again, in the same folder.
        assert!(svc.pty.is_live(pane.id.as_str()));
        svc.pty
            .write(pane.id.as_str(), b"echo back-$((1+1)); pwd\n")
            .expect("write");
        assert!(
            wait_until(Duration::from_secs(10), || text().contains("back-2")
                && text().matches("ws-agent").count() >= 2),
            "{}",
            text()
        );
        // The service saved the session (the app wires this to the sink).
        tandem_store::repo::sessions::save_on_handback(&store, &pane.id, None)
            .await
            .expect("save")
            .expect("started at spawn");
        assert_eq!(
            tandem_store::repo::sessions::saved_list(&store, &added.workspace.id)
                .await
                .expect("list")
                .len(),
            1
        );
        // The token travels in the environment, never in argv.
        let logged = svc.spawn_records();
        assert!(logged.iter().all(|r| r
            .argv
            .iter()
            .all(|a| !a.contains("handback;") || a.contains("$t"))));
        svc.pty.kill(pane.id.as_str()).expect("kill");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn f_cd_03_a_missing_engine_is_reported_not_spawned() {
        let dir = tempfile::tempdir().expect("tempdir");
        let data = DataDir::at(dir.path().join("data")).expect("data");
        let store = Store::open(&data).expect("store");
        let root = dir.path().join("ws-none");
        std::fs::create_dir_all(&root).expect("mkdir");
        let added = crate::services::workspaces::add(&store, root.to_str().expect("utf8"), None)
            .await
            .expect("ws");
        let tab = tabs::list(&store, &added.workspace.id)
            .await
            .expect("tabs")
            .remove(0);
        let pane = panes::create_with(&store, &tab.id, "agent", None, Some("kimi".into()))
            .await
            .expect("pane");
        let svc = TerminalService::new(PtyManager::new(), store.clone());
        let mut vars = std::collections::BTreeMap::new();
        vars.insert(
            "PATH".to_string(),
            dir.path().join("empty").display().to_string(),
        );
        svc.set_login_env(LoginEnv {
            vars,
            shell: "/bin/sh".into(),
            degraded: false,
        });
        // The app's own PATH is searched too; make sure kimi isn't there.
        if std::env::var("PATH")
            .unwrap_or_default()
            .split(':')
            .any(|d| std::path::Path::new(d).join("kimi").exists())
        {
            return;
        }
        let err = svc
            .spawn(&pane.id, 80, 24, None, Arc::new(VecSink::default()))
            .await
            .expect_err("missing");
        assert_eq!(err.code, ErrorCode::EngineMissing);
        assert_eq!(svc.pty.live_count(), 0);
    }

    #[test]
    fn f_rm_02_plain_terminals_use_the_chosen_distro() {
        assert_eq!(
            plain_distro(&serde_json::json!("Ubuntu-22.04")).as_deref(),
            Some("Ubuntu-22.04")
        );
        assert_eq!(plain_distro(&serde_json::json!(null)), None);
        assert_eq!(plain_distro(&serde_json::json!("")), None);
        assert_eq!(plain_distro(&serde_json::json!("--exec")), None);
    }

    #[tokio::test]
    async fn f_rm_02_2_drops_into_a_wsl_pane_are_linux_paths() {
        let dir = tempfile::tempdir().expect("tempdir");
        let data = DataDir::at(dir.path().join("data")).expect("data");
        let store = Store::open(&data).expect("store");
        let svc = TerminalService::new(PtyManager::new(), store);
        let pane = PaneId::from_db("pn_wsl".to_string());
        svc.mark_wsl_pane(pane.as_str());
        assert_eq!(
            svc.quote_paths(
                &pane,
                &[r"C:\x y\z".to_string(), r"\\wsl$\Ubuntu\home\a".to_string()]
            )
            .expect("quoted"),
            "'/mnt/c/x y/z' '/home/a'"
        );
        let err = svc
            .quote_paths(&pane, &[r"\\server\share\x".to_string()])
            .expect_err("no WSL path");
        assert_eq!(err.code, ErrorCode::Invalid);
    }

    #[test]
    fn f_wt_01_3_a_shell_is_ready_once_its_screen_stops_changing() {
        let mut q = QuietScreen::default();
        assert!(!q.settled(0, None));
        assert!(!q.settled(0, Some("   \n  ")));
        assert!(!q.settled(100, Some("user@host %")));
        assert!(!q.settled(1_000, Some("user@host %")));
        // Trailing blanks aren't a change; new text restarts the wait.
        assert!(!q.settled(1_200, Some("user@host % ")));
        assert!(!q.settled(1_400, Some("user@host %\nmotd")));
        assert!(!q.settled(2_800, Some("user@host %\nmotd")));
        assert!(q.settled(2_900, Some("user@host %\nmotd")));
    }
}
