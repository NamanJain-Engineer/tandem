//! Worktree setup (F-WT-01): the repository's `.vibezeo/worktree.json`, the
//! trust kept on this computer, the copy after every new worktree, the setup
//! pane and the teardown before removal.
//!
//! Commands from the file run only when their digest is trusted for the
//! repository in `$DATA/worktree-trust.json`. The copy needs no trust: it
//! moves the repository's own files into its own worktree.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use tandem_core::ids::{TabId, WorkspaceId};
use tandem_core::{DataDir, ErrorCode, IpcError};
use tandem_git::setup::{self, SETUP_FILE};
use tandem_git::{CopyReport, Repo, SetupError, WorktreeSetup};
use tandem_pty::env::LoginEnv;
use tandem_store::repo::panes::{self, PaneRecord};
use tandem_store::repo::workspaces::{self, Workspace};
use tandem_store::Store;

pub const TRUST_FILE: &str = "worktree-trust.json";
pub const TEARDOWN_LIMIT: Duration = Duration::from_secs(120);
/// The setup pane types its command once the shell has settled.
pub const SETUP_READY_TIMEOUT: Duration = Duration::from_secs(30);
pub const NOT_TRUSTED: &str = "Review this repository's worktree setup first";
pub const CHANGED: &str = "The worktree setup changed. Review it again.";

/// `worktree_setup_get`: the repository's setup file and whether its commands
/// are trusted here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SetupState {
    /// The file's absolute path.
    pub file: String,
    pub exists: bool,
    /// The parsed file, when it exists and is valid.
    pub config: Option<WorktreeSetup>,
    /// Why the file couldn't be read.
    pub error: Option<String>,
    /// The commands' digest; `None` without commands.
    pub digest: Option<String>,
    pub trusted: bool,
}

/// `lane_setup`: what happened when the worktree was made, and what's left.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct LaneSetup {
    pub state: SetupState,
    /// The copy into this worktree; `None` when there was nothing to copy.
    pub copy: Option<CopyReport>,
    /// Why the copy failed.
    pub copy_error: Option<String>,
    /// F-WT-06: the index and port block the worktree got.
    pub runtime: Option<crate::services::worktree_runtime::RuntimeInfo>,
}

/// What discarding a worktree did with its teardown.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case", tag = "kind")]
#[ts(export)]
pub enum TeardownOutcome {
    Ran {
        code: Option<i32>,
    },
    TimedOut,
    /// The command isn't trusted, so it didn't run.
    Untrusted,
    Failed {
        message: String,
    },
}

/// `lane_remove`'s result.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RemoveReport {
    pub teardown: Option<TeardownOutcome>,
}

fn setup_err(e: SetupError) -> IpcError {
    match e {
        SetupError::Io(e) => IpcError::new(ErrorCode::Io, e.to_string()).with_stage("setup"),
        other => IpcError::new(ErrorCode::Invalid, other.to_string()).with_stage("setup"),
    }
}

/// The main checkout's root for a workspace or worktree folder.
pub fn main_root(root: &str) -> Result<PathBuf, IpcError> {
    let repo = Repo::discover(Path::new(root))?.ok_or_else(|| {
        IpcError::new(ErrorCode::Invalid, "Worktrees need a git repository").with_stage("git")
    })?;
    Ok(repo.main_root())
}

// ---- trust ---------------------------------------------------------------

static TRUST_LOCK: Mutex<()> = Mutex::new(());

fn trust_key(main: &Path) -> String {
    std::fs::canonicalize(main)
        .unwrap_or_else(|_| main.to_path_buf())
        .to_string_lossy()
        .into_owned()
}

