//! `vibezeo run` for agents (F-TL-12 slice 2, ARCHITECTURE §6.3c): an agent
//! pane asks the app, over its signed status channel, to run a command with
//! the folder's secrets. The app checks where it comes from, asks (or finds
//! a repository rule, or this agent's "Allow for this agent"), runs it with
//! the login environment and the resolved `.env`, and replies with the
//! output, every secret value masked, and the exit code. The values never
//! enter the agent's process. The pane's ledger (F-EX-07) keeps the line,
//! the variables' names, who decided, the exit code and how long it took;
//! never the output or a value.

use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use tandem_core::approvals::{
    ApprovalBroker, ApprovalKind, ApprovalOutcome, ApprovalRequest, ApprovalScope,
};
use tandem_core::ids::PaneId;
use tandem_core::keychain::Keychain;
use tandem_core::run_request::{self, RunReply, RunRequest, OUTPUT_CAP};
use tandem_core::run_rules::RunRules;
use tandem_core::secrets as refs;
use tandem_core::shellquote::ShellDialect;
use tandem_core::source::SourceRef;
use tandem_engines::status_server::RunHandler;
use tandem_pty::mask::LiteralMasker;
use tandem_store::repo::{panes, secrets as names, workspaces};
use tandem_store::Store;
use tokio::io::AsyncReadExt;
use zeroize::Zeroizing;

use crate::services::secrets;
use crate::services::terminal::TerminalService;

/// Outside an agent pane (a plain terminal, a chat thread, a routine, the
/// Night Shift): nothing to ask on, so nothing runs.
pub const OUTSIDE: &str = "vibezeo run works in agent panes in Vibezeo";
pub const REMOTE: &str = "vibezeo run doesn't work in SSH or WSL panes yet";
pub const FLATPAK: &str = "vibezeo run isn't available in the Flatpak build yet";
pub const DECLINED: &str = "Not allowed: you declined";
pub const EXPIRED: &str = "Not allowed: nobody answered in 30 minutes";
pub const CANCELLED: &str = "Not allowed: the pane closed before an answer";
/// A second run while the first one's question is open: one at a time, so
/// the question the person reads can't be swapped under their cursor.
pub const ONE_AT_A_TIME: &str =
    "Not run: this pane already has a vibezeo run waiting for an answer. Ask again after it's answered";
pub const FOLDER_MOVED: &str = "Not run: the folder changed after it was checked";
pub const CLIENT_GONE: &str = "Ended: vibezeo run stopped waiting";
/// A refused run's ledger line: its line isn't kept, since its values were
/// never read and so aren't known to the redactor.
pub const NOT_STARTED: &str = "A run that didn't start (its line isn't kept)";

/// A run is stopped after this long.
pub const RUN_LIMIT: Duration = Duration::from_secs(10 * 60);

/// The program `vibezeo run` is, in `$DATA/run/bin`.
pub fn program_name() -> &'static str {
    if cfg!(windows) {
        "vibezeo.exe"
    } else {
        "vibezeo"
    }
}

/// Makes `$DATA/run/bin` hold only `vibezeo`: a link to the hook helper
/// (a copy on Windows, where links need a privilege). The folder is the
/// user's alone (0700), and a `bin` that's a link is refused, since what it
/// points at would be first on every agent's PATH. Returns the folder.
pub fn install(run_dir: &Path, hook: &Path) -> std::io::Result<PathBuf> {
    let bin = run_dir.join("bin");
    match std::fs::symlink_metadata(&bin) {
        Ok(m) if m.file_type().is_symlink() || !m.is_dir() => {
            return Err(std::io::Error::other(
                "run/bin isn't a plain folder; vibezeo won't be put there",
            ));
        }
        Ok(_) => {}
        Err(_) => std::fs::create_dir_all(&bin)?,
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o700))?;
    }
    let name = program_name();
    for entry in std::fs::read_dir(&bin)?.flatten() {
        if entry.file_name() != name {
            let path = entry.path();
            let _ = if path.is_dir() && !path.is_symlink() {
                std::fs::remove_dir_all(&path)
            } else {
                std::fs::remove_file(&path)
            };
        }
    }
    let target = bin.join(name);
    #[cfg(unix)]
    {
        if std::fs::read_link(&target).ok().as_deref() != Some(hook) {
            let _ = std::fs::remove_file(&target);
            std::os::unix::fs::symlink(hook, &target)?;
        }
    }
    #[cfg(not(unix))]
    {
        // A running vibezeo.exe can't be overwritten but can be renamed
        // over; if even that fails, the one already there stays.
        let fresh = bin.join(format!("{name}.new"));
        std::fs::copy(hook, &fresh)?;
        if let Err(e) = std::fs::rename(&fresh, &target) {
            let _ = std::fs::remove_file(&fresh);
            if !target.is_file() {
                return Err(e);
            }
            tracing::warn!(
                code = "secrets.vibezeo_in_use",
                "an older vibezeo stays until it exits"
            );
        }
    }
    Ok(bin)
}

pub struct AgentRuns {
    store: Store,
    keychain: Arc<dyn Keychain>,
    broker: Arc<ApprovalBroker>,
    terminals: Arc<TerminalService>,
    /// Where `run-rules.json` lives.
    data: PathBuf,
    /// Panes with a run's question open (one at a time).
    asking: std::sync::Mutex<std::collections::HashSet<String>>,
}

/// A pane's open question; dropping it lets the pane ask again.
struct Asking<'a>(&'a AgentRuns, String);

impl Drop for Asking<'_> {
    fn drop(&mut self) {
        if let Ok(mut panes) = self.0.asking.lock() {
            panes.remove(&self.1);
        }
    }
}

/// The handler the status server calls.
pub struct Handler(pub Arc<AgentRuns>);

impl RunHandler for Handler {
    fn run(&self, req: RunRequest) -> Pin<Box<dyn Future<Output = RunReply> + Send>> {
        let me = self.0.clone();
        Box::pin(async move { me.handle(req).await })
    }
}

/// What a run will use, once it's checked.
struct Checked {
    pane: PaneId,
    cwd: PathBuf,
    command: String,
    /// The `.env`'s references, the only variables the run gets from it.
    vars: Vec<refs::EnvVar>,
    /// [`refs::fingerprint`] of `vars`.
    fingerprint: String,
    repo: String,
    /// The pane's folder, canonical: `cwd` is inside it.
    root: PathBuf,
}

impl Checked {
    /// "API_KEY (stripe-test), DB_URL (db)": every variable the run gets
    /// from `.env`, by name, and its secret.
    fn names(&self) -> String {
        self.vars
            .iter()
            .map(|v| match refs::reference(&v.value) {
                Some(secret) => format!("{} ({secret})", v.name),
                None => v.name.clone(),
            })
            .collect::<Vec<_>>()
            .join(", ")
    }

