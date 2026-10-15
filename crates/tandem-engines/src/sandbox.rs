//! Agent sandbox (F-TL-11): an engine's own command runs with only what its
//! work needs: the filesystem read-only apart from the pane's folder, its
//! git data, the account's provider home and Vibezeo's tasks folder; the
//! user's keys and the other accounts' homes hidden; git hooks and config
//! read-only. It narrows what the process can reach and nothing more: the
//! engine signs in by itself, and nothing here reads a credential.
//!
//! Linux uses bubblewrap (`bwrap`) when it's installed and can run;
//! macOS and Windows report "not available yet" through the same trait.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// What a sandboxed agent may reach.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SandboxPolicy {
    /// The pane's folder (workspace or worktree): writable.
    pub root: PathBuf,
    /// The repository's git folder when it lies outside `root` (a
    /// worktree's common dir): writable, so commits work.
    pub git_common: Option<PathBuf>,
    /// The account's provider home: writable, so the engine's own sign-in
    /// keeps working.
    pub provider_home: Option<PathBuf>,
    /// `$DATA/providers`: hidden, apart from `provider_home`.
    pub providers: PathBuf,
    /// Vibezeo's tasks folder (the `tandem-tasks` server): writable.
    pub tasks: PathBuf,
    /// Vibezeo's run folder (the status socket): readable.
    pub run: PathBuf,
    /// The user's home.
    pub home: PathBuf,
    /// Other worktrees of the repository: hidden.
    pub hide: Vec<PathBuf>,
}

/// Secrets under the home that read as empty.
pub const HIDDEN_HOME: &[&str] = &[
    ".ssh",
    ".aws",
    ".gnupg",
    ".config/gh",
    ".docker",
    ".kube",
    ".netrc",
    ".local/share/keyrings",
    ".git-credentials",
];

/// How a path exists, for the builder (tests pass a fake).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Missing,
    Dir,
    File,
}

fn s(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

/// bubblewrap's arguments for `policy`, ending with `--`, before the
/// engine's own argv. Later mounts cover earlier ones, so the order is:
/// read-only root, writable areas, hidden areas (then the provider home
/// back on top of the hidden providers), read-only git internals.
pub fn bwrap_args(
    bwrap: &Path,
    policy: &SandboxPolicy,
    kind: &dyn Fn(&Path) -> Kind,
) -> Vec<String> {
    let mut a = vec![
        s(bwrap),
        "--die-with-parent".into(),
        "--ro-bind".into(),
        "/".into(),
        "/".into(),
        "--dev".into(),
        "/dev".into(),
        "--proc".into(),
        "/proc".into(),
    ];
    let bind = |a: &mut Vec<String>, flag: &str, p: &Path| {
        a.push(flag.into());
        a.push(s(p));
        a.push(s(p));
    };
    // Writable.
    if kind(Path::new("/tmp")) == Kind::Dir {
        bind(&mut a, "--bind", Path::new("/tmp"));
    }
    bind(&mut a, "--bind", &policy.root);
    if let Some(g) = &policy.git_common {
        if kind(g) == Kind::Dir {
            bind(&mut a, "--bind", g);
        }
    }
    if kind(&policy.tasks) == Kind::Dir {
        bind(&mut a, "--bind", &policy.tasks);
    }
    let cache = policy.home.join(".cache");
    if kind(&cache) == Kind::Dir {
        a.push("--tmpfs".into());
        a.push(s(&cache));
    }
    // Hidden: secrets, other accounts' homes, other worktrees.
    let hide = |a: &mut Vec<String>, p: &Path| match kind(p) {
        Kind::Dir => {
            a.push("--tmpfs".into());
            a.push(s(p));
        }
        Kind::File => {
            a.push("--ro-bind".into());
            a.push("/dev/null".into());
            a.push(s(p));
        }
        Kind::Missing => {}
    };
    for rel in HIDDEN_HOME {
        hide(&mut a, &policy.home.join(rel));
    }
    hide(&mut a, &policy.providers);
    for p in &policy.hide {
        hide(&mut a, p);
    }
    // Back on top of the hidden providers: this account's home, writable;
    // the status socket's folder, readable.
    if let Some(h) = &policy.provider_home {
        if kind(h) == Kind::Dir {
            bind(&mut a, "--bind", h);
        }
    }
    if kind(&policy.run) == Kind::Dir {
        bind(&mut a, "--ro-bind", &policy.run);
    }
    // Git internals read-only, in the folder's repository and the common dir.
    let mut git_dirs = vec![policy.root.join(".git")];
    if let Some(g) = &policy.git_common {
        git_dirs.push(g.clone());
    }
    for g in git_dirs {
        if kind(&g) != Kind::Dir {
            continue;
        }
        for inner in ["hooks", "config"] {
            let p = g.join(inner);
            if kind(&p) != Kind::Missing {
                bind(&mut a, "--ro-bind", &p);
            }
        }
    }
    a.push("--".into());
    a
}

/// The repository's common git folder for a worktree at `root` (its `.git`
/// is a file naming `gitdir: …/worktrees/<name>`, whose `commondir` names
/// the repository's `.git`). `None` for a plain checkout or no repository.
pub fn git_common_dir(root: &Path) -> Option<PathBuf> {
    let dot = root.join(".git");
    if !dot.is_file() {
        return None;
    }
    let text = std::fs::read_to_string(&dot).ok()?;
    let gitdir = text.lines().find_map(|l| l.strip_prefix("gitdir:"))?.trim();
    let gitdir = root.join(gitdir);
    let common = std::fs::read_to_string(gitdir.join("commondir")).ok()?;
    let common = gitdir.join(common.trim());
    common.canonicalize().ok()
}

/// Whether the sandbox can be used here, and if not, why (the setting's
/// line, the pane chip's tooltip).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SandboxStatus {
    pub available: bool,
    /// "bubblewrap 0.9.0".
    pub tool: Option<String>,
    pub reason: Option<String>,
}

