use std::path::{Component, Path, PathBuf};

use unicode_normalization::UnicodeNormalization;

use crate::error::Error;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Policy {
    Read,
    Write,
}

pub fn contain(root: &Path, path: &Path, policy: Policy) -> Result<PathBuf, Error> {
    let raw = path.to_string_lossy();
    let folded = raw.nfc().collect::<String>().to_lowercase();
    if folded.contains("%2e") || raw.contains('\u{FF0E}') || raw.contains('\u{FF0F}') {
        return Err(reject("path uses an encoded or lookalike dot"));
    }

    let root = root.canonicalize().map_err(|err| {
        Error::new(
            "contain",
            format!("root {}: {err}", root.display()),
            "create the workspace directory first",
        )
    })?;

    let joined = if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    };

    let resolved = resolve_inside(&root, &joined)?;
    if !resolved.starts_with(&root) {
        return Err(reject("path escapes the workspace"));
    }
    if policy == Policy::Write && is_private_file(&resolved) {
        return Err(reject("refusing to write a private file"));
    }
    Ok(resolved)
}

fn reject(message: &str) -> Error {
    Error::new("contain", message, "choose a path inside the workspace")
}

fn resolve_inside(root: &Path, path: &Path) -> Result<PathBuf, Error> {
    let mut current = root.to_path_buf();
    let relative = path.strip_prefix(root).unwrap_or(path);
    for component in relative.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if !current.pop() || !current.starts_with(root) {
                    return Err(reject("path escapes the workspace"));
                }
            }
            Component::Normal(part) => {
                let text = part.to_string_lossy();
                let normal = text.nfc().collect::<String>();
                if normal.eq_ignore_ascii_case("..") || normal.contains('\u{FF0E}') {
                    return Err(reject("path uses a lookalike parent segment"));
                }
                let next = current.join(part);
                if next.exists() {
                    let canon = next.canonicalize().map_err(|err| {
                        Error::new("contain", format!("canonicalize: {err}"), "check the path")
                    })?;
                    if !canon.starts_with(root) {
                        return Err(reject("symlink escapes the workspace"));
                    }
                    current = canon;
                } else {
                    current = next;
                }
            }
            Component::RootDir | Component::Prefix(_) => {
                return Err(reject("absolute escape"));
            }
        }
    }
    Ok(current)
}

fn is_private_file(path: &Path) -> bool {
    let name = path
        .file_name()
        .and_then(|part| part.to_str())
        .unwrap_or("");
    if name.starts_with(".env") || name.ends_with(".pem") || name.starts_with("id_") {
        return true;
    }
    path.components()
        .any(|component| component.as_os_str() == ".git")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::symlink;

    #[test]
    fn f_tl_03_1_escape_corpus_rejected_nested_accepted() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path().join("ws");
        fs::create_dir_all(root.join("src")).expect("mkdir");
        fs::write(root.join("src").join("main.rs"), b"fn main() {}\n").expect("write");

        let nested = contain(&root, Path::new("src/main.rs"), Policy::Read).expect("nested");
        assert!(nested.ends_with("src/main.rs"));

        let outside = dir.path().join("outside.txt");
        fs::write(&outside, b"nope").expect("outside");
        assert!(contain(&root, Path::new("../outside.txt"), Policy::Read).is_err());
        assert!(contain(&root, Path::new("src/%2e%2e/outside.txt"), Policy::Read).is_err());
        assert!(contain(
            &root,
            Path::new("src/\u{FF0E}\u{FF0E}/outside.txt"),
            Policy::Read
        )
        .is_err());

        let link = root.join("escape");
        symlink(&outside, &link).expect("symlink");
        assert!(contain(&root, Path::new("escape"), Policy::Read).is_err());

        assert!(contain(&root, Path::new(".env"), Policy::Write).is_err());
        assert!(contain(&root, Path::new("keys/id_rsa"), Policy::Write).is_err());
        assert!(contain(&root, Path::new("notes/readme.md"), Policy::Write).is_ok());
    }
}