fn read_trust(data: &DataDir) -> BTreeMap<String, String> {
    std::fs::read(data.root().join(TRUST_FILE))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

pub fn is_trusted(data: &DataDir, main: &Path, digest: &str) -> bool {
    read_trust(data).get(&trust_key(main)).map(String::as_str) == Some(digest)
}

/// Records `digest` as trusted for the repository at `main`.
pub fn trust(data: &DataDir, main: &Path, digest: &str) -> Result<(), IpcError> {
    let _guard = TRUST_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let mut all = read_trust(data);
    all.insert(trust_key(main), digest.to_string());
    let bytes =
        serde_json::to_vec_pretty(&all).map_err(|e| IpcError::new(ErrorCode::Io, e.to_string()))?;
    tandem_core::atomic::atomic_write(&data.root().join(TRUST_FILE), &bytes, Some(0o600))
        .map_err(|e| IpcError::new(ErrorCode::Io, e.to_string()).with_stage("setup"))
}

// ---- state ---------------------------------------------------------------

pub fn state_at(data: &DataDir, main: &Path) -> SetupState {
    let file = main.join(SETUP_FILE).to_string_lossy().into_owned();
    match setup::read(main) {
        Ok(None) => SetupState {
            file,
            exists: false,
            config: None,
            error: None,
            digest: None,
            trusted: false,
        },
        Ok(Some(config)) => {
            let digest = config.digest();
            let trusted = digest.as_deref().is_some_and(|d| is_trusted(data, main, d));
            SetupState {
                file,
                exists: true,
                config: Some(config),
                error: None,
                digest,
                trusted,
            }
        }
        Err(e) => SetupState {
            file,
            exists: true,
            config: None,
            error: Some(e.to_string()),
            digest: None,
            trusted: false,
        },
    }
}

/// `worktree_setup_get`.
pub async fn get(store: &Store, data: &DataDir, ws: &WorkspaceId) -> Result<SetupState, IpcError> {
    let ws = workspaces::get(store, ws).await?;
    local_only(&ws)?;
    Ok(state_at(data, &main_root(&ws.root)?))
}

/// `worktree_setup_save`: writes the file and trusts its commands, since the
/// person at this computer just wrote them.
pub async fn save(
    store: &Store,
    data: &DataDir,
    ws: &WorkspaceId,
    config: WorktreeSetup,
) -> Result<SetupState, IpcError> {
    let ws = workspaces::get(store, ws).await?;
    local_only(&ws)?;
    let main = main_root(&ws.root)?;
    let config = config.normalized().map_err(setup_err)?;
    setup::write(&main, &config).map_err(setup_err)?;
    if let Some(digest) = config.digest() {
        trust(data, &main, &digest)?;
    }
    tracing::info!(code = "worktree_setup.saved", "worktree setup saved");
    Ok(state_at(data, &main))
}

/// `worktree_setup_trust`: trusts the commands the person reviewed. A file
/// that changed since (a different digest) is refused.
pub async fn trust_reviewed(
    store: &Store,
    data: &DataDir,
    ws: &WorkspaceId,
    digest: &str,
) -> Result<SetupState, IpcError> {
    let ws = workspaces::get(store, ws).await?;
    local_only(&ws)?;
    let main = main_root(&ws.root)?;
    let state = state_at(data, &main);
    if state.digest.as_deref() != Some(digest) {
        return Err(IpcError::new(ErrorCode::Conflict, CHANGED).with_stage("setup"));
    }
    trust(data, &main, digest)?;
    tracing::info!(code = "worktree_setup.trusted", "worktree setup trusted");
    Ok(state_at(data, &main))
}

fn local_only(ws: &Workspace) -> Result<(), IpcError> {
    if ws.kind != "local" {
        return Err(IpcError::new(
            ErrorCode::Unavailable,
            "Worktrees aren't available in remote folders yet",
        ));
    }
    Ok(())
}

// ---- copy ----------------------------------------------------------------

/// The copy made when each worktree was created, until `lane_setup` reads it.
static COPIES: LazyLock<Mutex<BTreeMap<String, Result<CopyReport, String>>>> =
    LazyLock::new(|| Mutex::new(BTreeMap::new()));

/// Runs the repository's copy into a new worktree. Never fails the worktree:
/// errors are kept for `lane_setup` and logged by kind only.
pub async fn copy_for_new_lane(lane: &Workspace) {
    let lane_root = PathBuf::from(&lane.root);
    let result = tokio::task::spawn_blocking(move || -> Result<Option<CopyReport>, String> {
        let repo = Repo::discover(&lane_root)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "Not a git repository".to_string())?;
        let main = repo.main_root();
        let Some(config) = setup::read(&main).map_err(|e| e.to_string())? else {
            return Ok(None);
        };
        if config.copy.is_empty() {
            return Ok(None);
        }
        setup::copy_into(&main, &lane_root, &config.copy)
            .map(Some)
            .map_err(|e| e.to_string())
    })
    .await
    .unwrap_or_else(|e| Err(e.to_string()));
    match &result {
        Ok(Some(report)) => tracing::info!(
            code = "worktree_setup.copied",
            files = report.copied.len(),
            skipped = report.skipped.len(),
            partial = report.partial,
            "worktree setup copy"
        ),
        Ok(None) => {}
        Err(_) => tracing::warn!(
            code = "worktree_setup.copy_failed",
            "worktree setup copy failed"
        ),
    }
    let entry = match result {
        Ok(Some(report)) => Ok(report),
        Ok(None) => return,
        Err(e) => Err(e),
    };
    let mut copies = COPIES.lock().unwrap_or_else(|p| p.into_inner());
    // Bounded: a worktree nobody asks about doesn't keep its report forever.
    if copies.len() > 64 {
        copies.clear();
    }
    copies.insert(lane.id.as_str().to_string(), entry);
}

