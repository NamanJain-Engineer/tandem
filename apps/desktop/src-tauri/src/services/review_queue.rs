//! F-DB-04: one queue of the agents whose work is ready to review, across
//! engines. A terminal agent or a thread that becomes done with changes in
//! its folder joins; risk labels come from local rules, approval stamps a
//! hash of the diff and goes stale when the diff moves on (D-2421).

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use ts_rs::TS;

use tandem_core::bus::{BusEvent, EventBus, Topic};
use tandem_core::source::SourceRef;
use tandem_core::status::{AgentStatus, StatusRecord};
use tandem_core::{ErrorCode, IpcError};
use tandem_git::{FileChange, FileDiff};

pub const EVENT: &str = "review:queue";
pub const SNOOZE_MINUTES: i64 = 60;
pub const CHECK_LIMIT: Duration = Duration::from_secs(600);
const CLOSING_MAX: usize = 1_000;
const RULE_FILES: usize = 5;
const MANY_FILES: usize = 20;
pub const CHECK_MAX: usize = 2_000;
pub const API_PATHS_MAX: usize = 50;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum ReviewRiskRule {
    ManyFiles,
    Migrations,
    Ci,
    Lockfile,
    Env,
    Auth,
    PublicApi,
    DeletedTests,
}

/// One rule that fired, with up to 5 of the files it matched.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ReviewRisk {
    pub rule: ReviewRiskRule,
    pub files: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum ReviewCheckState {
    Running,
    Passed,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ReviewCheck {
    pub state: ReviewCheckState,
    pub exit: Option<i32>,
    /// The last 200 lines.
    pub output: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum ReviewState {
    Review,
    /// Approved once; the diff has moved on since.
    Changed,
    Approved,
    Snoozed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ReviewItem {
    pub key: String,
    pub source: SourceRef,
    pub title: String,
    pub engine_id: Option<String>,
    pub workspace_id: String,
    pub workspace: String,
    pub branch: Option<String>,
    /// A worktree: Send back, Second opinion and Merge apply.
    pub lane_id: Option<String>,
    pub finished_at: i64,
    pub closing: Option<String>,
    pub files: u32,
    pub additions: u32,
    pub deletions: u32,
    pub risks: Vec<ReviewRisk>,
    pub check: Option<ReviewCheck>,
    pub diff_hash: String,
    pub state: ReviewState,
    pub snoozed_until: Option<i64>,
    pub turn_checkpoint_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ReviewQueueView {
    pub items: Vec<ReviewItem>,
}

/// A repository's review settings, kept in the app's settings by its root.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RepoReview {
    pub root: String,
    pub check: Option<String>,
    pub api_paths: Vec<String>,
}

fn name_of(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

const LOCKFILES: &[&str] = &[
    "package-lock.json",
    "pnpm-lock.yaml",
    "yarn.lock",
    "bun.lockb",
    "bun.lock",
    "Cargo.lock",
    "Gemfile.lock",
    "poetry.lock",
    "uv.lock",
    "Pipfile.lock",
    "go.sum",
    "composer.lock",
    "mix.lock",
    "Podfile.lock",
];

const AUTH_WORDS: &[&str] = &[
    "auth",
    "oauth",
    "login",
    "password",
    "secret",
    "crypto",
    "permission",
];

fn is_migration(path: &str) -> bool {
    path.split('/')
        .rev()
        .skip(1)
        .any(|s| s.eq_ignore_ascii_case("migrations") || s.eq_ignore_ascii_case("migrate"))
}

fn is_ci(path: &str) -> bool {
    path.starts_with(".github/workflows/")
        || path.starts_with(".circleci/")
        || path.starts_with(".buildkite/")
        || path == ".gitlab-ci.yml"
        || path == "azure-pipelines.yml"
        || name_of(path) == "Jenkinsfile"
}

fn is_env(path: &str) -> bool {
    let n = name_of(path);
    (n == ".env" || n.starts_with(".env."))
        && ![".example", ".sample", ".template"]
            .iter()
            .any(|s| n.ends_with(s))
}

fn is_auth(path: &str) -> bool {
    path.split('/').any(|seg| {
        let s = seg.to_ascii_lowercase();
        !s.starts_with("author") && AUTH_WORDS.iter().any(|w| s.contains(w))
    })
}

fn is_test(path: &str) -> bool {
    let n = name_of(path);
    let dirs = path.split('/').rev().skip(1);
    let in_test_dir = dirs
        .into_iter()
        .any(|s| s == "test" || s == "tests" || s == "__tests__");
    in_test_dir
        || n.contains(".test.")
        || n.contains(".spec.")
        || n.contains("_test.")
        || n.starts_with("test_")
}

/// The local rules (no AI): what the change touches, never a verdict.
pub fn risks(files: &[FileDiff], api_paths: &[String]) -> Vec<ReviewRisk> {
    let prefixes: Vec<&str> = api_paths
        .iter()
        .map(|p| p.trim().trim_start_matches("./"))
        .filter(|p| !p.is_empty())
        .collect();
    let mut hits: Vec<(ReviewRiskRule, Vec<String>)> = Vec::new();
    let mut add = |rule: ReviewRiskRule, path: &str| match hits.iter_mut().find(|(r, _)| *r == rule)
    {
        Some((_, fs)) => fs.push(path.to_string()),
        None => hits.push((rule, vec![path.to_string()])),
    };
    if files.len() >= MANY_FILES {
        for f in files {
            add(ReviewRiskRule::ManyFiles, &f.path);
        }
    }
    for f in files {
        let p = f.path.as_str();
        if is_migration(p) {
            add(ReviewRiskRule::Migrations, p);
        }
        if is_ci(p) {
            add(ReviewRiskRule::Ci, p);
        }
        if LOCKFILES.contains(&name_of(p)) {
            add(ReviewRiskRule::Lockfile, p);
        }
        if is_env(p) {
            add(ReviewRiskRule::Env, p);
        }
        if is_auth(p) {
            add(ReviewRiskRule::Auth, p);
        }
        if prefixes.iter().any(|pre| p.starts_with(pre)) {
            add(ReviewRiskRule::PublicApi, p);
        }
        if f.change == FileChange::Deleted && is_test(p) {
            add(ReviewRiskRule::DeletedTests, p);
        }
    }
    hits.sort_by_key(|(r, _)| *r);
    hits.into_iter()
        .map(|(rule, mut files)| {
            files.truncate(RULE_FILES);
            ReviewRisk { rule, files }
        })
        .collect()
}

/// SHA-256 over every file's path, change and patch.
pub fn diff_hash(files: &[FileDiff]) -> String {
    let mut h = Sha256::new();
    for f in files {
        h.update(f.path.as_bytes());
        h.update([0]);
        h.update(format!("{:?}", f.change).as_bytes());
        h.update([0]);
        h.update(f.patch.as_bytes());
        h.update([0]);
    }
    hex::encode(h.finalize())
}

/// The check command and API paths, as the user set them.
pub fn validate_repo(check: Option<&str>, api_paths: &[String]) -> Result<(), IpcError> {
    if check.is_some_and(|c| c.chars().count() > CHECK_MAX) {
        return Err(IpcError::new(
            ErrorCode::Invalid,
            "The check command is over 2,000 characters",
        ));
    }
    if api_paths.len() > API_PATHS_MAX {
        return Err(IpcError::new(ErrorCode::Invalid, "Up to 50 paths"));
    }
    Ok(())
}

/// Who a session is and where it works.
#[derive(Debug, Clone, PartialEq)]
pub struct Session {
    pub source: SourceRef,
    pub title: String,
    pub engine_id: Option<String>,
    pub workspace_id: String,
    pub workspace: String,
    pub root: PathBuf,
    pub branch: Option<String>,
    pub lane_id: Option<String>,
}

/// What the queue needs from the app, so it's tested with fakes.
#[async_trait::async_trait]
pub trait QueueHost: Send + Sync {
    /// `None`: the pane or thread is gone, or has no local workspace.
    async fn session(&self, key: &str) -> Option<Session>;
    /// The changes to review; `None` when the folder isn't a git repository.
    async fn changes(&self, s: &Session) -> Option<Vec<FileDiff>>;
    async fn closing(&self, key: &str, s: &Session) -> Option<String>;
    async fn turn_checkpoint(&self, key: &str, s: &Session) -> Option<String>;
    async fn checkpoint_diff(&self, id: &str) -> Result<Vec<FileDiff>, IpcError>;
    async fn repo(&self, s: &Session) -> RepoReview;
    async fn run_check(&self, command: &str, cwd: &Path) -> (Option<i32>, String);
    fn now(&self) -> i64;
}

#[derive(Default)]
struct Inner {
    items: HashMap<String, ReviewItem>,
    /// Diff hashes the user approved, by session key.
    approvals: HashMap<String, String>,
    /// Approved once, then changed: stays "Changed" until approved again.
    changed: HashSet<String>,
    /// Each key's status and when it began: a done whose start moved is a
    /// new finish even when the status bus coalesced working away (50 ms).
    last: HashMap<String, (AgentStatus, i64)>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Why {
    Finished,
    Refresh,
}

pub struct ReviewQueue {
    host: Arc<dyn QueueHost>,
    bus: EventBus,
    inner: Mutex<Inner>,
}

fn not_found() -> IpcError {
    IpcError::new(ErrorCode::NotFound, "That item isn't in the review queue")
}

impl ReviewQueue {
    pub fn new(host: Arc<dyn QueueHost>, bus: EventBus) -> Arc<Self> {
        Arc::new(Self {
            host,
            bus,
            inner: Mutex::new(Inner::default()),
        })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn view(&self) -> ReviewQueueView {
        let now = self.host.now();
        let mut inner = self.lock();
        // An hour's snooze runs out on its own.
        for item in inner.items.values_mut() {
            if item.snoozed_until.is_some_and(|t| t <= now) {
                item.snoozed_until = None;
                if item.state == ReviewState::Snoozed {
                    item.state = ReviewState::Review;
                }
            }
        }
        let mut items: Vec<ReviewItem> = inner.items.values().cloned().collect();
        drop(inner);
        items.sort_by(|a, b| a.finished_at.cmp(&b.finished_at).then(a.key.cmp(&b.key)));
        ReviewQueueView { items }
    }

    fn publish(&self) {
        self.bus
            .publish(BusEvent::new(Topic::Status, EVENT, self.view()));
    }

    /// `status:changed`: done joins, working leaves, gone forgets.
    pub async fn on_status(self: &Arc<Self>, records: Vec<StatusRecord>, removed: Vec<String>) {
        let mut changed = false;
        {
            let mut inner = self.lock();
            for key in &removed {
                inner.last.remove(key);
                inner.approvals.remove(key);
                inner.changed.remove(key);
                changed |= inner.items.remove(key).is_some();
            }
        }
        for rec in records {
            let key = rec.key.0.clone();
            let prev = self
                .lock()
                .last
                .insert(key.clone(), (rec.status, rec.since));
            match rec.status {
                AgentStatus::Done if prev != Some((AgentStatus::Done, rec.since)) => {
                    self.consider(&key, Why::Finished).await;
                    changed = false;
                    self.publish();
                }
                AgentStatus::Working | AgentStatus::Waiting => {
                    changed |= self.lock().items.remove(&key).is_some();
                }
                _ => {}
            }
        }
        if changed {
            self.publish();
        }
    }

    /// Builds or rebuilds one item from its session's changes.
    async fn consider(self: &Arc<Self>, key: &str, why: Why) {
        let Some(session) = self.host.session(key).await else {
            self.lock().items.remove(key);
            return;
        };
        let files = match self.host.changes(&session).await {
            Some(f) if !f.is_empty() => f,
            _ => {
                self.lock().items.remove(key);
                return;
            }
        };
        let repo = self.host.repo(&session).await;
        let closing = self
            .host
            .closing(key, &session)
            .await
            .map(|c| c.chars().take(CLOSING_MAX).collect::<String>());
        let turn = self.host.turn_checkpoint(key, &session).await;
        let hash = diff_hash(&files);
        let now = self.host.now();
        let run_check = {
            let mut inner = self.lock();
            let old = inner.items.get(key).cloned();
            let state = match inner.approvals.get(key) {
                Some(h) if *h == hash => ReviewState::Approved,
                Some(_) => {
                    inner.approvals.remove(key);
                    inner.changed.insert(key.to_string());
                    ReviewState::Changed
                }
                None if inner.changed.contains(key) => ReviewState::Changed,
                None => ReviewState::Review,
            };
            let snoozed_until = match why {
                Why::Finished => None,
                Why::Refresh => old.as_ref().and_then(|o| o.snoozed_until),
            }
            .filter(|t| *t > now);
            let state = if snoozed_until.is_some() && state != ReviewState::Approved {
                ReviewState::Snoozed
            } else {
                state
            };
            let same_diff = old.as_ref().is_some_and(|o| o.diff_hash == hash);
            let rerun = repo.check.is_some() && (why == Why::Finished || !same_diff);
            let check = if rerun {
                Some(ReviewCheck {
                    state: ReviewCheckState::Running,
                    exit: None,
                    output: String::new(),
                })
            } else if repo.check.is_some() {
                old.as_ref().and_then(|o| o.check.clone())
            } else {
                None
            };
            let item = ReviewItem {
                key: key.to_string(),
                source: session.source.clone(),
                title: session.title.clone(),
                engine_id: session.engine_id.clone(),
                workspace_id: session.workspace_id.clone(),
                workspace: session.workspace.clone(),
                branch: session.branch.clone(),
                lane_id: session.lane_id.clone(),
                finished_at: match (why, &old) {
                    (Why::Refresh, Some(o)) => o.finished_at,
                    _ => now,
                },
                closing,
                files: files.len() as u32,
                additions: files.iter().map(|f| f.additions).sum(),
                deletions: files.iter().map(|f| f.deletions).sum(),
                risks: risks(&files, &repo.api_paths),
                check,
                diff_hash: hash.clone(),
                state,
                snoozed_until,
                turn_checkpoint_id: turn,
            };
            inner.items.insert(key.to_string(), item);
            rerun.then(|| repo.check.clone()).flatten()
        };
        tracing::info!(code = "review.queued", "an item joined the review queue");
        if let Some(command) = run_check {
            self.spawn_check(key.to_string(), hash, command, session.root);
        }
    }

    fn spawn_check(self: &Arc<Self>, key: String, hash: String, command: String, cwd: PathBuf) {
        let me = self.clone();
        tokio::spawn(async move {
            let (exit, output) = me.host.run_check(&command, &cwd).await;
            let done = {
                let mut inner = me.lock();
                match inner.items.get_mut(&key) {
                    // A newer diff has its own run.
                    Some(item) if item.diff_hash == hash => {
                        item.check = Some(ReviewCheck {
                            state: if exit == Some(0) {
                                ReviewCheckState::Passed
                            } else {
                                ReviewCheckState::Failed
                            },
                            exit,
                            output,
                        });
                        true
                    }
                    _ => false,
                }
            };
            tracing::info!(
                code = "review.check_done",
                ok = exit == Some(0),
                "review check finished"
            );
            if done {
                me.publish();
            }
        });
    }

    /// Seeds from the status bus's records (startup), then the list.
    pub async fn seed(self: &Arc<Self>, records: Vec<StatusRecord>) {
        self.on_status(records, Vec::new()).await;
    }

    pub async fn list(self: &Arc<Self>, refresh: bool) -> ReviewQueueView {
        if refresh {
            let keys: Vec<String> = self.lock().items.keys().cloned().collect();
            for key in keys {
                self.consider(&key, Why::Refresh).await;
            }
            self.publish();
        }
        self.view()
    }

    fn item(&self, key: &str) -> Result<ReviewItem, IpcError> {
        self.lock().items.get(key).cloned().ok_or_else(not_found)
    }

    pub fn approve(&self, key: &str, hash: &str) -> Result<ReviewItem, IpcError> {
        let item = {
            let mut inner = self.lock();
            let current = inner
                .items
                .get(key)
                .ok_or_else(not_found)?
                .diff_hash
                .clone();
            if current != hash {
                return Err(IpcError::new(
                    ErrorCode::Conflict,
                    "Changed since you looked",
                ));
            }
            inner.approvals.insert(key.to_string(), current);
            inner.changed.remove(key);
            let item = inner.items.get_mut(key).ok_or_else(not_found)?;
            item.state = ReviewState::Approved;
            item.snoozed_until = None;
            item.clone()
        };
        tracing::info!(code = "review.approved", "review item approved");
        self.publish();
        Ok(item)
    }

    pub fn unapprove(&self, key: &str) -> Result<ReviewItem, IpcError> {
        let item = {
            let mut inner = self.lock();
            inner.approvals.remove(key);
            let item = inner.items.get_mut(key).ok_or_else(not_found)?;
            item.state = ReviewState::Review;
            item.clone()
        };
        self.publish();
        Ok(item)
    }

    pub fn snooze(&self, key: &str, minutes: Option<i64>) -> Result<ReviewItem, IpcError> {
        let now = self.host.now();
        let minutes = minutes.unwrap_or(SNOOZE_MINUTES).max(0);
        let item = {
            let mut inner = self.lock();
            let changed = inner.changed.contains(key);
            let approved = inner.approvals.contains_key(key);
            let item = inner.items.get_mut(key).ok_or_else(not_found)?;
            if minutes == 0 {
                item.snoozed_until = None;
                item.state = if approved {
                    ReviewState::Approved
                } else if changed {
                    ReviewState::Changed
                } else {
                    ReviewState::Review
                };
            } else {
                item.snoozed_until = Some(now + minutes * 60_000);
                item.state = ReviewState::Snoozed;
            }
            item.clone()
        };
        self.publish();
        Ok(item)
    }

    pub async fn item_diff(&self, key: &str, turn: bool) -> Result<Vec<FileDiff>, IpcError> {
        let item = self.item(key)?;
        if turn {
            let id = item.turn_checkpoint_id.ok_or_else(|| {
                IpcError::new(ErrorCode::Invalid, "This agent has no checkpoint yet")
            })?;
            return self.host.checkpoint_diff(&id).await;
        }
        let session = self.host.session(key).await.ok_or_else(not_found)?;
        Ok(self.host.changes(&session).await.unwrap_or_default())
    }

    pub async fn run_check(self: &Arc<Self>, key: &str) -> Result<(), IpcError> {
        let session = self.host.session(key).await.ok_or_else(not_found)?;
        let command = self
            .host
            .repo(&session)
            .await
            .check
            .ok_or_else(|| IpcError::new(ErrorCode::Invalid, "Set a check command first"))?;
        let hash = {
            let mut inner = self.lock();
            let item = inner.items.get_mut(key).ok_or_else(not_found)?;
            item.check = Some(ReviewCheck {
                state: ReviewCheckState::Running,
                exit: None,
                output: String::new(),
            });
            item.diff_hash.clone()
        };
        self.publish();
        self.spawn_check(key.to_string(), hash, command, session.root);
        Ok(())
    }
}

/// Whether a thread's finished work joins the queue. Teammate chats have no
/// workspace (F-DB-04), and a read-only thread, such as a second opinion's
/// reviewer (F-WT-05), can't have made the changes in its folder: they're
/// another agent's, and Send back would reach the wrong one (D-859).
fn reviews_thread(
    kind: tandem_store::repo::threads::ThreadKind,
    access: tandem_core::access::Access,
) -> bool {
    kind == tandem_store::repo::threads::ThreadKind::Thread
        && access != tandem_core::access::Access::Read
}

/// The app's host: panes, threads, workspaces, git, checkpoints, settings.
pub struct AppHost {
    pub app: tauri::AppHandle,
}

impl AppHost {
    fn state(&self) -> tauri::State<'_, crate::state::AppState> {
        use tauri::Manager;
        self.app.state::<crate::state::AppState>()
    }

    /// The repository a workspace belongs to: a worktree's main checkout.
    async fn repo_root(&self, s: &Session) -> Option<PathBuf> {
        use tandem_core::ids::WorkspaceId;
        let state = self.state();
        if s.lane_id.is_some() {
            let ws = tandem_store::repo::workspaces::get(
                &state.store,
                &WorkspaceId::from_db(s.workspace_id.clone()),
            )
            .await
            .ok()?;
            let parent = tandem_store::repo::workspaces::get(&state.store, ws.parent_id.as_ref()?)
                .await
                .ok()?;
            return Some(PathBuf::from(parent.root));
        }
        let repo = tandem_git::Repo::discover(&s.root).ok()??;
        Some(repo.root().to_path_buf())
    }
}

/// The settings maps the repository's review settings live in.
async fn repo_settings(store: &tandem_store::Store) -> (serde_json::Value, serde_json::Value) {
    use tandem_core::settings::SettingKey;
    let get = |k| async move {
        tandem_store::repo::settings::get(store, k)
            .await
            .unwrap_or(serde_json::Value::Null)
    };
    (
        get(SettingKey::ReviewChecks).await,
        get(SettingKey::ReviewApiPaths).await,
    )
}

/// `review_repo_get`'s answer for a repository root.
pub async fn repo_review(store: &tandem_store::Store, root: &str) -> RepoReview {
    let (checks, paths) = repo_settings(store).await;
    RepoReview {
        root: root.to_string(),
        check: checks
            .get(root)
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .filter(|c| !c.trim().is_empty()),
        api_paths: paths
            .get(root)
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|p| p.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default(),
    }
}

/// Saves a repository's check command and API paths (`review_repo_set`).
pub async fn save_repo(
    store: &tandem_store::Store,
    root: &str,
    check: Option<String>,
    api_paths: Vec<String>,
) -> Result<RepoReview, IpcError> {
    use tandem_core::settings::SettingKey;
    validate_repo(check.as_deref(), &api_paths)?;
    let (mut checks, mut paths) = repo_settings(store).await;
    let as_map = |v: &mut serde_json::Value| {
        if !v.is_object() {
            *v = serde_json::json!({});
        }
    };
    as_map(&mut checks);
    as_map(&mut paths);
    let check = check
        .map(|c| c.trim().to_string())
        .filter(|c| !c.is_empty());
    let api_paths: Vec<String> = api_paths
        .into_iter()
        .map(|p| p.trim().to_string())
        .filter(|p| !p.is_empty())
        .collect();
    if let Some(map) = checks.as_object_mut() {
        match &check {
            Some(c) => map.insert(root.to_string(), serde_json::json!(c)),
            None => map.remove(root),
        };
    }
    if let Some(map) = paths.as_object_mut() {
        if api_paths.is_empty() {
            map.remove(root);
        } else {
            map.insert(root.to_string(), serde_json::json!(api_paths));
        }
    }
    tandem_store::repo::settings::set(store, SettingKey::ReviewChecks, checks)
        .await
        .map_err(|e| IpcError::new(ErrorCode::Io, e.to_string()))?;
    tandem_store::repo::settings::set(store, SettingKey::ReviewApiPaths, paths)
        .await
        .map_err(|e| IpcError::new(ErrorCode::Io, e.to_string()))?;
    tracing::info!(code = "review.repo_saved", "review settings saved");
    Ok(repo_review(store, root).await)
}

/// The repository root a workspace's review settings are kept under.
pub async fn workspace_repo_root(
    store: &tandem_store::Store,
    workspace: &tandem_core::ids::WorkspaceId,
) -> Result<String, IpcError> {
    let ws = tandem_store::repo::workspaces::get(store, workspace)
        .await
        .map_err(|e| IpcError::new(ErrorCode::NotFound, e.to_string()))?;
    let root = match (&ws.lane_branch, &ws.parent_id) {
        (Some(_), Some(parent)) => {
            tandem_store::repo::workspaces::get(store, parent)
                .await
                .map_err(|e| IpcError::new(ErrorCode::NotFound, e.to_string()))?
                .root
        }
        _ => ws.root,
    };
    let not_git = || IpcError::new(ErrorCode::Invalid, "This folder isn't a git repository");
    let repo = tandem_git::Repo::discover(Path::new(&root))
        .map_err(|_| not_git())?
        .ok_or_else(not_git)?;
    Ok(repo.root().to_string_lossy().into_owned())
}

#[async_trait::async_trait]
impl QueueHost for AppHost {
    async fn session(&self, key: &str) -> Option<Session> {
        use tandem_core::ids::{PaneId, ThreadId};
        use tandem_store::repo::{panes, threads, workspaces};
        let state = self.state();
        let (source, title, engine_id, ws_id) = if let Some(id) = key.strip_prefix("pane:") {
            let pane = panes::get_open(&state.store, &PaneId::from_db(id.to_string()))
                .await
                .ok()?;
            let label = pane
                .engine_id
                .as_deref()
                .and_then(|e| state.terminals.engines.get(e))
                .map(|s| s.label);
            (
                SourceRef::Pane {
                    pane_id: id.to_string(),
                },
                pane.human_name
                    .or(label)
                    .unwrap_or_else(|| "Terminal".into()),
                pane.engine_id,
                pane.workspace_id,
            )
        } else {
            let id = key.strip_prefix("thread:")?;
            let t = threads::get(&state.store, &ThreadId::from_db(id.to_string()))
                .await
                .ok()??;
            if !reviews_thread(t.kind, t.access) {
                return None;
            }
            (
                SourceRef::Thread {
                    thread_id: id.to_string(),
                },
                t.title,
                Some(t.engine_id),
                t.workspace_id?,
            )
        };
        let ws = workspaces::get(&state.store, &ws_id).await.ok()?;
        // Local folders for now (D-2421).
        if ws.kind != "local" {
            return None;
        }
        let root = PathBuf::from(&ws.root);
        Some(Session {
            source,
            title,
            engine_id,
            workspace_id: ws.id.as_str().to_string(),
            branch: ws
                .lane_branch
                .clone()
                .or_else(|| crate::services::workspaces::git_branch(&root)),
            lane_id: ws.lane_branch.is_some().then(|| ws.id.as_str().to_string()),
            workspace: ws.name,
            root,
        })
    }

    async fn changes(&self, s: &Session) -> Option<Vec<FileDiff>> {
        let state = self.state();
        if let Some(lane) = &s.lane_id {
            let id = tandem_core::ids::WorkspaceId::from_db(lane.clone());
            return crate::services::lanes::diff(&state.store, &id).await.ok();
        }
        let repo = tandem_git::Repo::discover(&s.root).ok()??;
        match repo.diff_since("HEAD").await {
            Ok(files) => Some(files),
            // No commits yet: everything counts as added.
            Err(_) => repo.diff_from(None).await.ok(),
        }
    }

    async fn closing(&self, key: &str, _s: &Session) -> Option<String> {
        use tandem_core::redact::Redactor;
        let state = self.state();
        let text = if let Some(id) = key.strip_prefix("thread:") {
            crate::services::second_opinion::last_reply(
                &state.store,
                &tandem_core::ids::ThreadId::from_db(id.to_string()),
            )
            .await?
        } else {
            let id = key.strip_prefix("pane:")?;
            let screen = state.terminals.pty.screen_text(id)?;
            let lines: Vec<&str> = screen
                .lines()
                .map(str::trim_end)
                .filter(|l| !l.trim().is_empty())
                .collect();
            lines[lines.len().saturating_sub(3)..].join("\n")
        };
        let text = Redactor::global().redact(&text).into_owned();
        (!text.trim().is_empty()).then_some(text)
    }

    async fn turn_checkpoint(&self, key: &str, s: &Session) -> Option<String> {
        let state = self.state();
        let folder = std::fs::canonicalize(&s.root).ok()?;
        let folder = folder.to_string_lossy().into_owned();
        let thread = key.strip_prefix("thread:").map(str::to_string);
        let pane = key.strip_prefix("pane:");
        let rows = tandem_store::repo::checkpoints::list(&state.store, folder, thread)
            .await
            .ok()?;
        rows.into_iter()
            .find(|c| pane.is_none() || c.pane_id.as_deref() == pane)
            .map(|c| c.id)
    }

    async fn checkpoint_diff(&self, id: &str) -> Result<Vec<FileDiff>, IpcError> {
        self.state().checkpoints.diff(id).await
    }

    async fn repo(&self, s: &Session) -> RepoReview {
        let Some(root) = self.repo_root(s).await else {
            return RepoReview::default();
        };
        repo_review(&self.state().store, &root.to_string_lossy()).await
    }

    async fn run_check(&self, command: &str, cwd: &Path) -> (Option<i32>, String) {
        use crate::services::bestof;
        let env = self.state().terminals.login_env().await.vars.clone();
        // F-EX-06: with "one check at a time" on, it waits for its turn.
        let _turn = crate::services::check_queue::turn().await;
        let (exit, out) = bestof::run_check(command, cwd, &env, CHECK_LIMIT, None).await;
        let out = tandem_core::redact::Redactor::global()
            .redact(&bestof::tail(&out, bestof::TAIL_LINES))
            .into_owned();
        (exit, out)
    }

    fn now(&self) -> i64 {
        self.state().clock.now_ms()
    }
}

/// Builds the queue, seeds it from the status bus and follows it.
pub fn start(app: tauri::AppHandle) -> Arc<ReviewQueue> {
    use tauri::Manager;
    let state = app.state::<crate::state::AppState>();
    let queue = ReviewQueue::new(Arc::new(AppHost { app: app.clone() }), state.bus.clone());
    let mut rx = state.bus.subscribe(Topic::Status);
    let seed = state.terminals.status.store.snapshot();
    let q = queue.clone();
    tauri::async_runtime::spawn(async move {
        q.seed(seed).await;
        loop {
            match rx.recv().await {
                Ok(ev) if ev.name == "status:changed" => {
                    let records: Vec<StatusRecord> = ev
                        .payload
                        .get("records")
                        .cloned()
                        .and_then(|v| serde_json::from_value(v).ok())
                        .unwrap_or_default();
                    let removed: Vec<String> = ev
                        .payload
                        .get("removed")
                        .cloned()
                        .and_then(|v| serde_json::from_value(v).ok())
                        .unwrap_or_default();
                    q.on_status(records, removed).await;
                }
                Ok(_) => {}
                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                    tracing::warn!(missed = n, "review queue lagged")
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    });
    queue
}

#[cfg(test)]
mod tests {
    use super::*;
    use tandem_core::status::SessionKey;

    fn file(path: &str, change: FileChange, patch: &str) -> FileDiff {
        FileDiff {
            path: path.into(),
            old_path: None,
            change,
            additions: 1,
            deletions: 0,
            binary: false,
            patch: patch.into(),
            truncated: false,
        }
    }

    fn m(path: &str) -> FileDiff {
        file(path, FileChange::Modified, "+x\n")
    }

    #[test]
    fn f_db_04_1_a_read_only_thread_never_joins_the_queue() {
        use tandem_core::access::Access;
        use tandem_store::repo::threads::ThreadKind;
        // A second opinion's reviewer (F-WT-05) reads the worktree it
        // reviews: the diff there is another agent's work (D-859).
        assert!(!reviews_thread(ThreadKind::Thread, Access::Read));
        assert!(reviews_thread(ThreadKind::Thread, Access::Edits));
        assert!(reviews_thread(ThreadKind::Thread, Access::Full));
        // Teammate chats have no workspace.
        assert!(!reviews_thread(ThreadKind::TeammateChat, Access::Edits));
    }

    #[test]
    fn f_db_04_2_migrations_and_a_deleted_test_both_label_and_readme_alone_none() {
        let r = risks(
            &[
                m("db/migrations/0042.sql"),
                file("tests/auth.test.ts", FileChange::Deleted, ""),
                m("src/app.ts"),
            ],
            &[],
        );
        let rules: Vec<ReviewRiskRule> = r.iter().map(|x| x.rule).collect();
        assert!(rules.contains(&ReviewRiskRule::Migrations), "{r:?}");
        assert!(rules.contains(&ReviewRiskRule::DeletedTests), "{r:?}");
        let mig = r
            .iter()
            .find(|x| x.rule == ReviewRiskRule::Migrations)
            .expect("m");
        assert_eq!(mig.files, vec!["db/migrations/0042.sql"]);
        let del = r
            .iter()
            .find(|x| x.rule == ReviewRiskRule::DeletedTests)
            .expect("d");
        assert_eq!(del.files, vec!["tests/auth.test.ts"]);
        assert!(risks(&[m("README.md")], &[]).is_empty());
    }

    #[test]
    fn f_db_04_2_each_rule_and_its_limits() {
        let fire = |p: &str| risks(&[m(p)], &["src/api/".into()]);
        let one = |p: &str| fire(p).into_iter().map(|r| r.rule).collect::<Vec<_>>();
        assert_eq!(one(".github/workflows/ci.yml"), vec![ReviewRiskRule::Ci]);
        assert_eq!(one(".gitlab-ci.yml"), vec![ReviewRiskRule::Ci]);
        assert_eq!(one("web/pnpm-lock.yaml"), vec![ReviewRiskRule::Lockfile]);
        assert_eq!(one("Cargo.lock"), vec![ReviewRiskRule::Lockfile]);
        assert_eq!(one(".env.local"), vec![ReviewRiskRule::Env]);
        assert!(one(".env.example").is_empty());
        assert_eq!(one("src/oauth/callback.ts"), vec![ReviewRiskRule::Auth]);
        assert!(one("AUTHORS.md").is_empty());
        assert_eq!(one("src/api/users.ts"), vec![ReviewRiskRule::PublicApi]);
        // A migrations file name isn't a migrations folder.
        assert!(one("docs/migrations.md").is_empty());
        // A test that changed isn't a deleted test.
        assert!(one("src/a.test.ts").is_empty());
        let many: Vec<FileDiff> = (0..25).map(|i| m(&format!("f{i}.ts"))).collect();
        let r = risks(&many, &[]);
        assert_eq!(r[0].rule, ReviewRiskRule::ManyFiles);
        assert_eq!(r[0].files.len(), 5, "up to 5 files per rule");
        assert!(risks(&many[..19], &[]).is_empty());
    }

    #[test]
    fn f_db_04_3_the_hash_follows_the_diff() {
        let a = vec![m("a.ts")];
        assert_eq!(diff_hash(&a), diff_hash(&a.clone()));
        assert_ne!(
            diff_hash(&a),
            diff_hash(&[file("a.ts", FileChange::Modified, "+y\n")])
        );
        assert!(validate_repo(Some(&"x".repeat(2_001)), &[]).is_err());
        assert!(validate_repo(Some("pnpm test"), &vec!["a".to_string(); 50]).is_ok());
        assert!(validate_repo(None, &vec!["a".to_string(); 51]).is_err());
    }

    struct Fake {
        sessions: Mutex<HashMap<String, Session>>,
        diffs: Mutex<HashMap<String, Vec<FileDiff>>>,
        check: Mutex<Option<String>>,
        now: Mutex<i64>,
        checks_run: Mutex<Vec<String>>,
    }

    #[async_trait::async_trait]
    impl QueueHost for Fake {
        async fn session(&self, key: &str) -> Option<Session> {
            self.sessions.lock().expect("l").get(key).cloned()
        }
        async fn changes(&self, s: &Session) -> Option<Vec<FileDiff>> {
            self.diffs.lock().expect("l").get(&s.workspace_id).cloned()
        }
        async fn closing(&self, key: &str, _: &Session) -> Option<String> {
            Some(format!("closing of {key}"))
        }
        async fn turn_checkpoint(&self, _: &str, _: &Session) -> Option<String> {
            Some("cp_1".into())
        }
        async fn checkpoint_diff(&self, id: &str) -> Result<Vec<FileDiff>, IpcError> {
            Ok(vec![m(&format!("{id}.ts"))])
        }
        async fn repo(&self, s: &Session) -> RepoReview {
            RepoReview {
                root: s.root.to_string_lossy().into_owned(),
                check: self.check.lock().expect("l").clone(),
                api_paths: Vec::new(),
            }
        }
        async fn run_check(&self, command: &str, _: &Path) -> (Option<i32>, String) {
            self.checks_run.lock().expect("l").push(command.into());
            match command {
                "true" => (Some(0), String::new()),
                _ => (Some(1), "boom".into()),
            }
        }
        fn now(&self) -> i64 {
            *self.now.lock().expect("l")
        }
    }

    fn session(ws: &str, lane: bool) -> Session {
        Session {
            source: SourceRef::Pane { pane_id: ws.into() },
            title: format!("agent {ws}"),
            engine_id: Some("claude".into()),
            workspace_id: ws.into(),
            workspace: ws.into(),
            root: PathBuf::from(format!("/r/{ws}")),
            branch: Some(format!("tandem/{ws}")),
            lane_id: lane.then(|| ws.to_string()),
        }
    }

    fn rig() -> (Arc<Fake>, Arc<ReviewQueue>, EventBus) {
        let fake = Arc::new(Fake {
            sessions: Mutex::new(HashMap::new()),
            diffs: Mutex::new(HashMap::new()),
            check: Mutex::new(None),
            now: Mutex::new(1_000),
            checks_run: Mutex::new(Vec::new()),
        });
        let bus = EventBus::new();
        let q = ReviewQueue::new(fake.clone(), bus.clone());
        (fake, q, bus)
    }

    fn rec(key: &str, status: AgentStatus) -> StatusRecord {
        StatusRecord {
            key: SessionKey(key.into()),
            status,
            reason: None,
            since: 0,
            ts: 0,
            tool: None,
            title: None,
            preview: None,
        }
    }

    fn add(fake: &Fake, key: &str, ws: &str, diff: Option<Vec<FileDiff>>) {
        fake.sessions
            .lock()
            .expect("l")
            .insert(key.into(), session(ws, true));
        if let Some(d) = diff {
            fake.diffs.lock().expect("l").insert(ws.into(), d);
        }
    }

    #[tokio::test]
    async fn f_db_04_1_done_with_changes_joins_without_changes_doesnt_working_leaves() {
        let (fake, q, bus) = rig();
        let mut rx = bus.subscribe(Topic::Status);
        add(&fake, "pane:a", "wa", Some(vec![m("a.ts")]));
        add(&fake, "thread:b", "wb", Some(vec![m("b.ts")]));
        add(&fake, "pane:c", "wc", Some(Vec::new()));
        q.on_status(
            vec![
                rec("pane:a", AgentStatus::Done),
                rec("thread:b", AgentStatus::Done),
                rec("pane:c", AgentStatus::Done),
            ],
            Vec::new(),
        )
        .await;
        let ev = rx.recv().await.expect("event");
        assert_eq!(ev.name, EVENT);
        let keys: Vec<String> = q
            .list(false)
            .await
            .items
            .into_iter()
            .map(|i| i.key)
            .collect();
        assert_eq!(keys.len(), 2);
        assert!(keys.contains(&"pane:a".to_string()) && keys.contains(&"thread:b".to_string()));
        let a = q
            .list(false)
            .await
            .items
            .into_iter()
            .find(|i| i.key == "pane:a")
            .expect("a");
        assert_eq!(a.closing.as_deref(), Some("closing of pane:a"));
        assert_eq!((a.files, a.state), (1, ReviewState::Review));
        assert_eq!(a.turn_checkpoint_id.as_deref(), Some("cp_1"));
        // Working again: it leaves; gone: forgotten.
        q.on_status(vec![rec("pane:a", AgentStatus::Working)], Vec::new())
            .await;
        q.on_status(Vec::new(), vec!["thread:b".into()]).await;
        assert!(q.list(false).await.items.is_empty());
        // Idle after done (seen) keeps the item.
        q.on_status(vec![rec("pane:a", AgentStatus::Done)], Vec::new())
            .await;
        q.on_status(vec![rec("pane:a", AgentStatus::Idle)], Vec::new())
            .await;
        assert_eq!(q.list(false).await.items.len(), 1);
        assert_eq!(
            q.item_diff("pane:a", true).await.expect("turn")[0].path,
            "cp_1.ts"
        );
        assert_eq!(
            q.item_diff("pane:a", false).await.expect("all")[0].path,
            "a.ts"
        );
    }

    #[tokio::test]
    async fn f_db_04_1_a_coalesced_new_done_still_counts() {
        let (fake, q, _bus) = rig();
        add(&fake, "pane:a", "wa", Some(Vec::new()));
        q.on_status(vec![rec("pane:a", AgentStatus::Done)], Vec::new())
            .await;
        assert!(q.list(false).await.items.is_empty());
        // Working and done again inside one 50 ms batch: only the new start shows.
        fake.diffs
            .lock()
            .expect("l")
            .insert("wa".into(), vec![m("a.ts")]);
        let mut again = rec("pane:a", AgentStatus::Done);
        again.since = 5;
        q.on_status(vec![again.clone()], Vec::new()).await;
        assert_eq!(q.list(false).await.items.len(), 1);
        // The same record again changes nothing.
        fake.diffs
            .lock()
            .expect("l")
            .insert("wa".into(), Vec::new());
        q.on_status(vec![again], Vec::new()).await;
        assert_eq!(q.list(false).await.items.len(), 1);
    }

    #[tokio::test]
    async fn f_db_04_3_approval_goes_stale_when_the_diff_changes() {
        let (fake, q, _bus) = rig();
        add(&fake, "pane:a", "wa", Some(vec![m("a.ts")]));
        q.on_status(vec![rec("pane:a", AgentStatus::Done)], Vec::new())
            .await;
        let item = q.list(false).await.items.remove(0);
        assert_eq!(
            q.approve("pane:a", "stale").expect_err("old hash").code,
            ErrorCode::Conflict
        );
        assert_eq!(
            q.approve("pane:a", &item.diff_hash).expect("approve").state,
            ReviewState::Approved
        );
        // The same diff on a refresh stays approved.
        assert_eq!(q.list(true).await.items[0].state, ReviewState::Approved);
        // The agent edits a file and finishes again.
        q.on_status(vec![rec("pane:a", AgentStatus::Working)], Vec::new())
            .await;
        fake.diffs.lock().expect("l").insert(
            "wa".into(),
            vec![file("a.ts", FileChange::Modified, "+edited\n")],
        );
        q.on_status(vec![rec("pane:a", AgentStatus::Done)], Vec::new())
            .await;
        let item = q.list(false).await.items.remove(0);
        assert_eq!(item.state, ReviewState::Changed);
        // A refresh doesn't clear "Changed"; approving again does.
        assert_eq!(q.list(true).await.items[0].state, ReviewState::Changed);
        q.approve("pane:a", &item.diff_hash).expect("again");
        assert_eq!(
            q.unapprove("pane:a").expect("un").state,
            ReviewState::Review
        );
        // A refresh that finds no changes drops it.
        fake.diffs
            .lock()
            .expect("l")
            .insert("wa".into(), Vec::new());
        assert!(q.list(true).await.items.is_empty());
    }

    #[tokio::test]
    async fn f_db_04_snooze_hides_for_an_hour_or_until_it_finishes_again() {
        let (fake, q, _bus) = rig();
        add(&fake, "pane:a", "wa", Some(vec![m("a.ts")]));
        q.on_status(vec![rec("pane:a", AgentStatus::Done)], Vec::new())
            .await;
        let s = q.snooze("pane:a", None).expect("snooze");
        assert_eq!(
            (s.state, s.snoozed_until),
            (ReviewState::Snoozed, Some(1_000 + 3_600_000))
        );
        assert_eq!(q.list(true).await.items[0].state, ReviewState::Snoozed);
        *fake.now.lock().expect("l") = 1_000 + 3_600_000;
        assert_eq!(q.list(false).await.items[0].state, ReviewState::Review);
        q.snooze("pane:a", None).expect("snooze");
        q.on_status(vec![rec("pane:a", AgentStatus::Working)], Vec::new())
            .await;
        q.on_status(vec![rec("pane:a", AgentStatus::Done)], Vec::new())
            .await;
        assert_eq!(q.list(false).await.items[0].state, ReviewState::Review);
        assert_eq!(
            q.snooze("pane:a", Some(0)).expect("un").state,
            ReviewState::Review
        );
        assert_eq!(
            q.snooze("nope", None).expect_err("missing").code,
            ErrorCode::NotFound
        );
    }

    async fn settled(q: &Arc<ReviewQueue>) -> ReviewCheck {
        for _ in 0..100 {
            let c = q.list(false).await.items[0].check.clone().expect("check");
            if c.state != ReviewCheckState::Running {
                return c;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("the check never finished");
    }

    #[tokio::test]
    async fn f_db_04_5_the_check_passes_or_fails_with_its_output() {
        let (fake, q, _bus) = rig();
        add(&fake, "pane:a", "wa", Some(vec![m("a.ts")]));
        *fake.check.lock().expect("l") = Some("true".into());
        q.on_status(vec![rec("pane:a", AgentStatus::Done)], Vec::new())
            .await;
        assert_eq!(settled(&q).await.state, ReviewCheckState::Passed);
        // The same diff on a refresh keeps the result instead of running again.
        q.list(true).await;
        assert_eq!(fake.checks_run.lock().expect("l").len(), 1);
        *fake.check.lock().expect("l") = Some("false".into());
        q.run_check("pane:a").await.expect("run");
        let c = settled(&q).await;
        assert_eq!(
            (c.state, c.exit, c.output.as_str()),
            (ReviewCheckState::Failed, Some(1), "boom")
        );
        *fake.check.lock().expect("l") = None;
        assert_eq!(
            q.run_check("pane:a").await.expect_err("none").code,
            ErrorCode::Invalid
        );
    }
}