    /// What "Allow for this agent" allows: this line, in this folder.
    fn allow_key(&self) -> String {
        // `check` refuses a folder that isn't UTF-8, so this is exact.
        format!(
            "{}\n{}",
            self.cwd.to_str().unwrap_or_default(),
            self.command
        )
    }

    /// The folder is still the one checked: not replaced by a link (or
    /// anything else) that leads outside the pane's folder.
    fn folder_holds(&self) -> bool {
        std::fs::canonicalize(&self.cwd)
            .is_ok_and(|now| now == self.cwd && now.starts_with(&self.root))
    }
}

/// The environment a run gets: the login environment without anything
/// `.env` sets, and `PATH` without empty or relative entries (they'd find
/// a program in the folder the agent picked). On Windows, cmd.exe is also
/// told not to look in the current folder first for a bare command.
pub fn run_env(
    login: &std::collections::BTreeMap<String, String>,
    from_env: &[&str],
) -> Vec<(String, String)> {
    let mut env: Vec<(String, String)> = login
        .iter()
        .filter(|(k, _)| !from_env.contains(&k.as_str()))
        .filter(|(k, _)| !k.eq_ignore_ascii_case(NO_CWD_LOOKUP))
        .map(|(k, v)| {
            if k.eq_ignore_ascii_case("PATH") {
                (k.clone(), absolute_path_entries(v))
            } else {
                (k.clone(), v.clone())
            }
        })
        .collect();
    if cfg!(windows) {
        env.push((NO_CWD_LOOKUP.into(), "1".into()));
    }
    env
}

/// Tells cmd.exe (and `CreateProcess` searches) to skip the current folder.
const NO_CWD_LOOKUP: &str = "NoDefaultCurrentDirectoryInExePath";

/// `path`'s absolute entries; when none are left, a system default (an
/// empty `PATH` would make `sh` search the current folder).
fn absolute_path_entries(path: &str) -> String {
    let sep = if cfg!(windows) { ';' } else { ':' };
    let kept = path
        .split(sep)
        .filter(|p| !p.is_empty() && Path::new(p).is_absolute())
        .collect::<Vec<_>>()
        .join(&sep.to_string());
    if !kept.is_empty() {
        return kept;
    }
    if cfg!(windows) {
        let root = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
        format!(r"{root}\System32;{root}")
    } else {
        "/usr/local/bin:/usr/bin:/bin".into()
    }
}

/// The shell a run's line goes to.
fn dialect() -> ShellDialect {
    if cfg!(windows) {
        ShellDialect::Cmd
    } else {
        ShellDialect::Posix
    }
}

impl AgentRuns {
    pub fn new(
        store: Store,
        keychain: Arc<dyn Keychain>,
        broker: Arc<ApprovalBroker>,
        terminals: Arc<TerminalService>,
        data: PathBuf,
    ) -> Arc<Self> {
        Arc::new(Self {
            store,
            keychain,
            broker,
            terminals,
            data,
            asking: Default::default(),
        })
    }

    /// Serves a request in its own task, so the client going away (killed,
    /// or its tool timing out) can't drop the run halfway: the question is
    /// cancelled or the run's group is killed, and the ledger row is written.
    pub async fn handle(self: &Arc<Self>, req: RunRequest) -> RunReply {
        let (alive, waiting) = tokio::sync::watch::channel(());
        let me = self.clone();
        let task = tokio::spawn(async move { me.serve(req, waiting).await });
        let reply = task
            .await
            .unwrap_or_else(|_| RunReply::refused("The run stopped unexpectedly"));
        drop(alive);
        reply
    }

    async fn serve(&self, req: RunRequest, waiting: tokio::sync::watch::Receiver<()>) -> RunReply {
        let checked = match self.check(&req).await {
            Ok(c) => c,
            Err(why) => return RunReply::refused(why),
        };
        let decided = match self.decide(&checked, client_gone(waiting.clone())).await {
            Ok(d) => d,
            Err((d, why)) => {
                self.ledger(&checked, d, false, None, None).await;
                return RunReply::refused(why);
            }
        };
        let resolved = match secrets::resolve(self.keychain.as_ref(), &checked.vars) {
            Ok(r) => r,
            Err(e) => return RunReply::refused(e.message),
        };
        if !checked.folder_holds() {
            self.ledger(&checked, "folder_moved", false, None, None)
                .await;
            return RunReply::refused(FOLDER_MOVED);
        }
        let login = self.terminals.login_env().await;
        let from_env: Vec<&str> = resolved.env.keys().map(String::as_str).collect();
        let env = run_env(&login.vars, &from_env);
        let secret_env: Vec<(String, Zeroizing<String>)> = resolved
            .env
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        tracing::info!(
            code = "secrets.agent_run",
            "an agent's run with secrets started"
        );
        let started = std::time::Instant::now();
        let reply = run(
            &checked.command,
            &checked.cwd,
            &env,
            &secret_env,
            &resolved.secrets,
            RUN_LIMIT,
            &|pid| self.terminals.resources.adopt(checked.pane.as_str(), pid),
            client_gone(waiting),
        )
        .await;
        let took = i64::try_from(started.elapsed().as_millis()).unwrap_or(i64::MAX);
        self.ledger(&checked, decided, true, Some(&reply), Some(took))
            .await;
        reply
    }

    /// The run in the pane's ledger (F-EX-07): its line (once it `ran`,
    /// its values known to the redactor), the secrets' names, who decided,
    /// its exit code, how long it took and how it ended. Never its output.
    async fn ledger(
        &self,
        c: &Checked,
        decided: &'static str,
        ran: bool,
        reply: Option<&RunReply>,
        duration_ms: Option<i64>,
    ) {
        let ended = reply.and_then(|r| match r.error.as_deref() {
            Some(CLIENT_GONE) => Some("client_gone"),
            Some(e) if e.starts_with("Stopped after") => Some("time_limit"),
            _ => None,
        });
        self.terminals
            .activity
            .run(
                c.pane.as_str(),
                crate::services::activity::RunRecord {
                    command: if ran {
                        c.command.clone()
                    } else {
                        NOT_STARTED.into()
                    },
                    // Every variable the run got, by name, and its secret.
                    secrets: c
                        .vars
                        .iter()
                        .map(|v| match refs::reference(&v.value) {
                            Some(secret) => format!("{} ({secret})", v.name),
                            None => v.name.clone(),
                        })
                        .collect(),
                    decided,
                    exit_code: reply.and_then(|r| r.exit_code),
                    duration_ms,
                    ended,
                },
            )
            .await;
    }

