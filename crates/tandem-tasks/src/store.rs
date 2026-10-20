//! The file-backed tracker at `$DATA/tasks/` (F-TK-01, ARCHITECTURE §9.2).
//!
//! Every write takes a lock file (per issue, or `meta.lock`), so the app and
//! any number of MCP server processes can write at once. Issue files are
//! replaced atomically (temp, fsync, rename); history and comments are
//! append-only JSON lines. Lock order: `idempotency` → `meta` → issue.

use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use base64::Engine as _;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tandem_core::atomic::atomic_write;
use tandem_core::clock::{Clock, SystemClock};

use crate::error::TaskError;
use crate::lock;
use crate::model::*;

mod claims;
pub use claims::{normalize_claim_path, overlaps_of};

/// Idempotency keys are remembered this long (F-TK-01 AC2).
pub const IDEMPOTENCY_WINDOW_MS: i64 = 24 * 60 * 60 * 1000;
/// Page size cap for issues (F-TK-02 AC3).
pub const PAGE_MAX: usize = 50;
pub const LIST_PAGE_MAX: usize = 100;

#[derive(Clone)]
pub struct TaskStore {
    dir: PathBuf,
    clock: Arc<dyn Clock>,
}

#[derive(Debug, Serialize, Deserialize)]
struct IdempotencyEntry {
    key: String,
    ts: i64,
    result: Value,
}

#[derive(Debug, Serialize, Deserialize)]
struct Cursor {
    sort: String,
    after: CursorPos,
}

#[derive(Debug, Serialize, Deserialize)]
struct CursorPos {
    updated_at: i64,
    number: u64,
}

impl TaskStore {
    /// Opens (and creates) the tracker in `dir`, dropping idempotency keys
    /// older than the window.
    pub fn open(dir: impl Into<PathBuf>) -> Result<Self, TaskError> {
        Self::open_with_clock(dir, Arc::new(SystemClock))
    }

    pub fn open_with_clock(
        dir: impl Into<PathBuf>,
        clock: Arc<dyn Clock>,
    ) -> Result<Self, TaskError> {
        let store = Self {
            dir: dir.into(),
            clock,
        };
        fs::create_dir_all(store.issues_dir())?;
        fs::create_dir_all(store.dir.join("locks"))?;
        {
            let _g = lock::acquire(&store.lock_path("meta"))?;
            if !store.meta_path().exists() {
                store.write_json(&store.meta_path(), &Meta::default())?;
            }
        }
        store.compact_idempotency()?;
        Ok(store)
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn issues_dir(&self) -> PathBuf {
        self.dir.join("issues")
    }

    fn meta_path(&self) -> PathBuf {
        self.dir.join("meta.json")
    }

    fn lock_path(&self, name: &str) -> PathBuf {
        self.dir.join("locks").join(format!("{name}.lock"))
    }

    fn issue_path(&self, number: u64) -> PathBuf {
        self.issues_dir().join(format!("ISSUE-{number}.json"))
    }

    fn history_path(&self, number: u64) -> PathBuf {
        self.issues_dir()
            .join(format!("ISSUE-{number}.history.jsonl"))
    }

    fn comments_path(&self, number: u64) -> PathBuf {
        self.issues_dir()
            .join(format!("ISSUE-{number}.comments.jsonl"))
    }

    fn now(&self) -> i64 {
        self.clock.now_ms()
    }

    fn write_json<T: Serialize>(&self, path: &Path, value: &T) -> Result<(), TaskError> {
        let bytes =
            serde_json::to_vec_pretty(value).map_err(|e| TaskError::Corrupt(e.to_string()))?;
        atomic_write(path, &bytes, Some(0o600))?;
        Ok(())
    }

    fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T, TaskError> {
        let bytes = fs::read(path)?;
        serde_json::from_slice(&bytes).map_err(|e| {
            TaskError::Corrupt(format!(
                "{}: {e}",
                path.file_name().unwrap_or_default().to_string_lossy()
            ))
        })
    }

    fn append_line<T: Serialize>(path: &Path, value: &T) -> Result<(), TaskError> {
        let mut line = serde_json::to_vec(value).map_err(|e| TaskError::Corrupt(e.to_string()))?;
        line.push(b'\n');
        let mut options = OpenOptions::new();
        options.create(true).append(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(path)?;
        file.write_all(&line)?;
        file.sync_data()?;
        Ok(())
    }

    fn read_lines<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<Vec<T>, TaskError> {
        let file = match fs::File::open(path) {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e.into()),
        };
        let mut out = Vec::new();
        for line in BufReader::new(file).lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            // A torn last line (a crash mid-append) is skipped, not fatal.
            if let Ok(v) = serde_json::from_str(&line) {
                out.push(v);
            }
        }
        Ok(out)
    }

