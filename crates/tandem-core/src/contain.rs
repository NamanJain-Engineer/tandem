//! One path check for every file operation that does not come straight from
//! the local user's UI action: agents via tools, Conductor, the phone, tasks
//! and SSH (F-TL-03).

use std::path::{Component, Path, PathBuf};

use unicode_normalization::UnicodeNormalization;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContainPolicy {
    /// Reading. Private files (`.env*`, keys) are refused.
    Read,
    /// Writing. Private files and `.git/` internals are refused.
    Write,
    /// Reading where the user explicitly allowed private files.
    ReadAllowPrivate,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ContainError {
    #[error("that path is outside the folder")]
    Escape,
    #[error("that path uses look-alike or encoded characters")]
    Lookalike,
    #[error("that file is private")]
    Private,
    #[error("the folder doesn't exist")]
    RootMissing,
    #[error("couldn't read that path: {0}")]
    Io(String),
}

impl From<ContainError> for crate::error::IpcError {
    fn from(err: ContainError) -> Self {
        crate::error::IpcError::new(crate::error::ErrorCode::PathEscape, err.to_string())
            .with_stage("contain")
    }
}

/// Characters that render like `.`, `/` or `\` and must never appear in a
/// path handed to Tandem by an agent or remote caller.
const LOOKALIKES: [char; 12] = [
    '\u{FF0E}', // fullwidth full stop
    '\u{2024}', // one dot leader
    '\u{FE52}', // small full stop
    '\u{3002}', // ideographic full stop
    '\u{FF61}', // halfwidth ideographic full stop
    '\u{FF0F}', // fullwidth solidus
    '\u{2215}', // division slash
    '\u{2044}', // fraction slash
    '\u{29F8}', // big solidus
    '\u{FF3C}', // fullwidth reverse solidus
    '\u{2216}', // set minus
    '\u{0000}',
];

fn case_insensitive_fs() -> bool {
    cfg!(any(target_os = "macos", target_os = "windows"))
}

fn fold(s: &str) -> String {
    let nfc: String = s.nfc().collect();
    if case_insensitive_fs() {
        nfc.to_lowercase()
    } else {
        nfc
    }
}

fn path_starts_with(path: &Path, root: &Path) -> bool {
    if path.starts_with(root) {
        return true;
    }
    if !case_insensitive_fs() {
        return false;
    }
    let p = fold(&path.to_string_lossy());
    let r = fold(&root.to_string_lossy());
    p == r || p.starts_with(&format!("{r}{}", std::path::MAIN_SEPARATOR))
}

/// `fs::canonicalize`, but `C:\…` on Windows rather than `\\?\C:\…` (when
/// that is the same file), so an absolute request for a file that doesn't
/// exist yet still compares with the root. The same as std on Unix. Callers
/// that compare their own canonical paths with [`contain`]'s use this too.
pub fn canonical(path: &Path) -> std::io::Result<PathBuf> {
    dunce::canonicalize(path)
}

/// Resolves `requested` (relative to `root`, or absolute) and returns the
/// canonical path when it stays inside `root` and the policy allows it.
pub fn contain(
    root: &Path,
    requested: &Path,
    policy: ContainPolicy,
) -> Result<PathBuf, ContainError> {
    let raw = requested.to_string_lossy();
    let lowered = raw.to_ascii_lowercase();
    if lowered.contains("%2e") || lowered.contains("%2f") || lowered.contains("%5c") {
        return Err(ContainError::Lookalike);
    }
    let normalized: String = raw.nfkc().collect();
    if raw.chars().any(|c| LOOKALIKES.contains(&c))
        || (normalized != raw.nfc().collect::<String>() && normalized.contains(['.', '/', '\\']))
    {
        return Err(ContainError::Lookalike);
    }

    let root = canonical(root).map_err(|_| ContainError::RootMissing)?;
    let relative: PathBuf = if requested.is_absolute() {
        if !path_starts_with(requested, &root) {
            // An absolute path may still point inside root through a symlinked
            // prefix; compare canonical forms when it exists.
            match canonical(requested) {
                Ok(canon) if path_starts_with(&canon, &root) => canon
                    .strip_prefix(&root)
                    .map(Path::to_path_buf)
                    .unwrap_or_default(),
                _ => return Err(ContainError::Escape),
            }
        } else {
            let root_len = root.components().count();
            requested.components().skip(root_len).collect()
        }
    } else {
        requested.to_path_buf()
    };

    let mut current = root.clone();
    for component in relative.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if current == root || !current.pop() || !path_starts_with(&current, &root) {
                    return Err(ContainError::Escape);
                }
            }
            Component::Normal(part) => {
                let text = part.to_string_lossy();
                if text.trim_matches([' ', '.']).is_empty() && text.contains('.') {
                    // "...", ". ." and friends: Windows treats them as parents.
                    return Err(ContainError::Lookalike);
                }
                let next = current.join(part);
                match std::fs::symlink_metadata(&next) {
                    Ok(_) => {
                        let canon =
                            canonical(&next).map_err(|e| ContainError::Io(e.kind().to_string()))?;
                        if !path_starts_with(&canon, &root) {
                            return Err(ContainError::Escape);
                        }
                        current = canon;
                    }
                    Err(_) => current = next,
                }
            }
            Component::RootDir | Component::Prefix(_) => return Err(ContainError::Escape),
        }
    }

    if !path_starts_with(&current, &root) {
        return Err(ContainError::Escape);
    }
    let inside = current.strip_prefix(&root).unwrap_or(&current);
    match policy {
        ContainPolicy::ReadAllowPrivate => {}
        ContainPolicy::Read => {
            if is_private_file(inside) {
                return Err(ContainError::Private);
            }
        }
        ContainPolicy::Write => {
            if is_private_file(inside) || in_git_dir(inside) {
                return Err(ContainError::Private);
            }
        }
    }
    Ok(current)
}

