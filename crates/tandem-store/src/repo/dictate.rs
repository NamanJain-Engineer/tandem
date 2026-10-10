//! Dictate's local data (F-DI-08, F-DI-09; ARCHITECTURE §4.2): takes with
//! full-text search, the totals that survive Clear history, dictionary rules
//! (soft-deleted for 30 days) and locally mined suggestions (evidence kept
//! 90 days). Nothing here leaves the machine.

use rusqlite::{params, OptionalExtension, Row};

use crate::{now_ms, Store, StoreError};

/// Deleted rules can be restored this long.
pub const DELETED_KEEP_MS: i64 = 30 * 24 * 60 * 60 * 1000;
/// Suggestion evidence expires after this.
pub const EVIDENCE_KEEP_MS: i64 = 90 * 24 * 60 * 60 * 1000;
pub const PAGE: usize = 50;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TakeRow {
    pub id: String,
    pub created_at: i64,
    pub text: String,
    pub engine: String,
    pub model: String,
    pub word_count: u32,
    pub duration_ms: u64,
    pub app_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Totals {
    pub words: u64,
    pub speaking_ms: u64,
    pub takes: u64,
}

fn take_row(r: &Row) -> rusqlite::Result<TakeRow> {
    Ok(TakeRow {
        id: r.get(0)?,
        created_at: r.get(1)?,
        text: r.get(2)?,
        engine: r.get(3)?,
        model: r.get(4)?,
        word_count: r.get::<_, i64>(5)? as u32,
        duration_ms: r.get::<_, i64>(6)? as u64,
        app_id: r.get(7)?,
    })
}

const TAKE_COLS: &str =
    "t.id, t.created_at, t.text, t.engine, t.model, t.word_count, t.duration_ms, t.app_id";

/// Stores a take, indexes it and adds it to the totals, in one transaction.
pub async fn add_take(store: &Store, take: TakeRow) -> Result<(), StoreError> {
    store
        .write(move |tx| {
            tx.execute(
                "INSERT INTO dictate_takes (id, created_at, text, engine, model, word_count, duration_ms, app_id)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    take.id,
                    take.created_at,
                    take.text,
                    take.engine,
                    take.model,
                    take.word_count as i64,
                    take.duration_ms as i64,
                    take.app_id
                ],
            )?;
            let rowid = tx.last_insert_rowid();
            tx.execute(
                "INSERT INTO dictate_fts (rowid, text) VALUES (?1, ?2)",
                params![rowid, take.text],
            )?;
            tx.execute(
                "INSERT INTO dictate_stats (id, words, speaking_ms, takes) VALUES (1, ?1, ?2, 1)
                 ON CONFLICT(id) DO UPDATE SET words = words + ?1, speaking_ms = speaking_ms + ?2, takes = takes + 1",
                params![take.word_count as i64, take.duration_ms as i64],
            )?;
            Ok(())
        })
        .await
}

/// Turns what the user typed into an FTS5 query: each word a prefix term,
/// quoted, so punctuation can't become query syntax.
pub fn fts_query(input: &str) -> Option<String> {
    let terms: Vec<String> = input
        .split(|c: char| !c.is_alphanumeric() && c != '\'')
        .filter(|w| !w.is_empty())
        .map(|w| format!("\"{}\"*", w.replace('"', "")))
        .collect();
    (!terms.is_empty()).then(|| terms.join(" "))
}

/// A page of takes, newest first; with `query`, the ones matching it.
pub async fn history(
    store: &Store,
    query: Option<String>,
    before: Option<(i64, String)>,
    limit: usize,
) -> Result<(Vec<TakeRow>, Option<(i64, String)>), StoreError> {
    let limit = limit.clamp(1, 200);
    store
        .read(move |conn| {
            let (cutoff_at, cutoff_id) = before.unwrap_or((i64::MAX, String::new()));
            let fts = query.as_deref().and_then(fts_query);
            let mut rows = Vec::new();
            match fts {
                Some(q) => {
                    let sql = format!(
                        "SELECT {TAKE_COLS} FROM dictate_fts f JOIN dictate_takes t ON t.rowid = f.rowid
                         WHERE dictate_fts MATCH ?1 AND t.deleted = 0
                           AND (t.created_at < ?2 OR (t.created_at = ?2 AND t.id < ?3))
                         ORDER BY t.created_at DESC, t.id DESC LIMIT ?4"
                    );
                    let mut st = conn.prepare(&sql)?;
                    let it = st.query_map(params![q, cutoff_at, cutoff_id, (limit + 1) as i64], take_row)?;
                    for r in it {
                        rows.push(r?);
                    }
                }
                None => {
                    let sql = format!(
                        "SELECT {TAKE_COLS} FROM dictate_takes t WHERE t.deleted = 0
                           AND (t.created_at < ?1 OR (t.created_at = ?1 AND t.id < ?2))
                         ORDER BY t.created_at DESC, t.id DESC LIMIT ?3"
                    );
                    let mut st = conn.prepare(&sql)?;
                    let it = st.query_map(params![cutoff_at, cutoff_id, (limit + 1) as i64], take_row)?;
                    for r in it {
                        rows.push(r?);
                    }
                }
            }
            let next = if rows.len() > limit {
                rows.truncate(limit);
                rows.last().map(|r| (r.created_at, r.id.clone()))
            } else {
                None
            };
            Ok((rows, next))
        })
        .await
}

