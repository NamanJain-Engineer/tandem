//! Rotating, redacted local logs (F-SH-11). Every formatted line passes through
//! the shared redactor before it reaches disk. 10 MB × 5 files in `$LOGS`.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use tracing_subscriber::fmt::MakeWriter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;

use crate::error::CoreError;
use crate::redact::Redactor;

pub const LOG_FILE: &str = "tandem.log";
pub const MAX_BYTES: u64 = 10 * 1024 * 1024;
/// `tandem.log` plus `tandem.log.1` … `tandem.log.4`.
pub const KEEP: usize = 5;

struct Inner {
    dir: PathBuf,
    name: String,
    max_bytes: u64,
    file: Option<File>,
    size: u64,
}

impl Inner {
    fn path(&self) -> PathBuf {
        self.dir.join(&self.name)
    }

    fn open(&mut self) -> io::Result<&mut File> {
        if self.file.is_none() {
            fs::create_dir_all(&self.dir)?;
            let file = OpenOptions::new()
                .create(true)
                .append(true)
                .open(self.path())?;
            self.size = file.metadata().map(|m| m.len()).unwrap_or(0);
            self.file = Some(file);
        }
        self.file
            .as_mut()
            .ok_or_else(|| io::Error::other("log file not open"))
    }

    fn rotate(&mut self) -> io::Result<()> {
        self.file = None;
        let base = self.path();
        let numbered = |i: usize| self.dir.join(format!("{}.{i}", self.name));
        let oldest = numbered(KEEP - 1);
        if oldest.exists() {
            fs::remove_file(&oldest)?;
        }
        for i in (1..KEEP - 1).rev() {
            let from = numbered(i);
            if from.exists() {
                fs::rename(&from, numbered(i + 1))?;
            }
        }
        if base.exists() {
            fs::rename(&base, numbered(1))?;
        }
        self.size = 0;
        Ok(())
    }

    fn write_line(&mut self, bytes: &[u8]) -> io::Result<()> {
        if self.size > 0 && self.size + bytes.len() as u64 > self.max_bytes {
            self.rotate()?;
        }
        let file = self.open()?;
        file.write_all(bytes)?;
        self.size += bytes.len() as u64;
        Ok(())
    }
}

/// A size-rotating log file that redacts every write.
#[derive(Clone)]
pub struct RedactingFile {
    inner: Arc<Mutex<Inner>>,
}

impl RedactingFile {
    pub fn new(dir: &Path, name: &str, max_bytes: u64) -> Self {
        Self {
            inner: Arc::new(Mutex::new(Inner {
                dir: dir.to_path_buf(),
                name: name.to_string(),
                max_bytes,
                file: None,
                size: 0,
            })),
        }
    }

    pub fn write_text(&self, text: &str) -> io::Result<()> {
        let clean = Redactor::global().redact(text);
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        inner.write_line(clean.as_bytes())
    }
}

pub struct LineWriter {
    file: RedactingFile,
    buf: Vec<u8>,
}

impl Write for LineWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.buf.extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        if self.buf.is_empty() {
            return Ok(());
        }
        let text = String::from_utf8_lossy(&self.buf).into_owned();
        self.buf.clear();
        self.file.write_text(&text)
    }
}

impl Drop for LineWriter {
    fn drop(&mut self) {
        let _ = self.flush();
    }
}

impl<'a> MakeWriter<'a> for RedactingFile {
    type Writer = LineWriter;

    fn make_writer(&'a self) -> Self::Writer {
        LineWriter {
            file: self.clone(),
            buf: Vec::with_capacity(256),
        }
    }
}

/// Installs the global tracing subscriber writing to `$LOGS/tandem.log`.
/// `level` is a filter such as `info` or `tandem=debug,info`.
pub fn init(logs_dir: &Path, level: &str) -> Result<RedactingFile, CoreError> {
    let file = RedactingFile::new(logs_dir, LOG_FILE, MAX_BYTES);
    let filter = EnvFilter::try_new(level).unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::registry()
        .with(filter)
        .with(
            tracing_subscriber::fmt::layer()
                .with_ansi(false)
                .with_target(true)
                .with_writer(file.clone()),
        )
        .try_init()
        .map_err(|e| CoreError::new("logging", e.to_string(), "logging is already initialised"))?;
    Ok(file)
}

/// The last `n` lines of the current log file, oldest first.
pub fn tail(logs_dir: &Path, n: usize) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    let current = logs_dir.join(LOG_FILE);
    let previous = logs_dir.join(format!("{LOG_FILE}.1"));
    for path in [previous, current] {
        if let Ok(text) = fs::read_to_string(&path) {
            lines.extend(text.lines().map(str::to_string));
        }
    }
    let start = lines.len().saturating_sub(n);
    lines.split_off(start)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tracing_subscriber::fmt;

    #[test]
    fn f_sh_11_1_secret_patterns_absent_from_log_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let file = RedactingFile::new(dir.path(), LOG_FILE, MAX_BYTES);
        let subscriber = tracing_subscriber::registry()
            .with(fmt::layer().with_ansi(false).with_writer(file.clone()));
        let secrets = [
            "sk-proj-AbCdEfGhIjKlMnOpQrStUvWxYz0123456789",
            "ghp_0123456789abcdefghijABCDEFGHIJ012345",
            "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.dozjgNryP4J3jVmNHl0w5N_XgL0n3I9PlFUP0THsR8U",
            "-----BEGIN RSA PRIVATE KEY-----\nMIIEowIBAAKCAQEA7bq5\n-----END RSA PRIVATE KEY-----",
        ];
        tracing::subscriber::with_default(subscriber, || {
            for s in &secrets {
                tracing::info!(value = %s, "engine said {s}");
            }
        });
        let body = fs::read_to_string(dir.path().join(LOG_FILE)).expect("read");
        for s in &secrets {
            for piece in s.lines().filter(|l| l.len() > 12 && !l.contains("-----")) {
                assert!(!body.contains(piece), "leaked {piece}");
            }
        }
        assert!(body.contains("[REDACTED]"));
    }

    #[test]
    fn rotation_keeps_five_files() {
        let dir = tempfile::tempdir().expect("tempdir");
        let file = RedactingFile::new(dir.path(), LOG_FILE, 1024);
        let line = format!("{}\n", "x".repeat(300));
        for _ in 0..100 {
            file.write_text(&line).expect("write");
        }
        let count = fs::read_dir(dir.path()).expect("ls").count();
        assert_eq!(count, KEEP);
        assert!(dir.path().join(format!("{LOG_FILE}.4")).exists());
        assert!(fs::metadata(dir.path().join(LOG_FILE)).expect("meta").len() <= 1024);
    }
}