    /// Where it comes from, what it runs and what it uses.
    async fn check(&self, req: &RunRequest) -> Result<Checked, String> {
        if tandem_core::sandbox::in_flatpak() {
            return Err(FLATPAK.into());
        }
        let pane = PaneId::from_db(req.pane_id.clone());
        let record = panes::get_open(&self.store, &pane)
            .await
            .map_err(|_| OUTSIDE.to_string())?;
        if record.kind != "agent" {
            return Err(OUTSIDE.into());
        }
        let ws = workspaces::get(&self.store, &record.workspace_id)
            .await
            .map_err(|_| OUTSIDE.to_string())?;
        if ws.kind != "local" {
            return Err(REMOTE.into());
        }
        // The folder the pane is in: its worktree, for a worktree's pane.
        let root = std::fs::canonicalize(&ws.root)
            .map_err(|_| "This pane's folder is missing".to_string())?;
        // A folder name that isn't UTF-8 is refused: the question shows it,
        // and "Allow for this agent" keys on it, exactly.
        let cwd = std::fs::canonicalize(&req.cwd)
            .ok()
            .filter(|c| c.starts_with(&root) && c.to_str().is_some())
            .ok_or_else(|| format!("vibezeo run works inside this pane's folder, {}", ws.root))?;
        let command = run_request::command_line(&req.argv, dialect()).map_err(str::to_string)?;
        let parsed = secrets::read_env(&root)
            .map_err(|e| e.message)?
            .map(|(_, v)| v)
            .unwrap_or_default();
        // Only references reach an agent's run: a plain value (or a loader
        // variable) in `.env` would change what runs without anyone seeing.
        let vars = refs::for_agent(&parsed).map_err(|e| e.to_string())?;
        for p in refs::plan(&vars) {
            if let Some(name) = p.secret {
                if !names::exists(&self.store, &name)
                    .await
                    .map_err(|e| e.to_string())?
                {
                    return Err(format!("No secret named {name}"));
                }
            }
        }
        // Rules belong to the repository: a worktree uses its main checkout's.
        let repo = match &ws.parent_id {
            Some(parent) => workspaces::get(&self.store, parent)
                .await
                .map(|p| p.root)
                .unwrap_or_else(|_| ws.root.clone()),
            None => ws.root.clone(),
        };
        Ok(Checked {
            pane,
            cwd,
            command,
            fingerprint: refs::fingerprint(&vars),
            vars,
            repo,
            root,
        })
    }

    /// A repository rule, or this agent's earlier "Allow for this agent",
    /// each only while `.env` gives the run what it did then; else the
    /// person's answer. Who decided (as the ledger names it), or that and
    /// why not.
    async fn decide(
        &self,
        c: &Checked,
        gone: impl Future<Output = ()>,
    ) -> Result<&'static str, (&'static str, String)> {
        let rules = RunRules::load(&self.data);
        if rules.allows(&c.repo, &c.command, dialect(), &c.fingerprint) {
            return Ok("rule");
        }
        if self
            .terminals
            .agent_run_allowed(c.pane.as_str(), &c.allow_key(), &c.fingerprint)
        {
            return Ok("allowed_before");
        }
        let _asking = {
            let mut panes = self
                .asking
                .lock()
                .map_err(|_| ("busy", ONE_AT_A_TIME.to_string()))?;
            if !panes.insert(c.pane.as_str().to_string()) {
                return Err(("busy", ONE_AT_A_TIME.into()));
            }
            Asking(self, c.pane.as_str().to_string())
        };
        let with = if c.vars.is_empty() {
            "with no secrets".to_string()
        } else {
            format!("with {}", c.names())
        };
        let (view, ticket) = self.broker.request(ApprovalRequest {
            source: SourceRef::Pane {
                pane_id: c.pane.as_str().to_string(),
            },
            kind: ApprovalKind::Command,
            summary: format!("run `{}` {with}", c.command),
            detail: serde_json::json!({
                "vibezeoRun": {
                    "command": c.command,
                    "cwd": c.cwd.to_string_lossy(),
                    "vars": c.vars.iter().map(|v| serde_json::json!({
                        "name": v.name,
                        "secret": refs::reference(&v.value),
                    })).collect::<Vec<_>>(),
                    // A rule covers it, but `.env` changed since it was allowed.
                    "envChanged": rules.matches(&c.repo, &c.command, dialect()),
                }
            })
            .to_string(),
            options: vec![ApprovalScope::Once, ApprovalScope::Conversation],
        });
        // The client going away takes its question with it.
        let outcome = tokio::select! {
            outcome = ticket.outcome() => outcome,
            _ = gone => {
                self.broker.cancel(&view.id);
                self.terminals
                    .activity
                    .approval(c.pane.as_str(), "client_gone")
                    .await;
                return Err(("client_gone", CLIENT_GONE.into()));
            }
        };
        let (decided, answer) = match &outcome {
            ApprovalOutcome::Allowed { scope, via } => {
                let who = if *scope == ApprovalScope::Conversation {
                    "agent"
                } else {
                    "once"
                };
                (who, format!("allowed:{}", via_name(*via)))
            }
            ApprovalOutcome::Denied { via, .. } => {
                ("declined", format!("denied:{}", via_name(*via)))
            }
            ApprovalOutcome::Expired => ("expired", "expired".into()),
            ApprovalOutcome::Cancelled => ("cancelled", "cancelled".into()),
        };
        self.terminals
            .activity
            .approval(c.pane.as_str(), &answer)
            .await;
        match outcome {
            ApprovalOutcome::Allowed { scope, .. } => {
                if scope == ApprovalScope::Conversation {
                    self.terminals
                        .allow_agent_run(c.pane.as_str(), &c.allow_key(), &c.fingerprint);
                }
                // A rule that asked because `.env` changed holds again for it.
                if rules.matches(&c.repo, &c.command, dialect()) {
                    let mut rules = RunRules::load(&self.data);
                    rules.approve_env(&c.repo, &c.fingerprint);
                    if let Err(e) = rules.save(&self.data) {
                        tracing::warn!(code = "secrets.rules_save_failed", error = %e, "the rule will ask again");
                    }
                }
                Ok(decided)
            }
            ApprovalOutcome::Denied { .. } => Err((decided, DECLINED.into())),
            ApprovalOutcome::Expired => Err((decided, EXPIRED.into())),
            ApprovalOutcome::Cancelled => Err((decided, CANCELLED.into())),
        }
    }
}

/// `ApprovalVia` as stored (`inline`, `dashboard`, …).
fn via_name(via: tandem_core::approvals::ApprovalVia) -> String {
    serde_json::to_value(via)
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_default()
}

/// The last [`OUTPUT_CAP`] bytes of what a run prints.
#[derive(Default)]
struct Tail {
    bytes: Vec<u8>,
    cut: bool,
}

