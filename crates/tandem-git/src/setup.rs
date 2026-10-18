//! Worktree setup (F-WT-01): the repository's `.vibezeo/worktree.json`, the
//! copy of ignored files into a new worktree, and the digest that trust is
//! keyed on. Running the commands and keeping trust is the app's job.

use std::collections::BTreeMap;
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
    /// F-WT-06: ports per worktree, 1 to [`MAX_PORTS`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ports: Option<u16>,
    /// F-WT-06: variables for the worktree's panes, with placeholders.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,
    /// F-WT-09: the worktree's preview server.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preview: Option<Preview>,
}

/// F-WT-09: how a worktree's preview runs and where it answers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Preview {
    /// One command line, with F-WT-06's placeholders.
    pub command: String,
    /// The port of the block it answers on, as an offset (0: `VIBEZEO_PORT`).
    #[serde(default)]
    pub port: u16,
    /// What to open, starting with `/`.
    #[serde(default = "root_path")]
    pub path: String,
}

fn root_path() -> String {
    "/".into()
}

pub const MAX_PORTS: u16 = 100;
pub const MAX_ENV: usize = 50;
pub const MAX_ENV_NAME: usize = 64;
pub const MAX_ENV_VALUE: usize = 2_000;

/// Names a repository's file may not set (F-WT-06), with why: they would
/// change how shells start or what they run, where agents sign in or send
/// their requests (provider homes, endpoints, proxies, certificates,
/// D-2448), or they're Vibezeo's own. Case-insensitive.
pub fn reserved_reason(name: &str) -> Option<&'static str> {
    const SHELL: [&str; 14] = [
        "PATH",
        "HOME",
        "SHELL",
        "USER",
        "ZDOTDIR",
        "ENV",
        "BASH_ENV",
        "PROMPT_COMMAND",
        "LD_PRELOAD",
        "LD_LIBRARY_PATH",
        "TERM",
        "COLORTERM",
        "TERM_PROGRAM",
        "TERM_PROGRAM_VERSION",
    ];
    // Every provider home Vibezeo sets (the registry's `providerHomeEnv`,
    // F-AC-01); a test checks the registry against this list.
    const AGENTS: [&str; 15] = [
        "CLAUDE_CONFIG_DIR",
        "CODEX_HOME",
        "GEMINI_CLI_HOME",
        "KIRO_HOME",
        "XDG_CONFIG_HOME",
        "XDG_DATA_HOME",
        "XDG_STATE_HOME",
        "XDG_CACHE_HOME",
        "HTTP_PROXY",
        "HTTPS_PROXY",
        "ALL_PROXY",
        "NO_PROXY",
        "NODE_EXTRA_CA_CERTS",
        "SSL_CERT_FILE",
        "SSL_CERT_DIR",
    ];
    const MORE_AGENTS: [&str; 2] = ["REQUESTS_CA_BUNDLE", "CURL_CA_BUNDLE"];
    let upper = name.to_ascii_uppercase();
    let upper = upper.as_str();
    if SHELL.contains(&upper) || upper.starts_with("DYLD_") {
        return Some("it would change how terminals start or what they run");
    }
    if AGENTS.contains(&upper) || MORE_AGENTS.contains(&upper) || upper.ends_with("_BASE_URL") {
        return Some("it would change where agents sign in or send their requests");
    }
    if upper == "COMPOSE_PROJECT_NAME"
        || upper.starts_with("TANDEM_")
        || upper.starts_with("VIBEZEO_")
    {
        return Some("Vibezeo sets it");
    }
    None
}

pub fn reserved_name(name: &str) -> bool {
    reserved_reason(name).is_some()
}

fn valid_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
        && name.len() <= MAX_ENV_NAME
}

/// What a worktree's placeholders are filled in with (F-WT-06).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeCtx {
    pub slug: String,
    pub index: u32,
    pub repo: String,
    pub worktree: String,
    /// The first port and how many.
    pub ports: Option<(u16, u16)>,
}

