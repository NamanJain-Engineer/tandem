//! Worktree setup (F-WT-01): the repository's `.vibezeo/worktree.json`, the
//! copy of ignored files into a new worktree, and the digest that trust is
//! keyed on. Running the commands and keeping trust is the app's job.

use std::path::{Component, Path, PathBuf};

use ignore::gitignore::{Gitignore, GitignoreBuilder};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use ts_rs::TS;

/// Relative to the main checkout's root.
pub const SETUP_FILE: &str = ".vibezeo/worktree.json";
pub const MAX_FILE_BYTES: u64 = 64 * 1024;
pub const MAX_PATTERNS: usize = 100;
pub const MAX_COMMAND_CHARS: usize = 4_000;
pub const MAX_COPY_FILES: usize = 2_000;
pub const MAX_COPY_BYTES: u64 = 512 * 1024 * 1024;
/// Entries the walk looks at before it gives up (a guard, not a feature).
pub const MAX_WALK_ENTRIES: usize = 200_000;
/// Never walked: git's own data and Vibezeo's worktrees.
const SKIP_DIRS: [&str; 2] = [".git", ".tandem"];

#[derive(Debug, thiserror::Error)]
pub enum SetupError {
    #[error("{SETUP_FILE} isn't valid: {0}")]
    Malformed(String),
    #[error("{0}")]
    Invalid(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

/// `.vibezeo/worktree.json`. Unknown keys are ignored.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct WorktreeSetup {
    /// gitignore-syntax patterns, relative to the repository root.
    #[serde(default)]
    pub copy: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub setup: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub teardown: Option<String>,
}

impl WorktreeSetup {
    /// Parses and checks the limits. Blank commands and patterns drop out.
    pub fn parse(bytes: &[u8]) -> Result<Self, SetupError> {
        if bytes.len() as u64 > MAX_FILE_BYTES {
            return Err(SetupError::Malformed("it's larger than 64 KB".into()));
        }
        let raw: WorktreeSetup = serde_json::from_slice(bytes).map_err(|e| {
            SetupError::Malformed(format!("line {}, column {}", e.line(), e.column()))
        })?;
        raw.normalized()
    }

    /// Trims, drops blanks and checks the limits.
    pub fn normalized(self) -> Result<Self, SetupError> {
        let copy: Vec<String> = self
            .copy
            .into_iter()
            .map(|p| p.trim().to_string())
            .filter(|p| !p.is_empty() && !p.starts_with('#'))
            .collect();
        if copy.len() > MAX_PATTERNS {
            return Err(SetupError::Invalid(format!(
                "Use at most {MAX_PATTERNS} copy patterns"
            )));
        }
        let command = |c: Option<String>, what: &str| -> Result<Option<String>, SetupError> {
            let c = c.map(|c| c.trim().to_string()).filter(|c| !c.is_empty());
            if let Some(c) = &c {
                if c.chars().count() > MAX_COMMAND_CHARS {
                    return Err(SetupError::Invalid(format!(
                        "The {what} command is longer than {MAX_COMMAND_CHARS} characters"
                    )));
                }
                if c.contains(['\n', '\r']) {
                    return Err(SetupError::Invalid(format!(
                        "The {what} command must be one line"
                    )));
                }
            }
            Ok(c)
        };
        Ok(Self {
            copy,
            setup: command(self.setup, "setup")?,
            teardown: command(self.teardown, "teardown")?,
        })
    }

    pub fn has_commands(&self) -> bool {
        self.setup.is_some() || self.teardown.is_some()
    }

    /// What trust is keyed on: SHA-256 over both commands. `None` when there
    /// are no commands (copying needs no trust).
    pub fn digest(&self) -> Option<String> {
        if !self.has_commands() {
            return None;
        }
        let mut h = Sha256::new();
        h.update(b"setup\0");
        h.update(self.setup.as_deref().unwrap_or("").as_bytes());
        h.update(b"\0teardown\0");
        h.update(self.teardown.as_deref().unwrap_or("").as_bytes());
        Some(hex::encode(h.finalize()))
    }