    // ── meta ────────────────────────────────────────────────────────────

    pub fn meta(&self) -> Result<Meta, TaskError> {
        Self::read_json(&self.meta_path())
    }

    fn update_meta<R>(
        &self,
        f: impl FnOnce(&mut Meta) -> Result<R, TaskError>,
    ) -> Result<R, TaskError> {
        let _g = lock::acquire(&self.lock_path("meta"))?;
        let mut meta = self.meta()?;
        let out = f(&mut meta)?;
        self.write_json(&self.meta_path(), &meta)?;
        Ok(out)
    }

    /// Replaces the teammate actors (the app keeps them in step with its
    /// teammates); the human actor stays.
    pub fn sync_teammate_actors(&self, teammates: &[(String, String)]) -> Result<(), TaskError> {
        let current = self.meta()?;
        let wanted: Vec<Actor> = current
            .actors
            .iter()
            .filter(|a| a.kind == ActorKind::Human)
            .cloned()
            .chain(teammates.iter().map(|(id, name)| Actor {
                id: id.clone(),
                kind: ActorKind::Teammate,
                name: name.clone(),
            }))
            .collect();
        if wanted == current.actors {
            return Ok(());
        }
        self.update_meta(|m| {
            m.actors = wanted;
            Ok(())
        })
    }

    pub fn save_project(
        &self,
        id: Option<&str>,
        name: &str,
        key: Option<&str>,
    ) -> Result<Project, TaskError> {
        let name = name.trim();
        if name.is_empty() || name.chars().count() > 100 {
            return Err(TaskError::invalid("A project name is 1–100 characters."));
        }
        let key = key.map(str::trim).filter(|k| !k.is_empty());
        if let Some(k) = key {
            if k.len() > 10 || !k.chars().all(|c| c.is_ascii_alphanumeric()) {
                return Err(TaskError::invalid(
                    "A project key is up to 10 letters or digits.",
                ));
            }
        }
        self.update_meta(|m| match id {
            Some(id) => {
                let p = m
                    .projects
                    .iter_mut()
                    .find(|p| p.id == id)
                    .ok_or_else(|| TaskError::NotFound(id.to_string()))?;
                p.name = name.to_string();
                if key.is_some() {
                    p.key = key.map(str::to_string);
                }
                Ok(p.clone())
            }
            None => {
                let p = Project {
                    id: tandem_core::ids::new_id("prj"),
                    name: name.to_string(),
                    key: key.map(str::to_string),
                    archived: false,
                };
                m.projects.push(p.clone());
                Ok(p)
            }
        })
    }

