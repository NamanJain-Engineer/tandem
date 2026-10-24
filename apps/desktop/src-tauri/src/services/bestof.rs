//! Best of N (F-EX-04): one prompt to two or three agents, each in its own
//! worktree lane from the same commit; compare their diffs, run the same
//! check in each, merge the winner and discard the others. In a folder on
//! an SSH host every step runs there, with the host's git (D-1605).

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use tandem_core::bus::{BusEvent, Topic};
use tandem_core::ids::{new_id, AccountId, HostId, WorkspaceId};
use tandem_core::origin::{LocalReview, Origin};
use tandem_core::redact::Redactor;
use tandem_core::{ErrorCode, IpcError};
use tandem_git::{FileDiff, ForceConfirm, GitStatus, MergeReport, Repo};
use tandem_store::repo::bestof::{self as runs, BestOfLane, BestOfRun, CheckState, RunState};
use tandem_store::repo::presets::{Isolation, Seat, SeatKind};
use tandem_store::repo::workspaces::{self, Workspace};

use crate::services::remote_files::RemoteRoot;
use crate::services::terminal::LaunchMode;
use crate::services::{lanes, launch};
use crate::state::AppState;

pub const NOT_A_REPO: &str = "Best of N needs a git repository";
pub const WSL_UNAVAILABLE: &str = "Best of N isn't available in WSL folders yet";
pub const SEATS: &str = "Choose 2 or 3 agents";
pub const NO_CHECK: &str = "Add a check command first";
pub const FIRST_COMMIT: &str = "Make a first commit, then start Best of N";
const LANE_MISSING: &str = "The worktree folder is missing";
/// A host's check runs in the host user's login shell, as its terminals
/// and engine detection do; `$1` is the check (D-1605).
pub const HOST_CHECK_SHELL: &str = "exec \"${SHELL:-/bin/sh}\" -lc \"$1\"";
/// A check stops after this long.
pub const CHECK_LIMIT: Duration = Duration::from_secs(600);
/// Lines of output a check keeps.
pub const TAIL_LINES: usize = 200;
/// The size agents start at before their lane is shown; the view resizes.
const START_COLS: u16 = 120;
const START_ROWS: u16 = 32;

/// One seat of the sheet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct BestOfSeat {
    pub engine_id: String,
    pub account_id: Option<String>,
}

/// One lane's changes against the run's starting commit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct LaneDiff {
    pub seat: u32,
    pub files: Vec<FileDiff>,
    pub error: Option<String>,
}

/// `bestof_get`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct BestOfDetail {
    pub run: BestOfRun,
    pub diffs: Vec<LaneDiff>,
}

/// A check's process group while it runs, and whether it was stopped:
/// stopping before the spawn means it never starts, and stopping right
/// after kills it as soon as it's registered (F-WT-07 AC6).
#[derive(Debug, Default)]
pub struct CheckSlot {
    pid: Mutex<Option<u32>>,
    cancelled: std::sync::atomic::AtomicBool,
}

impl CheckSlot {
    /// Stops the check: kills it if running, or keeps it from starting.
    pub fn cancel(&self) {
        let pid = self.pid.lock().unwrap_or_else(|e| e.into_inner());
        self.cancelled
            .store(true, std::sync::atomic::Ordering::SeqCst);
        if let Some(pid) = *pid {
            kill_group(pid);
        }
    }

