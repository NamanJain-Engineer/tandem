//! The `git` CLI runner (ARCHITECTURE §7, TL-06 rule 6).
//!
//! Reads never run anything the repository configures: fsmonitor, hooks,
//! credential helpers, pagers, ssh commands, external diff tools and
//! textconv drivers (`--no-ext-diff --no-textconv`), and the filter drivers
//! a repo's `.git/config` can name for its `.gitattributes`. Writes (worktree add, merge, commit, push) are the
//! user's own action and use their config, but never fsmonitor.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::Stdio;

use crate::GitError;

/// What a git invocation is allowed to run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Hardened: nothing repo-configured runs.
    Read,
    /// The user's config and hooks, minus fsmonitor.
    Write,
}

#[derive(Debug, Clone)]
pub struct Output {
    pub stdout: Vec<u8>,
    pub stderr: String,
    pub code: Option<i32>,
}

impl Output {
    pub fn success(&self) -> bool {
        self.code == Some(0)
    }

    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.stdout).into_owned()
    }
}

/// Environment variables that would point git at another repository or
/// index than the one we pass with the working directory.
const SCRUBBED_ENV: &[&str] = &[
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_INDEX_FILE",
    "GIT_COMMON_DIR",
    "GIT_OBJECT_DIRECTORY",
    "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    "GIT_NAMESPACE",
    "GIT_CEILING_DIRECTORIES",
    "GIT_EXTERNAL_DIFF",
    "GIT_PAGER",
    "GIT_EDITOR",
    "GIT_SSH",
    "GIT_SSH_COMMAND",
    "GIT_ASKPASS",
    "GIT_CONFIG_PARAMETERS",
    "GIT_CONFIG_COUNT",
];

#[derive(Debug, Clone)]
pub struct Git {
    program: PathBuf,
    empty_hooks: PathBuf,
    path_env: Option<OsString>,
}

impl Git {
    /// `empty_hooks` is a directory Tandem owns and keeps empty
    /// (`$DATA/run/empty-hooks`); reads point `core.hooksPath` at it.
    pub fn new(program: impl Into<PathBuf>, empty_hooks: impl Into<PathBuf>) -> Self {
        Self {
            program: program.into(),
            empty_hooks: empty_hooks.into(),
            path_env: None,
        }
    }

    /// Finds `git` on `path` (the login environment's PATH), else on this
    /// process's PATH.
    pub fn locate(path: Option<&OsStr>, empty_hooks: impl Into<PathBuf>) -> Result<Self, GitError> {
        let search = path
            .map(OsStr::to_os_string)
            .or_else(|| std::env::var_os("PATH"))
            .unwrap_or_default();
        let names: &[&str] = if cfg!(windows) {
            &["git.exe", "git.cmd"]
        } else {
            &["git"]
        };
        let program = std::env::split_paths(&search)
            .flat_map(|dir| names.iter().map(move |n| dir.join(n)))
            .find(|p| p.is_file())
            .ok_or(GitError::NotInstalled)?;
        Ok(Self {
            program,
            empty_hooks: empty_hooks.into(),
            path_env: path.map(OsStr::to_os_string),
        })
    }

    pub fn program(&self) -> &Path {
        &self.program
    }

    /// The `-c` overrides every read carries, before any repo-specific ones.
    pub fn read_overrides(&self) -> Vec<OsString> {
        let mut hooks = OsString::from("core.hooksPath=");
        hooks.push(&self.empty_hooks);
        let mut args = Vec::new();
        for kv in [
            OsString::from("core.fsmonitor="),
            hooks,
            OsString::from("credential.helper="),
            OsString::from("core.pager=cat"),
            OsString::from("core.sshCommand="),
            OsString::from("core.askPass="),
            OsString::from("uploadpack.packObjectsHook="),
        ] {
            args.push(OsString::from("-c"));
            args.push(kv);
        }
        args
    }

    /// The `-c` overrides every write carries.
    pub fn write_overrides() -> Vec<OsString> {
        vec![OsString::from("-c"), OsString::from("core.fsmonitor=")]
    }