/// One placeholder in a template.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Placeholder {
    Slug,
    Index,
    Repo,
    Port(u16),
}

fn placeholder(inner: &str) -> Option<Placeholder> {
    match inner.trim() {
        "slug" => Some(Placeholder::Slug),
        "index" => Some(Placeholder::Index),
        "repo" => Some(Placeholder::Repo),
        "port" => Some(Placeholder::Port(0)),
        other => other
            .strip_prefix("port+")
            .and_then(|n| n.trim().parse::<u16>().ok())
            .map(Placeholder::Port),
    }
}

/// Splits a template into text and placeholders; `Err` names the first
/// unknown one.
fn tokens(template: &str) -> Result<Vec<Result<&str, Placeholder>>, String> {
    let mut out = Vec::new();
    let mut rest = template;
    while let Some(open) = rest.find("{{") {
        let Some(close) = rest[open..].find("}}") else {
            break;
        };
        out.push(Ok(&rest[..open]));
        let inner = &rest[open + 2..open + close];
        out.push(Err(
            placeholder(inner).ok_or_else(|| format!("{{{{{inner}}}}}"))?
        ));
        rest = &rest[open + close + 2..];
    }
    out.push(Ok(rest));
    Ok(out)
}

/// A template with its placeholders filled in (unknown ones are left as
/// written; validation refuses them first).
pub fn fill(template: &str, ctx: &RuntimeCtx) -> String {
    let Ok(parts) = tokens(template) else {
        return template.to_string();
    };
    parts
        .into_iter()
        .map(|t| match t {
            Ok(text) => text.to_string(),
            Err(Placeholder::Slug) => ctx.slug.clone(),
            Err(Placeholder::Index) => ctx.index.to_string(),
            Err(Placeholder::Repo) => ctx.repo.clone(),
            Err(Placeholder::Port(n)) => ctx
                .ports
                .map(|(start, _)| start.saturating_add(n).to_string())
                .unwrap_or_default(),
        })
        .collect()
}