    pub fn save_label(
        &self,
        id: Option<&str>,
        name: &str,
        color: Option<&str>,
    ) -> Result<Label, TaskError> {
        let name = name.trim();
        if name.is_empty() || name.chars().count() > 50 {
            return Err(TaskError::invalid("A label name is 1–50 characters."));
        }
        if let Some(c) = color {
            let ok = c.len() == 7
                && c.starts_with('#')
                && c[1..]
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
            if !ok {
                return Err(TaskError::invalid(
                    "A label color is #rrggbb in lower-case hex.",
                ));
            }
        }
        self.update_meta(|m| match id {
            Some(id) => {
                let l = m
                    .labels
                    .iter_mut()
                    .find(|l| l.id == id)
                    .ok_or_else(|| TaskError::NotFound(id.to_string()))?;
                l.name = name.to_string();
                if let Some(c) = color {
                    l.color = c.to_string();
                }
                Ok(l.clone())
            }
            None => {
                if m.labels.iter().any(|l| l.name.eq_ignore_ascii_case(name)) {
                    return Err(TaskError::invalid(format!(
                        "A label named \"{name}\" already exists."
                    )));
                }
                let l = Label {
                    id: tandem_core::ids::new_id("lbl"),
                    name: name.to_string(),
                    color: color.unwrap_or("#64748b").to_string(),
                };
                m.labels.push(l.clone());
                Ok(l)
            }
        })
    }

    // ── issues ──────────────────────────────────────────────────────────

    fn number_of(id: &str) -> Result<u64, TaskError> {
        parse_issue_id(id)
            .ok_or_else(|| TaskError::invalid(format!("\"{id}\" isn't an issue id (TAN-<n>).")))
    }

    pub fn get(&self, id: &str) -> Result<Issue, TaskError> {
        let n = Self::number_of(id)?;
        let path = self.issue_path(n);
        if !path.exists() {
            return Err(TaskError::NotFound(id.to_string()));
        }
        Self::read_json(&path)
    }

    /// Every issue on disk, in no particular order.
    pub fn all(&self) -> Result<Vec<Issue>, TaskError> {
        let mut out = Vec::new();
        for entry in fs::read_dir(self.issues_dir())? {
            let entry = entry?;
            let name = entry.file_name();
            let name = name.to_string_lossy();
            let Some(n) = name
                .strip_prefix("ISSUE-")
                .and_then(|r| r.strip_suffix(".json"))
            else {
                continue;
            };
            if n.parse::<u64>().is_err() {
                continue;
            }
            match Self::read_json::<Issue>(&entry.path()) {
                Ok(i) => out.push(i),
                Err(e) => {
                    tracing::warn!(code = "tasks.read_failed", error = %e, "skipped an issue file")
                }
            }
        }
        Ok(out)
    }

    pub fn list(
        &self,
        filter: &IssueFilter,
        cursor: Option<&str>,
        limit: Option<usize>,
    ) -> Result<Page<IssueSummary>, TaskError> {
        let limit = limit.unwrap_or(PAGE_MAX).clamp(1, PAGE_MAX);
        let after = cursor.map(decode_cursor).transpose()?;
        let text = filter
            .text
            .as_deref()
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .map(str::to_lowercase);
        let mut issues: Vec<Issue> = self
            .all()?
            .into_iter()
            .filter(|i| filter.include_archived.unwrap_or(false) || !i.archived)
            .filter(|i| filter.status.as_ref().is_none_or(|s| s.contains(&i.status)))
            .filter(|i| {
                filter
                    .assignee
                    .as_ref()
                    .is_none_or(|a| i.assignee.as_ref() == Some(a))
            })
            .filter(|i| filter.label.as_ref().is_none_or(|l| i.labels.contains(l)))
            .filter(|i| {
                filter
                    .project
                    .as_ref()
                    .is_none_or(|p| i.project.as_ref() == Some(p))
            })
            .filter(|i| {
                text.as_ref().is_none_or(|t| {
                    i.title.to_lowercase().contains(t)
                        || i.description.to_lowercase().contains(t)
                        || i.id.to_lowercase() == *t
                })
            })
            .collect();
        // Newest first; ties by number. The cursor is a position in this
        // order, so issues added meanwhile (newer) never shift later pages.
        issues.sort_by(|a, b| {
            b.updated_at
                .cmp(&a.updated_at)
                .then(b.number.cmp(&a.number))
        });
        let start = match after {
            Some(pos) => issues
                .iter()
                .position(|i| (i.updated_at, i.number) < (pos.updated_at, pos.number))
                .unwrap_or(issues.len()),
            None => 0,
        };
        let page: Vec<&Issue> = issues.iter().skip(start).take(limit).collect();
        let next_cursor = if start + page.len() < issues.len() {
            page.last().map(|i| {
                encode_cursor(&Cursor {
                    sort: "updated_desc".into(),
                    after: CursorPos {
                        updated_at: i.updated_at,
                        number: i.number,
                    },
                })
            })
        } else {
            None
        };
        Ok(Page {
            items: page.into_iter().map(IssueSummary::from).collect(),
            next_cursor,
        })
    }

