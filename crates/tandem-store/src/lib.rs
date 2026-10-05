//! Local storage: one SQLite database in WAL mode with a single writer thread
//! and a small read pool (ARCHITECTURE §4.1), plus file stores under `$DATA`.

pub mod migrate;
pub mod repo;
pub mod sort_key;

use std::path::{Path, PathBuf};
use std::sync::mpsc::{sync_channel, SyncSender};
use std::sync::{Arc, Mutex};

use rusqlite::{Connection, OpenFlags, Transaction};
use tandem_core::error::{CoreError, ErrorCode, IpcError};
use tandem_core::DataDir;
use tokio::sync::{oneshot, Semaphore};

pub const WRITE_QUEUE: usize = 256;
pub const READERS: usize = 4;

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("database error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("Tandem couldn't upgrade its data. Your data is unchanged. ({0})")]
    Migration(String),
    #[error("{0}")]
    NotFound(String),
    #[error("{0}")]
    Invalid(String),
    #[error("{0}")]
    Conflict(String),
    #[error("{0}")]
    LimitReached(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("the store is closed")]
    Closed,
    #[error("{0}")]
    Core(#[from] CoreError),
}

impl From<StoreError> for IpcError {
    fn from(err: StoreError) -> Self {
        let code = match &err {
            StoreError::NotFound(_) => ErrorCode::NotFound,
            StoreError::Invalid(_) => ErrorCode::Invalid,
            StoreError::Conflict(_) => ErrorCode::Conflict,
            StoreError::LimitReached(_) => ErrorCode::LimitReached,
            StoreError::Io(_) => ErrorCode::Io,
            StoreError::Core(c) => c.code,
            _ => ErrorCode::Internal,
        };
        IpcError::new(code, err.to_string()).with_stage("store")
    }
}

type Job = Box<dyn FnOnce(&mut Connection) + Send>;

/// Handle to the local database. Cheap to clone.
#[derive(Clone)]
pub struct Store {
    writer: SyncSender<Job>,
    readers: Arc<Mutex<Vec<Connection>>>,
    permits: Arc<Semaphore>,
    path: PathBuf,
}

fn configure(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL; PRAGMA foreign_keys=ON;
         PRAGMA busy_timeout=5000; PRAGMA temp_store=MEMORY;",
    )
}

impl Store {
    pub fn open(data: &DataDir) -> Result<Self, StoreError> {
        Self::open_at(&data.db_path(), &data.backups())
    }