/// `lane_setup`: the copy made at creation, and the setup's state. `None`
/// for a worktree on an SSH host, where setup isn't available (D-2209).
pub async fn lane_setup(
    store: &Store,
    data: &DataDir,
    lane: &WorkspaceId,
) -> Result<Option<LaneSetup>, IpcError> {
    let row = workspaces::get(store, lane).await?;
    if row.lane_branch.is_none() {
        return Err(IpcError::new(
            ErrorCode::Invalid,
            "That workspace isn't a worktree",
        ));
    }
    if row.kind != "local" {
        return Ok(None);
    }
    let copied = COPIES
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .remove(lane.as_str());
    let (copy, copy_error) = match copied {
        Some(Ok(report)) => (Some(report), None),
        Some(Err(e)) => (None, Some(e)),
        None => (None, None),
    };
    Ok(Some(LaneSetup {
        state: state_at(data, &main_root(&row.root)?),
        copy,
        copy_error,
        runtime: crate::services::worktree_runtime::take_note(lane.as_str()),
    }))
}

// ---- setup pane ----------------------------------------------------------

/// `lane_setup_run`: a shell pane in the worktree, in `tab`, for the trusted
/// setup command. The caller types the returned command once it's ready.
pub async fn setup_pane(
    store: &Store,
    data: &DataDir,
    lane: &WorkspaceId,
    tab: &TabId,
) -> Result<(PaneRecord, String), IpcError> {
    let row = workspaces::get(store, lane).await?;
    if row.lane_branch.is_none() {
        return Err(IpcError::new(
            ErrorCode::Invalid,
            "That workspace isn't a worktree",
        ));
    }
    if !Path::new(&row.root).is_dir() {
        return Err(IpcError::new(
            ErrorCode::NotFound,
            "The worktree folder is missing",
        ));
    }
    let main = main_root(&row.root)?;
    let state = state_at(data, &main);
    let Some(command) = state.config.and_then(|c| c.setup) else {
        return Err(IpcError::new(
            ErrorCode::NotFound,
            "This repository's worktree setup has no setup command",
        ));
    };
    if !state.trusted {
        return Err(IpcError::new(ErrorCode::PermissionDenied, NOT_TRUSTED).with_stage("setup"));
    }
    let pane = panes::create_with(store, tab, "terminal", Some(row.root.clone()), None).await?;
    tracing::info!(code = "worktree_setup.run", "worktree setup started");
    Ok((pane, command))
}

// ---- teardown ------------------------------------------------------------

/// The shell that runs a teardown: `sh -c` (`cmd /C` on Windows) with the
/// login environment.
fn teardown_command(
    login: &LoginEnv,
    extra: &BTreeMap<String, String>,
    command: &str,
    cwd: &Path,
) -> tokio::process::Command {
    let mut cmd = if cfg!(windows) {
        let mut c = tokio::process::Command::new("cmd");
        c.arg("/C").arg(command);
        c
    } else {
        let mut c = tokio::process::Command::new("/bin/sh");
        c.arg("-c").arg(command);
        c
    };
    cmd.current_dir(cwd)
        .env_clear()
        .envs(&login.vars)
        .envs(extra)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true);
    cmd
}

