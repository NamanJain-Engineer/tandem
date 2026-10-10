//! Links in what programs print (F-CD-09, ARCHITECTURE §5.9): a path such
//! as `src/main.rs:42:7`, taken from the pane's folder when it's relative,
//! becomes a link when it names a file; http(s) URLs are links as they are.

use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::inside_any;

/// Longer than any path a link could name.
const MAX_TEXT: usize = 4096;
/// What a sentence can put after a path: `see src/a.rs.` or `(src/a.rs)`.
const TRAILING: &[char] = &['.', ',', ';', ':', ')', ']', '}', '>', '\'', '"', '!', '?'];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum LinkKind {
    Url,
    File,
}

/// `pane_resolve_link`: what a link in a pane opens.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ResolvedLink {
    pub kind: LinkKind,
    /// The URL, or the file's absolute path.
    pub target: String,
    pub line: Option<u32>,
    pub col: Option<u32>,
    /// A file outside every workspace: it opens read-only.
    pub readonly: bool,
}

/// A positive number of a few digits (a line or a column).
fn number(text: &str) -> Option<u32> {
    if text.is_empty() || text.len() > 9 || !text.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    text.parse().ok().filter(|n| *n > 0)
}

/// `path[:line[:col]]` apart. A drive letter's colon (`C:\a.rs`) is the
/// path's own.
fn location(text: &str) -> (&str, Option<u32>, Option<u32>) {
    let Some((head, last)) = text.rsplit_once(':') else {
        return (text, None, None);
    };
    let Some(last) = number(last) else {
        return (text, None, None);
    };
    match head.rsplit_once(':') {
        Some((path, line)) => match number(line) {
            Some(line) => (path, Some(line), Some(last)),
            None => (head, Some(last), None),
        },
        None => (head, Some(last), None),
    }
}

/// `.` and `..` taken out the way the path reads (not through links), so
/// `/w/./src/../a.rs` shows as `/w/a.rs`.
fn tidy(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    out.push(part);
                }
            }
            other => out.push(other),
        }
    }
    out
}

fn file(text: &str, cwd: Option<&Path>, roots: &[PathBuf]) -> Option<ResolvedLink> {
    let (path, line, col) = location(text);
    if path.is_empty() {
        return None;
    }
    // Windows ignores a name's trailing dots and spaces, so `src/a.rs.` would
    // open `src/a.rs` with the sentence's full stop in the link: no file
    // name there ends that way, so it's punctuation.
    if cfg!(windows) && path.ends_with(['.', ' ']) {
        return None;
    }
    let path = Path::new(path);
    let abs = if path.is_absolute() {
        tidy(path)
    } else {
        tidy(&cwd?.join(path))
    };
    // Regular files only: never a folder, a device or a pipe.
    if !std::fs::metadata(&abs).ok()?.is_file() {
        return None;
    }
    Some(ResolvedLink {
        kind: LinkKind::File,
        readonly: !inside_any(&abs, roots),
        target: abs.to_string_lossy().into_owned(),
        line,
        col: line.and(col),
    })
}