    pub fn open_at(db: &Path, backups: &Path) -> Result<Self, StoreError> {
        let mut conn = Connection::open(db)?;
        configure(&conn)?;
        migrate::run(&mut conn, db, backups)?;

        let mut readers = Vec::with_capacity(READERS);
        for _ in 0..READERS {
            let reader = Connection::open_with_flags(
                db,
                OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
            )?;
            reader.execute_batch("PRAGMA busy_timeout=5000; PRAGMA foreign_keys=ON;")?;
            readers.push(reader);
        }

        let (tx, rx) = sync_channel::<Job>(WRITE_QUEUE);
        std::thread::Builder::new()
            .name("tandem-store-writer".into())
            .spawn(move || {
                while let Ok(job) = rx.recv() {
                    job(&mut conn);
                }
            })?;

        Ok(Self {
            writer: tx,
            readers: Arc::new(Mutex::new(readers)),
            permits: Arc::new(Semaphore::new(READERS)),
            path: db.to_path_buf(),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Runs `f` inside one write transaction on the writer thread.
    pub async fn write<T, F>(&self, f: F) -> Result<T, StoreError>
    where
        T: Send + 'static,
        F: FnOnce(&Transaction) -> Result<T, StoreError> + Send + 'static,
    {
        let (tx, rx) = oneshot::channel();
        let job: Job = Box::new(move |conn| {
            let result = (|| {
                let txn = conn.transaction()?;
                let out = f(&txn)?;
                txn.commit()?;
                Ok(out)
            })();
            let _ = tx.send(result);
        });
        let sender = self.writer.clone();
        tokio::task::spawn_blocking(move || sender.send(job))
            .await
            .map_err(|_| StoreError::Closed)?
            .map_err(|_| StoreError::Closed)?;
        rx.await.map_err(|_| StoreError::Closed)?
    }

    /// Blocking variant for code that runs outside the async runtime
    /// (headless routines, tests).
    pub fn write_blocking<T, F>(&self, f: F) -> Result<T, StoreError>
    where
        T: Send + 'static,
        F: FnOnce(&Transaction) -> Result<T, StoreError> + Send + 'static,
    {
        let (tx, rx) = std::sync::mpsc::channel();
        let job: Job = Box::new(move |conn| {
            let result = (|| {
                let txn = conn.transaction()?;
                let out = f(&txn)?;
                txn.commit()?;
                Ok(out)
            })();
            let _ = tx.send(result);
        });
        self.writer.send(job).map_err(|_| StoreError::Closed)?;
        rx.recv().map_err(|_| StoreError::Closed)?
    }

    /// Runs `f` on one of the read-only connections.
    pub async fn read<T, F>(&self, f: F) -> Result<T, StoreError>
    where
        T: Send + 'static,
        F: FnOnce(&Connection) -> Result<T, StoreError> + Send + 'static,
    {
        let _permit = self
            .permits
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| StoreError::Closed)?;
        let readers = self.readers.clone();
        tokio::task::spawn_blocking(move || {
            let conn = {
                let mut pool = readers.lock().unwrap_or_else(|e| e.into_inner());
                pool.pop()
            };
            let Some(conn) = conn else {
                return Err(StoreError::Closed);
            };
            let result = f(&conn);
            readers.lock().unwrap_or_else(|e| e.into_inner()).push(conn);
            result
        })
        .await
        .map_err(|_| StoreError::Closed)?
    }

    pub fn read_blocking<T, F>(&self, f: F) -> Result<T, StoreError>
    where
        F: FnOnce(&Connection) -> Result<T, StoreError>,
    {
        let conn = {
            let mut pool = self.readers.lock().unwrap_or_else(|e| e.into_inner());
            pool.pop()
        };
        let Some(conn) = conn else {
            return Err(StoreError::Closed);
        };
        let result = f(&conn);
        self.readers
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(conn);
        result
    }
}

/// Unix milliseconds.
pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn writes_are_visible_to_readers() {
        let dir = tempfile::tempdir().expect("tempdir");
        let data = DataDir::at(dir.path().to_path_buf()).expect("data");
        let store = Store::open(&data).expect("open");
        store
            .write(|t| {
                t.execute(
                    "INSERT INTO settings (key, value_json, updated_at) VALUES ('theme', '\"light\"', 1)",
                    [],
                )?;
                Ok(())
            })
            .await
            .expect("write");
        let got: String = store
            .read(|c| {
                Ok(c.query_row(
                    "SELECT value_json FROM settings WHERE key='theme'",
                    [],
                    |r| r.get(0),
                )?)
            })
            .await
            .expect("read");
        assert_eq!(got, "\"light\"");
    }

    #[tokio::test]
    async fn failed_write_rolls_back() {
        let dir = tempfile::tempdir().expect("tempdir");
        let data = DataDir::at(dir.path().to_path_buf()).expect("data");
        let store = Store::open(&data).expect("open");
        let result: Result<(), _> = store
            .write(|t| {
                t.execute(
                    "INSERT INTO settings (key, value_json, updated_at) VALUES ('a', '1', 1)",
                    [],
                )?;
                Err(StoreError::Invalid("nope".into()))
            })
            .await;
        assert!(result.is_err());
        let count: i64 = store
            .read(|c| Ok(c.query_row("SELECT COUNT(*) FROM settings", [], |r| r.get(0))?))
            .await
            .expect("read");
        assert_eq!(count, 0);
    }
}