    pub fn cancelled(&self) -> bool {
        self.cancelled.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Registers the spawned check; false (and killed) when already cancelled.
    fn register(&self, pid: u32) -> bool {
        let mut slot = self.pid.lock().unwrap_or_else(|e| e.into_inner());
        if self.cancelled() {
            kill_group(pid);
            return false;
        }
        *slot = Some(pid);
        true
    }

    fn clear(&self) {
        *self.pid.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }

    #[cfg(test)]
    pub fn pid(&self) -> Option<u32> {
        *self.pid.lock().unwrap_or_else(|e| e.into_inner())
    }
}

pub type GroupSlot = Arc<CheckSlot>;

/// How much of a check's output is kept: its last 64 KiB.
pub const OUTPUT_CAP: usize = 64 * 1024;

/// Running checks, per run, to stop them.
#[derive(Default)]
pub struct Checks {
    running: Mutex<HashMap<String, Vec<(tokio::task::AbortHandle, GroupSlot)>>>,
}

fn changed(state: &AppState, run: &str) {
    state.bus.publish(BusEvent::new(
        Topic::App,
        "bestof:changed",
        serde_json::json!({ "runId": run }),
    ));
}

fn invalid(msg: &str) -> IpcError {
    IpcError::new(ErrorCode::Invalid, msg).with_stage("bestof")
}

async fn run_of(state: &AppState, id: &str) -> Result<BestOfRun, IpcError> {
    runs::get(&state.store, id.to_string())
        .await?
        .ok_or_else(|| {
            IpcError::new(ErrorCode::NotFound, "That Best of N run is gone").with_stage("bestof")
        })
}

/// The host and folder when `ws` is in a folder on an SSH host (D-1605):
/// its git runs there, through the helper.
fn on_host(ws: &Workspace) -> Option<RemoteRoot> {
    (ws.kind == "ssh").then(|| RemoteRoot {
        host: HostId::from_db(ws.host_id.clone()),
        root: ws.root.clone(),
    })
}

/// A folder's git status, read here or on its host; `missing` when it
/// isn't a repository (or is gone).
async fn status_of(state: &AppState, ws: &Workspace, missing: &str) -> Result<GitStatus, IpcError> {
    if let Some(r) = on_host(ws) {
        return state
            .remote
            .files
            .git_status(&state.remote, &r)
            .await?
            .map(|(_, _, status)| status)
            .ok_or_else(|| invalid(missing));
    }
    let repo = Repo::discover(Path::new(&ws.root))
        .ok()
        .flatten()
        .ok_or_else(|| invalid(missing))?;
    Ok(repo.status().await?)
}

/// What `tandem-host run` starts for a check on a host: the login shell
/// with the check as one argument (`run` quotes each one for the host).
pub fn host_check_argv(command: &str) -> Vec<String> {
    vec![
        "/bin/sh".into(),
        "-c".into(),
        HOST_CHECK_SHELL.into(),
        "check".into(),
        format!("exec 2>&1\n{command}"),
    ]
}

/// `bestof_start`: every seat or none (the plan's pane limit), each in a
/// worktree from the current HEAD with the same prompt.
pub async fn start(
    state: &AppState,
    workspace: &WorkspaceId,
    prompt: &str,
    seats: &[BestOfSeat],
    check_command: Option<String>,
) -> Result<BestOfRun, IpcError> {
    if prompt.trim().is_empty() {
        return Err(invalid("Write the prompt first"));
    }
    if !(2..=3).contains(&seats.len()) {
        return Err(invalid(SEATS));
    }
    let ws = workspaces::get(&state.store, workspace).await?;
    if ws.kind == "wsl" {
        return Err(IpcError::new(ErrorCode::Unavailable, WSL_UNAVAILABLE).with_stage("bestof"));
    }
    // A host's lanes are made by its git (D-1602), from its HEAD; each
    // lane records that commit, which is the run's base.
    let base = if on_host(&ws).is_some() {
        if status_of(state, &ws, NOT_A_REPO).await?.no_commits {
            return Err(invalid(FIRST_COMMIT));
        }
        None
    } else {
        let repo = Repo::discover(Path::new(&ws.root))
            .ok()
            .flatten()
            .ok_or_else(|| invalid(NOT_A_REPO))?;
        Some(repo.head().await?.ok_or_else(|| invalid(FIRST_COMMIT))?)
    };
    let lineup: Vec<Seat> = seats
        .iter()
        .map(|s| Seat {
            kind: SeatKind::Agent,
            engine_id: Some(s.engine_id.clone()),
            account_id: s.account_id.clone().map(AccountId::from_db),
            role: None,
            task: None,
            command: None,
            url: None,
            count: 1,
        })
        .collect();
    let report =
        launch::launch(state, workspace, &lineup, Some(prompt), Isolation::Worktree).await?;
    let mut lane_rows: Vec<BestOfLane> = seats
        .iter()
        .enumerate()
        .map(|(i, s)| BestOfLane {
            seat: i as u32,
            lane_id: None,
            branch: None,
            engine_id: s.engine_id.clone(),
            account_id: s.account_id.clone(),
            pane_id: None,
            error: None,
            check: CheckState::None,
            check_exit: None,
            check_output: None,
        })
        .collect();
    for l in &report.launched {
        if let Some(row) = lane_rows.get_mut(l.seat_index as usize) {
            row.lane_id = Some(l.workspace_id.as_str().to_string());
            row.branch = l.lane_branch.clone();
            row.pane_id = Some(l.pane.id.as_str().to_string());
        }
        // Every agent starts now, not when its lane is first shown: the
        // output waits in the ring until a view attaches (as restores do).
        // The launch already counted these panes against the plan.
        if let Err(e) = state
            .terminals
            .spawn_with(
                &l.pane.id,
                START_COLS,
                START_ROWS,
                None,
                None,
                LaunchMode::Normal,
            )
            .await
        {
            if let Some(row) = lane_rows.get_mut(l.seat_index as usize) {
                row.error = Some(e.message.clone());
            }
        }
    }
    for f in &report.failed {
        if let Some(row) = lane_rows.get_mut(f.seat_index as usize) {
            row.error = Some(f.error.message.clone());
        }
    }
    let base = match base {
        Some(b) => b,
        None => {
            let mut found = String::new();
            for l in &report.launched {
                if let Ok(lane) = workspaces::get(&state.store, &l.workspace_id).await {
                    if let Some(b) = lane.lane_base {
                        found = b;
                        break;
                    }
                }
            }
            found
        }
    };
    let run = runs::insert(
        &state.store,
        BestOfRun {
            id: new_id("bo"),
            workspace_id: workspace.as_str().to_string(),
            title: launch::tab_name(Some(prompt)),
            tab_id: None,
            base,
            check_command: check_command
                .map(|c| c.trim().to_string())
                .filter(|c| !c.is_empty()),
            state: RunState::Running,
            created_at: state.clock.now_ms(),
            lanes: lane_rows,
        },
    )
    .await?;
    tracing::info!(
        code = "bestof.started",
        lanes = run.lanes.len(),
        "best of n started"
    );
    changed(state, &run.id);
    Ok(run)
}

/// `bestof_get`: the run with each lane's changes since its start.
pub async fn detail(state: &AppState, id: &str) -> Result<BestOfDetail, IpcError> {
    let run = run_of(state, id).await?;
    let mut diffs = Vec::new();
    for lane in &run.lanes {
        let Some(lane_id) = &lane.lane_id else {
            continue;
        };
        let (files, error) =
            match lanes::diff(&state.store, &WorkspaceId::from_db(lane_id.clone())).await {
                Ok(f) => (f, None),
                Err(e) => (Vec::new(), Some(e.message)),
            };
        diffs.push(LaneDiff {
            seat: lane.seat,
            files,
            error,
        });
    }
    Ok(BestOfDetail { run, diffs })
}

/// The last `n` lines of `text`.
pub fn tail(text: &str, n: usize) -> String {
    let lines: Vec<&str> = text.lines().collect();
    lines[lines.len().saturating_sub(n)..].join("\n")
}

/// Runs `command` in `cwd` with the login environment, stdout and stderr
/// together; `None` exit when it timed out or was killed.
pub async fn run_check(
    command: &str,
    cwd: &Path,
    env: &std::collections::BTreeMap<String, String>,
    limit: Duration,
    group_slot: Option<GroupSlot>,
) -> (Option<i32>, String) {
    run_prepared(local_check(command, cwd), env, limit, group_slot).await
}

/// A check here: `/bin/sh` (`cmd` on Windows) in `cwd`.
fn local_check(command: &str, cwd: &Path) -> tokio::process::Command {
    let mut c = if cfg!(windows) {
        let mut c = tokio::process::Command::new("cmd");
        c.args(["/C", &format!("{command} 2>&1")]);
        c
    } else {
        let mut c = tokio::process::Command::new("/bin/sh");
        c.args(["-c", &format!("exec 2>&1\n{command}")]);
        c
    };
    c.current_dir(cwd);
    c
}

/// Appends `chunk`, keeping only the last `cap` bytes; true once cut.
pub fn push_capped(buf: &mut Vec<u8>, chunk: &[u8], cap: usize) -> bool {
    buf.extend_from_slice(chunk);
    if buf.len() > cap {
        let cut = buf.len() - cap;
        buf.drain(..cut);
        return true;
    }
    false
}

/// Reads `from` to its end into `into`, capped (see [`push_capped`]).
async fn read_capped(
    mut from: impl tokio::io::AsyncRead + Unpin,
    into: Arc<Mutex<(Vec<u8>, bool)>>,
) {
    use tokio::io::AsyncReadExt;
    let mut chunk = vec![0u8; 8192];
    loop {
        match from.read(&mut chunk).await {
            Ok(0) | Err(_) => return,
            Ok(n) => {
                let mut b = into.lock().unwrap_or_else(|e| e.into_inner());
                let cut = push_capped(&mut b.0, &chunk[..n], OUTPUT_CAP);
                b.1 |= cut;
            }
        }
    }
}

fn captured(buf: &Mutex<(Vec<u8>, bool)>) -> String {
    let b = buf.lock().unwrap_or_else(|e| e.into_inner());
    let text = String::from_utf8_lossy(&b.0).into_owned();
    if b.1 {
        format!("[… earlier output cut]\n{text}")
    } else {
        text
    }
}

/// Runs a check's process (here, or the ssh that runs it on a host) in its
/// own process group, stdout and stderr together, keeping the last
/// [`OUTPUT_CAP`] bytes; `None` exit when it timed out or was stopped.
/// Killing a host check's ssh ends it there too: `tandem-host run` ends the
/// check's group once its input closes.
pub async fn run_prepared(
    mut cmd: tokio::process::Command,
    env: &std::collections::BTreeMap<String, String>,
    limit: Duration,
    group_slot: Option<GroupSlot>,
) -> (Option<i32>, String) {
    const STOPPED: &str = "Stopped.";
    if group_slot.as_ref().is_some_and(|s| s.cancelled()) {
        return (None, STOPPED.into());
    }
    cmd.env_clear()
        .envs(env)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    #[cfg(unix)]
    cmd.process_group(0);
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => return (None, e.to_string()),
    };
    let group = child.id();
    if let (Some(slot), Some(pid)) = (&group_slot, group) {
        if !slot.register(pid) {
            let _ = child.wait().await;
            return (None, STOPPED.into());
        }
    }
    let (out, err) = (
        Arc::new(Mutex::new((Vec::new(), false))),
        Arc::new(Mutex::new((Vec::new(), false))),
    );
    let readers = (
        child
            .stdout
            .take()
            .map(|o| tokio::spawn(read_capped(o, out.clone()))),
        child
            .stderr
            .take()
            .map(|e| tokio::spawn(read_capped(e, err.clone()))),
    );
    let waited = tokio::time::timeout(limit, child.wait()).await;
    let code = match &waited {
        Ok(Ok(status)) => status.code(),
        _ => None,
    };
    if waited.is_err() {
        if let Some(pid) = group {
            kill_group(pid);
        }
        let _ = child.wait().await;
    }
    // The group is gone once the check ends: Stop must not signal a
    // process group id the system has since given to someone else.
    if let Some(slot) = &group_slot {
        slot.clear();
    }
    // Grandchildren holding the pipes open can't keep the output waiting.
    for reader in [readers.0, readers.1].into_iter().flatten() {
        let _ = tokio::time::timeout(Duration::from_secs(2), reader).await;
    }
    let mut text = captured(&out);
    text.push_str(&captured(&err));
    match waited {
        Ok(Ok(_)) => (code, text),
        Ok(Err(e)) => (None, e.to_string()),
        Err(_) => {
            text.push_str(&format!(
                "\nStopped after {} minutes.",
                limit.as_secs() / 60
            ));
            (None, text)
        }
    }
}