impl SandboxStatus {
    pub fn unavailable(reason: &str) -> Self {
        Self {
            available: false,
            tool: None,
            reason: Some(reason.into()),
        }
    }
}

/// The platform's sandbox.
pub trait Sandboxer: Send + Sync {
    fn status(&self) -> SandboxStatus;
    /// The argv that runs `argv` inside the sandbox, or why it can't.
    fn wrap(&self, policy: &SandboxPolicy, argv: &[String]) -> Result<Vec<String>, SandboxStatus>;
}

pub const MISSING_BWRAP: &str = "Install bubblewrap (the bwrap command) to sandbox agents.";
pub const NOT_YET: &str = "Not available on this system yet.";

/// Runs a probe command: `(program, args) -> Ok(stdout)` or the error text.
pub type Probe = dyn Fn(&Path, &[&str]) -> Result<String, String> + Send + Sync;

fn real_kind(p: &Path) -> Kind {
    match std::fs::symlink_metadata(p) {
        Ok(m) if m.is_dir() => Kind::Dir,
        Ok(_) => Kind::File,
        Err(_) => Kind::Missing,
    }
}

/// bubblewrap, found on the login PATH and checked once.
pub struct Bubblewrap {
    path: Option<PathBuf>,
    status: SandboxStatus,
}

impl Bubblewrap {
    /// Looks for `bwrap` in `path_var` (the login PATH) and checks it can
    /// make a sandbox here with `probe`.
    pub fn detect(path_var: &str, probe: &Probe) -> Self {
        let found = std::env::split_paths(path_var)
            .map(|d| d.join("bwrap"))
            .find(|p| p.is_file());
        let Some(path) = found else {
            return Self {
                path: None,
                status: SandboxStatus::unavailable(MISSING_BWRAP),
            };
        };
        let version = probe(&path, &["--version"])
            .ok()
            .map(|v| v.trim().to_string());
        match probe(&path, &["--ro-bind", "/", "/", "--dev", "/dev", "true"]) {
            Ok(_) => Self {
                path: Some(path),
                status: SandboxStatus {
                    available: true,
                    tool: version.or_else(|| Some("bubblewrap".into())),
                    reason: None,
                },
            },
            Err(_) => Self {
                path: None,
                status: SandboxStatus::unavailable(
                    "bubblewrap can't make a sandbox here (user namespaces may be turned off).",
                ),
            },
        }
    }

