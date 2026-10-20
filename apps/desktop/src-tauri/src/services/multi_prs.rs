//! "Create pull requests" for a multi-repo worktree (F-CD-22): in each of
//! its repositories with new commits, the user's own `git push -u origin
//! tandem/<slug>` and `gh pr create --fill`, then each pull request's body
//! gains links to the others. Runs in the background after the native
//! confirm; UI-158 polls it. GitLab remotes are skipped in this slice.

use std::collections::HashMap;
use std::ffi::OsStr;
use std::path::Path;
use std::sync::{LazyLock, Mutex};

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use tandem_core::ids::WorkspaceId;
use tandem_core::{ErrorCode, IpcError};
use tandem_git::{Git, Mode};
use tandem_store::repo::lane_repos::{self, LaneRepo};
use tandem_store::repo::workspaces::{self, Workspace};
use tandem_store::Store;

use crate::services::lanes::{forge_for, Forge};
use crate::services::pr_checks::{first_line, Gh};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum PrStepState {
    Waiting,
    Pushing,
    Creating,
    Linking,
    Done,
    Skipped,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PrStep {
    pub repo: String,
    pub state: PrStepState,
    pub url: Option<String>,
    /// Why it was skipped or failed.
    pub reason: Option<String>,
}

/// `lane_prs_get`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PrsRun {
    pub lane_id: String,
    pub branch: String,
    /// False once every repository is done, skipped or failed.
    pub running: bool,
    pub steps: Vec<PrStep>,
}

pub const NO_COMMITS: &str = "no new commits";
pub const GITLAB: &str = "GitLab isn't supported yet";

static RUNS: LazyLock<Mutex<HashMap<String, PrsRun>>> = LazyLock::new(Mutex::default);

fn lock() -> std::sync::MutexGuard<'static, HashMap<String, PrsRun>> {
    RUNS.lock().unwrap_or_else(|e| e.into_inner())
}

fn set(lane: &str, i: usize, f: impl FnOnce(&mut PrStep)) {
    if let Some(run) = lock().get_mut(lane) {
        if let Some(step) = run.steps.get_mut(i) {
            f(step);
        }
    }
}

/// A multi-repo worktree and its repositories, or why it isn't one.
pub async fn lane_of(
    store: &Store,
    id: &WorkspaceId,
) -> Result<(Workspace, String, Vec<LaneRepo>), IpcError> {
    let lane = workspaces::get(store, id).await?;
    let repos = lane_repos::list(store, id).await?;
    let branch = lane.lane_branch.clone().unwrap_or_default();
    if repos.is_empty() || branch.is_empty() {
        return Err(IpcError::new(
            ErrorCode::Invalid,
            "Create pull requests is for multi-repo worktrees",
        ));
    }
    Ok((lane, branch, repos))
}

/// `lane_prs_get`.
pub fn get(lane_id: &str) -> Option<PrsRun> {
    lock().get(lane_id).cloned()
}

/// `lane_create_prs`, after the confirm: records the run and starts it.
pub async fn start(
    store: &Store,
    git: Git,
    gh: Box<dyn Gh>,
    id: &WorkspaceId,
) -> Result<PrsRun, IpcError> {
    let (lane, branch, repos) = lane_of(store, id).await?;
    let key = lane.id.as_str().to_string();
    {
        let mut runs = lock();
        if runs.get(&key).is_some_and(|r| r.running) {
            return Err(IpcError::new(
                ErrorCode::Conflict,
                "Already opening pull requests for this worktree",
            ));
        }
        runs.insert(
            key.clone(),
            PrsRun {
                lane_id: key.clone(),
                branch: branch.clone(),
                running: true,
                steps: repos
                    .iter()
                    .map(|r| PrStep {
                        repo: r.name.clone(),
                        state: PrStepState::Waiting,
                        url: None,
                        reason: None,
                    })
                    .collect(),
            },
        );
    }
    let run = get(&key).ok_or_else(|| IpcError::new(ErrorCode::Io, "The run went missing"))?;
    tokio::spawn(async move {
        drive(&git, gh.as_ref(), &key, &branch, &repos).await;
    });
    Ok(run)
}

/// How many commits the worktree's branch has past its base.
async fn ahead(git: &Git, root: &Path, base: &str) -> u32 {
    let range = format!("{base}..HEAD");
    git.ok(
        Mode::Read,
        root,
        &[
            OsStr::new("rev-list"),
            OsStr::new("--count"),
            OsStr::new(&range),
        ],
    )
    .await
    .map(|o| o.text().trim().parse().unwrap_or(0))
    .unwrap_or(0)
}

fn arg(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| (*s).to_string()).collect()
}