impl Tail {
    fn push(&mut self, chunk: &[u8]) {
        self.bytes.extend_from_slice(chunk);
        if self.bytes.len() > OUTPUT_CAP {
            let drop = self.bytes.len() - OUTPUT_CAP;
            self.bytes.drain(..drop);
            self.cut = true;
        }
    }
}

/// How long output still comes in after the command exits (and its group
/// was killed), before the reply goes back.
const DRAIN: Duration = Duration::from_millis(500);

/// Runs `command` under `/bin/sh -c` (`%SystemRoot%\\System32\\cmd.exe /D
/// /V:OFF /S /C` on Windows, the line passed as it is) in `cwd` with exactly `env` and
/// `secret_env`, stdout and stderr together through a masker for `values`,
/// for up to `limit`. When it exits, its process group is killed, so nothing
/// it left running in the background keeps the values.
#[allow(clippy::too_many_arguments)]
pub async fn run(
    command: &str,
    cwd: &Path,
    env: &[(String, String)],
    secret_env: &[(String, Zeroizing<String>)],
    values: &[Zeroizing<String>],
    limit: Duration,
    adopt: &(dyn Fn(u32) + Send + Sync),
    gone: impl Future<Output = ()>,
) -> RunReply {
    tokio::pin!(gone);
    #[cfg(windows)]
    let mut cmd = {
        let root = std::env::var_os("SystemRoot")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
        let mut c = tokio::process::Command::new(root.join("System32").join("cmd.exe"));
        // /D: no AutoRun. /V:OFF: no delayed `!VAR!` expansion, whatever
        // the registry says. /S with the outer quotes: cmd runs the line as
        // it is, the one the person approved.
        c.raw_arg(format!("/D /V:OFF /S /C \"{command}\""));
        c
    };
    #[cfg(not(windows))]
    let mut cmd = {
        let mut c = tokio::process::Command::new("/bin/sh");
        c.arg("-c").arg(command);
        c
    };
    cmd.current_dir(cwd)
        .env_clear()
        .envs(env.iter().map(|(k, v)| (k, v)))
        .envs(secret_env.iter().map(|(k, v)| (k, v.as_str())))
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    #[cfg(unix)]
    cmd.process_group(0);
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => return RunReply::refused(format!("Couldn't start the command: {e}")),
    };
    let pid = child.id();
    if let Some(pid) = pid {
        adopt(pid);
    }
    let (tx, mut rx) = tokio::sync::mpsc::channel::<Vec<u8>>(64);
    for pipe in [
        child
            .stdout
            .take()
            .map(|p| Box::new(p) as Box<dyn tokio::io::AsyncRead + Send + Unpin>),
        child
            .stderr
            .take()
            .map(|p| Box::new(p) as Box<dyn tokio::io::AsyncRead + Send + Unpin>),
    ]
    .into_iter()
    .flatten()
    {
        let tx = tx.clone();
        tokio::spawn(async move {
            let mut pipe = pipe;
            let mut buf = vec![0u8; 8192];
            while let Ok(n) = pipe.read(&mut buf).await {
                if n == 0 || tx.send(buf[..n].to_vec()).await.is_err() {
                    break;
                }
            }
        });
    }
    drop(tx);
    let mut masker = LiteralMasker::new(values.iter().map(|v| v.as_bytes()));
    let mut tail = Tail::default();
    let mut take = |chunk: &[u8], tail: &mut Tail| match masker.as_mut() {
        Some(m) => tail.push(&m.push(chunk)),
        None => tail.push(chunk),
    };
    let deadline = tokio::time::Instant::now() + limit;
    // Output and the exit, whichever comes; the command's own exit ends it.
    let mut pipes_open = true;
    let ended = loop {
        tokio::select! {
            chunk = rx.recv(), if pipes_open => match chunk {
                Some(chunk) => take(&chunk, &mut tail),
                None => pipes_open = false,
            },
            status = child.wait() => break Ended::Exited(status),
            _ = tokio::time::sleep_until(deadline) => break Ended::TimeLimit,
            _ = &mut gone => break Ended::ClientGone,
        }
    };
    let (status, error) = match ended {
        Ended::Exited(Ok(status)) => (Some(status), None),
        Ended::Exited(Err(e)) => (None, Some(format!("The command failed: {e}"))),
        Ended::TimeLimit => (
            None,
            Some(format!("Stopped after {} minutes", limit.as_secs() / 60)),
        ),
        Ended::ClientGone => (None, Some(CLIENT_GONE.to_string())),
    };
    // Whatever it started and left behind goes with it.
    kill_group(pid);
    let _ = child.kill().await;
    let drained = tokio::time::Instant::now() + DRAIN;
    while let Ok(Some(chunk)) = tokio::time::timeout_at(drained, rx.recv()).await {
        take(&chunk, &mut tail);
    }
    if let Some(m) = masker.as_mut() {
        tail.push(&m.flush());
    }
    RunReply {
        exit_code: status.and_then(|s| s.code()),
        output: String::from_utf8_lossy(&tail.bytes).into_owned(),
        cut: tail.cut,
        error,
    }
}

enum Ended {
    Exited(std::io::Result<std::process::ExitStatus>),
    TimeLimit,
    ClientGone,
}

/// Resolves when the request's client is gone (its sender dropped).
async fn client_gone(mut waiting: tokio::sync::watch::Receiver<()>) {
    while waiting.changed().await.is_ok() {}
}