/// How a check's whole process tree is killed (a test runner's workers
/// too): its process group on Unix, `taskkill /T /F` on Windows.
pub trait TreeKiller {
    fn argv(&self, pid: u32) -> Vec<String>;
}

pub struct UnixKiller;
pub struct WindowsKiller;

impl TreeKiller for UnixKiller {
    fn argv(&self, pid: u32) -> Vec<String> {
        ["kill", "-KILL", "--", &format!("-{pid}")]
            .map(String::from)
            .to_vec()
    }
}

impl TreeKiller for WindowsKiller {
    fn argv(&self, pid: u32) -> Vec<String> {
        ["taskkill", "/T", "/F", "/PID", &pid.to_string()]
            .map(String::from)
            .to_vec()
    }
}

fn system_killer() -> &'static dyn TreeKiller {
    if cfg!(windows) {
        &WindowsKiller
    } else {
        &UnixKiller
    }
}

/// Kills a check's whole process tree.
pub fn kill_group(pid: u32) {
    let argv = system_killer().argv(pid);
    let Some((program, args)) = argv.split_first() else {
        return;
    };
    let mut cmd = std::process::Command::new(program);
    cmd.args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // CREATE_NO_WINDOW: no console flashes up.
        cmd.creation_flags(0x0800_0000);
    }
    let _ = cmd.status();
}