/// Runs a trusted teardown in the worktree's folder, with the worktree's
/// runtime variables (`extra`, F-WT-06). `None` when there's no teardown
/// (or no folder to run it in).
pub async fn teardown(
    data: &DataDir,
    login: &LoginEnv,
    extra: &BTreeMap<String, String>,
    lane_root: &str,
    limit: Duration,
) -> Option<TeardownOutcome> {
    let cwd = Path::new(lane_root);
    if !cwd.is_dir() {
        return None;
    }
    let main = main_root(lane_root).ok()?;
    let state = state_at(data, &main);
    let command = state.config.and_then(|c| c.teardown)?;
    if !state.trusted {
        tracing::info!(
            code = "worktree_setup.teardown_untrusted",
            "teardown skipped"
        );
        return Some(TeardownOutcome::Untrusted);
    }
    let outcome = match teardown_command(login, extra, &command, cwd).spawn() {
        Err(e) => TeardownOutcome::Failed {
            message: e.to_string(),
        },
        Ok(mut child) => match tokio::time::timeout(limit, child.wait()).await {
            Ok(Ok(status)) => TeardownOutcome::Ran {
                code: status.code(),
            },
            Ok(Err(e)) => TeardownOutcome::Failed {
                message: e.to_string(),
            },
            Err(_) => {
                let _ = child.kill().await;
                TeardownOutcome::TimedOut
            }
        },
    };
    tracing::info!(code = "worktree_setup.teardown", outcome = ?outcome, "teardown finished");
    Some(outcome)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn git(dir: &Path, args: &[&str]) {
        let ok = Command::new("git")
            .args([
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@t",
                "-c",
                "commit.gpgsign=false",
            ])
            .args(args)
            .current_dir(dir)
            .output()
            .expect("git runs")
            .status
            .success();
        assert!(ok, "git {args:?}");
    }

    fn repo_with_worktree(setup_json: &str) -> (tempfile::TempDir, PathBuf, PathBuf, DataDir) {
        let tmp = tempfile::tempdir().expect("tmp");
        let main = tmp.path().join("app");
        std::fs::create_dir_all(main.join(".vibezeo")).expect("mkdir");
        git(&main, &["init", "-q", "-b", "main"]);
        std::fs::write(main.join("README.md"), "x\n").expect("write");
        git(&main, &["add", "."]);
        git(&main, &["commit", "-q", "-m", "init"]);
        std::fs::write(main.join(SETUP_FILE), setup_json).expect("write");
        let wt = tmp.path().join("wt");
        git(
            &main,
            &[
                "worktree",
                "add",
                "-q",
                wt.to_str().expect("utf8"),
                "-b",
                "tandem/wt",
            ],
        );
        let data = DataDir::at(tmp.path().join("data")).expect("data dir");
        (tmp, main, wt, data)
    }

    fn login() -> LoginEnv {
        LoginEnv {
            vars: std::env::vars().collect(),
            shell: PathBuf::from("/bin/sh"),
            degraded: false,
        }
    }

    #[test]
    fn f_wt_01_3_untrusted_until_the_reviewed_digest_is_trusted() {
        let (_tmp, main, _wt, data) = repo_with_worktree(r#"{"setup":"echo hi > marker"}"#);
        let state = state_at(&data, &main);
        assert!(state.exists && !state.trusted);
        let digest = state.digest.clone().expect("digest");
        trust(&data, &main, &digest).expect("trust");
        assert!(state_at(&data, &main).trusted);
        // Editing the command outside the app needs review again.
        std::fs::write(
            main.join(SETUP_FILE),
            r#"{"setup":"echo changed > marker"}"#,
        )
        .expect("edit");
        let changed = state_at(&data, &main);
        assert!(!changed.trusted);
        assert_ne!(changed.digest.as_deref(), Some(digest.as_str()));
        // The trust file lives in the data folder, never in the repository.
        assert!(data.root().join(TRUST_FILE).is_file());
        assert!(!main.join(".vibezeo").join(TRUST_FILE).exists());
    }

    #[test]
    fn f_wt_01_6_malformed_file_is_reported_not_fatal() {
        let (_tmp, main, _wt, data) = repo_with_worktree("{ not json");
        let state = state_at(&data, &main);
        assert!(state.exists);
        assert!(state.config.is_none());
        assert!(state
            .error
            .as_deref()
            .is_some_and(|e| e.contains("isn't valid")));
        assert!(!state.trusted);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn f_wt_01_5_trusted_teardown_runs_in_the_worktree_and_reports_its_code() {
        let (_tmp, main, wt, data) =
            repo_with_worktree(r#"{"teardown":"pwd > ../teardown-ran; exit 3"}"#);
        let wt_s = wt.to_str().expect("utf8");
        // Untrusted: skipped.
        let out = teardown(&data, &login(), &BTreeMap::new(), wt_s, TEARDOWN_LIMIT).await;
        assert_eq!(out, Some(TeardownOutcome::Untrusted));
        assert!(!wt.parent().expect("parent").join("teardown-ran").exists());
        let digest = state_at(&data, &main).digest.expect("digest");
        trust(&data, &main, &digest).expect("trust");
        let out = teardown(&data, &login(), &BTreeMap::new(), wt_s, TEARDOWN_LIMIT).await;
        assert_eq!(out, Some(TeardownOutcome::Ran { code: Some(3) }));
        let ran_in = std::fs::read_to_string(wt.parent().expect("parent").join("teardown-ran"))
            .expect("marker");
        assert_eq!(
            std::fs::canonicalize(ran_in.trim()).expect("canon"),
            std::fs::canonicalize(&wt).expect("canon")
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn f_wt_01_5_a_hanging_teardown_times_out() {
        let (_tmp, main, wt, data) = repo_with_worktree(r#"{"teardown":"sleep 30"}"#);
        let digest = state_at(&data, &main).digest.expect("digest");
        trust(&data, &main, &digest).expect("trust");
        let out = teardown(
            &data,
            &login(),
            &BTreeMap::new(),
            wt.to_str().expect("utf8"),
            Duration::from_millis(200),
        )
        .await;
        assert_eq!(out, Some(TeardownOutcome::TimedOut));
    }

    #[tokio::test]
    async fn f_wt_01_5_no_teardown_means_nothing_to_report() {
        let (_tmp, _main, wt, data) = repo_with_worktree(r#"{"copy":[".env"]}"#);
        let out = teardown(
            &data,
            &login(),
            &BTreeMap::new(),
            wt.to_str().expect("utf8"),
            TEARDOWN_LIMIT,
        )
        .await;
        assert_eq!(out, None);
    }

    #[tokio::test]
    async fn f_wt_01_a_worktree_on_an_ssh_host_has_no_setup_and_no_error() {
        let tmp = tempfile::tempdir().expect("tmp");
        let data = DataDir::at(tmp.path().join("data")).expect("data dir");
        let store = Store::open(&data).expect("store");
        let host = tandem_store::repo::hosts::add_ssh(
            &store,
            tandem_store::repo::hosts::NewSshHost {
                label: "box".into(),
                target: "me@box".into(),
                port: None,
                identity_file: None,
            },
        )
        .await
        .expect("host");
        let (parent, _) = workspaces::add(
            &store,
            workspaces::NewWorkspace {
                kind: "ssh".into(),
                host_id: host.id.as_str().to_string(),
                root: "/home/me/app".into(),
                name: "app".into(),
                wsl_distro: None,
            },
            None,
        )
        .await
        .expect("parent");
        let lane = workspaces::add_lane(
            &store,
            workspaces::NewLane {
                parent_id: parent.id.clone(),
                root: "/home/me/app/.tandem/worktrees/fix".into(),
                name: "fix".into(),
                branch: "tandem/fix".into(),
                base: "abc".into(),
            },
        )
        .await
        .expect("lane");
        assert_eq!(lane.kind, "ssh");
        let out = lane_setup(&store, &data, &lane.id).await.expect("no error");
        assert_eq!(out, None);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn f_wt_06_5_the_teardown_gets_the_worktrees_variables() {
        let (_tmp, main, wt, data) =
            repo_with_worktree(r#"{"teardown":"echo $COMPOSE_PROJECT_NAME > ../compose-name"}"#);
        let digest = state_at(&data, &main).digest.expect("digest");
        trust(&data, &main, &digest).expect("trust");
        let extra: BTreeMap<String, String> =
            [("COMPOSE_PROJECT_NAME".to_string(), "app-wt".to_string())]
                .into_iter()
                .collect();
        let out = teardown(
            &data,
            &login(),
            &extra,
            wt.to_str().expect("utf8"),
            TEARDOWN_LIMIT,
        )
        .await;
        assert_eq!(out, Some(TeardownOutcome::Ran { code: Some(0) }));
        let written = std::fs::read_to_string(wt.parent().expect("parent").join("compose-name"))
            .expect("written");
        assert_eq!(written.trim(), "app-wt");
    }
}