fn kill_group(pid: Option<u32>) {
    if let Some(pid) = pid {
        crate::services::bestof::kill_group(pid);
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use tandem_core::approvals::{ApprovalDecision, ApprovalVia};
    use tandem_core::keychain::MemoryKeychain;
    use tandem_store::repo::tabs;

    const VALUE: &str = "sk_test_4242xyz";

    struct Fx {
        _dir: tempfile::TempDir,
        root: PathBuf,
        store: Store,
        broker: Arc<ApprovalBroker>,
        terminals: Arc<TerminalService>,
        runs: Arc<AgentRuns>,
        agent: PaneId,
        plain: PaneId,
    }

    async fn fixture(env: &str) -> Fx {
        let dir = tempfile::tempdir().expect("dir");
        let data = tandem_core::DataDir::at(dir.path().join("data")).expect("data");
        let store = Store::open(&data).expect("store");
        let root = dir.path().join("shop");
        std::fs::create_dir_all(root.join("sub")).expect("mkdir");
        std::fs::write(root.join(".env"), env).expect(".env");
        let ws = crate::services::workspaces::add(&store, root.to_str().expect("utf8"), Some(5))
            .await
            .expect("ws")
            .workspace;
        let tab = tabs::list(&store, &ws.id).await.expect("tabs").remove(0);
        let agent = panes::create_with(&store, &tab.id, "agent", None, Some("claude".into()))
            .await
            .expect("agent")
            .id;
        let plain = panes::create(&store, &tab.id, "terminal", None)
            .await
            .expect("plain")
            .id;
        let kc: Arc<dyn Keychain> = Arc::new(MemoryKeychain::new());
        secrets::set(&store, kc.as_ref(), "stripe-test", VALUE, 1)
            .await
            .expect("secret");
        let broker = ApprovalBroker::new(Arc::new(tandem_core::clock::SystemClock));
        let terminals = TerminalService::new(tandem_pty::PtyManager::new(), store.clone());
        let runs = AgentRuns::new(
            store.clone(),
            kc,
            broker.clone(),
            terminals.clone(),
            data.root().to_path_buf(),
        );
        Fx {
            root: std::fs::canonicalize(&root).expect("canon"),
            _dir: dir,
            store,
            broker,
            terminals,
            runs,
            agent,
            plain,
        }
    }

    fn req(pane: &PaneId, cwd: &Path, argv: &[&str]) -> RunRequest {
        RunRequest {
            pane_id: pane.as_str().to_string(),
            cwd: cwd.to_string_lossy().into_owned(),
            argv: argv.iter().map(|a| a.to_string()).collect(),
            ts: 0,
            nonce: "0123456789abcdef".into(),
        }
    }

    /// Starts a run and waits for its approval to appear.
    async fn asked(
        fx: &Fx,
        r: RunRequest,
    ) -> (
        tandem_core::approvals::ApprovalView,
        tokio::task::JoinHandle<RunReply>,
    ) {
        let runs = fx.runs.clone();
        let task = tokio::spawn(async move { runs.handle(r).await });
        for _ in 0..200 {
            if let Some(v) = fx.broker.pending().into_iter().next() {
                return (v, task);
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("no approval: {:?}", task.await.ok());
    }

    fn allow(fx: &Fx, v: &tandem_core::approvals::ApprovalView, scope: ApprovalScope) {
        fx.broker
            .resolve(
                &v.id,
                ApprovalDecision::Allow { scope },
                ApprovalVia::Inline,
            )
            .expect("resolve");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn f_tl_12_7_allow_once_runs_it_with_the_value_masked_and_asks_by_name() {
        let fx = fixture("API_KEY=vibezeo-secret://stripe-test\nPORT=3000\n").await;
        let (view, task) = asked(
            &fx,
            req(&fx.agent, &fx.root.join("sub"), &["printenv", "API_KEY"]),
        )
        .await;
        assert_eq!(
            view.summary,
            "run `printenv API_KEY` with API_KEY (stripe-test)"
        );
        let detail: serde_json::Value = serde_json::from_str(&view.detail).expect("json");
        assert_eq!(detail["vibezeoRun"]["command"], "printenv API_KEY");
        assert_eq!(
            detail["vibezeoRun"]["vars"],
            serde_json::json!([{ "name": "API_KEY", "secret": "stripe-test" }])
        );
        assert_eq!(
            view.source,
            SourceRef::Pane {
                pane_id: fx.agent.as_str().to_string()
            }
        );
        assert_eq!(
            view.options,
            vec![ApprovalScope::Once, ApprovalScope::Conversation]
        );
        assert!(!view.detail.contains(VALUE) && !view.summary.contains(VALUE));
        allow(&fx, &view, ApprovalScope::Once);
        let reply = task.await.expect("task");
        assert_eq!(reply.error, None);
        assert_eq!(reply.exit_code, Some(0));
        assert_eq!(reply.output, "[REDACTED]\n");
        // Once is once: the next command asks again. A plain value in
        // `.env` doesn't reach an agent's run.
        let (again, task) = asked(&fx, req(&fx.agent, &fx.root, &["printenv", "PORT"])).await;
        allow(&fx, &again, ApprovalScope::Once);
        let reply = task.await.expect("task");
        assert_eq!((reply.exit_code, reply.output.as_str()), (Some(1), ""));
        // The pane's approvals aren't stored: what the app's approval
        // service does with each request writes no row for them.
        tandem_store::repo::approvals::insert_pending(&fx.store, &view)
            .await
            .expect("insert");
        let stored: i64 = fx
            .store
            .read(|c| Ok(c.query_row("SELECT COUNT(*) FROM approvals", [], |r| r.get(0))?))
            .await
            .expect("count");
        assert_eq!(stored, 0);
        // F-EX-07: the ledger has each answer and each run, never the value.
        let events = fx
            .terminals
            .activity
            .list("pane", fx.agent.as_str(), None, 0)
            .await
            .expect("ledger")
            .events;
        let kinds: Vec<(&str, Option<&str>)> = events
            .iter()
            .map(|e| (e.kind.as_str(), e.decided.as_deref()))
            .collect();
        assert_eq!(
            kinds,
            vec![
                ("approval", Some("allowed:inline")),
                ("run", Some("once")),
                ("approval", Some("allowed:inline")),
                ("run", Some("once")),
            ]
        );
        assert_eq!(events[1].summary, "printenv API_KEY");
        assert_eq!(events[3].exit_code, Some(1));
        assert_eq!(events[1].secrets, vec!["API_KEY (stripe-test)"]);
        assert_eq!(events[1].exit_code, Some(0));
        assert!(events[1].duration_ms.is_some());
        let all = serde_json::to_string(&events).expect("json");
        assert!(!all.contains(VALUE) && !all.contains("[REDACTED]"), "{all}");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn f_tl_12_8_the_exit_code_comes_back_and_long_output_keeps_its_end() {
        let fx = fixture("API_KEY=vibezeo-secret://stripe-test\n").await;
        let command = "head -c 70000 /dev/zero | tr '\\0' a; printf 'END %s' \"$API_KEY\"; exit 3";
        let (view, task) = asked(&fx, req(&fx.agent, &fx.root, &[command])).await;
        allow(&fx, &view, ApprovalScope::Once);
        let reply = task.await.expect("task");
        assert_eq!(reply.exit_code, Some(3));
        assert!(reply.cut);
        assert_eq!(reply.output.len(), OUTPUT_CAP);
        assert!(
            reply.output.ends_with("aEND [REDACTED]"),
            "{}",
            &reply.output[OUTPUT_CAP - 40..]
        );
        assert!(!reply.output.contains(VALUE));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn f_tl_12_9_deny_runs_nothing_and_allow_for_this_agent_covers_that_command_only() {
        let fx = fixture("API_KEY=vibezeo-secret://stripe-test\n").await;
        let (view, task) = asked(&fx, req(&fx.agent, &fx.root, &["touch", "ran"])).await;
        fx.broker
            .resolve(
                &view.id,
                ApprovalDecision::Deny { reason: None },
                ApprovalVia::Inline,
            )
            .expect("deny");
        let reply = task.await.expect("task");
        assert_eq!(reply.error.as_deref(), Some(DECLINED));
        assert_eq!(reply.exit_code, None);
        assert!(!fx.root.join("ran").exists());

        let (view, task) = asked(&fx, req(&fx.agent, &fx.root, &["echo", "one"])).await;
        allow(&fx, &view, ApprovalScope::Conversation);
        assert_eq!(task.await.expect("task").output, "one\n");
        // The same command runs without asking...
        let reply = fx
            .runs
            .handle(req(&fx.agent, &fx.root, &["echo", "one"]))
            .await;
        assert_eq!(reply.output, "one\n");
        // ...a different one asks...
        let (other, task) = asked(&fx, req(&fx.agent, &fx.root, &["echo", "one", "two"])).await;
        assert!(other.summary.contains("echo one two"));
        fx.broker.cancel_source(&other.source);
        assert_eq!(task.await.expect("task").error.as_deref(), Some(CANCELLED));
        // ...and closing the pane forgets it.
        fx.terminals.drop_run(fx.agent.as_str());
        let (_, task) = asked(&fx, req(&fx.agent, &fx.root, &["echo", "one"])).await;
        fx.broker.cancel_source(&SourceRef::Pane {
            pane_id: fx.agent.as_str().to_string(),
        });
        let _ = task.await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn f_tl_12_9_a_repository_rule_runs_its_command_with_arguments_without_asking() {
        let fx = fixture("API_KEY=vibezeo-secret://stripe-test\n").await;
        // The rule is keyed by the workspace's root as it was added.
        let ws_root = workspaces::get(
            &fx.store,
            &panes::get(&fx.store, &fx.agent)
                .await
                .expect("pane")
                .workspace_id,
        )
        .await
        .expect("ws")
        .root;
        let mut rules = RunRules::default();
        rules.set(&ws_root, &["echo deploy".into()]).expect("rule");
        rules.save(&fx.runs.data).expect("save");
        // The first run under a rule asks: nobody has seen this `.env` yet.
        let (view, task) = asked(&fx, req(&fx.agent, &fx.root, &["echo", "deploy"])).await;
        let detail: serde_json::Value = serde_json::from_str(&view.detail).expect("json");
        assert_eq!(detail["vibezeoRun"]["envChanged"], true);
        allow(&fx, &view, ApprovalScope::Once);
        assert_eq!(task.await.expect("task").output, "deploy\n");
        // Then it runs without asking, with any arguments...
        let reply = fx
            .runs
            .handle(req(&fx.agent, &fx.root, &["echo", "deploy", "--prod"]))
            .await;
        assert_eq!(reply.output, "deploy --prod\n");
        // ...until `.env` gives it something else: then it asks again.
        std::fs::write(
            fx.root.join(".env"),
            "API_KEY=vibezeo-secret://stripe-test\nMORE=vibezeo-secret://stripe-test\n",
        )
        .expect(".env");
        let (view, task) = asked(&fx, req(&fx.agent, &fx.root, &["echo", "deploy"])).await;
        assert!(
            view.summary
                .contains("API_KEY (stripe-test), MORE (stripe-test)"),
            "{}",
            view.summary
        );
        fx.broker.cancel_source(&view.source);
        let _ = task.await;
        let (view, task) = asked(&fx, req(&fx.agent, &fx.root, &["echo deploy; touch x"])).await;
        assert!(view.summary.contains("echo deploy; touch x"));
        fx.broker.cancel_source(&view.source);
        let _ = task.await;
        assert!(!fx.root.join("x").exists());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn f_tl_12_8_what_a_run_leaves_in_the_background_is_ended_with_it() {
        let started = std::time::Instant::now();
        let reply = run(
            "sleep 30 & echo $! > bg.pid; echo done",
            &std::env::temp_dir(),
            &[("PATH".into(), "/usr/bin:/bin".into())],
            &[],
            &[],
            Duration::from_secs(20),
            &|_| {},
            std::future::pending(),
        )
        .await;
        // The background sleep holds the pipe open, yet the reply comes back
        // once the command itself exits.
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "{:?}",
            started.elapsed()
        );
        assert_eq!(
            (reply.exit_code, reply.output.as_str()),
            (Some(0), "done\n")
        );
        let pid = std::fs::read_to_string(std::env::temp_dir().join("bg.pid"))
            .expect("pid")
            .trim()
            .to_string();
        let _ = std::fs::remove_file(std::env::temp_dir().join("bg.pid"));
        assert!(ends(&pid), "the background child outlived the run");
    }

    /// The process ends within 2 s: gone, or a zombie waiting for its new
    /// parent to reap it.
    fn ends(pid: &str) -> bool {
        (0..100).any(|_| {
            std::thread::sleep(Duration::from_millis(20));
            match std::fs::read_to_string(format!("/proc/{pid}/stat")) {
                Err(_) => true,
                Ok(stat) => stat
                    .rsplit(')')
                    .next()
                    .and_then(|rest| rest.split_whitespace().next())
                    .is_some_and(|state| state == "Z" || state == "X"),
            }
        })
    }

    async fn ledger_of(fx: &Fx) -> Vec<crate::services::activity::ActivityEvent> {
        fx.terminals
            .activity
            .list("pane", fx.agent.as_str(), None, 0)
            .await
            .expect("ledger")
            .events
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn f_tl_12_8_a_client_gone_while_asking_cancels_its_question_and_frees_the_pane() {
        let fx = fixture("API_KEY=vibezeo-secret://stripe-test\n").await;
        let (view, task) = asked(&fx, req(&fx.agent, &fx.root, &["touch", "ran"])).await;
        // The agent killed `vibezeo` (or its tool timed out).
        task.abort();
        let _ = task.await;
        for _ in 0..200 {
            if fx.broker.pending().is_empty() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert!(fx.broker.pending().is_empty(), "the question was cancelled");
        assert!(fx
            .broker
            .resolve(
                &view.id,
                ApprovalDecision::Allow {
                    scope: ApprovalScope::Once
                },
                ApprovalVia::Inline,
            )
            .is_err());
        assert!(!fx.root.join("ran").exists());
        // The rows follow the cancel.
        let mut events = Vec::new();
        for _ in 0..300 {
            events = ledger_of(&fx).await;
            if events.iter().any(|e| e.kind == "run") {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert!(events
            .iter()
            .any(|e| e.kind == "approval" && e.decided.as_deref() == Some("client_gone")));
        let run = events.iter().find(|e| e.kind == "run").expect("run row");
        assert_eq!(run.decided.as_deref(), Some("client_gone"));
        assert_eq!(run.summary, NOT_STARTED);
        // The pane can ask again.
        let (_, task) = asked(&fx, req(&fx.agent, &fx.root, &["echo", "again"])).await;
        fx.broker.cancel_source(&view.source);
        assert_eq!(task.await.expect("task").error.as_deref(), Some(CANCELLED));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn f_tl_12_8_a_client_gone_mid_run_ends_its_group_and_leaves_a_run_row() {
        let fx = fixture("API_KEY=vibezeo-secret://stripe-test\n").await;
        let pid_file = fx.root.join("bg.pid");
        let line = format!("sleep 30 & echo $! > {}; sleep 30", pid_file.display());
        let (view, task) = asked(&fx, req(&fx.agent, &fx.root, &[&line])).await;
        allow(&fx, &view, ApprovalScope::Once);
        let mut pid = String::new();
        for _ in 0..300 {
            pid = std::fs::read_to_string(&pid_file)
                .unwrap_or_default()
                .trim()
                .to_string();
            if !pid.is_empty() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert!(!pid.is_empty(), "the run started");
        task.abort();
        let _ = task.await;
        assert!(ends(&pid), "the run's group outlived its client");
        let mut run = None;
        for _ in 0..300 {
            run = ledger_of(&fx).await.into_iter().find(|e| e.kind == "run");
            if run.is_some() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        let run = run.expect("a run row");
        assert_eq!(run.decided.as_deref(), Some("once"));
        assert_eq!(run.ended.as_deref(), Some("client_gone"));
        assert_eq!(run.exit_code, None);
        assert!(fx.broker.pending().is_empty());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn f_tl_12_7_the_pane_is_freed_after_deny_expiry_and_cancel() {
        let fx = fixture("API_KEY=vibezeo-secret://stripe-test\n").await;
        // Deny.
        let (view, task) = asked(&fx, req(&fx.agent, &fx.root, &["echo", "a"])).await;
        fx.broker
            .resolve(
                &view.id,
                ApprovalDecision::Deny { reason: None },
                ApprovalVia::Inline,
            )
            .expect("deny");
        assert_eq!(task.await.expect("task").error.as_deref(), Some(DECLINED));
        // Cancel (the pane closed).
        let (view, task) = asked(&fx, req(&fx.agent, &fx.root, &["echo", "b"])).await;
        fx.broker.cancel_source(&view.source);
        assert_eq!(task.await.expect("task").error.as_deref(), Some(CANCELLED));
        // Expiry, on a broker with a short time to answer.
        let short = ApprovalBroker::with_ttl(
            Arc::new(tandem_core::clock::SystemClock),
            Duration::from_millis(100),
        );
        let runs = AgentRuns::new(
            fx.store.clone(),
            Arc::new(MemoryKeychain::new()),
            short.clone(),
            fx.terminals.clone(),
            fx._dir.path().join("data"),
        );
        let reply = runs.handle(req(&fx.agent, &fx.root, &["echo", "c"])).await;
        assert_eq!(reply.error.as_deref(), Some(EXPIRED));
        // Each time, the pane may ask again at once.
        let reply = tokio::time::timeout(
            Duration::from_secs(5),
            runs.handle(req(&fx.agent, &fx.root, &["echo", "d"])),
        )
        .await
        .expect("asked, then expired");
        assert_eq!(reply.error.as_deref(), Some(EXPIRED));
        let (_, task) = asked(&fx, req(&fx.agent, &fx.root, &["echo", "e"])).await;
        fx.broker.cancel_source(&view.source);
        let _ = task.await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn f_tl_12_7_an_env_edited_after_allow_for_this_agent_asks_again() {
        let fx = fixture("API_KEY=vibezeo-secret://stripe-test\n").await;
        let (view, task) = asked(&fx, req(&fx.agent, &fx.root, &["echo", "ok"])).await;
        allow(&fx, &view, ApprovalScope::Conversation);
        assert_eq!(task.await.expect("task").output, "ok\n");
        assert_eq!(
            fx.runs
                .handle(req(&fx.agent, &fx.root, &["echo", "ok"]))
                .await
                .output,
            "ok\n"
        );
        // The agent adds a reference: the allowance no longer holds.
        std::fs::write(
            fx.root.join(".env"),
            "API_KEY=vibezeo-secret://stripe-test\nTOKEN=vibezeo-secret://stripe-test\n",
        )
        .expect(".env");
        let (view, task) = asked(&fx, req(&fx.agent, &fx.root, &["echo", "ok"])).await;
        assert!(
            view.summary.contains("TOKEN (stripe-test)"),
            "{}",
            view.summary
        );
        fx.broker.cancel_source(&view.source);
        let _ = task.await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn f_tl_12_7_a_loader_variable_in_env_never_reaches_the_run() {
        // As a plain value: not passed at all.
        let fx =
            fixture("LD_PRELOAD=/tmp/evil.so\nNODE_OPTIONS=--require ./x\nBASH_ENV=./x\n").await;
        let (view, task) = asked(
            &fx,
            req(
                &fx.agent,
                &fx.root,
                &["sh -c 'echo \"[$LD_PRELOAD][$NODE_OPTIONS][$BASH_ENV]\"'"],
            ),
        )
        .await;
        assert!(
            view.summary.ends_with("with no secrets"),
            "{}",
            view.summary
        );
        allow(&fx, &view, ApprovalScope::Once);
        // (The machine's own login environment may set some of these.)
        let out = task.await.expect("task").output;
        assert!(
            !out.contains("evil.so") && !out.contains("--require ./x") && !out.contains("[./x]"),
            "{out}"
        );
        // As a reference: refused, by name, before anyone is asked.
        for name in [
            "LD_PRELOAD",
            "DYLD_INSERT_LIBRARIES",
            "NODE_OPTIONS",
            "PYTHONSTARTUP",
            "BASH_ENV",
            "ENV",
            "PATH",
            "GIT_SSH_COMMAND",
        ] {
            std::fs::write(
                fx.root.join(".env"),
                format!("{name}=vibezeo-secret://stripe-test\n"),
            )
            .expect(".env");
            let reply = fx.runs.handle(req(&fx.agent, &fx.root, &["true"])).await;
            assert_eq!(
                reply.error.as_deref(),
                Some(format!("vibezeo run doesn't set {name}: it can change what a program loads or runs").as_str()),
                "{name}"
            );
            assert!(fx.broker.pending().is_empty());
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn f_tl_12_10_outside_an_agent_pane_or_with_a_missing_secret_nothing_runs() {
        let fx = fixture("API_KEY=vibezeo-secret://gone\n").await;
        let refused = |r: RunReply| r.error.unwrap_or_default();
        assert_eq!(
            refused(
                fx.runs
                    .handle(req(&fx.plain, &fx.root, &["touch", "a"]))
                    .await
            ),
            OUTSIDE
        );
        let nobody = PaneId::from_db("pn_nope".into());
        assert_eq!(
            refused(
                fx.runs
                    .handle(req(&nobody, &fx.root, &["touch", "a"]))
                    .await
            ),
            OUTSIDE
        );
        assert_eq!(
            refused(
                fx.runs
                    .handle(req(&fx.agent, &fx.root, &["touch", "a"]))
                    .await
            ),
            "No secret named gone"
        );
        std::fs::write(fx.root.join(".env"), "A=1\n").expect(".env");
        let outside = fx.root.parent().expect("parent").to_path_buf();
        assert!(refused(
            fx.runs
                .handle(req(&fx.agent, &outside, &["touch", "a"]))
                .await
        )
        .starts_with("vibezeo run works inside this pane's folder"));
        assert_eq!(
            refused(
                fx.runs
                    .handle(req(&fx.agent, &fx.root, &["touch", "a\nb"]))
                    .await
            ),
            run_request::NO_LINE_BREAKS
        );
        std::fs::write(fx.root.join(".env"), "A=vibezeo-secret://Bad\n").expect(".env");
        assert!(refused(
            fx.runs
                .handle(req(&fx.agent, &fx.root, &["touch", "a"]))
                .await
        )
        .contains("isn't a valid vibezeo-secret:// reference"));
        assert!(fx.broker.pending().is_empty());
        assert!(!fx.root.join("a").exists());
    }

    #[tokio::test]
    async fn f_tl_12_7_run_bin_holds_only_vibezeo() {
        let dir = tempfile::tempdir().expect("dir");
        let run = dir.path().join("run");
        std::fs::create_dir_all(run.join("bin/stray")).expect("mkdir");
        std::fs::write(run.join("bin/claude"), "x").expect("write");
        let hook = dir.path().join("tandem-hook");
        std::fs::write(&hook, "#!/bin/sh\n").expect("hook");
        let bin = install(&run, &hook).expect("install");
        let names: Vec<String> = std::fs::read_dir(&bin)
            .expect("read")
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, vec!["vibezeo".to_string()]);
        assert_eq!(std::fs::read_link(bin.join("vibezeo")).expect("link"), hook);
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&bin).expect("meta").permissions().mode();
            assert_eq!(mode & 0o777, 0o700);
        }
        // Again is a no-op.
        install(&run, &hook).expect("again");
        // A bin that's a link to somewhere else is refused.
        let elsewhere = dir.path().join("elsewhere");
        std::fs::create_dir_all(&elsewhere).expect("mkdir");
        std::fs::remove_dir_all(&bin).expect("rm");
        std::os::unix::fs::symlink(&elsewhere, &bin).expect("link");
        assert!(install(&run, &hook).is_err());
        assert!(std::fs::read_dir(&elsewhere)
            .expect("read")
            .next()
            .is_none());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn f_tl_12_7_one_question_per_pane_and_the_folder_is_part_of_allow_for_this_agent() {
        let fx = fixture("API_KEY=vibezeo-secret://stripe-test\n").await;
        let (view, task) = asked(&fx, req(&fx.agent, &fx.root, &["echo", "one"])).await;
        // A second run while the first one's question is open isn't asked.
        let second = fx
            .runs
            .handle(req(&fx.agent, &fx.root, &["echo", "two"]))
            .await;
        assert_eq!(second.error.as_deref(), Some(ONE_AT_A_TIME));
        assert_eq!(fx.broker.pending().len(), 1, "the shown question stays");
        allow(&fx, &view, ApprovalScope::Conversation);
        assert_eq!(task.await.expect("task").output, "one\n");
        // Answered: the pane may ask again. The same line in another
        // folder isn't covered by "Allow for this agent".
        let (other, task) =
            asked(&fx, req(&fx.agent, &fx.root.join("sub"), &["echo", "one"])).await;
        assert!(other.detail.contains("sub"), "{}", other.detail);
        fx.broker.cancel_source(&other.source);
        assert_eq!(task.await.expect("task").error.as_deref(), Some(CANCELLED));
        let reply = fx
            .runs
            .handle(req(&fx.agent, &fx.root, &["echo", "one"]))
            .await;
        assert_eq!(reply.output, "one\n");
    }

    #[cfg(unix)]
    #[tokio::test(flavor = "multi_thread")]
    async fn f_tl_12_7_a_folder_replaced_by_a_link_after_the_check_runs_nothing() {
        let fx = fixture("").await;
        let outside = fx._dir.path().join("outside");
        std::fs::create_dir_all(&outside).expect("mkdir");
        let checked = Checked {
            pane: fx.agent.clone(),
            cwd: fx.root.join("sub"),
            command: "touch ran".into(),
            vars: Vec::new(),
            fingerprint: refs::fingerprint(&[]),
            repo: fx.root.to_string_lossy().into_owned(),
            root: fx.root.clone(),
        };
        assert!(checked.folder_holds());
        std::fs::remove_dir(fx.root.join("sub")).expect("rmdir");
        std::os::unix::fs::symlink(&outside, fx.root.join("sub")).expect("link");
        assert!(!checked.folder_holds());
    }

    #[test]
    fn f_tl_12_7_the_run_env_has_no_relative_path_entries_and_no_env_overrides() {
        let sep = if cfg!(windows) { ";" } else { ":" };
        let abs = if cfg!(windows) { r"C:\bin" } else { "/usr/bin" };
        let login: std::collections::BTreeMap<String, String> = [
            (
                "PATH".to_string(),
                [abs, "", ".", "bin", "./x", abs].join(sep),
            ),
            ("API_KEY".to_string(), "from-login".to_string()),
            (
                "NoDefaultCurrentDirectoryInExePath".to_string(),
                "0".to_string(),
            ),
            ("HOME".to_string(), "/home/me".to_string()),
        ]
        .into_iter()
        .collect();
        let env = run_env(&login, &["API_KEY"]);
        let get = |k: &str| {
            env.iter()
                .filter(|(n, _)| n == k)
                .map(|(_, v)| v.as_str())
                .collect::<Vec<_>>()
        };
        assert_eq!(get("PATH"), vec![[abs, abs].join(sep)]);
        assert!(get("API_KEY").is_empty(), ".env sets it");
        assert_eq!(get("HOME"), vec!["/home/me"]);
        // Nothing absolute left: a system default, never an empty PATH.
        let only_relative: std::collections::BTreeMap<String, String> =
            [("PATH".to_string(), [".", "bin", ""].join(sep))]
                .into_iter()
                .collect();
        let path = run_env(&only_relative, &[])
            .into_iter()
            .find(|(k, _)| k == "PATH")
            .map(|(_, v)| v)
            .expect("PATH");
        assert!(!path.is_empty());
        assert!(
            path.split(sep).all(|p| Path::new(p).is_absolute()),
            "{path}"
        );
        if cfg!(windows) {
            assert_eq!(get("NoDefaultCurrentDirectoryInExePath"), vec!["1"]);
        } else {
            assert!(get("NoDefaultCurrentDirectoryInExePath").is_empty());
        }
    }
}