    fn check_refs(meta: &Meta, req: &SaveIssue) -> Result<(), TaskError> {
        if let Some(labels) = &req.labels {
            for l in labels {
                if !meta.labels.iter().any(|x| &x.id == l) {
                    return Err(TaskError::invalid(format!(
                        "Label \"{l}\" doesn't exist; list_issue_labels shows the ids."
                    )));
                }
            }
        }
        if let Some(Some(p)) = &req.project {
            if !meta.projects.iter().any(|x| &x.id == p) {
                return Err(TaskError::invalid(format!(
                    "Project \"{p}\" doesn't exist; list_projects shows the ids."
                )));
            }
        }
        if let Some(Some(e)) = &req.suggested_engine {
            if !valid_engine_id(e) {
                return Err(TaskError::invalid(
                    "suggested_engine is an engine id: lowercase letters, digits and -.",
                ));
            }
        }
        if let Some(Some(a)) = &req.assignee {
            if !meta.actors.iter().any(|x| &x.id == a) {
                return Err(TaskError::invalid(format!(
                    "Actor \"{a}\" doesn't exist; list_actors shows the ids."
                )));
            }
        }
        Ok(())
    }

    fn check_fields(req: &SaveIssue) -> Result<(), TaskError> {
        if let Some(t) = &req.title {
            let n = t.trim().chars().count();
            if n == 0 || n > TITLE_MAX {
                return Err(TaskError::invalid("A title is 1–200 characters."));
            }
        }
        if req
            .description
            .as_ref()
            .is_some_and(|d| d.chars().count() > DESCRIPTION_MAX)
        {
            return Err(TaskError::invalid(
                "A description is at most 50,000 characters.",
            ));
        }
        if req
            .result
            .as_ref()
            .is_some_and(|d| d.chars().count() > DESCRIPTION_MAX)
        {
            return Err(TaskError::invalid("A result is at most 50,000 characters."));
        }
        if req.priority.is_some_and(|p| p > 4) {
            return Err(TaskError::invalid("Priority is 0 (none) to 4 (low)."));
        }
        if req
            .idempotency_key
            .as_ref()
            .is_some_and(|k| k.is_empty() || k.len() > IDEMPOTENCY_KEY_MAX)
        {
            return Err(TaskError::invalid(
                "An idempotency key is 1–128 characters.",
            ));
        }
        Ok(())
    }

    /// Runs `write` once per idempotency key within the window: a repeat
    /// returns the first result without writing (F-TK-01 AC2).
    fn idempotent<T: Serialize + for<'de> Deserialize<'de>>(
        &self,
        key: Option<&str>,
        write: impl FnOnce() -> Result<T, TaskError>,
    ) -> Result<T, TaskError> {
        let Some(key) = key else {
            return write();
        };
        let _g = lock::acquire(&self.lock_path("idempotency"))?;
        let path = self.dir.join("idempotency.jsonl");
        let cutoff = self.now() - IDEMPOTENCY_WINDOW_MS;
        let seen = Self::read_lines::<IdempotencyEntry>(&path)?
            .into_iter()
            .rev()
            .find(|e| e.key == key && e.ts >= cutoff);
        if let Some(entry) = seen {
            return serde_json::from_value(entry.result)
                .map_err(|e| TaskError::Corrupt(e.to_string()));
        }
        let out = write()?;
        let result = serde_json::to_value(&out).map_err(|e| TaskError::Corrupt(e.to_string()))?;
        Self::append_line(
            &path,
            &IdempotencyEntry {
                key: key.to_string(),
                ts: self.now(),
                result,
            },
        )?;
        Ok(out)
    }

