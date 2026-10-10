//! `fs.*` on the host: every path is held to the workspace root by
//! [`tandem_core::contain::contain`] (`..` and symlink escapes are refused,
//! F-RM-01 AC3), then the same file rules as local Files apply
//! (`tandem_core::files`: conflicts by hash, line endings kept, atomic
//! replace).

use std::io::Write;
use std::path::{Path, PathBuf};

use base64::Engine;
use tandem_core::contain::{contain, ContainError, ContainPolicy};
use tandem_core::files::{self, FileKind, FilesError, FsKind};

use crate::protocol::{
    DirListing, Entry, EntryKind, ErrorKind, FileData, FsCreate, FsList, FsPath, FsRename, FsStat,
    FsWrite, FsWritten, ReadKind, RpcError,
};

pub fn map_contain(e: ContainError) -> RpcError {
    match e {
        ContainError::RootMissing => {
            RpcError::new(ErrorKind::NotFound, "The remote folder doesn't exist")
        }
        ContainError::Io(m) => RpcError::new(ErrorKind::Io, m),
        other => RpcError::new(ErrorKind::PathEscape, other.to_string()),
    }
}

pub fn map_files(e: FilesError) -> RpcError {
    match e {
        FilesError::NotFound => RpcError::new(ErrorKind::NotFound, e.to_string()),
        FilesError::Conflict => RpcError::new(ErrorKind::Conflict, e.to_string()),
        FilesError::Exists(_) => RpcError::new(ErrorKind::Exists, e.to_string()),
        FilesError::ReadOnly(_) => RpcError::new(ErrorKind::ReadOnly, e.to_string()),
        FilesError::Invalid(_) => RpcError::new(ErrorKind::Invalid, e.to_string()),
        FilesError::Contain(c) => map_contain(c),
        FilesError::Io(io) if io.kind() == std::io::ErrorKind::NotFound => {
            RpcError::new(ErrorKind::NotFound, "That file or folder doesn't exist")
        }
        FilesError::Io(io) if io.kind() == std::io::ErrorKind::PermissionDenied => RpcError::new(
            ErrorKind::ReadOnly,
            "You don't have permission to change this file",
        ),
        FilesError::Io(io) => RpcError::new(ErrorKind::Io, io.to_string()),
    }
}

/// The canonical root and the contained path. The local user chose both
/// (the Files tab), so private files are readable as they are locally;
/// git's own data is never written.
fn resolve(root: &str, path: &str, write: bool) -> Result<(PathBuf, PathBuf), RpcError> {
    let root = Path::new(root)
        .canonicalize()
        .map_err(|_| RpcError::new(ErrorKind::NotFound, "The remote folder doesn't exist"))?;
    if path.is_empty() {
        return Err(RpcError::new(ErrorKind::Invalid, "No path"));
    }
    let abs =
        contain(&root, Path::new(path), ContainPolicy::ReadAllowPrivate).map_err(map_contain)?;
    if write {
        let rel = abs.strip_prefix(&root).unwrap_or(&abs);
        if rel
            .components()
            .any(|c| c.as_os_str().to_string_lossy().eq_ignore_ascii_case(".git"))
        {
            return Err(RpcError::new(
                ErrorKind::ReadOnly,
                "Vibezeo doesn't change git's own files",
            ));
        }
        if abs == root {
            return Err(RpcError::new(
                ErrorKind::Invalid,
                "That's the workspace folder itself",
            ));
        }
    }
    Ok((root, abs))
}

pub fn list(p: &FsList) -> Result<Vec<Entry>, RpcError> {
    let root = Path::new(&p.root)
        .canonicalize()
        .map_err(|_| RpcError::new(ErrorKind::NotFound, "The remote folder doesn't exist"))?;
    if !p.path.is_empty() {
        // `list_dir` refuses `..` itself; a symlinked folder must stay inside.
        resolve(&p.root, &p.path, false)?;
    }
    let entries = files::list_dir(&root, &p.path, p.show_ignored).map_err(map_files)?;
    Ok(entries
        .into_iter()
        .map(|e| Entry {
            name: e.name,
            path: e.path,
            kind: kind_out(e.kind),
            symlink: e.symlink,
            ignored: e.ignored,
        })
        .collect())
}

fn kind_out(k: FsKind) -> EntryKind {
    match k {
        FsKind::File => EntryKind::File,
        FsKind::Dir => EntryKind::Dir,
    }
}