    /// The real probe: runs the command and returns its stdout.
    pub fn run(program: &Path, args: &[&str]) -> Result<String, String> {
        let out = std::process::Command::new(program)
            .args(args)
            .stdin(std::process::Stdio::null())
            .output()
            .map_err(|e| e.to_string())?;
        if out.status.success() {
            Ok(String::from_utf8_lossy(&out.stdout).into_owned())
        } else {
            Err(String::from_utf8_lossy(&out.stderr).into_owned())
        }
    }
}

impl Sandboxer for Bubblewrap {
    fn status(&self) -> SandboxStatus {
        self.status.clone()
    }

    fn wrap(&self, policy: &SandboxPolicy, argv: &[String]) -> Result<Vec<String>, SandboxStatus> {
        let Some(bwrap) = &self.path else {
            return Err(self.status.clone());
        };
        let mut out = bwrap_args(bwrap, policy, &real_kind);
        out.extend(argv.iter().cloned());
        Ok(out)
    }
}

/// macOS and Windows for now.
pub struct NotYet;

impl Sandboxer for NotYet {
    fn status(&self) -> SandboxStatus {
        SandboxStatus::unavailable(NOT_YET)
    }
    fn wrap(&self, _: &SandboxPolicy, _: &[String]) -> Result<Vec<String>, SandboxStatus> {
        Err(self.status())
    }
}

/// This platform's sandbox, looked up with the login PATH.
#[cfg(target_os = "linux")]
pub fn platform(path_var: &str) -> Box<dyn Sandboxer> {
    Box::new(Bubblewrap::detect(path_var, &Bubblewrap::run))
}

