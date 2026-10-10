//! The Usage page (F-AC-07, UI-37): tokens from the engines' own local logs
//! across every account, by day, engine, account and workspace. Built on
//! this machine from files the engines already write; nothing is sent.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use chrono::{Duration, FixedOffset, NaiveDate};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use tandem_engines::registry::EngineRegistry;
use tandem_engines::usage_log::{self, LogSource, UsageDay, UsageScan};
use tandem_store::repo::accounts::Account;
use tandem_store::repo::workspaces::Workspace;

/// The heatmap's span.
pub const WEEKS: i64 = 12;
/// How many workspaces the page lists.
const TOP_WORKSPACES: usize = 8;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct EngineUsage {
    pub engine_id: String,
    pub label: String,
    pub tokens: u64,
    pub sessions: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AccountUsage {
    pub account_id: String,
    pub engine_id: String,
    pub label: String,
    pub tokens: u64,
    pub sessions: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct WorkspaceUsage {
    /// The workspace's name, or the folder's own name when it isn't one.
    pub name: String,
    pub path: String,
    pub tokens: u64,
}

/// `usage_report`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct UsageReport {
    pub since: String,
    pub until: String,
    pub total: u64,
    pub days: Vec<UsageDay>,
    /// Engines with any usage, most first.
    pub engines: Vec<EngineUsage>,
    pub accounts: Vec<AccountUsage>,
    pub workspaces: Vec<WorkspaceUsage>,
    pub skipped_lines: u64,
    pub files_read: u64,
    /// Engines whose logs Vibezeo can't read (no `usage_log_globs`).
    pub unsupported: Vec<String>,
}

/// One source per account of an engine that writes usage logs.
pub fn sources(accounts: &[Account], registry: &EngineRegistry) -> Vec<LogSource> {
    accounts
        .iter()
        .filter_map(|a| {
            let spec = registry.get(&a.engine_id)?;
            if spec.usage_log_globs.is_empty() || a.home_path.is_empty() {
                return None;
            }
            Some(LogSource {
                engine_id: a.engine_id.clone(),
                account_id: a.id.as_str().to_string(),
                home: PathBuf::from(&a.home_path),
                globs: spec.usage_log_globs.clone(),
            })
        })
        .collect()
}

/// The workspace a folder belongs to: the deepest workspace root containing it.
fn workspace_of<'a>(folder: &str, workspaces: &'a [Workspace]) -> Option<&'a Workspace> {
    let path = Path::new(folder);
    workspaces
        .iter()
        .filter(|w| w.kind == "local" && path.starts_with(&w.root))
        .max_by_key(|w| w.root.len())
}

pub fn build(
    scan: &UsageScan,
    since: NaiveDate,
    until: NaiveDate,
    accounts: &[Account],
    workspaces: &[Workspace],
    registry: &EngineRegistry,
) -> UsageReport {
    let label = |id: &str| {
        registry
            .get(id)
            .map(|s| s.label.clone())
            .unwrap_or_else(|| id.to_string())
    };
    let mut engines: BTreeMap<String, (u64, u64)> = BTreeMap::new();
    let mut by_account: BTreeMap<(String, String), u64> = BTreeMap::new();
    let mut folders: HashMap<(String, String), u64> = HashMap::new();
    for ((_, engine, account, folder), tally) in &scan.cells {
        let t = tally.total();
        engines.entry(engine.clone()).or_default().0 += t;
        *by_account
            .entry((engine.clone(), account.clone()))
            .or_default() += t;
        let (name, path) = match workspace_of(folder, workspaces) {
            Some(w) => (w.name.clone(), w.root.clone()),
            None => (
                Path::new(folder)
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "Unknown folder".into()),
                folder.clone(),
            ),
        };
        *folders.entry((name, path)).or_default() += t;
    }
    for ((engine, _), sessions) in &scan.sessions {
        if let Some(e) = engines.get_mut(engine) {
            e.1 += sessions.len() as u64;
        }
    }
    let mut engines: Vec<EngineUsage> = engines
        .into_iter()
        .filter(|(_, (t, _))| *t > 0)
        .map(|(id, (tokens, sessions))| EngineUsage {
            label: label(&id),
            engine_id: id,
            tokens,
            sessions,
        })
        .collect();
    engines.sort_by_key(|e| std::cmp::Reverse(e.tokens));
    let mut accounts_out: Vec<AccountUsage> = by_account
        .into_iter()
        .map(|((engine, account), tokens)| AccountUsage {
            label: accounts
                .iter()
                .find(|a| a.id.as_str() == account)
                .map(|a| a.label.clone())
                .unwrap_or_else(|| account.clone()),
            sessions: scan
                .sessions
                .get(&(engine.clone(), account.clone()))
                .map_or(0, |s| s.len() as u64),
            account_id: account,
            engine_id: engine,
            tokens,
        })
        .collect();
    accounts_out.sort_by_key(|a| std::cmp::Reverse(a.tokens));
    let mut ws: Vec<WorkspaceUsage> = folders
        .into_iter()
        .map(|((name, path), tokens)| WorkspaceUsage { name, path, tokens })
        .collect();
    ws.sort_by(|a, b| b.tokens.cmp(&a.tokens).then(a.name.cmp(&b.name)));
    ws.truncate(TOP_WORKSPACES);
    let days = usage_log::days(scan, since, until);
    let mut unsupported: Vec<String> = accounts
        .iter()
        .filter(|a| {
            registry
                .get(&a.engine_id)
                .is_some_and(|s| s.usage_log_globs.is_empty())
        })
        .map(|a| label(&a.engine_id))
        .collect();
    unsupported.sort();
    unsupported.dedup();
    UsageReport {
        since: since.format("%Y-%m-%d").to_string(),
        until: until.format("%Y-%m-%d").to_string(),
        total: days.iter().map(|d| d.tokens).sum(),
        days,
        engines,
        accounts: accounts_out,
        workspaces: ws,
        skipped_lines: scan.skipped_lines,
        files_read: scan.files_read,
        unsupported,
    }
}