/// `bestof_check`: the run's check command in every lane at once.
pub async fn check(state: &AppState, id: &str) -> Result<(), IpcError> {
    let run = run_of(state, id).await?;
    let command = run.check_command.clone().ok_or_else(|| invalid(NO_CHECK))?;
    let login = state.terminals.login_env().await;
    let env = login.vars.clone();
    stop_checks(state, id).await;
    let mut handles = Vec::new();
    for lane in run.lanes.iter().filter(|l| l.lane_id.is_some()) {
        let Ok(ws) = workspaces::get(
            &state.store,
            &WorkspaceId::from_db(lane.lane_id.clone().unwrap_or_default()),
        )
        .await
        else {
            continue;
        };
        // On a host, the check runs there through `tandem-host run`, in the
        // worktree the host's git made (D-1605).
        let cmd = match on_host(&ws) {
            Some(r) => {
                let (program, args) = state
                    .remote
                    .run_command(
                        &r.host,
                        &r.root,
                        &std::collections::BTreeMap::new(),
                        &host_check_argv(&command),
                    )
                    .await?;
                let mut c = tokio::process::Command::new(program);
                c.args(args);
                c
            }
            None => local_check(&command, Path::new(&ws.root)),
        };
        // F-WT-06: a local check sees the worktree's variables.
        let mut lane_env = env.clone();
        lane_env.extend(crate::services::worktree_runtime::vars_for(
            &state.data,
            &ws,
        ));
        runs::set_check(
            &state.store,
            id.to_string(),
            lane.seat,
            CheckState::Running,
            None,
            None,
        )
        .await?;
        let (store, bus, run_id, seat, env) = (
            state.store.clone(),
            state.bus.clone(),
            id.to_string(),
            lane.seat,
            lane_env,
        );
        let slot: GroupSlot = Arc::default();
        let task_slot = slot.clone();
        let task = tokio::spawn(async move {
            let (exit, out) = run_prepared(cmd, &env, CHECK_LIMIT, Some(task_slot)).await;
            let out = Redactor::global()
                .redact(&tail(&out, TAIL_LINES))
                .into_owned();
            let check = match exit {
                Some(0) => CheckState::Passed,
                Some(_) => CheckState::Failed,
                None => CheckState::Stopped,
            };
            let _ = runs::set_check(&store, run_id.clone(), seat, check, exit, Some(out)).await;
            bus.publish(BusEvent::new(
                Topic::App,
                "bestof:changed",
                serde_json::json!({ "runId": run_id }),
            ));
        });
        handles.push((task.abort_handle(), slot));
    }
    state
        .bestof_checks
        .running
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(id.to_string(), handles);
    changed(state, id);
    Ok(())
}

/// `bestof_check_stop`.
pub async fn stop_checks(state: &AppState, id: &str) {
    let handles = state
        .bestof_checks
        .running
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(id)
        .unwrap_or_default();
    if handles.is_empty() {
        return;
    }
    for (h, slot) in handles {
        slot.cancel();
        h.abort();
    }
    if let Ok(run) = run_of(state, id).await {
        for lane in run.lanes.iter().filter(|l| l.check == CheckState::Running) {
            let _ = runs::set_check(
                &state.store,
                id.to_string(),
                lane.seat,
                CheckState::Stopped,
                None,
                lane.check_output.clone(),
            )
            .await;
        }
    }
    changed(state, id);
}

fn force(origin: &Origin) -> Result<ForceConfirm, IpcError> {
    LocalReview::check(origin)
        .map(ForceConfirm::new)
        .ok_or_else(|| {
            IpcError::new(
                ErrorCode::PermissionDenied,
                "This needs a confirmation in the main window",
            )
        })
}

/// The native confirm's copy for "Keep this one…" (F-EX-04), after the
/// checks that refuse without asking: a finished run, a seat without a
/// worktree, a base checkout with uncommitted changes.
pub async fn keep_question(
    state: &AppState,
    id: &str,
    seat: u32,
) -> Result<(String, String), IpcError> {
    let run = run_of(state, id).await?;
    if run.state != RunState::Running {
        return Err(IpcError::new(ErrorCode::Conflict, "This run is finished").with_stage("bestof"));
    }
    let winner = run
        .lanes
        .iter()
        .find(|l| l.seat == seat && l.lane_id.is_some())
        .ok_or_else(|| invalid("That agent has no worktree"))?;
    let base_ws = workspaces::get(
        &state.store,
        &WorkspaceId::from_db(run.workspace_id.clone()),
    )
    .await?;
    if status_of(state, &base_ws, NOT_A_REPO)
        .await?
        .has_tracked_changes()
    {
        return Err(IpcError::new(
            ErrorCode::Conflict,
            format!("Commit or stash your changes in {} first.", base_ws.name),
        )
        .with_stage("bestof"));
    }
    let branch = winner.branch.clone().unwrap_or_default();
    let engine = state
        .terminals
        .engines
        .get(&winner.engine_id)
        .map(|e| e.label.clone())
        .unwrap_or_else(|| winner.engine_id.clone());
    let others = run
        .lanes
        .iter()
        .filter(|l| l.seat != seat && l.lane_id.is_some())
        .count();
    let worktrees = if others == 1 { "worktree" } else { "worktrees" };
    let base = base_ws.name;
    Ok((
        format!("Merge {branch} into {base}?"),
        format!(
            "Commits {engine}'s uncommitted changes on {branch}, merges it into {base}, and deletes the other {others} {worktrees} and their uncommitted changes. Their branches stay."
        ),
    ))
}