    fn compact_idempotency(&self) -> Result<(), TaskError> {
        let path = self.dir.join("idempotency.jsonl");
        if !path.exists() {
            return Ok(());
        }
        let _g = lock::acquire(&self.lock_path("idempotency"))?;
        let cutoff = self.now() - IDEMPOTENCY_WINDOW_MS;
        let all = Self::read_lines::<IdempotencyEntry>(&path)?;
        let keep: Vec<_> = all.iter().filter(|e| e.ts >= cutoff).collect();
        if keep.len() == all.len() {
            return Ok(());
        }
        let mut body = Vec::new();
        for e in keep {
            body.extend(serde_json::to_vec(e).map_err(|e| TaskError::Corrupt(e.to_string()))?);
            body.push(b'\n');
        }
        atomic_write(&path, &body, Some(0o600))?;
        Ok(())
    }

    /// Creates an issue (no `id`) or patches one at `expected_revision`
    /// (F-TK-01 AC1). Every changed field goes to the history (AC3).
    pub fn save_issue(&self, req: SaveIssue, by: &Writer) -> Result<Issue, TaskError> {
        Self::check_fields(&req)?;
        let key = req.idempotency_key.clone();
        self.idempotent(key.as_deref(), || match req.id.clone() {
            None => self.create_issue(req, by),
            Some(id) => self.patch_issue(&id, req, by),
        })
    }

    fn create_issue(&self, req: SaveIssue, by: &Writer) -> Result<Issue, TaskError> {
        let title = req
            .title
            .as_deref()
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .ok_or_else(|| TaskError::invalid("A new issue needs a title."))?
            .to_string();
        if req.expected_revision.is_some() {
            return Err(TaskError::invalid(
                "expected_revision is only for updates; leave it out when creating.",
            ));
        }
        let number = self.update_meta(|m| {
            Self::check_refs(m, &req)?;
            let n = m.next_number;
            m.next_number += 1;
            Ok(n)
        })?;
        let now = self.now();
        let issue = Issue {
            id: issue_id(number),
            number,
            revision: 1,
            title,
            description: req.description.unwrap_or_default(),
            status: req.status.unwrap_or(IssueStatus::Todo),
            priority: req.priority.unwrap_or(0),
            labels: req.labels.unwrap_or_default(),
            project: req.project.flatten(),
            assignee: req.assignee.flatten(),
            created_at: now,
            updated_at: now,
            archived: false,
            result: req.result,
            attempts: Vec::new(),
            suggested_engine: req.suggested_engine.flatten(),
        };
        let _g = lock::acquire(&self.lock_path(&format!("ISSUE-{number}")))?;
        self.write_json(&self.issue_path(number), &issue)?;
        self.append_history(
            number,
            by,
            now,
            "created",
            Value::Null,
            json!({"title": issue.title, "status": issue.status}),
        )?;
        Ok(issue)
    }

    fn append_history(
        &self,
        number: u64,
        by: &Writer,
        ts: i64,
        field: &str,
        from: Value,
        to: Value,
    ) -> Result<(), TaskError> {
        Self::append_line(
            &self.history_path(number),
            &HistoryEntry {
                ts,
                actor: by.actor.clone(),
                connection: by.connection.clone(),
                field: field.to_string(),
                from,
                to,
            },
        )
    }

