use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::Path;

use crate::error::Error;

/// Write `data` via a temp file in the same directory, fsync, then rename.
pub fn atomic_write(path: &Path, data: &[u8]) -> Result<(), Error> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent).map_err(|err| {
        Error::new(
            "atomic",
            format!("create {}: {err}", parent.display()),
            "check directory permissions",
        )
    })?;
    let mut tmp_name = path
        .file_name()
        .ok_or_else(|| Error::new("atomic", "path has no file name", "pass a file path"))?
        .to_os_string();
    tmp_name.push(".tmp");
    let tmp = parent.join(tmp_name);
    {
        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&tmp)
            .map_err(|err| {
                Error::new(
                    "atomic",
                    format!("open {}: {err}", tmp.display()),
                    "check directory permissions",
                )
            })?;
        file.write_all(data)
            .map_err(|err| Error::new("atomic", format!("write: {err}"), "retry the write"))?;
        file.sync_all()
            .map_err(|err| Error::new("atomic", format!("fsync: {err}"), "retry the write"))?;
    }
    fs::rename(&tmp, path).map_err(|err| {
        Error::new(
            "atomic",
            format!("rename {} -> {}: {err}", tmp.display(), path.display()),
            "retry the write",
        )
    })?;
    if let Ok(dir) = File::open(parent) {
        let _ = dir.sync_all();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_survives_reopen() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("note.txt");
        atomic_write(&path, b"hello").expect("write");
        assert_eq!(fs::read(&path).expect("read"), b"hello");
        assert!(!dir.path().join("note.txt.tmp").exists());
    }
}