pub async fn get_take(store: &Store, id: String) -> Result<Option<TakeRow>, StoreError> {
    store
        .read(move |conn| {
            let sql = format!(
                "SELECT {TAKE_COLS} FROM dictate_takes t WHERE t.id = ?1 AND t.deleted = 0"
            );
            Ok(conn.query_row(&sql, params![id], take_row).optional()?)
        })
        .await
}

/// Every take, oldest first (CSV export).
pub async fn all_takes(store: &Store) -> Result<Vec<TakeRow>, StoreError> {
    store
        .read(|conn| {
            let sql = format!(
                "SELECT {TAKE_COLS} FROM dictate_takes t WHERE t.deleted = 0 ORDER BY t.created_at, t.id"
            );
            let mut st = conn.prepare(&sql)?;
            let rows = st.query_map([], take_row)?.collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
        .await
}

fn unindex(
    tx: &rusqlite::Transaction,
    where_sql: &str,
    p: &[&dyn rusqlite::ToSql],
) -> Result<(), StoreError> {
    let sql = format!(
        "INSERT INTO dictate_fts (dictate_fts, rowid, text) SELECT 'delete', rowid, text FROM dictate_takes WHERE {where_sql}"
    );
    tx.execute(&sql, p)?;
    Ok(())
}

/// Deletes one take (the totals keep it).
pub async fn delete_take(store: &Store, id: String) -> Result<bool, StoreError> {
    store
        .write(move |tx| {
            unindex(tx, "id = ?1", &[&id])?;
            Ok(tx.execute("DELETE FROM dictate_takes WHERE id = ?1", params![id])? > 0)
        })
        .await
}

/// Clear history: every take goes, the totals stay (F-DI-08 AC2).
pub async fn clear_history(store: &Store) -> Result<(), StoreError> {
    store
        .write(|tx| {
            unindex(tx, "1 = 1", &[])?;
            tx.execute("DELETE FROM dictate_takes", [])?;
            Ok(())
        })
        .await
}

/// Takes older than `cutoff` go (the Keep history setting); totals stay.
pub async fn prune_takes(store: &Store, cutoff: i64) -> Result<usize, StoreError> {
    store
        .write(move |tx| {
            unindex(tx, "created_at < ?1", &[&cutoff])?;
            Ok(tx.execute(
                "DELETE FROM dictate_takes WHERE created_at < ?1",
                params![cutoff],
            )?)
        })
        .await
}

pub async fn update_take_text(
    store: &Store,
    id: String,
    text: String,
    words: u32,
) -> Result<bool, StoreError> {
    store
        .write(move |tx| {
            unindex(tx, "id = ?1", &[&id])?;
            let n = tx.execute(
                "UPDATE dictate_takes SET text = ?2, word_count = ?3 WHERE id = ?1",
                params![id, text, words as i64],
            )?;
            tx.execute(
                "INSERT INTO dictate_fts (rowid, text) SELECT rowid, text FROM dictate_takes WHERE id = ?1",
                params![id],
            )?;
            Ok(n > 0)
        })
        .await
}

pub async fn totals(store: &Store) -> Result<Totals, StoreError> {
    store
        .read(|conn| {
            Ok(conn
                .query_row(
                    "SELECT words, speaking_ms, takes FROM dictate_stats WHERE id = 1",
                    [],
                    |r| {
                        Ok(Totals {
                            words: r.get::<_, i64>(0)? as u64,
                            speaking_ms: r.get::<_, i64>(1)? as u64,
                            takes: r.get::<_, i64>(2)? as u64,
                        })
                    },
                )
                .optional()?
                .unwrap_or_default())
        })
        .await
}

// ── Dictionary ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuleRow {
    pub id: String,
    /// `word` or `correction`.
    pub kind: String,
    pub replacement: String,
    pub phrases: Vec<String>,
    pub match_case: bool,
    pub protect: bool,
    /// `exact` or `flexible`.
    pub spacing: String,
    pub enabled: bool,
    pub lang: String,
    pub app_profiles: Vec<String>,
    pub updated_at: i64,
    pub deleted_at: Option<i64>,
    pub sync_rev: i64,
}

fn rule_row(r: &Row) -> rusqlite::Result<RuleRow> {
    let kind: String = r.get(1)?;
    let preferred: Option<String> = r.get(2)?;
    let variants: Option<String> = r.get(3)?;
    let phrase: Option<String> = r.get(4)?;
    let replacement: Option<String> = r.get(5)?;
    let list = |s: Option<String>| -> Vec<String> {
        s.and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    };
    let (replacement, phrases) = if kind == "word" {
        (preferred.unwrap_or_default(), list(variants))
    } else {
        (replacement.unwrap_or_default(), list(phrase))
    };
    let profiles: String = r.get(11)?;
    Ok(RuleRow {
        id: r.get(0)?,
        kind,
        replacement,
        phrases,
        match_case: r.get::<_, i64>(6)? != 0,
        protect: r.get::<_, i64>(7)? != 0,
        spacing: r.get(8)?,
        enabled: r.get::<_, i64>(9)? != 0,
        lang: r.get(10)?,
        app_profiles: serde_json::from_str(&profiles).unwrap_or_default(),
        updated_at: r.get(12)?,
        deleted_at: r.get(13)?,
        sync_rev: r.get(14)?,
    })
}

const RULE_COLS: &str = "id, kind, preferred, variants_json, phrase, replacement, match_case, protect, spacing, enabled, lang, app_profiles_json, updated_at, deleted_at, sync_rev";

/// Live rules (not deleted), oldest first.
pub async fn rules(store: &Store) -> Result<Vec<RuleRow>, StoreError> {
    rules_where(store, "deleted_at IS NULL ORDER BY updated_at, id").await
}

/// Rules deleted within the last 30 days, newest first.
pub async fn deleted_rules(store: &Store) -> Result<Vec<RuleRow>, StoreError> {
    let cutoff = now_ms() - DELETED_KEEP_MS;
    store
        .read(move |conn| {
            let sql = format!(
                "SELECT {RULE_COLS} FROM dictionary_rules WHERE deleted_at IS NOT NULL AND deleted_at >= ?1 ORDER BY deleted_at DESC"
            );
            let mut st = conn.prepare(&sql)?;
            let rows = st.query_map(params![cutoff], rule_row)?.collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
        .await
}

/// Every rule, deleted ones included (sync, export).
pub async fn all_rules(store: &Store) -> Result<Vec<RuleRow>, StoreError> {
    rules_where(store, "1 = 1 ORDER BY updated_at, id").await
}

async fn rules_where(store: &Store, tail: &'static str) -> Result<Vec<RuleRow>, StoreError> {
    store
        .read(move |conn| {
            let sql = format!("SELECT {RULE_COLS} FROM dictionary_rules WHERE {tail}");
            let mut st = conn.prepare(&sql)?;
            let rows = st.query_map([], rule_row)?.collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
        .await
}

pub async fn get_rule(store: &Store, id: String) -> Result<Option<RuleRow>, StoreError> {
    store
        .read(move |conn| {
            let sql = format!("SELECT {RULE_COLS} FROM dictionary_rules WHERE id = ?1");
            Ok(conn.query_row(&sql, params![id], rule_row).optional()?)
        })
        .await
}

fn put_rule(tx: &rusqlite::Transaction, r: &RuleRow) -> Result<(), StoreError> {
    let list = |v: &Vec<String>| serde_json::to_string(v).unwrap_or_else(|_| "[]".into());
    let (preferred, variants, phrase, replacement) = if r.kind == "word" {
        (
            Some(r.replacement.clone()),
            Some(list(&r.phrases)),
            None,
            None,
        )
    } else {
        (
            None,
            None,
            Some(list(&r.phrases)),
            Some(r.replacement.clone()),
        )
    };
    tx.execute(
        "INSERT INTO dictionary_rules (id, kind, preferred, variants_json, phrase, replacement, match_case, protect, spacing, enabled, lang, app_profiles_json, updated_at, deleted_at, sync_rev)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)
         ON CONFLICT(id) DO UPDATE SET kind = ?2, preferred = ?3, variants_json = ?4, phrase = ?5, replacement = ?6,
           match_case = ?7, protect = ?8, spacing = ?9, enabled = ?10, lang = ?11, app_profiles_json = ?12,
           updated_at = ?13, deleted_at = ?14, sync_rev = ?15",
        params![
            r.id,
            r.kind,
            preferred,
            variants,
            phrase,
            replacement,
            r.match_case as i64,
            r.protect as i64,
            r.spacing,
            r.enabled as i64,
            r.lang,
            list(&r.app_profiles),
            r.updated_at,
            r.deleted_at,
            r.sync_rev
        ],
    )?;
    Ok(())
}

/// Inserts or replaces rules as given (timestamps included).
pub async fn put_rules(store: &Store, rows: Vec<RuleRow>) -> Result<(), StoreError> {
    store
        .write(move |tx| {
            for r in &rows {
                put_rule(tx, r)?;
            }
            Ok(())
        })
        .await
}

/// Soft-deletes rules (restorable for 30 days).
pub async fn delete_rules(store: &Store, ids: Vec<String>) -> Result<usize, StoreError> {
    let now = now_ms();
    store
        .write(move |tx| {
            let mut n = 0;
            for id in &ids {
                n += tx.execute(
                    "UPDATE dictionary_rules SET deleted_at = ?2, updated_at = ?2 WHERE id = ?1 AND deleted_at IS NULL",
                    params![id, now],
                )?;
            }
            Ok(n)
        })
        .await
}

pub async fn restore_rule(store: &Store, id: String) -> Result<bool, StoreError> {
    let now = now_ms();
    store
        .write(move |tx| {
            Ok(tx.execute(
                "UPDATE dictionary_rules SET deleted_at = NULL, updated_at = ?2 WHERE id = ?1 AND deleted_at IS NOT NULL",
                params![id, now],
            )? > 0)
        })
        .await
}

/// Forgets rules deleted more than 30 days ago and suggestions whose
/// evidence is older than 90 days.
pub async fn purge(store: &Store, now: i64) -> Result<(), StoreError> {
    store
        .write(move |tx| {
            tx.execute(
                "DELETE FROM dictionary_rules WHERE deleted_at IS NOT NULL AND deleted_at < ?1",
                params![now - DELETED_KEEP_MS],
            )?;
            tx.execute(
                "DELETE FROM dictionary_suggestions WHERE state = 'open' AND last_seen < ?1",
                params![now - EVIDENCE_KEEP_MS],
            )?;
            Ok(())
        })
        .await
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SuggestionRow {
    pub id: String,
    pub term: String,
    pub kind: String,
    pub evidence: Vec<String>,
    pub first_seen: i64,
    pub last_seen: i64,
    pub state: String,
}

fn suggestion_row(r: &Row) -> rusqlite::Result<SuggestionRow> {
    let ev: String = r.get(3)?;
    Ok(SuggestionRow {
        id: r.get(0)?,
        term: r.get(1)?,
        kind: r.get(2)?,
        evidence: serde_json::from_str(&ev).unwrap_or_default(),
        first_seen: r.get(4)?,
        last_seen: r.get(5)?,
        state: r.get(6)?,
    })
}

pub async fn open_suggestions(store: &Store) -> Result<Vec<SuggestionRow>, StoreError> {
    store
        .read(|conn| {
            let mut st = conn.prepare(
                "SELECT id, term, kind, evidence_json, first_seen, last_seen, state FROM dictionary_suggestions
                 WHERE state = 'open' ORDER BY last_seen DESC",
            )?;
            let rows = st.query_map([], suggestion_row)?.collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
        .await
}

/// Every suggestion's term and state (so dismissed ones aren't offered again).
pub async fn suggestion_terms(store: &Store) -> Result<Vec<(String, String)>, StoreError> {
    store
        .read(|conn| {
            let mut st = conn.prepare("SELECT lower(term), state FROM dictionary_suggestions")?;
            let rows = st
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
        .await
}

/// Adds or refreshes open suggestions (same term → evidence merged).
pub async fn upsert_suggestions(store: &Store, rows: Vec<SuggestionRow>) -> Result<(), StoreError> {
    store
        .write(move |tx| {
            for s in &rows {
                let existing: Option<(String, String, String)> = tx
                    .query_row(
                        "SELECT id, evidence_json, state FROM dictionary_suggestions WHERE lower(term) = lower(?1)",
                        params![s.term],
                        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                    )
                    .optional()?;
                match existing {
                    Some((_, _, state)) if state != "open" => {}
                    Some((id, ev, _)) => {
                        let mut all: Vec<String> = serde_json::from_str(&ev).unwrap_or_default();
                        for e in &s.evidence {
                            if !all.contains(e) {
                                all.push(e.clone());
                            }
                        }
                        all.truncate(5);
                        tx.execute(
                            "UPDATE dictionary_suggestions SET evidence_json = ?2, last_seen = ?3 WHERE id = ?1",
                            params![id, serde_json::to_string(&all).unwrap_or_default(), s.last_seen],
                        )?;
                    }
                    None => {
                        tx.execute(
                            "INSERT INTO dictionary_suggestions (id, term, kind, evidence_json, first_seen, last_seen, state)
                             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'open')",
                            params![
                                s.id,
                                s.term,
                                s.kind,
                                serde_json::to_string(&s.evidence).unwrap_or_default(),
                                s.first_seen,
                                s.last_seen
                            ],
                        )?;
                    }
                }
            }
            Ok(())
        })
        .await
}

pub async fn resolve_suggestion(
    store: &Store,
    id: String,
    remembered: bool,
) -> Result<Option<SuggestionRow>, StoreError> {
    store
        .write(move |tx| {
            let row = tx
                .query_row(
                    "SELECT id, term, kind, evidence_json, first_seen, last_seen, state FROM dictionary_suggestions WHERE id = ?1",
                    params![id],
                    suggestion_row,
                )
                .optional()?;
            if row.is_some() {
                tx.execute(
                    "UPDATE dictionary_suggestions SET state = ?2 WHERE id = ?1",
                    params![id, if remembered { "remembered" } else { "dismissed" }],
                )?;
            }
            Ok(row)
        })
        .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use tandem_core::DataDir;

    fn store() -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().expect("tempdir");
        let data = DataDir::at(dir.path().to_path_buf()).expect("data");
        let store = Store::open(&data).expect("store");
        (dir, store)
    }

    fn take(id: &str, at: i64, text: &str) -> TakeRow {
        TakeRow {
            id: id.into(),
            created_at: at,
            text: text.into(),
            engine: "local".into(),
            model: "whisper-base.en".into(),
            word_count: text.split_whitespace().count() as u32,
            duration_ms: 3_000,
            app_id: Some("org.gnome.gedit".into()),
        }
    }

    #[tokio::test]
    async fn f_di_08_1_search_finds_a_stored_take_and_pages() {
        let (_d, s) = store();
        add_take(&s, take("a", 1, "refactor the auth guard"))
            .await
            .expect("a");
        add_take(&s, take("b", 2, "ship the release notes"))
            .await
            .expect("b");
        add_take(&s, take("c", 3, "authentication flows"))
            .await
            .expect("c");
        let (rows, next) = history(&s, Some("auth".into()), None, 50).await.expect("q");
        let ids: Vec<&str> = rows.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(ids, vec!["c", "a"], "prefix match, newest first");
        assert!(next.is_none());
        let (rows, _) = history(&s, Some("release\"))".into()), None, 50)
            .await
            .expect("quotes are safe");
        assert_eq!(rows.len(), 1);
        let (page1, next) = history(&s, None, None, 2).await.expect("p1");
        assert_eq!(page1.len(), 2);
        let (page2, next2) = history(&s, None, next, 2).await.expect("p2");
        assert_eq!(
            page2.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(),
            vec!["a"]
        );
        assert!(next2.is_none());
    }

    #[tokio::test]
    async fn f_di_08_2_clear_history_keeps_the_totals() {
        let (_d, s) = store();
        add_take(&s, take("a", 1, "one two three"))
            .await
            .expect("a");
        add_take(&s, take("b", 2, "four five")).await.expect("b");
        assert_eq!(totals(&s).await.expect("t").words, 5);
        delete_take(&s, "a".into()).await.expect("del");
        assert_eq!(totals(&s).await.expect("t").words, 5, "delete keeps totals");
        clear_history(&s).await.expect("clear");
        assert!(history(&s, None, None, 50).await.expect("h").0.is_empty());
        assert!(history(&s, Some("four".into()), None, 50)
            .await
            .expect("h")
            .0
            .is_empty());
        let t = totals(&s).await.expect("t");
        assert_eq!((t.words, t.takes, t.speaking_ms), (5, 2, 6_000));
    }

    #[tokio::test]
    async fn f_di_08_correcting_reindexes_and_prune_keeps_totals() {
        let (_d, s) = store();
        add_take(&s, take("a", 1, "use tory")).await.expect("a");
        add_take(&s, take("b", 1_000, "newer")).await.expect("b");
        update_take_text(&s, "a".into(), "use Tauri".into(), 2)
            .await
            .expect("fix");
        assert_eq!(
            history(&s, Some("tauri".into()), None, 50)
                .await
                .expect("h")
                .0
                .len(),
            1
        );
        assert!(history(&s, Some("tory".into()), None, 50)
            .await
            .expect("h")
            .0
            .is_empty());
        assert_eq!(prune_takes(&s, 500).await.expect("prune"), 1);
        assert_eq!(all_takes(&s).await.expect("all").len(), 1);
        assert_eq!(totals(&s).await.expect("t").takes, 2);
    }

    fn rule(id: &str, kind: &str) -> RuleRow {
        RuleRow {
            id: id.into(),
            kind: kind.into(),
            replacement: "Tauri".into(),
            phrases: vec!["tory".into(), "tawri".into()],
            match_case: false,
            protect: true,
            spacing: "exact".into(),
            enabled: true,
            lang: "*".into(),
            app_profiles: vec!["org.gnome.gedit".into()],
            updated_at: 10,
            deleted_at: None,
            sync_rev: 0,
        }
    }

    #[tokio::test]
    async fn f_di_09_rules_round_trip_soft_delete_and_restore() {
        let (_d, s) = store();
        put_rules(&s, vec![rule("w", "word"), rule("c", "correction")])
            .await
            .expect("put");
        let all = rules(&s).await.expect("rules");
        assert_eq!(all.len(), 2);
        assert_eq!(
            all.iter().find(|r| r.id == "w").expect("w"),
            &rule("w", "word")
        );
        assert_eq!(
            all.iter().find(|r| r.id == "c").expect("c"),
            &rule("c", "correction")
        );
        assert_eq!(delete_rules(&s, vec!["w".into()]).await.expect("del"), 1);
        assert_eq!(rules(&s).await.expect("r").len(), 1);
        assert_eq!(deleted_rules(&s).await.expect("d").len(), 1);
        assert!(restore_rule(&s, "w".into()).await.expect("restore"));
        assert_eq!(rules(&s).await.expect("r").len(), 2);
        // Older than 30 days: purged for good.
        delete_rules(&s, vec!["c".into()]).await.expect("del");
        purge(&s, now_ms() + DELETED_KEEP_MS + 1)
            .await
            .expect("purge");
        assert!(get_rule(&s, "c".into()).await.expect("get").is_none());
    }

    #[tokio::test]
    async fn f_di_09_suggestions_merge_and_expire_and_stay_dismissed() {
        let (_d, s) = store();
        let sug = |id: &str, ev: &str, at: i64| SuggestionRow {
            id: id.into(),
            term: "Kubernetes".into(),
            kind: "proper_noun".into(),
            evidence: vec![ev.into()],
            first_seen: at,
            last_seen: at,
            state: "open".into(),
        };
        let now = now_ms();
        upsert_suggestions(&s, vec![sug("s1", "deploy to kubernetes", now)])
            .await
            .expect("up");
        upsert_suggestions(&s, vec![sug("s2", "the kubernetes pod", now)])
            .await
            .expect("up");
        let open = open_suggestions(&s).await.expect("open");
        assert_eq!(open.len(), 1);
        assert_eq!(open[0].evidence.len(), 2);
        resolve_suggestion(&s, "s1".into(), false)
            .await
            .expect("dismiss");
        upsert_suggestions(&s, vec![sug("s3", "again", now)])
            .await
            .expect("up");
        assert!(
            open_suggestions(&s).await.expect("open").is_empty(),
            "dismissed stays dismissed"
        );
        let (_d2, s2) = store();
        upsert_suggestions(&s2, vec![sug("old", "x", now - EVIDENCE_KEEP_MS - 10)])
            .await
            .expect("up");
        purge(&s2, now).await.expect("purge");
        assert!(open_suggestions(&s2).await.expect("open").is_empty());
    }

    #[test]
    fn fts_queries_are_quoted_prefixes() {
        assert_eq!(
            fts_query("auth guard").as_deref(),
            Some("\"auth\"* \"guard\"*")
        );
        assert_eq!(fts_query("\"* OR"), Some("\"OR\"*".into()));
        assert_eq!(fts_query("  ,, "), None);
    }
}