    /// Reads, checks the revision, applies `edit`, bumps the revision and
    /// records each changed field, all under the issue's lock.
    fn edit_issue(
        &self,
        id: &str,
        expected_revision: Option<u64>,
        by: &Writer,
        edit: impl FnOnce(&mut Issue) -> Result<(), TaskError>,
    ) -> Result<Issue, TaskError> {
        let number = Self::number_of(id)?;
        let _g = lock::acquire(&self.lock_path(&format!("ISSUE-{number}")))?;
        let path = self.issue_path(number);
        if !path.exists() {
            return Err(TaskError::NotFound(id.to_string()));
        }
        let before: Issue = Self::read_json(&path)?;
        if let Some(expected) = expected_revision {
            if expected != before.revision {
                return Err(TaskError::RevisionConflict {
                    id: before.id,
                    current_revision: before.revision,
                });
            }
        }
        let mut after = before.clone();
        edit(&mut after)?;
        let changes = diff_fields(&before, &after);
        if changes.is_empty() {
            return Ok(before);
        }
        let now = self.now();
        after.revision = before.revision + 1;
        after.updated_at = now.max(before.updated_at + 1);
        self.write_json(&path, &after)?;
        for (field, from, to) in changes {
            self.append_history(number, by, now, field, from, to)?;
        }
        Ok(after)
    }

    fn patch_issue(&self, id: &str, req: SaveIssue, by: &Writer) -> Result<Issue, TaskError> {
        let expected = req.expected_revision.ok_or_else(|| {
            TaskError::invalid("expected_revision is required when updating an issue.")
        })?;
        {
            let meta = self.meta()?;
            Self::check_refs(&meta, &req)?;
        }
        self.edit_issue(id, Some(expected), by, |i| {
            if let Some(t) = req.title {
                i.title = t.trim().to_string();
            }
            if let Some(d) = req.description {
                i.description = d;
            }
            if let Some(s) = req.status {
                i.status = s;
            }
            if let Some(p) = req.priority {
                i.priority = p;
            }
            if let Some(l) = req.labels {
                i.labels = l;
            }
            if let Some(p) = req.project {
                i.project = p;
            }
            if let Some(a) = req.assignee {
                i.assignee = a;
            }
            if let Some(e) = req.suggested_engine {
                i.suggested_engine = e;
            }
            if let Some(r) = req.result {
                i.result = Some(r);
            }
            Ok(())
        })
    }

    pub fn set_archived(
        &self,
        id: &str,
        archived: bool,
        expected_revision: Option<u64>,
        by: &Writer,
    ) -> Result<Issue, TaskError> {
        self.edit_issue(id, expected_revision, by, |i| {
            i.archived = archived;
            Ok(())
        })
    }

    /// Adds or replaces one attempt (the app's execution service; no
    /// revision check, it owns the attempt).
    pub fn put_attempt(
        &self,
        id: &str,
        attempt: AttemptRecord,
        by: &Writer,
    ) -> Result<Issue, TaskError> {
        self.edit_issue(id, None, by, |i| {
            match i.attempts.iter_mut().find(|a| a.id == attempt.id) {
                Some(a) => *a = attempt,
                None => i.attempts.push(attempt),
            }
            Ok(())
        })
    }

    /// Sets fields without a revision check: the execution service moving an
    /// issue along (status, result).
    pub fn force_update(
        &self,
        id: &str,
        status: Option<IssueStatus>,
        result: Option<String>,
        by: &Writer,
    ) -> Result<Issue, TaskError> {
        self.edit_issue(id, None, by, |i| {
            if let Some(s) = status {
                i.status = s;
            }
            if let Some(r) = result {
                i.result = Some(r);
            }
            Ok(())
        })
    }

    // ── comments and history ────────────────────────────────────────────