/// `<repo>-<slug>`, lowercase, anything but letters, digits, `-` and `_`
/// replaced by `-` (Compose's rule for project names).
pub fn compose_project(repo: &str, slug: &str) -> String {
    let raw = format!("{repo}-{slug}").to_lowercase();
    let mut name: String = raw
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect();
    if !name.starts_with(|c: char| c.is_ascii_alphanumeric()) {
        name.insert(0, 'w');
    }
    name
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
        if let Some(n) = self.ports {
            if n == 0 || n > MAX_PORTS {
                return Err(SetupError::Invalid(format!(
                    "\"ports\" must be between 1 and {MAX_PORTS}"
                )));
            }
        }
        if self.env.len() > MAX_ENV {
            return Err(SetupError::Invalid(format!(
                "Use at most {MAX_ENV} variables in \"env\""
            )));
        }
        let mut env = BTreeMap::new();
        for (name, value) in self.env {
            let name = name.trim().to_string();
            if !valid_name(&name) {
                return Err(SetupError::Invalid(format!(
                    "\"{name}\" isn't a valid variable name"
                )));
            }
            if let Some(why) = reserved_reason(&name) {
                return Err(SetupError::Invalid(format!(
                    "worktree.json can't set {name}: {why}"
                )));
            }
            if value.chars().count() > MAX_ENV_VALUE {
                return Err(SetupError::Invalid(format!(
                    "{name} is longer than {MAX_ENV_VALUE} characters"
                )));
            }
            if value.contains(['\n', '\r', '\0']) {
                return Err(SetupError::Invalid(format!("{name} must be one line")));
            }
            for t in tokens(&value).map_err(|p| {
                SetupError::Invalid(format!("{name} uses an unknown placeholder {p}"))
            })? {
                if let Err(Placeholder::Port(n)) = t {
                    match self.ports {
                        None => {
                            return Err(SetupError::Invalid(format!(
                                "{name} uses a port, but \"ports\" isn't set"
                            )))
                        }
                        Some(size) if n >= size => {
                            return Err(SetupError::Invalid(format!(
                                "{name} uses port+{n}, but each worktree has {size} ports"
                            )))
                        }
                        _ => {}
                    }
                }
            }
            env.insert(name, value);
        }
        let preview = match self.preview {
            None => None,
            Some(p) => {
                let Some(size) = self.ports else {
                    return Err(SetupError::Invalid("\"preview\" needs \"ports\"".into()));
                };
                if p.port >= size {
                    return Err(SetupError::Invalid(format!(
                        "The preview's port {} is outside the {size} ports each worktree has",
                        p.port
                    )));
                }
                if !p.path.starts_with('/') || p.path.contains(['\n', '\r', ' ']) {
                    return Err(SetupError::Invalid(
                        "The preview's path must start with /".into(),
                    ));
                }
                let Some(cmd) = command(Some(p.command), "preview")? else {
                    return Err(SetupError::Invalid("The preview needs a command".into()));
                };
                for t in tokens(&cmd).map_err(|ph| {
                    SetupError::Invalid(format!("The preview uses an unknown placeholder {ph}"))
                })? {
                    if let Err(Placeholder::Port(n)) = t {
                        if n >= size {
                            return Err(SetupError::Invalid(format!(
                                "The preview uses port+{n}, but each worktree has {size} ports"
                            )));
                        }
                    }
                }
                Some(Preview {
                    command: cmd,
                    port: p.port,
                    path: p.path,
                })
            }
        };
        Ok(Self {
            copy,
            setup: command(self.setup, "setup")?,
            teardown: command(self.teardown, "teardown")?,
            ports: self.ports,
            env,
            preview,
        })
    }

    pub fn has_commands(&self) -> bool {
        self.setup.is_some() || self.teardown.is_some()
    }

    /// F-WT-06: the file asks for per-worktree ports or variables.
    pub fn runtime_on(&self) -> bool {
        self.ports.is_some() || !self.env.is_empty()
    }

    /// What trust is keyed on: SHA-256 over both commands and, when there is
    /// any, `env` (F-WT-06). `None` when there's nothing to trust (copying
    /// and ports need none). Without `env` the digest is the one F-WT-01
    /// computed, so earlier trust stays valid.
    pub fn digest(&self) -> Option<String> {
        if !self.has_commands() && self.env.is_empty() && self.preview.is_none() {
            return None;
        }
        let mut h = Sha256::new();
        h.update(b"setup\0");
        h.update(self.setup.as_deref().unwrap_or("").as_bytes());
        h.update(b"\0teardown\0");
        h.update(self.teardown.as_deref().unwrap_or("").as_bytes());
        if !self.env.is_empty() {
            h.update(b"\0env\0");
            for (k, v) in &self.env {
                h.update(k.as_bytes());
                h.update(b"=");
                h.update(v.as_bytes());
                h.update(b"\0");
            }
        }
        // F-WT-09: added only when there is one, like `env`.
        if let Some(p) = &self.preview {
            h.update(b"\0preview\0");
            h.update(p.command.as_bytes());
        }
        Some(hex::encode(h.finalize()))
    }

    /// F-WT-09: the preview's command, filled in, and the port it answers on.
    pub fn preview_for(&self, ctx: &RuntimeCtx) -> Option<(String, u16)> {
        let p = self.preview.as_ref()?;
        let (start, _) = ctx.ports?;
        Some((fill(&p.command, ctx), start.saturating_add(p.port)))
    }

    /// The variables for a worktree's panes (F-WT-06): Vibezeo's own, and
    /// with `trusted` the file's `env`, filled in.
    pub fn runtime_vars(&self, ctx: &RuntimeCtx, trusted: bool) -> BTreeMap<String, String> {
        let mut out = BTreeMap::new();
        if !self.runtime_on() {
            return out;
        }
        out.insert("VIBEZEO_WORKTREE".into(), ctx.worktree.clone());
        out.insert("VIBEZEO_SLUG".into(), ctx.slug.clone());
        out.insert("VIBEZEO_INDEX".into(), ctx.index.to_string());
        out.insert(
            "COMPOSE_PROJECT_NAME".into(),
            compose_project(&ctx.repo, &ctx.slug),
        );
        if let Some((start, size)) = ctx.ports {
            out.insert("VIBEZEO_PORT".into(), start.to_string());
            out.insert(
                "VIBEZEO_PORTS".into(),
                format!("{start}-{}", start.saturating_add(size.saturating_sub(1))),
            );
        }
        if trusted {
            for (name, template) in &self.env {
                out.insert(name.clone(), fill(template, ctx));
            }
        }
        out
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

// Exported to TypeScript as `CopySkipReason`: the prompt library's
// `tandem_core::prompts::SkipReason` already owns that name there (D-835).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export, rename = "CopySkipReason")]
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
    /// F-WT-09: how many of the copied files share their data with the
    /// main checkout's (copy-on-write clones).
    #[serde(default)]
    pub cloned: u32,
}