/// The native confirm's copy for "Discard all…".
pub async fn discard_question(state: &AppState, id: &str) -> Result<(String, String), IpcError> {
    let run = run_of(state, id).await?;
    let n = run.lanes.iter().filter(|l| l.lane_id.is_some()).count();
    let title = if n == 1 {
        "Discard 1 worktree?".to_string()
    } else {
        format!("Discard all {n} worktrees?")
    };
    Ok((
        title,
        "Removes each worktree, its uncommitted changes and its panes. The branches stay."
            .to_string(),
    ))
}

/// `bestof_keep`: commits the winner's changes, merges its branch into the
/// base checkout, and removes the other worktrees (their branches stay).
/// A dirty base refuses before anything changes; a conflict keeps every lane.
pub async fn keep(
    state: &AppState,
    origin: &Origin,
    id: &str,
    seat: u32,
) -> Result<MergeReport, IpcError> {
    let run = run_of(state, id).await?;
    if run.state != RunState::Running {
        return Err(IpcError::new(ErrorCode::Conflict, "This run is finished").with_stage("bestof"));
    }
    let winner = run
        .lanes
        .iter()
        .find(|l| l.seat == seat)
        .and_then(|l| l.lane_id.clone().map(|id| (l, id)))
        .ok_or_else(|| invalid("That agent has no worktree"))?;
    let base_ws = workspaces::get(
        &state.store,
        &WorkspaceId::from_db(run.workspace_id.clone()),
    )
    .await?;
    if status_of(state, &base_ws, NOT_A_REPO)
        .await?
        .has_tracked_changes()
    {
        return Err(IpcError::new(
            ErrorCode::Conflict,
            format!("Commit or stash your changes in {} first.", base_ws.name),
        )
        .with_stage("bestof"));
    }
    stop_checks(state, id).await;
    tracing::info!(
        code = "bestof.keep",
        step = "checks stopped",
        "keeping a lane"
    );
    let login = state.terminals.login_env().await;
    let git = lanes::git_for(&state.data, &login)?;
    // Every lane's agent stops first.
    for lane in &run.lanes {
        if let Some(l) = &lane.lane_id {
            lanes::stop_panes(
                &state.store,
                &state.terminals.pty,
                &WorkspaceId::from_db(l.clone()),
            )
            .await?;
        }
    }
    tracing::info!(
        code = "bestof.keep",
        step = "agents stopped",
        "keeping a lane"
    );
    let lane_ws = workspaces::get(&state.store, &WorkspaceId::from_db(winner.1.clone())).await?;
    if !status_of(state, &lane_ws, LANE_MISSING).await?.is_clean() {
        let engine = state
            .terminals
            .engines
            .get(&winner.0.engine_id)
            .map(|e| e.label.clone())
            .unwrap_or_else(|| winner.0.engine_id.clone());
        let message = format!("Best of N: {} ({engine})", run.title);
        // A missing git identity comes back saying how to set one.
        if let Some(r) = on_host(&lane_ws) {
            // The host's git, with the host's identity (D-1605).
            use tandem_host::protocol::method;
            state
                .remote
                .call::<serde_json::Value>(
                    &r.host,
                    method::GIT_OP,
                    serde_json::json!({"root": r.root, "op": "stage"}),
                )
                .await?;
            tracing::info!(code = "bestof.keep", step = "staged", "keeping a lane");
            state
                .remote
                .call::<serde_json::Value>(
                    &r.host,
                    method::GIT_OP,
                    serde_json::json!({"root": r.root, "op": "commit", "message": message}),
                )
                .await?;
        } else {
            let lane_repo = Repo::discover(Path::new(&lane_ws.root))
                .ok()
                .flatten()
                .ok_or_else(|| invalid(LANE_MISSING))?;
            tandem_git::ops::stage_all(&git, &lane_repo).await?;
            tracing::info!(code = "bestof.keep", step = "staged", "keeping a lane");
            tandem_git::ops::commit(&git, &lane_repo, &message).await?;
        }
    }
    tracing::info!(code = "bestof.keep", step = "committed", "keeping a lane");
    let report = lanes::merge(&state.store, &git, &WorkspaceId::from_db(winner.1.clone())).await?;
    tracing::info!(
        code = "bestof.keep",
        step = "merged",
        merged = report.merged,
        "keeping a lane"
    );
    if !report.merged {
        changed(state, id);
        return Ok(report);
    }
    for lane in run.lanes.iter().filter(|l| l.seat != seat) {
        if let Some(l) = &lane.lane_id {
            if let Err(e) = lanes::remove(
                &state.store,
                &state.terminals.pty,
                &git,
                &WorkspaceId::from_db(l.clone()),
                Some(force(origin)?),
            )
            .await
            {
                tracing::warn!(code = "bestof.remove_failed", error = %e.message, "couldn't remove a worktree");
            }
        }
    }
    runs::set_state(&state.store, id.to_string(), RunState::Merged).await?;
    tracing::info!(code = "bestof.merged", "best of n merged");
    changed(state, id);
    Ok(report)
}