/// This platform's sandbox: not available yet (F-TL-11 is Linux first).
#[cfg(not(target_os = "linux"))]
pub fn platform(_path_var: &str) -> Box<dyn Sandboxer> {
    Box::new(NotYet)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn policy() -> SandboxPolicy {
        SandboxPolicy {
            root: "/w/repo/.tandem/worktrees/fix".into(),
            git_common: Some("/w/repo/.git".into()),
            provider_home: Some("/d/providers/claude/acc_1".into()),
            providers: "/d/providers".into(),
            tasks: "/d/tasks".into(),
            run: "/d/run".into(),
            home: "/h".into(),
            hide: vec!["/w/repo/.tandem/worktrees/other".into()],
        }
    }

    fn fs(entries: &[(&str, Kind)]) -> impl Fn(&Path) -> Kind {
        let map: HashMap<PathBuf, Kind> = entries
            .iter()
            .map(|(p, k)| (PathBuf::from(p), *k))
            .collect();
        move |p: &Path| map.get(p).copied().unwrap_or(Kind::Missing)
    }

    fn pos(a: &[String], seq: &[&str]) -> Option<usize> {
        a.windows(seq.len())
            .position(|w| w.iter().zip(seq).all(|(x, y)| x == y))
    }

    #[test]
    fn f_tl_11_1_the_folder_and_its_git_data_are_writable_the_rest_read_only() {
        let kinds = fs(&[
            ("/tmp", Kind::Dir),
            ("/w/repo/.git", Kind::Dir),
            ("/w/repo/.git/hooks", Kind::Dir),
            ("/w/repo/.git/config", Kind::File),
            ("/d/tasks", Kind::Dir),
            ("/d/run", Kind::Dir),
            ("/d/providers", Kind::Dir),
            ("/d/providers/claude/acc_1", Kind::Dir),
        ]);
        let a = bwrap_args(Path::new("/usr/bin/bwrap"), &policy(), &kinds);
        assert_eq!(a[0], "/usr/bin/bwrap");
        assert!(pos(&a, &["--ro-bind", "/", "/"]).is_some());
        let root = pos(
            &a,
            &[
                "--bind",
                "/w/repo/.tandem/worktrees/fix",
                "/w/repo/.tandem/worktrees/fix",
            ],
        );
        assert!(root.is_some(), "{a:?}");
        assert!(pos(&a, &["--bind", "/w/repo/.git", "/w/repo/.git"]).is_some());
        assert!(pos(&a, &["--bind", "/d/tasks", "/d/tasks"]).is_some());
        assert_eq!(a.last().map(String::as_str), Some("--"));
        // Nothing under the home is writable.
        assert!(!a
            .windows(2)
            .any(|w| w[0] == "--bind" && w[1].starts_with("/h")));
    }

    #[test]
    fn f_tl_11_2_keys_and_other_accounts_homes_are_hidden_this_account_comes_back() {
        let kinds = fs(&[
            ("/h/.ssh", Kind::Dir),
            ("/h/.netrc", Kind::File),
            ("/d/providers", Kind::Dir),
            ("/d/providers/claude/acc_1", Kind::Dir),
            ("/w/repo/.tandem/worktrees/other", Kind::Dir),
        ]);
        let a = bwrap_args(Path::new("bwrap"), &policy(), &kinds);
        assert!(pos(&a, &["--tmpfs", "/h/.ssh"]).is_some());
        assert!(pos(&a, &["--ro-bind", "/dev/null", "/h/.netrc"]).is_some());
        assert!(pos(&a, &["--tmpfs", "/w/repo/.tandem/worktrees/other"]).is_some());
        // Missing secrets add nothing.
        assert!(pos(&a, &["--tmpfs", "/h/.aws"]).is_none());
        let hidden = pos(&a, &["--tmpfs", "/d/providers"]).expect("providers hidden");
        let back = pos(
            &a,
            &[
                "--bind",
                "/d/providers/claude/acc_1",
                "/d/providers/claude/acc_1",
            ],
        )
        .expect("own home back");
        assert!(
            back > hidden,
            "the account's home comes after the hidden providers"
        );
    }

    #[test]
    fn f_tl_11_3_git_hooks_and_config_stay_read_only_after_the_writable_binds() {
        let mut p = policy();
        p.root = "/w/repo".into();
        p.git_common = None;
        let kinds = fs(&[
            ("/w/repo/.git", Kind::Dir),
            ("/w/repo/.git/hooks", Kind::Dir),
            ("/w/repo/.git/config", Kind::File),
        ]);
        let a = bwrap_args(Path::new("bwrap"), &p, &kinds);
        let root = pos(&a, &["--bind", "/w/repo", "/w/repo"]).expect("root");
        let hooks = pos(
            &a,
            &["--ro-bind", "/w/repo/.git/hooks", "/w/repo/.git/hooks"],
        )
        .expect("hooks");
        let config = pos(
            &a,
            &["--ro-bind", "/w/repo/.git/config", "/w/repo/.git/config"],
        )
        .expect("config");
        assert!(hooks > root && config > root);
    }

    #[test]
    fn f_tl_11_4_without_bwrap_or_where_it_cant_run_it_says_why() {
        let none = Bubblewrap::detect("", &|_, _| Ok(String::new()));
        assert_eq!(none.status().reason.as_deref(), Some(MISSING_BWRAP));
        assert!(none.wrap(&policy(), &["claude".into()]).is_err());
        let dir = tempfile::tempdir().expect("tmp");
        std::fs::write(dir.path().join("bwrap"), "").expect("bwrap");
        let path = dir.path().to_string_lossy().into_owned();
        let blocked = Bubblewrap::detect(&path, &|_, args| {
            if args == ["--version"] {
                Ok("bubblewrap 0.9.0\n".into())
            } else {
                Err("setting up uid map: Permission denied".into())
            }
        });
        assert!(!blocked.status().available);
        assert!(blocked
            .status()
            .reason
            .unwrap_or_default()
            .contains("can't make a sandbox"));
        let ok = Bubblewrap::detect(&path, &|_, args| {
            Ok(if args == ["--version"] {
                "bubblewrap 0.9.0\n".into()
            } else {
                String::new()
            })
        });
        assert_eq!(ok.status().tool.as_deref(), Some("bubblewrap 0.9.0"));
        assert_eq!(NotYet.status().reason.as_deref(), Some(NOT_YET));
    }

    #[test]
    fn f_tl_11_5_the_engine_argv_comes_after_the_sandbox_unchanged() {
        let dir = tempfile::tempdir().expect("tmp");
        std::fs::write(dir.path().join("bwrap"), "").expect("bwrap");
        let ok = Bubblewrap::detect(&dir.path().to_string_lossy(), &|_, _| Ok(String::new()));
        let argv: Vec<String> = ["/usr/bin/claude", "--resume", "ses 1"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let out = ok.wrap(&policy(), &argv).expect("wrapped");
        let sep = out.iter().position(|a| a == "--").expect("separator");
        assert_eq!(&out[sep + 1..], argv.as_slice());
    }

    /// The real thing, where bubblewrap is installed and can run.
    #[cfg(target_os = "linux")]
    #[test]
    fn f_tl_11_1_2_3_a_real_sandbox_keeps_the_agent_in_its_folder() {
        let path = std::env::var("PATH").unwrap_or_default();
        let bw = Bubblewrap::detect(&path, &Bubblewrap::run);
        if !bw.status().available {
            eprintln!("skipped: {:?}", bw.status().reason);
            return;
        }
        // Outside /tmp, which stays writable in the sandbox (D-2475).
        let Ok(dir) = tempfile::tempdir_in("/var/tmp") else {
            eprintln!("skipped: no /var/tmp");
            return;
        };
        let base = dir.path().canonicalize().expect("canon");
        let (root, home, data) = (base.join("ws"), base.join("home"), base.join("data"));
        for d in [
            root.join(".git/hooks"),
            home.join(".ssh"),
            data.join("providers/claude/acc_1"),
            data.join("providers/codex/acc_2"),
            data.join("tasks"),
            data.join("run"),
        ] {
            std::fs::create_dir_all(&d).expect("mkdir");
        }
        std::fs::write(home.join(".ssh/id_ed25519"), "KEY").expect("key");
        std::fs::write(data.join("providers/codex/acc_2/auth.json"), "OTHER").expect("other");
        std::fs::write(root.join(".git/config"), "[core]\n").expect("config");
        let policy = SandboxPolicy {
            root: root.clone(),
            git_common: None,
            provider_home: Some(data.join("providers/claude/acc_1")),
            providers: data.join("providers"),
            tasks: data.join("tasks"),
            run: data.join("run"),
            home: home.clone(),
            hide: Vec::new(),
        };
        let sh = |script: &str| {
            let argv = bw
                .wrap(&policy, &["/bin/sh".into(), "-c".into(), script.into()])
                .expect("wrap");
            std::process::Command::new(&argv[0])
                .args(&argv[1..])
                .output()
                .expect("run")
        };
        let ok = |script: &str| sh(script).status.success();
        assert!(
            ok(&format!("echo hi > {}/a.txt", root.display())),
            "the folder is writable"
        );
        assert!(
            !ok(&format!("echo x > {}/out.txt", home.display())),
            "the home isn't"
        );
        assert!(
            !ok(&format!("cat {}/.ssh/id_ed25519", home.display())),
            "keys are hidden"
        );
        assert!(
            !ok(&format!(
                "cat {}/providers/codex/acc_2/auth.json",
                data.display()
            )),
            "other accounts' homes are hidden"
        );
        assert!(
            ok(&format!(
                "echo t > {}/providers/claude/acc_1/token",
                data.display()
            )),
            "this account's home is writable"
        );
        assert!(
            !ok(&format!(
                "echo x > {}/.git/hooks/pre-commit",
                root.display()
            )),
            "hooks are read-only"
        );
        assert!(
            !ok(&format!("echo x >> {}/.git/config", root.display())),
            "config is read-only"
        );
    }

    #[test]
    fn f_tl_11_3_a_worktrees_common_git_dir_is_found() {
        let dir = tempfile::tempdir().expect("tmp");
        let repo = dir.path().join("repo");
        let wt = repo.join(".tandem/worktrees/fix");
        let admin = repo.join(".git/worktrees/fix");
        std::fs::create_dir_all(&wt).expect("wt");
        std::fs::create_dir_all(&admin).expect("admin");
        std::fs::write(wt.join(".git"), format!("gitdir: {}\n", admin.display())).expect(".git");
        std::fs::write(admin.join("commondir"), "../..\n").expect("commondir");
        let common = git_common_dir(&wt).expect("common");
        assert_eq!(common, repo.join(".git").canonicalize().expect("canon"));
        assert_eq!(git_common_dir(&repo), None, "a plain checkout has none");
    }
}