/// How one file is copied (F-WT-09): a copy-on-write clone where the
/// filesystem supports it, else an ordinary copy. A trait so tests can fake
/// the filesystem.
pub trait Cloner {
    /// Copies `src` to the new file `dest` (which must not exist); `true`
    /// when it was cloned rather than copied.
    fn copy(&self, src: &Path, dest: &Path) -> std::io::Result<bool>;
}

/// Clones with `FICLONE` on Linux (as `cp --reflink=auto` does), falling
/// back to an ordinary copy there and everywhere else.
pub struct SystemCloner;

impl Cloner for SystemCloner {
    fn copy(&self, src: &Path, dest: &Path) -> std::io::Result<bool> {
        #[cfg(target_os = "linux")]
        if reflink::clone_file(src, dest)? {
            return Ok(true);
        }
        std::fs::copy(src, dest)?;
        Ok(false)
    }
}

#[cfg(target_os = "linux")]
mod reflink {
    use std::fs::{File, OpenOptions};
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    use std::os::unix::io::AsRawFd;
    use std::path::Path;

    /// `_IOW(0x94, 9, int)`.
    const FICLONE: libc::c_ulong = 0x4004_9409;

    /// `Ok(true)` when `dest` was made as a clone of `src`; `Ok(false)` when
    /// the filesystem can't (nothing is left behind); `Err` only when `src`
    /// can't be read or `dest` can't be created.
    pub fn clone_file(src: &Path, dest: &Path) -> std::io::Result<bool> {
        let from = File::open(src)?;
        let mode = from.metadata()?.permissions().mode();
        let to = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(mode & 0o7777)
            .open(dest)?;
        // SAFETY: both descriptors are open for the call's duration; FICLONE
        // reads the source descriptor and writes nothing but the target.
        let rc = unsafe { libc::ioctl(to.as_raw_fd(), FICLONE as _, from.as_raw_fd()) };
        drop(to);
        if rc == 0 {
            std::fs::set_permissions(dest, std::fs::Permissions::from_mode(mode & 0o7777))?;
            return Ok(true);
        }
        // Not supported here (another filesystem, tmpfs, overlayfs…): undo.
        std::fs::remove_file(dest)?;
        Ok(false)
    }
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
    cloner: &'a dyn Cloner,
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
        let mut entries: Vec<_> = std::fs::read_dir(self.main.join(rel))?.flatten().collect();
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
        let mut entries: Vec<_> = std::fs::read_dir(self.main.join(rel))?.flatten().collect();
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
        if rel.components().any(|c| !matches!(c, Component::Normal(_))) {
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
        if self.cloner.copy(&src, &dest)? {
            self.report.cloned += 1;
        }
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
    copy_into_with(main_root, worktree, patterns, &SystemCloner)
}

/// [`copy_into`] with the way files are copied given (tests fake it).
pub fn copy_into_with(
    main_root: &Path,
    worktree: &Path,
    patterns: &[String],
    cloner: &dyn Cloner,
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
        cloner,
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
            .args([
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@t",
                "-c",
                "commit.gpgsign=false",
            ])
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
        put(
            &main.join(".gitignore"),
            ".env\n.env.local\nconfig/local.yml\nnode_modules/\nsecrets/\n",
        );
        put(&main.join(".env.example"), "TRACKED=main\n");
        put(&main.join("src/a.ts"), "export {};\n");
        git(&main, &["add", "."]);
        git(&main, &["commit", "-q", "-m", "init"]);
        put(&main.join(".env"), "KEY=1\n");
        put(&main.join(".env.local"), "LOCAL=1\n");
        put(&main.join("config/local.yml"), "db: dev\n");
        put(
            &main.join("node_modules/.env.cache"),
            "should not be read\n",
        );
        put(&main.join("secrets/dev.key"), "k\n");
        put(&main.join(".env.example"), "TRACKED=changed-in-main\n");
        let wt = tmp.path().join("wt");
        git(
            &main,
            &[
                "worktree",
                "add",
                "-q",
                wt.to_str().expect("utf8"),
                "-b",
                "tandem/wt",
            ],
        );
        (tmp, main, wt)
    }

