//! Crash-safe file writes: temp file, fsync, rename, fsync(dir) (PRODUCT_SPEC §5.2).

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

static COUNTER: AtomicU64 = AtomicU64::new(0);

/// Atomically replaces `path` with `bytes`. `mode` sets Unix permissions on the
/// new file (e.g. `Some(0o600)` for files holding private data).
pub fn atomic_write(path: &Path, bytes: &[u8], mode: Option<u32>) -> io::Result<()> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let name = path
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "path has no file name"))?;
    let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
    let mut tmp_name = std::ffi::OsString::from(".");
    tmp_name.push(name);
    tmp_name.push(format!(".{}.{unique}.tmp", std::process::id()));
    let tmp = parent.join(tmp_name);

    let result = (|| {
        let mut options = OpenOptions::new();
        options.create_new(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(mode.unwrap_or(0o644));
        }
        let mut file = options.open(&tmp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        #[cfg(not(unix))]
        let _ = mode;
        fs::rename(&tmp, path)?;
        if let Ok(dir) = File::open(parent) {
            // Directory fsync is unsupported on some platforms; the rename is
            // already durable there.
            let _ = dir.sync_all();
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_replaces_and_leaves_no_temp_files() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("note.json");
        atomic_write(&path, b"one", None).expect("write");
        atomic_write(&path, b"two", Some(0o600)).expect("write");
        assert_eq!(fs::read(&path).expect("read"), b"two");
        let leftovers = fs::read_dir(dir.path())
            .expect("list")
            .filter(|e| {
                e.as_ref()
                    .map(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
                    .unwrap_or(false)
            })
            .count();
        assert_eq!(leftovers, 0);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&path).expect("meta").permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
    }

    #[test]
    fn concurrent_writers_never_tear_the_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("state.json");
        let handles: Vec<_> = (0..8)
            .map(|i| {
                let path = path.clone();
                std::thread::spawn(move || {
                    let body = vec![b'a' + i as u8; 4096];
                    for _ in 0..20 {
                        atomic_write(&path, &body, None).expect("write");
                    }
                })
            })
            .collect();
        for h in handles {
            h.join().expect("join");
        }
        let body = fs::read(&path).expect("read");
        assert_eq!(body.len(), 4096);
        assert!(body.iter().all(|b| *b == body[0]));
    }
}