/// Scans and builds the report for the 12 weeks up to `today` (in `tz`).
pub fn report(
    accounts: &[Account],
    workspaces: &[Workspace],
    registry: &EngineRegistry,
    today: NaiveDate,
    tz: FixedOffset,
) -> UsageReport {
    let since = today - Duration::days(WEEKS * 7 - 1);
    let scan = usage_log::scan(&sources(accounts, registry), since, tz);
    build(&scan, since, today, accounts, workspaces, registry)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tandem_core::ids::{AccountId, WorkspaceId};

    fn account(id: &str, engine: &str, label: &str, home: &Path) -> Account {
        Account {
            id: AccountId::from_db(id.into()),
            engine_id: engine.into(),
            label: label.into(),
            home_path: home.to_string_lossy().into_owned(),
            is_default: true,
            plan_label: None,
            created_at: 0,
        }
    }

    fn workspace(root: &str, name: &str) -> Workspace {
        Workspace {
            id: WorkspaceId::new(),
            kind: "local".into(),
            host_id: "local".into(),
            root: root.into(),
            wsl_distro: None,
            name: name.into(),
            parent_id: None,
            lane_branch: None,
            lane_base: None,
            sort_key: "a".into(),
            pinned: false,
            collapsed: false,
            sessions_collapsed: false,
            created_at: 0,
        }
    }

    #[test]
    fn f_ac_07_1_report_totals_by_day_engine_account_and_workspace() {
        let dir = tempfile::tempdir().expect("tmp");
        let home = dir.path().join("claude-work");
        let p = home.join("projects/x");
        std::fs::create_dir_all(&p).expect("mkdir");
        let line = |ts: &str, id: &str, n: u64, cwd: &str| {
            serde_json::json!({"timestamp": ts, "cwd": cwd, "sessionId": "s1",
                "message": {"id": id, "usage": {"input_tokens": n, "output_tokens": 0}}})
            .to_string()
        };
        std::fs::write(
            p.join("a.jsonl"),
            [
                line("2026-10-07T10:00:00Z", "a", 100, "/home/ana/shop/web"),
                line("2026-10-08T10:00:00Z", "b", 50, "/home/ana/shop"),
                line("2026-10-09T10:00:00Z", "c", 25, "/elsewhere/tool"),
            ]
            .join("\n"),
        )
        .expect("write");
        let accounts = vec![account("acc_1", "claude", "Work", &home)];
        let ws = vec![workspace("/home/ana/shop", "Shop")];
        let utc = FixedOffset::east_opt(0).expect("utc");
        let today = NaiveDate::from_ymd_opt(2026, 10, 9).expect("date");
        let r = report(&accounts, &ws, &EngineRegistry::bundled(), today, utc);
        assert_eq!(r.days.len(), 84);
        assert_eq!(r.total, 175);
        assert_eq!(r.days.last().map(|d| d.tokens), Some(25));
        assert_eq!(r.engines.len(), 1);
        assert_eq!(
            (
                r.engines[0].label.as_str(),
                r.engines[0].tokens,
                r.engines[0].sessions
            ),
            ("Claude Code", 175, 1)
        );
        assert_eq!(r.accounts[0].label, "Work");
        assert_eq!(
            r.workspaces
                .iter()
                .map(|w| (w.name.as_str(), w.tokens))
                .collect::<Vec<_>>(),
            vec![("Shop", 150), ("tool", 25)]
        );
    }
}