    pub fn add_comment(
        &self,
        id: &str,
        body: &str,
        parent_id: Option<&str>,
        idempotency_key: Option<&str>,
        by: &Writer,
    ) -> Result<Comment, TaskError> {
        let n = body.chars().count();
        if body.trim().is_empty() || n > COMMENT_MAX {
            return Err(TaskError::invalid("A comment is 1–20,000 characters."));
        }
        if idempotency_key.is_some_and(|k| k.is_empty() || k.len() > IDEMPOTENCY_KEY_MAX) {
            return Err(TaskError::invalid(
                "An idempotency key is 1–128 characters.",
            ));
        }
        self.idempotent(idempotency_key, || {
            let number = Self::number_of(id)?;
            let _g = lock::acquire(&self.lock_path(&format!("ISSUE-{number}")))?;
            if !self.issue_path(number).exists() {
                return Err(TaskError::NotFound(id.to_string()));
            }
            let path = self.comments_path(number);
            if let Some(p) = parent_id {
                let known = Self::read_lines::<Comment>(&path)?
                    .iter()
                    .any(|c| c.id == p);
                if !known {
                    return Err(TaskError::invalid(format!(
                        "Comment \"{p}\" isn't on {id}."
                    )));
                }
            }
            let comment = Comment {
                id: tandem_core::ids::new_id("cmt"),
                issue_id: issue_id(number),
                ts: self.now(),
                actor: by.actor.clone(),
                body: body.to_string(),
                parent_id: parent_id.map(str::to_string),
            };
            Self::append_line(&path, &comment)?;
            Ok(comment)
        })
    }

    pub fn comments(&self, id: &str) -> Result<Vec<Comment>, TaskError> {
        let n = Self::number_of(id)?;
        if !self.issue_path(n).exists() {
            return Err(TaskError::NotFound(id.to_string()));
        }
        Self::read_lines(&self.comments_path(n))
    }

    pub fn history(&self, id: &str) -> Result<Vec<HistoryEntry>, TaskError> {
        let n = Self::number_of(id)?;
        if !self.issue_path(n).exists() {
            return Err(TaskError::NotFound(id.to_string()));
        }
        Self::read_lines(&self.history_path(n))
    }

    // ── connections (F-TK-02) ───────────────────────────────────────────

    fn connections_path(&self) -> PathBuf {
        self.dir.join("connections.json")
    }

    pub fn connections(&self) -> Result<Vec<Connection>, TaskError> {
        let path = self.connections_path();
        if !path.exists() {
            return Ok(Vec::new());
        }
        let map: HashMap<String, Connection> = Self::read_json(&path)?;
        let mut out: Vec<Connection> = map.into_values().collect();
        out.sort_by_key(|c| c.created_at);
        Ok(out)
    }

    fn edit_connections<R>(
        &self,
        f: impl FnOnce(&mut HashMap<String, Connection>) -> Result<R, TaskError>,
    ) -> Result<R, TaskError> {
        let _g = lock::acquire(&self.lock_path("connections"))?;
        let path = self.connections_path();
        let mut map: HashMap<String, Connection> = if path.exists() {
            Self::read_json(&path)?
        } else {
            HashMap::new()
        };
        let out = f(&mut map)?;
        self.write_json(&path, &map)?;
        Ok(out)
    }

    /// A connection for one engine session; its writes are recorded as
    /// `actor`.
    pub fn create_connection(&self, label: &str, actor: &str) -> Result<Connection, TaskError> {
        self.create_connection_in(label, actor, None)
    }

    /// A connection whose session works in `scope`; it can claim files
    /// there (F-TK-05).
    pub fn create_connection_in(
        &self,
        label: &str,
        actor: &str,
        scope: Option<ClaimScope>,
    ) -> Result<Connection, TaskError> {
        let conn = Connection {
            id: tandem_core::ids::new_id("tc"),
            created_at: self.now(),
            label: label.to_string(),
            actor: actor.to_string(),
            revoked_at: None,
            scope,
        };
        let out = conn.clone();
        self.edit_connections(move |m| {
            m.insert(conn.id.clone(), conn);
            Ok(())
        })?;
        Ok(out)
    }

    pub fn revoke_connection(&self, id: &str) -> Result<(), TaskError> {
        let now = self.now();
        self.edit_connections(|m| {
            let c = m
                .get_mut(id)
                .ok_or_else(|| TaskError::NotFound(id.to_string()))?;
            c.revoked_at.get_or_insert(now);
            Ok(())
        })
    }