pub fn read(p: &FsPath) -> Result<FileData, RpcError> {
    let (root, abs) = resolve(&p.root, &p.path, false)?;
    let read = files::read_file(&abs).map_err(map_files)?;
    Ok(FileData {
        path: files::display_path(&root, &abs),
        kind: match read.kind {
            FileKind::Text => ReadKind::Text,
            FileKind::Binary => ReadKind::Binary,
            FileKind::Image => ReadKind::Image,
        },
        text: read.text,
        size: read.size,
        truncated: read.truncated,
        hash: read.hash,
        mtime: read.mtime,
        data_url: read.data_url,
        hex: read.hex,
        large: read.large,
        writable: read.writable,
    })
}

pub fn write(p: &FsWrite) -> Result<FsWritten, RpcError> {
    let (_root, abs) = resolve(&p.root, &p.path, true)?;
    match (&p.text, &p.data_b64) {
        (Some(text), None) => {
            let w = files::write_file(&abs, text, p.expected_hash.as_deref()).map_err(map_files)?;
            Ok(FsWritten {
                hash: w.hash,
                mtime: w.mtime,
            })
        }
        (None, Some(b64)) => {
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(b64)
                .map_err(|_| RpcError::new(ErrorKind::Invalid, "That data isn't base64"))?;
            if p.expected_hash.is_some() {
                return Err(RpcError::new(
                    ErrorKind::Invalid,
                    "Raw writes create new files only",
                ));
            }
            if let Some(parent) = abs.parent() {
                std::fs::create_dir_all(parent).map_err(|e| map_files(e.into()))?;
            }
            let mut f = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&abs)
                .map_err(|e| match e.kind() {
                    std::io::ErrorKind::AlreadyExists => {
                        RpcError::new(ErrorKind::Exists, "That file already exists")
                    }
                    _ => map_files(e.into()),
                })?;
            f.write_all(&bytes).map_err(|e| map_files(e.into()))?;
            let meta = f.metadata().map_err(|e| map_files(e.into()))?;
            if p.path.starts_with(".tandem/") {
                crate::gitops::exclude_tandem(Path::new(&p.root));
            }
            Ok(FsWritten {
                hash: files::hash_bytes(&bytes),
                mtime: mtime(&meta),
            })
        }
        _ => Err(RpcError::new(
            ErrorKind::Invalid,
            "Send text or data, not both",
        )),
    }
}

fn mtime(meta: &std::fs::Metadata) -> i64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

pub fn stat(p: &FsPath) -> Result<FsStat, RpcError> {
    let (_root, abs) = if p.path.is_empty() || p.path == "." {
        let root = Path::new(&p.root)
            .canonicalize()
            .map_err(|_| RpcError::new(ErrorKind::NotFound, "The remote folder doesn't exist"))?;
        (root.clone(), root)
    } else {
        resolve(&p.root, &p.path, false)?
    };
    let meta = std::fs::metadata(&abs).map_err(|e| map_files(e.into()))?;
    Ok(FsStat {
        kind: if meta.is_dir() {
            EntryKind::Dir
        } else {
            EntryKind::File
        },
        size: meta.len(),
        mtime: mtime(&meta),
    })
}

pub fn create(p: &FsCreate) -> Result<(), RpcError> {
    let (_root, abs) = resolve(&p.root, &p.path, true)?;
    let kind = match p.kind {
        EntryKind::File => FsKind::File,
        EntryKind::Dir => FsKind::Dir,
    };
    files::create(&abs, kind).map_err(map_files)
}

pub fn rename(p: &FsRename) -> Result<(), RpcError> {
    let (_r, from) = resolve(&p.root, &p.from, true)?;
    let (_r, to) = resolve(&p.root, &p.to, true)?;
    files::rename(&from, &to).map_err(map_files)
}

pub fn delete(p: &FsPath) -> Result<(), RpcError> {
    let (_root, abs) = resolve(&p.root, &p.path, true)?;
    files::delete(&abs).map_err(map_files)
}

