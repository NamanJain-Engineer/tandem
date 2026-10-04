use rusqlite::{params, Connection};
use tandem_core::paths::Paths;
use tandem_core::Error;

pub struct Store {
    conn: Connection,
}

impl Store {
    pub fn open(paths: &Paths) -> Result<Self, Error> {
        std::fs::create_dir_all(&paths.data).map_err(|err| {
            Error::new(
                "store",
                format!("create data dir: {err}"),
                "check TANDEM_HOME permissions",
            )
        })?;
        let conn = Connection::open(paths.data.join("tandem.sqlite")).map_err(|err| {
            Error::new("store", format!("open sqlite: {err}"), "check disk space")
        })?;
        conn.pragma_update(None, "journal_mode", "WAL")
            .map_err(|err| Error::new("store", format!("wal: {err}"), "retry"))?;
        conn.pragma_update(None, "foreign_keys", "ON")
            .map_err(|err| Error::new("store", format!("fk: {err}"), "retry"))?;
        conn.execute_batch(include_str!("../migrations/0001_init.sql"))
            .map_err(|err| Error::new("store", format!("migrate: {err}"), "check the migration"))?;
        Ok(Self { conn })
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<(), Error> {
        self.conn
            .execute(
                "INSERT INTO settings (key, value) VALUES (?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                params![key, value],
            )
            .map_err(|err| Error::new("store", format!("set setting: {err}"), "retry"))?;
        Ok(())
    }

    pub fn get_setting(&self, key: &str) -> Result<Option<String>, Error> {
        let mut stmt = self
            .conn
            .prepare("SELECT value FROM settings WHERE key = ?1")
            .map_err(|err| Error::new("store", format!("prepare: {err}"), "retry"))?;
        let mut rows = stmt
            .query(params![key])
            .map_err(|err| Error::new("store", format!("query: {err}"), "retry"))?;
        if let Some(row) = rows
            .next()
            .map_err(|err| Error::new("store", format!("row: {err}"), "retry"))?
        {
            let value: String = row
                .get(0)
                .map_err(|err| Error::new("store", format!("read: {err}"), "retry"))?;
            return Ok(Some(value));
        }
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tandem_core::paths::Paths;

    #[test]
    fn settings_round_trip() {
        let dir = tempfile::tempdir().expect("tempdir");
        let paths = Paths::from_data(dir.path().to_path_buf());
        let store = Store::open(&paths).expect("open");
        store.set_setting("theme", "light").expect("set");
        drop(store);
        let store = Store::open(&paths).expect("reopen");
        assert_eq!(
            store.get_setting("theme").expect("get").as_deref(),
            Some("light")
        );
    }
}