    /// Builds the command. Reads in a repository also neutralize every
    /// filter driver its config names ([`driver_overrides`]), and
    /// diff-family reads get [`read_flags`].
    pub fn command(&self, mode: Mode, cwd: &Path, args: &[&OsStr]) -> tokio::process::Command {
        let mut cmd = tokio::process::Command::new(&self.program);
        cmd.current_dir(cwd);
        match mode {
            Mode::Read => {
                cmd.args(self.read_overrides());
                cmd.args(driver_overrides(cwd));
                cmd.env("GIT_OPTIONAL_LOCKS", "0");
            }
            Mode::Write => {
                cmd.args(Self::write_overrides());
                cmd.env("GIT_MERGE_AUTOEDIT", "no");
            }
        }
        for var in SCRUBBED_ENV {
            cmd.env_remove(var);
        }
        if let Some(path) = &self.path_env {
            cmd.env("PATH", path);
        }
        cmd.env("GIT_TERMINAL_PROMPT", "0")
            .env("LC_ALL", "C")
            .env("LANGUAGE", "C");
        match (mode, args.split_first()) {
            (Mode::Read, Some((sub, rest))) => {
                cmd.arg(sub).args(read_flags(sub)).args(rest);
            }
            _ => {
                cmd.args(args);
            }
        }
        cmd.stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        cmd
    }

    /// Runs git and returns its output whatever the exit code.
    pub async fn run(&self, mode: Mode, cwd: &Path, args: &[&OsStr]) -> Result<Output, GitError> {
        if mode == Mode::Read {
            // Kept empty by us; recreated if something removed it.
            std::fs::create_dir_all(&self.empty_hooks)?;
        }
        let out = self.command(mode, cwd, args).output().await.map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                GitError::NotInstalled
            } else {
                GitError::Io(e)
            }
        })?;
        Ok(Output {
            stdout: out.stdout,
            stderr: short_stderr(&out.stderr),
            code: out.status.code(),
        })
    }

    /// Runs git and fails on a non-zero exit.
    pub async fn ok(&self, mode: Mode, cwd: &Path, args: &[&OsStr]) -> Result<Output, GitError> {
        let out = self.run(mode, cwd, args).await?;
        if out.success() {
            Ok(out)
        } else {
            Err(GitError::Command {
                op: args
                    .first()
                    .map(|a| a.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                code: out.code,
                stderr: out.stderr,
            })
        }
    }
}

/// `-c filter.<d>.clean= …` for every filter driver the repository's
/// config defines, so a read can't run it (an empty command is no
/// command). Reading the config itself runs nothing.
///
/// Diff drivers are handled per command by [`read_flags`]: an empty
/// `diff.external` or `diff.<d>.command` makes git try to run "".
pub fn driver_overrides(cwd: &Path) -> Vec<OsString> {
    let Ok(repo) = git2::Repository::discover(cwd) else {
        return Vec::new();
    };
    let Ok(config) = repo.config().and_then(|mut c| c.snapshot()) else {
        return Vec::new();
    };
    let mut filters = std::collections::BTreeSet::new();
    if let Ok(entries) = config.entries(Some(r"^filter\..+\.")) {
        let _ = entries.for_each(|entry| {
            // The user's own (global, system) drivers are theirs to run;
            // only the repository's config is untrusted.
            if !matches!(
                entry.level(),
                git2::ConfigLevel::Local | git2::ConfigLevel::Worktree | git2::ConfigLevel::App
            ) {
                return;
            }
            // filter.<driver>.<key>; the driver name may itself contain dots.
            let Some(rest) = entry.name().and_then(|n| n.strip_prefix("filter.")) else {
                return;
            };
            if let Some((driver, _)) = rest.rsplit_once('.') {
                filters.insert(driver.to_string());
            }
        });
    }
    let mut args = Vec::new();
    for d in filters {
        for kv in [
            format!("filter.{d}.clean="),
            format!("filter.{d}.smudge="),
            format!("filter.{d}.process="),
            format!("filter.{d}.required=false"),
        ] {
            args.push(OsString::from("-c"));
            args.push(OsString::from(kv));
        }
    }
    args
}

/// Flags a read subcommand gets right after its name: no external diff
/// tool and no textconv driver.
pub fn read_flags(sub: &OsStr) -> &'static [&'static str] {
    match sub.to_str() {
        Some("diff" | "log" | "show" | "whatchanged") => &["--no-ext-diff", "--no-textconv"],
        Some("blame" | "annotate") => &["--no-textconv"],
        _ => &[],
    }
}

/// The last few lines of git's stderr, enough to say what went wrong.
fn short_stderr(raw: &[u8]) -> String {
    let text = String::from_utf8_lossy(raw);
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    let start = lines.len().saturating_sub(4);
    let mut out = lines[start..].join("\n");
    if out.len() > 600 {
        let mut cut = 600;
        while !out.is_char_boundary(cut) {
            cut -= 1;
        }
        out.truncate(cut);
    }
    out
}