/// `.env*`, `*.pem`, `*.key`, `id_*` (SSH keys) anywhere in the tree.
pub fn is_private_file(path: &Path) -> bool {
    let Some(name) = path.file_name().map(|n| n.to_string_lossy().to_lowercase()) else {
        return false;
    };
    name.starts_with(".env")
        || name.ends_with(".pem")
        || name.ends_with(".key")
        || name.ends_with(".p12")
        || name.ends_with(".pfx")
        || (name.starts_with("id_") && !name.ends_with(".pub"))
}

fn in_git_dir(path: &Path) -> bool {
    path.components()
        .any(|c| c.as_os_str().to_string_lossy().eq_ignore_ascii_case(".git"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    const ESCAPES: &str = include_str!("../tests/containment/escapes.txt");

    fn fixture() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path().join("ws");
        fs::create_dir_all(root.join("src/nested/deep")).expect("mkdir");
        fs::create_dir_all(root.join(".git/objects")).expect("mkdir");
        fs::write(root.join("src/main.rs"), b"fn main() {}\n").expect("write");
        fs::write(root.join(".env"), b"KEY=1\n").expect("write");
        fs::create_dir_all(dir.path().join("outside")).expect("mkdir");
        fs::write(dir.path().join("outside/secret.txt"), b"nope").expect("write");
        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;
            symlink(dir.path().join("outside"), root.join("linkdir")).expect("symlink");
            symlink(
                dir.path().join("outside/secret.txt"),
                root.join("src/linkfile"),
            )
            .expect("symlink");
            symlink(root.join("src/nested"), root.join("inner-link")).expect("symlink");
        }
        // Windows: the same links (creating them needs Developer Mode or an
        // elevated shell, as on CI runners).
        #[cfg(windows)]
        {
            use std::os::windows::fs::{symlink_dir, symlink_file};
            symlink_dir(dir.path().join("outside"), root.join("linkdir")).expect("symlink");
            symlink_file(
                dir.path().join("outside/secret.txt"),
                root.join("src/linkfile"),
            )
            .expect("symlink");
            symlink_dir(root.join("src/nested"), root.join("inner-link")).expect("symlink");
        }
        (dir, root)
    }

    #[test]
    fn f_tl_03_1_escape_corpus_rejected_nested_accepted() {
        let (_dir, root) = fixture();
        let mut checked = 0;
        for line in ESCAPES
            .lines()
            .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
        {
            let (expect, path) = line.split_once(' ').expect("'reject|accept <path>'");
            let path = path.replace("{ROOT}", &root.to_string_lossy());
            let result = contain(&root, Path::new(&path), ContainPolicy::Write);
            match expect {
                "reject" => assert!(result.is_err(), "should reject {path:?}: {result:?}"),
                "accept" => {
                    let resolved = result.unwrap_or_else(|e| panic!("should accept {path:?}: {e}"));
                    assert!(resolved.starts_with(canonical(&root).expect("canon")));
                }
                other => panic!("bad corpus verb {other}"),
            }
            checked += 1;
        }
        assert!(checked >= 30, "corpus too small: {checked}");
    }

    #[test]
    fn read_refuses_private_files_unless_allowed() {
        let (_dir, root) = fixture();
        assert_eq!(
            contain(&root, Path::new(".env"), ContainPolicy::Read),
            Err(ContainError::Private)
        );
        assert!(contain(&root, Path::new(".env"), ContainPolicy::ReadAllowPrivate).is_ok());
        assert!(contain(&root, Path::new(".git/HEAD"), ContainPolicy::Read).is_ok());
        assert_eq!(
            contain(&root, Path::new(".git/HEAD"), ContainPolicy::Write),
            Err(ContainError::Private)
        );
    }

    #[cfg(any(target_os = "macos", target_os = "windows"))]
    #[test]
    fn case_variants_of_root_stay_inside_on_case_insensitive_fs() {
        let (_dir, root) = fixture();
        let upper = PathBuf::from(root.to_string_lossy().to_uppercase()).join("src/main.rs");
        assert!(contain(&root, &upper, ContainPolicy::Read).is_ok());
        assert!(contain(&root, Path::new("SRC/../../outside"), ContainPolicy::Read).is_err());
    }
}
