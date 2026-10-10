//! Every table as JSON rows, for "Export my data" (F-TL-05, ARCHITECTURE
//! §16.5).

use rusqlite::types::ValueRef;
use rusqlite::Connection;
use serde_json::{Map, Value};

use crate::StoreError;

/// One table's rows, each a JSON object keyed by column name.
#[derive(Debug, Clone, PartialEq)]
pub struct TableDump {
    pub name: String,
    pub rows: Vec<Value>,
}

/// Dumps every table. SQLite's own tables and the full-text indexes
/// (virtual tables and their shadow tables) are left out: they hold nothing
/// the other tables don't. Blobs come out as `{"hex": "…"}`.
pub fn dump_tables(conn: &Connection) -> Result<Vec<TableDump>, StoreError> {
    let mut stmt = conn.prepare(
        "SELECT name, coalesce(sql, '') FROM sqlite_master WHERE type = 'table' ORDER BY name",
    )?;
    let tables = stmt
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
        .collect::<Result<Vec<_>, _>>()?;
    let virtuals: Vec<&str> = tables
        .iter()
        .filter(|(_, sql)| {
            sql.trim_start()
                .to_ascii_uppercase()
                .starts_with("CREATE VIRTUAL TABLE")
        })
        .map(|(name, _)| name.as_str())
        .collect();
    let mut out = Vec::new();
    for (name, _) in &tables {
        let fts = virtuals.iter().any(|v| {
            name == v
                || name
                    .strip_prefix(v)
                    .is_some_and(|rest| rest.starts_with('_'))
        });
        if name.starts_with("sqlite_") || fts {
            continue;
        }
        out.push(TableDump {
            name: name.clone(),
            rows: rows(conn, name)?,
        });
    }
    Ok(out)
}

fn rows(conn: &Connection, table: &str) -> Result<Vec<Value>, StoreError> {
    let quoted = format!("\"{}\"", table.replace('"', "\"\""));
    let mut stmt = conn.prepare(&format!("SELECT * FROM {quoted}"))?;
    let columns: Vec<String> = stmt
        .column_names()
        .into_iter()
        .map(str::to_string)
        .collect();
    let mut rows = stmt.query([])?;
    let mut out = Vec::new();
    while let Some(row) = rows.next()? {
        let mut obj = Map::with_capacity(columns.len());
        for (i, column) in columns.iter().enumerate() {
            obj.insert(column.clone(), json(row.get_ref(i)?));
        }
        out.push(Value::Object(obj));
    }
    Ok(out)
}

fn json(value: ValueRef<'_>) -> Value {
    match value {
        ValueRef::Null => Value::Null,
        ValueRef::Integer(i) => Value::from(i),
        ValueRef::Real(f) => serde_json::Number::from_f64(f).map_or(Value::Null, Value::Number),
        ValueRef::Text(t) => Value::String(String::from_utf8_lossy(t).into_owned()),
        ValueRef::Blob(b) => {
            let hex: String = b.iter().map(|byte| format!("{byte:02x}")).collect();
            serde_json::json!({ "hex": hex })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Store;
    use tandem_core::DataDir;

    fn store() -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().expect("tempdir");
        let data = DataDir::at(dir.path().to_path_buf()).expect("data");
        let store = Store::open(&data).expect("store");
        (dir, store)
    }

    #[test]
    fn f_tl_05_1_every_table_dumps_as_json_rows_without_full_text_shadows() {
        let (_dir, store) = store();
        store
            .write_blocking(|txn| {
                txn.execute(
                    "INSERT INTO teammates (id, name, name_fold, engine_id, home, settings_json, sort_key, created_at)
                     VALUES ('tm_1', 'Ada', 'ada', 'claude', '/t/ada', '{}', 'a0', 1)",
                    [],
                )?;
                Ok(())
            })
            .expect("insert");
        let tables = store.read_blocking(dump_tables).expect("dump");
        let names: Vec<&str> = tables.iter().map(|t| t.name.as_str()).collect();
        for expected in ["teammates", "threads", "workspaces", "settings", "routines"] {
            assert!(
                names.contains(&expected),
                "{expected} missing from {names:?}"
            );
        }
        assert!(
            !names
                .iter()
                .any(|n| n.starts_with("sqlite_") || n.contains("_fts")),
            "internal tables leaked: {names:?}"
        );
        let teammates = tables
            .iter()
            .find(|t| t.name == "teammates")
            .expect("teammates");
        assert_eq!(teammates.rows.len(), 1);
        assert_eq!(teammates.rows[0]["name"], "Ada");
        assert_eq!(teammates.rows[0]["created_at"], 1);
        assert_eq!(teammates.rows[0]["deleted_at"], Value::Null);
    }

    #[test]
    fn f_tl_05_blobs_and_reals_survive_as_json() {
        assert_eq!(
            json(ValueRef::Blob(&[0, 15, 255])),
            serde_json::json!({ "hex": "000fff" })
        );
        assert_eq!(json(ValueRef::Real(1.5)), serde_json::json!(1.5));
        assert_eq!(json(ValueRef::Text(b"hi")), serde_json::json!("hi"));
    }
}