    /// The live connection, or `CONNECTION_REVOKED` for a revoked or unknown
    /// one.
    pub fn connection(&self, id: &str) -> Result<Connection, TaskError> {
        let path = self.connections_path();
        if !path.exists() {
            return Err(TaskError::ConnectionRevoked);
        }
        let map: HashMap<String, Connection> = Self::read_json(&path)?;
        match map.get(id) {
            Some(c) if c.revoked_at.is_none() => Ok(c.clone()),
            _ => Err(TaskError::ConnectionRevoked),
        }
    }

    /// Revoked connections older than `keep_ms` are dropped.
    pub fn prune_connections(&self, keep_ms: i64) -> Result<(), TaskError> {
        let cutoff = self.now() - keep_ms;
        self.edit_connections(|m| {
            m.retain(|_, c| c.revoked_at.is_none_or(|t| t >= cutoff));
            Ok(())
        })
    }

    /// Counts per status (non-archived).
    pub fn counts_by_status(&self) -> Result<HashMap<&'static str, usize>, TaskError> {
        let mut out: HashMap<&'static str, usize> =
            IssueStatus::ALL.iter().map(|s| (s.id(), 0)).collect();
        for i in self.all()?.into_iter().filter(|i| !i.archived) {
            *out.entry(i.status.id()).or_default() += 1;
        }
        Ok(out)
    }
}

fn diff_fields(a: &Issue, b: &Issue) -> Vec<(&'static str, Value, Value)> {
    let mut out = Vec::new();
    macro_rules! field {
        ($name:ident) => {
            if a.$name != b.$name {
                out.push((stringify!($name), json!(a.$name), json!(b.$name)));
            }
        };
    }
    field!(title);
    field!(description);
    field!(status);
    field!(priority);
    field!(labels);
    field!(project);
    field!(assignee);
    field!(suggested_engine);
    field!(archived);
    field!(result);
    if a.attempts != b.attempts {
        let ids = |v: &Vec<AttemptRecord>| {
            v.iter()
                .map(|x| json!({"id": x.id, "state": x.state}))
                .collect::<Vec<_>>()
        };
        out.push(("attempts", json!(ids(&a.attempts)), json!(ids(&b.attempts))));
    }
    out
}

fn encode_cursor(c: &Cursor) -> String {
    let raw = serde_json::to_vec(c).unwrap_or_default();
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(raw)
}

fn decode_cursor(raw: &str) -> Result<CursorPos, TaskError> {
    let bad = || TaskError::invalid("That cursor isn't one list_issues returned.");
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(raw)
        .map_err(|_| bad())?;
    let c: Cursor = serde_json::from_slice(&bytes).map_err(|_| bad())?;
    if c.sort != "updated_desc" {
        return Err(bad());
    }
    Ok(c.after)
}

/// Pages through a simple list by offset cursor (comments, history).
pub fn page_by_offset<T: Clone>(
    items: &[T],
    cursor: Option<&str>,
    limit: Option<usize>,
) -> Result<(Vec<T>, Option<String>), TaskError> {
    let limit = limit.unwrap_or(LIST_PAGE_MAX).clamp(1, LIST_PAGE_MAX);
    let start = match cursor {
        None => 0,
        Some(c) => {
            let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
                .decode(c)
                .map_err(|_| TaskError::invalid("That cursor isn't one this tool returned."))?;
            String::from_utf8_lossy(&bytes)
                .strip_prefix("offset:")
                .and_then(|n| n.parse::<usize>().ok())
                .ok_or_else(|| TaskError::invalid("That cursor isn't one this tool returned."))?
        }
    };
    let page: Vec<T> = items.iter().skip(start).take(limit).cloned().collect();
    let end = start + page.len();
    let next = (end < items.len())
        .then(|| base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(format!("offset:{end}")));
    Ok((page, next))
}