/// Each repository in turn; one failing doesn't stop the others.
pub async fn drive(git: &Git, gh: &dyn Gh, key: &str, branch: &str, repos: &[LaneRepo]) {
    let mut opened: Vec<(usize, String)> = Vec::new();
    for (i, r) in repos.iter().enumerate() {
        let root = Path::new(&r.root);
        let url = tandem_git::lanes::origin_url(root);
        if forge_for(url.as_deref()) == Forge::Gitlab {
            set(key, i, |s| {
                s.state = PrStepState::Skipped;
                s.reason = Some(GITLAB.into());
            });
            continue;
        }
        if ahead(git, root, &r.base).await == 0 {
            set(key, i, |s| {
                s.state = PrStepState::Skipped;
                s.reason = Some(NO_COMMITS.into());
            });
            continue;
        }
        set(key, i, |s| s.state = PrStepState::Pushing);
        let pushed = git
            .run(
                Mode::Write,
                root,
                &[
                    OsStr::new("push"),
                    OsStr::new("-u"),
                    OsStr::new("origin"),
                    OsStr::new(branch),
                ],
            )
            .await;
        match pushed {
            Ok(out) if out.success() => {}
            Ok(out) => {
                set(key, i, |s| {
                    s.state = PrStepState::Failed;
                    s.reason = Some(first_line(&out.stderr));
                });
                continue;
            }
            Err(e) => {
                set(key, i, |s| {
                    s.state = PrStepState::Failed;
                    s.reason = Some(e.to_string());
                });
                continue;
            }
        }
        set(key, i, |s| s.state = PrStepState::Creating);
        let out = gh
            .run(root, &arg(&["pr", "create", "--fill", "--head", branch]))
            .await;
        let link = out
            .stdout
            .lines()
            .rev()
            .find(|l| l.trim().starts_with("https://"))
            .map(|l| l.trim().to_string());
        match (out.code, link) {
            (Some(0), Some(link)) => {
                set(key, i, |s| s.url = Some(link.clone()));
                opened.push((i, link));
            }
            _ => {
                set(key, i, |s| {
                    s.state = PrStepState::Failed;
                    s.reason = Some(first_line(&out.stderr));
                });
            }
        }
    }
    // Each opened pull request links the others.
    for (i, url) in &opened {
        if opened.len() < 2 {
            set(key, *i, |s| s.state = PrStepState::Done);
            continue;
        }
        set(key, *i, |s| s.state = PrStepState::Linking);
        let root = Path::new(&repos[*i].root);
        let body = gh
            .run(
                root,
                &arg(&["pr", "view", url, "--json", "body", "--jq", ".body"]),
            )
            .await;
        let others: Vec<String> = opened
            .iter()
            .filter(|(j, _)| j != i)
            .map(|(j, u)| format!("- {}: {u}", repos[*j].name))
            .collect();
        let text = format!(
            "{}\n\nRelated pull requests:\n{}\n",
            body.stdout.trim_end(),
            others.join("\n")
        );
        let file =
            std::env::temp_dir().join(format!("vibezeo-pr-body-{}-{i}.md", std::process::id()));
        let linked = match std::fs::write(&file, text) {
            Ok(()) => {
                let path = file.to_string_lossy().into_owned();
                let out = gh
                    .run(root, &arg(&["pr", "edit", url, "--body-file", &path]))
                    .await;
                let _ = std::fs::remove_file(&file);
                (out.code == Some(0))
                    .then_some(())
                    .ok_or(first_line(&out.stderr))
            }
            Err(e) => Err(e.to_string()),
        };
        set(key, *i, |s| match linked {
            Ok(()) => s.state = PrStepState::Done,
            Err(why) => {
                // The pull request is open; only its links failed.
                s.state = PrStepState::Done;
                s.reason = Some(format!("Couldn't link the others: {why}"));
            }
        });
    }
    if let Some(run) = lock().get_mut(key) {
        run.running = false;
    }
    tracing::info!(
        code = "lane.prs_done",
        opened = opened.len(),
        "pull requests opened"
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::pr_checks::GhOut;
    use async_trait::async_trait;

    /// Answers `pr create` with a URL per repository folder, records calls.
    #[derive(Default)]
    struct FakeGh {
        calls: Mutex<Vec<(String, Vec<String>)>>,
        edits: Mutex<HashMap<String, String>>,
    }

    #[async_trait]
    impl Gh for FakeGh {
        async fn run(&self, cwd: &Path, args: &[String]) -> GhOut {
            let repo = cwd
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            self.calls
                .lock()
                .expect("calls")
                .push((repo.clone(), args.to_vec()));
            let ok = |stdout: String| GhOut {
                code: Some(0),
                stdout,
                stderr: String::new(),
            };
            match args
                .first()
                .map(String::as_str)
                .zip(args.get(1).map(String::as_str))
            {
                Some(("pr", "create")) => ok(format!("https://github.com/o/{repo}/pull/1\n")),
                Some(("pr", "view")) => ok("Fill body\n".into()),
                Some(("pr", "edit")) => {
                    let body = std::fs::read_to_string(&args[4]).unwrap_or_default();
                    self.edits.lock().expect("edits").insert(repo, body);
                    ok(String::new())
                }
                _ => GhOut::default(),
            }
        }
    }

    fn git_cmd(cwd: &Path, args: &[&str]) -> String {
        let out = std::process::Command::new("git")
            .current_dir(cwd)
            .env("GIT_AUTHOR_NAME", "T")
            .env("GIT_AUTHOR_EMAIL", "t@example.com")
            .env("GIT_COMMITTER_NAME", "T")
            .env("GIT_COMMITTER_EMAIL", "t@example.com")
            .args(args)
            .output()
            .expect("git");
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    #[tokio::test]
    async fn f_cd_22_4_each_repository_is_pushed_and_its_pull_request_links_the_others() {
        let tmp = tempfile::tempdir().expect("tmp");
        let root = std::fs::canonicalize(tmp.path()).expect("canonical");
        let mut repos = Vec::new();
        for name in ["api", "web", "docs"] {
            let bare = root.join(format!("{name}.git"));
            git_cmd(
                &root,
                &["init", "-q", "--bare", bare.to_str().expect("utf8")],
            );
            let repo = root.join("shop").join(name);
            std::fs::create_dir_all(&repo).expect("mkdir");
            git_cmd(&repo, &["init", "-q", "-b", "main"]);
            std::fs::write(repo.join("README.md"), "x\n").expect("write");
            git_cmd(&repo, &["add", "."]);
            git_cmd(&repo, &["commit", "-qm", "first"]);
            git_cmd(
                &repo,
                &["remote", "add", "origin", bare.to_str().expect("utf8")],
            );
            let base = git_cmd(&repo, &["rev-parse", "HEAD"]);
            let wt = root.join("shop/.tandem/worktrees/tidy").join(name);
            git_cmd(
                &repo,
                &[
                    "worktree",
                    "add",
                    "-q",
                    wt.to_str().expect("utf8"),
                    "-b",
                    "tandem/tidy",
                ],
            );
            // docs has no new commits: it's skipped.
            if name != "docs" {
                std::fs::write(wt.join("change.txt"), "y\n").expect("write");
                git_cmd(&wt, &["add", "."]);
                git_cmd(&wt, &["commit", "-qm", "change"]);
            }
            repos.push(LaneRepo {
                name: name.into(),
                repo_root: repo.to_string_lossy().into_owned(),
                root: wt.to_string_lossy().into_owned(),
                base,
            });
        }
        let hooks = root.join("hooks");
        std::fs::create_dir_all(&hooks).expect("hooks");
        let git = Git::new("git", hooks);
        let gh = FakeGh::default();
        let key = "ws_multi_test";
        lock().insert(
            key.into(),
            PrsRun {
                lane_id: key.into(),
                branch: "tandem/tidy".into(),
                running: true,
                steps: repos
                    .iter()
                    .map(|r| PrStep {
                        repo: r.name.clone(),
                        state: PrStepState::Waiting,
                        url: None,
                        reason: None,
                    })
                    .collect(),
            },
        );
        drive(&git, &gh, key, "tandem/tidy", &repos).await;
        let run = get(key).expect("run");
        assert!(!run.running);
        let states: Vec<(String, PrStepState, Option<String>)> = run
            .steps
            .iter()
            .map(|s| (s.repo.clone(), s.state, s.reason.clone()))
            .collect();
        assert_eq!(
            states,
            [
                ("api".to_string(), PrStepState::Done, None),
                ("web".to_string(), PrStepState::Done, None),
                (
                    "docs".to_string(),
                    PrStepState::Skipped,
                    Some(NO_COMMITS.to_string())
                ),
            ]
        );
        // Pushed to each remote.
        for name in ["api", "web"] {
            let bare = root.join(format!("{name}.git"));
            assert!(!git_cmd(&bare, &["rev-parse", "tandem/tidy"]).is_empty());
        }
        assert!(std::process::Command::new("git")
            .current_dir(root.join("docs.git"))
            .args(["rev-parse", "--verify", "-q", "tandem/tidy"])
            .status()
            .map(|s| !s.success())
            .unwrap_or(false));
        // Each body links the other.
        let edits = gh.edits.lock().expect("edits").clone();
        assert!(
            edits["api"].contains("Related pull requests:\n- web: https://github.com/o/web/pull/1")
        );
        assert!(edits["web"].contains("- api: https://github.com/o/api/pull/1"));
        assert!(edits["api"].starts_with("Fill body"));
        lock().remove(key);
    }
}
