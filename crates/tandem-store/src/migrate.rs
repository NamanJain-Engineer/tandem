//! Forward-only migrations tracked by `PRAGMA user_version` (ARCHITECTURE §4.1).

use std::path::Path;

use rusqlite::Connection;

use crate::StoreError;

pub const MIGRATIONS: &[(&str, &str)] = &[
    ("0001_init", include_str!("../migrations/0001_init.sql")),
    ("0002_extras", include_str!("../migrations/0002_extras.sql")),
    (
        "0003_bestof_queued",
        include_str!("../migrations/0003_bestof_queued.sql"),
    ),
    (
        "0005_secrets",
        include_str!("../migrations/0005_secrets.sql"),
    ),
];

pub const KEEP_BACKUPS: usize = 3;

pub fn current_version(conn: &Connection) -> Result<usize, StoreError> {
    let v: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    Ok(v.max(0) as usize)
}

/// Applies pending migrations, each in its own transaction. Before any
/// migration runs on an existing database, a copy is saved in `backups/`.
pub fn run(conn: &mut Connection, db: &Path, backups: &Path) -> Result<(), StoreError> {
    let version = current_version(conn)?;
    if version > MIGRATIONS.len() {
        return Err(StoreError::Migration(format!(
            "this database was written by a newer Vibezeo (schema {version})"
        )));
    }
    if version == MIGRATIONS.len() {
        return Ok(());
    }
    if version > 0 {
        backup(conn, db, backups, version)?;
    }
    for (index, (name, sql)) in MIGRATIONS.iter().enumerate().skip(version) {
        let txn = conn.transaction()?;
        txn.execute_batch(sql)
            .map_err(|e| StoreError::Migration(format!("{name}: {e}")))?;
        txn.pragma_update(None, "user_version", (index + 1) as i64)?;
        txn.commit()?;
        tracing::info!(migration = name, "applied migration");
    }
    let fk_errors: i64 =
        conn.query_row("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |r| {
            r.get(0)
        })?;
    if fk_errors > 0 {
        return Err(StoreError::Migration(format!(
            "{fk_errors} foreign key violations"
        )));
    }
    Ok(())
}

fn backup(conn: &Connection, db: &Path, backups: &Path, version: usize) -> Result<(), StoreError> {
    std::fs::create_dir_all(backups)?;
    let target = backups.join(format!("tandem-{version}.db"));
    let _ = std::fs::remove_file(&target);
    conn.execute("VACUUM INTO ?1", [target.to_string_lossy().as_ref()])?;
    let mut existing: Vec<_> = std::fs::read_dir(backups)?
        .filter_map(Result::ok)
        .filter(|e| e.file_name().to_string_lossy().starts_with("tandem-"))
        .collect();
    existing.sort_by_key(|e| e.metadata().and_then(|m| m.modified()).ok());
    while existing.len() > KEEP_BACKUPS {
        let oldest = existing.remove(0);
        let _ = std::fs::remove_file(oldest.path());
    }
    let _ = db;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrations_apply_to_empty_db_and_pass_integrity_checks() {
        let dir = tempfile::tempdir().expect("tempdir");
        let db = dir.path().join("tandem.db");
        let mut conn = Connection::open(&db).expect("open");
        conn.execute_batch("PRAGMA foreign_keys=ON").expect("fk");
        run(&mut conn, &db, &dir.path().join("backups")).expect("migrate");
        assert_eq!(current_version(&conn).expect("v"), MIGRATIONS.len());
        let ok: String = conn
            .query_row("PRAGMA integrity_check", [], |r| r.get(0))
            .expect("check");
        assert_eq!(ok, "ok");
        let local: String = conn
            .query_row("SELECT kind FROM hosts WHERE id='local'", [], |r| r.get(0))
            .expect("local host");
        assert_eq!(local, "local");
        // Running again is a no-op.
        run(&mut conn, &db, &dir.path().join("backups")).expect("rerun");
    }

    #[test]
    fn f_ex_06_5_a_v2_database_with_best_of_n_lanes_upgrades_to_v3_intact() {
        let dir = tempfile::tempdir().expect("tempdir");
        let db = dir.path().join("tandem.db");
        let mut conn = Connection::open(&db).expect("open");
        conn.execute_batch("PRAGMA foreign_keys=ON").expect("fk");
        for (index, (_, sql)) in MIGRATIONS.iter().take(2).enumerate() {
            conn.execute_batch(sql).expect("old migration");
            conn.pragma_update(None, "user_version", (index + 1) as i64)
                .expect("version");
        }
        conn.execute_batch(
            "INSERT INTO bestof_runs (id, workspace_id, title, base, state, created_at)
               VALUES ('bo_1', 'ws_1', 'fix', 'abc', 'running', 1);
             INSERT INTO bestof_lanes (run_id, seat, lane_id, branch, engine_id, check_state, check_exit, check_output)
               VALUES ('bo_1', 0, 'ws_2', 'tandem/a', 'claude', 'passed', 0, 'ok'),
                      ('bo_1', 1, NULL, NULL, 'codex', 'none', NULL, NULL);",
        )
        .expect("v2 rows");
        run(&mut conn, &db, &dir.path().join("backups")).expect("migrate");
        assert_eq!(current_version(&conn).expect("v"), MIGRATIONS.len());
        type Lane = (i64, Option<String>, String, String, Option<i64>);
        let rows: Vec<Lane> = conn
            .prepare(
                "SELECT seat, branch, engine_id, check_state, check_exit FROM bestof_lanes \
                 WHERE run_id = 'bo_1' ORDER BY seat",
            )
            .expect("prepare")
            .query_map([], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
            })
            .expect("query")
            .collect::<Result<_, _>>()
            .expect("rows");
        assert_eq!(
            rows,
            vec![
                (
                    0,
                    Some("tandem/a".into()),
                    "claude".into(),
                    "passed".into(),
                    Some(0)
                ),
                (1, None, "codex".into(), "none".into(), None),
            ]
        );
        // The new state is accepted, and the run's lanes still go with it.
        conn.execute(
            "UPDATE bestof_lanes SET check_state = 'queued' WHERE seat = 1",
            [],
        )
        .expect("queued");
        conn.execute("DELETE FROM bestof_runs WHERE id = 'bo_1'", [])
            .expect("delete run");
        let left: i64 = conn
            .query_row("SELECT COUNT(*) FROM bestof_lanes", [], |r| r.get(0))
            .expect("count");
        assert_eq!(left, 0);
        assert!(dir.path().join("backups/tandem-2.db").is_file());
    }

    #[test]
    fn newer_schema_is_refused_and_left_unchanged() {
        let dir = tempfile::tempdir().expect("tempdir");
        let db = dir.path().join("tandem.db");
        let mut conn = Connection::open(&db).expect("open");
        conn.pragma_update(None, "user_version", 99_i64)
            .expect("bump");
        let err = run(&mut conn, &db, &dir.path().join("backups")).expect_err("refused");
        assert!(err.to_string().contains("Your data is unchanged"));
        assert_eq!(current_version(&conn).expect("v"), 99);
    }
}