/// What `text`, a link candidate in a pane, opens: an http(s) URL, or a
/// file that exists, with relative paths taken from `cwd` (the pane's
/// folder). Punctuation a sentence put after a path is tried without.
/// Files outside every one of `roots` (the workspaces) are read-only.
pub fn resolve(text: &str, cwd: Option<&Path>, roots: &[PathBuf]) -> Option<ResolvedLink> {
    let text = text.trim();
    if text.is_empty() || text.len() > MAX_TEXT {
        return None;
    }
    if text.starts_with("http://") || text.starts_with("https://") {
        return Some(ResolvedLink {
            kind: LinkKind::Url,
            target: text.to_string(),
            line: None,
            col: None,
            readonly: false,
        });
    }
    file(text, cwd, roots).or_else(|| {
        let bare = text.trim_end_matches(TRAILING);
        (bare != text).then(|| file(bare, cwd, roots)).flatten()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    struct Fixture {
        _dir: tempfile::TempDir,
        root: PathBuf,
        outside: PathBuf,
    }

    /// A workspace `repo` (with `src/main.rs` and `sub/src/main.rs`) and a
    /// folder outside it.
    fn fixture() -> Fixture {
        let dir = tempfile::tempdir().expect("tempdir");
        let base = crate::contain::canonical(dir.path()).expect("canonical");
        let root = base.join("repo");
        fs::create_dir_all(root.join("src")).expect("src");
        fs::create_dir_all(root.join("sub/src")).expect("sub");
        fs::write(root.join("src/main.rs"), "fn main() {}\n").expect("main");
        fs::write(root.join("sub/src/main.rs"), "// sub\n").expect("sub main");
        let outside = base.join("elsewhere");
        fs::create_dir_all(&outside).expect("outside");
        fs::write(outside.join("hosts"), "127.0.0.1 localhost\n").expect("hosts");
        Fixture {
            _dir: dir,
            root,
            outside,
        }
    }

    fn file_link(
        target: &Path,
        line: Option<u32>,
        col: Option<u32>,
        readonly: bool,
    ) -> Option<ResolvedLink> {
        Some(ResolvedLink {
            kind: LinkKind::File,
            target: target.to_string_lossy().into_owned(),
            line,
            col,
            readonly,
        })
    }

    #[test]
    fn f_cd_09_1_a_relative_path_with_line_and_column_resolves_against_the_cwd() {
        let f = fixture();
        let roots = [f.root.clone()];
        let main = f.root.join("src").join("main.rs");
        assert_eq!(
            resolve("src/main.rs:42:7", Some(&f.root), &roots),
            file_link(&main, Some(42), Some(7), false)
        );
        assert_eq!(
            resolve("./src/main.rs:42", Some(&f.root), &roots),
            file_link(&main, Some(42), None, false)
        );
        assert_eq!(
            resolve("sub/../src/main.rs", Some(&f.root), &roots),
            file_link(&main, None, None, false)
        );
        // From a folder below, `..` climbs back out.
        assert_eq!(
            resolve("../src/main.rs:3", Some(&f.root.join("sub")), &roots),
            file_link(&main, Some(3), None, false)
        );
        // Without a folder only absolute paths resolve.
        assert_eq!(resolve("src/main.rs", None, &roots), None);
        let abs = format!("{}:9:2", main.display());
        assert_eq!(
            resolve(&abs, None, &roots),
            file_link(&main, Some(9), Some(2), false)
        );
    }

    #[test]
    fn f_cd_09_2_a_file_outside_every_workspace_is_read_only() {
        let f = fixture();
        let roots = [f.root.clone()];
        let hosts = f.outside.join("hosts");
        assert_eq!(
            resolve(&hosts.to_string_lossy(), Some(&f.root), &roots),
            file_link(&hosts, None, None, true)
        );
        // Another workspace's file is editable too.
        assert_eq!(
            resolve(
                &hosts.to_string_lossy(),
                Some(&f.root),
                &[f.root.clone(), f.outside.clone()]
            ),
            file_link(&hosts, None, None, false)
        );
        #[cfg(unix)]
        if Path::new("/etc/hosts").is_file() {
            assert!(
                resolve("/etc/hosts", Some(&f.root), &roots)
                    .expect("hosts")
                    .readonly
            );
        }
    }

    #[test]
    fn f_cd_09_3_after_cd_sub_a_relative_path_resolves_against_sub() {
        let f = fixture();
        let roots = [f.root.clone()];
        let sub = f.root.join("sub");
        assert_eq!(
            resolve("src/main.rs:1", Some(&sub), &roots),
            file_link(&sub.join("src").join("main.rs"), Some(1), None, false)
        );
    }

    #[test]
    fn f_cd_09_only_files_that_exist_are_links_and_a_sentence_s_punctuation_is_dropped() {
        let f = fixture();
        let roots = [f.root.clone()];
        let main = f.root.join("src").join("main.rs");
        for text in [
            "src/main.rs.",
            "src/main.rs:42:7,",
            "src/main.rs)",
            "src/main.rs:",
        ] {
            let found = resolve(text, Some(&f.root), &roots).unwrap_or_else(|| panic!("{text}"));
            assert_eq!(found.target, main.to_string_lossy(), "{text}");
        }
        assert_eq!(
            resolve("src/main.rs:42:7,", Some(&f.root), &roots).and_then(|l| l.line),
            Some(42)
        );
        // A folder, a missing file, a line of 0: no file link.
        assert_eq!(resolve("src/", Some(&f.root), &roots), None);
        assert_eq!(resolve("src/nope.rs:3", Some(&f.root), &roots), None);
        assert_eq!(resolve("src/main.rs:0", Some(&f.root), &roots), None);
        assert_eq!(resolve("", Some(&f.root), &roots), None);
        assert_eq!(resolve(&"a/".repeat(3000), Some(&f.root), &roots), None);
        #[cfg(unix)]
        assert_eq!(resolve("/dev/null", Some(&f.root), &roots), None);
    }

    #[test]
    fn f_cd_09_urls_are_links_as_they_are() {
        let link = resolve("https://example.com/a?b=1", None, &[]).expect("url");
        assert_eq!(
            (link.kind, link.target.as_str()),
            (LinkKind::Url, "https://example.com/a?b=1")
        );
        assert_eq!(resolve("ftp://example.com/", None, &[]), None);
    }

    #[test]
    fn f_cd_09_a_line_and_column_come_apart_from_the_path() {
        assert_eq!(location("src/a.rs:42:7"), ("src/a.rs", Some(42), Some(7)));
        assert_eq!(location("src/a.rs:42"), ("src/a.rs", Some(42), None));
        assert_eq!(location("src/a.rs"), ("src/a.rs", None, None));
        assert_eq!(location(r"C:\w\a.rs:3:4"), (r"C:\w\a.rs", Some(3), Some(4)));
        assert_eq!(location(r"C:\w\a.rs"), (r"C:\w\a.rs", None, None));
        assert_eq!(location("a:b.rs:x"), ("a:b.rs:x", None, None));
    }
}