/// The folder picker: sub-folders of `path` (absolute; empty or `~` is
/// home), hidden ones last-but-listed, sorted by name.
pub fn dirs(path: &str, home: &Path) -> Result<DirListing, RpcError> {
    let wanted = match path.trim() {
        "" | "~" => home.to_path_buf(),
        p if p.starts_with("~/") => home.join(&p[2..]),
        p if Path::new(p).is_absolute() => PathBuf::from(p),
        _ => return Err(RpcError::new(ErrorKind::Invalid, "Use an absolute path")),
    };
    let canon = wanted
        .canonicalize()
        .map_err(|_| RpcError::new(ErrorKind::NotFound, "That folder doesn't exist on the host"))?;
    if !canon.is_dir() {
        return Err(RpcError::new(ErrorKind::Invalid, "That's not a folder"));
    }
    let mut dirs: Vec<String> = std::fs::read_dir(&canon)
        .map_err(|e| map_files(e.into()))?
        .flatten()
        .filter(|e| e.path().is_dir())
        .filter_map(|e| e.file_name().to_str().map(str::to_string))
        .collect();
    dirs.sort_by_key(|a| (a.starts_with('.'), a.to_lowercase()));
    Ok(DirListing {
        parent: canon.parent().map(|p| p.to_string_lossy().into_owned()),
        path: canon.to_string_lossy().into_owned(),
        dirs,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (tempfile::TempDir, String) {
        let dir = tempfile::tempdir().expect("tmp");
        let root = dir.path().join("home/tandem/project");
        std::fs::create_dir_all(root.join("src")).expect("mkdir");
        std::fs::write(root.join("src/main.rs"), "fn main() {}\n").expect("write");
        std::fs::write(dir.path().join("home/secret.txt"), "outside").expect("write");
        (dir, root.to_string_lossy().into_owned())
    }

    #[test]
    fn f_rm_01_3_dotdot_out_of_the_root_is_refused() {
        let (_d, root) = fixture();
        let err = read(&FsPath {
            root: root.clone(),
            path: "../../etc/passwd".into(),
        })
        .expect_err("refused");
        assert_eq!(err.kind, ErrorKind::PathEscape);
        let err = read(&FsPath {
            root: root.clone(),
            path: "../../secret.txt".into(),
        })
        .expect_err("refused");
        assert_eq!(err.kind, ErrorKind::PathEscape);
        let err = read(&FsPath {
            root,
            path: "/etc/passwd".into(),
        })
        .expect_err("refused");
        assert_eq!(err.kind, ErrorKind::PathEscape);
    }

    #[cfg(unix)]
    #[test]
    fn f_rm_01_3_a_symlink_out_of_the_root_is_refused() {
        let (d, root) = fixture();
        std::os::unix::fs::symlink(d.path().join("home"), Path::new(&root).join("up"))
            .expect("link");
        let err = read(&FsPath {
            root: root.clone(),
            path: "up/secret.txt".into(),
        })
        .expect_err("refused");
        assert_eq!(err.kind, ErrorKind::PathEscape);
        let err = write(&FsWrite {
            root,
            path: "up/new.txt".into(),
            text: Some("x".into()),
            data_b64: None,
            expected_hash: None,
        })
        .expect_err("refused");
        assert_eq!(err.kind, ErrorKind::PathEscape);
    }

    #[test]
    fn f_rm_01_files_read_write_conflict_and_list() {
        let (_d, root) = fixture();
        let f = read(&FsPath {
            root: root.clone(),
            path: "src/main.rs".into(),
        })
        .expect("read");
        assert_eq!(f.kind, ReadKind::Text);
        assert_eq!(f.text.as_deref(), Some("fn main() {}\n"));
        let saved = write(&FsWrite {
            root: root.clone(),
            path: "src/main.rs".into(),
            text: Some("fn main() { println!(); }\n".into()),
            data_b64: None,
            expected_hash: Some(f.hash.clone()),
        })
        .expect("save");
        assert_ne!(saved.hash, f.hash);
        let stale = write(&FsWrite {
            root: root.clone(),
            path: "src/main.rs".into(),
            text: Some("lost".into()),
            data_b64: None,
            expected_hash: Some(f.hash),
        })
        .expect_err("conflict");
        assert_eq!(stale.kind, ErrorKind::Conflict);
        let entries = list(&FsList {
            root: root.clone(),
            path: String::new(),
            show_ignored: false,
        })
        .expect("list");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].path, "src");
        let git = create(&FsCreate {
            root,
            path: ".git/hooks/x".into(),
            kind: EntryKind::File,
        })
        .expect_err("refused");
        assert_eq!(git.kind, ErrorKind::ReadOnly);
    }

    #[test]
    fn f_rm_01_attachments_are_written_once_under_the_root() {
        let (_d, root) = fixture();
        let req = FsWrite {
            root: root.clone(),
            path: ".tandem/attachments/th_1/shot.png".into(),
            text: None,
            data_b64: Some(
                base64::engine::general_purpose::STANDARD.encode([0x89, b'P', b'N', b'G']),
            ),
            expected_hash: None,
        };
        write(&req).expect("written");
        assert_eq!(
            std::fs::read(Path::new(&root).join(".tandem/attachments/th_1/shot.png"))
                .expect("read"),
            [0x89, b'P', b'N', b'G']
        );
        assert_eq!(write(&req).expect_err("exists").kind, ErrorKind::Exists);
    }

    #[test]
    fn f_rm_01_folder_picker_lists_folders_from_home() {
        let (d, _root) = fixture();
        let home = d.path().join("home");
        let l = dirs("~", &home).expect("dirs");
        assert_eq!(l.dirs, ["tandem"]);
        let l = dirs("~/tandem", &home).expect("dirs");
        assert_eq!(l.dirs, ["project"]);
        assert!(dirs("relative", &home).is_err());
    }
}
