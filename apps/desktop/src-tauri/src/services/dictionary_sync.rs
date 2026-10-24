//! Dictionary sync (F-DI-09, D-523, ARCHITECTURE §13): the rules only, to
//! and from `POST /v1/dictionary/sync`, when the user has turned on
//! **Sync dictionary** on a plan that includes it. Transcript evidence,
//! history and audio never leave; neither does rule text in the logs.
//!
//! A rule whose `sync_rev` is 0 changed here and waits to go up. A round
//! sends up to 500 of them with the last server rev this device saw; the
//! server keeps the newest write per rule and hands back what changed since.
//! A local change older than the server's newer one comes back as a
//! conflict, which waits for the user's "keep mine" or "keep theirs".

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use chrono::{DateTime, TimeZone, Utc};
use futures::future::BoxFuture;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use tandem_core::{ErrorCode, IpcError};
use tandem_store::repo::dictate::{self as repo, RuleRow};
use tandem_store::{Store, StoreError};
use tandem_voice::dictionary::{Rule, RuleKind, Spacing};

use crate::services::api::ApiError;
use crate::services::dictate::{row_from_rule, rule_from_row};

/// The server's batch limit.
pub const BATCH: usize = 500;
/// Rounds per sync: 10,000 changed rules is far past any real dictionary.
const MAX_ROUNDS: usize = 20;

/// What syncs of a rule: its words and options. No id (that's `rule_id`),
/// no timestamps (`updated_at` travels beside it) and nothing heard.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RulePayload {
    pub kind: RuleKind,
    pub replacement: String,
    pub phrases: Vec<String>,
    pub match_case: bool,
    pub protect: bool,
    pub spacing: Spacing,
    pub enabled: bool,
    pub lang: String,
    pub app_profiles: Vec<String>,
}

/// One rule change on the wire.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WireChange {
    pub rule_id: String,
    pub payload: RulePayload,
    pub deleted: bool,
    /// RFC 3339, millisecond precision.
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WirePulled {
    pub rule_id: String,
    /// Kept loose so one rule from a newer app doesn't fail the whole pull.
    pub payload: serde_json::Value,
    pub deleted: bool,
    pub updated_at: String,
    pub rev: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WireConflict {
    pub rule_id: String,
    pub remote: WirePulled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncRequest {
    pub since_rev: i64,
    pub changes: Vec<WireChange>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncResponse {
    pub rev: i64,
    pub changes: Vec<WirePulled>,
    #[serde(default)]
    pub conflicts: Vec<WireConflict>,
}

/// One `POST /v1/dictionary/sync`, with a fresh token.
pub type SyncCall =
    Arc<dyn Fn(SyncRequest) -> BoxFuture<'static, Result<SyncResponse, ApiError>> + Send + Sync>;

/// Who may sync right now: a hosted build, a signed-in account, a plan
/// that includes it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SyncAccess {
    pub hosted: bool,
    pub allowed: bool,
    /// The signed-in account (its email); `None` when signed out.
    pub account: Option<String>,
}

pub type AccessFn = Arc<dyn Fn() -> SyncAccess + Send + Sync>;
/// One `DELETE /v1/dictionary`, with a fresh token.
pub type ForgetCall = Arc<dyn Fn() -> BoxFuture<'static, Result<(), ApiError>> + Send + Sync>;

/// What the voice service needs to sync (set once at startup).
pub struct SyncDeps {
    pub access: AccessFn,
    pub call: SyncCall,
    pub forget: ForgetCall,
}

/// What a failed sync says.
pub fn api_error(e: &ApiError) -> IpcError {
    match e.status() {
        Some(403) => IpcError::new(
            ErrorCode::ForbiddenPlan,
            "Dictionary sync comes with Vibezeo accounts. It uses Vibezeo's servers and switches on when Vibezeo accounts open.",
        ),
        Some(401) => IpcError::new(
            ErrorCode::Unauthenticated,
            "Sign in to sync your dictionary.",
        ),
        _ => IpcError::new(
            ErrorCode::Unavailable,
            "Couldn't sync the dictionary right now. Your rules are safe on this computer.",
        ),
    }
}

/// Refuses before anything leaves: the build, the account, then the plan.
pub fn check(access: &SyncAccess) -> Result<String, IpcError> {
    if !access.hosted {
        return Err(IpcError::new(
            ErrorCode::Unavailable,
            "Dictionary sync needs a Vibezeo account.",
        ));
    }
    let Some(account) = access.account.clone() else {
        return Err(IpcError::new(
            ErrorCode::Unauthenticated,
            "Sign in to sync your dictionary.",
        ));
    };
    if !access.allowed {
        return Err(IpcError::new(
            ErrorCode::ForbiddenPlan,
            "Dictionary sync comes with Vibezeo accounts. It uses Vibezeo's servers and switches on when Vibezeo accounts open.",
        ));
    }
    Ok(account)
}

/// A rule both sides changed: the user keeps one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SyncConflict {
    pub id: String,
    pub local: Rule,
    pub remote: Rule,
}

/// `dictionary_sync`'s answer (ARCHITECTURE §12.5).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SyncResult {
    pub pushed: u32,
    pub pulled: u32,
    pub conflicts: Vec<SyncConflict>,
    #[ts(type = "number")]
    pub synced_at: i64,
}