    fn pats(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn f_wt_01_1_copies_ignored_matches_and_keeps_tracked_files() {
        let (_tmp, main, wt) = fixture();
        let report = copy_into(&main, &wt, &pats(&[".env*", "config/local.yml"])).expect("copy");
        assert_eq!(
            report.copied,
            vec![".env", ".env.local", "config/local.yml"]
        );
        for f in [".env", ".env.local", "config/local.yml"] {
            assert_eq!(
                std::fs::read(main.join(f)).expect("src"),
                std::fs::read(wt.join(f)).expect("dest"),
                "{f}"
            );
        }
        // The tracked file keeps the branch's version.
        assert_eq!(
            std::fs::read_to_string(wt.join(".env.example")).expect("read"),
            "TRACKED=main\n"
        );
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
        assert_eq!(
            std::fs::read_to_string(wt.join(".env")).expect("read"),
            "EDITED\n"
        );
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
        let report = copy_into(
            &main,
            &wt,
            &pats(&["../secret", "etc-link", "config/local.yml"]),
        )
        .expect("copy");
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
            ..Default::default()
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
            ..Default::default()
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
            ..Default::default()
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
        assert!(
            err.to_string()
                .contains(".vibezeo/worktree.json isn't valid"),
            "{err}"
        );
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
        assert_eq!(
            fixed_prefix("config/local.yml").as_deref(),
            Some("config/local.yml")
        );
        assert_eq!(fixed_prefix("/secrets/*.key").as_deref(), Some("secrets"));
        assert_eq!(fixed_prefix(".env*"), None);
        assert_eq!(fixed_prefix("**/.env"), None);
        assert_eq!(fixed_prefix("!config/x"), None);
        assert_eq!(fixed_prefix("/top").as_deref(), Some("top"));
    }

    fn ctx(ports: Option<(u16, u16)>) -> RuntimeCtx {
        RuntimeCtx {
            slug: "fix-auth".into(),
            index: 2,
            repo: "Shop App".into(),
            worktree: "/r/.tandem/worktrees/fix-auth".into(),
            ports,
        }
    }

    #[test]
    fn f_wt_06_2_env_placeholders_fill_in_and_wait_for_trust() {
        let setup = WorktreeSetup::parse(
            br#"{"ports":10,"env":{"DB":"app_{{slug}}","PORT":"{{port}}","API":"http://localhost:{{port+1}}/{{index}}/{{repo}}"}}"#,
        )
        .expect("valid");
        let vars = setup.runtime_vars(&ctx(Some((41010, 10))), true);
        assert_eq!(vars["DB"], "app_fix-auth");
        assert_eq!(vars["PORT"], "41010");
        assert_eq!(vars["API"], "http://localhost:41011/2/Shop App");
        assert_eq!(vars["VIBEZEO_PORT"], "41010");
        assert_eq!(vars["VIBEZEO_PORTS"], "41010-41019");
        assert_eq!(vars["VIBEZEO_SLUG"], "fix-auth");
        assert_eq!(vars["VIBEZEO_INDEX"], "2");
        assert_eq!(vars["VIBEZEO_WORKTREE"], "/r/.tandem/worktrees/fix-auth");
        assert_eq!(vars["COMPOSE_PROJECT_NAME"], "shop-app-fix-auth");
        // Untrusted: Vibezeo's own variables only.
        let untrusted = setup.runtime_vars(&ctx(Some((41010, 10))), false);
        assert!(!untrusted.contains_key("DB"));
        assert_eq!(untrusted["COMPOSE_PROJECT_NAME"], "shop-app-fix-auth");
        // No runtime keys: nothing at all.
        let plain = WorktreeSetup::parse(br#"{"copy":[".env"]}"#).expect("valid");
        assert!(plain.runtime_vars(&ctx(None), true).is_empty());
    }

    #[test]
    fn f_wt_06_6_reserved_names_and_bad_placeholders_make_the_file_invalid() {
        for (json, says) in [
            (r#"{"env":{"PATH":"/x"}}"#, "can't set PATH"),
            (r#"{"env":{"LD_PRELOAD":"/x.so"}}"#, "can't set LD_PRELOAD"),
            (
                r#"{"env":{"DYLD_INSERT_LIBRARIES":"x"}}"#,
                "can't set DYLD_INSERT_LIBRARIES",
            ),
            (r#"{"env":{"VIBEZEO_PORT":"1"}}"#, "can't set VIBEZEO_PORT"),
            (
                r#"{"env":{"COMPOSE_PROJECT_NAME":"x"}}"#,
                "can't set COMPOSE_PROJECT_NAME",
            ),
            (r#"{"env":{"1BAD":"x"}}"#, "isn't a valid variable name"),
            (
                r#"{"env":{"A":"{{nope}}"}}"#,
                "unknown placeholder {{nope}}",
            ),
            (r#"{"env":{"A":"{{port}}"}}"#, "\"ports\" isn't set"),
            (
                r#"{"ports":2,"env":{"A":"{{port+2}}"}}"#,
                "each worktree has 2 ports",
            ),
            (r#"{"ports":0}"#, "between 1 and 100"),
            (r#"{"ports":101}"#, "between 1 and 100"),
        ] {
            let err = WorktreeSetup::parse(json.as_bytes()).expect_err(json);
            assert!(err.to_string().contains(says), "{json}: {err}");
        }
    }

    #[test]
    fn f_wt_06_2_env_is_trusted_with_the_commands_and_old_trust_stays_valid() {
        let commands = WorktreeSetup::parse(br#"{"setup":"pnpm install"}"#).expect("valid");
        // The F-WT-01 digest of the same commands, computed as F-WT-01 did.
        let mut h = Sha256::new();
        h.update(b"setup\0pnpm install\0teardown\0");
        assert_eq!(commands.digest(), Some(hex::encode(h.finalize())));
        // Ports alone need no trust; env does, and changes the digest.
        let ports_only =
            WorktreeSetup::parse(br#"{"setup":"pnpm install","ports":5}"#).expect("ok");
        assert_eq!(ports_only.digest(), commands.digest());
        let with_env =
            WorktreeSetup::parse(br#"{"setup":"pnpm install","env":{"A":"1"}}"#).expect("ok");
        assert_ne!(with_env.digest(), commands.digest());
        let other_env =
            WorktreeSetup::parse(br#"{"setup":"pnpm install","env":{"A":"2"}}"#).expect("ok");
        assert_ne!(with_env.digest(), other_env.digest());
        let env_only = WorktreeSetup::parse(br#"{"env":{"A":"1"}}"#).expect("ok");
        assert!(env_only.digest().is_some());
    }

    #[test]
    fn f_wt_06_2_compose_project_names_follow_composes_rules() {
        assert_eq!(compose_project("app", "fix-auth"), "app-fix-auth");
        assert_eq!(compose_project("My.App", "Fix Auth"), "my-app-fix-auth");
        assert_eq!(compose_project("_x", "y"), "w_x-y");
    }

    #[test]
    fn f_wt_09_7_preview_needs_ports_a_port_inside_and_a_path() {
        for (json, says) in [
            (r#"{"preview":{"command":"x"}}"#, "needs \"ports\""),
            (
                r#"{"ports":2,"preview":{"command":"x","port":2}}"#,
                "outside the 2 ports",
            ),
            (
                r#"{"ports":2,"preview":{"command":"x","path":"nope"}}"#,
                "must start with /",
            ),
            (
                r#"{"ports":2,"preview":{"command":"serve {{port+3}}"}}"#,
                "port+3",
            ),
            (
                r#"{"ports":2,"preview":{"command":"serve {{wat}}"}}"#,
                "unknown placeholder",
            ),
            (
                r#"{"ports":2,"preview":{"command":"  "}}"#,
                "needs a command",
            ),
        ] {
            let err = WorktreeSetup::parse(json.as_bytes()).expect_err(json);
            assert!(err.to_string().contains(says), "{json}: {err}");
        }
        let ok = WorktreeSetup::parse(
            br#"{"ports":4,"preview":{"command":"serve {{port+1}}","port":1}}"#,
        )
        .expect("ok");
        assert_eq!(ok.preview.as_ref().map(|p| p.path.as_str()), Some("/"));
    }

    #[test]
    fn f_wt_09_1_the_preview_fills_in_its_port_and_is_trusted_with_the_commands() {
        let with = WorktreeSetup::parse(
            br#"{"ports":10,"setup":"pnpm i","preview":{"command":"pnpm dev --port {{port+2}} --name {{slug}}","port":2}}"#,
        )
        .expect("ok");
        let (cmd, port) = with.preview_for(&ctx(Some((41010, 10)))).expect("preview");
        assert_eq!(cmd, "pnpm dev --port 41012 --name fix-auth");
        assert_eq!(port, 41012);
        let without = WorktreeSetup::parse(br#"{"ports":10,"setup":"pnpm i"}"#).expect("ok");
        assert!(without.preview_for(&ctx(Some((41010, 10)))).is_none());
        // The preview is in the digest; without one the digest is unchanged.
        assert_ne!(with.digest(), without.digest());
        let mut h = Sha256::new();
        h.update(b"setup\0pnpm i\0teardown\0");
        assert_eq!(without.digest(), Some(hex::encode(h.finalize())));
        let preview_only =
            WorktreeSetup::parse(br#"{"ports":1,"preview":{"command":"serve"}}"#).expect("ok");
        assert!(preview_only.digest().is_some());
    }

    #[test]
    fn f_wt_06_6_names_that_steer_agents_are_refused_and_say_why() {
        for name in [
            "CLAUDE_CONFIG_DIR",
            "CODEX_HOME",
            "XDG_CONFIG_HOME",
            "XDG_DATA_HOME",
            "ANTHROPIC_BASE_URL",
            "OPENAI_BASE_URL",
            "HTTPS_PROXY",
            "https_proxy",
            "all_proxy",
            "NO_PROXY",
            "NODE_EXTRA_CA_CERTS",
            "SSL_CERT_FILE",
            "SSL_CERT_DIR",
            "REQUESTS_CA_BUNDLE",
        ] {
            let json = format!(r#"{{"env":{{"{name}":"x"}}}}"#);
            let err = WorktreeSetup::parse(json.as_bytes()).expect_err(name);
            assert!(
                err.to_string().contains(&format!(
                    "can't set {name}: it would change where agents sign in or send their requests"
                )),
                "{name}: {err}"
            );
        }
        // The user's own keys stay allowed: they're shown in the review.
        let ok = WorktreeSetup::parse(br#"{"env":{"ANTHROPIC_API_KEY":"x","DATABASE_URL":"y"}}"#);
        assert!(ok.is_ok());
        assert!(reserved_reason("PATH").is_some_and(|w| w.contains("terminals start")));
    }

    /// Clones every file it's given (a filesystem with copy-on-write).
    struct AlwaysClones;
    impl Cloner for AlwaysClones {
        fn copy(&self, src: &Path, dest: &Path) -> std::io::Result<bool> {
            std::fs::copy(src, dest)?;
            Ok(true)
        }
    }

    #[test]
    fn f_wt_09_6_clones_are_counted_where_the_filesystem_clones() {
        let (_tmp, main, wt) = fixture();
        let report = copy_into_with(
            &main,
            &wt,
            &pats(&[".env*", "config/local.yml"]),
            &AlwaysClones,
        )
        .expect("copy");
        assert_eq!(report.copied.len(), 3);
        assert_eq!(report.cloned, 3);
    }

    #[test]
    fn f_wt_09_6_the_system_copy_falls_back_and_keeps_bytes_and_modes() {
        let (_tmp, main, wt) = fixture();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(main.join(".env"), std::fs::Permissions::from_mode(0o600))
                .expect("chmod");
        }
        let report = copy_into(&main, &wt, &pats(&[".env*"])).expect("copy");
        assert_eq!(report.copied, vec![".env", ".env.local"]);
        // Cloned or copied, the bytes are the same.
        assert!(report.cloned <= 2);
        for f in [".env", ".env.local"] {
            assert_eq!(
                std::fs::read(main.join(f)).expect("src"),
                std::fs::read(wt.join(f)).expect("dest")
            );
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(wt.join(".env"))
                .expect("meta")
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn f_wt_09_6_a_failed_clone_leaves_nothing_behind() {
        let tmp = tempfile::tempdir().expect("tmp");
        let src = tmp.path().join("src.bin");
        std::fs::write(&src, b"data").expect("write");
        let dest = tmp.path().join("dest.bin");
        match reflink::clone_file(&src, &dest) {
            // A filesystem with copy-on-write: a real clone.
            Ok(true) => assert_eq!(std::fs::read(&dest).expect("read"), b"data"),
            // Without it: no half-made file.
            Ok(false) => assert!(!dest.exists()),
            Err(e) => panic!("{e}"),
        }
        // An existing destination is never overwritten.
        std::fs::write(&dest, b"keep").expect("write");
        assert!(reflink::clone_file(&src, &dest).is_err());
        assert_eq!(std::fs::read(&dest).expect("read"), b"keep");
    }

    /// On a copy-on-write filesystem (set `TANDEM_COW_DIR` to a folder on
    /// btrfs or XFS with reflinks) every copied file is a clone. Elsewhere
    /// there's nothing to check, so it returns early.
    #[cfg(target_os = "linux")]
    #[test]
    fn f_wt_09_6_on_a_cow_filesystem_every_copied_file_is_a_clone() {
        let Some(dir) = std::env::var_os("TANDEM_COW_DIR") else {
            return;
        };
        let tmp = tempfile::tempdir_in(dir).expect("tmp on the cow filesystem");
        let src = tmp.path().join("big.bin");
        std::fs::write(&src, vec![7u8; 4 << 20]).expect("write");
        let dest = tmp.path().join("clone.bin");
        assert!(
            reflink::clone_file(&src, &dest).expect("clone"),
            "not cloned"
        );
        assert_eq!(std::fs::read(&dest).expect("read").len(), 4 << 20);
    }
}