    pub fn to_json(&self) -> String {
        let mut text = serde_json::to_string_pretty(self).unwrap_or_else(|_| "{}".into());
        text.push('\n');
        text
    }
}

/// The repository's setup file, or `None` when there is none.
pub fn read(main_root: &Path) -> Result<Option<WorktreeSetup>, SetupError> {
    let path = main_root.join(SETUP_FILE);
    let meta = match std::fs::symlink_metadata(&path) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    if !meta.is_file() {
        return Err(SetupError::Malformed("it isn't a regular file".into()));
    }
    if meta.len() > MAX_FILE_BYTES {
        return Err(SetupError::Malformed("it's larger than 64 KB".into()));
    }
    WorktreeSetup::parse(&std::fs::read(&path)?).map(Some)
}

/// Writes the file atomically (the folder is created when missing).
pub fn write(main_root: &Path, setup: &WorktreeSetup) -> Result<(), SetupError> {
    let path = main_root.join(SETUP_FILE);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    tandem_core::atomic::atomic_write(&path, setup.to_json().as_bytes(), None)?;
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum SkipReason {
    /// The worktree already has it (a tracked file keeps the branch's version).
    Exists,
    Symlink,
    /// It would land outside the worktree.
    Outside,
    /// Over the file or size limit.
    Limit,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SkippedFile {
    pub path: String,
    pub reason: SkipReason,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct CopyReport {
    /// Relative paths, `/`-separated, in walk order.
    pub copied: Vec<String>,
    pub skipped: Vec<SkippedFile>,
    #[ts(type = "number")]
    pub bytes: u64,
    /// A limit stopped the copy.
    pub partial: bool,
}

/// The fixed leading path of an anchored pattern (`config/local.yml` →
/// `config/local.yml`, `secrets/*.key` → `secrets`), which lets the walk enter
/// an ignored folder the pattern names. Unanchored patterns have none.
fn fixed_prefix(pattern: &str) -> Option<String> {
    if pattern.starts_with('!') {
        return None;
    }
    let p = pattern.strip_prefix('/').unwrap_or(pattern);
    let p = p.trim_end_matches('/');
    let anchored = pattern.starts_with('/') || p.contains('/');
    if !anchored {
        return None;
    }
    let fixed: Vec<&str> = p
        .split('/')
        .take_while(|c| !c.contains(['*', '?', '[', '\\']))
        .collect();
    (!fixed.is_empty()).then(|| fixed.join("/"))
}

fn rel_string(rel: &Path) -> String {
    rel.components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

struct Walk<'a> {
    main: PathBuf,
    worktree: PathBuf,
    matcher: Gitignore,
    prefixes: Vec<String>,
    repo: Option<git2::Repository>,
    report: &'a mut CopyReport,
    seen: usize,
}

impl Walk<'_> {
    fn ignored_dir(&self, rel: &Path) -> bool {
        self.repo
            .as_ref()
            .is_some_and(|r| r.is_path_ignored(rel).unwrap_or(false))
    }

    /// A pattern names a path under `rel`.
    fn leads_into(&self, rel: &str) -> bool {
        let dir = format!("{rel}/");
        self.prefixes.iter().any(|p| p.starts_with(&dir))
    }

    fn full(&self) -> bool {
        self.report.copied.len() >= MAX_COPY_FILES || self.report.bytes >= MAX_COPY_BYTES
    }

    fn skip(&mut self, rel: &Path, reason: SkipReason) {
        self.report.skipped.push(SkippedFile {
            path: rel_string(rel),
            reason,
        });
    }

    /// Walks `rel` (a folder under the main checkout) for matches.
    fn dir(&mut self, rel: &Path) -> std::io::Result<()> {
        let mut entries: Vec<_> = std::fs::read_dir(self.main.join(rel))?
            .flatten()
            .collect();
        entries.sort_by_key(|e| e.file_name());
        for entry in entries {
            self.seen += 1;
            if self.seen > MAX_WALK_ENTRIES || self.report.partial {
                self.report.partial = true;
                return Ok(());
            }
            let name = entry.file_name();
            let child = rel.join(&name);
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            let is_dir = kind.is_dir();
            if is_dir && rel.as_os_str().is_empty() && SKIP_DIRS.iter().any(|s| name == *s) {
                continue;
            }
            let matched = self.matcher.matched(&child, is_dir).is_ignore();
            if kind.is_symlink() {
                if matched {
                    self.skip(&child, SkipReason::Symlink);
                }
                continue;
            }
            if matched {
                if is_dir {
                    self.copy_tree(&child)?;
                } else if kind.is_file() {
                    self.copy_file(&child)?;
                }
                continue;
            }
            if is_dir {
                let text = rel_string(&child);
                if !self.ignored_dir(&child) || self.leads_into(&text) {
                    self.dir(&child)?;
                }
            }
        }
        Ok(())
    }

    /// A matched folder: everything under it.
    fn copy_tree(&mut self, rel: &Path) -> std::io::Result<()> {
        let mut entries: Vec<_> = std::fs::read_dir(self.main.join(rel))?
            .flatten()
            .collect();
        entries.sort_by_key(|e| e.file_name());
        for entry in entries {
            self.seen += 1;
            if self.seen > MAX_WALK_ENTRIES || self.report.partial {
                self.report.partial = true;
                return Ok(());
            }
            let child = rel.join(entry.file_name());
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_symlink() {
                self.skip(&child, SkipReason::Symlink);
            } else if kind.is_dir() {
                self.copy_tree(&child)?;
            } else if kind.is_file() {
                self.copy_file(&child)?;
            }
        }
        Ok(())
    }

    fn copy_file(&mut self, rel: &Path) -> std::io::Result<()> {
        if rel
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
        {
            self.skip(rel, SkipReason::Outside);
            return Ok(());
        }
        let src = self.main.join(rel);
        let dest = self.worktree.join(rel);
        if std::fs::symlink_metadata(&dest).is_ok() {
            self.skip(rel, SkipReason::Exists);
            return Ok(());
        }
        let size = std::fs::metadata(&src)?.len();
        if self.full() || self.report.bytes.saturating_add(size) > MAX_COPY_BYTES {
            self.skip(rel, SkipReason::Limit);
            self.report.partial = true;
            return Ok(());
        }
        let Some(parent) = dest.parent() else {
            self.skip(rel, SkipReason::Outside);
            return Ok(());
        };
        // A tracked symlinked folder in the worktree must not carry the copy
        // out of it: create, then check where the parent really is.
        if !self.parent_inside(parent)? {
            self.skip(rel, SkipReason::Outside);
            return Ok(());
        }
        std::fs::copy(&src, &dest)?;
        self.report.bytes += size;
        self.report.copied.push(rel_string(rel));
        Ok(())
    }

    fn parent_inside(&self, parent: &Path) -> std::io::Result<bool> {
        // Walk down from the worktree root, refusing any existing symlink on
        // the way, so nothing is created outside it either.
        let rel = parent
            .strip_prefix(&self.worktree)
            .map_err(|_| std::io::Error::other("outside"))?;
        let mut at = self.worktree.clone();
        for c in rel.components() {
            at.push(c);
            match std::fs::symlink_metadata(&at) {
                Ok(m) if m.file_type().is_symlink() => return Ok(false),
                Ok(m) if m.is_dir() => {}
                Ok(_) => return Ok(false),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    std::fs::create_dir(&at)?;
                }
                Err(e) => return Err(e),
            }
        }
        let real = at.canonicalize()?;
        Ok(real.starts_with(&self.worktree))
    }
}

/// Copies what `patterns` match in the main checkout into `worktree`,
/// never overwriting. Blocking: call it off the async runtime.
pub fn copy_into(
    main_root: &Path,
    worktree: &Path,
    patterns: &[String],
) -> Result<CopyReport, SetupError> {
    let mut report = CopyReport::default();
    if patterns.is_empty() {
        return Ok(report);
    }
    let main = main_root.canonicalize()?;
    let worktree = worktree.canonicalize()?;
    let mut builder = GitignoreBuilder::new(&main);
    for p in patterns {
        builder
            .add_line(None, p)
            .map_err(|e| SetupError::Invalid(format!("Pattern \"{p}\": {e}")))?;
    }
    let matcher = builder
        .build()
        .map_err(|e| SetupError::Invalid(e.to_string()))?;
    let mut walk = Walk {
        repo: git2::Repository::open(&main).ok(),
        prefixes: patterns.iter().filter_map(|p| fixed_prefix(p)).collect(),
        main,
        worktree,
        matcher,
        report: &mut report,
        seen: 0,
    };
    walk.dir(Path::new(""))?;
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn git(dir: &Path, args: &[&str]) {
        let ok = Command::new("git")
            .args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false"])
            .args(args)
            .current_dir(dir)
            .output()
            .expect("git runs")
            .status
            .success();
        assert!(ok, "git {args:?}");
    }

    fn put(path: &Path, text: &str) {
        std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        std::fs::write(path, text).expect("write");
    }

    /// A repository with ignored `.env*`, `config/local.yml` and
    /// `node_modules/`, a tracked `.env.example`, and a worktree beside it.
    fn fixture() -> (tempfile::TempDir, PathBuf, PathBuf) {
        let tmp = tempfile::tempdir().expect("tmp");
        let main = tmp.path().join("app");
        std::fs::create_dir(&main).expect("mkdir");
        git(&main, &["init", "-q", "-b", "main"]);
        put(&main.join(".gitignore"), ".env\n.env.local\nconfig/local.yml\nnode_modules/\nsecrets/\n");
        put(&main.join(".env.example"), "TRACKED=main\n");
        put(&main.join("src/a.ts"), "export {};\n");
        git(&main, &["add", "."]);
        git(&main, &["commit", "-q", "-m", "init"]);
        put(&main.join(".env"), "KEY=1\n");
        put(&main.join(".env.local"), "LOCAL=1\n");
        put(&main.join("config/local.yml"), "db: dev\n");
        put(&main.join("node_modules/.env.cache"), "should not be read\n");
        put(&main.join("secrets/dev.key"), "k\n");
        put(&main.join(".env.example"), "TRACKED=changed-in-main\n");
        let wt = tmp.path().join("wt");
        git(&main, &["worktree", "add", "-q", wt.to_str().expect("utf8"), "-b", "tandem/wt"]);
        (tmp, main, wt)
    }

    fn pats(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn f_wt_01_1_copies_ignored_matches_and_keeps_tracked_files() {
        let (_tmp, main, wt) = fixture();
        let report = copy_into(&main, &wt, &pats(&[".env*", "config/local.yml"])).expect("copy");
        assert_eq!(report.copied, vec![".env", ".env.local", "config/local.yml"]);
        for f in [".env", ".env.local", "config/local.yml"] {
            assert_eq!(
                std::fs::read(main.join(f)).expect("src"),
                std::fs::read(wt.join(f)).expect("dest"),
                "{f}"
            );
        }
        // The tracked file keeps the branch's version.
        assert_eq!(std::fs::read_to_string(wt.join(".env.example")).expect("read"), "TRACKED=main\n");
        assert!(report
            .skipped
            .iter()
            .any(|s| s.path == ".env.example" && s.reason == SkipReason::Exists));
        // node_modules is ignored and no pattern leads into it.
        assert!(!wt.join("node_modules").exists());
        assert!(!report.copied.iter().any(|p| p.starts_with("node_modules")));
        assert!(!report.partial);
    }

    #[test]
    fn f_wt_01_1_anchored_patterns_enter_ignored_folders_and_folders_copy_whole() {
        let (_tmp, main, wt) = fixture();
        let report = copy_into(&main, &wt, &pats(&["secrets/dev.key"])).expect("copy");
        assert_eq!(report.copied, vec!["secrets/dev.key"]);
        std::fs::remove_dir_all(wt.join("secrets")).expect("rm");
        let report = copy_into(&main, &wt, &pats(&["secrets/"])).expect("copy");
        assert_eq!(report.copied, vec!["secrets/dev.key"]);
        // Negations exclude.
        let report = copy_into(&main, &wt, &pats(&[".env*", "!.env.local"])).expect("copy");
        assert_eq!(report.copied, vec![".env"]);
    }

    #[test]
    fn f_wt_01_1_copying_twice_never_overwrites() {
        let (_tmp, main, wt) = fixture();
        copy_into(&main, &wt, &pats(&[".env"])).expect("copy");
        std::fs::write(wt.join(".env"), "EDITED\n").expect("edit");
        let again = copy_into(&main, &wt, &pats(&[".env"])).expect("copy");
        assert!(again.copied.is_empty());
        assert_eq!(std::fs::read_to_string(wt.join(".env")).expect("read"), "EDITED\n");
    }

    #[cfg(unix)]
    #[test]
    fn f_wt_01_2_nothing_lands_outside_the_worktree() {
        let (tmp, main, wt) = fixture();
        let outside = tmp.path().join("outside");
        std::fs::create_dir(&outside).expect("mkdir");
        put(&tmp.path().join("secret"), "s\n");
        // A symlink in the main checkout is listed, never followed.
        std::os::unix::fs::symlink("/etc", main.join("etc-link")).expect("link");
        // A symlinked folder in the worktree must not carry a copy out.
        std::os::unix::fs::symlink(&outside, wt.join("config")).expect("link");
        let report =
            copy_into(&main, &wt, &pats(&["../secret", "etc-link", "config/local.yml"])).expect("copy");
        assert!(report.copied.is_empty(), "{report:?}");
        assert!(report
            .skipped
            .iter()
            .any(|s| s.path == "etc-link" && s.reason == SkipReason::Symlink));
        assert!(report
            .skipped
            .iter()
            .any(|s| s.path == "config/local.yml" && s.reason == SkipReason::Outside));
        assert_eq!(std::fs::read_dir(&outside).expect("ls").count(), 0);
        assert!(!wt.join("secret").exists());
    }

    #[test]
    fn f_wt_01_3_digest_changes_with_the_commands_only() {
        let a = WorktreeSetup {
            copy: pats(&[".env"]),
            setup: Some("pnpm install".into()),
            teardown: None,
        };
        let b = WorktreeSetup {
            copy: pats(&[".env", "x"]),
            ..a.clone()
        };
        let c = WorktreeSetup {
            setup: Some("pnpm install && curl evil".into()),
            ..a.clone()
        };
        assert_eq!(a.digest(), b.digest());
        assert_ne!(a.digest(), c.digest());
        // Moving a command between setup and teardown changes it too.
        let d = WorktreeSetup {
            copy: vec![],
            setup: None,
            teardown: Some("pnpm install".into()),
        };
        assert_ne!(a.digest(), d.digest());
        assert_eq!(WorktreeSetup::default().digest(), None);
    }

    #[test]
    fn f_wt_01_4_write_then_read_round_trips() {
        let tmp = tempfile::tempdir().expect("tmp");
        let setup = WorktreeSetup {
            copy: pats(&[".env*"]),
            setup: Some("  pnpm install ".into()),
            teardown: Some(String::new()),
        }
        .normalized()
        .expect("valid");
        write(tmp.path(), &setup).expect("write");
        let read_back = read(tmp.path()).expect("read").expect("some");
        assert_eq!(read_back.setup.as_deref(), Some("pnpm install"));
        assert_eq!(read_back.teardown, None);
        assert_eq!(read_back, setup);
        assert_eq!(read(&tmp.path().join("none")).expect("read"), None);
    }

    #[test]
    fn f_wt_01_6_malformed_files_say_where() {
        let err = WorktreeSetup::parse(b"{ \"copy\": [\".env\" ").expect_err("bad");
        assert!(err.to_string().contains(".vibezeo/worktree.json isn't valid"), "{err}");
        assert!(WorktreeSetup::parse(br#"{"setup":"a\nb"}"#).is_err());
        assert!(WorktreeSetup::parse(br#"{"copy":"not a list"}"#).is_err());
        let long = format!(r#"{{"setup":"{}"}}"#, "x".repeat(MAX_COMMAND_CHARS + 1));
        assert!(WorktreeSetup::parse(long.as_bytes()).is_err());
        // Unknown keys are fine.
        let ok = WorktreeSetup::parse(br#"{"copy":[".env"],"future":1}"#).expect("ok");
        assert_eq!(ok.copy, pats(&[".env"]));
    }

    #[test]
    fn f_wt_01_1_fixed_prefixes() {
        assert_eq!(fixed_prefix("config/local.yml").as_deref(), Some("config/local.yml"));
        assert_eq!(fixed_prefix("/secrets/*.key").as_deref(), Some("secrets"));
        assert_eq!(fixed_prefix(".env*"), None);
        assert_eq!(fixed_prefix("**/.env"), None);
        assert_eq!(fixed_prefix("!config/x"), None);
        assert_eq!(fixed_prefix("/top").as_deref(), Some("top"));
    }
}