/// What this device remembers between syncs: whose dictionary it last
/// synced and the server rev it had seen.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SyncCursor {
    pub account: String,
    pub rev: i64,
}

/// A conflict's server side, kept until the user picks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pending {
    pub remote: Rule,
    pub rev: i64,
}

#[derive(Debug, thiserror::Error)]
pub enum SyncError {
    #[error(transparent)]
    Api(#[from] ApiError),
    #[error(transparent)]
    Store(#[from] StoreError),
}

pub fn iso(ms: i64) -> String {
    Utc.timestamp_millis_opt(ms)
        .single()
        .unwrap_or_default()
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

pub fn parse_ms(s: &str) -> Option<i64> {
    DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|d| d.timestamp_millis())
}

pub fn payload_of(row: &RuleRow) -> RulePayload {
    let r = rule_from_row(row.clone());
    RulePayload {
        kind: r.kind,
        replacement: r.replacement,
        phrases: r.phrases,
        match_case: r.match_case,
        protect: r.protect,
        spacing: r.spacing,
        enabled: r.enabled,
        lang: r.lang,
        app_profiles: r.app_profiles,
    }
}

pub fn change_of(row: &RuleRow) -> WireChange {
    WireChange {
        rule_id: row.id.clone(),
        payload: payload_of(row),
        deleted: row.deleted_at.is_some(),
        updated_at: iso(row.updated_at),
    }
}

/// A pulled change as a stored row, or `None` when it doesn't parse.
pub fn row_of(p: &WirePulled) -> Option<RuleRow> {
    let payload: RulePayload = serde_json::from_value(p.payload.clone()).ok()?;
    let updated_at = parse_ms(&p.updated_at)?;
    if p.rule_id.is_empty() || p.rule_id.len() > 200 {
        return None;
    }
    let rule = Rule {
        id: p.rule_id.clone(),
        kind: payload.kind,
        replacement: payload.replacement,
        phrases: payload.phrases,
        match_case: payload.match_case,
        protect: payload.protect,
        spacing: payload.spacing,
        enabled: payload.enabled,
        lang: payload.lang,
        app_profiles: payload.app_profiles,
        updated_at,
        deleted_at: p.deleted.then_some(updated_at),
    };
    Some(row_from_rule(&rule, p.rev.max(1)))
}

/// One sync: rounds of push and pull until nothing is left to send. The
/// cursor comes back advanced past every round that completed, so a failed
/// round loses nothing.
pub async fn run(
    store: &Store,
    cursor: SyncCursor,
    account: &str,
    call: &SyncCall,
    now: i64,
) -> (
    SyncCursor,
    Result<(SyncResult, HashMap<String, Pending>), SyncError>,
) {
    let mut cursor = cursor;
    if cursor.account != account {
        // Another account's dictionary: start over and offer everything.
        if let Err(e) = repo::mark_rules_unsynced(store).await {
            return (cursor, Err(e.into()));
        }
        cursor = SyncCursor {
            account: account.to_string(),
            rev: 0,
        };
    }
    let mut result = SyncResult {
        pushed: 0,
        pulled: 0,
        conflicts: Vec::new(),
        synced_at: now,
    };
    let mut pending = HashMap::new();
    let mut held: HashSet<String> = HashSet::new();
    for _ in 0..MAX_ROUNDS {
        let rows = match repo::unsynced_rules(store).await {
            Ok(rows) => rows,
            Err(e) => return (cursor, Err(e.into())),
        };
        let batch: Vec<RuleRow> = rows
            .into_iter()
            .filter(|r| !held.contains(&r.id))
            .take(BATCH)
            .collect();
        let full = batch.len() == BATCH;
        let req = SyncRequest {
            since_rev: cursor.rev,
            changes: batch.iter().map(change_of).collect(),
        };
        let resp = match call(req).await {
            Ok(r) => r,
            Err(e) => return (cursor, Err(e.into())),
        };
        let lost: HashSet<String> = resp.conflicts.iter().map(|c| c.rule_id.clone()).collect();
        // What went up is synced, unless it changed again meanwhile.
        let sent: Vec<(String, i64)> = batch
            .iter()
            .filter(|r| !lost.contains(&r.id))
            .map(|r| (r.id.clone(), r.updated_at))
            .collect();
        result.pushed += sent.len() as u32;
        if let Err(e) = repo::mark_rules_synced(store, sent, resp.rev.max(1)).await {
            return (cursor, Err(e.into()));
        }
        let mut rows = Vec::new();
        for p in &resp.changes {
            if lost.contains(&p.rule_id) {
                continue;
            }
            let Some(row) = row_of(p) else {
                tracing::warn!(
                    code = "dictionary.sync_skipped",
                    "a pulled rule didn't parse"
                );
                continue;
            };
            let local = match repo::get_rule(store, row.id.clone()).await {
                Ok(l) => l,
                Err(e) => return (cursor, Err(e.into())),
            };
            // A newer edit made here during the round goes up next time.
            if matches!(&local, Some(l) if l.sync_rev == 0 && l.updated_at > row.updated_at) {
                continue;
            }
            rows.push(row);
        }
        result.pulled += rows.len() as u32;
        if let Err(e) = repo::put_rules(store, rows).await {
            return (cursor, Err(e.into()));
        }
        for c in &resp.conflicts {
            held.insert(c.rule_id.clone());
            let (Some(remote), Ok(Some(local))) = (
                row_of(&c.remote),
                repo::get_rule(store, c.rule_id.clone()).await,
            ) else {
                continue;
            };
            result.conflicts.push(SyncConflict {
                id: c.rule_id.clone(),
                local: rule_from_row(local),
                remote: rule_from_row(remote.clone()),
            });
            pending.insert(
                c.rule_id.clone(),
                Pending {
                    remote: rule_from_row(remote.clone()),
                    rev: c.remote.rev,
                },
            );
        }
        cursor.rev = resp.rev;
        if !full {
            break;
        }
    }
    (cursor, Ok((result, pending)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use parking_lot::Mutex;
    use tandem_core::DataDir;

    /// The API's rules (apps/api/src/services/dictionary.ts) in memory: the
    /// newest write per rule wins; an older, different change is a conflict.
    #[derive(Default)]
    struct Server {
        rows: Mutex<HashMap<String, WirePulled>>,
        rev: Mutex<i64>,
        bodies: Mutex<Vec<serde_json::Value>>,
    }

    fn call(server: Arc<Server>) -> SyncCall {
        Arc::new(move |req: SyncRequest| {
            let s = server.clone();
            Box::pin(async move {
                s.bodies
                    .lock()
                    .push(serde_json::to_value(&req).expect("json"));
                let mut rows = s.rows.lock();
                let mut rev = s.rev.lock();
                let mut written = HashSet::new();
                let mut conflicts = Vec::new();
                for c in req.changes {
                    let payload = serde_json::to_value(&c.payload).expect("json");
                    if let Some(stored) = rows.get(&c.rule_id) {
                        if parse_ms(&c.updated_at) < parse_ms(&stored.updated_at) {
                            if stored.payload != payload || stored.deleted != c.deleted {
                                conflicts.push(WireConflict {
                                    rule_id: c.rule_id.clone(),
                                    remote: stored.clone(),
                                });
                            }
                            continue;
                        }
                    }
                    *rev += 1;
                    written.insert(c.rule_id.clone());
                    rows.insert(
                        c.rule_id.clone(),
                        WirePulled {
                            rule_id: c.rule_id,
                            payload,
                            deleted: c.deleted,
                            updated_at: c.updated_at,
                            rev: *rev,
                        },
                    );
                }
                let mut changes: Vec<WirePulled> = rows
                    .values()
                    .filter(|r| r.rev > req.since_rev && !written.contains(&r.rule_id))
                    .cloned()
                    .collect();
                changes.sort_by_key(|r| r.rev);
                Ok(SyncResponse {
                    rev: *rev,
                    changes,
                    conflicts,
                })
            })
        })
    }

    struct Device {
        _dir: tempfile::TempDir,
        store: Store,
        cursor: SyncCursor,
    }

    fn device() -> Device {
        let dir = tempfile::tempdir().expect("tmp");
        let data = DataDir::at(dir.path().to_path_buf()).expect("data");
        let store = Store::open(&data).expect("store");
        Device {
            _dir: dir,
            store,
            cursor: SyncCursor::default(),
        }
    }

    impl Device {
        async fn edit(&self, id: &str, to: &str, at: i64) {
            let mut r = Rule::correction(id, id, to);
            r.updated_at = at;
            repo::put_rules(&self.store, vec![row_from_rule(&r, 0)])
                .await
                .expect("put");
        }
        async fn sync(&mut self, server: &Arc<Server>, at: i64) -> SyncResult {
            let (cursor, out) = run(
                &self.store,
                self.cursor.clone(),
                "me@tandem.test",
                &call(server.clone()),
                at,
            )
            .await;
            self.cursor = cursor;
            out.expect("sync").0
        }
        async fn to(&self, id: &str) -> Option<String> {
            repo::get_rule(&self.store, id.into())
                .await
                .expect("get")
                .filter(|r| r.deleted_at.is_none())
                .map(|r| r.replacement)
        }
    }

    #[tokio::test]
    async fn f_di_09_rules_reach_another_device_and_deletions_follow() {
        let server = Arc::new(Server::default());
        let (mut a, mut b) = (device(), device());
        a.edit("tauri", "Tauri", 1_000).await;
        a.edit("pnpm", "pnpm", 1_001).await;
        let first = a.sync(&server, 2_000).await;
        assert_eq!((first.pushed, first.pulled), (2, 0));
        let got = b.sync(&server, 2_100).await;
        assert_eq!(got.pulled, 2);
        assert_eq!(b.to("tauri").await.as_deref(), Some("Tauri"));
        // Pulled rules are synced: nothing goes back up.
        assert!(repo::unsynced_rules(&b.store).await.expect("q").is_empty());
        repo::delete_rules(&b.store, vec!["pnpm".into()])
            .await
            .expect("delete");
        assert_eq!(b.sync(&server, 2_200).await.pushed, 1);
        a.sync(&server, 2_300).await;
        assert_eq!(a.to("pnpm").await, None);
        assert_eq!(a.to("tauri").await.as_deref(), Some("Tauri"));
    }

    #[tokio::test]
    async fn f_di_09_the_request_carries_rules_only() {
        let server = Arc::new(Server::default());
        let mut a = device();
        a.edit("tauri", "Tauri", 1_000).await;
        a.sync(&server, 2_000).await;
        let body = server.bodies.lock()[0].clone();
        let keys = |v: &serde_json::Value| {
            let mut k: Vec<String> = v.as_object().expect("obj").keys().cloned().collect();
            k.sort();
            k
        };
        assert_eq!(keys(&body), ["changes", "since_rev"]);
        let change = &body["changes"][0];
        assert_eq!(
            keys(change),
            ["deleted", "payload", "rule_id", "updated_at"]
        );
        assert_eq!(
            keys(&change["payload"]),
            [
                "appProfiles",
                "enabled",
                "kind",
                "lang",
                "matchCase",
                "phrases",
                "protect",
                "replacement",
                "spacing"
            ]
        );
        assert_eq!(change["updated_at"], "1970-01-01T00:00:01.000Z");
    }

    #[tokio::test]
    async fn f_di_09_an_older_edit_is_a_conflict_until_the_user_picks() {
        let server = Arc::new(Server::default());
        let (mut a, mut b) = (device(), device());
        a.edit("tauri", "Tauri", 1_000).await;
        a.sync(&server, 1_500).await;
        b.sync(&server, 1_600).await;
        // A edits offline first; B edits later and syncs first.
        a.edit("tauri", "Tauri.", 2_000).await;
        b.edit("tauri", "TAURI", 3_000).await;
        b.sync(&server, 3_500).await;
        let got = a.sync(&server, 4_000).await;
        assert_eq!(got.conflicts.len(), 1);
        assert_eq!(got.conflicts[0].local.replacement, "Tauri.");
        assert_eq!(got.conflicts[0].remote.replacement, "TAURI");
        // Nothing is overwritten while the user decides, and it comes back.
        assert_eq!(a.to("tauri").await.as_deref(), Some("Tauri."));
        assert_eq!(a.sync(&server, 4_100).await.conflicts.len(), 1);
    }

    #[tokio::test]
    async fn f_di_09_a_newer_edit_elsewhere_replaces_the_synced_one() {
        let server = Arc::new(Server::default());
        let (mut a, mut b) = (device(), device());
        a.edit("tauri", "Tauri", 1_000).await;
        a.sync(&server, 1_500).await;
        b.edit("tauri", "TAURI", 5_000).await;
        b.sync(&server, 5_500).await;
        assert_eq!(a.sync(&server, 6_000).await.pulled, 1);
        assert_eq!(a.to("tauri").await.as_deref(), Some("TAURI"));
    }

    #[tokio::test]
    async fn f_di_09_another_account_starts_over() {
        let server = Arc::new(Server::default());
        let mut a = device();
        a.edit("tauri", "Tauri", 1_000).await;
        a.sync(&server, 1_500).await;
        let other = Arc::new(Server::default());
        let (cursor, out) = run(
            &a.store,
            a.cursor.clone(),
            "other@tandem.test",
            &call(other.clone()),
            2_000,
        )
        .await;
        assert_eq!(out.expect("sync").0.pushed, 1);
        assert_eq!(cursor.account, "other@tandem.test");
        assert_eq!(other.bodies.lock()[0]["since_rev"], 0);
    }

    #[tokio::test]
    async fn f_di_09_large_dictionaries_go_up_in_batches() {
        let server = Arc::new(Server::default());
        let mut a = device();
        let rows: Vec<RuleRow> = (0..(BATCH + 20))
            .map(|i| {
                let mut r = Rule::correction(&format!("r{i}"), &format!("w{i}"), "x");
                r.updated_at = 1_000 + i as i64;
                row_from_rule(&r, 0)
            })
            .collect();
        repo::put_rules(&a.store, rows).await.expect("put");
        let got = a.sync(&server, 9_000).await;
        assert_eq!(got.pushed as usize, BATCH + 20);
        let sizes: Vec<usize> = server
            .bodies
            .lock()
            .iter()
            .map(|b| b["changes"].as_array().map_or(0, Vec::len))
            .collect();
        assert_eq!(sizes, vec![BATCH, 20]);
    }
}