/// `bestof_discard`: every lane's worktree goes; branches stay.
pub async fn discard(state: &AppState, origin: &Origin, id: &str) -> Result<(), IpcError> {
    let run = run_of(state, id).await?;
    stop_checks(state, id).await;
    let login = state.terminals.login_env().await;
    let git = lanes::git_for(&state.data, &login)?;
    for lane in &run.lanes {
        if let Some(l) = &lane.lane_id {
            let lane_id = WorkspaceId::from_db(l.clone());
            if workspaces::get(&state.store, &lane_id).await.is_ok() {
                lanes::remove(
                    &state.store,
                    &state.terminals.pty,
                    &git,
                    &lane_id,
                    Some(force(origin)?),
                )
                .await?;
            }
        }
    }
    runs::set_state(&state.store, id.to_string(), RunState::Discarded).await?;
    changed(state, id);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::process::Command;
    use tandem_core::DataDir;
    use tandem_store::repo::workspaces::NewWorkspace;
    use tandem_store::Store;

    fn git(cwd: &Path, args: &[&str]) -> String {
        let out = Command::new("git")
            .args([
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@example.com",
                "-c",
                "commit.gpgsign=false",
            ])
            .args(args)
            .current_dir(cwd)
            .output()
            .expect("git");
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    /// A repository workspace with two lanes recorded as a run.
    async fn fixture() -> (tempfile::TempDir, AppState, PathBuf, BestOfRun) {
        let tmp = tempfile::tempdir().expect("tmp");
        let data = DataDir::at(tmp.path().join("data")).expect("data");
        let store = Store::open(&data).expect("store");
        let state = AppState::for_tests(data, store);
        state.terminals.set_login_env(tandem_pty::env::LoginEnv {
            vars: std::env::vars()
                .chain([
                    ("GIT_AUTHOR_NAME".into(), "t".into()),
                    ("GIT_AUTHOR_EMAIL".into(), "t@example.com".into()),
                    ("GIT_COMMITTER_NAME".into(), "t".into()),
                    ("GIT_COMMITTER_EMAIL".into(), "t@example.com".into()),
                ])
                .collect(),
            shell: "/bin/sh".into(),
            degraded: false,
        });
        let repo = tmp.path().join("shop");
        std::fs::create_dir_all(&repo).expect("mkdir");
        git(&repo, &["init", "-q", "-b", "main"]);
        git(&repo, &["config", "user.name", "t"]);
        git(&repo, &["config", "user.email", "t@example.com"]);
        std::fs::write(repo.join("a.txt"), "one\n").expect("w");
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "-qm", "first"]);
        let repo = std::fs::canonicalize(&repo).expect("canon");
        let (ws, _) = workspaces::add(
            &state.store,
            NewWorkspace {
                kind: "local".into(),
                host_id: "local".into(),
                root: repo.display().to_string(),
                name: "shop".into(),
                wsl_distro: None,
            },
            None,
        )
        .await
        .expect("ws");
        let login = state.terminals.login_env().await;
        let g = lanes::git_for(&state.data, &login).expect("git");
        let mut rows = Vec::new();
        for seat in 0..2u32 {
            let lane = lanes::create(&state.store, &g, &ws.id, "fix login")
                .await
                .expect("lane");
            rows.push(BestOfLane {
                seat,
                lane_id: Some(lane.id.as_str().to_string()),
                branch: lane.lane_branch.clone(),
                engine_id: "claude".into(),
                account_id: None,
                pane_id: None,
                error: None,
                check: CheckState::None,
                check_exit: None,
                check_output: None,
            });
        }
        let base = git(&repo, &["rev-parse", "HEAD"]);
        let run = runs::insert(
            &state.store,
            BestOfRun {
                id: new_id("bo"),
                workspace_id: ws.id.as_str().to_string(),
                title: "Fix login".into(),
                tab_id: None,
                base,
                check_command: Some("test -f a.txt && grep -q TWO a.txt".into()),
                state: RunState::Running,
                created_at: 0,
                lanes: rows,
            },
        )
        .await
        .expect("run");
        (tmp, state, repo, run)
    }

    fn lane_root(state_run: &BestOfRun, seat: usize, repo: &Path) -> PathBuf {
        let branch = state_run.lanes[seat].branch.clone().expect("branch");
        repo.join(".tandem/worktrees")
            .join(branch.trim_start_matches("tandem/"))
    }

    fn main_window() -> Origin {
        Origin::Local {
            window: "main".into(),
        }
    }

    #[tokio::test]
    async fn f_ex_04_2_compare_shows_each_lanes_changes_committed_and_not() {
        let (_t, state, repo, run) = fixture().await;
        let one = lane_root(&run, 0, &repo);
        std::fs::write(one.join("a.txt"), "one\nTWO\n").expect("w");
        git(&one, &["commit", "-qam", "agent commit"]);
        std::fs::write(one.join("b.txt"), "new\n").expect("w");
        let d = detail(&state, &run.id).await.expect("detail");
        let files: Vec<&str> = d.diffs[0].files.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(files, ["a.txt", "b.txt"]);
        assert!(d.diffs[1].files.is_empty());
    }

    #[tokio::test]
    async fn f_ex_04_3_the_check_runs_in_each_lane_with_its_own_result() {
        let (_t, state, repo, run) = fixture().await;
        std::fs::write(lane_root(&run, 1, &repo).join("a.txt"), "one\nTWO\n").expect("w");
        check(&state, &run.id).await.expect("check");
        let mut got = None;
        for _ in 0..100 {
            let r = run_of(&state, &run.id).await.expect("run");
            if r.lanes.iter().all(|l| l.check != CheckState::Running) {
                got = Some(r);
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        let r = got.expect("checks finished");
        assert_eq!(
            (r.lanes[0].check, r.lanes[0].check_exit),
            (CheckState::Failed, Some(1))
        );
        assert_eq!(
            (r.lanes[1].check, r.lanes[1].check_exit),
            (CheckState::Passed, Some(0))
        );
    }

    #[tokio::test]
    async fn f_ex_04_4_keep_commits_merges_and_removes_the_others_but_not_their_branches() {
        let (_t, state, repo, run) = fixture().await;
        let winner = lane_root(&run, 1, &repo);
        std::fs::write(winner.join("a.txt"), "one\nWINNER\n").expect("w");
        std::fs::write(lane_root(&run, 0, &repo).join("a.txt"), "loser\n").expect("w");
        // A base checkout with uncommitted changes refuses before anything changes.
        std::fs::write(repo.join("a.txt"), "dirty\n").expect("w");
        let err = keep(&state, &main_window(), &run.id, 1)
            .await
            .expect_err("dirty");
        assert_eq!(err.code, ErrorCode::Conflict);
        assert!(lane_root(&run, 0, &repo).is_dir());
        git(&repo, &["checkout", "--", "a.txt"]);
        // Only the main window may discard worktrees.
        let other = Origin::Local {
            window: "dictate-widget".into(),
        };
        assert!(LocalReview::check(&other).is_none());
        let report = keep(&state, &main_window(), &run.id, 1)
            .await
            .expect("keep");
        assert!(report.merged, "{report:?}");
        assert_eq!(
            std::fs::read_to_string(repo.join("a.txt")).expect("r"),
            "one\nWINNER\n"
        );
        assert!(
            !lane_root(&run, 0, &repo).exists(),
            "the other worktree stays"
        );
        let branches = git(&repo, &["branch", "--list", "tandem/*"]);
        assert!(
            branches.contains(run.lanes[0].branch.as_deref().expect("b")),
            "{branches}"
        );
        assert_eq!(
            run_of(&state, &run.id).await.expect("run").state,
            RunState::Merged
        );
    }

    #[tokio::test]
    async fn f_ex_04_4_the_confirm_names_the_branch_the_base_and_the_others_and_a_dirty_base_refuses_first(
    ) {
        let (_t, state, repo, run) = fixture().await;
        let n = run.lanes.len();
        let branch = run.lanes[1].branch.clone().expect("b");
        let base = workspaces::get(
            &state.store,
            &WorkspaceId::from_db(run.workspace_id.clone()),
        )
        .await
        .expect("ws")
        .name;
        let (title, body) = keep_question(&state, &run.id, 1).await.expect("question");
        assert_eq!(title, format!("Merge {branch} into {base}?"));
        assert!(
            body.contains(&format!(
                "merges it into {base}, and deletes the other {} worktree",
                n - 1
            )),
            "{body}"
        );
        assert!(body.ends_with("Their branches stay."), "{body}");
        let (title, _) = discard_question(&state, &run.id).await.expect("discard");
        assert_eq!(title, format!("Discard all {n} worktrees?"));
        std::fs::write(repo.join("a.txt"), "dirty\n").expect("w");
        let err = keep_question(&state, &run.id, 1).await.expect_err("dirty");
        assert_eq!(
            err.message,
            format!("Commit or stash your changes in {base} first.")
        );
    }

    #[tokio::test]
    async fn f_ex_04_5_a_folder_without_git_and_bad_seats_are_refused() {
        let (tmp, state, _repo, _run) = fixture().await;
        let plain = tmp.path().join("plain");
        std::fs::create_dir_all(&plain).expect("mkdir");
        let (ws, _) = workspaces::add(
            &state.store,
            NewWorkspace {
                kind: "local".into(),
                host_id: "local".into(),
                root: plain.display().to_string(),
                name: "plain".into(),
                wsl_distro: None,
            },
            None,
        )
        .await
        .expect("ws");
        let seats = vec![
            BestOfSeat {
                engine_id: "claude".into(),
                account_id: None
            };
            2
        ];
        let err = start(&state, &ws.id, "do it", &seats, None)
            .await
            .expect_err("no git");
        assert_eq!(err.message, NOT_A_REPO);
        // A WSL folder has no worktrees yet (D-1605): refused before
        // anything starts, even when the same path is a repository here.
        // An SSH folder goes to its host instead (E2E-M14-11), so it isn't
        // refused for being remote.
        for kind in ["ssh", "wsl"] {
            let dir = tmp.path().join(kind);
            std::fs::create_dir_all(&dir).expect("mkdir");
            git(&dir, &["init", "-q", "-b", "main"]);
            let (remote, _) = workspaces::add(
                &state.store,
                NewWorkspace {
                    kind: kind.into(),
                    host_id: "local".into(),
                    root: dir.display().to_string(),
                    name: kind.into(),
                    wsl_distro: None,
                },
                None,
            )
            .await
            .expect("ws");
            let err = start(&state, &remote.id, "do it", &seats, None)
                .await
                .expect_err("remote");
            if kind == "wsl" {
                assert_eq!(
                    (err.code, err.message.as_str()),
                    (ErrorCode::Unavailable, WSL_UNAVAILABLE)
                );
            } else {
                assert_ne!(err.message, WSL_UNAVAILABLE);
                assert!(!err.message.contains("this computer"), "{}", err.message);
            }
        }
        let err = start(&state, &ws.id, "do it", &seats[..1], None)
            .await
            .expect_err("one seat");
        assert_eq!(err.message, SEATS);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn f_ex_04_5_over_the_live_pane_limit_nothing_is_launched() {
        let (_t, state, repo, run) = fixture().await;
        let max = state
            .entitlements
            .limits()
            .panes
            .expect("Free has a pane limit");
        // Fill the plan so three more don't fit.
        let busy = max.saturating_sub(2);
        for n in 0..busy {
            state
                .terminals
                .pty
                .spawn(
                    tandem_pty::SpawnSpec {
                        pane_id: format!("pn_busy{n}"),
                        program: "/bin/sh".into(),
                        args: vec!["-c".into(), "sleep 30".into()],
                        cwd: repo.clone(),
                        env: std::env::vars().collect(),
                        cols: 80,
                        rows: 24,
                        scrollback_lines: 100,
                        mark_token: None,
                    },
                    None,
                )
                .expect("spawn");
        }
        let before = git(&repo, &["branch", "--list", "tandem/*"]);
        let seats = vec![
            BestOfSeat {
                engine_id: "claude".into(),
                account_id: None
            };
            3
        ];
        let ws = WorkspaceId::from_db(run.workspace_id.clone());
        let err = start(&state, &ws, "do it", &seats, None)
            .await
            .expect_err("over the limit");
        for n in 0..busy {
            let _ = state.terminals.pty.kill(&format!("pn_busy{n}"));
        }
        assert_eq!(err.code, ErrorCode::LimitReached);
        assert_eq!(err.message, format!("Free includes {max} live panes"));
        assert_eq!(
            git(&repo, &["branch", "--list", "tandem/*"]),
            before,
            "no worktree was made"
        );
        assert_eq!(
            runs::list(&state.store, Some(run.workspace_id.clone()))
                .await
                .expect("runs")
                .len(),
            1
        );
    }

    #[test]
    fn f_rm_01_f_ex_04_3_a_hosts_check_runs_in_its_login_shell_with_output_joined() {
        let argv = host_check_argv("pnpm test 'a b'");
        assert_eq!(argv[..3], ["/bin/sh", "-c", HOST_CHECK_SHELL]);
        // The command reaches the login shell as one argument, unquoted
        // here: `run` quotes each argument for the host.
        assert_eq!(argv[4], "exec 2>&1\npnpm test 'a b'");
        assert_eq!(argv.len(), 5);
    }

    #[test]
    fn f_ex_04_3_tail_keeps_the_last_lines() {
        let text: String = (1..=300).map(|i| format!("line {i}\n")).collect();
        let t = tail(&text, 200);
        assert_eq!(t.lines().count(), 200);
        assert!(t.starts_with("line 101"));
        assert!(t.ends_with("line 300"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn f_ex_04_3_a_check_runs_in_the_lane_with_its_exit_code_and_output() {
        let tmp = tempfile::tempdir().expect("tmp");
        std::fs::write(tmp.path().join("marker"), "here").expect("w");
        let env: std::collections::BTreeMap<String, String> = std::env::vars().collect();
        let (exit, out) = run_check(
            "cat marker; echo oops >&2; exit 3",
            tmp.path(),
            &env,
            Duration::from_secs(10),
            None,
        )
        .await;
        assert_eq!(exit, Some(3));
        assert!(out.contains("here") && out.contains("oops"), "{out}");
        let (exit, out) = run_check(
            "sleep 5",
            tmp.path(),
            &env,
            Duration::from_millis(300),
            None,
        )
        .await;
        assert_eq!(exit, None);
        assert!(out.contains("Stopped"));
        // A finished check leaves no process group for Stop to signal.
        let slot: GroupSlot = Arc::default();
        run_check(
            "true",
            tmp.path(),
            &env,
            Duration::from_secs(10),
            Some(slot.clone()),
        )
        .await;
        assert_eq!(slot.pid(), None);
    }

    #[test]
    fn f_wt_07_output_keeps_the_last_64_kib() {
        let mut buf = Vec::new();
        assert!(!push_capped(&mut buf, b"abc", 4));
        assert!(push_capped(&mut buf, b"defg", 4));
        assert_eq!(buf, b"defg");
        let big = "x".repeat(OUTPUT_CAP * 2);
        let mut buf = Vec::new();
        assert!(push_capped(&mut buf, big.as_bytes(), OUTPUT_CAP));
        assert_eq!(buf.len(), OUTPUT_CAP);
    }

    #[test]
    fn f_wt_07_6_the_tree_kill_on_each_platform() {
        assert_eq!(UnixKiller.argv(42), ["kill", "-KILL", "--", "-42"]);
        assert_eq!(
            WindowsKiller.argv(42),
            ["taskkill", "/T", "/F", "/PID", "42"]
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn f_wt_07_6_a_cancelled_slot_never_starts_or_kills_at_once() {
        let tmp = tempfile::tempdir().expect("tmp");
        let env: std::collections::BTreeMap<String, String> = std::env::vars().collect();
        let marker = tmp.path().join("ran");
        // Cancelled before: the check never starts.
        let slot: GroupSlot = Arc::default();
        slot.cancel();
        let (exit, _) = run_check(
            "touch ran",
            tmp.path(),
            &env,
            Duration::from_secs(10),
            Some(slot),
        )
        .await;
        assert_eq!(exit, None);
        assert!(!marker.exists(), "a stopped check never starts");
        // Cancelled while running: killed with its children, output kept.
        let slot: GroupSlot = Arc::default();
        let (dir, env2, task_slot) = (tmp.path().to_path_buf(), env.clone(), slot.clone());
        let task = tokio::spawn(async move {
            run_check(
                "echo started; sleep 30 & wait",
                &dir,
                &env2,
                Duration::from_secs(60),
                Some(task_slot),
            )
            .await
        });
        for _ in 0..200 {
            if slot.pid().is_some() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
        let t = std::time::Instant::now();
        slot.cancel();
        let (exit, out) = task.await.expect("join");
        assert!(
            t.elapsed() < Duration::from_secs(5),
            "killed, not waited out"
        );
        assert_eq!(exit, None);
        assert!(out.contains("started"), "{out}");
    }
}
